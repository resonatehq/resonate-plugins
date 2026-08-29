# Review trail — n8n plugin specification

**Final verdict: approve** (after 2 rounds)

Round 1 raised a blocking defect (a permanent 500 on a `loadWorkflow: true` retry
classified as `release`) together with several coverage gaps; those were fixed and
re-executed in round 2. The round 2 findings report follows verbatim.

---

# 0. API Surface

Derived from the OpenAPI document this instance serves at `{base_url}/api/v1/openapi.yml` (n8n Public API 1.1.1 on n8n 2.36.8) and from `GET /api/v1/discover`, before reading §4. `docs.n8n.io` is outside this session's egress policy, so no narrative page was used.

**Workflow — the primary resource and its lifecycle**

- `POST /api/v1/workflows` — create a workflow from `name`/`nodes`/`connections`/`settings`; the instance assigns the id.
- `GET /api/v1/workflows` — page the workflows to find an id, narrowed by `name`, `active`, `tags`, `projectId`.
- `GET /api/v1/workflows/{workflowId}` — read one workflow; its node/connection/settings shape is what an update has to send back.
- `PUT /api/v1/workflows/{id}` — replace a workflow's definition.
- `DELETE /api/v1/workflows/{id}` — hard-delete a workflow.
- `POST /api/v1/workflows/{id}/archive` / `POST /api/v1/workflows/{id}/unarchive` — the soft-delete pair, for a caller who wants a workflow out of the way without losing it; archive is documented idempotent.

**Workflow publication — the state that decides whether a workflow runs**

- `POST /api/v1/workflows/{id}/publish` — make a version live (what v1 called activating): the call a caller reaches for to turn a workflow on.
- `POST /api/v1/workflows/{id}/unpublish` — take it off.
- `/activate` and `/deactivate` are `deprecated: true` aliases of these two; a plugin carries the current names only, never both.

**Execution — the durable unit the provider offers**

- `GET /api/v1/executions` — page executions filtered by `workflowId`, `status`, `projectId`: the read that turns "did my workflow run" into an id.
- `GET /api/v1/executions/{id}` — read one execution: its `status`, and with `includeData=true` its run data. The get-by-id of the plugin's durable unit, and the only way to reach an execution a caller already holds an id for.
- `POST /api/v1/executions/{id}/retry` — re-run a failed execution, optionally against the workflow's current definition (`loadWorkflow`). The only call in the Public API that starts an ordinary workflow run, and it yields an execution whose status a caller follows to a terminal state — the one long-running unit a promise can await.
- `POST /api/v1/executions/{id}/stop` — cancel a run in flight.
- `DELETE /api/v1/executions/{id}` — delete an execution record.

**Peripheral**

- `GET /api/v1/workflows/{workflowId}/history` and `GET /api/v1/workflows/{id}/{versionId}` — the saved versions and one version's definition; wanted only by a caller that publishes a specific `versionId`.
- `POST|GET|PUT|DELETE /api/v1/tags…`, `GET|PUT /api/v1/workflows/{workflowId}/tags`, `GET|PUT /api/v1/executions/{id}/tags` — labels; a caller filtering `GET /workflows?tags=` has to have made them.
- `POST /api/v1/executions/stop` — bulk stop by filter, a fan-out convenience over the per-id stop.

**What the provider cannot support**

The Public API has no endpoint that starts a workflow run from its id: there is no `POST /workflows/{id}/execute` in the served `openapi.yml` path list, and `/discover` reports the workflow operations as create/update/delete/read/activate/deactivate/list only. Ordinary runs begin out of band — a webhook, a schedule, the editor — so no plugin can offer `workflow.run`, and `execution.retry` is the only invocation an operation can make. That is the provider's limit, not the specification's.

**Bounds excluded.** Administration: `/users`, `/projects`, `/roles`, `/role-mapping-rules`, `/variables`, `/credentials`, `/settings/*` (LDAP, SSO, OTel, security policy, log streaming), `/audit`, `/insights/summary`, `/source-control/pull`, `/community-packages`, `/n8n-packages/*`, `/projects/{id}/folders`, and the two `/transfer` calls. Transport and capability mechanics: `GET /api/v1/discover`, `GET /api/v1/openapi.yml`. `/data-tables/*` is a resource of its own; `/workflows/{id}/test-runs/*` is the Evaluation feature — a separate resource needing a configured evaluation trigger and a licensed instance (documented `402`) — not the workflow lifecycle this plugin is about.

## Coverage

§4 exposes fourteen operations — `workflow.create`, `.get`, `.list`, `.update`, `.publish`, `.unpublish`, `.archive`, `.unarchive`, `.delete`, `execution.list`, `.get`, `.retry`, `.stop`, `.delete` — and every one of them is a call §0 asks for. The workflow lifecycle (including the soft-delete pair and the publication state) is complete, and the provider's one durable unit is exposed with a get-by-id, a list, a cancel, a delete, and await semantics on the retry. Nothing in §4 is surface §0 does not want. Of §0's Peripheral group, tags are label administration and `POST /executions/stop` is a fan-out over the per-id stop already exposed, so neither is counted against a document already carrying fourteen operations; the versions read is the one peripheral gap counted below.

Executed against a live n8n 2.36.8 built from §5.1 and seeded by §5.2, driving the specification's own snippets: all fourteen operations, each resolved value matching its Resolved schema, and every `code` in every Rejected enum reached at least once — `not_found` (unknown workflow, unknown project, unknown folder, unknown version, unknown execution), `invalid_request` (missing required property, unknown property, read-only property, bad settings enum, bad `excludePinnedData`/`active` enum, bad cursor on both lists, `limit` > 250 on `/executions`, a bad execution id, publishing a trigger-less workflow, publishing/updating an archived one), `conflict` (webhook path held by another published workflow, on both `workflow.update` and `workflow.publish` — and the update's version was confirmed saved as a draft: `versionCounter` 1→2, `activeVersionId` unchanged, `/history` 1→2 entries), `not_retryable` (409 on retrying a succeeded execution), `not_stoppable` (500 then re-read on a terminal `error` execution), `not_deletable` (400 "Cannot delete a running execution"), `workflow_changed` (500 after removing the node the stopped execution stood on, with `loadWorkflow: true`; no orphan execution is created by that 500 — `/executions?workflowId=…` shows none in any status), `execution_failed`, and `deleted` (a `waiting` retry deleted mid-poll — a waiting execution *is* deletable, so the poll's GET answered 404 and the promise rejected `deleted`). Idempotency and re-entry: `workflow.publish`, `.unpublish` and `.archive` re-delivered to the same result; `workflow.unarchive`'s 400 recovery re-read and resolved; `execution.stop` re-delivered on an already-canceled execution resolved from the re-read; `execution.retry` re-entered with the same promise id found the existing retry by `retryOf` and started no second run (exactly one retry execution per source in every case). The durable path ran end to end: a retry whose remaining work took 25 s exceeded `timeout=10`, `requests.Timeout` fired, `_find_retry` located the running retry, the loop polled it to `success` and resolved after 30.1 s. `_check` raised `("halt", …)` on a bad key (401) and on a scope-limited key (403, minted with `workflow:list` only); an expired `timeout_at` raised `("release", "promise timed out")`. Two claims the implementation depends on were confirmed directly: `GET /executions` omits `running` executions unless `status=running` but *does* list `waiting` ones (so `_find_retry`'s running/any two-pass covers every state a retry can be in), and all fourteen read-only properties 4.2.1 names are rejected by `PUT /workflows/{id}`. Not exercised: `settings.redactionPolicy` below an instance floor (422) — the floor lives behind `/settings/security-policy`, which answers `403 "Your license does not allow for feat:personalSpacePolicy"` here — and the `conflict`/`not_retryable` variants that need a workflow review or a queued execution. Two environment notes, neither a defect: `docker build` of 5.1 fails here only because `pip install requests` cannot verify this sandbox's egress-proxy certificate, so I built an otherwise identical image with `--cert` pointed at the proxy CA in the `py` stage and ran everything downstream verbatim; and `python3 skills/plugin-specify/lint.py` passes with the single expected warning that `Reviewed by` is filled. The 5.2 serviceability block, re-run from a removed container, minted its key once and completed unattended.

## Findings

**minor — 4.14.4: the delete response's `id` is a number, not the string the reused 4.12.4 declares.**
4.14.4 is "Same as 4.12.4", whose `id` is `{"type": "string"}` with the note "The OpenAPI document types it as a number; the instance returns a string." That holds for the retry response and for `GET /executions/{id}`, but not for the delete. Executed: `DELETE /api/v1/executions/4` → `200 {"id": 4, …}` with `type(body["id"]) is int`; the same record read a moment earlier through `GET /api/v1/executions/4` carried `"id": "4"`. `execution.delete`'s Resolved value is `= response.body`, so the caller gets a number where the schema promises a string. Either give 4.14.4 its own schema for `id` or extend the note to say the delete response is the one place the number survives.

**minor — 4.1.4: `versionId` and `versionCounter` are described as things they are not.**
`versionId` — "Identifier of the workflow's latest saved version; advances on every update" — does not advance on every update. Executed: created a workflow (`versionId` fe471e06, `/history` 1 entry), `PUT` with only `name` changed → 200, `versionId` still fe471e06 and `/history` still 1; a second `PUT` that moved a node's `position` → `versionId` 78971506, `/history` 2. `versionCounter` — "Number of saved versions" — is not a count of versions either: create, archive, unarchive left `GET /workflows/{id}/history` with three entries while `versionCounter` was still 1. The shipped source is explicit about the rule: `@n8n/db/dist/migrations/sqlite/1784000000003-LimitWorkflowVersionTriggerToContent.js` recreates the `workflow_version_increment` trigger as `WHEN OLD.versionCounter = NEW.versionCounter AND (OLD.nodes IS NOT NEW.nodes OR OLD.settings IS NOT NEW.settings)`, i.e. it counts changes to `nodes` or `settings` only, starting at 1 (`ALTER TABLE … ADD COLUMN "versionCounter" integer NOT NULL DEFAULT 1`).

**minor — 4.1.4: `triggerCount` is not "Number of trigger nodes in the workflow".**
It counts the *active* triggers and is 0 while the workflow is unpublished. Executed: created a workflow with three Webhook nodes, one of them `disabled` — the create response and a subsequent `workflow.get` both carried `triggerCount: 0`; after `workflow.publish` the same workflow read `triggerCount: 2`. The provider's own text agrees: `{base_url}/api/v1/openapi.yml#/components/schemas/workflowCreate` describes `triggerCount` as "Number of active trigger nodes in the workflow".

**minor — §4 has no read of a workflow's saved versions; §0 "Peripheral — `GET /api/v1/workflows/{workflowId}/history`".**
4.5.1 offers `versionId` ("Version to publish. The workflow's latest version when omitted"), and 4.5.2 rejects `not_found` for an unknown one — executed: `POST /workflows/{id}/publish` with a made-up `versionId` → `404 "Version not found"`. But nothing in §4 can enumerate the versions, so the parameter only ever carries a `versionId` the caller happens to hold from a create/update response: publishing an *older* version, which is what the parameter exists for, is unreachable. Documented at `{base_url}/api/v1/openapi.yml#/paths/~1workflows~1{workflowId}~1history/get`; executed against this instance it returns `{data: [{versionId, workflowId, authors, name, description, createdAt, updatedAt}], nextCursor}` and needs no scope the §5.2 key lacks.

**nit — 4.10.4 and 4.12.4: the `includeData` field lists are short by two.**
4.10.4 says the run data "and the fields recorded with it — data, storedAt, jsonSizeBytes and workflowVersionId — are in an item only when includeData is set". Executed: `GET /api/v1/executions?limit=1` returns items keyed `finished, id, mode, retryOf, retrySuccessId, startedAt, status, stoppedAt, waitTill, workflowId`; with `includeData=true` the same item adds `customData` and `workflowData` as well as the four named. Relatedly, 4.12.4's `workflowData` is described as "In the retry response" only — executed, `GET /api/v1/executions/{id}?includeData=true` also carries it.

**nit — 4.12.4 omits `tracingContext` and `deduplicationKey`, which every execution read carries.**
Executed: `GET /api/v1/executions/1` returns `id, finished, mode, retryOf, retrySuccessId, status, createdAt, startedAt, stoppedAt, deletedAt, workflowId, waitTill, storedAt, tracingContext, deduplicationKey, jsonSizeBytes, binaryDataSizeBytes, workflowVersionId, usedPrivateCredentials` — the last two of the first fifteen being the two 4.12.4 does not declare; the delete response carries them too. `execution.get` and `execution.delete` resolve with the whole body, so both reach the caller.

**nit — 4.2.1: the get→update round-trip instruction is incomplete; `description` comes back `null` and `PUT` rejects `null`.**
4.2.1 tells the caller that "that body carries only writable properties, so the read-only ones in this response (id, active, activeVersionId, versionId, versionCounter, isArchived, sourceWorkflowId, triggerCount, meta, tags, shared, activeVersion, createdAt, updatedAt) are dropped before it is sent back" — all fourteen confirmed rejected by `PUT` (executed, one property at a time). But a workflow with no description reads back `"description": null`, and `description` is not in that list: executed, `PUT /api/v1/workflows/{id}` with `description: null` → `400 {"message":"request/body/description must be string"}`, so the round trip the sentence prescribes still rejects `invalid_request`. Either add `description` (when null) to the properties to drop, or note that 4.4.1's `description` is non-nullable on the wire.

**nit — 4.4.5: the re-delivery comment states a provider fact that is false.**
"Re-delivery re-sends the same definition: n8n stores another version of the same content, and the workflow ends in the state this body describes." The second clause holds; the first does not — n8n's version trigger fires on changed `nodes`/`settings` only. Executed: `PUT` repeating the stored definition byte for byte left `/workflows/{id}/history` at three entries and `versionId` unchanged, while the next `PUT` with a moved node added one.

**nit — 4.12.5: every 500 on a `loadWorkflow: true` retry settles as `workflow_changed`, including a transient one.**
The permanent case is real and now handled inline before `_check`, which is the right shape — executed: node removed from the current definition, `POST /api/v1/executions/6/retry {"loadWorkflow": true}` → `500 {"message":"Internal server error"}`, rejected `workflow_changed`, and no execution is started by that 500. The residue is that n8n returns the same opaque body for any unhandled error and the OpenAPI documents no 500 for this call at all (`{base_url}/api/v1/openapi.yml#/paths/~1executions~1{id}~1retry/post` lists 200, 401, 404, 409), so a database hiccup during a `loadWorkflow` retry settles the promise permanently with a reason that is not true. Worth a sentence in 4.12.1 owning that, or a discriminator before the rejection — the source execution's `data.resultData.lastNodeExecuted` against the current definition's node names separates the two cases without another write.

## Verdict

**approve** — no blocking or significant finding. Round 1's blocking defect (a permanent 500 classified `release`) and its coverage gaps are fixed and were re-executed here; what remains is four wrong or missing declared facts and five points of incompleteness, all fixable in place.
