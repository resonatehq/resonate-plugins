//! `plugin::process` against a real Grafana — never a mock.
//!
//! Bring the provider up with specification §5.1/§5.2 first; the exported
//! `GRAFANA_BASE_URL`, `GRAFANA_TOKEN`, `GRAFANA_FIXTURE_OK` and
//! `GRAFANA_FIXTURE_FAIL` are what [`config`] and the fixtures below read.
//! Every documented condition is induced with real inputs against that
//! instance — a refused query is really refused by the queried system, a
//! missing UID is really missing, a corrupted token is really rejected.
//!
//! §5 provisions two data sources: `FixtureOk`, a TestData data source that
//! answers any query with generated frames, and `FixtureFail`, a Prometheus
//! data source pointed at a stub HTTP server inside the container that
//! answers every request `400 bad_data` — the query the queried system
//! itself refuses. One induction needs a third, a Prometheus data source
//! pointed at a closed port, and provisions it here with raw provider calls.
//!
//! Two rows of the condition table have no Grafana instance: every operation
//! is `request_response` — one request, one answer — so there is no
//! pending → terminal path and no poll loop for a deadline to cut short.
//! `process` never consults `promise.timeout_at`.

use std::collections::HashMap;

use base64::Engine;
use serde_json::{json, Value};

use resonate_core::types::{PromiseRecord, PromiseState, PromiseValue};
use resonate_plugin_grafana::plugin::{self, Config};

/// The verdict `process` returns, spelled out: the crate's own alias for it
/// is an implementation detail.
type Verdict = Result<Result<String, String>, Result<String, String>>;

/// A Prometheus data source pointed at a port nothing listens on, so its
/// queries fail with a status that is a fact about the moment (502, never
/// reached) rather than about the query. Provisioned by [`dead_datasource`].
const DEAD_UID: &str = "fixture-dead";

// ─── query.run (§4.1) ─────────────────────────────────────────────────────────

/// resolved — the succeeding work item: a query the data source answers.
/// Resolved is `= response.body`, so the frames come back whole.
#[tokio::test]
async fn query_run_resolves_when_every_query_returns() {
    let p = promise(
        &promise_id("query-ok"),
        "query.run",
        json!({
            "queries": [{
                "refId": "A",
                "datasource": {"uid": fixture_ok(), "type": "grafana-testdata-datasource"},
                "scenarioId": "random_walk",
                "maxDataPoints": 3,
                "intervalMs": 1000,
            }],
            "from": "now-5m",
            "to": "now",
        }),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    let entry = &value["results"]["A"];
    assert_eq!(entry["status"], 200, "{value}");
    // Present only when the query failed — this one did not.
    assert!(entry.get("error").is_none(), "{value}");
    let frames = entry["frames"].as_array().expect("frames");
    assert!(!frames.is_empty(), "{value}");
    assert_eq!(frames[0]["schema"]["refId"], "A");
    assert!(frames[0]["data"]["values"].is_array(), "{value}");
}

/// `query_failed` — the failing work item: the queried system refuses the
/// query with a 4xx of its own. `detail` is `= response.body.results`, so it
/// carries the per-refId `error`, `errorSource` and `status`.
#[tokio::test]
async fn query_run_rejects_query_failed() {
    let p = promise(
        &promise_id("query-failed"),
        "query.run",
        json!({
            "queries": [{
                "refId": "A",
                "datasource": {"uid": fixture_fail(), "type": "prometheus"},
                "expr": "up",
            }],
            "from": "now-5m",
            "to": "now",
        }),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "query_failed");
    let entry = &value["detail"]["A"];
    assert_eq!(entry["status"], 400, "{value}");
    assert_eq!(entry["errorSource"], "downstream", "{value}");
    assert!(
        entry["error"].as_str().unwrap_or_default().contains("this query is always refused"),
        "{value}"
    );
    // detail is the whole results object: one entry per query.
    assert_eq!(value["detail"].as_object().unwrap().len(), 1, "{value}");
}

/// `not_found` — a data source UID no data source has. `detail` is the 404
/// error envelope.
#[tokio::test]
async fn query_run_rejects_not_found() {
    let p = promise(
        &promise_id("query-404"),
        "query.run",
        json!({
            "queries": [{
                "refId": "A",
                "datasource": {"uid": "no-such-datasource-e0f1", "type": "prometheus"},
                "expr": "up",
            }],
            "from": "now-5m",
            "to": "now",
        }),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "not_found");
    assert_eq!(value["detail"], json!({"message": "Data source not found"}));
}

/// `invalid_request` — the Param schema documents `queries` as `minItems: 1`.
/// A 400 with no `results` is the malformed-request half of that status, so
/// `detail` is the error envelope rather than the results.
#[tokio::test]
async fn query_run_rejects_invalid_request() {
    let p = promise(
        &promise_id("query-bad"),
        "query.run",
        json!({"queries": [], "from": "now-5m", "to": "now"}),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert_eq!(value["detail"]["messageId"], "query.noQueries", "{value}");
    assert_eq!(value["detail"]["statusCode"], 400, "{value}");
    assert!(value["detail"].get("results").is_none(), "{value}");
}

/// release — a query failed, but with a status that is not a fact about the
/// query: 502, the data source was never reached. Not a verdict; the frame
/// drops the task and the message is redelivered.
#[tokio::test]
async fn query_run_releases_when_a_failed_query_carries_no_4xx_of_its_own() {
    dead_datasource().await;
    let p = promise(
        &promise_id("query-release"),
        "query.run",
        json!({
            "queries": [{
                "refId": "A",
                "datasource": {"uid": DEAD_UID, "type": "prometheus"},
                "expr": "up",
            }],
            "from": "now-5m",
            "to": "now",
        }),
    );

    let reason = released(plugin::process(&config(), &p).await);

    // The reason is the response text: the results the classification read.
    let body: Value = serde_json::from_str(&reason).expect("reason is the response body");
    assert_eq!(body["results"]["A"]["status"], 502, "{reason}");
    assert!(body["results"]["A"]["error"].is_string(), "{reason}");
}

/// halt — the token is rejected, and no retry of ours can fix that.
#[tokio::test]
async fn query_run_halts_on_a_rejected_credential() {
    let p = promise(
        &promise_id("query-halt"),
        "query.run",
        json!({
            "queries": [{"refId": "A", "datasource": {"uid": fixture_ok()}}],
            "from": "now-5m",
            "to": "now",
        }),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("Invalid API key"), "{reason}");
}

/// Re-entry — a redelivery repeats the read. Nothing is created and nothing
/// is injected (§ Idempotency), so the second call answers like the first.
#[tokio::test]
async fn query_run_repeats_the_read_on_re_entry() {
    let p = promise(
        &promise_id("query-reentry"),
        "query.run",
        json!({
            "queries": [{
                "refId": "A",
                "datasource": {"uid": fixture_ok(), "type": "grafana-testdata-datasource"},
                "scenarioId": "random_walk",
            }],
            "from": "now-5m",
            "to": "now",
        }),
    );

    let first = resolved(plugin::process(&config(), &p).await);
    let second = resolved(plugin::process(&config(), &p).await);

    assert_eq!(first["results"]["A"]["status"], 200, "{first}");
    assert_eq!(second["results"]["A"]["status"], 200, "{second}");
}

// ─── datasource.list (§4.2) ───────────────────────────────────────────────────
//
// `invalid_request` is the operation's only rejection and it has no
// induction: the request carries no input at all — no path segment, no query
// parameter, no body — so there is nothing a caller can malform, and the two
// statuses Grafana answers `GET /api/datasources` with besides 200 are 401
// and 403, which `_check` takes to halt before the branch is reached.

/// resolved — `= response.body`: the array of the token's organization.
#[tokio::test]
async fn datasource_list_resolves_with_the_data_sources() {
    let p = promise(&promise_id("ds-list"), "datasource.list", json!({}));

    let value = resolved(plugin::process(&config(), &p).await);

    let items = value.as_array().expect("an array");
    let ok = items
        .iter()
        .find(|d| d["uid"] == json!(fixture_ok()))
        .unwrap_or_else(|| panic!("{} is missing: {value}", fixture_ok()));
    assert_eq!(ok["type"], "grafana-testdata-datasource", "{ok}");
    assert_eq!(ok["name"], "FixtureOk", "{ok}");
    assert_eq!(ok["access"], "proxy", "{ok}");
    assert_eq!(ok["isDefault"], true, "{ok}");
    assert!(ok["id"].is_number(), "{ok}");
    let fail = items
        .iter()
        .find(|d| d["uid"] == json!(fixture_fail()))
        .unwrap_or_else(|| panic!("{} is missing: {value}", fixture_fail()));
    assert_eq!(fail["type"], "prometheus", "{fail}");
}

/// halt — the token is rejected.
#[tokio::test]
async fn datasource_list_halts_on_a_rejected_credential() {
    let p = promise(&promise_id("ds-list-halt"), "datasource.list", json!({}));

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("Invalid API key"), "{reason}");
}

/// Re-entry — the read repeats.
#[tokio::test]
async fn datasource_list_repeats_the_read_on_re_entry() {
    let p = promise(&promise_id("ds-list-reentry"), "datasource.list", json!({}));

    let first = resolved(plugin::process(&config(), &p).await);
    let second = resolved(plugin::process(&config(), &p).await);

    assert_eq!(first, second);
}

// ─── datasource.get (§4.3) ────────────────────────────────────────────────────

/// resolved — `= response.body`, including the `type` and `jsonData` a caller
/// needs to compose a `query.run` query.
#[tokio::test]
async fn datasource_get_resolves_with_the_data_source() {
    let p = promise(
        &promise_id("ds-get"),
        "datasource.get",
        json!({"uid": fixture_ok()}),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["uid"], json!(fixture_ok()));
    assert_eq!(value["type"], "grafana-testdata-datasource");
    assert_eq!(value["name"], "FixtureOk");
    assert_eq!(value["jsonData"], json!({}));
    // Single-record keys the list does not carry.
    assert!(value["secureJsonFields"].is_object(), "{value}");
    assert!(value["version"].is_number(), "{value}");
}

/// `not_found` — a UID no data source has. `detail` is the 404 envelope.
#[tokio::test]
async fn datasource_get_rejects_not_found() {
    let p = promise(
        &promise_id("ds-get-404"),
        "datasource.get",
        json!({"uid": "no-such-datasource-e0f1"}),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(
        value,
        json!({"code": "not_found", "detail": {"message": "Data source not found"}})
    );
}

/// `invalid_request` — `uid` is required by the Param schema. Rejected here,
/// without a request: an immutable param that fails the schema now fails it
/// on every redelivery.
#[tokio::test]
async fn datasource_get_rejects_invalid_request() {
    let p = promise(&promise_id("ds-get-bad"), "datasource.get", json!({}));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("uid"),
        "{value}"
    );
}

/// halt — the token is rejected.
#[tokio::test]
async fn datasource_get_halts_on_a_rejected_credential() {
    let p = promise(
        &promise_id("ds-get-halt"),
        "datasource.get",
        json!({"uid": fixture_ok()}),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("Invalid API key"), "{reason}");
}

/// Re-entry — the read repeats.
#[tokio::test]
async fn datasource_get_repeats_the_read_on_re_entry() {
    let p = promise(
        &promise_id("ds-get-reentry"),
        "datasource.get",
        json!({"uid": fixture_ok()}),
    );

    let first = resolved(plugin::process(&config(), &p).await);
    let second = resolved(plugin::process(&config(), &p).await);

    assert_eq!(first, second);
}

// ─── Dispatch ─────────────────────────────────────────────────────────────────

/// A func no operation serves is a permanent rejection naming it.
#[tokio::test]
async fn an_unknown_func_is_rejected() {
    let p = promise(&promise_id("unknown"), "query.explode", json!({}));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "unknown_func", "detail": "query.explode"}));
}

// ─── Fixtures ─────────────────────────────────────────────────────────────────

/// §2, built from the environment §5.2 exports. There is no `poll` to
/// override: no operation polls.
fn config() -> Config {
    Config {
        base_url: env("GRAFANA_BASE_URL"),
        token: env("GRAFANA_TOKEN"),
    }
}

/// The same Grafana, with a token it will reject.
fn bad_credential() -> Config {
    Config {
        token: format!("{}-wrong", env("GRAFANA_TOKEN")),
        ..config()
    }
}

fn fixture_ok() -> String {
    env("GRAFANA_FIXTURE_OK")
}

fn fixture_fail() -> String {
    env("GRAFANA_FIXTURE_FAIL")
}

fn env(key: &str) -> String {
    std::env::var(key)
        .unwrap_or_else(|_| panic!("{key} is unset — run specification §5.1 and §5.2 first"))
}

/// Every operation is `request_response`, so `timeout_at` is never read; a
/// minute from now is simply a promise that is not already expired.
fn promise(id: &str, func: &str, args: Value) -> PromiseRecord {
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
        timeout_at: now_ms() + 60_000,
        created_at: now_ms(),
        settled_at: None,
    }
}

/// A fresh promise id per test.
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

/// The third data source §5 does not provision: a Prometheus data source at a
/// closed port, so a query to it fails without ever reaching a system that
/// could refuse it. Created once and left in place — 409 is Grafana saying it
/// already exists.
async fn dead_datasource() {
    let status = http()
        .post(format!("{}/api/datasources", env("GRAFANA_BASE_URL")))
        .header("Authorization", format!("Bearer {}", env("GRAFANA_TOKEN")))
        .json(&json!({
            "name": "FixtureDead",
            "uid": DEAD_UID,
            "type": "prometheus",
            "access": "proxy",
            "url": "http://127.0.0.1:1",
        }))
        .send()
        .await
        .expect("create data source request")
        .status()
        .as_u16();
    assert!(
        status == 200 || status == 409,
        "provisioning {DEAD_UID}: {status}"
    );
}
