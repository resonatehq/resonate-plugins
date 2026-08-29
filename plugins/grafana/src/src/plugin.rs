//! The grafana plugin. Everything provider-specific lives here, translated
//! from plugins/grafana/spec/specification.md.

use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;
use std::time::Duration;

use resonate_core::types::PromiseRecord;

// `sanitize` is deliberately not used: the specification's Idempotency note
// records that every Grafana operation is a read — `query.run` executes data
// source queries, `datasource.list` and `datasource.get` read configuration —
// and that no value is injected into Grafana, so there is nowhere to put
// sanitize(promise.id).
use crate::worker::b64_decode;

/// Specification §2, translated: bare field = required (a serde default
/// satisfies it), Option<T> = optional, `= poll` cascades via accessor.
/// Grafana declares neither an optional key nor a `poll`: no operation polls,
/// each is one request answered by one response (§4.N.5 Monitoring:
/// `request_response`).
#[derive(Clone, Deserialize)]
pub struct Config {
    pub base_url: String,
    pub token: String,
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
/// never sleep past it — Grafana has no loop to bound: every operation
/// answers in one response.
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
        "query.run" => query_run(config, &args).await,
        "datasource.list" => datasource_list(config).await,
        "datasource.get" => datasource_get(config, &args).await,
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

/// Specification §4.1, translated from its Python. One request, one answer:
/// `/api/ds/query` runs the queries and returns their frames, so there is no
/// begin-then-poll and no downstream clock to sleep on.
async fn query_run(cfg: &Config, args: &Value) -> Verdict {
    // The §4.1.3 body: the four keys of the Param schema, each present only
    // where the caller sent it. A query item is forwarded to its data source
    // unchanged, its own query fields and all.
    let mut body = Map::new();
    for key in ["queries", "from", "to", "debug"] {
        if let Some(value) = args.get(key) {
            body.insert(key.to_string(), value.clone());
        }
    }

    // [dataproxy] timeout — 30s by default — bounds only core backend HTTP
    // data sources; a plugin data source can run well past it, so a query
    // slow enough to outrun this read timeout raises instead of answering.
    let r = send(
        client()
            .post(format!("{}/api/ds/query", cfg.base_url))
            .header("Authorization", auth(cfg))
            .json(&Value::Object(body))
            .timeout(Duration::from_secs(60)),
    )
    .await?;
    check(&r)?;

    if r.status == 404 {
        return reject("not_found", Some(r.json()));
    }
    if r.status == 400 {
        // 400 is both a malformed request and "one or more data source
        // queries were unsuccessful"; only the latter carries results.
        let body = r.json();
        let Some(results) = body.get("results").filter(|v| !v.is_null()) else {
            return reject("invalid_request", Some(body));
        };
        for entry in results.as_object().into_iter().flat_map(Map::values) {
            if entry.get("error").unwrap_or(&Value::Null).is_null() {
                continue;
            }
            // 400 carries the status the queried system answered when it
            // refused this query; 502 means it was never reached and 500
            // that the plugin raised, neither a fact about the query.
            match entry.get("status").and_then(Value::as_i64) {
                Some(s) if s != 429 && (400..500).contains(&s) => {}
                _ => return Err(Err(r.text.clone())),
            }
        }
        return reject("query_failed", Some(results.clone()));
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(json!(r.text)));
    }
    // Resolved = response.body.
    Ok(Ok(r.text))
}

/// Specification §4.2, translated from its Python.
async fn datasource_list(cfg: &Config) -> Verdict {
    // Takes no arguments and is not paginated: Grafana answers with every
    // data source of the token's organization, up to [datasources]
    // datasource_limit.
    let r = send(
        client()
            .get(format!("{}/api/datasources", cfg.base_url))
            .header("Authorization", auth(cfg))
            .timeout(Duration::from_secs(10)),
    )
    .await?;
    check(&r)?;

    if r.status >= 400 {
        return reject("invalid_request", Some(json!(r.text)));
    }
    Ok(Ok(r.text))
}

/// Specification §4.3, translated from its Python.
async fn datasource_get(cfg: &Config, args: &Value) -> Verdict {
    let uid = match arg_str(args, "uid") {
        Ok(v) => quote(v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    let r = send(
        client()
            .get(format!("{}/api/datasources/uid/{uid}", cfg.base_url))
            .header("Authorization", auth(cfg))
            .timeout(Duration::from_secs(10)),
    )
    .await?;
    check(&r)?;

    if r.status == 404 {
        return reject("not_found", Some(r.json()));
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(json!(r.text)));
    }
    Ok(Ok(r.text))
}

// ─── Authentication (§3) ──────────────────────────────────────────────────────

/// The §3 header. A service account token is presented as-is on every
/// request: nothing is exchanged, so there is no token call to make.
fn auth(cfg: &Config) -> String {
    format!("Bearer {}", cfg.token)
}

// ─── HTTP ─────────────────────────────────────────────────────────────────────

/// One response, kept as text so a non-JSON error body is still reportable.
struct Response {
    status: u16,
    text: String,
}

impl Response {
    /// The body as JSON — `null` where it is not JSON at all, never a crash.
    fn json(&self) -> Value {
        serde_json::from_str(&self.text).unwrap_or(Value::Null)
    }
}

/// The specification's `_check`: applied to every response before that
/// operation's own status branches.
fn check(r: &Response) -> Result<(), Failure> {
    // 401 unauthorisedError and 403 forbiddenError are the only statuses
    // Grafana documents for a rejected or under-permissioned credential;
    // on /api/ds/query a 403 is "Access denied" to a queried data source.
    if r.status == 401 || r.status == 403 {
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
    // redelivery retry it. Every operation is a read, so re-entry is safe.
    let resp = req.send().await.map_err(|e| Err(e.to_string()))?;
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
/// schema now does not satisfy it on any redelivery: permanent, and the
/// operation's Rejected schema has `invalid_request` for it.
fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("args.{key} is required and must be a string"))
}

/// Percent-encode one path segment — the specification's `quote(…, safe="")`.
/// A data source UID is caller-supplied.
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
