//! End to end: a minimal Resonate server with only this plugin's worker.
//!
//! The only test that exercises the frame — claim, heartbeat, settle —
//! together with `plugin::process`. Everything upstream of the worker (the
//! HTTP edge, the poll transport, auth) is the server's own test surface,
//! not this crate's, so none of it is compiled in: an in-memory engine, the
//! timeout sweep, and a router holding one worker under one scheme.
//!
//! Bring the provider up with specification §5.1/§5.2 first — the same
//! environment `tests/process.rs` uses.

use std::collections::HashMap;
use std::sync::{Arc, Weak};
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine;
use serde_json::{json, Value};

use resonate::config::Config as ServerConfig;
use resonate::deadlines;
use resonate::processing::processing_timeouts;
use resonate::server::Server;
use resonate::transport::TransportDispatcher;
use resonate_core::types::{RequestEnvelope, RequestHead, ResponseEnvelope, PROTOCOL_VERSION};
use resonate_core::{ResonateRouter, ResonateServer, ResonateWorker, Unavailable};
use resonate_plugin_elasticsearch::{plugin::Config, Worker, SCHEME};
use resonate_server_dbms::{engine_port::ResonateEngine, engine_sqlite::SqliteEngine};

/// The succeeding work item: the promise reaches `resolved`, carrying the
/// §4.1.2 Resolved value.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_promise_targeting_this_plugin_resolves_from_a_successful_copy() {
    let (server, _shutdown) = harness().await;
    let id = promise_id("e2e-ok");
    let dest = dest_index("e2e_ok");

    create(
        &server,
        &id,
        json!({
            "func": "reindex.create",
            "args": {
                "source": {"index": env("ELASTICSEARCH_FIXTURE_OK")},
                "dest": {"index": dest},
            },
        }),
    )
    .await;
    let (state, value) = settled(&server, &id).await;

    assert_eq!(state, "resolved", "{value}");
    assert_eq!(
        value["description"],
        format!("reindex from [{}] to [{dest}]", env("ELASTICSEARCH_FIXTURE_OK"))
    );
    assert!(value["id"].as_str().unwrap_or_default().contains(':'), "{value}");
    assert_eq!(value["cancelled"], false);
    assert_eq!(value["response"]["total"], 200);
    assert_eq!(value["response"]["created"], 200);
    assert_eq!(value["response"]["failures"], json!([]));
}

/// The failing work item: the promise reaches `rejected`, carrying the
/// §4.1.2 Rejected value with `code: reindex_failed`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_promise_targeting_this_plugin_rejects_from_a_failed_copy() {
    let (server, _shutdown) = harness().await;
    let id = promise_id("e2e-fail");
    let dest = dest_index("e2e_fail");

    create(
        &server,
        &id,
        json!({
            "func": "reindex.create",
            "args": {
                "source": {"index": env("ELASTICSEARCH_FIXTURE_FAIL")},
                "dest": {"index": dest},
            },
        }),
    )
    .await;
    let (state, value) = settled(&server, &id).await;

    assert_eq!(state, "rejected", "{value}");
    assert_eq!(value["code"], "reindex_failed");
    assert_eq!(value["detail"]["type"], "illegal_argument_exception");
}

// ─── The server ───────────────────────────────────────────────────────────────

/// A whole Resonate server, in this process, with one worker on it. The
/// returned sender owns the timeout sweep: dropping it stops the loop, so a
/// test holds it for as long as it holds the server.
async fn harness() -> (Arc<Server>, tokio::sync::watch::Sender<bool>) {
    let mut config = ServerConfig::default();
    // Nothing else is enabled: this plugin is the only way out of the server.
    config.transports.http_push.enabled = false;
    config.transports.http_poll.enabled = false;
    let lease_timeout = config.tasks.lease_timeout;
    let engine: Arc<dyn ResonateEngine> = Arc::new(
        SqliteEngine::open(":memory:", config.tasks.retry_timeout, config.debug)
            .expect("in-memory engine"),
    );

    // The ring — server holds router, router holds worker, worker holds server
    // — closed the way the server's own composition root closes it: the link
    // back up is weak, so nothing in the ring outlives the test.
    let state = Arc::new_cyclic(|weak: &Weak<Server>| {
        let port: Arc<dyn ResonateServer> = Arc::new(ServerHandle(weak.clone()));
        let mut workers: HashMap<String, Arc<dyn ResonateWorker>> = HashMap::new();
        workers.insert(
            SCHEME.to_string(),
            Arc::new(Worker::new(port, plugin_config(), lease_timeout)),
        );
        let router: Arc<dyn ResonateRouter> = Arc::new(TransportDispatcher::new(workers));
        let timer = deadlines::build(&config.timeouts, weak.clone());
        Server::new(config, engine, router, timer)
    });

    state.router().init(false).await.expect("the worker starts");
    state.start_timer().await;

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    tokio::spawn(processing_timeouts::timeout_processing_loop(
        state.clone(),
        shutdown_rx,
    ));
    (state, shutdown_tx)
}

/// The server port the worker holds. Weak, because the worker is inside the
/// server: a strong handle would keep the whole ring alive forever.
struct ServerHandle(Weak<Server>);

#[async_trait]
impl ResonateServer for ServerHandle {
    async fn process(&self, req: &RequestEnvelope) -> Result<ResponseEnvelope, Unavailable> {
        match self.0.upgrade() {
            Some(server) => server.process(req).await,
            None => Err(Unavailable::new("the server is gone")),
        }
    }
}

/// §2, from the environment §5.2 exports, with `poll` at 1s.
fn plugin_config() -> Config {
    Config {
        base_url: env("ELASTICSEARCH_BASE_URL"),
        api_key: env("ELASTICSEARCH_API_KEY"),
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

// ─── Fixtures ─────────────────────────────────────────────────────────────────

fn env(key: &str) -> String {
    std::env::var(key)
        .unwrap_or_else(|_| panic!("{key} is unset — run specification §5.1 and §5.2 first"))
}

fn promise_id(what: &str) -> String {
    format!("elasticsearch.{what}.{}", nanos())
}

/// A fresh destination index per test. Index names are lowercase.
fn dest_index(what: &str) -> String {
    format!("resonate_dest_{what}_{}", nanos())
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
