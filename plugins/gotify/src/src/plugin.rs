//! The gotify plugin. Everything provider-specific lives here, translated
//! from plugins/gotify/spec/specification.md.

use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;
use std::time::Duration;

use resonate_core::types::PromiseRecord;

use crate::worker::{b64_decode, sanitize};

/// Specification §2, translated: bare field = required (a serde default
/// satisfies it), Option<T> = optional, `= poll` cascades via accessor.
/// Gotify observes every operation on the response of the request that
/// begins it, so there is nothing to poll and no `poll` key.
#[derive(Clone, Deserialize)]
pub struct Config {
    pub base_url: String,
    pub client_token: String,
}

/// §4.1.5 `SCAN`: the documented maximum page size, and so the width of the
/// recovery window.
const SCAN: u32 = 200;

/// §4.1.5 `STAMP`: the extras key the injected identity is written under.
const STAMP: &str = "resonate::promise";

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
        "message.create" => message_create(config, promise, &args).await,
        "application.list" => application_list(config).await,
        "message.list" => message_list(config, &args).await,
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

/// Specification §4.1, translated from its Python. Gotify accepts no
/// idempotency key, so the begin is preceded by a scan for a message an
/// earlier delivery already stamped: fetch, then create.
async fn message_create(cfg: &Config, promise: &PromiseRecord, args: &Value) -> Verdict {
    // The Python indexes args["appid"] directly; a promise param is
    // immutable, so an absent appid is permanent rather than a crash.
    let Some(raw_appid) = args.get("appid").filter(|v| !v.is_null()) else {
        return reject("invalid_request", Some(json!("args.appid is required")));
    };
    // = str(args["appid"]): a malformed appid still travels, and the locate
    // request below answers it with the 400 §4.1.1 documents.
    let appid = quote(&scalar(raw_appid));
    let token = sanitize(&promise.id);

    // Locate: extras are stored verbatim and returned by the message reads,
    // so a message stamped by an earlier delivery is recoverable. Messages
    // come back newest first and 200 is the documented maximum page size, so
    // this single page is the recovery window.
    let q = check(
        get(
            &format!("{}/application/{appid}/message", cfg.base_url),
            cfg,
            &[("limit".to_string(), SCAN.to_string())],
        )
        .await?,
    )?;
    if q.status == 404 {
        // "application does not exist" — also the answer for an application
        // owned by another user.
        return reject("application_not_found", None);
    }
    if q.status >= 400 {
        return reject("invalid_request", Some(q.error_description()));
    }
    let messages = q
        .json()
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for m in &messages {
        let stamped = m
            .get("extras")
            .and_then(Value::as_object)
            .and_then(|extras| extras.get(STAMP))
            .and_then(Value::as_str);
        if stamped == Some(token.as_str()) {
            return Ok(Ok(m.to_string()));
        }
    }

    let mut body: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    // Extras are only accepted on an application/json request. A caller value
    // under STAMP is replaced; extras that is not an object carries nothing
    // to merge.
    let mut extras: Map<String, Value> = args
        .get("extras")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    extras.insert(STAMP.to_string(), json!(token));
    body.insert("extras".into(), Value::Object(extras));

    let r = check(
        send(
            client()
                .post(format!("{}/message", cfg.base_url))
                .header("X-Gotify-Key", &cfg.client_token)
                .header("Content-Type", "application/json")
                .json(&Value::Object(body)),
        )
        .await?,
    )?;
    if r.status >= 400 {
        return reject("invalid_request", Some(r.error_description()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.2, translated from its Python.
async fn application_list(cfg: &Config) -> Verdict {
    let r = check(get(&format!("{}/application", cfg.base_url), cfg, &[]).await?)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(r.error_description()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.3, translated from its Python.
async fn message_list(cfg: &Config, args: &Value) -> Verdict {
    // Following paging.next to the end is the caller's loop (one promise per
    // page), not ours.
    let params = query(args);
    let r = check(get(&format!("{}/message", cfg.base_url), cfg, &params).await?)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(r.error_description()));
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

    /// `response.body.errorDescription` — what every documented rejection
    /// quotes. An absent field is JSON null, never a crash.
    fn error_description(&self) -> Value {
        self.json()
            .get("errorDescription")
            .cloned()
            .unwrap_or(Value::Null)
    }
}

/// The specification's `_check`: applied to every response before that
/// request's own status branches.
fn check(r: Response) -> Result<Response, Failure> {
    // 401 covers a missing, unparseable or non-client token; 403 covers a
    // permission the user lacks and a session Gotify wants re-elevated.
    // Both end only when an operator issues or elevates a token.
    if r.status == 401 || r.status == 403 {
        return Err(Ok(r.text));
    }
    if r.status == 429 || r.status >= 500 {
        return Err(Err(r.text));
    }
    Ok(r)
}

/// One connection pool for the whole plugin: a client per request would open
/// a new pool per call and leak sockets under load.
fn client() -> reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new).clone()
}

/// §3: the client token travels in `X-Gotify-Key` on every request.
async fn get(url: &str, cfg: &Config, params: &[(String, String)]) -> Result<Response, Failure> {
    send(
        client()
            .get(url)
            .header("X-Gotify-Key", &cfg.client_token)
            .query(params),
    )
    .await
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

/// The args object rendered as query parameters — §4.3 passes them through
/// whole. A null is dropped rather than sent as the string "null".
fn query(args: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(obj) = args.as_object() else {
        return out;
    };
    for (key, value) in obj {
        if value.is_null() {
            continue;
        }
        out.push((key.clone(), scalar(value)));
    }
    out
}

/// Python's `str(…)` for a JSON value: a string is its own contents, and
/// everything else its JSON text — so a boolean renders `true`/`false`.
fn scalar(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
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
