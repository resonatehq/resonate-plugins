//! The baserow plugin. Everything provider-specific lives here, translated
//! from plugins/baserow/spec/specification.md.

use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;
use std::time::Duration;

use resonate_core::types::PromiseRecord;

// `sanitize` is deliberately not used: the specification's Idempotency note
// records that Baserow has no client-supplied identity anywhere — an export
// job is created without a key and looked up only by its server-assigned
// integer id — so there is nowhere to inject sanitize(promise.id).
use crate::worker::b64_decode;

/// Specification §2, translated: bare field = required (a serde default
/// satisfies it), Option<T> = optional, `= poll` cascades via accessor.
#[derive(Clone, Deserialize)]
pub struct Config {
    pub base_url: String,
    pub email: String,
    pub password: String,
    #[serde(default = "poll_default", with = "humantime_serde")]
    pub poll: Duration,
}

fn poll_default() -> Duration {
    Duration::from_secs(2)
}

/// §4.1.4: the export job states that never change again.
const TERMINAL: [&str; 4] = ["finished", "failed", "cancelled", "expired"];

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
/// never sleep past it.
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
        "export.create" => export_create(config, promise, &args).await,
        "export.get" => export_get(config, &args).await,
        "table.list" => table_list(config, &args).await,
        "field.list" => field_list(config, &args).await,
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

/// One turn of the §4.1 poll loop, mirroring the specification's `try`
/// block: its `continue`, its `return`, and its assignment to `job`.
enum Step {
    /// 401: the access token expired and was minted again.
    Reauth(String),
    /// 404: the job record is gone.
    NotFound(String),
    /// The job as the provider now reports it.
    Job(Value),
}

/// Specification §4.1, translated from its Python. Begin — Baserow offers no
/// idempotency key, so a redelivery starts a second export and leaves the
/// first cancelled — then poll on the downstream clock; the worker frame
/// heartbeats the lease independently, so this cadence may back off freely.
async fn export_create(cfg: &Config, promise: &PromiseRecord, args: &Value) -> Verdict {
    let api = format!("{}/api", cfg.base_url);
    let mut auth = authenticate(cfg).await?;
    let table_id = match arg_int(args, "table_id") {
        Ok(v) => quote(&v.to_string()),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };
    let mut body: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    body.remove("table_id");

    // Creating an export job sets every other unfinished export job of this user
    // to cancelled, so a re-delivery costs the previous attempt's job and a
    // second full export of the table.
    let r = send(
        client()
            .post(format!("{api}/database/export/table/{table_id}/"))
            .header("Authorization", &auth)
            .json(&Value::Object(body)),
    )
    .await?;
    if r.status == 404 {
        if r.json().get("error") == Some(&json!("ERROR_VIEW_DOES_NOT_EXIST")) {
            return reject("view_not_found", Some(r.json()));
        }
        return reject("table_not_found", Some(r.json()));
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(r.json()));
    }

    let mut job = r.json();
    let mut failures = 0;
    while !TERMINAL.contains(&state(&job)) {
        let now = now_ms();
        if now >= promise.timeout_at {
            return Err(Err("promise timed out".into()));
        }
        // Never sleep past the promise deadline: the next iteration has to
        // observe it and stop rather than wake after the server has already
        // settled the promise.
        let remaining = Duration::from_millis((promise.timeout_at - now).max(0) as u64);
        tokio::time::sleep(cfg.poll.min(remaining)).await;
        // The job id exists only in this attempt's memory: nothing on the job
        // record ties it to the promise, so a release here loses the running job.
        let job_url = format!("{api}/database/export/{}/", quote(&id_of(&job)));
        let step: Result<Step, Failure> = async {
            let r = get(&job_url, &auth, &[]).await?;
            if r.status == 401 {
                // Access tokens live BASEROW_ACCESS_TOKEN_LIFETIME_MINUTES
                // (10 by default); this endpoint runs no permission check, so a
                // 401 here is an expired token.
                return Ok(Step::Reauth(authenticate(cfg).await?));
            }
            if r.status == 404 {
                return Ok(Step::NotFound(r.text.clone()));
            }
            check(&r)?;
            Ok(Step::Job(r.json()))
        }
        .await;
        match step {
            Ok(Step::Reauth(fresh)) => {
                auth = fresh;
                continue;
            }
            Ok(Step::NotFound(text)) => return reject("job_not_found", Some(json!(text))),
            Ok(Step::Job(fresh)) => {
                job = fresh;
                failures = 0;
            }
            Err(failure) => {
                // A halt ends the attempt at once; only the unclassified
                // failures accumulate toward the streak cap.
                if failure.is_ok() {
                    return Err(failure);
                }
                failures += 1;
                if failures >= 5 {
                    return Err(failure);
                }
            }
        }
    }

    if state(&job) == "finished" {
        let keys = [
            "id",
            "table",
            "view",
            "exporter_type",
            "state",
            "exported_file_name",
            "created_at",
            "url",
        ];
        // The 4.1.2 Resolved mapping.
        let value: Map<String, Value> = keys
            .iter()
            .map(|k| ((*k).to_string(), job.get(*k).cloned().unwrap_or(Value::Null)))
            .collect();
        return Ok(Ok(Value::Object(value).to_string()));
    }
    if state(&job) == "cancelled" {
        return reject("cancelled", Some(job));
    }
    if state(&job) == "expired" {
        return reject("expired", Some(job));
    }
    reject("export_failed", Some(job))
}

/// Specification §4.2, translated from its Python.
async fn export_get(cfg: &Config, args: &Value) -> Verdict {
    let api = format!("{}/api", cfg.base_url);
    let auth = authenticate(cfg).await?;
    let job_id = match arg_int(args, "job_id") {
        Ok(v) => quote(&v.to_string()),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    let r = get(&format!("{api}/database/export/{job_id}/"), &auth, &[]).await?;
    // ERROR_EXPORT_JOB_DOES_NOT_EXIST also answers for a job of another user.
    if r.status == 404 {
        return reject("not_found", Some(json!(r.text)));
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(json!(r.text)));
    }
    Ok(Ok(r.text))
}

/// Specification §4.3, translated from its Python.
async fn table_list(cfg: &Config, args: &Value) -> Verdict {
    let api = format!("{}/api", cfg.base_url);
    let auth = authenticate(cfg).await?;
    let database_id = match arg_int(args, "database_id") {
        Ok(v) => quote(&v.to_string()),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    let r = get(
        &format!("{api}/database/tables/database/{database_id}/"),
        &auth,
        &[],
    )
    .await?;
    if r.status == 404 {
        return reject("not_found", Some(json!(r.text)));
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(json!(r.text)));
    }
    Ok(Ok(r.text))
}

/// Specification §4.4, translated from its Python.
async fn field_list(cfg: &Config, args: &Value) -> Verdict {
    let api = format!("{}/api", cfg.base_url);
    let auth = authenticate(cfg).await?;
    let table_id = match arg_int(args, "table_id") {
        Ok(v) => quote(&v.to_string()),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };
    let params = query_only(args, &["view"]);

    let r = get(
        &format!("{api}/database/fields/table/{table_id}/"),
        &auth,
        &params,
    )
    .await?;
    if r.status == 404 {
        if r.json().get("error") == Some(&json!("ERROR_VIEW_DOES_NOT_EXIST")) {
            return reject("view_not_found", Some(r.json()));
        }
        return reject("table_not_found", Some(r.json()));
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(json!(r.text)));
    }
    Ok(Ok(r.text))
}

// ─── Authentication (§3) ──────────────────────────────────────────────────────

/// Exchange the configured credentials for an access token — the
/// specification's `_auth`.
async fn authenticate(cfg: &Config) -> Result<String, Failure> {
    let r = send(
        client()
            .post(format!("{}/api/user/token-auth/", cfg.base_url))
            .json(&json!({"email": cfg.email, "password": cfg.password})),
    )
    .await?;
    check(&r)?;
    if r.status >= 400 {
        return Err(Ok(r.text));
    }
    let Some(access) = r.json().get("access_token").and_then(Value::as_str).map(str::to_string)
    else {
        // A 200 carrying two_factor_auth instead of the tokens: the account has
        // two-factor authentication enabled and cannot be authenticated with an
        // email and password alone.
        return Err(Ok(r.text));
    };
    Ok(format!("JWT {access}"))
}

// ─── HTTP ─────────────────────────────────────────────────────────────────────

/// One response, kept as text so a non-JSON error body is still reportable.
struct Response {
    status: u16,
    text: String,
}

impl Response {
    /// The body as JSON — null where the provider answered something else, so
    /// a rejection's `detail` stays reportable rather than crashing.
    fn json(&self) -> Value {
        serde_json::from_str(&self.text).unwrap_or(Value::Null)
    }
}

/// The specification's `_check`: the classification every response that is
/// not one of an operation's enumerated outcomes falls through to.
fn check(r: &Response) -> Result<(), Failure> {
    // 401 PERMISSION_DENIED / ERROR_INVALID_ACCESS_TOKEN, 402
    // ERROR_FEATURE_NOT_AVAILABLE (no premium license for the workspace) and
    // 403 ERROR_FEATURE_DISABLED all end only when an operator acts.
    if r.status == 401 || r.status == 402 || r.status == 403 {
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

async fn get(url: &str, auth: &str, params: &[(String, String)]) -> Result<Response, Failure> {
    send(client().get(url).header("Authorization", auth).query(params)).await
}

async fn send(req: reqwest::RequestBuilder) -> Result<Response, Failure> {
    // A request that produced no response is unclassified: release, and let
    // redelivery retry it.
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

/// A required integer argument of an operation's Param schema.
///
/// A promise's param is immutable, so a param that does not satisfy the
/// schema now does not satisfy it on any redelivery: permanent, and every
/// operation's Rejected schema has `invalid_request` for it.
fn arg_int(args: &Value, key: &str) -> Result<i64, String> {
    args.get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("args.{key} is required and must be an integer"))
}

/// The state of an export job — "" where the provider sent no state, which is
/// not terminal, so the loop keeps watching rather than crashing.
fn state(job: &Value) -> &str {
    job.get("state").and_then(Value::as_str).unwrap_or_default()
}

/// The id of an export job, as a path segment.
fn id_of(job: &Value) -> String {
    match job.get("id") {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

/// Query parameters from the args object, taking only the keys named — and
/// only where present, which is the specification's
/// `{k: args[k] for k in (…) if k in args}`.
fn query_only(args: &Value, keep: &[&str]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(obj) = args.as_object() else {
        return out;
    };
    for (key, value) in obj {
        if !keep.contains(&key.as_str()) {
            continue;
        }
        if let Some(s) = scalar(value) {
            out.push((key.clone(), s));
        }
    }
    out
}

/// A null is dropped rather than sent as the string "null"; a boolean renders
/// lowercase, as a query string spells it.
fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
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
