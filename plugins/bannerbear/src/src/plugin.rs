//! The bannerbear plugin. Everything provider-specific lives here, translated
//! from plugins/bannerbear/spec/specification.md.

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
    pub api_key: String,
    #[serde(default = "poll_default", with = "humantime_serde")]
    pub poll: Duration,
    #[serde(default, with = "humantime_serde")]
    pub poll_image: Option<Duration>,
    #[serde(default, with = "humantime_serde")]
    pub poll_animation: Option<Duration>,
}

fn poll_default() -> Duration {
    Duration::from_secs(2)
}

impl Config {
    /// §2: `poll_image` defaults to `poll`.
    fn poll_image(&self) -> Duration {
        self.poll_image.unwrap_or(self.poll)
    }

    /// §2: `poll_animation` defaults to `poll`.
    fn poll_animation(&self) -> Duration {
        self.poll_animation.unwrap_or(self.poll)
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
        "image.create" => image_create(config, promise, &args).await,
        "animation.create" => animation_create(config, promise, &args).await,
        "template.get" => template_get(config, &args).await,
        "template.list" => template_list(config, &args).await,
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

/// Specification §4.1, translated from its Python. Begin, then poll on the
/// downstream clock — the worker frame heartbeats the lease independently, so
/// this cadence may back off freely.
async fn image_create(cfg: &Config, promise: &PromiseRecord, args: &Value) -> Verdict {
    let auth = auth(cfg);
    if let Err(e) = require_render_args(args) {
        return reject("invalid_request", Some(json!(e)));
    }

    // Unkeyed POST: a re-entry renders again — a duplicate render is
    // benign, costing one render credit.
    let r = send(
        client()
            .post(format!("{}/images", api()))
            .header("Authorization", &auth)
            .json(&with_metadata(args, promise)),
    )
    .await?;
    if r.status == 400 || r.status == 404 || r.status == 422 {
        return reject("invalid_request", Some(r.json()));
    }
    check(&r)?;
    let mut img = r.json();

    let mut failures = 0u32;
    while state(&img) == "pending" {
        let now = now_ms();
        if now >= promise.timeout_at {
            return Err(Err("promise timed out".into()));
        }
        // Never sleep past the promise deadline: the next iteration has to
        // observe it and stop rather than wake after the server has already
        // settled the promise.
        let remaining = Duration::from_millis((promise.timeout_at - now).max(0) as u64);
        tokio::time::sleep(cfg.poll_image().min(remaining)).await;
        let url = format!("{}/images/{}", api(), quote(uid(&img)));
        let r = match get(&url, &auth, &[]).await.and_then(|r| check(&r).map(|()| r)) {
            Ok(r) => r,
            // A halt is an operator's to clear; it does not get absorbed.
            Err(Ok(reason)) => return Err(Ok(reason)),
            Err(Err(reason)) => {
                // The uid is unrecoverable on re-entry — absorb, bounded.
                failures += 1;
                if failures >= 5 {
                    return Err(Err(reason));
                }
                continue;
            }
        };
        failures = 0;
        img = r.json();
    }

    if state(&img) == "completed" {
        // The 4.1.2 Resolved mapping. An absent `files` is null, not a crash.
        return Ok(Ok(json!({"files": img.get("files").cloned().unwrap_or(Value::Null)}).to_string()));
    }
    // detail = the failed image object.
    reject("render_failed", Some(img))
}

/// Specification §4.2, translated from its Python. The same shape as §4.1 with
/// the animation resource's own status vocabulary and poll cadence.
async fn animation_create(cfg: &Config, promise: &PromiseRecord, args: &Value) -> Verdict {
    let auth = auth(cfg);
    if let Err(e) = require_render_args(args) {
        return reject("invalid_request", Some(json!(e)));
    }

    // Unkeyed POST, as in 4.1: re-entry renders again — benign.
    let r = send(
        client()
            .post(format!("{}/animations", api()))
            .header("Authorization", &auth)
            .json(&with_metadata(args, promise)),
    )
    .await?;
    if r.status == 400 || r.status == 404 || r.status == 422 {
        return reject("invalid_request", Some(r.json()));
    }
    check(&r)?;
    let mut anim = r.json();

    let mut failures = 0u32;
    while state(&anim) == "queued" || state(&anim) == "rendering" {
        let now = now_ms();
        if now >= promise.timeout_at {
            return Err(Err("promise timed out".into()));
        }
        // Never sleep past the promise deadline.
        let remaining = Duration::from_millis((promise.timeout_at - now).max(0) as u64);
        tokio::time::sleep(cfg.poll_animation().min(remaining)).await;
        let url = format!("{}/animations/{}", api(), quote(uid(&anim)));
        let r = match get(&url, &auth, &[]).await.and_then(|r| check(&r).map(|()| r)) {
            Ok(r) => r,
            Err(Ok(reason)) => return Err(Ok(reason)),
            Err(Err(reason)) => {
                failures += 1;
                if failures >= 5 {
                    return Err(Err(reason));
                }
                continue;
            }
        };
        failures = 0;
        anim = r.json();
    }

    if state(&anim) == "completed" {
        // The 4.2.2 Resolved mapping.
        return Ok(Ok(
            json!({"files": anim.get("files").cloned().unwrap_or(Value::Null)}).to_string()
        ));
    }
    // detail = response.body.error.
    reject(
        "render_failed",
        Some(anim.get("error").cloned().unwrap_or(Value::Null)),
    )
}

/// Specification §4.3, translated from its Python.
async fn template_get(cfg: &Config, args: &Value) -> Verdict {
    let auth = auth(cfg);
    let uid = match arg_str(args, "uid") {
        Ok(v) => quote(v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    let r = get(&format!("{}/image_templates/{uid}", api()), &auth, &[]).await?;
    if r.status == 404 {
        return reject("not_found", None);
    }
    check(&r)?;
    Ok(Ok(r.text))
}

/// Specification §4.4, translated from its Python.
async fn template_list(cfg: &Config, args: &Value) -> Verdict {
    let auth = auth(cfg);

    // Pagination is the caller's loop (one promise per page), not ours.
    let params = query(args);
    let r = get(&format!("{}/image_templates", api()), &auth, &params).await?;
    if r.status == 400 || r.status == 422 {
        return reject("invalid_request", Some(r.json()));
    }
    check(&r)?;
    Ok(Ok(r.text))
}

// ─── Authentication (§3) ──────────────────────────────────────────────────────

/// A static bearer key: there is no token exchange to make, so the only way
/// the credential can be found wanting is a 401/402/403 on the call itself.
fn auth(cfg: &Config) -> String {
    format!("Bearer {}", cfg.api_key)
}

// ─── HTTP ─────────────────────────────────────────────────────────────────────

/// The API root of the specification's front matter. Overridable only through
/// the environment: Bannerbear is SaaS only — there is no §5 provider to run —
/// so the tests stand a local mock in its place and point the plugin at it.
/// Read once, before any request goes out.
fn api() -> &'static str {
    static API: OnceLock<String> = OnceLock::new();
    API.get_or_init(|| {
        std::env::var("BANNERBEAR_API")
            .unwrap_or_else(|_| "https://api.bannerbear.com/v5".to_string())
    })
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

/// The specification's `_check`: the classification every response that is
/// not one of an operation's enumerated outcomes falls through to.
fn check(r: &Response) -> Result<(), Failure> {
    // 401 key invalid, 402 quota exhausted, 403 key lacks access —
    // an operator must act.
    if r.status == 401 || r.status == 402 || r.status == 403 {
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

/// The render body: the args as given, plus the injected identity.
/// `metadata` is `sanitize(promise.id)`, never the raw id.
fn with_metadata(args: &Value, promise: &PromiseRecord) -> Value {
    let mut body: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    body.insert("metadata".into(), json!(sanitize(&promise.id)));
    Value::Object(body)
}

/// The keys both render Param schemas mark required.
///
/// A promise's param is immutable, so a param that does not satisfy the
/// schema now does not satisfy it on any redelivery: permanent, and
/// `invalid_request` is the code both Rejected schemas carry for it. The
/// provider would answer the same POST with a 400/422 anyway; checking here
/// only spares the round trip.
fn require_render_args(args: &Value) -> Result<(), String> {
    arg_str(args, "template")?;
    if !args.get("modifications").map(Value::is_object).unwrap_or(false) {
        return Err("args.modifications is required and must be an object".to_string());
    }
    Ok(())
}

/// A required string argument of an operation's Param schema.
fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("args.{key} is required and must be a string"))
}

/// `body.status` — absent (a body that is not the documented object) reads as
/// no status at all, which leaves every poll loop and never matches
/// "completed": the render is then reported failed rather than crashing.
fn state(body: &Value) -> &str {
    body.get("status").and_then(Value::as_str).unwrap_or_default()
}

/// `body.uid` — the poll target's path segment.
fn uid(body: &Value) -> &str {
    body.get("uid").and_then(Value::as_str).unwrap_or_default()
}

/// Query parameters from the args object: the Python hands `args` to
/// `requests` wholesale.
///
/// An array argument becomes one pair per element; every other value becomes
/// one pair. A null is dropped rather than sent as the string "null", and a
/// boolean renders lowercase.
fn query(args: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(obj) = args.as_object() else {
        return out;
    };
    for (key, value) in obj {
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
        // `to_string` on a JSON bool is already lowercase.
        other => Some(other.to_string()),
    }
}

/// Percent-encode one path segment. Template and render uids are
/// caller-supplied: a segment is a segment, never a way out of the path.
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
