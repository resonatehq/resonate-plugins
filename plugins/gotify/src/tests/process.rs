//! `plugin::process` against a real Gotify — never a mock.
//!
//! Bring the provider up with specification §5.1/§5.2 first; the exported
//! `GOTIFY_BASE_URL`, `GOTIFY_CLIENT_TOKEN`, `GOTIFY_FIXTURE_OK` and
//! `GOTIFY_FIXTURE_FAIL` are what [`config`] and the fixtures below read.
//! Every documented condition is induced with real inputs against that
//! instance — a real application accepts a real notification, an application
//! this token's user does not own is really invisible, a corrupted token is
//! really rejected.
//!
//! §5 provisions the two work items: `GOTIFY_FIXTURE_OK` is an application
//! owned by the user the client token belongs to (the succeeding item), and
//! `GOTIFY_FIXTURE_FAIL` is an application owned by a second user (the
//! failing item — Gotify reports it exactly as it reports an application that
//! does not exist).
//!
//! Two rows of the induction table have no counterpart here. Every operation
//! is `request_response`: nothing polls, so there is no pending → terminal
//! path and no deadline the plugin can observe — `timeout_at` bounds no loop.
//! And §4.2 `application.list` takes no arguments and `GET /application`
//! accepts no input, so its `invalid_request` has no constraint left to
//! violate; the only 4xx that endpoint produces is the credential failure
//! `_check` turns into a halt.

use std::collections::HashMap;

use base64::Engine;
use serde_json::{json, Value};

use resonate_core::types::{PromiseRecord, PromiseState, PromiseValue};
use resonate_plugin_gotify::plugin::{self, Config};

/// The verdict `process` returns, spelled out: the crate's own alias for it
/// is an implementation detail.
type Verdict = Result<Result<String, String>, Result<String, String>>;

/// §4.1.5 `STAMP`.
const STAMP: &str = "resonate::promise";

// ─── message.create (§4.1) ────────────────────────────────────────────────────

/// resolved — the succeeding work item: the application the client token's
/// user owns. The resolved value is the created message, and it carries the
/// injected identity in its extras.
#[tokio::test]
async fn message_create_resolves_with_the_created_message() {
    let id = promise_id("create-ok");
    let p = promise(
        &id,
        "message.create",
        json!({
            "appid": fixture_ok(),
            "message": "resonate says hello",
            "title": "resonate",
            "priority": 5,
            "extras": {"client::display": {"contentType": "text/plain"}},
        }),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["appid"], fixture_ok());
    assert_eq!(value["message"], "resonate says hello");
    assert_eq!(value["title"], "resonate");
    assert_eq!(value["priority"], 5);
    assert!(value["id"].is_number(), "{value}");
    assert!(value["date"].is_string(), "{value}");
    // The caller's extras survive alongside the injected identity.
    assert_eq!(value["extras"]["client::display"]["contentType"], "text/plain");
    // = sanitize(promise.id)
    let stamp = value["extras"][STAMP].as_str().expect("the stamp is a string");
    assert!(stamp.starts_with(&id), "{stamp} should be derived from {id}");
}

/// `application_not_found` — the failing work item: an application owned by
/// another user. Gotify answers that exactly as it answers an application id
/// that names nothing at all, so both inductions are this one condition.
#[tokio::test]
async fn message_create_rejects_application_not_found() {
    let p = promise(
        &promise_id("create-otheruser"),
        "message.create",
        json!({"appid": fixture_fail(), "message": "not yours"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    // detail: absent.
    assert_eq!(value, json!({"code": "application_not_found"}));

    let p = promise(
        &promise_id("create-noapp"),
        "message.create",
        json!({"appid": 987_654, "message": "nowhere"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "application_not_found"}));
}

/// `invalid_request` from the create request — `title` is documented as a
/// string.
#[tokio::test]
async fn message_create_rejects_invalid_request_on_the_create() {
    let p = promise(
        &promise_id("create-badtitle"),
        "message.create",
        json!({"appid": fixture_ok(), "message": "x", "title": 123}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    // detail = response.body.errorDescription of the 4xx.
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("title"),
        "{value}"
    );
}

/// `invalid_request` from the locate request — `appid` is documented as an
/// integer, and a malformed one is rejected by the scan before anything is
/// sent.
#[tokio::test]
async fn message_create_rejects_invalid_request_on_the_locate() {
    let p = promise(
        &promise_id("create-badappid"),
        "message.create",
        json!({"appid": "not-an-integer", "message": "x"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert_eq!(value["detail"], "invalid id");
}

/// `invalid_request` decided locally — `appid` is required by the Param
/// schema, and a promise param is immutable, so no redelivery can supply it.
#[tokio::test]
async fn message_create_rejects_invalid_request_without_appid() {
    let p = promise(
        &promise_id("create-noappid"),
        "message.create",
        json!({"message": "x"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("appid"),
        "{value}"
    );
}

/// halt — the credential is rejected, and no retry of ours can fix that.
#[tokio::test]
async fn message_create_halts_on_a_rejected_credential() {
    let p = promise(
        &promise_id("create-halt"),
        "message.create",
        json!({"appid": fixture_ok(), "message": "x"}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    // The halt reason is the body of the 401 `_check` classified.
    assert_eq!(error_code(&reason), 401, "{reason}");
}

/// Re-entry — a redelivery finds the message the earlier attempt stamped
/// instead of sending a second notification.
#[tokio::test]
async fn message_create_recovers_on_re_entry() {
    let id = promise_id("create-reentry");
    let p = promise(
        &id,
        "message.create",
        json!({"appid": fixture_ok(), "message": "exactly once"}),
        in_ms(60_000),
    );

    let first = resolved(plugin::process(&config(), &p).await);
    let second = resolved(plugin::process(&config(), &p).await);

    assert_eq!(first["id"], second["id"]);
    assert_eq!(first["date"], second["date"]);
    // The recovered value has the same Message shape as the created one.
    assert_eq!(second["message"], "exactly once");
    assert_eq!(second["appid"], fixture_ok());
    // One promise, one notification.
    assert_eq!(stamped_messages(fixture_ok(), &id).await.len(), 1);
}

// ─── application.list (§4.2) ──────────────────────────────────────────────────

/// resolved — = response.body: the applications of the client token's user,
/// unpaged. The application §5 gave the other user is not among them.
#[tokio::test]
async fn application_list_resolves_with_the_applications() {
    let p = promise(
        &promise_id("app-list"),
        "application.list",
        json!({}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    let apps = value.as_array().expect("the body is an array");
    let ok = apps
        .iter()
        .find(|a| a["id"] == fixture_ok())
        .unwrap_or_else(|| panic!("{} is not listed: {value}", fixture_ok()));
    assert_eq!(ok["name"], "resonate-ok");
    assert_eq!(ok["description"], "owned by admin");
    assert_eq!(ok["internal"], false);
    assert!(ok["image"].is_string(), "{ok}");
    assert!(ok["createdAt"].is_string(), "{ok}");
    assert!(ok["sortKey"].is_string(), "{ok}");
    assert!(ok["defaultPriority"].is_number(), "{ok}");
    assert!(
        !apps.iter().any(|a| a["id"] == fixture_fail()),
        "another user's application is listed: {value}"
    );
}

/// halt — the credential is rejected.
#[tokio::test]
async fn application_list_halts_on_a_rejected_credential() {
    let p = promise(
        &promise_id("app-list-halt"),
        "application.list",
        json!({}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    // The halt reason is the body of the 401 `_check` classified.
    assert_eq!(error_code(&reason), 401, "{reason}");
}

// ─── message.list (§4.3) ──────────────────────────────────────────────────────

/// resolved — = response.body, one page newest first. The arguments are
/// passed straight through as query parameters.
#[tokio::test]
async fn message_list_resolves_with_one_page() {
    // Two messages, so a limit of 1 leaves a further page and `paging.next`
    // is present.
    let seeded = promise_id("list-seed");
    send_message(fixture_ok(), &format!("{seeded}-a")).await;
    send_message(fixture_ok(), &format!("{seeded}-b")).await;

    let p = promise(
        &promise_id("msg-list"),
        "message.list",
        json!({"limit": 1}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["paging"]["limit"], 1);
    assert_eq!(value["paging"]["size"], 1);
    let messages = value["messages"].as_array().expect("messages");
    assert_eq!(messages.len(), 1, "{value}");
    assert_eq!(messages[0]["message"], format!("{seeded}-b"), "newest first");
    assert!(messages[0]["id"].is_number(), "{value}");
    // A further page exists, so paging.since names it and paging.next is the
    // path of it.
    assert_eq!(value["paging"]["since"], messages[0]["id"]);
    assert!(
        value["paging"]["next"].as_str().unwrap_or_default().contains("since="),
        "{value}"
    );
}

/// `invalid_request` — `limit` is documented as `maximum: 200`.
#[tokio::test]
async fn message_list_rejects_invalid_request() {
    let p = promise(
        &promise_id("msg-list-bad"),
        "message.list",
        json!({"limit": 300}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    // detail = response.body.errorDescription of the 4xx.
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("limit"),
        "{value}"
    );
}

/// halt — the credential is rejected.
#[tokio::test]
async fn message_list_halts_on_a_rejected_credential() {
    let p = promise(
        &promise_id("msg-list-halt"),
        "message.list",
        json!({}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    // The halt reason is the body of the 401 `_check` classified.
    assert_eq!(error_code(&reason), 401, "{reason}");
}

// ─── Dispatch ─────────────────────────────────────────────────────────────────

/// A func no operation serves is a permanent rejection naming it.
#[tokio::test]
async fn an_unknown_func_is_rejected() {
    let p = promise(
        &promise_id("unknown"),
        "message.explode",
        json!({}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "unknown_func", "detail": "message.explode"}));
}

// ─── Fixtures ─────────────────────────────────────────────────────────────────

/// §2, built from the environment §5.2 exports. Gotify has no `poll` key:
/// every operation is decided by the response of its own request.
fn config() -> Config {
    Config {
        base_url: env("GOTIFY_BASE_URL"),
        client_token: env("GOTIFY_CLIENT_TOKEN"),
    }
}

/// The same Gotify, with a token it will reject.
fn bad_credential() -> Config {
    Config {
        client_token: format!("{}-wrong", env("GOTIFY_CLIENT_TOKEN")),
        ..config()
    }
}

/// The application the client token's user owns.
fn fixture_ok() -> i64 {
    env("GOTIFY_FIXTURE_OK").parse().expect("GOTIFY_FIXTURE_OK is an integer")
}

/// The application a second user owns — invisible to this token.
fn fixture_fail() -> i64 {
    env("GOTIFY_FIXTURE_FAIL").parse().expect("GOTIFY_FIXTURE_FAIL is an integer")
}

fn env(key: &str) -> String {
    std::env::var(key)
        .unwrap_or_else(|_| panic!("{key} is unset — run specification §5.1 and §5.2 first"))
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
/// unchanged, so a stamp may be checked against it with `starts_with`.
fn promise_id(what: &str) -> String {
    format!("gotify.{what}.{}", nanos())
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

/// `errorCode` of a Gotify error body — how a halt reason identifies itself.
fn error_code(reason: &str) -> i64 {
    serde_json::from_str::<Value>(reason)
        .unwrap_or(Value::Null)["errorCode"]
        .as_i64()
        .unwrap_or_default()
}

fn halted(verdict: Verdict) -> String {
    match verdict {
        Err(Ok(reason)) => reason,
        other => panic!("expected halt, got {other:?}"),
    }
}

// ─── Provisioning (raw provider calls, not the plugin's code path) ────────────

fn http() -> reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new).clone()
}

/// One notification, sent outside the plugin so a read operation's test does
/// not depend on `message.create`.
async fn send_message(appid: i64, message: &str) {
    let status = http()
        .post(format!("{}/message", env("GOTIFY_BASE_URL")))
        .header("X-Gotify-Key", env("GOTIFY_CLIENT_TOKEN"))
        .json(&json!({"appid": appid, "message": message}))
        .send()
        .await
        .expect("send message request")
        .status();
    assert!(status.is_success(), "sending to {appid}: {status}");
}

/// The messages of one application that carry a stamp derived from this
/// promise id — the recovery window §4.1.5 scans, read directly.
async fn stamped_messages(appid: i64, promise_id: &str) -> Vec<Value> {
    let body: Value = http()
        .get(format!(
            "{}/application/{appid}/message?limit=200",
            env("GOTIFY_BASE_URL")
        ))
        .header("X-Gotify-Key", env("GOTIFY_CLIENT_TOKEN"))
        .send()
        .await
        .expect("list application messages request")
        .json()
        .await
        .expect("list application messages response is JSON");
    body["messages"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|m| {
            m["extras"][STAMP]
                .as_str()
                .is_some_and(|s| s.starts_with(promise_id))
        })
        .collect()
}
