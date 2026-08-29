<h1 align="center">Resonate Plugins</h1>

<p align="center">
  <strong>Call any API like a local function.</strong><br>
  Transport plugins that expose a provider's API as durable promises, so every
  Resonate SDK can call it in any language — no client library, no HTTP in your code.
</p>

<p align="center">
  <a href="https://github.com/resonatehq/resonate">Resonate</a> ·
  <a href="Plugins.md">Catalog</a> ·
  <a href="plugins/">Plugins</a>
</p>

---

Integrating a provider usually means finding a client library for your
language, or writing the HTTP by hand — and then doing it again for the
next language, and deciding all over again which failures are worth
retrying.

A plugin does it once. You create a promise; the Resonate server calls
the provider and settles the promise with the outcome — resolved for
success, rejected with a code for every failure the provider documents as
permanent, retried on its own for everything transient. From your side
that is a function call. It looks the same in Python, TypeScript, Go and
Java, because the integration lives in the server rather than in any
SDK.

Then, for anything long-running, you get await semantics for free. A Dag
run takes an hour. A render takes four seconds. A support ticket closes
twenty-eight days after someone opens it. Awaiting one of those normally
costs a worker, a webhook receiver, a retry loop, and a state machine to
remember where you were when the process died. Here it costs a promise:
nothing in your process has to stay alive for it, and nothing has to be
reconstructed when it doesn't.

## How it works

Address the plugin with a tag and describe the work in the param:

```json
{
  "id": "nightly-etl-2026-08-29",
  "timeoutAt": 1756500000000,
  "tags": { "resonate:target": "airflow://default" },
  "param": { "func": "dagrun.trigger", "args": { "dag_id": "nightly_etl" } }
}
```

The `resonate:target` tag is what makes the server create a task and
deliver it. (On the wire the param travels base64-encoded in
`param.data`; the document above is what it decodes to.)

The plugin triggers the Dag, polls it to a terminal state, and settles
the promise:

```json
{
  "state": "RESOLVED",
  "value": { "dag_id": "nightly_etl", "dag_run_id": "…", "state": "success" }
}
```

A failed Dag rejects with `{"code": "run_failed"}`. An unknown `dag_id`
rejects with `{"code": "dag_not_found"}`. A rate limit or a 503 doesn't
settle anything — the work is redelivered and picked up where it left
off, because every request the plugin sends is a deterministic function
of the promise. Credentials that stop working halt the task and wait for
an operator instead of hammering the provider.

No SDK in the loop. No glue service. No webhook endpoint to host.

## The catalog

| | Plugin | Represents | |
|---|---|---|---|
| <img src="assets/airflow.png" alt="" width="28" height="28"> | **Apache Airflow** | a Dag run, from trigger to terminal state | [plugins/airflow](plugins/airflow) |
| <img src="assets/bannerbear.png" alt="" width="28" height="28"> | **Bannerbear** | an image or video render | [plugins/bannerbear](plugins/bannerbear) |
| <img src="assets/baserow.png" alt="" width="28" height="28"> | **Baserow** | a table export job, resolving with the file URL | [plugins/baserow](plugins/baserow) |
| <img src="assets/gotify.png" alt="" width="28" height="28"> | **Gotify** | a push notification, resolving on acceptance | [plugins/gotify](plugins/gotify) |
| <img src="assets/n8n.png" alt="" width="28" height="28"> | **n8n** | a workflow execution | [plugins/n8n](plugins/n8n) |
| <img src="assets/rundeck.png" alt="" width="28" height="28"> | **Rundeck** | a job execution | [plugins/rundeck](plugins/rundeck) |
| <img src="assets/zendesk.png" alt="" width="28" height="28"> | **Zendesk** | a support ticket, until it closes — days later | [plugins/zendesk](plugins/zendesk) |

[Plugins.md](Plugins.md) is the full catalog: everything shipped and
everything still on the list.

## Anatomy of a plugin

```
plugins/<scheme>/
  README.md
  spec/specification.md   the implementation specification
  src/                    the Rust crate
```

Each plugin exposes one address — `<scheme>://[{instance}]` — and a
handful of operations named `resource.verb`: the provider actions a
caller makes, plus the reads needed to build their arguments
(`dag.list`, `template.get`) and to inspect the record afterwards. Some
of those actions finish in one round trip; some run for hours. The
caller's code does not change either way.

Every operation is classified on two axes, and the classification is a
promise about behaviour, not documentation:

- **Invocation** — `read`, `create_idempotent`, `fetch_then_create`, or
  `create`: how much the plugin can guarantee about a redelivery not
  doing the work twice. It takes the highest rung the provider actually
  supports, and says so plainly when that rung is low.
- **Monitoring** — `request_response` or `request_poll`: whether the
  outcome is in the reply or has to be watched for, bounded by the
  promise's own `timeoutAt`.

## How they get built

A plugin is written once, in a specification, and translated from there.
The pipeline lives in [skills/](skills):

| | |
|---|---|
| [plugin-specify](skills/plugin-specify) | Write `spec/specification.md` — tables, request lines, JSON Schemas, and Python. Every claim from documentation fetched now, never from memory. A [linter](skills/plugin-specify/lint.py) enforces the mechanical rules. |
| [plugin-review](skills/plugin-review) | Verify the specification against the live provider, then execute every operation by hand against a real instance in Docker. Findings, ranked, with the URL or the executed call that proves each one. |
| [plugin-implement](skills/plugin-implement) | Translate the specification into Rust, structure preserved. Tests run against the real provider — never a mock, where the provider can be self-hosted. |

The specification is the source of truth. If the code and the provider
disagree, the specification was wrong, and that is where the fix goes.

## Built with Resonate

Resonate is a durable execution engine you can hold in your head — the
main repository is at
**[resonatehq/resonate](https://github.com/resonatehq/resonate)**.
