---
name: plugin-prepare
description: Plan a plugin before specifying it — the provider's motions and resources, the API surface to support, and the algebra each operation needs. Writes plugins/<name>/spec/preparation.md.
---

# plugin-prepare

Read [plugin](../plugin) first — the goal, the approach, the algebra.

Write `plugins/<name>/spec/preparation.md`: the design the specification is
then written from. It decides *what* the plugin exposes and *what shape*
each operation has; the specification decides how. Every claim comes from
the provider's documentation fetched now, never from memorized API
knowledge.

## Procedure

1. Fetch the API documentation, and the OpenAPI document if one is
   published. Actually fetch each URL and confirm what came back is the
   document rather than an error page or a login wall. Where one is
   unreachable from here, record what you read instead — a mirror, the
   provider's shipped source, an official client library. Never fall back
   on recall.
2. Name the **motions**: what people actually do with this system, in their
   words, not the API's. "Render an image from a template." "Open a support
   ticket and see it through." A motion a caller would not perform through
   a durable promise is not a motion for us.
3. Name the **resources** those motions act on, and pick the primary one —
   the one the plugin is about. Most providers have exactly one.
4. Map each motion onto the provider's endpoints, method and path. This is
   where the provider's async splits become visible: a motion answered by a
   submit endpoint plus a status endpoint is one motion, not two.
5. Decide the surface. The primary resource's whole lifecycle, every verb
   the provider offers on it; the durable units; the reads that find ids,
   constrain arguments, and inspect records. Out: instance administration
   (users, permissions, licences, settings, schema management the provider
   expects out of band) and transport mechanics (poll endpoints, token
   exchanges, webhook registration, pagination plumbing).
6. Give each operation its algebra — `call` or `call + poll`, nothing
   finer. `call + poll` wherever the provider splits submit from status,
   since collapsing that split is the point. How the operation begins
   safely is the specification's Invocation rung and is not decided here.
   Add `<resource>.submit` only where the wait is long enough that a
   caller might not want it and the handle stays good long enough to be
   redeemed.
7. Write the document. Where a decision was close, say so in Open questions
   rather than burying it — the specification agent inherits your judgement
   and should know which parts were judgement.

## Template

~~~markdown
# <Provider> — preparation

| | |
|---|---|
| **Docs** | <url, or "url — unreachable from here (403); read <what> instead"> |
| **OpenAPI** | <same form, or "none published"> |
| **Primary resource** | `<resource>` |

## Motions

| motion | what the caller is doing | provider endpoints |
|---|---|---|
| <name> | <one clause> | `POST /v1/images`, `GET /v1/images/{uid}` |

## Surface

| operation | algebra | provider endpoints | resolves with |
|---|---|---|---|
| `image.create` | `call + poll` | `POST /v1/images`, `GET /v1/images/{uid}` | the finished image |
| `image.submit` | `call` | `POST /v1/images` | the handle |
| `image.get` | `call` | `GET /v1/images/{uid}` | the record |

## Lifecycle — `<primary resource>`

| verb | provider endpoint | exposed as |
|---|---|---|
| create | `POST /v1/images` | `image.create`, `image.submit` |
| read | `GET /v1/images/{uid}` | `image.get` |
| list | `GET /v1/images` | `image.list` |
| update | — | — |
| delete | — | — |

## Out of scope

| what | why |
|---|---|
| `POST /v1/accounts` | instance administration |

## Open questions

- <a judgement the specification agent should know was a judgement>
~~~

Write `—` in the lifecycle table where the provider has no such endpoint;
that is an answer. A verb the provider offers and the plugin does not
expose needs a reason here and in the specification, not silence.

## Style

Tables, endpoints, and short clauses. No prose paragraphs, no alternatives
considered, no restating what the `plugin` skill already says.
