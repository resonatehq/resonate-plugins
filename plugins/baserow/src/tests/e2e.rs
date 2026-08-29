//! End to end: a minimal Resonate server with only this plugin's worker.
//!
//! The only test that exercises the frame — claim, heartbeat, settle —
//! together with `plugin::process`. Everything upstream of the worker (the
//! HTTP edge, the poll transport, auth) is the server's own test surface,
//! not this crate's, so none of it is compiled in: an in-memory store, the
//! timer and its sweep, and a router holding one worker under one scheme.
//!
//! Bring the provider up with specification §5.1/§5.2 first — the same
//! environment `tests/process.rs` uses.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};

use resonate::config::Config as ServerConfig;
use resonate::deadlines;
use resonate::processing::processing_timeouts;
use resonate::server::Server;
use resonate::transport::TransportDispatcher;
use resonate_core::types::{RequestEnvelope, RequestHead, ResponseEnvelope, PROTOCOL_VERSION};
use resonate_core::{ResonateRouter, ResonateServer, ResonateWorker, Unavailable};
use resonate_plugin_baserow::{plugin::Config, Worker, SCHEME};
use resonate_server_dbms::engine_port::ResonateEngine;
use resonate_server_dbms::engine_sqlite::SqliteEngine;

/// The succeeding work item: `Tasks` is all text, so the import job §5's
/// fixture accepts reaches "finished" and the promise reaches `resolved`,
/// carrying the §4.12.2 Resolved value — including the
/// `original_file_name` stamp the frame's `sanitize` produced.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_promise_targeting_this_plugin_resolves_from_a_finished_import() {
    let (server, _shutdown) = harness().await;
    let id = promise_id("e2e-ok");

    create(
        &server,
        &id,
        json!({
            "func": "table.import",
            "args": {"table_id": ok_table(), "data": [["e2e", "end to end"]]},
        }),
    )
    .await;
    let (state, value) = settled(&server, &id).await;

    assert_eq!(state, "resolved", "{value}");
    assert_eq!(value["state"], "finished");
    assert_eq!(value["type"], "file_import");
    assert_eq!(value["table_id"], ok_table());
    assert_eq!(value["report"]["failing_rows"], json!({}));
    assert!(
        value["original_file_name"].as_str().unwrap_or_default().starts_with(&id),
        "{value}"
    );
}

/// The failing work item: `Amounts` has a number field, so writing a
/// non-numeric value to it is rejected by the field and the promise reaches
/// `rejected`, carrying the §4.3.2 Rejected value with `code:
/// invalid_request` and a `detail` keyed by field name.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_promise_targeting_this_plugin_rejects_from_a_refused_value() {
    let (server, _shutdown) = harness().await;
    let id = promise_id("e2e-fail");

    create(
        &server,
        &id,
        json!({
            "func": "row.create",
            "args": {
                "table_id": fail_table(),
                "values": {"Label": "e2e", "Amount": "not-a-number"},
                "user_field_names": true,
            },
        }),
    )
    .await;
    let (state, value) = settled(&server, &id).await;

    assert_eq!(state, "rejected", "{value}");
    assert_eq!(value["code"], "invalid_request");
    assert!(value["detail"]["Amount"].is_array(), "{value}");
}

// ─── The server ───────────────────────────────────────────────────────────────

/// A whole Resonate server, in this process, with one worker on it. The
/// returned sender owns the timeout sweep: dropping it stops it, so a test
/// holds it for as long as it holds the server.
async fn harness() -> (Arc<Server>, tokio::sync::watch::Sender<bool>) {
    let mut config = ServerConfig::default();
    // Nothing else is enabled: this plugin is the only way out of the server.
    config.transports.http_push.enabled = false;
    config.transports.http_poll.enabled = false;
    let lease_timeout = config.tasks.lease_timeout;

    let engine: Arc<dyn ResonateEngine> = Arc::new(
        SqliteEngine::open(
            ":memory:",
            config.tasks.retry_timeout,
            config.storage.sqlite.preload_limit,
            true,
            config.debug,
        )
        .expect("in-memory store"),
    );

    // The worker holds the server port directly — the same `process` path a
    // remote worker's HTTP calls take. The ring is server → router → worker →
    // server, and it is closed after construction through `LatePort`: the
    // server owns the router that owns the worker, so the worker's handle
    // cannot be handed out until the server exists.
    let port = Arc::new(LatePort::default());
    let mut workers: HashMap<String, Arc<dyn ResonateWorker>> = HashMap::new();
    workers.insert(
        SCHEME.to_string(),
        Arc::new(Worker::new(
            Arc::clone(&port) as Arc<dyn ResonateServer>,
            plugin_config(),
            lease_timeout,
        )),
    );
    let router: Arc<dyn ResonateRouter> = Arc::new(TransportDispatcher::new(workers));

    // The timer's callbacks point back at the server, so it is built from the
    // weak handle `new_cyclic` hands out.
    let state = Arc::new_cyclic(|weak: &std::sync::Weak<Server>| {
        let timer = deadlines::build(&config.timeouts, weak.clone());
        Server::new(config, engine, router, timer)
    });
    port.set(Arc::clone(&state) as Arc<dyn ResonateServer>);

    state.router().init(false).await.expect("the worker starts");
    state.start_timer().await;

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    tokio::spawn(processing_timeouts::timeout_processing_loop(
        state.clone(),
        shutdown_rx,
    ));
    (state, shutdown_tx)
}

/// The server port the worker is built with, filled in once the server it
/// points at exists.
#[derive(Default)]
struct LatePort(std::sync::OnceLock<Arc<dyn ResonateServer>>);

impl LatePort {
    fn set(&self, server: Arc<dyn ResonateServer>) {
        let _ = self.0.set(server);
    }
}

#[async_trait::async_trait]
impl ResonateServer for LatePort {
    async fn process(&self, req: &RequestEnvelope) -> Result<ResponseEnvelope, Unavailable> {
        self.0
            .get()
            .expect("the port is filled in before anything routes")
            .process(req)
            .await
    }
}

/// §2, from the environment §5.2 exports, with `poll` at 1s.
fn plugin_config() -> Config {
    Config {
        base_url: env("BASEROW_BASE_URL"),
        email: env("BASEROW_EMAIL"),
        password: env("BASEROW_PASSWORD"),
        poll: Duration::from_secs(1),
        poll_export: None,
        poll_table: None,
    }
}

/// Create the promise whose `resonate:target` is this plugin's address. The
/// tag is what makes the server create a task and deliver it.
async fn create(server: &Arc<Server>, id: &str, param: Value) {
    let response = server
        .process(&RequestEnvelope {
            kind: "promise.create".into(),
            head: head(),
            data: json!({
                "id": id,
                "timeoutAt": now_ms() + 300_000,
                "param": {
                    "headers": { "content-type": "application/json" },
                    "data": base64::engine::general_purpose::STANDARD.encode(param.to_string()),
                },
                "tags": { "resonate:target": format!("{SCHEME}://default") },
            }),
        })
        .await
        .expect("promise.create reached the server");
    assert_eq!(response.head.status, 200, "{:?}", response.data);
}

/// Wait for the promise to leave `pending`, and decode its value.
async fn settled(server: &Arc<Server>, id: &str) -> (String, Value) {
    for _ in 0..300 {
        let response = server
            .process(&RequestEnvelope {
                kind: "promise.get".into(),
                head: head(),
                data: json!({ "id": id }),
            })
            .await
            .expect("promise.get reached the server");
        assert_eq!(response.head.status, 200, "{:?}", response.data);
        let promise = &response.data["promise"];
        let state = promise["state"].as_str().unwrap_or_default().to_string();
        if state != "pending" {
            let data = promise["value"]["data"].as_str().unwrap_or_default();
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data)
                .expect("value.data is base64");
            let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
            return (state, value);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    panic!("{id} never settled");
}

fn head() -> RequestHead {
    RequestHead {
        corr_id: format!("e2e-{}", nanos()),
        version: PROTOCOL_VERSION.to_string(),
        auth: None,
        debug_time: None,
    }
}

// ─── Fixtures ─────────────────────────────────────────────────────────────────

/// `Tasks`: all text, so every write it is given succeeds.
fn ok_table() -> i64 {
    env("BASEROW_FIXTURE_OK").parse().expect("BASEROW_FIXTURE_OK is an integer")
}

/// `Amounts`: `Amount` is a number field, so a non-numeric value fails.
fn fail_table() -> i64 {
    env("BASEROW_FIXTURE_FAIL")
        .parse()
        .expect("BASEROW_FIXTURE_FAIL is an integer")
}

fn env(key: &str) -> String {
    std::env::var(key)
        .unwrap_or_else(|_| panic!("{key} is unset — run specification §5.1 and §5.2 first"))
}

fn promise_id(what: &str) -> String {
    format!("baserow.{what}.{}", nanos())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_millis() as i64
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_nanos()
}
