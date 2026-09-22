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
Status: todo
Requirements: DOM-082
Depends on: FIF-056
Acceptance: a `buy` records how it arose — an ordinary purchase, or shares issued as a stock dividend whose cost basis is their taxable value at issue — and the taxable value is sourced rather than invented.
Notes: Split out of FIF-056 in revision 2 and blocked then; DOM-082 left the undecided list in revision 3. FIF-025 is the Saxo dividend heuristic that produces these buys.
Confirmed unblocked in revision 10: OQ-014 asks where a `Herbeleggingsdividend`'s share count and price come from, but it blocks IMP-SAXO-013 only, so it freezes the Saxo importer (FIF-023, FIF-025) and not this type. Build the origin so that the taxable value arrives from a caller and is never computed here; that is what keeps DOM-082 reviewable while OQ-014 is open, and it is why the item sits before the importer that fills it.
Its dependency FIF-056 is `done` (`74e4bd1`, recorded `5d54c2b`); `transaction.rs` already names this item at the absent field, so the increment is a variant field on `Buy` plus its unit tests, not a new module.

## FIF-058 Consumption versus citation
Status: blocked
Requirements: DOM-101
Depends on: FIF-056
Acceptance: a transaction consumes the source records its existence answers and may cite further records without consuming them; consumption is what clears a record from the completion queue, citation is what preserves the audit trail; a decomposition names which of its transactions consumes.
Blocked by: DOM-101 is on the undecided list.
Notes: Split out of FIF-056. DOM-070 (at most one consumer) is in FIF-060 and blocked alongside it; SRV-022 and DOM-119 (batch deletion refused on a citation the batch did not derive) are in FIF-012 and FIF-036, which can only be finished once this is settled.

## FIF-059 Manual entry entity
Status: todo
Requirements: DOM-097, DOM-098, DOM-099, DOM-100
Depends on: FIF-005
Acceptance: a `ManualEntry` type separate from `SourceRecord`, holding account, security, what the user supplied, and **the broker identities of the source records it answers** rather than internal keys, so it survives their deletion; the separation is structural — nothing in the type system lets an import undo remove one.
Notes: New in this revision; the previous plan modeled manual information as a `manual` source record kind, which DEC-038 reversed.

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

## FIF-007 Source record identity and idempotency
Status: done
Requirements: DOM-022, DOM-023, DOM-024
Depends on: FIF-005
Acceptance: an identity abstraction that takes either a broker reference or a hash of the parsed business fields, always scoped to the account; identical rows in two accounts produce distinct identities; re-importing the same rows into the same account produces no new source records. Format-specific identity rules live in FIF-020 and FIF-027.
Done in revision 5, commit `fdf8990`. `identity.rs` offers `IdentitySource::BrokerReference` / `ParsedFields`, both scoped to the account, with a length-prefixed framing tested against flattening collisions and against a reference aliasing a hash.

## FIF-008 EUR valuation and the stored gross
Status: todo
Requirements: DOM-025, DOM-026, DOM-027, DOM-028, DOM-029, DOM-084, DOM-085, DOM-086
Depends on: FIF-004, FIF-056
Acceptance: every transaction stores native figures and EUR figures at the same scales, together with the rate, its source and its date; the valuation date is the trade date, never settlement; no separate currency-gain figure exists anywhere in the model; **the EUR gross total is stored as well as the unit price**; the stored rate is foreign units per EUR (`EUR = native / rate`), and a format quoting the inverse converts at full precision from the figures the file states, never from the rounded stored rate.
Notes: DOM-085 (DEC-028) and DOM-086 (DEC-027) were new in revision 2 and change the shape of every transaction record; this is why the item sits before storage rather than beside it. Revision 3 splits DOM-104 — that the stored gross, not the unit price, is what every calculation reads — into FIF-077, because it is undecided.

## FIF-077 The stored gross governs every calculation
Status: blocked
Requirements: DOM-104
Depends on: FIF-008
Acceptance: allocation shares are taken pro-rata from the booked EUR gross total and never rebuilt from the unit price; the unit price exists for display and reconciliation against the statement and nothing computes with it, enforced so that a caller cannot reach for it by accident.
Blocked by: DOM-104 is on the undecided list.
Notes: Split out of FIF-008 in revision 3. FIF-014's formulas read whichever figure this settles, so the two must be reviewed together once it is decided.

## FIF-009 FX rate resolution
Status: todo
Requirements: DOM-030, DOM-031, DOM-032, DOM-033, DOM-034, DOM-035, ARC-019, ARC-027, TST-019, TST-020, TST-021
Depends on: FIF-008
Acceptance: rate source precedence `broker` > `ecb` > `native`; broker-stated EUR figures used verbatim with the implied quotient stored as the rate and marked informational, since nothing computes with it; EUR-denominated transactions get rate 1 and source `native`; an ECB lookup with no rate for the trade date falls back to the most recent published rate before it and stores that rate's own date; the fallback is bounded — nothing before the series begins in 1999, and a substitution more than seven days stale is an error; fees convert at the leg's rate; a missing, unfetchable rate fails with an error naming currency and date. The rate source is an injected trait; unit tests use a fake table with weekend and holiday gaps. No test opens a socket.

## FIF-010 ECB rate cache and seeding
Status: todo
Requirements: ARC-015, ARC-016, ARC-017, ARC-018
Depends on: FIF-009, FIF-011
Acceptance: a rate table keyed by currency and date; a seeding path that ingests the ECB full historical series (1999 onward) and a top-up path for the rolling 90-day feed; once seeded, imports resolve rates with no outbound call. Seeding is tested against a committed recorded fragment of the series, never a live fetch.

## FIF-011 SQLite storage and migrations
Status: todo
Requirements: ARC-011, ARC-012, ARC-013, ARC-014, DOM-071, TST-004
Depends on: FIF-005, FIF-056, FIF-059
Acceptance: one SQLite file, default `./fifolio.db`, created on first run and overridable; versioned `sqlx` migrations applied on startup; repositories for every entity, the manual entry included; a unique constraint on security ISIN; integration tests against a real temporary database covering migration from empty and the ISIN uniqueness failure.

## FIF-012 Storage-enforced invariants
Status: todo
Requirements: DOM-066, DOM-068, DOM-069, DOM-072, DOM-094, DOM-110, DOM-119
Depends on: FIF-011
Acceptance: each invariant is refused at the persistence/service boundary with a distinguishable error: a closing may only be attributed if every earlier closing of the same account and security is attributed; an attribution may only be deleted if no later attribution exists for that account and security; a transaction in an attribution cannot be edited, re-rated or deleted; a `transfer_in` emitted by a `transfer_out` cannot be deleted independently of it; a manual entry is never deleted by an import undo; a batch may only be deleted if no transaction derived from it participates in an attribution, and if no record it owns is cited by a transaction the batch did not derive, the refusal naming those transactions. Integration tested against a real temporary database, one test per invariant.
Notes: DOM-049, DOM-067 and DOM-070 moved to FIF-060 in revision 2. Revision 3 moves the two quantity invariants, DOM-064 and DOM-065, to FIF-078; what remains here is the lifecycle set — ordering of attribution, immutability, deletion refusals — which is decided and needs no effective-quantity arithmetic, so this item no longer depends on FIF-061.

## FIF-078 Allocation quantity invariants
Status: blocked
Requirements: DOM-064, DOM-065
Depends on: FIF-012, FIF-061
Acceptance: allocated quantities against an opening, each scaled to a common position in the canonical order, never exceed its effective quantity at that position, and a closing's allocations sum exactly to its quantity; both refused at the persistence/service boundary with a distinguishable error, integration tested against a real temporary database.
Blocked by: DOM-064 and DOM-065 are on the undecided list.
Notes: Split out of FIF-012 in revision 3. DOM-064 compares effective quantities at a common position, which is why this half, and not FIF-012, carries the FIF-061 dependency.

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
Requirements: DOM-058, DOM-059, DOM-060, DOM-061, DOM-062, DOM-063, DOM-093, DOM-105
Depends on: FIF-013, FIF-061, FIF-004, FIF-054
Acceptance: allocated cost, buy fee, proceeds, sell fee and gain computed on demand from the parent transactions, exactly as the formulas in `domain.md` state, with the opening side divided by its effective quantity **as of the closing** and the closing side by the closing's own quantity; every closing variant carries `eur_gross` and `eur_fees` so one formula reads them all, and `split` carries neither; each share rounded to 2 decimals independently with drift absorbed by the last share, opening-side last being the allocation that exhausts the parcel and closing-side last being the last allocation of that closing in canonical order; sell fees never spread beyond their own closing. Unit tests include a division that is exact, one leaving one cent, and one leaving many.
Notes: Blocked in revision 2 on DOM-059, DOM-105 and DOM-112; the first two are now decided. DOM-112 (a `transfer_out`'s `eur_gross` derived from its own allocations) moved to FIF-080 in revision 3, so this item covers the cash closings only.

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
Requirements: DOM-002, DOM-042, DOM-044, DOM-045, DOM-046, DOM-048, DOM-120, ARC-023
Depends on: FIF-007, FIF-011
Acceptance: an importer trait over a source file that yields source records; rows where every field the variant needs is present and unambiguous are derived automatically; rows that affect holdings but lack something only the user knows become pending, which is the completion queue; dividends, interest, deposits, withdrawals and account fees are recognized as non-position, counted and not stored; everything the user supplies becomes a manual entry. XLSX reading via `calamine` and CSV via `csv` sit behind the same reader abstraction; a delimited row stores its verbatim line while a spreadsheet row, having none, stores the canonical rendering `domain.md` defines — each cell as the file holds it, an Excel serial date staying `45208`, keyed by column name in sheet column order — so that re-reading the same file reproduces the same string.
Notes: DOM-120 is new in revision 7 (DEC-057) and sits here because the reader abstraction is what constructs the stored raw content; FIF-005, which owns the `SourceRecord` type, is `done` and its field is untyped as to how it was rendered. DOM-043, the classification taxonomy these three outcomes belong to, moved to FIF-064 in revision 2. Revision 3 moves DOM-047, "nothing is invented", to FIF-081, it being undecided; the three outcomes stand without it.

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
Requirements: IMP-SAXO-026, IMP-SAXO-027
Depends on: FIF-019, FIF-006
Acceptance: rows are ordered on `Transactiedatum`, then the first populated of `Bk Record Id`, `Booking Id` and `Transactie-ID`, then file position taken in reverse; `Corporate action-Id` is never an ordering column, because it is not monotonic with date. Tested on a fixture date carrying several rows where only that id is populated, so file position settles them.
Blocked by: IMP-SAXO-026 is on the undecided list (OQ-013).
Notes: New in revision 2; ordering is now computed from file content (DOM-040). Newly blocked in
revision 5: OQ-013 leaves undetermined where a row carrying none of the three booking ids sorts,
and those rows are precisely Saxo's corporate actions. IMP-SAXO-027 (`Corporate action-Id` is never
an ordering column) is decided, but it is one clause of the same ordering key and was not split out;
the mechanism it binds to, `ordering.rs` from FIF-006, is unaffected and already built.

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

All 328 live requirement identifiers in `design/` are assigned to exactly one item. Nothing is
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

Re-verified mechanically in revision 8: the `Requirements:` lines name 328 distinct ids, each
exactly once, and they are precisely the live ids in `design/`, the nine retired ones assigned to
nothing. The two splits of this revision moved one id each and added no coverage: IMP-003 from
FIF-065 to FIF-089, SRV-056 from FIF-070 to FIF-090. Every item carrying one of the thirty-two ids
on a `Blocks:` line is `blocked` and no other item is — twenty-three items.

Re-verified mechanically in revision 10: the `Requirements:` lines name 328 distinct ids, each
exactly once, and they are precisely the live ids in `design/`, the nine retired ones assigned to
nothing. Every item carrying one of the thirty-two ids on a `Blocks:` line is `blocked` and no other
item is — twenty-three items. Nothing is uncovered and nothing is deferred.
