export const meta = {
  name: 'resonate-plugin-factory',
  description: 'Pick the most popular unimplemented plugin that clears the simplicity bar, then specify → review → apply findings → implement (Opus clean-room agents in a throwaway clone)',
  phases: [
    { title: 'Pick', detail: 'shortlist unchecked plugins that clear the simplicity bar, take the most popular' },
    { title: 'Checkout', detail: 'fresh clone into a temp dir, branch named after the plugin' },
    { title: 'Specify', detail: 'plugin-specify skill', model: 'opus' },
    { title: 'Review', detail: 'plugin-review skill', model: 'opus' },
    { title: 'Fix', detail: 'apply review findings to the specification', model: 'opus' },
    { title: 'Implement', detail: 'plugin-implement skill', model: 'opus' },
    { title: 'Finalize', detail: 'tick Plugins.md, commit, push the branch' },
  ],
}

const ORIGIN = 'https://github.com/resonatehq/resonate-plugins.git'
const GH = 'resonatehq/resonate-plugins'

// ── Pick ────────────────────────────────────────────────────────────────
phase('Pick')

const PICK = {
  type: 'object',
  properties: {
    provider: { type: 'string', description: 'Display name exactly as it appears in Plugins.md' },
    scheme: { type: 'string', description: 'Plugin scheme: lowercase, no separators (the plugins/<scheme>/ folder name)' },
    reason: { type: 'string', description: 'One sentence: why this is the best simple-and-popular candidate' },
    simplicity: { type: 'string', description: 'The evidence for the simplicity bar: auth model, operation surface, whether an OpenAPI document and a Docker image exist' },
    popularity: { type: 'string', description: 'The evidence for popularity, with the numbers found: GitHub stars, package downloads, catalogue presence, or market position' },
    runners_up: { type: 'string', description: 'The other shortlisted candidates and why each lost' },
  },
  required: ['provider', 'scheme', 'reason', 'simplicity', 'popularity', 'runners_up'],
}

const pick = await agent(
  `Fetch https://raw.githubusercontent.com/${GH}/main/Plugins.md (a table: ` +
  `Plugin | Spec | Impl with [x]/[ ] cells) and list the existing plugin folders with ` +
  `\`gh api repos/${GH}/contents/plugins --jq '.[].name'\`. Also list open plugin branches ` +
  `with \`git ls-remote --heads ${ORIGIN}\` — a plugin with an open branch is taken. ` +
  `Consider only plugins unchecked in BOTH columns, with no folder and no branch. ` +
  `Among those, choose the one at the INTERSECTION OF SIMPLE AND POPULAR, in two steps.\n\n` +
  `Step 1 — simplicity is a BAR, not a ranking. A candidate clears it if it has: a public, ` +
  `well-documented REST API (published OpenAPI is a strong plus), single-token auth, a small ` +
  `operation surface (one obvious unit of work plus a few reads), and ideally a Docker image ` +
  `so tests can run live. Providers requiring paid accounts, OAuth dances, or enterprise ` +
  `licenses do not clear the bar. Shortlist EVERY remaining candidate that clears it — ` +
  `expect several, and do not stop at the first one.\n\n` +
  `Step 2 — rank the shortlist by POPULARITY and take the most popular. Popularity means how ` +
  `widely the provider is actually used: GitHub stars and recent commit activity for ` +
  `open-source providers, package-registry download counts for its official clients, presence ` +
  `in integration catalogues (Zapier, n8n, Airbyte, Temporal), and general market position. ` +
  `Look these numbers up rather than relying on memory, and cite the figures you find. Break ` +
  `ties toward the provider a Resonate user is more likely to already run.\n\n` +
  `A very popular provider that misses the simplicity bar is still out — the bar is not traded ` +
  `away for popularity. Report the shortlist and why each runner-up lost.`,
  { schema: PICK, model: 'opus', label: 'pick' },
)

log(`picked ${pick.provider} (${pick.scheme}): ${pick.reason}`)
log(`popularity: ${pick.popularity}`)
log(`runners-up: ${pick.runners_up}`)

const repo = `/tmp/resonate-plugin-factory/${pick.scheme}`
const spec = `${repo}/plugins/${pick.scheme}/spec/specification.md`

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
  `Docker is available for §5 where the provider is self-hostable. Do not commit or push. ` +
  `Your final message: the file path, the operations you chose, and any points where the ` +
  `skill left you uncertain.`,
  { model: 'opus', label: `specify:${pick.scheme}` },
)

// ── Review ──────────────────────────────────────────────────────────────
phase('Review')

const REVIEW = {
  type: 'object',
  properties: {
    verdict: { type: 'string', enum: ['approve', 'approve-with-fixes', 'reject'] },
    has_findings: { type: 'boolean' },
    findings_report: { type: 'string', description: 'The full findings report, verbatim, ranked as the skill prescribes' },
  },
  required: ['verdict', 'has_findings', 'findings_report'],
}

const review = await agent(
  `Read ${repo}/skills/plugin-review/SKILL.md and follow it exactly to review ${spec}. ` +
  `Network access is available; Docker is available for the §5 environment where the ` +
  `specification has one. Where executed evidence requires credentials that are not ` +
  `available, record that and continue — do not sign up for accounts. ` +
  `Do not commit or push. Fill the Reviewed by row per the skill.`,
  { schema: REVIEW, model: 'opus', label: `review:${pick.scheme}` },
)

log(`review verdict: ${review.verdict}`)

// ── Fix ─────────────────────────────────────────────────────────────────
if (review.has_findings) {
  phase('Fix')
  await agent(
    `The specification ${spec} was reviewed; the findings below are authoritative. ` +
    `Read ${repo}/skills/plugin-specify/SKILL.md (the format the document must conform to), ` +
    `then apply every finding to the specification — the reviewer's corrections, not workarounds. ` +
    `Where a finding proposes a rule change to a skill rather than a spec change, skip it and ` +
    `say so. Re-run python3 ${repo}/skills/plugin-specify/lint.py ${spec} until clean. ` +
    `Do not commit or push.\n\n--- FINDINGS ---\n${review.findings_report}`,
    { model: 'opus', label: `fix:${pick.scheme}` },
  )
} else {
  log('no findings — skipping Fix')
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
  `Report: what you built, actual per-test results (including the §5 live run ` +
  `and the end-to-end run, verbatim failures if any), and any points where the skill or ` +
  `specification left you uncertain or blocked.`,
  { schema: IMPL, model: 'opus', label: `implement:${pick.scheme}` },
)

// ── Finalize ────────────────────────────────────────────────────────────
phase('Finalize')

const tick = impl.tests_passed
  ? 'tick BOTH the Spec and Impl cells of that row to [x]'
  : 'tick ONLY the Spec cell to [x]; leave Impl as [ ] (tests did not fully pass)'

await agent(
  `In ${repo} on branch ${pick.scheme} (verify with git branch --show-current): ` +
  `edit Plugins.md — find the row "| ${pick.provider} |" and ${tick}. ` +
  `Then git add -A, commit with message "${pick.scheme}: specification + implementation ` +
  `(plugin factory)" ending with the trailer "Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>", ` +
  `and git push -u origin ${pick.scheme}. Confirm the push succeeded, then stop any ` +
  `plugin-${pick.scheme}-test container that is still running.`,
  { effort: 'low', label: `finalize:${pick.scheme}` },
)

return {
  provider: pick.provider,
  scheme: pick.scheme,
  pick_rationale: { reason: pick.reason, simplicity: pick.simplicity, popularity: pick.popularity, runners_up: pick.runners_up },
  review_verdict: review.verdict,
  tests_passed: impl.tests_passed,
  implementation_report: impl.report,
  note: `Branch ${pick.scheme} pushed to ${GH} from throwaway clone ${repo} (safe to delete). Review and merge by hand.`,
}
