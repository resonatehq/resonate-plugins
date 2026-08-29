//! `plugin::process` against a local mock standing where Zendesk would be.
//!
//! The specification's Notes say `Self-hosted: no` — Zendesk is SaaS only,
//! there is no §5 to provision, and the tests cannot reach a real account. So
//! every condition is induced against a `wiremock` server whose responses are
//! built from the §4.N.4 Integration Response schemas, and every outgoing
//! request is additionally asserted against the §4.N.3 Integration Request
//! schema: method, path, the injected `sanitize(promise.id)`, and the body
//! mapping.
//!
//! The circularity is worth stating plainly: the mocks are built from the same
//! specification as the code, so these tests can only show that the code says
//! what the specification says. They cannot show that the specification is
//! true of Zendesk. Only review against the live provider breaks the loop.
//!
//! One mock server serves the whole binary — the plugin reads its API root
//! from the environment once — so the tests are serial (`.cargo/config.toml`),
//! and each one resets the server before mounting its own stubs.

use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use resonate_core::types::{PromiseRecord, PromiseState, PromiseValue};
use resonate_plugin_zendesk::plugin::{self, Config};

/// The verdict `process` returns, spelled out: the crate's own alias for it
/// is an implementation detail.
type Verdict = Result<Result<String, String>, Result<String, String>>;

// ─── ticket.create (§4.1) ─────────────────────────────────────────────────────

/// resolved — the succeeding work item. The external_id lookup answers empty,
/// so the ticket is created, and the 4.1.2 Resolved value is the created
/// ticket record.
#[tokio::test]
async fn ticket_create_resolves_with_the_created_ticket() {
    let server = mock().await;
    let created = ticket(101, "new");
    lookup_stub(server, json!({"tickets": []})).await;
    Mock::given(method("POST"))
        .and(path("/api/v2/tickets"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"ticket": created})))
        .mount(server)
        .await;

    let id = promise_id("create-ok");
    let p = promise(&id, "ticket.create", create_args(), in_ms(60_000));
    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value, created);

    // The 4.1.3 Integration Request, preceded by the external_id lookup the
    // §4.1.5 Python opens with.
    let sent = requests(server).await;
    assert_eq!(paths(&sent).len(), 2, "{:?}", paths(&sent));
    let lookup = &sent[0];
    assert_eq!(lookup.method.as_str(), "GET");
    assert_eq!(lookup.url.path(), "/api/v2/tickets");
    assert_eq!(basic(lookup), expected_auth());
    let looked_up = query(lookup);
    assert_eq!(looked_up.len(), 1, "{looked_up:?}");
    assert_eq!(looked_up[0].0, "external_id");

    let post = &sent[1];
    assert_eq!(post.method.as_str(), "POST");
    assert_eq!(post.url.path(), "/api/v2/tickets");
    assert_eq!(basic(post), expected_auth());
    let body: Value = post.body_json().expect("the create body is JSON");
    assert_eq!(body["ticket"]["subject"], create_args()["subject"]);
    assert_eq!(body["ticket"]["comment"], create_args()["comment"]);
    assert_eq!(body["ticket"]["tags"], create_args()["tags"]);
    // external_id and the Idempotency-Key are both sanitize(promise.id),
    // never the raw id, and the lookup asked for that same value.
    let external_id = body["ticket"]["external_id"].as_str().expect("external_id is a string");
    assert!(external_id.starts_with(&id), "{external_id} should be derived from {id}");
    assert_ne!(external_id, id, "external_id should carry sanitize's digest");
    assert_eq!(header(post, "idempotency-key"), external_id);
    assert_eq!(looked_up[0].1, external_id);
}

/// recovery / re-entry — the same promise id twice. The second delivery finds
/// the ticket this promise already created by its external_id and resolves
/// with it rather than creating a second one.
#[tokio::test]
async fn ticket_create_recovers_the_ticket_by_external_id_on_re_entry() {
    let server = mock().await;
    let created = ticket(202, "new");
    // First delivery: nothing carries the external_id yet. Second: it does.
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"tickets": []})))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"tickets": [created.clone()]})),
        )
        .with_priority(2)
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v2/tickets"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"ticket": created})))
        .mount(server)
        .await;

    let p = promise(&promise_id("create-reentry"), "ticket.create", create_args(), in_ms(60_000));
    let first = resolved(plugin::process(&config(), &p).await);
    let second = resolved(plugin::process(&config(), &p).await);

    assert_eq!(first, created);
    assert_eq!(second, created);
    // Three requests, not four: the second delivery never reached the POST.
    let sent = requests(server).await;
    assert_eq!(
        sent.iter().map(|r| r.method.as_str().to_string()).collect::<Vec<_>>(),
        vec!["GET".to_string(), "POST".to_string(), "GET".to_string()]
    );
}

/// `invalid_request` — the create fails validation. detail = the 4xx body as
/// text.
#[tokio::test]
async fn ticket_create_rejects_invalid_request_from_the_provider() {
    for status in [400, 422] {
        let server = mock().await;
        let detail = r#"{"error":"RecordInvalid","description":"Record validation errors"}"#;
        lookup_stub(server, json!({"tickets": []})).await;
        Mock::given(method("POST"))
            .and(path("/api/v2/tickets"))
            .respond_with(ResponseTemplate::new(status).set_body_string(detail))
            .mount(server)
            .await;

        let p = promise(&promise_id("create-invalid"), "ticket.create", create_args(), in_ms(60_000));
        let value = rejected(plugin::process(&config(), &p).await);

        assert_eq!(value, json!({"code": "invalid_request", "detail": detail}), "{status}");
    }
}

/// `invalid_request` — the external_id lookup itself answers 4xx, which the
/// §4.1.5 Python reports the same way.
#[tokio::test]
async fn ticket_create_rejects_invalid_request_from_the_lookup() {
    let server = mock().await;
    let detail = r#"{"error":"InvalidEndpoint","description":"Not found"}"#;
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets"))
        .respond_with(ResponseTemplate::new(400).set_body_string(detail))
        .mount(server)
        .await;

    let p = promise(&promise_id("create-lookup"), "ticket.create", create_args(), in_ms(60_000));
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "invalid_request", "detail": detail}));
    assert_eq!(requests(server).await.len(), 1, "the POST should never go out");
}

/// `invalid_request` — a param that violates the 4.1.1 Param schema before any
/// request goes out. The param is immutable, so no redelivery could fix it.
#[tokio::test]
async fn ticket_create_rejects_invalid_request_locally() {
    let server = mock().await;

    // `comment` is required.
    let p = promise(
        &promise_id("create-nocomment"),
        "ticket.create",
        json!({"subject": "Printer on fire"}),
        in_ms(60_000),
    );
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(value["detail"].as_str().unwrap_or_default().contains("comment"), "{value}");
    assert!(requests(server).await.is_empty(), "nothing should have been sent");
}

/// halt — the credential is corrupt: 401 "Couldn't authenticate you". No
/// retry of ours clears it, so an operator must act.
#[tokio::test]
async fn ticket_create_halts_on_a_rejected_credential() {
    let server = mock().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets"))
        .respond_with(
            ResponseTemplate::new(401).set_body_string(r#"{"error":"Couldn't authenticate you"}"#),
        )
        .mount(server)
        .await;

    let p = promise(&promise_id("create-halt"), "ticket.create", create_args(), in_ms(60_000));
    let reason = halted(plugin::process(&corrupt_config(), &p).await);

    assert!(reason.contains("authenticate"), "{reason}");
}

// ─── ticket.get (§4.2) ────────────────────────────────────────────────────────

/// resolved — the 4.2.2 Resolved value is `response.body.ticket`, not the
/// whole envelope.
#[tokio::test]
async fn ticket_get_resolves_with_the_ticket() {
    let server = mock().await;
    let record = ticket(101, "open");
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets/101"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ticket": record})))
        .mount(server)
        .await;

    let p = promise(
        &promise_id("get-ok"),
        "ticket.get",
        json!({"ticket_id": 101, "include": "users"}),
        in_ms(60_000),
    );
    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value, record);
    // The 4.2.3 Integration Request: the id in the path, `include` in the query.
    let sent = requests(server).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].method.as_str(), "GET");
    assert_eq!(sent[0].url.path(), "/api/v2/tickets/101");
    assert_eq!(query(&sent[0]), vec![("include".to_string(), "users".to_string())]);
    assert_eq!(basic(&sent[0]), expected_auth());
}

/// `not_found` — a nonexistent id, which is also the answer for a deleted
/// ticket. detail = the 404 body as text.
#[tokio::test]
async fn ticket_get_rejects_not_found() {
    let server = mock().await;
    let detail = r#"{"error":"RecordNotFound","description":"Not found"}"#;
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets/999999"))
        .respond_with(ResponseTemplate::new(404).set_body_string(detail))
        .mount(server)
        .await;

    let p = promise(&promise_id("get-404"), "ticket.get", json!({"ticket_id": 999999}), in_ms(60_000));
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "not_found", "detail": detail}));
}

/// `invalid_request` — an unknown sideload name in `include`.
#[tokio::test]
async fn ticket_get_rejects_invalid_request() {
    let server = mock().await;
    let detail = r#"{"error":"InvalidSideload","description":"unknown sideload: nope"}"#;
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets/101"))
        .respond_with(ResponseTemplate::new(400).set_body_string(detail))
        .mount(server)
        .await;

    let p = promise(
        &promise_id("get-invalid"),
        "ticket.get",
        json!({"ticket_id": 101, "include": "nope"}),
        in_ms(60_000),
    );
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "invalid_request", "detail": detail}));
}

/// `invalid_request` — `ticket_id` is required by the 4.2.1 Param schema, and
/// it is an integer.
#[tokio::test]
async fn ticket_get_rejects_invalid_request_locally() {
    let server = mock().await;

    let p = promise(&promise_id("get-noid"), "ticket.get", json!({"include": "users"}), in_ms(60_000));
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(value["detail"].as_str().unwrap_or_default().contains("ticket_id"), "{value}");
    assert!(requests(server).await.is_empty(), "nothing should have been sent");
}

/// halt — 403: the agent's role, or the token's scope, does not permit the
/// read.
#[tokio::test]
async fn ticket_get_halts_on_a_rejected_credential() {
    let server = mock().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets/101"))
        .respond_with(ResponseTemplate::new(403).set_body_string(r#"{"error":"Forbidden"}"#))
        .mount(server)
        .await;

    let p = promise(&promise_id("get-halt"), "ticket.get", json!({"ticket_id": 101}), in_ms(60_000));
    let reason = halted(plugin::process(&corrupt_config(), &p).await);

    assert!(reason.contains("Forbidden"), "{reason}");
}

// ─── ticket.list (§4.3) ───────────────────────────────────────────────────────

/// resolved — the 4.3.2 Resolved value is the whole body: the page and its
/// cursors. Pagination is the caller's loop, one promise per page.
#[tokio::test]
async fn ticket_list_resolves_with_the_page() {
    let server = mock().await;
    let page = json!({
        "tickets": [ticket(101, "open"), ticket(102, "pending")],
        "meta": {"has_more": true, "after_cursor": "c2", "before_cursor": "c1"},
        "links": {"next": "https://acme.zendesk.com/api/v2/tickets?page[after]=c2", "prev": null},
    });
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page.clone()))
        .mount(server)
        .await;

    let p = promise(
        &promise_id("list-ok"),
        "ticket.list",
        json!({"external_id": "abc", "page[size]": 2, "page[after]": "c1"}),
        in_ms(60_000),
    );
    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value, page);
    // The 4.3.3 Integration Request: every named argument becomes a query
    // parameter, an integer rendered as its digits.
    let sent = requests(server).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].method.as_str(), "GET");
    assert_eq!(sent[0].url.path(), "/api/v2/tickets");
    assert_eq!(
        query(&sent[0]),
        vec![
            ("external_id".to_string(), "abc".to_string()),
            ("page[size]".to_string(), "2".to_string()),
            ("page[after]".to_string(), "c1".to_string()),
        ]
    );
}

/// `invalid_request` — a malformed or expired cursor.
#[tokio::test]
async fn ticket_list_rejects_invalid_request() {
    let server = mock().await;
    let detail = r#"{"error":"InvalidPaginationParameter","description":"invalid cursor"}"#;
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets"))
        .respond_with(ResponseTemplate::new(400).set_body_string(detail))
        .mount(server)
        .await;

    let p = promise(
        &promise_id("list-invalid"),
        "ticket.list",
        json!({"page[size]": 2, "page[after]": "not-a-cursor"}),
        in_ms(60_000),
    );
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "invalid_request", "detail": detail}));
}

/// halt — the credential is corrupt.
#[tokio::test]
async fn ticket_list_halts_on_a_rejected_credential() {
    let server = mock().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets"))
        .respond_with(
            ResponseTemplate::new(401).set_body_string(r#"{"error":"Couldn't authenticate you"}"#),
        )
        .mount(server)
        .await;

    let p = promise(&promise_id("list-halt"), "ticket.list", json!({}), in_ms(60_000));
    let reason = halted(plugin::process(&corrupt_config(), &p).await);

    assert!(reason.contains("authenticate"), "{reason}");
}

// ─── ticket.search (§4.4) ─────────────────────────────────────────────────────

/// resolved — the whole body, and `filter[type]=ticket` is the plugin's, not
/// the caller's: the query string must not carry a type: term.
#[tokio::test]
async fn ticket_search_resolves_with_the_page() {
    let server = mock().await;
    let page = json!({
        "results": [ticket(101, "open")],
        "meta": {"has_more": false, "after_cursor": "c9"},
        "links": {"next": null, "prev": null},
    });
    Mock::given(method("GET"))
        .and(path("/api/v2/search/export"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page.clone()))
        .mount(server)
        .await;

    let p = promise(
        &promise_id("search-ok"),
        "ticket.search",
        json!({"query": "status:open tags:escalated", "page[size]": 100}),
        in_ms(60_000),
    );
    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value, page);
    // The 4.4.3 Integration Request.
    let sent = requests(server).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].method.as_str(), "GET");
    assert_eq!(sent[0].url.path(), "/api/v2/search/export");
    assert_eq!(
        query(&sent[0]),
        vec![
            ("query".to_string(), "status:open tags:escalated".to_string()),
            ("page[size]".to_string(), "100".to_string()),
            ("filter[type]".to_string(), "ticket".to_string()),
        ]
    );
}

/// `invalid_request` — a query that fails validation, or an expired cursor.
#[tokio::test]
async fn ticket_search_rejects_invalid_request() {
    let server = mock().await;
    let detail = r#"{"error":"invalid","description":"Invalid search: type is not supported"}"#;
    Mock::given(method("GET"))
        .and(path("/api/v2/search/export"))
        .respond_with(ResponseTemplate::new(422).set_body_string(detail))
        .mount(server)
        .await;

    let p = promise(
        &promise_id("search-invalid"),
        "ticket.search",
        json!({"query": "type:ticket status:open"}),
        in_ms(60_000),
    );
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "invalid_request", "detail": detail}));
}

/// `invalid_request` — `query` is required by the 4.4.1 Param schema.
#[tokio::test]
async fn ticket_search_rejects_invalid_request_locally() {
    let server = mock().await;

    let p = promise(&promise_id("search-noquery"), "ticket.search", json!({"page[size]": 10}), in_ms(60_000));
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(value["detail"].as_str().unwrap_or_default().contains("query"), "{value}");
    assert!(requests(server).await.is_empty(), "nothing should have been sent");
}

/// halt — the credential is corrupt.
#[tokio::test]
async fn ticket_search_halts_on_a_rejected_credential() {
    let server = mock().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/search/export"))
        .respond_with(
            ResponseTemplate::new(401).set_body_string(r#"{"error":"Couldn't authenticate you"}"#),
        )
        .mount(server)
        .await;

    let p = promise(
        &promise_id("search-halt"),
        "ticket.search",
        json!({"query": "status:open"}),
        in_ms(60_000),
    );
    let reason = halted(plugin::process(&corrupt_config(), &p).await);

    assert!(reason.contains("authenticate"), "{reason}");
}

// ─── ticket.update (§4.5) ─────────────────────────────────────────────────────

/// resolved — the 4.5.2 Resolved value is `response.body.ticket`; the audit
/// the update generated is not part of it.
#[tokio::test]
async fn ticket_update_resolves_with_the_updated_ticket() {
    let server = mock().await;
    let record = ticket(101, "solved");
    Mock::given(method("PUT"))
        .and(path("/api/v2/tickets/101"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ticket": record,
            "audit": {"id": 9001, "ticket_id": 101, "created_at": "2026-08-29T12:00:00Z", "events": []},
        })))
        .mount(server)
        .await;

    let p = promise(&promise_id("update-ok"), "ticket.update", update_args(), in_ms(60_000));
    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value, record);
    // The 4.5.3 Integration Request: ticket_id travels in the path, every
    // other argument in the body under `ticket`.
    let sent = requests(server).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].method.as_str(), "PUT");
    assert_eq!(sent[0].url.path(), "/api/v2/tickets/101");
    assert_eq!(basic(&sent[0]), expected_auth());
    let body: Value = sent[0].body_json().expect("the update body is JSON");
    let mut expected = update_args();
    expected.as_object_mut().expect("args is an object").remove("ticket_id");
    assert_eq!(body, json!({"ticket": expected}));
}

/// `not_found` — a nonexistent id.
#[tokio::test]
async fn ticket_update_rejects_not_found() {
    let server = mock().await;
    let detail = r#"{"error":"RecordNotFound","description":"Not found"}"#;
    Mock::given(method("PUT"))
        .and(path("/api/v2/tickets/999999"))
        .respond_with(ResponseTemplate::new(404).set_body_string(detail))
        .mount(server)
        .await;

    let p = promise(
        &promise_id("update-404"),
        "ticket.update",
        json!({"ticket_id": 999999, "status": "solved"}),
        in_ms(60_000),
    );
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "not_found", "detail": detail}));
}

/// `conflict` — `safe_update` was set and the ticket changed after
/// `updated_stamp`.
#[tokio::test]
async fn ticket_update_rejects_conflict() {
    let server = mock().await;
    let detail = r#"{"error":"UpdateConflict","description":"the ticket has changed"}"#;
    Mock::given(method("PUT"))
        .and(path("/api/v2/tickets/101"))
        .respond_with(ResponseTemplate::new(409).set_body_string(detail))
        .mount(server)
        .await;

    let p = promise(
        &promise_id("update-409"),
        "ticket.update",
        json!({
            "ticket_id": 101,
            "status": "solved",
            "safe_update": true,
            "updated_stamp": "2026-08-29T11:00:00Z",
        }),
        in_ms(60_000),
    );
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "conflict", "detail": detail}));
}

/// `invalid_request` — 422 RecordInvalid, which is also the answer for any
/// update to a closed ticket.
#[tokio::test]
async fn ticket_update_rejects_invalid_request() {
    let server = mock().await;
    let detail = r#"{"error":"RecordInvalid","description":"Record validation errors","details":{"base":[{"description":"Status: closed prevents ticket update"}]}}"#;
    Mock::given(method("PUT"))
        .and(path("/api/v2/tickets/101"))
        .respond_with(ResponseTemplate::new(422).set_body_string(detail))
        .mount(server)
        .await;

    let p = promise(
        &promise_id("update-422"),
        "ticket.update",
        json!({"ticket_id": 101, "status": "open"}),
        in_ms(60_000),
    );
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "invalid_request", "detail": detail}));
}

/// `invalid_request` — `ticket_id` is required by the 4.5.1 Param schema, and
/// it is an integer, not the string spelling of one.
#[tokio::test]
async fn ticket_update_rejects_invalid_request_locally() {
    let server = mock().await;

    let p = promise(
        &promise_id("update-badid"),
        "ticket.update",
        json!({"ticket_id": "101", "status": "solved"}),
        in_ms(60_000),
    );
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(value["detail"].as_str().unwrap_or_default().contains("ticket_id"), "{value}");
    assert!(requests(server).await.is_empty(), "nothing should have been sent");
}

/// halt — the credential is corrupt.
#[tokio::test]
async fn ticket_update_halts_on_a_rejected_credential() {
    let server = mock().await;
    Mock::given(method("PUT"))
        .and(path("/api/v2/tickets/101"))
        .respond_with(
            ResponseTemplate::new(401).set_body_string(r#"{"error":"Couldn't authenticate you"}"#),
        )
        .mount(server)
        .await;

    let p = promise(&promise_id("update-halt"), "ticket.update", update_args(), in_ms(60_000));
    let reason = halted(plugin::process(&corrupt_config(), &p).await);

    assert!(reason.contains("authenticate"), "{reason}");
}

/// re-entry — this endpoint takes no idempotency key and nothing on the update
/// carries the promise's identity, so a redelivery re-sends the whole body:
/// the field values converge, and the comment in the body is appended a second
/// time. That is the documented trade, asserted rather than papered over.
#[tokio::test]
async fn ticket_update_re_sends_the_whole_body_on_re_entry() {
    let server = mock().await;
    let record = ticket(101, "solved");
    Mock::given(method("PUT"))
        .and(path("/api/v2/tickets/101"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ticket": record, "audit": {"id": 9001, "ticket_id": 101, "events": []},
        })))
        .mount(server)
        .await;

    let p = promise(&promise_id("update-reentry"), "ticket.update", update_args(), in_ms(60_000));
    let first = resolved(plugin::process(&config(), &p).await);
    let second = resolved(plugin::process(&config(), &p).await);

    assert_eq!(first, second);
    let sent = requests(server).await;
    assert_eq!(sent.len(), 2, "{:?}", paths(&sent));
    let body = |r: &Request| r.body_json::<Value>().expect("the update body is JSON");
    assert_eq!(body(&sent[0]), body(&sent[1]));
    assert!(body(&sent[0])["ticket"]["comment"].is_object(), "the comment rides along twice");
}

// ─── ticket.delete (§4.6) ─────────────────────────────────────────────────────

/// resolved — the 204 carries no body, so the 4.6.2 Resolved value is built
/// from the param: the id that was deleted.
#[tokio::test]
async fn ticket_delete_resolves_with_the_deleted_id() {
    let server = mock().await;
    Mock::given(method("DELETE"))
        .and(path("/api/v2/tickets/101"))
        .respond_with(ResponseTemplate::new(204))
        .mount(server)
        .await;

    let p = promise(&promise_id("delete-ok"), "ticket.delete", json!({"ticket_id": 101}), in_ms(60_000));
    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"id": 101, "deleted": true}));
    // The 4.6.3 Integration Request: no body, no query.
    let sent = requests(server).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].method.as_str(), "DELETE");
    assert_eq!(sent[0].url.path(), "/api/v2/tickets/101");
    assert_eq!(query(&sent[0]), Vec::<(String, String)>::new());
    assert_eq!(basic(&sent[0]), expected_auth());
}

/// `not_found` — a nonexistent id, and also what a redelivery meets once this
/// promise's own delete has landed: the ticket is soft-deleted and this
/// endpoint no longer sees it.
#[tokio::test]
async fn ticket_delete_rejects_not_found_including_on_re_entry() {
    let server = mock().await;
    let detail = r#"{"error":"RecordNotFound","description":"Not found"}"#;
    Mock::given(method("DELETE"))
        .and(path("/api/v2/tickets/101"))
        .respond_with(ResponseTemplate::new(204))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v2/tickets/101"))
        .respond_with(ResponseTemplate::new(404).set_body_string(detail))
        .with_priority(2)
        .mount(server)
        .await;

    let p = promise(&promise_id("delete-404"), "ticket.delete", json!({"ticket_id": 101}), in_ms(60_000));
    let first = resolved(plugin::process(&config(), &p).await);
    let second = rejected(plugin::process(&config(), &p).await);

    assert_eq!(first, json!({"id": 101, "deleted": true}));
    assert_eq!(second, json!({"code": "not_found", "detail": detail}));
}

/// `invalid_request` — the delete fails validation.
#[tokio::test]
async fn ticket_delete_rejects_invalid_request() {
    let server = mock().await;
    let detail = r#"{"error":"InvalidRequest","description":"cannot be deleted"}"#;
    Mock::given(method("DELETE"))
        .and(path("/api/v2/tickets/101"))
        .respond_with(ResponseTemplate::new(400).set_body_string(detail))
        .mount(server)
        .await;

    let p = promise(&promise_id("delete-400"), "ticket.delete", json!({"ticket_id": 101}), in_ms(60_000));
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "invalid_request", "detail": detail}));
}

/// `invalid_request` — `ticket_id` is required by the 4.6.1 Param schema.
#[tokio::test]
async fn ticket_delete_rejects_invalid_request_locally() {
    let server = mock().await;

    let p = promise(&promise_id("delete-noid"), "ticket.delete", json!({}), in_ms(60_000));
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(value["detail"].as_str().unwrap_or_default().contains("ticket_id"), "{value}");
    assert!(requests(server).await.is_empty(), "nothing should have been sent");
}

/// halt — 403: deleting a ticket is beyond the token's scope.
#[tokio::test]
async fn ticket_delete_halts_on_a_rejected_credential() {
    let server = mock().await;
    Mock::given(method("DELETE"))
        .and(path("/api/v2/tickets/101"))
        .respond_with(ResponseTemplate::new(403).set_body_string(r#"{"error":"Forbidden"}"#))
        .mount(server)
        .await;

    let p = promise(&promise_id("delete-halt"), "ticket.delete", json!({"ticket_id": 101}), in_ms(60_000));
    let reason = halted(plugin::process(&corrupt_config(), &p).await);

    assert!(reason.contains("Forbidden"), "{reason}");
}

// ─── ticket.merge (§4.7) ──────────────────────────────────────────────────────

/// resolved, and the pending → terminal path: the merge answers with a
/// `queued` job, so a resolved value carrying `results` is proof the poll loop
/// watched the job to its terminal state rather than reporting the queueing.
#[tokio::test]
async fn ticket_merge_resolves_when_the_job_completes() {
    let server = mock().await;
    let results = json!([{"id": 202, "action": "update", "status": "Updated", "success": true}]);
    merge_stub(server, 200, job("job1", "queued", Value::Null)).await;
    poll_stub(server, "/api/v2/job_statuses/job1", 1, job("job1", "working", Value::Null)).await;
    poll_stub(server, "/api/v2/job_statuses/job1", 2, job("job1", "completed", results.clone())).await;

    let p = promise(&promise_id("merge-ok"), "ticket.merge", merge_args(), in_ms(60_000));
    let value = resolved(plugin::process(&config(), &p).await);

    // The 4.7.2 Resolved schema: seven named keys of the job status, and an
    // absent one is null rather than missing.
    assert_eq!(
        value,
        json!({
            "id": "job1",
            "url": "https://acme.zendesk.com/api/v2/job_statuses/job1",
            "status": "completed",
            "message": null,
            "progress": 1,
            "total": 1,
            "results": results,
        })
    );

    // The 4.7.3 Integration Request: the target id in the path, everything
    // else in the body.
    let sent = requests(server).await;
    assert_eq!(sent.len(), 3, "{:?}", paths(&sent));
    assert_eq!(sent[0].method.as_str(), "POST");
    assert_eq!(sent[0].url.path(), "/api/v2/tickets/101/merge");
    assert_eq!(basic(&sent[0]), expected_auth());
    let mut expected = merge_args();
    expected.as_object_mut().expect("args is an object").remove("ticket_id");
    assert_eq!(sent[0].body_json::<Value>().expect("the merge body is JSON"), expected);
    for poll in &sent[1..] {
        assert_eq!(poll.method.as_str(), "GET");
        assert_eq!(poll.url.path(), "/api/v2/job_statuses/job1");
    }
}

/// `merge_failed` — the failing work item: job status "failed". detail = the
/// terminal job status object.
#[tokio::test]
async fn ticket_merge_rejects_merge_failed() {
    let server = mock().await;
    let failed = job("job2", "failed", Value::Null);
    merge_stub(server, 200, job("job2", "queued", Value::Null)).await;
    poll_stub(server, "/api/v2/job_statuses/job2", 1, failed.clone()).await;

    let p = promise(&promise_id("merge-failed"), "ticket.merge", merge_args(), in_ms(60_000));
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "merge_failed", "detail": failed}));
}

/// `killed` — job status "killed", the other terminal failure.
#[tokio::test]
async fn ticket_merge_rejects_killed() {
    let server = mock().await;
    let killed = job("job3", "killed", Value::Null);
    merge_stub(server, 200, job("job3", "queued", Value::Null)).await;
    poll_stub(server, "/api/v2/job_statuses/job3", 1, killed.clone()).await;

    let p = promise(&promise_id("merge-killed"), "ticket.merge", merge_args(), in_ms(60_000));
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "killed", "detail": killed}));
}

/// `job_not_found` — the job status record disappears while polling.
#[tokio::test]
async fn ticket_merge_rejects_job_not_found() {
    let server = mock().await;
    let detail = r#"{"error":"RecordNotFound","description":"Not found"}"#;
    merge_stub(server, 200, job("job4", "queued", Value::Null)).await;
    Mock::given(method("GET"))
        .and(path("/api/v2/job_statuses/job4"))
        .respond_with(ResponseTemplate::new(404).set_body_string(detail))
        .mount(server)
        .await;

    let p = promise(&promise_id("merge-jobgone"), "ticket.merge", merge_args(), in_ms(60_000));
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "job_not_found", "detail": detail}));
}

/// `not_found` — the target ticket does not exist.
#[tokio::test]
async fn ticket_merge_rejects_not_found() {
    let server = mock().await;
    let detail = r#"{"error":"RecordNotFound","description":"Not found"}"#;
    Mock::given(method("POST"))
        .and(path("/api/v2/tickets/999999/merge"))
        .respond_with(ResponseTemplate::new(404).set_body_string(detail))
        .mount(server)
        .await;

    let mut args = merge_args();
    args["ticket_id"] = json!(999999);
    let p = promise(&promise_id("merge-404"), "ticket.merge", args, in_ms(60_000));
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "not_found", "detail": detail}));
}

/// `invalid_request` — the merge fails validation: a solved or closed target,
/// or a source that is not below "solved".
#[tokio::test]
async fn ticket_merge_rejects_invalid_request() {
    let server = mock().await;
    let detail = r#"{"error":"RecordInvalid","description":"Cannot merge into a closed ticket"}"#;
    merge_stub_body(server, 422, detail).await;

    let p = promise(&promise_id("merge-422"), "ticket.merge", merge_args(), in_ms(60_000));
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "invalid_request", "detail": detail}));
}

/// `invalid_request` — `ids` is required by the 4.7.1 Param schema.
#[tokio::test]
async fn ticket_merge_rejects_invalid_request_locally() {
    let server = mock().await;

    let p = promise(
        &promise_id("merge-noids"),
        "ticket.merge",
        json!({"ticket_id": 101, "target_comment": "merged"}),
        in_ms(60_000),
    );
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(value["detail"].as_str().unwrap_or_default().contains("ids"), "{value}");
    assert!(requests(server).await.is_empty(), "nothing should have been sent");
}

/// The deadline — `timeout_at` already in the past. No verdict: the server
/// settles a timed-out promise itself. The merge was queued, so it runs either
/// way; the loop just refuses to watch past the deadline.
#[tokio::test]
async fn ticket_merge_releases_at_the_deadline() {
    let server = mock().await;
    merge_stub(server, 200, job("job5", "queued", Value::Null)).await;

    let p = promise(&promise_id("merge-deadline"), "ticket.merge", merge_args(), in_ms(-1_000));
    let reason = released(plugin::process(&config(), &p).await);

    assert_eq!(reason, "promise timed out");
    // The deadline is observed before the first poll, not after it.
    assert_eq!(paths(&requests(server).await), vec!["/api/v2/tickets/101/merge".to_string()]);
}

/// The consecutive-failure streak cap. 5xx is otherwise the frame's release
/// path and is not induced per plugin, but the cap is unreachable without a
/// series of them: five in a row and the poll loop gives up and releases.
#[tokio::test]
async fn ticket_merge_releases_after_five_consecutive_failures() {
    let server = mock().await;
    merge_stub(server, 200, job("job6", "queued", Value::Null)).await;
    Mock::given(method("GET"))
        .and(path("/api/v2/job_statuses/job6"))
        .respond_with(ResponseTemplate::new(503).set_body_string("upstream unavailable"))
        .mount(server)
        .await;

    let p = promise(&promise_id("merge-streak"), "ticket.merge", merge_args(), in_ms(60_000));
    let reason = released(plugin::process(&config(), &p).await);

    assert_eq!(reason, "upstream unavailable");
    // One queueing POST and exactly five polls: the fifth failure is the cap.
    assert_eq!(requests(server).await.len(), 6);
}

/// halt — the credential is corrupt.
#[tokio::test]
async fn ticket_merge_halts_on_a_rejected_credential() {
    let server = mock().await;
    merge_stub_body(server, 401, r#"{"error":"Couldn't authenticate you"}"#).await;

    let p = promise(&promise_id("merge-halt"), "ticket.merge", merge_args(), in_ms(60_000));
    let reason = halted(plugin::process(&corrupt_config(), &p).await);

    assert!(reason.contains("authenticate"), "{reason}");
}

// ─── ticketcomment.list (§4.8) ────────────────────────────────────────────────

/// resolved — the whole body: the ticket's conversation, which the ticket
/// record itself does not carry.
#[tokio::test]
async fn ticketcomment_list_resolves_with_the_page() {
    let server = mock().await;
    let page = json!({
        "comments": [{
            "id": 5001,
            "type": "Comment",
            "author_id": 7,
            "body": "It is on fire.",
            "public": true,
            "created_at": "2026-08-29T12:00:00Z",
            "audit_id": 9001,
            "attachments": [],
            "via": {"channel": "api", "source": {}},
        }],
        "meta": {"has_more": false, "after_cursor": "c1", "before_cursor": "c1"},
        "links": {"next": null, "prev": null},
    });
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets/101/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page.clone()))
        .mount(server)
        .await;

    let p = promise(
        &promise_id("comments-ok"),
        "ticketcomment.list",
        json!({
            "ticket_id": 101,
            "include": "users",
            "include_inline_images": true,
            "sort": "-created_at",
            "page[size]": 50,
        }),
        in_ms(60_000),
    );
    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value, page);
    // The 4.8.3 Integration Request: a boolean renders lowercase, an integer
    // as its digits.
    let sent = requests(server).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].method.as_str(), "GET");
    assert_eq!(sent[0].url.path(), "/api/v2/tickets/101/comments");
    assert_eq!(
        query(&sent[0]),
        vec![
            ("include".to_string(), "users".to_string()),
            ("include_inline_images".to_string(), "true".to_string()),
            ("sort".to_string(), "-created_at".to_string()),
            ("page[size]".to_string(), "50".to_string()),
        ]
    );
}

/// `not_found` — a nonexistent id, which is also the answer for a deleted
/// ticket.
#[tokio::test]
async fn ticketcomment_list_rejects_not_found() {
    let server = mock().await;
    let detail = r#"{"error":"RecordNotFound","description":"Not found"}"#;
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets/999999/comments"))
        .respond_with(ResponseTemplate::new(404).set_body_string(detail))
        .mount(server)
        .await;

    let p = promise(
        &promise_id("comments-404"),
        "ticketcomment.list",
        json!({"ticket_id": 999999}),
        in_ms(60_000),
    );
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "not_found", "detail": detail}));
}

/// `invalid_request` — a malformed or expired cursor.
#[tokio::test]
async fn ticketcomment_list_rejects_invalid_request() {
    let server = mock().await;
    let detail = r#"{"error":"InvalidPaginationParameter","description":"invalid cursor"}"#;
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets/101/comments"))
        .respond_with(ResponseTemplate::new(400).set_body_string(detail))
        .mount(server)
        .await;

    let p = promise(
        &promise_id("comments-400"),
        "ticketcomment.list",
        json!({"ticket_id": 101, "page[size]": 10, "page[after]": "nope"}),
        in_ms(60_000),
    );
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "invalid_request", "detail": detail}));
}

/// `invalid_request` — `ticket_id` is required by the 4.8.1 Param schema.
#[tokio::test]
async fn ticketcomment_list_rejects_invalid_request_locally() {
    let server = mock().await;

    let p = promise(&promise_id("comments-noid"), "ticketcomment.list", json!({}), in_ms(60_000));
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(value["detail"].as_str().unwrap_or_default().contains("ticket_id"), "{value}");
    assert!(requests(server).await.is_empty(), "nothing should have been sent");
}

/// halt — the credential is corrupt.
#[tokio::test]
async fn ticketcomment_list_halts_on_a_rejected_credential() {
    let server = mock().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets/101/comments"))
        .respond_with(ResponseTemplate::new(403).set_body_string(r#"{"error":"Forbidden"}"#))
        .mount(server)
        .await;

    let p = promise(
        &promise_id("comments-halt"),
        "ticketcomment.list",
        json!({"ticket_id": 101}),
        in_ms(60_000),
    );
    let reason = halted(plugin::process(&corrupt_config(), &p).await);

    assert!(reason.contains("Forbidden"), "{reason}");
}

// ─── Dispatch ─────────────────────────────────────────────────────────────────

/// A func no operation serves is a permanent rejection naming it.
#[tokio::test]
async fn an_unknown_func_is_rejected() {
    let server = mock().await;

    let p = promise(&promise_id("unknown"), "ticket.explode", json!({}), in_ms(60_000));
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "unknown_func", "detail": "ticket.explode"}));
    assert!(requests(server).await.is_empty());
}

/// A param that is not a `{func, args}` envelope at all is permanent too — it
/// is immutable, so no redelivery reads it differently.
#[tokio::test]
async fn a_param_without_a_func_is_rejected() {
    let _ = mock().await;
    let mut p = promise(&promise_id("nofunc"), "ticket.get", json!({}), in_ms(60_000));
    p.param.data = Some(base64::engine::general_purpose::STANDARD.encode("{}"));

    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "invalid_request", "detail": "param has no func"}));
}

// ─── The mock provider ────────────────────────────────────────────────────────

/// One mock server for the whole binary. The plugin resolves its API root
/// once, so the address cannot change between tests — and the tests are
/// serial, so resetting here gives each one a clean server.
async fn mock() -> &'static MockServer {
    static SERVER: tokio::sync::OnceCell<MockServer> = tokio::sync::OnceCell::const_new();
    let server = SERVER
        .get_or_init(|| async {
            let server = MockServer::start().await;
            // Where §5 would name a provider to run, this names the mock. The
            // paths below are then the specification's own, `/api/v2/...`.
            std::env::set_var("ZENDESK_API", format!("{}/api/v2", server.uri()));
            server
        })
        .await;
    server.reset().await;
    server
}

/// The external_id lookup §4.1.5 opens with.
async fn lookup_stub(server: &MockServer, body: Value) {
    Mock::given(method("GET"))
        .and(path("/api/v2/tickets"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

/// The merge call of §4.7.3, answering with a JSON body.
async fn merge_stub(server: &MockServer, status: u16, job_status: Value) {
    Mock::given(method("POST"))
        .and(path("/api/v2/tickets/101/merge"))
        .respond_with(ResponseTemplate::new(status).set_body_json(json!({"job_status": job_status})))
        .mount(server)
        .await;
}

/// The merge call, answering with a body kept verbatim: a rejection's `detail`
/// is the response body as text.
async fn merge_stub_body(server: &MockServer, status: u16, body: &str) {
    Mock::given(method("POST"))
        .and(path("/api/v2/tickets/101/merge"))
        .respond_with(ResponseTemplate::new(status).set_body_string(body))
        .mount(server)
        .await;
}

/// One answer of the poll loop, in sequence: `nth` orders the stubs and
/// `up_to_n_times(1)` retires each once it has answered.
async fn poll_stub(server: &MockServer, at: &str, nth: u8, job_status: Value) {
    Mock::given(method("GET"))
        .and(path(at.to_string()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"job_status": job_status})))
        .up_to_n_times(1)
        .with_priority(nth)
        .mount(server)
        .await;
}

/// A §4.1.4 ticket object, trimmed to the keys the schema marks required plus
/// the ones these tests read.
fn ticket(id: u64, status: &str) -> Value {
    json!({
        "id": id,
        "url": format!("https://acme.zendesk.com/api/v2/tickets/{id}.json"),
        "external_id": null,
        "created_at": "2026-08-29T12:00:00Z",
        "updated_at": "2026-08-29T12:00:00Z",
        "subject": "Printer on fire",
        "description": "It is on fire.",
        "status": status,
        "priority": "high",
        "requester_id": 7,
        "submitter_id": 7,
        "assignee_id": null,
        "group_id": null,
        "tags": ["urgent"],
        "custom_fields": [],
        "via": {"channel": "api", "source": {"from": {}, "to": {}, "rel": null}},
    })
}

/// A §4.7.4 job status object.
fn job(id: &str, status: &str, results: Value) -> Value {
    json!({
        "id": id,
        "url": format!("https://acme.zendesk.com/api/v2/job_statuses/{id}"),
        "job_type": "TicketMergeJob",
        "status": status,
        "progress": if status == "queued" { 0 } else { 1 },
        "total": 1,
        "results": results,
    })
}

async fn requests(server: &MockServer) -> Vec<Request> {
    server.received_requests().await.expect("request recording is on")
}

fn paths(sent: &[Request]) -> Vec<String> {
    sent.iter().map(|r| r.url.path().to_string()).collect()
}

fn query(r: &Request) -> Vec<(String, String)> {
    r.url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect()
}

fn header(r: &Request, name: &str) -> String {
    r.headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

fn basic(r: &Request) -> String {
    header(r, "authorization")
}

// ─── Fixtures ─────────────────────────────────────────────────────────────────

/// §2. `poll` is overridden to 1s — the 2s default would double every
/// pending → terminal test's wall clock for nothing. `subdomain` is required
/// even though the mock seam overrides the API root it would otherwise build.
fn config() -> Config {
    Config {
        subdomain: "acme".to_string(),
        email: "agent@acme.com".to_string(),
        api_token: "zd_test_token".to_string(),
        poll: Duration::from_secs(1),
    }
}

/// The halt induction of the skill's condition table: a corrupt credential.
/// Against a mock the wire answer is what decides, so the stub answers 401 or
/// 403 — this config is the input that earns it.
fn corrupt_config() -> Config {
    Config { api_token: "not-the-token".to_string(), ..config() }
}

/// §3: an API token is presented as HTTP Basic, the user being
/// `{email}/token`.
fn expected_auth() -> String {
    let basic = base64::engine::general_purpose::STANDARD.encode("agent@acme.com/token:zd_test_token");
    format!("Basic {basic}")
}

/// A well-formed `ticket.create` argument object: the 4.1.1 Param schema
/// requires `comment`.
fn create_args() -> Value {
    json!({
        "subject": "Printer on fire",
        "comment": {"body": "It is on fire.", "public": true},
        "priority": "high",
        "tags": ["urgent"],
    })
}

/// A well-formed `ticket.update` argument object, carrying a comment so the
/// re-entry test has something to observe being appended twice.
fn update_args() -> Value {
    json!({
        "ticket_id": 101,
        "status": "solved",
        "priority": "normal",
        "comment": {"body": "Extinguished.", "public": false},
        "tags": ["resolved"],
    })
}

/// A well-formed `ticket.merge` argument object: the 4.7.1 Param schema
/// requires `ticket_id` and `ids`.
fn merge_args() -> Value {
    json!({
        "ticket_id": 101,
        "ids": [202, 203],
        "target_comment": "merged the duplicates",
        "source_comment": "merged into 101",
        "target_comment_is_public": false,
    })
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
        tags: std::collections::HashMap::new(),
        timeout_at,
        created_at: now_ms(),
        settled_at: None,
    }
}

/// A fresh promise id per test — and one that survives the frame's sanitize
/// unchanged, so the injected `external_id` may be checked against it with
/// `starts_with`.
fn promise_id(what: &str) -> String {
    format!("zendesk.{what}.{}", nanos())
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
