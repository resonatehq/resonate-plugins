//! `plugin::process` against a local mock standing where Bannerbear would be.
//!
//! The specification's Notes say `Self-hosted: no` — Bannerbear is SaaS only,
//! there is no §5 to provision, and the tests cannot reach the real API. So
//! every condition is induced against a `wiremock` server whose responses are
//! built from the §4.N.4 Integration Response schemas, and every outgoing
//! request is additionally asserted against the §4.N.3 Integration Request
//! schema: method, path, the injected `sanitize(promise.id)`, and the body
//! mapping.
//!
//! The circularity is worth stating plainly: the mocks are built from the same
//! specification as the code, so these tests can only show that the code says
//! what the specification says. They cannot show that the specification is
//! true of Bannerbear. Only review against the live provider breaks the loop.
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
use resonate_plugin_bannerbear::plugin::{self, Config};

/// The verdict `process` returns, spelled out: the crate's own alias for it
/// is an implementation detail.
type Verdict = Result<Result<String, String>, Result<String, String>>;

/// A well-formed `image.create` / `animation.create` argument object: both
/// Param schemas require `template` and `modifications`.
fn render_args(template: &str) -> Value {
    json!({
        "template": template,
        "modifications": {"objects": [{"name": "title", "text": "hello"}]},
    })
}

// ─── image.create (§4.1) ──────────────────────────────────────────────────────

/// resolved — the succeeding work item, and the pending → terminal path: the
/// create response is `pending` with no `files`, so a resolved value carrying
/// `files` is proof the poll loop watched the render to its terminal state
/// rather than reporting the create.
#[tokio::test]
async fn image_create_resolves_when_the_render_completes() {
    let server = mock().await;
    let uid = "img_resolve_1";
    let files = json!({"image_url": "https://cdn.bannerbear.com/img_resolve_1.png"});
    create_stub(server, "/v5/images", 200, image(uid, "pending", json!({}))).await;
    // One `pending` poll, then the terminal read: priority orders them, and
    // `up_to_n_times` retires the first once it has answered.
    poll_stub(server, &format!("/v5/images/{uid}"), 1, image(uid, "pending", json!({}))).await;
    poll_stub(
        server,
        &format!("/v5/images/{uid}"),
        2,
        json!({"uid": uid, "status": "completed", "files": files}),
    )
    .await;

    let id = promise_id("image-resolve");
    let p = promise(&id, "image.create", render_args("tpl_a"), in_ms(60_000));
    let value = resolved(plugin::process(&config(), &p).await);

    // The 4.1.2 Resolved schema is `{files}` and nothing else.
    assert_eq!(value, json!({"files": files}));

    // The 4.1.3 Integration Request.
    let sent = requests(server).await;
    let post = &sent[0];
    assert_eq!(post.method.as_str(), "POST");
    assert_eq!(post.url.path(), "/v5/images");
    assert_eq!(bearer(post), "Bearer bb_test_key");
    let body: Value = post.body_json().expect("the create body is JSON");
    assert_eq!(body["template"], "tpl_a");
    assert_eq!(body["modifications"], render_args("tpl_a")["modifications"]);
    // metadata = sanitize(promise.id), never the raw id.
    let metadata = body["metadata"].as_str().expect("metadata is a string");
    assert!(metadata.starts_with(&id), "{metadata} should be derived from {id}");
    assert_ne!(metadata, id, "metadata should carry sanitize's digest");
    // Two polls: `self` is the poll target, keyed by uid.
    assert_eq!(sent.len(), 3, "{:?}", paths(&sent));
    for poll in &sent[1..] {
        assert_eq!(poll.method.as_str(), "GET");
        assert_eq!(poll.url.path(), format!("/v5/images/{uid}"));
        assert_eq!(bearer(poll), "Bearer bb_test_key");
    }
}

/// `render_failed` — the failing work item. detail = the failed image object.
#[tokio::test]
async fn image_create_rejects_render_failed() {
    let server = mock().await;
    let uid = "img_fail_1";
    let failed = json!({"uid": uid, "status": "failed", "error": "layer 'title' not found"});
    create_stub(server, "/v5/images", 200, image(uid, "pending", json!({}))).await;
    poll_stub(server, &format!("/v5/images/{uid}"), 1, failed.clone()).await;

    let p = promise(
        &promise_id("image-failed"),
        "image.create",
        render_args("tpl_a"),
        in_ms(60_000),
    );
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "render_failed", "detail": failed}));
}

/// `invalid_request` — the provider's synchronous validation failures: an
/// unknown template is a 404, an invalid modification a 422, a malformed body
/// a 400. detail = the 4xx body.
#[tokio::test]
async fn image_create_rejects_invalid_request_from_the_provider() {
    for status in [400, 404, 422] {
        let server = mock().await;
        let detail = json!({"message": format!("rejected with {status}")});
        create_stub(server, "/v5/images", status, detail.clone()).await;

        let p = promise(
            &promise_id("image-invalid"),
            "image.create",
            render_args("no_such_template"),
            in_ms(60_000),
        );
        let value = rejected(plugin::process(&config(), &p).await);

        assert_eq!(value, json!({"code": "invalid_request", "detail": detail}));
    }
}

/// `invalid_request` — a param that violates the 4.1.1 Param schema before
/// any request goes out. The param is immutable, so no redelivery could fix
/// it: permanent, and never worth a round trip.
#[tokio::test]
async fn image_create_rejects_invalid_request_locally() {
    let server = mock().await;

    let p = promise(
        &promise_id("image-noargs"),
        "image.create",
        // `modifications` is required.
        json!({"template": "tpl_a"}),
        in_ms(60_000),
    );
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(
        value["detail"].as_str().unwrap_or_default().contains("modifications"),
        "{value}"
    );
    assert!(requests(server).await.is_empty(), "nothing should have been sent");
}

/// halt — 401 key invalid, 402 quota exhausted, 403 key lacks access. None of
/// them is a verdict on the promise and no retry of ours clears them: an
/// operator must act.
#[tokio::test]
async fn image_create_halts_on_a_rejected_key() {
    for status in [401, 402, 403] {
        let server = mock().await;
        create_stub(server, "/v5/images", status, json!({"message": "unauthorized"})).await;

        let p = promise(
            &promise_id("image-halt"),
            "image.create",
            render_args("tpl_a"),
            in_ms(60_000),
        );
        let reason = halted(plugin::process(&config(), &p).await);

        assert!(reason.contains("unauthorized"), "{status}: {reason}");
    }
}

/// The deadline — `timeout_at` already in the past. No verdict: the server
/// settles a timed-out promise itself. The render was started, so the credit
/// is spent either way; the loop just refuses to watch past the deadline.
#[tokio::test]
async fn image_create_releases_at_the_deadline() {
    let server = mock().await;
    let uid = "img_deadline_1";
    create_stub(server, "/v5/images", 200, image(uid, "pending", json!({}))).await;

    let p = promise(
        &promise_id("image-deadline"),
        "image.create",
        render_args("tpl_a"),
        in_ms(-1_000),
    );
    let reason = released(plugin::process(&config(), &p).await);

    assert_eq!(reason, "promise timed out");
    // The deadline is observed before the first poll, not after it.
    assert_eq!(paths(&requests(server).await), vec!["/v5/images".to_string()]);
}

/// Re-entry — the create is an unkeyed POST, so a redelivery renders again.
/// That is the documented trade: a duplicate render is benign, costing one
/// render credit, and both attempts reach the same verdict.
#[tokio::test]
async fn image_create_renders_again_on_re_entry() {
    let server = mock().await;
    let files = json!({"image_url": "https://cdn.bannerbear.com/img_reentry.png"});
    create_stub(
        server,
        "/v5/images",
        200,
        json!({"uid": "img_reentry", "status": "completed", "files": files}),
    )
    .await;

    let id = promise_id("image-reentry");
    let p = promise(&id, "image.create", render_args("tpl_a"), in_ms(60_000));
    let first = resolved(plugin::process(&config(), &p).await);
    let second = resolved(plugin::process(&config(), &p).await);

    assert_eq!(first, second);
    let sent = requests(server).await;
    assert_eq!(paths(&sent), vec!["/v5/images".to_string(), "/v5/images".to_string()]);
    // Same promise, same injected identity — the renders are distinguishable
    // as this promise's, even though nothing dedupes them.
    let metadata = |r: &Request| r.body_json::<Value>().unwrap()["metadata"].as_str().unwrap().to_string();
    assert_eq!(metadata(&sent[0]), metadata(&sent[1]));
    assert!(metadata(&sent[0]).starts_with(&id));
}

// ─── animation.create (§4.2) ──────────────────────────────────────────────────

/// resolved — queued, then rendering, then completed: both non-terminal
/// statuses keep the loop.
#[tokio::test]
async fn animation_create_resolves_when_the_render_completes() {
    let server = mock().await;
    let uid = "anim_resolve_1";
    let files = json!({"mp4_url": "https://cdn.bannerbear.com/anim_resolve_1.mp4"});
    create_stub(server, "/v5/animations", 200, animation(uid, "queued")).await;
    poll_stub(server, &format!("/v5/animations/{uid}"), 1, animation(uid, "queued")).await;
    poll_stub(server, &format!("/v5/animations/{uid}"), 2, animation(uid, "rendering")).await;
    poll_stub(
        server,
        &format!("/v5/animations/{uid}"),
        3,
        json!({"uid": uid, "status": "completed", "files": files, "progress": 100}),
    )
    .await;

    let id = promise_id("animation-resolve");
    let p = promise(&id, "animation.create", render_args("anim_tpl"), in_ms(60_000));
    let value = resolved(plugin::process(&config(), &p).await);

    // The 4.2.2 Resolved schema is `{files}` and nothing else.
    assert_eq!(value, json!({"files": files}));

    // The 4.2.3 Integration Request.
    let sent = requests(server).await;
    let post = &sent[0];
    assert_eq!(post.method.as_str(), "POST");
    assert_eq!(post.url.path(), "/v5/animations");
    assert_eq!(bearer(post), "Bearer bb_test_key");
    let body: Value = post.body_json().expect("the create body is JSON");
    assert_eq!(body["template"], "anim_tpl");
    assert_eq!(body["modifications"], render_args("anim_tpl")["modifications"]);
    assert!(body["metadata"].as_str().unwrap().starts_with(&id), "{body}");
    assert_eq!(sent.len(), 4, "{:?}", paths(&sent));
}

/// `render_failed` — detail = response.body.error, not the whole object.
#[tokio::test]
async fn animation_create_rejects_render_failed() {
    let server = mock().await;
    let uid = "anim_fail_1";
    create_stub(server, "/v5/animations", 200, animation(uid, "queued")).await;
    poll_stub(
        server,
        &format!("/v5/animations/{uid}"),
        1,
        json!({"uid": uid, "status": "failed", "error": "source video unreachable"}),
    )
    .await;

    let p = promise(
        &promise_id("animation-failed"),
        "animation.create",
        render_args("anim_tpl"),
        in_ms(60_000),
    );
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(
        value,
        json!({"code": "render_failed", "detail": "source video unreachable"})
    );
}

/// `invalid_request` — the provider's synchronous validation failures.
#[tokio::test]
async fn animation_create_rejects_invalid_request_from_the_provider() {
    for status in [400, 404, 422] {
        let server = mock().await;
        let detail = json!({"message": format!("rejected with {status}")});
        create_stub(server, "/v5/animations", status, detail.clone()).await;

        let p = promise(
            &promise_id("animation-invalid"),
            "animation.create",
            render_args("no_such_template"),
            in_ms(60_000),
        );
        let value = rejected(plugin::process(&config(), &p).await);

        assert_eq!(value, json!({"code": "invalid_request", "detail": detail}));
    }
}

/// `invalid_request` — a param that violates the 4.2.1 Param schema.
#[tokio::test]
async fn animation_create_rejects_invalid_request_locally() {
    let server = mock().await;

    let p = promise(
        &promise_id("animation-noargs"),
        "animation.create",
        // `template` is required, and must be a string.
        json!({"template": 7, "modifications": {}}),
        in_ms(60_000),
    );
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(value["detail"].as_str().unwrap_or_default().contains("template"), "{value}");
    assert!(requests(server).await.is_empty(), "nothing should have been sent");
}

/// halt — the same three operator-required statuses.
#[tokio::test]
async fn animation_create_halts_on_a_rejected_key() {
    let server = mock().await;
    create_stub(server, "/v5/animations", 402, json!({"message": "quota exhausted"})).await;

    let p = promise(
        &promise_id("animation-halt"),
        "animation.create",
        render_args("anim_tpl"),
        in_ms(60_000),
    );
    let reason = halted(plugin::process(&config(), &p).await);

    assert!(reason.contains("quota exhausted"), "{reason}");
}

/// The deadline — `timeout_at` already in the past.
#[tokio::test]
async fn animation_create_releases_at_the_deadline() {
    let server = mock().await;
    create_stub(server, "/v5/animations", 200, animation("anim_deadline", "queued")).await;

    let p = promise(
        &promise_id("animation-deadline"),
        "animation.create",
        render_args("anim_tpl"),
        in_ms(-1_000),
    );
    let reason = released(plugin::process(&config(), &p).await);

    assert_eq!(reason, "promise timed out");
    assert_eq!(paths(&requests(server).await), vec!["/v5/animations".to_string()]);
}

/// Re-entry — an unkeyed POST, as in §4.1: the redelivery renders again.
#[tokio::test]
async fn animation_create_renders_again_on_re_entry() {
    let server = mock().await;
    let files = json!({"mp4_url": "https://cdn.bannerbear.com/anim_reentry.mp4"});
    create_stub(
        server,
        "/v5/animations",
        200,
        json!({"uid": "anim_reentry", "status": "completed", "files": files}),
    )
    .await;

    let p = promise(
        &promise_id("animation-reentry"),
        "animation.create",
        render_args("anim_tpl"),
        in_ms(60_000),
    );
    let first = resolved(plugin::process(&config(), &p).await);
    let second = resolved(plugin::process(&config(), &p).await);

    assert_eq!(first, second);
    assert_eq!(requests(server).await.len(), 2);
}

// ─── template.get (§4.3) ──────────────────────────────────────────────────────

/// resolved — = response.body, the whole template including its config.
#[tokio::test]
async fn template_get_resolves_with_the_template() {
    let server = mock().await;
    let template = json!({
        "uid": "tpl_a",
        "name": "Announcement",
        "available_modifications": [{"name": "title", "text": null}],
    });
    Mock::given(method("GET"))
        .and(path("/v5/image_templates/tpl_a"))
        .respond_with(ResponseTemplate::new(200).set_body_json(template.clone()))
        .mount(server)
        .await;

    let p = promise(
        &promise_id("template-get"),
        "template.get",
        json!({"uid": "tpl_a"}),
        in_ms(60_000),
    );
    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value, template);
    // The 4.3.3 Integration Request: no injected identity, uid in the path.
    let sent = requests(server).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].method.as_str(), "GET");
    assert_eq!(sent[0].url.path(), "/v5/image_templates/tpl_a");
    assert_eq!(bearer(&sent[0]), "Bearer bb_test_key");
}

/// `not_found` — a nonexistent uid. The 4.3.2 Rejected schema is `{code}`
/// alone: no detail.
#[tokio::test]
async fn template_get_rejects_not_found() {
    let server = mock().await;
    Mock::given(method("GET"))
        .and(path("/v5/image_templates/no_such_template"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"message": "not found"})))
        .mount(server)
        .await;

    let p = promise(
        &promise_id("template-get-404"),
        "template.get",
        json!({"uid": "no_such_template"}),
        in_ms(60_000),
    );
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "not_found"}));
}

/// `invalid_request` — `uid` is required by the 4.3.1 Param schema, and a
/// param that lacks it never will have it.
#[tokio::test]
async fn template_get_rejects_invalid_request() {
    let server = mock().await;

    let p = promise(&promise_id("template-get-bad"), "template.get", json!({}), in_ms(60_000));
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value["code"], "invalid_request");
    assert!(value["detail"].as_str().unwrap_or_default().contains("uid"), "{value}");
    assert!(requests(server).await.is_empty(), "nothing should have been sent");
}

/// halt — the key lacks access to this workspace.
#[tokio::test]
async fn template_get_halts_on_a_rejected_key() {
    let server = mock().await;
    Mock::given(method("GET"))
        .and(path("/v5/image_templates/tpl_a"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({"message": "forbidden"})))
        .mount(server)
        .await;

    let p = promise(
        &promise_id("template-get-halt"),
        "template.get",
        json!({"uid": "tpl_a"}),
        in_ms(60_000),
    );
    let reason = halted(plugin::process(&config(), &p).await);

    assert!(reason.contains("forbidden"), "{reason}");
}

// ─── template.list (§4.4) ─────────────────────────────────────────────────────

/// resolved — = response.body, an array. Pagination is the caller's loop, so
/// `page` goes out as given and one call returns one page.
#[tokio::test]
async fn template_list_resolves_with_the_templates() {
    let server = mock().await;
    let page = json!([{"uid": "tpl_a", "name": "Announcement"}, {"uid": "tpl_b", "name": "Quote"}]);
    Mock::given(method("GET"))
        .and(path("/v5/image_templates"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page.clone()))
        .mount(server)
        .await;

    let p = promise(
        &promise_id("template-list"),
        "template.list",
        json!({"page": 2}),
        in_ms(60_000),
    );
    let value = resolved(plugin::process(&config(), &p).await);

    assert_eq!(value, page);
    // The 4.4.3 Integration Request: page is a query parameter.
    let sent = requests(server).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].method.as_str(), "GET");
    assert_eq!(sent[0].url.path(), "/v5/image_templates");
    assert_eq!(query(&sent[0]), vec![("page".to_string(), "2".to_string())]);
}

/// `invalid_request` — a page the provider will not accept. detail = the 4xx
/// body.
#[tokio::test]
async fn template_list_rejects_invalid_request() {
    for status in [400, 422] {
        let server = mock().await;
        let detail = json!({"message": "page must be a positive integer"});
        Mock::given(method("GET"))
            .and(path("/v5/image_templates"))
            .respond_with(ResponseTemplate::new(status).set_body_json(detail.clone()))
            .mount(server)
            .await;

        let p = promise(
            &promise_id("template-list-bad"),
            "template.list",
            json!({"page": -1}),
            in_ms(60_000),
        );
        let value = rejected(plugin::process(&config(), &p).await);

        assert_eq!(value, json!({"code": "invalid_request", "detail": detail}));
    }
}

/// halt — the key is invalid.
#[tokio::test]
async fn template_list_halts_on_a_rejected_key() {
    let server = mock().await;
    Mock::given(method("GET"))
        .and(path("/v5/image_templates"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({"message": "bad api key"})))
        .mount(server)
        .await;

    let p = promise(&promise_id("template-list-halt"), "template.list", json!({}), in_ms(60_000));
    let reason = halted(plugin::process(&config(), &p).await);

    assert!(reason.contains("bad api key"), "{reason}");
}

// ─── Dispatch ─────────────────────────────────────────────────────────────────

/// A func no operation serves is a permanent rejection naming it.
#[tokio::test]
async fn an_unknown_func_is_rejected() {
    let server = mock().await;

    let p = promise(&promise_id("unknown"), "image.explode", json!({}), in_ms(60_000));
    let value = rejected(plugin::process(&config(), &p).await);

    assert_eq!(value, json!({"code": "unknown_func", "detail": "image.explode"}));
    assert!(requests(server).await.is_empty());
}

/// A param that is not a `{func, args}` envelope at all is permanent too — it
/// is immutable, so no redelivery reads it differently.
#[tokio::test]
async fn a_param_without_a_func_is_rejected() {
    let _ = mock().await;
    let mut p = promise(&promise_id("nofunc"), "image.create", json!({}), in_ms(60_000));
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
            // paths below are then the specification's own, `/v5/...`.
            std::env::set_var("BANNERBEAR_API", format!("{}/v5", server.uri()));
            server
        })
        .await;
    server.reset().await;
    server
}

/// The create call of §4.1.3 / §4.2.3.
async fn create_stub(server: &MockServer, at: &str, status: u16, body: Value) {
    Mock::given(method("POST"))
        .and(path(at.to_string()))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(server)
        .await;
}

/// One answer of the poll loop, in sequence: `nth` orders the stubs and
/// `up_to_n_times(1)` retires each once it has answered.
async fn poll_stub(server: &MockServer, at: &str, nth: u8, body: Value) {
    Mock::given(method("GET"))
        .and(path(at.to_string()))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .up_to_n_times(1)
        .with_priority(nth)
        .mount(server)
        .await;
}

/// A §4.1.4 image object.
fn image(uid: &str, status: &str, files: Value) -> Value {
    json!({
        "uid": uid,
        "status": status,
        "template": "tpl_a",
        "files": if status == "completed" { files } else { Value::Null },
        "self": format!("https://api.bannerbear.com/v5/images/{uid}"),
    })
}

/// A §4.2.4 animation object, still rendering.
fn animation(uid: &str, status: &str) -> Value {
    json!({
        "uid": uid,
        "status": status,
        "template": "anim_tpl",
        "progress": if status == "queued" { 0 } else { 50 },
        "self": format!("https://api.bannerbear.com/v5/animations/{uid}"),
    })
}

async fn requests(server: &MockServer) -> Vec<Request> {
    server.received_requests().await.expect("request recording is on")
}

fn paths(sent: &[Request]) -> Vec<String> {
    sent.iter().map(|r| r.url.path().to_string()).collect()
}

fn query(r: &Request) -> Vec<(String, String)> {
    r.url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

fn bearer(r: &Request) -> String {
    r.headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

// ─── Fixtures ─────────────────────────────────────────────────────────────────

/// §2. `poll` is overridden to 1s — the 2s default would double every
/// pending → terminal test's wall clock for nothing — and `poll_image` and
/// `poll_animation` are left unset so the cascade to `poll` is what the poll
/// loops actually run on.
fn config() -> Config {
    Config {
        api_key: "bb_test_key".to_string(),
        poll: Duration::from_secs(1),
        poll_image: None,
        poll_animation: None,
    }
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
/// unchanged, so the injected `metadata` may be checked against it with
/// `starts_with`.
fn promise_id(what: &str) -> String {
    format!("bannerbear.{what}.{}", nanos())
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
