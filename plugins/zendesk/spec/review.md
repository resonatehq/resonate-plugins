# Zendesk plugin — review trail

**Final verdict: approve** (after 2 round(s)).

---

# 0. API Surface

Derived from Zendesk's published `Support API` OpenAPI document (`openapi 3.0.3`,
`info.version 2.0.0`, `title "Support API"`, 337 paths) before reading §4.
`developer.zendesk.com` is refused by this session's egress proxy (403 to CONNECT
for both `curl` and WebFetch), so the OpenAPI was read from the mirror
`github.com/mtebusi/zendesk-oas-json` (`zendeskopenapi.json`; re-downloaded this
round and byte-identical to the copy used previously, md5 `030e3998…`), and doc
prose from the `description` fields that document carries plus web search.

**Tickets** — the primary resource; its full lifecycle is the plugin.

- `POST /api/v2/tickets` → `201` — open a ticket (subject, comment, requester,
  assignee, priority, type, tags, custom fields).
- `GET /api/v2/tickets/{ticket_id}` → `200` — read one ticket by id; the
  provider's own text notes this call does **not** return the ticket's comments.
- `GET /api/v2/tickets` → `200` — page the account's tickets, or filter by
  `external_id`, to find an id.
- `PUT /api/v2/tickets/{ticket_id}` → `200` — update a ticket: status, assignee,
  priority, tags, custom fields, and adding a public or internal comment
  (Zendesk has no "add comment" endpoint — a comment is written through the
  ticket update, so an SDK caller cannot hold a conversation without this call).
- `DELETE /api/v2/tickets/{ticket_id}` → `204` — soft-delete a ticket.

**Ticket comments** — the ticket's conversation, published separately from the
ticket record.

- `GET /api/v2/tickets/{ticket_id}/comments` → `200` — read the comment thread.
  Show Ticket omits comments and says so, so this is the read that completes the
  ticket for a caller, not an optional extra.

**Search** — how a caller finds ticket ids by criteria rather than by paging.

- `GET /api/v2/search` → `200` — query the account (`query`, `sort_by`,
  `sort_order`); offset pagination, capped at 1000 results.
- `GET /api/v2/search/export` → `200` — the same query with cursor pagination and
  `filter[type]`, for result sets above 1000. Peripheral where `/search` is
  already exposed: one of the two is what a caller needs, not both.

**Background jobs** — Zendesk's durable named unit. Bulk and merge endpoints
return a `job_status` and queue work; `job_status.status` runs
`queued|working|failed|completed|killed`.

- `POST /api/v2/tickets/{ticket_id}/merge` → `200` — merge source tickets into a
  target; returns a `job_status`, work happens in the background. This is the one
  Zendesk ticket action that is genuinely long-running, so it is the operation
  the promise's await semantics are worth anything for.
- `GET /api/v2/job_statuses/{job_status_id}` → `200` — observe a background job.
  As the completion mechanism for the merge it is transport, and plugin-specify's
  Bounds put a poll endpoint out; it would earn a place of its own only as a plain
  read for jobs a caller started out of band, which this plugin's surface cannot
  start. Out.

**Deliberately out.**

- `POST /api/v2/uploads` — attachments are `multipart/form-data` binary; a promise
  param is JSON, so this is not a call a plugin can carry.
- `/api/v2/users*`, `/api/v2/groups*`, `/api/v2/ticket_fields*`,
  `/api/v2/ticket_forms*`, `/api/v2/macros*`, `/api/v2/views*`,
  `/api/v2/triggers*`, `/api/v2/automations*` — instance administration and
  schema/definition management, excluded by plugin-specify's Bounds even though
  `users` and `groups` are what `requester_id`/`assignee_id` name.
- `/api/v2/tickets/create_many|update_many|destroy_many`,
  `/api/v2/tickets/show_many` — bulk variants of calls already exposed
  one-at-a-time; a caller reaching Zendesk through one promise per operation does
  not need them, and each adds a second async path.
- `/api/v2/tickets/count`, `/api/v2/tickets/{id}/audits`, `/related`,
  `/incidents`, `/collaborators`, `/followers`, `/email_ccs`,
  `/api/v2/deleted_tickets*`, `/api/v2/suspended_tickets*` — peripheral reads and
  secondary queues.

**What the provider cannot support.** There is no idempotency key on
`PUT /api/v2/tickets/{id}`, `DELETE /api/v2/tickets/{id}`, or
`POST /api/v2/tickets/{id}/merge` — only `POST /api/v2/tickets` accepts
`Idempotency-Key`. Nor is there an id-filtered read of deleted tickets
(`GET /api/v2/deleted_tickets` takes only `sort_by`/`sort_order`), so a delete
cannot be made re-entrant either. A plugin therefore cannot make update, delete
or merge `create_idempotent`; that is a property of Zendesk, not a defect in the
specification.

## Coverage

Document under review: `/tmp/resonate-plugin-factory/zendesk/plugins/zendesk/spec/specification.md`.
The `Reviewed by` row was overwritten to `Claude Opus 5, 2026-08-29`; no other
edit was made and nothing was committed. (`git status` also shows pre-existing
deletions under `plugins/zendesk/src/` — out of scope, not mine.)
`python3 skills/plugin-specify/lint.py` passes (0 errors; the one warning is the
now-filled `Reviewed by` row, expected post-review).

**Coverage against §0 is now complete and there is no coverage finding.** The
document exposes exactly the eight calls §0 wants — create, get, list,
search/export, update, delete, merge, and (new this round) `ticketcomment.list`
on `GET /api/v2/tickets/{ticket_id}/comments` — and exposes nothing §0 does not
want; the standalone `job.get` that §0 rules out as transport is gone, and with
it the poll-disclaimer sentence, correctly (no read remains that could be
mistaken for the merge's completion mechanism — verified by grep).

Fact-checking source: this session's egress proxy answers `403` to `CONNECT` for
every `*.zendesk.com` host, for `curl` and for WebFetch alike. No Zendesk account
or API token is available and none was created, so **no operation was executed
against the live provider** — the document is verified against the OpenAPI plus
published prose, not against wire behaviour. Re-checked by hand this round and
holding: base URL against the OpenAPI `servers` block; success statuses `201`,
`200`, `200`, `200`, `200`, `204`, `200`, `200`; the `status`/`priority`/`type`
enums (exact match to `TicketObject`); every property of §4.1.1 and §4.1.4 exists
in `TicketObject` (write-only `metadata`, `requester`, `safe_update`,
`updated_stamp` correctly excluded from the response schema); `comment` required
on create (`TicketCreateInput.required = ["comment"]`); `ids` required on merge;
`external_id` is a documented query parameter of List Tickets with the
non-uniqueness caveat the code's comment repeats; every §4.8 comment and
attachment field against `TicketCommentObject` / `AttachmentBaseObject`,
including the `malware_scan_result` enum and the `include` / `include_inline_images`
/ `sort` parameters and the ascending default; the search-export facts
(`filter[type]` required, `type:` in the query is an error, 1000/page with 100
recommended, `links.prev` always null, one-hour cursor, 100 requests per minute);
the 400-deletions-per-minute limit; and, by web search, the `Idempotency-Key`
facts (any string ≤255 chars, two-hour expiry, `x-idempotency-lookup: hit|miss`,
`400` on a body mismatch), `safe_update`/`updated_stamp` → `409`, the
5000-comment ceiling, and the five job states. All eight §4 mapping and
consistency invariants were re-checked mechanically and hold: every
`= promise.param.x` exists in the matching Param Schema with the same type (25 in
4.1, 21 in 4.5, 5 in 4.7, none missing, no type mismatches), every
`= response.body.x` resolves in the Integration Response, every constructed
`code` appears in its Rejected enum and vice versa, every `cfg.<key>`
(`subdomain`, `email`, `api_token`, `poll`) is in §2, `poll` is present exactly
because 4.7 is the sole `request_poll`, and each `{?…}` request line matches its
`_query` key tuple exactly.

In place of the §5 run the specification cannot have (SaaS-only, correctly
declared — no §5 environment exists to stand up), the eight Python blocks were
extracted, compiled clean, and driven end to end against a local stub reproducing
Zendesk's documented shapes and statuses
(`/tmp/claude-0/-home-user-resonate-plugins/ac146825-dddc-568c-bcf5-ab898c73ec61/scratchpad/mock.py`
and `drive.py`). Confirmed there: all eight verdicts match their Resolved schemas;
at least one rejection code per operation fires — `invalid_request` on create for
both a `400 ParameterMissing` and a `422 RecordInvalid` (last round's mislabelled
`conflict` is gone and the enum no longer carries it), `not_found` on
get/update/delete/merge/comments, `conflict` on a `safe_update` collision,
`invalid_request` on a `type:` term in the export query and on a bad comments
cursor, `job_not_found` on a job that vanishes mid-poll, `merge_failed` and
`killed` on terminal job states; `ticket.create` re-entered with the same promise
id returns the same ticket id rather than creating a second one, with
`external_id == sanitize(promise.id)`; the poll loop's new permanent-status
residue now settles `invalid_request` on a `400` from
`GET /api/v2/job_statuses/{id}` instead of dying on `KeyError` (last round's
finding 9, verified fixed); a 5xx inside that loop raises `release` after the
five-failure cap; `401`/`403` raise `halt` and `429` raises `release`; and a past
`timeout_at` raises `("release", "promise timed out")`. Booleans render as
`"true"`/`"false"` on the wire (observed: `include_inline_images=true`). This stub
proves the code's control flow, not Zendesk's behaviour — every status it returns
is one the document claims.

Three claims stayed unverified and no finding is raised on them: that Show Ticket
accepts `include` at all and rejects an unknown sideload name (the sideloading page
is behind the blocked host and the OpenAPI models no `include` on Show Ticket); the
`page[after]`/`page[before]` cursor parameters on `GET /api/v2/tickets` and
`GET /api/v2/tickets/{id}/comments` (documented in Zendesk's pagination guide,
absent from the OpenAPI parameter lists); and the claim that a List Tickets or
List Comments cursor can *expire* — the export endpoint's one-hour expiry is
documented as a difference from ordinary cursor pagination, which suggests
ordinary cursors do not expire, but absence of documentation does not prove it.
Two behaviours were re-confirmed as disclosed provider limitations rather than
defects: `ticket.delete` re-entered after a successful delete rejects `not_found`
(executed against the stub; `GET /api/v2/deleted_tickets` takes only
`sort_by`/`sort_order` per the OpenAPI, so no id-filtered read and no higher
Invocation rung exists, and 4.6.1 states the risk), and `ticket.merge`
re-delivery queuing a second merge (4.7.1 states it). `ticket.create`'s
`fetch_then_create` label now matches its code and the combination is sound under
both re-delivery and concurrent delivery (the `Idempotency-Key` still covers the
race inside the two-hour window; the `external_id` lookup covers it after).

## Findings

### minor

**1. §4.7.1 — the merge status rules are wrong on the target side and probably too
narrow on the source side.**
The `ticket_id` description says "A solved or closed ticket cannot receive a
merge." Zendesk's own merging rules — the page the merge endpoint's OpenAPI
description defers to for "ticket merging rules"
(`https://support.zendesk.com/hc/en-us/articles/4408882445594-Merging-tickets`) —
say the opposite about Solved: *"The tickets must be less than Solved, though you
can merge an unsolved ticket into a Solved ticket. Merging an unsolved ticket into
a Solved ticket will not reopen the Solved ticket."* Only **Closed** is barred as
a merge target. As written, the specification tells a caller that a merge Zendesk
accepts is impossible. (The host is blocked from this session — the quotation is
from three independent search retrievals of that article, two of them scoped to
`support.zendesk.com`, all agreeing verbatim.) Fix: `"The target ticket, which
receives the merged comments. A closed ticket cannot receive a merge; a solved
ticket can, and the merge does not reopen it."`

Secondary, same sentence pair: the `ids` description enumerates the source rule as
`"new", "open" or "pending", or a custom status in one of those categories`, but
the documented rule is *less than Solved*, and Zendesk's standard status ladder is
New → Open → Pending → **On-hold** → Solved → Closed (the `hold` value the
document's own 4.1.4 `status` enum carries). Unless Zendesk singles `hold` out,
the enumeration wrongly excludes it; "must be below \"solved\"" alone — the phrase
the sentence already opens with — is both correct and sufficient, so the safest
fix is to drop the three-value list. Lower confidence than the target-side error:
the article's exact treatment of On-hold could not be read directly.

### nit

**2. §4.7.4 — the `killed` job state contradicts the OpenAPI, and the OpenAPI is
wrong.**
Reported as its own finding per the evidence-ordering rule, unchanged from last
round and still live in the document. `components.schemas.JobStatusObject.properties.status`
carries no `enum`, only the sentence `"The current status. One of the following:
\"queued\", \"working\", \"failed\", \"completed\""` — four states. Zendesk's Job
Statuses doc page
(`https://developer.zendesk.com/api-reference/ticketing/ticket-management/job_statuses/`,
confirmed by search: the status is one of "queued", "working", "failed",
"completed", or "killed") documents five. The specification follows the doc page
and on the merits is right — the OpenAPI's prose sentence is not a machine-readable
enum and is stale — so no change is needed, but `JOB_TERMINAL` and the `killed`
rejection code both rest on the weaker of two disagreeing sources and that belongs
on the record.

**3. §4.2.1 and §4.3.1 — the `include` sideload enumeration is incomplete.**
Both descriptions present a closed list — "users, groups, organizations,
metric_sets and slas … comment_count" — and 4.2.1/4.3.1 say an unknown sideload
name in `include` is rejected, so the list reads as the accepted set. `last_audits`
is a documented ticket sideload that is not in it (Zendesk's Incremental Exports
page, `https://developer.zendesk.com/api-reference/ticketing/ticket-management/incremental_exports/`,
says the `last_audits` sideload is not supported *on incremental endpoints for
performance reasons* — which presupposes it is supported on the ordinary ticket
endpoints these two operations call). The OpenAPI models no ticket sideloads at
all and `developer.zendesk.com` is blocked here, so the full list could not be
recovered; the fix is either to complete it from the Tickets page's Sideloads
table or to stop presenting it as exhaustive. (The `comment_count` half of the
sentence is now correct — it reads "a property added to the ticket object itself",
matching Zendesk's "you can include a `comment_count` property in the JSON objects
returned by GET requests by sideloading it". Last round's finding 5 is fixed.)

**4. §4.1.1 — `collaborators` is missing from the create param schema.**
`TicketCreateInput` carries `collaborators` (`"POST requests only. Users to add as
cc's when creating a ticket"`, items `{name, email}` per `CollaboratorObject`),
and it is the only way to CC someone at create time who is not already a user with
a known id. 4.1.1 exposes its sibling `collaborator_ids` and the update-side
analogue `additional_collaborators` appears in 4.5.1, so the omission looks like an
oversight rather than a bound. Fix: add
`"collaborators": {"type": "array", "description": "Users to add as CCs when creating the ticket, as ids, email addresses, or objects with name and email.", "items": {}}`
to 4.1.1 and the matching `= promise.param.collaborators` to 4.1.3.

## Verdict

**approve** — minor and nit findings only; no blocking or significant finding.
All ten findings from the previous round were applied and verified fixed:
`ticketcomment.list` closes the comment-thread coverage gap, `ticket.create` no
longer mislabels every `400` as `conflict` (the code is gone from the enum too)
and its Invocation cell now reads `fetch_then_create` matching its code, `job.get`
and its disclaimer are gone, `comment_count` is described as a property,
the `Sources` header row is removed, `page[before]` is now a real parameter on
4.3, and the merge poll loop has its permanent-status residue. The one applied fix
that introduced a defect is the merge status sentence (finding 1) — the
source-side constraint was added correctly-in-spirit but the target-side sentence
it sits beside is contradicted by the same Zendesk article. Note that no operation
was executed against the live provider: every `*.zendesk.com` host is blocked by
this session's egress policy and no credentials were available, so wire behaviour
remains unverified.
