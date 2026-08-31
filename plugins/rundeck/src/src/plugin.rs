//! The rundeck plugin. Everything provider-specific lives here, translated
//! from plugins/rundeck/spec/specification.md.

use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;
use std::time::Duration;

use resonate_core::types::PromiseRecord;

use crate::worker::{b64_decode, sanitize};

/// Specification §2, translated: bare field = required (a serde default
/// satisfies it), Option<T> = optional, `= poll` cascades via accessor.
#[derive(Clone, Deserialize)]
pub struct Config {
    pub base_url: String,
    pub api_token: String,
    #[serde(default = "poll_default", with = "humantime_serde")]
    pub poll: Duration,
    #[serde(default, with = "humantime_serde")]
    pub poll_job_run: Option<Duration>,
    #[serde(default, with = "humantime_serde")]
    pub poll_job_retry: Option<Duration>,
    #[serde(default, with = "humantime_serde")]
    pub poll_execution_abort: Option<Duration>,
}

fn poll_default() -> Duration {
    Duration::from_secs(5)
}

impl Config {
    /// `= poll`: a per-operation cadence falls back to the shared one.
    fn poll_job_run(&self) -> Duration {
        self.poll_job_run.unwrap_or(self.poll)
    }

    fn poll_job_retry(&self) -> Duration {
        self.poll_job_retry.unwrap_or(self.poll)
    }

    fn poll_execution_abort(&self) -> Duration {
        self.poll_execution_abort.unwrap_or(self.poll)
    }
}

/// The verdict vocabulary of [`process`], named once so the helpers below can
/// speak it too.
///
///   Ok(Ok(json))     -> resolve
///   Ok(Err(json))    -> reject
///   Err(Ok(reason))  -> halt
///   Err(Err(reason)) -> release
type Verdict = Result<Result<String, String>, Result<String, String>>;

/// A failure that is not a verdict on the promise: halt or release. Every
/// helper returns this on its error channel so `?` propagates it unchanged.
type Failure = Result<String, String>;

/// The job option every execution this plugin creates is stamped with, so a
/// redelivery re-finds that execution rather than starting a second one.
const CORR: &str = "rescorr";

/// §4.1.4: the statuses an execution never leaves.
const TERMINAL: [&str; 6] = [
    "succeeded",
    "failed",
    "failed-with-retry",
    "aborted",
    "timedout",
    "other",
];

/// The §4.1.2 Resolved mapping: the execution record's keys, projected.
const RESULT: [&str; 16] = [
    "id",
    "href",
    "permalink",
    "status",
    "customStatus",
    "project",
    "executionType",
    "user",
    "date-started",
    "date-ended",
    "job",
    "description",
    "argstring",
    "successfulNodes",
    "failedNodes",
    "retriedExecution",
];

/// One call per delivered task. Complete the operation: begin, poll to
/// its terminal state, decide.
///
///   Ok(Ok(json))     -> resolve, json matches the op's Resolved schema
///   Ok(Err(json))    -> reject, json matches the op's Rejected schema
///                       (documented-permanent failures only)
///   Err(Ok(reason))  -> halt: redelivery pauses until an operator
///                       intervenes (task.halt). Only for conditions the
///                       provider positively reports as operator-required.
///                       A halted promise still times out.
///   Err(Err(reason)) -> release: no verdict, the task is dropped
///                       (task.release) and the message is redelivered;
///                       re-entry must be safe. Also the deadline path —
///                       the server settles a timed-out promise itself.
///
/// Loops are bounded by promise.timeout_at (milliseconds since epoch) and
/// never sleep past it. Injected identity values are
/// sanitize(&promise.id), never the raw id.
pub async fn process(config: &Config, promise: &PromiseRecord) -> Verdict {
    let param = match decode_param(promise) {
        Ok(v) => v,
        Err(e) => {
            let detail = json!({"code": "invalid_request", "detail": e});
            return Ok(Err(detail.to_string()));
        }
    };
    let Some(func) = param.get("func").and_then(Value::as_str) else {
        let detail = json!({"code": "invalid_request", "detail": "param has no func"});
        return Ok(Err(detail.to_string()));
    };
    let args = param.get("args").cloned().unwrap_or_else(|| json!({}));

    match func {
        "job.run" => job_run(config, promise, &args).await,
        "job.submit" => job_submit(config, promise, &args).await,
        "job.retry" => job_retry(config, promise, &args).await,
        "execution.get" => execution_get(config, &args).await,
        "execution.list" => execution_list(config, &args).await,
        "execution.output" => execution_output(config, &args).await,
        "execution.state" => execution_state(config, &args).await,
        "execution.abort" => execution_abort(config, promise, &args).await,
        "execution.delete" => execution_delete(config, &args).await,
        "job.get" => job_get(config, &args).await,
        "job.list" => job_list(config, &args).await,
        "project.list" => project_list(config, &args).await,
        _ => Ok(Err(json!({"code": "unknown_func", "detail": func}).to_string())),
    }
}

/// promise.param.data is base64 UTF-8 JSON.
fn decode_param(promise: &PromiseRecord) -> Result<Value, String> {
    let data = promise.param.data.as_deref().ok_or("param has no data")?;
    let bytes = b64_decode(data).ok_or("param.data is not base64")?;
    serde_json::from_slice(&bytes).map_err(|e| format!("param: {e}"))
}

// ─── Operations ───────────────────────────────────────────────────────────────

/// Specification §4.1, translated from its Python. Begin idempotently
/// (duplicate = re-attach where the provider supports it), then poll on
/// the downstream clock — the worker frame heartbeats the lease
/// independently, so this cadence may back off freely.
async fn job_run(cfg: &Config, promise: &PromiseRecord, args: &Value) -> Verdict {
    let api = api(cfg);
    let job_id = match arg_str(args, "id") {
        Ok(v) => quote(v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };
    let token = sanitize(&promise.id);

    // The stamp is only searchable within a project, and /job/[ID]/info is
    // the cheapest read that names one. It answers XML unless asked for JSON.
    let r = send(
        client()
            .get(format!("{api}/job/{job_id}/info"))
            .header(AUTH, &cfg.api_token)
            .header("Accept", "application/json"),
    )
    .await?;
    check(&r)?;
    if r.status == 404 {
        return reject("job_not_found", Some(message(&r)));
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(json!(r.text)));
    }

    let project = string(r.json().get("project"));
    let execution = match find(cfg, &project, &token).await? {
        Some(e) => e,
        None => {
            let body = stamp(args, &token, &["loglevel", "asUser", "filter", "runAtTime"]);
            let r = send(
                client()
                    .post(format!("{api}/job/{job_id}/run"))
                    .header(AUTH, &cfg.api_token)
                    .header("Content-Type", "application/json")
                    .json(&body),
            )
            .await?;
            check(&r)?;
            if r.status == 409 {
                // api.error.execution.conflict: the job's own limit on concurrent
                // executions. It clears when the execution now running ends.
                return Err(Err(r.text));
            }
            if r.status == 404 {
                return reject("job_not_found", Some(message(&r)));
            }
            if r.status >= 400 {
                return reject("invalid_request", Some(json!(r.text)));
            }
            r.json()
        }
    };

    let id = execution.get("id").cloned().unwrap_or(Value::Null);
    match watch(cfg, &id, cfg.poll_job_run(), promise).await? {
        Some(execution) => settle(&execution),
        None => reject("execution_deleted", None),
    }
}

/// Specification §4.2, translated from its Python.
async fn job_submit(cfg: &Config, promise: &PromiseRecord, args: &Value) -> Verdict {
    let api = api(cfg);
    let job_id = match arg_str(args, "id") {
        Ok(v) => quote(v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };
    let token = sanitize(&promise.id);

    let r = send(
        client()
            .get(format!("{api}/job/{job_id}/info"))
            .header(AUTH, &cfg.api_token)
            .header("Accept", "application/json"),
    )
    .await?;
    check(&r)?;
    if r.status == 404 {
        return reject("job_not_found", Some(message(&r)));
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(json!(r.text)));
    }

    let project = string(r.json().get("project"));
    if let Some(execution) = find(cfg, &project, &token).await? {
        return Ok(Ok(execution.to_string()));
    }

    let body = stamp(args, &token, &["loglevel", "asUser", "filter", "runAtTime"]);
    let r = send(
        client()
            .post(format!("{api}/job/{job_id}/run"))
            .header(AUTH, &cfg.api_token)
            .header("Content-Type", "application/json")
            .json(&body),
    )
    .await?;
    check(&r)?;
    if r.status == 409 {
        // api.error.execution.conflict: the job's own limit on concurrent
        // executions. It clears when the execution now running ends.
        return Err(Err(r.text));
    }
    if r.status == 404 {
        return reject("job_not_found", Some(message(&r)));
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(json!(r.text)));
    }
    Ok(Ok(r.text))
}

/// Specification §4.3, translated from its Python.
async fn job_retry(cfg: &Config, promise: &PromiseRecord, args: &Value) -> Verdict {
    let api = api(cfg);
    let job_id = match arg_str(args, "id") {
        Ok(v) => quote(v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };
    let prior_id = match arg_id(args, "executionId") {
        Ok(v) => quote(&v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };
    let token = sanitize(&promise.id);

    // The prior execution names the project the stamp is searchable in.
    let r = send(
        client()
            .get(format!("{api}/execution/{prior_id}"))
            .header(AUTH, &cfg.api_token),
    )
    .await?;
    check(&r)?;
    if r.status == 404 {
        return reject("execution_not_found", Some(message(&r)));
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(json!(r.text)));
    }
    let prior = r.json();

    // This endpoint ignores argString in a JSON body; options is the form it
    // honours, and it merges with the prior execution's options rather than
    // replacing them, an explicit value winning — so the stamp is always this
    // promise's, never one inherited from the execution being retried.
    let mut body = Map::new();
    for k in ["failedNodes", "loglevel", "asUser"] {
        if let Some(v) = args.get(k) {
            body.insert(k.to_string(), v.clone());
        }
    }
    let mut options = args
        .get("options")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    options.insert(CORR.to_string(), json!(token));
    body.insert("options".to_string(), Value::Object(options));
    let body = Value::Object(body);

    let project = string(prior.get("project"));
    let execution = match find(cfg, &project, &token).await? {
        Some(e) => e,
        None => {
            let r = send(
                client()
                    .post(format!("{api}/job/{job_id}/retry/{prior_id}"))
                    .header(AUTH, &cfg.api_token)
                    .header("Content-Type", "application/json")
                    .json(&body),
            )
            .await?;
            check(&r)?;
            if r.status == 409 {
                // api.error.execution.conflict: the job's own limit on concurrent
                // executions. It clears when the execution now running ends.
                return Err(Err(r.text));
            }
            if r.status == 404 {
                // One status, three causes: an unknown job, an execution this job
                // cannot resolve ("Execution ID does not exist", which is also what
                // an unknown job id answers), and an execution with no failed node
                // list ("Failed node List for execution ID does not exist"). The
                // message distinguishes them only by wording, so both ids are
                // re-read instead.
                let detail = message(&r);
                let j = send(
                    client()
                        .get(format!("{api}/job/{job_id}/info"))
                        .header(AUTH, &cfg.api_token)
                        .header("Accept", "application/json"),
                )
                .await?;
                check(&j)?;
                if j.status == 404 {
                    return reject("job_not_found", Some(detail));
                }
                let e = send(
                    client()
                        .get(format!("{api}/execution/{prior_id}"))
                        .header(AUTH, &cfg.api_token),
                )
                .await?;
                check(&e)?;
                if e.status == 404 {
                    return reject("execution_not_found", Some(detail));
                }
                return reject("execution_not_retryable", Some(detail));
            }
            if r.status >= 400 {
                return reject("invalid_request", Some(json!(r.text)));
            }
            r.json()
        }
    };

    let id = execution.get("id").cloned().unwrap_or(Value::Null);
    match watch(cfg, &id, cfg.poll_job_retry(), promise).await? {
        Some(execution) => settle(&execution),
        None => reject("execution_deleted", None),
    }
}

/// Specification §4.4, translated from its Python.
async fn execution_get(cfg: &Config, args: &Value) -> Verdict {
    let api = api(cfg);
    let exec_id = match arg_id(args, "id") {
        Ok(v) => quote(&v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    let r = send(
        client()
            .get(format!("{api}/execution/{exec_id}"))
            .header(AUTH, &cfg.api_token),
    )
    .await?;
    check(&r)?;
    if r.status == 404 {
        return reject("not_found", Some(message(&r)));
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(json!(r.text)));
    }
    Ok(Ok(r.text))
}

/// Specification §4.5, translated from its Python.
async fn execution_list(cfg: &Config, args: &Value) -> Verdict {
    let api = api(cfg);
    let raw = match arg_str(args, "project") {
        Ok(v) => v,
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };
    let project = quote(raw);

    // "*" is a project name only on the running list; the query endpoint
    // answers 404 api.error.project.missing for it. Both answer the same
    // paging/executions body.
    let (url, keys): (String, &[&str]) = if raw == "*" {
        (
            format!("{api}/project/*/executions/running"),
            &["jobIdFilter", "includePostponed", "max", "offset"],
        )
    } else {
        (
            format!("{api}/project/{project}/executions"),
            &[
                "statusFilter",
                "jobIdListFilter",
                "excludeJobIdListFilter",
                "jobListFilter",
                "excludeJobListFilter",
                "groupPath",
                "groupPathExact",
                "excludeGroupPath",
                "excludeGroupPathExact",
                "jobFilter",
                "excludeJobFilter",
                "jobExactFilter",
                "excludeJobExactFilter",
                "userFilter",
                "abortedbyFilter",
                "executionTypeFilter",
                "optionFilter",
                "recentFilter",
                "olderFilter",
                "begin",
                "end",
                "adhoc",
                "max",
                "offset",
            ],
        )
    };

    // Pagination is the caller's loop (one promise per page), not ours.
    let r = send(
        client()
            .get(url)
            .header(AUTH, &cfg.api_token)
            .query(&query(args, keys)),
    )
    .await?;
    check(&r)?;
    if r.status == 404 {
        return reject("not_found", Some(message(&r)));
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(json!(r.text)));
    }
    Ok(Ok(r.text))
}

/// Specification §4.6, translated from its Python.
async fn execution_output(cfg: &Config, args: &Value) -> Verdict {
    let api = api(cfg);
    let exec_id = match arg_id(args, "id") {
        Ok(v) => quote(&v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    // Tailing is the caller's loop: each promise reads one window and hands
    // back the offset and lastModified the next one starts from.
    let r = send(
        client()
            .get(format!("{api}/execution/{exec_id}/output"))
            .header(AUTH, &cfg.api_token)
            .header("Accept", "application/json")
            .query(&query(
                args,
                &[
                    "nodename",
                    "stepctx",
                    "offset",
                    "lastlines",
                    "maxlines",
                    "lastmod",
                    "compacted",
                ],
            )),
    )
    .await?;
    check(&r)?;
    if r.status == 404 {
        return reject("not_found", Some(message(&r)));
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(json!(r.text)));
    }
    Ok(Ok(r.text))
}

/// Specification §4.7, translated from its Python.
async fn execution_state(cfg: &Config, args: &Value) -> Verdict {
    let api = api(cfg);
    let exec_id = match arg_id(args, "id") {
        Ok(v) => quote(&v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    let r = send(
        client()
            .get(format!("{api}/execution/{exec_id}/state"))
            .header(AUTH, &cfg.api_token)
            .header("Accept", "application/json"),
    )
    .await?;
    check(&r)?;
    if r.status == 404 {
        return reject("not_found", Some(message(&r)));
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(json!(r.text)));
    }
    Ok(Ok(r.text))
}

/// Specification §4.8, translated from its Python.
async fn execution_abort(cfg: &Config, promise: &PromiseRecord, args: &Value) -> Verdict {
    let api = api(cfg);
    let raw = match arg_id(args, "id") {
        Ok(v) => v,
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };
    let exec_id = quote(&raw);

    // The OpenAPI defines only POST on this path (operationId
    // apiExecutionAbort); the API Reference narrative writes it GET. Both
    // methods answer, and the OpenAPI is the authority.
    //
    // A repeated abort is free: on an execution that has already stopped
    // Rundeck answers 200 with abort.status "failed", reason "Job is not
    // running", and the execution's terminal status.
    let r = send(
        client()
            .post(format!("{api}/execution/{exec_id}/abort"))
            .header(AUTH, &cfg.api_token)
            .query(&query(args, &["asUser", "forceIncomplete"])),
    )
    .await?;
    check(&r)?;
    if r.status == 404 {
        return reject("not_found", Some(message(&r)));
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(json!(r.text)));
    }
    let body = r.json();
    let abort = body.get("abort");
    let abort_status = string(abort.and_then(|a| a.get("status")));
    let execution_status = string(body.get("execution").and_then(|e| e.get("status")));
    if abort_status == "failed" && !TERMINAL.contains(&execution_status.as_str()) {
        let reason = abort
            .and_then(|a| a.get("reason"))
            .cloned()
            .unwrap_or(Value::Null);
        return reject("abort_failed", Some(reason));
    }

    match watch(cfg, &json!(raw), cfg.poll_execution_abort(), promise).await? {
        Some(execution) => Ok(Ok(result(&execution).to_string())),
        None => reject("deleted", None),
    }
}

/// Specification §4.9, translated from its Python.
async fn execution_delete(cfg: &Config, args: &Value) -> Verdict {
    let api = api(cfg);
    let exec_id = match arg_id(args, "id") {
        Ok(v) => quote(&v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    // Re-delivery after a delete that landed sees the same 404 a wrong id
    // sees; Rundeck keeps no tombstone that would separate them.
    let r = send(
        client()
            .delete(format!("{api}/execution/{exec_id}"))
            .header(AUTH, &cfg.api_token),
    )
    .await?;
    check(&r)?;
    if r.status == 404 {
        return reject("not_found", Some(message(&r)));
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(json!(r.text)));
    }
    Ok(Ok(json!({}).to_string()))
}

/// Specification §4.10, translated from its Python.
async fn job_get(cfg: &Config, args: &Value) -> Verdict {
    let api = api(cfg);
    let job_id = match arg_str(args, "id") {
        Ok(v) => quote(v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    let r = send(
        client()
            .get(format!("{api}/job/{job_id}"))
            .header(AUTH, &cfg.api_token)
            .header("Accept", "application/json")
            .query(&[("format", "json")]),
    )
    .await?;
    check(&r)?;
    if r.status == 404 {
        return reject("not_found", Some(message(&r)));
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(json!(r.text)));
    }
    // job-json-v44 wraps the single job in a one-element array; an absent
    // element is null rather than a crash.
    let job = r
        .json()
        .as_array()
        .and_then(|a| a.first().cloned())
        .unwrap_or(Value::Null);
    Ok(Ok(job.to_string()))
}

/// Specification §4.11, translated from its Python.
async fn job_list(cfg: &Config, args: &Value) -> Verdict {
    let api = api(cfg);
    let project = match arg_str(args, "project") {
        Ok(v) => quote(v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    let params = query(
        args,
        &[
            "idlist",
            "groupPath",
            "groupPathExact",
            "jobFilter",
            "jobExactFilter",
            "scheduledFilter",
            "serverNodeUUIDFilter",
            "tags",
            "max",
            "offset",
        ],
    );
    let r = send(
        client()
            .get(format!("{api}/project/{project}/jobs"))
            .header(AUTH, &cfg.api_token)
            .query(&params),
    )
    .await?;
    check(&r)?;
    if r.status == 404 {
        return reject("not_found", Some(message(&r)));
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(json!(r.text)));
    }
    Ok(Ok(r.text))
}

/// Specification §4.12, translated from its Python.
async fn project_list(cfg: &Config, args: &Value) -> Verdict {
    let api = api(cfg);

    let r = send(
        client()
            .get(format!("{api}/projects"))
            .header(AUTH, &cfg.api_token)
            .query(&query(args, &["meta"])),
    )
    .await?;
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(json!(r.text)));
    }
    Ok(Ok(r.text))
}

// ─── The specification's shared helpers ───────────────────────────────────────

/// `_find`: the execution this promise already created, if any. The stamp is
/// only searchable within a project, so the caller names one.
async fn find(cfg: &Config, project: &str, token: &str) -> Result<Option<Value>, Failure> {
    let api = api(cfg);
    let proj = quote(project);
    let r = send(
        client()
            .get(format!("{api}/project/{proj}/executions"))
            .header(AUTH, &cfg.api_token)
            .query(&[
                ("optionFilter".to_string(), format!("-{CORR} {token}")),
                ("max".to_string(), "20".to_string()),
            ]),
    )
    .await?;
    check(&r)?;
    if r.status >= 400 {
        return Ok(None);
    }
    // optionFilter is a partial match over the option values, so confirm the
    // stamp exactly.
    let body = r.json();
    let executions = body
        .get("executions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for e in executions {
        let stamp = e
            .get("job")
            .and_then(|j| j.get("options"))
            .and_then(|o| o.get(CORR))
            .and_then(Value::as_str);
        if stamp == Some(token) {
            return Ok(Some(e));
        }
    }
    Ok(None)
}

/// `_stamp`: the run body, carrying this promise's correlation stamp.
fn stamp(args: &Value, token: &str, keys: &[&str]) -> Value {
    let mut body = Map::new();
    for k in keys {
        if let Some(v) = args.get(*k) {
            body.insert((*k).to_string(), v.clone());
        }
    }
    match args.get("options") {
        Some(options) if !options.is_null() => {
            // An options map makes Rundeck ignore argString entirely, so the
            // stamp joins the map the caller supplied.
            let mut options = options.as_object().cloned().unwrap_or_default();
            options.insert(CORR.to_string(), json!(token));
            body.insert("options".to_string(), Value::Object(options));
        }
        _ => {
            // Undeclared options survive: Rundeck parses the whole argString into
            // the execution's job.options and applies declared defaults anyway.
            let base = args.get("argString").and_then(Value::as_str).unwrap_or("");
            let arg_string = format!("{base} -{CORR} {token}");
            body.insert("argString".to_string(), json!(arg_string.trim()));
        }
    }
    Value::Object(body)
}

/// `_await`: poll one execution to a terminal status. `Ok(None)` is the
/// execution having been deleted out from under the poll.
async fn watch(
    cfg: &Config,
    exec_id: &Value,
    cadence: Duration,
    promise: &PromiseRecord,
) -> Result<Option<Value>, Failure> {
    let api = api(cfg);
    let eid = quote(&id_str(exec_id));
    loop {
        let now = now_ms();
        if now >= promise.timeout_at {
            return Err(Err("promise timed out".into()));
        }
        let r = send(
            client()
                .get(format!("{api}/execution/{eid}"))
                .header(AUTH, &cfg.api_token),
        )
        .await?;
        check(&r)?;
        if r.status == 404 {
            return Ok(None);
        }
        if r.status >= 400 {
            return Err(Err(r.text));
        }
        let execution = r.json();
        if TERMINAL.contains(&string(execution.get("status")).as_str()) {
            return Ok(Some(execution));
        }
        // "scheduled" is not terminal and holds until runAtTime.
        //
        // Never sleep past the promise deadline: the next iteration has to
        // observe it and stop rather than wake after the server has already
        // settled the promise.
        let remaining = Duration::from_millis((promise.timeout_at - now_ms()).max(0) as u64);
        tokio::time::sleep(cadence.min(remaining)).await;
    }
}

/// `_settle`: the verdict a terminal execution carries.
fn settle(execution: &Value) -> Verdict {
    let status = string(execution.get("status"));
    if status == "succeeded" {
        return Ok(Ok(result(execution).to_string()));
    }
    if status == "failed" {
        return reject("execution_failed", Some(execution.clone()));
    }
    if status == "failed-with-retry" {
        // This execution is over; the retry Rundeck started is a different
        // execution, named by retriedExecution.
        return reject("execution_failed_with_retry", Some(execution.clone()));
    }
    if status == "aborted" {
        return reject("execution_aborted", Some(execution.clone()));
    }
    if status == "timedout" {
        return reject("execution_timedout", Some(execution.clone()));
    }
    // "other": a custom exit status, its string in customStatus.
    reject("execution_other", Some(execution.clone()))
}

/// The §4.1.2 Resolved mapping. An absent key maps to null, never a crash.
fn result(execution: &Value) -> Value {
    let value: Map<String, Value> = RESULT
        .iter()
        .map(|k| {
            (
                (*k).to_string(),
                execution.get(*k).cloned().unwrap_or(Value::Null),
            )
        })
        .collect();
    Value::Object(value)
}

// ─── Authentication (§3) ──────────────────────────────────────────────────────

/// `_auth`: the only credential Rundeck's API takes.
const AUTH: &str = "X-Rundeck-Auth-Token";

// ─── HTTP ─────────────────────────────────────────────────────────────────────

/// The API root of §1: every path below hangs off it.
fn api(cfg: &Config) -> String {
    format!("{}/api/59", cfg.base_url)
}

/// One response, kept as text so a non-JSON error body is still reportable.
struct Response {
    status: u16,
    text: String,
}

impl Response {
    fn json(&self) -> Value {
        serde_json::from_str(&self.text).unwrap_or(Value::Null)
    }
}

/// `response.body.message` — what every documented 404 rejection quotes. An
/// absent field is null, never a crash.
fn message(r: &Response) -> Value {
    r.json().get("message").cloned().unwrap_or(Value::Null)
}

/// The specification's `_check`: the classification every response runs
/// through before an operation reads its own status branches.
fn check(r: &Response) -> Result<(), Failure> {
    // 403 with errorCode "unauthorized" is the only authorization status
    // Rundeck returns: an absent, unknown or under-privileged token. It ends
    // when an operator issues a token or grants the ACL, not before.
    if r.status == 403 {
        return Err(Ok(r.text.clone()));
    }
    if r.status == 429 || r.status >= 500 {
        return Err(Err(r.text.clone()));
    }
    Ok(())
}

/// One connection pool for the whole plugin: a client per request would open
/// a new pool per call and leak sockets under load.
fn client() -> reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new).clone()
}

async fn send(req: reqwest::RequestBuilder) -> Result<Response, Failure> {
    // A request that produced no response is unclassified: release, and let
    // redelivery retry it. Every operation is safe to re-enter.
    let resp = req
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| Err(e.to_string()))?;
    let status = resp.status().as_u16();
    let text = resp.text().await.map_err(|e| Err(e.to_string()))?;
    Ok(Response { status, text })
}

// ─── Pure helpers ─────────────────────────────────────────────────────────────

/// Build a value to an operation's Rejected schema. `detail` is absent where
/// the schema says it is absent.
fn reject(code: &str, detail: Option<Value>) -> Verdict {
    let mut value = json!({ "code": code });
    if let Some(detail) = detail {
        value["detail"] = detail;
    }
    Ok(Err(value.to_string()))
}

/// A required string argument of an operation's Param schema.
///
/// A promise's param is immutable, so a param that does not satisfy the
/// schema now does not satisfy it on any redelivery: permanent, and every
/// operation's Rejected schema has `invalid_request` for it.
fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("args.{key} is required and must be a string"))
}

/// A required id argument the Param schema types `["string", "integer"]` —
/// the specification's `str(args[key])`.
fn arg_id(args: &Value, key: &str) -> Result<String, String> {
    match args.get(key) {
        Some(Value::String(s)) => Ok(s.clone()),
        Some(Value::Number(n)) => Ok(n.to_string()),
        _ => Err(format!(
            "args.{key} is required and must be a string or an integer"
        )),
    }
}

/// `str(v)` on a value the provider returned: a JSON string is its own
/// contents, anything else its JSON rendering.
fn id_str(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// A response field read as a string; absent or non-string reads as empty.
fn string(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or_default().to_string()
}

/// `_query`: the query parameters an operation forwards from its args.
/// Rundeck's list filters are repeated keys, which requests renders from a
/// list value; booleans go on the wire as true/false.
fn query(args: &Value, keys: &[&str]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for key in keys {
        let Some(value) = args.get(*key) else {
            continue;
        };
        match value {
            Value::Null => continue,
            Value::Array(items) => {
                for item in items {
                    if let Some(s) = scalar(item) {
                        out.push(((*key).to_string(), s));
                    }
                }
            }
            other => {
                if let Some(s) = scalar(other) {
                    out.push(((*key).to_string(), s));
                }
            }
        }
    }
    out
}

fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Bool(true) => Some("true".to_string()),
        Value::Bool(false) => Some("false".to_string()),
        other => Some(other.to_string()),
    }
}

/// Percent-encode one path segment — the specification's `quote(…, safe="")`.
fn quote(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for b in segment.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*b as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
