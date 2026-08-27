//! `plugin::process` against a real Apache Airflow — never a mock.
//!
//! Bring the provider up with specification §5.1/§5.2 first; the exported
//! `AIRFLOW_BASE_URL`, `AIRFLOW_USERNAME` and `AIRFLOW_PASSWORD` are what
//! [`config`] reads. Every documented condition below is induced with real
//! inputs against that instance — a failing Dag really fails, a missing id is
//! really missing, a corrupted credential is really rejected.
//!
//! §5 provisions Airflow's example Dags (`AIRFLOW__CORE__LOAD_EXAMPLES`), and
//! Airflow creates every Dag paused: a paused Dag accepts a trigger but its
//! run never leaves "queued". So the work items are named here —
//! `example_simplest_dag` succeeds, `example_failed_dag` fails,
//! `example_xcom` publishes an XCom — and unpaused as test setup. A Dag that
//! is deliberately left paused (`STUCK_DAG`) is how a run that never finishes
//! is provisioned, for the `deleted` and deadline paths.

use std::collections::HashMap;
use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};

use resonate_core::types::{PromiseRecord, PromiseState, PromiseValue};
use resonate_plugin_airflow::plugin::{self, Config};

/// The verdict `process` returns, spelled out: the crate's own alias for it
/// is an implementation detail.
type Verdict = Result<Result<String, String>, Result<String, String>>;

/// Succeeds in seconds: one trivial task.
const OK_DAG: &str = "example_simplest_dag";
/// Fails in seconds: its only task raises.
const FAIL_DAG: &str = "example_failed_dag";
/// Publishes `return_value` from `push_by_returning`.
const XCOM_DAG: &str = "example_xcom";
/// Never unpaused by any test, so a run triggered on it stays "queued" for
/// as long as the test needs it to.
const STUCK_DAG: &str = "example_bash_operator";

// ─── dagrun.trigger (§4.1) ────────────────────────────────────────────────────

/// resolved — the succeeding work item. Also the pending → terminal path:
/// the trigger response is "queued" with a null `start_date`, so a resolved
/// value carrying `start_date`, `end_date` and `duration` is proof the poll
/// loop watched the run to its terminal state rather than reporting the
/// trigger.
#[tokio::test]
async fn dagrun_trigger_resolves_when_the_run_succeeds() {
    unpause(OK_DAG).await;
    let id = promise_id("trigger-ok");
    let p = promise(&id, "dagrun.trigger", json!({"dag_id": OK_DAG}), in_ms(300_000));

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["dag_id"], OK_DAG);
    assert_eq!(value["state"], "success");
    assert_eq!(value["run_type"], "manual");
    // = sanitize(promise.id)
    let run_id = value["dag_run_id"].as_str().expect("dag_run_id is a string");
    assert!(run_id.starts_with(&id), "{run_id} should be derived from {id}");
    assert_eq!(value["logical_date"], Value::Null);
    assert!(value["run_after"].is_string(), "{value}");
    assert!(value["start_date"].is_string(), "run never started: {value}");
    assert!(value["end_date"].is_string(), "run not observed to its end: {value}");
    assert!(value["duration"].is_number(), "{value}");
    assert_eq!(value["conf"], json!({}));
    assert_eq!(value["note"], Value::Null);
    // The Resolved schema is exactly these eleven keys.
    assert_eq!(value.as_object().unwrap().len(), 11, "{value}");
}

/// `run_failed` — the failing work item.
#[tokio::test]
async fn dagrun_trigger_rejects_run_failed() {
    unpause(FAIL_DAG).await;
    let id = promise_id("trigger-fail");
    let p = promise(&id, "dagrun.trigger", json!({"dag_id": FAIL_DAG}), in_ms(300_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "run_failed");
    // detail = the terminal Dag run object.
    assert_eq!(value["detail"]["state"], "failed");
    assert_eq!(value["detail"]["dag_id"], FAIL_DAG);
    assert!(value["detail"]["dag_run_id"].as_str().unwrap().starts_with(&id));
}

/// `dag_not_found` — a nonexistent Dag id.
#[tokio::test]
async fn dagrun_trigger_rejects_dag_not_found() {
    let id = promise_id("trigger-nodag");
    let p = promise(
        &id,
        "dagrun.trigger",
        json!({"dag_id": "no_such_dag_e0f1"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "dag_not_found");
    // detail = response.body.detail of the 404.
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("no_such_dag_e0f1"),
        "{value}"
    );
}

/// `invalid_request` — `logical_date` is documented as a date-time.
#[tokio::test]
async fn dagrun_trigger_rejects_invalid_request() {
    unpause(OK_DAG).await;
    let id = promise_id("trigger-bad");
    let p = promise(
        &id,
        "dagrun.trigger",
        json!({"dag_id": OK_DAG, "logical_date": "not-a-date"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    // detail = response.body.detail of the 422.
    assert!(!value["detail"].is_null(), "{value}");
}

/// `conflict` — a 409 raised by a unique constraint that is not our own run
/// id: an existing run already holds this `logical_date`.
#[tokio::test]
async fn dagrun_trigger_rejects_conflict() {
    unpause(OK_DAG).await;
    let id = promise_id("trigger-conflict");
    // Seeded under a run id of its own, so the 409 cannot be our run.
    let logical_date = format!("2030-01-01T00:00:00.{:06}Z", nanos() % 1_000_000);
    let seed = format!("{id}-seed");
    let status = trigger(
        OK_DAG,
        json!({"dag_run_id": seed, "logical_date": logical_date}),
    )
    .await;
    assert_eq!(status, 200, "seeding the conflicting run");

    let p = promise(
        &id,
        "dagrun.trigger",
        json!({"dag_id": OK_DAG, "logical_date": logical_date}),
        in_ms(60_000),
    );
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "conflict");
    assert!(!value["detail"].is_null(), "{value}");
}

/// `deleted` — the run is removed while the poll loop is watching it.
#[tokio::test]
async fn dagrun_trigger_rejects_deleted() {
    let id = promise_id("trigger-deleted");
    let p = promise(&id, "dagrun.trigger", json!({"dag_id": STUCK_DAG}), in_ms(120_000));

    // STUCK_DAG is paused, so the run this creates stays "queued" and the
    // poll loop keeps asking — which is what makes the deletion observable.
    let deleter = async {
        let run_id = await_run(STUCK_DAG, &id).await;
        assert_eq!(delete_run(STUCK_DAG, &run_id).await, 204);
    };
    let config = config();
    let (verdict, ()) = tokio::join!(plugin::process(&config, &p), deleter);
    let value = rejected(verdict);

    assert_eq!(value["code"], "deleted");
    // detail: absent.
    assert_eq!(value.as_object().unwrap().len(), 1, "{value}");
}

/// The deadline — `timeout_at` already in the past. No verdict: the server
/// settles a timed-out promise itself.
#[tokio::test]
async fn dagrun_trigger_releases_at_the_deadline() {
    let id = promise_id("trigger-deadline");
    let p = promise(&id, "dagrun.trigger", json!({"dag_id": STUCK_DAG}), in_ms(-1_000));

    let reason = released(plugin::process(&config(), &p).await);

    assert_eq!(reason, "promise timed out");
    // The run was created before the deadline was observed; take it back.
    let run_id = await_run(STUCK_DAG, &id).await;
    delete_run(STUCK_DAG, &run_id).await;
}

/// halt — the credential is rejected, and no retry of ours can fix that.
#[tokio::test]
async fn dagrun_trigger_halts_on_rejected_credentials() {
    let id = promise_id("trigger-halt");
    let p = promise(&id, "dagrun.trigger", json!({"dag_id": OK_DAG}), in_ms(60_000));

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("credentials rejected"), "{reason}");
}

/// Re-entry — a redelivery re-attaches to the run an earlier attempt
/// created, rather than starting a second one.
#[tokio::test]
async fn dagrun_trigger_reattaches_on_re_entry() {
    unpause(OK_DAG).await;
    let id = promise_id("trigger-reentry");
    let p = promise(&id, "dagrun.trigger", json!({"dag_id": OK_DAG}), in_ms(300_000));

    let first = resolved(plugin::process(&config(), &p).await);
    let second = resolved(plugin::process(&config(), &p).await);

    assert_eq!(first["dag_run_id"], second["dag_run_id"]);
    assert_eq!(first["start_date"], second["start_date"]);
    // One promise, one run.
    assert_eq!(runs_with_prefix(OK_DAG, &id).await.len(), 1);
}

// ─── dagrun.get (§4.2) ────────────────────────────────────────────────────────

/// resolved — = response.body.
#[tokio::test]
async fn dagrun_get_resolves_with_the_run() {
    unpause(OK_DAG).await;
    let run_id = succeeded_run(OK_DAG, &promise_id("get-run")).await;
    let p = promise(
        &promise_id("dagrun-get"),
        "dagrun.get",
        json!({"dag_id": OK_DAG, "dag_run_id": run_id}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["dag_run_id"], run_id);
    assert_eq!(value["dag_id"], OK_DAG);
    assert_eq!(value["state"], "success");
    assert_eq!(value["run_type"], "manual");
}

/// `not_found` — a nonexistent run id.
#[tokio::test]
async fn dagrun_get_rejects_not_found() {
    let p = promise(
        &promise_id("dagrun-get-404"),
        "dagrun.get",
        json!({"dag_id": OK_DAG, "dag_run_id": "no_such_run_e0f1"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "not_found"}));
}

/// `invalid_request` — `dag_run_id` is required by the Param schema.
#[tokio::test]
async fn dagrun_get_rejects_invalid_request() {
    let p = promise(
        &promise_id("dagrun-get-bad"),
        "dagrun.get",
        json!({"dag_id": OK_DAG}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("dag_run_id"),
        "{value}"
    );
}

/// halt — the credential is rejected.
#[tokio::test]
async fn dagrun_get_halts_on_rejected_credentials() {
    let p = promise(
        &promise_id("dagrun-get-halt"),
        "dagrun.get",
        json!({"dag_id": OK_DAG, "dag_run_id": "whatever"}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("credentials rejected"), "{reason}");
}

// ─── taskinstance.list (§4.3) ─────────────────────────────────────────────────

/// resolved — = response.body.
#[tokio::test]
async fn taskinstance_list_resolves_with_the_task_instances() {
    unpause(OK_DAG).await;
    let run_id = succeeded_run(OK_DAG, &promise_id("ti-run")).await;
    let p = promise(
        &promise_id("ti-list"),
        "taskinstance.list",
        json!({"dag_id": OK_DAG, "dag_run_id": run_id}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    let instances = value["task_instances"].as_array().expect("task_instances");
    assert_eq!(instances.len(), 1, "{value}");
    assert_eq!(instances[0]["task_id"], "my_task");
    assert_eq!(instances[0]["dag_run_id"], run_id);
    assert_eq!(instances[0]["state"], "success");
    assert_eq!(instances[0]["map_index"], -1);
}

/// `not_found` — a nonexistent run id.
#[tokio::test]
async fn taskinstance_list_rejects_not_found() {
    let p = promise(
        &promise_id("ti-404"),
        "taskinstance.list",
        json!({"dag_id": OK_DAG, "dag_run_id": "no_such_run_e0f1"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "not_found"}));
}

/// `invalid_request` — `limit` is documented as `minimum: 0`.
#[tokio::test]
async fn taskinstance_list_rejects_invalid_request() {
    unpause(OK_DAG).await;
    let run_id = succeeded_run(OK_DAG, &promise_id("ti-bad-run")).await;
    let p = promise(
        &promise_id("ti-bad"),
        "taskinstance.list",
        json!({"dag_id": OK_DAG, "dag_run_id": run_id, "limit": -1}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(!value["detail"].is_null(), "{value}");
}

/// halt — the credential is rejected.
#[tokio::test]
async fn taskinstance_list_halts_on_rejected_credentials() {
    let p = promise(
        &promise_id("ti-halt"),
        "taskinstance.list",
        json!({"dag_id": OK_DAG, "dag_run_id": "whatever"}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("credentials rejected"), "{reason}");
}

// ─── xcom.get (§4.4) ──────────────────────────────────────────────────────────

/// resolved — = response.body.
#[tokio::test]
async fn xcom_get_resolves_with_the_entry() {
    unpause(XCOM_DAG).await;
    let run_id = succeeded_run(XCOM_DAG, &promise_id("xcom-run")).await;
    let p = promise(
        &promise_id("xcom-get"),
        "xcom.get",
        json!({
            "dag_id": XCOM_DAG,
            "dag_run_id": run_id,
            "task_id": "push_by_returning",
            "xcom_key": "return_value",
        }),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["key"], "return_value");
    assert_eq!(value["value"], json!({"a": "b"}));
    assert_eq!(value["task_id"], "push_by_returning");
    assert_eq!(value["dag_id"], XCOM_DAG);
    assert_eq!(value["run_id"], run_id);
    assert_eq!(value["map_index"], -1);
    assert!(value["timestamp"].is_string(), "{value}");
}

/// `not_found` — a key no task pushed.
#[tokio::test]
async fn xcom_get_rejects_not_found() {
    unpause(XCOM_DAG).await;
    let run_id = succeeded_run(XCOM_DAG, &promise_id("xcom-404-run")).await;
    let p = promise(
        &promise_id("xcom-404"),
        "xcom.get",
        json!({
            "dag_id": XCOM_DAG,
            "dag_run_id": run_id,
            "task_id": "push_by_returning",
            "xcom_key": "no_such_key_e0f1",
        }),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "not_found"}));
}

/// `invalid_request` — `map_index` is documented as an integer.
#[tokio::test]
async fn xcom_get_rejects_invalid_request() {
    let p = promise(
        &promise_id("xcom-bad"),
        "xcom.get",
        json!({
            "dag_id": XCOM_DAG,
            "dag_run_id": "whatever",
            "task_id": "push_by_returning",
            "xcom_key": "return_value",
            "map_index": "not-an-integer",
        }),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(!value["detail"].is_null(), "{value}");
}

/// halt — the credential is rejected.
#[tokio::test]
async fn xcom_get_halts_on_rejected_credentials() {
    let p = promise(
        &promise_id("xcom-halt"),
        "xcom.get",
        json!({
            "dag_id": XCOM_DAG,
            "dag_run_id": "whatever",
            "task_id": "push_by_returning",
            "xcom_key": "return_value",
        }),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("credentials rejected"), "{reason}");
}

// ─── dag.get (§4.5) ───────────────────────────────────────────────────────────

/// resolved — = response.body, including the `params` a caller needs to
/// compose `dagrun.trigger`'s `conf`.
#[tokio::test]
async fn dag_get_resolves_with_the_dag_details() {
    let p = promise(
        &promise_id("dag-get"),
        "dag.get",
        json!({"dag_id": OK_DAG}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["dag_id"], OK_DAG);
    assert!(value.get("params").is_some(), "details-only key missing: {value}");
    assert!(value.get("fileloc").is_some(), "{value}");
}

/// `not_found` — a nonexistent Dag id.
#[tokio::test]
async fn dag_get_rejects_not_found() {
    let p = promise(
        &promise_id("dag-get-404"),
        "dag.get",
        json!({"dag_id": "no_such_dag_e0f1"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "not_found"}));
}

/// `invalid_request` — `dag_id` is required by the Param schema.
#[tokio::test]
async fn dag_get_rejects_invalid_request() {
    let p = promise(&promise_id("dag-get-bad"), "dag.get", json!({}), in_ms(60_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(value["detail"].as_str().unwrap_or_default().contains("dag_id"), "{value}");
}

/// halt — the credential is rejected.
#[tokio::test]
async fn dag_get_halts_on_rejected_credentials() {
    let p = promise(
        &promise_id("dag-get-halt"),
        "dag.get",
        json!({"dag_id": OK_DAG}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("credentials rejected"), "{reason}");
}

// ─── dag.list (§4.6) ──────────────────────────────────────────────────────────

/// resolved — = response.body. The filter is passed straight through.
#[tokio::test]
async fn dag_list_resolves_with_the_dags() {
    let p = promise(
        &promise_id("dag-list"),
        "dag.list",
        json!({"dag_id_prefix_pattern": OK_DAG, "limit": 5}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    let dags = value["dags"].as_array().expect("dags");
    assert_eq!(dags.len(), 1, "{value}");
    assert_eq!(dags[0]["dag_id"], OK_DAG);
    assert!(value["total_entries"].is_number(), "{value}");
}

/// `invalid_request` — `limit` is documented as `minimum: 0`.
#[tokio::test]
async fn dag_list_rejects_invalid_request() {
    let p = promise(
        &promise_id("dag-list-bad"),
        "dag.list",
        json!({"limit": -1}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(!value["detail"].is_null(), "{value}");
}

/// halt — the credential is rejected.
#[tokio::test]
async fn dag_list_halts_on_rejected_credentials() {
    let p = promise(&promise_id("dag-list-halt"), "dag.list", json!({}), in_ms(60_000));

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("credentials rejected"), "{reason}");
}

// ─── Dispatch ─────────────────────────────────────────────────────────────────

/// A func no operation serves is a permanent rejection naming it.
#[tokio::test]
async fn an_unknown_func_is_rejected() {
    let p = promise(&promise_id("unknown"), "dagrun.explode", json!({}), in_ms(60_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "unknown_func", "detail": "dagrun.explode"}));
}

// ─── Fixtures ─────────────────────────────────────────────────────────────────

/// §2, built from the environment §5.2 exports. `poll` is overridden to 1s:
/// the default 30s would make every pending → terminal test wait a full
/// interval past the run's own end.
fn config() -> Config {
    Config {
        base_url: env("AIRFLOW_BASE_URL"),
        username: env("AIRFLOW_USERNAME"),
        password: env("AIRFLOW_PASSWORD"),
        poll: Duration::from_secs(1),
    }
}

/// The same Airflow, with a password it will reject.
fn bad_credential() -> Config {
    Config {
        password: format!("{}-wrong", env("AIRFLOW_PASSWORD")),
        ..config()
    }
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
/// unchanged, so a run id may be checked against it with `starts_with`.
fn promise_id(what: &str) -> String {
    format!("airflow.{what}.{}", nanos())
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

/// One client and one token for the whole test binary. §5 runs Airflow
/// standalone on SQLite, whose metadata database locks up under concurrent
/// writers, so provisioning keeps its footprint as small as it can.
fn http() -> reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new).clone()
}

async fn bearer() -> String {
    static TOKEN: tokio::sync::OnceCell<String> = tokio::sync::OnceCell::const_new();
    TOKEN
        .get_or_init(|| async {
            let body: Value = http()
                .post(format!("{}/auth/token", env("AIRFLOW_BASE_URL")))
                .json(
                    &json!({"username": env("AIRFLOW_USERNAME"), "password": env("AIRFLOW_PASSWORD")}),
                )
                .send()
                .await
                .expect("token request")
                .json()
                .await
                .expect("token response is JSON");
            format!("Bearer {}", body["access_token"].as_str().expect("access_token"))
        })
        .await
        .clone()
}

/// A paused Dag's runs never leave "queued", so every Dag a test expects to
/// finish has to be unpaused first. Reads before writing: on the SQLite
/// metadata database §5 provisions, an unnecessary write is a lock the
/// scheduler then has to wait for.
async fn unpause(dag_id: &str) {
    let dag: Value = http()
        .get(format!("{}/api/v2/dags/{dag_id}", env("AIRFLOW_BASE_URL")))
        .header("Authorization", bearer().await)
        .send()
        .await
        .expect("get dag request")
        .json()
        .await
        .expect("get dag response is JSON");
    if dag["is_paused"] == json!(false) {
        return;
    }
    let status = http()
        .patch(format!(
            "{}/api/v2/dags/{dag_id}?update_mask=is_paused",
            env("AIRFLOW_BASE_URL")
        ))
        .header("Authorization", bearer().await)
        .json(&json!({"is_paused": false}))
        .send()
        .await
        .expect("unpause request")
        .status();
    assert!(status.is_success(), "unpausing {dag_id}: {status}");
}

async fn trigger(dag_id: &str, body: Value) -> u16 {
    http()
        .post(format!("{}/api/v2/dags/{dag_id}/dagRuns", env("AIRFLOW_BASE_URL")))
        .header("Authorization", bearer().await)
        .json(&body)
        .send()
        .await
        .expect("trigger request")
        .status()
        .as_u16()
}

async fn delete_run(dag_id: &str, run_id: &str) -> u16 {
    http()
        .delete(format!(
            "{}/api/v2/dags/{dag_id}/dagRuns/{run_id}",
            env("AIRFLOW_BASE_URL")
        ))
        .header("Authorization", bearer().await)
        .send()
        .await
        .expect("delete request")
        .status()
        .as_u16()
}

async fn runs(dag_id: &str) -> Vec<Value> {
    let body: Value = http()
        .get(format!(
            "{}/api/v2/dags/{dag_id}/dagRuns?limit=200&order_by=-run_after",
            env("AIRFLOW_BASE_URL")
        ))
        .header("Authorization", bearer().await)
        .send()
        .await
        .expect("list runs request")
        .json()
        .await
        .expect("list runs response is JSON");
    body["dag_runs"].as_array().cloned().unwrap_or_default()
}

/// The runs this promise created — the run id is derived from the promise
/// id, so the promise id is its prefix.
async fn runs_with_prefix(dag_id: &str, promise_id: &str) -> Vec<String> {
    runs(dag_id)
        .await
        .iter()
        .filter_map(|r| r["dag_run_id"].as_str())
        .filter(|id| id.starts_with(promise_id))
        .map(str::to_string)
        .collect()
}

/// Wait for the run a promise created to exist, and return its id.
async fn await_run(dag_id: &str, promise_id: &str) -> String {
    for _ in 0..60 {
        if let Some(id) = runs_with_prefix(dag_id, promise_id).await.pop() {
            return id;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    panic!("no run for {promise_id} on {dag_id} appeared");
}

/// The state of one run, or "" if it does not exist yet.
async fn run_state(dag_id: &str, run_id: &str) -> String {
    let body: Value = http()
        .get(format!(
            "{}/api/v2/dags/{dag_id}/dagRuns/{run_id}",
            env("AIRFLOW_BASE_URL")
        ))
        .header("Authorization", bearer().await)
        .send()
        .await
        .expect("get run request")
        .json()
        .await
        .unwrap_or(Value::Null);
    body["state"].as_str().unwrap_or_default().to_string()
}

/// A finished, successful run to read from — provisioned outside the plugin
/// so a read operation's test does not depend on `dagrun.trigger`.
async fn succeeded_run(dag_id: &str, run_id: &str) -> String {
    assert_eq!(
        trigger(dag_id, json!({"dag_run_id": run_id, "logical_date": null})).await,
        200,
        "triggering {dag_id}"
    );
    for _ in 0..180 {
        let state = run_state(dag_id, run_id).await;
        if state == "success" {
            return run_id.to_string();
        }
        assert_ne!(state, "failed", "{dag_id}/{run_id} was expected to succeed");
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    panic!("{dag_id}/{run_id} did not finish");
}
