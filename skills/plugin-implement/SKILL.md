---
name: plugin-implement
description: Implement a Resonate transport plugin in Rust from its specification — plugins/<name>/spec/specification.md → plugins/<name>/src.
---

# plugin-implement

Implement `plugins/<name>/src` from `plugins/<name>/spec/specification.md`.
The specification is the source of truth; do not re-derive facts from the
provider's documentation, and do not deviate from the specification. If
the specification is wrong or incomplete, stop and report — do not patch
around it.

## Architecture

Every plugin is the same Rust library crate shape, copied from
[reference/](reference/). The canonical worked example is the in-repo
`plugins/airflow/src` — read it before implementing, for the frame's
style only: where it disagrees with a specification (param shape, error
kinds, polling, helper placement — e.g. a specification's `_check`
wrapping each request before its status branches), the specification
governs.

- `Cargo.toml` — package `resonate-plugin-<name>`, a library.
- `src/lib.rs` — exports `Worker`, `Config`, `SCHEME`, and `pub mod
  plugin` (the integration tests call `plugin::process` directly). The
  server binary constructs `Worker` with its `ResonateServer` port and
  registers it on the router under the scheme.
- `src/worker.rs` — the standard frame, identical in every plugin. Never
  edit it. It implements `ResonateWorker::send` (send = accepted for
  delivery: spawn and return), then per delivery: claim the task
  (`task.acquire` with the message's fencing version, `pid: "self"`),
  heartbeat the lease at a third of the TTL in its own task (the lease
  clock and the downstream clock are independent), call
  `plugin::process`, and settle: `Ok(Ok(v))` fulfills resolved,
  `Ok(Err(v))` fulfills rejected, `Err(Ok(reason))` halts the task
  (`task.halt` — redelivery pauses until an operator continues it),
  `Err(Err(reason))` drops the task (`task.release`) — the message is
  redelivered promptly. The server settles timed-out promises itself.
- `src/plugin.rs` — everything provider-specific. This is the only file
  you write.

## Procedure

1. Copy `reference/` into `plugins/<name>/src` — the crate root is
   `plugins/<name>/src` (so `Cargo.toml` is `plugins/<name>/src/Cargo.toml`
   and the code is `plugins/<name>/src/src/`). Replace `{name}` with the
   plugin scheme.
2. Translate specification §2 into `Config`: bare type = required field,
   `Option<T>` = optional, literal defaults via `#[serde(default = ...)]`,
   `= poll` cascades via an `unwrap_or(self.poll)` accessor, `Duration`
   fields via `humantime_serde`. An `= instance` default is resolved by
   whoever loads the config section — the plugin never sees the address;
   declare the field required.
3. Implement `process`: decode `param.data` (base64 JSON, as the
   reference's `process` shows) and match on its `func`, one arm per §4
   operation, dispatching to one private function per operation. An
   unknown func rejects with code `unknown_func`; a missing or mistyped
   required arg rejects `invalid_request` locally — the Python's
   `KeyError` is not a model. Both are frame-level rejections
   (plugin-specify: produced by the frame, not the operation) and
   legitimately carry codes outside the operation's Rejected enum.
4. Translate each §4.N.5 Python function into its Rust function,
   preserving structure exactly: the same requests in the same order, the
   same enumerated permanent status codes → `Ok(Err(...))`, halt
   conditions → `Err(Ok(...))`, everything unclassified → `Err(Err(...))`,
   the same loop with the same
   `promise.timeout_at` bound and failure-streak cap, the same comments.
   Use async `reqwest` with `.await` (the frame is tokio-async). Build
   rejected values to the op's Rejected schema; build resolved values by
   applying the op's Resolved mapping. Translation rules the Python does
   not state: clamp every poll sleep to the time remaining before
   `promise.timeout_at`; an absent response field maps to JSON `null`,
   never a crash (a Rejected `detail` may therefore be `null` — tests
   assert it as such). The wire bytes are not among them: the
   specification's Python is what the reviewer executed against the live
   provider, so the request this code sends must be byte-for-byte the
   request that Python sent. Where it cannot be — the Python renders a
   value one way and `reqwest` another — the specification is wrong and
   rule 4's "stop and report" applies; do not silently correct it here.
5. Injected identity values are `sanitize(&promise.id)` (from the frame,
   never the raw id) — exactly where the specification writes
   `sanitize(promise.id)`.
6. Validate: `cargo check` clean, then
   `python3 skills/plugin-implement/check.py plugins/<name>` clean — it is
   the mechanical half of this step (every operation has an arm, every
   rejection code is constructed, no provider fact in the code that the
   specification does not carry, no template placeholder left behind, and
   nothing `.gitignore` matches ready to be committed). It compiles
   nothing; step 7 is the other half.
7. Test per the Testing section: `cargo test` clean — with the §5
   provider running when the specification has one.
8. End to end, per the Testing section's last paragraph: a minimal
   in-process server with only this worker registered; create a promise;
   see it settle.

## Testing

Tests live in `plugins/<name>/src/tests/process.rs` and test
`plugin::process` only. The frame, config extraction, and registration
are not the plugin's to test. Tests run serially: ship
`.cargo/config.toml` with `RUST_TEST_THREADS = "1"` — a §5 provider need
not survive parallel load, and its failure under it looks like a plugin
bug.

When the specification has §5 (Notes `Self-hosted: yes`), tests run
against the real provider — never a mock: run 5.1/5.2 first, then build
`Config` from the `{SCHEME}_{KEY}` environment variables 5.2 exported,
overriding `poll` to `1s` (the test default; an individual test may use
a larger value when a condition needs a wider window — e.g. a delete
that is only legal on a finished execution). A local test helper constructs the
`PromiseRecord` (fresh promise id per test, base64 `{func, args}` param,
`timeout_at`). Every documented condition is induced with real inputs:

| condition | induction |
|---|---|
| resolved | the succeeding work item §5 provisions |
| `<resource>_failed` | the failing work item §5 provisions |
| `not_found` | a nonexistent id |
| `invalid_request` | violate a constraint the Param schema documents |
| halt | corrupt the credential in `Config` |
| recovery / re-entry | call `process` twice with the same promise id |
| deadline | `timeout_at` in the past |
| pending → terminal | real work takes real time |

One test per (operation × documented condition). Each test asserts the
verdict variant and the value against the operation's Resolved or
Rejected schema. Provisioning the provider with the resources these
inductions need (a project, jobs, templates) is test setup, done with
raw provider calls from the test files — it is not patching the
specification; the two test files may duplicate a small provisioning
helper.

SaaS-only providers (no §5): the same table against a local mock
(wiremock), stubbing responses from the Integration Response schemas
with the success status the Integration Request line's `→ <status>`
names, and additionally asserting each outgoing request against the
Integration Request schema (method, path, injected `sanitize`, body
mapping). The mock seam is an environment override: the API base
constant honours `{SCHEME}_API`, read once via `OnceLock` — that
process-wide override is a second reason the tests run serially. Run one
mock server per test binary and `reset()` it per test; stub a
pending → terminal sequence as a response series
(`up_to_n_times(1)` plus priorities). State the circularity in a
test-file comment: the mocks are built from the same specification as
the code; only review against the live provider breaks the loop.

429, 5xx, and network conditions are not induced or mocked per plugin —
unclassified failure is the frame's release path. One exception: the
consecutive-failure streak cap may be covered by a mocked 5xx series; it
is unreachable otherwise.

Finally, end to end (`tests/e2e.rs`): compile a minimal Resonate server
with only this worker — the `resonate` crate (a dev-dependency:
`resonate = { git = "https://github.com/resonatehq/resonate", branch =
"core-crate" }`, the same source as `resonate-core`) provides an
in-process `ResonateServer` implementation;
register nothing but this plugin's `Worker`. Create a promise whose
`resonate:target` tag is this plugin's address and whose param is a real
`{func, args}`, deliver the resulting Execute message to the worker, and
await settlement: the promise must reach the terminal state and value
the specification's schemas promise — resolved for the succeeding §5
work item, rejected with the right `code` for the failing one. For a
SaaS-only plugin the wiremock stands in for §5, and the failing item is
the work operation's own terminal failure code. This is
the only test that exercises the frame's claim/heartbeat/settle wiring
together with `process`; everything upstream of the worker is the
server's own test surface, not this crate's.
