# Review trail — Rundeck plugin

**Verdict: approve** — after 2 round(s) of review.

---

# 0. API Surface

Derived from `https://raw.githubusercontent.com/rundeck/docs/6.2.0/docs/.vuepress/public/files/rundeck-api.yml`
(`openapi: 3.0.1`, `info.version: "59"`) and
`https://raw.githubusercontent.com/rundeck/docs/6.2.0/docs/api/index.md` (the
API Reference page's own source). The canonical
`https://docs.rundeck.com/docs/files/rundeck-api.yml` and
`https://docs.rundeck.com/docs/api/` are both unreachable from this
environment — re-confirmed this round: `curl` returns
`CONNECT tunnel failed, response 403` for each — which is what the header's
Notes rows already say, naming the mirrors that were read instead.

**Primary resource: the execution** — Rundeck's durable, named unit of work. A
job is its definition; a project is the scope that names both.

**Executions**

- `POST /api/59/job/{id}/run` — start an execution of a job, with option
  values, a node filter, a log level or a future `runAtTime`; answers
  immediately with the new execution record, `status: "running"` or
  `"scheduled"`.
- `POST /api/59/job/{id}/retry/{executionId}` — re-run a job from a prior
  execution, inheriting its option values, optionally restricted to the nodes
  that failed.
- `GET /api/59/execution/{id}` — read one execution's record and its status;
  the read a caller uses to see how a run ended.
- `GET /api/59/project/{project}/executions` — query a project's executions by
  status, job, user, time window, node or option value, paged by
  `max`/`offset`; the read that finds an execution id.
- `GET /api/59/project/{project}/executions/running` — list what is running now
  in one project, or in all projects with `*`; takes `jobIdFilter`,
  `includePostponed`, `max` and `offset`.
- `GET /api/59/job/{id}/executions` — the executions of one job, filtered by
  `status` and `includeJobRef` and paged; the same list scoped to a job.
- `POST /api/59/execution/{id}/abort` — stop a running execution; answers with
  an abort state of `pending`, `failed` or `aborted`.
- `DELETE /api/59/execution/{id}` — delete one execution and its log.
- `POST /api/59/executions/delete` — delete a set of executions by id (the
  batch variant of delete).
- `DELETE /api/59/job/{id}/executions` — delete every execution of one job (the
  job-scoped variant of delete).
- `GET /api/59/execution/{id}/output` — read the execution's log output, whole
  or tailed by `offset`/`lastlines`/`maxlines`, narrowable by
  `nodename`/`stepctx`; the run's result, published separately from its record.
- `GET /api/59/execution/{id}/state` — read per-step, per-node workflow state
  for a running or completed execution.

**Jobs** — what an execution runs; the reads that name one and constrain
`run`'s arguments.

- `GET /api/59/project/{project}/jobs` — list a project's jobs, filtered by
  name, group, tags or schedule state; the read that finds a job id.
- `GET /api/59/job/{id}` — read one job definition (json, yaml or xml), whose
  `options` constrain what `run` may pass.
- `GET /api/59/job/{id}/info` — job metadata, including the owning project and
  the next scheduled run time (peripheral).
- `GET /api/59/job/{id}/forecast` — a job's forecast schedule (peripheral).

Job **create/update/delete** (`POST /api/59/project/{project}/jobs/import`,
`DELETE /api/59/job/{id}`, `POST /api/59/jobs/delete`) and the
enable/disable toggles are definition management: the payload is a whole
job-definition document in Rundeck's own XML/YAML/JSON format, authored in the
UI or synced from SCM, which plugin-specify's Bounds put out of band. Not a
finding either way — omitting them is right; including a `job.delete` or a
`job.create` would be **minor** surface §0 does not want.

**Projects** — the scope every execution query is addressed to.

- `GET /api/59/projects` — list projects, with optional `meta`; the read that
  supplies `{project}` for the execution queries and job list.
- `GET /api/59/project/{project}` — read one project's info and config
  (peripheral).

Project creation and deletion, ACLs, tokens, users, roles, SCM, plugins,
calendars, licence, runners, webhooks, key storage, system config, log storage,
metrics, healthchecks, tours, project archive export/import and execution mode
are instance administration and are out. The ad-hoc runs
(`POST /api/59/project/{project}/run/command`, `/run/script`, `/run/url`) are
the same execution machinery with the definition inlined; Rundeck has a named
unit — the job — so Bounds excludes them.

**Lifecycle of the primary resource (execution)**

| verb | provider endpoint | in §4 |
|---|---|---|
| create | `POST /api/59/job/{id}/run` (also `POST /api/59/job/{id}/retry/{executionId}`) | yes — 4.1 `job.run` (submits and waits), 4.2 `job.submit`, 4.3 `job.retry` |
| read | `GET /api/59/execution/{id}` | yes — 4.4 `execution.get` |
| list | `GET /api/59/project/{project}/executions` | yes — 4.5 `execution.list` |
| update | — (an execution's only mutation is `POST /api/59/execution/{id}/abort`) | yes — 4.8 `execution.abort` (submits and waits) |
| delete | `DELETE /api/59/execution/{id}` (batch: `POST /api/59/executions/delete`; job-scoped: `DELETE /api/59/job/{id}/executions`) | yes — 4.9 `execution.delete`; batch and job-scoped folded, and 4.9.1 says so |

`job/{id}/run` is a submit that answers immediately with a `running` execution,
which the caller then polls at `GET /execution/{id}` until its status is
terminal. That split is HTTP's, not the caller's: §4 should answer the create
row with one operation that submits and waits. The same holds for `retry`, and
for `abort`, whose own response can be `pending`.

## Coverage

Every lifecycle row is answered, and answered the right way: `job.run`,
`job.retry` and `execution.abort` each collapse Rundeck's submit-and-poll split
into a single waiting operation, and `job.submit` is the sanctioned
fire-and-forget variant beside `job.run`, not instead of it. The job-scoped
execution list, the running list (now including the cross-project `*` form),
the batch and job-scoped deletes and the node/step-narrowed output reads are
folded into the verbs they implement, with disclaimers where a reader could
mistake a read for a completion mechanism. §4 exposes nothing §0 does not want.
The two peripheral reads §0 names, `GET /job/{id}/info` and
`GET /job/{id}/forecast`, are absent as caller-facing operations (`info` is used
internally by `job.run`, `job.submit` and `job.retry` to resolve a project);
`job.get` and `job.list` cover what a caller needs from a job, so I accept the
omission and raise nothing. `job.list`'s exposed filter set is exactly the one
`index.md` documents.

Executed against `rundeck/rundeck:6.1.0` from §5 (`apiversion: 59` on the wire),
5.1 written to `spec/Dockerfile` and 5.2 run unattended: image built, provider
came up serviceable, both fixtures imported and exported; extra fixtures
(`fixture-req` with a `required`/`enforced` option, `fixture-disabled` with
`executionEnabled: false`, `fixture-single` with `multipleExecutions: false`,
`fixture-failopt` with two defaulted options) seeded to reach the rejection
codes. Every operation was called with the specification's §4 Python extracted
verbatim. Confirmed: `project.list` (bare and `meta=*`), `job.list`, `job.get`
(one-element array), `execution.get`, `execution.list` (project-scoped, array
filters as repeated keys, `adhoc` rendered `false`, `statusFilter` outside the
four-value enum matching nothing rather than erroring, and the new
`project: "*"` branch), `execution.output` (whole, `stepctx`-narrowed —
`stepctx=2` returned only the step-2 entry and echoed `filter`,
`nodename`-narrowed — a non-matching node returned zero entries, `lastlines`,
`compacted=true` returning object entries), `execution.state`,
`execution.delete`, `job.run` (options map, `argString`, and bare), `job.submit`,
`job.retry`, `execution.abort`. Re-entry with the same promise id produced no
duplicate execution for `job.run`, `job.submit` or `job.retry`, including for a
`scheduled` execution parked on `runAtTime`; re-aborting settled identically.
The `rescorr` mechanism holds across the whole `sanitize` yield the plugin
contract specifies (17–117 chars of `[A-Za-z0-9._-]`): tokens of 33, 40 and 117
characters, one with a leading `-` and dots, survived both the `options`-map and
the `argString` path, landed in `job.options.rescorr` byte-for-byte and were
recovered by `_find`. Rejection codes exercised live: `job_not_found`,
`invalid_request` (400 `api.error.job.disabled`; 400
`api.error.job.options-invalid` for both a non-allowed value and a missing
required option), `execution_failed`, `execution_deleted` (raced by aborting
then deleting mid-poll), `not_found` on all five reads and on delete,
`execution_not_found`, and the new `execution_not_retryable`. §4.3's
three-way 404 discrimination — the round-1 blocking finding — is fixed and
correct on every branch I could construct: retrying a succeeded execution →
`execution_not_retryable` ("Failed node List for execution ID does not exist:
1"), an unknown job id → `job_not_found`, an unknown execution id →
`execution_not_found`, an execution belonging to a different job →
`execution_not_retryable`, `failedNodes: false` on a succeeded execution →
`execution_not_retryable`, and a genuinely retryable failed execution →
resolved through to its terminal state. `_check`'s halt set is right: an absent
or bogus token returns **403** with `errorCode: unauthorized` on every endpoint
tried — Rundeck never answered 401. The 409 it releases on is real
(`api.error.execution.conflict` on `multipleExecutions: false`), and `DELETE` of
a running **or scheduled** execution answers **500**
(`api.error.exec.delete.failed`), so 4.9.1's release-and-retry is correct on
both. 4.3.1's two retry claims were re-confirmed on the wire: a JSON-body
`argString` is ignored (the retry came back `-a 1 -b 2`, the job's defaults,
losing the prior execution's `-a AA -b BB`), while `options` merge over the
prior execution's with the explicit value winning (`-a MERGED -b BB -rescorr …`).
Not reachable with documented features and therefore unverified: `abort_failed`
(every abort of a live execution returned `pending` or `aborted`; `failed`
appeared only on already-terminal executions, where the code's
`status not in TERMINAL` guard correctly declines to reject).

## Findings

### minor

**§4.5.1 / §4.5.5 — the cross-project running list silently drops `max` and
`offset`, on a claim the provider contradicts.** 4.5.1 says of `project: "*"`
that "that request returns the running executions of every project and takes
`jobIdFilter` alone; every other filter is dropped for it", and 4.5.5's `*`
branch sets `keys = ("jobIdFilter",)` accordingly. The endpoint takes more than
that. The OpenAPI's `get /project/{project}/executions/running` declares `max`,
`offset`, `jobIdFilter` and `includePostponed` (the last "If true, include
scheduled and queued executions. Since: v32"), and executed evidence agrees —
raw:

```
GET /api/59/project/*/executions/running?max=1          → 200 paging {'count':1,'total':1,'offset':0,'max':1}
GET /api/59/project/*/executions/running?max=1&offset=1 → 200 paging {'count':0,'total':1,'offset':1,'max':1}
```

Through the operation, with three executions running, the same values are
discarded:

```
execution_list(cfg, promise{"project":"*","max":1})            → paging {'count':3,'total':3,'offset':0,'max':20}
execution_list(cfg, promise{"project":"*","max":1,"offset":1}) → paging {'count':3,'total':3,'offset':0,'max':20}
```

So `max` and `offset`, both declared in 4.5.1's Param Schema with no
per-branch caveat, are accepted and ignored: a caller cannot page the
cross-project running list, and cannot reach `includePostponed` at all. Fix:
add `"max"` and `"offset"` (and, if wanted, `includePostponed`) to the `*`
branch's `keys`, and correct the sentence — it is `jobIdFilter` plus the paging
parameters that survive, not `jobIdFilter` alone.

### nit

**§4.8.4 — `abort.reason` is not only present when `status` is `failed`.** The
Integration Response declares `"reason": { "description": "Present when status
is failed." }`. Executed against a running execution with the `forceIncomplete`
parameter 4.8.1 exposes:

```
POST /api/59/execution/19/abort?forceIncomplete=true
→ 200 {"abort": {"status": "pending"}, ...}          (first call)
→ 200 {"abort": {"status": "aborted", "reason": "Marked as incomplete"}, ...}
```

`reason` accompanies an `aborted` status too. The implementation only reads it
on the `failed` branch, so nothing settles wrongly; the declared constraint is
just narrower than the provider's behaviour.

**Source contradiction — the abort method (unrecorded, carried over from the
previous round).** `docs/api/index.md` line 4544 documents
`GET /api/{{ $apiMinVersion }}/execution/[ID]/abort`; the OpenAPI defines only
`post` on `/execution/{id}/abort` (`operationId: apiExecutionAbort`). Both work
live. §4.8.3's `POST` is the right choice by the OpenAPI-beats-narrative rule,
but the specification still presents it without noting that the reference page
names a different method, so a reader checking §4.8 against the linked
Documentation row finds a mismatch with no explanation. The preparation's Open
questions raised exactly this and asked that it be settled visibly.

## Verdict

**approve** — the round-1 blocking finding (`job.retry` settling
`job_not_found` for a job that exists) is fixed and its replacement discriminates
correctly on every reachable branch, and the four round-1 minors and two of the
nits are resolved. What remains is one minor (two documented paging parameters
silently dropped on the cross-project running list, on a claim the OpenAPI and
the wire both contradict) and two nits.

## Re-attestation

Checked against `/tmp/resonate-plugin-factory/rundeck/plugins/rundeck/spec/specification.md`
as it now stands. Lint re-run: `0 errors, 2 warnings` — L-14 ("Reviewed by is
filled") is expected post-review and is caused by the attestation row itself,
and L-19 (a `create` operation polls without a consecutive-failure counter) is
the pre-existing `_await` design that releases rather than counting; neither is
new, and neither is touched by these fixes, which changed one `call` read and
two pieces of descriptive text. The Reviewed by row now reads
`Claude Opus 5, 2026-08-31`.

**minor — §4.5.1 / §4.5.5 cross-project running list drops `max` and `offset`:
applied, and I agree with how.** 4.5.5's `*` branch is now
`keys = ("jobIdFilter", "includePostponed", "max", "offset")` (line 1168), the
4.5.3 request template line reads
`GET /api/59/project/*/executions/running{?jobIdFilter,includePostponed,max,offset = promise.param.*} → 200`
(line 1123), and 4.5.1's sentence is corrected to "takes `jobIdFilter`,
`includePostponed`, `max` and `offset`; every other filter is dropped for it"
(line 967) — the false "`jobIdFilter` alone" is gone. The fix went further than
the minimum in the right direction: `includePostponed` was taken up as well and
declared as a new param, and both branch-only params carry an explicit caveat
naming what the project-scoped query uses instead (`jobIdFilter` → "The
project-scoped query narrows by job with `jobIdListFilter` instead";
`includePostponed` → "The project-scoped query selects them with `statusFilter`
instead"), so the schema no longer promises a parameter on a branch that ignores
it. `max` and `offset` correctly keep their un-caveated descriptions, since they
now apply on both branches. Nothing adjacent broke: the project-scoped `keys`
tuple is unchanged and still lists all 24 filters, `required` is still
`["project"]`, and the new boolean `includePostponed` is safely rendered by the
existing `_query` helper, which maps `True`/`False` to the strings `"true"` and
`"false"` — the same path `adhoc` was executed on last round. The grep for
`running list` / `executions/running` turns up no surviving copy of the old
claim anywhere else in the document.

**nit — §4.8.4 `abort.reason` presence: applied, and I agree with how.** The
description is now "Why the abort ended in this status. Accompanies `failed`
(\"Job is not running\"), and `aborted` when the execution was marked incomplete
(\"Marked as incomplete\"). Absent from `pending`." That matches the executed
evidence in the finding on all three statuses, and it adds the `pending`
absence, which the old text only implied. The sibling `status` description and
the 4.8.2 `detail` row (`abort_failed: = response.body.abort.reason`) still
agree with it, and 4.8.5 still reads `reason` only on the `failed` branch via
`.get`, so the widened declaration does not change what settles.

**nit — source contradiction on the abort method: applied, and I agree with
how.** §4.8.5 now opens with "The OpenAPI defines only POST on this path
(`operationId apiExecutionAbort`); the API Reference narrative writes it GET.
Both methods answer, and the OpenAPI is the authority." (lines 1656-1658). That
is the settlement the finding asked for: the reader who checks §4.8.3's `POST`
against the linked `index.md` now finds the discrepancy named and the choice
justified by the precedence rule. Placement is in the implementation prose
rather than beside 4.8.3, one subsection away, which I consider fine — it is the
first thing above the `requests.post` call it explains. The pre-existing note
about a repeated abort answering `failed`/"Job is not running" was kept intact
below it.

No fix was wrong and none introduced a defect. One observation, raised for the
record and not as a new finding since it predates this round and the previous
verdict stands: §4.8 still names its deletion rejection `deleted`
(4.8.2 enum, 4.8.5 line 1681) where §4.1/§4.2/§4.3 use `execution_deleted` — an
inconsistency untouched by, and unrelated to, the three findings applied here.

**Verdict: approve** — unchanged from the previous round.
