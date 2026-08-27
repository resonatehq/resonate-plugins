//! End to end: a minimal Resonate server with only this plugin's worker.
//!
//! The only test that exercises the frame — claim, heartbeat, settle —
//! together with `plugin::process`. Everything upstream of the worker (the
//! HTTP edge, the poll transport, auth) is the server's own test surface,
//! not this crate's, so none of it is compiled in: an in-memory store, the
//! two background loops, a router holding one worker under one scheme.
//!
//! Bannerbear is SaaS only — the specification has no §5 — so the work items
//! are stubbed on the same local mock `tests/process.rs` uses, and the same
//! circularity applies: the mock is built from the specification, so this can
//! show the wiring settles a promise, not that Bannerbear behaves this way.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use resonate::config::Config as ServerConfig;
use resonate::persistence::{persistence_sqlite::SqliteStorage, Storage};
use resonate::processing::{processing_messages, processing_timeouts};
use resonate::server::Server;
use resonate::transport::TransportDispatcher;
use resonate_core::types::{RequestEnvelope, RequestHead, PROTOCOL_VERSION};
use resonate_core::{ResonateRouter, ResonateServer, ResonateWorker};
use resonate_plugin_bannerbear::{plugin::Config, Worker, SCHEME};

/// The succeeding work item: the promise reaches `resolved`, carrying the
/// §4.1.2 Resolved value.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_promise_targeting_this_plugin_resolves_from_a_completed_render() {
    let server = mock().await;
    let files = json!({"image_url": "https://cdn.bannerbear.com/e2e_ok.png"});
    create_stub(server, json!({"uid": "e2e_ok", "status": "pending", "files": null})).await;
    Mock::given(method("GET"))
        .and(path("/v5/images/e2e_ok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"uid": "e2e_ok", "status": "completed", "files": files}),
        ))
        .mount(server)
        .await;

    let (resonate, _shutdown) = harness();
    let id = promise_id("e2e-ok");

    create(&resonate, &id, json!({"func": "image.create", "args": render_args()})).await;
    let (state, value) = settled(&resonate, &id).await;

    assert_eq!(state, "resolved", "{value}");
    assert_eq!(value, json!({"files": files}));
    // The render carried this promise's injected identity.
    let sent = server.received_requests().await.expect("recording is on");
    let metadata = sent[0].body_json::<Value>().unwrap()["metadata"].as_str().unwrap().to_string();
    assert!(metadata.starts_with(&id), "{metadata} should be derived from {id}");
}

/// The failing work item: the promise reaches `rejected`, carrying the
/// §4.1.2 Rejected value with `code: render_failed`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_promise_targeting_this_plugin_rejects_from_a_failed_render() {
    let server = mock().await;
    create_stub(server, json!({"uid": "e2e_fail", "status": "pending", "files": null})).await;
    Mock::given(method("GET"))
        .and(path("/v5/images/e2e_fail"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"uid": "e2e_fail", "status": "failed", "error": "layer 'title' not found"}),
        ))
        .mount(server)
        .await;

    let (resonate, _shutdown) = harness();
    let id = promise_id("e2e-fail");

    create(&resonate, &id, json!({"func": "image.create", "args": render_args()})).await;
    let (state, value) = settled(&resonate, &id).await;

    assert_eq!(state, "rejected", "{value}");
    assert_eq!(value["code"], "render_failed");
    assert_eq!(value["detail"]["status"], "failed");
    assert_eq!(value["detail"]["error"], "layer 'title' not found");
}

// ─── The server ───────────────────────────────────────────────────────────────

/// A whole Resonate server, in this process, with one worker on it. The
/// returned sender owns the background loops: dropping it stops them, so a
/// test holds it for as long as it holds the server.
fn harness() -> (Arc<Server>, tokio::sync::watch::Sender<bool>) {
    let mut config = ServerConfig::default();
    // Nothing else is enabled: this plugin is the only way out of the server.
    config.transports.http_push.enabled = false;
    config.transports.http_poll.enabled = false;
    let lease_timeout = config.tasks.lease_timeout;
    let retry_timeout = config.tasks.retry_timeout;

    let storage = Storage::Sqlite(
        SqliteStorage::open(":memory:", retry_timeout).expect("in-memory store"),
    );
    let state = Arc::new(Server::new(config, None, storage));

    // The worker holds the server port directly — the same `process` path a
    // remote worker's HTTP calls take.
    let port: Arc<dyn ResonateServer> = state.clone();
    let mut workers: HashMap<String, Arc<dyn ResonateWorker>> = HashMap::new();
    workers.insert(
        SCHEME.to_string(),
        Arc::new(Worker::new(port, plugin_config(), lease_timeout)),
    );
    let router: Arc<dyn ResonateRouter> = Arc::new(TransportDispatcher::new(workers));

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    tokio::spawn(processing_timeouts::timeout_processing_loop(
        state.clone(),
        shutdown_rx.clone(),
    ));
    tokio::spawn(processing_messages::message_processing_loop(
        state.clone(),
        router,
        shutdown_rx,
    ));
    (state, shutdown_tx)
}

/// §2, with `poll` at 1s and the two render cadences cascading to it.
fn plugin_config() -> Config {
    Config {
        api_key: "bb_test_key".to_string(),
        poll: Duration::from_secs(1),
        poll_image: None,
        poll_animation: None,
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
                "timeoutAt": now_ms() + 120_000,
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
    for _ in 0..60 {
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

// ─── The mock provider ────────────────────────────────────────────────────────

/// One mock server for the whole binary: the plugin resolves its API root
/// once. The tests are serial, so resetting here gives each one a clean
/// server.
async fn mock() -> &'static MockServer {
    static SERVER: tokio::sync::OnceCell<MockServer> = tokio::sync::OnceCell::const_new();
    let server = SERVER
        .get_or_init(|| async {
            let server = MockServer::start().await;
            std::env::set_var("BANNERBEAR_API", format!("{}/v5", server.uri()));
            server
        })
        .await;
    server.reset().await;
    server
}

async fn create_stub(server: &MockServer, body: Value) {
    Mock::given(method("POST"))
        .and(path("/v5/images"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

fn render_args() -> Value {
    json!({
        "template": "tpl_a",
        "modifications": {"objects": [{"name": "title", "text": "hello"}]},
    })
}

fn promise_id(what: &str) -> String {
    format!("bannerbear.{what}.{}", nanos())
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
