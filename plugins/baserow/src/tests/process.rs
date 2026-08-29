//! `plugin::process` against a real Baserow — never a mock.
//!
//! Bring the provider up with specification §5.1/§5.2 first; the exported
//! `BASEROW_BASE_URL`, `BASEROW_EMAIL`, `BASEROW_PASSWORD`,
//! `BASEROW_FIXTURE_OK` and `BASEROW_FIXTURE_FAIL` are what [`config`] and
//! the fixtures below read. Every documented condition is induced with real
//! inputs against that instance — a missing table is really missing, a
//! corrupted credential is really rejected, an export job really runs on the
//! celery worker and takes real time to finish.
//!
//! §5 provisions two tables. `Tasks` (`BASEROW_FIXTURE_OK`) is all text, so
//! every value it is given imports and every export of it finishes: it is
//! the succeeding work item. `Amounts` (`BASEROW_FIXTURE_FAIL`) has a number
//! field `Amount`, so a non-numeric value written to it is rejected by the
//! field — synchronously as `ERROR_REQUEST_BODY_VALIDATION` on a row write,
//! and asynchronously as a `report.failing_rows` entry on an import job that
//! still reaches "finished". It is the failing work item.
//!
//! Three rejection codes the specification constructs have no induction on
//! this fixture, and no test below claims one:
//!
//!   * `already_deleted` (§4.5, §4.9) — Baserow's row lookup excludes
//!     trashed rows, so deleting an already-deleted row answers 404
//!     `ERROR_ROW_DOES_NOT_EXIST`, which is what §4.5.1 itself documents for
//!     a re-delivery. `row_delete_rejects_row_not_found_on_re_entry` covers
//!     that path.
//!   * `field_not_found` (§4.1) — every field a row.list query can name
//!     wrongly answers 400 (`ERROR_ORDER_BY_FIELD_NOT_FOUND`,
//!     `ERROR_FILTER_FIELD_NOT_FOUND`), which is `invalid_request`.
//!   * `cancelled`, `expired`, `export_failed`, `job_not_found` (§4.10) and
//!     `cancelled`, `import_failed` (§4.12) — a csv export of a two-row text
//!     table cannot fail and finishes in well under a second, no endpoint
//!     deletes an export job, and `expired` is 60 minutes away.

use std::collections::HashMap;
use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};

use resonate_core::types::{PromiseRecord, PromiseState, PromiseValue};
use resonate_plugin_baserow::plugin::{self, Config};

/// The verdict `process` returns, spelled out: the crate's own alias for it
/// is an implementation detail.
type Verdict = Result<Result<String, String>, Result<String, String>>;

/// A table id no table has.
const NO_TABLE: i64 = 99_999;
/// A row id no row has.
const NO_ROW: i64 = 99_999;
/// A view id no view has.
const NO_VIEW: i64 = 99_999;
/// A job id no job has.
const NO_JOB: i64 = 99_999;
/// A database id no application has.
const NO_DATABASE: i64 = 99_999;

// ─── row.list (§4.1) ──────────────────────────────────────────────────────────

/// resolved — = response.body, one page of the table's rows.
#[tokio::test]
async fn row_list_resolves_with_the_page() {
    let p = promise(
        &promise_id("row-list"),
        "row.list",
        json!({"table_id": ok_table(), "size": 200, "user_field_names": true}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert!(value["count"].is_number(), "{value}");
    assert_eq!(value["previous"], Value::Null);
    let results = value["results"].as_array().expect("results is an array");
    assert!(!results.is_empty(), "{value}");
    assert!(results[0]["id"].is_i64(), "{value}");
    assert!(results[0]["order"].is_string(), "{value}");
    // user_field_names: the keys are the field names §5 gave the table.
    assert!(results[0].get("Name").is_some(), "{value}");
}

/// `table_not_found` — a nonexistent table id.
#[tokio::test]
async fn row_list_rejects_table_not_found() {
    let p = promise(
        &promise_id("row-list-notable"),
        "row.list",
        json!({"table_id": NO_TABLE}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "table_not_found");
    // detail = response.body.detail of the 404.
    assert!(value["detail"].is_string(), "{value}");
}

/// `view_not_found` — a nonexistent view id.
#[tokio::test]
async fn row_list_rejects_view_not_found() {
    let p = promise(
        &promise_id("row-list-noview"),
        "row.list",
        json!({"table_id": ok_table(), "view_id": NO_VIEW}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "view_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `invalid_request` — the Param schema caps `size` at 200.
#[tokio::test]
async fn row_list_rejects_invalid_request() {
    let p = promise(
        &promise_id("row-list-bad"),
        "row.list",
        json!({"table_id": ok_table(), "size": 201}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(!value["detail"].is_null(), "{value}");
}

/// halt — the credential is rejected, and no retry of ours can fix that.
#[tokio::test]
async fn row_list_halts_on_rejected_credentials() {
    let p = promise(
        &promise_id("row-list-halt"),
        "row.list",
        json!({"table_id": ok_table()}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("ERROR_INVALID_CREDENTIALS"), "{reason}");
}

// ─── row.get (§4.2) ───────────────────────────────────────────────────────────

/// resolved — = response.body, the row.
#[tokio::test]
async fn row_get_resolves_with_the_row() {
    let row_id = seed_row("row-get").await;
    let p = promise(
        &promise_id("row-get"),
        "row.get",
        json!({"table_id": ok_table(), "row_id": row_id, "user_field_names": true}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["id"], row_id);
    assert!(value["order"].is_string(), "{value}");
    assert_eq!(value["Name"], "row-get");
}

/// `table_not_found` — a nonexistent table id.
#[tokio::test]
async fn row_get_rejects_table_not_found() {
    let p = promise(
        &promise_id("row-get-notable"),
        "row.get",
        json!({"table_id": NO_TABLE, "row_id": 1}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "table_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `row_not_found` — a nonexistent row id.
#[tokio::test]
async fn row_get_rejects_row_not_found() {
    let p = promise(
        &promise_id("row-get-norow"),
        "row.get",
        json!({"table_id": ok_table(), "row_id": NO_ROW}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "row_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `view_not_found` — a nonexistent view id.
#[tokio::test]
async fn row_get_rejects_view_not_found() {
    let row_id = seed_row("row-get-noview").await;
    let p = promise(
        &promise_id("row-get-noview"),
        "row.get",
        json!({"table_id": ok_table(), "row_id": row_id, "view": NO_VIEW}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "view_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `invalid_request` — `row_id` is required by the Param schema.
#[tokio::test]
async fn row_get_rejects_invalid_request() {
    let p = promise(
        &promise_id("row-get-bad"),
        "row.get",
        json!({"table_id": ok_table()}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("row_id"),
        "{value}"
    );
}

/// halt — the credential is rejected.
#[tokio::test]
async fn row_get_halts_on_rejected_credentials() {
    let p = promise(
        &promise_id("row-get-halt"),
        "row.get",
        json!({"table_id": ok_table(), "row_id": 1}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("ERROR_INVALID_CREDENTIALS"), "{reason}");
}

// ─── row.create (§4.3) ────────────────────────────────────────────────────────

/// resolved — = response.body, the created row.
#[tokio::test]
async fn row_create_resolves_with_the_created_row() {
    let p = promise(
        &promise_id("row-create"),
        "row.create",
        json!({
            "table_id": ok_table(),
            "values": {"Name": "created", "Notes": "by row.create"},
            "user_field_names": true,
        }),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert!(value["id"].is_i64(), "{value}");
    assert!(value["order"].is_string(), "{value}");
    assert_eq!(value["Name"], "created");
    assert_eq!(value["Notes"], "by row.create");
}

/// `table_not_found` — a nonexistent table id.
#[tokio::test]
async fn row_create_rejects_table_not_found() {
    let p = promise(
        &promise_id("row-create-notable"),
        "row.create",
        json!({"table_id": NO_TABLE, "values": {}}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "table_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `row_not_found` — the `before` row does not exist.
#[tokio::test]
async fn row_create_rejects_row_not_found() {
    let p = promise(
        &promise_id("row-create-norow"),
        "row.create",
        json!({"table_id": ok_table(), "values": {}, "before": NO_ROW}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "row_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `view_not_found` — a nonexistent view id.
#[tokio::test]
async fn row_create_rejects_view_not_found() {
    let p = promise(
        &promise_id("row-create-noview"),
        "row.create",
        json!({"table_id": ok_table(), "values": {}, "view": NO_VIEW}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "view_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `invalid_request` — the failing work item: `Amount` is a number field and
/// a non-numeric value is not accepted by it.
#[tokio::test]
async fn row_create_rejects_invalid_request() {
    let p = promise(
        &promise_id("row-create-bad"),
        "row.create",
        json!({
            "table_id": fail_table(),
            "values": {"Label": "bad", "Amount": "not-a-number"},
            "user_field_names": true,
        }),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    // §4.3.2: for ERROR_REQUEST_BODY_VALIDATION an object keyed by field name.
    assert!(value["detail"]["Amount"].is_array(), "{value}");
}

/// halt — the credential is rejected.
#[tokio::test]
async fn row_create_halts_on_rejected_credentials() {
    let p = promise(
        &promise_id("row-create-halt"),
        "row.create",
        json!({"table_id": ok_table(), "values": {}}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("ERROR_INVALID_CREDENTIALS"), "{reason}");
}

// ─── row.update (§4.4) ────────────────────────────────────────────────────────

/// resolved — = response.body, the updated row.
#[tokio::test]
async fn row_update_resolves_with_the_updated_row() {
    let row_id = seed_row("row-update").await;
    let p = promise(
        &promise_id("row-update"),
        "row.update",
        json!({
            "table_id": ok_table(),
            "row_id": row_id,
            "values": {"Notes": "updated"},
            "user_field_names": true,
        }),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["id"], row_id);
    assert_eq!(value["Name"], "row-update");
    assert_eq!(value["Notes"], "updated");
}

/// Re-entry — the write is a function of the promise, so a second delivery
/// re-applies the same values and leaves the same row state.
#[tokio::test]
async fn row_update_is_unchanged_on_re_entry() {
    let row_id = seed_row("row-update-reentry").await;
    let p = promise(
        &promise_id("row-update-reentry"),
        "row.update",
        json!({
            "table_id": ok_table(),
            "row_id": row_id,
            "values": {"Notes": "idempotent"},
            "user_field_names": true,
        }),
        in_ms(60_000),
    );

    let first = resolved(plugin::process(&config(), &p).await);
    let second = resolved(plugin::process(&config(), &p).await);

    assert_eq!(first, second);
    assert_eq!(second["Notes"], "idempotent");
}

/// `table_not_found` — a nonexistent table id.
#[tokio::test]
async fn row_update_rejects_table_not_found() {
    let p = promise(
        &promise_id("row-update-notable"),
        "row.update",
        json!({"table_id": NO_TABLE, "row_id": 1, "values": {}}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "table_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `row_not_found` — a nonexistent row id.
#[tokio::test]
async fn row_update_rejects_row_not_found() {
    let p = promise(
        &promise_id("row-update-norow"),
        "row.update",
        json!({"table_id": ok_table(), "row_id": NO_ROW, "values": {}}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "row_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `view_not_found` — a nonexistent view id.
#[tokio::test]
async fn row_update_rejects_view_not_found() {
    let row_id = seed_row("row-update-noview").await;
    let p = promise(
        &promise_id("row-update-noview"),
        "row.update",
        json!({"table_id": ok_table(), "row_id": row_id, "values": {}, "view": NO_VIEW}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "view_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `invalid_request` — the failing work item's number field.
#[tokio::test]
async fn row_update_rejects_invalid_request() {
    let row_id = seed_fail_row("row-update-bad").await;
    let p = promise(
        &promise_id("row-update-bad"),
        "row.update",
        json!({
            "table_id": fail_table(),
            "row_id": row_id,
            "values": {"Amount": "not-a-number"},
            "user_field_names": true,
        }),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(value["detail"]["Amount"].is_array(), "{value}");
}

/// halt — the credential is rejected.
#[tokio::test]
async fn row_update_halts_on_rejected_credentials() {
    let p = promise(
        &promise_id("row-update-halt"),
        "row.update",
        json!({"table_id": ok_table(), "row_id": 1, "values": {}}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("ERROR_INVALID_CREDENTIALS"), "{reason}");
}

// ─── row.delete (§4.5) ────────────────────────────────────────────────────────

/// resolved — the endpoint answers 204, so the Resolved value is empty.
#[tokio::test]
async fn row_delete_resolves_with_an_empty_object() {
    let row_id = seed_row("row-delete").await;
    let p = promise(
        &promise_id("row-delete"),
        "row.delete",
        json!({"table_id": ok_table(), "row_id": row_id}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({}));
    assert_eq!(row_status(ok_table(), row_id).await, 404, "the row survived");
}

/// Re-entry — §4.5.1: a re-delivery after an earlier attempt already deleted
/// the row rejects `row_not_found`. (Baserow's row lookup excludes trashed
/// rows, so `already_deleted` is not the answer here.)
#[tokio::test]
async fn row_delete_rejects_row_not_found_on_re_entry() {
    let row_id = seed_row("row-delete-reentry").await;
    let p = promise(
        &promise_id("row-delete-reentry"),
        "row.delete",
        json!({"table_id": ok_table(), "row_id": row_id}),
        in_ms(60_000),
    );

    assert_eq!(resolved(plugin::process(&config(), &p).await), json!({}));
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "row_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `table_not_found` — a nonexistent table id.
#[tokio::test]
async fn row_delete_rejects_table_not_found() {
    let p = promise(
        &promise_id("row-delete-notable"),
        "row.delete",
        json!({"table_id": NO_TABLE, "row_id": 1}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "table_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `row_not_found` — a nonexistent row id.
#[tokio::test]
async fn row_delete_rejects_row_not_found() {
    let p = promise(
        &promise_id("row-delete-norow"),
        "row.delete",
        json!({"table_id": ok_table(), "row_id": NO_ROW}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "row_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `view_not_found` — a nonexistent view id.
#[tokio::test]
async fn row_delete_rejects_view_not_found() {
    let row_id = seed_row("row-delete-noview").await;
    let p = promise(
        &promise_id("row-delete-noview"),
        "row.delete",
        json!({"table_id": ok_table(), "row_id": row_id, "view": NO_VIEW}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "view_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `invalid_request` — `table_id` is required by the Param schema.
#[tokio::test]
async fn row_delete_rejects_invalid_request() {
    let p = promise(
        &promise_id("row-delete-bad"),
        "row.delete",
        json!({"row_id": 1}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("table_id"),
        "{value}"
    );
}

/// halt — the credential is rejected.
#[tokio::test]
async fn row_delete_halts_on_rejected_credentials() {
    let p = promise(
        &promise_id("row-delete-halt"),
        "row.delete",
        json!({"table_id": ok_table(), "row_id": 1}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("ERROR_INVALID_CREDENTIALS"), "{reason}");
}

// ─── row.move (§4.6) ──────────────────────────────────────────────────────────

/// resolved — = response.body, the moved row.
#[tokio::test]
async fn row_move_resolves_with_the_moved_row() {
    let first = seed_row("row-move-first").await;
    let second = seed_row("row-move-second").await;
    let p = promise(
        &promise_id("row-move"),
        "row.move",
        json!({
            "table_id": ok_table(),
            "row_id": second,
            "before_id": first,
            "user_field_names": true,
        }),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["id"], second);
    assert_eq!(value["Name"], "row-move-second");
    assert!(value["order"].is_string(), "{value}");
}

/// `table_not_found` — a nonexistent table id.
#[tokio::test]
async fn row_move_rejects_table_not_found() {
    let p = promise(
        &promise_id("row-move-notable"),
        "row.move",
        json!({"table_id": NO_TABLE, "row_id": 1}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "table_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `row_not_found` — the `before_id` row does not exist.
#[tokio::test]
async fn row_move_rejects_row_not_found() {
    let row_id = seed_row("row-move-norow").await;
    let p = promise(
        &promise_id("row-move-norow"),
        "row.move",
        json!({"table_id": ok_table(), "row_id": row_id, "before_id": NO_ROW}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "row_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `view_not_found` — a nonexistent view id.
#[tokio::test]
async fn row_move_rejects_view_not_found() {
    let row_id = seed_row("row-move-noview").await;
    let p = promise(
        &promise_id("row-move-noview"),
        "row.move",
        json!({"table_id": ok_table(), "row_id": row_id, "view": NO_VIEW}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "view_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `invalid_request` — `row_id` is required by the Param schema.
#[tokio::test]
async fn row_move_rejects_invalid_request() {
    let p = promise(
        &promise_id("row-move-bad"),
        "row.move",
        json!({"table_id": ok_table()}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("row_id"),
        "{value}"
    );
}

/// halt — the credential is rejected.
#[tokio::test]
async fn row_move_halts_on_rejected_credentials() {
    let p = promise(
        &promise_id("row-move-halt"),
        "row.move",
        json!({"table_id": ok_table(), "row_id": 1}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("ERROR_INVALID_CREDENTIALS"), "{reason}");
}

// ─── rows.create (§4.7) ───────────────────────────────────────────────────────

/// resolved — = response.body, the created rows under `items`.
#[tokio::test]
async fn rows_create_resolves_with_the_created_rows() {
    let p = promise(
        &promise_id("rows-create"),
        "rows.create",
        json!({
            "table_id": ok_table(),
            "rows": [{"Name": "batch-a"}, {"Name": "batch-b"}],
            "user_field_names": true,
        }),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    let items = value["items"].as_array().expect("items is an array");
    assert_eq!(items.len(), 2, "{value}");
    assert_eq!(items[0]["Name"], "batch-a");
    assert_eq!(items[1]["Name"], "batch-b");
    assert!(items[0]["id"].is_i64(), "{value}");
}

/// `table_not_found` — a nonexistent table id.
#[tokio::test]
async fn rows_create_rejects_table_not_found() {
    let p = promise(
        &promise_id("rows-create-notable"),
        "rows.create",
        json!({"table_id": NO_TABLE, "rows": [{}]}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "table_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `row_not_found` — the `before` row does not exist.
#[tokio::test]
async fn rows_create_rejects_row_not_found() {
    let p = promise(
        &promise_id("rows-create-norow"),
        "rows.create",
        json!({"table_id": ok_table(), "rows": [{}], "before": NO_ROW}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "row_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `view_not_found` — a nonexistent view id.
#[tokio::test]
async fn rows_create_rejects_view_not_found() {
    let p = promise(
        &promise_id("rows-create-noview"),
        "rows.create",
        json!({"table_id": ok_table(), "rows": [{}], "view": NO_VIEW}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "view_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `invalid_request` — the failing work item's number field.
#[tokio::test]
async fn rows_create_rejects_invalid_request() {
    let p = promise(
        &promise_id("rows-create-bad"),
        "rows.create",
        json!({
            "table_id": fail_table(),
            "rows": [{"Amount": "not-a-number"}],
            "user_field_names": true,
        }),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(!value["detail"].is_null(), "{value}");
}

/// halt — the credential is rejected.
#[tokio::test]
async fn rows_create_halts_on_rejected_credentials() {
    let p = promise(
        &promise_id("rows-create-halt"),
        "rows.create",
        json!({"table_id": ok_table(), "rows": [{}]}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("ERROR_INVALID_CREDENTIALS"), "{reason}");
}

// ─── rows.update (§4.8) ───────────────────────────────────────────────────────

/// resolved — = response.body, the updated rows under `items`.
#[tokio::test]
async fn rows_update_resolves_with_the_updated_rows() {
    let a = seed_row("rows-update-a").await;
    let b = seed_row("rows-update-b").await;
    let p = promise(
        &promise_id("rows-update"),
        "rows.update",
        json!({
            "table_id": ok_table(),
            "rows": [{"id": a, "Notes": "one"}, {"id": b, "Notes": "two"}],
            "user_field_names": true,
        }),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    let items = value["items"].as_array().expect("items is an array");
    assert_eq!(items.len(), 2, "{value}");
    assert_eq!(items[0]["id"], a);
    assert_eq!(items[0]["Notes"], "one");
    assert_eq!(items[1]["Notes"], "two");
}

/// `table_not_found` — a nonexistent table id.
#[tokio::test]
async fn rows_update_rejects_table_not_found() {
    let p = promise(
        &promise_id("rows-update-notable"),
        "rows.update",
        json!({"table_id": NO_TABLE, "rows": [{"id": 1}]}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "table_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `row_not_found` — one item names a row that does not exist.
#[tokio::test]
async fn rows_update_rejects_row_not_found() {
    let p = promise(
        &promise_id("rows-update-norow"),
        "rows.update",
        json!({"table_id": ok_table(), "rows": [{"id": NO_ROW}]}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "row_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `view_not_found` — a nonexistent view id.
#[tokio::test]
async fn rows_update_rejects_view_not_found() {
    let row_id = seed_row("rows-update-noview").await;
    let p = promise(
        &promise_id("rows-update-noview"),
        "rows.update",
        json!({"table_id": ok_table(), "rows": [{"id": row_id}], "view": NO_VIEW}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "view_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `row_ids_not_unique` — one row id appears twice.
#[tokio::test]
async fn rows_update_rejects_row_ids_not_unique() {
    let row_id = seed_row("rows-update-dup").await;
    let p = promise(
        &promise_id("rows-update-dup"),
        "rows.update",
        json!({
            "table_id": ok_table(),
            "rows": [{"id": row_id}, {"id": row_id}],
        }),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "row_ids_not_unique");
    assert!(value["detail"].is_string(), "{value}");
}

/// `invalid_request` — the failing work item's number field.
#[tokio::test]
async fn rows_update_rejects_invalid_request() {
    let row_id = seed_fail_row("rows-update-bad").await;
    let p = promise(
        &promise_id("rows-update-bad"),
        "rows.update",
        json!({
            "table_id": fail_table(),
            "rows": [{"id": row_id, "Amount": "not-a-number"}],
            "user_field_names": true,
        }),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(!value["detail"].is_null(), "{value}");
}

/// halt — the credential is rejected.
#[tokio::test]
async fn rows_update_halts_on_rejected_credentials() {
    let p = promise(
        &promise_id("rows-update-halt"),
        "rows.update",
        json!({"table_id": ok_table(), "rows": [{"id": 1}]}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("ERROR_INVALID_CREDENTIALS"), "{reason}");
}

// ─── rows.delete (§4.9) ───────────────────────────────────────────────────────

/// resolved — the endpoint answers 204, so the Resolved value is empty.
#[tokio::test]
async fn rows_delete_resolves_with_an_empty_object() {
    let a = seed_row("rows-delete-a").await;
    let b = seed_row("rows-delete-b").await;
    let p = promise(
        &promise_id("rows-delete"),
        "rows.delete",
        json!({"table_id": ok_table(), "row_ids": [a, b]}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({}));
    assert_eq!(row_status(ok_table(), a).await, 404, "row {a} survived");
    assert_eq!(row_status(ok_table(), b).await, 404, "row {b} survived");
}

/// `table_not_found` — a nonexistent table id.
#[tokio::test]
async fn rows_delete_rejects_table_not_found() {
    let p = promise(
        &promise_id("rows-delete-notable"),
        "rows.delete",
        json!({"table_id": NO_TABLE, "row_ids": [1]}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "table_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `row_not_found` — one listed row does not exist. No row is deleted when
/// any id in the list fails.
#[tokio::test]
async fn rows_delete_rejects_row_not_found() {
    let row_id = seed_row("rows-delete-norow").await;
    let p = promise(
        &promise_id("rows-delete-norow"),
        "rows.delete",
        json!({"table_id": ok_table(), "row_ids": [row_id, NO_ROW]}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "row_not_found");
    assert!(value["detail"].is_string(), "{value}");
    assert_eq!(row_status(ok_table(), row_id).await, 200, "row {row_id} was deleted");
}

/// `view_not_found` — a nonexistent view id.
#[tokio::test]
async fn rows_delete_rejects_view_not_found() {
    let row_id = seed_row("rows-delete-noview").await;
    let p = promise(
        &promise_id("rows-delete-noview"),
        "rows.delete",
        json!({"table_id": ok_table(), "row_ids": [row_id], "view": NO_VIEW}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "view_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `row_ids_not_unique` — one row id appears twice.
#[tokio::test]
async fn rows_delete_rejects_row_ids_not_unique() {
    let row_id = seed_row("rows-delete-dup").await;
    let p = promise(
        &promise_id("rows-delete-dup"),
        "rows.delete",
        json!({"table_id": ok_table(), "row_ids": [row_id, row_id]}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "row_ids_not_unique");
    assert!(value["detail"].is_string(), "{value}");
}

/// `invalid_request` — `row_ids` is required by the Param schema.
#[tokio::test]
async fn rows_delete_rejects_invalid_request() {
    let p = promise(
        &promise_id("rows-delete-bad"),
        "rows.delete",
        json!({"table_id": ok_table()}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("row_ids"),
        "{value}"
    );
}

/// halt — the credential is rejected.
#[tokio::test]
async fn rows_delete_halts_on_rejected_credentials() {
    let p = promise(
        &promise_id("rows-delete-halt"),
        "rows.delete",
        json!({"table_id": ok_table(), "row_ids": [1]}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("ERROR_INVALID_CREDENTIALS"), "{reason}");
}

// ─── export.create (§4.10) ────────────────────────────────────────────────────

/// resolved — the succeeding work item, and the pending → terminal path: the
/// create response is state "pending" with a null `url`, so a resolved value
/// carrying state "finished", an `exported_file_name` and a `url` is proof
/// the poll loop watched the job to its terminal state rather than reporting
/// the create.
#[tokio::test]
async fn export_create_resolves_when_the_export_finishes() {
    let p = promise(
        &promise_id("export-create"),
        "export.create",
        json!({"table_id": ok_table(), "exporter_type": "csv"}),
        in_ms(120_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["state"], "finished");
    assert_eq!(value["exporter_type"], "csv");
    assert_eq!(value["table"], ok_table());
    assert_eq!(value["view"], Value::Null);
    assert!(value["id"].is_i64(), "{value}");
    assert!(value["exported_file_name"].is_string(), "{value}");
    assert!(value["url"].is_string(), "{value}");
    assert!(value["created_at"].is_string(), "{value}");
    assert_eq!(value["progress_percentage"], json!(100.0));
    // The Resolved schema is exactly these nine keys.
    assert_eq!(value.as_object().expect("object").len(), 9, "{value}");
}

/// `table_not_found` — a nonexistent table id.
#[tokio::test]
async fn export_create_rejects_table_not_found() {
    let p = promise(
        &promise_id("export-notable"),
        "export.create",
        json!({"table_id": NO_TABLE, "exporter_type": "csv"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "table_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `view_not_found` — a nonexistent view id.
#[tokio::test]
async fn export_create_rejects_view_not_found() {
    let p = promise(
        &promise_id("export-noview"),
        "export.create",
        json!({"table_id": ok_table(), "exporter_type": "csv", "view_id": NO_VIEW}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "view_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `invalid_request` — `exporter_type` is an enum in the Param schema.
#[tokio::test]
async fn export_create_rejects_invalid_request() {
    let p = promise(
        &promise_id("export-bad"),
        "export.create",
        json!({"table_id": ok_table(), "exporter_type": "not-an-exporter"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(value["detail"]["exporter_type"].is_array(), "{value}");
}

/// halt — the credential is rejected.
#[tokio::test]
async fn export_create_halts_on_rejected_credentials() {
    let p = promise(
        &promise_id("export-halt"),
        "export.create",
        json!({"table_id": ok_table(), "exporter_type": "csv"}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("ERROR_INVALID_CREDENTIALS"), "{reason}");
}

/// halt — §4.10.1: only the csv exporter is registered without a licence,
/// and the 402 `ERROR_FEATURE_NOT_AVAILABLE` the others answer is
/// `_check`'s second halt: an operator must license the instance.
#[tokio::test]
async fn export_create_halts_when_the_exporter_needs_a_licence() {
    let p = promise(
        &promise_id("export-licence"),
        "export.create",
        json!({"table_id": ok_table(), "exporter_type": "json"}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&config(), &p).await);

    assert!(reason.contains("ERROR_FEATURE_NOT_AVAILABLE"), "{reason}");
}

/// The deadline — `timeout_at` already in the past. No verdict: the server
/// settles a timed-out promise itself.
#[tokio::test]
async fn export_create_releases_at_the_deadline() {
    let p = promise(
        &promise_id("export-deadline"),
        "export.create",
        json!({"table_id": ok_table(), "exporter_type": "csv"}),
        in_ms(-1_000),
    );

    let reason = released(plugin::process(&config(), &p).await);

    assert_eq!(reason, "promise timed out");
}

// ─── export.get (§4.11) ───────────────────────────────────────────────────────

/// resolved — = response.body, the export job.
#[tokio::test]
async fn export_get_resolves_with_the_job() {
    let job_id = seed_export().await;
    let p = promise(
        &promise_id("export-get"),
        "export.get",
        json!({"job_id": job_id}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["id"], job_id);
    assert_eq!(value["exporter_type"], "csv");
    assert!(value["state"].is_string(), "{value}");
    assert!(value["status"].is_string(), "{value}");
    assert!(value["created_at"].is_string(), "{value}");
}

/// `job_not_found` — a nonexistent job id.
#[tokio::test]
async fn export_get_rejects_job_not_found() {
    let p = promise(
        &promise_id("export-get-404"),
        "export.get",
        json!({"job_id": NO_JOB}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "job_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `invalid_request` — `job_id` is required by the Param schema.
#[tokio::test]
async fn export_get_rejects_invalid_request() {
    let p = promise(
        &promise_id("export-get-bad"),
        "export.get",
        json!({}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("job_id"),
        "{value}"
    );
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

// ─── table.import (§4.12) ─────────────────────────────────────────────────────

/// resolved — the succeeding work item, and the pending → terminal path: the
/// create response is state "pending" with `progress_percentage` 0, so a
/// resolved value at "finished" and 100 is proof the poll loop watched the
/// job. `original_file_name` is the stamp = sanitize(promise.id).
#[tokio::test]
async fn table_import_resolves_when_the_import_finishes() {
    let id = promise_id("table-import");
    let p = promise(
        &id,
        "table.import",
        json!({"table_id": ok_table(), "data": [["imported", "by table.import"]]}),
        in_ms(300_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["state"], "finished");
    assert_eq!(value["type"], "file_import");
    assert_eq!(value["table_id"], ok_table());
    assert_eq!(value["progress_percentage"], 100);
    assert_eq!(value["human_readable_error"], "");
    assert_eq!(value["report"]["failing_rows"], json!({}));
    // = sanitize(promise.id)
    let stamp = value["original_file_name"].as_str().expect("original_file_name");
    assert!(stamp.starts_with(&id), "{stamp} should be derived from {id}");
    assert!(value["database_id"].is_i64(), "{value}");
    // The Resolved schema is exactly these nine keys.
    assert_eq!(value.as_object().expect("object").len(), 9, "{value}");
}

/// resolved — the failing work item. §5 provisions `Amounts` with a number
/// field, so a non-numeric value leaves the job at "finished" with that row
/// named in `report.failing_rows` by its index in `data`: those rows were
/// rejected by their field types and were not created.
#[tokio::test]
async fn table_import_resolves_with_failing_rows() {
    let p = promise(
        &promise_id("table-import-failing"),
        "table.import",
        json!({"table_id": fail_table(), "data": [["a", "not-a-number"]]}),
        in_ms(300_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["state"], "finished");
    assert_eq!(value["table_id"], fail_table());
    let failing = value["report"]["failing_rows"]
        .as_object()
        .expect("failing_rows is an object");
    assert_eq!(failing.len(), 1, "{value}");
    assert!(failing.contains_key("0"), "{value}");
}

/// Re-entry — the job an earlier attempt created is recovered by its stamp,
/// so a second delivery attaches to it rather than importing the rows twice.
#[tokio::test]
async fn table_import_reattaches_on_re_entry() {
    let id = promise_id("table-import-reentry");
    let p = promise(
        &id,
        "table.import",
        json!({"table_id": ok_table(), "data": [["reentry", "once only"]]}),
        in_ms(300_000),
    );

    let first = resolved(plugin::process(&config(), &p).await);
    let second = resolved(plugin::process(&config(), &p).await);

    assert_eq!(first["id"], second["id"]);
    assert_eq!(first["original_file_name"], second["original_file_name"]);
    // One promise, one job.
    let stamp = first["original_file_name"].as_str().expect("original_file_name");
    assert_eq!(import_jobs_stamped(stamp).await, 1);
}

/// `table_not_found` — a nonexistent table id.
#[tokio::test]
async fn table_import_rejects_table_not_found() {
    let p = promise(
        &promise_id("table-import-notable"),
        "table.import",
        json!({"table_id": NO_TABLE, "data": [["x"]]}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "table_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `invalid_request` — the Param schema requires at least one row in `data`.
#[tokio::test]
async fn table_import_rejects_invalid_request() {
    let p = promise(
        &promise_id("table-import-bad"),
        "table.import",
        json!({"table_id": ok_table(), "data": []}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(value["detail"]["data"].is_array(), "{value}");
}

/// halt — the credential is rejected.
#[tokio::test]
async fn table_import_halts_on_rejected_credentials() {
    let p = promise(
        &promise_id("table-import-halt"),
        "table.import",
        json!({"table_id": ok_table(), "data": [["x"]]}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("ERROR_INVALID_CREDENTIALS"), "{reason}");
}

/// The deadline — `timeout_at` already in the past.
#[tokio::test]
async fn table_import_releases_at_the_deadline() {
    let p = promise(
        &promise_id("table-import-deadline"),
        "table.import",
        json!({"table_id": ok_table(), "data": [["deadline", "row"]]}),
        in_ms(-1_000),
    );

    let reason = released(plugin::process(&config(), &p).await);

    assert_eq!(reason, "promise timed out");
}

// ─── job.get (§4.13) ──────────────────────────────────────────────────────────

/// resolved — = response.body, the job.
#[tokio::test]
async fn job_get_resolves_with_the_job() {
    let job_id = seed_import_job().await;
    let p = promise(
        &promise_id("job-get"),
        "job.get",
        json!({"job_id": job_id}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["id"], job_id);
    assert_eq!(value["type"], "file_import");
    assert!(value["state"].is_string(), "{value}");
    assert!(value["report"]["failing_rows"].is_object(), "{value}");
}

/// `job_not_found` — a nonexistent job id.
#[tokio::test]
async fn job_get_rejects_job_not_found() {
    let p = promise(
        &promise_id("job-get-404"),
        "job.get",
        json!({"job_id": NO_JOB}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "job_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `invalid_request` — `job_id` is required by the Param schema.
#[tokio::test]
async fn job_get_rejects_invalid_request() {
    let p = promise(&promise_id("job-get-bad"), "job.get", json!({}), in_ms(60_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("job_id"),
        "{value}"
    );
}

/// halt — the credential is rejected.
#[tokio::test]
async fn job_get_halts_on_rejected_credentials() {
    let p = promise(
        &promise_id("job-get-halt"),
        "job.get",
        json!({"job_id": 1}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("ERROR_INVALID_CREDENTIALS"), "{reason}");
}

// ─── database.list (§4.14) ────────────────────────────────────────────────────

/// resolved — = response.body, every application these credentials can see,
/// with the tables of each database inline.
///
/// `invalid_request` is this operation's only rejection code and its Param
/// schema takes no arguments, so nothing about the promise can induce it:
/// the endpoint has no query to malform.
#[tokio::test]
async fn database_list_resolves_with_the_applications() {
    let p = promise(
        &promise_id("database-list"),
        "database.list",
        json!({}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    let apps = value.as_array().expect("the body is an array");
    let database = apps
        .iter()
        .find(|a| a["type"] == "database")
        .expect("a database application");
    assert!(database["id"].is_i64(), "{value}");
    assert!(database["name"].is_string(), "{value}");
    assert!(database["workspace"]["id"].is_i64(), "{value}");
    assert!(database["created_on"].is_string(), "{value}");
    let tables = database["tables"].as_array().expect("tables inline");
    assert!(
        tables.iter().any(|t| t["id"] == ok_table()),
        "the fixture table is not listed: {value}"
    );
}

/// halt — the credential is rejected.
#[tokio::test]
async fn database_list_halts_on_rejected_credentials() {
    let p = promise(
        &promise_id("database-list-halt"),
        "database.list",
        json!({}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("ERROR_INVALID_CREDENTIALS"), "{reason}");
}

// ─── table.list (§4.15) ───────────────────────────────────────────────────────

/// resolved — = response.body, the tables of one database.
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
    let table = tables
        .iter()
        .find(|t| t["id"] == ok_table())
        .expect("the fixture table");
    assert!(table["name"].is_string(), "{value}");
    assert!(table["order"].is_i64(), "{value}");
    assert_eq!(table["data_sync"], Value::Null);
}

/// `database_not_found` — a nonexistent database id.
#[tokio::test]
async fn table_list_rejects_database_not_found() {
    let p = promise(
        &promise_id("table-list-404"),
        "table.list",
        json!({"database_id": NO_DATABASE}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "database_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `invalid_request` — `database_id` is required by the Param schema.
#[tokio::test]
async fn table_list_rejects_invalid_request() {
    let p = promise(
        &promise_id("table-list-bad"),
        "table.list",
        json!({}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("database_id"),
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

// ─── field.list (§4.16) ───────────────────────────────────────────────────────

/// resolved — = response.body, the fields of one table.
#[tokio::test]
async fn field_list_resolves_with_the_fields() {
    let p = promise(
        &promise_id("field-list"),
        "field.list",
        json!({"table_id": fail_table()}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    let fields = value.as_array().expect("the body is an array");
    let amount = fields
        .iter()
        .find(|f| f["name"] == "Amount")
        .expect("the Amount field §5 provisions");
    assert_eq!(amount["type"], "number");
    assert_eq!(amount["table_id"], fail_table());
    assert_eq!(amount["primary"], false);
    assert_eq!(amount["read_only"], false);
    assert!(amount["id"].is_i64(), "{value}");
    assert!(amount["order"].is_i64(), "{value}");
}

/// `table_not_found` — a nonexistent table id.
#[tokio::test]
async fn field_list_rejects_table_not_found() {
    let p = promise(
        &promise_id("field-list-404"),
        "field.list",
        json!({"table_id": NO_TABLE}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "table_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `invalid_request` — `table_id` is required by the Param schema.
#[tokio::test]
async fn field_list_rejects_invalid_request() {
    let p = promise(
        &promise_id("field-list-bad"),
        "field.list",
        json!({}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("table_id"),
        "{value}"
    );
}

/// halt — the credential is rejected.
#[tokio::test]
async fn field_list_halts_on_rejected_credentials() {
    let p = promise(
        &promise_id("field-list-halt"),
        "field.list",
        json!({"table_id": ok_table()}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("ERROR_INVALID_CREDENTIALS"), "{reason}");
}

// ─── view.list (§4.17) ────────────────────────────────────────────────────────

/// resolved — = response.body, the views of one table.
#[tokio::test]
async fn view_list_resolves_with_the_views() {
    let p = promise(
        &promise_id("view-list"),
        "view.list",
        json!({"table_id": ok_table(), "type": "grid", "include": ["filters", "sortings"]}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    let views = value.as_array().expect("the body is an array");
    assert!(!views.is_empty(), "{value}");
    assert_eq!(views[0]["table_id"], ok_table());
    assert_eq!(views[0]["type"], "grid");
    assert!(views[0]["id"].is_i64(), "{value}");
    assert!(views[0]["name"].is_string(), "{value}");
    // include=filters,sortings — comma-joined into a single query value.
    assert!(views[0]["filters"].is_array(), "{value}");
    assert!(views[0]["sortings"].is_array(), "{value}");
}

/// `table_not_found` — a nonexistent table id.
#[tokio::test]
async fn view_list_rejects_table_not_found() {
    let p = promise(
        &promise_id("view-list-404"),
        "view.list",
        json!({"table_id": NO_TABLE}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "table_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `invalid_request` — `limit` is documented as an integer.
#[tokio::test]
async fn view_list_rejects_invalid_request() {
    let p = promise(
        &promise_id("view-list-bad"),
        "view.list",
        json!({"table_id": ok_table(), "limit": "not-an-integer"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(value["detail"]["limit"].is_array(), "{value}");
}

/// halt — the credential is rejected.
#[tokio::test]
async fn view_list_halts_on_rejected_credentials() {
    let p = promise(
        &promise_id("view-list-halt"),
        "view.list",
        json!({"table_id": ok_table()}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("ERROR_INVALID_CREDENTIALS"), "{reason}");
}

// ─── rowhistory.list (§4.18) ──────────────────────────────────────────────────

/// resolved — = response.body, one page of the row's change history, with
/// the before and after values of every field an action touched.
#[tokio::test]
async fn rowhistory_list_resolves_with_the_page() {
    let row_id = seed_row("rowhistory").await;
    update_row(ok_table(), row_id, json!({"Notes": "history"})).await;
    let p = promise(
        &promise_id("rowhistory"),
        "rowhistory.list",
        json!({"table_id": ok_table(), "row_id": row_id, "limit": 200, "offset": 0}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert!(value["count"].is_number(), "{value}");
    assert_eq!(value["previous"], Value::Null);
    let results = value["results"].as_array().expect("results is an array");
    assert!(!results.is_empty(), "no history for row {row_id}: {value}");
    assert_eq!(results[0]["action_type"], "update_rows");
    assert!(results[0]["timestamp"].is_string(), "{value}");
    assert!(results[0]["before"].is_object(), "{value}");
    assert!(results[0]["after"].is_object(), "{value}");
    assert!(results[0]["fields_metadata"].is_object(), "{value}");
}

/// `table_not_found` — a nonexistent table id.
#[tokio::test]
async fn rowhistory_list_rejects_table_not_found() {
    let p = promise(
        &promise_id("rowhistory-notable"),
        "rowhistory.list",
        json!({"table_id": NO_TABLE, "row_id": 1}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "table_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `row_not_found` — a nonexistent row id.
#[tokio::test]
async fn rowhistory_list_rejects_row_not_found() {
    let p = promise(
        &promise_id("rowhistory-norow"),
        "rowhistory.list",
        json!({"table_id": ok_table(), "row_id": NO_ROW}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "row_not_found");
    assert!(value["detail"].is_string(), "{value}");
}

/// `invalid_request` — `row_id` is required by the Param schema.
#[tokio::test]
async fn rowhistory_list_rejects_invalid_request() {
    let p = promise(
        &promise_id("rowhistory-bad"),
        "rowhistory.list",
        json!({"table_id": ok_table()}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("row_id"),
        "{value}"
    );
}

/// halt — the credential is rejected.
#[tokio::test]
async fn rowhistory_list_halts_on_rejected_credentials() {
    let p = promise(
        &promise_id("rowhistory-halt"),
        "rowhistory.list",
        json!({"table_id": ok_table(), "row_id": 1}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.contains("ERROR_INVALID_CREDENTIALS"), "{reason}");
}

// ─── Dispatch ─────────────────────────────────────────────────────────────────

/// A func no operation serves is a permanent rejection naming it.
#[tokio::test]
async fn an_unknown_func_is_rejected() {
    let p = promise(&promise_id("unknown"), "row.explode", json!({}), in_ms(60_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "unknown_func", "detail": "row.explode"}));
}

/// A param that is not a `{func, args}` document is a permanent rejection:
/// the param is immutable, so no redelivery can make it one.
#[tokio::test]
async fn a_param_without_a_func_is_rejected() {
    let mut p = promise(&promise_id("nofunc"), "row.list", json!({}), in_ms(60_000));
    p.param = PromiseValue {
        headers: None,
        data: Some(base64::engine::general_purpose::STANDARD.encode(r#"{"args":{}}"#)),
    };

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(
        value,
        json!({"code": "invalid_request", "detail": "param has no func"})
    );
}

// ─── Fixtures ─────────────────────────────────────────────────────────────────

/// §2, built from the environment §5.2 exports. `poll` is overridden to 1s:
/// the default 2s would make every pending → terminal test wait a full
/// interval past the job's own end. `poll_export` and `poll_table` are left
/// unset so both cascade to it.
fn config() -> Config {
    Config {
        base_url: env("BASEROW_BASE_URL"),
        email: env("BASEROW_EMAIL"),
        password: env("BASEROW_PASSWORD"),
        poll: Duration::from_secs(1),
        poll_export: None,
        poll_table: None,
    }
}

/// The same Baserow, with a password it will reject.
fn bad_credential() -> Config {
    Config {
        password: format!("{}-wrong", env("BASEROW_PASSWORD")),
        ..config()
    }
}

/// `Tasks`: all text, so every write it is given succeeds.
fn ok_table() -> i64 {
    env("BASEROW_FIXTURE_OK").parse().expect("BASEROW_FIXTURE_OK is an integer")
}

/// `Amounts`: `Amount` is a number field, so a non-numeric value fails.
fn fail_table() -> i64 {
    env("BASEROW_FIXTURE_FAIL")
        .parse()
        .expect("BASEROW_FIXTURE_FAIL is an integer")
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

/// A fresh promise id per test — and one that survives the frame's sanitize
/// unchanged, so a stamp may be checked against it with `starts_with`.
fn promise_id(what: &str) -> String {
    format!("baserow.{what}.{}", nanos())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_millis() as i64
}

fn in_ms(delta: i64) -> i64 {
    now_ms() + delta
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after the epoch")
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

/// A JWT, minted per call: `BASEROW_ACCESS_TOKEN_LIFETIME_MINUTES` is 10 and
/// a whole test binary outlives that.
async fn bearer() -> String {
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

/// A row in `Tasks`, named after the test that asked for it.
async fn seed_row(name: &str) -> i64 {
    create_row(ok_table(), json!({"Name": name, "Notes": "seed"})).await
}

/// A row in `Amounts`, with a value its number field accepts.
async fn seed_fail_row(name: &str) -> i64 {
    create_row(fail_table(), json!({"Label": name, "Amount": 1})).await
}

async fn create_row(table_id: i64, values: Value) -> i64 {
    let body: Value = http()
        .post(format!(
            "{}/api/database/rows/table/{table_id}/?user_field_names=true",
            env("BASEROW_BASE_URL")
        ))
        .header("Authorization", bearer().await)
        .json(&values)
        .send()
        .await
        .expect("create row request")
        .json()
        .await
        .expect("create row response is JSON");
    body["id"].as_i64().unwrap_or_else(|| panic!("no row id in {body}"))
}

async fn update_row(table_id: i64, row_id: i64, values: Value) {
    let status = http()
        .patch(format!(
            "{}/api/database/rows/table/{table_id}/{row_id}/?user_field_names=true",
            env("BASEROW_BASE_URL")
        ))
        .header("Authorization", bearer().await)
        .json(&values)
        .send()
        .await
        .expect("update row request")
        .status();
    assert!(status.is_success(), "updating row {row_id}: {status}");
}

async fn row_status(table_id: i64, row_id: i64) -> u16 {
    http()
        .get(format!(
            "{}/api/database/rows/table/{table_id}/{row_id}/",
            env("BASEROW_BASE_URL")
        ))
        .header("Authorization", bearer().await)
        .send()
        .await
        .expect("get row request")
        .status()
        .as_u16()
}

/// The database the §5 fixture tables live in.
async fn database_id() -> i64 {
    let body: Value = http()
        .get(format!("{}/api/applications/", env("BASEROW_BASE_URL")))
        .header("Authorization", bearer().await)
        .send()
        .await
        .expect("applications request")
        .json()
        .await
        .expect("applications response is JSON");
    body.as_array()
        .expect("applications is an array")
        .iter()
        .find(|a| {
            a["tables"]
                .as_array()
                .is_some_and(|ts| ts.iter().any(|t| t["id"] == ok_table()))
        })
        .and_then(|a| a["id"].as_i64())
        .expect("the database holding the fixture table")
}

/// An export job to read back with export.get.
async fn seed_export() -> i64 {
    let body: Value = http()
        .post(format!(
            "{}/api/database/export/table/{}/",
            env("BASEROW_BASE_URL"),
            ok_table()
        ))
        .header("Authorization", bearer().await)
        .json(&json!({"exporter_type": "csv"}))
        .send()
        .await
        .expect("export request")
        .json()
        .await
        .expect("export response is JSON");
    body["id"].as_i64().unwrap_or_else(|| panic!("no export job id in {body}"))
}

/// A file_import job to read back with job.get.
async fn seed_import_job() -> i64 {
    let body: Value = http()
        .post(format!(
            "{}/api/database/tables/{}/import/async/",
            env("BASEROW_BASE_URL"),
            ok_table()
        ))
        .header("Authorization", bearer().await)
        .json(&json!({
            "data": [["seeded", "for job.get"]],
            "original_file_name": format!("seed-{}", nanos()),
        }))
        .send()
        .await
        .expect("import request")
        .json()
        .await
        .expect("import response is JSON");
    body["id"].as_i64().unwrap_or_else(|| panic!("no job id in {body}"))
}

/// How many of this account's file_import jobs carry this stamp.
async fn import_jobs_stamped(stamp: &str) -> usize {
    let body: Value = http()
        .get(format!(
            "{}/api/jobs/?type=file_import&limit=100&offset=0",
            env("BASEROW_BASE_URL")
        ))
        .header("Authorization", bearer().await)
        .send()
        .await
        .expect("jobs request")
        .json()
        .await
        .expect("jobs response is JSON");
    body["jobs"]
        .as_array()
        .expect("jobs is an array")
        .iter()
        .filter(|j| j["original_file_name"] == stamp)
        .count()
}
