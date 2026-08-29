//! End to end: a minimal Resonate server with only this plugin's worker.
//!
//! The only test that exercises the frame — claim, heartbeat, settle —
//! together with `plugin::process`. Everything upstream of the worker (the
//! HTTP edge, the poll transport, auth) is the server's own test surface,
//! not this crate's, so none of it is compiled in: an in-memory store, one
//! background loop, and a router holding one worker under one scheme.
//!
//! Bring the provider up with specification §5.1/§5.2 first — the same
//! environment `tests/process.rs` uses. The two work items are §5's own:
//! `N8N_FIXTURE_OK`, whose retry succeeds, and `N8N_FIXTURE_FAIL`, whose
//! retry fails again.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine;
use serde_json::{json, Value};

use resonate::config::Config as ServerConfig;
use resonate::deadlines;
use resonate::processing::processing_timeouts;
use resonate::server::Server;
use resonate::transport::TransportDispatcher;
use resonate_core::types::{Message, RequestEnvelope, RequestHead, PROTOCOL_VERSION};
use resonate_core::{ResonateRouter, ResonateServer, ResonateWorker, Unavailable};
use resonate_server_dbms::engine_port::ResonateEngine;
use resonate_server_dbms::engine_sqlite::SqliteEngine;
use resonate_plugin_n8n::{plugin::Config, Worker, SCHEME};

/// The succeeding work item: the promise reaches `resolved`, carrying the
/// §4.12.2 Resolved value.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_promise_targeting_this_plugin_resolves_from_a_successful_retry() {
    let (server, _shutdown) = harness().await;
    let id = promise_id("e2e-ok");
    let source = env("N8N_FIXTURE_OK");

    create(
        &server,
        &id,
        json!({"func": "execution.retry", "args": {"id": source}}),
    )
    .await;
    let (state, value) = settled(&server, &id).await;

    assert_eq!(state, "resolved", "{value}");
    assert_eq!(value["status"], "success");
    assert_eq!(value["mode"], "retry");
    assert_eq!(value["retryOf"].to_string().trim_matches('"'), source);
}

/// The failing work item: the promise reaches `rejected`, carrying the
/// §4.12.2 Rejected value with `code: execution_failed`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_promise_targeting_this_plugin_rejects_from_a_failed_retry() {
    let (server, _shutdown) = harness().await;
    let id = promise_id("e2e-fail");
    let source = env("N8N_FIXTURE_FAIL");

    create(
        &server,
        &id,
        json!({"func": "execution.retry", "args": {"id": source}}),
    )
    .await;
    let (state, value) = settled(&server, &id).await;

    assert_eq!(state, "rejected", "{value}");
    assert_eq!(value["code"], "execution_failed");
    assert_eq!(value["detail"]["status"], "error");
}

// ─── The server ───────────────────────────────────────────────────────────────

/// A whole Resonate server, in this process, with one worker on it. The
/// returned sender owns the background loop: dropping it stops it, so a test
/// holds it for as long as it holds the server.
async fn harness() -> (Arc<Server>, tokio::sync::watch::Sender<bool>) {
    let mut config = ServerConfig::default();
    // Nothing else is enabled: this plugin is the only way out of the server.
    config.transports.http_push.enabled = false;
    config.transports.http_poll.enabled = false;
    config.transports.gcps.enabled = false;
    config.transports.bash_exec.enabled = false;
    let lease_timeout = config.tasks.lease_timeout;

    let engine: Arc<dyn ResonateEngine> = Arc::new(
        SqliteEngine::open(
            ":memory:",
            config.tasks.retry_timeout,
            config.storage.sqlite.preload_limit,
            config.storage.sqlite.migrate,
            config.debug,
        )
        .expect("in-memory store"),
    );

    // The ring: the server holds the router, the router holds the worker, and
    // the worker calls the server back. `new_cyclic` is what closes it.
    let state = Arc::new_cyclic(|weak: &Weak<Server>| {
        let mut workers: HashMap<String, Arc<dyn ResonateWorker>> = HashMap::new();
        workers.insert(
            SCHEME.to_string(),
            Arc::new(LateWorker {
                server: weak.clone(),
                config: plugin_config(),
                lease_timeout,
                worker: OnceLock::new(),
            }),
        );
        let router: Arc<dyn ResonateRouter> = Arc::new(TransportDispatcher::new(workers));
        let timer = deadlines::build(&config.timeouts, weak.clone());
        Server::new(config, engine, router, timer)
    });

    state.router().init(false).await.expect("worker started");
    state.start_timer().await;

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    tokio::spawn(processing_timeouts::timeout_processing_loop(
        state.clone(),
        shutdown_rx,
    ));
    (state, shutdown_tx)
}

/// The plugin's worker, built on first delivery.
///
/// The frame takes the server port as an `Arc`, and the server is what owns
/// the router that owns this — so the strong handle does not exist yet when
/// the router is constructed. `Arc::new_cyclic` hands out a weak one, and
/// this upgrades it the first time a message arrives, by which point the
/// server is live. Nothing else about the frame's path changes: `send` below
/// is the real `Worker::send`.
struct LateWorker {
    server: Weak<Server>,
    config: Config,
    lease_timeout: i64,
    worker: OnceLock<Worker>,
}

#[async_trait]
impl ResonateWorker for LateWorker {
    async fn send(&self, address: &str, msg: &Message) -> Result<(), Unavailable> {
        let worker = self.worker.get_or_init(|| {
            let port: Arc<dyn ResonateServer> =
                self.server.upgrade().expect("the server is alive");
            Worker::new(port, self.config.clone(), self.lease_timeout)
        });
        worker.send(address, msg).await
    }
}

/// §2, from the environment §5.2 exports, with `poll` at 1s.
fn plugin_config() -> Config {
    Config {
        base_url: env("N8N_BASE_URL"),
        api_key: env("N8N_API_KEY"),
        poll: Duration::from_secs(1),
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

fn env(key: &str) -> String {
    std::env::var(key)
        .unwrap_or_else(|_| panic!("{key} is unset — run specification §5.1 and §5.2 first"))
}

fn promise_id(what: &str) -> String {
    format!("n8n.{what}.{}", nanos())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}
