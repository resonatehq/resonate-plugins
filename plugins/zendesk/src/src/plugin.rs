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
/// `subdomain` carries `= instance` in §2: the address's instance is resolved
/// by whoever loads the config section, never by the plugin — which never sees
/// the address — so the field is simply required here.
#[derive(Clone, Deserialize)]
pub struct Config {
    pub subdomain: String,
    pub email: String,
    pub api_token: String,
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

/// §4.7: the job status values that end the merge poll loop.
const JOB_TERMINAL: [&str; 3] = ["completed", "failed", "killed"];

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
        "ticket.get" => ticket_get(config, &args).await,
        "ticket.list" => ticket_list(config, &args).await,
        "ticket.search" => ticket_search(config, &args).await,
        "ticket.update" => ticket_update(config, &args).await,
        "ticket.delete" => ticket_delete(config, &args).await,
        "ticket.merge" => ticket_merge(config, promise, &args).await,
        "ticketcomment.list" => ticketcomment_list(config, &args).await,
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

/// Specification §4.1, translated from its Python.
async fn ticket_create(cfg: &Config, promise: &PromiseRecord, args: &Value) -> Verdict {
    let api = api(cfg);
    let auth = auth(cfg);
    // The 4.1.1 Param schema requires `comment`, and a promise's param is
    // immutable: no redelivery would read it differently.
    if !args.get("comment").map(Value::is_object).unwrap_or(false) {
        return reject(
            "invalid_request",
            Some(json!("args.comment is required and must be an object")),
        );
    }
    let key = sanitize(&promise.id);

    // The Idempotency-Key is honoured for two hours only; past that window the
    // external_id stamped below is the sole handle on the ticket this promise
    // created. Filtering by external_id is a list query, not a unique lookup:
    // Zendesk does not enforce uniqueness on external_id. The lookup costs one
    // request on every delivery, the first included, and pays only past that
    // window.
    let r = get(
        &format!("{}/tickets", api),
        &auth,
        &[("external_id".to_string(), key.clone())],
    )
    .await?;
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(json!(r.text)));
    }
    let found = r.json();
    let found = found.get("tickets").and_then(Value::as_array);
    if let Some(first) = found.and_then(|t| t.first()) {
        return Ok(Ok(first.to_string()));
    }

    let r = send(
        client()
            .post(format!("{}/tickets", api))
            .header("Authorization", &auth)
            .header("Idempotency-Key", &key)
            .json(&json!({"ticket": with_external_id(args, &key)})),
    )
    .await?;
    check(&r)?;
    if r.status >= 400 {
        // 400 ParameterMissing for a malformed body, 422 RecordInvalid for a
        // field the account rejects.
        return reject("invalid_request", Some(json!(r.text)));
    }
    Ok(Ok(field(&r.json(), "ticket").to_string()))
}

/// Specification §4.2, translated from its Python.
async fn ticket_get(cfg: &Config, args: &Value) -> Verdict {
    let api = api(cfg);
    let auth = auth(cfg);
    let ticket_id = match arg_id(args, "ticket_id") {
        Ok(v) => v,
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    let r = get(
        &format!("{}/tickets/{}", api, ticket_id),
        &auth,
        &query(args, &["include"]),
    )
    .await?;
    // A deleted ticket answers 404 here; it stays listed by
    // GET /api/v2/deleted_tickets until it is purged.
    if r.status == 404 {
        return reject("not_found", Some(json!(r.text)));
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(json!(r.text)));
    }
    Ok(Ok(field(&r.json(), "ticket").to_string()))
}

/// Specification §4.3, translated from its Python.
async fn ticket_list(cfg: &Config, args: &Value) -> Verdict {
    let api = api(cfg);
    let auth = auth(cfg);

    let r = get(
        &format!("{}/tickets", api),
        &auth,
        &query(
            args,
            &[
                "external_id",
                "include",
                "page[size]",
                "page[after]",
                "page[before]",
            ],
        ),
    )
    .await?;
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(json!(r.text)));
    }
    Ok(Ok(r.text))
}

/// Specification §4.4, translated from its Python.
async fn ticket_search(cfg: &Config, args: &Value) -> Verdict {
    let api = api(cfg);
    let auth = auth(cfg);
    // The 4.4.1 Param schema requires `query`.
    let Some(_) = args.get("query").and_then(Value::as_str) else {
        return reject(
            "invalid_request",
            Some(json!("args.query is required and must be a string")),
        );
    };
    let mut params = query(args, &["query", "page[size]", "page[after]"]);
    // The exported object type is given in filter[type]; a type: term inside the
    // query string is an error on this endpoint.
    params.push(("filter[type]".to_string(), "ticket".to_string()));

    let r = get(&format!("{}/search/export", api), &auth, &params).await?;
    check(&r)?;
    if r.status >= 400 {
        // 422 {"error": "invalid", "description": ...} for a malformed query or
        // an expired cursor.
        return reject("invalid_request", Some(json!(r.text)));
    }
    Ok(Ok(r.text))
}

/// Specification §4.5, translated from its Python.
async fn ticket_update(cfg: &Config, args: &Value) -> Verdict {
    let api = api(cfg);
    let auth = auth(cfg);
    let ticket_id = match arg_id(args, "ticket_id") {
        Ok(v) => v,
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };
    let body = without(args, "ticket_id");

    // This endpoint takes no idempotency key: a re-delivery re-applies the field
    // values, which converge, but appends the comment in the body a second time.
    let r = send(
        client()
            .put(format!("{}/tickets/{}", api, ticket_id))
            .header("Authorization", &auth)
            .json(&json!({ "ticket": body })),
    )
    .await?;
    if r.status == 404 {
        return reject("not_found", Some(json!(r.text)));
    }
    if r.status == 409 {
        // safe_update: the ticket changed after updated_stamp.
        return reject("conflict", Some(json!(r.text)));
    }
    check(&r)?;
    if r.status >= 400 {
        // 422 RecordInvalid, which is also the answer for any update to a
        // closed ticket.
        return reject("invalid_request", Some(json!(r.text)));
    }
    Ok(Ok(field(&r.json(), "ticket").to_string()))
}

/// Specification §4.6, translated from its Python.
async fn ticket_delete(cfg: &Config, args: &Value) -> Verdict {
    let api = api(cfg);
    let auth = auth(cfg);
    let ticket_id = match arg_id(args, "ticket_id") {
        Ok(v) => v,
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    let r = send(
        client()
            .delete(format!("{}/tickets/{}", api, ticket_id))
            .header("Authorization", &auth),
    )
    .await?;
    // Also the answer once this promise's own delete has landed: the ticket is
    // soft-deleted and this endpoint no longer sees it.
    if r.status == 404 {
        return reject("not_found", Some(json!(r.text)));
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(json!(r.text)));
    }
    Ok(Ok(json!({"id": args["ticket_id"], "deleted": true}).to_string()))
}

/// Specification §4.7, translated from its Python. Queue the merge, then poll
/// its job status on the downstream clock — the worker frame heartbeats the
/// lease independently, so this cadence may back off freely.
async fn ticket_merge(cfg: &Config, promise: &PromiseRecord, args: &Value) -> Verdict {
    let api = api(cfg);
    let auth = auth(cfg);
    let ticket_id = match arg_id(args, "ticket_id") {
        Ok(v) => v,
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };
    // The 4.7.1 Param schema requires `ids`.
    if !args.get("ids").map(Value::is_array).unwrap_or(false) {
        return reject(
            "invalid_request",
            Some(json!("args.ids is required and must be an array")),
        );
    }
    let body = without(args, "ticket_id");

    // No idempotency key on this endpoint, and nothing on the job status ties it
    // to the promise: a re-delivery queues a second merge whose sources are by
    // then closed, so its results report them as failures.
    let r = send(
        client()
            .post(format!("{}/tickets/{}/merge", api, ticket_id))
            .header("Authorization", &auth)
            .json(&body),
    )
    .await?;
    if r.status == 404 {
        return reject("not_found", Some(json!(r.text)));
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(json!(r.text)));
    }

    let mut job = field(&r.json(), "job_status");
    let mut failures = 0u32;
    while !JOB_TERMINAL.contains(&status(&job)) {
        let now = now_ms();
        if now >= promise.timeout_at {
            return Err(Err("promise timed out".into()));
        }
        // Never sleep past the promise deadline: the next iteration has to
        // observe it and stop rather than wake after the server has already
        // settled the promise.
        let remaining = Duration::from_millis((promise.timeout_at - now).max(0) as u64);
        tokio::time::sleep(cfg.poll.min(remaining)).await;
        let url = format!("{}/job_statuses/{}", api, quote(&text(&job, "id")));
        let r = match get(&url, &auth, &[]).await {
            Ok(r) => r,
            // A halt is an operator's to clear; it does not get absorbed.
            Err(Ok(reason)) => return Err(Ok(reason)),
            Err(Err(reason)) => {
                failures += 1;
                if failures >= 5 {
                    return Err(Err(reason));
                }
                continue;
            }
        };
        if r.status == 404 {
            return reject("job_not_found", Some(json!(r.text)));
        }
        match check(&r) {
            Ok(()) => {}
            Err(Ok(reason)) => return Err(Ok(reason)),
            Err(Err(reason)) => {
                // The merge is already queued and its job id is only held here:
                // absorb the transient answer, bounded by the streak cap.
                failures += 1;
                if failures >= 5 {
                    return Err(Err(reason));
                }
                continue;
            }
        }
        if r.status >= 400 {
            return reject("invalid_request", Some(json!(r.text)));
        }
        job = field(&r.json(), "job_status");
        failures = 0;
    }

    if status(&job) == "completed" {
        // The 4.7.2 Resolved mapping. An absent key is null, not a crash.
        let keys = ["id", "url", "status", "message", "progress", "total", "results"];
        let mut value = Map::new();
        for k in keys {
            value.insert(k.to_string(), job.get(k).cloned().unwrap_or(Value::Null));
        }
        return Ok(Ok(Value::Object(value).to_string()));
    }
    if status(&job) == "killed" {
        return reject("killed", Some(job));
    }
    // detail = the terminal job status object.
    reject("merge_failed", Some(job))
}

/// Specification §4.8, translated from its Python.
async fn ticketcomment_list(cfg: &Config, args: &Value) -> Verdict {
    let api = api(cfg);
    let auth = auth(cfg);
    let ticket_id = match arg_id(args, "ticket_id") {
        Ok(v) => v,
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    let r = get(
        &format!("{}/tickets/{}/comments", api, ticket_id),
        &auth,
        &query(
            args,
            &[
                "include",
                "include_inline_images",
                "sort",
                "page[size]",
                "page[after]",
                "page[before]",
            ],
        ),
    )
    .await?;
    // A deleted ticket answers 404 here, as does a ticket the credentials
    // cannot see.
    if r.status == 404 {
        return reject("not_found", Some(json!(r.text)));
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(json!(r.text)));
    }
    Ok(Ok(r.text))
}

// ─── Authentication (§3) ──────────────────────────────────────────────────────

/// §3: an API token is presented as HTTP Basic, the user being
/// `{email}/token`. There is no token exchange to make, so the only way the
/// credential can be found wanting is a 401/403 on the call itself.
fn auth(cfg: &Config) -> String {
    use base64::Engine;
    let basic = base64::engine::general_purpose::STANDARD
        .encode(format!("{}/token:{}", cfg.email, cfg.api_token));
    format!("Basic {basic}")
}

// ─── HTTP ─────────────────────────────────────────────────────────────────────

/// The API root of the specification's front matter, per account subdomain.
/// Overridable only through the environment: Zendesk is SaaS only — there is
/// no §5 provider to run — so the tests stand a local mock in its place and
/// point the plugin at it. The override is read once, before any request goes
/// out.
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
    // 401 "Couldn't authenticate you" and 403 (the agent's role, or the token's
    // scope, does not permit the action) end only when an operator acts.
    if r.status == 401 || r.status == 403 {
        return Err(Ok(r.text.clone()));
    }
    // Every documented Zendesk limit answers 429 with Retry-After and resets on
    // its own: the account limit per minute, and the per-endpoint limits (30
    // updates per 10 minutes per user per ticket, 400 ticket deletions per
    // minute, 100 search exports per minute).
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

/// The create body: the args as given, plus the injected identity.
/// `external_id` is `sanitize(promise.id)`, never the raw id — it is the
/// handle on the ticket once the two hour Idempotency-Key window has passed.
fn with_external_id(args: &Value, key: &str) -> Value {
    let mut body: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    body.insert("external_id".into(), json!(key));
    Value::Object(body)
}

/// The args minus one key: the request body of an operation whose id travels
/// in the path.
fn without(args: &Value, key: &str) -> Value {
    let mut body: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    body.remove(key);
    Value::Object(body)
}

/// A required integer argument of an operation's Param schema, rendered as the
/// path segment the Python's `quote(str(...))` produces.
///
/// A promise's param is immutable, so a param that does not satisfy the schema
/// now does not satisfy it on any redelivery: permanent, and `invalid_request`
/// is the code every Rejected schema here carries for it.
fn arg_id(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_i64)
        .map(|v| quote(&v.to_string()))
        .ok_or_else(|| format!("args.{key} is required and must be an integer"))
}

/// A field of a response body — absent (or a body that is not the documented
/// object) reads as JSON null rather than crashing.
fn field(body: &Value, key: &str) -> Value {
    body.get(key).cloned().unwrap_or(Value::Null)
}

/// `job_status.status` — absent reads as no status at all, which never matches
/// a terminal value: the loop then runs to the promise deadline rather than
/// reporting a merge it never observed.
fn status(job: &Value) -> &str {
    job.get("status").and_then(Value::as_str).unwrap_or_default()
}

/// A string field of a response body, empty when absent.
fn text(body: &Value, key: &str) -> String {
    body.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// The specification's `_query`: one pair per named argument that is present.
/// A boolean renders lowercase — Python's own `str(True)` would not — and an
/// array renders comma-joined. A null is dropped rather than sent as the
/// string "null", which is what `requests` does with a `None` parameter.
fn query(args: &Value, keys: &[&str]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for key in keys {
        let Some(value) = args.get(*key) else {
            continue;
        };
        let rendered = match value {
            Value::Null => continue,
            Value::Bool(b) => (if *b { "true" } else { "false" }).to_string(),
            Value::String(s) => s.clone(),
            Value::Array(items) => items
                .iter()
                .map(|item| match item {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect::<Vec<_>>()
                .join(","),
            other => other.to_string(),
        };
        out.push(((*key).to_string(), rendered));
    }
    out
}

/// Percent-encode one path segment, as the Python's `quote(..., safe="")`
/// does. Ticket and job ids are caller-supplied: a segment is a segment, never
/// a way out of the path.
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
