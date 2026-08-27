//! `plugin::process` against a local mock standing where Zendesk would be.
//!
//! Zendesk is SaaS only — the specification's Notes say `Self-hosted: no`, so
//! there is no §5 environment to run the plugin against. The mock answers with
//! the §4.N.4 Integration Response bodies and the statuses each §4.N.5 names,
//! and every outgoing request is asserted against the §4.N.3 Integration
//! Request: method, path, the injected `sanitize(promise.id)`, and the body
//! mapping.
//!
//! The circularity is real and worth stating: these mocks are built from the
//! same specification as the code they exercise, so agreement between them
//! proves internal consistency and nothing about Zendesk. Only review against
//! the live provider (skills/plugin-review) breaks the loop.
//!
//! One mock serves the whole binary — the plugin reads the API root once,
//! before its first request — so the tests are serial (`.cargo/config.toml`)
//! and each resets the mock before mounting its own.

use std::collections::HashMap;
use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Match, Mock, MockServer, Request, ResponseTemplate};

use resonate_core::types::{PromiseRecord, PromiseState, PromiseValue};
use resonate_plugin_zendesk::plugin::{self, Config};

/// The verdict `process` returns, spelled out: the crate's own alias for it
/// is an implementation detail.
type Verdict = Result<Result<String, String>, Result<String, String>>;

/// A ticket as §4.1.4 shapes it.
fn ticket(id: i64, status: &str) -> Value {
    json!({
        "ticket": {
            "id": id,
            "status": status,
            "external_id": "stamped",
            "subject": "Printer on fire",
            "tags": ["urgent", "hardware"],
            "url": format!("https://acme.zendesk.com/api/v2/tickets/{id}.json"),
            "created_at": "2026-08-26T09:00:00Z",
            "updated_at": "2026-08-26T09:30:00Z",
        }
    })
}

// ─── ticket.create (§4.1) ─────────────────────────────────────────────────────

/// resolved — the ticket reaches `closed`. Also the pending → terminal path:
/// the first poll sees `solved`, which is *not* terminal, so a resolved value
/// at all is proof the loop kept watching. And the §4.1.3 Integration Request:
/// the lookup query, the `Idempotency-Key` header, and `ticket.external_id`
/// are one and the same stamped identity.
#[tokio::test]
async fn ticket_create_resolves_when_the_ticket_closes() {
    let server = zendesk().await;
    let id = promise_id("create-ok");
    mount_lookup(server, &id, json!({"tickets": []})).await;
    Mock::given(method("POST"))
        .and(path("/api/v2/tickets"))
        .and(header("authorization", GOOD.as_str()))
        .and(Stamped::header("idempotency-key", &id))
        .respond_with(ResponseTemplate::new(201).set_body_json(ticket(42, "new")))
        .mount(server)
        .await;
    // "solved" reopens on customer reply; only "closed" is frozen.
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ticket(42, "solved")))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ticket(42, "closed")))
        .mount(server)
        .await;

    let p = promise(&id, "ticket.create", args(), in_ms(60_000));
    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["id"], 42);
    assert_eq!(value["status"], "closed");
    assert_eq!(value["subject"], "Printer on fire");
    assert_eq!(value["tags"], json!(["urgent", "hardware"]));
    assert_eq!(value["created_at"], "2026-08-26T09:00:00Z");
    assert_eq!(value["updated_at"], "2026-08-26T09:30:00Z");
    // The 4.1.2 Resolved schema is exactly these six keys — `url` and
    // `external_id` are on the wire but not in the mapping.
    assert_eq!(value.as_object().unwrap().len(), 6, "{value}");

    // §4.1.3, on the wire.
    let requests = server.received_requests().await.unwrap();
    let post = requests.iter().find(|r| is(r, "POST")).expect("a POST");
    let body: Value = serde_json::from_slice(&post.body).expect("the POST body is JSON");
    assert_eq!(body["ticket"]["comment"], args()["comment"]);
    assert_eq!(body["ticket"]["subject"], "Printer on fire");
    assert_eq!(body["ticket"]["priority"], "high");
    let stamp = body["ticket"]["external_id"].as_str().expect("external_id");
    // = sanitize(promise.id): the frame keeps the promise id as its prefix.
    assert!(stamp.starts_with(&id), "{stamp} should be derived from {id}");
    assert_eq!(post.headers["idempotency-key"], stamp);
    let lookup = requests.first().expect("the lookup comes first");
    assert_eq!(lookup.url.path(), "/api/v2/tickets");
    assert_eq!(query(lookup, "external_id"), Some(stamp.to_string()));
    // The poll saw a non-terminal state and asked again.
    assert_eq!(requests.iter().filter(|r| r.url.path() == "/api/v2/tickets/42").count(), 2);
}

/// `invalid_request` — the create is refused as a property of the request.
#[tokio::test]
async fn ticket_create_rejects_invalid_request() {
    let server = zendesk().await;
    let id = promise_id("create-invalid");
    mount_lookup(server, &id, json!({"tickets": []})).await;
    let error = json!({"error": "RecordInvalid", "details": {"base": [{"description": "Requester: Email is invalid"}]}});
    Mock::given(method("POST"))
        .and(path("/api/v2/tickets"))
        .respond_with(ResponseTemplate::new(422).set_body_json(error.clone()))
        .mount(server)
        .await;

    let p = promise(&id, "ticket.create", args(), in_ms(60_000));
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    // detail = response.body of the 4xx.
    assert_eq!(value["detail"], error);
}

/// `deleted` — the ticket is soft-deleted before it ever closes.
#[tokio::test]
async fn ticket_create_rejects_deleted() {
    let server = zendesk().await;
    let id = promise_id("create-deleted");
    mount_lookup(server, &id, json!({"tickets": []})).await;
    Mock::given(method("POST"))
        .and(path("/api/v2/tickets"))
        .respond_with(ResponseTemplate::new(201).set_body_json(ticket(43, "open")))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets/43"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"error": "RecordNotFound"})))
        .mount(server)
        .await;

    let p = promise(&id, "ticket.create", args(), in_ms(60_000));
    let value = rejected(plugin::process(&config(), &p).await);

    // detail: absent (the 404 has no body worth keeping).
    assert_eq!(value, json!({"code": "deleted"}));
}

/// Re-entry — a redelivery re-attaches to the ticket an earlier attempt
/// created rather than opening a second one, and picks deterministically when
/// the un-enforced `external_id` has more than one hit.
#[tokio::test]
async fn ticket_create_reattaches_on_re_entry() {
    let server = zendesk().await;
    let id = promise_id("create-reentry");
    let hits = json!({"tickets": [ticket(91, "open")["ticket"], ticket(77, "closed")["ticket"]]});
    mount_lookup(server, &id, hits).await;
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets/77"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ticket(77, "closed")))
        .mount(server)
        .await;

    let p = promise(&id, "ticket.create", args(), in_ms(60_000));
    let value = resolved(plugin::process(&config(), &p).await);

    // min(ids): external_id is not unique, so the choice is deterministic.
    assert_eq!(value["id"], 77);
    let requests = server.received_requests().await.unwrap();
    assert!(!requests.iter().any(|r| is(r, "POST")), "a second ticket was opened");
}

/// The deadline — `timeout_at` already in the past. No verdict: the server
/// settles a timed-out promise itself.
#[tokio::test]
async fn ticket_create_releases_at_the_deadline() {
    let server = zendesk().await;
    let id = promise_id("create-deadline");
    mount_lookup(server, &id, json!({"tickets": [ticket(55, "open")["ticket"]]})).await;

    let p = promise(&id, "ticket.create", args(), in_ms(-1_000));
    let reason = released(plugin::process(&config(), &p).await);

    assert_eq!(reason, "promise timed out");
}

/// halt — the credential is rejected, and no retry of ours can fix that.
#[tokio::test]
async fn ticket_create_halts_on_rejected_credentials() {
    let server = zendesk().await;
    mount_unauthorized(server).await;
    let id = promise_id("create-halt");

    let p = promise(&id, "ticket.create", args(), in_ms(60_000));
    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("Couldn't authenticate you"), "{reason}");
}

/// `invalid_request`, decided locally — `comment` is required by §4.1.1, and a
/// promise's param is immutable, so no redelivery can mend it.
#[tokio::test]
async fn ticket_create_rejects_a_param_without_a_comment() {
    let server = zendesk().await;
    let id = promise_id("create-nocomment");

    let p = promise(&id, "ticket.create", json!({"subject": "Printer on fire"}), in_ms(60_000));
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(value["detail"].as_str().unwrap_or_default().contains("comment"), "{value}");
    assert!(server.received_requests().await.unwrap().is_empty(), "the provider was called");
}

// ─── ticket.comment (§4.2) ────────────────────────────────────────────────────

/// resolved — the comment is new, so it is PUT, and the updated ticket is the
/// value. Also §4.2.3 on the wire: the body is the comment, re-wrapped.
#[tokio::test]
async fn ticket_comment_resolves_after_putting_the_comment() {
    let server = zendesk().await;
    mount_comments(server, 42, json!({"comments": [{"id": 1, "body": "something else"}]})).await;
    Mock::given(method("PUT"))
        .and(path("/api/v2/tickets/42"))
        .and(header("authorization", GOOD.as_str()))
        .and(body_json(json!({"ticket": {"comment": {"body": "on it", "public": false}}})))
        .respond_with(ResponseTemplate::new(200).set_body_json(ticket(42, "open")))
        .mount(server)
        .await;

    let p = promise(&promise_id("comment-ok"), "ticket.comment", comment_args(), in_ms(60_000));
    let value = resolved(plugin::process(&config(), &p).await);

    // = response.body.ticket, whole.
    assert_eq!(value, ticket(42, "open")["ticket"]);
}

/// Re-entry — the identical body is already on the ticket, so the unkeyed PUT
/// is skipped and the current record answers instead.
#[tokio::test]
async fn ticket_comment_resolves_from_the_record_when_the_body_already_landed() {
    let server = zendesk().await;
    mount_comments(server, 42, json!({"comments": [{"id": 9, "body": "on it"}]})).await;
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ticket(42, "pending")))
        .mount(server)
        .await;

    let p = promise(&promise_id("comment-dedup"), "ticket.comment", comment_args(), in_ms(60_000));
    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value, ticket(42, "pending")["ticket"]);
    let requests = server.received_requests().await.unwrap();
    assert!(!requests.iter().any(|r| is(r, "PUT")), "the comment was duplicated");
}

/// `not_found` — no such ticket, seen on the dedup scan.
#[tokio::test]
async fn ticket_comment_rejects_not_found() {
    let server = zendesk().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets/42/comments"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"error": "RecordNotFound"})))
        .mount(server)
        .await;

    let p = promise(&promise_id("comment-404"), "ticket.comment", comment_args(), in_ms(60_000));
    let value = rejected(plugin::process(&config(), &p).await);

    // detail: absent.
    assert_eq!(value, json!({"code": "not_found"}));
}

/// `not_found` — the ticket disappears between the scan and the PUT.
#[tokio::test]
async fn ticket_comment_rejects_not_found_on_the_put() {
    let server = zendesk().await;
    mount_comments(server, 42, json!({"comments": []})).await;
    Mock::given(method("PUT"))
        .and(path("/api/v2/tickets/42"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"error": "RecordNotFound"})))
        .mount(server)
        .await;

    let p = promise(&promise_id("comment-put404"), "ticket.comment", comment_args(), in_ms(60_000));
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "not_found"}));
}

/// `closed` — a closed ticket is frozen, and the 422 says so.
#[tokio::test]
async fn ticket_comment_rejects_closed() {
    let server = zendesk().await;
    mount_comments(server, 42, json!({"comments": []})).await;
    let error = json!({
        "error": "RecordInvalid",
        "description": "Record validation errors",
        "details": {"status": [{"description": "Status: closed prevents ticket update"}]},
    });
    Mock::given(method("PUT"))
        .and(path("/api/v2/tickets/42"))
        .respond_with(ResponseTemplate::new(422).set_body_json(error.clone()))
        .mount(server)
        .await;

    let p = promise(&promise_id("comment-closed"), "ticket.comment", comment_args(), in_ms(60_000));
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "closed");
    assert_eq!(value["detail"], error);
}

/// `invalid_request` — a validation failure with no frozen-ticket signal.
#[tokio::test]
async fn ticket_comment_rejects_invalid_request() {
    let server = zendesk().await;
    mount_comments(server, 42, json!({"comments": []})).await;
    let error = json!({
        "error": "RecordInvalid",
        "details": {"base": [{"description": "Comment: cannot be blank"}]},
    });
    Mock::given(method("PUT"))
        .and(path("/api/v2/tickets/42"))
        .respond_with(ResponseTemplate::new(400).set_body_json(error.clone()))
        .mount(server)
        .await;

    let p = promise(&promise_id("comment-invalid"), "ticket.comment", comment_args(), in_ms(60_000));
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert_eq!(value["detail"], error);
}

/// halt — the credential is rejected.
#[tokio::test]
async fn ticket_comment_halts_on_rejected_credentials() {
    let server = zendesk().await;
    mount_unauthorized(server).await;

    let p = promise(&promise_id("comment-halt"), "ticket.comment", comment_args(), in_ms(60_000));
    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("Couldn't authenticate you"), "{reason}");
}

/// `invalid_request`, decided locally — §4.2.1 requires `comment.body`.
#[tokio::test]
async fn ticket_comment_rejects_a_param_without_a_body() {
    let server = zendesk().await;

    let p = promise(
        &promise_id("comment-nobody"),
        "ticket.comment",
        json!({"id": 42, "comment": {"public": true}}),
        in_ms(60_000),
    );
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(value["detail"].as_str().unwrap_or_default().contains("body"), "{value}");
    assert!(server.received_requests().await.unwrap().is_empty(), "the provider was called");
}

// ─── ticket.get (§4.3) ────────────────────────────────────────────────────────

/// resolved — = response.body.ticket.
#[tokio::test]
async fn ticket_get_resolves_with_the_ticket() {
    let server = zendesk().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets/42"))
        .and(header("authorization", GOOD.as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(ticket(42, "solved")))
        .mount(server)
        .await;

    let p = promise(&promise_id("get-ok"), "ticket.get", json!({"id": 42}), in_ms(60_000));
    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value, ticket(42, "solved")["ticket"]);
}

/// `not_found` — deleted, or never existed.
#[tokio::test]
async fn ticket_get_rejects_not_found() {
    let server = zendesk().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets/999"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"error": "RecordNotFound"})))
        .mount(server)
        .await;

    let p = promise(&promise_id("get-404"), "ticket.get", json!({"id": 999}), in_ms(60_000));
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "not_found"}));
}

/// halt — the credential is rejected.
#[tokio::test]
async fn ticket_get_halts_on_rejected_credentials() {
    let server = zendesk().await;
    mount_unauthorized(server).await;

    let p = promise(&promise_id("get-halt"), "ticket.get", json!({"id": 42}), in_ms(60_000));
    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("Couldn't authenticate you"), "{reason}");
}

/// `invalid_request`, decided locally — §4.3.1 requires an integer `id`.
#[tokio::test]
async fn ticket_get_rejects_a_param_without_an_id() {
    let server = zendesk().await;

    let p = promise(&promise_id("get-noid"), "ticket.get", json!({"id": "42"}), in_ms(60_000));
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(value["detail"].as_str().unwrap_or_default().contains("id"), "{value}");
    assert!(server.received_requests().await.unwrap().is_empty(), "the provider was called");
}

// ─── Dispatch ─────────────────────────────────────────────────────────────────

/// A func no operation serves is a permanent rejection naming it.
#[tokio::test]
async fn an_unknown_func_is_rejected() {
    let _server = zendesk().await;
    let p = promise(&promise_id("unknown"), "ticket.delete", json!({}), in_ms(60_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "unknown_func", "detail": "ticket.delete"}));
}

/// A param that is not the `{func, args}` envelope is rejected by the frame.
#[tokio::test]
async fn an_undecodable_param_is_rejected() {
    let _server = zendesk().await;
    let mut p = promise(&promise_id("junk"), "ticket.get", json!({"id": 1}), in_ms(60_000));
    p.param.data = Some("not base64!!".into());

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
}

// ─── The mock provider ────────────────────────────────────────────────────────

/// One mock for the whole binary: the plugin reads the API root once, so the
/// address it is pointed at cannot change afterwards. Each test resets it and
/// mounts its own expectations — hence the serial run.
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

/// `GET /api/v2/tickets?external_id={sanitize(promise.id)}` — §4.1.5's
/// recovery lookup.
async fn mount_lookup(server: &MockServer, promise_id: &str, body: Value) {
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets"))
        .and(header("authorization", GOOD.as_str()))
        .and(Stamped::query("external_id", promise_id))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

/// `GET /api/v2/tickets/{id}/comments?sort_order=desc&per_page=100` — §4.2.5's
/// dedup scan.
async fn mount_comments(server: &MockServer, id: i64, body: Value) {
    Mock::given(method("GET"))
        .and(path(format!("/api/v2/tickets/{id}/comments")))
        .and(header("authorization", GOOD.as_str()))
        .and(query_param("sort_order", "desc"))
        .and(query_param("per_page", "100"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

/// What Zendesk answers a bad API token with — §3's credential, refused.
async fn mount_unauthorized(server: &MockServer) {
    Mock::given(header("authorization", BAD.as_str()))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(json!({"error": {"title": "Couldn't authenticate you"}})),
        )
        .mount(server)
        .await;
}

/// A value the frame stamped: `= sanitize(promise.id)`. `sanitize` keeps the
/// promise id as the prefix of its yield, which is what a test outside the
/// frame can assert about it.
struct Stamped {
    key: String,
    prefix: String,
    header: bool,
}

impl Stamped {
    fn query(key: &str, promise_id: &str) -> Self {
        Self { key: key.into(), prefix: promise_id.into(), header: false }
    }

    fn header(key: &str, promise_id: &str) -> Self {
        Self { key: key.into(), prefix: promise_id.into(), header: true }
    }
}

impl Match for Stamped {
    fn matches(&self, request: &Request) -> bool {
        let value = if self.header {
            request.headers.get(self.key.as_str()).and_then(|v| v.to_str().ok()).map(str::to_string)
        } else {
            query(request, &self.key)
        };
        value.is_some_and(|v| v.starts_with(&self.prefix))
    }
}

fn query(request: &Request, key: &str) -> Option<String> {
    request
        .url
        .query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

fn is(request: &Request, verb: &str) -> bool {
    request.method.as_str() == verb
}

// ─── Fixtures ─────────────────────────────────────────────────────────────────

/// §2. `poll` is overridden to 1s: the default 15m is sized to a human
/// support process, and a test cannot wait one.
fn config() -> Config {
    Config {
        subdomain: "acme".into(),
        email: EMAIL.into(),
        api_token: TOKEN.into(),
        poll: Duration::from_secs(1),
    }
}

/// The same Zendesk, with a token it will reject.
fn bad_credential() -> Config {
    Config { api_token: format!("{TOKEN}-wrong"), ..config() }
}

const EMAIL: &str = "bot@acme.com";
const TOKEN: &str = "t0ken";

/// §3: `Authorization: Basic base64("{email}/token:{api_token}")`.
fn basic(token: &str) -> String {
    let credential = format!("{EMAIL}/token:{token}");
    format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(credential))
}

static GOOD: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| basic(TOKEN));
static BAD: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| basic(&format!("{TOKEN}-wrong")));

/// §4.1.1 args, filled in.
fn args() -> Value {
    json!({
        "comment": {"body": "The printer is on fire.", "public": true},
        "subject": "Printer on fire",
        "priority": "high",
        "tags": ["urgent", "hardware"],
    })
}

/// §4.2.1 args, filled in.
fn comment_args() -> Value {
    json!({"id": 42, "comment": {"body": "on it", "public": false}})
}

fn promise(id: &str, func: &str, args: Value, timeout_at: i64) -> PromiseRecord {
    let param = json!({ "func": func, "args": args }).to_string();
    PromiseRecord {
        id: id.to_string(),
        state: PromiseState::Pending,
        param: PromiseValue {
            headers: None,
            data: Some(base64::engine::general_purpose::STANDARD.encode(param)),
        },
        value: PromiseValue::default(),
        tags: HashMap::new(),
        timeout_at,
        created_at: now_ms(),
        settled_at: None,
    }
}

/// A fresh promise id per test — and one that survives the frame's sanitize
/// unchanged, so a stamped value may be checked against it with
/// `starts_with`.
fn promise_id(what: &str) -> String {
    format!("zendesk.{what}.{}", nanos())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn in_ms(delta: i64) -> i64 {
    now_ms() + delta
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

// ─── Verdicts ─────────────────────────────────────────────────────────────────

fn resolved(verdict: Verdict) -> Value {
    match verdict {
        Ok(Ok(v)) => serde_json::from_str(&v).expect("resolved value is JSON"),
        other => panic!("expected resolved, got {other:?}"),
    }
}

fn rejected(verdict: Verdict) -> Value {
    match verdict {
        Ok(Err(v)) => serde_json::from_str(&v).expect("rejected value is JSON"),
        other => panic!("expected rejected, got {other:?}"),
    }
}

fn halted(verdict: Verdict) -> String {
    match verdict {
        Err(Ok(reason)) => reason,
        other => panic!("expected halt, got {other:?}"),
    }
}

fn released(verdict: Verdict) -> String {
    match verdict {
        Err(Err(reason)) => reason,
        other => panic!("expected release, got {other:?}"),
    }
}
