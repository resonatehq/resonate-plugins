//! `plugin::process` against a real n8n — never a mock.
//!
//! Bring the provider up with specification §5.1/§5.2 first; the exported
//! `N8N_BASE_URL`, `N8N_API_KEY`, `N8N_FIXTURE_OK` and `N8N_FIXTURE_FAIL`
//! are what [`config`] and the tests read. Every documented condition below
//! is induced with real inputs against that instance — a failing workflow
//! really fails, a missing id is really missing, a corrupted key is really
//! rejected.
//!
//! §5 provisions three published workflows — `plugin-helper`, `plugin-ok`
//! and `plugin-fail` — and one failed execution of each of the last two:
//! `N8N_FIXTURE_OK`, whose retry now succeeds because the helper it calls
//! has since been published, and `N8N_FIXTURE_FAIL`, whose target is never
//! registered and whose retry fails again. Everything else a condition needs
//! — a workflow to archive, an execution to stop, a retry slow enough to be
//! caught in flight — is provisioned here with raw provider calls, under
//! names and webhook paths made unique per run so the suite can be run again
//! against the same instance.
//!
//! Not covered: `execution_crashed`. n8n records that status for an
//! execution whose own process died under it, which no API call induces;
//! the branch is reachable only by killing the instance mid-run.

use std::collections::HashMap;
use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};

use resonate_core::types::{PromiseRecord, PromiseState, PromiseValue};
use resonate_plugin_n8n::plugin::{self, Config};

/// The verdict `process` returns, spelled out: the crate's own alias for it
/// is an implementation detail.
type Verdict = Result<Result<String, String>, Result<String, String>>;

// ─── workflow.create (§4.1) ───────────────────────────────────────────────────

/// resolved — = response.body, the workflow n8n stored.
#[tokio::test]
async fn workflow_create_resolves_with_the_workflow() {
    let name = unique("create");
    let p = promise(
        &promise_id("wf-create"),
        "workflow.create",
        json!({"name": name, "nodes": [], "connections": {}, "settings": {"executionOrder": "v1"}}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["name"], name);
    assert!(value["id"].is_string(), "{value}");
    // Created unpublished: its triggers are not registered until
    // workflow.publish runs.
    assert_eq!(value["active"], false);
    assert_eq!(value["activeVersionId"], Value::Null);
    assert_eq!(value["isArchived"], false);

    delete_workflow(value["id"].as_str().expect("id")).await;
}

/// `not_found` — a projectId that names nothing.
#[tokio::test]
async fn workflow_create_rejects_not_found() {
    let p = promise(
        &promise_id("wf-create-404"),
        "workflow.create",
        json!({
            "name": unique("create-404"),
            "nodes": [],
            "connections": {},
            "settings": {"executionOrder": "v1"},
            "projectId": "no_such_project_e0f1",
        }),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "not_found");
    // detail = response.body.message of the 404, naming the missing project.
    assert_eq!(value["detail"], "Project not found");
}

/// `invalid_request` — `name` is required by the Param schema.
#[tokio::test]
async fn workflow_create_rejects_invalid_request() {
    let p = promise(
        &promise_id("wf-create-bad"),
        "workflow.create",
        json!({"nodes": [], "connections": {}, "settings": {"executionOrder": "v1"}}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    // detail = response.body.message of the 4xx, naming the property.
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("name"),
        "{value}"
    );
}

/// Re-entry — n8n assigns the id and accepts no client-supplied one, so a
/// redelivery after a lost response creates a second, unpublished workflow.
/// The Param schema says so; this is what it looks like.
#[tokio::test]
async fn workflow_create_creates_a_second_workflow_on_re_entry() {
    let name = unique("create-reentry");
    let p = promise(
        &promise_id("wf-create-reentry"),
        "workflow.create",
        json!({"name": name, "nodes": [], "connections": {}, "settings": {"executionOrder": "v1"}}),
        in_ms(60_000),
    );

    let first = resolved(plugin::process(&config(), &p).await);
    let second = resolved(plugin::process(&config(), &p).await);

    assert_ne!(first["id"], second["id"], "{first} / {second}");
    assert_eq!(first["name"], second["name"]);
    assert_eq!(second["active"], false);

    delete_workflow(first["id"].as_str().expect("id")).await;
    delete_workflow(second["id"].as_str().expect("id")).await;
}

/// halt — the API key is rejected, and no retry of ours can fix that.
#[tokio::test]
async fn workflow_create_halts_on_a_rejected_key() {
    let p = promise(
        &promise_id("wf-create-halt"),
        "workflow.create",
        json!({"name": unique("halt"), "nodes": [], "connections": {}, "settings": {}}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.to_lowercase().contains("unauthorized"), "{reason}");
}

// ─── workflow.get (§4.2) ──────────────────────────────────────────────────────

/// resolved — = response.body.
#[tokio::test]
async fn workflow_get_resolves_with_the_workflow() {
    let name = unique("get");
    let id = create_workflow(json!({
        "name": name, "nodes": [], "connections": {}, "settings": {"executionOrder": "v1"}
    }))
    .await;
    let p = promise(
        &promise_id("wf-get"),
        "workflow.get",
        json!({"id": id, "excludePinnedData": "true"}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["id"], id);
    assert_eq!(value["name"], name);
    assert!(value["versionId"].is_string(), "{value}");

    delete_workflow(&id).await;
}

/// `not_found` — a workflow id that names nothing.
#[tokio::test]
async fn workflow_get_rejects_not_found() {
    let p = promise(
        &promise_id("wf-get-404"),
        "workflow.get",
        json!({"id": "no_such_workflow_e0f1"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "not_found");
    assert_eq!(value["detail"], "Not Found");
}

/// `invalid_request` — `id` is required by the Param schema.
#[tokio::test]
async fn workflow_get_rejects_invalid_request() {
    let p = promise(&promise_id("wf-get-bad"), "workflow.get", json!({}), in_ms(60_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("id"),
        "{value}"
    );
}

/// halt — the API key is rejected.
#[tokio::test]
async fn workflow_get_halts_on_a_rejected_key() {
    let p = promise(
        &promise_id("wf-get-halt"),
        "workflow.get",
        json!({"id": "whatever"}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.to_lowercase().contains("unauthorized"), "{reason}");
}

// ─── workflow.list (§4.3) ─────────────────────────────────────────────────────

/// resolved — = response.body, one page. The filters pass straight through.
#[tokio::test]
async fn workflow_list_resolves_with_the_page() {
    let name = unique("list");
    let id = create_workflow(json!({
        "name": name, "nodes": [], "connections": {}, "settings": {"executionOrder": "v1"}
    }))
    .await;
    let p = promise(
        &promise_id("wf-list"),
        "workflow.list",
        json!({"name": name, "active": "false", "excludePinnedData": "true", "limit": 10}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    let data = value["data"].as_array().expect("data");
    assert_eq!(data.len(), 1, "{value}");
    assert_eq!(data[0]["id"], id);
    assert_eq!(data[0]["name"], name);
    // Last page: paging is the caller's loop, and there is nothing after it.
    assert_eq!(value["nextCursor"], Value::Null);

    delete_workflow(&id).await;
}

/// `invalid_request` — a cursor n8n did not issue.
#[tokio::test]
async fn workflow_list_rejects_invalid_request() {
    let p = promise(
        &promise_id("wf-list-bad"),
        "workflow.list",
        json!({"cursor": "not-a-cursor"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert_eq!(value["detail"], "An invalid cursor was provided");
}

/// halt — the API key is rejected.
#[tokio::test]
async fn workflow_list_halts_on_a_rejected_key() {
    let p = promise(&promise_id("wf-list-halt"), "workflow.list", json!({}), in_ms(60_000));

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.to_lowercase().contains("unauthorized"), "{reason}");
}

// ─── workflow.update (§4.4) ───────────────────────────────────────────────────

/// resolved — = response.body, the workflow as stored, at a new version.
#[tokio::test]
async fn workflow_update_resolves_with_the_workflow() {
    let id = create_workflow(json!({
        "name": unique("update"), "nodes": [], "connections": {},
        "settings": {"executionOrder": "v1"}
    }))
    .await;
    let renamed = unique("update-renamed");
    let p = promise(
        &promise_id("wf-update"),
        "workflow.update",
        json!({
            "id": id,
            "name": renamed,
            "nodes": [],
            "connections": {},
            "settings": {"executionOrder": "v1"},
            "publishIfActive": false,
        }),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["id"], id);
    assert_eq!(value["name"], renamed);
    // n8n rejects a body carrying id or publishIfActive, so a 200 is proof
    // that neither reached it: they are the path and the query.
    assert!(value["versionId"].is_string(), "{value}");

    delete_workflow(&id).await;
}

/// `not_found` — a workflow id that names nothing.
#[tokio::test]
async fn workflow_update_rejects_not_found() {
    let p = promise(
        &promise_id("wf-update-404"),
        "workflow.update",
        json!({
            "id": "no_such_workflow_e0f1",
            "name": "x",
            "nodes": [],
            "connections": {},
            "settings": {"executionOrder": "v1"},
        }),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "not_found");
    assert!(!value["detail"].is_null(), "{value}");
}

/// `conflict` — the new version cannot be published: its webhook path is one
/// another published workflow already holds. The version is still saved.
#[tokio::test]
async fn workflow_update_rejects_conflict() {
    let held = unique_path("held");
    let holder = create_workflow(webhook_workflow(&unique("update-holder"), &held, None)).await;
    publish(&holder).await;
    let mine = create_workflow(webhook_workflow(
        &unique("update-mine"),
        &unique_path("mine"),
        None,
    ))
    .await;
    publish(&mine).await;

    // The same workflow, moved onto the webhook path the other one holds.
    let taken = webhook_workflow(&unique("update-taken"), &held, None);
    let p = promise(
        &promise_id("wf-update-conflict"),
        "workflow.update",
        json!({
            "id": mine,
            "name": taken["name"],
            "nodes": taken["nodes"],
            "connections": taken["connections"],
            "settings": taken["settings"],
        }),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "conflict");
    // detail = response.body of the 409.
    assert!(
        value["detail"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("conflict"),
        "{value}"
    );

    delete_workflow(&mine).await;
    delete_workflow(&holder).await;
}

/// `invalid_request` — a read-only property of a workflow.get response sent
/// back in the body, which the Param schema's description calls out.
#[tokio::test]
async fn workflow_update_rejects_invalid_request() {
    let id = create_workflow(json!({
        "name": unique("update-bad"), "nodes": [], "connections": {},
        "settings": {"executionOrder": "v1"}
    }))
    .await;
    let p = promise(
        &promise_id("wf-update-bad"),
        "workflow.update",
        json!({
            "id": id,
            "name": "x",
            "nodes": [],
            "connections": {},
            "settings": {"executionOrder": "v1"},
            "active": true,
        }),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("active"),
        "{value}"
    );

    delete_workflow(&id).await;
}

/// halt — the API key is rejected.
#[tokio::test]
async fn workflow_update_halts_on_a_rejected_key() {
    let p = promise(
        &promise_id("wf-update-halt"),
        "workflow.update",
        json!({"id": "whatever", "name": "x", "nodes": [], "connections": {}, "settings": {}}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.to_lowercase().contains("unauthorized"), "{reason}");
}

// ─── workflow.publish (§4.5) ──────────────────────────────────────────────────

/// resolved — the workflow, now serving, its activeVersionId the version
/// that is published. Publishing an already published workflow re-publishes
/// it and settles the same way, which is what a redelivery meets.
#[tokio::test]
async fn workflow_publish_resolves_with_the_published_workflow() {
    let id = create_workflow(webhook_workflow(
        &unique("publish"),
        &unique_path("publish"),
        None,
    ))
    .await;
    let p = promise(
        &promise_id("wf-publish"),
        "workflow.publish",
        json!({"id": id, "name": "published by the plugin"}),
        in_ms(60_000),
    );

    let first = resolved(plugin::process(&config(), &p).await);
    let again = resolved(plugin::process(&config(), &p).await);

    assert_eq!(first["id"], id);
    assert_eq!(first["active"], true);
    assert!(first["activeVersionId"].is_string(), "{first}");
    assert_eq!(again["active"], true);
    assert_eq!(again["activeVersionId"], first["activeVersionId"]);

    delete_workflow(&id).await;
}

/// `not_found` — a workflow id that names nothing.
#[tokio::test]
async fn workflow_publish_rejects_not_found() {
    let p = promise(
        &promise_id("wf-publish-404"),
        "workflow.publish",
        json!({"id": "no_such_workflow_e0f1"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "not_found");
    assert!(!value["detail"].is_null(), "{value}");
}

/// `conflict` — a webhook path another published workflow already holds.
#[tokio::test]
async fn workflow_publish_rejects_conflict() {
    let held = unique_path("pub-held");
    let holder = create_workflow(webhook_workflow(&unique("publish-holder"), &held, None)).await;
    publish(&holder).await;
    let mine = create_workflow(webhook_workflow(&unique("publish-mine"), &held, None)).await;
    let p = promise(
        &promise_id("wf-publish-conflict"),
        "workflow.publish",
        json!({"id": mine}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "conflict");
    assert!(
        value["detail"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("conflict"),
        "{value}"
    );

    delete_workflow(&mine).await;
    delete_workflow(&holder).await;
}

/// `invalid_request` — the version has no trigger, webhook or polling node,
/// so there is nothing to register.
#[tokio::test]
async fn workflow_publish_rejects_invalid_request() {
    let id = create_workflow(json!({
        "name": unique("publish-bad"),
        "settings": {"executionOrder": "v1"},
        "nodes": [http_node("http://localhost:5678/webhook/never")],
        "connections": {},
    }))
    .await;
    let p = promise(
        &promise_id("wf-publish-bad"),
        "workflow.publish",
        json!({"id": id}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("trigger node"),
        "{value}"
    );

    delete_workflow(&id).await;
}

/// halt — the API key is rejected.
#[tokio::test]
async fn workflow_publish_halts_on_a_rejected_key() {
    let p = promise(
        &promise_id("wf-publish-halt"),
        "workflow.publish",
        json!({"id": "whatever"}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.to_lowercase().contains("unauthorized"), "{reason}");
}

// ─── workflow.unpublish (§4.6) ────────────────────────────────────────────────

/// resolved — the workflow, no longer serving. Unpublishing one that is not
/// published succeeds and changes nothing, which is what a redelivery meets.
#[tokio::test]
async fn workflow_unpublish_resolves_with_the_workflow() {
    let id = create_workflow(webhook_workflow(
        &unique("unpublish"),
        &unique_path("unpublish"),
        None,
    ))
    .await;
    publish(&id).await;
    let p = promise(
        &promise_id("wf-unpublish"),
        "workflow.unpublish",
        json!({"id": id}),
        in_ms(60_000),
    );

    let first = resolved(plugin::process(&config(), &p).await);
    let again = resolved(plugin::process(&config(), &p).await);

    assert_eq!(first["id"], id);
    assert_eq!(first["active"], false);
    assert_eq!(first["activeVersionId"], Value::Null);
    assert_eq!(again["active"], false);

    delete_workflow(&id).await;
}

/// `not_found` — a workflow id that names nothing.
#[tokio::test]
async fn workflow_unpublish_rejects_not_found() {
    let p = promise(
        &promise_id("wf-unpublish-404"),
        "workflow.unpublish",
        json!({"id": "no_such_workflow_e0f1"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "not_found");
    assert!(!value["detail"].is_null(), "{value}");
}

/// `invalid_request` — `id` is required by the Param schema.
#[tokio::test]
async fn workflow_unpublish_rejects_invalid_request() {
    let p = promise(
        &promise_id("wf-unpublish-bad"),
        "workflow.unpublish",
        json!({}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("id"),
        "{value}"
    );
}

/// halt — the API key is rejected.
#[tokio::test]
async fn workflow_unpublish_halts_on_a_rejected_key() {
    let p = promise(
        &promise_id("wf-unpublish-halt"),
        "workflow.unpublish",
        json!({"id": "whatever"}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.to_lowercase().contains("unauthorized"), "{reason}");
}

// ─── workflow.archive (§4.7) ──────────────────────────────────────────────────

/// resolved — the archived workflow. Archiving an archived workflow answers
/// 200 with it unchanged, so a redelivery settles the same way.
#[tokio::test]
async fn workflow_archive_resolves_with_the_archived_workflow() {
    let id = create_workflow(json!({
        "name": unique("archive"), "nodes": [], "connections": {},
        "settings": {"executionOrder": "v1"}
    }))
    .await;
    let p = promise(
        &promise_id("wf-archive"),
        "workflow.archive",
        json!({"id": id}),
        in_ms(60_000),
    );

    let first = resolved(plugin::process(&config(), &p).await);
    let again = resolved(plugin::process(&config(), &p).await);

    assert_eq!(first["id"], id);
    assert_eq!(first["isArchived"], true);
    assert_eq!(first["active"], false);
    assert_eq!(first["activeVersionId"], Value::Null);
    assert_eq!(again["isArchived"], true);

    delete_workflow(&id).await;
}

/// `not_found` — a workflow id that names nothing.
#[tokio::test]
async fn workflow_archive_rejects_not_found() {
    let p = promise(
        &promise_id("wf-archive-404"),
        "workflow.archive",
        json!({"id": "no_such_workflow_e0f1"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "not_found");
    assert_eq!(value["detail"], "Workflow not found");
}

/// `invalid_request` — the Param schema types `id` as a string.
#[tokio::test]
async fn workflow_archive_rejects_invalid_request() {
    let p = promise(
        &promise_id("wf-archive-bad"),
        "workflow.archive",
        json!({"id": 7}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("id"),
        "{value}"
    );
}

/// halt — the API key is rejected.
#[tokio::test]
async fn workflow_archive_halts_on_a_rejected_key() {
    let p = promise(
        &promise_id("wf-archive-halt"),
        "workflow.archive",
        json!({"id": "whatever"}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.to_lowercase().contains("unauthorized"), "{reason}");
}

// ─── workflow.unarchive (§4.8) ────────────────────────────────────────────────

/// resolved — the workflow, back and unpublished.
#[tokio::test]
async fn workflow_unarchive_resolves_with_the_workflow() {
    let id = create_workflow(json!({
        "name": unique("unarchive"), "nodes": [], "connections": {},
        "settings": {"executionOrder": "v1"}
    }))
    .await;
    archive(&id).await;
    let p = promise(
        &promise_id("wf-unarchive"),
        "workflow.unarchive",
        json!({"id": id}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["id"], id);
    assert_eq!(value["isArchived"], false);
    assert_eq!(value["active"], false);

    delete_workflow(&id).await;
}

/// Re-entry — n8n answers 400 "Workflow is not archived." to the second
/// call, and the stored workflow decides: not archived is the state this
/// call asks for, so the promise resolves rather than rejecting.
#[tokio::test]
async fn workflow_unarchive_resolves_on_re_entry() {
    let id = create_workflow(json!({
        "name": unique("unarchive-reentry"), "nodes": [], "connections": {},
        "settings": {"executionOrder": "v1"}
    }))
    .await;
    archive(&id).await;
    let p = promise(
        &promise_id("wf-unarchive-reentry"),
        "workflow.unarchive",
        json!({"id": id}),
        in_ms(60_000),
    );

    let first = resolved(plugin::process(&config(), &p).await);
    let again = resolved(plugin::process(&config(), &p).await);

    assert_eq!(first["isArchived"], false);
    // The second answer is the workflow as re-read, not the 400's body.
    assert_eq!(again["id"], id);
    assert_eq!(again["isArchived"], false);

    delete_workflow(&id).await;
}

/// `not_found` — a workflow id that names nothing.
#[tokio::test]
async fn workflow_unarchive_rejects_not_found() {
    let p = promise(
        &promise_id("wf-unarchive-404"),
        "workflow.unarchive",
        json!({"id": "no_such_workflow_e0f1"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "not_found");
    assert_eq!(value["detail"], "Workflow not found");
}

/// `invalid_request` — `id` is required by the Param schema.
#[tokio::test]
async fn workflow_unarchive_rejects_invalid_request() {
    let p = promise(
        &promise_id("wf-unarchive-bad"),
        "workflow.unarchive",
        json!({}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("id"),
        "{value}"
    );
}

/// halt — the API key is rejected.
#[tokio::test]
async fn workflow_unarchive_halts_on_a_rejected_key() {
    let p = promise(
        &promise_id("wf-unarchive-halt"),
        "workflow.unarchive",
        json!({"id": "whatever"}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.to_lowercase().contains("unauthorized"), "{reason}");
}

// ─── workflow.delete (§4.9) ───────────────────────────────────────────────────

/// resolved — the deleted workflow as it last stood. A redelivery finds
/// nothing and rejects not_found, which the second call shows.
#[tokio::test]
async fn workflow_delete_resolves_with_the_deleted_workflow() {
    let name = unique("delete");
    let id = create_workflow(json!({
        "name": name, "nodes": [], "connections": {}, "settings": {"executionOrder": "v1"}
    }))
    .await;
    let p = promise(
        &promise_id("wf-delete"),
        "workflow.delete",
        json!({"id": id}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);
    let again = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["id"], id);
    assert_eq!(value["name"], name);
    assert_eq!(again["code"], "not_found");
}

/// `not_found` — a workflow id that names nothing.
#[tokio::test]
async fn workflow_delete_rejects_not_found() {
    let p = promise(
        &promise_id("wf-delete-404"),
        "workflow.delete",
        json!({"id": "no_such_workflow_e0f1"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "not_found");
    assert_eq!(value["detail"], "Not Found");
}

/// `invalid_request` — `id` is required by the Param schema.
#[tokio::test]
async fn workflow_delete_rejects_invalid_request() {
    let p = promise(
        &promise_id("wf-delete-bad"),
        "workflow.delete",
        json!({"id": null}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("id"),
        "{value}"
    );
}

/// halt — the API key is rejected.
#[tokio::test]
async fn workflow_delete_halts_on_a_rejected_key() {
    let p = promise(
        &promise_id("wf-delete-halt"),
        "workflow.delete",
        json!({"id": "whatever"}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.to_lowercase().contains("unauthorized"), "{reason}");
}

// ─── execution.list (§4.10) ───────────────────────────────────────────────────

/// resolved — = response.body, one page.
#[tokio::test]
async fn execution_list_resolves_with_the_page() {
    let p = promise(
        &promise_id("ex-list"),
        "execution.list",
        json!({"status": "error", "includeData": false, "limit": 5}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    let data = value["data"].as_array().expect("data");
    assert!(!data.is_empty(), "§5 provisions two failed executions: {value}");
    for e in data {
        assert_eq!(e["status"], "error", "{e}");
        // includeData false: the run data is left out of every item.
        assert!(e.get("data").is_none(), "{e}");
    }
    assert!(value.get("nextCursor").is_some(), "{value}");
}

/// `invalid_request` — the Param schema caps `limit` at 250.
#[tokio::test]
async fn execution_list_rejects_invalid_request() {
    let p = promise(
        &promise_id("ex-list-bad"),
        "execution.list",
        json!({"limit": 999}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert_eq!(value["detail"], "request/query/limit must be <= 250");
}

/// halt — the API key is rejected.
#[tokio::test]
async fn execution_list_halts_on_a_rejected_key() {
    let p = promise(&promise_id("ex-list-halt"), "execution.list", json!({}), in_ms(60_000));

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.to_lowercase().contains("unauthorized"), "{reason}");
}

// ─── execution.get (§4.11) ────────────────────────────────────────────────────

/// resolved — = response.body, §5's failed execution.
#[tokio::test]
async fn execution_get_resolves_with_the_execution() {
    let p = promise(
        &promise_id("ex-get"),
        "execution.get",
        json!({"id": env("N8N_FIXTURE_FAIL"), "includeData": true}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["id"], env("N8N_FIXTURE_FAIL"));
    assert_eq!(value["status"], "error");
    assert_eq!(value["mode"], "webhook");
    // includeData true: the run data comes with it.
    assert!(value.get("data").is_some(), "{value}");
}

/// `not_found` — an execution id that names nothing.
#[tokio::test]
async fn execution_get_rejects_not_found() {
    let p = promise(
        &promise_id("ex-get-404"),
        "execution.get",
        json!({"id": 99_999_999}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "not_found");
    assert_eq!(value["detail"], "Not Found");
}

/// `invalid_request` — the Param schema documents the id as a decimal
/// integer.
#[tokio::test]
async fn execution_get_rejects_invalid_request() {
    let p = promise(
        &promise_id("ex-get-bad"),
        "execution.get",
        json!({"id": "not-a-number"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert_eq!(value["detail"], "request/params/id must be number");
}

/// halt — the API key is rejected.
#[tokio::test]
async fn execution_get_halts_on_a_rejected_key() {
    let p = promise(
        &promise_id("ex-get-halt"),
        "execution.get",
        json!({"id": 1}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.to_lowercase().contains("unauthorized"), "{reason}");
}

// ─── execution.retry (§4.12) ──────────────────────────────────────────────────

/// resolved — the succeeding work item §5 provisions: `N8N_FIXTURE_OK`
/// failed on a helper workflow that was not published yet, and its retry now
/// runs to "success". The resolved value is the retry's own record, not the
/// source's — the pending → terminal path this operation exists for.
#[tokio::test]
async fn execution_retry_resolves_when_the_retry_succeeds() {
    let source = env("N8N_FIXTURE_OK");
    let p = promise(
        &promise_id("ex-retry-ok"),
        "execution.retry",
        json!({"id": source}),
        in_ms(300_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value["status"], "success");
    assert_eq!(value["mode"], "retry");
    assert_eq!(value["finished"], true);
    // = the source execution, whichever spelling this response used for it.
    assert_eq!(id_of(&value["retryOf"]), source);
    assert_ne!(id_of(&value["id"]), source);
    assert!(value["workflowId"].is_string(), "{value}");
    assert!(value["startedAt"].is_string(), "{value}");
    // The Resolved schema names ten keys: a response carries those of them it
    // has, and the mapping adds nothing else.
    for key in value.as_object().expect("object").keys() {
        assert!(
            RETRY_RESOLVED_KEYS.contains(&key.as_str()),
            "{key} is not in the Resolved schema: {value}"
        );
    }
}

const RETRY_RESOLVED_KEYS: [&str; 10] = [
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

/// `execution_failed` — the failing work item §5 provisions: the target of
/// `N8N_FIXTURE_FAIL`'s HTTP Request node is never registered, so its retry
/// fails again.
#[tokio::test]
async fn execution_retry_rejects_execution_failed() {
    let source = env("N8N_FIXTURE_FAIL");
    let p = promise(
        &promise_id("ex-retry-fail"),
        "execution.retry",
        json!({"id": source}),
        in_ms(300_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "execution_failed");
    // detail = the terminal execution.
    assert_eq!(value["detail"]["status"], "error");
    assert_eq!(value["detail"]["mode"], "retry");
    assert_eq!(id_of(&value["detail"]["retryOf"]), source);
}

/// Re-entry — a redelivery finds the retry an earlier delivery started by
/// its retryOf and observes it, rather than starting a second one.
#[tokio::test]
async fn execution_retry_reattaches_on_re_entry() {
    let (workflow, source) = failed_execution("retry-reentry").await;
    let p = promise(
        &promise_id("ex-retry-reentry"),
        "execution.retry",
        json!({"id": source}),
        in_ms(300_000),
    );

    let first = rejected(plugin::process(&config(), &p).await);
    let second = rejected(plugin::process(&config(), &p).await);

    assert_eq!(first["code"], "execution_failed");
    assert_eq!(second["code"], "execution_failed");
    // One promise, one retry.
    assert_eq!(id_of(&first["detail"]["id"]), id_of(&second["detail"]["id"]));
    assert_eq!(retries_of(&workflow, &source).await.len(), 1);

    delete_workflow(&workflow).await;
}

/// `not_found` — an execution id that names nothing.
#[tokio::test]
async fn execution_retry_rejects_not_found() {
    let p = promise(
        &promise_id("ex-retry-404"),
        "execution.retry",
        json!({"id": 99_999_999}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "not_found");
    assert_eq!(value["detail"], "Not Found");
}

/// `not_retryable` — the source execution already succeeded.
#[tokio::test]
async fn execution_retry_rejects_not_retryable() {
    let (workflow, source) = succeeded_execution("retry-notretryable").await;
    let p = promise(
        &promise_id("ex-retry-notretryable"),
        "execution.retry",
        json!({"id": source}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "not_retryable");
    assert_eq!(
        value["detail"],
        "The execution succeeded, so it cannot be retried."
    );

    delete_workflow(&workflow).await;
}

/// `workflow_changed` — loadWorkflow asks for the workflow's current
/// definition, and it no longer holds the node the stopped execution was
/// standing on. n8n answers 500, and every redelivery meets the same one.
#[tokio::test]
async fn execution_retry_rejects_workflow_changed() {
    let (workflow, source) = failed_execution("retry-changed").await;
    // Take the node the execution failed on out of the definition.
    let stored = get_workflow(&workflow).await;
    put_workflow(
        &workflow,
        json!({
            "name": stored["name"],
            "settings": stored["settings"],
            "nodes": [stored["nodes"][0]],
            "connections": {},
        }),
    )
    .await;
    let p = promise(
        &promise_id("ex-retry-changed"),
        "execution.retry",
        json!({"id": source, "loadWorkflow": true}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "workflow_changed");
    // detail = response.body.message of the 500.
    assert_eq!(value["detail"], "Internal server error");

    delete_workflow(&workflow).await;
}

/// `invalid_request` — the Param schema documents the id as a decimal
/// integer.
#[tokio::test]
async fn execution_retry_rejects_invalid_request() {
    let p = promise(
        &promise_id("ex-retry-bad"),
        "execution.retry",
        json!({"id": "not-a-number"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert_eq!(value["detail"], "request/params/id must be number");
}

/// `deleted` — the retry is removed while the poll loop is watching it. Its
/// workflow parks it at a Wait node, so it stays observable long enough for
/// the deletion to land.
#[tokio::test]
async fn execution_retry_rejects_deleted() {
    let fixture = slow_retry_source("retry-deleted").await;
    let p = promise(
        &promise_id("ex-retry-deleted"),
        "execution.retry",
        json!({"id": fixture.execution}),
        in_ms(120_000),
    );

    let deleter = async {
        let retry = await_retry(&fixture.workflow, &fixture.execution).await;
        assert_eq!(delete_execution(&retry).await, 200, "deleting the retry");
    };
    let config = config();
    let (verdict, ()) = tokio::join!(plugin::process(&config, &p), deleter);
    let value = rejected(verdict);

    assert_eq!(value["code"], "deleted");
    // detail: absent.
    assert_eq!(value.as_object().expect("object").len(), 1, "{value}");

    fixture.clean_up().await;
}

/// `execution_canceled` — the retry is stopped while the poll loop is
/// watching it.
#[tokio::test]
async fn execution_retry_rejects_execution_canceled() {
    let fixture = slow_retry_source("retry-canceled").await;
    let p = promise(
        &promise_id("ex-retry-canceled"),
        "execution.retry",
        json!({"id": fixture.execution}),
        in_ms(120_000),
    );

    let stopper = async {
        let retry = await_retry(&fixture.workflow, &fixture.execution).await;
        assert_eq!(stop_execution(&retry).await, 200, "stopping the retry");
    };
    let config = config();
    let (verdict, ()) = tokio::join!(plugin::process(&config, &p), stopper);
    let value = rejected(verdict);

    assert_eq!(value["code"], "execution_canceled");
    assert_eq!(value["detail"]["status"], "canceled");

    fixture.clean_up().await;
}

/// The deadline — `timeout_at` already in the past. No verdict: the server
/// settles a timed-out promise itself.
#[tokio::test]
async fn execution_retry_releases_at_the_deadline() {
    let (workflow, source) = failed_execution("retry-deadline").await;
    let p = promise(
        &promise_id("ex-retry-deadline"),
        "execution.retry",
        json!({"id": source}),
        in_ms(-1_000),
    );

    let reason = released(plugin::process(&config(), &p).await);

    assert_eq!(reason, "promise timed out");

    delete_workflow(&workflow).await;
}

/// halt — the API key is rejected.
#[tokio::test]
async fn execution_retry_halts_on_a_rejected_key() {
    let p = promise(
        &promise_id("ex-retry-halt"),
        "execution.retry",
        json!({"id": env("N8N_FIXTURE_FAIL")}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.to_lowercase().contains("unauthorized"), "{reason}");
}

// ─── execution.stop (§4.13) ───────────────────────────────────────────────────

/// resolved — the execution's state after the stop. A second call meets the
/// 500 n8n answers for an execution that can no longer be stopped, and the
/// execution's own status — canceled — settles the promise the same way,
/// which is what a redelivery meets.
#[tokio::test]
async fn execution_stop_resolves_with_the_stopped_execution() {
    let (workflow, execution) = running_execution("stop").await;
    let p = promise(
        &promise_id("ex-stop"),
        "execution.stop",
        json!({"id": execution}),
        in_ms(60_000),
    );

    let first = resolved(plugin::process(&config(), &p).await);
    let again = resolved(plugin::process(&config(), &p).await);

    assert_eq!(first["status"], "canceled");
    assert_eq!(first["finished"], false);
    assert_eq!(first["mode"], "webhook");
    assert!(first["startedAt"].is_string(), "{first}");
    // The Resolved schema is these five keys, those of them the response has.
    for key in first.as_object().expect("object").keys() {
        assert!(
            ["mode", "status", "finished", "startedAt", "stoppedAt"].contains(&key.as_str()),
            "{key} is not in the Resolved schema: {first}"
        );
    }
    assert_eq!(again["status"], "canceled");

    delete_workflow(&workflow).await;
}

/// `not_found` — an execution id that names nothing.
#[tokio::test]
async fn execution_stop_rejects_not_found() {
    let p = promise(
        &promise_id("ex-stop-404"),
        "execution.stop",
        json!({"id": 99_999_999}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "not_found");
    assert_eq!(value["detail"], "Failed to find execution to stop");
}

/// `not_stoppable` — the execution had already reached a terminal state the
/// stop cannot touch: §5's failed fixture.
#[tokio::test]
async fn execution_stop_rejects_not_stoppable() {
    let p = promise(
        &promise_id("ex-stop-terminal"),
        "execution.stop",
        json!({"id": env("N8N_FIXTURE_FAIL")}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "not_stoppable");
    // detail = the execution as re-read, whose status is the terminal state
    // it had already reached.
    assert_eq!(value["detail"]["status"], "error");
    assert_eq!(id_of(&value["detail"]["id"]), env("N8N_FIXTURE_FAIL"));
}

/// `invalid_request` — the Param schema documents the id as a decimal
/// integer.
#[tokio::test]
async fn execution_stop_rejects_invalid_request() {
    let p = promise(
        &promise_id("ex-stop-bad"),
        "execution.stop",
        json!({"id": "not-a-number"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert_eq!(value["detail"], "request/params/id must be number");
}

/// halt — the API key is rejected.
#[tokio::test]
async fn execution_stop_halts_on_a_rejected_key() {
    let p = promise(
        &promise_id("ex-stop-halt"),
        "execution.stop",
        json!({"id": 1}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.to_lowercase().contains("unauthorized"), "{reason}");
}

// ─── execution.delete (§4.14) ─────────────────────────────────────────────────

/// resolved — the deleted execution as it last stood. The record is
/// hard-deleted, so a redelivery finds nothing and rejects not_found.
#[tokio::test]
async fn execution_delete_resolves_with_the_deleted_execution() {
    let (workflow, execution) = failed_execution("delete").await;
    let p = promise(
        &promise_id("ex-delete"),
        "execution.delete",
        json!({"id": execution}),
        in_ms(60_000),
    );

    let value = resolved(plugin::process(&config(), &p).await);
    let again = rejected(plugin::process(&config(), &p).await);

    assert_eq!(id_of(&value["id"]), execution);
    assert_eq!(value["status"], "error");
    assert_eq!(again["code"], "not_found");

    delete_workflow(&workflow).await;
}

/// `not_found` — an execution id that names nothing.
#[tokio::test]
async fn execution_delete_rejects_not_found() {
    let p = promise(
        &promise_id("ex-delete-404"),
        "execution.delete",
        json!({"id": 99_999_999}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "not_found");
    assert_eq!(value["detail"], "Not Found");
}

/// `not_deletable` — the execution is still running.
#[tokio::test]
async fn execution_delete_rejects_not_deletable() {
    let (workflow, execution) = running_execution("delete-running").await;
    let p = promise(
        &promise_id("ex-delete-running"),
        "execution.delete",
        json!({"id": execution}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "not_deletable");
    assert_eq!(value["detail"], "Cannot delete a running execution");

    stop_execution(&execution).await;
    delete_workflow(&workflow).await;
}

/// `invalid_request` — the Param schema documents the id as a decimal
/// integer. n8n answers it the same 400 it answers a running execution with,
/// so this is also the test that the two are told apart by the execution's
/// own status.
#[tokio::test]
async fn execution_delete_rejects_invalid_request() {
    let p = promise(
        &promise_id("ex-delete-bad"),
        "execution.delete",
        json!({"id": "not-a-number"}),
        in_ms(60_000),
    );

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert_eq!(value["detail"], "request/params/id must be number");
}

/// halt — the API key is rejected.
#[tokio::test]
async fn execution_delete_halts_on_a_rejected_key() {
    let p = promise(
        &promise_id("ex-delete-halt"),
        "execution.delete",
        json!({"id": 1}),
        in_ms(60_000),
    );

    let reason = halted(plugin::process(&bad_credential(), &p).await);

    assert!(reason.to_lowercase().contains("unauthorized"), "{reason}");
}

// ─── Dispatch ─────────────────────────────────────────────────────────────────

/// A func no operation serves is a permanent rejection naming it.
#[tokio::test]
async fn an_unknown_func_is_rejected() {
    let p = promise(&promise_id("unknown"), "workflow.explode", json!({}), in_ms(60_000));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "unknown_func", "detail": "workflow.explode"}));
}

/// A param that is not a `{func, args}` object cannot name an operation.
#[tokio::test]
async fn a_param_without_a_func_is_rejected() {
    let mut p = promise(&promise_id("nofunc"), "workflow.get", json!({}), in_ms(60_000));
    p.param.data = Some(base64::engine::general_purpose::STANDARD.encode("{}"));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert_eq!(value["detail"], "param has no func");
}

// ─── Fixtures ─────────────────────────────────────────────────────────────────

/// §2, built from the environment §5.2 exports. `poll` is overridden to 1s:
/// the default 5s would make every pending → terminal test wait a full
/// interval past the retry's own end.
fn config() -> Config {
    Config {
        base_url: env("N8N_BASE_URL"),
        api_key: env("N8N_API_KEY"),
        poll: Duration::from_secs(1),
    }
}

/// The same n8n, with an API key it will reject.
fn bad_credential() -> Config {
    Config {
        api_key: format!("{}-wrong", env("N8N_API_KEY")),
        ..config()
    }
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

/// A fresh promise id per test.
fn promise_id(what: &str) -> String {
    format!("n8n.{what}.{}", nanos())
}

/// A workflow name no other run of this suite holds.
fn unique(what: &str) -> String {
    format!("plugin-{what}-{}", nanos())
}

/// A webhook path no other run holds: n8n refuses to publish two workflows
/// onto the same one.
fn unique_path(what: &str) -> String {
    format!("t-{what}-{}", nanos())
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

/// An execution id, whichever way the response spelled it: `retryOf` is a
/// number in a retry response and a string in a listing, and `id` is a
/// string everywhere but the delete response.
fn id_of(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
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

fn url(path: &str) -> String {
    format!("{}/api/v1{path}", env("N8N_BASE_URL"))
}

async fn call(req: reqwest::RequestBuilder) -> (u16, Value) {
    let response = req
        .header("X-N8N-API-KEY", env("N8N_API_KEY"))
        .send()
        .await
        .expect("provisioning request");
    let status = response.status().as_u16();
    let text = response.text().await.expect("provisioning response");
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

/// A workflow node id: unique per run, in the shape n8n stores.
fn node_id(tag: u16) -> String {
    let n = nanos() as u64;
    format!(
        "{:08x}-{:04x}-4{:03x}-8{:03x}-{:012x}",
        (n >> 32) as u32,
        tag,
        (n >> 20) & 0xfff,
        (n >> 8) & 0xfff,
        n & 0xffff_ffff_ffff
    )
}

/// `method` is what the caller of this webhook uses: the tests fire their own
/// triggers with POST, and an HTTP Request node calling one calls it with GET.
fn webhook_node(path: &str, method: &str) -> Value {
    json!({
        "id": node_id(0x1a),
        "name": "Webhook",
        "type": "n8n-nodes-base.webhook",
        "typeVersion": 2,
        "position": [0, 0],
        "webhookId": node_id(0x1b),
        "parameters": {"httpMethod": method, "path": path, "options": {}},
    })
}

fn http_node(target: &str) -> Value {
    json!({
        "id": node_id(0x2a),
        "name": "Call",
        "type": "n8n-nodes-base.httpRequest",
        "typeVersion": 4.2,
        "position": [220, 0],
        "parameters": {"url": target, "options": {}},
    })
}

fn wait_node(seconds: u32) -> Value {
    json!({
        "id": node_id(0x3a),
        "name": "Wait",
        "type": "n8n-nodes-base.wait",
        "typeVersion": 1.1,
        "position": [440, 0],
        "parameters": {"amount": seconds, "unit": "seconds"},
    })
}

/// A webhook trigger, optionally feeding one HTTP Request node whose target
/// decides the run's outcome.
fn webhook_workflow(name: &str, path: &str, target: Option<&str>) -> Value {
    webhook_workflow_method(name, path, "POST", target)
}

fn webhook_workflow_method(name: &str, path: &str, method: &str, target: Option<&str>) -> Value {
    let mut nodes = vec![webhook_node(path, method)];
    let mut connections = json!({});
    if let Some(target) = target {
        nodes.push(http_node(target));
        connections = json!({"Webhook": {"main": [[{"node": "Call", "type": "main", "index": 0}]]}});
    }
    json!({
        "name": name,
        "settings": {"executionOrder": "v1"},
        "nodes": nodes,
        "connections": connections,
    })
}

async fn create_workflow(body: Value) -> String {
    let (status, workflow) = call(http().post(url("/workflows")).json(&body)).await;
    assert_eq!(status, 200, "creating a workflow: {workflow}");
    workflow["id"].as_str().expect("workflow id").to_string()
}

async fn get_workflow(id: &str) -> Value {
    let (status, workflow) = call(http().get(url(&format!("/workflows/{id}")))).await;
    assert_eq!(status, 200, "reading workflow {id}: {workflow}");
    workflow
}

async fn put_workflow(id: &str, body: Value) {
    let (status, workflow) = call(http().put(url(&format!("/workflows/{id}"))).json(&body)).await;
    assert_eq!(status, 200, "updating workflow {id}: {workflow}");
}

async fn publish(id: &str) {
    let (status, workflow) = call(
        http()
            .post(url(&format!("/workflows/{id}/publish")))
            .json(&json!({})),
    )
    .await;
    assert_eq!(status, 200, "publishing workflow {id}: {workflow}");
}

async fn archive(id: &str) {
    let (status, workflow) = call(http().post(url(&format!("/workflows/{id}/archive")))).await;
    assert_eq!(status, 200, "archiving workflow {id}: {workflow}");
}

/// Deleting a workflow takes its executions with it, so this is the only
/// clean-up a test needs.
async fn delete_workflow(id: &str) {
    call(http().delete(url(&format!("/workflows/{id}")))).await;
}

async fn delete_execution(id: &str) -> u16 {
    call(http().delete(url(&format!("/executions/{id}")))).await.0
}

async fn stop_execution(id: &str) -> u16 {
    call(http().post(url(&format!("/executions/{id}/stop")))).await.0
}

/// Fire a published workflow's webhook.
async fn fire(path: &str) {
    http()
        .post(format!("{}/webhook/{path}", env("N8N_BASE_URL")))
        .json(&json!({}))
        .send()
        .await
        .expect("firing the webhook");
}

async fn executions(query: &[(&str, &str)]) -> Vec<Value> {
    let (status, page) = call(http().get(url("/executions")).query(query)).await;
    assert_eq!(status, 200, "listing executions: {page}");
    page["data"].as_array().cloned().unwrap_or_default()
}

/// Wait for a workflow's run to reach a status, and return its execution id.
async fn await_execution(workflow: &str, status: &str) -> String {
    for _ in 0..120 {
        let page = executions(&[("workflowId", workflow), ("status", status)]).await;
        if let Some(e) = page.first() {
            return id_of(&e["id"]);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    panic!("no {status} execution of {workflow} appeared");
}

/// The retries of one execution — a retry carries retryOf = its source.
async fn retries_of(workflow: &str, source: &str) -> Vec<String> {
    let mut out = Vec::new();
    for status in ["running", "waiting", "success", "error", "canceled", "crashed"] {
        for e in executions(&[("workflowId", workflow), ("status", status)]).await {
            if id_of(&e["retryOf"]) == source {
                out.push(id_of(&e["id"]));
            }
        }
    }
    out
}

/// Wait for the retry the plugin started to exist, and return its id.
async fn await_retry(workflow: &str, source: &str) -> String {
    for _ in 0..240 {
        if let Some(id) = retries_of(workflow, source).await.pop() {
            return id;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    panic!("no retry of {source} on {workflow} appeared");
}

/// A published workflow of its own, fired once, whose run failed: its HTTP
/// Request node points at a webhook path nothing registers.
async fn failed_execution(what: &str) -> (String, String) {
    let path = unique_path(what);
    let target = format!("{}/webhook/{}", env("N8N_BASE_URL"), unique_path("never"));
    let workflow =
        create_workflow(webhook_workflow(&unique(what), &path, Some(&target))).await;
    publish(&workflow).await;
    fire(&path).await;
    let execution = await_execution(&workflow, "error").await;
    (workflow, execution)
}

/// A published workflow of its own, fired once, whose run succeeded: a
/// webhook trigger with nothing after it to fail.
async fn succeeded_execution(what: &str) -> (String, String) {
    let path = unique_path(what);
    let workflow = create_workflow(webhook_workflow(&unique(what), &path, None)).await;
    publish(&workflow).await;
    fire(&path).await;
    let execution = await_execution(&workflow, "success").await;
    (workflow, execution)
}

/// A published workflow of its own, fired once, whose run is still going: a
/// Wait node short enough to stay in the instance's memory holds the
/// execution in status "running" for its length.
async fn running_execution(what: &str) -> (String, String) {
    let path = unique_path(what);
    let mut definition = webhook_workflow(&unique(what), &path, None);
    definition["nodes"]
        .as_array_mut()
        .expect("nodes")
        .push(wait_node(60));
    definition["connections"] =
        json!({"Webhook": {"main": [[{"node": "Wait", "type": "main", "index": 0}]]}});
    let workflow = create_workflow(definition).await;
    publish(&workflow).await;
    fire(&path).await;
    let execution = await_execution(&workflow, "running").await;
    (workflow, execution)
}

/// A failed execution whose retry does not finish while the plugin watches
/// it: the node it failed on — a call to a helper workflow that was not
/// published yet — succeeds the second time and hands on to a Wait node long
/// enough to hold the retry in status "waiting".
struct SlowRetry {
    workflow: String,
    helper: String,
    execution: String,
}

impl SlowRetry {
    async fn clean_up(&self) {
        delete_workflow(&self.workflow).await;
        delete_workflow(&self.helper).await;
    }
}

async fn slow_retry_source(what: &str) -> SlowRetry {
    let helper_path = unique_path(&format!("{what}-helper"));
    // The HTTP Request node that calls it uses GET, so the webhook that
    // answers has to be registered for GET.
    let helper = create_workflow(webhook_workflow_method(
        &unique(&format!("{what}-helper")),
        &helper_path,
        "GET",
        None,
    ))
    .await;

    let path = unique_path(what);
    let target = format!("{}/webhook/{helper_path}", env("N8N_BASE_URL"));
    let mut definition = webhook_workflow(&unique(what), &path, Some(&target));
    // A wait past the length n8n holds a run in memory for: the execution is
    // put away in status "waiting" instead.
    definition["nodes"]
        .as_array_mut()
        .expect("nodes")
        .push(wait_node(600));
    definition["connections"] = json!({
        "Webhook": {"main": [[{"node": "Call", "type": "main", "index": 0}]]},
        "Call": {"main": [[{"node": "Wait", "type": "main", "index": 0}]]},
    });
    let workflow = create_workflow(definition).await;
    publish(&workflow).await;

    // The helper is not published yet, so the call fails and the run with it.
    fire(&path).await;
    let execution = await_execution(&workflow, "error").await;
    publish(&helper).await;

    SlowRetry { workflow, helper, execution }
}
