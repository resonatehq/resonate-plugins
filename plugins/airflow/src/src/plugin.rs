//! The airflow plugin. Everything provider-specific lives here, translated
//! from plugins/airflow/spec/specification.md.

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
    pub username: String,
    pub password: String,
    #[serde(default = "poll_default", with = "humantime_serde")]
    pub poll: Duration,
}

fn poll_default() -> Duration {
    Duration::from_secs(30)
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
        "dagrun.trigger" => dagrun_trigger(config, promise, &args).await,
        "dagrun.get" => dagrun_get(config, &args).await,
        "taskinstance.list" => taskinstance_list(config, &args).await,
        "xcom.get" => xcom_get(config, &args).await,
        "dag.get" => dag_get(config, &args).await,
        "dag.list" => dag_list(config, &args).await,
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
async fn dagrun_trigger(cfg: &Config, promise: &PromiseRecord, args: &Value) -> Verdict {
    let api = format!("{}/api/v2", cfg.base_url);
    let mut auth = token(cfg).await?;
    let dag_id = match arg_str(args, "dag_id") {
        Ok(v) => quote(v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };
    let run_id = sanitize(&promise.id);
    let run_path = format!("{api}/dags/{dag_id}/dagRuns/{}", quote(&run_id));

    let mut body: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    body.remove("dag_id");
    // Required key, may be null.
    body.insert(
        "logical_date".into(),
        args.get("logical_date").cloned().unwrap_or(Value::Null),
    );

    // (dag_id, dag_run_id) is unique with no expiry window, so a run created
    // by an earlier attempt is always recoverable by GET.
    let r = get(&run_path, &auth, &[]).await?;
    if r.status == 404 {
        let mut create = body.clone();
        create.insert("dag_run_id".into(), json!(run_id));
        let r = send(
            client()
                .post(format!("{api}/dags/{dag_id}/dagRuns"))
                .header("Authorization", &auth)
                .json(&Value::Object(create)),
        )
        .await?;
        if r.status == 404 {
            return reject("dag_not_found", Some(r.detail()));
        }
        // 400: import errors, manual runs not allowed, Dag param validation,
        // or a run_id outside allowed_run_id_pattern. 422: body schema.
        if r.status == 400 || r.status == 422 {
            return reject("invalid_request", Some(r.detail()));
        }
        if r.status == 409 {
            // 409 covers every unique constraint on the run row, not only
            // (dag_id, run_id) — e.g. a (dag_id, logical_date) collision
            // with an existing run. Only our own run id existing means a
            // previous attempt created it.
            let g = get(&run_path, &auth, &[]).await?;
            if g.status == 404 {
                return reject("conflict", Some(r.detail()));
            }
            check(&g)?;
        } else {
            check(&r)?;
        }
    } else {
        check(&r)?;
    }

    let run = loop {
        let now = now_ms();
        if now >= promise.timeout_at {
            return Err(Err("promise timed out".into()));
        }
        let r = get(&run_path, &auth, &[]).await?;
        if r.status == 401 {
            // JWTs expire after [api_auth] jwt_expiration_time (default 86400s).
            auth = token(cfg).await?;
            continue;
        }
        if r.status == 404 {
            return reject("deleted", None);
        }
        check(&r)?;
        let run = r.json();
        let state = run.get("state").and_then(Value::as_str).unwrap_or_default();
        if state == "success" || state == "failed" {
            break run;
        }
        // A paused Dag accepts the trigger but the scheduler never queues its
        // task instances: the run sits in "queued" until the Dag is unpaused.
        //
        // Never sleep past the promise deadline: the next iteration has to
        // observe it and stop rather than wake after the server has already
        // settled the promise.
        let remaining = Duration::from_millis((promise.timeout_at - now).max(0) as u64);
        tokio::time::sleep(cfg.poll.min(remaining)).await;
    };

    if run.get("state").and_then(Value::as_str) == Some("success") {
        let keys = [
            "dag_id",
            "dag_run_id",
            "state",
            "logical_date",
            "run_after",
            "start_date",
            "end_date",
            "duration",
            "conf",
            "run_type",
            "note",
        ];
        // The 4.1.2 Resolved mapping.
        let value: Map<String, Value> = keys
            .iter()
            .map(|k| ((*k).to_string(), run.get(*k).cloned().unwrap_or(Value::Null)))
            .collect();
        return Ok(Ok(Value::Object(value).to_string()));
    }
    reject("run_failed", Some(run))
}

/// Specification §4.2, translated from its Python.
async fn dagrun_get(cfg: &Config, args: &Value) -> Verdict {
    let api = format!("{}/api/v2", cfg.base_url);
    let auth = token(cfg).await?;

    let (dag_id, dag_run_id) = match (arg_str(args, "dag_id"), arg_str(args, "dag_run_id")) {
        (Ok(a), Ok(b)) => (quote(a), quote(b)),
        (Err(e), _) | (_, Err(e)) => return reject("invalid_request", Some(json!(e))),
    };
    let r = get(&format!("{api}/dags/{dag_id}/dagRuns/{dag_run_id}"), &auth, &[]).await?;
    if r.status == 404 {
        return reject("not_found", None);
    }
    if r.status == 400 || r.status == 422 {
        return reject("invalid_request", Some(r.detail()));
    }
    check(&r)?;
    Ok(Ok(r.text))
}

/// Specification §4.3, translated from its Python.
async fn taskinstance_list(cfg: &Config, args: &Value) -> Verdict {
    let api = format!("{}/api/v2", cfg.base_url);
    let auth = token(cfg).await?;

    // Pagination is the caller's loop (one promise per page), not ours.
    let params = query_except(args, &["dag_id", "dag_run_id"]);
    let (dag_id, dag_run_id) = match (arg_str(args, "dag_id"), arg_str(args, "dag_run_id")) {
        (Ok(a), Ok(b)) => (quote(a), quote(b)),
        (Err(e), _) | (_, Err(e)) => return reject("invalid_request", Some(json!(e))),
    };
    let r = get(
        &format!("{api}/dags/{dag_id}/dagRuns/{dag_run_id}/taskInstances"),
        &auth,
        &params,
    )
    .await?;
    if r.status == 404 {
        return reject("not_found", None);
    }
    if r.status == 400 || r.status == 422 {
        return reject("invalid_request", Some(r.detail()));
    }
    check(&r)?;
    Ok(Ok(r.text))
}

/// Specification §4.4, translated from its Python.
async fn xcom_get(cfg: &Config, args: &Value) -> Verdict {
    let api = format!("{}/api/v2", cfg.base_url);
    let auth = token(cfg).await?;

    let params = query_only(args, &["map_index", "deserialize", "stringify"]);
    let mut path = Vec::new();
    for k in ["dag_id", "dag_run_id", "task_id", "xcom_key"] {
        match arg_str(args, k) {
            Ok(v) => path.push(quote(v)),
            Err(e) => return reject("invalid_request", Some(json!(e))),
        }
    }
    let (dag_id, dag_run_id, task_id, xcom_key) = (&path[0], &path[1], &path[2], &path[3]);
    let r = get(
        &format!(
            "{api}/dags/{dag_id}/dagRuns/{dag_run_id}\
             /taskInstances/{task_id}/xcomEntries/{xcom_key}"
        ),
        &auth,
        &params,
    )
    .await?;
    if r.status == 404 {
        return reject("not_found", None);
    }
    if r.status == 400 || r.status == 422 {
        return reject("invalid_request", Some(r.detail()));
    }
    check(&r)?;
    Ok(Ok(r.text))
}

/// Specification §4.5, translated from its Python.
async fn dag_get(cfg: &Config, args: &Value) -> Verdict {
    let api = format!("{}/api/v2", cfg.base_url);
    let auth = token(cfg).await?;
    let dag_id = match arg_str(args, "dag_id") {
        Ok(v) => quote(v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    let r = get(&format!("{api}/dags/{dag_id}/details"), &auth, &[]).await?;
    if r.status == 404 {
        return reject("not_found", None);
    }
    if r.status == 400 || r.status == 422 {
        return reject("invalid_request", Some(r.detail()));
    }
    check(&r)?;
    Ok(Ok(r.text))
}

/// Specification §4.6, translated from its Python.
async fn dag_list(cfg: &Config, args: &Value) -> Verdict {
    let api = format!("{}/api/v2", cfg.base_url);
    let auth = token(cfg).await?;

    // Pagination is the caller's loop (one promise per page), not ours.
    let params = query_except(args, &[]);
    let r = get(&format!("{api}/dags"), &auth, &params).await?;
    if r.status == 400 || r.status == 422 {
        return reject("invalid_request", Some(r.detail()));
    }
    check(&r)?;
    Ok(Ok(r.text))
}

// ─── Authentication (§3) ──────────────────────────────────────────────────────

/// Exchange the configured credentials for a bearer token.
async fn token(cfg: &Config) -> Result<String, Failure> {
    let r = send(
        client()
            .post(format!("{}/auth/token", cfg.base_url))
            .json(&json!({"username": cfg.username, "password": cfg.password})),
    )
    .await?;
    if r.status == 401 || r.status == 403 {
        return Err(Ok(format!("credentials rejected: {}", r.text)));
    }
    check(&r)?;
    let Some(access) = r.json().get("access_token").and_then(Value::as_str).map(str::to_string)
    else {
        return Err(Err(format!("no access_token in token response: {}", r.text)));
    };
    Ok(format!("Bearer {access}"))
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

    /// `response.body.detail` — what every documented rejection quotes.
    fn detail(&self) -> Value {
        self.json().get("detail").cloned().unwrap_or(Value::Null)
    }
}

/// The specification's `_check`: the classification every response that is
/// not one of an operation's enumerated outcomes falls through to.
fn check(r: &Response) -> Result<(), Failure> {
    // 403: authenticated but not permitted — an operator must act.
    if r.status == 403 {
        return Err(Ok(r.text.clone()));
    }
    if r.status >= 400 {
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

async fn get(url: &str, auth: &str, params: &[(String, String)]) -> Result<Response, Failure> {
    send(client().get(url).header("Authorization", auth).query(params)).await
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

/// Query parameters from the args object, skipping the keys that are path
/// segments rather than query parameters.
fn query_except(args: &Value, skip: &[&str]) -> Vec<(String, String)> {
    query(args, |k| !skip.contains(&k))
}

/// Query parameters from the args object, taking only the keys named — and
/// only where present.
fn query_only(args: &Value, keep: &[&str]) -> Vec<(String, String)> {
    query(args, |k| keep.contains(&k))
}

/// An array argument becomes one pair per element; every other value becomes
/// one pair. A null is dropped rather than sent as the string "null".
fn query(args: &Value, want: impl Fn(&str) -> bool) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(obj) = args.as_object() else {
        return out;
    };
    for (key, value) in obj {
        if !want(key.as_str()) {
            continue;
        }
        match value {
            Value::Array(items) => {
                for item in items {
                    if let Some(s) = scalar(item) {
                        out.push((key.clone(), s));
                    }
                }
            }
            other => {
                if let Some(s) = scalar(other) {
                    out.push((key.clone(), s));
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
        other => Some(other.to_string()),
    }
}

/// Percent-encode one path segment — the specification's `quote(…, safe="")`.
/// Dag ids and run ids are caller-supplied and routinely contain `:`, `+`
/// and `/`.
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
