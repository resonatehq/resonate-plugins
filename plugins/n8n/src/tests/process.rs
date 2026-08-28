//! `plugin::process` against a real n8n — never a mock.
//!
//! Bring the provider up with specification §5.1/§5.2 first; the exported
//! `N8N_BASE_URL`, `N8N_API_KEY`, `N8N_FIXTURE_OK` and `N8N_FIXTURE_FAIL` are
//! what [`config`] and the inductions below read. Every documented condition
//! is induced with real inputs against that instance — a failing workflow
//! really fails, a missing id is really missing, a corrupted key is really
//! rejected.
//!
//! §5 provisions two failed executions: `N8N_FIXTURE_OK`, whose retry
//! succeeds now that the gate workflow is active, and `N8N_FIXTURE_FAIL`,
//! whose workflow throws unconditionally. Both settle in well under the
//! specification's ten-second request timeout, so the retry endpoint answers
//! with the terminal record and the poll loop never runs. Three conditions —
//! recovery through `_find_retry`, the deadline, and `deleted` — need the
//! endpoint to still be holding the response open, so this file provisions a
//! third fixture of its own: `resonate-slow`, a workflow that burns twenty
//! seconds before throwing.

use std::collections::HashMap;
use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};

use resonate_core::types::{PromiseRecord, PromiseState, PromiseValue};
use resonate_plugin_n8n::plugin::{self, Config};

/// The verdict `process` returns, spelled out: the crate's own alias for it
/// is an implementation detail.
type Verdict = Result<Result<String, String>, Result<String, String>>;

// ─── execution.retry (§4.1) ───────────────────────────────────────────────────

/// resolved — the succeeding work item. The retry endpoint holds its response
/// until the execution settles, so this run's terminal record arrives in the
/// retry response itself: the Resolved mapping keeps only the keys that
/// response carries.
#[tokio::test]
async fn execution_retry_resolves_when_the_retried_execution_succeeds() {
    let p = promise(
        &promise_id("retry-ok"),
        "execution.retry",
        json!({"id": fixture_ok()}),
        in_ms(300_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["status"], "success");
    assert_eq!(value["mode"], "retry");
    assert_eq!(value["finished"], true);
    assert!(value["id"].is_string(), "{value}");
    assert!(value["workflowId"].is_string(), "{value}");
    assert!(value["startedAt"].is_string(), "{value}");
    // = response.body.retryOf, which the retry response renders as a number.
    assert_eq!(value["retryOf"].to_string(), fixture_ok());
    // The retry response omits createdAt, stoppedAt, waitTill and
    // retrySuccessId, and the Resolved mapping keeps only what is present.
    let keys: Vec<&String> = value.as_object().unwrap().keys().collect();
    assert_eq!(
        keys,
        vec!["finished", "id", "mode", "retryOf", "startedAt", "status", "workflowId"],
        "{value}"
    );
}

/// `execution_failed` — the failing work item.
#[tokio::test]
async fn execution_retry_rejects_execution_failed() {
    let p = promise(
        &promise_id("retry-fail"),
        "execution.retry",
        json!({"id": fixture_fail()}),
        in_ms(300_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "execution_failed");
    // detail = the terminal execution record.
    assert_eq!(value["detail"]["status"], "error");
    assert_eq!(value["detail"]["mode"], "retry");
    assert_eq!(value["detail"]["retryOf"].to_string(), fixture_fail());
}

/// `not_found` — a nonexistent execution id.
#[tokio::test]
async fn execution_retry_rejects_not_found() {
    let p = promise(
        &promise_id("retry-404"),
        "execution.retry",
        json!({"id": 999_999_999_i64}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "not_found");
    // detail = response.body.message of the 404.
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("999999999"),
        "{value}"
    );
}

/// `conflict` — the source execution already succeeded, so there is nothing
/// to resume.
#[tokio::test]
async fn execution_retry_rejects_conflict() {
    let succeeded = succeeded_execution().await;
    let p = promise(
        &promise_id("retry-409"),
        "execution.retry",
        json!({"id": succeeded}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "conflict");
    // detail = response.body.message of the 409.
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("succeeded"),
        "{value}"
    );
}

/// `invalid_request` — the Param schema documents `id` as numeric, and n8n's
/// request validator rejects anything else.
#[tokio::test]
async fn execution_retry_rejects_invalid_request() {
    let p = promise(
        &promise_id("retry-400"),
        "execution.retry",
        json!({"id": "not-a-number"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    // detail = response.body.message of the 400.
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("must be number"),
        "{value}"
    );
}

/// Recovery, and pending → terminal. `resonate-slow` runs for twenty seconds,
/// longer than the specification's ten-second request timeout, so the retry
/// POST never answers: the execution it started has to be found by its
/// `retryOf`, then polled from `running` to its terminal state.
#[tokio::test]
async fn execution_retry_recovers_the_execution_the_blocking_post_started() {
    let source = slow_failed_execution().await;
    let p = promise(
        &promise_id("retry-recover"),
        "execution.retry",
        json!({"id": source}),
        in_ms(300_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "execution_failed");
    // The record came from a read, not from the retry response: a read
    // renders retryOf as a string and carries the keys the retry response
    // omits.
    assert_eq!(value["detail"]["retryOf"], source);
    assert_eq!(value["detail"]["status"], "error");
    assert!(value["detail"]["stoppedAt"].is_string(), "{value}");
}

/// `deleted` — the retried execution's record is removed after it reaches a
/// terminal state and before the poll loop reads it back. n8n refuses to
/// delete a running execution, so the window only exists once the run has
/// ended: `poll` is widened to hold the loop asleep across it.
#[tokio::test]
async fn execution_retry_rejects_deleted() {
    let source = slow_failed_execution().await;
    let p = promise(
        &promise_id("retry-deleted"),
        "execution.retry",
        json!({"id": source}),
        in_ms(300_000),
    );
    let config = Config { poll: Duration::from_secs(30), ..config() };

    let deleter = async {
        let id = await_terminal_retry_of(&source).await;
        assert_eq!(delete_execution(&id).await, 200, "deleting {id}");
    };
    let (verdict, ()) = tokio::join!(plugin::process(&config, &p), deleter);
    let value = rejected(verdict);

    assert_eq!(value["code"], "deleted");
    // detail: absent.
    assert_eq!(value, json!({"code": "deleted"}));
}

/// The deadline — `timeout_at` already in the past. No verdict: the server
/// settles a timed-out promise itself. Induced on `resonate-slow`, because a
/// retry whose terminal record arrives in the POST response never reaches the
/// poll loop that observes the deadline.
#[tokio::test]
async fn execution_retry_releases_at_the_deadline() {
    let source = slow_failed_execution().await;
    let p = promise(
        &promise_id("retry-deadline"),
        "execution.retry",
        json!({"id": source}),
        in_ms(-1_000),
    );

    let reason = released(plugin::process(&config(), &p).await);

    assert_eq!(reason, "promise timed out");
}

/// halt — the API key is rejected, and no retry of ours can fix that.
#[tokio::test]
async fn execution_retry_halts_on_a_rejected_api_key() {
    let p = promise(
        &promise_id("retry-halt"),
        "execution.retry",
        json!({"id": fixture_ok()}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("unauthorized"), "{reason}");
}

/// Re-entry. The retry endpoint accepts no client-supplied identity and
/// stamps none (§ Idempotency), so a re-delivery cannot re-attach: it starts
/// a second retry of the same source, costing one more run of the workflow.
/// Both attempts still reach a verdict on their own execution.
#[tokio::test]
async fn execution_retry_starts_a_second_run_on_re_entry() {
    let id = promise_id("retry-reentry");
    let p = promise(&id, "execution.retry", json!({"id": fixture_ok()}), in_ms(300_000));

    let first = resolved(plugin::process(&config(), &p).await);
    let second = resolved(plugin::process(&config(), &p).await);

    assert_eq!(first["status"], "success");
    assert_eq!(second["status"], "success");
    assert_ne!(first["id"], second["id"], "a re-delivery ran the workflow again");
}

// ─── execution.get (§4.2) ─────────────────────────────────────────────────────

/// resolved — = response.body. `includeData` is sent as the wire form n8n's
/// validator accepts, so the node run data comes back with the record.
#[tokio::test]
async fn execution_get_resolves_with_the_execution() {
    let p = promise(
        &promise_id("get-ok"),
        "execution.get",
        json!({"id": fixture_ok(), "includeData": true}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["id"], fixture_ok());
    assert_eq!(value["status"], "error");
    assert_eq!(value["retryOf"], Value::Null);
    assert!(value["workflowId"].is_string(), "{value}");
    // includeData=true: the record carries its node run data and workflow.
    assert!(value["data"].is_object(), "{value}");
    assert!(value["workflowData"].is_object(), "{value}");
}

/// `not_found` — a nonexistent execution id.
#[tokio::test]
async fn execution_get_rejects_not_found() {
    let p = promise(
        &promise_id("get-404"),
        "execution.get",
        json!({"id": 999_999_999_i64}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    // detail: absent.
    assert_eq!(value, json!({"code": "not_found"}));
}

/// `invalid_request` — the Param schema documents `id` as numeric.
#[tokio::test]
async fn execution_get_rejects_invalid_request() {
    let p = promise(
        &promise_id("get-400"),
        "execution.get",
        json!({"id": "not-a-number"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("must be number"),
        "{value}"
    );
}

/// halt — the API key is rejected.
#[tokio::test]
async fn execution_get_halts_on_a_rejected_api_key() {
    let p = promise(
        &promise_id("get-halt"),
        "execution.get",
        json!({"id": fixture_ok()}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("unauthorized"), "{reason}");
}

// ─── execution.list (§4.3) ────────────────────────────────────────────────────

/// resolved — = response.body. The filter is passed straight through.
#[tokio::test]
async fn execution_list_resolves_with_a_page_of_executions() {
    let p = promise(
        &promise_id("list-ok"),
        "execution.list",
        json!({"status": "error", "limit": 2}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    let rows = value["data"].as_array().expect("data");
    assert!(rows.len() <= 2, "limit was not applied: {value}");
    assert!(!rows.is_empty(), "§5 provisions two failed executions: {value}");
    for row in rows {
        assert_eq!(row["status"], "error", "{value}");
    }
    assert!(value.get("nextCursor").is_some(), "{value}");
}

/// `invalid_request` — `status` is an enum the request validator enforces.
#[tokio::test]
async fn execution_list_rejects_invalid_request() {
    let p = promise(
        &promise_id("list-400"),
        "execution.list",
        json!({"status": "not-a-status"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("must be equal to one of the allowed values"),
        "{value}"
    );
}

/// halt — the API key is rejected.
#[tokio::test]
async fn execution_list_halts_on_a_rejected_api_key() {
    let p = promise(&promise_id("list-halt"), "execution.list", json!({}), in_ms(60_000));

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("unauthorized"), "{reason}");
}

// ─── Dispatch ─────────────────────────────────────────────────────────────────

/// A func no operation serves is a permanent rejection naming it.
#[tokio::test]
async fn an_unknown_func_is_rejected() {
    let p = promise(&promise_id("unknown"), "execution.explode", json!({}), in_ms(60_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "unknown_func", "detail": "execution.explode"}));
}

/// A missing required arg is rejected locally, without a request: a promise's
/// param is immutable, so it cannot become valid on a redelivery.
#[tokio::test]
async fn a_missing_required_arg_is_rejected() {
    let p = promise(&promise_id("no-id"), "execution.get", json!({}), in_ms(60_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(value["detail"].as_str().unwrap_or_default().contains("args.id"), "{value}");
}

// ─── Fixtures ─────────────────────────────────────────────────────────────────

/// §2, built from the environment §5.2 exports. `poll` is overridden to 1s:
/// the default 2s would make every pending → terminal test wait an interval
/// past the execution's own end.
fn config() -> Config {
    Config {
        base_url: env("N8N_BASE_URL"),
        api_key: env("N8N_API_KEY"),
        poll: Duration::from_secs(1),
    }
}

/// The same n8n, with a key it will answer 401 to.
fn bad_credential() -> Config {
    Config { api_key: format!("{}-wrong", env("N8N_API_KEY")), ..config() }
}

fn fixture_ok() -> String {
    env("N8N_FIXTURE_OK")
}

fn fixture_fail() -> String {
    env("N8N_FIXTURE_FAIL")
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

/// A fresh promise id per test.
fn promise_id(what: &str) -> String {
    format!("n8n.{what}.{}", nanos())
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

// ─── Provisioning (raw provider calls, not the plugin's code path) ────────────

fn http() -> reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new).clone()
}

async fn api(method: reqwest::Method, path: &str, key: &str, body: Option<Value>) -> (u16, Value) {
    let mut req = http()
        .request(method, format!("{}/api/v1{path}", env("N8N_BASE_URL")))
        .header("X-N8N-API-KEY", key);
    if let Some(body) = body {
        req = req.json(&body);
    }
    let resp = req.send().await.expect("provisioning request");
    let status = resp.status().as_u16();
    let text = resp.text().await.expect("provisioning response");
    (status, serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

async fn get(path: &str) -> (u16, Value) {
    api(reqwest::Method::GET, path, &env("N8N_API_KEY"), None).await
}

async fn post(path: &str, body: Value) -> (u16, Value) {
    api(reqwest::Method::POST, path, &env("N8N_API_KEY"), Some(body)).await
}

/// A successful execution, for the `conflict` induction: retrying one is what
/// n8n answers 409 to. Made by retrying `N8N_FIXTURE_OK`, whose retry
/// succeeds now that §5.2 has activated the gate.
async fn succeeded_execution() -> String {
    let (status, body) = post(&format!("/executions/{}/retry", fixture_ok()), json!({})).await;
    assert_eq!(status, 200, "seeding a successful execution: {body}");
    assert_eq!(body["status"], "success", "{body}");
    body["id"].as_str().expect("id").to_string()
}

/// `resonate-slow`: a webhook workflow that burns twenty seconds in a Code
/// node and then throws. Twenty seconds is longer than the specification's
/// ten-second request timeout, so a retry of one of its executions leaves the
/// POST unanswered — the only way to reach `_find_retry`, the poll loop, and
/// the window in which a terminal execution can be deleted.
async fn slow_workflow() -> String {
    static ID: tokio::sync::OnceCell<String> = tokio::sync::OnceCell::const_new();
    ID.get_or_init(|| async {
        let (_, page) = get("/workflows?limit=250").await;
        if let Some(found) = page["data"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|w| w["name"] == "resonate-slow")
        {
            return found["id"].as_str().expect("workflow id").to_string();
        }
        let nodes = json!([
            {
                "parameters": {"httpMethod": "POST", "path": "resonate-slow", "options": {}},
                "name": "Webhook",
                "type": "n8n-nodes-base.webhook",
                "typeVersion": 2,
                "position": [0, 0]
            },
            {
                "parameters": {
                    "jsCode": "const t = Date.now(); while (Date.now() - t < 20000) {} \
                               throw new Error('resonate slow fixture always fails');"
                },
                "name": "Code",
                "type": "n8n-nodes-base.code",
                "typeVersion": 2,
                "position": [220, 0]
            }
        ]);
        let (status, body) = post(
            "/workflows",
            json!({
                "name": "resonate-slow",
                "nodes": nodes,
                "connections": {"Webhook": {"main": [[{"node": "Code", "type": "main", "index": 0}]]}},
                "settings": {"executionOrder": "v1"}
            }),
        )
        .await;
        assert_eq!(status, 200, "creating resonate-slow: {body}");
        let id = body["id"].as_str().expect("workflow id").to_string();
        let (status, body) = post(&format!("/workflows/{id}/activate"), json!({})).await;
        assert_eq!(status, 200, "activating resonate-slow: {body}");
        id
    })
    .await
    .clone()
}

/// One fresh failed execution of `resonate-slow` — a retry source of its own,
/// so the `retryOf` scans below cannot see another test's retry.
async fn slow_failed_execution() -> String {
    let workflow = slow_workflow().await;
    let before = failed_slow_executions(&workflow).await;
    for _ in 0..30 {
        let fired = http()
            .post(format!("{}/webhook/resonate-slow", env("N8N_BASE_URL")))
            .json(&json!({}))
            .send()
            .await
            .expect("webhook request")
            .status()
            .is_success();
        if fired {
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    for _ in 0..90 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        if let Some(id) = failed_slow_executions(&workflow)
            .await
            .into_iter()
            .find(|id| !before.contains(id))
        {
            return id;
        }
    }
    panic!("no failed execution of resonate-slow appeared");
}

async fn failed_slow_executions(workflow: &str) -> Vec<String> {
    let (_, page) = get(&format!("/executions?workflowId={workflow}&status=error&limit=250")).await;
    page["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| e["id"].as_str())
        .map(str::to_string)
        .collect()
}

/// The retry of `source`, once it is terminal — n8n refuses to delete a
/// running execution, so the `deleted` induction has to wait for the run to
/// end. Scans the same two listings the specification's `_find_retry` does:
/// running executions are omitted from the default page.
async fn await_terminal_retry_of(source: &str) -> String {
    for _ in 0..180 {
        for query in ["?status=running&limit=250", "?limit=250"] {
            let (_, page) = get(&format!("/executions{query}")).await;
            for e in page["data"].as_array().into_iter().flatten() {
                let retry_of = match &e["retryOf"] {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    _ => continue,
                };
                let status = e["status"].as_str().unwrap_or_default();
                if retry_of == source && matches!(status, "success" | "error" | "canceled" | "crashed")
                {
                    return e["id"].as_str().expect("execution id").to_string();
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    panic!("no terminal retry of {source} appeared");
}

/// §5.2 mints one API key, and its scopes stop at `execution:retry` — nothing
/// in the plugin deletes, so nothing in §5 needs to. The `deleted` condition
/// does, so this mints a second key with `execution:delete` from the owner
/// account §5.2 creates.
async fn delete_key() -> String {
    static KEY: tokio::sync::OnceCell<String> = tokio::sync::OnceCell::const_new();
    KEY.get_or_init(|| async {
        let base = env("N8N_BASE_URL");
        let login = http()
            .post(format!("{base}/rest/login"))
            .json(&json!({
                "emailOrLdapLoginId": "resonate@example.com",
                "password": "Resonate123"
            }))
            .send()
            .await
            .expect("owner login");
        assert!(login.status().is_success(), "owner login: {}", login.status());
        let cookie = login
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .filter_map(|v| v.split(';').next())
            .collect::<Vec<_>>()
            .join("; ");
        let body: Value = http()
            .post(format!("{base}/rest/api-keys"))
            .header(reqwest::header::COOKIE, cookie)
            .json(&json!({
                "label": format!("resonate-delete-{}", nanos()),
                "expiresAt": null,
                "scopes": ["execution:delete"]
            }))
            .send()
            .await
            .expect("api key request")
            .json()
            .await
            .expect("api key response is JSON");
        body["data"]["rawApiKey"].as_str().expect("rawApiKey").to_string()
    })
    .await
    .clone()
}

async fn delete_execution(id: &str) -> u16 {
    let key = delete_key().await;
    api(reqwest::Method::DELETE, &format!("/executions/{id}"), &key, None).await.0
}
