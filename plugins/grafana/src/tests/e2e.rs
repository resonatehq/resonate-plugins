//! End to end: a minimal Resonate server with only this plugin's worker.
//!
//! The only test that exercises the frame — claim, heartbeat, settle —
//! together with `plugin::process`. Everything upstream of the worker (the
//! HTTP edge, the poll transport, auth) is the server's own test surface,
//! not this crate's, so none of it is compiled in: an in-memory engine, the
//! timeout sweep, a router holding one worker under one scheme.
//!
//! Bring the provider up with specification §5.1/§5.2 first — the same
//! environment `tests/process.rs` uses.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};

use async_trait::async_trait;
use resonate::config::Config as ServerConfig;
use resonate::deadlines;
use resonate::processing::processing_timeouts;
use resonate::server::Server;
use resonate::transport::TransportDispatcher;
use resonate_core::types::{Message, RequestEnvelope, RequestHead, PROTOCOL_VERSION};
use resonate_core::{ResonateRouter, ResonateServer, ResonateWorker, Unavailable};
use resonate_plugin_grafana::{plugin::Config, Worker, SCHEME};
use resonate_server_dbms::engine_port::ResonateEngine;
use resonate_server_dbms::engine_sqlite::SqliteEngine;

/// The succeeding work item: the query the TestData data source §5
/// provisions answers. The promise reaches `resolved`, carrying the §4.1.2
/// Resolved value — `= response.body`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_promise_targeting_this_plugin_resolves_from_an_answered_query() {
    let (server, _shutdown) = harness();
    let id = promise_id("e2e-ok");

    create(
        &server,
        &id,
        json!({
            "func": "query.run",
            "args": {
                "queries": [{
                    "refId": "A",
                    "datasource": {"uid": env("GRAFANA_FIXTURE_OK"), "type": "grafana-testdata-datasource"},
                    "scenarioId": "random_walk",
                }],
                "from": "now-5m",
                "to": "now",
            }
        }),
    )
    .await;
    let (state, value) = settled(&server, &id).await;

    assert_eq!(state, "resolved", "{value}");
    assert_eq!(value["results"]["A"]["status"], 200, "{value}");
    assert!(
        !value["results"]["A"]["frames"].as_array().expect("frames").is_empty(),
        "{value}"
    );
}

/// The failing work item: the query the queried system refuses. The promise
/// reaches `rejected`, carrying the §4.1.2 Rejected value with
/// `code: query_failed`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_promise_targeting_this_plugin_rejects_from_a_refused_query() {
    let (server, _shutdown) = harness();
    let id = promise_id("e2e-fail");

    create(
        &server,
        &id,
        json!({
            "func": "query.run",
            "args": {
                "queries": [{
                    "refId": "A",
                    "datasource": {"uid": env("GRAFANA_FIXTURE_FAIL"), "type": "prometheus"},
                    "expr": "up",
                }],
                "from": "now-5m",
                "to": "now",
            }
        }),
    )
    .await;
    let (state, value) = settled(&server, &id).await;

    assert_eq!(state, "rejected", "{value}");
    assert_eq!(value["code"], "query_failed");
    assert_eq!(value["detail"]["A"]["status"], 400, "{value}");
    assert_eq!(value["detail"]["A"]["errorSource"], "downstream", "{value}");
}

// ─── The server ───────────────────────────────────────────────────────────────

/// A whole Resonate server, in this process, with one worker on it. The
/// returned sender owns the background loop: dropping it stops it, so a test
/// holds it for as long as it holds the server.
fn harness() -> (Arc<Server>, tokio::sync::watch::Sender<bool>) {
    let mut config = ServerConfig::default();
    // Nothing else is enabled: this plugin is the only way out of the server.
    config.transports.http_push.enabled = false;
    config.transports.http_poll.enabled = false;
    let lease_timeout = config.tasks.lease_timeout;
    let retry_timeout = config.tasks.retry_timeout;

    let engine: Arc<dyn ResonateEngine> = Arc::new(
        SqliteEngine::open(":memory:", retry_timeout, config.debug).expect("in-memory engine"),
    );

    // The ring — server holds router, router holds the worker, the worker
    // holds the server — closed the way the server's own composition root
    // closes it: `new_cyclic` hands the weak side to the worker before the
    // server exists, so the router can be a constructor argument.
    let state = Arc::new_cyclic(|weak: &std::sync::Weak<Server>| {
        let mut workers: HashMap<String, Arc<dyn ResonateWorker>> = HashMap::new();
        workers.insert(
            SCHEME.to_string(),
            Arc::new(Handle {
                server: weak.clone(),
                config: plugin_config(),
                lease_timeout,
            }),
        );
        let router: Arc<dyn ResonateRouter> = Arc::new(TransportDispatcher::new(workers));
        let timer = deadlines::build(&config.timeouts, weak.clone());
        Server::new(config, engine, router, timer)
    });

    // The sweep: what re-delivers a task whose lease expired, and what settles
    // a promise that timed out. Delivery itself is not a loop any more — a
    // transition's messages go to the router as soon as it commits.
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    tokio::spawn(processing_timeouts::timeout_processing_loop(
        state.clone(),
        shutdown_rx,
    ));
    (state, shutdown_tx)
}

/// The worker's end of the ring. The frame's [`Worker`] holds its server
/// port strongly, and the server is what owns the router that reaches it, so
/// the strong handle is built per delivery from the weak one the ring was
/// closed with — the same weak link the server gives its own transports.
struct Handle {
    server: std::sync::Weak<Server>,
    config: Config,
    lease_timeout: i64,
}

#[async_trait]
impl ResonateWorker for Handle {
    async fn send(&self, address: &str, msg: &Message) -> Result<(), Unavailable> {
        let Some(server) = self.server.upgrade() else {
            return Err(Unavailable::new("the server is gone"));
        };
        Worker::new(server, self.config.clone(), self.lease_timeout)
            .send(address, msg)
            .await
    }
}

/// §2, from the environment §5.2 exports.
fn plugin_config() -> Config {
    Config {
        base_url: env("GRAFANA_BASE_URL"),
        token: env("GRAFANA_TOKEN"),
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
    for _ in 0..120 {
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
    format!("grafana.{what}.{}", nanos())
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
