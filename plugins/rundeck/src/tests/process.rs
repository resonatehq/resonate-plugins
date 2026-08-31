//! `plugin::process` against a real Rundeck — never a mock.
//!
//! Bring the provider up with specification §5.1/§5.2 first; the exported
//! `RUNDECK_BASE_URL`, `RUNDECK_API_TOKEN`, `RUNDECK_FIXTURE_OK` and
//! `RUNDECK_FIXTURE_FAIL` are what [`config`] and [`fixtures`] read. Every
//! documented condition below is induced with real inputs against that
//! instance — a failing job really fails, a missing id is really missing, a
//! corrupted token is really rejected.
//!
//! §5 provisions two jobs: `fixture-ok` succeeds, `fixture-fail` exits
//! non-zero. Four of the terminal statuses §4.1.4 enumerates need jobs §5
//! does not provision, so [`provision`] imports them as test setup, with raw
//! provider calls: `fixture-retry` (Rundeck starts a retry, so the first
//! execution ends `failed-with-retry`), `fixture-timeout` (a job timeout it
//! sleeps past, so `timedout`), `fixture-other` (a Flow Control halt with a
//! custom status, so `other` with a `customStatus`), `fixture-slow` (sleeps
//! long enough to be aborted or watched), and `fixture-cond` (fails while
//! its `pass` option is empty, succeeds once a retry sets it — the only way
//! a retry on this instance can succeed where the execution it retries
//! failed).
//!
//! Two documented conditions are not induced here:
//!
//! * `abort_failed` (§4.8) needs `abort.status` "failed" on an execution
//!   that is still going. On this single-node instance Rundeck answers
//!   "pending" to every abort of a running execution — however many times it
//!   is repeated — and "failed" only once the execution has stopped, which
//!   is the terminal-status branch instead. The state that produces it (an
//!   execution the database calls running with no thread behind it) is a
//!   crashed or clustered server, not an input a test can supply.
//! * `invalid_request` for `project.list` (§4.12): its only parameter is
//!   `meta`, free text Rundeck answers 200 to whatever it holds, and the
//!   operation has no required argument to omit.

use std::collections::HashMap;
use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};

use resonate_core::types::{PromiseRecord, PromiseState, PromiseValue};
use resonate_plugin_rundeck::plugin::{self, Config};

/// The verdict `process` returns, spelled out: the crate's own alias for it
/// is an implementation detail.
type Verdict = Result<Result<String, String>, Result<String, String>>;

/// A job UUID no project holds.
const NO_JOB: &str = "no-such-job-e0f1";
/// An execution id no project holds.
const NO_EXECUTION: i64 = 999_999;

// ─── job.run (§4.1) ───────────────────────────────────────────────────────────

/// resolved — the succeeding work item. Also the pending → terminal path:
/// the run response is "running" with no `date-ended`, so a resolved value
/// carrying `date-ended` is proof the poll loop watched the execution to its
/// terminal state rather than reporting the acceptance.
#[tokio::test]
async fn job_run_resolves_when_the_execution_succeeds() {
    let f = fixtures().await;
    let id = promise_id("run-ok");
    let p = promise(
        &id,
        "job.run",
        json!({"id": f.ok, "argString": "-seconds 3"}),
        in_ms(300_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["status"], "succeeded");
    assert_eq!(value["project"], f.project);
    assert_eq!(value["executionType"], "user");
    assert_eq!(value["job"]["id"], f.ok);
    assert!(
        value["date-ended"].is_object(),
        "not watched to its end: {value}"
    );
    assert!(value["successfulNodes"].is_array(), "{value}");
    // = sanitize(promise.id), appended to the argString the caller supplied
    // and parsed back out into job.options.
    let stamp = value["job"]["options"]["rescorr"]
        .as_str()
        .unwrap_or_default();
    assert!(stamp.starts_with(&id), "{stamp} should be derived from {id}");
    assert_eq!(value["job"]["options"]["seconds"], "3");
    // The §4.1.2 Resolved schema is exactly these sixteen keys.
    assert_eq!(value.as_object().map(|o| o.len()), Some(16), "{value}");
    assert_eq!(value["customStatus"], Value::Null);
    assert_eq!(value["retriedExecution"], Value::Null);
}

/// resolved with an options map — the other branch of the stamp: an options
/// map makes Rundeck ignore argString, so the stamp joins the map instead of
/// the string.
#[tokio::test]
async fn job_run_resolves_with_an_options_map() {
    let f = fixtures().await;
    let id = promise_id("run-options");
    let p = promise(
        &id,
        "job.run",
        json!({"id": f.ok, "options": {"seconds": "1"}}),
        in_ms(300_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["status"], "succeeded");
    assert_eq!(value["job"]["options"]["seconds"], "1");
    assert!(value["job"]["options"]["rescorr"]
        .as_str()
        .unwrap_or_default()
        .starts_with(&id));
}

/// `execution_failed` — the failing work item.
#[tokio::test]
async fn job_run_rejects_execution_failed() {
    let f = fixtures().await;
    let id = promise_id("run-fail");
    let p = promise(&id, "job.run", json!({"id": f.fail}), in_ms(300_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "execution_failed");
    // detail = the terminal execution record.
    assert_eq!(value["detail"]["status"], "failed");
    assert_eq!(value["detail"]["job"]["id"], f.fail);
    assert!(value["detail"]["failedNodes"].is_array(), "{value}");
}

/// `execution_failed_with_retry` — a job that declares a retry: this
/// execution is over and Rundeck started a separate one, named by
/// `retriedExecution`.
#[tokio::test]
async fn job_run_rejects_execution_failed_with_retry() {
    let f = fixtures().await;
    let id = promise_id("run-retrying");
    let p = promise(&id, "job.run", json!({"id": f.retry}), in_ms(300_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "execution_failed_with_retry");
    assert_eq!(value["detail"]["status"], "failed-with-retry");
    assert!(
        value["detail"]["retriedExecution"]["id"].is_number(),
        "{value}"
    );
}

/// `execution_aborted` — the execution is aborted while the poll loop
/// watches it.
#[tokio::test]
async fn job_run_rejects_execution_aborted() {
    let f = fixtures().await;
    let id = promise_id("run-aborted");
    let p = promise(&id, "job.run", json!({"id": f.slow}), in_ms(300_000));

    let aborter = async {
        let execution = await_stamped(&id).await;
        abort_execution(execution).await;
    };
    let cfg = config();
    let (verdict, ()) = tokio::join!(plugin::process(&cfg, &p), aborter);
    let value = rejected(verdict);

    assert_eq!(value["code"], "execution_aborted");
    assert_eq!(value["detail"]["status"], "aborted");
    assert_eq!(value["detail"]["abortedby"], "admin");
}

/// `execution_timedout` — a job whose own timeout expires before its step
/// finishes.
#[tokio::test]
async fn job_run_rejects_execution_timedout() {
    let f = fixtures().await;
    let id = promise_id("run-timedout");
    let p = promise(&id, "job.run", json!({"id": f.timeout}), in_ms(300_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "execution_timedout");
    assert_eq!(value["detail"]["status"], "timedout");
}

/// `execution_other` — a custom exit status, its string in `customStatus`.
#[tokio::test]
async fn job_run_rejects_execution_other() {
    let f = fixtures().await;
    let id = promise_id("run-other");
    let p = promise(&id, "job.run", json!({"id": f.other}), in_ms(300_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "execution_other");
    assert_eq!(value["detail"]["status"], "other");
    assert_eq!(value["detail"]["customStatus"], "custom-status");
}

/// `job_not_found` — a nonexistent job UUID.
#[tokio::test]
async fn job_run_rejects_job_not_found() {
    let id = promise_id("run-nojob");
    let p = promise(&id, "job.run", json!({"id": NO_JOB}), in_ms(60_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "job_not_found");
    // detail = response.body.message of the 404.
    assert_eq!(value["detail"], format!("Job does not exist: {NO_JOB}"));
}

/// `invalid_request` — `runAtTime` is documented as ISO-8601, and Rundeck
/// refuses the request when it is not.
#[tokio::test]
async fn job_run_rejects_invalid_request() {
    let f = fixtures().await;
    let id = promise_id("run-bad");
    let p = promise(
        &id,
        "job.run",
        json!({"id": f.ok, "runAtTime": "not-a-date"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    // detail = the error body, verbatim.
    let detail = value["detail"].as_str().unwrap_or_default().to_string();
    assert!(detail.contains("only ISO 8601 is supported"), "{value}");
}

/// `execution_deleted` — the execution is removed before the poll loop sees
/// it finish. The cadence is widened so the deletion lands inside one sleep:
/// Rundeck refuses to delete a running execution, so the window has to be
/// wide enough for the execution to finish first.
#[tokio::test]
async fn job_run_rejects_execution_deleted() {
    let f = fixtures().await;
    let id = promise_id("run-deleted");
    let p = promise(&id, "job.run", json!({"id": f.ok}), in_ms(300_000));
    let mut cfg = config();
    cfg.poll_job_run = Some(Duration::from_secs(25));

    let deleter = async {
        let execution = await_stamped(&id).await;
        await_terminal(execution).await;
        assert_eq!(delete_execution(execution).await, 204);
    };
    let (verdict, ()) = tokio::join!(plugin::process(&cfg, &p), deleter);
    let value = rejected(verdict);

    // detail: absent.
    assert_eq!(value, json!({"code": "execution_deleted"}));
}

/// halt — the token is rejected, and no retry of ours can fix that.
#[tokio::test]
async fn job_run_halts_on_a_rejected_token() {
    let f = fixtures().await;
    let id = promise_id("run-halt");
    let p = promise(&id, "job.run", json!({"id": f.ok}), in_ms(60_000));

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("unauthorized"), "{reason}");
}

/// The deadline — `timeout_at` already in the past. No verdict: the server
/// settles a timed-out promise itself.
#[tokio::test]
async fn job_run_releases_at_the_deadline() {
    let f = fixtures().await;
    let id = promise_id("run-deadline");
    let p = promise(&id, "job.run", json!({"id": f.ok}), in_ms(-1_000));

    let reason = released(plugin::process(&config(), &p).await);

    assert_eq!(reason, "promise timed out");
    // The execution was created before the deadline was observed.
    assert_eq!(await_stamped_all(&id).await.len(), 1);
}

/// Re-entry — a redelivery re-finds the execution an earlier attempt
/// created, rather than starting a second one.
#[tokio::test]
async fn job_run_refinds_its_execution_on_re_entry() {
    let f = fixtures().await;
    let id = promise_id("run-reentry");
    let p = promise(&id, "job.run", json!({"id": f.ok}), in_ms(300_000));

    let first = resolved(plugin::process(&config(), &p).await);
    let second = resolved(plugin::process(&config(), &p).await);

    assert_eq!(first["id"], second["id"]);
    assert_eq!(first["date-started"], second["date-started"]);
    // One promise, one execution.
    assert_eq!(await_stamped_all(&id).await.len(), 1);
}

// ─── job.submit (§4.2) ────────────────────────────────────────────────────────

/// resolved — the accepted execution handle, not a finished execution.
#[tokio::test]
async fn job_submit_resolves_with_the_accepted_execution() {
    let f = fixtures().await;
    let id = promise_id("submit-ok");
    let p = promise(&id, "job.submit", json!({"id": f.slow}), in_ms(60_000));

    let value = resolved(plugin::process(&config(), &p).await);

    // = response.body, the whole execution record.
    assert_eq!(value["status"], "running");
    assert_eq!(value["project"], f.project);
    assert!(value["id"].is_number(), "{value}");
    assert!(value["job"]["options"]["rescorr"]
        .as_str()
        .unwrap_or_default()
        .starts_with(&id));
    assert_eq!(value["date-ended"], Value::Null);

    abort_execution(value["id"].as_i64().unwrap_or_default()).await;
}

/// resolved on a scheduled execution — `runAtTime` parks it in the
/// non-terminal status "scheduled", which submit hands back as it is.
#[tokio::test]
async fn job_submit_resolves_with_a_scheduled_execution() {
    let f = fixtures().await;
    let id = promise_id("submit-scheduled");
    let p = promise(
        &id,
        "job.submit",
        json!({"id": f.ok, "runAtTime": in_iso8601(3_600_000)}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["status"], "scheduled");
    assert_eq!(value["executionType"], "user-scheduled");

    abort_execution(value["id"].as_i64().unwrap_or_default()).await;
}

/// `job_not_found` — a nonexistent job UUID.
#[tokio::test]
async fn job_submit_rejects_job_not_found() {
    let id = promise_id("submit-nojob");
    let p = promise(&id, "job.submit", json!({"id": NO_JOB}), in_ms(60_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(
        value,
        json!({"code": "job_not_found", "detail": format!("Job does not exist: {NO_JOB}")})
    );
}

/// `invalid_request` — `runAtTime` is documented as ISO-8601.
#[tokio::test]
async fn job_submit_rejects_invalid_request() {
    let f = fixtures().await;
    let id = promise_id("submit-bad");
    let p = promise(
        &id,
        "job.submit",
        json!({"id": f.ok, "runAtTime": "not-a-date"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("only ISO 8601 is supported"),
        "{value}"
    );
}

/// halt — the token is rejected.
#[tokio::test]
async fn job_submit_halts_on_a_rejected_token() {
    let f = fixtures().await;
    let id = promise_id("submit-halt");
    let p = promise(&id, "job.submit", json!({"id": f.ok}), in_ms(60_000));

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("unauthorized"), "{reason}");
}

/// Re-entry — a redelivery hands back the execution the first delivery
/// created, with whatever status it has since reached.
#[tokio::test]
async fn job_submit_refinds_its_execution_on_re_entry() {
    let f = fixtures().await;
    let id = promise_id("submit-reentry");
    let p = promise(&id, "job.submit", json!({"id": f.ok}), in_ms(60_000));

    let first = resolved(plugin::process(&config(), &p).await);
    let execution = first["id"].as_i64().unwrap_or_default();
    await_terminal(execution).await;
    let second = resolved(plugin::process(&config(), &p).await);

    assert_eq!(first["id"], second["id"]);
    assert_eq!(first["status"], "running");
    assert_eq!(second["status"], "succeeded");
    // One promise, one execution.
    assert_eq!(await_stamped_all(&id).await.len(), 1);
}

// ─── job.retry (§4.3) ─────────────────────────────────────────────────────────

/// resolved — the retry succeeds where the execution it retries failed:
/// `fixture-cond` fails while its `pass` option is empty, and the options
/// given here are merged over the prior execution's.
#[tokio::test]
async fn job_retry_resolves_when_the_retry_succeeds() {
    let f = fixtures().await;
    let prior = failed_execution(&f.cond).await;
    let id = promise_id("retry-ok");
    let p = promise(
        &id,
        "job.retry",
        json!({"id": f.cond, "executionId": prior, "options": {"pass": "yes"}}),
        in_ms(300_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["status"], "succeeded");
    assert_eq!(value["job"]["id"], f.cond);
    assert_eq!(value["job"]["options"]["pass"], "yes");
    assert!(value["job"]["options"]["rescorr"]
        .as_str()
        .unwrap_or_default()
        .starts_with(&id));
    assert!(value["date-ended"].is_object(), "{value}");
    // The §4.1.2 Resolved schema is exactly these sixteen keys.
    assert_eq!(value.as_object().map(|o| o.len()), Some(16), "{value}");
}

/// `execution_failed` — the retry fails too.
#[tokio::test]
async fn job_retry_rejects_execution_failed() {
    let f = fixtures().await;
    let prior = failed_execution(&f.fail).await;
    let id = promise_id("retry-fail");
    let p = promise(
        &id,
        "job.retry",
        json!({"id": f.fail, "executionId": prior}),
        in_ms(300_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "execution_failed");
    assert_eq!(value["detail"]["status"], "failed");
    // A retry is a new execution, not the one it retries.
    assert_ne!(value["detail"]["id"], json!(prior));
}

/// `execution_not_found` — the prior execution does not exist, which the
/// read that opens the operation discovers.
#[tokio::test]
async fn job_retry_rejects_execution_not_found() {
    let f = fixtures().await;
    let id = promise_id("retry-noexec");
    let p = promise(
        &id,
        "job.retry",
        json!({"id": f.fail, "executionId": NO_EXECUTION}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(
        value,
        json!({
            "code": "execution_not_found",
            "detail": format!("Execution does not exist: {NO_EXECUTION}"),
        })
    );
}

/// `job_not_found` — the job UUID is unknown. The retry 404 names the
/// execution rather than the job, because the endpoint resolves the
/// execution within the job; both ids are re-read to tell the causes apart.
#[tokio::test]
async fn job_retry_rejects_job_not_found() {
    let f = fixtures().await;
    let prior = failed_execution(&f.fail).await;
    let id = promise_id("retry-nojob");
    let p = promise(
        &id,
        "job.retry",
        json!({"id": NO_JOB, "executionId": prior}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(
        value,
        json!({
            "code": "job_not_found",
            "detail": format!("Execution ID does not exist: {prior}"),
        })
    );
}

/// `execution_not_retryable` — a succeeded execution has no failed-node
/// list, and Rundeck retries only executions that do.
#[tokio::test]
async fn job_retry_rejects_execution_not_retryable() {
    let f = fixtures().await;
    let prior = succeeded_execution().await;
    let id = promise_id("retry-noretry");
    let p = promise(
        &id,
        "job.retry",
        json!({"id": f.ok, "executionId": prior}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(
        value,
        json!({
            "code": "execution_not_retryable",
            "detail": format!("Failed node List for execution ID does not exist: {prior}"),
        })
    );
}

/// `invalid_request` — `executionId` is required by the Param schema.
#[tokio::test]
async fn job_retry_rejects_invalid_request() {
    let f = fixtures().await;
    let id = promise_id("retry-bad");
    let p = promise(&id, "job.retry", json!({"id": f.fail}), in_ms(60_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("executionId"),
        "{value}"
    );
}

/// `execution_deleted` — the new execution is removed before the poll loop
/// sees it finish.
#[tokio::test]
async fn job_retry_rejects_execution_deleted() {
    let f = fixtures().await;
    let prior = failed_execution(&f.fail).await;
    let id = promise_id("retry-deleted");
    let p = promise(
        &id,
        "job.retry",
        json!({"id": f.fail, "executionId": prior}),
        in_ms(300_000),
    );
    let mut cfg = config();
    cfg.poll_job_retry = Some(Duration::from_secs(25));

    let deleter = async {
        let execution = await_stamped(&id).await;
        await_terminal(execution).await;
        assert_eq!(delete_execution(execution).await, 204);
    };
    let (verdict, ()) = tokio::join!(plugin::process(&cfg, &p), deleter);
    let value = rejected(verdict);

    assert_eq!(value, json!({"code": "execution_deleted"}));
}

/// halt — the token is rejected.
#[tokio::test]
async fn job_retry_halts_on_a_rejected_token() {
    let f = fixtures().await;
    let id = promise_id("retry-halt");
    let p = promise(
        &id,
        "job.retry",
        json!({"id": f.fail, "executionId": 1}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("unauthorized"), "{reason}");
}

/// The deadline — `timeout_at` already in the past.
#[tokio::test]
async fn job_retry_releases_at_the_deadline() {
    let f = fixtures().await;
    let prior = failed_execution(&f.fail).await;
    let id = promise_id("retry-deadline");
    let p = promise(
        &id,
        "job.retry",
        json!({"id": f.fail, "executionId": prior}),
        in_ms(-1_000),
    );

    let reason = released(plugin::process(&config(), &p).await);

    assert_eq!(reason, "promise timed out");
    // The retry was started before the deadline was observed.
    assert_eq!(await_stamped_all(&id).await.len(), 1);
}

/// Re-entry — a redelivery re-finds the retry an earlier attempt started.
#[tokio::test]
async fn job_retry_refinds_its_execution_on_re_entry() {
    let f = fixtures().await;
    let prior = failed_execution(&f.cond).await;
    let id = promise_id("retry-reentry");
    let p = promise(
        &id,
        "job.retry",
        json!({"id": f.cond, "executionId": prior, "options": {"pass": "yes"}}),
        in_ms(300_000),
    );

    let first = resolved(plugin::process(&config(), &p).await);
    let second = resolved(plugin::process(&config(), &p).await);

    assert_eq!(first["id"], second["id"]);
    assert_eq!(await_stamped_all(&id).await.len(), 1);
}

// ─── execution.get (§4.4) ─────────────────────────────────────────────────────

/// resolved — = response.body.
#[tokio::test]
async fn execution_get_resolves_with_the_execution() {
    let f = fixtures().await;
    let execution = succeeded_execution().await;
    let p = promise(
        &promise_id("exec-get"),
        "execution.get",
        json!({"id": execution}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["id"], json!(execution));
    assert_eq!(value["status"], "succeeded");
    assert_eq!(value["project"], f.project);
    assert_eq!(value["job"]["id"], f.ok);
}

/// `not_found` — a nonexistent execution id.
#[tokio::test]
async fn execution_get_rejects_not_found() {
    let p = promise(
        &promise_id("exec-get-404"),
        "execution.get",
        json!({"id": NO_EXECUTION}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(
        value,
        json!({
            "code": "not_found",
            "detail": format!("Execution does not exist: {NO_EXECUTION}"),
        })
    );
}

/// `invalid_request` — `id` is required by the Param schema.
#[tokio::test]
async fn execution_get_rejects_invalid_request() {
    let p = promise(
        &promise_id("exec-get-bad"),
        "execution.get",
        json!({}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("args.id"),
        "{value}"
    );
}

/// halt — the token is rejected.
#[tokio::test]
async fn execution_get_halts_on_a_rejected_token() {
    let p = promise(
        &promise_id("exec-get-halt"),
        "execution.get",
        json!({"id": 1}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("unauthorized"), "{reason}");
}

// ─── execution.list (§4.5) ────────────────────────────────────────────────────

/// resolved — = response.body, one page of the project's executions.
#[tokio::test]
async fn execution_list_resolves_with_the_page() {
    let f = fixtures().await;
    let execution = succeeded_execution().await;
    let p = promise(
        &promise_id("exec-list"),
        "execution.list",
        json!({
            "project": f.project,
            "jobIdListFilter": [f.ok],
            "statusFilter": "succeeded",
            "max": 20,
        }),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["paging"]["max"], 20);
    assert_eq!(value["paging"]["offset"], 0);
    let executions = value["executions"].as_array().cloned().unwrap_or_default();
    assert!(!executions.is_empty(), "{value}");
    for e in &executions {
        assert_eq!(e["status"], "succeeded");
        assert_eq!(e["job"]["id"], f.ok);
    }
    assert!(
        executions.iter().any(|e| e["id"] == json!(execution)),
        "{execution} missing from {value}"
    );
}

/// resolved — project "*" is the cross-project running list, a different
/// endpoint answering the same body.
#[tokio::test]
async fn execution_list_resolves_with_the_running_list() {
    let f = fixtures().await;
    let running = run_job(&f.slow, json!({})).await;
    let p = promise(
        &promise_id("exec-list-running"),
        "execution.list",
        json!({"project": "*", "max": 20}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    let executions = value["executions"].as_array().cloned().unwrap_or_default();
    assert!(
        executions.iter().any(|e| e["id"] == json!(running)),
        "{running} missing from {value}"
    );
    assert!(value["paging"]["total"].is_number(), "{value}");

    abort_execution(running).await;
}

/// `not_found` — a nonexistent project.
#[tokio::test]
async fn execution_list_rejects_not_found() {
    let p = promise(
        &promise_id("exec-list-404"),
        "execution.list",
        json!({"project": "no-such-project-e0f1"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(
        value,
        json!({
            "code": "not_found",
            "detail": "Project does not exist: no-such-project-e0f1",
        })
    );
}

/// `invalid_request` — `begin` is documented as a unix millisecond timestamp
/// or a W3C dateTime, and Rundeck refuses the request when it is neither.
#[tokio::test]
async fn execution_list_rejects_invalid_request() {
    let f = fixtures().await;
    let p = promise(
        &promise_id("exec-list-bad"),
        "execution.list",
        json!({"project": f.project, "begin": "garbage"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("did not have a valid time or dateTime format"),
        "{value}"
    );
}

/// halt — the token is rejected.
#[tokio::test]
async fn execution_list_halts_on_a_rejected_token() {
    let f = fixtures().await;
    let p = promise(
        &promise_id("exec-list-halt"),
        "execution.list",
        json!({"project": f.project}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("unauthorized"), "{reason}");
}

// ─── execution.output (§4.6) ──────────────────────────────────────────────────

/// resolved — = response.body, one window of the log.
#[tokio::test]
async fn execution_output_resolves_with_the_log_window() {
    let execution = succeeded_execution().await;
    let p = promise(
        &promise_id("exec-output"),
        "execution.output",
        json!({"id": execution, "maxlines": 10}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["id"], execution.to_string());
    assert_eq!(value["execCompleted"], true);
    assert_eq!(value["execState"], "succeeded");
    assert!(value["offset"].is_string(), "{value}");
    let entries = value["entries"].as_array().cloned().unwrap_or_default();
    assert!(
        entries.iter().any(|e| e["log"] == "ok"),
        "the job's own output is missing: {value}"
    );
}

/// `not_found` — a nonexistent execution id.
#[tokio::test]
async fn execution_output_rejects_not_found() {
    let p = promise(
        &promise_id("exec-output-404"),
        "execution.output",
        json!({"id": NO_EXECUTION}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(
        value,
        json!({
            "code": "not_found",
            "detail": format!("Execution does not exist: {NO_EXECUTION}"),
        })
    );
}

/// `invalid_request` — `id` is required by the Param schema.
#[tokio::test]
async fn execution_output_rejects_invalid_request() {
    let p = promise(
        &promise_id("exec-output-bad"),
        "execution.output",
        json!({"maxlines": 10}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("args.id"),
        "{value}"
    );
}

/// halt — the token is rejected.
#[tokio::test]
async fn execution_output_halts_on_a_rejected_token() {
    let p = promise(
        &promise_id("exec-output-halt"),
        "execution.output",
        json!({"id": 1}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("unauthorized"), "{reason}");
}

// ─── execution.state (§4.7) ───────────────────────────────────────────────────

/// resolved — = response.body, the workflow state.
#[tokio::test]
async fn execution_state_resolves_with_the_workflow_state() {
    let execution = succeeded_execution().await;
    let p = promise(
        &promise_id("exec-state"),
        "execution.state",
        json!({"id": execution}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["executionId"], json!(execution));
    assert_eq!(value["executionState"], "SUCCEEDED");
    assert_eq!(value["completed"], true);
    assert_eq!(value["stepCount"], 2);
    let steps = value["steps"].as_array().cloned().unwrap_or_default();
    assert_eq!(steps.len(), 2, "{value}");
    assert_eq!(steps[0]["executionState"], "SUCCEEDED");
}

/// `not_found` — a nonexistent execution id.
#[tokio::test]
async fn execution_state_rejects_not_found() {
    let p = promise(
        &promise_id("exec-state-404"),
        "execution.state",
        json!({"id": NO_EXECUTION}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "not_found");
    assert_eq!(
        value["detail"],
        format!("Execution does not exist: {NO_EXECUTION}")
    );
}

/// `invalid_request` — `id` is required by the Param schema.
#[tokio::test]
async fn execution_state_rejects_invalid_request() {
    let p = promise(
        &promise_id("exec-state-bad"),
        "execution.state",
        json!({}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("args.id"),
        "{value}"
    );
}

/// halt — the token is rejected.
#[tokio::test]
async fn execution_state_halts_on_a_rejected_token() {
    let p = promise(
        &promise_id("exec-state-halt"),
        "execution.state",
        json!({"id": 1}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("unauthorized"), "{reason}");
}

// ─── execution.abort (§4.8) ───────────────────────────────────────────────────

/// resolved — the abort is accepted and the execution is watched until it
/// has actually stopped.
#[tokio::test]
async fn execution_abort_resolves_once_the_execution_stops() {
    let f = fixtures().await;
    let execution = run_job(&f.slow, json!({})).await;
    let p = promise(
        &promise_id("abort-ok"),
        "execution.abort",
        json!({"id": execution}),
        in_ms(120_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["id"], json!(execution));
    assert_eq!(value["status"], "aborted");
    assert!(
        value["date-ended"].is_object(),
        "not watched to its end: {value}"
    );
    // The §4.1.2 Resolved schema is exactly these sixteen keys.
    assert_eq!(value.as_object().map(|o| o.len()), Some(16), "{value}");
}

/// resolved on an execution that already stopped — the repeated abort
/// Rundeck answers "failed" / "Job is not running" to, which is not a
/// refusal because the execution is terminal. Re-entry, in other words.
#[tokio::test]
async fn execution_abort_resolves_on_a_stopped_execution() {
    let execution = succeeded_execution().await;
    let p = promise(
        &promise_id("abort-again"),
        "execution.abort",
        json!({"id": execution}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["id"], json!(execution));
    // abort does not judge which terminal status the execution reached.
    assert_eq!(value["status"], "succeeded");
}

/// `not_found` — a nonexistent execution id.
#[tokio::test]
async fn execution_abort_rejects_not_found() {
    let p = promise(
        &promise_id("abort-404"),
        "execution.abort",
        json!({"id": NO_EXECUTION}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(
        value,
        json!({
            "code": "not_found",
            "detail": format!("Execution does not exist: {NO_EXECUTION}"),
        })
    );
}

/// `invalid_request` — `id` is required by the Param schema.
#[tokio::test]
async fn execution_abort_rejects_invalid_request() {
    let p = promise(
        &promise_id("abort-bad"),
        "execution.abort",
        json!({"forceIncomplete": true}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("args.id"),
        "{value}"
    );
}

/// `deleted` — the execution is removed before the abort's poll loop sees it
/// stop.
#[tokio::test]
async fn execution_abort_rejects_deleted() {
    let f = fixtures().await;
    let execution = run_job(&f.slow, json!({})).await;
    let p = promise(
        &promise_id("abort-deleted"),
        "execution.abort",
        json!({"id": execution}),
        in_ms(120_000),
    );
    let mut cfg = config();
    cfg.poll_execution_abort = Some(Duration::from_secs(25));

    let deleter = async {
        await_terminal(execution).await;
        assert_eq!(delete_execution(execution).await, 204);
    };
    let (verdict, ()) = tokio::join!(plugin::process(&cfg, &p), deleter);
    let value = rejected(verdict);

    // detail: absent.
    assert_eq!(value, json!({"code": "deleted"}));
}

/// halt — the token is rejected.
#[tokio::test]
async fn execution_abort_halts_on_a_rejected_token() {
    let p = promise(
        &promise_id("abort-halt"),
        "execution.abort",
        json!({"id": 1}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("unauthorized"), "{reason}");
}

/// The deadline — `timeout_at` already in the past.
#[tokio::test]
async fn execution_abort_releases_at_the_deadline() {
    let f = fixtures().await;
    let execution = run_job(&f.slow, json!({})).await;
    let p = promise(
        &promise_id("abort-deadline"),
        "execution.abort",
        json!({"id": execution}),
        in_ms(-1_000),
    );

    let reason = released(plugin::process(&config(), &p).await);

    assert_eq!(reason, "promise timed out");
    // The abort was sent before the deadline was observed.
    assert_eq!(await_terminal(execution).await["status"], "aborted");
}

// ─── execution.delete (§4.9) ──────────────────────────────────────────────────

/// resolved — 204 No Content, an empty value.
#[tokio::test]
async fn execution_delete_resolves_empty() {
    let execution = a_succeeded_execution().await;
    let p = promise(
        &promise_id("delete-ok"),
        "execution.delete",
        json!({"id": execution}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({}));
    assert_eq!(execution_status(execution).await, None);
}

/// `not_found` on re-entry — a redelivery after a delete that landed sees
/// the same 404 a wrong id sees, because Rundeck keeps no tombstone that
/// would separate them.
#[tokio::test]
async fn execution_delete_rejects_not_found_on_re_entry() {
    let execution = a_succeeded_execution().await;
    let id = promise_id("delete-reentry");
    let p = promise(
        &id,
        "execution.delete",
        json!({"id": execution}),
        in_ms(60_000),
    );

    let first = resolved(plugin::process(&config(), &p).await);
    let second = rejected(plugin::process(&config(), &p).await);

    assert_eq!(first, json!({}));
    assert_eq!(
        second,
        json!({
            "code": "not_found",
            "detail": format!("Execution does not exist: {execution}"),
        })
    );
}

/// `not_found` — an execution id that never existed.
#[tokio::test]
async fn execution_delete_rejects_not_found() {
    let p = promise(
        &promise_id("delete-404"),
        "execution.delete",
        json!({"id": NO_EXECUTION}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(
        value,
        json!({
            "code": "not_found",
            "detail": format!("Execution does not exist: {NO_EXECUTION}"),
        })
    );
}

/// `invalid_request` — `id` is required by the Param schema.
#[tokio::test]
async fn execution_delete_rejects_invalid_request() {
    let p = promise(
        &promise_id("delete-bad"),
        "execution.delete",
        json!({}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("args.id"),
        "{value}"
    );
}

/// halt — the token is rejected.
#[tokio::test]
async fn execution_delete_halts_on_a_rejected_token() {
    let p = promise(
        &promise_id("delete-halt"),
        "execution.delete",
        json!({"id": NO_EXECUTION}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("unauthorized"), "{reason}");
}

// ─── job.get (§4.10) ──────────────────────────────────────────────────────────

/// resolved — = response.body[0], the single job Rundeck wraps in an array.
#[tokio::test]
async fn job_get_resolves_with_the_job_definition() {
    let f = fixtures().await;
    let p = promise(
        &promise_id("job-get"),
        "job.get",
        json!({"id": f.ok}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    // The array wrapper is gone: this is the job object itself.
    assert_eq!(value["uuid"], f.ok);
    assert_eq!(value["name"], "fixture-ok");
    assert_eq!(value["executionEnabled"], true);
    let options = value["options"].as_array().cloned().unwrap_or_default();
    assert_eq!(options.len(), 1, "{value}");
    assert_eq!(options[0]["name"], "seconds");
    assert!(value["sequence"]["commands"].is_array(), "{value}");
}

/// `not_found` — a nonexistent job UUID.
#[tokio::test]
async fn job_get_rejects_not_found() {
    let p = promise(
        &promise_id("job-get-404"),
        "job.get",
        json!({"id": NO_JOB}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(
        value,
        json!({
            "code": "not_found",
            "detail": format!("Job ID does not exist: {NO_JOB}"),
        })
    );
}

/// `invalid_request` — `id` is required by the Param schema.
#[tokio::test]
async fn job_get_rejects_invalid_request() {
    let p = promise(
        &promise_id("job-get-bad"),
        "job.get",
        json!({}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("args.id"),
        "{value}"
    );
}

/// halt — the token is rejected.
#[tokio::test]
async fn job_get_halts_on_a_rejected_token() {
    let f = fixtures().await;
    let p = promise(
        &promise_id("job-get-halt"),
        "job.get",
        json!({"id": f.ok}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("unauthorized"), "{reason}");
}

// ─── job.list (§4.11) ─────────────────────────────────────────────────────────

/// resolved — = response.body, the project's jobs.
#[tokio::test]
async fn job_list_resolves_with_the_jobs() {
    let f = fixtures().await;
    let p = promise(
        &promise_id("job-list"),
        "job.list",
        json!({"project": f.project, "jobExactFilter": "fixture-ok"}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    let jobs = value.as_array().cloned().unwrap_or_default();
    assert_eq!(jobs.len(), 1, "{value}");
    assert_eq!(jobs[0]["id"], f.ok);
    assert_eq!(jobs[0]["name"], "fixture-ok");
    assert_eq!(jobs[0]["project"], f.project);
}

/// `not_found` — a nonexistent project.
#[tokio::test]
async fn job_list_rejects_not_found() {
    let p = promise(
        &promise_id("job-list-404"),
        "job.list",
        json!({"project": "no-such-project-e0f1"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(
        value,
        json!({
            "code": "not_found",
            "detail": "Project does not exist: no-such-project-e0f1",
        })
    );
}

/// `invalid_request` — `project` is required by the Param schema.
#[tokio::test]
async fn job_list_rejects_invalid_request() {
    let p = promise(
        &promise_id("job-list-bad"),
        "job.list",
        json!({"max": 5}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("args.project"),
        "{value}"
    );
}

/// halt — the token is rejected.
#[tokio::test]
async fn job_list_halts_on_a_rejected_token() {
    let f = fixtures().await;
    let p = promise(
        &promise_id("job-list-halt"),
        "job.list",
        json!({"project": f.project}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("unauthorized"), "{reason}");
}

// ─── project.list (§4.12) ─────────────────────────────────────────────────────

/// resolved — = response.body, the projects on the server.
#[tokio::test]
async fn project_list_resolves_with_the_projects() {
    let f = fixtures().await;
    let p = promise(
        &promise_id("project-list"),
        "project.list",
        json!({}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    let projects = value.as_array().cloned().unwrap_or_default();
    assert!(
        projects.iter().any(|p| p["name"] == json!(f.project)),
        "{value}"
    );
    for p in &projects {
        assert!(p["url"].is_string(), "{p}");
    }
}

/// resolved with `meta` — the one parameter the operation takes.
#[tokio::test]
async fn project_list_resolves_with_metadata() {
    let p = promise(
        &promise_id("project-list-meta"),
        "project.list",
        json!({"meta": "*"}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    let projects = value.as_array().cloned().unwrap_or_default();
    assert!(!projects.is_empty(), "{value}");
    assert!(projects[0]["meta"].is_array(), "{value}");
}

/// halt — the token is rejected. §3's probe, in other words.
#[tokio::test]
async fn project_list_halts_on_a_rejected_token() {
    let p = promise(
        &promise_id("project-list-halt"),
        "project.list",
        json!({}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("unauthorized"), "{reason}");
}

// ─── The frame's own rejections ───────────────────────────────────────────────

/// `unknown_func` — a func no operation answers to.
#[tokio::test]
async fn an_unknown_func_is_rejected() {
    let p = promise(
        &promise_id("unknown"),
        "job.explode",
        json!({}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(
        value,
        json!({"code": "unknown_func", "detail": "job.explode"})
    );
}

/// `invalid_request` — a param that carries no func at all.
#[tokio::test]
async fn a_param_without_a_func_is_rejected() {
    let mut p = promise(&promise_id("nofunc"), "job.run", json!({}), in_ms(60_000));
    p.param.data = Some(b64(&json!({"args": {}}).to_string()));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(
        value,
        json!({"code": "invalid_request", "detail": "param has no func"})
    );
}

// ─── Config (§2, from the environment §5.2 exports) ───────────────────────────

/// `poll` is 1s — the test default. An individual test widens the cadence it
/// needs a window inside.
fn config() -> Config {
    Config {
        base_url: env("RUNDECK_BASE_URL"),
        api_token: env("RUNDECK_API_TOKEN"),
        poll: Duration::from_secs(1),
        poll_job_run: None,
        poll_job_retry: None,
        poll_execution_abort: None,
    }
}

/// The halt induction: a token Rundeck does not know.
fn bad_credential() -> Config {
    Config {
        api_token: "not-a-real-rundeck-token".to_string(),
        ..config()
    }
}

fn env(key: &str) -> String {
    std::env::var(key)
        .unwrap_or_else(|_| panic!("{key} is unset — run specification §5.1 and §5.2 first"))
}

// ─── Promises ─────────────────────────────────────────────────────────────────

fn promise(id: &str, func: &str, args: Value, timeout_at: i64) -> PromiseRecord {
    let param = json!({ "func": func, "args": args }).to_string();
    PromiseRecord {
        id: id.to_string(),
        state: PromiseState::Pending,
        param: PromiseValue {
            headers: None,
            data: Some(b64(&param)),
        },
        value: PromiseValue::default(),
        tags: HashMap::new(),
        timeout_at,
        created_at: now_ms(),
        settled_at: None,
    }
}

fn b64(s: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(s)
}

/// A fresh promise id per test — and one that survives the frame's sanitize
/// unchanged, so the `rescorr` stamp it becomes has it as a prefix and the
/// partial-match `optionFilter` finds it.
fn promise_id(what: &str) -> String {
    format!("rundeck.{what}.{}", nanos())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn in_ms(delta: i64) -> i64 {
    now_ms() + delta
}

/// An ISO-8601 instant `delta` milliseconds from now, in the form
/// `runAtTime` documents.
fn in_iso8601(delta: i64) -> String {
    let secs = (now_ms() + delta) / 1000;
    let days = secs.div_euclid(86_400);
    let time = secs.rem_euclid(86_400);
    // Civil date from the unix day number.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = era * 400 + yoe + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}+0000",
        time / 3_600,
        (time % 3_600) / 60,
        time % 60
    )
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

/// The jobs the inductions need. §5.2 provisions `fixture-ok` and
/// `fixture-fail` and exports their UUIDs; the rest are imported here, once
/// per test binary, because four of the six terminal statuses §4.1.4
/// enumerates have no job in §5 that reaches them.
struct Fixtures {
    project: String,
    ok: String,
    fail: String,
    retry: String,
    timeout: String,
    other: String,
    slow: String,
    cond: String,
}

async fn fixtures() -> &'static Fixtures {
    static FIXTURES: tokio::sync::OnceCell<Fixtures> = tokio::sync::OnceCell::const_new();
    FIXTURES.get_or_init(provision).await
}

async fn provision() -> Fixtures {
    let ok = env("RUNDECK_FIXTURE_OK");
    let fail = env("RUNDECK_FIXTURE_FAIL");
    // The project the §5 fixtures live in, read rather than assumed.
    let (status, info) = get(&format!("job/{ok}/info")).await;
    assert_eq!(status, 200, "reading the fixture job: {info}");
    let project = info["project"]
        .as_str()
        .expect("the fixture job names a project")
        .to_string();

    let yaml = format!(
        r#"
- name: fixture-retry
  group: ''
  project: {project}
  description: exits non-zero, and Rundeck starts one retry
  loglevel: INFO
  executionEnabled: true
  multipleExecutions: true
  retry: '1'
  sequence:
    keepgoing: false
    strategy: node-first
    commands:
      - exec: /bin/false
- name: fixture-timeout
  group: ''
  project: {project}
  description: sleeps past its own job timeout
  loglevel: INFO
  executionEnabled: true
  multipleExecutions: true
  timeout: '5s'
  sequence:
    keepgoing: false
    strategy: node-first
    commands:
      - exec: sleep 120
- name: fixture-other
  group: ''
  project: {project}
  description: halts with a custom exit status
  loglevel: INFO
  executionEnabled: true
  multipleExecutions: true
  sequence:
    keepgoing: false
    strategy: node-first
    commands:
      - configuration:
          halt: 'true'
          status: custom-status
        nodeStep: false
        type: flow-control
- name: fixture-slow
  group: ''
  project: {project}
  description: sleeps long enough to be aborted or watched
  loglevel: INFO
  executionEnabled: true
  multipleExecutions: true
  sequence:
    keepgoing: false
    strategy: node-first
    commands:
      - exec: sleep 600
- name: fixture-cond
  group: ''
  project: {project}
  description: fails while the pass option is empty, succeeds once it is set
  loglevel: INFO
  executionEnabled: true
  multipleExecutions: true
  options:
    - name: pass
      value: ''
  sequence:
    keepgoing: false
    strategy: node-first
    commands:
      - exec: sh -c 'test -n "${{option.pass}}"'
"#
    );
    let (status, body) = request(
        http()
            .post(format!(
                "{}/api/59/project/{project}/jobs/import?dupeOption=update",
                env("RUNDECK_BASE_URL")
            ))
            .header("Content-Type", "application/yaml")
            .body(yaml),
    )
    .await;
    assert_eq!(status, 200, "importing the extra fixtures: {body}");
    assert_eq!(body["failed"], json!([]), "importing the extra fixtures");

    let imported = |name: &str| -> String {
        body["succeeded"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|j| j["name"] == json!(name))
            .and_then(|j| j["id"].as_str())
            .unwrap_or_else(|| panic!("{name} was not imported: {body}"))
            .to_string()
    };
    Fixtures {
        retry: imported("fixture-retry"),
        timeout: imported("fixture-timeout"),
        other: imported("fixture-other"),
        slow: imported("fixture-slow"),
        cond: imported("fixture-cond"),
        project,
        ok,
        fail,
    }
}

/// One client for the whole test binary: a client per call would open a new
/// connection pool each time.
fn http() -> reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new).clone()
}

async fn request(req: reqwest::RequestBuilder) -> (u16, Value) {
    let response = req
        .header("X-Rundeck-Auth-Token", env("RUNDECK_API_TOKEN"))
        .header("Accept", "application/json")
        .send()
        .await
        .expect("the provider answered");
    let status = response.status().as_u16();
    let text = response.text().await.expect("a response body");
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

async fn get(path: &str) -> (u16, Value) {
    request(http().get(format!("{}/api/59/{path}", env("RUNDECK_BASE_URL")))).await
}

/// Start one execution of a job and return its id.
async fn run_job(job: &str, body: Value) -> i64 {
    let (status, response) = request(
        http()
            .post(format!("{}/api/59/job/{job}/run", env("RUNDECK_BASE_URL")))
            .header("Content-Type", "application/json")
            .json(&body),
    )
    .await;
    assert_eq!(status, 200, "running {job}: {response}");
    response["id"].as_i64().expect("an execution id")
}

/// The status of one execution, or `None` once it has been deleted.
async fn execution_status(execution: i64) -> Option<String> {
    let (status, body) = get(&format!("execution/{execution}")).await;
    if status == 404 {
        return None;
    }
    assert_eq!(status, 200, "reading execution {execution}: {body}");
    Some(body["status"].as_str().unwrap_or_default().to_string())
}

/// Wait for one execution to reach a terminal status, and return its record.
async fn await_terminal(execution: i64) -> Value {
    for _ in 0..300 {
        let (status, body) = get(&format!("execution/{execution}")).await;
        assert_eq!(status, 200, "reading execution {execution}: {body}");
        let terminal = matches!(
            body["status"].as_str().unwrap_or_default(),
            "succeeded" | "failed" | "failed-with-retry" | "aborted" | "timedout" | "other"
        );
        if terminal {
            return body;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    panic!("execution {execution} never finished");
}

/// The executions a promise stamped, found the way the plugin finds them:
/// `optionFilter` is a partial match over the option values, and the stamp
/// starts with the promise id.
async fn await_stamped_all(promise_id: &str) -> Vec<i64> {
    let project = &fixtures().await.project;
    let (status, body) = request(
        http()
            .get(format!(
                "{}/api/59/project/{project}/executions",
                env("RUNDECK_BASE_URL")
            ))
            .query(&[
                ("optionFilter", format!("-rescorr {promise_id}")),
                ("max", "20".to_string()),
            ]),
    )
    .await;
    assert_eq!(status, 200, "searching for {promise_id}: {body}");
    body["executions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| e["id"].as_i64())
        .collect()
}

/// Wait for the execution a promise created to exist, and return its id.
async fn await_stamped(promise_id: &str) -> i64 {
    for _ in 0..120 {
        if let Some(id) = await_stamped_all(promise_id).await.first() {
            return *id;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    panic!("no execution stamped {promise_id} appeared");
}

/// One finished, successful execution to read from — shared across the read
/// operations' tests, so they do not each pay for a run of their own.
async fn succeeded_execution() -> i64 {
    static EXECUTION: tokio::sync::OnceCell<i64> = tokio::sync::OnceCell::const_new();
    *EXECUTION.get_or_init(a_succeeded_execution).await
}

/// A finished, successful execution nothing else holds a reference to — for
/// the tests that delete one.
async fn a_succeeded_execution() -> i64 {
    let f = fixtures().await;
    let execution = run_job(&f.ok, json!({})).await;
    let terminal = await_terminal(execution).await;
    assert_eq!(terminal["status"], "succeeded", "{terminal}");
    execution
}

/// A finished, failed execution, which is what a retry needs: Rundeck
/// retries an execution only while a failed-node list exists for it.
async fn failed_execution(job: &str) -> i64 {
    let execution = run_job(job, json!({})).await;
    let terminal = await_terminal(execution).await;
    assert_eq!(terminal["status"], "failed", "{terminal}");
    assert!(
        terminal["failedNodes"].is_array(),
        "a retryable execution needs a failed-node list: {terminal}"
    );
    execution
}

async fn abort_execution(execution: i64) {
    let (status, body) = request(http().post(format!(
        "{}/api/59/execution/{execution}/abort",
        env("RUNDECK_BASE_URL")
    )))
    .await;
    assert_eq!(status, 200, "aborting {execution}: {body}");
}

async fn delete_execution(execution: i64) -> u16 {
    request(http().delete(format!(
        "{}/api/59/execution/{execution}",
        env("RUNDECK_BASE_URL")
    )))
    .await
    .0
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}
