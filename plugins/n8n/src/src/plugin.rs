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
/// never sleep past it. No identity value is injected: the Idempotency note
/// records that no write in the Public API accepts a client-supplied key or
/// id, so `sanitize(&promise.id)` reaches no request — a retry an earlier
/// delivery started is correlated by the source execution id n8n stamps
/// into `retryOf` instead.
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
        "workflow.create" => workflow_create(config, &args).await,
        "workflow.get" => workflow_get(config, &args).await,
        "workflow.list" => workflow_list(config, &args).await,
        "workflow.update" => workflow_update(config, &args).await,
        "workflow.publish" => workflow_publish(config, &args).await,
        "workflow.unpublish" => workflow_unpublish(config, &args).await,
        "workflow.archive" => workflow_archive(config, &args).await,
        "workflow.unarchive" => workflow_unarchive(config, &args).await,
        "workflow.delete" => workflow_delete(config, &args).await,
        "execution.list" => execution_list(config, &args).await,
        "execution.get" => execution_get(config, &args).await,
        "execution.retry" => execution_retry(config, promise, &args).await,
        "execution.stop" => execution_stop(config, &args).await,
        "execution.delete" => execution_delete(config, &args).await,
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
async fn workflow_create(cfg: &Config, args: &Value) -> Verdict {
    // n8n assigns the id; a duplicate from a re-delivery is a second
    // unpublished workflow, which runs nothing until it is published.
    let r = send(auth(client().post(format!("{}/workflows", api(cfg))), cfg).json(args)).await?;
    check(&r)?;
    if r.status == 404 {
        return reject("not_found", Some(r.message()));
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(r.message()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.2, translated from its Python.
async fn workflow_get(cfg: &Config, args: &Value) -> Verdict {
    let workflow_id = match arg_str(args, "id") {
        Ok(v) => quote(v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    let r = get(
        cfg,
        &format!("{}/workflows/{workflow_id}", api(cfg)),
        &query(args, &["excludePinnedData"]),
    )
    .await?;
    check(&r)?;
    if r.status == 404 {
        return reject("not_found", Some(r.message()));
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(r.message()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.3, translated from its Python.
async fn workflow_list(cfg: &Config, args: &Value) -> Verdict {
    // Paging is the caller's loop (one promise per page), not ours.
    let r = get(
        cfg,
        &format!("{}/workflows", api(cfg)),
        &query(
            args,
            &[
                "active",
                "tags",
                "name",
                "projectId",
                "excludePinnedData",
                "offset",
                "limit",
                "cursor",
            ],
        ),
    )
    .await?;
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(r.message()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.4, translated from its Python.
async fn workflow_update(cfg: &Config, args: &Value) -> Verdict {
    let workflow_id = match arg_str(args, "id") {
        Ok(v) => quote(v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };
    let mut body: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    body.remove("id");
    body.remove("publishIfActive");

    // Re-delivery re-sends the same definition: n8n stores another version of
    // the same content, and the workflow ends in the state this body describes.
    let r = send(
        auth(
            client().put(format!("{}/workflows/{workflow_id}", api(cfg))),
            cfg,
        )
        .query(&query(args, &["publishIfActive"]))
        .json(&Value::Object(body)),
    )
    .await?;
    check(&r)?;
    if r.status == 404 {
        return reject("not_found", Some(r.message()));
    }
    if r.status == 409 {
        // The version is saved; only its publication is blocked.
        return reject("conflict", Some(r.json()));
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(r.message()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.5, translated from its Python.
async fn workflow_publish(cfg: &Config, args: &Value) -> Verdict {
    let workflow_id = match arg_str(args, "id") {
        Ok(v) => quote(v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };
    let mut body: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    body.remove("id");

    // Publishing is idempotent: a re-delivery re-publishes the same version and
    // answers 200 with the same workflow.
    let r = send(
        auth(
            client().post(format!("{}/workflows/{workflow_id}/publish", api(cfg))),
            cfg,
        )
        .json(&Value::Object(body)),
    )
    .await?;
    check(&r)?;
    if r.status == 404 {
        return reject("not_found", Some(r.message()));
    }
    if r.status == 409 {
        return reject("conflict", Some(r.json()));
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(r.message()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.6, translated from its Python.
async fn workflow_unpublish(cfg: &Config, args: &Value) -> Verdict {
    let workflow_id = match arg_str(args, "id") {
        Ok(v) => quote(v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    // Unpublishing is idempotent: a re-delivery answers 200 with the same
    // already unpublished workflow.
    let r = send(auth(
        client().post(format!("{}/workflows/{workflow_id}/unpublish", api(cfg))),
        cfg,
    ))
    .await?;
    check(&r)?;
    if r.status == 404 {
        return reject("not_found", Some(r.message()));
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(r.message()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.7, translated from its Python.
async fn workflow_archive(cfg: &Config, args: &Value) -> Verdict {
    let workflow_id = match arg_str(args, "id") {
        Ok(v) => quote(v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    // Archiving is idempotent: n8n answers 200 with the current workflow when it
    // is already archived, so a re-delivery settles the same way.
    let r = send(auth(
        client().post(format!("{}/workflows/{workflow_id}/archive", api(cfg))),
        cfg,
    ))
    .await?;
    check(&r)?;
    if r.status == 404 {
        return reject("not_found", Some(r.message()));
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(r.message()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.8, translated from its Python.
async fn workflow_unarchive(cfg: &Config, args: &Value) -> Verdict {
    let workflow_id = match arg_str(args, "id") {
        Ok(v) => quote(v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    let r = send(auth(
        client().post(format!("{}/workflows/{workflow_id}/unarchive", api(cfg))),
        cfg,
    ))
    .await?;
    check(&r)?;
    if r.status == 404 {
        return reject("not_found", Some(r.message()));
    }
    if r.status == 400 {
        // n8n answers 400 "Workflow is not archived." — the same answer a
        // re-delivery meets once the unarchive landed, so the stored workflow
        // decides: not archived is the state this call asks for.
        let g = get(cfg, &format!("{}/workflows/{workflow_id}", api(cfg)), &[]).await?;
        check(&g)?;
        let archived = g
            .json()
            .get("isArchived")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if g.status == 200 && !archived {
            return Ok(Ok(g.text));
        }
        return reject("invalid_request", Some(r.message()));
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(r.message()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.9, translated from its Python.
async fn workflow_delete(cfg: &Config, args: &Value) -> Verdict {
    let workflow_id = match arg_str(args, "id") {
        Ok(v) => quote(v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    // The delete takes the workflow's executions with it, so a re-delivery
    // after a lost response finds nothing and rejects not_found.
    let r = send(auth(
        client().delete(format!("{}/workflows/{workflow_id}", api(cfg))),
        cfg,
    ))
    .await?;
    check(&r)?;
    if r.status == 404 {
        return reject("not_found", Some(r.message()));
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(r.message()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.10, translated from its Python.
async fn execution_list(cfg: &Config, args: &Value) -> Verdict {
    // Paging is the caller's loop (one promise per page), not ours.
    let r = get(
        cfg,
        &format!("{}/executions", api(cfg)),
        &query(
            args,
            &[
                "status",
                "workflowId",
                "projectId",
                "includeData",
                "ignoreDataSizeLimit",
                "redactExecutionData",
                "limit",
                "cursor",
            ],
        ),
    )
    .await?;
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(r.message()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.11, translated from its Python.
async fn execution_get(cfg: &Config, args: &Value) -> Verdict {
    let execution_id = match arg_id(args, "id") {
        Ok(v) => quote(&v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    let r = get(
        cfg,
        &format!("{}/executions/{execution_id}", api(cfg)),
        &query(
            args,
            &["includeData", "ignoreDataSizeLimit", "redactExecutionData"],
        ),
    )
    .await?;
    check(&r)?;
    if r.status == 404 {
        return reject("not_found", Some(r.message()));
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(r.message()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.12, translated from its Python. Start the retry — or
/// re-attach to the one an earlier delivery started, found by its retryOf —
/// then poll it to a terminal state on the downstream clock; the worker
/// frame heartbeats the lease independently, so this cadence is free.
async fn execution_retry(cfg: &Config, promise: &PromiseRecord, args: &Value) -> Verdict {
    let source_id = match arg_id(args, "id") {
        Ok(v) => v,
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };
    let source_path = quote(&source_id);

    let r = get(cfg, &format!("{}/executions/{source_path}", api(cfg)), &[]).await?;
    check(&r)?;
    if r.status == 404 {
        return reject("not_found", Some(r.message()));
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(r.message()));
    }
    let Some(workflow_id) = r
        .json()
        .get("workflowId")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return Err(Err(format!(
            "execution {source_id} carries no workflowId: {}",
            r.text
        )));
    };

    let mut e = find_retry(cfg, &workflow_id, &source_id).await?;
    if e.is_none() {
        let mut body = Map::new();
        if let Some(v) = args.get("loadWorkflow") {
            body.insert("loadWorkflow".into(), v.clone());
        }
        // The call runs the workflow and answers only once the run ends, so
        // it outlives the response timeout on all but the shortest runs; the
        // retry it started keeps running and is picked up by retryOf below.
        let p = send_allowing_timeout(
            auth(
                client().post(format!("{}/executions/{source_path}/retry", api(cfg))),
                cfg,
            )
            .json(&Value::Object(body.clone())),
        )
        .await?;
        if let Some(p) = p {
            if p.status == 500 && body.get("loadWorkflow") == Some(&json!(true)) {
                // The current definition no longer holds a node the stopped
                // execution was standing on. n8n cannot start the retry, and a
                // re-delivery sends the same bytes and meets the same 500.
                return reject("workflow_changed", Some(p.message()));
            }
            check(&p)?;
            if p.status == 404 {
                return reject("not_found", Some(p.message()));
            }
            if p.status == 409 {
                // The source execution succeeded, was aborted before its data
                // was saved, or is still queued.
                return reject("not_retryable", Some(p.message()));
            }
            if p.status >= 400 {
                return reject("invalid_request", Some(p.message()));
            }
            e = Some(p.json());
        }
    }

    let mut failures = 0;
    loop {
        let now = now_ms();
        if now >= promise.timeout_at {
            return Err(Err("promise timed out".into()));
        }
        if let Some(current) = &e {
            let status = current.get("status").and_then(Value::as_str).unwrap_or_default();
            if matches!(status, "success" | "error" | "canceled" | "crashed") {
                break;
            }
        }
        // Never sleep past the promise deadline: the next iteration has to
        // observe it and stop rather than wake after the server has already
        // settled the promise.
        let remaining = Duration::from_millis((promise.timeout_at - now).max(0) as u64);
        tokio::time::sleep(cfg.poll.min(remaining)).await;

        match observe_retry(cfg, &workflow_id, &source_id, e.as_ref()).await {
            Ok(Step::Settle(verdict)) => return verdict,
            Ok(Step::Seen(next)) => e = next,
            Err(failure) => {
                // A halt is not a transient failure: it ends the delivery.
                if failure.is_ok() {
                    return Err(failure);
                }
                failures += 1;
                if failures >= 5 {
                    return Err(failure);
                }
                continue;
            }
        }
        failures = 0;
    }

    let e = e.unwrap_or(Value::Null);
    let keys = [
        "id",
        "status",
        "finished",
        "mode",
        "retryOf",
        "retrySuccessId",
        "workflowId",
        "startedAt",
        "stoppedAt",
        "waitTill",
    ];
    match e.get("status").and_then(Value::as_str).unwrap_or_default() {
        // The 4.12.2 Resolved mapping.
        "success" => Ok(Ok(Value::Object(pick(&e, &keys)).to_string())),
        "error" => reject("execution_failed", Some(e)),
        "canceled" => reject("execution_canceled", Some(e)),
        _ => reject("execution_crashed", Some(e)),
    }
}

/// One turn of §4.12.5's poll loop.
enum Step {
    /// The execution as it now stands — or none found yet.
    Seen(Option<Value>),
    /// A verdict the loop answers with straight away.
    Settle(Verdict),
}

async fn observe_retry(
    cfg: &Config,
    workflow_id: &str,
    source_id: &str,
    e: Option<&Value>,
) -> Result<Step, Failure> {
    let Some(current) = e else {
        return Ok(Step::Seen(find_retry(cfg, workflow_id, source_id).await?));
    };
    let retry_id = quote(&py_str(current.get("id")));
    let g = get(cfg, &format!("{}/executions/{retry_id}", api(cfg)), &[]).await?;
    check(&g)?;
    if g.status == 404 {
        return Ok(Step::Settle(reject("deleted", None)));
    }
    if g.status >= 400 {
        return Ok(Step::Settle(reject("invalid_request", Some(g.message()))));
    }
    Ok(Step::Seen(Some(g.json())))
}

/// §4.12.5's `_executions_page`: any 4xx here is unclassified — this read is
/// the plugin's own bookkeeping, not one of the operation's outcomes.
async fn executions_page(cfg: &Config, params: &[(String, String)]) -> Result<Value, Failure> {
    let r = get(cfg, &format!("{}/executions", api(cfg)), params).await?;
    check(&r)?;
    if r.status >= 400 {
        return Err(Err(r.text));
    }
    Ok(r.json())
}

/// A retry carries retryOf = the source execution id and always takes a
/// higher id than its source. /executions answers newest id first and omits
/// running executions unless status=running is asked for, so both are read.
async fn find_retry(
    cfg: &Config,
    workflow_id: &str,
    source_id: &str,
) -> Result<Option<Value>, Failure> {
    let source = parse_id(source_id)?;
    for status in ["running", "any"] {
        let mut cursor: Option<String> = None;
        loop {
            let mut params = vec![
                ("workflowId".to_string(), workflow_id.to_string()),
                ("limit".to_string(), "250".to_string()),
            ];
            if status != "any" {
                params.push(("status".to_string(), status.to_string()));
            }
            if let Some(c) = &cursor {
                params.push(("cursor".to_string(), c.clone()));
            }
            let page = executions_page(cfg, &params).await?;
            let empty = Vec::new();
            let mut older = false;
            for e in page["data"].as_array().unwrap_or(&empty) {
                if py_str_or_empty(e.get("retryOf")) == source_id {
                    return Ok(Some(e.clone()));
                }
                if parse_id(&py_str(e.get("id")))? < source {
                    older = true;
                    break;
                }
            }
            cursor = page
                .get("nextCursor")
                .and_then(Value::as_str)
                .map(str::to_string);
            if older || cursor.is_none() {
                break;
            }
        }
    }
    Ok(None)
}

/// Specification §4.13, translated from its Python.
async fn execution_stop(cfg: &Config, args: &Value) -> Verdict {
    let execution_id = match arg_id(args, "id") {
        Ok(v) => quote(&v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };
    let keys = ["mode", "status", "finished", "startedAt", "stoppedAt"];

    let p = send(auth(
        client().post(format!("{}/executions/{execution_id}/stop", api(cfg))),
        cfg,
    ))
    .await?;
    if p.status == 500 {
        // n8n answers 500 for an execution whose status is not new, running,
        // waiting or unknown, and the same 500 for an ordinary server fault, so
        // the execution's own status separates them.
        let g = get(cfg, &format!("{}/executions/{execution_id}", api(cfg)), &[]).await?;
        check(&g)?;
        if g.status == 404 {
            return reject("not_found", Some(g.message()));
        }
        if g.status >= 400 {
            return reject("invalid_request", Some(g.message()));
        }
        let e = g.json();
        return match e.get("status").and_then(Value::as_str).unwrap_or_default() {
            // Already stopped — by an earlier delivery of this promise or by
            // someone else; the state this call asks for holds either way.
            // The 4.13.2 Resolved mapping.
            "canceled" => Ok(Ok(Value::Object(pick(&e, &keys)).to_string())),
            "success" | "error" | "crashed" => reject("not_stoppable", Some(e)),
            _ => Err(Err(p.text)),
        };
    }
    check(&p)?;
    if p.status == 404 {
        return reject("not_found", Some(p.message()));
    }
    if p.status >= 400 {
        return reject("invalid_request", Some(p.message()));
    }
    // The 4.13.2 Resolved mapping.
    Ok(Ok(Value::Object(pick(&p.json(), &keys)).to_string()))
}

/// Specification §4.14, translated from its Python.
async fn execution_delete(cfg: &Config, args: &Value) -> Verdict {
    let execution_id = match arg_id(args, "id") {
        Ok(v) => quote(&v),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    // The record is hard-deleted, so a re-delivery after a lost response finds
    // nothing and rejects not_found.
    let r = send(auth(
        client().delete(format!("{}/executions/{execution_id}", api(cfg))),
        cfg,
    ))
    .await?;
    check(&r)?;
    if r.status == 404 {
        return reject("not_found", Some(r.message()));
    }
    if r.status == 400 {
        // n8n answers 400 "Cannot delete a running execution", and the same 400
        // for an id it cannot parse, so the execution's own status separates
        // them. Only status running blocks the delete.
        let g = get(cfg, &format!("{}/executions/{execution_id}", api(cfg)), &[]).await?;
        check(&g)?;
        if g.status == 200 && g.json().get("status") == Some(&json!("running")) {
            return reject("not_deletable", Some(r.message()));
        }
        return reject("invalid_request", Some(r.message()));
    }
    if r.status >= 400 {
        return reject("invalid_request", Some(r.message()));
    }
    Ok(Ok(r.text))
}

// ─── HTTP ─────────────────────────────────────────────────────────────────────

/// The API root: §2's `base_url` and the specification's `API`.
fn api(cfg: &Config) -> String {
    format!("{}/api/v1", cfg.base_url)
}

/// §3: the API key rides on every request, and every response is JSON.
fn auth(req: reqwest::RequestBuilder, cfg: &Config) -> reqwest::RequestBuilder {
    req.header("X-N8N-API-KEY", &cfg.api_key)
        .header("Accept", "application/json")
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

    /// `response.body.message` — what the documented rejections quote. An
    /// absent field is JSON null, never a crash.
    fn message(&self) -> Value {
        self.json().get("message").cloned().unwrap_or(Value::Null)
    }
}

/// The specification's `_check`: the classification every response that is
/// not one of an operation's enumerated outcomes falls through to.
fn check(r: &Response) -> Result<(), Failure> {
    // 401 is a missing or rejected API key; 403 is a key whose scopes do not
    // cover the call. Both end only when an operator issues or re-scopes a key.
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

async fn get(cfg: &Config, url: &str, params: &[(String, String)]) -> Result<Response, Failure> {
    send(auth(client().get(url), cfg).query(params)).await
}

async fn send(req: reqwest::RequestBuilder) -> Result<Response, Failure> {
    // A request that produced no response is unclassified: release, and let
    // redelivery retry it. Every operation is safe to re-enter.
    match send_allowing_timeout(req).await? {
        Some(r) => Ok(r),
        None => Err(Err("request timed out".into())),
    }
}

/// `send`, except that a response which never arrives inside the request
/// timeout is `None` rather than a failure — §4.12.5 catches
/// `requests.Timeout` on the retry call, whose run outlives it.
async fn send_allowing_timeout(req: reqwest::RequestBuilder) -> Result<Option<Response>, Failure> {
    let resp = match req.timeout(Duration::from_secs(10)).send().await {
        Ok(resp) => resp,
        Err(e) if e.is_timeout() => return Ok(None),
        Err(e) => return Err(Err(e.to_string())),
    };
    let status = resp.status().as_u16();
    match resp.text().await {
        Ok(text) => Ok(Some(Response { status, text })),
        Err(e) if e.is_timeout() => Ok(None),
        Err(e) => Err(Err(e.to_string())),
    }
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

/// A Resolved mapping that names its keys: those of them the response
/// carries, in the schema's order.
fn pick(value: &Value, keys: &[&str]) -> Map<String, Value> {
    keys.iter()
        .filter_map(|k| value.get(*k).map(|v| ((*k).to_string(), v.clone())))
        .collect()
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

/// A required id the Param schema types `["string", "number"]`, rendered the
/// way the specification's `str(args["id"])` renders it.
fn arg_id(args: &Value, key: &str) -> Result<String, String> {
    match args.get(key) {
        Some(Value::String(s)) => Ok(s.clone()),
        Some(Value::Number(n)) => Ok(n.to_string()),
        _ => Err(format!(
            "args.{key} is required and must be a string or a number"
        )),
    }
}

/// An execution id is a decimal integer; the specification compares them
/// with `int(...)`. One that will not parse is unclassified.
fn parse_id(id: &str) -> Result<i64, Failure> {
    id.parse::<i64>()
        .map_err(|e| Err(format!("execution id {id:?} is not a decimal integer: {e}")))
}

/// The specification's `str(v)` over a JSON value: a string is itself, every
/// other value its JSON rendering. An absent field is the empty string
/// rather than a crash.
fn py_str(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

/// The specification's `str(v or "")`: a falsy value — null, 0, "" — is the
/// empty string, which no execution id equals.
fn py_str_or_empty(value: Option<&Value>) -> String {
    match value {
        Some(Value::Bool(false)) => String::new(),
        Some(Value::Number(n)) if n.as_f64() == Some(0.0) => String::new(),
        other => py_str(other),
    }
}

/// The specification's `_query`: n8n reads query values as text — active and
/// excludePinnedData are the strings "true"/"false", and the booleans of
/// /executions are parsed from the same spelling, which is not Python's
/// True/False. A key the args do not carry is not sent at all.
fn query(args: &Value, names: &[&str]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for name in names {
        let Some(value) = args.get(*name) else {
            continue;
        };
        match value {
            // A null is dropped rather than sent as the string "null".
            Value::Null => continue,
            Value::Bool(b) => out.push(((*name).to_string(), b.to_string())),
            Value::String(s) => out.push(((*name).to_string(), s.clone())),
            other => out.push(((*name).to_string(), other.to_string())),
        }
    }
    out
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
