# Testing

Why this document exists: correctness here is not observable by using the application. A wrong cost basis produces a plausible number in a tax return and stays wrong for years. The tests are the only thing that says the arithmetic is right.

# Layers

## Unit

Scope: one module, no I/O, no database, no network. [TST-001]

Everything in `fifolio-core` that decides something is unit tested: the FIFO proposal, allocation derivation and the drift rule, the quotation factor, FX rate selection including the previous-business-day fallback, canonical ordering, identity and idempotency, and each importer's row classification. [TST-002]

## Integration

Scope: a crate against its real dependencies, in process. [TST-003]

`fifolio-core` against a real temporary SQLite database, covering migrations, the invariants that storage enforces, and batch deletion returning source records to pending. [TST-004] `fifolio-server` against its HTTP surface, covering status codes, the problem+json shape, the proposal fingerprint conflict, and refusals such as attributing a security that still has pending records. [TST-005]

## End to end

Scope: the real binaries, a real database, a real port. [TST-006]

Spawn `fifolio-server` on a temporary database and a free port, drive it with the `fifolio-cli` binary, and assert on what a user would see: import a fixture, resolve the completion queue, approve an attribution, run a report. [TST-007] This is the only layer that exercises argument parsing, the HTTP client, and the two processes agreeing.

Keep it to the paths that matter. End-to-end tests are the slowest and the most annoying to diagnose, so breadth belongs in the layers below.

# Coverage

Thresholds, per crate: [TST-008]

| Crate | Threshold |
| --- | --- |
| `fifolio-core` | 95% |
| `fifolio-server` | 90% |
| `fifolio-cli` | 75%, TUI render layer excluded |

Per crate rather than workspace-wide, so a well-covered core cannot mask an untested client, and so the TUI is not held to a number that would only buy assertions on rendered strings.

The render layer is excluded because it draws; everything it draws is decided in the UI-agnostic client layer, which is not excluded. [TST-009] If logic starts appearing inside the render layer, the exclusion is wrong and the code should move, not the threshold.

## What is gated: changed lines

The thresholds are enforced as **patch coverage** — they apply to the lines a change adds or modifies, not to the whole codebase. [TST-025] Measured with `cargo llvm-cov --lcov` feeding `diff-cover`, scoped per crate with `--include`, and counting new files via `--include-untracked`.

    cargo llvm-cov --workspace --lcov --output-path target/lcov.info
    diff-cover target/lcov.info --compare-branch=<base> --include-untracked \
      --include 'crates/fifolio-core/**' --fail-under=95

Gating the whole codebase would block the build-out by construction: a crate whose only content is a stub sits at 0% against a 90% threshold, so nothing could ever land. Judging a change on the code it actually wrote is the honest question anyway, and it keeps the thresholds fixed rather than moving them to fit the code.

The base of the comparison is whatever the change is being judged against: the previous commit when an item is committed, the target branch in a pull request. [TST-026]

## The absolute figures

The same per-crate thresholds apply to the codebase as a whole once build-out is complete. Until then they are reported, not gated. [TST-027] A project that reaches its last work item without meeting them has a gap patch coverage did not catch — untested code that predates the rule.

**Coverage is a floor, not evidence.** It proves lines ran, not that anything was checked. It is there to catch code nobody tested at all. Confidence in the FIFO engine comes from the property tests below, and from `test-reviewer`, which judges whether an assertion verifies anything — a question no percentage can answer.

# Properties

Some rules are better stated as invariants over generated input than as examples. Using `proptest`: [TST-010]

* Allocated quantities against a buy never exceed its quantity
* The allocations of a sale sum exactly to the sale's quantity
* Allocation shares sum exactly to the parent figure once a parcel is fully consumed, for every division and every rounding remainder
* A buy's fees are fully distributed once its last unit is sold, and not before
* A split leaves an opening's total cost unchanged while scaling its effective quantity, and an opening's effective quantity as of a position before any split equals its stated quantity
* Successive splits compose exactly: applying 1-for-3 then 3-for-1 returns the original quantity, with no accumulated residue
* Each record emitted by a transfer carries its own parcel's cost, never a pooled average: two equal parcels acquired at different prices emerge at different unit costs
* A transfer out preserves total cost basis and parcel count across the transfer_in records it emits
* An opening's effective quantity equals its stated quantity under no splits, and is stable under a split applied twice with inverse ratios
* Attributing a sequence of closings in canonical order never leaves an opening over-consumed
* Order computed from a file is identical however many times that file is imported, and independent of what was imported before it
* An undo followed by a re-import restores exactly the transactions that existed before

These catch the errors that matter: an off-by-one-cent drift that only appears at one particular split, or a rounding rule that fails on the seventh lot.

# Fixtures

The real exports in `example_exports/` are personal financial records and are gitignored. Tests use **anonymized fixtures derived from them**, committed to the repository. [TST-011]

Anonymization replaces account ids, client ids, personal names, IBANs and instrument-level identifying detail, and perturbs amounts. [TST-012] It must preserve every structural property the importers depend on, because that is the whole point of using real files: [TST-013]

* Saxo: XLSX container, Dutch headers, the non-breaking spaces and the leading space in header names, Excel serial dates, the per-currency account suffix, the free-text `Acties` strings with their rounded prices, multi-row corporate actions sharing a `Corporate action-Id`, reversal rows, and at least one row of every `Acties` value observed
* Trade Republic: quoted CSV, the full column set, ISO-8601 timestamps with sub-second precision, UUID transaction ids, negative cash-flow amounts, and populated `original_*` columns

Fixtures test **parsing, classification and idempotency**. They do not test arithmetic, because anonymization perturbs the numbers. [TST-014]

# Arithmetic

Money rules are tested generically, with synthetic inputs whose expected results are derivable by hand, rather than by pinning figures from one account's history. [TST-015] Real-world cases informed the rules; they are not the assertions.

Each rule gets its own cases, including the boundaries: [TST-016]

* EUR derivation from a broker-stated total and a separately stated cost
* Percent-of-par against per-unit quotation, including that mixing them up is a factor of 100
* Fee distribution where the division is exact, where it leaves one cent, and where it leaves many
* Fractional quantities at full scale

# Reports

Snapshot tested with `insta`, over a fixed scenario built in code rather than imported. [TST-017] Each output format has its own snapshot, so a change to the human-readable layout cannot silently alter the CSV a spreadsheet depends on. [TST-018]

# External services

**Tests never touch the network.** [TST-019] The ECB rate source is an injected dependency; [TST-020] tests supply a fake with a known rate table, including gaps at weekends so the fallback is exercised. [TST-021] The cache-seeding path is tested against a recorded fragment of the ECB series, not a live fetch.

# The terminal UI

The TUI is a render-and-dispatch layer over a UI-agnostic client. That client holds the view stack, the key-to-action mapping, the completion queue state and the dialog logic, and is unit tested headlessly. [TST-022]

What remains in the render layer is drawing. It is covered by end-to-end tests at the level of "this ran and produced a frame", not by assertions on drawn characters, which break on every layout change and verify nothing about behavior.

Localization gets one test that every key present in one catalog is present in the other. [TST-023] That is a build-time check, not a runtime concern.

# What is enforced by machine, not by prose

Thresholds, lints and formatting belong in CI and `Cargo.toml`, where they are checked rather than remembered: `cargo test`, `cargo llvm-cov` with the per-crate thresholds above, `cargo clippy -- -D warnings`, `cargo fmt --check`. [TST-024]
