# Review — Baserow

**Verdict: approve** after 1 round.

---

# 0. API Surface

Written from Baserow's own OpenAPI document (`GET /api/schema.json`, `info.version` 2.3.3, served by the provider itself — `api.baserow.io` and `baserow.io` are blocked by this session's egress proxy, so the shipped spec of the §5 image is the fetched documentation) before reading §4.

**Rows — the primary resource; a table's rows are what a caller reaches for**

- `POST /api/database/rows/table/{table_id}/` — create one row from field values.
- `GET /api/database/rows/table/{table_id}/` — page a table's rows with search / filter / order / view; how a caller finds row ids.
- `GET /api/database/rows/table/{table_id}/{row_id}/` — read one row.
- `PATCH /api/database/rows/table/{table_id}/{row_id}/` — set field values on one row.
- `DELETE /api/database/rows/table/{table_id}/{row_id}/` — delete one row.
- `POST /api/database/rows/table/{table_id}/batch/` — create many rows in one call.
- `PATCH /api/database/rows/table/{table_id}/batch/` — update many identified rows in one call.
- `POST /api/database/rows/table/{table_id}/batch-delete/` — delete many rows in one call.
- `PATCH /api/database/rows/table/{table_id}/{row_id}/move/` — reposition a row before another (peripheral).

**Durable units — the async jobs Baserow names and retains**

- `POST /api/database/export/table/{table_id}/` → an export job, polled by `GET /api/database/export/{job_id}/` to `finished`/`failed`/`cancelled`/`expired`, yielding the exported file's url.
- `POST /api/database/tables/{table_id}/import/async/` → a `file_import` job, polled by `GET /api/jobs/{job_id}/` to a terminal state; the only bulk row load.

**Reads that find ids and constrain arguments**

- `GET /api/applications/` — the applications these credentials can see, with a database's tables inline; the entry read for database and table ids.
- `GET /api/database/tables/database/{database_id}/` — the tables of one database.
- `GET /api/database/fields/table/{table_id}/` — a table's fields (id, name, type, read_only); what constrains every row write and the column order of an import.
- `GET /api/database/views/table/{table_id}/` — a table's views; the view id row reads, row writes and exports take.

**Out by Bounds.** Workspaces, workspace users, invitations, teams, role assignments, settings, licences, admin, health, trash, snapshots (instance administration). Table / field / view create-update-delete and data-sync configuration (schema and definition management Baserow expects out of band). `POST /api/user/token-auth/` and the two job-polling reads themselves (transport mechanics). Row comments, row-name lookup, adjacent-row and row-history reads are peripheral audit/UI reads, not the surface a caller drives an integration with. Baserow offers no trigger endpoint and no server-side run other than the two jobs above, so nothing else in the API carries await semantics — that is a property of the provider, not a gap in the specification.

## Coverage

§4 covers §0 exactly on the calls a caller needs: all nine row calls, both durable jobs with their polls, and all four id/constraint reads are present with the right method and path (every doc link's `operationId` and tag were checked against the fetched schema and all 19 resolve to the endpoint the request line uses). Nothing §0 asks for is missing. §4 exposes three reads beyond §0: `export.get` (4.11) and `job.get` (4.13) — the reads of each action's own record, which plugin-specify explicitly admits and which carry the required "not the completion mechanism" disclaimer, so no finding — and `rowhistory.list` (4.18), which is peripheral surface (finding 6). At 18 operations the document sits well above plugin-specify's "expect 3–8", though the row lifecycle plus its batch trio accounts for most of it.

Executed against a live `baserow/baserow:2.3.3` built from the 5.1 block and seeded by 5.2 (fixtures `BASEROW_FIXTURE_OK=1`, `BASEROW_FIXTURE_FAIL=2`; both blocks ran unattended and left the provider serviceable). All 18 implementation snippets were pasted verbatim with `sanitize`, a `SimpleNamespace` `cfg` from the §2 table and a `promise` with `timeout_at`, and every operation was called: all resolved values matched their Resolved schema (including the explicit `keys` projections of 4.10 and 4.12), and at least one documented rejection per operation was reproduced — `table_not_found`, `row_not_found`, `view_not_found`, `database_not_found`, `job_not_found`, `row_ids_not_unique`, `invalid_request` (page size > 200, unknown order_by field, empty `items`/`data`, non-numeric value into a number field). `table.import` re-entry with the same promise id found the stamped job by `original_file_name` and re-observed it without a second import (7 rows before and after; `GET /api/jobs/` honours `type`/`limit`(max 100)/`offset` and orders `-id`, newest first). `export.create` re-entry created a second export, as its comment discloses. Halt paths confirmed live: `json`/`xml`/`excel`/`file` exports answer `402 ERROR_FEATURE_NOT_AVAILABLE` on an unlicensed instance and the snippet raised `("halt", …)`; a second account not in the workspace gets `400 ERROR_USER_NOT_IN_GROUP` on the row, field, view and table reads, which `_check` also halts on. Constants were checked against the container's `config/settings/base.py`: `ROW_PAGE_SIZE_LIMIT` 200, page size default 100, `BATCH_ROWS_SIZE_LIMIT` 200, `BASEROW_ACCESS_TOKEN_LIFETIME_MINUTES` 10, `EXPORT_FILE_EXPIRE_MINUTES` 60, `BASEROW_JOB_EXPIRATION_TIME_LIMIT` 30 days, `BASEROW_JOB_SOFT_TIME_LIMIT` 30 minutes; row-history `default_limit`/`max_limit` both 200. The export-cancellation claim was reproduced at the API level (two exports created back to back: the first went to `cancelled`, the second `finished`), confirming `_cancel_unfinished_jobs` in `contrib/database/export/handler.py`. `python3 skills/plugin-specify/lint.py` passes clean; every `= promise.param.x`, `= response.body.x`, `code`, and `cfg.<key>` was cross-checked by hand and all 22 `requests` calls carry `timeout=10`. Not reachable in this environment and therefore unverified: `export.create`'s `expired` (needs a 60-minute wait), `export.create`/`table.import`'s in-loop `job_not_found` (needs the job to vanish mid-poll), `table.import`'s `import_failed` (over-wide and type-incompatible rows both still reach `finished` with `report.failing_rows`), `database.list`'s `invalid_request`, and the 429/5xx release paths.

## Findings

**minor — §4.1.1, `search_mode` default is wrong.** The description says "full-text-with-count when omitted"; the default is `compat`. `search_mode` is read as `query_params.get("search_mode")` (`contrib/database/api/rows/views.py:398`) and falls through to `search_mode or settings.DEFAULT_SEARCH_MODE` (`contrib/database/table/models.py:176`), with `DEFAULT_SEARCH_MODE = os.getenv("BASEROW_DEFAULT_SEARCH_MODE", "compat")` (`config/settings/base.py:1404`). Executed: `GET /api/database/rows/table/1/?search=lph` → `count 1`, `…&search_mode=compat` → `count 1`, `…&search_mode=full-text-with-count` → `count 0` (only the substring/LIKE mode matches "lph" inside "Alpha"). A caller who omits the argument gets legacy substring search, not full text.

**minor — §4.16.4, the field `type` enum has a member the provider does not register and misses one it does.** `data_sync` is not a field type: the fetched schema's `FieldField` is a 28-member `oneOf` ending `…MultipleCollaboratorsFieldField, UUIDFieldField, AutonumberFieldField, PasswordFieldField, FormViewEditRowFieldField, AIFieldField`, and `grep 'type = "'` over `contrib/database/fields/field_types.py` in the container lists `… uuid autonumber password form_view_edit_row` with no `data_sync`. Replace `"data_sync"` with `"form_view_edit_row"`.

**minor — §4.7.4, the metadata key is `updated_field_ids`, not `update_field_ids`.** Executed `rows.create` with `include_metadata: true` returned `{"items": […], "metadata": {"updated_field_ids": [1, 2]}}`; the shipped source builds the key as `updated_field_ids` (`contrib/database/api/rows/serializers.py:51,251`; `api/rows/views.py:184`) and adds `cascade_update` only when a cascading update occurred (`views.py:188-191`). The OpenAPI's `include_metadata` prose says `update_field_ids` — a provider-documentation error the description copied; executed evidence and the shipped source agree against it. §4.8.4 inherits the error through "Same as 4.7.4".

**minor — §4.1.1 / §4.1.2, `field_not_found` cannot be produced by this operation as specified.** Executed: `order_by=field_9999` → `400 ERROR_ORDER_BY_FIELD_NOT_FOUND`, `filters` naming field 9999 → `400 ERROR_FILTER_FIELD_NOT_FOUND` (both settle `invalid_request`, as the same description also says), and `include=field_9999` / `exclude=field_9999` → `200`. The endpoint's documented `404 ERROR_FIELD_DOES_NOT_EXIST` is reachable only through the `{link_row_field}__join={target_field},…` query parameter that the fetched schema lists but §4.1.1 does not expose. Either expose the join parameter or drop `field_not_found` from the enum and from the first sentence of the description.

**minor — §4.5.1, `already_deleted` is not what a re-delete produces.** The description says the operation rejects "already_deleted when the row is already in the trash". Executed: `row.delete` on row 5 → `("resolved", {})` (204); the same call again → `404 ERROR_ROW_DOES_NOT_EXIST` → `("rejected", {"code": "row_not_found", …})`. A trashed row is filtered out of the table's base queryset, so the 404 always wins; the documented `400 ERROR_CANNOT_DELETE_ALREADY_DELETED_ITEM` in the endpoint's error enum is not reached for rows. The same sentence in §4.9.1 has the same problem (`rows.delete` on already-deleted ids 6 and 7 → `row_not_found`).

**minor (coverage) — §4.18 `rowhistory.list` is surface §0 does not ask for.** It is not an id-finding read, not a shape-constraining read, and not the read of an action's own record; it is a per-row audit trail. Against §0's Reads and Bounds it is the one operation of the eighteen that a caller driving Baserow through durable promises would not reach for, and dropping it brings the document closer to plugin-specify's 3–8.

**nit — provider-documentation contradictions the specification silently resolves in favour of live behaviour (all three resolutions are correct; recording them as required).** (a) §4.12.3 writes `POST /api/database/tables/{table_id}/import/async/ → 200`, while the fetched OpenAPI documents `202` for that operation; live answers `200` with the job body. (b) `_find_import_job` reads `r.json()["jobs"]`, while the OpenAPI types `GET /api/jobs/` as a bare array; live answers `{"jobs": [...]}` (`api/jobs/views.py:84`). (c) `_check` halts on `402` for `ERROR_FEATURE_NOT_AVAILABLE`, while the export operation documents only `400`/`404`; live answers `402` for `json`, `xml`, `excel` and `file` on an unlicensed instance.

**nit — §4.3.1 points at the wrong counterpart.** "use rows.update for a write that must survive re-delivery unchanged" names the batch operation (4.8); the single-row counterpart of `row.create` is `row.update` (4.4).

**nit — §4.7.1 / §4.8.1 / §4.9.1 contradict their own `send_webhook_events` parameter.** Each description ends "This endpoint does not fire row-created/updated/deleted webhooks" while the same schema documents `send_webhook_events` as "false suppresses the table's webhooks for this write". Both facts are the provider's (the OpenAPI carries both the `**WARNING:** This endpoint doesn't yet work with row created webhooks` note and the flag), but the two sentences read as a contradiction in one document; say in the parameter description that the flag has no effect on this endpoint today.

**Verdict: approve.** No blocking or significant findings: every operation settled truthfully against the live provider, both `request_poll` operations bounded their loops on `timeout_at`, `table.import`'s `fetch_then_create` re-entry duplicated no external work, and every rejection reproduced carried the code its schema declares.
