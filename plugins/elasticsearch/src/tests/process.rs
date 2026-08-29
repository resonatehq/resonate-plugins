//! `plugin::process` against a real Elasticsearch — never a mock.
//!
//! Bring the provider up with specification §5.1/§5.2 first; the exported
//! `ELASTICSEARCH_BASE_URL`, `ELASTICSEARCH_API_KEY`,
//! `ELASTICSEARCH_FIXTURE_OK` and `ELASTICSEARCH_FIXTURE_FAIL` are what
//! [`config`] and the provisioning helpers read. Every documented condition
//! below is induced with real inputs against that instance — the failing
//! fixture really fails, a missing id is really missing, a corrupted API key
//! is really rejected.
//!
//! §5 provisions the two work items: `resonate_fixture_ok`, 200 documents a
//! reindex copies cleanly, and `resonate_fixture_fail`, an index with
//! `_source` disabled, from which a reindex cannot read a document and whose
//! task therefore completes carrying an `error`.
//!
//! A reindex of 200 documents finishes in milliseconds, which leaves no
//! window to observe a running task in. Where a test needs one — the
//! pending → terminal path, cancellation, re-entry — it throttles the copy
//! with `requests_per_second` and a small `source.size`, so the copy runs for
//! about ten seconds and the task is really there to be found.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{json, Value};

use resonate_core::types::{PromiseRecord, PromiseState, PromiseValue};
use resonate_plugin_elasticsearch::plugin::{self, Config};

/// The verdict `process` returns, spelled out: the crate's own alias for it
/// is an implementation detail.
type Verdict = Result<Result<String, String>, Result<String, String>>;

/// Documents §5 puts in the succeeding fixture.
const FIXTURE_DOCS: u64 = 200;

// ─── reindex.create (§4.1) ────────────────────────────────────────────────────

/// resolved — the succeeding work item, copied whole.
#[tokio::test]
async fn reindex_create_resolves_when_the_copy_succeeds() {
    let dest = dest_index("create-ok");
    let p = promise(
        &promise_id("create-ok"),
        "reindex.create",
        json!({"source": {"index": fixture_ok()}, "dest": {"index": dest}}),
        in_ms(300_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    // The 4.1.2 Resolved mapping: the six keys of the completed task record.
    assert!(value["id"].as_str().unwrap_or_default().contains(':'), "{value}");
    assert_eq!(
        value["description"],
        format!("reindex from [{}] to [{dest}]", fixture_ok())
    );
    assert!(value["start_time_in_millis"].is_number(), "{value}");
    assert!(value["running_time_in_nanos"].is_number(), "{value}");
    assert_eq!(value["cancelled"], false);
    assert_eq!(value["response"]["total"], FIXTURE_DOCS);
    assert_eq!(value["response"]["created"], FIXTURE_DOCS);
    assert_eq!(value["response"]["failures"], json!([]));
    assert_eq!(value.as_object().unwrap().len(), 6, "{value}");
    assert_eq!(count(&dest).await, FIXTURE_DOCS);
}

/// pending → terminal — a throttled copy is still running when the first
/// reading of the task record arrives, so the resolved value is proof the
/// poll loop watched it to completion rather than reporting the submission.
#[tokio::test]
async fn reindex_create_polls_a_running_copy_to_its_terminal_state() {
    let dest = dest_index("create-poll");
    let p = promise(
        &promise_id("create-poll"),
        "reindex.create",
        throttled(&dest),
        in_ms(300_000),
    );

    let started = Instant::now();
    let value = resolved(plugin::process(&config(), &p).await);
    let elapsed = started.elapsed();

    assert!(
        elapsed >= Duration::from_secs(3),
        "the copy finished in {elapsed:?} — it was not observed while running"
    );
    // Throttling is what made it run that long, and it is reported by the
    // terminal record the loop finally read.
    assert!(
        value["response"]["throttled_millis"].as_f64().unwrap_or(0.0) > 0.0,
        "{value}"
    );
    assert_eq!(value["response"]["total"], FIXTURE_DOCS);
    assert_eq!(value["response"]["created"], FIXTURE_DOCS);
}

/// `reindex_failed` — the failing work item: every document of
/// `resonate_fixture_fail` is unreadable, so the task completes carrying an
/// `error` and no `response`.
#[tokio::test]
async fn reindex_create_rejects_reindex_failed_on_a_task_error() {
    let dest = dest_index("create-fail");
    let p = promise(
        &promise_id("create-fail"),
        "reindex.create",
        json!({"source": {"index": fixture_fail()}, "dest": {"index": dest}}),
        in_ms(300_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "reindex_failed");
    // detail = the terminal task error.
    assert_eq!(value["detail"]["type"], "illegal_argument_exception");
    assert!(
        value["detail"]["reason"]
            .as_str()
            .unwrap_or_default()
            .contains("didn't store _source"),
        "{value}"
    );
}

/// `reindex_failed` — the other shape: the task ran to its end but the
/// destination rejected every document. `conflicts` defaults to "abort", so
/// the version conflicts land in `response.failures`.
#[tokio::test]
async fn reindex_create_rejects_reindex_failed_on_failures() {
    let dest = dest_index("create-conflict");
    seed(&dest).await;
    let p = promise(
        &promise_id("create-conflict"),
        "reindex.create",
        json!({
            "source": {"index": fixture_ok()},
            "dest": {"index": dest, "op_type": "create"},
            "conflicts": "abort",
        }),
        in_ms(300_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "reindex_failed");
    // detail = response.failures.
    let failures = value["detail"].as_array().expect("failures is an array");
    assert!(!failures.is_empty(), "{value}");
    assert_eq!(failures[0]["cause"]["type"], "version_conflict_engine_exception");
    assert_eq!(failures[0]["status"], 409);
    assert_eq!(failures[0]["index"], dest);
}

/// `invalid_request` — an unknown field nested inside `source`, which the
/// Param schema forbids (`additionalProperties: false`) and Elasticsearch
/// refuses with a 400 before any task exists.
#[tokio::test]
async fn reindex_create_rejects_invalid_request() {
    let dest = dest_index("create-bad");
    let p = promise(
        &promise_id("create-bad"),
        "reindex.create",
        json!({
            "source": {"index": fixture_ok(), "no_such_field": 1},
            "dest": {"index": dest},
        }),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    // detail = response.body.error of the 400.
    assert_eq!(value["detail"]["type"], "x_content_parse_exception");
    assert!(
        value["detail"]["caused_by"]["reason"]
            .as_str()
            .unwrap_or_default()
            .contains("no_such_field"),
        "{value}"
    );
}

/// `cancelled` — the copy is cancelled through the task management API while
/// the poll loop is watching it.
#[tokio::test]
async fn reindex_create_rejects_cancelled() {
    let dest = dest_index("create-cancel");
    let id = promise_id("create-cancel");
    let p = promise(&id, "reindex.create", throttled(&dest), in_ms(300_000));

    // Throttling keeps the copy running long enough to be cancelled.
    let canceller = async {
        let task_id = await_task(&id).await;
        assert_eq!(cancel_task(&task_id).await, 200, "cancelling {task_id}");
    };
    let config = config();
    let (verdict, ()) = tokio::join!(plugin::process(&config, &p), canceller);
    let value = rejected(verdict);

    assert_eq!(value["code"], "cancelled");
    // detail = response.canceled.
    assert_eq!(value["detail"], "by user request");
}

/// halt — the API key is rejected, and no retry of ours can fix that.
#[tokio::test]
async fn reindex_create_halts_on_a_rejected_api_key() {
    let dest = dest_index("create-halt");
    let p = promise(
        &promise_id("create-halt"),
        "reindex.create",
        json!({"source": {"index": fixture_ok()}, "dest": {"index": dest}}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("security_exception"), "{reason}");
}

/// The deadline — `timeout_at` already in the past. No verdict: the server
/// settles a timed-out promise itself.
#[tokio::test]
async fn reindex_create_releases_at_the_deadline() {
    let dest = dest_index("create-deadline");
    let p = promise(
        &promise_id("create-deadline"),
        "reindex.create",
        json!({
            "source": {"index": fixture_ok()},
            "dest": {"index": dest},
            "max_docs": 1,
        }),
        in_ms(-1_000),
    );

    let reason = released(plugin::process(&config(), &p).await);

    assert_eq!(reason, "promise timed out");
}

/// Re-entry — a redelivery that arrives while the copy is still running finds
/// it in the task listing by its `X-Opaque-Id` and re-attaches, rather than
/// starting a second copy.
#[tokio::test]
async fn reindex_create_reattaches_to_a_running_copy_on_re_entry() {
    let dest = dest_index("create-reentry");
    let id = promise_id("create-reentry");
    let p = promise(&id, "reindex.create", throttled(&dest), in_ms(300_000));

    let config = config();
    let redelivery = async {
        // Only once the first attempt's task exists is a redelivery able to
        // recognise it; that recognition is what is under test.
        await_task(&id).await;
        plugin::process(&config, &p).await
    };
    let (first, second) = tokio::join!(plugin::process(&config, &p), redelivery);

    let first = resolved(first);
    let second = resolved(second);
    assert_eq!(first["id"], second["id"], "two copies were started");
    assert_eq!(first["start_time_in_millis"], second["start_time_in_millis"]);
    assert_eq!(count(&dest).await, FIXTURE_DOCS);
}

// ─── reindex.get (§4.2) ───────────────────────────────────────────────────────

/// resolved — = response.body, the whole task record.
#[tokio::test]
async fn reindex_get_resolves_with_the_task_record() {
    let task_id = finished_reindex(&dest_index("get-run")).await;
    let p = promise(
        &promise_id("reindex-get"),
        "reindex.get",
        json!({"task_id": task_id}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["id"], task_id);
    assert_eq!(value["completed"], true);
    assert_eq!(value["cancelled"], false);
    assert_eq!(value["response"]["total"], FIXTURE_DOCS);
    assert!(value["start_time_in_millis"].is_number(), "{value}");
    assert!(value["running_time_in_nanos"].is_number(), "{value}");
}

/// `not_found` — a well-formed id no task carries.
#[tokio::test]
async fn reindex_get_rejects_not_found() {
    let p = promise(
        &promise_id("reindex-get-404"),
        "reindex.get",
        json!({"task_id": "no5uchN0d3IdE0f1AAAAAA:999999"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    // detail: absent.
    assert_eq!(value, json!({"code": "not_found"}));
}

/// `invalid_request` — the Param schema documents the id as
/// `<node id>:<task number>`.
#[tokio::test]
async fn reindex_get_rejects_invalid_request() {
    let p = promise(
        &promise_id("reindex-get-bad"),
        "reindex.get",
        json!({"task_id": "not-a-task-id"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    // detail = response.body.error of the 400.
    assert_eq!(value["detail"]["type"], "illegal_argument_exception");
    assert!(
        value["detail"]["reason"]
            .as_str()
            .unwrap_or_default()
            .contains("malformed task id"),
        "{value}"
    );
}

/// halt — the API key is rejected.
#[tokio::test]
async fn reindex_get_halts_on_a_rejected_api_key() {
    let p = promise(
        &promise_id("reindex-get-halt"),
        "reindex.get",
        json!({"task_id": "no5uchN0d3IdE0f1AAAAAA:999999"}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("security_exception"), "{reason}");
}

// ─── index.list (§4.3) ────────────────────────────────────────────────────────

/// resolved — = response.body.
#[tokio::test]
async fn index_list_resolves_with_the_resolution() {
    let p = promise(
        &promise_id("index-list"),
        "index.list",
        json!({"name": fixture_ok(), "expand_wildcards": ["open", "closed"]}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    let indices = value["indices"].as_array().expect("indices");
    assert_eq!(indices.len(), 1, "{value}");
    assert_eq!(indices[0]["name"], fixture_ok());
    assert_eq!(indices[0]["attributes"], json!(["open"]));
    assert_eq!(value["aliases"], json!([]));
    assert_eq!(value["data_streams"], json!([]));
}

/// `not_found` — a concrete, non-wildcarded target that does not exist.
#[tokio::test]
async fn index_list_rejects_not_found() {
    let p = promise(
        &promise_id("index-list-404"),
        "index.list",
        json!({"name": "no_such_index_e0f1"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "not_found");
    // detail = response.body.error of the 404.
    assert_eq!(value["detail"]["type"], "index_not_found_exception");
}

/// `invalid_request` — `expand_wildcards` is documented as an enum.
#[tokio::test]
async fn index_list_rejects_invalid_request() {
    let p = promise(
        &promise_id("index-list-bad"),
        "index.list",
        json!({"name": fixture_ok(), "expand_wildcards": "no_such_state"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert_eq!(value["detail"]["type"], "illegal_argument_exception");
}

/// halt — the API key is rejected.
#[tokio::test]
async fn index_list_halts_on_a_rejected_api_key() {
    let p = promise(
        &promise_id("index-list-halt"),
        "index.list",
        json!({"name": fixture_ok()}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("security_exception"), "{reason}");
}

// ─── index.get (§4.4) ─────────────────────────────────────────────────────────

/// resolved — = response.body, keyed by concrete index name.
#[tokio::test]
async fn index_get_resolves_with_the_index() {
    let p = promise(
        &promise_id("index-get"),
        "index.get",
        json!({"index": fixture_ok(), "flat_settings": false}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    let index = &value[fixture_ok()];
    assert_eq!(index["aliases"], json!({}));
    // The shape a reindex destination must already have.
    assert_eq!(index["mappings"]["properties"]["n"]["type"], "long");
    assert_eq!(index["settings"]["index"]["provided_name"], fixture_ok());
}

/// `not_found` — a concrete target that does not exist.
#[tokio::test]
async fn index_get_rejects_not_found() {
    let p = promise(
        &promise_id("index-get-404"),
        "index.get",
        json!({"index": "no_such_index_e0f1"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "not_found");
    // detail = response.body.error of the 404.
    assert_eq!(value["detail"]["type"], "index_not_found_exception");
}

/// `invalid_request` — `features` is documented as an enum.
#[tokio::test]
async fn index_get_rejects_invalid_request() {
    let p = promise(
        &promise_id("index-get-bad"),
        "index.get",
        json!({"index": fixture_ok(), "features": "no_such_feature"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert_eq!(value["detail"]["type"], "illegal_argument_exception");
}

/// halt — the API key is rejected.
#[tokio::test]
async fn index_get_halts_on_a_rejected_api_key() {
    let p = promise(
        &promise_id("index-get-halt"),
        "index.get",
        json!({"index": fixture_ok()}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("security_exception"), "{reason}");
}

// ─── Dispatch ─────────────────────────────────────────────────────────────────

/// A func no operation serves is a permanent rejection naming it.
#[tokio::test]
async fn an_unknown_func_is_rejected() {
    let p = promise(&promise_id("unknown"), "reindex.explode", json!({}), in_ms(60_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "unknown_func", "detail": "reindex.explode"}));
}

/// A missing required argument is decided locally: the param is immutable, so
/// no redelivery can make it satisfy the schema.
#[tokio::test]
async fn a_missing_required_argument_is_rejected() {
    let p = promise(&promise_id("no-arg"), "index.list", json!({}), in_ms(60_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("name"),
        "{value}"
    );
}

// ─── Fixtures ─────────────────────────────────────────────────────────────────

/// §2, built from the environment §5.2 exports. `poll` is overridden to 1s:
/// the default 2s would make every pending → terminal test wait an extra
/// interval past the copy's own end.
fn config() -> Config {
    Config {
        base_url: env("ELASTICSEARCH_BASE_URL"),
        api_key: env("ELASTICSEARCH_API_KEY"),
        poll: Duration::from_secs(1),
    }
}

/// The same Elasticsearch, with an API key it will reject.
fn bad_credential() -> Config {
    Config { api_key: "not-a-real-api-key".into(), ..config() }
}

fn fixture_ok() -> String {
    env("ELASTICSEARCH_FIXTURE_OK")
}

fn fixture_fail() -> String {
    env("ELASTICSEARCH_FIXTURE_FAIL")
}

fn env(key: &str) -> String {
    std::env::var(key)
        .unwrap_or_else(|_| panic!("{key} is unset — run specification §5.1 and §5.2 first"))
}

/// A copy slow enough to be observed while it runs: `requests_per_second`
/// throttles between batches, and a small `source.size` makes the batches
/// many.
fn throttled(dest: &str) -> Value {
    json!({
        "source": {"index": fixture_ok(), "size": 10},
        "dest": {"index": dest},
        "requests_per_second": 20,
    })
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
/// unchanged, so the `X-Opaque-Id` it becomes may be recognised in the task
/// listing with `starts_with`.
fn promise_id(what: &str) -> String {
    format!("elasticsearch.{what}.{}", nanos())
}

/// A fresh destination index per test. Index names are lowercase.
fn dest_index(what: &str) -> String {
    format!("resonate_dest_{}_{}", what.replace('-', "_"), nanos())
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

fn api_key() -> String {
    format!("ApiKey {}", env("ELASTICSEARCH_API_KEY"))
}

async fn json_get(path: &str) -> Value {
    http()
        .get(format!("{}{path}", env("ELASTICSEARCH_BASE_URL")))
        .header("Authorization", api_key())
        .send()
        .await
        .expect("provisioning GET")
        .json()
        .await
        .unwrap_or(Value::Null)
}

/// Documents in an index, after a refresh so the count is the copy's own
/// result rather than what happens to be searchable.
async fn count(index: &str) -> u64 {
    let _ = json_get(&format!("/{index}/_refresh")).await;
    json_get(&format!("/{index}/_count")).await["count"]
        .as_u64()
        .unwrap_or_default()
}

/// A destination already holding the fixture's documents, so a second copy
/// under `op_type: "create"` conflicts on every id.
async fn seed(dest: &str) {
    let status = http()
        .post(format!("{}/_reindex?refresh=true", env("ELASTICSEARCH_BASE_URL")))
        .header("Authorization", api_key())
        .json(&json!({"source": {"index": fixture_ok()}, "dest": {"index": dest}}))
        .send()
        .await
        .expect("seed reindex")
        .status();
    assert!(status.is_success(), "seeding {dest}: {status}");
}

/// One completed reindex to read back — provisioned outside the plugin so a
/// read operation's test does not depend on `reindex.create`.
async fn finished_reindex(dest: &str) -> String {
    let body: Value = http()
        .post(format!(
            "{}/_reindex?wait_for_completion=false",
            env("ELASTICSEARCH_BASE_URL")
        ))
        .header("Authorization", api_key())
        .json(&json!({"source": {"index": fixture_ok()}, "dest": {"index": dest}}))
        .send()
        .await
        .expect("submit reindex")
        .json()
        .await
        .expect("submit response is JSON");
    let task_id = body["task"].as_str().expect("task id").to_string();
    for _ in 0..120 {
        let run = json_get(&format!("/_reindex/{}", encode(&task_id))).await;
        if run["completed"] == json!(true) {
            return task_id;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    panic!("{task_id} did not complete");
}

/// The running reindex this promise started, found the way the plugin finds
/// it: by the `X-Opaque-Id` the frame's sanitize derives from the promise id.
async fn await_task(promise_id: &str) -> String {
    for _ in 0..120 {
        let listing = json_get("/_tasks?actions=indices%3Adata%2Fwrite%2Freindex").await;
        if let Some(nodes) = listing["nodes"].as_object() {
            for node in nodes.values() {
                let Some(tasks) = node["tasks"].as_object() else {
                    continue;
                };
                for (task_id, task) in tasks {
                    let opaque = task["headers"]["X-Opaque-Id"].as_str().unwrap_or_default();
                    if opaque.starts_with(promise_id) && task.get("parent_task_id").is_none() {
                        return task_id.clone();
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("no running reindex for {promise_id} appeared");
}

async fn cancel_task(task_id: &str) -> u16 {
    http()
        .post(format!(
            "{}/_tasks/{}/_cancel",
            env("ELASTICSEARCH_BASE_URL"),
            encode(task_id)
        ))
        .header("Authorization", api_key())
        .send()
        .await
        .expect("cancel request")
        .status()
        .as_u16()
}

/// Percent-encode a task id for one path segment: it carries a `:`.
fn encode(segment: &str) -> String {
    segment.replace(':', "%3A")
}
