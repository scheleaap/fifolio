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

Per crate, enforced in CI, measured with `cargo llvm-cov`: [TST-008]

| Crate | Threshold |
| --- | --- |
| `fifolio-core` | 95% |
| `fifolio-server` | 90% |
| `fifolio-cli` | 75%, TUI render layer excluded |

Per crate rather than workspace-wide, so a well-covered core cannot mask an untested client, and so the TUI is not held to a number that would only buy assertions on rendered strings.

The render layer is excluded because it draws; everything it draws is decided in the UI-agnostic client layer, which is not excluded. [TST-009] If logic starts appearing inside the render layer, the exclusion is wrong and the code should move, not the threshold.

**Coverage is a floor, not evidence.** It proves lines ran, not that anything was checked. It is there to catch code nobody tested at all. Confidence in the FIFO engine comes from the property tests below.

# Properties

Some rules are better stated as invariants over generated input than as examples. Using `proptest`: [TST-010]

* Allocated quantities against a buy never exceed its quantity
* The allocations of a sale sum exactly to the sale's quantity
* Allocation shares sum exactly to the parent figure, for every split and every rounding remainder
* A buy's fees are fully distributed once its last unit is sold, and not before
* A quantity adjustment leaves total cost per lot unchanged
* A lot transfer preserves total cost basis and the count of lots
* Attributing a sequence of sells in canonical order never leaves a buy over-consumed

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
