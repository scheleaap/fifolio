# PLAN

Ordered work items derived from `design/`. One item is one coherent, testable increment.
Item ids (`FIF-nnn`) are stable and never reused. Ordering is by dependency first, then by risk:
domain types, arithmetic, FIFO and storage come before anything that merely exposes them.
Document order is plan order; new items are inserted where they belong, not appended.

Statuses: `todo` | `in-progress` | `done` | `blocked`.

A `blocked` item carries at least one requirement named on a `Blocks:` line in
`design/open-questions.md`, which is the authoritative list of what is undecided. Nothing else makes
an item blocked.
Such an item cannot be reviewed against the specification, so it is not implemented. Where the
decided remainder of an item was a large, coherent increment in its own right, the undecided
requirement was **split out into its own item** rather than freezing the whole area; each such split
is recorded on both halves. Where the undecided requirement is the substance of the item, the item
is blocked whole.

Revision history, retired items, decisions required and the requirement coverage report are at the
bottom.

---

## FIF-001 Workspace and CI baseline
Status: done
Requirements: ARC-001, ARC-002, TST-008, TST-024, TST-025, TST-026, TST-027
Depends on: none
Acceptance: Cargo workspace (edition 2024, resolver 3) with `fifolio-core`, `fifolio-server`, `fifolio-cli`; core carries no HTTP or terminal dependency; CI runs `cargo test`, `cargo fmt --check`, `cargo clippy -- -D warnings` and `cargo llvm-cov`; the per-crate thresholds 95 / 90 / 75 gate **changed lines** via `diff-cover` against the base commit or the target branch, with the absolute figures reported and not gated until build-out completes.
Notes: The patch-coverage requirements TST-025, TST-026 and TST-027 are new in this revision of `testing.md`; `.github/workflows/ci.yml` already implements them, so this item stays `done` and simply gains the ids. The stub binaries printing "not implemented yet" are *not* part of this item; replacing them is FIF-032 and FIF-042.

## FIF-002 Test layer conventions and harness
Status: done
Requirements: TST-001, TST-002, TST-003, TST-006, ARC-024
Depends on: FIF-001
Acceptance: the repository documents and demonstrates the three layers — unit (one module, no I/O), integration (crate against real deps in process, temp SQLite / real HTTP surface), end-to-end (real binaries, real db, free port); a shared test-support module provides a temporary-database helper and a free-port helper; every test names the requirement ids it covers, in the convention `.claude/agents/README.md` describes.
Notes: TST-002 enumerates what must end up unit tested in core; the individual items below each carry their own share. This item owns the convention and the harness, not the coverage of every rule.
The harness is the workspace member `fifolio-test-support`, a dev-dependency of the other three crates; its crate documentation is where the layers and the requirement-id convention are written down. The unit-layer demonstration lives there too, core carrying no logic yet; the first core unit tests arrive with FIF-004.
Both binaries are stubs that exit before reading `argv`, so the end-to-end demonstration can only assert that they are built and fail loudly. The temporary database and the free port are passed but nothing consumes them; the assertion that a live process opens the one and binds the other belongs to FIF-032 and FIF-042. ARC-024 is checked mechanically against `Cargo.lock` in `fifolio-core/tests/dependency_graph.rs`.

## FIF-003 Anonymized broker fixtures
Status: done
Requirements: TST-011, TST-012, TST-013, TST-014, TST-028, TST-029
Depends on: FIF-001
Acceptance: committed fixtures under a fixtures directory, derived from `design/example_exports/` (gitignored, present locally):
* Saxo: a real XLSX container, one sheet, the 31 Dutch headers byte-for-byte including `Bk\xa0Record\xa0Id`, `Booking\xa0Id` and the leading space in ` Positie-ID`; Excel serial dates; rows emitted newest first; `Rekening-ID` values with `EUR`/`USD`/`CAD` suffixes on one base account; free-text `Acties` strings with their 2-decimal prices; at least one row of every observed `Acties` value (`Koop`, `Verkoop`, `Deponering`, `Expiratie`, `Fusie`, `Terugkoopaanbod`, `Terugboeking`, `Stock split`, `Omwisseling`, `Dividend`, `Keuzedividend`, `Herbeleggingsdividend`, `Rente`, `Service fee`, `ADR-kosten`, `Storting`, `Opname`); at least one multi-row corporate action sharing a `Corporate action-Id` where only one row carries a `Positie-ID` and another carries the money (the Philips shape); at least one three-row group under one `Corporate action-Id` (the TransAlta shape, which is what the within-group ordinal in IMP-SAXO-008 exists for); at least one reversal row; at least one `Bond` instrument; every fixture file confined to a single calendar year of trade dates.
* Trade Republic: quoted CSV, all 23 columns, ISO-8601 UTC `datetime` with sub-second precision distinct from `date`, UUID `transaction_id`, negative cash-flow amounts, populated `original_amount` / `original_currency` / `fx_rate` on at least one non-trade row, one `TAX_EXCHANGE` pair, one `STOCKPERK` credit with its paired `TRADING`/`BUY`, and a file whose row order matches neither `date` nor `datetime` (the 2025 shape).
* Account ids, client ids, personal names, IBANs and instrument-level identifying detail replaced; amounts perturbed.
* A committed, runnable anonymization script plus a README stating that fixtures test parsing, classification and idempotency only, never arithmetic.
* Anonymization perturbs **amounts only**: quantities and dates are structural — a perturbed quantity breaks the share counts a corporate action is recognized by, and a perturbed date breaks the per-file calendar-year boundary and the ordering cases — and free text naming a security is rebuilt rather than stripped, an `Acties` label carrying an instrument name no column holds.
Notes: This blocks every importer test, which is why it is third. The generator must be re-runnable against the real exports so a future export shape can be folded in; it must not embed the real values.
TST-028 and TST-029 are new in revision 7 and were added to this item rather than to a new one: they state what the generator must not touch, which is an acceptance clause of the generator, not a separate increment. TST-030 (a blank Saxo cell round-trips as an empty cell, which the reader must accept) is **not** here: the fixture writer is what cannot reproduce it, but the assertion is on the reader, so it sits in FIF-019.
Done in revision 8, commit `610ca2c`, which is the work revision 7 found uncommitted. The workspace member `tools/anonymize-exports` (32 unit tests) plus `tools/anonymize-exports/tests/fixture_structure.rs` (24 tests) and the committed fixtures under `fixtures/`: five Saxo XLSX (188 rows) and four Trade Republic CSV (65 rows), the whole corpus rather than a sample, because the structural properties the importers depend on are spread across the years. Identities are rank-derived pseudonyms with valid check digits, order-preserving for the booking counters; amounts move by up to 10% with zero, one, sign and scale preserved; quantities and dates untouched [TST-028, TST-029]. The tool refuses to write a file still containing a collected original, and generation is byte-reproducible.

## FIF-004 Money scales and rounding mode
Status: done
Requirements: ARC-006, ARC-007, ARC-010, DOM-087, TST-015, TST-016
Depends on: FIF-001
Acceptance: newtypes or wrappers over `rust_decimal` for quantity (8), unit price (6), monetary amount (2) and FX rate (6); no `f32`/`f64` anywhere in core; a single rounding function, **half away from zero**, applied only at storage and presentation boundaries, so that a realized loss rounds symmetrically to a gain; unit tests with hand-derivable synthetic inputs covering the scales and fractional quantities at full scale.
Notes: Split twice. Revision 2 moved ARC-009 (what "full precision" means for intermediate arithmetic) to FIF-054. Revision 3 moves the quotation factor (ARC-008, DOM-038) to FIF-075, which is now the undecided half; the scales and the rounding mode are decided and reviewable without it. TST-016's other boundary cases are asserted in FIF-021 (EUR derivation) and FIF-014 (fee distribution remainders); this item owns the convention and the cases that are purely about scale.
Done in revision 5, commit `1eec01a`. `decimal.rs` carries the four newtypes, `round_to` as the single half-away-from-zero rounding, and the `QuotedPrice` / `EffectivePrice` split that keeps the factor rule of FIF-075 reviewable; no `f32`/`f64` in core, enforced by `clippy::float_arithmetic`.
Amended in revision 19, without reopening the item: ARC-010 was restated in `72abea7` (DEC-067) to say rounding happens **before** the storage and presentation boundaries and is the caller's act, a store refusing an unrounded value rather than absorbing it. Nothing this item shipped is invalidated — `round_to` is still the single half-away-from-zero function and callers are still who apply it. What the restatement changes is who checks, and that is a clause of **FIF-011**, whose acceptance now carries it.

## FIF-075 Trade value and the quotation factor
Status: blocked
Requirements: ARC-008, DOM-038
Depends on: FIF-004, FIF-005
Acceptance: `trade_value(quantity, unit_price, factor)` with factor 1 per unit or 0.01 percent of par, applied exactly once, and a distinct path for a unit price obtained by dividing a value by a quantity, which is already effective and must not take the factor again; unit tests covering per-unit vs percent-of-par, including that confusing them is a factor of 100.
Blocked by: ARC-008 and DOM-038 are on the undecided list.
Notes: Split out of FIF-004 in revision 3. FIF-026 (the bond assertion that 3000 nominal at 139.46 costs 4183.80) and FIF-021 (EUR price reproduced with the factor applied once) both read this rule and are only fully reviewable once it is settled; their own requirements are decided, so they stay `todo`.

## FIF-054 Intermediate precision policy
Status: done
Requirements: ARC-009
Depends on: FIF-004
Acceptance: a stated, tested policy for how precision is retained across a chain of operations — where a division may be left unrounded, what happens when `rust_decimal`'s 28-significant-digit limit is reached, and at which call boundaries a value must already be at its scale.
Notes: Split out of FIF-004 in revision 2 and blocked then; ARC-009 left the undecided list in revision 3, so this is now buildable. Every item that divides — FIF-014, FIF-061, FIF-063 — reads it.
Done in revision 5, commit `e38de9d`. `precision.rs` states the policy in its module documentation and tests it: the 28-significant-digit limit (not 28 decimals), its half-to-even rounding which is *not* ARC-010, unreliable round-tripping of a quotient, `checked_*` on overflow, and the two boundaries — persistence and presentation — at which a value must already be at its scale.

## FIF-005 Core reference entities
Status: done
Requirements: DOM-004, DOM-005, DOM-006, DOM-007, DOM-008, DOM-017, DOM-037
Depends on: FIF-004
Acceptance: `Account`, `Security`, `SourceRecord` and `ImportBatch` types; `SecurityType` is the fixed enum `stock` | `bond` | `etf` | `fund` | `derivative` | `other`; `Quotation` is the fixed enum `per_unit` (default) | `percent_of_par`, editable independently of type; securities carry an auto-created flag; a source record holds raw content plus parsed fields and is constructible but never mutable; a batch records account, filename, format, timestamp and its four counts.
Notes: Types only, no persistence and no engine. `Quotation`'s *defaulting* rule and the exact-price-storage rule moved to FIF-055; this item owns the enum and its editability. Reviewable against the Entities section of `domain.md` on its own.
Done in revision 5, commit `9846b5a`. `entities.rs` carries `Account`, `Isin`, `SecurityType`, `Quotation` (`PerUnit` by `#[default]`), `Security` with its auto-created flag and independent `with_quotation` / `with_security_type`, `SourceRecord` (getters only, no setters), `SourceFormat`, `ImportCounts` and `ImportBatch`.

## FIF-055 Quotation defaulting and exact price storage
Status: done
Requirements: DOM-036, DOM-039
Depends on: FIF-005
Acceptance: quotation is defaulted from the broker's instrument type at auto-creation and never guessed afterwards; prices are stored exactly as the statement shows them, so any figure in the application reconciles against a broker document.
Notes: Split out of FIF-005 in revision 2 and blocked then; DOM-036 and DOM-039 left the undecided list in revision 3. Interacts with DEC-041 (an undeterminable quotation rejects the import) and with IMP-TR-021, which is still blocked (FIF-069).
Done in revision 6, commit `12790ea`. `quotation.rs` carries `quotation_for(SecurityType)`: a bond is `percent_of_par`, everything else `per_unit`. It takes a type and not a security, so re-deriving the quotation of an existing one — which would undo a user's correction on the next import — is unreachable, which is how "never guessed afterwards" is enforced rather than checked.
DOM-039 ships **documented but not asserted here**: this item stores no price, so its only possible test compared two decimal literals and would have passed with the module deleted. The rule is carried by `QuotedPrice` / `EffectivePrice` (FIF-004) plus the importer's divide-by-factor step, and the observable assertion — a stored price equal to the figure on a sample document — lands with FIF-021 and FIF-026. Recorded here so the gap is not rediscovered.
Amended in revision 8, without reopening the item: DOM-039 was restated in `8dbca69` to promise reconciliation against the **booked amounts** rather than against the printed unit price. `quotation.rs` neither stores nor prints a price, so nothing it ships is invalidated; what changes is the assertion FIF-021 and FIF-026 owe, which is now that the derived price reproduces the booked gross.
A first version of the module took a per-format "is percent-of-par confirmed" flag and returned an undeterminable variant for an unconfirmed bond; that would have answered OQ-006 in code. It was removed. The trigger stays with IMP-TR-020 / IMP-TR-021 in the blocked FIF-069.

## FIF-056 Transaction sum type and its variants
Status: done
Requirements: DOM-010, DOM-012, DOM-016, DOM-081, DOM-083
Depends on: FIF-005
Acceptance: `Transaction` is a sum type whose six variants — `buy`, `transfer_in`, `sell`, `expiration`, `transfer_out`, `split` — each carry only their own fields, so one variant can never be read as another; every variant carries a trade date; `fees` is a single field summing commission, exchange fees and transaction taxes; opening variants (`buy`, `transfer_in`) and closing variants (`sell`, `expiration`, `transfer_out`) are distinguishable in the type system and `split` is neither; `transfer_in` carries a source (`broker` or corporate action) and a date provenance (`transfer_date` or `inherited`), neither of which is editable; a transaction cites the source records it was derived from.
Notes: Replaces the transaction/corporate-action pair revision 1 planned; see the Retired items section for FIF-018. Types only. DOM-011 and DOM-013 (the `order` key and the account/security/source-record relations every variant carries) moved to FIF-076 in revision 3, both being undecided; DOM-082 (a buy's origin) is in FIF-057, DOM-101 (consume vs cite) in FIF-058. Until FIF-076 lands, the variants carry their own fields and a date but no relations, which is a smaller increment than it looks and is the reason FIF-076 sits immediately after it.
Done in revision 9, commit `74e4bd1`, which is the work revisions 8 and 9 found uncommitted: `crates/fifolio-core/src/transaction.rs` and the `mod transaction;` line in `lib.rs`. Revision 9 re-checked the module's surface against acceptance before committing: six variants under `Opening` / `Closing` / `Split`, a `Derivation` carrying the trade date on every variant, `TransferInSource` and `DateProvenance` read-only behind accessors.
Shape of what was built: `Transaction` nests `Opening` (`Buy`, `TransferIn`) over `Closing` (`Sell`, `Expiration`, `TransferOut`) over `Split`, so DOM-081's grouping is a type a function can take rather than a label it must check. Every variant carries a `Derivation` — trade date plus the source-record identities it cites [DOM-016] — and citations are by `RecordIdentity`, not by a relation, since DOM-013 is FIF-076's. The `>= 1` cardinality is deliberately not enforced: OQ-002 says an emitted `transfer_in` is derived from no row, so a non-empty check here would answer it in code. Fields belonging to other items (EUR pairs, buy origin, expiration quantity, transfer and split ratios) are absent and the module documents each absence against its owning item.

## FIF-057 Buy origin and stock-dividend cost basis
Status: done
Requirements: DOM-082
Depends on: FIF-056
Acceptance: a `buy` records how it arose — an ordinary purchase, or shares issued as a stock dividend whose cost basis is their taxable value at issue — and the taxable value is sourced rather than invented.
Notes: Split out of FIF-056 in revision 2 and blocked then; DOM-082 left the undecided list in revision 3. FIF-025 is the Saxo dividend heuristic that produces these buys.
Confirmed unblocked in revision 10: OQ-014 asks where a `Herbeleggingsdividend`'s share count and price come from, but it blocks IMP-SAXO-013 only, so it freezes the Saxo importer (FIF-023, FIF-025) and not this type. Build the origin so that the taxable value arrives from a caller and is never computed here; that is what keeps DOM-082 reviewable while OQ-014 is open, and it is why the item sits before the importer that fills it.
Its dependency FIF-056 is `done` (`74e4bd1`, recorded `5d54c2b`); `transaction.rs` already names this item at the absent field, so the increment is a variant field on `Buy` plus its unit tests, not a new module.
Done in revision 11, commit `c1c3d58`. Verified against `crates/fifolio-core/src/transaction.rs` rather than the commit message: `Buy` carries `origin: BuyOrigin`, whose `StockDividend` variant holds the parcel's `taxable_value` as a carried `Money` with no constructor deriving it from quantity and unit price, and `Purchase` holds nothing. Its unit test fixture deliberately sets a taxable value that is *not* quantity times unit price, so a later implementation that computes the basis instead of reading it fails the test. Where that figure comes from for a Saxo `Herbeleggingsdividend` is still OQ-014 and still FIF-023 / FIF-025's problem.
Partly corrected by FIF-091: DEC-061 (revision 13) settled that the taxable value **is** the buy's stored EUR gross, so the carried `taxable_value` field this item shipped is gone and the fixture's divergence it protected is the failure FIF-091 forbids. DOM-082 stays here; DOM-123 is FIF-091's.
**Partly invalidated in revision 13 by DEC-061 (DOM-123).** The shipped shape holds the taxable value as a field of its own, and the very fixture this item's note cites as its safeguard — 81.00 against a stated 3 × 26.10 — is the divergence DEC-061 names and forbids. The item is **not** reopened and its acceptance is left as written, so the record of what was built and why stays legible; the correction is **FIF-091**, which carries DOM-123 and states what must change. DOM-082 itself is unchanged: the basis is still the taxable value and is still never computed here.

## FIF-058 Consumption versus citation
Status: blocked
Requirements: DOM-101
Depends on: FIF-056
Acceptance: a transaction consumes the source records its existence answers and may cite further records without consuming them; consumption is what clears a record from the completion queue, citation is what preserves the audit trail; a decomposition names which of its transactions consumes.
Blocked by: DOM-101 is on the undecided list.
Notes: Split out of FIF-056. DOM-070 (at most one consumer) is in FIF-060 and blocked alongside it; SRV-022 and DOM-119 (batch deletion refused on a citation the batch did not derive) are in FIF-012 and FIF-036, which can only be finished once this is settled.

## FIF-059 Manual entry entity
Status: done
Requirements: DOM-098, DOM-099, DOM-100, DOM-122
Depends on: FIF-005
Acceptance: a `ManualEntry` type separate from `SourceRecord`, holding account, security, what the user supplied, and **the broker identities of the source records it answers** rather than internal keys, so it survives their deletion; the separation is structural — nothing in the type system lets an import undo remove one; what was supplied is the closed set of **three** shapes — a share count, a stock-or-cash election, a target security with a ratio — and an acquisition date is not among them, it being fixed at import and never corrected by hand.
Notes: New in revision 4; the previous plan modeled manual information as a `manual` source record kind, which DEC-038 reversed.
DOM-122 was added to this item in revision 12. It is the one identifier `design/` gained in commit `9d8c9b9` (DEC-060), and it is a closure clause on DOM-097 rather than a separate increment: it says which shape is *not* in the set this type models, so it belongs where the set is defined. It invalidates no completed work — FIF-024 already carried the fixed acquisition date from DEC-040, and FIF-038 already states that no endpoint edits one.
Done in revision 13. Reviewed against acceptance rather than rebuilt: `manual_entry.rs` carries `ManualEntry` (account, `Isin`, `Supplied`, `Vec<RecordIdentity>`, accessors only) [DOM-098], `Supplied` with exactly the three DOM-097 shapes and no acquisition date [DOM-122], and an integer `Ratio` (DEC-055). The separation from `SourceRecord` is structural — no import batch, no owned record — so an undo has nothing here to remove [DOM-100]; that is a compile-time property no test can name. The one change the review made was to the module documentation, which called the variant set open to a further variant where DEC-060 closes it.
Revision 12 marked this `in-progress`; revision 13 put it back to `todo`, which is the convention revisions 5, 7 and 9 applied to FIF-055, FIF-003 and FIF-056 — uncommitted is unreviewed, and an unreviewed item is the item still to build. The deviation was not harmless: `in-progress` is not a selectable status, so a plan carrying two of them named nothing to build while two modules sat uncommitted. Completion is still recorded against commits, never against a working tree.
Done in revision 14, commit `4af8667`, which is the module revision 13 found uncommitted.
**Partly invalidated the same day by DEC-062 (`a0ee437`), which landed after that commit.** DOM-097's list is five shapes, not three, and `manual_entry.rs` shipped the three. The correction is **FIF-092**, which now carries DOM-097; this item keeps DOM-098 to DOM-100 and DOM-122 — the entity, its broker-identity references, its structural separation from `SourceRecord` and the closure clause on the acquisition date — none of which DEC-062 touches. The acceptance above is left as written, including the word "three", so what was built and on what reading stays legible.

## FIF-092 Manual entry shapes are the five the completion queue asks for
Status: done
Requirements: DOM-097
Depends on: FIF-059
Acceptance: `Supplied` carries exactly the shapes `domain.md` now lists — a stock-or-cash election, carrying a share count when the answer is stock; a ratio alone, for a split; a target security with a ratio, for an exchange; a quantity disposed with an optional target security, for a cash merger, tender or partial buyback — and no others; a share count that exists only as part of a stock election cannot be constructed apart from one; each shape is reachable from the completion-queue case in `cli.md` and `importers.md` that asks for it, and a unit test names that case per shape.
Notes: New in revision 14, carrying DOM-097 away from FIF-059.
**This item exists because a completed item is now partly wrong.** FIF-059 shipped `Supplied` with three variants — `ShareCount`, `Election`, `Exchange` — in commit `4af8667`; commit `a0ee437` (DEC-062) landed afterwards and widened DOM-097 to five, withdrawing DEC-060's claim that the set closed at three. The two missing shapes are a split's bare ratio and a disposed quantity with an optional target, both of which `importers.md` already marks pending, so the gap is not hypothetical: the completion queue would have had rows with no shape to answer them in. Rather than editing FIF-059's acceptance so the shipped code reads as correct in hindsight, the correction is carried here and reviewed on its own.
`ShareCount` as a standalone variant is the part that must change rather than be added to: DOM-097 binds the count to the stock election, and leaving both shapes available lets a caller record a count with no election behind it.
DOM-122 stays with FIF-059. It says which shape is *not* in the set, and DEC-062 explicitly leaves DEC-060's own subject standing.
Done in revision 14. `Supplied` now reads `Election` (with `Election::Stock { shares }`), `Split`, `Exchange` and `Disposal { quantity, target: Option<Isin> }`; the standalone `ShareCount` variant is gone, so a count is reachable only through a stock election. One unit test per completion-queue case names the case and the `Acties` row behind it, and the exhaustive match over all five stands in for the closure clause.

## FIF-006 Per-file order computation
Status: done
Requirements: DOM-040, DOM-088, DOM-102
Depends on: FIF-005
Acceptance: each source record carries an integer `order` computed **when the file is read**, from the file's own content: trade date, then the format's stated ordering columns, then the row's position normalized to the file's direction, so the order within a file is total and no import is ever refused for ambiguity; every variant takes part, `split` included; re-reading the same file reproduces the order exactly.
Notes: Rewritten in revision 2; the revision-1 acceptance — trade date, execution time, per-account import sequence — is gone, DOM-041 retired, DEC-039 and DEC-045 replacing it. Revision 3 splits out the cross-file part: DOM-011, DOM-013 and DOM-111 moved to FIF-076, all three undecided. What remains is the order *within one file*, which is decided, self-contained and testable by importing a fixture twice. Per-format ordering columns are FIF-066 (Saxo) and FIF-027 (Trade Republic).
Done in revision 5, commit `17cb593`. `ordering.rs` assigns orders from trade date, then the format-supplied ordering columns in their stated precedence, then row position normalized by `FileDirection`; tested to be a permutation and to reproduce exactly on a second read. The Saxo binding of the ordering columns (FIF-066) is now blocked by OQ-013, which does not affect this mechanism.

## FIF-076 Canonical order across files and the transaction order key
Status: blocked
Requirements: DOM-011, DOM-013, DOM-111
Depends on: FIF-006, FIF-056
Acceptance: the canonical order over an account and security is (trade date, `order`, batch age), so two records from different files sharing a date are settled by the age of the owning batch; every transaction variant carries the `order` of the source record it consumes and relations to account, security and one or more source records.
Blocked by: DOM-011, DOM-013 and DOM-111 are on the undecided list.
Notes: Split out of FIF-006 and FIF-056 in revision 3; DOM-011 and DOM-013 are one sentence of `domain.md` and belong together. Everything that reads the canonical order — FIF-013, FIF-061, FIF-014, FIF-015 — depends on this rather than on FIF-006 alone.
FIF-012 built DOM-066 and DOM-068 over the stand-in **(trade date, row id)**, the canonical order being unavailable, and DOM-072 over a single `derived_by_batch` per transaction rather than over the records a transaction was derived from. Both are this item's to replace: the two comparisons in `storage/attributions.rs` and their note, and `derived_transactions` in `storage/entities.rs`, which must be re-keyed on the transaction-to-source-record relation so DOM-072 answers for every batch whose records a transaction was derived from. Its acceptance therefore reads DOM-066, DOM-068 and DOM-072 through, and their integration tests move with the comparison.

## FIF-007 Source record identity and idempotency
Status: done
Requirements: DOM-022, DOM-023, DOM-024
Depends on: FIF-005
Acceptance: an identity abstraction that takes either a broker reference or a hash of the parsed business fields, always scoped to the account; identical rows in two accounts produce distinct identities; re-importing the same rows into the same account produces no new source records. Format-specific identity rules live in FIF-020 and FIF-027.
Done in revision 5, commit `fdf8990`. `identity.rs` offers `IdentitySource::BrokerReference` / `ParsedFields`, both scoped to the account, with a length-prefixed framing tested against flattening collisions and against a reference aliasing a hash.

## FIF-008 EUR valuation and the stored gross
Status: done
Requirements: DOM-025, DOM-026, DOM-027, DOM-028, DOM-029, DOM-085, DOM-086
Depends on: FIF-004, FIF-056
Acceptance: every transaction stores native figures and EUR figures at the same scales, together with the rate, its source and its date; the valuation date is the trade date, never settlement; no separate currency-gain figure exists anywhere in the model; **the EUR gross total is stored as well as the unit price**; the stored rate is foreign units per EUR (`EUR = native / rate`), and a format quoting the inverse converts at full precision from the figures the file states, never from the rounded stored rate.
Notes: DOM-085 (DEC-028) and DOM-086 (DEC-027) were new in revision 2 and change the shape of every transaction record; this is why the item sits before storage rather than beside it. Revision 3 splits DOM-104 — that the stored gross, not the unit price, is what every calculation reads — into FIF-077, because it is undecided.
Started and uncommitted at revision 13: `crates/fifolio-core/src/valuation.rs` is untracked, `lib.rs` declares it, and `crates/fifolio-core/src/transaction.rs` is modified to carry a `Valued` native/EUR pair per money figure plus a `Conversion` (rate, source, date) on every money-bearing variant, with the trade date named as the valuation date and the EUR gross stored alongside the unit price. The suite passes. It stays `todo` for the same reason FIF-059 does: there is no commit to cite. What is outstanding is review against the eight requirements above and a commit.
Whoever finishes it should read **FIF-091** first. That item collapses a stock dividend's taxable value into the EUR gross this item introduces, and the working tree still carries both figures separately; committing the pair as-is entrenches the divergence DEC-061 forbids.
Still uncommitted at revision 14 and still `todo`, for the same reason: `valuation.rs` is untracked and `transaction.rs` is modified in the working tree. The suite is green there (166 tests). The outstanding work is review against the eight requirements above, then a commit. Revision 14 puts **FIF-092** ahead of this item — that is a correction to code already committed wrong, and it touches `manual_entry.rs` only, so the two do not collide in the tree.
Still uncommitted at revision 15, a third consecutive revision, and still `todo`: `valuation.rs` is untracked and `transaction.rs` modified, the suite green at 169 tests. FIF-092 landed in `cd09f97` around that tree without disturbing it, so nothing now sits ahead of this item. If a fourth revision finds it uncommitted, what needs fixing is the hand-off, not the item — the outstanding work is review against the eight requirements and a commit, then **FIF-091** on top, collapsing the stock dividend's taxable value into the EUR gross before the pair DEC-061 forbids is entrenched.
Done in revision 15's build. The tree described above was reviewed against the eight requirements and committed as it stood, with one correction: a unit-test comment in `valuation.rs` called 218.32 USD "the worked Saxo buy", which is neither its booked amount (238.00) nor its gross (230.00); the figures now match `importers.md`. `Expiration` carrying a `gross` of zero, and `TransferOut` carrying none, are this item's two judgement calls — the first so every cash closing answers one formula, the second because a transfer's basis is derived from its allocations (DOM-112, FIF-080). **FIF-091** is next and lands on top: the stock dividend's `taxable_value` is still a second figure beside the EUR gross in this commit.
Review of that commit moved **DOM-084** to FIF-014: its substance is that an allocation share derives from the native/EUR pair the same way on both sides and is never stored independently, and no allocation type exists yet, so nothing here could assert it. What this item does own of it — that both halves are one kind at one scale — is DOM-029.

## FIF-077 The stored gross governs every calculation
Status: blocked
Requirements: DOM-104
Depends on: FIF-008
Acceptance: allocation shares are taken pro-rata from the booked EUR gross total and never rebuilt from the unit price; the unit price exists for display and reconciliation against the statement and nothing computes with it, enforced so that a caller cannot reach for it by accident.
Blocked by: DOM-104 is on the undecided list.
Notes: Split out of FIF-008 in revision 3. FIF-014's formulas read whichever figure this settles, so the two must be reviewed together once it is decided.

## FIF-091 A stock dividend's taxable value is its stored EUR gross
Status: done
Requirements: DOM-123
Depends on: FIF-057, FIF-008
Acceptance: a `buy` issued as a stock dividend holds the taxable value at issue and the EUR gross as **one stored figure**, not two that can disagree; there is no field, constructor argument or accessor by which a caller can set them to different values, so the rule that every calculation reads the gross cannot pick up a number other than the basis; the unit tests of FIF-057 that assert a taxable value differing from the buy's own EUR figures are replaced by tests asserting the identity, and the replacement is deliberate rather than a deleted test.
Notes: New in revision 13, carrying the one identifier `design/` gained in commit `47af98a` (DEC-061).
**This item exists because a completed item is now partly wrong.** FIF-057 (`c1c3d58`) shipped `BuyOrigin::StockDividend { taxable_value }` as an independent `Money`, and its fixture sets 81.00 against a stated 3 × 26.10 = 78.30 — which is exactly the pair DEC-061 names as the failure it is closing. Rather than editing FIF-057's acceptance to make the shipped code retroactively correct, the correction is carried here where it can be reviewed on its own. FIF-057 keeps its record and gains a pointer to this item.
It depends on FIF-008 because the figure the taxable value must **become** is the stored EUR gross, which FIF-008 is what introduces; doing it before that would mean collapsing into a field that does not yet exist. Since FIF-008 is implemented but uncommitted, the cheapest correct order is to finish FIF-008 and land this on top, rather than committing a shape that must then be unpicked.
Named next to build in revision 16, FIF-008 having landed in `9a1e457`. The working tree is clean and the suite green, so this item starts from a committed base; the two figures to collapse are `BuyOrigin::StockDividend { taxable_value }` in `transaction.rs` and the buy's stored EUR gross, and the fixtures asserting 81.00 against 3 x 26.10 are the tests DEC-061 makes wrong.
Where a Saxo `Herbeleggingsdividend`'s figure comes from is untouched by this and is still OQ-014, blocking IMP-SAXO-013 in FIF-023. DEC-061 says the two numbers are one; it does not say where that one number is read.
Done in revision 16. `BuyOrigin::StockDividend` is now a unit variant and `Buy::taxable_value` reads the stored EUR gross, so no constructor argument, field or accessor can state the two apart. FIF-057's assertion that the two figures differ is replaced, not deleted: the test that asserted 81.00 against 3 x 26.10 now asserts the identity, and three cases were added for an ordinary purchase, a foreign gross whose EUR half is the basis, and a zero gross.

## FIF-009 FX rate resolution
Status: done
Requirements: DOM-030, DOM-031, DOM-032, DOM-033, DOM-034, DOM-035, ARC-019, ARC-027, TST-019, TST-020, TST-021
Depends on: FIF-008
Acceptance: rate source precedence `broker` > `ecb` > `native`; broker-stated EUR figures used verbatim with the implied quotient stored as the rate and marked informational, since nothing computes with it; EUR-denominated transactions get rate 1 and source `native`; an ECB lookup with no rate for the trade date falls back to the most recent published rate before it and stores that rate's own date; the fallback is bounded — nothing before the series begins in 1999, and a substitution more than seven days stale is an error; fees convert at the leg's rate; a missing, unfetchable rate fails with an error naming currency and date. The rate source is an injected trait; unit tests use a fake table with weekend and holiday gaps. No test opens a socket.
Notes: Named next to build in revision 17, FIF-008 having landed in `9a1e457` and FIF-091 in `aa6aac5`. The working tree is clean and the suite green (173 tests), so this item starts from a committed base and is the first of the arithmetic items that needs no type of its own from an earlier item.
The rate table itself is **not** here: persistence, seeding and the top-up feed are FIF-010, which depends on this item and on storage. What this item owns is resolution — the precedence, the fallback and its bound, the error — behind an injected trait, so it is unit testable with a fake table and reaches no socket [TST-019, TST-020, TST-021]. `valuation.rs` (FIF-008) already carries `Conversion` (rate, source, date), which is the shape a resolved rate must fill; ARC-027's "EUR = native / rate" direction is settled there and must not be re-decided here.
Done in revision 17, commit `6384132`, in `crates/fifolio-core/src/fx.rs`. `resolve` answers all three sources behind the injected `RateTable` port: an EUR leg is tested first and takes rate 1 from `native`, a file-booked EUR figure takes the importer's quotient from `broker`, and a native-only leg looks the ECB rate up. The gap fallback stores the substituting publication's own date, and is bounded by `series_start()` and `MAX_SUBSTITUTION_DAYS`, so `BeforeSeries`, `Unavailable` and `StaleSubstitute` are three distinct errors and each names the currency and the date. `fee_in_eur` converts at the leg's own rate and refuses an informational one; `RateSource::is_informational` is that rule in mechanical form, and sits beside the enum in `valuation.rs`. Twelve unit tests run against a fake table built from the real 2024 Easter publication gap; none opens a socket.

## FIF-010 ECB rate cache and seeding
Status: done
Requirements: ARC-015, ARC-016, ARC-017, ARC-018
Depends on: FIF-009, FIF-011
Acceptance: a rate table keyed by currency and date; a seeding path that ingests the ECB full historical series (1999 onward) and a top-up path for the rolling 90-day feed; once seeded, imports resolve rates with no outbound call. Seeding is tested against a committed recorded fragment of the series, never a live fetch; **a top-up adds days the cache lacks and never rewrites one it holds**, even when the 90-day window restates that day at a different figure [DEC-068].
Notes: Named next to build in revision 20, both dependencies now `done` — FIF-009 in `6384132` and FIF-011 in `e5c2900` — and none of ARC-015 to ARC-018 is on a `Blocks:` line. It is the first `todo` in document order and the first item whose two halves, a schema and an outside feed, were each built by an earlier item.
This item supplies the table behind FIF-009's `RateTable` port; `latest_on_or_before` is the one query it must answer, and the reason that port has one method rather than an exact lookup plus a walk is recorded in `fx.rs` and must not be re-litigated here. The resolution rules — precedence, the bound, the three errors — are FIF-009's and are `done`; what is new is persistence, seeding from the full historical series and the top-up from the rolling 90-day feed.
The migration that adds the rate table belongs with FIF-011's versioned migrations [ARC-012] and its columns must already be at scale, since `codec::at_scale` is the only way a decimal reaches a column: an FX rate is scale 6.
ARC-016 ("once cached, imports work offline") is the observable half: a resolution against a seeded table must make no outbound call, and no test may open a socket [TST-019 to TST-021]. Seeding is tested against a committed recorded fragment of the ECB series, never a live fetch, which means the fragment is a fixture this item adds. The fetcher itself is an injected port for the same reason the rate table is.
ARC-019 (the error naming currency and date) is **not** here — it is FIF-009's and shipped as `Unavailable`. This item must surface that error unchanged rather than adding a second one.
ARC-017 and ARC-018 are **not closed by this item**. What it supplies is the ingest half: the parser, the two additive paths and the `RateFeed` port. Nothing in the workspace implements that port or records the two document URLs, so "seeded from the ECB's complete historical series" and "topped up from the 90-day feed" have no production caller yet — the acceptance scopes the fetcher out deliberately. The two requirements stay open until FIF-041 (SRV-046, SRV-047) supplies a `RateFeed` and the URLs it fetches; they must not be marked covered here.
The restatement policy is DEC-068: a top-up keeps the cached rate rather than rewriting it, which ARC-018 did not state either way.
Revision 21 found the whole increment written and **uncommitted**: `crates/fifolio-core/migrations/0002_fx_rate.sql`, `crates/fifolio-core/src/ecb.rs`, `crates/fifolio-core/src/storage/rates.rs`, `crates/fifolio-core/tests/rate_cache.rs` and `fixtures/ecb/` (two recorded document fragments plus a README) are untracked, with `lib.rs`, `storage/mod.rs` and `tests/storage.rs` modified. `cargo test --workspace` is green there: 233 tests, 0 failures, against 208 on the last clean tree. It stays `todo` under the convention revisions 5, 7, 9, 13, 15 and 19 applied — uncommitted is unreviewed, and an unreviewed item is the item still to build. The outstanding work is review against ARC-015 and ARC-016, and against the ingest half of ARC-017 and ARC-018 this item scopes itself to, then a commit. Not a rebuild.
What landed in `design/` while that tree sat there is DEC-068 (`d160fee`), the acceptance clause added above; the tree already implements it as insert-and-keep in `storage/rates.rs` rather than an upsert, so the decision documents what was built. Check that a second ingest of an overlapping day is asserted to leave the stored figure alone, and that `ecb.rs` reads the same document shape for both paths rather than two parsers.
`d160fee` cites the new rule as **`[ARC-026]`, an identifier `architecture.md` already uses** for "translation is confined to the CLI" (line 69), which FIF-043 covers. This plan does **not** reassign ARC-026: the top-up rule is carried as an acceptance clause of ARC-018 here, and the collision is a `spec-auditor` finding, alongside the duplicate `[ARC-025]` revision 19 flagged and which is still unfixed. It changes no coverage count and blocks nothing, but until the specification gives the rule an identifier of its own, "every requirement id belongs to exactly one item" is true of the ids and not of the rules.
Done in revision 22: the increment revision 21 found uncommitted was committed as `bd7946b`, and this revision is the review pass over it. Verified against the code: `migrations/0002_fx_rate.sql` keys the table on (currency, rate_date) as a composite primary key, so one publication per currency per day is the table's own rule and a restated day is a conflict the repository resolves rather than a second row [ARC-015, DOM-034]; `ecb.rs` reads both documents with one parser and refuses rather than skips, now including exponent notation, which `Decimal::from_str` would have read as a magnitude two orders out; `storage/rates.rs` inserts and keeps [DEC-068], and `fx::resolve` reads an in-memory snapshot, so nothing on the resolution path opens a socket [ARC-016, TST-019 to TST-021]. The review moved the cache's row counts off `CachedRates::len`, which existed for the tests alone and is not part of the `RateTable` port [ARC-016], onto a count against the table itself, and added the scale boundary from the ECB side — six decimals stored unchanged, `0.000001` included — beside the existing refusal of seven [ARC-007, ARC-010]. ARC-017 and ARC-018 stay open: the fetcher and the two document URLs are FIF-041's, as the acceptance scopes.

## FIF-011 SQLite storage and migrations
Status: done
Requirements: ARC-011, ARC-012, ARC-013, ARC-014, DOM-071, TST-004
Depends on: FIF-005, FIF-056, FIF-059
Acceptance: one SQLite file, default `./fifolio.db`, created on first run and overridable; versioned `sqlx` migrations applied on startup; repositories for every entity, the manual entry included; a unique constraint on security ISIN; integration tests against a real temporary database covering migration from empty and the ISIN uniqueness failure. A repository **refuses** a figure carrying more decimals than its kind's scale allows, with a distinguishable error, rather than rounding it on the way in [ARC-010 as restated by DEC-067]; one integration test per scale asserts the refusal.
Notes: Named next to build in revision 18. Its three dependencies are `done` — FIF-005 (`9846b5a`), FIF-056 (`74e4bd1`) and FIF-059 (`4af8667`, widened by FIF-092 in `cd09f97`) — and none of its six requirements is on a `Blocks:` line. It is not the first `todo` in document order: FIF-010 sits ahead of it and waits on this item for the table it caches into, which is why storage comes first.
It is the first item to leave pure core arithmetic, and the first whose tests are integration-layer against a real temporary database rather than unit tests [TST-004]; `fifolio-test-support` (FIF-002) already provides the temporary-database helper, so the harness is not part of this increment.
What it persists is the types built so far and no more: `Account`, `Security`, `SourceRecord`, `ImportBatch` (FIF-005), the six `Transaction` variants with their `Valued` pairs and `Conversion` (FIF-056, FIF-008), and `ManualEntry` with its five `Supplied` shapes (FIF-059, FIF-092). Fields whose rules are still blocked must not be invented here: a transaction carries no relation to account, security or source record yet — DOM-013 is FIF-076's and is blocked — and no allocation or attribution table is called for by this item's requirements. A migration that adds those columns later is cheaper than a schema that guesses them now.
ARC-010's rounding boundary is FIF-004's `round_to`, and FIF-054 names persistence as one of the two boundaries at which a value must already be at its scale; the repositories are where that is enforced, so a scale violation should be impossible to write rather than merely untested.
Revision 19 found the whole increment written and **uncommitted**: `crates/fifolio-core/migrations/0001_initial.sql`, `crates/fifolio-core/src/storage/` (`mod.rs`, `codec.rs`, `entities.rs`, `manual_entries.rs`, `transactions.rs`) and `crates/fifolio-core/tests/storage.rs` are untracked, with `Cargo.toml`, `lib.rs`, `entities.rs`, `precision.rs` and `tests/temporary_database.rs` modified. The suite is green there: 207 tests, 0 failures, of which `tests/storage.rs` contributes 21 at the integration layer. It stays `todo` under the convention revisions 5, 7, 9, 13 and 15 applied — uncommitted is unreviewed, and an unreviewed item is the item still to build. The outstanding work is review against the six requirements above and a commit, not a rebuild.
The one thing that landed in `design/` while that tree sat there is DEC-067 (`72abea7`), which is the acceptance clause added above; the tree already implements it as `StorageError::UnscaledValue` via `codec::at_scale`, so the decision documents what was built rather than contradicting it. Check that the refusal is asserted per scale and not only for money.
Done in revision 20, commit `e5c2900`, which committed the tree revision 19 found uncommitted. Verified against the code rather than the message: `migrations/0001_initial.sql` is applied on connection from an embedded migrator, `./fifolio.db` is the default and `--database`'s override is a parameter of the open call [ARC-011 to ARC-014]; each transaction variant has a detail table of its own rather than one wide nullable table, so a row of one variant cannot be read as another; `Isin` is the securities primary key and trims and uppercases on construction, a second insert reporting `DuplicateIsin` [DOM-071]; `codec::at_scale` is the only path a decimal takes to a column and refuses an over-scaled figure with `StorageError::UnscaledValue`, asserted once per scale — money 2, unit price 6, FX rate 6, quantity 8 — which is the DEC-067 clause. No transaction-to-account, -security or -record relation was invented, as this item required. Twenty-two integration tests against a real temporary database [TST-004]; the workspace suite is 208 tests, 0 failures, on a clean tree.

## FIF-012 Storage-enforced invariants
Status: todo
Requirements: DOM-066, DOM-068, DOM-069, DOM-072, DOM-094, DOM-110, DOM-119
Depends on: FIF-011
Acceptance: each invariant is refused at the persistence/service boundary with a distinguishable error: a closing may only be attributed if every earlier closing of the same account and security is attributed; an attribution may only be deleted if no later attribution exists for that account and security; a transaction in an attribution cannot be edited, re-rated or deleted; a `transfer_in` emitted by a `transfer_out` cannot be deleted independently of it; a manual entry is never deleted by an import undo; a batch may only be deleted if no transaction derived from it participates in an attribution, and if no record it owns is cited by a transaction the batch did not derive, the refusal naming those transactions. Integration tested against a real temporary database, one test per invariant.
Notes: DOM-049, DOM-067 and DOM-070 moved to FIF-060 in revision 2. Revision 3 moves the two quantity invariants, DOM-064 and DOM-065, to FIF-078; what remains here is the lifecycle set — ordering of attribution, immutability, deletion refusals — which is decided and needs no effective-quantity arithmetic, so this item no longer depends on FIF-061.
Named next to build in revision 23. FIF-011 is `done` (`e5c2900`), none of the seven requirements is on a `Blocks:` line, it is the first `todo` in document order, and the working tree is clean, so this item starts from a committed base.
It is the first item to need a schema FIF-011 deliberately did not write. FIF-011 persisted the entities and the six transaction variants and **no relations**, so this item must add what its own invariants read and no more: an attribution keyed on account and security with its allocations, the transaction's relation to account and security, and a record's owning batch. The emitted-`transfer_in`-to-`transfer_out` link DOM-094 refuses to break is a relation between two transactions and is within scope; the wider "relation to >= 1 source records" of DOM-013 is **blocked** (OQ-002) and belongs to FIF-076, so nothing here may be read as closing it. Where an invariant needs a record relation it does not yet have — DOM-119's citation check — persist the relation the invariant needs and leave the ordering question alone.
What this item owns is refusal, not computation. Allocation *figures* are FIF-014's and the attribution *service* is FIF-015's, both `todo`; the rows this item stores are the ones the invariants are stated over, and their derivation stays where it is. The quantity invariants that would need effective quantity are FIF-078's and blocked.
Each refusal needs a distinguishable error in the FIF-011 style — `StorageError::UnscaledValue` is the precedent — and one integration test per invariant against a real temporary database [TST-004], using `fifolio-test-support`'s helper. Seven requirements, so seven refusals: DOM-066 order of attribution, DOM-068 deletion of an attribution, DOM-069 immutability of an attributed transaction, DOM-094 the emitted `transfer_in`, DOM-110 an import undo sparing a manual entry, DOM-072 and DOM-119 the two batch-deletion refusals, the second naming the transactions that block it.
DOM-110 is stated here while the undo behavior it protects is FIF-062's; assert it as the storage-level refusal, not by running an undo.
Revision 24 re-confirmed this item as next and confirmed it was **not started**: revision 23 named it, the session then stopped, and the tree carries no untracked source file. The base is `6db99f1`, suite green at 237 tests. Nothing about the scope above changed.

## FIF-078 Allocation quantity invariants
Status: blocked
Requirements: DOM-064, DOM-065
Depends on: FIF-012, FIF-061
Acceptance: allocated quantities against an opening, each scaled to a common position in the canonical order, never exceed its effective quantity at that position, and a closing's allocations sum exactly to its quantity; both refused at the persistence/service boundary with a distinguishable error, integration tested against a real temporary database.
Blocked by: DOM-064 and DOM-065 are on the undecided list.
Notes: Split out of FIF-012 in revision 3. DOM-064 compares effective quantities at a common position, which is why this half, and not FIF-012, carries the FIF-061 dependency.
The boundary DOM-065's refusal must cover is the **empty** allocation set: FIF-012 stores `approve(closing, &[])` as an attribution with no allocation rows, and that closing then counts as attributed and unblocks every later closing under DOM-066 while consuming nothing. The behavior is pinned by a test of FIF-012's, so refusing it here is a change to that test rather than a silent one.

## FIF-060 Pending-record block and single consumption
Status: todo
Requirements: DOM-049, DOM-067, DOM-070
Depends on: FIF-012, FIF-058
Acceptance: a security with any pending source record is blocked from attribution **in the account that record belongs to** and not elsewhere; a source record is consumed by at most one transaction though it may be cited by several.
Notes: Split out of FIF-012 and FIF-015 in revision 2 and blocked then; all three ids left the undecided list in revision 3. DOM-049 and DOM-067 state the same block from two sides, which is why they are one item. Still gated on FIF-058 (DOM-101), which defines consumption.

## FIF-013 FIFO proposal engine
Status: todo
Requirements: DOM-056, DOM-057
Depends on: FIF-076, FIF-056, FIF-061
Acceptance: given an account, a security and a closing transaction, the engine consumes the oldest openings with unattributed effective quantity remaining, in canonical order, until the closed quantity is covered, splitting the final opening; if the available unattributed quantity is short, it returns a shortfall naming the missing quantity instead of a proposal. Pure function over transactions and existing allocations, unit tested with no database.
Notes: Revision 3 splits the expiration rule (DOM-092, DOM-114) into FIF-079, and moves the ordering dependency from FIF-006 to FIF-076, which is where canonical order now lives.

## FIF-079 Expiration quantity
Status: blocked
Requirements: DOM-092, DOM-114
Depends on: FIF-013
Acceptance: an `expiration`'s quantity is the unattributed remainder rather than a stated figure, and a row that would expire a position with nothing remaining stays pending rather than being derived, so no division by a zero quantity can arise.
Blocked by: DOM-092 and DOM-114 are on the undecided list.
Notes: Split out of FIF-013 in revision 3. IMP-SAXO-013's `Expiratie` mapping (FIF-023) states the same pending rule from the importer side and is blocked too.

## FIF-061 Splits and effective quantity
Status: blocked
Requirements: DOM-089, DOM-103, DOM-113
Depends on: FIF-076, FIF-054
Acceptance: a `split` carries an integer numerator and denominator; an opening's effective quantity **as of a position in the canonical order** is its stated quantity times the ratios of every split for that security falling between the opening and that position, computed as an exact rational and rounded only for display; its effective unit price at that position is its total cost over that effective quantity; stated figures are never rewritten; successive splits compose with no accumulated residue, so 1-for-3 then 3-for-1 returns the original quantity exactly.
Blocked by: DOM-113 is on the undecided list, and it is the substance of the item: the rational representation is what the rest of the rule is built on. ARC-009 (FIF-054) bears on it too.
Notes: New in this revision, replacing the "quantity adjustment" half of the retired FIF-018. DOM-103 is the reason effective quantity is parameterized by position at all: measuring as of today halves the cost of everything sold before a split.

## FIF-014 Allocation figure derivation and the drift rule
Status: todo
Requirements: DOM-058, DOM-059, DOM-060, DOM-061, DOM-062, DOM-063, DOM-084, DOM-093, DOM-105, DOM-125
Depends on: FIF-013, FIF-061, FIF-004, FIF-054
Acceptance: allocated cost, buy fee, proceeds, sell fee and gain computed on demand from the parent transactions, exactly as the formulas in `domain.md` state, with the opening side divided by its effective quantity **as of the closing** and the closing side by the closing's own quantity; every closing variant carries `eur_gross` and `eur_fees` so one formula reads them all, and `split` carries neither; each share rounded to 2 decimals independently with drift absorbed by the last share, opening-side last being the allocation that exhausts the parcel and closing-side last being the last allocation of that closing in canonical order; sell fees never spread beyond their own closing; **a gain is arithmetic on the four rounded shares**, not computed exactly and rounded afterwards, so every allocation reconciles to its own columns. Unit tests include a division that is exact, one leaving one cent, and one leaving many, and one where rounding-then-subtracting and subtracting-then-rounding differ, asserting the former.
Notes: Blocked in revision 2 on DOM-059, DOM-105 and DOM-112; the first two are now decided. DOM-112 (a `transfer_out`'s `eur_gross` derived from its own allocations) moved to FIF-080 in revision 3, so this item covers the cash closings only.
DOM-084 moved here from FIF-008 in revision 15: allocation shares derive from the native/EUR pair the same way and are never stored independently, which is only assertable once allocations exist.
DOM-125 is new in revision 14 (DEC-066). It belongs here, with the rounding rule it qualifies, and not with the reports: its second half — a total is the sum of the rounded rows — is a consequence the report items FIF-030 and FIF-031 inherit by summing what this item produces, and neither is built yet. It costs up to two cents against the exact figure per allocation, deliberately; say so in the code comment, or someone will "fix" it.

## FIF-080 Transfer out has no proceeds and no gain
Status: blocked
Requirements: DOM-112
Depends on: FIF-014, FIF-063
Acceptance: a `transfer_out`'s `eur_gross` is the basis it carries onward rather than proceeds — derived from its own allocations, not stored — so the opening side is computed first and no gain is ever computed for it.
Blocked by: DOM-112 is on the undecided list, as are the transfer rules it rests on (FIF-063). Three questions reach it as of revision 8: OQ-003, OQ-011 and the new OQ-016.
Notes: Split out of FIF-014 in revision 3 so the cash closings' formulas can be built.

## FIF-015 Attribution service
Status: todo
Requirements: DOM-018, DOM-019, DOM-020, DOM-054, DOM-055
Depends on: FIF-014, FIF-012
Acceptance: approve-or-decline only, no partial edit; creating an attribution validates same account and same security, every allocated opening preceding the closing in canonical order, and quantities summing exactly to the closing quantity; declining writes nothing and leaves the closing blocking later closings by construction.
Notes: DOM-049 (the pending-record refusal, previously here) moved to FIF-060.

## FIF-063 Transfer out: emission, basis and decomposition
Status: blocked
Requirements: DOM-090, DOM-091, DOM-106, DOM-107, DOM-115, DOM-116
Depends on: FIF-015, FIF-061
Acceptance: approving a `transfer_out` emits **one `transfer_in` per consumed parcel**, each carrying that parcel's acquisition date, date provenance `inherited`, and **its own allocated cost** rather than a share of a pooled total; each emitted quantity is the consumed quantity times the transfer's ratio at the quantity scale, the last record absorbing the remainder so the emitted quantities sum exactly; the transfer's fees are added to the carried basis in proportion to the basis each record carries, the last absorbing that remainder too; an event paying cash for part of a holding and exchanging the rest decomposes into a `sell` and a `transfer_out` citing the same source records, with the group's summed cash net of any reversal and **all** of its costs on the sell leg and no money of its own on the transfer leg.
Blocked by: DOM-090, DOM-091, DOM-106 and DOM-107 are on the undecided list. DOM-106 is now named by OQ-016 as well as OQ-003: the transfer's *own* fee and the *allocated* buy fee are two separate open questions about the same field. DOM-115 and DOM-116 left it in revision 3 but are not separable: DOM-115 rounds the quantities of the records DOM-090 emits, and DOM-116 divides the money of the decomposition DOM-091 defines. Neither can be reviewed against the specification while the rule it qualifies is open, so the item stays blocked whole rather than being split into an unreviewable half.
Notes: New in revision 2, replacing the "lot transfer" half of the retired FIF-018. DEC-046 is explicit that pooling would give two equal parcels bought at 100 and 200 the same unit cost of 150 — a total that is right with every individual figure wrong — so the per-parcel rule is the point of the item, not a detail of it.

## FIF-016 Property tests for the engine
Status: todo
Requirements: TST-010
Depends on: FIF-014, FIF-061, FIF-063, FIF-062
Acceptance: `proptest` suites asserting every property in `testing.md`: no opening over-consumed; allocations sum exactly; shares sum exactly to the parent for every division and remainder; a buy's fees fully distributed at its last unit sold and not before; a split leaves total cost unchanged while scaling effective quantity, and effective quantity before any split equals the stated quantity; successive inverse splits compose exactly; each record a transfer emits carries its own parcel's cost, never a pooled average; a transfer preserves total basis and parcel count; attributing a sequence of closings in canonical order never over-consumes an opening; order computed from a file is identical however often it is imported and independent of import history; an undo followed by a re-import restores exactly the transactions that existed before.
Notes: Blocked in revision 2; TST-010 left the undecided list in revision 3. Its dependencies FIF-061 and FIF-063 are still blocked, so it is not yet startable.

## FIF-017 Import and derivation framework
Status: todo
Requirements: DOM-002, DOM-042, DOM-044, DOM-045, DOM-046, DOM-048, DOM-120, DOM-124, ARC-023
Depends on: FIF-007, FIF-011
Acceptance: an importer trait over a source file that yields source records; rows where every field the variant needs is present and unambiguous are derived automatically; rows that affect holdings but lack something only the user knows become pending, which is the completion queue; **cash** dividends, interest, deposits, withdrawals and account fees are recognized as non-position, counted and not stored, while a dividend that issues shares is a position event whose rows are stored and whose buy carries a stock-dividend origin; everything the user supplies becomes a manual entry. XLSX reading via `calamine` and CSV via `csv` sit behind the same reader abstraction; a delimited row stores its verbatim line while a spreadsheet row, having none, stores the canonical rendering `domain.md` defines — each cell as the file holds it, an Excel serial date staying `45208`, keyed by column name in sheet column order — so that re-reading the same file reproduces the same string.
Notes: DOM-120 is new in revision 7 (DEC-057) and sits here because the reader abstraction is what constructs the stored raw content; FIF-005, which owns the `SourceRecord` type, is `done` and its field is untyped as to how it was rendered. DOM-043, the classification taxonomy these three outcomes belong to, moved to FIF-064 in revision 2. Revision 3 moves DOM-047, "nothing is invented", to FIF-081, it being undecided; the three outcomes stand without it.
DOM-124 is new in revision 14 (DEC-063) and sits here rather than in FIF-025 because it is the exception clause on DOM-002 and DOM-046, which this item owns; it is format-agnostic, and the Saxo heuristic that recognizes such a dividend is FIF-025's, which already derives the buy. Nothing completed is invalidated: the classification is not built yet.

## FIF-081 Nothing is created from nothing
Status: blocked
Requirements: DOM-047
Depends on: FIF-017
Acceptance: there is no path, in core or at any surface, that creates a transaction other than from source records; the absence is structural rather than a check, and a test asserts it for every construction path.
Blocked by: DOM-047 is on the undecided list, as is its server-side counterpart SRV-034 (FIF-087).
Notes: Split out of FIF-017 in revision 3.

## FIF-064 Import classification taxonomy
Status: todo
Requirements: DOM-043
Depends on: FIF-017
Acceptance: the classification a source record receives at import is a closed, stated set, and every importer maps into it exhaustively.
Notes: Split out of FIF-017 in revision 2 and blocked then; DOM-043 left the undecided list in revision 3.

## FIF-062 Manual entry lifecycle across undo and re-import
Status: todo
Requirements: DOM-108, DOM-109
Depends on: FIF-059, FIF-017
Acceptance: undoing an import removes its source records and the transactions derived from them and leaves every manual entry standing; re-importing the same rows reconnects each entry by the record identities it names and restores its transaction automatically, so an undo followed by a re-import returns the account exactly where it was; an entry whose records are absent is listed as waiting, naming what it expects.
Notes: New in this revision (DEC-038). This is the behavior the separate manual-entry entity exists for, and TST-010's last property asserts it.

## FIF-065 Import guard: one calendar year per file
Status: todo
Requirements: IMP-001, IMP-002
Depends on: FIF-017
Acceptance: a file whose rows carry trade dates in more than one calendar year is refused, on the trade date rather than a booking timestamp or the filename; the refusal names the years met. Tested against the Saxo and Trade Republic fixtures, each of which is confined to one year, plus a synthetic file spanning two.
Notes: New in revision 2 (DEC-052) and blocked on IMP-003, which left the undecided list in revision 3. Revision 8 splits IMP-003, the account guard, into FIF-089: OQ-015 is new and blocks it. The year guard reads only trade dates and is decided, so it stays here and stays buildable. Renamed accordingly. The server-side half is SRV-051 in FIF-070.

## FIF-089 Import guard: one account per file
Status: blocked
Requirements: IMP-003
Depends on: FIF-017, FIF-065
Acceptance: an import whose file names a different account than the target is refused naming both, and a file carrying rows from more than one account is refused outright.
Blocked by: IMP-003 is on the undecided list (OQ-015).
Notes: Split out of FIF-065 in revision 8. OQ-015 observes that the Trade Republic export carries no account identifier at all, so the check has nothing to read there; whether the requirement admits uncheckable formats or Trade Republic files are matched some other way decides the shape of this item, not merely a detail of it. Its server-side counterpart is SRV-056 in FIF-090, blocked by the same question.

## FIF-019 Saxo: file reading and header normalization
Status: todo
Requirements: IMP-SAXO-001, IMP-SAXO-002, IMP-SAXO-003, IMP-SAXO-004, TST-030
Depends on: FIF-017, FIF-003
Acceptance: reads the single sheet with its one header row and 31 columns; matches headers after whitespace normalization, so `Bk\xa0Record\xa0Id`, `Booking\xa0Id` and ` Positie-ID` resolve; converts Excel serial numbers to dates; rejects a non-Dutch header set with a clear error rather than mis-mapping; a blank cell is accepted in both the shapes it arrives in — a zero-length shared string as Saxo writes it and an empty cell as the fixture writer produces it — and a test says so. Tested against the Saxo fixture.
Notes: Revision 3 splits IMP-SAXO-025, the newest-first direction rule, into FIF-083. TST-030 is new in revision 7: it is a known divergence of the fixtures (FIF-003) but its assertion is on this reader, which is why it is owned here.

## FIF-083 Saxo: newest-first row direction
Status: blocked
Requirements: IMP-SAXO-025
Depends on: FIF-019, FIF-006
Acceptance: the reader recognizes that Saxo emits rows newest first and normalizes the file's direction before row positions are used for ordering, so file position as a tie-breaker means oldest-first position.
Blocked by: IMP-SAXO-025 is on the undecided list.
Notes: Split out of FIF-019 in revision 3. FIF-066's third ordering key ("file position taken in reverse") states the same thing from the ordering side and is only correct once this is settled.

## FIF-066 Saxo: ordering columns
Status: blocked
Requirements: IMP-SAXO-026, IMP-SAXO-027, IMP-SAXO-036
Depends on: FIF-019, FIF-006
Acceptance: rows are ordered on `Transactiedatum`, then on `Bk Record Id`, then `Booking Id`, then `Transactie-ID`, **each compared only against itself**, then file position taken in reverse; two rows populating different counters share no comparable value and fall through to file position; `Corporate action-Id` is never an ordering column, because it is not monotonic with date. Tested on a fixture date carrying several rows where only that id is populated, so file position settles them, and on one of the ten fixture dates that carry a mixture of counters, asserting the order does not depend on which column a row happens to populate.
Blocked by: IMP-SAXO-026 is on the undecided list (OQ-013).
Notes: New in revision 2; ordering is now computed from file content (DOM-040). Newly blocked in
revision 5: OQ-013 leaves undetermined where a row carrying none of the three booking ids sorts,
and those rows are precisely Saxo's corporate actions. IMP-SAXO-027 (`Corporate action-Id` is never
an ordering column) is decided, but it is one clause of the same ordering key and was not split out;
the mechanism it binds to, `ordering.rs` from FIF-006, is unaffected and already built.
Restated in revision 14 by DEC-064, which adds IMP-SAXO-036: the three booking counters are three
ordering columns, not one folded key, because their magnitudes are unrelated and ten fixture dates
carry a mixture, which same-day order then settles arbitrarily — and same-day order decides which
parcel a same-day sell consumes. The acceptance above is rewritten accordingly; the item was
`blocked` and unbuilt, so nothing is invalidated. IMP-SAXO-036 is not separately decidable: its last
clause hands absent counters back to OQ-013, which is what blocks this item.

## FIF-020 Saxo: account normalization and row identity
Status: todo
Requirements: DOM-003, IMP-SAXO-005, IMP-SAXO-006, IMP-SAXO-007, IMP-SAXO-024
Depends on: FIF-019, FIF-007
Acceptance: `Rekening-ID` currency suffix stripped, so `.../1000000EUR|USD|CAD` collapse onto one account; `Klant-id` never used as the account; identity is the first populated of `Transactie-ID`, `Bk Record Id`, `Booking Id`, `Corporate action-Id`; if two rows still produce one identity the file is rejected rather than deduplicated. Re-importing the fixture twice yields no new records.
Notes: The collision rejection (IMP-SAXO-024) was new in revision 2. Revision 3 splits the `Corporate action-Id` composite identity (IMP-SAXO-008) into FIF-084, it being undecided; until that lands, the corporate-action fallback is the bare id, so the collision refusal will fire on the TransAlta shape rather than distinguishing its rows. That is the safe failure and the refusal test covers it.

## FIF-084 Saxo: corporate action row identity
Status: blocked
Requirements: IMP-SAXO-008
Depends on: FIF-020
Acceptance: when identity falls through to `Corporate action-Id`, that id plus `Acties` plus `Boekingsbedrag` plus **the row's ordinal within its `Corporate action-Id` group in file order** form the identity, so the three TransAlta rows stay distinct across re-imports.
Blocked by: IMP-SAXO-008 is on the undecided list.
Notes: Split out of FIF-020 in revision 3. Without the ordinal, two rows sharing a label and an amount deduplicate as a re-import and money vanishes from the event; DEC-033 records why the ordinal and not a hash of the row.

## FIF-021 Saxo: money derivation
Status: todo
Requirements: IMP-SAXO-009, IMP-SAXO-010, IMP-SAXO-023, IMP-SAXO-029, IMP-SAXO-030, IMP-SAXO-032
Depends on: FIF-019, FIF-008
Acceptance: `Boekingsbedrag` read as native cash movement including costs, `Aantal` as the same amount in EUR and never as a quantity, `Totale kosten` as EUR costs (always negative), `Omrekeningskoers` as the native-to-EUR multiplier whose **reciprocal** is the stored rate; the derivation is sign-aware — EUR gross = |Aantal| − |Totale kosten| on a buy and |Aantal| + |Totale kosten| on a disposal; EUR fees = |Totale kosten|; EUR price = EUR gross / (quantity × factor), so the quoted price is reproduced and the factor is applied once; native figures follow the same shape with costs converted back using `Omrekeningskoers`, not the stored rate, **the subtraction keeping full precision like every other intermediate**, so the sample buy stores a native unit price of 5.750036 against a printed 5.75; rate source `broker`. Unit tests reproduce all three worked examples in `importers.md`: the 40 @ 5.75 USD buy (EUR gross 209.63, EUR price 5.240750), the 60 @ 30.65 sell (EUR gross 1839.24, EUR price 30.654) and the 3000 @ 139.46 bond (EUR price 139.46, not 1.3946).
Notes: The sign-aware derivation (DEC-035) was new in revision 2; revision 1's single formula understated every disposal's proceeds by twice the fee. Blocked in revision 2 on IMP-SAXO-030, which left the undecided list in revision 3. The factor in the EUR-price step is FIF-075 and still undecided, so the bond case in the third worked example lands with that item.
IMP-SAXO-032 is **reversed in revision 8**. Revision 7 read it (DEC-054) as an exception to the intermediate-precision policy of FIF-054, rounding the native fee so the stored price matched the printed one; DEC-059 supersedes DEC-054 and settles it the other way, so IMP-SAXO-032 is now an instance of that policy rather than an exception to it, and the plan's earlier acceptance clause is gone rather than edited into silence. Nothing was built against the old reading — this item has never been started — so no completed work is invalidated. It changes no EUR figure and therefore no tax figure either way.
DOM-039 was restated in the same commit: reconciliation against a broker document is on the **booked amounts**, which are exact, and not on the printed unit price. FIF-055 records that the observable assertion for DOM-039 lands here; that assertion is now "the derived price reproduces the booked gross", not "the stored price equals the printed one", and the sell case (30.654 against a printed 30.65) already in this item's acceptance is exactly it.

## FIF-022 Saxo: `Acties` label parsing
Status: todo
Requirements: IMP-SAXO-011, IMP-SAXO-012
Depends on: FIF-019
Acceptance: quantity and direction parsed out of the free-text label (`Koop 40 @ 5.75 USD`, `Verkoop -60 @ 30.65 EUR`, `Deponering 300 @ 51.40 EUR`); the label price is exposed only as a 2-decimal display value and is structurally unable to reach any money field, with `Deponering` (FIF-024) the one sanctioned exception; a test asserts the sell case where 30.65 in the label differs from 30.654 derived from the columns; an unparsable label is a parse failure, not a guess.
Notes: Blocked in revision 2 on IMP-SAXO-012, which left the undecided list in revision 3.

## FIF-088 Saxo: reversal rows and group summation
Status: todo
Requirements: IMP-SAXO-033, IMP-SAXO-034, IMP-SAXO-035
Depends on: FIF-022, FIF-021
Acceptance: `Terugboeking` is matched as a **suffix** and never as a bare `Acties` value, so `Terugkoopaanbod - Terugboeking` and `Dividend - Terugboeking` classify as their prefix, reversing; summing a `Corporate action-Id` group, a reversal's cash and its costs both **subtract**, reproducing the DeVolksbank tender: 3946.14 paid, −1998.07 reversed, 1948.07 net; summation is over the **EUR** figures the rows carry and never over the native ones, because a group's rows may hold different `Omrekeningskoers` values. Unit tested over the reversal rows of the Saxo fixture for the classification, and over synthetic figures for the summation, the fixture's amounts being perturbed.
Notes: New in revision 7 (DEC-056). Cut as its own item rather than folded into FIF-023, which is blocked, so that a decided rule is not frozen behind an undecided one; the group summation it provides is what FIF-067 (cash merger, tender and buyback decomposition) and FIF-025 (dividend grouping) each call, and both are gated on FIF-023 anyway. That reversal costs subtract is **chosen, not observed**: both reversal rows in five years of exports carry zero in `Totale kosten`. The code comment must say so and name the check to perform if a costed reversal ever arrives, because nothing in the tests can catch it being wrong.

## FIF-023 Saxo: row classification
Status: blocked
Requirements: IMP-SAXO-013
Depends on: FIF-022, FIF-017, FIF-056
Acceptance: `Transactietype` plus `Acties` map to exactly the table in `importers.md`, onto the transaction variants: `Koop` → `buy`, `Verkoop` → `sell`, both automatic; `Deponering` → `transfer_in`, automatic (FIF-024); `Expiratie` → `expiration`, automatic, pending if nothing remains; `Fusie`, `Terugkoopaanbod` (with `Terugboeking`) → pending `sell` and/or `transfer_out` (FIF-067); `Stock split` → pending `split`; `Omwisseling` → pending `transfer_out`; dividend labels → FIF-025; `Rente`, `Service fee`, `ADR-kosten`, `Storting`, `Opname` → recognized as non-position, not stored. Table-driven test over the fixture, one case per label; an unknown label fails to parse rather than being silently dropped.
Blocked by: IMP-SAXO-013 is on the undecided list, and it is the whole of the item — the classification table is its single requirement, so there is nothing to split off. Everything downstream of it (FIF-024, FIF-025, FIF-067) waits.
Notes: Newly blocked in revision 3; it was `todo` in revision 2. **Three** open questions now reach it: OQ-005 (when an expiration of nothing is detected), OQ-008 (an `Acties` value outside the table, with a live `Overige Corporate Action` instance) and, new in revision 8, OQ-014 — where a `Herbeleggingsdividend`'s share count and price come from, the export carrying neither. All three must close. OQ-014 is the one with scope beyond this item: 19 events in the sample against 12 for every other corporate action combined, so its answer decides how much of the tool's use is data entry, and it bears on FIF-025 and FIF-057.

## FIF-024 Saxo: `Deponering` transfers in
Status: todo
Requirements: IMP-SAXO-014, IMP-SAXO-015, IMP-SAXO-016, IMP-SAXO-017, IMP-SAXO-028, DOM-080
Depends on: FIF-023, FIF-009
Acceptance: a `Deponering` row (zero `Boekingsbedrag` and `Aantal`) produces a complete `transfer_in` with source `broker`, quantity and price from the label taken at face value as a split-restated acquisition cost, acquisition date set to the transfer date and date provenance `transfer_date`; this is the one place the label's price is authoritative, and the code says so and says why; the date is **never** corrected afterwards, so grandfathered Altbestand status is not represented; because `Omrekeningskoers` is 1 even for foreign-currency instruments, a non-EUR `Deponering` resolves its EUR cost basis by ECB lookup on the transfer date with source `ecb`; a later split row applies on top with no double counting.
Notes: Changed in this revision: DEC-040 reverses DEC-011's editable acquisition date. The date is now fixed, and the plan no longer carries an edit path for it (SRV-054, in FIF-038).

## FIF-025 Saxo: dividend grouping by `Positie-ID`
Status: todo
Requirements: IMP-SAXO-018, IMP-SAXO-019
Depends on: FIF-023, FIF-057
Acceptance: dividend-type rows (`Dividend`, `Keuzedividend`, `Herbeleggingsdividend`) are grouped by `Corporate action-Id` *before* evaluation; if any row in a group carries a `Positie-ID`, the whole group becomes one pending entry requiring an explicit stock-or-cash decision with stock pre-selected, and choosing stock derives a `buy` with origin `stock_dividend`; otherwise the group is recognized as non-position and not stored. A regression test reproduces the two-row Philips shape — position-marked row carrying zero, cash row carrying the money with no marker — and asserts that row-by-row evaluation would drop the money while grouping does not.
Notes: Calibrated heuristic; its failure mode is a blocked attribution with a named shortfall, not a wrong figure. Keep that reasoning in the code comment. Blocked in revision 2 on IMP-SAXO-018, which left the undecided list in revision 3; the cost basis of the derived buy (FIF-057) is decided too. Still gated on FIF-023.

## FIF-026 Saxo: security mapping and bond quotation
Status: todo
Requirements: IMP-SAXO-020, IMP-SAXO-021, IMP-SAXO-022
Depends on: FIF-019, FIF-005
Acceptance: `Instrument ISIN`, `Instrument` and `Type` map onto the security by the explicit table — `Stock` → `stock`, `Bond` → `bond` with quotation `percent_of_par`, `Etf` → `etf`, `MutualFund` → `fund`, `Cash` → no security and a non-position row, anything else rejects the import; ISIN is the key, so a changed or delisting-annotated name (`*Delisted 20231011 (...)`) resolves to the same security and does not create a second one. Test asserts a 3000-nominal bond at 139.46 costs 4183.80.

## FIF-067 Saxo: cash merger, tender and buyback decomposition
Status: todo
Requirements: IMP-SAXO-031
Depends on: FIF-023, FIF-063
Acceptance: a `Fusie` or `Terugkoopaanbod` group, reversals included, becomes a pending entry asking for the quantity disposed and any target security, and derives a `sell` and/or a `transfer_out` with the cash and all of the costs on the sell leg.
Notes: Split out of FIF-023 in revision 2; IMP-SAXO-031 left the undecided list in revision 3. Still gated on FIF-063, whose transfer rules it derives against, and on FIF-023.

## FIF-027 Trade Republic: CSV reading, identity, ordering and sign convention
Status: todo
Requirements: IMP-TR-001, IMP-TR-002, IMP-TR-003, IMP-TR-004, IMP-TR-015, IMP-TR-022, IMP-TR-023
Depends on: FIF-017, FIF-003, FIF-007, FIF-006
Acceptance: quoted comma-delimited CSV with dot decimals and one header row of 23 columns; `date` is the effective date and becomes the trade date, `datetime` is a booking timestamp used only as an ordering column after the date and never described as an execution time; the two are independent fields that diverge by up to six days on a corporate action; rows are ordered on `date`, then `datetime`, then file position, because the 2025 export is sorted by neither; `shares` is the quantity and `price` the unit price; `transaction_id` UUID used directly as the identity; cash-flow signs interpreted so buys and fees are negative. Re-importing the fixture twice yields no new records.
Notes: Blocked in revision 2 on IMP-TR-001, which left the undecided list in revision 3. IMP-TR-023 was restated in revision 7 and is sharper than the plan's earlier reading: the 2022 to 2024 exports ascend by both columns, while 2025 ascends by `date` and **descends** by `datetime` within a date. The requirement id and the item are unchanged; the fixture must carry the 2025 shape, which FIF-003 already states.

## FIF-028 Trade Republic: money mapping
Status: todo
Requirements: IMP-TR-005, IMP-TR-006, IMP-TR-007, IMP-TR-016, IMP-TR-017
Depends on: FIF-027, FIF-008
Acceptance: `price`, `amount`, `fee`, `tax`, `currency` map to the native figures; **`amount` excludes the fee**, the opposite of Saxo, so gross = |amount| and fees = |fee| + |tax| in the settlement currency, checked against the sample's 35 × 75.09 = 2628.15 with a fee of −1.00 carried separately; `fx_rate` is already foreign units per EUR and needs no inversion; a `TRADING` row with `original_*` populated **rejects the import**, since no foreign-currency trade has been observed and the mapping is unspecified.
Notes: IMP-TR-016 and IMP-TR-017 are new in this revision and reverse the previous plan's assumption that a populated `original_*` triple values the trade.

## FIF-029 Trade Republic: row classification
Status: todo
Requirements: IMP-TR-008, IMP-TR-009, IMP-TR-010, IMP-TR-011, IMP-TR-012, IMP-TR-013, IMP-TR-014
Depends on: FIF-028, FIF-056
Acceptance: `TRADING`/`BUY` and `TRADING`/`SELL` derive automatically; `CORPORATE_ACTION`/`TAX_EXCHANGE` is handled by FIF-068 and `CORPORATE_ACTION`/anything else rejects the import naming the type, the row and the transaction id; `CASH`/`DIVIDEND`, `INTEREST_PAYMENT`, `CUSTOMER_INBOUND`, `TRANSFER_INBOUND`, `STOCKPERK` are recognized as non-position and not stored, the `STOCKPERK` credit specifically without any check that its paired `TRADING`/`BUY` exists; any other type **naming a security** rejects the import, and any other type naming no security is not stored but is counted and named in the summary; nothing is inferred from quantity signs for an unrecognized type; the Saxo `Positie-ID` heuristic is not applied here.
Notes: Previously blocked on decision D1 — which `CORPORATE_ACTION` types besides `TAX_EXCHANGE` map to a lot transfer. DEC-019 resolved it: none do, and an unrecognized type rejects the file. D1 is closed.

## FIF-068 Trade Republic: `TAX_EXCHANGE`
Status: todo
Requirements: IMP-TR-018, IMP-TR-019
Depends on: FIF-029, FIF-063
Acceptance: the importer derives **only** the `transfer_out`, from the negative-quantity row; the positive row supplies the target security and the ratio and is cited, not turned into a transaction; the matching `transfer_in` records are emitted on approval, one per parcel consumed; the pairing key is same account, same effective `date`, same absolute quantity, opposite signs, with the ratio the **exact integer pair** `|target| : |source|` reduced by its greatest common divisor and never divided out, since a one-for-three ratio has no finite decimal expansion and a rounded one leaves a residue that grows across applications; an unpaired row, or more than two sharing a key, rejects the import.
Notes: Split out of FIF-029 in revision 2 and blocked then; both ids left the undecided list in revision 3. Still gated on FIF-063. DEC-032 records that tax-neutral treatment is inference from the absence of any monetary figure, to be settled against the user's TR tax report; that reasoning belongs in the code comment. IMP-TR-019 was restated in revision 7 (DEC-055): the ratio is an integer pair, the same representation a `split` carries in FIF-061, so the two must agree.

## FIF-069 Trade Republic: security type mapping
Status: blocked
Requirements: IMP-TR-020, IMP-TR-021
Depends on: FIF-027, FIF-005
Acceptance: `asset_class` `STOCK` → `stock`, `FUND` → `fund`, anything else rejects the import; ETFs arrive as `FUND` and are recorded as funds, flagged auto-created for review; a row whose instrument would map to `bond` rejects the import naming the ISIN, because no TR bond has been observed and the percent-of-par convention cannot be assumed to carry over.
Blocked by: IMP-TR-020 and IMP-TR-021 are on the undecided list.
Notes: New in this revision.

## FIF-030 Income tax overview report
Status: todo
Requirements: DOM-001, DOM-073, DOM-074, DOM-075, DOM-095, DOM-117, DOM-121
Depends on: FIF-015
Acceptance: gain/loss aggregated per year (the year of the disposal) and per account, with columns year, account, proceeds, sell fees, cost, buy fees, gain/loss and **a count of outstanding disposals**, the count being a column rather than a second block so every output format keeps one record shape, and a non-zero count saying in plain words that the year is incomplete wherever the format has room for a sentence; optional account filter and optional tax year filter, unfiltered meaning all accounts and all years; only attributed disposals contribute, and an unattributed one is reported as outstanding rather than counted at zero; a `transfer_out` realizes nothing and contributes no row; the figures are raw gain/loss with no tax-law treatment applied.
Notes: Blocked in revision 2 on DOM-095 and DOM-117, both of which left the undecided list in revision 3. DOM-121 is new in revision 7 (DEC-058); the count column is what keeps DOM-117 from being a silent understatement, so the two are one item. FIF-046's `insta` snapshots and FIF-040's endpoint both carry the extra column.

## FIF-031 Acquisition report
Status: todo
Requirements: DOM-076, DOM-077, DOM-079, DOM-096, DOM-118
Depends on: FIF-015, FIF-061
Acceptance: every opening transaction listed with date, account, security, effective quantity, remaining quantity, effective unit price, fees and realized gain/loss, the two quantity columns both **as of today** so they share one scale and compare to a broker statement; a `transfer_in` appears as an opening in its own right and names the opening it inherited from; the year filter selects openings having at least one allocation in that year; the account filter applies as in FIF-030.
Notes: Renamed from "Buy report" in revision 2; the report covers both opening variants and the CLI token is `acquisitions` (DEC-034). Blocked in revision 2 on DOM-076 and DOM-078; DOM-076 is now decided and DOM-078, the per-disposal detail lines, moved to FIF-082 in revision 3. What is left is the opening-level table.

## FIF-082 Acquisition report: per-disposal detail lines
Status: blocked
Requirements: DOM-078
Depends on: FIF-031, FIF-014
Acceptance: beneath each opening, every attributed disposal is listed with its date, quantity consumed, allocated proceeds, allocated sell fee, allocated cost, allocated buy fee and gain/loss.
Blocked by: DOM-078 is on the undecided list.
Notes: Split out of FIF-031 in revision 3.

## FIF-032 Server binary, arguments and OpenAPI
Status: todo
Requirements: SRV-001, SRV-002, SRV-003, SRV-004, SRV-005, SRV-006, ARC-003, ARC-022
Depends on: FIF-011
Acceptance: `fifolio-server` replaces the stub, binds `127.0.0.1:8000` and nothing else, accepts `--port` and `--database` (default `./fifolio.db`); `fifolio-server openapi` prints the spec and exits without binding; `GET /openapi.json` returns the same spec; the server process is the only one that opens the database.

## FIF-033 Problem+json errors and the HTTP test harness
Status: todo
Requirements: ARC-020, ARC-021, TST-005
Depends on: FIF-032
Acceptance: every error response is `application/problem+json` per RFC 9457 with a stable machine-readable `type` per error class, and each core error variant maps to exactly one `type`; an integration harness spins the router over a temporary database in process and asserts status codes and body shape. Later server items add their own cases to this harness.

## FIF-034 Accounts and securities endpoints
Status: todo
Requirements: SRV-007, SRV-008, SRV-009, SRV-010, SRV-011
Depends on: FIF-033
Acceptance: CRUDL for accounts and securities; deleting an account or a security referenced by any source record is refused; creating a security with an existing ISIN is a conflict; type and quotation are editable, which is how an auto-created security is corrected.

## FIF-035 Import endpoint
Status: todo
Requirements: SRV-012, SRV-013, SRV-014, SRV-015, SRV-016, SRV-018, SRV-049
Depends on: FIF-034, FIF-023, FIF-029
Acceptance: the caller supplies target account, format and file; formats are Saxo NL XLSX and Trade Republic DE CSV; unknown ISINs are auto-created and flagged; re-posting the same file changes nothing; non-position rows are counted and not stored; the response **names every unrecognized row type it met with how many rows carried it**; a sell exceeding holdings imports fine and surfaces only at attribution.
Notes: Revision 3 splits the counted summary (SRV-017) into FIF-085, it being undecided; SRV-049's naming of unrecognized types is decided and stays here.

## FIF-085 Import response summary counts
Status: blocked
Requirements: SRV-017
Depends on: FIF-035
Acceptance: the import response reports five counts — derived automatically, pending, recognized as non-position, failed to parse, and securities auto-created — and they reconcile against the rows of the fixture.
Blocked by: SRV-017 is on the undecided list.
Notes: Split out of FIF-035 in revision 3. The same five counts are what a batch records (SRV-020, FIF-036), so the two should be reviewed together once this is settled.

## FIF-070 Multi-year refusal at the endpoint
Status: todo
Requirements: SRV-051
Depends on: FIF-035, FIF-065
Acceptance: posting a file whose rows carry trade dates in more than one calendar year is refused at the endpoint with its own problem type, the refusal reaching the caller rather than being swallowed into a generic parse failure.
Notes: Blocked in revision 2 on SRV-056; it and IMP-003 both left the undecided list in revision 3, and SRV-056 re-entered it in revision 8 (OQ-015), so revision 8 splits it into FIF-090. Renamed accordingly; the year refusal is decided on both sides.

## FIF-090 Account-mismatch refusal at the endpoint
Status: blocked
Requirements: SRV-056
Depends on: FIF-070, FIF-089
Acceptance: the file's account id is checked against the target account, and a mismatch, or a file carrying rows from more than one account, refuses the import naming both.
Blocked by: SRV-056 is on the undecided list (OQ-015), as is the domain guard it exposes (IMP-003, FIF-089).
Notes: Split out of FIF-070 in revision 8.

## FIF-036 Import batch endpoints
Status: todo
Requirements: SRV-019, SRV-020, SRV-022
Depends on: FIF-035
Acceptance: every import creates a batch; batches are readable and listable with account, filename, format, timestamp and counts; deletion is refused, naming the offenders, when any derived transaction participates in an attribution or when a record the batch owns is cited by a transaction the batch did not derive.
Notes: Revision 3 splits the deletion behavior itself (SRV-021) into FIF-086; what remains is batch creation, reading and the two refusals, which is reviewable on its own because a refused deletion never reaches the removal path.

## FIF-086 Batch deletion removes its records
Status: blocked
Requirements: SRV-021
Depends on: FIF-036, FIF-071
Acceptance: deleting a batch removes exactly the source records it owns and everything derived from them, and never a manual entry.
Blocked by: SRV-021 is on the undecided list. What "the records it owns" means is itself open (SRV-052, FIF-071).

## FIF-071 Batch ownership on re-import
Status: blocked
Requirements: SRV-052
Depends on: FIF-036
Acceptance: a source record belongs to every batch that supplied it and the **newest** owns it; re-importing a year transfers ownership to the new batch and the superseded batches then own nothing.
Blocked by: SRV-052 is on the undecided list.
Notes: New in this revision. It decides what "the records a batch owns" in SRV-021 and SRV-022 means, so FIF-036 is only fully reviewable once this is settled.

## FIF-037 Source record endpoints
Status: todo
Requirements: SRV-023, SRV-024, SRV-027
Depends on: FIF-035
Acceptance: source records readable and listable with filters on account, batch, security and consumed-or-pending; the pending filter is the completion queue; there is no update endpoint, and a mistake is corrected by deleting the derived transaction and the manual entry.

## FIF-072 Manual entry endpoints
Status: todo
Requirements: SRV-025, SRV-026, SRV-048, SRV-053, SRV-055
Depends on: FIF-037, FIF-062
Acceptance: create a manual entry, which is the only way information no export contains enters the system; creation is idempotent on its content and the source record identities it cites, so replaying an exported file cannot duplicate one; list all entries for export, and list those whose source records are absent with what each is waiting for; delete an entry, which is the only thing that removes one; importing source records whose identities a waiting entry names reconnects it and restores the transaction it completed, without asking.
Notes: New in this revision; the previous plan folded manual information into FIF-037 as a source record kind.

## FIF-038 Transaction endpoints
Status: todo
Requirements: SRV-028, SRV-029, SRV-031, SRV-032, SRV-033, SRV-054
Depends on: FIF-037, FIF-072
Acceptance: transactions readable and listable with filters on account, security, type and date range, including an unattributed-closings filter; a derive endpoint builds a transaction from one or more pending source records plus what the user supplied, storing the supplied part as a manual entry cited alongside the imported records; deleting a derived transaction returns its source records to pending; **no endpoint edits one** — a transferred parcel's acquisition date in particular is fixed at import and never corrected.
Notes: Revision 3 splits SRV-034, that no endpoint creates a transaction from nothing, into FIF-087, it being undecided.
DOM-069 forbids three operations on an attributed transaction — edited, re-rated, deleted. FIF-012 refuses two; the third has no surface anywhere in the workspace to refuse, and this item is where the acceptance says there is to be none (`no endpoint edits one`). Should an edit surface ever be added, here or elsewhere, DOM-069's edit clause is that item's to refuse and to test; it is carried here so the gap is tracked where it would be closed rather than only in a test comment.

## FIF-087 No endpoint creates a transaction from nothing
Status: blocked
Requirements: SRV-034
Depends on: FIF-038, FIF-081
Acceptance: the HTTP surface offers no route that constructs a transaction other than by derivation from source records, asserted by a test over the routing table rather than by convention.
Blocked by: SRV-034 is on the undecided list, as is its domain counterpart DOM-047 (FIF-081).
Notes: Split out of FIF-038 in revision 3.

## FIF-073 Transfer out approval emits its transfer ins
Status: blocked
Requirements: SRV-030
Depends on: FIF-038, FIF-063
Acceptance: approving a `transfer_out` also creates the `transfer_in` records it implies, in the same operation.
Blocked by: SRV-030 is on the undecided list, as is the domain rule it exposes (DOM-090, in FIF-063).

## FIF-039 Attribution endpoints
Status: todo
Requirements: SRV-035, SRV-036, SRV-037, SRV-038, SRV-039, SRV-040, SRV-041, SRV-042, SRV-043, SRV-050
Depends on: FIF-038, FIF-015
Acceptance: a proposal endpoint returns the closing, the proposed allocations with derived figures and a fingerprint, and is also addressable as "the next closing awaiting attribution" for an account and security; the fingerprint covers the closing, every allocation's opening id and quantity **and every derived money figure displayed**, as a hash of a canonical serialization stable across processes, so a re-rating between display and approval changes it; an uncoverable closing returns the named shortfall instead; a security with pending records is refused, naming what is outstanding; creating an attribution requires the fingerprint the client was shown, and a mismatch is a conflict; attributions can be read, listed and deleted but never updated; deletion is refused when a later attribution exists for that account and security; declining is not an API call.

## FIF-040 Report endpoints
Status: todo
Requirements: SRV-044, SRV-045
Depends on: FIF-039, FIF-030, FIF-031
Acceptance: read-only endpoints for the income tax overview and the acquisition report, both accepting optional account and year filters, returning exactly the figures the CLI formats with no additional computation client-side.

## FIF-041 FX rate endpoints
Status: todo
Requirements: SRV-046, SRV-047
Depends on: FIF-033, FIF-010
Acceptance: list cached rates with filters, and trigger a refresh from the ECB feed; the cache seeds from the full historical series on first use. Integration tests inject a fake rate source; nothing reaches the network.

## FIF-042 CLI binary, arguments and HTTP-only access
Status: todo
Requirements: CLI-001, CLI-006, ARC-004, ARC-005
Depends on: FIF-032
Acceptance: `fifolio-cli` replaces the stub and starts the interactive application; `--server-url` defaults to `http://127.0.0.1:8000`; the crate has no database dependency, no dependency on `fifolio-core` and no domain rule of its own — every figure it shows comes from the server.

## FIF-043 Localization catalogs and language selection
Status: todo
Requirements: CLI-007, CLI-023, CLI-024, CLI-025, CLI-029, CLI-030, ARC-026, TST-023
Depends on: FIF-042
Acceptance: Fluent catalogs for `en` and `nl`, keyed by identifier, with plurals and interpolation handled by the catalog; `--lang` wins when given, otherwise the language derives from `LC_ALL`, `LC_MESSAGES`, `LANG`, where a locale resolving to `nl` selects Dutch and anything else English; an unrecognized `--lang` is an error listing the supported languages; a missing Dutch key falls back to English rather than showing the key; a test asserts key parity in both directions and the build fails when it breaks; neither core nor server carries a locale concern.

## FIF-044 Translation scope and locale formatting
Status: todo
Requirements: CLI-026, CLI-027, CLI-028, CLI-035, CLI-036
Depends on: FIF-043
Acceptance: translated surfaces are exactly view titles, breadcrumbs, interactive column headers, hint bar verbs, dialog prompts and buttons, and user-facing error and status messages; report output in every format, all CSV and JSON, and stored data are never translated; API error `type` values stay as-is on the wire and are mapped to translated messages in the CLI; dates and numbers follow the language inside the interactive application only (`2025-01-20` / `1825.50` vs `20-01-2025` / `1.825,50`), while everywhere else dates are ISO 8601 and decimals use a dot.

## FIF-045 UI-agnostic client layer
Status: todo
Requirements: ARC-025, TST-022
Depends on: FIF-042
Acceptance: a client layer holding the typed HTTP client, the view stack, the key-to-action mapping, completion queue state and dialog logic, with no `ratatui` type in its signatures; unit tested headlessly against a stubbed HTTP layer; the render layer can only draw what this layer decided.

## FIF-046 `report` subcommand
Status: todo
Requirements: CLI-002, CLI-003, CLI-004, CLI-008, TST-017, TST-018
Depends on: FIF-045, FIF-040
Acceptance: `fifolio-cli report <income-tax|acquisitions>` prints the corresponding report with `--format` `human` | `csv` | `json`, **defaulting to `json`**, and optional `--account` and `--year`; output is untranslated, ISO dates, dot decimals, and pipeable; reports are absent from the interactive application; `insta` snapshots over a fixed scenario built in code, one snapshot per format, so a layout change cannot silently alter the CSV.
Notes: The subcommand tokens and the JSON default are new in this revision (DEC-034).

## FIF-047 `export-manual-information` subcommand
Status: todo
Requirements: CLI-005, CLI-009, CLI-038
Depends on: FIF-045, FIF-072
Acceptance: writes every manual entry to the named file as **a single JSON document carrying a schema version**, recording per entry the account, the security, what was supplied, and the identities of the imported source records it was attached to.
Notes: Previously blocked on decision D2, the file format. DEC-020 and CLI-038 resolved it: versioned JSON, with a matching import (FIF-074). D2 is closed.
Revision 12: CLI-009 and CLI-038 were restated in `9d8c9b9` (DEC-060) to drop the acquisition date from the list of manual shapes the exported document carries. The ids and this item's acceptance are unchanged — the shapes are FIF-059's closed set, which the exporter simply serializes — so nothing here is invalidated; recorded so the narrowing is not rediscovered as a discrepancy against an older reading of `cli.md`.

## FIF-074 `import-manual-information` subcommand
Status: todo
Requirements: CLI-037, CLI-039, CLI-040
Depends on: FIF-047
Acceptance: replays an exported file into an empty or partial database; an entry whose imported source records are not present is reported and skipped rather than guessed at; replay is idempotent, so an entry already present is recognized and not duplicated. Round-trip test: export, wipe, re-import the broker fixture, replay, and assert the database matches.
Notes: New in this revision (CLI-037, DEC-020).

## FIF-048 TUI shell: layout, navigation, hint bar
Status: todo
Requirements: CLI-010, CLI-011, CLI-012, CLI-013, CLI-031, CLI-032, TST-009
Depends on: FIF-045
Acceptance: one full-width view at a time with a breadcrumb on top and a context hint bar at the bottom; Enter descends, Esc ascends, over the stack Accounts › Securities › Transactions with the attribution flow and the completion queue reachable from an account or a security; the hint bar is a single dim line listing only the keys valid in the current view, each paired with a verb, with no fixed F-key row, degrading gracefully when the terminal is narrow; all column widths are computed from measured string widths, never hardcoded, verified with the longer Dutch strings; the render layer is excluded from coverage and contains no decision.

## FIF-049 Dialogs
Status: todo
Requirements: CLI-014, CLI-015, CLI-016
Depends on: FIF-048
Acceptance: dialog buttons name their action rather than answering a question; the safe choice is leftmost and focused by default; destructive actions are visually distinct. Button set and focus are decided in the client layer and unit tested there.

## FIF-050 TUI browsing, management, import and batches
Status: todo
Requirements: CLI-017, CLI-018, CLI-019
Depends on: FIF-049, FIF-036, FIF-034
Acceptance: the interactive application covers browsing, account and security management and import; import batches are browsable with file, account, timestamp and counts, and deletable to undo an import; deleting a batch whose transactions are attributed is refused and the dialog names what stands in the way.

## FIF-051 TUI completion queue
Status: todo
Requirements: CLI-020, CLI-021, CLI-022, CLI-042
Depends on: FIF-050, FIF-038, FIF-072
Acceptance: an entry shows the imported rows it covers, every figure the file does state, and the one missing part; what is asked matches the event — stock-or-cash election plus share count for dividends, ratio for splits, target security and ratio for exchanges, quantity disposed and any target security for cash mergers, tenders and partial buybacks, and a transfer out approved like any other disposal; money, dates, currency and rate are never retyped; a group containing a reversal shows it as a reversal instead of folding it into a total; manual entries whose source records are absent are listed separately, naming the account, security and rows each waits for, and can be deleted when obsolete.
Notes: Blocked in revision 2 on CLI-020, which left the undecided list in revision 3.

## FIF-052 TUI attribution flow
Status: todo
Requirements: CLI-033, CLI-034, CLI-041
Depends on: FIF-051, FIF-039
Acceptance: after selecting an account and optionally a security, the application requests the next closing awaiting attribution — a sell, an expiration or a transfer out — and shows it with proposed openings and per-allocation figures; a transfer out shows no gain and names the `transfer_in` records approving it will create; approving posts the proposal back with its fingerprint and moves to the next closing; declining stores nothing and stops, because later closings are blocked; a shortfall is displayed with the missing quantity named instead of a proposal; when the security has anything outstanding in the completion queue, the application says so and links there rather than offering a proposal.

## FIF-053 End-to-end tests
Status: todo
Requirements: TST-007
Depends on: FIF-052, FIF-046
Acceptance: tests spawn the real `fifolio-server` binary on a temporary database and a free port and drive it with the real `fifolio-cli` binary: import a fixture, resolve the completion queue, approve an attribution, run a report, asserting on what a user would see. Kept to those paths; no network access.

---

# Retired items

Ids are never reused.

* **FIF-018 — Corporate action engine.** Dropped in this revision. `domain.md` replaced the separate corporate-action entity with transaction variants (DEC-024, DEC-025), retiring DOM-014, DOM-015 and DOM-050 to DOM-053. Its two halves became FIF-061 (splits and effective quantity) and FIF-063 (transfer out emission, basis and decomposition); DOM-016, the citation rule, moved to FIF-056.

# Revision history

**Revision 24 (this run).** A resumption check after the previous session was stopped mid-run.
Nothing moved since revision 23: `HEAD` is still `6db99f1`, `design/` is unchanged since `d160fee`,
the only modified file is this plan (revision 23's own entry, never committed), and no source file
is untracked, so **FIF-012 was not started** before the stop. Re-verified mechanically: **333**
identifiers in `design/`, each named by exactly one item; **32** blocked ids across OQ-001 to
OQ-016; **23** items `blocked` and no item blocked on anything else; 91 items, 18 `done`, 50 `todo`.
`cargo test --workspace` on the clean tree: **237 passed, 0 failed**.

* **Added / split / dropped / re-scoped / renumbered:** none. No dependency moved. **Uncovered:** none.
* **Completed:** none. **Newly blocked:** none. **Unblocked:** none.
* Next to build is unchanged: **FIF-012**, storage-enforced invariants — the first `todo` in
  document order, its one dependency FIF-011 `done` (`e5c2900`), none of its seven requirements on
  a `Blocks:` line.

**Revision 23.** A status reconciliation. `design/` is unchanged since `d160fee`, which
revision 21 already absorbed, so the live set is still **333 identifiers**, each named by exactly
one item; `open-questions.md` is untouched and still names the same **32 blocked ids** across
OQ-001 to OQ-016, and the same twenty-three items are `blocked`. The working tree is clean and
nothing sits uncommitted, which is the first revision since 14 that can say so. No item was added,
split, dropped, re-scoped, renumbered or re-ordered, and no dependency moved.

* **Completed:** none this run; FIF-010 was completed and reviewed in revision 22 (`bd7946b`, tightened in `6db99f1`).
* **Newly blocked:** none. **Unblocked:** none. **Uncovered:** none.
* **Bookkeeping fixed:** revision 22 marked FIF-010 `done` in that item's notes but wrote no history entry, and left revision 21 labelled "this run". Both are corrected here; the entry below reconstructs revision 22 from the item note and the commits, and is marked as such.
* Next to build: **FIF-012**, storage-enforced invariants. Its one dependency FIF-011 is `done`, none of its seven requirements is on a `Blocks:` line, and it is the first `todo` in document order.

**Revision 22 (reconstructed in revision 23 from FIF-010's note and commits `bd7946b`, `6db99f1`).**
The review pass over the increment revision 21 found untracked. FIF-010 moved `todo` -> `done`;
ARC-017 and ARC-018 stay open with FIF-041, as that item's acceptance scopes. No other status,
placement or dependency changed, and `design/` gained no identifier.

**Revision 21.** A status reconciliation with one specification change to absorb.
`design/` changed in `d160fee` (DEC-068): a top-up of the rate cache adds days the cache lacks and
never rewrites one it holds. The live set is still **333 identifiers**, each named by exactly one
item — the new rule was cited as `[ARC-026]`, which `architecture.md` already uses for the
localization scope, so it introduces no id. `open-questions.md` is untouched and still names the
same **32 blocked ids** across OQ-001 to OQ-016; the twenty-three blocked items are unchanged. No
item was added, split, dropped, re-scoped, renumbered or re-ordered, and no dependency moved.

* **Completed:** none.
* **Invalidated by the specification change:** nothing. DEC-068 restates ARC-018, which is FIF-010's and is not yet committed, so it documents the code in the working tree rather than contradicting shipped work.
* **Newly blocked:** none. **Unblocked:** none.
* **Not marked done:** FIF-010. The rate cache, the two ingest paths, the `RateFeed` port, the migration and the recorded ECB fragments are all written and **uncommitted**; the suite is green at 233 tests. It stays `todo` for the reason five earlier revisions held an item back — uncommitted is unreviewed — and it is again the item to build, the outstanding work being review and the commit, not a rebuild.
* **Hand-off, said plainly:** this is the sixth revision to find a finished increment sitting untracked (FIF-055, FIF-003, FIF-056, FIF-059, FIF-008, FIF-011 before it). The plan cannot fix that; it can only keep refusing to record as done what no commit carries.
* **For `spec-auditor`, not a planning decision:** `architecture.md` now uses `[ARC-026]` for two different rules and `[ARC-025]` for two, the latter unfixed since revision 19. Neither blocks anything and neither changes a coverage count, but the top-up rule has no identifier of its own.
* Next to build: **FIF-010**, ECB rate cache and seeding. Both dependencies are `done`, none of ARC-015 to ARC-018 is on a `Blocks:` line, and it is the first `todo` in document order. FIF-012 is equally ready and sits behind it.

**Revision 20 (this run).** A status reconciliation. `design/` is unchanged since `72abea7`, which
revision 19 already absorbed, so the live set is still **333 identifiers**, each named by exactly
one item, and `open-questions.md` is untouched and still names the same **32 blocked ids** across
OQ-001 to OQ-016; the twenty-three blocked items are unchanged. No item was added, split, dropped,
re-scoped, renumbered or re-ordered, and no dependency moved.

* **Completed:** FIF-011 (`e5c2900`), SQLite storage and migrations, which revisions 18 and 19 both named next to build. The uncommitted tree revision 19 recorded is now committed and reviewed against its six requirements and the DEC-067 refusal clause; see the item's closing note for what was checked. The hand-off problem is closed again.
* **Invalidated by the specification change:** nothing; the specification did not change.
* **Newly blocked:** none. **Unblocked:** none — FIF-010 and FIF-012 become *ready*, which is not the same thing; neither was ever blocked, both were waiting on FIF-011.
* **Uncovered requirements:** none. The nine identifiers absent from every item — DOM-009, DOM-014, DOM-015, DOM-021, DOM-041 and DOM-050 to DOM-053 — are the retired set `domain.md` itself lists as retired.
* **Uncommitted work:** none. The tree is clean and `cargo test --workspace` passes 208 tests, 0 failures.
* **Still open for `spec-auditor`, not a planning decision:** the duplicate `[ARC-025]` citation revision 19 flagged in `architecture.md` is unfixed. Coverage is unaffected.
* Next to build: **FIF-010**, ECB rate cache and seeding. It is the first `todo` in document order, both its dependencies FIF-009 and FIF-011 are `done`, and none of ARC-015 to ARC-018 is on a `Blocks:` line. It closes the FX area: FIF-009 built the resolver behind a port and this builds the table and the feed behind it. FIF-012 is equally ready and sits behind it in plan order; taking FIF-010 first keeps the whole rate story reviewable as one before the storage invariants grow the schema.

**Revision 19 (this run).** A status reconciliation with one specification change to absorb.
`design/` changed in `72abea7` (DEC-067): ARC-010 is restated so that rounding happens **before**
the storage and presentation boundaries and storage refuses an unrounded value instead of rounding
it. The commit adds a decision, not a requirement identifier, so the live set is still **333
identifiers**, each named by exactly one item. `open-questions.md` is untouched and still names the
same **32 blocked ids** across OQ-001 to OQ-016; the twenty-three blocked items are unchanged. No
item was added, split, dropped, renumbered or re-ordered, and no dependency moved.

* **Completed:** none. Revision 18 named FIF-011 next to build; the work exists but is uncommitted, so the item is still `todo`.
* **Invalidated by the specification change:** nothing completed. ARC-010 sits on FIF-004 (`1eec01a`), which shipped `round_to` and left the applying to callers; DEC-067 decides who *checks*, which is FIF-011's repositories and is unbuilt as far as the history goes. FIF-004 gains a pointer rather than an edited acceptance; **FIF-011's acceptance gains the refusal clause**, which is allowed because the item is `todo`.
* **Re-scoped:** FIF-011 only, by that one clause.
* **Newly blocked:** none. **Unblocked:** none. **Uncovered requirements:** none. The nine identifiers absent from every item — DOM-009, DOM-014, DOM-015, DOM-021, DOM-041 and DOM-050 to DOM-053 — are the retired set `domain.md` itself lists as retired.
* **Uncommitted work: FIF-011, whole.** The migrations directory, `src/storage/` and `tests/storage.rs` are untracked and five tracked files are modified. `cargo test --workspace` passes 207 tests, 0 failures. This is the hand-off problem revisions 13 to 15 recorded, returning after three clean revisions; the fix is a commit, not a replan.
* **Flagged for `spec-auditor`, not a planning decision:** the sentence `72abea7` rewrote cites `[ARC-025]` on its rationale clause, while ARC-025 is already the UI-agnostic client layer requirement further down `architecture.md`. One identifier now labels two unrelated statements. Coverage is unaffected — ARC-025 is FIF-045's — but the citation is wrong in one of the two places.
* Next to build: **FIF-011**, SQLite storage and migrations, unchanged from revision 18 and for the same reason: its dependencies FIF-005, FIF-056 and FIF-059 are all `done`, none of ARC-011 to ARC-014, DOM-071 or TST-004 is on a `Blocks:` line, and everything ahead of it in plan order is `done` or `blocked` except FIF-010, which waits on it. The work is to read the untracked tree against the six requirements and the new refusal clause, then commit it. FIF-012, FIF-017, FIF-032 and FIF-010 all unblock behind it.

**Revision 18 (this run).** A status reconciliation, not a replan, for the second consecutive
revision. `design/` is unchanged since revision 14 — `b21d225` is still the last commit to touch it
— so the live set is still **333 identifiers**, each named by exactly one item, and
`open-questions.md` still names the same **32 blocked ids** across OQ-001 to OQ-016. The
twenty-three blocked items are unchanged. No item was added, split, dropped, re-scoped, renumbered
or re-ordered, and no dependency moved.

* **Completed:** FIF-009 (`6384132`), FX rate resolution, which revision 17 named as next to build. Verified against `crates/fifolio-core/src/fx.rs` rather than the commit message: `resolve` answers the three sources in the stated precedence behind an injected `RateTable`, the gap fallback stores the substituting publication's own date and is bounded at both ends by `series_start()` and `MAX_SUBSTITUTION_DAYS`, and `BeforeSeries`, `Unavailable` and `StaleSubstitute` each name the currency and the date. `fee_in_eur` refuses an informational rate. Twelve unit tests, none opening a socket [TST-019 to TST-021]. Revision 17 recorded the item as done without a commit hash; the hash is now on the item.
* **Invalidated by the specification change:** nothing; the specification did not change.
* **Newly blocked:** none. **Unblocked:** none. **Uncovered requirements:** none. The nine identifiers absent from every item — DOM-009, DOM-014, DOM-015, DOM-021, DOM-041 and DOM-050 to DOM-053 — are the retired set `domain.md` itself lists as retired.
* **Uncommitted work:** none. The tree is clean and `cargo test --workspace` passes 185 tests, 0 failures. The hand-off problem revisions 13 to 15 recorded has now stayed closed for three consecutive revisions.
* Next to build: **FIF-011**, SQLite storage and migrations. Its dependencies FIF-005, FIF-056 and FIF-059 are all `done`, and none of ARC-011 to ARC-014, DOM-071 or TST-004 is on a `Blocks:` line. FIF-010 stands ahead of it in document order and is not ready: it depends on this item. Everything else ahead of FIF-011 is `done` or `blocked`. This is the point at which the plan leaves core arithmetic, and the point at which the remaining `todo` items stop being reachable one at a time — FIF-012, FIF-017, FIF-032 and FIF-010 all unblock behind it, so a schema that guesses at the still-undecided relations would be expensive to unpick.

**Revision 17 (this run).** A status reconciliation, not a replan. `design/` is unchanged since
revision 14 (`b21d225` is still the last commit to touch it), so the live set is still **333
identifiers**, each named by exactly one item, and `open-questions.md` still names the same **32
blocked ids** across OQ-001 to OQ-016. The twenty-three blocked items are unchanged. No item was
added, split, dropped, re-scoped, renumbered or re-ordered, and no dependency moved.

* **Completed:** FIF-091 (`aa6aac5`), a stock dividend's taxable value is its stored EUR gross, which revision 16 named as next to build. Verified against `crates/fifolio-core/src/transaction.rs` rather than the commit message: `BuyOrigin::StockDividend` is a unit variant and `Buy::taxable_value()` returns `self.gross.eur()`, so no field, constructor argument or accessor states the two apart. FIF-057's fixture asserting 81.00 against 3 x 26.10 is replaced by one asserting the identity at 78.30, and three cases sit beside it — an ordinary purchase has no taxable value, a USD gross takes its EUR half and never the native amount, and a zero gross is a stated figure. The replacement is visible in the diff, not a deletion.
* **Invalidated by the specification change:** nothing; the specification did not change.
* **Newly blocked:** none. **Unblocked:** none. **Uncovered requirements:** none. The nine identifiers absent from every item — DOM-009, DOM-014, DOM-015, DOM-021, DOM-041 and DOM-050 to DOM-053 — are the retired set `domain.md` itself lists as retired, so they are not live and need no item.
* **Uncommitted work:** none. The tree is clean and `cargo test --workspace` passes 173 tests, 0 failures. The hand-off problem revisions 13 to 15 recorded has now stayed closed for two consecutive revisions.
* Next to build: **FIF-009**, FX rate resolution. Its only dependency FIF-008 is `done`; none of DOM-030 to DOM-035, ARC-019, ARC-027, TST-019, TST-020 or TST-021 is on a `Blocks:` line; it is the first `todo` in plan order whose dependencies are all satisfied. Everything ahead of it in plan order is `done` or `blocked`. It is resolution only — the cache and the ECB seeding are FIF-010, which also waits on storage (FIF-011).

**Revision 16.** A status reconciliation, not a replan. `design/` is unchanged since
revision 14 (`b21d225` is still the last commit to touch it), so the live set is still **333
identifiers**, each named by exactly one item, and `open-questions.md` still names the same **32
blocked ids** across OQ-001 to OQ-016. The twenty-three blocked items are unchanged. No item was
added, split, dropped, re-scoped, renumbered or re-ordered, and no dependency moved.

* **Completed:** FIF-008 (`9a1e457`, tightened in `2d4da5b`), EUR valuation and the stored gross, which revision 15 named as next to build and which had stood uncommitted for three revisions. `valuation.rs` is now tracked and `transaction.rs` carries the `Valued` native/EUR pair and a `Conversion` per money-bearing variant. The hand-off worry revision 15 recorded is closed: nothing is uncommitted in the tree now, and the suite is green (`cargo test --workspace`, 0 failures).
* **Moved without a new item:** DOM-084 went from FIF-008 to FIF-014 during revision 15's build, and both items record it. Coverage is unaffected — it is still named exactly once.
* **Invalidated by the specification change:** nothing; the specification did not change.
* **Newly blocked:** none. **Unblocked:** none. **Uncovered requirements:** none.
* Next to build: **FIF-091**, a stock dividend's taxable value is its stored EUR gross. Both dependencies, FIF-057 and FIF-008, are `done`; DOM-123 is on no `Blocks:` line; it is the first `todo` in plan order whose dependencies are all satisfied. It is the correction FIF-008's own notes point at: `transaction.rs` still carries `BuyOrigin::StockDividend { taxable_value }` as an independent `Money`, with a fixture of 81.00 against a stated 3 x 26.10 = 78.30, which is the divergence DEC-061 forbids.

**Revision 15.** A status reconciliation, not a replan. `design/` is unchanged since
revision 14 (`b21d225` is still the last commit to touch it), so the live set is still **333
identifiers**, each named by exactly one item, and `open-questions.md` still names the same **32
blocked ids** across OQ-001 to OQ-016. The twenty-three blocked items are unchanged. No item was
added, split, dropped, re-scoped, renumbered or re-ordered, and no dependency moved.

* **Completed:** FIF-092 (`cd09f97`), the manual entry shapes, which revision 14 named as next to build and which that commit also recorded as `done`. Verified against `crates/fifolio-core/src/manual_entry.rs` rather than the commit message: `Supplied` has the five shapes DOM-097 lists, the standalone `ShareCount` variant is gone, and each shape carries a unit test naming its completion-queue case.
* **Invalidated by the specification change:** nothing; the specification did not change.
* **Newly blocked:** none. **Unblocked:** none. **Uncovered requirements:** none.
* **Still uncommitted:** FIF-008. `valuation.rs` is untracked and `transaction.rs` modified, for the third consecutive revision. It stays `todo` under the rule revisions 5, 7, 9 and 13 applied: uncommitted is unreviewed, and an unreviewed item is still the item to build. The full suite is green there (169 tests, 0 failures).
* Next to build: **FIF-008**, EUR valuation and the stored gross. Both dependencies, FIF-004 and FIF-056, are `done`; none of DOM-025 to DOM-029, DOM-084, DOM-085 or DOM-086 is on a `Blocks:` line — the undecided part of that area, DOM-104, is already split out into FIF-077. It is the first `todo` in plan order whose dependencies are all satisfied. The work is review and a commit, not a rebuild, and **FIF-091 lands on top of it**.

**Revision 14 (this run).** `design/` changed twice since revision 13, in `a0ee437` (DEC-062) and
`b21d225` (DEC-063 to DEC-066). Three requirement identifiers are added — DOM-124, DOM-125,
IMP-SAXO-036 — and one, DOM-097, is restated in a way that contradicts shipped code. The live set is
**333**. `open-questions.md` is untouched and still names the same **32 blocked ids** across OQ-001
to OQ-016, so the twenty-three blocked items are unchanged.

* **Completed:** FIF-059 (`4af8667`), the manual entry entity, which revision 13 named as next to build.
* **Invalidated by the specification change: yes, for the second time.** DEC-062 withdraws DEC-060's claim that DOM-097's list closed at three shapes and names five. FIF-059's commit landed *before* that decision, so `manual_entry.rs` ships three variants and is missing a split's bare ratio and a disposed quantity with an optional target — both of which `importers.md` already marks pending, so the completion queue would meet rows it has no shape to answer. Said as its own item, **FIF-092**, which takes DOM-097 from FIF-059; FIF-059 stays `done` with a pointer and keeps DOM-098 to DOM-100 and DOM-122.
* **Added:** FIF-092 (DOM-097), placed immediately after FIF-059, which it corrects and which everything reading the shapes — FIF-062, FIF-072, FIF-051 — sits behind.
* **Added coverage without new items:** DOM-124 (DEC-063, a dividend taken in shares is stored) joins FIF-017, which already owns the DOM-002 / DOM-046 rule it qualifies; DOM-125 (DEC-066, a gain is arithmetic on the rounded shares) joins FIF-014, which owns the rounding rule it qualifies; IMP-SAXO-036 (DEC-064, three ordering columns rather than one folded key) joins FIF-066, whose acceptance is rewritten to match. None of the three is separately reviewable from the item it joins, and none of the three items is built, so no completed work is touched.
* **FIF-066's acceptance was rewritten rather than annotated**, which is allowed only because the item is `blocked` and unbuilt. IMP-SAXO-036 does not unblock it: its closing clause hands absent counters back to OQ-013, which is exactly what blocks it.
* **Newly blocked:** none. **Unblocked:** none. Nothing was dropped or renumbered, and no dependency moved.
* Next to build: **FIF-092**, the manual entry shapes. Its only dependency FIF-059 is `done`, DOM-097 is on no `Blocks:` line, and it is the first `todo` in plan order whose dependencies are all satisfied — it sits immediately after the item it corrects, which is where a correction is reviewable. It is ahead of FIF-008 deliberately: DOM-097's shapes are already committed wrong, and FIF-062, FIF-072 and FIF-051 all read them, so the longer it stands the more is built on three variants where the queue asks for five. The work is one enum and its tests, in `manual_entry.rs`, which FIF-008's uncommitted tree does not touch — but that tree (`valuation.rs` untracked, `transaction.rs` modified) is still there, so commit around it deliberately rather than sweeping it into this item's commit.
* After that: **FIF-008**, EUR valuation and the stored gross, whose module is written and uncommitted; the work there is review and a commit, and **FIF-091 lands on top of it** — read that item before committing the pair of figures DEC-061 forbids.

**Revision 13.** `design/` changed again, in commit `47af98a` (DEC-061): a stock
dividend's taxable value and its EUR gross are **one** figure, not two. It adds one requirement
identifier, DOM-123, taking the live set to **330**. `open-questions.md` is untouched and still
names the same **32 blocked ids** across OQ-001 to OQ-016, so the twenty-three blocked items are
unchanged.

* **Invalidated by the specification change: yes, and for the first time.** DEC-061 contradicts part of what FIF-057 shipped in `c1c3d58` — the taxable value as a field of its own, with a test fixture setting it to 81.00 against the buy's own 78.30, which is the exact pair the decision names. Said as its own item, **FIF-091**, rather than by editing FIF-057's acceptance until the shipped code looks right in hindsight. FIF-057 stays `done` with a pointer; the correction is reviewable on its own.
* **Added:** FIF-091 (DOM-123), placed after FIF-008 because the figure the taxable value collapses into is the EUR gross FIF-008 introduces.
* **Status convention corrected.** Revision 12 introduced `in-progress` for FIF-059, whose module was written and uncommitted. `in-progress` is not selectable, and this revision found **two** such modules — FIF-059's `manual_entry.rs` and FIF-008's `valuation.rs`, both untracked, both green — which between them would have left the plan naming nothing to build while everything downstream waited on their commits. Both are `todo` again, which is what revisions 5, 7 and 9 did with FIF-055, FIF-003 and FIF-056: uncommitted is unreviewed, and an unreviewed item is still the item to build. The notes on both say the outstanding work is review and a commit, not a rebuild.
* **Completed:** none. **Newly blocked:** none. **Unblocked:** none. No item was split, dropped, renumbered or re-ordered, and no dependency moved.
* Next to build: **FIF-059**, the manual entry entity, unchanged from revision 11's answer and for the same reason — its only dependency FIF-005 is `done`, none of DOM-097 to DOM-100 or DOM-122 is on a `Blocks:` line, and it is the first such item in plan order. FIF-011, and through it the whole storage and import chain, waits on it. The work is to read `manual_entry.rs` against the acceptance and commit it. FIF-008 is the next one after, and FIF-091 lands on top of that.

**Revision 12.** `design/` **did** change since revision 8, which revisions 10 and 11
both missed: commit `9d8c9b9` (DEC-060) settles that an acquisition date can never be corrected by
hand. It adds one requirement identifier, DOM-122, and restates the prose of CLI-009 and CLI-038
without changing their ids. The live set is therefore **329 identifiers**, not 328.
`open-questions.md` is untouched and still names the same **32 blocked ids** across OQ-001 to
OQ-016, so the twenty-three blocked items are unchanged.

* **Added coverage:** DOM-122 joins FIF-059 rather than becoming an item of its own; it closes the set DOM-097 opens and cannot be reviewed apart from it. No item was split, dropped, renumbered or re-ordered, and no dependency moved.
* **Invalidated by the specification change:** nothing. DEC-060 confirms the direction DEC-040 already set and the plan already carried — FIF-024 fixes the date at import, FIF-038 forbids editing it, and neither is `done`. This is recorded as a line of its own because a changed specification must be checked against completed work out loud, not silently.
* **In progress:** FIF-059, whose module is written and green but uncommitted (`crates/fifolio-core/src/manual_entry.rs`, untracked; `lib.rs` modified). It is not `done`: there is no commit to cite.
* **Completed:** none this revision. **Newly blocked:** none. **Unblocked:** none.
* Next to build: **FIF-008**, EUR valuation and the stored gross. Both dependencies, FIF-004 and FIF-056, are `done`; none of DOM-025 to DOM-029, DOM-084, DOM-085 or DOM-086 appears on a `Blocks:` line — the undecided part of that area, DOM-104, was already split into FIF-077. It is the first `todo` in plan order whose dependencies are all satisfied, FIF-059 above it now being `in-progress`. Commit FIF-059 first.

**Revision 11.** A status reconciliation, not a replan. `design/` is unchanged since
revision 8 (`8dbca69` is still the last commit to touch it), so the live set is still **328
identifiers**, each named by exactly one item, and `open-questions.md` still names the same **32
blocked ids** across OQ-001 to OQ-016. No item was added, split, re-scoped, renumbered or
re-ordered, and no dependency moved.

* **Completed:** FIF-057 (`c1c3d58`), the buy origin, which revision 10 named as next to build and which was committed but left `todo` in the plan. The full suite passes (`cargo test`, 143 tests across the workspace, 0 failures) and the working tree is clean.
* **Newly blocked:** none. **Unblocked:** none. The twenty-three blocked items are those of revisions 8 to 10.
* Next to build: **FIF-059**, the manual entry entity. Its only dependency, FIF-005, is `done`; DOM-097 to DOM-100 appear on no `Blocks:` line. It is the first `todo` in plan order whose dependencies are all satisfied — the two `todo` items before it in document order are none, and the ones after it (FIF-008, FIF-064) depend on items that are themselves `todo` or blocked.

**Revision 10 (this run).** A status reconciliation, not a replan. `design/` is unchanged since
revision 8 (`8dbca69` is still the last commit to touch it), so the live set is still **328
identifiers**, each named by exactly one item, and `open-questions.md` still names the same **32
blocked ids** across OQ-001 to OQ-016. No item was added, split, re-scoped, renumbered or
re-ordered, and no dependency moved.

* **Completed:** FIF-056 (`74e4bd1`, recorded as done in `5d54c2b`), the transaction sum type — the work revisions 8 and 9 both found outstanding. Verified against `crates/fifolio-core/src/transaction.rs` rather than against the commit message: six variants grouped as `Opening` / `Closing` / `Split`, a `Derivation` carrying the trade date and the cited record identities on every variant, `TransferInSource` and `DateProvenance` read-only. The working tree is clean, so nothing else is implemented-but-unreviewed this revision, which is the first time since revision 4 that is true.
* **Newly blocked:** none. **Unblocked:** none. The twenty-three blocked items are the twenty-three of revisions 8 and 9.
* The revision-9 entry's bullet "Not marked done: FIF-056" is superseded by this one rather than edited; it was written before `5d54c2b`, and the hand-off it warned about did close on the next run.
* Next to build: **FIF-057**, the buy origin and stock-dividend cost basis. Its only dependency, FIF-056, is `done`, and DOM-082 is on no `Blocks:` line. It is the first `todo` in plan order and the only one whose dependencies are all satisfied — FIF-059, the next candidate, likewise depends only on the `done` FIF-005, so if FIF-057 stalls that is the item to take instead.

**Revision 9 (this run).** A status reconciliation, not a replan. `design/` is unchanged since
revision 8 (`8dbca69` is still the last commit to touch it; `3399056` changed only the workflow
script), so the live set is still **328 identifiers**, each named by exactly one item, and
`open-questions.md` still names the same **32 blocked ids** across OQ-001 to OQ-016. No item was
added, split, re-scoped, renumbered or re-ordered, and no dependency moved. Coverage was re-verified
mechanically: every id in `design/` that is not on the retired list appears once in this file, every
id on a `Blocks:` line sits on an item marked `blocked`, and no `todo` item carries one.

* **Completed:** none.
* **Newly blocked:** none. **Unblocked:** none.
* **Not marked done:** FIF-056. `crates/fifolio-core/src/transaction.rs` implements it and `lib.rs` declares the module, but the work is uncommitted and therefore unreviewed, so the item stays `todo` under the rule revisions 5 and 7 applied to FIF-055 and FIF-003. Both of those were committed and marked `done` one revision later; this is the same position, not a new one.
* Next to build: **FIF-056** again, and the outstanding work on it is review and the commit, not a rebuild. Its dependency FIF-005 is `done` and none of DOM-010, DOM-012, DOM-016, DOM-081, DOM-083 is blocked. If a second consecutive revision finds it uncommitted, what needs fixing is the hand-off, not the item.

**Revision 8.** `design/` changed in commit `8dbca69`: five decisions recorded
(DEC-055 to DEC-059), one of which supersedes a decision revision 7 planned against, and **three new
open questions**. The live requirement set is unchanged at **328 ids**, each still named exactly
once. One item completed, two split, one requirement reversed in place.

* **Completed:** FIF-003 (`610ca2c`), the anonymized broker fixtures, which revision 7 found implemented but uncommitted. Verified against `fixtures/` and `tools/anonymize-exports/` rather than against the commit message: nine fixture files, 253 rows, 56 tests in the tool. `cargo test --workspace` passes, 124 tests. It had been the first unblocked `todo` since revision 1 and every importer item waited on it.
* **Newly blocked, each after a split so a decided guard is not frozen behind an undecided one:** FIF-065 → **FIF-089** (one account per file, IMP-003) and FIF-070 → **FIF-090** (account-mismatch refusal at the endpoint, SRV-056). Both halves that keep the old id cover the calendar-year guard, which reads only trade dates and is decided; both were renamed to say what they now cover. OQ-015 is what blocks the account half, and it is not a wording gap: the Trade Republic export carries no account identifier at all, so the check has nothing to read.
* **Unblocked:** none.
* **Reversed in place, not quietly edited:** IMP-SAXO-032 in FIF-021. DEC-059 supersedes DEC-054, so the plan's revision-7 clause — round Saxo's native fee so the stored price matches the printed one — is now wrong in the opposite direction: the subtraction keeps full precision and the derived price is authoritative, the sample buy storing 5.750036 against a printed 5.75. FIF-021 has never been started, so nothing built is invalidated; the acceptance line was rewritten and the note says what it used to say and why it changed. DOM-039 was restated alongside it (reconciliation is on the booked amounts, not the printed price); that lands on FIF-021 and FIF-026 too, and an amendment note was added to the `done` FIF-055, which records the debt without reopening the item.
* **New questions reaching already-blocked items, no status change:** OQ-014 (a stock dividend's cost basis) is a third question on IMP-SAXO-013 in FIF-023, and it is the costliest of the three — 19 events in the sample against 12 for every other corporate action combined. OQ-016 (whether a transfer's own fee is basis or fees) joins OQ-003 on DOM-106 and OQ-011 on DOM-112, in FIF-063 and FIF-080.
* **Blocked ids: thirty-two, up from thirty. Blocked items: twenty-three, up from twenty-one.**
* Next to build: **FIF-056**, the transaction sum type. Its only dependency, FIF-005, is `done`, and none of DOM-010, DOM-012, DOM-016, DOM-081, DOM-083 is blocked. Its own splits FIF-076 and FIF-058 are blocked, so it lands without relations to account, security and source record; the note on it says so.

**Revision 7.** `design/` changed for the first time since revision 4: nine requirement
identifiers were added and three restated, taking the live set from 319 to **328**. No identifier
was retired and nothing completed was invalidated — the additions land on items that are all still
`todo`, and the one rule that touches shipped code, IMP-SAXO-032, is an importer exception to a
policy `precision.rs` (FIF-054) states rather than a change to it.

* **Added:** **FIF-088** (Saxo reversal rows and group summation), carrying IMP-SAXO-033, IMP-SAXO-034 and IMP-SAXO-035. Cut as its own item rather than folded into FIF-023, which is blocked: the suffix rule and the group summation are decided and reviewable on their own, and freezing them behind an undecided classification table would cost FIF-067 and FIF-025 nothing they do not already wait for.
* **Absorbed into existing items,** each because it is an acceptance clause of a rule that item already owns, not an increment of its own: DOM-120 → FIF-017 (the reader constructs the stored raw content; FIF-005, which owns the type, is `done`), DOM-121 → FIF-030 (the outstanding count is what keeps DOM-117 from being a silent understatement), IMP-SAXO-032 → FIF-021, TST-028 and TST-029 → FIF-003, TST-030 → FIF-019 rather than FIF-003, because the divergence is the fixture writer's but the assertion is the reader's.
* **Restated, no item moved:** IMP-TR-023 (2025 descends by `datetime` within a date, it does not merely fail to sort) in FIF-027, IMP-TR-019 (the transfer ratio is an exact integer pair, as a split's is) in FIF-068, DOM-075 in FIF-030.
* **Newly blocked:** none. **Unblocked:** none. `open-questions.md` names the same thirty ids across OQ-001 to OQ-013; OQ-008 gained a live instance in its prose — the `Overige Corporate Action` row on Eco Wave Power, worth about one euro — but blocks the same requirement.
* **Completed:** none. FIF-003 is implemented in the working tree and **uncommitted**: `tools/anonymize-exports` with 32 unit tests, 24 fixture-structure tests, and nine fixture files. `cargo test --workspace` passes. It stays `todo` on the same rule revision 5 applied to FIF-055 — uncommitted is unreviewed — and it is again the item to build. The note on it says what is outstanding: review, cite the two new TST ids, commit. Not a rebuild.

**Revision 6.** A status reconciliation, not a replan. `design/` is unchanged: the same
319 live requirement identifiers, and `open-questions.md` still names the same thirty blocked ids
across OQ-001 to OQ-013. No item was added, split, re-scoped or renumbered, and no dependency moved.

* **Completed:** FIF-055 (`12790ea`), the item revision 5 held at `todo` because the code was uncommitted. Verified against `crates/fifolio-core/src/quotation.rs` rather than the commit message. `cargo test --workspace` passes, 60 unit tests in core.
* **One acceptance clause shipped without a test.** DOM-039 (prices stored as the statement shows them) is documented in `quotation.rs` and carried by the FIF-004 types, but nothing in this item can observe it. The item is `done` and the gap is written on it; FIF-021 and FIF-026 own the assertion.
* **Newly blocked:** none. **Unblocked:** none. The twenty-one blocked items are the twenty-one of revision 5.
* Next to build: FIF-003, the anonymized broker fixtures. It has been the first unblocked `todo` since revision 1 and every importer item waits on it.

**Revision 5.** A status reconciliation against the code and `git log`, not a replan. No
item was re-cut, renumbered or re-scoped, and no acceptance criterion changed.

* **Completed, each verified against `crates/fifolio-core/src/` rather than against the commit message:** FIF-002 (`90c9b9a`, already `done` in revision 4), FIF-004 (`1eec01a`), FIF-054 (`e38de9d`), FIF-005 (`9846b5a`), FIF-006 (`17cb593`), FIF-007 (`fdf8990`). Every one meets its acceptance; what each shipped is recorded on the item. `cargo test --workspace` passes, 63 unit tests in core.
* **Newly blocked:** FIF-066 (Saxo ordering columns), by OQ-013, which is new in `design/open-questions.md`. It blocks IMP-SAXO-026. Blocked whole rather than split, because this run does not re-cut items; IMP-SAXO-027 rides along with it.
* **Unblocked:** none. Every requirement blocked in revision 4 is still named on a `Blocks:` line, so the twenty items blocked then remain blocked; OQ-013 makes twenty-one.
* **Not marked done:** FIF-055. `crates/fifolio-core/src/quotation.rs` implements it and `lib.rs` declares the module, but the work is uncommitted and unreviewed, so the item stays `todo`.

Revision 4's closing sentence — that no item had been implemented since revision 1 — is superseded
by this entry rather than edited.

**Revision 4.** `design/` gained `open-questions.md`, which replaces `spec-auditor`'s
regenerated list as the authority on what is undecided, precisely because that list churned between
revisions 2 and 3 and split items on a boundary that then moved. The twenty-nine ids its `Blocks:`
lines name are **identical, id for id, to the set revision 3 was built against**, so no item changed
status, no item was split, none was merged back and no dependency moved. The churn the revision-3
note warned about has stopped by construction: the list now changes only when a person changes it.

What changed in this file: the definition of `blocked` at the top now cites `open-questions.md`; the
Decisions required section is regrouped by question id (OQ-001 to OQ-012) rather than by theme, so a
closed question maps straight onto the items it releases; coverage was re-verified mechanically.

No item has been implemented since revision 1. `fifolio-core` is still a doc comment and both
binaries are still stubs, so FIF-001 remains the only `done` item and FIF-002 is the next to build.

**Revision 3.** `design/` is unchanged: the same 319 live requirement identifiers, the
same text. What changed is `spec-auditor`'s undecided list, and it changed almost wholesale — 27 of
the 41 ids blocked in revision 2 are now decided, and 15 ids that were decided are now blocked. No
item was implemented between the revisions, so nothing completed was invalidated; FIF-001 is still
the only `done` item.

* Unblocked, nothing else changed: FIF-054, FIF-055, FIF-057, FIF-060, FIF-016, FIF-064, FIF-065, FIF-021, FIF-022, FIF-025, FIF-067, FIF-027, FIF-068, FIF-030, FIF-051, FIF-070.
* Newly blocked whole, having no decided remainder to split off: **FIF-023** (IMP-SAXO-013 is the classification table and is its only requirement). This is the costly one: FIF-024, FIF-025 and FIF-067 sit behind it, and so does the Saxo half of FIF-035.
* Newly blocked after a split, the decided half keeping the old id: FIF-004 → **FIF-075** (quotation factor), FIF-006 → **FIF-076** (canonical order and the transaction order key), FIF-008 → **FIF-077** (stored gross governs), FIF-012 → **FIF-078** (allocation quantity invariants), FIF-013 → **FIF-079** (expiration quantity), FIF-014 → **FIF-080** (transfer out has no gain), FIF-017 → **FIF-081** (nothing created from nothing), FIF-031 → **FIF-082** (per-disposal detail lines), FIF-019 → **FIF-083** (newest-first direction), FIF-020 → **FIF-084** (corporate action row identity), FIF-035 → **FIF-085** (response summary counts), FIF-036 → **FIF-086** (batch deletion removes its records), FIF-038 → **FIF-087** (no endpoint creates from nothing).
* FIF-063 keeps four undecided ids and stays blocked whole; DOM-115 and DOM-116 became decided but qualify rules that did not, so splitting them out would produce an item no reviewer could judge.
* Dependency edits following the splits: FIF-013, FIF-061 and FIF-015 now depend on FIF-076 rather than FIF-006; FIF-012 no longer depends on FIF-061, the arithmetic having moved to FIF-078; FIF-014 gains FIF-054.

The volatility is itself worth recording. Two consecutive revisions have each re-drawn most of the
undecided list, and items have been split on a boundary that moved the next run. The splits are
cheap to undo — a split item states which id it came out of — but a third run with this much churn
would be evidence that the specification, not the plan, is what needs settling.

**Revision 2.** `design/` grew from 244 to 319 live requirement identifiers and retired
nine. The changes that invalidate earlier planning, rather than merely adding to it:

* The corporate-action entity is gone; transactions are a six-variant sum type. FIF-018 retired, FIF-056, FIF-061 and FIF-063 added.
* Manual information is a separate entity, not a `manual` source record kind (DEC-038). FIF-059, FIF-062 and FIF-072 added; FIF-017, FIF-037 and FIF-047 re-scoped.
* Ordering is computed from file content at read time and stored, not from a per-account import sequence (DEC-039, DEC-045). FIF-006 rewritten, DOM-041 retired, FIF-066 added.
* The EUR gross total is stored and governs every calculation; the stored rate is foreign units per EUR and is informational (DEC-027, DEC-028, DEC-037). FIF-008 rewritten.
* Saxo's EUR derivation is sign-aware (DEC-035); the previous single formula understated every disposal by twice the fee. FIF-021 corrected.
* A transferred parcel's acquisition date is now fixed at import, reversing DEC-011 (DEC-040). FIF-024 corrected.
* Coverage thresholds gate changed lines (DEC-023). FIF-001 gains TST-025 to TST-027; CI already implements them.
* Decisions D1 (Trade Republic corporate action types) and D2 (`export-manual-information` format) are closed by DEC-019 and DEC-020. FIF-029 and FIF-047 unblocked; FIF-068 and FIF-074 added.
* Items split so that a decided increment is not frozen behind an undecided requirement: FIF-054 out of FIF-004, FIF-055 out of FIF-005, FIF-057 and FIF-058 out of FIF-056, FIF-060 out of FIF-012 and FIF-015, FIF-064 out of FIF-017, FIF-067 out of FIF-023, FIF-068 out of FIF-029, FIF-069 out of FIF-027, FIF-070 out of FIF-035, FIF-071 out of FIF-036, FIF-073 out of FIF-038.

No item has been implemented since revision 1, so no completed work was invalidated. FIF-001
remains the only `done` item and its acceptance still holds.

**Revision 1.** Initial plan, FIF-001 to FIF-053.

# Decisions required

**Thirty-two** requirements are named on the `Blocks:` lines of `design/open-questions.md`, and the
**twenty-three** items carrying them are `blocked`. Revision 8 raised both: OQ-014, OQ-015 and
OQ-016 are new, and only OQ-015 names requirements no question named before, IMP-003 and SRV-056.
This plan does not resolve any of them; closing a question is a change to `design/`, not to
`PLAN.md`. Mapped from the open question to the owning item:

* **OQ-001** a transaction's `order` when it consumes no record, or several — DOM-011 (FIF-076), DOM-101 (FIF-058), DOM-091 (FIF-063)
* **OQ-002** an emitted `transfer_in` has no source record — DOM-090 (FIF-063), DOM-013 (FIF-076), DOM-047 (FIF-081), SRV-030 (FIF-073), SRV-034 (FIF-087)
* **OQ-003** whether a transfer carries the buy fee — DOM-106, DOM-107 (FIF-063), DOM-112 (FIF-080)
* **OQ-004** exact-rational quantities against 8-decimal allocations — DOM-064, DOM-065 (FIF-078), DOM-113 (FIF-061)
* **OQ-005** when an expiration of nothing is detected — DOM-092, DOM-114 (FIF-079), IMP-SAXO-013 (FIF-023)
* **OQ-006** a Trade Republic bond — IMP-TR-020, IMP-TR-021 (FIF-069)
* **OQ-007** ownership under re-import — DOM-111 (FIF-076), SRV-052 (FIF-071), SRV-021 (FIF-086)
* **OQ-008** a Saxo `Acties` value outside the table — IMP-SAXO-013 (FIF-023)
* **OQ-009** the direction of the corporate-action ordinal — IMP-SAXO-008 (FIF-084), IMP-SAXO-025 (FIF-083)
* **OQ-010** `quantity × unit_price × factor` is never invoked — ARC-008, DOM-038 (FIF-075), DOM-104 (FIF-077)
* **OQ-011** a `transfer_out` row in the acquisition report — DOM-078 (FIF-082), DOM-112 (FIF-080)
* **OQ-012** what a failed row does to its import — SRV-017 (FIF-085)
* **OQ-013** where a row with no ordering column sorts — IMP-SAXO-026 (FIF-066)
* **OQ-014** a stock dividend's cost basis — IMP-SAXO-013 (FIF-023)
* **OQ-015** verifying the account id against a Trade Republic file — IMP-003 (FIF-089), SRV-056 (FIF-090)
* **OQ-016** whether a transfer's own fee is basis or fees — DOM-106 (FIF-063), DOM-112 (FIF-080)

Several questions reach one item from different directions, and every one of them must close before
that item unblocks: FIF-023 by OQ-005, OQ-008 and OQ-014; FIF-080 by OQ-003, OQ-011 and OQ-016;
FIF-063 by OQ-001, OQ-002, OQ-003 and OQ-016; FIF-089 and FIF-090 by OQ-015 alone.

OQ-014 and OQ-015 are worth singling out because neither can be answered by reading the
specification harder. OQ-014 needs a figure that is in no column of the export and in no Saxo
report; OQ-015 needs a rule for a format that carries no account identifier at all. Both are
questions for a person, and OQ-014 governs the largest block of manual work in the sample.

D1 and D2, raised in revision 1, are closed; see the revision history. The twenty-seven ids blocked
in revision 2 and decided in revision 3 are listed there too.

# Requirement coverage

Re-verified again in revision 24 by the same script, against an unchanged `design/`: **333** ids,
each named by exactly one item, none uncovered, none deferred; the twenty-three items carrying one
of the thirty-two blocked ids are `blocked` and no others are.

Re-verified mechanically in revision 23 by extracting the identifiers from `design/*.md` and
comparing them against the `Requirements:` lines of this file: those lines name **333** distinct
ids, each exactly once, and they are precisely the live ids in `design/` less the nine retired ones,
which are assigned to nothing. `design/` gained no identifier since `d160fee`. Every item carrying
one of the thirty-two ids on a `Blocks:` line of `open-questions.md` is `blocked` and no other item
is — twenty-three items of ninety-one, checked by script. No placement changed. Nothing is
uncovered and nothing is deferred.

Re-verified mechanically in revision 21 by extracting the identifiers from `design/*.md` and
comparing them against the `Requirements:` lines of this file: those lines name **333** distinct
ids, each exactly once, and they are precisely the live ids in `design/` less the nine retired ones,
which are assigned to nothing. `d160fee` added a rule but no identifier, citing the existing
`ARC-026`; the rule is covered as an acceptance clause of FIF-010 and ARC-026 itself stays with
FIF-043, where the sentence it originally labelled lives. Every item carrying one of the thirty-two
ids on a `Blocks:` line of `open-questions.md` is `blocked` and no other item is — twenty-three
items of ninety-one, checked by script. No placement changed. Nothing is uncovered and nothing is
deferred.

Re-verified mechanically in revision 20 by extracting the identifiers from `design/*.md` and
comparing them against the `Requirements:` lines of this file: those lines name **333** distinct
ids, each exactly once, and they are precisely the live ids in `design/` less the nine retired ones,
which are assigned to nothing. `design/` gained no identifier since `72abea7`. Every item carrying
one of the thirty-two ids on a `Blocks:` line of `open-questions.md` is `blocked` and no other item
is — twenty-three items of ninety-one, checked by script. No placement changed. Nothing is uncovered
and nothing is deferred.

Re-verified mechanically in revision 19 by extracting the identifiers from `design/*.md` and
comparing them against the `Requirements:` lines of this file: those lines name **333** distinct
ids, each exactly once, and they are precisely the live ids in `design/` less the nine retired ones,
which are assigned to nothing. `72abea7` restated ARC-010 and added DEC-067 but introduced no
identifier. Every item carrying one of the thirty-two ids on a `Blocks:` line of
`open-questions.md` is `blocked` and no other item is — twenty-three items, checked by script.
No placement changed. Nothing is uncovered and nothing is deferred.

All **333** live requirement identifiers in `design/` are assigned to exactly one item. Nothing is
uncovered and nothing is deliberately deferred. The nine retired identifiers — DOM-009, DOM-014,
DOM-015, DOM-021, DOM-041, DOM-050, DOM-051, DOM-052, DOM-053 — are assigned to nothing, by design.

Coverage worth calling out because the owning item is not the obvious one:

* DOM-003 (per-currency sub-accounts normalize onto one Depot) sits in FIF-020 with the Saxo suffix rule, because that is the only place it is observable.
* DOM-002 and DOM-046 are both owned by FIF-017, which is where non-position rows are recognized and discarded.
* DOM-080 (Altbestand is lost on a transferred parcel) is owned by FIF-024, because `Deponering` is what creates the fixed date.
* DOM-110 (an import undo never deletes a manual entry) sits with the other storage invariants in FIF-012, while the behavior it protects is FIF-062.
* TST-002 enumerates what core must unit test; FIF-002 owns the convention, and the rules it lists are asserted inside the items that implement them.
* ARC-023 (spreadsheet reader plus CSV reader) is owned by FIF-017, where the reader abstraction lives.
* IMP-001 and IMP-002 (the calendar-year guard) are in FIF-065 and IMP-003 (the account guard) in FIF-089 as of revision 8. Revision 2 kept the three together on the ground that one refusal rule reads the same rows twice; OQ-015 makes them separable in practice, since the year guard reads trade dates every format has and the account guard reads an identifier one format does not carry at all.
* DOM-011 and DOM-013 are one sentence of `domain.md` and are owned jointly by FIF-076, not by FIF-006 and FIF-056 separately.

Verified mechanically this revision: every `Requirements:` line taken together names 319 distinct
ids, each exactly once, and they are precisely the live ids in `design/`. Verified alongside it that
every item carrying an id named on a `Blocks:` line of `design/open-questions.md` is `blocked`, and
that no other item is — twenty items, twenty-nine ids.

Re-verified in revision 5 against the same lists: thirty ids, twenty-one items, the additions being
IMP-SAXO-026 and FIF-066. Coverage is otherwise unchanged; `design/` gained no requirement
identifier this revision, only the open question.

Re-verified mechanically in revision 6: the `Requirements:` lines name 319 distinct ids, each
exactly once, and they are precisely the live ids in `design/`; the nine unassigned ids are the
retired ones. Every item carrying one of the thirty ids on a `Blocks:` line is `blocked` and no
other item is — twenty-one items, unchanged.

Re-verified mechanically in revision 7 against the grown specification: 328 distinct ids, each named
exactly once, precisely the live ids in `design/`; the same nine retired ids are assigned to nothing;
the same twenty-one items are `blocked` and no other is. Two more placements worth calling out:

* DOM-120 (a spreadsheet row's canonical rendering) is owned by FIF-017, where the reader abstraction lives, not by FIF-005, which owns the `SourceRecord` type and is `done`.
* TST-030 (a blank Saxo cell round-trips as an empty cell) is owned by FIF-019, the reader that must accept both shapes, not by FIF-003, whose writer causes the divergence.

Re-verified mechanically in revision 18 by extracting the identifiers from `design/*.md` and
comparing them against the `Requirements:` lines of this file: those lines name **333** distinct
ids, each exactly once, and they are precisely the live ids in `design/` less the nine retired ones,
which are assigned to nothing. `design/` gained no identifier since revision 14. Every item carrying
one of the thirty-two ids on a `Blocks:` line of `open-questions.md` is `blocked` and no other item
is — twenty-three items, unchanged. No placement changed. Nothing is uncovered and nothing is
deferred.

Re-verified mechanically in revision 16: the `Requirements:` lines name 333 distinct ids, each
exactly once, and they are precisely the live ids in `design/` less the nine retired ones, which are
assigned to nothing; every item carrying one of the thirty-two ids on a `Blocks:` line is `blocked`
and no other item is — twenty-three items, unchanged. DOM-084's move from FIF-008 to FIF-014 in
revision 15 is the only placement that changed since revision 14.

Re-verified mechanically in revision 8: the `Requirements:` lines name 328 distinct ids, each
exactly once, and they are precisely the live ids in `design/`, the nine retired ones assigned to
nothing. The two splits of this revision moved one id each and added no coverage: IMP-003 from
FIF-065 to FIF-089, SRV-056 from FIF-070 to FIF-090. Every item carrying one of the thirty-two ids
on a `Blocks:` line is `blocked` and no other item is — twenty-three items.

Re-verified mechanically in revision 10: the `Requirements:` lines name 328 distinct ids, each
exactly once, and they are precisely the live ids in `design/`, the nine retired ones assigned to
nothing. Every item carrying one of the thirty-two ids on a `Blocks:` line is `blocked` and no other
item is — twenty-three items. Nothing is uncovered and nothing is deferred.

Re-verified mechanically in revision 11: unchanged from revision 10 — the `Requirements:` lines name
328 distinct ids, each exactly once, and they are precisely the live ids in `design/`. Every item
carrying one of the thirty-two ids on a `Blocks:` line is `blocked` and no other item is — twenty-three
items. Nothing is uncovered and nothing is deferred.

Re-verified mechanically in revision 12 against a specification that had grown: the `Requirements:`
lines name **329** distinct ids, each exactly once, and they are precisely the live ids in
`design/`, the nine retired ones assigned to nothing. The single addition is DOM-122, on FIF-059.
Revisions 10 and 11 reported 328 and claimed `design/` unchanged since `8dbca69`; that was wrong —
`9d8c9b9` had already landed — and DOM-122 was uncovered for two revisions. The check now reads the
identifiers out of `design/` rather than trusting the previous revision's count. Every item carrying
one of the thirty-two ids on a `Blocks:` line is `blocked` and no other item is — twenty-three
items. Nothing is uncovered and nothing is deferred.

Re-verified mechanically in revision 13 by extracting the identifiers from `design/*.md` and
comparing them against the `Requirements:` lines of this file: those lines name **330** distinct
ids, each exactly once, and they are precisely the live ids in `design/`; the nine retired ids are
assigned to nothing. The single addition since revision 12 is **DOM-123**, on the new FIF-091.
Every item carrying one of the thirty-two ids on a `Blocks:` line is `blocked` and no other item is
— twenty-three items, unchanged. Nothing is uncovered and nothing is deferred.

Re-verified mechanically in revision 14 against a specification that had grown again: the
`Requirements:` lines name **333** distinct ids, each exactly once, and they are precisely the live
ids in `design/` — the check reads the ids out of `design/` and subtracts the nine retired ones
`domain.md` lists, rather than trusting the previous revision's count. The three additions are
DOM-124 on FIF-017, DOM-125 on FIF-014 and IMP-SAXO-036 on FIF-066; the one move is DOM-097 from
FIF-059 to FIF-092, for the reason that item states. Every item carrying one of the thirty-two ids
on a `Blocks:` line of `open-questions.md` is `blocked` and no other item is — twenty-three items.
Nothing is uncovered and nothing is deferred.
