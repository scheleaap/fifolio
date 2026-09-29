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

**DEC-036 — Effective quantity is measured at the closing's position, not today's.** Supersedes the
formula in DEC-024's model. A disposal states its quantity in the units current when it happened, so
dividing its cost basis by an effective quantity that includes later splits halves the basis of
anything sold before a split — invisible in every scenario without one, which is most of them. The
invariant capping allocations against an opening compares quantities scaled to a common position,
since quantities stated at different positions are in different unit scales.

**DEC-037 — The booked EUR total governs every calculation.** Supersedes DEC-028's two-path rule,
which promised that full allocations use the booked total and partial ones the unit price but never
gave the second formula, so two implementers would produce different cents. Shares are taken
pro-rata from the booked total and never rebuilt from the unit price; the drift rule already
guarantees they sum back to it. The unit price is kept for display and reconciliation and nothing
computes with it.

**DEC-038 — Manual entries are a separate entity from source records.** They shared one entity with
a `kind` field, and an awkward split underneath: an imported record belongs to a batch, a manual one
to the account. That difference is exactly what made an undo dangerous, and it was expressed as
prose rather than as shape. Splitting them makes it structural: an undo removes source records and
cannot touch manual entries. A manual entry references the records it answers by their **broker
identity** rather than an internal key, so it survives their deletion and reconnects automatically
when the same rows return — an undo followed by a re-import restores the account exactly. An entry
with nothing to attach to is listed as waiting, so a vanished completion has an explanation.

**DEC-039 — Order is computed from file content at read time and stored.** Supersedes the undefined
"import sequence". Rows are sorted on the trade date, then on whatever further ordering columns the
format provides in a stated precedence, then on file position normalized to the file's own
direction. Saxo emits newest-first and its `Bk Record Id` / `Booking Id` / `Transactie-ID` are
monotonic counters that ascend with date; its `Corporate action-Id` is not monotonic and is never an
ordering column. Because the third key is always available the order is total, nothing is ever
refused for ambiguity, and the result depends on the file rather than on import history — which
removes the reproducibility limitation DEC-029 accepted. Stored because the file may be gone when
the figure is next needed.

**DEC-040 — A transferred parcel's acquisition date is never corrected.** Supersedes DEC-011's
editable date. The transfer date stands permanently, so there is no edit endpoint and no re-rating
rule. The accepted cost is that pre-2009 grandfathered status is not represented on any transferred
parcel, and several Saxo positions price to the mid-2000s.

**DEC-041 — Quotation has no unknown state; an undeterminable one rejects the import.** Rather than
a third enum variant with a pending rule. Consistent with how unmapped instrument types are already
treated, and it keeps the trade-value calculation total.

**DEC-042 — `Deponering` is the one place the `Acties` label's price is authoritative.** The general
rule forbids using that label for money because its price is rounded to two decimals, but
`Boekingsbedrag` and `Aantal` are zero on transfer rows so no column carries the figure. The
rounding is carried into the cost basis: 0.15 EUR on the sample bond.

**DEC-043 — An exchange splits basis pro-rata with the last parcel absorbing the remainder, and its
fees join the carried basis.** The same drift rule allocation shares follow, so there is one
rounding behaviour. Fees are an incidental cost of acquiring the replacement holding and reduce the
gain when it is eventually disposed of, rather than vanishing or being booked as a loss on an event
treated as tax-neutral everywhere else.

**DEC-044 — Imports are refused above one calendar year; within a year the newest import owns its
rows.** A year at a time keeps a corporate action's rows inside one file and makes ownership
unambiguous. The remaining overlap is the ordinary year-to-date workflow — re-downloading the
current year as it fills — and there the newest import claims the rows it covers, so undoing it
removes that year while the superseded batches own nothing. A partial year is accepted.

**DEC-045 — Canonical order is (trade date, order, batch age).** `order` is scoped to the file it
was computed from, so it cannot be compared across files on its own. Trade date sorts first and one
year is one file, so records sharing a date almost always share a file; batch age settles the rest.
The residue, recorded as a limitation rather than engineered around: a broker re-issuing a year with
a backdated row can compute a position equal to a stored one, because records are never edited.

**DEC-046 — Each record a transfer emits carries its own parcel's cost.** Supersedes DEC-043's
wording. "Basis divided in proportion to the quantity consumed" reads as pooling the total and
splitting by quantity, which gives every emitted parcel the same unit cost — two equal parcels
bought at 100 and 200 both emerging at 150. That destroys the per-parcel basis one record per parcel
exists to preserve, and does it silently: the total stays right and each individual figure is wrong,
surfacing only when the parcels are disposed of in different years. Fees divide in proportion to the
basis each record carries, and emitted quantity is consumed quantity times the ratio, the last
record absorbing both remainders.

**DEC-047 — Split ratios are integer pairs, computed as exact rationals.** A one-for-three reverse
split has no finite decimal expansion, so rounding at each application leaves a residue that grows
across successive splits and can stop a parcel exhausting exactly. Effective quantity is an exact
rational and is rounded only for display.

**DEC-048 — Every closing variant carries `eur_gross` and `eur_fees`; `split` is excluded by name.**
One allocation formula reads them all. A `transfer_out`'s gross is the basis it carries onward and
is derived from its own allocations rather than stored, so the opening side is computed first.

**DEC-049 — A `TAX_EXCHANGE` derives only its `transfer_out`.** The importer took both sides, the
domain emits the inbound side at approval; the domain is right. The positive-quantity row supplies
the target security and ratio and is cited rather than turned into a transaction. Deriving both at
import would collapse a position built from several purchases into one parcel with one acquisition
date, losing what the design exists to preserve.

**DEC-050 — An expiration of a position with nothing remaining stays pending.** A redemption of
nothing is a data problem, not a transaction, so it never enters the model and the division by a
zero quantity cannot arise.

**DEC-051 — A part-cash, part-exchange event puts the cash and all the costs on the sell leg.** The
fee was charged on the payout, so attaching it to the sale deducts it in the year it was incurred
rather than deferring it to a disposal that may be years away. The transfer leg carries only the
basis it inherits.

**DEC-052 — The import checks the file's account id against the target account.** An account records
its broker id at creation; a mismatch, or a file carrying rows from more than one account, refuses
the import. This is the guard against the mis-aimed import that batch deletion exists to undo.

**DEC-053 — Smaller determinations.** The multi-year check uses the **trade date**, which the domain
already treats as authoritative, so a file named for one year can still be refused. A transaction's
`order` is that of the record it consumes, which is unique by construction. The acquisition report
states effective quantity **as of today**, so its two quantity columns share one scale. Saxo's
stored FX rate is the reciprocal of `Omrekeningskoers` at full precision rather than the implied
quotient of the booked amounts; the two differ in the fifth decimal, and since only the booked EUR
total is ever computed with, the stored rate serves the audit trail alone. Deleting a batch is
refused when a record it owns is cited by a transaction it did not derive. Rounding is **half away
from zero**, since "half-up" is ambiguous for the negative amounts a realized loss produces. The
ECB fallback is bounded: nothing before the series begins in 1999, and a substitution more than
seven days stale is an error. Report formats are named `human`, `csv` and `json`. Only attributed
disposals contribute to the income tax overview; an unattributed one is reported as outstanding
rather than counted as zero.

**DEC-054 — Saxo's native fee is rounded to the money scale before the native gross is derived.**
Against the general rule that intermediates keep full precision and round only at the storage
boundary. Carried at full precision the sample buy yields a native unit price of `5.750036` where
the broker statement shows `5.75`, which the requirement that prices reconcile against a broker
document forbids. The native figures exist to be reconciled; that is the whole of their job, and
the EUR figures the tax reports compute from are exact either way. Chosen deliberately so it reads
as policy rather than as a rounding bug.

**DEC-055 — A transfer's ratio is an exact integer pair, not a decimal.** The same reasoning that
made a split's ratio a pair: a one-for-three ratio has no finite decimal expansion, so rounding it
leaves a residue that grows across applications, and the requirement that emitted quantities sum
exactly to the transferred quantity times the ratio is only well defined when the ratio is exact.
The importer reduces the two stated quantities rather than dividing them.

**DEC-056 — `Terugboeking` is a suffix, and a reversal subtracts both cash and costs.** The exports
carry no bare `Terugboeking`: the observed values are `Terugkoopaanbod - Terugboeking` and
`Dividend - Terugboeking`, so a suffixed value classifies as its prefix, reversing. Its cash
subtracts, which the DeVolksbank tender confirms (`3946.14` paid, `-1998.07` reversed). Its costs
subtract too, which the data does **not** confirm: both reversal rows carry zero costs, so the rule
was chosen for arithmetic consistency with the cash rather than from evidence, and is flagged in
`importers.md` as the thing to check if a costed reversal ever arrives. Summation is on the EUR
figures, since a group's rows may carry different conversion rates.

**DEC-057 — A spreadsheet row's "verbatim" form is a defined canonical rendering.** A spreadsheet
row is typed cells and has no verbatim text, so any stored string is the importer's construction.
Rather than weaken the audit trail by making the field optional, or change the storage schema to
hold typed cells, the rendering is specified — cells as the file holds them, keyed by column name,
in sheet column order — so that two implementations and two imports agree.

**DEC-058 — Outstanding disposals are a count column, not a separate block.** The report must never
understate a year silently, but a second record shape would give the CSV and JSON outputs two
schemas and complicate every consumer. A per-row count preserves one shape, and a sentence in the
human format carries the warning where there is room for it.

**DEC-059 — Supersedes DEC-054: no rounding exception; the derived price is authoritative.**
DEC-054 rounded Saxo's converted native fee to the money scale so the stored native price would
match the statement, citing the requirement that prices reconcile against a broker document. That
reasoning was wrong, because a more specific rule already settles it in the other direction: the
`Acties` label's 2-decimal price must never be used for money, and the sample sell is deliberately
stored as `30.654` against a printed `30.65`. A stored price is what the booked amounts imply, and
it may differ from a rounded display in the trailing decimals. Adding an exception to preserve a
promise the specification had already broken elsewhere would have made the rule incoherent rather
than the arithmetic correct. `DOM-039` is restated to promise reconciliation against the booked
amounts, which are exact, rather than against the printed unit price.

**DEC-060 — An acquisition date can never be corrected by hand.** `cli.md` listed one among the
manual entry shapes, against `SRV-054` and `IMP-SAXO-016`, which fix the date at import and never
correct it. The user settled it: no correction. `DOM-097`'s three shapes are therefore the closed
set, and the consequence is accepted — where a broker transfer carries a date that loses a parcel's
grandfathered status, that status stays lost rather than being restorable by typing. A date the
system did not derive is a date no audit trail supports.

**DEC-061 — A stock dividend's taxable value and its EUR gross are one stored figure.** The model
had held them as two independent fields, and a fixture made them differ, 78.30 against 81.00. Since
every calculation reads the gross while the basis is the taxable value, the two diverging produces a
cost basis that is wrong and silent about it. The user settled it: the value taxed at issue is the
cost. Stored once rather than twice-and-asserted-equal, because a single field cannot drift. If a
broker is ever seen to report a taxable value differing from the shares' value at issue, this is the
entry to revisit.

**DEC-062 — Corrects DEC-060's over-reach: DOM-097 carries five shapes, not three.** DEC-060
recorded the user's decision that an acquisition date is never corrected by hand, and in writing it
down also declared DOM-097's list closed at three shapes. That second part was not decided by
anyone and was wrong: `cli.md`'s completion queue asks for a split's ratio alone and for a disposed
quantity with an optional target security, and `importers.md` already marks exactly those rows
pending. DOM-097 now lists what the queue asks for. DEC-060 stands on its own subject; only its
claim of closure at three is withdrawn.

**DEC-063 — A dividend taken in shares is stored; only cash dividends are not.** DOM-002 and
DOM-046 said without qualification that dividend rows are recognized and not stored, while
IMP-SAXO-018 marks a share-issuing dividend pending and DOM-082 gives the resulting buy a
stock-dividend origin. Both were normative and neither cited the other, so one implementer would
have discarded the 2022 Philips acquisition and another kept it. Cash dividends are income and stay
unstored; a dividend that issues a parcel is a position event.

**DEC-064 — Saxo's three counters are three ordering columns, not one.** IMP-SAXO-026 folded
`Bk Record Id`, `Booking Id` and `Transactie-ID` into a single key on the stated ground that all
three ascend with date. They do, but as disjoint counters whose magnitudes differ by an order of
magnitude, so a date carrying a mixture was ordered by which column a row happened to populate.
Ten dates in the fixtures carry such a mixture and same-day order decides which parcel a same-day
sell consumes. Each counter is now compared only against itself; rows populating different counters
fall through to file position. Rejected: keeping the folded key and recording the inversion as a
limitation, since it is a cost-basis input and the cost of doing it properly is one comparison.

**DEC-065 — The stored rendering of a spreadsheet row is JSON.** DEC-057 specified the rendering's
content and called it reproducible, but named no syntax: no separator, no escaping, no empty-versus-
absent rule, no statement of which spelling of the header is the key. A JSON object of column name
to cell string, keys in sheet order, settles all four by reference rather than by inventing rules,
at the cost of a field that is not line-shaped. It is on-disk format: a re-import must reproduce it
byte for byte.

**DEC-066 — A gain is arithmetic on the rounded shares, and a total is the sum of rounded rows.**
DOM-061 rounds each allocated share and lands the drift on the last, but a gain is not a share, so
computing it from the four rounded figures or exactly-then-rounded differ by up to two cents per
allocation and systematically across a year. Rounded shares win: every report row reconciles to its
own columns and every total to the rows above it, which is the first thing a reader checks and the
first thing that undermines a figure when it fails. The exact-then-rounded reading is closer per
allocation and was rejected for that reason.

**DEC-067 — Storage refuses an unrounded value rather than rounding it.** ARC-010 read "rounding
happens at storage and presentation boundaries", which the repositories implemented as a refusal:
the caller rounds, the store checks. The two readings differ on what a store does with a figure
carrying more decimals than its scale allows. Refusal wins, because a store that rounds on the way
in cannot distinguish a figure that was meant to be rounded from one that arrived wrong, and the
second is a bug worth surfacing at the point it occurs rather than absorbing silently. The wording
now says before the boundary, and names the check.

**DEC-068 — A restated day keeps the cached rate; the 90-day window never rewrites one.** ARC-018
says the cache is topped up from the rolling window afterwards, but not what happens when that
window restates a day the historical series [ARC-017] already deposited, at a different figure.
The two readings are "last document wins" and "first document wins". First wins: the rate is
stored on a conversion that was valued at it, and a transaction's EUR figures must stay
reconcilable against the rate and date recorded beside them [DEC-002], which an overwrite breaks
silently and retroactively. Insert-and-keep is the mechanical form of that. The cost is accepted
and stated here rather than hidden: an ECB *correction* to an already-cached day will never reach
the cache, and nothing reports the divergence. Re-rating a stored transaction is a deliberate act
under DEC-002, so a correction that matters is applied that way rather than by a silent top-up. Ratified by the specification owner; the rule is stated in architecture.md as ARC-028.

**DEC-069 — Corrects DEC-064: the Saxo same-day choice is made per date, not per pair of rows.**
DEC-064 replaced the folded counter key with a pairwise rule — compare two rows by a counter when
they share one, fall back to file position when they do not. That is not transitive and so is not a
sort order at all: given `Bk 5`, `Booking 100` and `Bk 7`, the first and third compare by counter
while each compares to the second by position, and no arrangement satisfies all three. The
correction keeps DEC-064's reason and fixes its mechanism. Within one date: if every row populates
the same counter column, that counter orders the date; otherwise the date is ordered by reversed
file position and no counter is consulted. The decision is per date, so the comparison is total.

**DEC-070 — The Saxo export has three sheets, and the two that were never opened carry the
quantities.** Every Saxo rule until now was written against `Transacties` alone, because that is the
sheet the file opens on. `_Transacties` holds the position side of each event and `Bookings` the
components of each cash movement. The consequences are not cosmetic:

* The `Acties` label is no longer a source of money **anywhere**. `Verhandelde waarde` is the exact
  traded value, and on the thirteen transferred-in parcels it differs from the label price times the
  quantity by up to 1.50 on one parcel and 3.30 in total — a cost basis error, not a display
  rounding. IMP-SAXO-028's "half-cent tolerance of 0.15" was an artifact of reading one sheet.
* The corporate actions the classification table marked `pending` are derivable: split ratios as
  exact integer pairs (Tesla 45:15, OBAM 20:4), both legs of a merger and of an exchange, the tender
  quantity, the expiration quantity, and a stock election's share count with its taxable value.
* The 19 `Herbeleggingsdividend` events issue no shares. Each decomposes into a fractional cash
  payment and its withholding tax, and the eligible position never grows across five years. The
  largest anticipated block of manual data entry does not exist.

The fixtures reproduce one sheet and so do not reproduce the file. They are rebuilt, and the
anonymizer extended to all three sheets, before any Saxo importer work continues. [DEC-070]

**DEC-071 — Corrects DEC-070: a corporate action has any number of legs, and a reversal is one of
them.** DEC-070 described a corporate action as two `_Transacties` rows distinguished by
`Trade Event Type`. Two of the ten groups in the sample are not that shape, and both matter.

The 2023 Philips dividend carries **two** `Gekocht` legs of one share each at 34.74. A rule reading
"the `Gekocht` leg" takes one share and silently drops the other, so a share is acquired, held and
eventually sold without ever entering the ledger. Sides are therefore summed, never indexed.

The DeVolksbank tender carries three legs: a disposal of 2000 at 999.03, a `Gekocht` of 2000 at
999.03, and a disposal of 2000 at 99.90. The middle leg is the `Terugboeking` — an exact negative of
the first — and reading it as an opening would invent a 19,980.65 acquisition. Exactly opposing
legs are cancelled before the event is read, which leaves the single disposal that `Bookings` and
the cash ledger both show.

Both errors are of the same kind: they produce a plausible number from a rule that fit the examples
looked at. The rule now states the observed leg shapes in full so the next reader can see what it
must handle. [DEC-071]

**DEC-072 — Corrects DEC-071: a reversal is identified by its label, not by its shape.** DEC-071
said an exactly opposing pair cancels, and described the pair as legs whose "quantity, price and
traded value are the exact negatives" of each other. Both halves were wrong.

The price on a cancelling pair is **equal**, not negated — the DeVolksbank legs both read 999.03,
as that entry's own worked example printed. Taken literally the rule matches nothing in five years
of exports, and the tender it was written for would not have cancelled.

Read charitably as negated quantity and value with equal price, it then matches too much: the
sample's only `Omwisseling` is `-3 @ 168.63 / 505.89` against `3 @ 168.63 / -505.89`, the same shape
as the reversal. Cancelling it destroys a genuine exchange and leaves its group empty.

Cancellation therefore keys on the `- Terugboeking` suffix, which Saxo states explicitly, and a
group without one never cancels. This document already refuses to classify a row by its shape when
a stated value is available; the reversal rule was violating its own principle.

Two further points settled while correcting it: `Deponering` is a third side rather than a missing
one, since a two-sided reading drops all 13 transfer legs; and a side's summed quantity keeps the
file's sign, the side naming its own direction. [DEC-072, IMP-SAXO-046, IMP-SAXO-047, IMP-SAXO-048]

**DEC-073 — A stored Trade Republic row in a foreign currency rejects the import.** Trade Republic's
`fx_rate` was foreign units per EUR up to 2024-07-02 and its reciprocal from 2024-10-01 on; the 2025
rows settle it from the file alone (`0.04 USD` at `0.853242` states `0.03 EUR`, which only
multiplication reproduces). Keying on the date is fragile if the change was gradual or
account-specific, keying on whether the rate is above 1 fails near parity, and taking the ECB rate
contradicts DEC-003. Every foreign-currency row in four years of exports is a dividend, which is
not stored, so refusing a stored one blocks nothing and extends IMP-TR-017 from trades to every
stored row. The convention is decided when such a row first appears. [DEC-073, IMP-TR-017]

**DEC-074 — A row that fails to parse rejects the whole import.** Skipping it would leave a batch
owning part of a file, and a skipped buy understates holdings in a way that surfaces only at
attribution (SRV-018), far from its cause. Storing it as pending cannot scope the attribution block
when the row yields no ISIN. Rejection matches DEC-019, DEC-041 and IMP-SAXO-038, and is cheap to
recover from because import is idempotent and one file covers one year. A parse failure on a real
export is a parser defect to fix, not a row to lose. The refusal names every failed row so that one
round of fixes suffices. [DEC-074, SRV-017, SRV-058]

**DEC-075 — The account check runs only where the file states an account id.** The Trade Republic
export carries no account identifier, so there is nothing to compare. IMP-003 already refuses a file
that *names* a different account, and SRV-012 makes the caller supply the account because source
files rarely identify it reliably; a file naming none therefore takes the caller's. Refusing every
Trade Republic import instead would make the format unusable. [DEC-075, IMP-003, SRV-056]

**DEC-076 — Withholding tax is not modeled.** Saxo states a `Tax Percentage` and a
`Corporate actions - Voorheffing` component on dividends. Crediting foreign withholding is applying
tax law, which DEC-001 and DOM-001 leave to the user, and cash dividends are not stored at all
(DEC-063), so there is no stored dividend for a withheld amount to belong to. Nothing is lost: the
source record keeps the row verbatim (DOM-007). [DEC-076]

**DEC-077 — Corrects DEC-031: a Trade Republic bond rejects the import.** DEC-031 said such a bond is
created pending review; DEC-041, later, gave quotation no unknown state and made an undeterminable
one reject the import, and IMP-TR-020 and IMP-TR-021 both reject. DEC-041 governs. The trigger is
IMP-TR-020's catch-all, since no `asset_class` value maps to `bond`. [DEC-077, IMP-TR-020,
IMP-TR-021]

**DEC-078 — An import fetches a rate it needs and does not have.** ARC-019 already spoke of a rate
"neither cached nor fetchable", and SRV-047 seeds the cache "on first use", which for a new user is
the first import. So an import may reach the network, but only to fill the cache, never to rewrite
it (ARC-028), and once cached it works offline (ARC-016). An import can therefore fail for want of
a connection, and only when the rate is uncached. [DEC-078, ARC-019]

**DEC-079 — An emitted `transfer_in` cites the `transfer_out`'s records and keeps its parcel's order.**
Records emitted on approval derive from no row of their own, yet DOM-013 requires a source record and
DOM-047 forbids creating from nothing. They cite the source records of the `transfer_out` that
produced them, and take the `order` of the parcel each carries alongside its inherited date, so FIFO
in the receiving account consumes them in their original sequence. Taking the `transfer_out`'s order
instead would sort a parcel after a same-day buy it preceded. [DEC-079, DOM-090]

**DEC-080 — A transfer carrying its own fee rejects the import.** DOM-107 added such a fee to the
carried basis; the alternative records it as fees, which keeps it separable, as DEC-004 keeps buy
fees. No transfer, exchange or merger in the exports carries a fee, so neither reading can be checked
against data. Refusing one blocks nothing today and follows DEC-073. The fee's division across
emitted records, and its undefined case at a zero basis, go with it. [DEC-080, DOM-107]

**DEC-081 — An emitted `transfer_in` carries its parcel's allocated buy fee as fees.** DEC-004 makes
gain proceeds minus sell fees minus cost minus buy fees, and a transfer realizes nothing, so a buy
fee left behind at the transfer would never be deducted from any gain. It travels as the emitted
record's fees rather than being folded into cost, keeping it separable as DEC-004 and DEC-080 do.
[DEC-081, DOM-106]

**DEC-082 — The corporate-action ordinal counts in normalized order.** Saxo files are newest-first,
so a row added to a group by a later export of the same year lands at the top in file order and
shifts every existing row's ordinal, changing identities that SRV-015 needs stable across
re-imports. In normalized, oldest-first order a later row takes the next ordinal and existing ones
keep theirs. Corrects DEC-033's "in file order". [DEC-082, IMP-SAXO-008, IMP-SAXO-025]

**DEC-083 — Emitted quantities sum to the rounded product.** Storage refuses a quantity off its
scale (ARC-010), so the only sum emitted records can reach is the transferred quantity times the
ratio rounded once to the quantity scale, half away from zero, the rounding ARC-010 prescribes.
[DEC-083, DOM-115]

**DEC-084 — No operation re-rates a stored transaction in v1.** DOM-069, SRV-050, ARC-028 and DEC-068
speak of re-rating, but no requirement defines what it does, and the storage method written for it
restated the rate while leaving the EUR figures as they were, breaking DOM-028. It is removed.
DOM-069's re-rating clause then holds by construction, as its edit clause already does. DEC-068's
remedy for an ECB correction, re-rating the transaction, is therefore unavailable until re-rating
is specified; the gap is accepted, since a correction to an already-cached day is rare and small.
[DEC-084]

**DEC-085 — A date after the newest publication has no rate yet.** A Saturday trade imported on
Sunday and a weekday trade imported before that day's 16:00 CET publication look the same to the
cache: nothing after the date. Only the first will never be published, and substituting the earlier
rate for the second fixes a guess into the transaction for good. A day counts as a non-publication
day under DOM-034 only when a later publication proves it was skipped; before that ARC-019 refuses.
The cost is waiting a day, and imports are mostly of past years. Rejected: a calendar of weekends
and TARGET holidays, correct in both cases but a table to maintain. [DEC-085, DOM-034, ARC-019]

**DEC-086 — An emitted `transfer_in` counts as derived by its `transfer_out`'s batch.** DEC-079 has
an emitted record cite the `transfer_out`'s source records. It has no batch of its own, so DOM-119
read it as a foreign citation and refused, forever, to undo the batch holding the transfer. DOM-094
already deletes emitted records with their `transfer_out`, so they belong to its batch for DOM-119.
A later disposal that consumed one is still protected, by SRV-022's attribution refusal.
[DEC-086, DOM-119]

**DEC-087 — An unreadable ordering key is a failed row.** SRV-058 did not define "fails to parse",
so an unreadable trade date could be read as a file-level refusal naming only the first such row.
DEC-074 names every failed row "so that one round of fixes suffices", which a first-row refusal
defeats. Any row-level value the import cannot read, its ordering key, identity or classification,
makes the row a failed row, and all of them are named together. [DEC-087, SRV-058]

**DEC-088 — One refusal reports every ground; a batch has no failed count.** A file with trade dates
in two years and a failed row was refused on the years alone, so the user needed a second round to
learn of the row, which DEC-074's "one round of fixes" rules out. Every ground the import can
determine is reported together. And since a failed row refuses the whole import (SRV-058), a stored
batch can never have failed rows, so the batch's `failed` count is removed from its fields, as it
already was from the response summary (SRV-017). [DEC-088, SRV-059]

**DEC-089 — A referenced account is neither deleted nor renamed.** An account's only fields, broker
and id, are its key: record identities are scoped to it (DOM-024) and IMP-003 checks imported files
against its id. Rewriting that key under existing records would change their identities and
re-judge files already accepted, against the immutability DOM-008 gives records. SRV-008 already
refused deletion over a source record; batches, manual entries and transactions name the account
too, and deleting it under them would orphan them. A mistaken account is corrected by undoing its
batches, fixing it, and re-importing, which idempotence (SRV-015) makes cheap. [DEC-089, SRV-008]

**Provisional decisions DEC-090 to DEC-093.** Taken on a best-effort basis on 2026-09-29 so the
server could be finished without halting, at the user's instruction. Each is the most conservative
reading consistent with the rest of the specification, and each awaits the user's ratification; see
the provisional section of `open-questions.md`.

**DEC-090 — Provisional: order of a transaction built from several records, and of a decomposition's
legs.** A transaction consuming several records takes the lowest of their orders, so it sorts where
its first row does. Of a decomposition, the `sell` consumes (DOM-101), as it carries all the money
(DOM-116), and sorts first; the `transfer_out` cites the same records and sorts immediately after.
The effect: FIFO gives the sale the oldest parcels. Alternative not taken: dividing every parcel
between the legs in proportion, which may better fit a per-share mixed consideration. [DEC-090,
DOM-011, DOM-101, DOM-091]

**DEC-091 — Provisional: exhaustion is judged at the quantity scale.** An effective quantity stays an
exact rational (DOM-113) but is compared at the 8-decimal quantity scale, rounded half away from
zero; a parcel whose remaining quantity rounds to zero there is exhausted, and allocation sums are
checked at that scale. The residue below 1e-8 of a share is dropped. [DEC-091, DOM-064, DOM-065,
DOM-113]

**DEC-092 — Provisional: canonical order uses the oldest supplying batch; undo returns ownership.**
Ordering by the owning batch's age made a re-import reorder records nobody edited, so the tie-break
uses the oldest batch that supplied the record, which never changes. Deleting a batch removes the
records it owns unless a remaining batch also supplied them, in which case the newest remaining
supplier owns them again: undoing a re-import restores the previous import instead of losing the
year. [DEC-092, DOM-111, SRV-021, SRV-052]

**DEC-093 — Provisional: a `transfer_out` has no line in the acquisition report.** It realizes no
gain and has no proceeds, so it is not a disposal; its parcels are reported where the emitted
`transfer_in` records are later consumed, carrying their inherited cost and buy fee. [DEC-093,
DOM-078, DOM-112]
