//! End to end: a minimal Resonate server with only this plugin's worker.
//!
//! The only test that exercises the frame — claim, heartbeat, settle —
//! together with `plugin::process`. Everything upstream of the worker (the
//! HTTP edge, the poll transport, auth) is the server's own test surface,
//! not this crate's, so none of it is compiled in: an in-memory store, the
//! two background loops, a router holding one worker under one scheme.
//!
//! Zendesk is SaaS only — the specification has no §5 — so the work items are
//! stubbed on a local mock, as in `tests/process.rs`, and the same
//! circularity applies: the mock is built from the specification, so this can
//! show the wiring settles a promise, not that Zendesk behaves this way.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine;
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use resonate::config::Config as ServerConfig;
use resonate::deadlines;
use resonate::processing::processing_timeouts;
use resonate::server::Server;
use resonate_core::types::{Message, RequestEnvelope, RequestHead, PROTOCOL_VERSION};
use resonate_core::{scheme_of, ResonateRouter, ResonateServer, ResonateWorker, Unavailable};
use resonate_plugin_zendesk::{plugin::Config, Worker, SCHEME};
use resonate_server_dbms::engine_sqlite::SqliteEngine;

/// The succeeding work item, on the operation that injects the promise's
/// identity: the promise reaches `resolved` carrying the §4.1.2 Resolved
/// value, and the ticket was stamped with `sanitize(promise.id)`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_promise_targeting_this_plugin_resolves_from_a_created_ticket() {
    let server = mock().await;
    let created = json!({
        "id": 101,
        "url": "https://acme.zendesk.com/api/v2/tickets/101.json",
        "created_at": "2026-08-29T12:00:00Z",
        "updated_at": "2026-08-29T12:00:00Z",
        "status": "new",
        "subject": "Printer on fire",
        "description": "It is on fire.",
        "requester_id": 7,
        "tags": [],
        "custom_fields": [],
        "via": {"channel": "api", "source": {}},
    });
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"tickets": []})))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v2/tickets"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"ticket": created.clone()})))
        .mount(server)
        .await;

    let (resonate, _shutdown) = harness().await;
    let id = promise_id("e2e-create");

    create(
        &resonate,
        &id,
        json!({
            "func": "ticket.create",
            "args": {"subject": "Printer on fire", "comment": {"body": "It is on fire."}},
        }),
    )
    .await;
    let (state, value) = settled(&resonate, &id).await;

    assert_eq!(state, "resolved", "{value}");
    assert_eq!(value, created);
    // The ticket carried this promise's injected identity.
    let sent = server.received_requests().await.expect("recording is on");
    let post = sent.iter().find(|r| r.method.as_str() == "POST").expect("the create went out");
    let body: Value = post.body_json().expect("the create body is JSON");
    let external_id = body["ticket"]["external_id"].as_str().expect("external_id is a string");
    assert!(external_id.starts_with(&id), "{external_id} should be derived from {id}");
    assert_eq!(
        post.headers.get("idempotency-key").and_then(|v| v.to_str().ok()).unwrap_or_default(),
        external_id
    );
}

/// The work operation, watched to a terminal state under the frame: the merge
/// job completes and the promise reaches `resolved` with the §4.7.2 value.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_promise_targeting_this_plugin_resolves_from_a_completed_merge() {
    let server = mock().await;
    let results = json!([{"id": 202, "action": "update", "status": "Updated", "success": true}]);
    merge_stub(server, job("e2e_ok", "queued", Value::Null)).await;
    Mock::given(method("GET"))
        .and(path("/api/v2/job_statuses/e2e_ok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"job_status": job("e2e_ok", "completed", results.clone())}),
        ))
        .mount(server)
        .await;

    let (resonate, _shutdown) = harness().await;
    let id = promise_id("e2e-merge-ok");

    create(&resonate, &id, json!({"func": "ticket.merge", "args": merge_args()})).await;
    let (state, value) = settled(&resonate, &id).await;

    assert_eq!(state, "resolved", "{value}");
    assert_eq!(value["id"], "e2e_ok");
    assert_eq!(value["status"], "completed");
    assert_eq!(value["results"], results);
    assert_eq!(value["message"], Value::Null);
}

/// The failing work item: the merge job reaches "failed", so the promise
/// reaches `rejected` carrying the §4.7.2 Rejected value with
/// `code: merge_failed`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_promise_targeting_this_plugin_rejects_from_a_failed_merge() {
    let server = mock().await;
    merge_stub(server, job("e2e_fail", "queued", Value::Null)).await;
    Mock::given(method("GET"))
        .and(path("/api/v2/job_statuses/e2e_fail"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"job_status": job("e2e_fail", "failed", Value::Null)}),
        ))
        .mount(server)
        .await;

    let (resonate, _shutdown) = harness().await;
    let id = promise_id("e2e-merge-fail");

    create(&resonate, &id, json!({"func": "ticket.merge", "args": merge_args()})).await;
    let (state, value) = settled(&resonate, &id).await;

    assert_eq!(state, "rejected", "{value}");
    assert_eq!(value["code"], "merge_failed");
    assert_eq!(value["detail"]["status"], "failed");
    assert_eq!(value["detail"]["id"], "e2e_fail");
}

// ─── The server ───────────────────────────────────────────────────────────────

/// A router with one late worker.
///
/// The ring is server -> router -> worker -> server, and `Server::new` takes
/// the router: something in it has to be late. The composition root closes the
/// ring with `Arc::new_cyclic` and hands each worker a weak handle; the plugin
/// frame takes a strong `Arc`, so here the router is the late link instead.
/// Nothing routes before the worker is set — the first promise is created
/// after `harness` returns.
#[derive(Default)]
struct LateRouter {
    worker: std::sync::OnceLock<Arc<dyn ResonateWorker>>,
}

#[async_trait]
impl ResonateRouter for LateRouter {
    async fn route(&self, address: &str, msg: &Message) -> Result<(), Unavailable> {
        // This plugin's scheme is the only one registered: nothing else is a
        // way out of this server.
        if scheme_of(address).as_deref() != Some(SCHEME) {
            return Err(Unavailable::unroutable(format!(
                "no worker registered for {address}"
            )));
        }
        let worker = self
            .worker
            .get()
            .ok_or_else(|| Unavailable::new("the worker is not registered yet"))?;
        worker.send(address, msg).await
    }
}

/// A whole Resonate server, in this process, with one worker on it. The
/// returned sender owns the background loop: dropping it stops it, so a test
/// holds it for as long as it holds the server.
async fn harness() -> (Arc<Server>, tokio::sync::watch::Sender<bool>) {
    let mut config = ServerConfig::default();
    // Nothing else is enabled: this plugin is the only way out of the server.
    config.transports.http_push.enabled = false;
    config.transports.http_poll.enabled = false;
    let lease_timeout = config.tasks.lease_timeout;

    // Durable state, in memory: nothing this test writes outlives it.
    let engine = Arc::new(
        SqliteEngine::open(
            ":memory:",
            config.tasks.retry_timeout,
            config.storage.sqlite.preload_limit,
            config.storage.sqlite.migrate,
            config.debug,
        )
        .expect("in-memory engine"),
    );

    let router = Arc::new(LateRouter::default());
    let state = Arc::new_cyclic(|weak: &std::sync::Weak<Server>| {
        let timer = deadlines::build(&config.timeouts, weak.clone());
        Server::new(
            config,
            engine,
            Arc::clone(&router) as Arc<dyn ResonateRouter>,
            timer,
        )
    });

    // The worker holds the server port directly — the same `process` path a
    // remote worker's HTTP calls take.
    let worker: Arc<dyn ResonateWorker> = Arc::new(Worker::new(
        Arc::clone(&state) as Arc<dyn ResonateServer>,
        plugin_config(),
        lease_timeout,
    ));
    let _ = router.worker.set(worker);

    state.start_timer().await;
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    tokio::spawn(processing_timeouts::timeout_processing_loop(
        Arc::clone(&state),
        shutdown_rx,
    ));
    (state, shutdown_tx)
}

/// §2, with `poll` at 1s.
fn plugin_config() -> Config {
    Config {
        subdomain: "acme".to_string(),
        email: "agent@acme.com".to_string(),
        api_token: "zd_test_token".to_string(),
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
            std::env::set_var("ZENDESK_API", format!("{}/api/v2", server.uri()));
            server
        })
        .await;
    server.reset().await;
    server
}

async fn merge_stub(server: &MockServer, job_status: Value) {
    Mock::given(method("POST"))
        .and(path("/api/v2/tickets/101/merge"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"job_status": job_status})))
        .mount(server)
        .await;
}

/// A §4.7.4 job status object.
fn job(id: &str, status: &str, results: Value) -> Value {
    json!({
        "id": id,
        "url": format!("https://acme.zendesk.com/api/v2/job_statuses/{id}"),
        "job_type": "TicketMergeJob",
        "status": status,
        "progress": if status == "queued" { 0 } else { 1 },
        "total": 1,
        "results": results,
    })
}

fn merge_args() -> Value {
    json!({"ticket_id": 101, "ids": [202], "target_comment": "merged the duplicates"})
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
