---
name: plugin-review
description: Review a plugin specification — verify it against the live provider, execute the implementation manually, fill Reviewed by, report findings.
---

# plugin-review

Review `plugins/<name>/spec/specification.md`. You are not the author: your
job is to find defects. Scope is `spec/` — the plugin's `README.md` and
`src/` are out of scope. Fill the `Reviewed by` row only after completing
every step below; your final message is the findings report.

## Procedure

1. Read `skills/plugin-specify/SKILL.md` (the format and the plugin
   contract the document is written against), then the specification's
   header and §3 only — enough to know the provider, its API base, and
   where its documentation lives. **Do not read §4 yet.**
2. Fetch the provider's live documentation and the OpenAPI spec from the
   header's Notes table. Do not use memorized API knowledge. If a URL the
   Notes table cites does not resolve from here, the row must already say
   so and name what was read instead; a bare citation of a source nobody
   could fetch is a **minor** finding, because it presents a mirror's
   claims as the provider's. Verify against the best source you can reach
   and record which one it was — but never substitute recall for a source.
3. From that documentation alone, write **§0 API Surface**: the API
   surface a caller wanting to drive this provider from an SDK wants.
   Name each call by its method and path, grouped by resource, and say in
   a clause what a caller uses it for. Judge it as someone building on
   this provider: the primary resource and its lifecycle, the durable
   units it offers, the reads that find ids and constrain arguments.
   Apply plugin-specify's Bounds — instance administration and transport
   mechanics are out.

   Name the primary resource and close §0 with its lifecycle table, one
   row per verb, so completeness is a fact rather than an impression:

   | verb | provider endpoint | in §4 |
   |---|---|---|
   | create | `POST /api/v2/tickets` | |
   | read | `GET /api/v2/tickets/{id}` | |
   | list | `GET /api/v2/tickets` | |
   | update | `PUT /api/v2/tickets/{id}` | |
   | delete | `DELETE /api/v2/tickets/{id}` | |

   Write `—` where the provider has no such endpoint; that is an answer,
   not a gap. Fill the last column when you read §4.

   Where the provider splits an action into a submit and a poll, the
   lifecycle row is the action, and §4 should answer it with one operation
   that submits and waits — the split is HTTP's limitation, not the
   caller's contract, and collapsing it is what the plugin is for. §4
   reproducing the split as two operations, so that a caller must submit
   and then poll for themselves, is a **significant** finding. A
   `<resource>.submit` alongside the waiting operation is the sanctioned
   fire-and-forget variant and is not that; `submit` on its own is. Batch and scoped
   variants belong to the verb they implement, not to rows of their own.
   Supporting a resource means supporting its lifecycle: half an API is a
   plugin a caller has to abandon the moment they need the other half.

   Write this before reading §4 — and before `spec/preparation.md`, which
   is the author's plan for the same question and would anchor you to it —
   and do not revise it afterwards. Its whole value is being an
   independent answer: derived from the provider, not recovered from the
   documents under review. Once §0 stands, read both; where the plan and
   §0 disagree, that disagreement is the finding. Then read §4 and compare.
   Coverage gaps are findings like any other, cited against §0:

   - a call a caller plainly needs and cannot make → **significant**. Any
     lifecycle row where the provider has an endpoint and §4 does not
     expose it is such a call by default; a plugin that can create a
     record but not read or update it is not an integration a caller can
     build on. Rank it lower only if the specification gives a reason the
     verb is unreachable, and say why you accepted it.
   - a peripheral call a caller would occasionally want → **minor**.
   - an operation §4 exposes that §0 does not want — administration,
     transport mechanics, an inflated read — → **minor**, since surface
     nobody asked for is surface to maintain and to get wrong.

   Where §4 and §0 disagree because the provider's API cannot support
   what a caller wants (no trigger endpoint, a resource only writable out
   of band), that is not a finding against the specification: say so in
   §0 and move on. A plugin cannot expose what the provider does not.
4. Verify facts: every endpoint, method, field name, status code, enum,
   default, and idempotency claim against the fetched documentation.
   Where the documentation is silent, the provider's shipped source and
   configuration (inside the container) are sanctioned evidence — record
   what proved each claim. When sources disagree, or live behaviour
   contradicts a published statement: executed evidence beats the OpenAPI
   beats the narrative docs — and report the contradiction as its own
   finding.
5. Verify internal consistency:
   - every `= promise.param.x` in an Integration Request exists in the
     Promise Param Schema with the same type;
   - every `= response.body.x` in a Promise Value Schema exists in the
     Integration Response schema;
   - every `code` the Python constructs appears in the Rejected enum, and
     vice versa;
   - every `cfg.<key>` the Python reads appears in the §2 table;
   - each operation's Invocation and Monitoring classification matches
     what the code does and is the highest Invocation rung the provider
     documents;
   - Reject only on statuses the provider documents as permanent for that
     call; every `Exception("halt", ...)` site rests on a
     provider-documented operator-required condition; everything
     unclassified raises (release).
6. If the specification has a §5, execute the implementation manually:
   1. Run 5.1/5.2: write the 5.1 block to `spec/Dockerfile` (a generated,
      gitignored artifact — overwrite whatever is there; finding a
      pre-existing file is expected, not a finding), run the 5.2 block;
      it leaves the provider running and serviceable.
   2. Drop into the environment (`docker exec -it <container> python3`, or
      run Python on the host against the exported configuration — both
      have Python 3 and `requests`).
   3. Paste the specification's implementation snippets and define
      `sanitize` around them:
      ```python
      import hashlib
      def sanitize(pid): return hashlib.sha256(pid.encode()).hexdigest()[:32]
      ```
      Build `cfg` from the §2 table (a `SimpleNamespace`; `Duration`
      values need `.total_seconds()`) and a `promise` with `id`,
      `param = {"func": ..., "args": ...}`, and `timeout_at` in
      milliseconds. Verdicts are tuples:
      `("resolved", value)` / `("rejected", value)`; a raise is no
      verdict.
   4. Call every operation. Confirm: resolved values match the Resolved
      schema; each documented rejection case produces its `code`
      (exercise at least one per operation, e.g. an unknown resource);
      re-entering a multi-turn operation with the same promise id is safe
      (no duplicate external work); a transient failure raises rather
      than settling. Mind the provider's operational realities (e.g.
      seed or enable the resources the operations need). Seed whatever
      the provider's documented features allow to reach each rejection
      code; beyond documented features, report the code as unverified
      rather than engineering around it.
   5. Tear the environment down: remove the container and the image;
      leave the generated `spec/Dockerfile`.
7. Fill the empty `| **Reviewed by** | |` row with your model name and
   the date, exactly as `<Model Name>, YYYY-MM-DD` (e.g.
   `Claude Opus, 2026-08-26`). Make no other edits: propose fixes in the
   report, do not apply them.

## Report

Open with `# 0. API Surface` — the surface from step 3, verbatim as you
wrote it before reading §4 — then the coverage paragraph, then the
findings. A reader who disagrees with a coverage finding needs to see the
yardstick that produced it.

Findings ranked blocking / significant / minor / nit:

- **blocking** — a wrong settlement or a wrong classification (outcome
  class, Invocation rung): the implementation lies, or forgoes a
  guarantee the provider documents.
- **significant** — wrong wire behaviour: a request that cannot do what
  the specification claims.
- **minor** — a wrong declared fact (type, enum member, field).
- **nit** — incompleteness: an undeclared-but-present field (unless the
  implementation reads it — then minor), a missing constraint.

Coverage findings — §4 against §0 — take the rank step 3 gives them:
significant for a call a caller plainly needs, minor for a peripheral one
or for surface §0 does not want.

Each finding: section number, what is wrong, the correct fact with the URL
or the executed call that proves it — for a coverage finding, the §0 entry
it fails against. Do not list what is correct: §0 and a one-paragraph
coverage summary (what was executed and confirmed) are the only exceptions,
and they belong at the top rather than among the findings. End with one verdict, by rubric: any blocking finding → reject;
significant findings only → approve-with-fixes; minor and nit only →
approve.
