//! The baserow plugin. Everything provider-specific lives here, translated
//! from plugins/baserow/spec/specification.md.

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
    pub email: String,
    pub password: String,
    #[serde(default = "poll_default", with = "humantime_serde")]
    pub poll: Duration,
    #[serde(default, with = "humantime_serde")]
    pub poll_export: Option<Duration>,
    #[serde(default, with = "humantime_serde")]
    pub poll_table: Option<Duration>,
}

fn poll_default() -> Duration {
    Duration::from_secs(2)
}

impl Config {
    /// §2 `poll_export = poll`: unset cascades to `poll`.
    fn poll_export(&self) -> Duration {
        self.poll_export.unwrap_or(self.poll)
    }

    /// §2 `poll_table = poll`: unset cascades to `poll`.
    fn poll_table(&self) -> Duration {
        self.poll_table.unwrap_or(self.poll)
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

/// §4.10.5's `EXPORT_TERMINAL`: `finished`, `failed`, `cancelled` and
/// `expired` are terminal export-job states; `pending` and `exporting` are
/// running.
const EXPORT_TERMINAL: [&str; 4] = ["finished", "failed", "cancelled", "expired"];

/// §4.10.5's `JOB_TERMINAL`: `finished`, `failed` and `cancelled` are
/// terminal job states; `pending` and `started` are running.
const JOB_TERMINAL: [&str; 3] = ["finished", "failed", "cancelled"];

/// The `error` field of a Baserow 4xx body is a documented machine-readable
/// enum, listed per endpoint in the OpenAPI. These two name a standing
/// permission problem: the credentials are valid, but the account is not a
/// member of the table's workspace, or holds no rights on the table.
const PERMISSION_ERRORS: [&str; 2] = ["ERROR_USER_NOT_IN_GROUP", "ERROR_NO_PERMISSION_TO_TABLE"];

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
        "row.list" => row_list(config, &args).await,
        "row.get" => row_get(config, &args).await,
        "row.create" => row_create(config, &args).await,
        "row.update" => row_update(config, &args).await,
        "row.delete" => row_delete(config, &args).await,
        "row.move" => row_move(config, &args).await,
        "rows.create" => rows_create(config, &args).await,
        "rows.update" => rows_update(config, &args).await,
        "rows.delete" => rows_delete(config, &args).await,
        "export.create" => export_create(config, promise, &args).await,
        "export.get" => export_get(config, &args).await,
        "table.import" => table_import(config, promise, &args).await,
        "job.get" => job_get(config, &args).await,
        "database.list" => database_list(config).await,
        "table.list" => table_list(config, &args).await,
        "field.list" => field_list(config, &args).await,
        "view.list" => view_list(config, &args).await,
        "rowhistory.list" => rowhistory_list(config, &args).await,
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
async fn row_list(cfg: &Config, args: &Value) -> Verdict {
    let auth = token(cfg).await?;
    let table_id = match arg_int(args, "table_id") {
        Ok(v) => v,
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    let mut params = query(
        args,
        &[
            "page",
            "size",
            "search",
            "search_mode",
            "order_by",
            "include",
            "exclude",
            "user_field_names",
            "view_id",
        ],
    );
    if let Some(filters) = args.get("filters") {
        // json.dumps of the filter tree: the same JSON document, rendered
        // without the spaces Python's default separators add.
        params.push(("filters".to_string(), filters.to_string()));
    }

    let r = get(
        &format!("{}/api/database/rows/table/{table_id}/", cfg.base_url),
        &auth,
        &params,
    )
    .await?;
    if r.status == 404 {
        match r.error().as_deref() {
            Some("ERROR_TABLE_DOES_NOT_EXIST") => {
                return reject("table_not_found", Some(r.detail()))
            }
            Some("ERROR_FIELD_DOES_NOT_EXIST") => {
                return reject("field_not_found", Some(r.detail()))
            }
            Some("ERROR_VIEW_DOES_NOT_EXIST") => return reject("view_not_found", Some(r.detail())),
            _ => {}
        }
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(r.detail()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.2, translated from its Python.
async fn row_get(cfg: &Config, args: &Value) -> Verdict {
    let auth = token(cfg).await?;
    let (table_id, row_id) = match (arg_int(args, "table_id"), arg_int(args, "row_id")) {
        (Ok(t), Ok(r)) => (t, r),
        (Err(e), _) | (_, Err(e)) => return reject("invalid_request", Some(json!(e))),
    };

    let r = get(
        &format!(
            "{}/api/database/rows/table/{table_id}/{row_id}/",
            cfg.base_url
        ),
        &auth,
        &query(args, &["include", "user_field_names", "view"]),
    )
    .await?;
    if r.status == 404 {
        match r.error().as_deref() {
            Some("ERROR_TABLE_DOES_NOT_EXIST") => {
                return reject("table_not_found", Some(r.detail()))
            }
            Some("ERROR_ROW_DOES_NOT_EXIST") => return reject("row_not_found", Some(r.detail())),
            Some("ERROR_VIEW_DOES_NOT_EXIST") => return reject("view_not_found", Some(r.detail())),
            _ => {}
        }
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(r.detail()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.3, translated from its Python.
async fn row_create(cfg: &Config, args: &Value) -> Verdict {
    let auth = token(cfg).await?;
    let table_id = match arg_int(args, "table_id") {
        Ok(v) => v,
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };
    let values = match arg_object(args, "values") {
        Ok(v) => v,
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    // Unkeyed POST: a re-delivery costs one extra row in the table, and the
    // promise resolves with the second one.
    let r = send(
        client()
            .post(format!(
                "{}/api/database/rows/table/{table_id}/",
                cfg.base_url
            ))
            .header("Authorization", &auth)
            .query(&query(
                args,
                &["before", "user_field_names", "view", "send_webhook_events"],
            ))
            .json(values),
    )
    .await?;
    if r.status == 404 {
        match r.error().as_deref() {
            Some("ERROR_TABLE_DOES_NOT_EXIST") => {
                return reject("table_not_found", Some(r.detail()))
            }
            Some("ERROR_ROW_DOES_NOT_EXIST") => return reject("row_not_found", Some(r.detail())),
            Some("ERROR_VIEW_DOES_NOT_EXIST") => return reject("view_not_found", Some(r.detail())),
            _ => {}
        }
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(r.detail()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.4, translated from its Python.
async fn row_update(cfg: &Config, args: &Value) -> Verdict {
    let auth = token(cfg).await?;
    let (table_id, row_id) = match (arg_int(args, "table_id"), arg_int(args, "row_id")) {
        (Ok(t), Ok(r)) => (t, r),
        (Err(e), _) | (_, Err(e)) => return reject("invalid_request", Some(json!(e))),
    };
    let values = match arg_object(args, "values") {
        Ok(v) => v,
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    // A repeat of this PATCH writes the same values to the same row, so a
    // re-delivery costs one request and changes nothing.
    let r = send(
        client()
            .patch(format!(
                "{}/api/database/rows/table/{table_id}/{row_id}/",
                cfg.base_url
            ))
            .header("Authorization", &auth)
            .query(&query(
                args,
                &["user_field_names", "view", "send_webhook_events"],
            ))
            .json(values),
    )
    .await?;
    if r.status == 404 {
        match r.error().as_deref() {
            Some("ERROR_TABLE_DOES_NOT_EXIST") => {
                return reject("table_not_found", Some(r.detail()))
            }
            Some("ERROR_ROW_DOES_NOT_EXIST") => return reject("row_not_found", Some(r.detail())),
            Some("ERROR_VIEW_DOES_NOT_EXIST") => return reject("view_not_found", Some(r.detail())),
            _ => {}
        }
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(r.detail()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.5, translated from its Python.
async fn row_delete(cfg: &Config, args: &Value) -> Verdict {
    let auth = token(cfg).await?;
    let (table_id, row_id) = match (arg_int(args, "table_id"), arg_int(args, "row_id")) {
        (Ok(t), Ok(r)) => (t, r),
        (Err(e), _) | (_, Err(e)) => return reject("invalid_request", Some(json!(e))),
    };

    let r = send(
        client()
            .delete(format!(
                "{}/api/database/rows/table/{table_id}/{row_id}/",
                cfg.base_url
            ))
            .header("Authorization", &auth)
            .query(&query(args, &["view", "send_webhook_events"])),
    )
    .await?;
    if r.status == 404 {
        match r.error().as_deref() {
            Some("ERROR_TABLE_DOES_NOT_EXIST") => {
                return reject("table_not_found", Some(r.detail()))
            }
            Some("ERROR_ROW_DOES_NOT_EXIST") => return reject("row_not_found", Some(r.detail())),
            Some("ERROR_VIEW_DOES_NOT_EXIST") => return reject("view_not_found", Some(r.detail())),
            _ => {}
        }
    }
    if r.status == 400 && r.error().as_deref() == Some("ERROR_CANNOT_DELETE_ALREADY_DELETED_ITEM") {
        return reject("already_deleted", Some(r.detail()));
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(r.detail()));
    }
    Ok(Ok(json!({}).to_string()))
}

/// Specification §4.6, translated from its Python.
async fn row_move(cfg: &Config, args: &Value) -> Verdict {
    let auth = token(cfg).await?;
    let (table_id, row_id) = match (arg_int(args, "table_id"), arg_int(args, "row_id")) {
        (Ok(t), Ok(r)) => (t, r),
        (Err(e), _) | (_, Err(e)) => return reject("invalid_request", Some(json!(e))),
    };

    // Moving a row already at the requested position is a no-op, so a
    // re-delivery costs one request and changes nothing.
    let r = send(
        client()
            .patch(format!(
                "{}/api/database/rows/table/{table_id}/{row_id}/move/",
                cfg.base_url
            ))
            .header("Authorization", &auth)
            .query(&query(
                args,
                &[
                    "before_id",
                    "user_field_names",
                    "view",
                    "send_webhook_events",
                ],
            )),
    )
    .await?;
    if r.status == 404 {
        match r.error().as_deref() {
            Some("ERROR_TABLE_DOES_NOT_EXIST") => {
                return reject("table_not_found", Some(r.detail()))
            }
            Some("ERROR_ROW_DOES_NOT_EXIST") => return reject("row_not_found", Some(r.detail())),
            Some("ERROR_VIEW_DOES_NOT_EXIST") => return reject("view_not_found", Some(r.detail())),
            _ => {}
        }
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(r.detail()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.7, translated from its Python.
async fn rows_create(cfg: &Config, args: &Value) -> Verdict {
    let auth = token(cfg).await?;
    let table_id = match arg_int(args, "table_id") {
        Ok(v) => v,
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };
    let rows = match arg_array(args, "rows") {
        Ok(v) => v,
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    // Unkeyed POST: a re-delivery costs one extra copy of every row.
    let r = send(
        client()
            .post(format!(
                "{}/api/database/rows/table/{table_id}/batch/",
                cfg.base_url
            ))
            .header("Authorization", &auth)
            .query(&query(
                args,
                &[
                    "before",
                    "include_metadata",
                    "user_field_names",
                    "view",
                    "send_webhook_events",
                ],
            ))
            .json(&json!({ "items": rows })),
    )
    .await?;
    if r.status == 404 {
        match r.error().as_deref() {
            Some("ERROR_TABLE_DOES_NOT_EXIST") => {
                return reject("table_not_found", Some(r.detail()))
            }
            Some("ERROR_ROW_DOES_NOT_EXIST") => return reject("row_not_found", Some(r.detail())),
            Some("ERROR_VIEW_DOES_NOT_EXIST") => return reject("view_not_found", Some(r.detail())),
            _ => {}
        }
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(r.detail()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.8, translated from its Python.
async fn rows_update(cfg: &Config, args: &Value) -> Verdict {
    let auth = token(cfg).await?;
    let table_id = match arg_int(args, "table_id") {
        Ok(v) => v,
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };
    let rows = match arg_array(args, "rows") {
        Ok(v) => v,
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    // Every item names the row it writes to, so a re-delivery writes the same
    // values to the same rows and changes nothing.
    let r = send(
        client()
            .patch(format!(
                "{}/api/database/rows/table/{table_id}/batch/",
                cfg.base_url
            ))
            .header("Authorization", &auth)
            .query(&query(
                args,
                &[
                    "include_metadata",
                    "user_field_names",
                    "view",
                    "send_webhook_events",
                ],
            ))
            .json(&json!({ "items": rows })),
    )
    .await?;
    if r.status == 404 {
        match r.error().as_deref() {
            Some("ERROR_TABLE_DOES_NOT_EXIST") => {
                return reject("table_not_found", Some(r.detail()))
            }
            Some("ERROR_ROW_DOES_NOT_EXIST") => return reject("row_not_found", Some(r.detail())),
            Some("ERROR_VIEW_DOES_NOT_EXIST") => return reject("view_not_found", Some(r.detail())),
            _ => {}
        }
    }
    if r.status == 400 && r.error().as_deref() == Some("ERROR_ROW_IDS_NOT_UNIQUE") {
        return reject("row_ids_not_unique", Some(r.detail()));
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(r.detail()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.9, translated from its Python.
async fn rows_delete(cfg: &Config, args: &Value) -> Verdict {
    let auth = token(cfg).await?;
    let table_id = match arg_int(args, "table_id") {
        Ok(v) => v,
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };
    let row_ids = match arg_array(args, "row_ids") {
        Ok(v) => v,
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    let r = send(
        client()
            .post(format!(
                "{}/api/database/rows/table/{table_id}/batch-delete/",
                cfg.base_url
            ))
            .header("Authorization", &auth)
            .query(&query(args, &["view", "send_webhook_events"]))
            .json(&json!({ "items": row_ids })),
    )
    .await?;
    if r.status == 404 {
        match r.error().as_deref() {
            Some("ERROR_TABLE_DOES_NOT_EXIST") => {
                return reject("table_not_found", Some(r.detail()))
            }
            Some("ERROR_ROW_DOES_NOT_EXIST") => return reject("row_not_found", Some(r.detail())),
            Some("ERROR_VIEW_DOES_NOT_EXIST") => return reject("view_not_found", Some(r.detail())),
            _ => {}
        }
    }
    if r.status == 400 {
        match r.error().as_deref() {
            Some("ERROR_ROW_IDS_NOT_UNIQUE") => {
                return reject("row_ids_not_unique", Some(r.detail()))
            }
            Some("ERROR_CANNOT_DELETE_ALREADY_DELETED_ITEM") => {
                return reject("already_deleted", Some(r.detail()))
            }
            _ => {}
        }
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(r.detail()));
    }
    Ok(Ok(json!({}).to_string()))
}

/// Specification §4.10, translated from its Python. Begin, then poll on the
/// downstream clock — the worker frame heartbeats the lease independently,
/// so this cadence may back off freely.
async fn export_create(cfg: &Config, promise: &PromiseRecord, args: &Value) -> Verdict {
    let mut auth = token(cfg).await?;
    let table_id = match arg_int(args, "table_id") {
        Ok(v) => v,
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    let mut body: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    body.remove("table_id");

    // The export job carries no client-supplied identity and no listing
    // endpoint exposes it, so a re-delivery costs a second export — and,
    // because creating a job sets this account's other unfinished export
    // jobs to "cancelled", it also cancels the job the first attempt made.
    let r = send(
        client()
            .post(format!(
                "{}/api/database/export/table/{table_id}/",
                cfg.base_url
            ))
            .header("Authorization", &auth)
            .json(&Value::Object(body)),
    )
    .await?;
    if r.status == 404 {
        match r.error().as_deref() {
            Some("ERROR_TABLE_DOES_NOT_EXIST") => {
                return reject("table_not_found", Some(r.detail()))
            }
            Some("ERROR_VIEW_DOES_NOT_EXIST") => return reject("view_not_found", Some(r.detail())),
            _ => {}
        }
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(r.detail()));
    }

    let mut job = r.json();
    let Some(job_id) = job.get("id").and_then(Value::as_i64) else {
        return Err(Err(format!("export job has no id: {}", r.text)));
    };
    let job_url = format!("{}/api/database/export/{job_id}/", cfg.base_url);
    let mut failures = 0;
    while !EXPORT_TERMINAL.contains(&state_of(&job)) {
        let now = now_ms();
        if now >= promise.timeout_at {
            return Err(Err("promise timed out".to_string()));
        }
        // Never sleep past the promise deadline: the next iteration has to
        // observe it and stop rather than wake after the server has already
        // settled the promise.
        let remaining = Duration::from_millis((promise.timeout_at - now).max(0) as u64);
        tokio::time::sleep(cfg.poll_export().min(remaining)).await;

        // The specification's try/except around one poll: a halt propagates
        // at once, every other failure is counted, and five consecutive ones
        // end the attempt.
        match export_poll(cfg, &mut auth, &job_url).await {
            Ok(Poll::Job(j)) => {
                job = j;
                failures = 0;
            }
            Ok(Poll::Reauth) => continue,
            Ok(Poll::Verdict(v)) => return v,
            Err(halt @ Ok(_)) => return Err(halt),
            Err(release) => {
                failures += 1;
                if failures >= 5 {
                    return Err(release);
                }
            }
        }
    }

    let keys = [
        "id",
        "table",
        "view",
        "exporter_type",
        "state",
        "exported_file_name",
        "created_at",
        "progress_percentage",
        "url",
    ];
    match state_of(&job) {
        "finished" => Ok(Ok(pick(&job, &keys))), // the 4.10.2 Resolved mapping
        "cancelled" => reject("cancelled", Some(job)),
        "expired" => reject("expired", Some(job)),
        _ => reject("export_failed", Some(job)),
    }
}

/// What one turn of §4.10.5's poll loop can produce.
enum Poll {
    /// The export job as the provider last reported it.
    Job(Value),
    /// A 401: the access token was re-minted, take the loop round again.
    Reauth,
    /// A documented permanent outcome of the poll itself.
    Verdict(Verdict),
}

/// One turn of §4.10.5's poll loop — the body of its `try`, so that every
/// failure raised inside it is one the loop's failure streak counts.
async fn export_poll(cfg: &Config, auth: &mut String, job_url: &str) -> Result<Poll, Failure> {
    let g = get(job_url, auth, &[]).await?;
    if g.status == 401 {
        // BASEROW_ACCESS_TOKEN_LIFETIME_MINUTES is 10 by default, so a poll
        // loop outliving that window re-mints on the next 401.
        *auth = token(cfg).await?;
        return Ok(Poll::Reauth);
    }
    if g.status == 404 {
        return Ok(Poll::Verdict(reject("job_not_found", Some(g.detail()))));
    }
    check(&g)?;
    if g.status >= 400 {
        return Ok(Poll::Verdict(reject("invalid_request", Some(g.detail()))));
    }
    Ok(Poll::Job(g.json()))
}

/// Specification §4.11, translated from its Python.
async fn export_get(cfg: &Config, args: &Value) -> Verdict {
    let auth = token(cfg).await?;
    let job_id = match arg_int(args, "job_id") {
        Ok(v) => v,
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    let r = get(
        &format!("{}/api/database/export/{job_id}/", cfg.base_url),
        &auth,
        &[],
    )
    .await?;
    if r.status == 404 {
        return reject("job_not_found", Some(r.detail()));
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(r.detail()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.12, translated from its Python. `fetch_then_create`: the
/// job an earlier attempt created is recovered by the stamp it carries, so a
/// re-delivery attaches to it rather than importing the rows a second time.
async fn table_import(cfg: &Config, promise: &PromiseRecord, args: &Value) -> Verdict {
    let mut auth = token(cfg).await?;
    let table_id = match arg_int(args, "table_id") {
        Ok(v) => v,
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };
    let data = match arg_array(args, "data") {
        Ok(v) => v.clone(),
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };
    let stamp = sanitize(&promise.id);

    let mut job = match find_import_job(cfg, &auth, &stamp).await? {
        Some(job) => job,
        None => {
            let mut body = Map::new();
            body.insert("data".to_string(), data);
            body.insert("original_file_name".to_string(), json!(stamp));
            for k in ["configuration", "importer_type"] {
                if let Some(v) = args.get(k) {
                    body.insert(k.to_string(), v.clone());
                }
            }
            let r = send(
                client()
                    .post(format!(
                        "{}/api/database/tables/{table_id}/import/async/",
                        cfg.base_url
                    ))
                    .header("Authorization", &auth)
                    .json(&Value::Object(body)),
            )
            .await?;
            if r.status == 404 {
                return reject("table_not_found", Some(r.detail()));
            }
            if r.status == 400 && r.error().as_deref() == Some("ERROR_MAX_JOB_COUNT_EXCEEDED") {
                // Clears on its own once this account's running file_import
                // jobs finish.
                return Err(Err(r.text));
            }
            check(&r)?;
            if r.status >= 400 {
                return reject("invalid_request", Some(r.detail()));
            }
            r.json()
        }
    };

    let Some(job_id) = job.get("id").and_then(Value::as_i64) else {
        return Err(Err(format!("import job has no id: {job}")));
    };
    let job_url = format!("{}/api/jobs/{job_id}/", cfg.base_url);
    while !JOB_TERMINAL.contains(&state_of(&job)) {
        let now = now_ms();
        if now >= promise.timeout_at {
            return Err(Err("promise timed out".to_string()));
        }
        // Never sleep past the promise deadline.
        let remaining = Duration::from_millis((promise.timeout_at - now).max(0) as u64);
        tokio::time::sleep(cfg.poll_table().min(remaining)).await;

        let g = get(&job_url, &auth, &[]).await?;
        if g.status == 401 {
            auth = token(cfg).await?;
            continue;
        }
        if g.status == 404 {
            return reject("job_not_found", Some(g.detail()));
        }
        check(&g)?;
        if g.status >= 400 {
            return reject("invalid_request", Some(g.detail()));
        }
        job = g.json();
    }

    if state_of(&job) == "finished" {
        let keys = [
            "id",
            "type",
            "state",
            "progress_percentage",
            "table_id",
            "database_id",
            "original_file_name",
            "human_readable_error",
            "report",
        ];
        // A finished job can still carry report.failing_rows: those rows were
        // rejected by their field types and were not created.
        return Ok(Ok(pick(&job, &keys))); // the 4.12.2 Resolved mapping
    }
    if state_of(&job) == "cancelled" {
        return reject("cancelled", Some(job));
    }
    reject("import_failed", Some(job))
}

/// Specification §4.12.5's `_find_import_job`.
///
/// A file_import job echoes original_file_name, and GET /api/jobs/ lists
/// this account's jobs newest first, at most 100 per page, retained
/// BASEROW_JOB_EXPIRATION_TIME_LIMIT (30 days). Ten pages bound the scan;
/// a stamp older than the account's last 1000 file_import jobs is not
/// found and the import is repeated, duplicating its rows.
async fn find_import_job(cfg: &Config, auth: &str, stamp: &str) -> Result<Option<Value>, Failure> {
    let mut offset = 0;
    for _ in 0..10 {
        let params = [
            ("type".to_string(), "file_import".to_string()),
            ("limit".to_string(), "100".to_string()),
            ("offset".to_string(), offset.to_string()),
        ];
        let r = get(&format!("{}/api/jobs/", cfg.base_url), auth, &params).await?;
        check(&r)?;
        if r.status >= 400 {
            return Err(Err(r.text));
        }
        let jobs = r
            .json()
            .get("jobs")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for job in &jobs {
            if job.get("original_file_name").and_then(Value::as_str) == Some(stamp) {
                return Ok(Some(job.clone()));
            }
        }
        if jobs.len() < 100 {
            return Ok(None);
        }
        offset += 100;
    }
    Ok(None)
}

/// Specification §4.13, translated from its Python.
async fn job_get(cfg: &Config, args: &Value) -> Verdict {
    let auth = token(cfg).await?;
    let job_id = match arg_int(args, "job_id") {
        Ok(v) => v,
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    let r = get(&format!("{}/api/jobs/{job_id}/", cfg.base_url), &auth, &[]).await?;
    if r.status == 404 {
        return reject("job_not_found", Some(r.detail()));
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(r.detail()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.14, translated from its Python.
async fn database_list(cfg: &Config) -> Verdict {
    let auth = token(cfg).await?;

    let r = get(&format!("{}/api/applications/", cfg.base_url), &auth, &[]).await?;
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(r.detail()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.15, translated from its Python.
async fn table_list(cfg: &Config, args: &Value) -> Verdict {
    let auth = token(cfg).await?;
    let database_id = match arg_int(args, "database_id") {
        Ok(v) => v,
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    let r = get(
        &format!(
            "{}/api/database/tables/database/{database_id}/",
            cfg.base_url
        ),
        &auth,
        &[],
    )
    .await?;
    if r.status == 404 {
        return reject("database_not_found", Some(r.detail()));
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(r.detail()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.16, translated from its Python.
async fn field_list(cfg: &Config, args: &Value) -> Verdict {
    let auth = token(cfg).await?;
    let table_id = match arg_int(args, "table_id") {
        Ok(v) => v,
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    let r = get(
        &format!("{}/api/database/fields/table/{table_id}/", cfg.base_url),
        &auth,
        &[],
    )
    .await?;
    if r.status == 404 {
        return reject("table_not_found", Some(r.detail()));
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(r.detail()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.17, translated from its Python.
async fn view_list(cfg: &Config, args: &Value) -> Verdict {
    let auth = token(cfg).await?;
    let table_id = match arg_int(args, "table_id") {
        Ok(v) => v,
        Err(e) => return reject("invalid_request", Some(json!(e))),
    };

    let r = get(
        &format!("{}/api/database/views/table/{table_id}/", cfg.base_url),
        &auth,
        &query(args, &["type", "limit", "include"]),
    )
    .await?;
    if r.status == 404 {
        return reject("table_not_found", Some(r.detail()));
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(r.detail()));
    }
    Ok(Ok(r.text))
}

/// Specification §4.18, translated from its Python.
async fn rowhistory_list(cfg: &Config, args: &Value) -> Verdict {
    let auth = token(cfg).await?;
    let (table_id, row_id) = match (arg_int(args, "table_id"), arg_int(args, "row_id")) {
        (Ok(t), Ok(r)) => (t, r),
        (Err(e), _) | (_, Err(e)) => return reject("invalid_request", Some(json!(e))),
    };

    let r = get(
        &format!(
            "{}/api/database/rows/table/{table_id}/{row_id}/history/",
            cfg.base_url
        ),
        &auth,
        &query(args, &["limit", "offset"]),
    )
    .await?;
    if r.status == 404 {
        match r.error().as_deref() {
            Some("ERROR_TABLE_DOES_NOT_EXIST") => {
                return reject("table_not_found", Some(r.detail()))
            }
            Some("ERROR_ROW_DOES_NOT_EXIST") => return reject("row_not_found", Some(r.detail())),
            _ => {}
        }
    }
    check(&r)?;
    if r.status >= 400 {
        return reject("invalid_request", Some(r.detail()));
    }
    Ok(Ok(r.text))
}

// ─── Authentication (§3) ──────────────────────────────────────────────────────

/// The specification's `_token`: exchange the configured credentials for a
/// JWT access token.
async fn token(cfg: &Config) -> Result<String, Failure> {
    let r = send(
        client()
            .post(format!("{}/api/user/token-auth/", cfg.base_url))
            .json(&json!({"email": cfg.email, "password": cfg.password})),
    )
    .await?;
    if r.status == 429 || r.status >= 500 {
        return Err(Err(r.text));
    }
    // 401 ERROR_INVALID_CREDENTIALS / ERROR_DEACTIVATED_USER /
    // ERROR_AUTH_PROVIDER_DISABLED / ERROR_EMAIL_VERIFICATION_REQUIRED.
    if r.status >= 400 {
        return Err(Ok(r.text));
    }
    let Some(access) = r
        .json()
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return Err(Err(format!("no access_token in token response: {}", r.text)));
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
    fn json(&self) -> Value {
        serde_json::from_str(&self.text).unwrap_or(Value::Null)
    }

    /// The specification's `_error`: the documented machine-readable failure
    /// code of a Baserow 4xx body.
    fn error(&self) -> Option<String> {
        self.json()
            .get("error")
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    /// The specification's `_detail`: `response.body.detail`, or the raw body
    /// when it is not JSON. An absent field is JSON null, never a crash.
    fn detail(&self) -> Value {
        match serde_json::from_str::<Value>(&self.text) {
            Ok(v) => v.get("detail").cloned().unwrap_or(Value::Null),
            Err(_) => Value::String(self.text.clone()),
        }
    }
}

/// The specification's `_check`: the classification every response that is
/// not one of an operation's enumerated outcomes falls through to.
fn check(r: &Response) -> Result<(), Failure> {
    // 401 ERROR_INVALID_ACCESS_TOKEN: the access token is missing, expired or
    // invalid. 402 ERROR_FEATURE_NOT_AVAILABLE: the instance holds no licence
    // for the requested feature (the json, xml, excel and file exporters).
    if r.status == 401 || r.status == 402 || r.status == 403 {
        return Err(Ok(r.text.clone()));
    }
    if r.status >= 400 {
        if let Some(e) = r.error() {
            if PERMISSION_ERRORS.contains(&e.as_str()) {
                return Err(Ok(r.text.clone()));
            }
        }
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

/// Build a value to an operation's Rejected schema. Every rejection the
/// specification constructs carries a `detail`, which is JSON null wherever
/// the response body had none.
fn reject(code: &str, detail: Option<Value>) -> Verdict {
    let mut value = json!({ "code": code });
    if let Some(detail) = detail {
        value["detail"] = detail;
    }
    Ok(Err(value.to_string()))
}

/// A job's `state`. An absent field is no state, never a crash.
fn state_of(job: &Value) -> &str {
    job.get("state").and_then(Value::as_str).unwrap_or_default()
}

/// A Resolved mapping: exactly these keys, an absent one as JSON null.
fn pick(source: &Value, keys: &[&str]) -> String {
    let value: Map<String, Value> = keys
        .iter()
        .map(|k| {
            (
                (*k).to_string(),
                source.get(*k).cloned().unwrap_or(Value::Null),
            )
        })
        .collect();
    Value::Object(value).to_string()
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

/// A required object argument of an operation's Param schema.
fn arg_object<'a>(args: &'a Value, key: &str) -> Result<&'a Value, String> {
    match args.get(key) {
        Some(v) if v.is_object() => Ok(v),
        _ => Err(format!("args.{key} is required and must be an object")),
    }
}

/// A required array argument of an operation's Param schema.
fn arg_array<'a>(args: &'a Value, key: &str) -> Result<&'a Value, String> {
    match args.get(key) {
        Some(v) if v.is_array() => Ok(v),
        _ => Err(format!("args.{key} is required and must be an array")),
    }
}

/// The specification's `_query`. A boolean reaches the query string as
/// "true"/"false"; Python's True/False is not a form Baserow documents.
/// Field lists (order_by, include, exclude) are comma-joined into a single
/// value. A key the args do not carry is not sent.
fn query(args: &Value, keys: &[&str]) -> Vec<(String, String)> {
    let mut params = Vec::new();
    let Some(obj) = args.as_object() else {
        return params;
    };
    for k in keys {
        let Some(v) = obj.get(*k) else {
            continue;
        };
        let rendered = match v {
            Value::Bool(b) => (if *b { "true" } else { "false" }).to_string(),
            Value::Array(items) => items.iter().map(scalar).collect::<Vec<String>>().join(","),
            // A null parameter is dropped rather than sent as the string
            // "null" — what `requests` does with a None.
            Value::Null => continue,
            other => scalar(other),
        };
        params.push(((*k).to_string(), rendered));
    }
    params
}

/// One query value, as `str(v)` renders it: a string unquoted, anything else
/// in its JSON form.
fn scalar(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
