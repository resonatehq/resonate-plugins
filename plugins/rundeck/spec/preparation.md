# Rundeck — preparation

| | |
|---|---|
| **Docs** | `https://docs.rundeck.com/docs/api/` — unreachable from here (egress proxy denies CONNECT, 403); read instead the docs site's own source, `https://raw.githubusercontent.com/rundeck/docs/6.2.0/docs/api/index.md` (230 KB, the full API Reference) |
| **OpenAPI** | `https://docs.rundeck.com/docs/files/rundeck-api.yml` — unreachable from here (CONNECT 403); read instead the same file from the docs repo, `https://raw.githubusercontent.com/rundeck/docs/6.2.0/docs/.vuepress/public/files/rundeck-api.yml` (504 KB, `openapi: 3.0.1`, `info.version: "59"`) — the file `docs/api/api-spec.md` embeds as `<rundeck-swagger-ui specFile="/files/rundeck-api.yml"/>` |
| **Primary resource** | `execution` |
| **Notes** | Version history: `https://raw.githubusercontent.com/rundeck/docs/6.2.0/docs/api/rundeck-api-versions.md`; its newest entry is **Version 59**, so paths below are quoted at `/api/59`. In the reference source the prefix is templated `/api/{{ $apiMinVersion }}`; endpoints that entered later carry their own literal version (`/api/12`, `/api/18`, `/api/24`) and are quoted as written. Branch `6.2.0`; both files are identical on the default branch `4.0.x`. The reference and the OpenAPI spec disagree on one method — see Open questions. Auth: `X-Rundeck-Auth-Token` header (or `authtoken` param); password session and Enterprise JWT bearer also documented. |

## Motions

| motion | what the caller is doing | provider endpoints |
|---|---|---|
| run a job to completion | trigger a saved job and get back what it did | `POST /api/59/job/[ID]/run`, `GET /api/59/execution/[ID]` |
| start a job, collect later | trigger now (or at `runAtTime`), redeem the result on another turn | `POST /api/59/job/[ID]/run`, `GET /api/59/execution/[ID]` |
| re-run what failed | retry a failed run on its failed nodes, or on the same node set | `POST /api/24/job/[ID]/retry/[EXECID]`, `GET /api/59/execution/[ID]` |
| check on a run | is it finished, and did it succeed | `GET /api/59/execution/[ID]` |
| read what a run printed | the log, live or after the fact, whole or per node/step | `GET /api/59/execution/[ID]/output` |
| find which step failed on which node | structured per-step, per-node workflow state | `GET /api/59/execution/[ID]/state` |
| stop a run | abort a run going wrong, and know when it has actually stopped | `GET /api/59/execution/[ID]/abort`, `GET /api/59/execution/[ID]` |
| find past runs | enumerate a project's or a job's runs, filtered | `GET /api/59/project/[PROJECT]/executions`, `GET /api/59/job/[ID]/executions` |
| clean up run records | delete an execution, or a batch of them | `DELETE /api/12/execution/[ID]`, `POST /api/12/executions/delete` |
| find the job and its options | discover projects, job UUIDs, and the options a job declares | `GET /api/59/projects`, `GET /api/59/project/[PROJECT]/jobs`, `GET /api/59/job/[ID]` |

## Surface

| operation | algebra | provider endpoints | resolves with |
|---|---|---|---|
| `job.run` | `call + poll` | `POST /api/59/job/[ID]/run`, `GET /api/59/execution/[ID]` | the finished execution record |
| `job.submit` | `call` | `POST /api/59/job/[ID]/run` | the execution handle — accepted, not finished |
| `job.retry` | `call + poll` | `POST /api/24/job/[ID]/retry/[EXECID]`, `GET /api/59/execution/[ID]` | the finished retry execution record |
| `execution.get` | `call` | `GET /api/59/execution/[ID]` | the execution record |
| `execution.list` | `call` | `GET /api/59/project/[PROJECT]/executions`, `GET /api/59/job/[ID]/executions` | `paging` + `executions` |
| `execution.output` | `call` | `GET /api/59/execution/[ID]/output` | log entries + read position and exec state |
| `execution.state` | `call` | `GET /api/59/execution/[ID]/state` | per-step and per-node workflow state |
| `execution.abort` | `call + poll` | `GET /api/59/execution/[ID]/abort`, `GET /api/59/execution/[ID]` | the execution record in a terminal state |
| `execution.delete` | `call` | `DELETE /api/12/execution/[ID]` | `204 No Content` |
| `job.get` | `call` | `GET /api/59/job/[ID]` | the job definition, incl. declared options |
| `job.list` | `call` | `GET /api/59/project/[PROJECT]/jobs` | job UUIDs + names, groups, project |
| `project.list` | `call` | `GET /api/59/projects` | project names |

### Poll terminal states

From the documented `status` enum (`docs/api/index.md`, "Listing Running Executions"):

| | statuses |
|---|---|
| terminal | `succeeded`, `failed`, `aborted`, `timedout`, `failed-with-retry`, `other` |
| non-terminal | `running`, `scheduled` |

* `failed-with-retry` — *this* execution is over; Rundeck starts a **separate** retry execution, which is not the id being polled.
* `scheduled` — an execution created with `runAtTime` sits here until that time.
* `other` — a custom exit status; the string is in `customStatus`.
* `execution.abort` polls the same set: usually settles `aborted`, but on a race can settle `succeeded`/`failed`/`timedout`/`other`.

### Scoped variants — folded into the operation above, not given rows

| variant | folded into | why |
|---|---|---|
| `POST /api/12/job/[ID]/executions` | `job.run` | same endpoint under an older alias |
| `GET /api/59/job/[ID]/executions` (`status`, `max`/`offset`, `includeJobRef` since v50) | `execution.list` | same verb, scoped by job; dispatch here when the caller passes a job id instead of a project |
| `GET /api/59/project/[PROJECT]/executions/running` (`PROJECT` may be `*`; `jobIdFilter` since v32) | `execution.list` | already reachable as `statusFilter=running` |
| `GET /api/59/execution/[ID]/output/node/[NODE]`, `.../output/step/[STEPCTX]`, `.../output/node/[NODE]/step/[STEPCTX]` | `execution.output` | same read, narrowed |
| `POST /api/12/executions/delete` (`ids`; returns `failures`/`failedCount`/`successCount`/`allsuccessful`/`requestCount`) | `execution.delete` | batch form of the same verb |
| `DELETE /api/12/job/[ID]/executions` | `execution.delete` | batch form of the same verb, scoped by job |
| `GET /api/18/job/[ID]/info` (job metadata incl. `project`, `averageDuration`) | `job.get`, `job.run` | needed internally to resolve a job's project; not a caller-facing operation |

### Parameters that matter

| operation | parameters |
|---|---|
| `job.run`, `job.submit` | JSON body (v14+): `options` map (v18+, overrides `argString`), `argString`, `loglevel` (`DEBUG`/`VERBOSE`/`INFO`/`WARN`/`ERROR`), `asUser`, `filter`, `runAtTime` (v18+, ISO-8601 with timezone). `meta.KEY` (v32+) is query-only — it has no JSON body form. |
| `job.retry` | `failedNodes` (`true`/empty = failed nodes only, `false` = same node set), plus `argString`/`loglevel`/`asUser`/`options`; anything unspecified inherits the prior execution's values |
| `execution.list` | `statusFilter`, `jobIdListFilter`, `userFilter`, `abortedbyFilter`, `recentFilter`/`olderFilter`/`begin`/`end`, `executionTypeFilter` (v20+), `adhoc`, `max`/`offset`; response carries a `paging` block |
| `execution.output` | `offset`, `lastlines`, `maxlines`, `lastmod`, `compacted` (v21+). Works on a running or a finished execution. **v59 changed `compacted=true`** to return every log entry as a consistent object, omitting only unchanged fields, instead of mixing bare strings and empty hashes; v58 and earlier keep the old behavior. |
| `execution.abort` | `asUser`, `forceIncomplete`; returns `{"abort":{"status":"pending\|failed\|aborted","reason":...},"execution":{...}}` |
| `execution.delete` | requires `delete_execution` in the `application` context |
| `job.get` | `format` = `json` (v44+), `xml`, `yaml` |
| `job.list` | `idlist`, `groupPath`, `groupPathExact`, `jobFilter`, `jobExactFilter`, `scheduledFilter`, `tags`, `max`/`offset` |

## Lifecycle — `execution`

| verb | provider endpoint | exposed as |
|---|---|---|
| create | `POST /api/59/job/[ID]/run`; `POST /api/24/job/[ID]/retry/[EXECID]` | `job.run`, `job.submit`, `job.retry` |
| read | `GET /api/59/execution/[ID]`; `GET /api/59/execution/[ID]/output`; `GET /api/59/execution/[ID]/state` | `execution.get`, `execution.output`, `execution.state` |
| list | `GET /api/59/project/[PROJECT]/executions`; `GET /api/59/job/[ID]/executions` | `execution.list` |
| update | `GET /api/59/execution/[ID]/abort` — the only state transition Rundeck offers on an execution; there is no `PUT`/`PATCH` on `/execution/[ID]`, which is an answer, not a gap | `execution.abort` |
| delete | `DELETE /api/12/execution/[ID]` (+ `POST /api/12/executions/delete`, `DELETE /api/12/job/[ID]/executions`) | `execution.delete` |

## Change against the existing plugin

The plugin exists; its `spec/` and `src/` were deleted and it is being planned again from the live documentation. Against the five operations it had:

| | operations |
|---|---|
| keep | `job.run`, `job.get`, `job.list`, `execution.get` |
| rename | `executionoutput.get` → `execution.output` — caller-visible; every operation on the primary resource now names it the same way. Call this out in the specification. |
| add | `execution.list`, `execution.abort`, `execution.delete` (the list/update/delete lifecycle verbs); `project.list` (the id-finding read without which `job.list` and `execution.list` are uncallable); `job.submit`; `execution.state` |
| drop | — |

## Out of scope

| what | why |
|---|---|
| `GET`/`POST /api/59/project/[PROJECT]/jobs/export\|import`, `DELETE /api/59/job/[ID]`, `POST /api/59/jobs/delete` | job-definition management — schema Rundeck expects to be maintained out of band, via the UI or SCM |
| `POST /api/59/job/[ID]/execution/enable\|disable`, `.../schedule/enable\|disable` and their bulk forms (`/api/59/jobs/execution/enable\|disable`, `/api/59/jobs/schedule/enable\|disable`) | same — job-definition administration, not a motion a workflow performs |
| authentication tokens, config management, execution mode, cluster mode, ACLs, key storage, plugins, webhooks, calendars, license, user profile and user classes, log storage, metrics, SCM, runner management, scheduler takeover | instance administration |
| project create/delete/config/archive/readme/ACLs; `GET /api/59/project/[PROJECT]/resources` and node/tag reads | instance administration; only `GET /api/59/projects` is kept, as an id-finding read |
| `GET /api/59/system/info` | instance administration — usable as an auth probe, not an operation |
| paging plumbing, the `Accept`-header / `?format=` / URL-extension format negotiation, the `offset`/`lastmod` tailing loop, `GET /api/59/execution/[ID]/output/state` | transport mechanics the plugin hides |
| `GET`/`POST /api/59/project/[PROJECT]/run/command`, `POST /api/59/project/[PROJECT]/run/script`, `.../run/url` | **considered, dropped.** These do create an execution, so they are lifecycle-adjacent. But they are arbitrary remote command dispatch, not the provider's durable named unit; a workflow that wants one should define a job. |
| `POST /api/19/job/[ID]/input/file`, `GET /api/19/job/[ID]/input/files`, `GET /api/19/execution/[ID]/input/files`, `GET /api/19/jobs/file/[ID]` | **considered, dropped.** A real input path into `job.run`, but only for jobs declaring `file`-type options, so peripheral. |
| `GET /api/29/executions/metrics`, `GET /api/59/project/[PROJECT]/executions/metrics`, `GET /api/59/project/[PROJECT]/history`, `GET /api/59/job/[ID]/forecast`, `/workflow`, `/meta`, `/tags` | reporting and UI metadata; no motion drives them |
| `GET /api/40/execution/[ID]/result/dataAvailable`, `GET /api/40/execution/[ID]/result/data`, `/roimetrics/*` | Enterprise-only reads |

Record the last two "considered, dropped" rows in the specification too, so the reviewer sees a decision rather than silence.

## Open questions

- **`execution.abort` method.** The two official documents disagree. `docs/api/index.md` documents `GET /api/V/execution/[ID]/abort` (and its index links it as `GET`); the OpenAPI spec defines `/execution/{id}/abort` with **`post`** only, `operationId: apiExecutionAbort`. Verify both against the live container during review and pick the one that works; if both are accepted, prefer `POST`.
- **`failed-with-retry` vs `retried`.** The prose enum in `index.md` lists `failed-with-retry`; the JSON examples in both `index.md` and the OpenAPI spec show the placeholder string `succeeded/failed/aborted/timedout/retried/other`. Treat both spellings as terminal until the live container settles which one the wire actually carries.
- **`statusFilter` is narrower than `status`.** `execution.list` documents `statusFilter` as one of `running`/`succeeded`/`failed`/`aborted` only — no `timedout`, `scheduled`, `failed-with-retry` or `other`. Decide whether the plugin passes those through and lets Rundeck reject them, or rejects them itself.
- **`optionFilter` on `execution.list`.** Present in the OpenAPI spec (`filter executions by option values (partial match, e.g. '-test 123')`) but absent from `index.md`'s Execution Query parameter list. It is also the mechanism the previous implementation used for idempotency recovery (inject `sanitize(promise.id)` as an undeclared `rescorr` option, recover via `optionFilter`). Exposing it to callers and using it internally may collide — confirm during Invocation, and confirm the parameter exists at all on the live container.
- **`execution.list` dispatching on which argument is present** (project → `/project/[PROJECT]/executions`, job id → `/job/[ID]/executions`) is one operation by the scoped-variant rule, but it is a judgement. The alternative is requiring `project` always and making callers resolve it through `job.get`.
- **`job.submit` naming.** `submit` is the reserved verb, so `job.submit` it is — but the name reads as "submit a job definition". The resource-consistent alternative is `execution.submit`/`execution.run`, at the cost of renaming the existing `job.run`.
- **`execution.output` / `execution.state` are named by what they return**, not by a verb, unlike every other row. Consistent with each other; flagged so the choice is visible.
- **`job.retry` is the weakest of the added operations** — a scoped variant of the create verb rather than a lifecycle row of its own. Cut it first if the surface must shrink.
