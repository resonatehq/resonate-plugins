//! `plugin::process` against a real Rundeck — never a mock.
//!
//! Bring the provider up with specification §5.1/§5.2 first; the exported
//! `RUNDECK_BASE_URL` and `RUNDECK_API_TOKEN` are what [`config`] reads. Every
//! documented condition below is induced with real inputs against that
//! instance — a failing job really fails, a missing id is really missing, a
//! corrupted token is really refused.
//!
//! §5 brings up an empty Rundeck: no project, no jobs, so no work items. The
//! work items are therefore provisioned here, by [`provision`], as a project
//! and seven jobs imported through Rundeck's own job-import endpoint — raw
//! provider calls, not the plugin's code path. Each job exists to induce one
//! documented terminal status: `ok` succeeds, `fail` fails, `timeout` exceeds
//! its own timeout, `retry` fails with a retry queued, `other` halts with a
//! custom exit status, `slow` runs long enough to be aborted or watched, and
//! `enforced` declares an option whose value Rundeck validates.

use std::collections::HashMap;
use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};

use resonate_core::types::{PromiseRecord, PromiseState, PromiseValue};
use resonate_plugin_rundeck::plugin::{self, Config};

/// The verdict `process` returns, spelled out: the crate's own alias for it
/// is an implementation detail.
type Verdict = Result<Result<String, String>, Result<String, String>>;

/// The project [`provision`] creates and every job below lives in.
const PROJECT: &str = "resonate";

/// Succeeds in seconds: one `echo`.
const OK_JOB: &str = "11111111-1111-4111-8111-111111111111";
/// Fails in seconds: its only step is `/bin/false`.
const FAIL_JOB: &str = "22222222-2222-4222-8222-222222222222";
/// Sleeps for five minutes: long enough to abort, to watch, or to leave
/// running past a deadline.
const SLOW_JOB: &str = "33333333-3333-4333-8333-333333333333";
/// Sleeps past its own `timeout: 5`, so Rundeck stops it: status `timedout`.
const TIMEOUT_JOB: &str = "44444444-4444-4444-8444-444444444444";
/// Fails with `retry: 1`, so this execution ends `failed-with-retry` and
/// Rundeck starts a separate retry execution.
const RETRY_JOB: &str = "55555555-5555-4555-8555-555555555555";
/// Halts through the Flow Control step with a custom exit status: `other`.
const OTHER_JOB: &str = "66666666-6666-4666-8666-666666666666";
/// Declares `color` as required and enforced over `[red, green]`, so an
/// option value outside that set is refused by the job's option validation.
const ENFORCED_JOB: &str = "77777777-7777-4777-8777-777777777777";
/// A job UUID no project holds.
const NO_JOB: &str = "00000000-0000-4000-8000-000000000000";

// ─── job.run (§4.1) ───────────────────────────────────────────────────────────

/// resolved — the succeeding work item. Also the pending → terminal path: the
/// run response is `running` with no `date-ended`, so a resolved value
/// carrying `date-ended` is proof the poll loop watched the execution to its
/// terminal state rather than reporting the run.
#[tokio::test]
async fn job_run_resolves_when_the_execution_succeeds() {
    provision().await;
    let id = promise_id("run-ok");
    let p = promise(&id, "job.run", json!({"id": OK_JOB}), in_ms(300_000));

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["status"], "succeeded");
    assert_eq!(value["project"], PROJECT);
    assert_eq!(value["job"]["id"], OK_JOB);
    assert!(value["id"].is_i64(), "{value}");
    assert!(value["href"].is_string(), "{value}");
    assert!(value["permalink"].is_string(), "{value}");
    assert!(value["user"].is_string(), "{value}");
    assert!(value["date-started"]["unixtime"].is_i64(), "{value}");
    assert!(value["date-ended"]["unixtime"].is_i64(), "not watched to its end: {value}");
    assert!(value["description"].is_string(), "{value}");
    assert!(value["successfulNodes"].is_array(), "{value}");
    // = sanitize(promise.id), injected as the rescorr option.
    let argstring = value["argstring"].as_str().expect("argstring is a string");
    assert!(argstring.contains(&format!("-rescorr {id}")), "{argstring}");
    // The Resolved mapping is these keys and only these keys — of the
    // thirteen it names, a succeeded execution carries every one but
    // `failedNodes`.
    assert!(value.get("failedNodes").is_none(), "{value}");
    assert_eq!(value.as_object().unwrap().len(), 12, "{value}");
}

/// `execution_failed` — the failing work item.
#[tokio::test]
async fn job_run_rejects_execution_failed() {
    provision().await;
    let id = promise_id("run-fail");
    let p = promise(&id, "job.run", json!({"id": FAIL_JOB}), in_ms(300_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "execution_failed");
    // detail = the terminal execution object.
    assert_eq!(value["detail"]["status"], "failed");
    assert_eq!(value["detail"]["job"]["id"], FAIL_JOB);
}

/// `execution_aborted` — the execution is aborted while the poll loop watches
/// it.
#[tokio::test]
async fn job_run_rejects_execution_aborted() {
    provision().await;
    let id = promise_id("run-abort");
    let p = promise(&id, "job.run", json!({"id": SLOW_JOB}), in_ms(300_000));

    let aborter = async {
        let execution = await_execution(&id).await;
        assert_eq!(abort(&execution).await, 200);
    };
    let config = config();
    let (verdict, ()) = tokio::join!(plugin::process(&config, &p), aborter);
    let value = rejected(verdict);

    assert_eq!(value["code"], "execution_aborted");
    assert_eq!(value["detail"]["status"], "aborted");
    assert!(value["detail"]["abortedby"].is_string(), "{value}");
}

/// `execution_timedout` — the job's own `timeout` stops it.
#[tokio::test]
async fn job_run_rejects_execution_timedout() {
    provision().await;
    let id = promise_id("run-timedout");
    let p = promise(&id, "job.run", json!({"id": TIMEOUT_JOB}), in_ms(300_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "execution_timedout");
    assert_eq!(value["detail"]["status"], "timedout");
}

/// `execution_failed_with_retry` — this execution is over and Rundeck started
/// a separate retry execution, which is not observed here.
#[tokio::test]
async fn job_run_rejects_execution_failed_with_retry() {
    provision().await;
    let id = promise_id("run-retry");
    let p = promise(&id, "job.run", json!({"id": RETRY_JOB}), in_ms(300_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "execution_failed_with_retry");
    assert_eq!(value["detail"]["status"], "failed-with-retry");
    // The retry execution has an id of its own — a different execution.
    let retried = &value["detail"]["retriedExecution"];
    assert!(retried["id"].is_i64(), "{value}");
    assert_ne!(retried["id"], value["detail"]["id"], "{value}");
}

/// `execution_other` — a custom exit status.
#[tokio::test]
async fn job_run_rejects_execution_other() {
    provision().await;
    let id = promise_id("run-other");
    let p = promise(&id, "job.run", json!({"id": OTHER_JOB}), in_ms(300_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "execution_other");
    assert_eq!(value["detail"]["status"], "other");
    assert_eq!(value["detail"]["customStatus"], "resonate-custom");
}

/// `job_not_found` — a job UUID no project holds.
#[tokio::test]
async fn job_run_rejects_job_not_found() {
    provision().await;
    let id = promise_id("run-nojob");
    let p = promise(&id, "job.run", json!({"id": NO_JOB}), in_ms(60_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "job_not_found");
    // detail = response.body.message of the 404.
    assert!(
        value["detail"].as_str().unwrap_or_default().contains(NO_JOB),
        "{value}"
    );
}

/// `invalid_request` — an option value the job's option validation refuses.
#[tokio::test]
async fn job_run_rejects_invalid_request() {
    provision().await;
    let id = promise_id("run-badopt");
    let p = promise(
        &id,
        "job.run",
        json!({"id": ENFORCED_JOB, "options": {"color": "purple"}}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    // detail = response.body.message of the 4xx.
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("purple"),
        "{value}"
    );
}

/// `deleted` — the execution is removed before its terminal state is
/// observed. Rundeck only permits deleting a finished execution, so the
/// window is between the execution finishing and the next poll: `poll` is
/// stretched to 20s to hold that window open while the test deletes.
#[tokio::test]
async fn job_run_rejects_deleted() {
    provision().await;
    let id = promise_id("run-deleted");
    let p = promise(&id, "job.run", json!({"id": OK_JOB}), in_ms(120_000));

    let deleter = async {
        let execution = await_execution(&id).await;
        await_terminal(&execution).await;
        assert_eq!(delete(&execution).await, 204);
    };
    let config = Config { poll: Duration::from_secs(20), ..config() };
    let (verdict, ()) = tokio::join!(plugin::process(&config, &p), deleter);
    let value = rejected(verdict);

    assert_eq!(value["code"], "deleted");
    // detail: absent.
    assert_eq!(value.as_object().unwrap().len(), 1, "{value}");
}

/// The deadline — `timeout_at` already in the past. No verdict: the server
/// settles a timed-out promise itself.
#[tokio::test]
async fn job_run_releases_at_the_deadline() {
    provision().await;
    let id = promise_id("run-deadline");
    let p = promise(&id, "job.run", json!({"id": SLOW_JOB}), in_ms(-1_000));

    let reason = released(plugin::process(&config(), &p).await);

    assert_eq!(reason, "promise timed out");
    // The execution was started before the deadline was observed; take it back.
    abort(&await_execution(&id).await).await;
}

/// halt — the token is refused, and no retry of ours can fix that.
#[tokio::test]
async fn job_run_halts_on_a_refused_token() {
    provision().await;
    let id = promise_id("run-halt");
    let p = promise(&id, "job.run", json!({"id": OK_JOB}), in_ms(60_000));

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("unauthorized"), "{reason}");
}

/// Re-entry — a redelivery locates the execution an earlier attempt started,
/// by the `rescorr` option stamped on it, rather than starting a second one.
#[tokio::test]
async fn job_run_reattaches_on_re_entry() {
    provision().await;
    let id = promise_id("run-reentry");
    let p = promise(&id, "job.run", json!({"id": OK_JOB}), in_ms(300_000));

    let first = resolved(plugin::process(&config(), &p).await);
    let second = resolved(plugin::process(&config(), &p).await);

    assert_eq!(first["id"], second["id"]);
    assert_eq!(first["date-started"], second["date-started"]);
    // One promise, one execution.
    assert_eq!(executions_of(&id).await.len(), 1);
}

/// The `argString` form of the injection: with no `options`, the identity is
/// appended to `argString` instead — and the execution is still locatable by
/// it.
#[tokio::test]
async fn job_run_appends_the_identity_to_argstring() {
    provision().await;
    let id = promise_id("run-argstring");
    let p = promise(
        &id,
        "job.run",
        json!({"id": OK_JOB, "argString": "-extra one"}),
        in_ms(300_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    let argstring = value["argstring"].as_str().expect("argstring is a string");
    assert!(argstring.starts_with("-extra one -rescorr "), "{argstring}");
    assert!(argstring.contains(&format!("-rescorr {id}")), "{argstring}");
    assert_eq!(executions_of(&id).await.len(), 1);
}

// ─── job.get (§4.2) ───────────────────────────────────────────────────────────

/// resolved — = response.body, the job-json export, including the declared
/// `options` a caller needs to compose `job.run`.
#[tokio::test]
async fn job_get_resolves_with_the_job_definition() {
    provision().await;
    let p = promise(
        &promise_id("job-get"),
        "job.get",
        json!({"id": ENFORCED_JOB}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    let jobs = value.as_array().expect("job-json is an array");
    assert_eq!(jobs.len(), 1, "{value}");
    assert_eq!(jobs[0]["id"], ENFORCED_JOB);
    assert_eq!(jobs[0]["name"], "enforced");
    assert_eq!(jobs[0]["options"][0]["name"], "color");
    assert_eq!(jobs[0]["options"][0]["values"], json!(["red", "green"]));
}

/// `not_found` — a job UUID no project holds.
#[tokio::test]
async fn job_get_rejects_not_found() {
    provision().await;
    let p = promise(
        &promise_id("job-get-404"),
        "job.get",
        json!({"id": NO_JOB}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "not_found"}));
}

/// `invalid_request` — `id` is required by the Param schema. A promise's
/// param is immutable, so this is permanent and decided locally.
#[tokio::test]
async fn job_get_rejects_invalid_request() {
    provision().await;
    let p = promise(&promise_id("job-get-bad"), "job.get", json!({}), in_ms(60_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(value["detail"].as_str().unwrap_or_default().contains("id"), "{value}");
}

/// halt — the token is refused.
#[tokio::test]
async fn job_get_halts_on_a_refused_token() {
    provision().await;
    let p = promise(
        &promise_id("job-get-halt"),
        "job.get",
        json!({"id": OK_JOB}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("unauthorized"), "{reason}");
}

// ─── job.list (§4.3) ──────────────────────────────────────────────────────────

/// resolved — = response.body. The filters are passed straight through.
#[tokio::test]
async fn job_list_resolves_with_the_jobs() {
    provision().await;
    let p = promise(
        &promise_id("job-list"),
        "job.list",
        json!({"project": PROJECT, "jobExactFilter": "ok", "max": 5}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    let jobs = value.as_array().expect("jobs is an array");
    assert_eq!(jobs.len(), 1, "{value}");
    assert_eq!(jobs[0]["id"], OK_JOB);
    assert_eq!(jobs[0]["name"], "ok");
    assert_eq!(jobs[0]["project"], PROJECT);
}

/// `not_found` — a project that does not exist.
#[tokio::test]
async fn job_list_rejects_not_found() {
    provision().await;
    let p = promise(
        &promise_id("job-list-404"),
        "job.list",
        json!({"project": "no_such_project_e0f1"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "not_found"}));
}

/// `invalid_request` — `max` is documented as `minimum: 0`.
#[tokio::test]
async fn job_list_rejects_invalid_request() {
    provision().await;
    let p = promise(
        &promise_id("job-list-bad"),
        "job.list",
        json!({"project": PROJECT, "max": -1}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    // detail = response.body.message of the 4xx.
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("max"),
        "{value}"
    );
}

/// halt — the token is refused.
#[tokio::test]
async fn job_list_halts_on_a_refused_token() {
    provision().await;
    let p = promise(
        &promise_id("job-list-halt"),
        "job.list",
        json!({"project": PROJECT}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("unauthorized"), "{reason}");
}

// ─── execution.get (§4.4) ─────────────────────────────────────────────────────

/// resolved — = response.body. Provisioned outside the plugin, so a read
/// operation's test does not depend on `job.run`.
#[tokio::test]
async fn execution_get_resolves_with_the_execution() {
    provision().await;
    let execution = succeeded_execution("exec-get").await;
    let p = promise(
        &promise_id("exec-get"),
        "execution.get",
        json!({"id": execution}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["id"].to_string(), execution);
    assert_eq!(value["status"], "succeeded");
    assert_eq!(value["job"]["id"], OK_JOB);
    assert_eq!(value["project"], PROJECT);
}

/// `not_found` — an execution id that does not exist.
#[tokio::test]
async fn execution_get_rejects_not_found() {
    provision().await;
    let p = promise(
        &promise_id("exec-get-404"),
        "execution.get",
        json!({"id": "99999999"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "not_found"}));
}

/// `invalid_request` — `id` is required by the Param schema.
#[tokio::test]
async fn execution_get_rejects_invalid_request() {
    provision().await;
    let p = promise(
        &promise_id("exec-get-bad"),
        "execution.get",
        json!({}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(value["detail"].as_str().unwrap_or_default().contains("id"), "{value}");
}

/// halt — the token is refused.
#[tokio::test]
async fn execution_get_halts_on_a_refused_token() {
    provision().await;
    let p = promise(
        &promise_id("exec-get-halt"),
        "execution.get",
        json!({"id": "1"}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("unauthorized"), "{reason}");
}

// ─── executionoutput.get (§4.5) ───────────────────────────────────────────────

/// resolved — = response.body, the log the job published. `compacted` is a
/// boolean the query string has to carry lowercase.
#[tokio::test]
async fn executionoutput_get_resolves_with_the_output() {
    provision().await;
    let execution = succeeded_execution("out-get").await;
    let p = promise(
        &promise_id("out-get"),
        "executionoutput.get",
        json!({"id": execution, "maxlines": 50, "compacted": true}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["id"], execution);
    assert_eq!(value["execCompleted"], json!(true));
    assert_eq!(value["execState"], "succeeded");
    // compacted came back as requested, so the lowercase rendering was
    // accepted — Rundeck's flag is case-sensitive.
    assert_eq!(value["compacted"], json!(true));
    let entries = value["entries"].as_array().expect("entries");
    assert!(
        entries.iter().any(|e| e["log"] == json!("ok")),
        "the job's output is missing: {value}"
    );
}

/// `not_found` — an execution id that does not exist.
#[tokio::test]
async fn executionoutput_get_rejects_not_found() {
    provision().await;
    let p = promise(
        &promise_id("out-get-404"),
        "executionoutput.get",
        json!({"id": "99999999"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "not_found"}));
}

/// `invalid_request` — `offset` is documented as `minimum: 0`. This
/// endpoint's 4xx body reports the fault under `error` and carries no
/// `message`, and the Rejected schema maps `detail` from `message`: absent
/// becomes null rather than a crash.
#[tokio::test]
async fn executionoutput_get_rejects_invalid_request() {
    provision().await;
    let execution = succeeded_execution("out-bad").await;
    let p = promise(
        &promise_id("out-get-bad"),
        "executionoutput.get",
        json!({"id": execution, "offset": -1}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert_eq!(value["detail"], Value::Null, "{value}");
}

/// halt — the token is refused.
#[tokio::test]
async fn executionoutput_get_halts_on_a_refused_token() {
    provision().await;
    let p = promise(
        &promise_id("out-get-halt"),
        "executionoutput.get",
        json!({"id": "1"}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("unauthorized"), "{reason}");
}

// ─── Dispatch ─────────────────────────────────────────────────────────────────

/// A func no operation serves is a permanent rejection naming it.
#[tokio::test]
async fn an_unknown_func_is_rejected() {
    let p = promise(&promise_id("unknown"), "job.explode", json!({}), in_ms(60_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "unknown_func", "detail": "job.explode"}));
}

// ─── Fixtures ─────────────────────────────────────────────────────────────────

/// §2, built from the environment §5.2 exports. `poll` is overridden to 1s:
/// the default 5s would make every pending → terminal test wait a full
/// interval past the execution's own end.
fn config() -> Config {
    Config {
        base_url: env("RUNDECK_BASE_URL"),
        api_token: env("RUNDECK_API_TOKEN"),
        poll: Duration::from_secs(1),
    }
}

/// The same Rundeck, with a token it will refuse.
fn bad_credential() -> Config {
    Config {
        api_token: format!("{}wrong", env("RUNDECK_API_TOKEN")),
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
/// unchanged, so the injected `rescorr` value has it as a prefix.
fn promise_id(what: &str) -> String {
    format!("rundeck.{what}.{}", nanos())
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

/// The work items, in Rundeck's job-json import format. One job per terminal
/// status §4.1.4 documents, plus one whose option validation refuses a value.
const JOBS: &str = r#"[
  {"uuid": "11111111-1111-4111-8111-111111111111", "name": "ok",
   "description": "succeeds in a second", "loglevel": "INFO",
   "multipleExecutions": true, "executionEnabled": true,
   "sequence": {"keepgoing": false, "strategy": "node-first",
                "commands": [{"exec": "echo ok"}]}},
  {"uuid": "22222222-2222-4222-8222-222222222222", "name": "fail",
   "description": "fails in a second", "loglevel": "INFO",
   "multipleExecutions": true, "executionEnabled": true,
   "sequence": {"keepgoing": false, "strategy": "node-first",
                "commands": [{"exec": "/bin/false"}]}},
  {"uuid": "33333333-3333-4333-8333-333333333333", "name": "slow",
   "description": "runs for five minutes", "loglevel": "INFO",
   "multipleExecutions": true, "executionEnabled": true,
   "sequence": {"keepgoing": false, "strategy": "node-first",
                "commands": [{"exec": "sleep 300"}]}},
  {"uuid": "44444444-4444-4444-8444-444444444444", "name": "timeout",
   "description": "exceeds its own timeout", "loglevel": "INFO",
   "multipleExecutions": true, "executionEnabled": true, "timeout": "5",
   "sequence": {"keepgoing": false, "strategy": "node-first",
                "commands": [{"exec": "sleep 120"}]}},
  {"uuid": "55555555-5555-4555-8555-555555555555", "name": "retry",
   "description": "fails and is retried once", "loglevel": "INFO",
   "multipleExecutions": true, "executionEnabled": true, "retry": "1",
   "sequence": {"keepgoing": false, "strategy": "node-first",
                "commands": [{"exec": "/bin/false"}]}},
  {"uuid": "66666666-6666-4666-8666-666666666666", "name": "other",
   "description": "halts with a custom exit status", "loglevel": "INFO",
   "multipleExecutions": true, "executionEnabled": true,
   "sequence": {"keepgoing": false, "strategy": "node-first",
                "commands": [{"type": "flow-control", "nodeStep": false,
                              "configuration": {"halt": "true", "fail": "false",
                                                "status": "resonate-custom"}}]}},
  {"uuid": "77777777-7777-4777-8777-777777777777", "name": "enforced",
   "description": "declares an option whose value is enforced",
   "loglevel": "INFO", "multipleExecutions": true, "executionEnabled": true,
   "options": [{"name": "color", "required": true, "enforced": true,
                "values": ["red", "green"]}],
   "sequence": {"keepgoing": false, "strategy": "node-first",
                "commands": [{"exec": "echo ${option.color}"}]}}
]"#;

/// One client for the whole test binary.
fn http() -> reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new).clone()
}

fn api(path: &str) -> String {
    format!("{}/api/59{path}", env("RUNDECK_BASE_URL"))
}

fn with_token(req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    req.header("X-Rundeck-Auth-Token", env("RUNDECK_API_TOKEN"))
        .header("Accept", "application/json")
}

/// The project and the jobs the tests run. §5 brings up an empty Rundeck, so
/// the work items are created here — once per test binary, and idempotently:
/// the project is created if absent and the import updates in place under
/// fixed UUIDs, so a re-run against a surviving container is a no-op.
async fn provision() {
    static ONCE: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();
    ONCE.get_or_init(|| async {
        let status = with_token(http().post(api("/projects")))
            .json(&json!({"name": PROJECT}))
            .send()
            .await
            .expect("create project request")
            .status()
            .as_u16();
        // 409: the project is already there, from an earlier run.
        assert!(status == 201 || status == 409, "creating project: {status}");

        let response = with_token(
            http().post(api(&format!(
                "/project/{PROJECT}/jobs/import\
                 ?format=json&dupeOption=update&uuidOption=preserve"
            ))),
        )
        .header("Content-Type", "application/json")
        .body(JOBS)
        .send()
        .await
        .expect("job import request");
        assert!(response.status().is_success(), "importing jobs: {:?}", response.status());
        let body: Value = response.json().await.expect("import response is JSON");
        assert_eq!(
            body["failed"].as_array().map(Vec::len).unwrap_or(0),
            0,
            "importing jobs: {body}"
        );
        assert_eq!(body["succeeded"].as_array().map(Vec::len).unwrap_or(0), 7, "{body}");
    })
    .await;
}

/// Every execution in the project whose `rescorr` option carries this promise
/// id — the executions the promise created, found the way §4.1's own locate
/// step finds them. The sanitized token is the promise id plus a digest, so
/// the promise id is a prefix of it and `argstring` is matched on that.
async fn executions_of(promise_id: &str) -> Vec<String> {
    let body: Value = with_token(http().get(api(&format!("/project/{PROJECT}/executions"))))
        .query(&[("max", "200")])
        .send()
        .await
        .expect("list executions request")
        .json()
        .await
        .expect("list executions response is JSON");
    body["executions"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter(|e| {
            e["argstring"]
                .as_str()
                .unwrap_or_default()
                .contains(&format!("-rescorr {promise_id}"))
        })
        .filter_map(|e| e["id"].as_i64())
        .map(|id| id.to_string())
        .collect()
}

/// Wait for the execution a promise started to exist, and return its id.
async fn await_execution(promise_id: &str) -> String {
    for _ in 0..120 {
        if let Some(id) = executions_of(promise_id).await.pop() {
            return id;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    panic!("no execution for {promise_id} appeared");
}

/// The state of one execution, or "" if it does not exist.
async fn execution_status(execution: &str) -> String {
    let body: Value = with_token(http().get(api(&format!("/execution/{execution}"))))
        .send()
        .await
        .expect("get execution request")
        .json()
        .await
        .unwrap_or(Value::Null);
    body["status"].as_str().unwrap_or_default().to_string()
}

/// Wait for an execution to leave `running`/`scheduled` — Rundeck refuses to
/// delete one that has not finished.
async fn await_terminal(execution: &str) {
    for _ in 0..120 {
        let status = execution_status(execution).await;
        if !matches!(status.as_str(), "running" | "scheduled" | "") {
            return;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    panic!("execution {execution} never finished");
}

async fn abort(execution: &str) -> u16 {
    with_token(http().post(api(&format!("/execution/{execution}/abort"))))
        .send()
        .await
        .expect("abort request")
        .status()
        .as_u16()
}

async fn delete(execution: &str) -> u16 {
    with_token(http().delete(api(&format!("/execution/{execution}"))))
        .send()
        .await
        .expect("delete request")
        .status()
        .as_u16()
}

/// A finished, successful execution to read from — provisioned outside the
/// plugin so a read operation's test does not depend on `job.run`.
async fn succeeded_execution(what: &str) -> String {
    let body: Value = with_token(http().post(api(&format!("/job/{OK_JOB}/run"))))
        .json(&json!({"options": {"rescorr": format!("seed-{what}-{}", nanos())}}))
        .send()
        .await
        .expect("run request")
        .json()
        .await
        .expect("run response is JSON");
    let execution = body["id"].as_i64().expect("execution id").to_string();
    for _ in 0..120 {
        match execution_status(&execution).await.as_str() {
            "succeeded" => return execution,
            "running" | "scheduled" => {}
            other => panic!("execution {execution} was expected to succeed, got {other}"),
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    panic!("execution {execution} did not finish");
}
