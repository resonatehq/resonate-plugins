---
name: plugin-bind
description: Write a plugin's SDK bindings — one typed descriptor per operation, per SDK, so a caller writes ctx.rpc(openrouter.completion, args) instead of a raw func string and target. Writes plugins/<name>/sdk/<language>/.
---

# plugin-bind

Read [plugin](../plugin) first. Write `plugins/<name>/sdk/<language>/` for
every SDK, from `plugins/<name>/spec/specification.md`. Nothing here is
invented: every name, argument and value comes from §4 of that document.

## What a binding is

A caller invokes a local function with `ctx.rpc(fn, args)`, where `fn` is a
pointer the SDK resolves. A plugin operation has no pointer — it is a func
name and an address. So the binding exports, per operation, a **descriptor**
that carries both and is typed by the operation's schemas:

```ts
{ kind: "integration", func: "completion.create", target: "openrouter://default" }
```

The call site then reads the same as a local one:

```ts
const image = await ctx.rpc(bannerbear.image.create, { template: "...", modifications: [...] })
```

That is the whole point: the same three tokens — verb, arguments, await —
whether the function is in this file or behind a provider's API.

## The SDK prerequisite

The descriptor form needs one overload the SDKs do not have yet. TypeScript
already carries the two it desugars to (`src/context.ts`):

```ts
rpc<F extends Func>(func: F, ...args: ParamsWithOptions<F>): RFC<Return<F>>;
rpc<T>(func: string, ...args: any[]): RFC<T>;
```

so the addition is a third:

```ts
rpc<A, R>(desc: Integration<A, R>, args: A, opts?: Partial<Options>): RFC<R>;
```

which forwards to the string form with `opts.target = desc.target`. Each
other SDK needs the equivalent against its own call shape — read that SDK's
source for the real signature rather than assuming this one.

Say plainly in your final message that the bindings are inert until the
overload lands, and name the SDKs still missing it. A binding that compiles
and cannot be called is worse than one that does not exist, because it
looks finished.

## Procedure

1. Read the specification. Every operation's func name is its §4 heading;
   the target is `<scheme>://{instance}`, defaulting to `default` per §1.
2. For each SDK, read its README and its `Context` source for the real call
   shape and naming conventions. Do not carry TypeScript's shape into
   Python, Go or Java.
3. Translate §4.N.1 into the argument type and §4.N.2 Resolved into the
   value type. `= response.body` means the provider's body unmodified: type
   it as the SDK's open JSON type, not as a struct you invent.
4. Translate §4.N.2 Rejected into the error type — one variant per `code`
   in the enum. A caller must be able to branch on `not_found` without
   matching a string.
5. Emit one descriptor per operation, grouped by resource, so
   `bannerbear.image.create` and `bannerbear.image.get` read as a family.
6. Ship the §2 configuration keys as a documented type where the SDK can
   carry it, so a caller sees what the server side needs.

## Rules

- The binding is generated from the specification and adds nothing. No
  convenience wrappers, no retries, no client-side polling — the plugin
  already does all of that, on the server, durably. A binding that retries
  is a bug.
- Names match §4 exactly. `execution.output` is `execution.output`, not
  `getExecutionLogs`.
- One file per plugin per SDK where the language allows it. A caller
  installs one thing.
- Where the specification says an operation is `call + poll`, the binding
  says nothing extra: waiting is the promise's job and is invisible here.
  That invisibility is the feature.
