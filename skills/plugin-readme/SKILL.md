---
name: plugin-readme
description: Write a plugin's README — what the upstream application is, one representative example in every SDK, and what an operator must configure. Writes plugins/<name>/README.md.
---

# plugin-readme

Read [plugin](../plugin) first. Write `plugins/<name>/README.md` from
`spec/specification.md`, `spec/preparation.md`, and the bindings in `sdk/`.
The reader is someone deciding whether this plugin solves their problem, in
about ninety seconds.

## Procedure

1. **Say what the upstream application is** — two or three sentences, for
   someone who has never used it. What it does, who runs it, why a workflow
   would call it. Not marketing, not a feature list. A reader who already
   knows the provider should be able to skip it; a reader who does not
   should not have to leave the page.
2. **Pick one motion** from `preparation.md`'s Motions table — the one a
   caller would perform first, which is almost always the primary
   resource's `call + poll` action. One motion, not a tour.
3. **Show that same motion in every SDK**, from the descriptors in `sdk/`.
   The same operation, the same arguments, the same result used the same
   way. The point is that a reader can find their language and see that the
   shape does not change; that only works if the examples are genuinely
   parallel, so do not idiomatise one and not another.
4. **Explain the configuration** — every §2 key: what it is, where an
   operator gets it, what happens when it is absent. A key with an empty
   default cell is required and the reader must know that before they start.
5. **List the operations** as a table linking to the specification, with one
   clause each. This is a contents page, not documentation — the
   specification is the documentation.

## Template

~~~markdown
# <Provider>

<what the application is, 2–3 sentences>

## Example

<one sentence naming the motion and what it resolves with>

### TypeScript
```ts
...
```
### Python
```python
...
```
### Go
```go
...
```
### Java
```java
...
```

## Configuration

`[<scheme>]` in the server's configuration, or `[<scheme>.<instance>]` to
address it as `<scheme>://<instance>`.

| key | required | what it is |
|---|---|---|
| `base_url` | yes | Where the instance lives, e.g. `https://rundeck.acme.com` |
| `api_key` | yes | A token from <where the operator creates it> |
| `poll` | no, `30s` | How often a waiting operation re-checks |

## Operations

| operation | |
|---|---|
| [`job.run`](spec/specification.md#41-jobrun) | run a job and wait for the execution |
~~~

## Rules

- Every example must be runnable as written, against the operations the
  specification actually declares. An example that names an argument §4
  does not have is a defect, not a simplification.
- Show the waiting operation, not the fire-and-forget one. `submit` is the
  exception a caller reaches for later; `create` is what the plugin is for.
- Do not restate the specification. Schemas, status codes, rejection codes
  and terminal states live there and are linked, not copied.
- Do not explain durable promises. That is the repository's README; this
  page is about one provider.
- Where an SDK has no binding yet, say so in its place rather than omitting
  the language — a missing section reads as an oversight, a stated gap reads
  as a roadmap.
