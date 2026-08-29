export const meta = {
  name: 'resonate-plugin-factory',
  description: 'Pick the most popular unimplemented plugin that clears the simplicity bar, then specify → review → apply findings → implement (Opus clean-room agents in a throwaway clone)',
  phases: [
    { title: 'Pick', detail: 'shortlist unchecked plugins that clear the simplicity bar, take the most popular' },
    { title: 'Checkout', detail: 'fresh clone into a temp dir, branch named after the plugin' },
    { title: 'Specify', detail: 'plugin-specify skill, gated on lint --pre-review' },
    { title: 'Review', detail: 'plugin-review skill, re-run after each fix until the verdict holds' },
    { title: 'Fix', detail: 'apply review findings to the specification' },
    { title: 'Implement', detail: 'plugin-implement skill' },
    { title: 'Verify', detail: 'check.py and cargo test, run by an agent that did not write the code' },
    { title: 'Finalize', detail: 'record the review, tick Plugins.md, rebase, commit, push the branch' },
  ],
}

const ORIGIN = 'https://github.com/resonatehq/resonate-plugins.git'
const GH = 'resonatehq/resonate-plugins'
const ROOT = '/tmp/resonate-plugin-factory'

// ── Pick ────────────────────────────────────────────────────────────────
phase('Pick')

const PICK = {
  type: 'object',
  properties: {
    provider: { type: 'string', description: 'Display name exactly as it appears in Plugins.md' },
    scheme: { type: 'string', description: 'Plugin scheme: lowercase, no separators (the plugins/<scheme>/ folder name)' },
    reason: { type: 'string', description: 'One sentence: why this is the best simple-and-popular candidate' },
    operations: {
      type: 'string',
      description:
        'The operations the plugin will expose, each as a quoted endpoint path and method: the provider action(s) a caller would call, and the reads needed to build their arguments. Where an action is long-running, name the separate endpoint that observes it to a terminal state and the states it can end in.',
    },
    simplicity: { type: 'string', description: 'The evidence for the simplicity bar: auth model, operation surface, whether an OpenAPI document and a Docker image exist' },
    popularity: { type: 'string', description: 'The evidence for popularity, with the numbers found: GitHub stars, package downloads, catalogue presence, or market position' },
    runners_up: { type: 'string', description: 'The other shortlisted candidates and why each lost' },
  },
  required: ['provider', 'scheme', 'reason', 'operations', 'simplicity', 'popularity', 'runners_up'],
}

const pick = await agent(
  `Fetch https://raw.githubusercontent.com/${GH}/main/Plugins.md (a table: ` +
  `Plugin | Spec | Impl with [x]/[ ] cells). List the open plugin branches with ` +
  `\`git ls-remote --heads ${ORIGIN}\` — a plugin with an open branch is taken — and the ` +
  `claims of concurrent runs on this machine with \`ls ${ROOT}\`. Note there is no \`gh\` ` +
  `CLI here; use raw.githubusercontent.com and git over HTTPS for everything.\n\n` +
  `Consider only plugins unchecked in BOTH columns, with no branch and no claim. ` +
  `Among those, choose the one at the INTERSECTION OF SIMPLE AND POPULAR, in three steps. ` +
  `Simplicity is a BAR, not a ranking; only popularity ranks. Step 1 is neither — it is the ` +
  `homework the other two are judged on.\n\n` +

  `Step 1 — NAME THE OPERATIONS. A plugin exposes a provider's API as durable promises so that ` +
  `any Resonate SDK can call it as if it were a locally defined function. What makes a good ` +
  `candidate is therefore an API worth reaching that way — not a slow one. A provider whose ` +
  `every call answers in one round trip is a fine plugin; where an action does run long, the ` +
  `promise gives it await semantics for free, which is a bonus on top and never the reason to ` +
  `pick or to skip a provider.\n\n` +
  `So: from the provider's live API documentation, name the operations the plugin will actually ` +
  `expose — the provider action or actions a caller would make, and the reads needed to build ` +
  `their arguments. Quote each endpoint's method and path. Where an action is long-running, also ` +
  `name the separate endpoint that observes it to a terminal state and the states it can end in, ` +
  `because that is what the specification will have to write. These are the operations the ` +
  `specification is expected to contain, so name them as precisely as §4 will have to; a ` +
  `candidate you cannot describe at this resolution has not been researched yet.\n\n` +

  `Step 2 — SIMPLICITY. A candidate clears it if it has: a public, well-documented REST API ` +
  `(published OpenAPI is a strong plus), single-token auth, a small operation surface (the ` +
  `operations from step 1 and little else), and ideally a Docker image so tests can run live. Providers ` +
  `requiring paid accounts, OAuth dances, or enterprise licenses do not clear the bar. A large ` +
  `API is not disqualifying when the operations of step 1 are a small, well-bounded corner of it — ` +
  `judge the surface the plugin will touch, not the provider's total endpoint count. Shortlist ` +
  `EVERY remaining candidate that clears it — expect several, and do not stop at the ` +
  `first one.\n\n` +

  `Step 3 — rank the shortlist by POPULARITY and take the most popular. Popularity means how ` +
  `widely the provider is actually used: GitHub stars and recent commit activity for ` +
  `open-source providers, package-registry download counts for its official clients, presence ` +
  `in integration catalogues (Zapier, n8n, Airbyte, Temporal), and general market position. ` +
  `Look these numbers up rather than relying on memory, and cite the figures you find. Break ` +
  `ties toward the provider a Resonate user is more likely to already run.\n\n` +

  `The bar is not traded away for popularity. Report the shortlist and why each runner-up lost.\n\n` +

  `Finally, claim your choice so a concurrent run cannot take it: ` +
  `\`mkdir -p ${ROOT} && mkdir ${ROOT}/<scheme>.claim\`. mkdir fails if the directory exists, ` +
  `which means another run claimed it first — in that case go back to your shortlist, take the ` +
  `next candidate, and claim that one. Report only a scheme you successfully claimed.`,
  { schema: PICK, model: 'opus', label: 'pick' },
)

log(`picked ${pick.provider} (${pick.scheme}): ${pick.reason}`)
log(`operations: ${pick.operations}`)
log(`popularity: ${pick.popularity}`)
log(`runners-up: ${pick.runners_up}`)

const repo = `${ROOT}/${pick.scheme}`
const spec = `${repo}/plugins/${pick.scheme}/spec/specification.md`
const lint = `python3 ${repo}/skills/plugin-specify/lint.py`
const check = `python3 ${repo}/skills/plugin-implement/check.py`

// ── Checkout ────────────────────────────────────────────────────────────
phase('Checkout')

await agent(
  `Run: rm -rf ${repo} && git clone ${ORIGIN} ${repo} && ` +
  `git -C ${repo} checkout -b ${pick.scheme}. ` +
  `Confirm with git -C ${repo} branch --show-current.`,
  { effort: 'low', label: `checkout:${pick.scheme}` },
)

// ── Specify ─────────────────────────────────────────────────────────────
phase('Specify')

await agent(
  `Read ${repo}/skills/plugin-specify/SKILL.md and follow it exactly to create the plugin ` +
  `specification for ${pick.provider}. Write the document to ${spec} (create the directories; ` +
  `also create ${repo}/plugins/${pick.scheme}/README.md containing only "# ${pick.provider}"). ` +
  `Docker is available for §5 where the provider is self-hostable. Do not commit or push.\n\n` +
  `The candidate was selected on the strength of these operations, which §4 is expected to ` +
  `contain:\n${pick.operations}\n\n` +
  `Follow the skill, not this list, if the live documentation disagrees with it — but say ` +
  `plainly in your final message where you diverged and why. A specification that quietly ` +
  `contains a different set of operations than the one the candidate was chosen for means the ` +
  `choice rested on something that was never checked.\n\n` +
  `Before you finish, \`${lint} --pre-review ${spec}\` must exit 0. It is stricter than the ` +
  `plain invocation on exactly one point: the Reviewed by row must still be empty, because ` +
  `review is a separate step by a separate agent. Do not fill it.\n\n` +
  `Your final message: the file path, the operations you chose, the lint output, and any ` +
  `points where the skill left you uncertain.`,
  { model: 'opus', label: `specify:${pick.scheme}` },
)

const specGate = await agent(
  `Run \`${lint} --pre-review ${spec}\` and report its exit code and full output verbatim. ` +
  `Change nothing.`,
  {
    effort: 'low',
    label: `gate:specify:${pick.scheme}`,
    schema: {
      type: 'object',
      properties: {
        clean: { type: 'boolean', description: 'true only if the linter exited 0' },
        output: { type: 'string' },
      },
      required: ['clean', 'output'],
    },
  },
)

if (!specGate.clean) {
  log(`specification does not pass lint --pre-review — stopping:\n${specGate.output}`)
  return {
    provider: pick.provider,
    scheme: pick.scheme,
    stopped_at: 'Specify',
    reason: 'the specification does not pass lint --pre-review',
    lint_output: specGate.output,
    pick_rationale: { reason: pick.reason, operations: pick.operations, simplicity: pick.simplicity, popularity: pick.popularity, runners_up: pick.runners_up },
    note: `Nothing was pushed. The clone is ${repo}; the claim is ${ROOT}/${pick.scheme}.claim (remove it to free the plugin).`,
  }
}

// ── Review → Fix → Review ───────────────────────────────────────────────
// The verdict is a gate, not a log line, and the attestation must describe
// the document that ships: a review of the pre-fix specification says
// nothing about the one Fix leaves behind. So the loop re-reviews.

const REVIEW = {
  type: 'object',
  properties: {
    verdict: { type: 'string', enum: ['approve', 'approve-with-fixes', 'reject'] },
    has_findings: { type: 'boolean' },
    findings_report: { type: 'string', description: 'The full findings report, verbatim, ranked as the skill prescribes' },
  },
  required: ['verdict', 'has_findings', 'findings_report'],
}

const ROUNDS = 3
let review
let round = 0

for (round = 1; round <= ROUNDS; round++) {
  phase('Review')
  review = await agent(
    `Review round ${round} of at most ${ROUNDS}.\n\n` +
    `Read ${repo}/skills/plugin-review/SKILL.md and follow it exactly to review ${spec}. ` +
    `Network access is available; Docker is available for the §5 environment where the ` +
    `specification has one. Where executed evidence requires credentials that are not ` +
    `available, record that and continue — do not sign up for accounts. ` +
    `Do not commit or push. Fill the Reviewed by row per the skill — overwrite whatever is ` +
    `there, so the row always names the review of the document as it now stands.` +
    (round > 1
      ? `\n\nThis is a re-review after the previous round's findings were applied. You are ` +
        `reviewing the document as it now stands, not the diff: find defects, including any ` +
        `the previous fix introduced. The previous round's findings, for context only — do ` +
        `not assume they were applied correctly:\n\n${review.findings_report}`
      : ''),
    { schema: REVIEW, model: 'opus', label: `review:${pick.scheme}:${round}` },
  )
  log(`review round ${round} verdict: ${review.verdict}`)

  if (!review.has_findings) {
    log('no findings — nothing to fix')
    break
  }
  if (review.verdict === 'approve') break
  if (round === ROUNDS) break

  phase('Fix')
  await agent(
    `The specification ${spec} was reviewed; the findings below are authoritative. ` +
    `Read ${repo}/skills/plugin-specify/SKILL.md (the format the document must conform to), ` +
    `then apply every finding to the specification — the reviewer's corrections, not workarounds. ` +
    `A finding often has consequences beyond the line it names: if correcting a settlement ` +
    `leaves a §5 fixture exercising the wrong path, or a Rejected code with no way to reach it, ` +
    `carry the correction through to §5 and the schemas too. ` +
    `Where a finding proposes a rule change to a skill rather than a spec change, skip it and ` +
    `say so. Where you believe a finding is WRONG, say so with the evidence rather than ` +
    `applying it — but the bar is evidence, not preference. ` +
    `Re-run \`${lint} ${spec}\` until clean. Do not commit or push.\n\n` +
    `--- FINDINGS (round ${round}) ---\n${review.findings_report}`,
    { model: 'opus', label: `fix:${pick.scheme}:${round}` },
  )
}

if (review.verdict === 'reject') {
  log(`still rejected after ${round} round(s) — stopping before implementation`)
  return {
    provider: pick.provider,
    scheme: pick.scheme,
    stopped_at: 'Review',
    reason: `the specification was still rejected after ${round} review round(s)`,
    review_verdict: review.verdict,
    findings_report: review.findings_report,
    pick_rationale: { reason: pick.reason, operations: pick.operations, simplicity: pick.simplicity, popularity: pick.popularity, runners_up: pick.runners_up },
    note: `Nothing was pushed. The specification is at ${spec} in clone ${repo}; the claim is ${ROOT}/${pick.scheme}.claim (remove it to free the plugin).`,
  }
}

// ── Implement ───────────────────────────────────────────────────────────
phase('Implement')

const IMPL = {
  type: 'object',
  properties: {
    tests_passed: { type: 'boolean', description: 'true only if cargo test (including §5 live and e2e) passed completely' },
    report: { type: 'string', description: 'What was built, actual per-test results (verbatim failures if any), and uncertainty points' },
  },
  required: ['tests_passed', 'report'],
}

const impl = await agent(
  `Read ${repo}/skills/plugin-implement/SKILL.md and follow it exactly to implement the ` +
  `${pick.provider} plugin from ${spec} into ${repo}/plugins/${pick.scheme}/src. ` +
  `Docker is available for the §5 environment; network access is available for cargo. ` +
  `Do not commit or push. Do not modify the skills or the reference template — if the ` +
  `specification is wrong, the skill tells you what to do. ` +
  `Before you finish, \`${check} ${repo}/plugins/${pick.scheme}\` must exit 0. ` +
  `Leave the §5 provider running: the next step re-runs the tests. ` +
  `Report: what you built, actual per-test results (including the §5 live run ` +
  `and the end-to-end run, verbatim failures if any), and any points where the skill or ` +
  `specification left you uncertain or blocked.`,
  { schema: IMPL, model: 'opus', label: `implement:${pick.scheme}` },
)

// ── Verify ──────────────────────────────────────────────────────────────
// tests_passed above is the implementer's own account of its own work. This
// step is the only independent evidence the workflow has.
phase('Verify')

const verify = await agent(
  `Do not read, write or repair any code — you are checking someone else's work and your ` +
  `only job is to report what the tools say.\n\n` +
  `1. Run \`${check} ${repo}/plugins/${pick.scheme}\` and capture its output and exit code.\n` +
  `2. The §5 provider from the specification should still be running. If it is not, run the ` +
  `   §5.2 block from ${spec} to bring it up, exactly as written.\n` +
  `3. In ${repo}/plugins/${pick.scheme}/src run \`cargo test --all-targets\` with the ` +
  `   \`{SCHEME}_{KEY}\` environment 5.2 exports, and capture the full summary lines and ` +
  `   every failure verbatim.\n\n` +
  `Report exactly what happened. If something fails, that is the answer — do not fix it, and ` +
  `do not describe a failure as passing.`,
  {
    model: 'opus',
    label: `verify:${pick.scheme}`,
    schema: {
      type: 'object',
      properties: {
        check_clean: { type: 'boolean', description: 'true only if check.py exited 0' },
        tests_passed: { type: 'boolean', description: 'true only if cargo test --all-targets passed with zero failures' },
        evidence: { type: 'string', description: 'check.py output and the cargo test summary lines, verbatim' },
      },
      required: ['check_clean', 'tests_passed', 'evidence'],
    },
  },
)

log(`verify: check.py ${verify.check_clean ? 'clean' : 'FAILED'}, cargo test ${verify.tests_passed ? 'passed' : 'FAILED'}`)
if (verify.tests_passed !== impl.tests_passed) {
  log(`implementer reported tests_passed=${impl.tests_passed}; independent run says ${verify.tests_passed}`)
}

// ── Finalize ────────────────────────────────────────────────────────────
phase('Finalize')

const green = verify.check_clean && verify.tests_passed
const tick = green
  ? 'tick BOTH the Spec and Impl cells of that row to [x]'
  : 'tick ONLY the Spec cell to [x]; leave Impl as [ ] (the independent verification did not pass)'

await agent(
  `In ${repo} on branch ${pick.scheme} (verify with git branch --show-current):\n\n` +
  `1. Write the review trail to plugins/${pick.scheme}/spec/review.md: the final verdict ` +
  `   (${review.verdict}) after ${round} round(s), then the findings report below verbatim. ` +
  `   It is the only durable record that these findings were raised and answered; without it ` +
  `   the Reviewed by row asserts a review nobody can read.\n` +
  `2. Bring the branch up to date: git fetch origin main && git merge origin/main. Resolve ` +
  `   conflicts if any — Plugins.md is edited by every run, so expect one there.\n` +
  `3. Edit Plugins.md — find the row for "${pick.provider}" and ${tick}.\n` +
  `4. git add -A. Then run \`git status --porcelain\` and confirm every staged path is a file ` +
  `   this plugin owns: plugins/${pick.scheme}/** and Plugins.md, nothing else. Nothing ` +
  `   .gitignore matches may be committed — never force-add. Unstage anything else.\n` +
  `5. Commit with message "${pick.scheme}: specification + implementation (plugin factory)" ` +
  `   ending with the trailer "Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>", and ` +
  `   \`git push -u origin ${pick.scheme}\`. Confirm the push succeeded.\n` +
  `6. Clean up this run: remove the plugin-${pick.scheme}-test container AND its image, then ` +
  `   \`docker builder prune -f\`. A run that only stops the container leaves gigabytes behind ` +
  `   for the next one. Finally remove the claim ${ROOT}/${pick.scheme}.claim.\n\n` +
  `--- FINDINGS ---\n${review.findings_report}`,
  { effort: 'low', label: `finalize:${pick.scheme}` },
)

return {
  provider: pick.provider,
  scheme: pick.scheme,
  pick_rationale: { reason: pick.reason, operations: pick.operations, simplicity: pick.simplicity, popularity: pick.popularity, runners_up: pick.runners_up },
  review_verdict: review.verdict,
  review_rounds: round,
  findings_report: review.findings_report,
  tests_passed: green,
  implementer_claimed: impl.tests_passed,
  verification_evidence: verify.evidence,
  implementation_report: impl.report,
  note: `Branch ${pick.scheme} pushed to ${GH} from throwaway clone ${repo} (safe to delete). Review and merge by hand.`,
}
