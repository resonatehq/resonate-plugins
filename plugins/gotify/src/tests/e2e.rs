//! End to end: a minimal Resonate server with only this plugin's worker.
//!
//! The only test that exercises the frame — claim, heartbeat, settle —
//! together with `plugin::process`. Everything upstream of the worker (the
//! HTTP edge, the poll transport, auth) is the server's own test surface,
//! not this crate's, so none of it is compiled in: an in-memory store, the
//! two background loops, a router holding one worker under one scheme.
//!
//! Bring the provider up with specification §5.1/§5.2 first — the same
//! environment `tests/process.rs` uses.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};

use resonate::config::Config as ServerConfig;
use resonate::processing::{processing_messages, processing_timeouts};
use resonate::server::Server;
use resonate::transport::TransportDispatcher;
use resonate_server_dbms::{persistence_sqlite::SqliteStorage, Storage};
use resonate_core::types::{RequestEnvelope, RequestHead, PROTOCOL_VERSION};
use resonate_core::{ResonateRouter, ResonateServer, ResonateWorker};
use resonate_plugin_gotify::{plugin::Config, Worker, SCHEME};

/// The succeeding work item — the application the client token's user owns:
/// the promise reaches `resolved`, carrying the §4.1.2 Resolved value.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_promise_targeting_this_plugin_resolves_from_an_accepted_message() {
    let (server, _shutdown) = harness();
    let id = promise_id("e2e-ok");

    create(
        &server,
        &id,
        json!({
            "func": "message.create",
            "args": {"appid": fixture("GOTIFY_FIXTURE_OK"), "message": "end to end"},
        }),
    )
    .await;
    let (state, value) = settled(&server, &id).await;

    assert_eq!(state, "resolved", "{value}");
    assert_eq!(value["appid"], fixture("GOTIFY_FIXTURE_OK"));
    assert_eq!(value["message"], "end to end");
    assert!(value["id"].is_number(), "{value}");
    // = sanitize(promise.id)
    let stamp = value["extras"]["resonate::promise"]
        .as_str()
        .expect("the stamp is a string");
    assert!(stamp.starts_with(&id), "{stamp} should be derived from {id}");
}

/// The failing work item — the application §5 gave a second user: the promise
/// reaches `rejected`, carrying the §4.1.2 Rejected value with
/// `code: application_not_found`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_promise_targeting_this_plugin_rejects_from_an_unowned_application() {
    let (server, _shutdown) = harness();
    let id = promise_id("e2e-fail");

    create(
        &server,
        &id,
        json!({
            "func": "message.create",
            "args": {"appid": fixture("GOTIFY_FIXTURE_FAIL"), "message": "end to end"},
        }),
    )
    .await;
    let (state, value) = settled(&server, &id).await;

    assert_eq!(state, "rejected", "{value}");
    assert_eq!(value, json!({"code": "application_not_found"}));
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

/// §2, from the environment §5.2 exports.
fn plugin_config() -> Config {
    Config {
        base_url: env("GOTIFY_BASE_URL"),
        client_token: env("GOTIFY_CLIENT_TOKEN"),
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

fn fixture(key: &str) -> i64 {
    env(key).parse().unwrap_or_else(|_| panic!("{key} is an integer"))
}

fn env(key: &str) -> String {
    std::env::var(key)
        .unwrap_or_else(|_| panic!("{key} is unset — run specification §5.1 and §5.2 first"))
}

fn promise_id(what: &str) -> String {
    format!("gotify.{what}.{}", nanos())
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
