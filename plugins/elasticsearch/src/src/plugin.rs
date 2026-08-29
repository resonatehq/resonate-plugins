//! The elasticsearch plugin. Everything provider-specific lives here,
//! translated from plugins/elasticsearch/spec/specification.md.

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
    pub api_key: String,
    #[serde(default = "poll_default", with = "humantime_serde")]
    pub poll: Duration,
}

fn poll_default() -> Duration {
    Duration::from_secs(2)
}

/// §4.1.5 `REINDEX_ACTION`: the transport action every reindex task runs
/// under, and the filter the task listing is narrowed by.
const REINDEX_ACTION: &str = "indices:data/write/reindex";

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
        "reindex.create" => reindex_create(config, promise, &args).await,
        "reindex.get" => reindex_get(config, &args).await,
        "index.list" => index_list(config, &args).await,
        "index.get" => index_get(config, &args).await,
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
async fn reindex_create(cfg: &Config, promise: &PromiseRecord, args: &Value) -> Verdict {
    let token = sanitize(&promise.id);

    let task_id = match running(cfg, &token).await? {
        Some(task_id) => task_id,
        None => {
            let mut body = Map::new();
            for key in ["source", "dest", "conflicts", "max_docs", "script"] {
                if let Some(v) = args.get(key) {
                    body.insert(key.to_string(), v.clone());
                }
            }
            let mut query = params(
                args,
                &[
                    "refresh",
                    "requests_per_second",
                    "slices",
                    "timeout",
                    "wait_for_active_shards",
                    "require_alias",
                ],
            );
            query.push(("wait_for_completion".into(), "false".into()));
            // A completed task has left the task listing: a copy that already
            // finished is copied again, overwriting the same document ids under
            // op_type "index" and conflicting under "create".
            let r = send(
                client()
                    .post(format!("{}/_reindex", cfg.base_url))
                    .header("Authorization", auth(cfg))
                    .header("X-Opaque-Id", &token)
                    .query(&query)
                    .json(&Value::Object(body)),
            )
            .await?;
            check(&r)?;
            if r.status >= 400 {
                // Residue: identical bytes every redelivery — permanent. Only
                // the request itself is validated here; an unknown source
                // index, a bad script or an unauthorized action end the task
                // instead.
                return reject("invalid_request", Some(r.error()));
            }
            match r.json().get("task").and_then(Value::as_str) {
                Some(task_id) => task_id.to_string(),
                None => return Err(Err(format!("no task id in reindex response: {}", r.text))),
            }
        }
    };

    let task = quote(&task_id);
    let mut failed = 0;
    let run = loop {
        if now_ms() >= promise.timeout_at {
            return Err(Err("promise timed out".into()));
        }
        match fetch_task(cfg, &task).await {
            Ok(run) => {
                failed = 0;
                if run.get("completed").and_then(Value::as_bool).unwrap_or(false) {
                    break run;
                }
                sleep_bounded(cfg, promise).await;
            }
            Err(failure) => {
                // A completed task has left the task listing, so leaving the
                // loop costs a second full copy of the whole index.
                if failure.is_ok() || failed >= 4 {
                    return Err(failure);
                }
                failed += 1;
                sleep_bounded(cfg, promise).await;
            }
        }
    };

    if let Some(error) = run.get("error") {
        return reject("reindex_failed", Some(error.clone()));
    }
    let result = run.get("response").cloned().unwrap_or_else(|| json!({}));
    let canceled = result.get("canceled").cloned();
    if run.get("cancelled").and_then(Value::as_bool).unwrap_or(false) || canceled.is_some() {
        // A cancellation the task record reports only through `cancelled` has
        // no reason to quote: the detail is then null.
        return reject("cancelled", Some(canceled.unwrap_or(Value::Null)));
    }
    if result
        .get("failures")
        .and_then(Value::as_array)
        .is_some_and(|f| !f.is_empty())
    {
        return reject("reindex_failed", Some(result["failures"].clone()));
    }
    // The 4.1.2 Resolved mapping.
    let keys = [
        "id",
        "description",
        "start_time_in_millis",
        "running_time_in_nanos",
        "cancelled",
        "response",
    ];
    let value: Map<String, Value> = keys
        .iter()
        .filter_map(|k| run.get(*k).map(|v| ((*k).to_string(), v.clone())))
        .collect();
    Ok(Ok(Value::Object(value).to_string()))
}

/// Specification §4.2, translated from its Python.
async fn reindex_get(cfg: &Config, args: &Value) -> Verdict {
    let task_id = match arg_str(args, "task_id") {
        Ok(v) => quote(v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    let r = get(cfg, &format!("{}/_reindex/{task_id}", cfg.base_url), &[]).await?;
    if r.status == 404 {
        return reject("not_found", None);
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(r.error()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.3, translated from its Python.
async fn index_list(cfg: &Config, args: &Value) -> Verdict {
    let query = params(
        args,
        &["expand_wildcards", "ignore_unavailable", "allow_no_indices", "mode"],
    );
    let name = match arg_str(args, "name") {
        Ok(v) => quote(v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };
    let r = get(
        cfg,
        &format!("{}/_resolve/index/{name}", cfg.base_url),
        &query,
    )
    .await?;
    if r.status == 404 {
        return reject("not_found", Some(r.error()));
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(r.error()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.4, translated from its Python.
async fn index_get(cfg: &Config, args: &Value) -> Verdict {
    let query = params(
        args,
        &[
            "allow_no_indices",
            "expand_wildcards",
            "features",
            "flat_settings",
            "ignore_unavailable",
            "include_defaults",
            "local",
            "master_timeout",
        ],
    );
    let index = match arg_str(args, "index") {
        Ok(v) => quote(v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };
    let r = get(cfg, &format!("{}/{index}", cfg.base_url), &query).await?;
    if r.status == 404 {
        return reject("not_found", Some(r.error()));
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(r.error()));
    }
    Ok(Ok(r.text))
}

// ─── Reindex task plumbing (§4.1.5) ───────────────────────────────────────────

/// The specification's `_running`: the id of the reindex this promise already
/// started, recovered from the task listing.
///
/// A running reindex still carries X-Opaque-Id in its task headers. A sliced
/// reindex repeats the header on its child tasks, which carry parent_task_id.
/// A node that stops responding mid-listing is reported in node_failures and
/// its tasks are missing from this response.
async fn running(cfg: &Config, token: &str) -> Result<Option<String>, Failure> {
    let r = get(
        cfg,
        &format!("{}/_tasks", cfg.base_url),
        &[("actions".to_string(), REINDEX_ACTION.to_string())],
    )
    .await?;
    check(&r)?;
    let body = r.json();
    let Some(nodes) = body.get("nodes").and_then(Value::as_object) else {
        return Ok(None);
    };
    for node in nodes.values() {
        let Some(tasks) = node.get("tasks").and_then(Value::as_object) else {
            continue;
        };
        for (task_id, task) in tasks {
            let opaque = task
                .get("headers")
                .and_then(|h| h.get("X-Opaque-Id"))
                .and_then(Value::as_str);
            if opaque == Some(token) && task.get("parent_task_id").is_none() {
                return Ok(Some(task_id.clone()));
            }
        }
    }
    Ok(None)
}

/// One reading of the reindex task record. Its error channel is what the
/// specification's poll loop counts against the consecutive-failure cap.
async fn fetch_task(cfg: &Config, task: &str) -> Result<Value, Failure> {
    let r = get(cfg, &format!("{}/_reindex/{task}", cfg.base_url), &[]).await?;
    if r.status == 404 {
        // The id no longer resolves: the task's node left the cluster with no
        // stored result. Redelivery re-enters through the task listing.
        return Err(Err(r.text.clone()));
    }
    check(&r)?;
    Ok(r.json())
}

/// Never sleep past the promise deadline: the next iteration has to observe
/// it and stop rather than wake after the server has already settled the
/// promise.
async fn sleep_bounded(cfg: &Config, promise: &PromiseRecord) {
    let remaining = Duration::from_millis((promise.timeout_at - now_ms()).max(0) as u64);
    tokio::time::sleep(cfg.poll.min(remaining)).await;
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

    /// `response.body.error` — what every documented rejection quotes.
    fn error(&self) -> Value {
        self.json().get("error").cloned().unwrap_or(Value::Null)
    }
}

/// The specification's `_check`: the classification every response that is
/// not one of an operation's enumerated outcomes falls through to.
fn check(r: &Response) -> Result<(), Failure> {
    // 401: the API key is rejected. 403: security_exception — the key lacks a
    // cluster or index privilege the action requires.
    if r.status == 401 || r.status == 403 {
        return Err(Ok(r.text.clone()));
    }
    if r.status == 429 || r.status >= 500 {
        return Err(Err(r.text.clone()));
    }
    Ok(())
}

/// The specification's `_auth`.
fn auth(cfg: &Config) -> String {
    format!("ApiKey {}", cfg.api_key)
}

/// One connection pool for the whole plugin: a client per request would open
/// a new pool per call and leak sockets under load.
fn client() -> reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new).clone()
}

async fn get(cfg: &Config, url: &str, params: &[(String, String)]) -> Result<Response, Failure> {
    send(client().get(url).header("Authorization", auth(cfg)).query(params)).await
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

/// The specification's `_params`: the named args, rendered as query
/// parameters.
///
/// Elasticsearch parses only lowercase "true"/"false", and reads a query key
/// repeated across occurrences as its last occurrence — a list of values is
/// sent comma-separated.
fn params(args: &Value, keys: &[&str]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for key in keys {
        let Some(value) = args.get(*key) else {
            continue;
        };
        let rendered = match value {
            Value::Array(items) => items.iter().map(scalar).collect::<Vec<_>>().join(","),
            other => scalar(other),
        };
        out.push(((*key).to_string(), rendered));
    }
    out
}

fn scalar(value: &Value) -> String {
    match value {
        Value::Bool(true) => "true".to_string(),
        Value::Bool(false) => "false".to_string(),
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Percent-encode one path segment — the specification's `quote(…, safe="")`.
/// A task id is `<node id>:<task number>`, and an index expression may carry
/// commas, wildcards and a remote cluster prefix.
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
