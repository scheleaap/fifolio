export const meta = {
  name: 'work-it',
  description: 'Audit the spec, plan, implement work items, review independently, and commit on green',
  phases: [
    { title: 'Audit', detail: 'spec-auditor checks design/ against the code' },
    { title: 'Plan', detail: 'planner picks the next work item from PLAN.md' },
    { title: 'Implement', detail: 'implementer builds the item, then addresses findings' },
    { title: 'Review', detail: 'conformance-reviewer and test-reviewer, independently' },
    { title: 'Commit', detail: 'verify green and commit onto the run branch' },
  ],
}

// Bounds. MAX_ROUNDS is the fix loop cap from .claude/agents/README.md.
const MAX_ROUNDS = 3

// By default run until the plan is exhausted. HARD_CAP is a runaway backstop, not a policy:
// it sits well under the runtime's 1000-agent limit, and being hit is reported, never silent.
const HARD_CAP = 100
const requested = parseInt(String(args ?? ''), 10)
const MAX_ITEMS = Number.isFinite(requested) && requested > 0 ? Math.min(requested, HARD_CAP) : HARD_CAP
const OPEN_ENDED = !(Number.isFinite(requested) && requested > 0)

const VERDICT = {
  type: 'object',
  properties: {
    verdict: { type: 'string', enum: ['pass', 'fail'] },
    blockers: { type: 'integer' },
    majors: { type: 'integer' },
    minors: { type: 'integer' },
    decisions: { type: 'integer' },
    summary: { type: 'string' },
    findings: { type: 'string' },
  },
  required: ['verdict', 'blockers', 'majors', 'minors', 'decisions', 'summary', 'findings'],
}

const AUDIT = {
  type: 'object',
  properties: {
    verdict: { type: 'string', enum: ['pass', 'fail'] },
    blockers: { type: 'integer' },
    majors: { type: 'integer' },
    minors: { type: 'integer' },
    decisions: { type: 'integer' },
    blockedRequirements: { type: 'array', items: { type: 'string' } },
    summary: { type: 'string' },
    findings: { type: 'string' },
  },
  required: ['verdict', 'blockers', 'majors', 'minors', 'decisions', 'blockedRequirements', 'summary', 'findings'],
}

const PLAN = {
  type: 'object',
  properties: {
    nextItemId: { type: 'string' },
    nextItemTitle: { type: 'string' },
    requirements: { type: 'string' },
    acceptance: { type: 'string' },
    remaining: { type: 'integer' },
    decisions: { type: 'integer' },
    summary: { type: 'string' },
  },
  required: ['nextItemId', 'nextItemTitle', 'requirements', 'acceptance', 'remaining', 'decisions', 'summary'],
}

const IMPL = {
  type: 'object',
  properties: {
    verdict: { type: 'string', enum: ['pass', 'fail'] },
    decisions: { type: 'integer' },
    summary: { type: 'string' },
    changed: { type: 'string' },
    disagreements: { type: 'string' },
  },
  required: ['verdict', 'decisions', 'summary', 'changed'],
}

const COMMIT = {
  type: 'object',
  properties: {
    committed: { type: 'boolean' },
    branch: { type: 'string' },
    subject: { type: 'string' },
    summary: { type: 'string' },
  },
  required: ['committed', 'branch', 'subject', 'summary'],
}

const RETURN_RULE = `
Return your findings through the structured output. Put the full finding list, in the format in
.claude/agents/README.md, in "findings". Put the severity counts in the integer fields. Set
"verdict" to "pass" only when blockers and majors are both zero.`

const completed = []

// Guards against the one failure mode an open-ended loop cannot survive: a planner that keeps
// handing back an item that never gets marked done, so the same work repeats forever.
const attempted = new Set()

const stop = (reason, detail) => ({ outcome: 'stopped', reason, detail, completed })

// ---------------------------------------------------------------- audit

phase('Audit')
const audit = await agent(
  `Audit the specification in design/ against the code in this repository.

An unimplemented requirement is the normal state before implementation; report it, but it is not
a reason to stop. Classify as decision-required anything a human has to decide: a contradiction
between documents, or a requirement that does not determine behavior.

In "blockedRequirements", list every requirement identifier named by a decision-required finding,
for example ["DOM-106", "IMP-TR-018"]. That list is what stops work on the items depending on it,
so it must be complete and must contain nothing else.${RETURN_RULE}`,
  { agentType: 'spec-auditor', label: 'audit spec', phase: 'Audit', schema: AUDIT },
)

if (!audit) return stop('spec-auditor did not return')

// A blocker is a wrong result or a violated invariant: nothing may be built over it.
if (audit.blockers > 0) return stop('the specification has a blocking defect', audit.findings)

// Decisions no longer halt the run. They block only the items that depend on them, so the
// specified majority of the system can be built while the undecided remainder waits.
const blocked = new Set(audit.blockedRequirements || [])
log(`Specification audit: ${audit.summary}`)
if (blocked.size) {
  log(`${audit.decisions} open decision(s) block ${blocked.size} requirement(s): ${[...blocked].join(', ')}`)
}

// ---------------------------------------------------------- item loop

for (let i = 0; i < MAX_ITEMS; i++) {
  phase('Plan')
  const plan = await agent(
    `Update PLAN.md against design/ and the current code, then name the next work item to build:
the first item whose status is todo, whose dependencies are all done, and none of whose
requirements appear in the blocked list below.

Blocked requirements (undecided in the specification; any item covering one of these must be set
to blocked and must not be selected): ${blocked.size ? [...blocked].join(', ') : 'none'}

Set nextItemId to "" if no item is ready. Set decisions above zero only if the specification
leaves something open that prevents planning. Put a short account of what changed in "summary".`,
    { agentType: 'planner', label: `plan (item ${i + 1})`, phase: 'Plan', schema: PLAN },
  )

  if (!plan) return stop('planner did not return')
  if (plan.decisions > 0) return stop('planning needs your decision', plan.summary)
  if (!plan.nextItemId) {
    log(plan.remaining > 0 ? 'No item is ready; remaining items are blocked.' : 'Plan exhausted.')
    break
  }

  const itemReqs = String(plan.requirements || '').split(/[^A-Z0-9-]+/).filter(Boolean)
  const clash = itemReqs.filter((r) => blocked.has(r))
  if (clash.length) {
    return stop(
      `item ${plan.nextItemId} depends on requirements that are still undecided`,
      `${clash.join(', ')}\n\n${audit.findings}`,
    )
  }

  if (attempted.has(plan.nextItemId)) {
    return stop(
      `planner returned ${plan.nextItemId} again after it was already worked`,
      'The plan is not advancing. Check that PLAN.md statuses are being updated.',
    )
  }
  attempted.add(plan.nextItemId)

  const item = `${plan.nextItemId} ${plan.nextItemTitle}`
  log(OPEN_ENDED
    ? `Item ${i + 1} (${plan.remaining} remaining): ${item}`
    : `Item ${i + 1}/${MAX_ITEMS}: ${item}`)

  phase('Implement')
  let impl = await agent(
    `Implement work item ${plan.nextItemId} from PLAN.md: ${plan.nextItemTitle}

Requirements: ${plan.requirements}
Acceptance: ${plan.acceptance}

Implement only this item. If it cannot be completed as specified, stop and set decisions above
zero rather than guessing. Ship tests with the change and run the suite before reporting.`,
    { agentType: 'implementer', label: `build ${plan.nextItemId}`, phase: 'Implement', schema: IMPL },
  )

  if (!impl) return stop('implementer did not return', item)
  if (impl.decisions > 0) return stop(`item ${plan.nextItemId} needs your decision`, impl.summary)
  if (impl.verdict !== 'pass') return stop(`item ${plan.nextItemId} could not be completed`, impl.summary)

  // --------------------------------------------------- review / fix loop

  let round = 0
  let conformance
  let tests

  for (;;) {
    // Barrier: both reviews are needed together to decide pass, fail or stop.
    // Fresh agents every round — a reviewer must never inherit the implementer's reasoning.
    const reviews = await parallel([
      () => agent(
        `Verify that work item ${plan.nextItemId} matches the specification.

Requirements in scope: ${plan.requirements}
Acceptance: ${plan.acceptance}

Read the code yourself. Do not rely on any account of what was built.${RETURN_RULE}`,
        { agentType: 'conformance-reviewer', label: `conformance r${round + 1}`, phase: 'Review', schema: VERDICT },
      ),
      () => agent(
        `Judge whether the tests for work item ${plan.nextItemId} verify what they claim.

Requirements in scope: ${plan.requirements}

Coverage thresholds are CI's job; do not recompute them.${RETURN_RULE}`,
        { agentType: 'test-reviewer', label: `tests r${round + 1}`, phase: 'Review', schema: VERDICT },
      ),
    ])

    conformance = reviews[0]
    tests = reviews[1]

    if (!conformance || !tests) return stop('a reviewer did not return', item)

    const decisions = conformance.decisions + tests.decisions
    if (decisions > 0) {
      return stop(`review of ${plan.nextItemId} needs your decision`,
        `${conformance.findings}\n\n${tests.findings}`)
    }

    if (conformance.verdict === 'pass' && tests.verdict === 'pass') break

    round++
    if (round > MAX_ROUNDS) {
      return stop(`${plan.nextItemId} still has findings after ${MAX_ROUNDS} rounds`,
        `${conformance.findings}\n\n${tests.findings}`)
    }

    log(`Round ${round}: ${conformance.blockers + tests.blockers} blockers, ${conformance.majors + tests.majors} majors`)

    impl = await agent(
      `Address these review findings on work item ${plan.nextItemId}. Change nothing else.

Conformance findings:
${conformance.findings}

Test findings:
${tests.findings}

If you believe a finding is wrong, say so with your reasoning in "disagreements" rather than
complying silently. Run the suite before reporting.`,
      { agentType: 'implementer', label: `fix r${round}`, phase: 'Implement', schema: IMPL },
    )

    if (!impl) return stop('implementer did not return during the fix loop', item)
    if (impl.decisions > 0) return stop(`fixing ${plan.nextItemId} needs your decision`, impl.summary)
  }

  // -------------------------------------------------------------- commit

  phase('Commit')
  const commit = await agent(
    `Commit work item ${plan.nextItemId} (${plan.nextItemTitle}).

Before committing, verify green yourself. Do not trust earlier reports:
  cargo test --workspace
  cargo clippy --workspace --all-targets -- -D warnings
  cargo fmt --all --check
  cargo llvm-cov --workspace --lcov --output-path target/lcov.info

Then patch coverage on the lines this item touched, per crate, against HEAD. Thresholds and
rationale are in design/testing.md; the whole codebase is NOT gated, only this item's diff:
  diff-cover target/lcov.info --compare-branch=HEAD --include-untracked \
    --include 'crates/fifolio-core/**' --fail-under=95
  diff-cover target/lcov.info --compare-branch=HEAD --include-untracked \
    --include 'crates/fifolio-server/**' --fail-under=90
  diff-cover target/lcov.info --compare-branch=HEAD --include-untracked \
    --include 'crates/fifolio-cli/**' \
    --exclude 'crates/fifolio-cli/src/tui/render/**' --fail-under=75

A crate this item did not touch has no changed lines and passes trivially; that is expected, not a
reason to widen the check.

If anything fails, commit nothing and set committed to false with the failing output in "summary".

Commit on the current branch. Do not create a branch, do not switch branches, and do not
push. Report the branch you committed on.

Do not commit target/lcov.info. Commit only files belonging to this item, plus PLAN.md. The subject line names the item id and
what it does; the body lists the requirement identifiers covered.`,
    { label: `commit ${plan.nextItemId}`, phase: 'Commit', schema: COMMIT },
  )

  if (!commit) return stop('commit agent did not return', item)
  if (!commit.committed) return stop(`${plan.nextItemId} did not pass the commit gate`, commit.summary)

  log(`Committed ${plan.nextItemId} on ${commit.branch}: ${commit.subject}`)
  completed.push({
    item,
    branch: commit.branch,
    subject: commit.subject,
    rounds: round,
    minors: conformance.minors + tests.minors,
    disagreements: impl.disagreements || '',
  })
}

// Reaching the cap is not completion. Say so, rather than letting it read as "plan exhausted".
const cappedOut = completed.length >= MAX_ITEMS
if (cappedOut) {
  log(`Stopped at ${MAX_ITEMS} items. Work may remain; run again to continue.`)
}

return {
  outcome: cappedOut ? 'capped' : 'finished',
  completed,
  specAudit: { minors: audit.minors, summary: audit.summary },
}
