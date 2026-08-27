//! The rundeck plugin. Everything provider-specific lives here, translated
//! from plugins/rundeck/spec/specification.md.

use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;
use std::time::Duration;

use resonate_core::types::PromiseRecord;

use crate::worker::{b64_decode, sanitize};

/// Specification §1: every path is under this API version.
const API: &str = "/api/59";

/// Specification §2, translated: bare field = required (a serde default
/// satisfies it), Option<T> = optional, `= poll` cascades via accessor.
#[derive(Clone, Deserialize)]
pub struct Config {
    pub base_url: String,
    pub api_token: String,
    #[serde(default = "poll_default", with = "humantime_serde")]
    pub poll: Duration,
}

fn poll_default() -> Duration {
    Duration::from_secs(5)
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
        "job.get" => job_get(config, &args).await,
        "job.list" => job_list(config, &args).await,
        "execution.get" => execution_get(config, &args).await,
        "executionoutput.get" => executionoutput_get(config, &args).await,
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
    let job_id = match arg_str(args, "id") {
        Ok(v) => quote(v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };
    let token = sanitize(&promise.id);

    let info = get(cfg, &format!("{}{API}/job/{job_id}/info", cfg.base_url), &[]).await?;
    if info.status == 404 {
        return reject("job_not_found", Some(info.message()));
    }
    if info.status >= 400 {
        return reject("invalid_request", Some(info.message()));
    }
    let Some(project) = info.json().get("project").and_then(Value::as_str).map(quote) else {
        return Err(Err(format!("no project in job info response: {}", info.text)));
    };

    // Locate: every job accepts undeclared options, and optionFilter matches
    // option values — an execution started by an earlier delivery is
    // recoverable by its stamped rescorr option.
    let q = get(
        cfg,
        &format!("{}{API}/project/{project}/executions", cfg.base_url),
        &[("optionFilter".to_string(), format!("-rescorr {token}"))],
    )
    .await?;
    if q.status >= 400 {
        return reject("invalid_request", Some(q.message()));
    }
    let hits: Vec<Value> = q
        .json()
        .get("executions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut execution = if !hits.is_empty() {
        // Deterministic under races.
        hits.iter()
            .min_by_key(|x| x.get("id").and_then(Value::as_i64).unwrap_or(i64::MAX))
            .cloned()
            .unwrap_or(Value::Null)
    } else {
        let mut body: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
        body.remove("id");
        if body.contains_key("argString") && !body.contains_key("options") {
            let arg_string = body
                .get("argString")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            body.insert("argString".into(), json!(format!("{arg_string} -rescorr {token}")));
        } else {
            let mut options: Map<String, Value> = body
                .get("options")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            options.insert("rescorr".into(), json!(token));
            body.insert("options".into(), Value::Object(options));
        }
        let r = send(
            auth(cfg, client().post(format!("{}{API}/job/{job_id}/run", cfg.base_url)))
                .json(&Value::Object(body)),
        )
        .await?;
        if r.status == 404 {
            return reject("job_not_found", Some(r.message()));
        }
        if r.status == 409 {
            // api.error.execution.conflict: the job is already running
            // (multipleExecutions off) or executions are disabled — clears
            // with time.
            return Err(Err(r.text));
        }
        if r.status >= 400 {
            return reject("invalid_request", Some(r.message()));
        }
        r.json()
    };

    while matches!(status_of(&execution), "running" | "scheduled") {
        let now = now_ms();
        if now >= promise.timeout_at {
            return Err(Err("promise timed out".into()));
        }
        // Never sleep past the promise deadline: the next iteration has to
        // observe it and stop rather than wake after the server has already
        // settled the promise.
        let remaining = Duration::from_millis((promise.timeout_at - now).max(0) as u64);
        tokio::time::sleep(cfg.poll.min(remaining)).await;

        let execution_id = quote(&id_of(&execution));
        let g = get(
            cfg,
            &format!("{}{API}/execution/{execution_id}", cfg.base_url),
            &[],
        )
        .await?;
        if g.status == 404 {
            return reject("deleted", None);
        }
        if g.status >= 400 {
            return reject("invalid_request", Some(g.message()));
        }
        execution = g.json();
    }

    match status_of(&execution) {
        "succeeded" => {
            let keys = [
                "id",
                "href",
                "permalink",
                "status",
                "project",
                "user",
                "date-started",
                "date-ended",
                "job",
                "description",
                "argstring",
                "successfulNodes",
                "failedNodes",
            ];
            // The 4.1.2 Resolved mapping: the keys the terminal execution
            // carries, and only those.
            let value: Map<String, Value> = keys
                .iter()
                .filter_map(|k| execution.get(*k).map(|v| ((*k).to_string(), v.clone())))
                .collect();
            Ok(Ok(Value::Object(value).to_string()))
        }
        "failed" => reject("execution_failed", Some(execution)),
        "aborted" => reject("execution_aborted", Some(execution)),
        "timedout" => reject("execution_timedout", Some(execution)),
        // The retry runs as a separate execution with its own id; this one is over.
        "failed-with-retry" => reject("execution_failed_with_retry", Some(execution)),
        _ => reject("execution_other", Some(execution)),
    }
}

/// Specification §4.2, translated from its Python.
async fn job_get(cfg: &Config, args: &Value) -> Verdict {
    let job_id = match arg_str(args, "id") {
        Ok(v) => quote(v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    let r = get(cfg, &format!("{}{API}/job/{job_id}", cfg.base_url), &[]).await?;
    if r.status == 404 {
        return reject("not_found", None);
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(r.message()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.3, translated from its Python.
async fn job_list(cfg: &Config, args: &Value) -> Verdict {
    let project = match arg_str(args, "project") {
        Ok(v) => quote(v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    // Pagination is the caller's loop (one promise per page), not ours.
    let params = query_except(args, &["project"]);
    let r = get(
        cfg,
        &format!("{}{API}/project/{project}/jobs", cfg.base_url),
        &params,
    )
    .await?;
    if r.status == 404 {
        return reject("not_found", None);
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(r.message()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.4, translated from its Python.
async fn execution_get(cfg: &Config, args: &Value) -> Verdict {
    let execution_id = match arg_str(args, "id") {
        Ok(v) => quote(v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    let r = get(
        cfg,
        &format!("{}{API}/execution/{execution_id}", cfg.base_url),
        &[],
    )
    .await?;
    if r.status == 404 {
        return reject("not_found", None);
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(r.message()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.5, translated from its Python.
async fn executionoutput_get(cfg: &Config, args: &Value) -> Verdict {
    let execution_id = match arg_str(args, "id") {
        Ok(v) => quote(v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    // Following the log to its end is the caller's loop (one promise per
    // offset), not ours. Rundeck's compacted parameter is case-sensitive, so
    // a boolean renders as "true"/"false".
    let params = query_except(args, &["id"]);
    let r = get(
        cfg,
        &format!("{}{API}/execution/{execution_id}/output", cfg.base_url),
        &params,
    )
    .await?;
    if r.status == 404 {
        return reject("not_found", None);
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(r.message()));
    }
    Ok(Ok(r.text))
}

// ─── HTTP ─────────────────────────────────────────────────────────────────────

/// One response, kept as text so a non-JSON error body is still reportable.
struct Response {
    status: u16,
    text: String,
}

impl Response {
    fn json(&self) -> Value {
        serde_json::from_str(&self.text).unwrap_or(Value::Null)
    }

    /// `response.body.message` — what every documented rejection quotes. An
    /// error body without one is a null detail, not a crash.
    fn message(&self) -> Value {
        self.json().get("message").cloned().unwrap_or(Value::Null)
    }
}

/// The specification's `_check`: the classification every response that is
/// not one of an operation's enumerated outcomes falls through to.
fn check(r: &Response) -> Result<(), Failure> {
    // Rundeck answers both an unauthenticated call and a call the token's ACL
    // does not permit with 403 errorCode "unauthorized" — an operator must
    // issue a token or grant the ACL.
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

/// Specification §3.
fn auth(cfg: &Config, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    req.header("X-Rundeck-Auth-Token", &cfg.api_token)
        .header("Accept", "application/json")
}

async fn get(cfg: &Config, url: &str, params: &[(String, String)]) -> Result<Response, Failure> {
    send(auth(cfg, client().get(url)).query(params)).await
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
    let response = Response { status, text };
    check(&response)?;
    Ok(response)
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

/// `status` of an execution object — absent is no documented status, which
/// the §4.1 tail classifies as `execution_other`.
fn status_of(execution: &Value) -> &str {
    execution.get("status").and_then(Value::as_str).unwrap_or_default()
}

/// `id` of an execution object, as a path segment. Rundeck numbers
/// executions, but a listing that returned it as a string is still usable.
fn id_of(execution: &Value) -> String {
    match execution.get("id") {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

/// Query parameters from the args object, skipping the keys that are path
/// segments rather than query parameters.
fn query_except(args: &Value, skip: &[&str]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(obj) = args.as_object() else {
        return out;
    };
    for (key, value) in obj {
        if skip.contains(&key.as_str()) {
            continue;
        }
        // A JSON boolean renders lowercase — Rundeck's flags are
        // case-sensitive. A null is dropped rather than sent as "null".
        match value {
            Value::Null => continue,
            Value::String(s) => out.push((key.clone(), s.clone())),
            other => out.push((key.clone(), other.to_string())),
        }
    }
    out
}

/// Percent-encode one path segment — the specification's `quote(…, safe="")`.
/// Job ids and execution ids are caller-supplied.
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
