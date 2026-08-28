//! `plugin::process` against a real Baserow — never a mock.
//!
//! Bring the provider up with specification §5.1/§5.2 first; the exported
//! `BASEROW_BASE_URL`, `BASEROW_EMAIL`, `BASEROW_PASSWORD`,
//! `BASEROW_FIXTURE_OK` and `BASEROW_FIXTURE_FAIL` are what [`config`] and the
//! fixtures below read. Every documented condition is induced with real
//! inputs against that instance — a real table is exported, a trashed table is
//! really unresolvable, a corrupted credential is really rejected.
//!
//! Three documented conditions of `export.create` have no live induction and
//! so have no test here:
//!
//!   * `export_failed` — §5.2: "Baserow documents no feature that drives an
//!     export job to state 'failed', so export_failed has no fixture".
//!   * `expired` — §5.2: it "occurs only 60 minutes after creation", and
//!     EXPORT_FILE_EXPIRE_MINUTES is not settable through §5.1's image.
//!   * `job_not_found` — the job record would have to disappear between two
//!     polls, and Baserow's API exposes no endpoint that deletes an export
//!     job (the table's own deletion only nulls `ExportJob.table`).

use std::collections::HashMap;
use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};

use resonate_core::types::{PromiseRecord, PromiseState, PromiseValue};
use resonate_plugin_baserow::plugin::{self, Config};

/// The verdict `process` returns, spelled out: the crate's own alias for it
/// is an implementation detail.
type Verdict = Result<Result<String, String>, Result<String, String>>;

// ─── export.create (§4.1) ─────────────────────────────────────────────────────

/// resolved — the succeeding work item §5 provisions. Also the
/// pending → terminal path: the create response is `state: "pending"` with a
/// null `url`, so a resolved value carrying `state: "finished"` and a real
/// `url` is proof the poll loop watched the job to its terminal state rather
/// than reporting the creation.
#[tokio::test]
async fn export_create_resolves_when_the_export_finishes() {
    let p = promise(
        &promise_id("export-create-ok"),
        "export.create",
        json!({"table_id": fixture_ok(), "exporter_type": "csv"}),
        in_ms(120_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert!(value["id"].is_i64(), "{value}");
    assert_eq!(value["table"], fixture_ok());
    assert_eq!(value["view"], Value::Null);
    assert_eq!(value["exporter_type"], "csv");
    assert_eq!(value["state"], "finished");
    let file = value["exported_file_name"].as_str().expect("exported_file_name");
    assert!(file.ends_with(".csv"), "{value}");
    assert!(value["created_at"].is_string(), "{value}");
    let url = value["url"].as_str().expect("url is a string once finished");
    assert!(url.contains(file), "{value}");
    // The Resolved schema is exactly these eight keys.
    assert_eq!(value.as_object().unwrap().len(), 8, "{value}");
}

/// `table_not_found` — the trashed table §5.2 provisions. TableHandler
/// excludes trashed tables, so the id is permanently unresolvable.
#[tokio::test]
async fn export_create_rejects_table_not_found() {
    let p = promise(
        &promise_id("export-create-notable"),
        "export.create",
        json!({"table_id": fixture_fail(), "exporter_type": "csv"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "table_not_found");
    // detail = response.body of the 404 ({error, detail}).
    assert_eq!(value["detail"]["error"], "ERROR_TABLE_DOES_NOT_EXIST");
    assert!(value["detail"]["detail"].is_string(), "{value}");
}

/// `view_not_found` — a view id no view has.
#[tokio::test]
async fn export_create_rejects_view_not_found() {
    let p = promise(
        &promise_id("export-create-noview"),
        "export.create",
        json!({"table_id": fixture_ok(), "exporter_type": "csv", "view_id": 999_999}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "view_not_found");
    assert_eq!(value["detail"]["error"], "ERROR_VIEW_DOES_NOT_EXIST");
}

/// `invalid_request` — `exporter_type` is documented as an enum, and an
/// unregistered exporter is one of the validation failures the Param schema
/// names.
#[tokio::test]
async fn export_create_rejects_invalid_request() {
    let p = promise(
        &promise_id("export-create-bad"),
        "export.create",
        json!({"table_id": fixture_ok(), "exporter_type": "no_such_exporter"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    // detail = response.body of the 400 ({error, detail}).
    assert_eq!(value["detail"]["error"], "ERROR_REQUEST_BODY_VALIDATION");
    assert!(!value["detail"]["detail"]["exporter_type"].is_null(), "{value}");
}

/// `cancelled` — creating an export job sets every other unfinished export job
/// of the same user to `cancelled`, so a second export started while this one
/// is still running is the whole induction. Two details of the §5 provider's
/// own export task — observed while building this test, and no part of the
/// specification — decide when that second export has to land: the task writes
/// `state: "exporting"` over whatever the row held when it starts, so a
/// cancellation that arrives while the job is still `pending` is lost, and it
/// re-reads the row before writing the last row, so one that arrives while the
/// job is `exporting` is honoured. That window is the export's own duration —
/// milliseconds on the two-row fixture table — so this test exports a table
/// large enough to be worth cancelling and watches for `exporting` at a
/// cadence far below it. Export job ids are sequential and the tests run
/// serially, so the job the plugin is about to create is the one after this
/// test's own.
#[tokio::test]
async fn export_create_rejects_cancelled() {
    let table = big_table().await;
    let target = create_export_job(fixture_ok()).await + 1;
    let p = promise(
        &promise_id("export-create-cancelled"),
        "export.create",
        json!({"table_id": table, "exporter_type": "csv"}),
        in_ms(60_000),
    );

    let canceller = async {
        for _ in 0..2_000 {
            match export_job_state(target).await {
                // The plugin has not created its job yet, or its job is
                // queued and a cancellation now would be overwritten.
                None | Some(Pending) => {}
                // Running: a second export of any table cancels this one.
                Some(Exporting) => {
                    create_export_job(fixture_ok()).await;
                }
                // Terminal: either this loop already cancelled it, or the
                // export won the race and the assertions below say so.
                Some(Terminal) => break,
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    let config = config();
    let (verdict, ()) = tokio::join!(plugin::process(&config, &p), canceller);
    let value = rejected(verdict);

    assert_eq!(value["code"], "cancelled");
    // detail = the terminal export job object.
    assert_eq!(value["detail"]["id"], target);
    assert_eq!(value["detail"]["state"], "cancelled");
    assert_eq!(value["detail"]["table"], table);
}

/// The deadline — `timeout_at` already in the past. No verdict: the server
/// settles a timed-out promise itself. The export job the create call made
/// is left running; nothing on it ties it to this promise, which is what the
/// specification's comment in the poll loop records.
#[tokio::test]
async fn export_create_releases_at_the_deadline() {
    let p = promise(
        &promise_id("export-create-deadline"),
        "export.create",
        json!({"table_id": fixture_ok(), "exporter_type": "csv"}),
        in_ms(-1_000),
    );

    let reason = released(plugin::process(&config(), &p).await);

    assert_eq!(reason, "promise timed out");
}

/// halt — the credential is rejected, and no retry of ours can fix that.
#[tokio::test]
async fn export_create_halts_on_rejected_credentials() {
    let p = promise(
        &promise_id("export-create-halt"),
        "export.create",
        json!({"table_id": fixture_ok(), "exporter_type": "csv"}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("ERROR_INVALID_CREDENTIALS"), "{reason}");
}

/// Re-entry — Baserow has no idempotency: nothing on an export job carries a
/// client-supplied identity, so a redelivery cannot re-attach and exports the
/// table a second time under a second job id. Both attempts still reach the
/// same verdict, which is what makes the redelivery safe.
#[tokio::test]
async fn export_create_exports_again_on_re_entry() {
    let p = promise(
        &promise_id("export-create-reentry"),
        "export.create",
        json!({"table_id": fixture_ok(), "exporter_type": "csv"}),
        in_ms(120_000),
    );

    let first = resolved(plugin::process(&config(), &p).await);
    let second = resolved(plugin::process(&config(), &p).await);

    assert_eq!(first["state"], "finished");
    assert_eq!(second["state"], "finished");
    assert_ne!(first["id"], second["id"], "one promise, two export jobs");
}

// ─── export.get (§4.2) ────────────────────────────────────────────────────────

/// resolved — = response.body. The job is provisioned with a raw call, so
/// this read does not depend on `export.create`.
#[tokio::test]
async fn export_get_resolves_with_the_job() {
    let job_id = create_export_job(fixture_ok()).await;
    let p = promise(
        &promise_id("export-get"),
        "export.get",
        json!({"job_id": job_id}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["id"], job_id);
    assert_eq!(value["table"], fixture_ok());
    assert_eq!(value["exporter_type"], "csv");
    assert!(value["state"].is_string(), "{value}");
    // = response.body, so the keys the Resolved mapping of §4.1 drops are here.
    assert!(value["progress_percentage"].is_number(), "{value}");
    assert!(value.get("status").is_some(), "{value}");
}

/// `not_found` — a job id no export job has.
#[tokio::test]
async fn export_get_rejects_not_found() {
    let p = promise(
        &promise_id("export-get-404"),
        "export.get",
        json!({"job_id": 999_999}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "not_found");
    // detail = response.body of the 404 as text.
    let detail = value["detail"].as_str().expect("detail is text");
    assert!(detail.contains("ERROR_EXPORT_JOB_DOES_NOT_EXIST"), "{detail}");
}

/// `invalid_request` — `job_id` is required by the Param schema.
#[tokio::test]
async fn export_get_rejects_invalid_request() {
    let p = promise(&promise_id("export-get-bad"), "export.get", json!({}), in_ms(60_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(value["detail"].as_str().unwrap_or_default().contains("job_id"), "{value}");
}

/// halt — the credential is rejected.
#[tokio::test]
async fn export_get_halts_on_rejected_credentials() {
    let p = promise(
        &promise_id("export-get-halt"),
        "export.get",
        json!({"job_id": 1}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("ERROR_INVALID_CREDENTIALS"), "{reason}");
}

// ─── table.list (§4.3) ────────────────────────────────────────────────────────

/// resolved — = response.body, the array carrying the table id
/// `export.create` takes.
#[tokio::test]
async fn table_list_resolves_with_the_tables() {
    let p = promise(
        &promise_id("table-list"),
        "table.list",
        json!({"database_id": database_id().await}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    let tables = value.as_array().expect("the body is an array");
    let ok = tables
        .iter()
        .find(|t| t["id"] == fixture_ok())
        .unwrap_or_else(|| panic!("the fixture table is missing: {value}"));
    assert!(ok["name"].is_string(), "{ok}");
    assert_eq!(ok["database_id"], database_id().await);
    assert!(ok["order"].is_i64(), "{ok}");
    // The trashed table is not listed.
    assert!(!tables.iter().any(|t| t["id"] == fixture_fail()), "{value}");
}

/// `not_found` — a database id no application has.
#[tokio::test]
async fn table_list_rejects_not_found() {
    let p = promise(
        &promise_id("table-list-404"),
        "table.list",
        json!({"database_id": 999_999}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "not_found");
    // detail = response.body of the 404 as text.
    let detail = value["detail"].as_str().expect("detail is text");
    assert!(detail.contains("ERROR_APPLICATION_DOES_NOT_EXIST"), "{detail}");
}

/// `invalid_request` — `database_id` is required by the Param schema.
#[tokio::test]
async fn table_list_rejects_invalid_request() {
    let p = promise(&promise_id("table-list-bad"), "table.list", json!({}), in_ms(60_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("database_id"),
        "{value}"
    );
}

/// halt — the credential is rejected.
#[tokio::test]
async fn table_list_halts_on_rejected_credentials() {
    let p = promise(
        &promise_id("table-list-halt"),
        "table.list",
        json!({"database_id": 1}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("ERROR_INVALID_CREDENTIALS"), "{reason}");
}

// ─── field.list (§4.4) ────────────────────────────────────────────────────────

/// resolved — = response.body, the array carrying the field ids
/// `export.create` takes in `fields`, `order_by` and `filters`.
#[tokio::test]
async fn field_list_resolves_with_the_fields() {
    let p = promise(
        &promise_id("field-list"),
        "field.list",
        json!({"table_id": fixture_ok()}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    let fields = value.as_array().expect("the body is an array");
    assert!(!fields.is_empty(), "the seeded table has fields: {value}");
    assert!(fields.iter().any(|f| f["primary"] == json!(true)), "{value}");
    for field in fields {
        assert_eq!(field["table_id"], fixture_ok());
        assert!(field["id"].is_i64(), "{field}");
        assert!(field["name"].is_string(), "{field}");
        assert!(field["type"].is_string(), "{field}");
        assert!(field["read_only"].is_boolean(), "{field}");
    }
}

/// `table_not_found` — the trashed table §5.2 provisions.
#[tokio::test]
async fn field_list_rejects_table_not_found() {
    let p = promise(
        &promise_id("field-list-notable"),
        "field.list",
        json!({"table_id": fixture_fail()}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "table_not_found");
    // detail = response.body of the 404 ({error, detail}).
    assert_eq!(value["detail"]["error"], "ERROR_TABLE_DOES_NOT_EXIST");
}

/// `view_not_found` — a real table, and a view id no view has.
#[tokio::test]
async fn field_list_rejects_view_not_found() {
    let p = promise(
        &promise_id("field-list-noview"),
        "field.list",
        json!({"table_id": fixture_ok(), "view": 999_999}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "view_not_found");
    assert_eq!(value["detail"]["error"], "ERROR_VIEW_DOES_NOT_EXIST");
}

/// `invalid_request` — `view` is documented as an integer, and the query
/// parameter is passed through as the caller wrote it.
#[tokio::test]
async fn field_list_rejects_invalid_request() {
    let p = promise(
        &promise_id("field-list-bad"),
        "field.list",
        json!({"table_id": fixture_ok(), "view": "not-an-integer"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    // detail = response.body of the 400 as text.
    let detail = value["detail"].as_str().expect("detail is text");
    assert!(detail.contains("ERROR_QUERY_PARAMETER_VALIDATION"), "{detail}");
}

/// halt — the credential is rejected.
#[tokio::test]
async fn field_list_halts_on_rejected_credentials() {
    let p = promise(
        &promise_id("field-list-halt"),
        "field.list",
        json!({"table_id": fixture_ok()}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("ERROR_INVALID_CREDENTIALS"), "{reason}");
}

// ─── Dispatch ─────────────────────────────────────────────────────────────────

/// A func no operation serves is a permanent rejection naming it.
#[tokio::test]
async fn an_unknown_func_is_rejected() {
    let p = promise(&promise_id("unknown"), "export.explode", json!({}), in_ms(60_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "unknown_func", "detail": "export.explode"}));
}

// ─── Fixtures ─────────────────────────────────────────────────────────────────

/// §2, built from the environment §5.2 exports. `poll` is overridden to 1s:
/// the default 2s would make every pending → terminal test wait a full
/// interval past the export's own end.
fn config() -> Config {
    Config {
        base_url: env("BASEROW_BASE_URL"),
        email: env("BASEROW_EMAIL"),
        password: env("BASEROW_PASSWORD"),
        poll: Duration::from_secs(1),
    }
}

/// The same Baserow, with a password it will reject.
fn bad_credential() -> Config {
    Config {
        password: format!("{}-wrong", env("BASEROW_PASSWORD")),
        ..config()
    }
}

/// The table §5.2 seeds with example fields and rows: the export succeeds and
/// the exported file is not empty.
fn fixture_ok() -> i64 {
    env("BASEROW_FIXTURE_OK").parse().expect("BASEROW_FIXTURE_OK is an integer")
}

/// The table §5.2 trashes: its id is permanently unresolvable.
fn fixture_fail() -> i64 {
    env("BASEROW_FIXTURE_FAIL").parse().expect("BASEROW_FIXTURE_FAIL is an integer")
}

fn env(key: &str) -> String {
    std::env::var(key)
        .unwrap_or_else(|_| panic!("{key} is unset — run specification §5.1 and §5.2 first"))
}

fn promise(id: &str, func: &str, args: Value, timeout_at: i64) -> PromiseRecord {
    let param = json!({ "func": func, "args": args }).to_string();
    PromiseRecord {
        id: id.to_string(),
        state: PromiseState::Pending,
        param: PromiseValue {
            headers: None,
            data: Some(base64::engine::general_purpose::STANDARD.encode(param)),
        },
        value: PromiseValue::default(),
        tags: HashMap::new(),
        timeout_at,
        created_at: now_ms(),
        settled_at: None,
    }
}

/// A fresh promise id per test. Nothing of it reaches Baserow — the
/// specification's Idempotency note records that there is nowhere to put it.
fn promise_id(what: &str) -> String {
    format!("baserow.{what}.{}", nanos())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn in_ms(delta: i64) -> i64 {
    now_ms() + delta
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

// ─── Verdicts ─────────────────────────────────────────────────────────────────

fn resolved(verdict: Verdict) -> Value {
    match verdict {
        Ok(Ok(v)) => serde_json::from_str(&v).expect("resolved value is JSON"),
        other => panic!("expected resolved, got {other:?}"),
    }
}

fn rejected(verdict: Verdict) -> Value {
    match verdict {
        Ok(Err(v)) => serde_json::from_str(&v).expect("rejected value is JSON"),
        other => panic!("expected rejected, got {other:?}"),
    }
}

fn halted(verdict: Verdict) -> String {
    match verdict {
        Err(Ok(reason)) => reason,
        other => panic!("expected halt, got {other:?}"),
    }
}

fn released(verdict: Verdict) -> String {
    match verdict {
        Err(Err(reason)) => reason,
        other => panic!("expected release, got {other:?}"),
    }
}

// ─── Provisioning (raw provider calls, not the plugin's code path) ────────────

fn http() -> reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new).clone()
}

/// One access token, re-minted every five minutes: tokens live
/// BASEROW_ACCESS_TOKEN_LIFETIME_MINUTES (10 by default), which a whole test
/// binary can outlast, and the `cancelled` induction cannot afford a token
/// request between noticing the plugin's job and cancelling it.
async fn bearer() -> String {
    static TOKEN: tokio::sync::Mutex<Option<(String, std::time::Instant)>> =
        tokio::sync::Mutex::const_new(None);
    let mut cached = TOKEN.lock().await;
    if let Some((token, minted)) = cached.as_ref() {
        if minted.elapsed() < Duration::from_secs(300) {
            return token.clone();
        }
    }
    let fresh = mint().await;
    *cached = Some((fresh.clone(), std::time::Instant::now()));
    fresh
}

async fn mint() -> String {
    let body: Value = http()
        .post(format!("{}/api/user/token-auth/", env("BASEROW_BASE_URL")))
        .json(&json!({"email": env("BASEROW_EMAIL"), "password": env("BASEROW_PASSWORD")}))
        .send()
        .await
        .expect("token request")
        .json()
        .await
        .expect("token response is JSON");
    format!("JWT {}", body["access_token"].as_str().expect("access_token"))
}

/// The database the §5.2 fixtures live in — `table.list`'s argument, which
/// §5.2 does not export, read back off the fixture table itself.
async fn database_id() -> i64 {
    let body: Value = http()
        .get(format!(
            "{}/api/database/tables/{}/",
            env("BASEROW_BASE_URL"),
            fixture_ok()
        ))
        .header("Authorization", bearer().await)
        .send()
        .await
        .expect("get table request")
        .json()
        .await
        .expect("get table response is JSON");
    body["database_id"].as_i64().expect("database_id")
}

/// Start one export job of this table, outside the plugin. Every such job
/// cancels the user's other unfinished jobs — which is what the `cancelled`
/// induction is made of.
async fn create_export_job(table_id: i64) -> i64 {
    let body: Value = http()
        .post(format!(
            "{}/api/database/export/table/{table_id}/",
            env("BASEROW_BASE_URL")
        ))
        .header("Authorization", bearer().await)
        .json(&json!({"exporter_type": "csv"}))
        .send()
        .await
        .expect("create export request")
        .json()
        .await
        .expect("create export response is JSON");
    body["id"].as_i64().unwrap_or_else(|| panic!("no export job id in {body}"))
}

/// Where an export job is, as the `cancelled` induction needs to read it.
#[derive(PartialEq)]
enum JobState {
    /// Queued: a cancellation now is overwritten when the task starts.
    Pending,
    /// Running: a cancellation now is honoured before the last row.
    Exporting,
    /// finished, failed, cancelled or expired.
    Terminal,
}
use JobState::{Exporting, Pending, Terminal};

/// The state of one export job, or `None` while no job has that id yet.
async fn export_job_state(job_id: i64) -> Option<JobState> {
    let response = http()
        .get(format!(
            "{}/api/database/export/{job_id}/",
            env("BASEROW_BASE_URL")
        ))
        .header("Authorization", bearer().await)
        .send()
        .await
        .expect("get export request");
    if response.status().as_u16() == 404 {
        return None;
    }
    let body: Value = response.json().await.expect("get export response is JSON");
    Some(match body["state"].as_str().unwrap_or_default() {
        "pending" => Pending,
        "exporting" => Exporting,
        _ => Terminal,
    })
}

/// A table whose export takes long enough to be cancelled mid-flight. §5.2
/// provisions only the two-row `BASEROW_FIXTURE_OK`, whose export is over in
/// milliseconds, so the `cancelled` induction fills a table of its own: 4000
/// rows of 4KB export for a few hundred milliseconds. Provisioned once per
/// test binary.
async fn big_table() -> i64 {
    static TABLE: tokio::sync::OnceCell<i64> = tokio::sync::OnceCell::const_new();
    *TABLE
        .get_or_init(|| async {
            let base = env("BASEROW_BASE_URL");
            let table: Value = http()
                .post(format!(
                    "{base}/api/database/tables/database/{}/",
                    database_id().await
                ))
                .header("Authorization", bearer().await)
                .json(&json!({"name": format!("Fixture Big {}", nanos())}))
                .send()
                .await
                .expect("create table request")
                .json()
                .await
                .expect("create table response is JSON");
            let table_id = table["id"].as_i64().expect("table id");

            // Only the text fields of the seeded table take a text value.
            let fields: Value = http()
                .get(format!("{base}/api/database/fields/table/{table_id}/"))
                .header("Authorization", bearer().await)
                .send()
                .await
                .expect("list fields request")
                .json()
                .await
                .expect("list fields response is JSON");
            let mut row = serde_json::Map::new();
            for field in fields.as_array().expect("fields is an array") {
                if field["type"] == "text" || field["type"] == "long_text" {
                    let id = field["id"].as_i64().expect("field id");
                    row.insert(format!("field_{id}"), json!("x".repeat(2_000)));
                }
            }

            let items: Vec<Value> = (0..200).map(|_| Value::Object(row.clone())).collect();
            for _ in 0..20 {
                let status = http()
                    .post(format!("{base}/api/database/rows/table/{table_id}/batch/"))
                    .header("Authorization", bearer().await)
                    .json(&json!({ "items": items }))
                    .send()
                    .await
                    .expect("batch rows request")
                    .status();
                assert!(status.is_success(), "filling the big table: {status}");
            }
            table_id
        })
        .await
}
