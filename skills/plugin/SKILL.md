---
name: plugin
description: What a Resonate transport plugin is, and the approach every plugin takes — the entry point to plugin-prepare, plugin-specify, plugin-review and plugin-implement.
---

# plugin

## The goal

A plugin exposes a provider's API as durable promises, so that any Resonate
SDK can call it as if it were a locally defined function. One integration,
every language, no client library.

Where an action runs long, the promise carries await semantics on top for
free. That is the icing. It is not the qualification.

## The approach

A provider that cannot hold an HTTP request open splits one action in two:
submit, then poll. A durable promise can hold. So the plugin collapses the
split — the default operation submits *and* waits, and resolves with the
result.

Support the resource's whole lifecycle. Half an API is a plugin a caller
abandons the moment they need the other half.

## The algebra

Each operation is written as what its implementation must do:

| atom | |
|---|---|
| `call` | one request; its answer is the outcome |
| `poll` | re-read until a terminal state |
| `find` | look up work an earlier delivery may already have started |
| `+` | then |
| `?` | only if needed |

| algebra | Invocation | Monitoring |
|---|---|---|
| `call` | `read`, `create`, `create_idempotent` | `request_response` |
| `call + poll` | `create`, `create_idempotent` | `request_poll` |
| `find? + call` | `fetch_then_create` | `request_response` |
| `find? + call + poll` | `fetch_then_create` | `request_poll` |

## Bannerbear, worked

The provider: `POST /v1/images` → `202` with a pending record;
`GET /v1/images/{uid}` → poll until `completed` or `failed`.

| operation | algebra | resolves with |
|---|---|---|
| `image.create` | `call + poll` | the finished image |
| `image.submit` | `call` | the handle — accepted, not finished |
| `image.get` | `call` | the record |

Bannerbear hands you two calls because HTTP made it. A caller wants one, so
`image.create` is both of them and is the default. `image.submit` exists
only because a render is slow enough that someone might not want to wait;
`submit` is the reserved verb for that everywhere, and never appears
without its waiting counterpart.

## The pipeline

| skill | produces |
|---|---|
| [plugin-prepare](../plugin-prepare) | the design: motions, resources, the surface to support, the algebra → `spec/preparation.md` |
| [plugin-specify](../plugin-specify) | the specification → `spec/specification.md` |
| [plugin-review](../plugin-review) | §0 API Surface derived independently, then findings against the specification |
| [plugin-implement](../plugin-implement) | the Rust crate → `src/` |
