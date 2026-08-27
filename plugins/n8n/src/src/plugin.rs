//! The n8n plugin. Everything provider-specific lives here, translated
//! from plugins/n8n/spec/specification.md.

use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;
use std::time::Duration;

use resonate_core::types::PromiseRecord;

use crate::worker::b64_decode;

/// Specification §2, translated: bare field = required (a serde default
/// satisfies it), Option<T> = optional, `= poll` cascades via accessor.
#[derive(Clone, Deserialize)]
pub struct Config {
    pub base_url: String,
    pub api_key: String,
    #[serde(default = "poll_default", with = "humantime_serde")]
    pub poll: Duration,
}

fn poll_default() -> Duration {
    Duration::from_secs(2)
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

/// §4.1.4: the statuses that end an execution. `new`, `running`, `waiting`
/// and `unknown` are not terminal; `waiting` and `unknown` can persist
/// indefinitely.
const TERMINAL: [&str; 4] = ["success", "error", "canceled", "crashed"];

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
/// never sleep past it. n8n stamps no client-supplied identity on a retry
/// (§ Idempotency: the new execution records only `retryOf`), so no
/// `sanitize(promise.id)` is injected or recovered anywhere below.
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
        "execution.retry" => execution_retry(config, promise, &args).await,
        "execution.get" => execution_get(config, &args).await,
        "execution.list" => execution_list(config, &args).await,
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

/// Specification §4.1, translated from its Python. The retry endpoint accepts
/// no client-supplied identity, so the run cannot be begun idempotently — a
/// re-delivery retries the source execution again. What re-entry can still do
/// is find the execution an interrupted attempt started (`find_retry`) and
/// watch that one, rather than nothing.
async fn execution_retry(cfg: &Config, promise: &PromiseRecord, args: &Value) -> Verdict {
    let raw_id = match arg_id(args, "id") {
        Ok(v) => v,
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };
    let source_id = quote(&raw_id);
    let mut body: Map<String, Value> = Map::new();
    if let Some(load_workflow) = args.get("loadWorkflow") {
        body.insert("loadWorkflow".into(), load_workflow.clone());
    }

    // Unkeyed POST: a re-delivery retries the source execution again, costing
    // one more run of the workflow.
    //
    // The endpoint withholds its response until the execution it started
    // settles, so a workflow that runs longer than the request timeout never
    // answers here. That execution is unaffected and is recovered below.
    let r = send_tolerating_timeout(
        client()
            .post(format!("{}/executions/{source_id}/retry", api(cfg)))
            .header(API_KEY_HEADER, &cfg.api_key)
            .json(&Value::Object(body)),
    )
    .await?;
    // The endpoint answers 500 when the execution it started was cancelled,
    // so a 5xx is not evidence that nothing ran. The started execution is
    // recovered below; only its absence is a release.
    let r = r.filter(|r| r.status < 500);

    let mut run: Option<Value> = None;
    let mut execution_id: Option<String> = None;
    if let Some(r) = r {
        if r.status == 404 {
            return reject("not_found", Some(r.message()));
        }
        if r.status == 409 {
            // The source execution succeeded, is still queued, or was aborted
            // before any node data was stored, so there is nothing to resume.
            return reject("conflict", Some(r.message()));
        }
        if r.status == 400 {
            return reject("invalid_request", Some(r.message()));
        }
        check(&r)?;
        if r.status >= 400 {
            return reject("invalid_request", Some(json!(r.text)));
        }
        // The response body is the execution as it settled.
        let body = r.json();
        let Some(id) = scalar(body.get("id")) else {
            return Err(Err(format!("retry response carries no id: {}", r.text)));
        };
        execution_id = Some(quote(&id));
        run = Some(body);
    }
    if execution_id.is_none() {
        execution_id = find_retry(cfg, &raw_id).await?;
    }
    let Some(execution_id) = execution_id else {
        return Err(Err("retried execution not found".into()));
    };

    let mut failures = 0;
    let run = loop {
        if let Some(run) = &run {
            if TERMINAL.contains(&status_of(run)) {
                break run.clone();
            }
        }
        let now = now_ms();
        if now >= promise.timeout_at {
            return Err(Err("promise timed out".into()));
        }
        // Never sleep past the promise deadline: the next iteration has to
        // observe it and stop rather than wake after the server has already
        // settled the promise.
        let remaining = Duration::from_millis((promise.timeout_at - now).max(0) as u64);
        tokio::time::sleep(cfg.poll.min(remaining)).await;

        let g = match send(
            client()
                .get(format!("{}/executions/{execution_id}", api(cfg)))
                .header(API_KEY_HEADER, &cfg.api_key),
        )
        .await
        .and_then(|g| check(&g).map(|()| g))
        {
            Ok(g) => g,
            Err(halt @ Ok(_)) => return Err(halt),
            Err(release) => {
                // The retried execution's id is unrecoverable on re-entry —
                // absorb, bounded.
                failures += 1;
                if failures >= 5 {
                    return Err(release);
                }
                continue;
            }
        };
        failures = 0;
        if g.status == 404 {
            // A terminal execution's record was removed; n8n refuses to delete
            // a running one.
            return reject("deleted", None);
        }
        if g.status >= 400 {
            return reject("invalid_request", Some(json!(g.text)));
        }
        run = Some(g.json());
    };

    match status_of(&run) {
        "success" => {
            let keys = [
                "id",
                "finished",
                "mode",
                "status",
                "retryOf",
                "retrySuccessId",
                "workflowId",
                "createdAt",
                "startedAt",
                "stoppedAt",
                "waitTill",
            ];
            // The 4.1.2 Resolved mapping; the retry response omits createdAt,
            // stoppedAt, waitTill and retrySuccessId.
            let value: Map<String, Value> = keys
                .iter()
                .filter_map(|k| run.get(*k).map(|v| ((*k).to_string(), v.clone())))
                .collect();
            Ok(Ok(Value::Object(value).to_string()))
        }
        "canceled" => reject("cancelled", Some(run)),
        "crashed" => reject("crashed", Some(run)),
        _ => reject("execution_failed", Some(run)),
    }
}

/// Specification §4.2, translated from its Python.
async fn execution_get(cfg: &Config, args: &Value) -> Verdict {
    let execution_id = match arg_id(args, "id") {
        Ok(v) => quote(&v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    let keys = ["includeData", "ignoreDataSizeLimit", "redactExecutionData"];
    let r = send(
        client()
            .get(format!("{}/executions/{execution_id}", api(cfg)))
            .header(API_KEY_HEADER, &cfg.api_key)
            .query(&query(args, Some(&keys))),
    )
    .await?;
    if r.status == 404 {
        return reject("not_found", None);
    }
    if r.status == 400 {
        return reject("invalid_request", Some(r.message()));
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(json!(r.text)));
    }
    Ok(Ok(r.text))
}

/// Specification §4.3, translated from its Python.
async fn execution_list(cfg: &Config, args: &Value) -> Verdict {
    // Pagination is the caller's loop (one promise per page), not ours.
    let r = send(
        client()
            .get(format!("{}/executions", api(cfg)))
            .header(API_KEY_HEADER, &cfg.api_key)
            .query(&query(args, None)),
    )
    .await?;
    if r.status == 400 {
        return reject("invalid_request", Some(r.message()));
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(json!(r.text)));
    }
    Ok(Ok(r.text))
}

// ─── Recovery ─────────────────────────────────────────────────────────────────

/// The specification's `_find_retry`. A retried execution carries
/// retryOf = the source execution id, and its row exists before the retry
/// endpoint starts blocking. GET /executions returns newest first, caps limit
/// at 250, and omits running executions unless status=running is set, so both
/// listings are scanned.
async fn find_retry(cfg: &Config, source_id: &str) -> Result<Option<String>, Failure> {
    for params in [
        vec![("status".to_string(), "running".to_string()), ("limit".to_string(), "250".to_string())],
        vec![("limit".to_string(), "250".to_string())],
    ] {
        let r = send(
            client()
                .get(format!("{}/executions", api(cfg)))
                .header(API_KEY_HEADER, &cfg.api_key)
                .query(&params),
        )
        .await?;
        check(&r)?;
        if r.status >= 400 {
            return Err(Err(r.text));
        }
        let body = r.json();
        let empty = Vec::new();
        let rows = body.get("data").and_then(Value::as_array).unwrap_or(&empty);
        for e in rows {
            // retryOf is a number in the retry response and a string in a
            // read, so both are compared as strings; null means "not a retry".
            if scalar(e.get("retryOf")).as_deref() == Some(source_id) {
                return Ok(Some(quote(&scalar(e.get("id")).unwrap_or_default())));
            }
        }
    }
    Ok(None)
}

// ─── HTTP ─────────────────────────────────────────────────────────────────────

/// §3: the API key travels in this header on every request.
const API_KEY_HEADER: &str = "X-N8N-API-KEY";

/// The API root — the § API line.
fn api(cfg: &Config) -> String {
    format!("{}/api/v1", cfg.base_url)
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

    /// The specification's `_message`: `response.body.message`, or the raw
    /// body — anything fronting a self-hosted base_url can answer 4xx without
    /// n8n's JSON body.
    fn message(&self) -> Value {
        match self.json().get("message") {
            Some(message) => message.clone(),
            None => Value::String(self.text.clone()),
        }
    }
}

/// The specification's `_check`: the classification every response that is
/// not one of an operation's enumerated outcomes falls through to.
fn check(r: &Response) -> Result<(), Failure> {
    // 401: the API key is absent, malformed, or expired. 403: the key lacks
    // the endpoint's scope. Both need an operator.
    if r.status == 401 || r.status == 403 {
        return Err(Ok(r.text.clone()));
    }
    // n8n documents no rate-limit status; a 429 or 5xx comes from the server
    // itself or from whatever fronts it.
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

/// The specification's `timeout=10` on every request.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// A request that produced no response is unclassified: release, and let
/// redelivery retry it.
async fn send(req: reqwest::RequestBuilder) -> Result<Response, Failure> {
    raw(req).await.map_err(|e| Err(e.to_string()))
}

/// The retry POST alone, which the specification wraps in `except
/// ReadTimeout`: a timeout there is not a failure but the endpoint still
/// holding the response open, and the execution it started is recovered by
/// `find_retry`. Every other transport failure is still a release.
async fn send_tolerating_timeout(
    req: reqwest::RequestBuilder,
) -> Result<Option<Response>, Failure> {
    match raw(req).await {
        Ok(r) => Ok(Some(r)),
        Err(e) if e.is_timeout() => Ok(None),
        Err(e) => Err(Err(e.to_string())),
    }
}

async fn raw(req: reqwest::RequestBuilder) -> Result<Response, reqwest::Error> {
    let resp = req.timeout(REQUEST_TIMEOUT).send().await?;
    let status = resp.status().as_u16();
    let text = resp.text().await?;
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

/// The `id` argument of §4.1.1 and §4.2.1 — `["integer", "string"]`, spliced
/// into the path as the specification's `str(args["id"])`.
///
/// A promise's param is immutable, so a param that does not satisfy the
/// schema now does not satisfy it on any redelivery: permanent, and both
/// operations' Rejected schemas have `invalid_request` for it.
fn arg_id(args: &Value, key: &str) -> Result<String, String> {
    match args.get(key) {
        Some(Value::String(s)) => Ok(s.clone()),
        Some(Value::Number(n)) => Ok(n.to_string()),
        _ => Err(format!(
            "args.{key} is required and must be an integer or a string"
        )),
    }
}

/// The status of an execution record; "" when the field is absent, which is
/// not terminal.
fn status_of(run: &Value) -> &str {
    run.get("status").and_then(Value::as_str).unwrap_or_default()
}

/// The specification's `_query`, over the args object. `keys` is the
/// whitelist; `None` passes every arg through.
fn query(args: &Value, keys: Option<&[&str]>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(obj) = args.as_object() else {
        return out;
    };
    for (key, value) in obj {
        if keys.is_some_and(|keys| !keys.contains(&key.as_str())) {
            continue;
        }
        if let Some(rendered) = scalar(Some(value)) {
            out.push((key.clone(), rendered));
        }
    }
    out
}

/// A JSON scalar as the string the wire carries. n8n's request validator
/// answers 400 "must be boolean" for Python's `True`/`False` rendering; the
/// wire form is `true`/`false`, which is how serde_json renders a bool. A
/// null is dropped rather than sent as the string "null".
fn scalar(value: Option<&Value>) -> Option<String> {
    match value {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(other) => Some(other.to_string()),
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
