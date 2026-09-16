# Development cycle

Five agents. The user specifies; the rest runs unattended.

    spec-auditor ──► planner ──► implementer ──► conformance-reviewer
                                      ▲                  │
                                      └──────────────────┤
                                                         ▼
                                              test-reviewer ──► commit

## Rules that make this work

**Reviewers never inherit the implementer's context.** A reviewer spawned as a fork of the implementer will rationalize its choices instead of catching them. Always spawn reviewers fresh, never with `subagent_type: fork`.

**Reviewers never write code.** They have no `Write` or `Edit` tool. Findings go back to the implementer, which keeps the judgement and the fix in different heads.

**The fix loop is bounded.** The implementer addresses findings and the reviewer re-runs, at most **three** rounds. If findings persist after the third, the cycle stops and reports what remains. Two agents disagreeing forever is a failure mode, not diligence.

**Anything needing a human decision stops the cycle immediately.** No bounded loop, no attempt to resolve it. A `decision-required` finding means the specification does not determine the answer, and guessing at it is how a wrong cost basis ends up in a tax return.

**Commits happen only on a clean pass from both reviewers.** Plus green CI: `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check`, and the per-crate coverage thresholds in `design/testing.md`.

## Requirement identifiers

Every testable statement in `design/` carries a stable id, written in brackets at the end of the statement.

| Prefix | Document |
| --- | --- |
| `DOM` | `design/domain.md` |
| `ARC` | `design/architecture.md` |
| `SRV` | `design/server.md` |
| `CLI` | `design/cli.md` |
| `IMP` | `design/importers.md` |
| `TST` | `design/testing.md` |

Ids are never reused or renumbered. A requirement that is removed leaves its number retired, so that a finding or a test naming it stays interpretable.

Tests name the requirements they cover, so that specification coverage is checkable rather than impressionistic.

## Finding format

Every reviewer emits findings in this shape, and nothing else:

    ### <severity> <one-line claim>
    - Requirement: DOM-014 (design/domain.md:87)
    - Location: crates/fifolio-core/src/fifo.rs:142
    - Category: missing | incomplete | incorrect | extra | untested | redundant
    - Evidence: what the code does, against what the requirement says
    - Action: the smallest change that resolves it

Severities: `blocker` (wrong results or violated invariant), `major` (requirement unmet), `minor` (requirement met, quality issue), `decision-required` (specification does not determine the answer).

The last line of every reviewer's output is a machine-readable verdict:

    VERDICT: pass
    VERDICT: fail blockers=1 majors=3 minors=0 decisions=0

A `decisions` count above zero always means stop, whatever else the verdict says.
