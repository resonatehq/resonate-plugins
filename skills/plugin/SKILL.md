---
name: plugin
description: What a Resonate transport plugin is, and the approach every plugin takes — the entry point to plugin-prepare, plugin-specify, plugin-review and plugin-implement.
---

# plugin

## The goal

A plugin exposes a provider's API as durable promises, so that any Resonate
SDK can call it as if it were a locally defined function. One integration,
every language, no client library.

Literally as if local: where a caller writes `ctx.rpc(fn, args)` for a
function in their own codebase, a plugin's binding exports a descriptor
carrying the func name and the address, so the call site reads
`ctx.rpc(bannerbear.image.create, args)` — same three tokens, same await.

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

Classify every operation by what the provider makes a caller do. Two
values, and they are the whole vocabulary at this level:

| algebra | | Monitoring |
|---|---|---|
| `call` | one request; its answer is the outcome | `request_response` |
| `call + poll` | submit, then re-read until a terminal state | `request_poll` |

How an operation *begins* safely — a plain read, an idempotency key, or a
look-before-you-create — is a finer question, answered against the
provider's documentation once the surface is settled. That is the
specification's Invocation rung, not this.

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
| [plugin-bind](../plugin-bind) | a typed descriptor per operation, per SDK → `sdk/<language>/` |
| [plugin-readme](../plugin-readme) | what the provider is, one example per SDK, the config → `README.md` |
