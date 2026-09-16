---
description: Run the development cycle — audit, plan, implement, review, commit — unattended
argument-hint: "[max work items; default: until the plan is exhausted]"
---

Run the `work-it` workflow.

Call the Workflow tool with `{ name: "work-it", args: "$ARGUMENTS" }`. Passing `$ARGUMENTS` here is
the explicit opt-in to multi-agent orchestration; do not ask for it again.

If `$ARGUMENTS` is empty the workflow runs until the plan is exhausted, committing each item
before starting the next. A number caps how many items it will take on. Either way it stops early
on anything needing a decision, and a runaway backstop of 100 items applies.

The workflow runs in the background and its result arrives as a task notification. Left
open-ended it may run for a long time; the user can watch it with `/workflows` and stop it. Do not
re-invoke it while it is running, and do not do any of its work yourself in the meantime.

## What it does

`.claude/workflows/work-it.js` holds the script; `.claude/agents/README.md` holds the rules the
agents share. In outline: `spec-auditor` audits `design/` against the code, `planner` picks the next
ready item from `PLAN.md`, `implementer` builds it, `conformance-reviewer` and `test-reviewer` judge
it independently and in parallel, findings go back to the implementer for at most three rounds, and
a final agent verifies green itself before committing.

## When it returns

Report to the user, in plain terms:

* Which items were committed, on which branch, and how many fix rounds each took
* Any `minor` findings that were left unaddressed, since nothing forces them
* Any disagreement an implementer recorded with a reviewer's finding — a reviewer can be wrong, and
  that is worth the user's eye
* The specification audit summary

If the outcome is `stopped`, lead with why. A stop for `needs your decision` means the
specification does not determine an answer: present the options and what each implies, so the
user can decide without reading the code. Do not resolve it yourself.

If it stopped after three review rounds, show the surviving findings. Two agents failing to
converge usually means the specification is ambiguous rather than the code being wrong.

An outcome of `capped` means the backstop was reached, not that the work is done. Say so, and that
running again continues from where it left off.

A stop saying the planner returned the same item twice means `PLAN.md` statuses are not being
updated. Report it as a plan defect, not as a code problem.

## What you do not do

Do not fix findings yourself, do not commit, and do not adjust the plan. If the user wants to
continue after a stop, they will say so, and the specification is the thing to change first.
