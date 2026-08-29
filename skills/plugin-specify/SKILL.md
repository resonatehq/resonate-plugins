---
name: plugin-specify
description: Write the specification.md for a Resonate transport plugin — the implementation source for plugins/<name>/spec/specification.md.
---

# plugin-specify

Write `plugins/<name>/spec/specification.md` for one provider. The document
is an implementation instruction for a coding agent. It contains tables,
request lines, JSON Schemas, and Python — no prose. Every claim must come
from the provider's live documentation, fetched now. Do not use memorized
API knowledge: APIs change.

## Concepts

A plugin exposes a provider's API as durable promises, so that any
Resonate SDK can call it as if it were a locally defined function — no
client library, no HTTP in the caller's code, no integration written once
per language. That is what a plugin is for, and it is worth doing for an
API whose every call answers in one round trip.

Where an action is long-running, the promise carries await semantics on
top for free: the caller awaits, the plugin sees the action through to
its terminal state, and nothing in the caller's process has to stay alive
meanwhile. That is the icing. It is not the qualification — a document
that specifies three fast reads accurately is a good plugin, and hunting
for something slow to justify one is how a plugin ends up specifying
operations nobody asked for.

The plugin function `op(cfg, promise)` runs in the Resonate server when
the promise's Execute message is delivered. Its job is to complete the
operation: begin it, poll it to a terminal state where it has one, and
settle.

- `promise.id` — the promise id (string).
- `promise.timeout_at` — the caller's `timeoutAt`, milliseconds since
  epoch.
- `promise.param["args"]` — the caller's arguments.
- `cfg` — the §2 configuration, fully resolved before the plugin runs:
  defaults applied, `= poll` cascaded, `= instance` filled from the
  address by the config loader.
- `sanitize(promise.id)` — engine-provided, deterministic. It yields
  `<up to 100 chars of the promise id, every character outside
  [A-Za-z0-9._-] replaced by _>-<16 hex chars>`: ASCII, `[A-Za-z0-9._-]`
  only, 17–117 characters. It does not collapse `..`, does not
  lower-case, and is not a fixed-width digest. Every value injected into
  the provider — idempotency keys, correlation fields, client-supplied
  ids — is `sanitize(promise.id)`, never the raw id. Same promise ⇒ same
  token, so dedup and lookups keep working. If the provider's constraint
  on the injected identity is tighter than the yield above (length cap
  below 117, hex-only, no dots, case-folded), the operation cannot use
  `sanitize` as the injected identity — say so in the Idempotency row and
  take a lower Invocation rung.

In §4.N.5 Python, `sanitize`, `cfg`, and `promise` are ambient — no
import, no definition. `cfg.<key>` is the §2 value; a `Duration` key is a
`timedelta`, so cadences are written `cfg.poll.total_seconds()`. A key
declared `Option<Duration>` with default `= poll` is read as the
already-cascaded `cfg.poll_<op>`, never `cfg.poll_x or cfg.poll`.

Mapping notation, used in schema `description` fields and URL templates:
`= promise.param.x` (copied from args), `= sanitize(promise.id)` (injected
identity), `= response.body.x` (projected from the provider response),
`{?a,b,c = promise.param.*}` (the listed query parameters, each copied
from the same-named arg, all optional), `= poll` (defaults to the `poll`
config key), `= instance` (defaults to the address instance name).

## Outcomes

Every operation ends in exactly one of four outcomes: one success
verdict, one failure verdict, and two non-verdicts. Verdicts settle the
promise and are RETURNED; non-verdicts leave it unsettled and are RAISED.
Failures are classified by what would have to change for the same
request to succeed:

- *nothing can* → `return ("rejected", value)`. Value matches the
  Rejected schema. The request is a deterministic function of the durable
  promise — every redelivery sends identical bytes — so a failure that
  depends only on the request can never heal, and a terminal non-success
  state never un-happens. Reject when the provider documents the status
  as a property of the request (validation error, unknown resource) or
  reports a terminal non-success state of the work.
- *time alone* → `raise Exception("release", reason)`. Retry right away:
  the failure is about the moment (rate limit, server error, network).
  The server re-delivers and re-entry must be safe. Any exception that is
  not a halt (library errors, network failures) is also treated as a
  release.
- *a person, outside this system* → `raise Exception("halt", reason)`.
  Retry only after an operator acts: the failure is about our standing
  (credentials rejected, payment required, permission denied) — retrying
  changes nothing until a person fixes it, and hammering the provider
  meanwhile is noise. Re-delivery pauses until the operator continues the
  task. A halted promise still times out at `timeout_at`.

Tie-breaks:

- **Halt requires a documented status.** Raise halt only where the
  provider's documentation for that endpoint names a status meaning
  credentials rejected, payment required, or permission denied. A status
  the docs do not name is not halt. Never infer halt from body text.
- **Waiting is not a person.** If the condition clears on its own within
  any plausible `timeoutAt` — rate limit, burst cap, maintenance window,
  a quota that resets on a documented cycle — it is release, not halt,
  even at 402/403. Halt is for a condition that only a completed billing
  action, a key rotation, or a role grant ends.
- **A refreshable credential is not a standing problem.** Where
  authentication is multi-step (§3) and the token has a documented
  lifetime, a 401 inside a poll loop re-exchanges the token and
  continues; a 401 from the token exchange itself is halt.
- **Every terminal non-success state is a rejection with its own code.**
  Enumerate the provider's terminal states in 4.N.4's status enum. One
  (or a named set) resolves; each remaining terminal state gets a
  distinct Rejected `code` (`run_failed`, `cancelled`, `expired`) — never
  folded into `invalid_request`. A state that is non-terminal but can
  persist indefinitely (paused, awaiting input) stays in the loop until
  `timeout_at`; say so in the Param description.

The halt/release split is provider-wide and lives in one module-level
`_check(r)` helper, defined in the first Implementation block:

```python
def _check(r):
    # Halt statuses: only those this provider's docs name as
    # operator-required. Adjust the tuple per the documentation.
    if r.status_code in (401, 403):
        raise Exception("halt", r.text)
    if r.status_code == 429 or r.status_code >= 500:
        raise Exception("release", r.text)
    return r
```

Rejection is per-endpoint and written inline at every call site — the
permanent statuses and their `code`s are facts about that one call, and a
documented status may move class (a 403 that means "no access to this
template" is rejected, not halt; handle it inline before `_check`). The
call-site pattern: documented statuses first, `_check`, then the
deterministic residue:

```python
r = _check(requests.post(..., timeout=10))
if r.status_code == 409:
    ...                                  # documented: e.g. the recovery path
if r.status_code == 404:
    return ("rejected", {"code": "not_found", "detail": r.text})
if r.status_code >= 400:
    # Residue: identical bytes every redelivery — permanent.
    return ("rejected", {"code": "invalid_request", "detail": r.text})
```

## Procedure

1. Fetch the provider's API documentation. Find the OpenAPI spec if one is
   published; verify the URL resolves. Confirm the current API version.
2. Choose the operations, in this order and nothing beyond it:
   - **Work** — every provider action a caller would make and want the
     outcome of. Where the provider has a durable, named unit (a run, a
     job, an export), that unit is the work; an inline variant of the same
     machinery (an ad-hoc command, a raw script run) is the same work with
     its definition inlined — exclude it unless the provider has no named
     unit. Where the provider has no long-running unit, the work is simply
     its actions, specified `request_response`: that is a plugin like any
     other, since the value is exposing the API as a promise every SDK can
     call. Never hunt for something slow to justify a plugin, and never
     inflate a read into an action to have one.
   - **Composition** — the reads a caller needs to construct the work's
     args: enumerate the resource (`<resource>.list`) and fetch the one
     resource whose shape constrains the args (`<resource>.get`). Include
     only where the work's args actually reference provider-defined names
     or ids.
   - **Inspection** — the plain read of the work's own record, and, where
     the work publishes results separately, the read that retrieves them.
   - Nothing else. No update, delete, or admin operation unless it is
     itself work. A poll endpoint, a token exchange, or a webhook
     registration is internal mechanics, never an operation.

   Expect 3–6 operations. Where a work operation polls, each inspection
   operation's Param description states "A plain read — not the completion
   mechanism; `<work op>` observes independently." Where no operation
   polls there is no completion mechanism to disclaim, and the sentence is
   omitted.

   Names are `resource.verb`, both segments lowercase with no separators
   (`dagrun`, not `dag_run`). Verbs: `create`, `get`, `list`, `update`,
   `delete`; use the provider's own verb only when none of those is
   truthful (`dagrun.trigger`). When one resource has two reads — its
   record and a separately published output — the output read is its own
   compound resource (`executionoutput.get`). The Python function name is
   the func with `.` → `_`.
3. Fill the template below, one subsection at a time, per the rules in the
   next section.
4. Validate. Run `python3 skills/plugin-specify/lint.py
   plugins/<name>/spec/specification.md`; it must pass clean. Then
   confirm by hand what the lint cannot: every endpoint, field name,
   status code, enum, and default against the fetched documentation.
   Where the documentation is silent on a status, read-only probes of the
   §5 instance — and requests the provider rejects without effect (a run
   against a nonexistent id) — are admissible evidence; record what
   proved each claim.
5. Test, only if the provider can run locally end to end: run the §5
   blocks and confirm the image builds, the provider comes up
   serviceable, and every configuration value and environment variable
   the implementations need is exported. Operations are not executed at
   specification time; that is the implementation's test.
6. Leave `Reviewed by` empty. Review is a separate step by a separate
   agent following `skills/plugin-review`.

## Template

~~~markdown
# <Provider>

| | |
|---|---|
| **API** | `<base URL>` |
| **Idempotency** | <mechanism + window, or "No idempotency"> |
| **Reviewed by** | |

**Notes**

| | |
|---|---|
| **OpenAPI** | `<spec URL>` |
| **Self-hosted** | <yes — §5 | no — SaaS only, no §5> |

## 1. Address

```
<scheme>://[{instance}]        # omitted instance = "default"
```

## 2. Configuration

```toml
[<scheme>.{instance}]          # [<scheme>] = [<scheme>.default]
```

| key | type | default | example |
|---|---|---|---|
| `<key>` | `<Rust type>` | <default> | `<example>` |

## 3. Authentication & Authorization

| | |
|---|---|
| **Documentation** | [<title>](<provider auth doc URL>) |
| **Probe** | `<read-only request, full path from host root>` → `200` |

```
<the auth header, e.g. Authorization: Bearer {api_key}>
```

## 4. Operations

### 4.1 <resource.verb>

| | |
|---|---|
| **Documentation** | [<title>](<this operation's provider doc URL>) |

```json
{ "func": "<resource.verb>", "args": { ... } }
```

### 4.1.1 Promise Param Schema

```json
{ "description": "...", "type": "object", "properties": { ... }, "required": [ ... ] }
```

### 4.1.2 Promise Value Schema

#### Resolved

```json
{ "type": "object", "properties": { ... } }
```

#### Rejected

```json
{
  "type": "object",
  "properties": {
    "code": { "type": "string", "enum": [ ... ] },
    "detail": { "description": "..." }
  },
  "required": ["code"]
}
```

### 4.1.3 Integration Request

```
<METHOD> <full path from host root> → <success status>
<operation-added header>: <value>
```

```json
{ "type": "object", "properties": { ... }, "required": [ ... ] }
```

### 4.1.4 Integration Response

```json
{ "type": "object", "properties": { ... } }
```

### 4.1.5 Implementation

| | |
|---|---|
| **Invocation** | `read` \| `create_idempotent` \| `fetch_then_create` \| `create` |
| **Monitoring** | `request_response` \| `request_poll` |

```python
def <resource_verb>(cfg, promise):
    ...
```

## 5. Test

### 5.1 Base Image

```dockerfile
<test environment>
```

### 5.2 Run

```sh
<build and run; wait until serviceable; export configuration>
```
~~~

## Rules per section

**Header.** Table 1: API base URL; the idempotency mechanism with its exact
window, or "No idempotency"; `Reviewed by` empty. The Idempotency row also
documents the provider's constraints on the injected identity value
(charset, length, uniqueness scope). For a self-hosted provider the API
row is `{base_url}/<path>` and `base_url: String` is a required §2 key.
Notes table: the OpenAPI URL, and the Self-hosted row (`yes — §5` or
`no — SaaS only, no §5`). No other rows. The API row is the origin+prefix
the implementation composes; request lines and the Probe are written as
the full path from the host root, including any version segment already
present in the API row — `POST /api/v2/tickets`, not `POST /tickets`.

**§1 Address.** One address per plugin: `<scheme>://[{instance}]`. The
instance is an opaque alias selecting the config section; omitted means
`default`. Never put a verb or resource in the address.

**§2 Configuration.** The TOML section header line, then the table.
Types are Rust: a bare type is required (a default satisfies it),
`Option<T>` is optional. Empty default cell means the user must set it.
Include `poll: Duration` iff some operation's Monitoring is
`request_poll`; its default is the fastest such operation's cadence — a
small fraction of that operation's typical duration (a seconds-scale
render ~2s, an hours-scale workflow ~30s, a days-scale human process
~15m). When one operation's duration spans scales, size `poll` to the
fast end — the slow end costs only cheap polls. Never chattier than the
work warrants. When there are two or more
`request_poll` operations: one `poll_<resource>: Option<Duration>` per
operation (`poll_<resource>_<verb>` if a resource has several). An
operation that genuinely shares the base cadence defaults `= poll`; an
operation at a different scale takes a literal default sized to that
scale. A provider value that naturally equals the instance name gets
default `= instance`. Secrets are plain `String` fields.

**§3 Authentication.** One Documentation link to the provider's auth page.
One Probe row: a cheap read-only request that returns 200 with valid
credentials. A fenced block showing the auth flow: the composed header,
preceded by the token-exchange request line when authentication is
multi-step. Nothing about where keys are created or what they cost — that
is the provider user's concern.

**§4 Operations.** One `### 4.N` per operation: the Documentation table
(exactly one link — the operation's own doc, not the poll endpoint's), the
envelope block, then subsections .1–.5. Spell out every operation fully.
Never merge two operations into one section. Reuse forms allowed in
schema subsections, nothing else: "Same as 4.N.M." as the entire body;
"Same as 4.N.M, plus:" followed by a schema of only the additional
properties; and, inside a schema, a subschema replaced by
`{"description": "Same as 4.N.M"}`, optionally with a pointer into it
(`Same as 4.N.M .dags[]`). Inside a `####` half, `Same as 4.N.M.` refers
to that operation's same-titled half. References may point forward.

**.1 Promise Param Schema.** JSON Schema of `args` — the caller's
vocabulary, not the provider's wire shape: strip provider request
envelopes (`{"ticket": {...}}` → the ticket's own properties; 4.N.3
re-wraps them). Path segments are args like any other. Omit every
transport-owned field: correlation values, idempotency keys, webhook
URLs, status fields. The top-level `description` states the operation
contract: what resolves it, what rejects it, and — whenever Monitoring is
`request_poll` — the duration scale for sizing `timeoutAt` (omit it for
`request_response`). Constrain everything the provider constrains (enums,
ranges). Mark required only what the provider requires. Top-level param
and request-body schemas set `additionalProperties: false` when the
provider rejects unknown keys and omit it otherwise; a subobject the
provider passes through opaquely sets `additionalProperties: true`
explicitly.

**.2 Promise Value Schema.** Two `####` headings, `Resolved` then
`Rejected`, each one JSON Schema. A `request_poll` operation's Resolved
schema names each projected property with its `= response.body.x`
mapping, and the Python builds it from an explicit `keys` tuple. A `read`
operation's Resolved schema is the whole body, or the single record when
the provider wraps it (`{"type": "object", "description":
"= response.body.<wrapper>"}`); a top-level array response is
`{"type": "array", "description": "= response.body"}`. Rejected is `{code, detail}` with
`required: ["code"]`: `code` an enum of this operation's permanent
failure cases. Omit the `detail` property only when no code carries one;
when at least one does, keep it and state per code what it carries,
writing `<code>: absent` for those that do not. Codes are `snake_case`:
the unqualified name (`not_found`, `invalid_request`, `conflict`) when
the operation touches one resource; qualified (`dag_not_found`) only when
one operation can miss on two different resources; the work's own
terminal failure is `<resource>_failed`. `unknown_func` and the
param-decode `invalid_request` are produced by the frame, not by an
operation; they never appear in an operation's Rejected enum.

**.3 Integration Request.** A fenced request block: method and full path
from the host root with mappings inline, ending with `→` and the
documented success status
(`GET /api/v2/tickets/{promise.param.id} → 200`), one line per header
this operation adds (`Idempotency-Key: {sanitize(promise.id)}`). The §3 auth
header is not repeated. Query parameters use
`{?a,b,c = promise.param.*}` appended to the path — never a
per-parameter `?x={...}` list. A constant the API requires (a fixed query
parameter, a version segment) is written literally — constants are not
mappings and never appear in the `{?...}` form. Then the body JSON Schema if there is a
body: property descriptions are mappings, types copied from the param
schema. No sentences.

**.4 Integration Response.** JSON Schema of the full response body,
envelopes included (if the provider wraps records, the schema shows the
wrapper). Include the fields the implementation reads plus identity,
status enum, and error fields; enumerate every terminal state in the
status enum. Field descriptions state facts about values, not
instructions.

**.5 Implementation.** The Invocation/Monitoring table, then one Python
function using `requests`, real and parseable. Classify every operation
on both dimensions; the pair determines the function's shape.

Invocation — how the operation begins. Take the highest rung the provider
documents:

- `read` — no effect; a plain request.
- `create_idempotent` — a client-supplied key or id makes creation
  idempotent: send `sanitize(promise.id)`. The key covers only the
  constraints it controls — every other documented uniqueness constraint
  is its own permanent rejection code.
- `fetch_then_create` — no key (or the key's window can expire), but the
  stamped `sanitize(promise.id)` is locatable: fetch first, create on a
  miss.
- `create` — neither key nor lookup: begin unconditionally. When a
  duplicate is benign, state the cost in a comment. When it is not and no
  higher rung exists, `create` still stands — the duplicate window is
  re-delivery only; state the risk in the Param description as part of
  the operation contract, and the cost in a comment.

Monitoring — how the outcome is observed:

- `request_response` — the terminal state is in the response; settle from
  it: documented-permanent statuses inline to `("rejected", ...)`,
  `_check(r)` for the rest, `("resolved", ...)` from the body. Never
  `raise_for_status()` — it collapses halt into release.
- `request_poll` — begin, then loop. Two shapes, chosen by whether the
  begin step already yielded the current state:
  - *state in hand* (a create whose response carries the status):
    `while <non-terminal>:` deadline-check, sleep, re-GET, re-test.
  - *state not in hand* (any `fetch_then_create`, or a begin returning
    only an id): `while True:` deadline-check, GET, break on terminal,
    sleep.

  In both, the deadline check is the first statement of the body:

  ```python
  if time.time() * 1000 >= promise.timeout_at:
      raise Exception("release", "promise timed out")
  ```

  and no sleep runs past the deadline:

  ```python
  time.sleep(min(cfg.poll.total_seconds(),
                 (promise.timeout_at - time.time() * 1000) / 1000))
  ```

Any logic the provider dictates is valid implementation logic — token
refresh (re-authenticating inside a poll loop when the token expires),
multi-step flows, secondary lookups — provided the contract holds: settle
truthfully, reject only on documented-permanent failures, raise for
no-verdict, loops bounded by `promise.timeout_at`, re-entry safe. Further
rules:

- Imports, module constants (`API = "..."`), `_check`, and `_`-prefixed
  helpers are written once, in the first Implementation block, and reused
  by every later block; no later block repeats them.
- Every `requests` call carries `timeout=10`.
- Rejection statuses are enumerated per call site from that endpoint's
  documentation, then the residue line `if r.status_code >= 400` rejects
  `invalid_request` (see Outcomes). Never map a status the docs name to a
  different class than the docs support.
- Every caller-supplied value placed in a URL path is escaped with
  `urllib.parse.quote(..., safe="")`.
- The Python must put the same bytes on the wire as the implementation
  will: `plugin-review` executes these snippets against the live
  provider, so a snippet that only works in Python proves nothing about
  the plugin. Two idioms need writing out rather than leaving to
  `requests`: a boolean copied into a query string renders as
  `"true"`/`"false"`, never Python's `True`/`False`; and an array-valued
  query parameter is joined the way the provider documents — one
  repeated key per element only where the provider says so, otherwise a
  single comma-joined value. Both belong in the `_query` helper that
  filters args into `params`, written once in the first Implementation
  block.
- Inside a poll loop, if the external identity cannot be recovered on
  re-entry, absorb transient failures with a consecutive-failure cap of
  5: catch exceptions, re-raise halts (`e.args[:1] == ("halt",)`), count
  and `continue` on everything else.
- Comments state provider-specific facts only (terminal-state rules,
  dedup properties, costs). No comments explaining the plugin contract.

**§5 Test.** Only for providers that can run locally end to end
(self-hostable in Docker); the Notes `Self-hosted` row says which. A
provider that cannot gets no §5 — the document ends at §4. The
specification is the single source: no files beside it.

The environment provisions the implementation's test fixtures: at least
one work item that succeeds and one that fails (e.g. a Dag that
completes and a Dag built to fail), ready to run (unpaused, enabled) —
and 5.2 exports their identifiers as `{SCHEME}_FIXTURE_OK` and
`{SCHEME}_FIXTURE_FAIL`.

**5.1 Base Image**: a Dockerfile standing up the provider; the image ends
with a current Python 3 and `requests` available. Whoever runs 5.2 first
writes this block to `spec/Dockerfile` — a generated, gitignored
artifact. **5.2 Run**: an `sh` block that runs unattended from
`plugins/<name>/` and leaves the provider running and serviceable (no
teardown — that is the caller's step): remove any stale container, build
and run as `plugin-<name>-test`, wait until the provider is actually
serviceable (gate on the slowest warm-up path — e.g. mint a token, not
just an unauthenticated version check), and export configuration as
`{SCHEME}_{KEY}` environment variables for every §2 key with an empty
default cell (keys with defaults are not exported). The same Dockerfile
is reused later to test the implementation.

## Style

- No prose outside tables, fenced blocks, and `Same as 4.N.M.` lines.
- No design alternatives, no unimplemented upgrades, no meta-commentary.
- No provider account setup, pricing, or tier information.
- State each fact exactly once, in its structured place.
