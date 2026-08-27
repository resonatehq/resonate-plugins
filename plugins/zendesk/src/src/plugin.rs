//! The zendesk plugin. Everything provider-specific lives here, translated
//! from plugins/zendesk/spec/specification.md.

use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;
use std::time::Duration;

use resonate_core::types::PromiseRecord;

use crate::worker::{b64_decode, sanitize};

/// Specification §2, translated: bare field = required (a serde default
/// satisfies it), Option<T> = optional, `= poll` cascades via accessor.
///
/// `subdomain` defaults to `= instance` — the address instance name. The
/// frame hands `process` the promise, not the address, so that cascade
/// belongs to whoever loads the configuration section: this field is the
/// already-cascaded value.
#[derive(Clone, Deserialize)]
pub struct Config {
    pub subdomain: String,
    pub email: String,
    pub api_token: String,
    #[serde(default = "poll_default", with = "humantime_serde")]
    pub poll: Duration,
}

fn poll_default() -> Duration {
    Duration::from_secs(15 * 60)
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
        "ticket.create" => ticket_create(config, promise, &args).await,
        "ticket.comment" => ticket_comment(config, &args).await,
        "ticket.get" => ticket_get(config, &args).await,
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
async fn ticket_create(cfg: &Config, promise: &PromiseRecord, args: &Value) -> Verdict {
    let api = api(cfg);
    let external_id = sanitize(&promise.id);

    // §4.1.1 requires `comment`, an object.
    if let Err(e) = require_object(args, "comment") {
        return reject("invalid_request", Some(json!(e)));
    }

    // Locate our ticket by the stamped external_id: on re-entry this
    // recovers the ticket a previous attempt created.
    let r = get(
        cfg,
        &format!("{api}/tickets"),
        &[("external_id", external_id.clone())],
    )
    .await?;
    check(&r)?;
    let hits = r.json();
    let hits = hits["tickets"].as_array().cloned().unwrap_or_default();

    let tid = if !hits.is_empty() {
        // external_id is not unique: pick deterministically
        match hits.iter().filter_map(|t| t["id"].as_i64()).min() {
            Some(id) => id,
            None => return Err(Err(format!("ticket without an id: {}", r.text))),
        }
    } else {
        // The Idempotency-Key makes a duplicate POST inside the 2h window
        // a safe replay.
        let mut ticket: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
        ticket.insert("external_id".into(), json!(external_id));
        let r = send(
            auth(client().post(format!("{api}/tickets")), cfg)
                .header("Idempotency-Key", &external_id)
                .json(&json!({ "ticket": Value::Object(ticket) })),
        )
        .await?;
        if r.status == 400 || r.status == 404 || r.status == 422 {
            return reject("invalid_request", Some(r.json()));
        }
        check(&r)?;
        match r.json()["ticket"]["id"].as_i64() {
            Some(id) => id,
            None => return Err(Err(format!("no ticket.id in the create response: {}", r.text))),
        }
    };

    // "solved" is NOT terminal — it reopens on customer reply; only
    // "closed" is frozen.
    let ticket = loop {
        let now = now_ms();
        if now >= promise.timeout_at {
            return Err(Err("promise timed out".into()));
        }
        let r = get(cfg, &format!("{api}/tickets/{tid}"), &[]).await?;
        if r.status == 404 {
            return reject("deleted", None); // soft-deleted before closing
        }
        check(&r)?;
        let t = r.json()["ticket"].clone();
        if t["status"].as_str() == Some("closed") {
            break t;
        }
        // Never sleep past the promise deadline: the next iteration has to
        // observe it and stop rather than wake after the server has already
        // settled the promise.
        let remaining = Duration::from_millis((promise.timeout_at - now).max(0) as u64);
        tokio::time::sleep(cfg.poll.min(remaining)).await;
    };

    let keys = ["id", "status", "subject", "tags", "created_at", "updated_at"];
    // The 4.1.2 Resolved mapping.
    let value: Map<String, Value> = keys
        .iter()
        .map(|k| ((*k).to_string(), ticket.get(*k).cloned().unwrap_or(Value::Null)))
        .collect();
    Ok(Ok(Value::Object(value).to_string()))
}

/// Specification §4.2, translated from its Python.
async fn ticket_comment(cfg: &Config, args: &Value) -> Verdict {
    let api = api(cfg);

    let id = match arg_i64(args, "id") {
        Ok(id) => id,
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };
    // §4.2.1 requires `comment`, an object, with a `body` string.
    let comment = match require_object(args, "comment") {
        Ok(c) if c.get("body").map(Value::is_string) == Some(true) => Value::Object(c.clone()),
        Ok(_) => {
            let e = "args.comment.body is required and must be a string";
            return reject("invalid_request", Some(json!(e)));
        }
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    // The PUT is unkeyed: best-effort dedup — scan the newest 100 comments
    // for an identical body before re-PUTting.
    let r = get(
        cfg,
        &format!("{api}/tickets/{id}/comments"),
        &[("sort_order", "desc".into()), ("per_page", "100".into())],
    )
    .await?;
    if r.status == 404 {
        return reject("not_found", None);
    }
    check(&r)?;
    let landed = r.json()["comments"]
        .as_array()
        .map(|cs| cs.iter().any(|c| c["body"] == comment["body"]))
        .unwrap_or(false);

    if !landed {
        let r = send(
            auth(client().put(format!("{api}/tickets/{id}")), cfg)
                .json(&json!({ "ticket": { "comment": comment } })),
        )
        .await?;
        if r.status == 404 {
            return reject("not_found", None);
        }
        if r.status == 400 || r.status == 422 {
            let body = r.json();
            // 422 covers all validation failures; "closed" only when the
            // frozen-ticket signal is present.
            let closed = body
                .get("details")
                .map(Value::to_string)
                .unwrap_or_default()
                .to_lowercase()
                .contains("closed");
            let code = if closed { "closed" } else { "invalid_request" };
            return reject(code, Some(body));
        }
        check(&r)?;
        return Ok(Ok(r.json()["ticket"].to_string()));
    }

    // Landed previously: resolve with the current record.
    let r = get(cfg, &format!("{api}/tickets/{id}"), &[]).await?;
    check(&r)?;
    Ok(Ok(r.json()["ticket"].to_string()))
}

/// Specification §4.3, translated from its Python.
async fn ticket_get(cfg: &Config, args: &Value) -> Verdict {
    let api = api(cfg);
    let tid = match arg_i64(args, "id") {
        Ok(id) => id,
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    let r = get(cfg, &format!("{api}/tickets/{tid}"), &[]).await?;
    if r.status == 404 {
        return reject("not_found", None);
    }
    check(&r)?;
    Ok(Ok(r.json()["ticket"].to_string()))
}

// ─── Authentication (§3) ──────────────────────────────────────────────────────

/// An API token, sent as HTTP Basic `{email}/token:{api_token}`: there is no
/// token exchange to make, so the only way the credential can be found wanting
/// is a 401/403 on the call itself.
fn auth(req: reqwest::RequestBuilder, cfg: &Config) -> reqwest::RequestBuilder {
    req.basic_auth(format!("{}/token", cfg.email), Some(&cfg.api_token))
}

// ─── HTTP ─────────────────────────────────────────────────────────────────────

/// The API root of the specification's front matter, composed from §2's
/// `subdomain`. Overridable only through the environment: Zendesk is SaaS only
/// — there is no §5 provider to run — so the tests stand a local mock in its
/// place and point the plugin at it. Read once, before any request goes out.
fn api(cfg: &Config) -> String {
    static OVERRIDE: OnceLock<Option<String>> = OnceLock::new();
    match OVERRIDE.get_or_init(|| std::env::var("ZENDESK_API").ok()) {
        Some(base) => base.clone(),
        None => format!("https://{}.zendesk.com/api/v2", cfg.subdomain),
    }
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
    // 401 token rejected, 403 role lacks permission — an operator must act.
    if r.status == 401 || r.status == 403 {
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

async fn get(
    cfg: &Config,
    url: &str,
    params: &[(&str, String)],
) -> Result<Response, Failure> {
    send(auth(client().get(url), cfg).query(params)).await
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

/// A required integer argument of an operation's Param schema.
///
/// A promise's param is immutable, so a param that does not satisfy the
/// schema now does not satisfy it on any redelivery: permanent.
fn arg_i64(args: &Value, key: &str) -> Result<i64, String> {
    args.get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("args.{key} is required and must be an integer"))
}

/// A required object argument of an operation's Param schema.
fn require_object<'a>(args: &'a Value, key: &str) -> Result<&'a Map<String, Value>, String> {
    args.get(key)
        .and_then(Value::as_object)
        .ok_or_else(|| format!("args.{key} is required and must be an object"))
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
