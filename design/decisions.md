# Decisions

Resolved ambiguities, newest last. Each entry says what was open, what was chosen, and why. The
requirement documents say what the system does; this says why it does that rather than something
else, so a later reader can tell a deliberate choice from an accident.

Entries are never edited once written. A reversal is a new entry citing the one it supersedes.

---

**DEC-001 — Raw gain/loss only.** German tax treatment (Teilfreistellung, loss pots, Vorabpauschale,
Pauschbetrag) is out of scope. The reports produce figures the user applies tax law to. Tax rules
change with legislation; FIFO attribution does not.

**DEC-002 — Store native and EUR, with the rate.** Rather than converting at import and discarding
the native figures, or keeping native only. Persisting the rate, its source and its date makes the
EUR figures reproducible and a re-rating mechanical instead of lossy.

**DEC-003 — Rate source: broker, else ECB, else previous business day.** A broker-stated EUR amount
is what was actually paid and is the most defensible figure. Absent one, the ECB daily reference
rate for the trade date is the customary source; a weekend or holiday falls back to the most recent
published rate, with that rate's own date stored so the substitution is visible.

**DEC-004 — Sell fees belong to the sale.** Gain is proceeds minus sell fees minus cost minus buy
fees. Splitting a sale's fee across its own allocations for the buy report is a presentation of the
same total, not a change of rule; within one sale, splitting by quantity and by proceeds share give
identical numbers.

**DEC-005 — Allocations store quantity only.** Money is derived from the parent transactions on
read, so approved figures cannot drift from their sources. The risk that a later correction silently
changes an approved attribution is closed by DOM-069 instead: a transaction in an attribution is
immutable.

**DEC-006 — Rounding drift lands on the last share.** Each share rounds independently and the
remainder is absorbed by the last, so shares always sum exactly to the parent. Buy-side, "last" is
the allocation that exhausts the lot, which is what makes a buy's fees fully distributed once its
last unit sells and not before.

**DEC-007 — Source records are a separate entity.** A single real event is often several rows: the
TransAlta merger paid over three bookings, the DeVolksbank tender was a payment plus a reversal.
Transactions therefore derive from one or more source records rather than mapping to rows.

**DEC-008 — Two corporate action kinds.** `quantity adjustment` and `lot transfer` only. An event
that pays cash for units is a sell and one that issues units is a buy, each citing the corporate
action rows it came from. Several broker-named events behave identically for FIFO, so naming them
separately would be surface without behaviour.

**DEC-009 — Lot transfers carry original acquisition dates.** One new lot per old lot, keeping its
date and cost. This matches the treatment German law generally applies to a qualifying
reorganization and keeps FIFO order meaningful across the exchange.

**DEC-010 — Completion, not creation.** The user fills gaps in records that were imported; nothing
is created from nothing. Every supplied value becomes a `manual` source record, so hand-entered
information is as traceable as imported information.

**DEC-011 — `Deponering` price is a historical acquisition price.** Checked against market closes on
the 2021-11-29 transfer date across nine positions: deviations ran from −87% to +384% in both
directions, which rules out a transfer-date valuation. Split restatement preserves
`quantity × price`, so the product is the true cost. The acquisition date defaults to the transfer
date and stays editable, because that one figure is not real and Altbestand status depends on it.

**DEC-012 — Saxo dividends: group by `Corporate action-Id`, flag on `Positie-ID`.** The `Acties`
label is worthless as a signal — Saxo used `Dividend`, `Keuzedividend` and `Herbeleggingsdividend`
for both cash and stock. `Positie-ID` isolated exactly the six Philips rows out of 74. Stock is
pre-selected because it cannot be accepted blindly, whereas cash is one keystroke that would discard
an acquisition. A heuristic is acceptable here only because its failure mode is safe: an understated
position surfaces as a blocked attribution with a named shortfall.

**DEC-013 — `Herbeleggingsdividend` is cash.** All 19 rows reconciled to the cent against declared
dividends per share on unchanged 3-share ING and KPN positions, with 15% Dutch withholding. The
label is misleading; no shares were ever issued.

**DEC-014 — Bonds are quoted percent of par.** The Dutch 7.5% bond paid coupons of exactly 225.00 on
a nominal of 3000 and redeemed for exactly 3000.00, so `3000 @ 139.46` is a cost of 4183.80, not
418,380. Quotation is a property of the security, defaulted from the broker's instrument type and
editable, so a mis-quoted instrument is corrected without reclassifying what it is.

**DEC-015 — Identity prefers the broker's reference.** Trade Republic has a UUID; Saxo has a tuple of
four id columns. Both survive re-exports that change formatting, which a hash of the row does not,
and two genuinely distinct trades can share date, security, quantity, price and fees.

**DEC-016 — Per-crate coverage thresholds.** 95 / 90 / 75, with the TUI render layer excluded, rather
than one workspace number. A single figure lets a well-tested core mask an untested client, or
forces brittle assertions on drawn characters. Coverage is a floor against untested code, not
evidence of correctness; that comes from the property tests.

**DEC-017 — Fixtures are anonymized and committed.** The real exports are personal financial records
and stay gitignored, so importer tests would otherwise not run in CI. Anonymization perturbs amounts,
which is why fixtures test parsing, classification and idempotency only, and arithmetic is tested
separately with synthetic inputs.

**DEC-018 — `fifolio-cli` does not depend on `fifolio-core`.** The CLI reaches data only over HTTP
and holds no domain logic; depending on core would link SQLite into the client binary. The cost is
that response types are defined twice.

**DEC-019 — An unknown Trade Republic corporate action rejects the file.** `TAX_EXCHANGE` is the only
type observed and the only one mapped. Nothing is inferred from quantity signs: an event whose shape
resembles a lot transfer is not thereby one, and a miscategorized corporate action corrupts a cost
basis permanently. The accepted cost is that one unrecognized row blocks a whole file. The remedy —
specify the type, then import again — is the same "change the specification first" loop the
development workflow already enforces.

**DEC-020 — Manual information exports as versioned JSON, with a matching import.** Manual records
have heterogeneous shapes that flatten badly into a table, and the file is read by a program.
Export without import would leave a record that has to be re-keyed by hand, which is most of the
work it was meant to save. Replay is idempotent, and an entry whose source records are absent is
reported and skipped rather than guessed at.

**DEC-021 — `STOCKPERK` is cash; the paired buy is the acquisition.** Trade Republic books a
promotional free share as two same-day rows: a `CASH` / `STOCKPERK` credit and a `TRADING` / `BUY`
of the same ISIN for the same amount, carrying quantity and price. Net cash is zero and the cost
basis is the share's value. The credit is therefore not stored, and the pairing is not verified.
The pairing is observed once, so an unpaired stockperk would lose an acquisition — accepted because
that surfaces as a blocked attribution with a named shortfall the first time those units are sold,
never as a wrong figure in a report. `TRANSFER_INBOUND` is cash arriving from another account and is
indistinguishable in effect from `CUSTOMER_INBOUND`.

**DEC-022 — A populated `symbol` is what makes a row dangerous, not its category.** `STOCKPERK`
arrived as a `CASH` row naming a security, which the category-keyed rule would have discarded.
An unrecognized type naming a security therefore rejects the file, extending DEC-019 past
`CORPORATE_ACTION`. An unrecognized type naming no security is almost certainly cash: it is not
stored, but the import summary names it and counts it, so a new broker type becomes visible on its
first appearance rather than years later.

**DEC-023 — Coverage thresholds gate changed lines, not the codebase.** Absolute per-crate gating
blocks a greenfield build-out by construction: `fifolio-server`'s only content is a six-line stub
that no test executes, so it sits at 0% against a 90% threshold and nothing could ever land. The
alternatives were worse — a ratchet needs a baseline file and machinery, and lowering thresholds to
fit the code means they are not thresholds. Patch coverage judges a change on the code it actually
wrote, keeps the figures fixed at 95 / 90 / 75, and keeps CI green from the first commit.
Implemented as `cargo llvm-cov --lcov` into `diff-cover`, scoped per crate with `--include` and
counting new files with `--include-untracked`; verified end to end against this repository before
being written down. The absolute figures remain the target for the finished project, reported but
not gated until build-out completes.
