//! End to end: a minimal Resonate server with only this plugin's worker.
//!
//! The only test that exercises the frame — claim, heartbeat, settle —
//! together with `plugin::process`. Everything upstream of the worker (the
//! HTTP edge, the poll transport, auth) is the server's own test surface,
//! not this crate's, so none of it is compiled in: an in-memory store, the
//! two background loops, a router holding one worker under one scheme.
//!
//! Zendesk is SaaS only — there is no §5 provider — so the work item that
//! succeeds (a ticket that closes) and the one that fails (a ticket deleted
//! before it closes) are provisioned on the same local mock `tests/process.rs`
//! uses, with the same circularity: the mock is built from the specification
//! the code was built from.

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
use resonate_plugin_zendesk::{plugin::Config, Worker, SCHEME};

/// The succeeding work item: the promise reaches `resolved`, carrying the
/// §4.1.2 Resolved value.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_promise_targeting_this_plugin_resolves_when_the_ticket_closes() {
    let mock = zendesk().await;
    mount_create(mock, 42).await;
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ticket(42, "closed")))
        .mount(mock)
        .await;
    let (server, _shutdown) = harness();
    let id = promise_id("e2e-ok");

    create(&server, &id, json!({"func": "ticket.create", "args": args()})).await;
    let (state, value) = settled(&server, &id).await;

    assert_eq!(state, "resolved", "{value}");
    assert_eq!(value["id"], 42);
    assert_eq!(value["status"], "closed");
    assert_eq!(value["subject"], "Printer on fire");
    assert_eq!(value.as_object().unwrap().len(), 6, "{value}");
}

/// The failing work item: the ticket is deleted before it ever closes, so the
/// promise reaches `rejected` with §4.1.2's `code: deleted`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_promise_targeting_this_plugin_rejects_when_the_ticket_is_deleted() {
    let mock = zendesk().await;
    mount_create(mock, 43).await;
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets/43"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"error": "RecordNotFound"})))
        .mount(mock)
        .await;
    let (server, _shutdown) = harness();
    let id = promise_id("e2e-deleted");

    create(&server, &id, json!({"func": "ticket.create", "args": args()})).await;
    let (state, value) = settled(&server, &id).await;

    assert_eq!(state, "rejected", "{value}");
    assert_eq!(value, json!({"code": "deleted"}));
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

/// §2, with `poll` at 1s.
fn plugin_config() -> Config {
    Config {
        subdomain: "acme".into(),
        email: "bot@acme.com".into(),
        api_token: "t0ken".into(),
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

// ─── The mock provider ────────────────────────────────────────────────────────

/// One mock for the whole binary: the plugin reads the API root once, so the
/// address it is pointed at cannot change afterwards.
async fn zendesk() -> &'static MockServer {
    static SERVER: tokio::sync::OnceCell<MockServer> = tokio::sync::OnceCell::const_new();
    let server = SERVER
        .get_or_init(|| async {
            let server = MockServer::start().await;
            std::env::set_var("ZENDESK_API", format!("{}/api/v2", server.uri()));
            server
        })
        .await;
    server.reset().await;
    server
}

/// The §4.1.5 opening: nothing found under the stamped `external_id`, so the
/// ticket is created.
async fn mount_create(server: &MockServer, id: i64) {
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"tickets": []})))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v2/tickets"))
        .respond_with(ResponseTemplate::new(201).set_body_json(ticket(id, "new")))
        .mount(server)
        .await;
}

/// A ticket as §4.1.4 shapes it.
fn ticket(id: i64, status: &str) -> Value {
    json!({
        "ticket": {
            "id": id,
            "status": status,
            "external_id": "stamped",
            "subject": "Printer on fire",
            "tags": ["urgent", "hardware"],
            "created_at": "2026-08-26T09:00:00Z",
            "updated_at": "2026-08-26T09:30:00Z",
        }
    })
}

/// §4.1.1 args, filled in.
fn args() -> Value {
    json!({
        "comment": {"body": "The printer is on fire.", "public": true},
        "subject": "Printer on fire",
        "priority": "high",
        "tags": ["urgent", "hardware"],
    })
}

fn promise_id(what: &str) -> String {
    format!("zendesk.{what}.{}", nanos())
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
