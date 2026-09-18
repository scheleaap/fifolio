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

**DEC-023 — Coverage thresholds gate changed lines, not the codebase.** Supersedes the enforcement half of DEC-016; its per-crate figures and reasoning stand. Absolute per-crate gating
blocks a greenfield build-out by construction: `fifolio-server`'s only content is a six-line stub
that no test executes, so it sits at 0% against a 90% threshold and nothing could ever land. The
alternatives were worse — a ratchet needs a baseline file and machinery, and lowering thresholds to
fit the code means they are not thresholds. Patch coverage judges a change on the code it actually
wrote, keeps the figures fixed at 95 / 90 / 75, and keeps CI green from the first commit.
Implemented as `cargo llvm-cov --lcov` into `diff-cover`, scoped per crate with `--include` and
counting new files with `--include-untracked`; verified end to end against this repository before
being written down. The absolute figures remain the target for the finished project, reported but
not gated until build-out completes.

**DEC-024 — No lot entity; the transaction type becomes a sum type.** Supersedes DEC-008. The first proposal was an
explicit lot, opened by buys and transfers and rescaled by splits. The user observed that a lot and
a buy transaction model the same thing, and proposed transaction variants instead. That is right and
it removes an entity rather than adding one. `Transaction` becomes a sum type: `buy`, `transfer_in`,
`sell`, `expiration`, `transfer_out`, `split`, each carrying only its own fields, so a split can
never be read as a trade. The `corporate action` entity is retired. Transactions stay immutable and
statement-matching, so DOM-039 and DOM-069 survive — which rewriting buys in place would have cost.

**DEC-025 — Attribution generalizes to closing against opening transactions.** Opening variants are
`buy` and `transfer_in`; closing variants are `sell`, `expiration` and `transfer_out`. A
`transfer_out` runs through the same approve-the-proposal flow and emits one `transfer_in` per
consumed parcel, each inheriting that parcel's acquisition date and its share of the basis. One
transfer_in per parcel rather than per event is what preserves separate acquisition dates through an
exchange, which is the whole reason a lot model looked necessary. It realizes no gain.

**DEC-026 — `transfer_in` covers both a broker transfer and a corporate-action transfer.** Supersedes DEC-011's framing of a Deponering as a buy; its price and date findings stand. One type,
with the origin on a `source` field. The distinction that mattered — a broker transfer's acquisition
date is a defaulted guess and editable, a corporate-action transfer's is inherited fact and is not —
is carried by a date-provenance field rather than by a second type. `expiration` stays separate from
`sell` because its quantity rule differs: it closes the whole remaining position rather than a
stated figure. Stock-dividend shares are a `buy` with an origin field, since nothing is transferred.

**DEC-027 — The stored FX rate is foreign units per EUR.** The ECB convention, matching Trade
Republic. Saxo quotes the inverse, so its importer inverts at full precision from the figures the
file states, never from the 6-decimal stored rate — for a currency like JPY, inverting a rounded
rate loses four significant figures.

**DEC-028 — The EUR gross total is stored alongside the unit price.** Dividing a booked total by a
quantity and multiplying back does not round-trip at fractional quantities, so the specification
previously said both "the booked total is what was paid" and "the total is a product of a rounded
unit price". A full allocation now takes its figures from the booked total, a partial one from the
unit price, and the drift rule absorbs the difference.

**DEC-029 — Splits are interleaved by import sequence.** No special rule puts a corporate action
before or after trades on its date; every variant takes part in one canonical order. The accepted
cost is that a same-day split and trade are ordered by which was imported first, so re-importing in
a different order can change a same-day result. Recorded as a known limitation rather than removed.

**DEC-030 — Pending records block attribution per account, not globally.** Consistent with every
other rule in the model, and FIFO never crosses accounts, so an unresolved Saxo split cannot affect
the same ISIN held at Trade Republic.

**DEC-031 — Instrument types map by explicit table; `Cash` creates no security.** Saxo's
`MutualFund` is `fund`; its `Cash` rows are non-position and create nothing. Trade Republic carries
ETFs as `FUND`, so its ETFs are recorded as funds — wrong but inconsequential in v1, where nothing
turns on the distinction, and visible because auto-created securities are flagged. An unmapped value
rejects the import. The bond case is the exception: percent-of-par is confirmed only for Saxo, so a
bond arriving from Trade Republic is created pending review rather than given a quotation.

**DEC-032 — `TAX_EXCHANGE` is treated as tax-neutral, on evidence, pending a document.** Narrows DEC-019. A fund
merger redomiciling an Amundi ETF from Luxembourg to Ireland, one for one. The rows carry no price,
no amount and no tax, and a German broker must withhold on a realized gain, so nothing appears to
have been realized. That is inference from absence and the label says "TAX"; the Trade Republic tax
report for the year settles it. The quantity sign decides direction, which narrows DEC-019 to "an
*unrecognized* type is never classified by shape" — for a recognized type the sign is stated data.

**DEC-033 — Saxo's fallback identity gains a within-group ordinal.** The corporate action id plus
`Acties` plus `Boekingsbedrag` can collide within one event, and the TransAlta merger pays over
three rows under one id. A collision would be read as a re-import and silently drop a row, losing
money from the event — the exact failure DEC-007 exists to prevent. The row's ordinal in file order
within its group closes it; a residual collision rejects the file rather than deduplicating.

**DEC-034 — Report tokens `income-tax` and `acquisitions`; `--format` defaults to JSON.** Output is
usually piped, so the machine-readable form is the default. `export-manual-information` takes an
explicit file argument, matching its import counterpart rather than writing to a surprise location.
The buy report is renamed the acquisition report, because a `transfer_in` is an acquisition and not
a buy.

**DEC-035 — Two arithmetic errors in the specification, corrected.** Both were mine and both would
have produced plausible, silently wrong tax figures. The Saxo EUR gross derivation was written as
one rule for buys and disposals; `Aantal` is a cash movement, so a disposal's gross is
`|Aantal| + |Totale kosten|`, and the single rule understated every disposal's proceeds by twice the
fee. Separately, the importer derived an EUR unit price by dividing gross by quantity with no
quotation factor while the domain then multiplied by 0.01 again, understating a bond's cost basis a
hundredfold. The factor is now applied exactly once, and the importer divides by it so the stored
price remains the price the statement shows.
