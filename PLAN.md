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
**Largely invalidated on the Saxo side in revision 29 by DEC-070 (`c04362c`).** The real workbook has
**three** sheets and the anonymizer reproduces one, so the five committed Saxo fixtures are not
fixtures of the file: `_Transacties` (the position side) and `Bookings` (the components of each cash
movement) are absent, and they are where the quantities, the traded values and the tax figures live.
The acceptance above is left as written, including "one sheet", so what was built and on what
reading stays legible; the correction is **FIF-093**, which carries the new TST-031. The Trade
Republic half is untouched, as is everything about identity substitution and amount perturbation —
FIF-093 extends that machinery to two more sheets rather than replacing it.

## FIF-093 Saxo fixtures carry all three sheets
Status: done
Requirements: TST-031
Depends on: FIF-003
Acceptance: the anonymizer reads and writes `Transacties`, `_Transacties` and `Bookings`, each with
its own header row and row count (31, 24 and 21 columns; 188, 33 and 242 rows in the sample), and the
committed Saxo fixtures are regenerated from `design/example_exports/` with all three sheets present;
the join keys survive anonymization intact and are asserted to — `Transactie-ID` and
`Corporate action-Id` between `Transacties` and `_Transacties`, `Bk Record Id` / `Booking Id` /
`Corporate action-Id` between `Transacties` and `Bookings` — so a row that joined before joins after
and to the same counterpart; the structural properties the new sheets carry are asserted the way the
first sheet's already are, namely every position-affecting row having a `_Transacties` counterpart,
a corporate action's two legs under one `Corporate action-Id` distinguished by `Trade Event Type`,
and at least one `Bookings` row carrying a withholding percentage; identities are still substituted
and amounts still perturbed on the two new sheets, with quantities and dates untouched
[TST-012, TST-013, TST-028, TST-029]; generation stays byte-reproducible.
Notes: New in revision 29, carrying TST-031, the one identifier `testing.md` gained in `c04362c`.
**This item exists because completed work no longer matches the specification.** DEC-070 says
plainly that the fixtures are rebuilt and the anonymizer extended before any Saxo importer work
continues, so this item sits ahead of FIF-019 and is not optional: every Saxo rule from IMP-SAXO-028
onward now reads a column on a sheet the fixtures do not contain, and an importer tested against
them would pass while reading nothing.
It is a fixture item, not an importer item: nothing here derives a transaction or decides a
classification. What it must not do is infer the joins — they are stated in the table under "File
format" in `importers.md` [IMP-SAXO-037] and asserting them is this item's job, using them is
FIF-019's and FIF-020's.
The amount perturbation now has a cross-sheet obligation it did not have with one sheet: a booked
amount on `Transacties` and the components of that same movement on `Bookings` must move together,
or a fixture will show components that do not sum to their booking. The same holds for
`Verhandelde waarde` against the quantity and price on `_Transacties`. Whether the anonymizer
perturbs a group once or each figure independently is an implementation choice this acceptance
leaves to the implementer, but the sum must survive, because IMP-SAXO-034's group summation is
tested against these files.
Revision 30 finds this item **built in the working tree and uncommitted**: `tools/anonymize-exports`
is modified across `saxo.rs`, `perturb.rs`, `pseudonym.rs`, `lib.rs` and `tests/fixture_structure.rs`,
the five Saxo fixtures are regenerated and now carry `Transacties`, `_Transacties` and `Bookings`,
and `cargo test --workspace` gives **340 passed, 0 failed**, up from 315. It stays `todo` for the
reason revisions 13 to 15 kept FIF-008 at `todo`: completion is recorded against a commit, never
against a working tree, and uncommitted is unreviewed. The outstanding work is therefore review
against the acceptance above and a commit, not a rebuild.
What the review should check, since a reading of the tree suggests each is present and none has been
confirmed against the specification: the three headers (31 / 24 / 21) byte for byte; the join-key
assertions in `fixture_structure.rs` (`every_position_affecting_row_has_a_detail_counterpart`,
`every_bookings_row_joins_the_booking_it_decomposes`, `a_corporate_action_carries_both_its_legs_under_one_id`,
`a_bookings_row_carries_a_withholding_percentage`); the cross-sheet perturbation obligation
(`the_components_of_a_booking_still_sum_to_it`, `a_traded_value_is_still_its_quantity_at_its_price`);
quantities and dates untouched; and byte-reproducibility
(`a_written_fixture_reads_back_as_it_was_written_and_is_byte_reproducible`).
Done in revision 31, commit `7a0d64f`, which is the work revision 30 found in the tree. The tree is
clean, `cargo test --workspace` gives **344 passed, 0 failed** — four more than the uncommitted state
revision 30 measured — and each named assertion above exists and runs. The three sheets are present
in all five committed fixtures.

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
Status: todo
Requirements: DOM-101
Depends on: FIF-056
Acceptance: a transaction consumes the source records its existence answers and may cite further records without consuming them; consumption is what clears a record from the completion queue, citation is what preserves the audit trail; a decomposition names which of its transactions consumes.
Blocked by (until revision 69): DOM-101 is on the undecided list.
Revision 69: unblocked. OQ-001 is provisionally answered by DEC-090: of a decomposition the `sell` consumes and the `transfer_out` only cites. Outside the server scope the user set, so not selected this run.
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
Status: done
Requirements: DOM-011, DOM-013, DOM-111
Depends on: FIF-006, FIF-056
Acceptance: the canonical order over an account and security is (trade date, `order`, batch age), so two records from different files sharing a date are settled by the age of the owning batch; every transaction variant carries the `order` of the source record it consumes and relations to account, security and one or more source records.
Blocked by (until revision 69): DOM-011 (OQ-001) and DOM-111 (OQ-007) are on the undecided list. DOM-013 left it in revision 47 (OQ-002, DEC-079) but stays here: it is one sentence of `domain.md` with DOM-011.
Revision 69: **unblocked** by the provisional DEC-090 and DEC-092, and the acceptance is amended accordingly, not rewritten: (a) a transaction consuming several records takes the **lowest** of their orders; (b) of a decomposition's two legs the `sell` sorts first and the `transfer_out` immediately after it (the legs themselves are FIF-100's; this item provides the key that can express "immediately after"); (c) the third key is the age of the **oldest** batch that supplied the record, not the owning batch, and it never changes when ownership moves. Test: re-importing a year into a newer batch leaves the canonical order of every record unchanged. Named next to build in revision 69.
Notes: Split out of FIF-006 and FIF-056 in revision 3; DOM-011 and DOM-013 are one sentence of `domain.md` and belong together. Everything that reads the canonical order — FIF-013, FIF-061, FIF-014, FIF-015 — depends on this rather than on FIF-006 alone.
FIF-012 built DOM-066 and DOM-068 over the stand-in **(trade date, row id)**, the canonical order being unavailable, and DOM-072 over a single `derived_by_batch` per transaction rather than over the records a transaction was derived from. Both are this item's to replace: the two comparisons in `storage/attributions.rs` and their note, and `derived_transactions` in `storage/entities.rs`, which must be re-keyed on the transaction-to-source-record relation so DOM-072 answers for every batch whose records a transaction was derived from. Its acceptance therefore reads DOM-066, DOM-068 and DOM-072 through, and their integration tests move with the comparison.
Revision 69 note: `source_record` has a single `batch_id` today (migration `0003`), which is the first batch to insert the row because a re-import currently leaves it untouched. DEC-092 needs the oldest supplier fixed apart from the owner, since FIF-071 moves ownership to the newest supplier; record the first supplying batch as its own fact (set once at insert, or read from the supplier relation FIF-071 needs) rather than reading the owner. If DEC-090 or DEC-092 is overridden on ratification, this item reopens.
Done in revision 70: migration `0006_canonical_order.sql` adds a record's first supplying batch and a transaction's `order`, batch age and leg; the canonical order is (trade date, `order`, oldest supplier batch) and replaces FIF-012's stand-in in `storage/attributions.rs` and `derived_transactions`. Provisional DEC-094, DEC-095, DEC-096.

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
Revision 42 moved **DOM-086** to **FIF-097**: OQ-021 (`b9017d8`) now names it on a `Blocks:` line.
This item stays `done`: its commit implemented DOM-086 as then written, and whether that code stands
is recorded against FIF-097 rather than by reopening this item.
Revision 47 returns **DOM-086** here and retires FIF-097. DEC-073 closed OQ-021 without changing DOM-086's rule. A Trade Republic row that would be stored in a foreign currency now rejects the import (IMP-TR-017, FIF-029), so no Trade Republic `fx_rate` ever reaches the stored rate. The stored convention and Saxo's inversion, which this commit implemented, stand as written. DOM-086's aside "Trade Republic and the ECB quote it this way" no longer holds after 2024-07-02. That is a wording finding for `spec-auditor`, not a blocker. No behavior depends on it.

## FIF-098 Remove the unspecified re-rate operation
Status: done
Requirements: none (removal under DEC-084)
Depends on: none
Acceptance: `TransactionRepository::re_rate` and the tests that exercise it are removed; DOM-069's refusal of deletion on an attributed transaction stays tested; no other behavior changes; the suite is green.
Notes: Added in revision 46 by the user's decision. `re_rate` (storage/transactions.rs, from FIF-012 `3136a8a`) had no requirement and restated the rate without the EUR figures, breaking DOM-028. Its re-rating clause of DOM-069 now holds by construction, like the edit clause carried by FIF-087's note. Placed before FIF-034 so it is built next.
Revision 47 moves it to directly after FIF-008, unchanged in scope. Revision 47 unblocks eleven items, several of them earlier in document order than FIF-034 and ready, so the position revision 46 chose no longer made it next. After FIF-008 is also where it belongs. `re_rate` breaks DOM-028, which FIF-008 owns. This is completed work the specification has put in question, which is what this item is.
Done in revision 48, commit `4095318`: `re_rate` and its tests are removed from `storage/transactions.rs` and `tests/invariants.rs`. DOM-069's refusal to delete an attributed transaction is still tested.

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
Requirements: DOM-030, DOM-031, DOM-032, DOM-033, DOM-034, DOM-035, ARC-027, TST-019, TST-020, TST-021
Depends on: FIF-008
Acceptance: rate source precedence `broker` > `ecb` > `native`; broker-stated EUR figures used verbatim with the implied quotient stored as the rate and marked informational, since nothing computes with it; EUR-denominated transactions get rate 1 and source `native`; an ECB lookup with no rate for the trade date falls back to the most recent published rate before it and stores that rate's own date; the fallback is bounded — nothing before the series begins in 1999, and a substitution more than seven days stale is an error; fees convert at the leg's rate; a missing, unfetchable rate fails with an error naming currency and date. The rate source is an injected trait; unit tests use a fake table with weekend and holiday gaps. No test opens a socket.
Notes: Named next to build in revision 17, FIF-008 having landed in `9a1e457` and FIF-091 in `aa6aac5`. The working tree is clean and the suite green (173 tests), so this item starts from a committed base and is the first of the arithmetic items that needs no type of its own from an earlier item.
The rate table itself is **not** here: persistence, seeding and the top-up feed are FIF-010, which depends on this item and on storage. What this item owns is resolution — the precedence, the fallback and its bound, the error — behind an injected trait, so it is unit testable with a fake table and reaches no socket [TST-019, TST-020, TST-021]. `valuation.rs` (FIF-008) already carries `Conversion` (rate, source, date), which is the shape a resolved rate must fill; ARC-027's "EUR = native / rate" direction is settled there and must not be re-decided here.
Done in revision 17, commit `6384132`, in `crates/fifolio-core/src/fx.rs`. `resolve` answers all three sources behind the injected `RateTable` port: an EUR leg is tested first and takes rate 1 from `native`, a file-booked EUR figure takes the importer's quotient from `broker`, and a native-only leg looks the ECB rate up. The gap fallback stores the substituting publication's own date, and is bounded by `series_start()` and `MAX_SUBSTITUTION_DAYS`, so `BeforeSeries`, `Unavailable` and `StaleSubstitute` are three distinct errors and each names the currency and the date. `fee_in_eur` converts at the leg's own rate and refuses an informational one; `RateSource::is_informational` is that rule in mechanical form, and sits beside the enum in `valuation.rs`. Twelve unit tests run against a fake table built from the real 2024 Easter publication gap; none opens a socket.
**ARC-019 left this item in revision 29**, OQ-019 having been recorded in `409f76f`. Nothing shipped
is invalidated: the error naming currency and date exists as `Unavailable` and stays, and this item
keeps the three errors it built. What is undecided is whether an import may reach the network for a
rate it lacks, which is the "nor fetchable" half of the requirement and was never built here. It is
carried by **FIF-095** so that a `done` item is not turned `blocked` and its record erased.

## FIF-101 A date after the newest publication has no rate yet
Status: done
Requirements: none (correction of FIF-009's DOM-034 under DEC-085)
Depends on: FIF-009
Acceptance: resolving an ECB leg whose trade date lies after the newest publication the rate table holds fails with `RateError::Unavailable` naming the currency and the date, instead of substituting the newest rate. A gap day followed by a later publication still substitutes the preceding rate and stores that rate's own date, and the seven-day bound and `BeforeSeries` are unchanged. Unit tests against a fake table: a Saturday with the following Monday published substitutes Friday; the same Saturday with Friday as the newest publication is `Unavailable`; a weekday before its own publication is `Unavailable`. No test opens a socket.
Notes: New in revision 49. **This item exists because completed work no longer matches the specification.** `fca2736` added DEC-085 and amended DOM-034: a day counts as a non-publication day only once a later publication proves it was skipped. FIF-009 (`6384132`) substitutes for any date with nothing on or before it, within the bound, so a trade dated after the newest cached publication resolves to a guess. This is the case DEC-085 forbids. DOM-034 stays on FIF-009's `Requirements:` line, so each id is still carried once. This item carries the corrected clause, and FIF-009's record is left as built.
Where the rule is enforced is an implementation choice the specification does not constrain: in `fx::resolve`, which needs the port to expose the newest publication, or in the table handed to resolution. The uncommitted FIF-095 tree already makes `ecb::rates_for`'s `FetchedRates` answer nothing past the cache's coverage end. If that is kept, this item is reviewed against it rather than rebuilt, but it must still hold for every `RateTable` an import resolves against, not only the fetched one. DEC-085 rejects a weekend and TARGET holiday calendar, so none is added.
Done: enforced in `fx::resolve` through a new `RateTable::newest_publication`, so it holds for every table. The three acceptance tests are unit tests in `fx.rs`; `once_seeded_a_resolution_reads_the_cache_and_nothing_else` now tops up so a later publication proves the Easter Saturday was skipped.

## FIF-095 Fetching a rate an import does not have
Status: done
Requirements: ARC-019
Depends on: FIF-009, FIF-010, FIF-101
Acceptance: an import that needs a rate the cache does not hold fetches it itself. An empty cache is seeded from the full series (SRV-047). Otherwise the 90-day feed tops it up, or the full series does for a date the window no longer covers. The fetch only fills the cache and never rewrites a cached day (ARC-028). An import that still has no rate, offline for instance, fails with the `fx::RateError::Unavailable` FIF-009 already raises, naming the currency and date. Tested through the feed port with recorded documents; no test opens a socket (TST-019 to TST-021).
Notes: Split out of FIF-009 in revision 29, recorded on both halves. OQ-019 observes that no
requirement assigns the fetch: seeding and top-up are separate acts (FIF-010, FIF-041), and whether
an import reaches the network at all decides whether an import can fail for want of a connection —
and whether TST-019 to TST-021's "no test opens a socket" needs a port here at all. The error itself
is `fx::ResolveError::Unavailable` and is `done`; this item must not add a second one.
Unblocked in revision 47: DEC-078 closed OQ-019 and rewrote ARC-019. The seed and top-up paths FIF-010 built are what this item calls from the import path. It must not add a second fetch mechanism.
Revision 49 adds FIF-101 to its dependencies. Under DEC-085, an import offline with a trade dated after the cache must fail with `Unavailable` and must not substitute. That refusal is FIF-101's rule, and this item's offline clause relies on it.
ARC-019 is **not closed by this item**. What it supplies is `ecb::rates_for`, the fetch an import calls for the trade dates it must value, built on FIF-010's `seed` and `top_up` paths. No import values legs yet: `import::import` reads, orders and classifies rows, and the import endpoint FIF-035 is `todo`, so nothing in the workspace calls `rates_for` outside its tests. ARC-019 stays open until the item that values legs on import calls `rates_for`, with an integration test driving that import against a counting feed and an empty or stale cache; it must not be marked covered here. This is the treatment FIF-010 gave ARC-017 and ARC-018.
Done: `ecb::rates_for` fetches through FIF-010's `seed` and `top_up` only, keyed on the new `CachedRates::coverage`; a feed failure leaves the day out so resolution raises `RateError::Unavailable`. Tested in `tests/rate_cache.rs` against recorded documents; no test opens a socket.

## FIF-010 ECB rate cache and seeding
Status: done
Requirements: ARC-015, ARC-016, ARC-017, ARC-018, ARC-028
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
The identifier collision this item flagged in revisions 21 and 22 is **fixed in `design/`**, not
here: `409f76f` renumbered the top-up rule from the already-used `ARC-026` to **ARC-028**, which
revision 29 adds to the `Requirements:` line above. The rule was already implemented and already an
acceptance clause; the id now names it, so the clause and the id are one thing again. ARC-026 stays
with FIF-043, where the sentence it originally labelled lives.

## FIF-011 SQLite storage and migrations
Status: done
Requirements: ARC-011, ARC-012, ARC-013, ARC-014, ARC-029, DOM-071, TST-004
Depends on: FIF-005, FIF-056, FIF-059
Acceptance: one SQLite file, default `./fifolio.db`, created on first run and overridable; versioned `sqlx` migrations applied on startup; repositories for every entity, the manual entry included; a unique constraint on security ISIN; integration tests against a real temporary database covering migration from empty and the ISIN uniqueness failure. A repository **refuses** a figure carrying more decimals than its kind's scale allows, with a distinguishable error, rather than rounding it on the way in [ARC-010 as restated by DEC-067]; one integration test per scale asserts the refusal.
Notes: Named next to build in revision 18. Its three dependencies are `done` — FIF-005 (`9846b5a`), FIF-056 (`74e4bd1`) and FIF-059 (`4af8667`, widened by FIF-092 in `cd09f97`) — and none of its six requirements is on a `Blocks:` line. It is not the first `todo` in document order: FIF-010 sits ahead of it and waits on this item for the table it caches into, which is why storage comes first.
It is the first item to leave pure core arithmetic, and the first whose tests are integration-layer against a real temporary database rather than unit tests [TST-004]; `fifolio-test-support` (FIF-002) already provides the temporary-database helper, so the harness is not part of this increment.
What it persists is the types built so far and no more: `Account`, `Security`, `SourceRecord`, `ImportBatch` (FIF-005), the six `Transaction` variants with their `Valued` pairs and `Conversion` (FIF-056, FIF-008), and `ManualEntry` with its five `Supplied` shapes (FIF-059, FIF-092). Fields whose rules are still blocked must not be invented here: a transaction carries no relation to account, security or source record yet — DOM-013 is FIF-076's and is blocked — and no allocation or attribution table is called for by this item's requirements. A migration that adds those columns later is cheaper than a schema that guesses them now.
ARC-010's rounding boundary is FIF-004's `round_to`, and FIF-054 names persistence as one of the two boundaries at which a value must already be at its scale; the repositories are where that is enforced, so a scale violation should be impossible to write rather than merely untested.
Revision 19 found the whole increment written and **uncommitted**: `crates/fifolio-core/migrations/0001_initial.sql`, `crates/fifolio-core/src/storage/` (`mod.rs`, `codec.rs`, `entities.rs`, `manual_entries.rs`, `transactions.rs`) and `crates/fifolio-core/tests/storage.rs` are untracked, with `Cargo.toml`, `lib.rs`, `entities.rs`, `precision.rs` and `tests/temporary_database.rs` modified. The suite is green there: 207 tests, 0 failures, of which `tests/storage.rs` contributes 21 at the integration layer. It stays `todo` under the convention revisions 5, 7, 9, 13 and 15 applied — uncommitted is unreviewed, and an unreviewed item is the item still to build. The outstanding work is review against the six requirements above and a commit, not a rebuild.
The one thing that landed in `design/` while that tree sat there is DEC-067 (`72abea7`), which is the acceptance clause added above; the tree already implements it as `StorageError::UnscaledValue` via `codec::at_scale`, so the decision documents what was built rather than contradicting it. Check that the refusal is asserted per scale and not only for money.
Done in revision 20, commit `e5c2900`, which committed the tree revision 19 found uncommitted. Verified against the code rather than the message: `migrations/0001_initial.sql` is applied on connection from an embedded migrator, `./fifolio.db` is the default and `--database`'s override is a parameter of the open call [ARC-011 to ARC-014]; each transaction variant has a detail table of its own rather than one wide nullable table, so a row of one variant cannot be read as another; `Isin` is the securities primary key and trims and uppercases on construction, a second insert reporting `DuplicateIsin` [DOM-071]; `codec::at_scale` is the only path a decimal takes to a column and refuses an over-scaled figure with `StorageError::UnscaledValue`, asserted once per scale — money 2, unit price 6, FX rate 6, quantity 8 — which is the DEC-067 clause. No transaction-to-account, -security or -record relation was invented, as this item required. Twenty-two integration tests against a real temporary database [TST-004]; the workspace suite is 208 tests, 0 failures, on a clean tree.
**ARC-029** was added to this item in revision 29 and closes an identifier gap rather than adding
work. `409f76f` renumbered the duplicate `ARC-025` — the sentence saying a store that rounds on the
way in cannot tell a figure meant to be rounded from one that arrived wrong — to ARC-029; that is
the rationale of the DEC-067 refusal clause above, which `codec::at_scale` and
`StorageError::UnscaledValue` already implement and test per scale. Nothing new is owed. ARC-025
stays with FIF-045, where the sentence it originally labelled lives.

## FIF-012 Storage-enforced invariants
Status: done
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
Done in revision 25, commit `3136a8a`: migration `0003_invariants.sql` adds the relations the invariants read (attribution keyed on account and security with its allocations, a transaction's account and security, a record's owning batch, the `transfer_out`-to-`transfer_in` emission link) and no more; each refusal is its own `StorageError` variant — `EarlierClosingUnattributed`, `LaterAttributionExists`, `TransactionAttributed`, `EmittedTransferIn`, `BatchTransactionAttributed`, `BatchRecordsCited` — and `crates/fifolio-core/tests/invariants.rs` covers all seven requirements against a real temporary database. Two things the implementation pinned that later items inherit: `approve(closing, &[])` stores an attribution with no allocation rows and that closing then unblocks later ones (FIF-078 must refuse it under DOM-065, and that is a change to a test written here), and an emitted `transfer_in` belongs to no batch, so an undo reaches it only through the group its `transfer_out` heads.

## FIF-078 Allocation quantity invariants
Status: todo
Requirements: DOM-064, DOM-065
Depends on: FIF-012, FIF-061
Acceptance: allocated quantities against an opening, each scaled to a common position in the canonical order, never exceed its effective quantity at that position, and a closing's allocations sum exactly to its quantity; both refused at the persistence/service boundary with a distinguishable error, integration tested against a real temporary database.
Blocked by (until revision 69): DOM-064 and DOM-065 are on the undecided list.
Revision 69: unblocked by the provisional DEC-091. Acceptance amended: both comparisons are made at the 8-decimal quantity scale, half away from zero; a parcel whose remaining effective quantity rounds to zero there is exhausted. Outside the server scope the user set, so not selected this run.
Notes: Split out of FIF-012 in revision 3. DOM-064 compares effective quantities at a common position, which is why this half, and not FIF-012, carries the FIF-061 dependency.
The boundary DOM-065's refusal must cover is the **empty** allocation set: FIF-012 stores `approve(closing, &[])` as an attribution with no allocation rows, and that closing then counts as attributed and unblocks every later closing under DOM-066 while consuming nothing. The behavior is pinned by a test of FIF-012's, so refusing it here is a change to that test rather than a silent one.

## FIF-060 Pending-record block and single consumption
Status: todo
Requirements: DOM-049, DOM-067, DOM-070
Depends on: FIF-012, FIF-058
Acceptance: a security with any pending source record is blocked from attribution **in the account that record belongs to** and not elsewhere; a source record is consumed by at most one transaction though it may be cited by several.
Notes: Split out of FIF-012 and FIF-015 in revision 2 and blocked then; all three ids left the undecided list in revision 3. DOM-049 and DOM-067 state the same block from two sides, which is why they are one item. Still gated on FIF-058 (DOM-101), which defines consumption.

## FIF-013 FIFO proposal engine
Status: done
Requirements: DOM-056, DOM-057
Depends on: FIF-076, FIF-056, FIF-061
Acceptance: given an account, a security and a closing transaction, the engine consumes the oldest openings with unattributed effective quantity remaining, in canonical order, until the closed quantity is covered, splitting the final opening; if the available unattributed quantity is short, it returns a shortfall naming the missing quantity instead of a proposal. Pure function over transactions and existing allocations, unit tested with no database.
Notes: Revision 3 splits the expiration rule (DOM-092, DOM-114) into FIF-079, and moves the ordering dependency from FIF-006 to FIF-076, which is where canonical order now lives.
Revision 71: named next to build; FIF-076, FIF-056 and FIF-061 are done. Measure remaining quantity through `EffectiveQuantity::at_quantity_scale` as of the closing's `OrderKey` (DEC-091, DEC-097), so exhaustion and the shortfall use the same 8-decimal view FIF-014 and FIF-078 will; the effective-quantity calculation refuses a position preceding the opening, so only openings before the closing are candidates.
Done in revision 72: `fifo.rs` `propose` consumes the oldest candidate openings in canonical order (row id breaking an `OrderKey` tie, DEC-095) until the closing's quantity is covered, splitting the last, or returns the shortfall (DOM-057); `effective_quantity::unattributed_quantity` gives a parcel's remainder as of the closing, each prior allocation rescaled from its own closing, as the difference of the two sides at the quantity scale (provisional DEC-099). A closing of nothing and an over-allocated parcel are refused (provisional DEC-098). Pure, no database.

## FIF-079 Expiration quantity
Status: blocked
Requirements: DOM-092, DOM-114
Depends on: FIF-013
Acceptance: an `expiration`'s quantity is the unattributed remainder rather than a stated figure, and a row that would expire a position with nothing remaining stays pending rather than being derived, so no division by a zero quantity can arise.
Blocked by: DOM-092 and DOM-114 are on the undecided list.
Notes: Split out of FIF-013 in revision 3. IMP-SAXO-013's `Expiratie` mapping (FIF-023) states the same pending rule from the importer side and is blocked too.

## FIF-061 Splits and effective quantity
Status: done
Requirements: DOM-089, DOM-103, DOM-113
Depends on: FIF-076, FIF-054
Acceptance: a `split` carries an integer numerator and denominator; an opening's effective quantity **as of a position in the canonical order** is its stated quantity times the ratios of every split for that security falling between the opening and that position, computed as an exact rational and rounded only for display; its effective unit price at that position is its total cost over that effective quantity; stated figures are never rewritten; successive splits compose with no accumulated residue, so 1-for-3 then 3-for-1 returns the original quantity exactly.
Blocked by (until revision 69): DOM-113 is on the undecided list, and it is the substance of the item: the rational representation is what the rest of the rule is built on. ARC-009 (FIF-054) bears on it too.
Revision 69: unblocked by the provisional DEC-091. Acceptance amended: effective quantity stays an exact rational, and wherever it is compared (exhaustion, allocation sums) it is taken at the 8-decimal quantity scale, half away from zero; the residue below 1e-8 of a share is dropped. Provide that scaled view here so FIF-013, FIF-014 and FIF-078 share one rounding. If DEC-091 is overridden on ratification, this item reopens.
Revision 70: named next to build; FIF-076 is done.
Done in revision 71: migration `0007_split_ratio.sql` stores a split's integer ratio; `effective_quantity.rs` computes an opening's effective quantity and unit price as of an `OrderKey` as an exact `BigRational`, applying only the splits strictly between (provisional DEC-097), with `EffectiveQuantity::at_quantity_scale` the one 8-decimal view FIF-013, FIF-014 and FIF-078 compare through (DEC-091). Stated figures are never rewritten.
Notes: New in this revision, replacing the "quantity adjustment" half of the retired FIF-018. DOM-103 is the reason effective quantity is parameterized by position at all: measuring as of today halves the cost of everything sold before a split.

## FIF-014 Allocation figure derivation and the drift rule
Status: done
Requirements: DOM-058, DOM-059, DOM-060, DOM-061, DOM-062, DOM-063, DOM-084, DOM-093, DOM-105, DOM-125
Depends on: FIF-013, FIF-061, FIF-004, FIF-054
Acceptance: allocated cost, buy fee, proceeds, sell fee and gain computed on demand from the parent transactions, exactly as the formulas in `domain.md` state, with the opening side divided by its effective quantity **as of the closing** and the closing side by the closing's own quantity; every closing variant carries `eur_gross` and `eur_fees` so one formula reads them all, and `split` carries neither; each share rounded to 2 decimals independently with drift absorbed by the last share, opening-side last being the allocation that exhausts the parcel and closing-side last being the last allocation of that closing in canonical order; sell fees never spread beyond their own closing; **a gain is arithmetic on the four rounded shares**, not computed exactly and rounded afterwards, so every allocation reconciles to its own columns. Unit tests include a division that is exact, one leaving one cent, and one leaving many, and one where rounding-then-subtracting and subtracting-then-rounding differ, asserting the former.
Notes: Blocked in revision 2 on DOM-059, DOM-105 and DOM-112; the first two are now decided. DOM-112 (a `transfer_out`'s `eur_gross` derived from its own allocations) moved to FIF-080 in revision 3, so this item covers the cash closings only.
DOM-084 moved here from FIF-008 in revision 15: allocation shares derive from the native/EUR pair the same way and are never stored independently, which is only assertable once allocations exist.
DOM-125 is new in revision 14 (DEC-066). It belongs here, with the rounding rule it qualifies, and not with the reports: its second half — a total is the sum of the rounded rows — is a consequence the report items FIF-030 and FIF-031 inherit by summing what this item produces, and neither is built yet. It costs up to two cents against the exact figure per allocation, deliberately; say so in the code comment, or someone will "fix" it.
Revision 72: named next to build; FIF-013, FIF-061, FIF-004 and FIF-054 are done. Divide the opening side by `effective_quantity` as of the closing's `OrderKey` (exact `BigRational`, DOM-103), and take the drift-absorbing "last" allocations in the same canonical order `fifo.rs` uses (row id breaking an `OrderKey` tie, DEC-095). Allocation quantities are at the 8-decimal scale (DEC-091); an opening is exhausted when `unattributed_quantity` reaches zero on that scale, which is what makes its allocation the opening-side last. Pure, no database, like FIF-013.
Done in revision 72: `allocation.rs` `opening_shares` and `closing_shares` derive cost, buy fee, proceeds and sell fee on demand from the parents, each share an exact rational quotient rounded once to 2 decimals with the last absorbing the drift (opening side: the exhausting allocation; closing side: the last in canonical order, DEC-095); the closing side divides by the closing's stated quantity (provisional DEC-100); a last share may be negative (provisional DEC-101); a gain is arithmetic on the four rounded shares (DOM-125). Cash closings only; `transfer_out` is FIF-080's. Pure, no database.

## FIF-080 Transfer out has no proceeds and no gain
Status: todo
Requirements: DOM-112
Depends on: FIF-014, FIF-063
Acceptance: a `transfer_out`'s `eur_gross` is the basis it carries onward rather than proceeds — derived from its own allocations, not stored — so the opening side is computed first and no gain is ever computed for it.
Blocked by (until revision 69): DOM-112 is on the undecided list (OQ-011). As of revision 47 OQ-003 and OQ-016 are closed, and the transfer rules it rests on (FIF-063) are decided.
Revision 69: unblocked; OQ-011 is provisionally answered by DEC-093. Outside the server scope the user set, so not selected this run.
Notes: Split out of FIF-014 in revision 3 so the cash closings' formulas can be built.

## FIF-015 Attribution service
Status: done
Requirements: DOM-018, DOM-019, DOM-020, DOM-054, DOM-055
Depends on: FIF-014, FIF-012
Acceptance: approve-or-decline only, no partial edit; creating an attribution validates same account and same security, every allocated opening preceding the closing in canonical order, and quantities summing exactly to the closing quantity; declining writes nothing and leaves the closing blocking later closings by construction.
Notes: DOM-049 (the pending-record refusal, previously here) moved to FIF-060.
Revision 73: named next to build; FIF-014 and FIF-012 are done. `storage/attributions.rs` `approve` already refuses a non-closing, a closing already attributed and an earlier unattributed closing (DOM-066, FIF-012); it does not yet check DOM-019, DOM-020 or the sum. Add those checks in the service in front of it: same account and security for every allocated opening; each opening strictly before the closing by the stored `OrderKey`, row id breaking a tie (DEC-095); allocation quantities summing to the closing quantity, both at the 8-decimal scale (DEC-091). The sum rule's id, DOM-065, stays on FIF-078 (out of this run's scope); share one check rather than writing it twice. Approval takes a proposal from `fifo.rs` and writes it unchanged or not at all (DOM-054); there is no edit path. Declining is a no-op that writes nothing (DOM-055). The pending-record refusal (DOM-067) is FIF-060's, outside this scope, so do not add it here.
Done in revision 73: `attribution.rs` `approve` checks, in the same SQLite transaction as storage's own refusals and the write, that every allocation names an opening of the closing's account and security (DOM-019) strictly before it in canonical order (DOM-020, DEC-095), with a positive quantity and each opening once (provisional DEC-103), and that the quantities sum to the closing's at the 8-decimal scale through `allocation::covered`, the one check DOM-065 shares; it then stores the allocations unchanged (DOM-054). An `expiration` is refused until FIF-079 decides its quantity (provisional DEC-102). Declining has no function (DOM-055). The proposal-equals-display guarantee is FIF-039's fingerprint (provisional DEC-104). Integration tests in `crates/fifolio-core/tests/attribution.rs`.

## FIF-063 Transfer out: emission and basis
Status: done
Requirements: DOM-090, DOM-106, DOM-107, DOM-115
Depends on: FIF-015, FIF-061
Acceptance: approving a `transfer_out` emits **one `transfer_in` per consumed parcel**. Each carries that parcel's acquisition date, date provenance `inherited`, **its own allocated cost** rather than a share of a pooled total, and its own allocated buy fee as fees, separate from the cost (DOM-106). Each cites the `transfer_out`'s source records and takes the `order` of the parcel it carries, so the parcels sort in the receiving account as they did before (DOM-090). Each emitted quantity is the consumed quantity times the transfer's ratio at the quantity scale. The last record absorbs the remainder, so the emitted quantities sum exactly to the transferred quantity times the ratio, rounded once to the quantity scale, half away from zero (DOM-115). A transfer carrying a fee of its own is refused, naming the row, and never divided (DOM-107).
Notes: New in revision 2, replacing the "lot transfer" half of the retired FIF-018. DEC-046 is explicit that pooling would give two equal parcels bought at 100 and 200 the same unit cost of 150 — a total that is right with every individual figure wrong — so the per-parcel rule is the point of the item, not a detail of it.
Revision 47 splits and unblocks this item. DEC-079 to DEC-083 closed OQ-002, OQ-003, OQ-016, OQ-017 and OQ-018, and DOM-090, DOM-106, DOM-107 and DOM-115 were rewritten accordingly. DOM-091 (OQ-001) is still undecided, so the decomposition moves to the new **FIF-100** with DOM-116, which divides that decomposition's money and cannot be reviewed without it. Revision 3's objection to splitting was that DOM-115 and DOM-116 each qualify an open rule. That now holds only for DOM-116. The emission is a complete increment: it is reviewable against DOM-090 alone. DOM-107 reaches the importers: FIF-067 and FIF-068 surface the refusal on a row, and this item owns the rule that a transfer's fee is never divided. It is still not startable, because FIF-015 waits on FIF-013 and FIF-061 is blocked.
Revision 74: named next to build; FIF-015 and FIF-061 are done, and none of DOM-090, DOM-106, DOM-107, DOM-115 is on a `Blocks:` line. Emit from `attribution.rs` `approve` in the same SQLite transaction as the allocations, so approval and emission commit or fail together (SRV-030's "same operation" is FIF-073's to expose, not a second write path). Per-parcel cost and buy fee come from `allocation.rs`'s derived figures, never pooled (DOM-106, DEC-081); order is the carried parcel's stored `OrderKey` (DOM-090); quantities follow DOM-115 (DEC-083), the "last" record taken in the same canonical order `fifo.rs` uses (DEC-095). Storage already knows an emitted `transfer_in` (`StorageError::EmittedTransferIn`, FIF-102) and `DateProvenance::Inherited`. The DOM-107 fee refusal is a domain check here; the importers' row-level surfacing stays with FIF-067 and FIF-068. The decomposition of cash-and-exchange (DOM-091, DOM-116) is FIF-100's, out of this run's scope.

## FIF-100 Cash-and-exchange decomposition
Status: todo
Requirements: DOM-091, DOM-116
Depends on: FIF-063
Acceptance: an event paying cash for part of a holding and exchanging the rest decomposes into a `sell` and a `transfer_out`, both citing the same source records. The sell leg takes the group's summed cash net of any reversal and **all** of its costs. The transfer leg carries no money of its own, only the basis it inherits.
Blocked by (until revision 69): DOM-091 is on the undecided list (OQ-001: which record each leg consumes and what orders the two legs). DOM-116 divides the money of that decomposition and is blocked with it rather than split into an unreviewable half.
Revision 69: unblocked by the provisional DEC-090. Acceptance amended: the `sell` consumes the group's records and sorts first; the `transfer_out` cites them and sorts immediately after, so FIFO gives the sale the oldest parcels. Outside the server scope the user set, so not selected this run.
Notes: Split out of FIF-063 in revision 47, recorded on both halves. FIF-067 derives Saxo's cash mergers against this.

## FIF-016 Property tests for the engine
Status: todo
Requirements: TST-010
Depends on: FIF-014, FIF-061, FIF-063, FIF-062
Acceptance: `proptest` suites asserting every property in `testing.md`: no opening over-consumed; allocations sum exactly; shares sum exactly to the parent for every division and remainder; a buy's fees fully distributed at its last unit sold and not before; a split leaves total cost unchanged while scaling effective quantity, and effective quantity before any split equals the stated quantity; successive inverse splits compose exactly; each record a transfer emits carries its own parcel's cost, never a pooled average; a transfer preserves total basis and parcel count; attributing a sequence of closings in canonical order never over-consumes an opening; order computed from a file is identical however often it is imported and independent of import history; an undo followed by a re-import restores exactly the transactions that existed before.
Notes: Blocked in revision 2; TST-010 left the undecided list in revision 3. Its dependencies FIF-061 and FIF-063 are still blocked, so it is not yet startable.

## FIF-017 Import and derivation framework
Status: done
Requirements: DOM-002, DOM-042, DOM-044, DOM-045, DOM-046, DOM-048, DOM-120, DOM-124, ARC-023
Depends on: FIF-007, FIF-011
Acceptance: an importer trait over a source file that yields source records; rows where every field the variant needs is present and unambiguous are derived automatically; rows that affect holdings but lack something only the user knows become pending, which is the completion queue; **cash** dividends, interest, deposits, withdrawals and account fees are recognized as non-position, counted and not stored, while a dividend that issues shares is a position event whose rows are stored and whose buy carries a stock-dividend origin; everything the user supplies becomes a manual entry. XLSX reading via `calamine` and CSV via `csv` sit behind the same reader abstraction; a delimited row stores its verbatim line while a spreadsheet row, having none, stores the canonical rendering `domain.md` defines — each cell as the file holds it, an Excel serial date staying `45208`, keyed by column name in sheet column order — so that re-reading the same file reproduces the same string.
Named next to build in revision 25: the first `todo` in document order whose dependencies, FIF-007 and FIF-011, are both `done`, and none of whose nine requirements is on a `Blocks:` line. Everything ahead of it in the plan is either `done`, `blocked`, or waits on a `blocked` item — FIF-060 on FIF-058, FIF-013 on FIF-076 — so the import framework, not the FIFO engine, is where decided work continues.
Notes: DOM-120 is new in revision 7 (DEC-057) and sits here because the reader abstraction is what constructs the stored raw content; FIF-005, which owns the `SourceRecord` type, is `done` and its field is untyped as to how it was rendered. DOM-043, the classification taxonomy these three outcomes belong to, moved to FIF-064 in revision 2. Revision 3 moves DOM-047, "nothing is invented", to FIF-081, it being undecided; the three outcomes stand without it.
DOM-124 is new in revision 14 (DEC-063) and sits here rather than in FIF-025 because it is the exception clause on DOM-002 and DOM-046, which this item owns; it is format-agnostic, and the Saxo heuristic that recognizes such a dividend is FIF-025's, which already derives the buy. Nothing completed is invalidated: the classification is not built yet.
Done in revision 26, commit `bb09bb0`, which is the work revision 25 named and the session then stopped on. Verified against the code rather than the message: `import/reader.rs` puts `SpreadsheetReader` (calamine) and `DelimitedReader` (csv) behind one `RowReader` [ARC-023]; a delimited row keeps its verbatim line and a spreadsheet row the canonical JSON rendering — cell as the file holds it, an Excel serial staying the serial, keys in sheet column order — with a test that re-reading the same bytes reproduces the same string [DOM-120]. `import/mod.rs` carries the `Importer` trait and the `import` function that reads, orders and classifies; `RowClassification` is the three outcomes [DOM-044, DOM-045, DOM-046], `NonPositionKind` is the five cash kinds that are counted and not stored [DOM-002], a share-issuing dividend having no kind there because it is a position event [DOM-124]; `completion` is the only constructor of a `ManualEntry` on the import path [DOM-048]. Orders are assigned over every row the file holds, stored or not, so a row's `order` depends on the file alone. `cargo test --workspace` is green on the clean tree: **299 passed, 0 failed**, up from 254 at revision 25.
Two things later items inherit. The trait's `classify` takes the whole row slice and answers one classification per row, because a Saxo corporate action is classified by its group and not by its row alone; an importer returning the wrong count is a refusal, not a silent truncation. And DOM-043's closed set shipped here as the `RowClassification` type, which is **FIF-064's** requirement, not this item's — FIF-064 is what must review it and say whether anything remains.

## FIF-102 An emitted transfer_in does not block undoing its transfer's batch
Status: done
Requirements: none (correction of DOM-119 under DEC-086; DOM-119 stays on FIF-012)
Depends on: FIF-012
Acceptance: `foreign_citations` treats a `transfer_in` emitted by a `transfer_out` as derived by the batch that derived that `transfer_out`; a failing test written first shows a batch whose `transfer_out` emitted records, each citing the batch's stored records, can be deleted; a batch whose records are cited by a genuinely foreign transaction is still refused; an emitted record consumed by an attributed disposal still refuses under SRV-022.
Notes: Added in revision 52 by the user's decision. Found by FIF-081's implementer: `foreign_citations` (storage/entities.rs) reads an emitted `transfer_in`, which has no batch, as foreign, so under DEC-079 its batch could never be undone. Two emission tests in `invariants.rs` pass only because they cite records never stored; they should cite stored ones. FIF-081 then adds its stored-record handle to `Derivation::new`. Done in revision 54: reviewed and green, committed in one commit with the unreviewed work below at the user's instruction.
Named next to build in revision 53: the first `todo` in document order whose dependency, FIF-012, is `done`; it carries no blocked id. Everything ahead of it is `done`, `blocked`, or waits on a `blocked` item (FIF-060 on FIF-058; FIF-013, FIF-014, FIF-015, FIF-063 and FIF-016 on FIF-076 and FIF-061). The uncommitted working tree already rewrites the two emission tests in `invariants.rs` to cite their emitter; review that against this item before writing anew.

## FIF-081 Nothing is created from nothing
Status: done
Requirements: DOM-047
Depends on: FIF-017, FIF-102
Acceptance: there is no path, in core or at any surface, that creates a transaction other than from source records; the absence is structural rather than a check, and a test asserts it for every construction path.
Notes: Split out of FIF-017 in revision 3. Revision 54: this item's partial, **unreviewed** code was committed at the user's instruction so the tree is clean; build on it and review it against the specification as if uncommitted. Done in revision 55: passed conformance and test review; committed as `fdad876`, `6a8b5ba`, `35b3374`.
Unblocked in revision 47: DEC-079 closed OQ-002. An emitted `transfer_in` cites the source records of the `transfer_out` that produced it (FIF-063), so the emission path is derivation from source records too. The test must cover it once FIF-063 exists; until then it covers every path that exists.
Named next to build in revision 55: the first `todo` in document order whose dependencies are all `done` and which carries no blocked id.

## FIF-064 Import classification taxonomy
Status: done
Requirements: DOM-043
Depends on: FIF-017
Acceptance: the classification a source record receives at import is a closed, stated set, and every importer maps into it exhaustively.
Notes: Split out of FIF-017 in revision 2 and blocked then; DOM-043 left the undecided list in revision 3.
Named next to build in revision 26. FIF-017 is `done` (`bb09bb0`), DOM-043 is on no `Blocks:` line, and it is the first `todo` in document order whose dependency is `done` — everything ahead of it is `done`, `blocked`, or waiting on a `blocked` item (FIF-060 on FIF-058, FIF-013 and FIF-014 on FIF-076 and FIF-061).
Read this before starting: **most of the type this item asks for already exists.** FIF-017 shipped `RowClassification` — `DerivedAutomatically`, `Pending`, `NonPosition(NonPositionKind)` — and made `Importer::classify` a required trait method returning it, so the set is closed by the enum and no format can answer outside it. The honest residual increment is therefore small and is these three things, not a new type: (a) the set is *stated* as the taxonomy of DOM-043, in one place, with the rule that a new classification is an amendment to `domain.md` and never a format's invention; (b) exhaustiveness is guaranteed mechanically against importers that do not exist yet — no catch-all variant, no `Default`, no path that returns a classification without deciding one; (c) a test that names DOM-043 and fails if the set grows a variant nothing maps to.
If review finds all three already true of `bb09bb0`, say so and close the item against that commit rather than inventing work to justify it; an item whose substance landed inside its predecessor is a real outcome and belongs in the record. What it must **not** do is grow into the per-format classifiers — Saxo's is FIF-023 and blocked (OQ-005, OQ-008, OQ-014), Trade Republic's is FIF-029.
Done in revision 27, commit `3e9fe46`. It went the way the note allowed for: mostly a review that closed DOM-043 against what FIF-017 had already shipped, plus the three residual things, and no new type. (a) `RowClassification`'s documentation now states the set as *the* taxonomy of DOM-043 and says a new classification is an amendment to `domain.md` first and never a format's invention; the module header points at it as the only statement. (b) The closure is mechanical, not asserted: no catch-all variant, no `Default`, and `Importer::classify` must answer one of the three per row, so a row a format cannot place is a `RowError` rather than a fourth outcome. (c) `the_classification_taxonomy_is_the_three_outcomes_of_dom_043` pins the set with `strum::EnumCount` against the outcomes an import actually reaches, and asserts each outcome is reached by a row, so a variant nothing maps to fails rather than passes unnoticed. `cargo test --workspace` on the clean tree: **300 passed, 0 failed**, up from 299.

## FIF-062 Manual entry lifecycle across undo and re-import
Status: done
Requirements: DOM-108, DOM-109
Depends on: FIF-059, FIF-017
Acceptance: undoing an import removes its source records and the transactions derived from them and leaves every manual entry standing; re-importing the same rows reconnects each entry by the record identities it names and restores its transaction automatically, so an undo followed by a re-import returns the account exactly where it was; an entry whose records are absent is listed as waiting, naming what it expects.
Notes: New in this revision (DEC-038). This is the behavior the separate manual-entry entity exists for, and TST-010's last property asserts it.
Named next to build in revision 27. Its two dependencies are `done` — FIF-059 (`4af8667`, widened by FIF-092 in `cd09f97`) and FIF-017 (`bb09bb0`) — neither DOM-108 nor DOM-109 is on a `Blocks:` line, and everything ahead of it in document order is `done`, `blocked`, or waiting on a `blocked` item: FIF-060 on FIF-058, FIF-013 on FIF-076, FIF-014 and FIF-015 on FIF-061 and FIF-013, FIF-016 on FIF-061 and FIF-063.
What it can build on and what it must not assume: FIF-011 (`e5c2900`) persists `ManualEntry` with its `Vec<RecordIdentity>` references, which is what a reconnection reads, and FIF-012 (`3136a8a`) already refuses to delete a manual entry on an import undo [DOM-110] as a storage invariant. This item is the *behavior* that invariant protects — the undo itself, the reconnection on re-import, and the waiting listing — not a second copy of the refusal.
Two things earlier items pinned that bear on the undo path. FIF-012's note records that an emitted `transfer_in` belongs to no batch, so an undo reaches it only through the group its `transfer_out` heads; that path is FIF-063's and blocked, so scope this item to records and transactions a batch owns and say so where the code would otherwise look incomplete. And a transaction's relation to the source records it was derived from is still DOM-013 in the blocked FIF-076, so "the transactions derived from them" must be read through the `derived_by_batch` relation FIF-011 and FIF-012 actually store, not through a record-level relation this item would have to invent.
The re-import half is testable end to end against the framework FIF-017 shipped, since identity [DOM-022 to DOM-024, FIF-007] is what a reconnection matches on and is `done`; TST-010's last property ("an undo followed by a re-import restores exactly the transactions that existed before") is **FIF-016's** to assert as a property test and must not be pulled forward here.
Where DOM-108 is split, recorded in review: what this item ships is the *pairing* — `reconnected(batch)` hands back each entry the import completed with the records it answers, which is everything a derivation needs and is what makes the restoration ask the user nothing. The clause "its transaction is restored automatically" also needs a caller that derives and stores, and no derivation path from records to a `Transaction` exists in the workspace yet; that wiring is **SRV-055, FIF-072's**, whose acceptance already carries it word for word and which depends on this item. Two things FIF-072 inherits, both stated on `reconnected`: the listing is presence-based, so a *first* import of an entry's rows reports it exactly as a re-import does and the caller owns the idempotency check that keeps SRV-055 from storing a second copy of a transaction that already exists; and a record an approval emitted belongs to no batch, so that path stays FIF-063's.
Done in revision 28, commit `4b6c89d`, which is the work revision 27 named and the session then
stopped on — the fourth consecutive revision of that shape. Verified against the code rather than
the commit message: `storage/manual_entries.rs` carries `waiting`, which lists every entry no stored
source record answers together with the identities it expects [DOM-109], and `reconnected(batch)`,
which pairs each entry an import completed with the records it answers in the order the entry names
them [DOM-108]; both read one presence predicate, so an entry cannot be reported waiting and
reconnected at once. The undo half is the batch deletion FIF-011 and FIF-012 already store, with
FIF-012's `ManualEntryDeletedByUndo` refusal standing over it [DOM-110]; this item adds no second
copy of that refusal. `crates/fifolio-core/tests/manual_entry_lifecycle.rs` is the integration
cover, at the layer these two requirements are observable in [TST-004]. `cargo test --workspace` on
the clean tree: **310 passed, 0 failed**, up from 300.
The two splits the commit message records stand and are inherited as written: the clause of DOM-108
that says the restored transaction is derived and stored needs a records-to-`Transaction` path the
workspace does not have, and that wiring is SRV-055's in **FIF-072**, whose acceptance already
carries it; the emitted-`transfer_in` path is **FIF-063's** and blocked.

## FIF-065 Import guard: one calendar year per file
Status: done
Requirements: IMP-001, IMP-002
Depends on: FIF-017
Acceptance: a file whose rows carry trade dates in more than one calendar year is refused, on the trade date rather than a booking timestamp or the filename; the refusal names the years met. Tested against the Saxo and Trade Republic fixtures, each of which is confined to one year, plus a synthetic file spanning two.
Notes: New in revision 2 (DEC-052) and blocked on IMP-003, which left the undecided list in revision 3. Revision 8 splits IMP-003, the account guard, into FIF-089: OQ-015 is new and blocks it. The year guard reads only trade dates and is decided, so it stays here and stays buildable. Renamed accordingly. The server-side half is SRV-051 in FIF-070.
Named next to build in revision 28. Its dependency FIF-017 is `done` (`bb09bb0`), neither IMP-001
nor IMP-002 is on a `Blocks:` line, and it is the first `todo` in document order whose dependencies
are all `done` — everything ahead of it is `done`, `blocked`, or waiting on a `blocked` item
(FIF-060 on FIF-058, FIF-013 on FIF-076, FIF-014 and FIF-015 behind them, FIF-016 on FIF-061 and
FIF-063). The base is `4b6c89d`, the tree clean and the suite green at 310 tests.
Where it goes: `import::import` already reads an `ordering_key` per row before anything is
classified, and that key carries the trade date [DOM-040], so the years are in hand at exactly the
point a whole-file refusal is still cheap. That makes this a guard inside `import`, not a per-format
duty — the same reason identity scoping and order assignment live there and no format can forget
them. A new `ImportError` variant naming the years met, in the style of `Unorderable`, is the shape;
`ImportError::Unorderable` is also the precedent for refusing the file rather than dropping rows.
The refusal must read the trade date and nothing else [IMP-002]: not the filename, which `import`
never sees, and not a booking timestamp, which a format may also put in its ordering columns. A
partial year passes — the first Saxo export runs 2021-11-29 to 2021-12-31 — so the test that a
single-year file is accepted is as much the requirement as the refusal is.
**The fixture half of the acceptance above cannot be met by this item and is deferred, deliberately
rather than by omission.** Reading a Saxo or Trade Republic fixture for its trade dates needs that
format's `ordering_key`, and both importers are `todo` (FIF-019, FIF-027); nothing in the workspace
can turn fixture bytes into a dated row today. So this item's tests are integration-layer over a
test-double importer plus a synthetic two-year file, and the assertion that each committed fixture
is confined to one calendar year — which is a property of the fixtures [TST-011 to TST-014], already
asserted structurally in `tools/anonymize-exports/tests/fixture_structure.rs` — is re-asserted
through this guard by FIF-019 and FIF-027 when they can run it. Neither of those items' acceptance
changes; this note is where the obligation is recorded so it is not rediscovered.
Done in revision 29, commit `769bb48`, which is the work revision 28 named. Verified against the
code rather than the commit message: the guard sits in `import::import`, reads the trade date of
each row's ordering key and nothing else, and refuses the whole file with an `ImportError` naming
every year met in ascending order; `crates/fifolio-core/tests/import_year_guard.rs` covers the
refusal, a partial year passing as a single year, and the guard ignoring a booking timestamp a
format also puts in its ordering columns. `cargo test --workspace` on the clean tree: **315 passed,
0 failed**, up from 310. The deferred fixture clause above stands unchanged and is now additionally
owed through **FIF-093**, the fixtures being rebuilt before FIF-019 can run it.
Revision 58: SRV-059 (DEC-088) makes this guard's first-ground refusal a defect when other grounds
also hold. Corrected by **FIF-103**; this item's own acceptance still holds.

## FIF-099 A row that fails to parse rejects the whole import
Status: done
Requirements: SRV-058
Depends on: FIF-017
Acceptance: an import in which any row fails to parse is refused as a whole, and nothing is stored. A row fails to parse when its **ordering key**, its identity or its classification cannot be read (DEC-087). The refusal names **every** failed row by position with its reason, not only the first, whichever of the three each failed on. An unreadable ordering key no longer stops the import at the first such row. `Import::failures` and the failed-to-parse count are gone, because no import can succeed with a failed row. Tested with a synthetic file carrying failing rows of all three kinds among good ones, including two with unreadable ordering keys.
Notes: New in revision 47 (DEC-074, SRV-058). **This is completed work the specification has put in question.** FIF-017 (`done`) built `import::import` to count a failed row and carry on, reporting it in `Import::failures`. DEC-074 reverses that. The refusal is a new `ImportError` variant, and FIF-033's exhaustive match in `fifolio-server/src/problem.rs` needs a `ProblemType` for it. Built at the core level. FIF-035 exposes it at the endpoint. SRV-017's summary (FIF-085) loses its failed-to-parse count for the same reason.
Named next to build in revision 56: the first `todo` in document order whose dependencies are all `done` and which carries no blocked id.
Revision 57: acceptance widened for DEC-087 (`242ab6a`), which rewrote SRV-058 so that an unreadable ordering key is a failed row too. **This puts more of FIF-017's completed work in question**: FIF-017 built `ImportError::Unorderable`, which stops the import at the first row whose key cannot be read, and FIF-033 maps it to the `unorderable-row` problem type. Both now fall under this item's refusal. Said here rather than as a new item because SRV-058 is this item's one requirement and the item is not yet done. The uncommitted partial work in the tree (the `FailedRows` variant, migration `0004_no_failed_count.sql`, the `failed-rows` problem type) predates DEC-087 and still keeps `Unorderable` as a first-row stop, so it does not yet meet this acceptance. The year guard (IMP-002, FIF-065) reads the ordering keys' trade dates; how it combines with rows whose key is unreadable is for the implementer to check against IMP-002 and SRV-058, and to raise with `spec-auditor` if they conflict.
Named next to build again in revision 57.
Revision 58: DEC-088 (`951cb67`) removes the `failed` count from the import batch's fields in
`domain.md` (DOM-017), so the stored batch loses it too, as the uncommitted migration
`0004_no_failed_count.sql` already does; this is in scope here, not a change to FIF-005's
acceptance. DEC-088 also adds SRV-059, which answers the question revision 57 left about the year
guard: a file with two years and failed rows is refused with both grounds. That combination is
**FIF-103**, not this item; here, the failed-row refusal need only be complete on its own.
Named next to build again in revision 58.
Revision 59: **done**, committed as `116be0c`. `ImportError::Unorderable` and `Import::failures` are
replaced by `ImportError::FailedRows` (problem type `failed-rows`), and migration
`0004_no_failed_count.sql` drops the batch's failed count (DOM-017, DEC-088). The acceptance test
`every_failed_row_refuses_the_import_and_is_named` in `import/mod.rs` carries two unreadable ordering
keys, one unclassifiable and one unidentifiable row among good ones and asserts all four are named.
`cargo test --workspace`: **596 passed, 0 failed**. The multi-ground combination stays FIF-103.

## FIF-089 Import guard: one account per file
Status: done
Requirements: IMP-003
Depends on: FIF-017, FIF-065
Acceptance: where the format states an account id, an import whose file names a different account than the target is refused naming both, and a file carrying rows from more than one account is refused outright. A format stating none, as Trade Republic's does not, is not checked and is imported into the account the caller names. Tested against the Saxo fixture (matching, mismatching) and a Trade Republic fixture (unchecked).
Notes: Split out of FIF-065 in revision 8. OQ-015 observes that the Trade Republic export carries no account identifier at all, so the check has nothing to read there; whether the requirement admits uncheckable formats or Trade Republic files are matched some other way decides the shape of this item, not merely a detail of it. Its server-side counterpart is SRV-056 in FIF-090, blocked by the same question.
Unblocked in revision 47: DEC-075 closed OQ-015 and rewrote IMP-003. Saxo's account id is the normalized one FIF-020 derives (DOM-003), so the comparison is against the normalized id and not the raw suffixed cell.

Revision 60: **done**, committed as `d086bf8`. `ImportError::AccountMismatch` (naming both ids) and
`ImportError::MultipleAccounts`, mapped to 422 problem types `account-mismatch` and
`multiple-accounts`; a format with no account id is not checked, and a row whose account cannot be
read is a failed row. Tested in `fifolio-core/tests/import_account_guard.rs` against the Saxo fixture
(matching, mismatching) and Trade Republic (unchecked). `cargo test --workspace`: **602 passed, 0
failed**. The combination with the other guards stays FIF-103.

## FIF-103 One refusal reports every ground
Status: done
Requirements: SRV-059
Depends on: FIF-099, FIF-089
Acceptance: an import refused on more than one ground reports every ground it can determine in one refusal: trade dates in more than one year (IMP-002), a file account differing from the target or several accounts in one file (IMP-003), and failed rows (SRV-058), in any combination. No guard short-circuits another. Nothing is stored. Tested with synthetic files carrying each pair of grounds and all three together, each refusal naming every ground and, for failed rows, every row.
Notes: New in revision 58 (DEC-088, `951cb67`). **This puts completed work in question**: FIF-065 (`done`) refuses on the years as soon as it sees them, so a file with two years and a failed row is refused on the years alone, which DEC-088 names as the defect. FIF-065's record is left as built; the correction is here. Built at the core level, where all three guards live in `import::import`. The endpoint shape of a combined refusal reaches the caller through FIF-070 and FIF-090, which now depend on this item. A row whose ordering key is unreadable has no trade date, so it is a failed row and contributes no year; whether it should also be counted toward the year check is not stated, and if the implementer finds the two requirements conflict, that is a `spec-auditor` finding.

Revision 61: **done**, committed as `5e225eb`. All three guards now report into one
`ImportError::Refused`, none short-circuiting another; tested in
`fifolio-core/tests/import_refusal_grounds.rs` (each pair of grounds and all three). This resolves
the FIF-065 defect DEC-088 names. `cargo test --workspace`: **609 passed, 0 failed**.

## FIF-019 Saxo: file reading and header normalization
Status: done
Requirements: IMP-SAXO-001, IMP-SAXO-002, IMP-SAXO-003, IMP-SAXO-004, IMP-SAXO-037, TST-030
Depends on: FIF-017, FIF-093
Acceptance: reads **all three sheets** — `Transacties` (31 columns), `_Transacties` (24) and `Bookings` (21) — each with its one header row, and refuses a workbook missing any of them rather than importing the cash ledger alone; joins them as `importers.md` states, a `Transacties` row to its `_Transacties` counterpart on `Transactie-ID` else `Corporate action-Id`, and to its `Bookings` components on `Bk Record Id` / `Booking Id` else `Corporate action-Id`, with a corporate action's legs being **its `_Transacties` rows under one `Corporate action-Id`, however many there are** — one, two or three in the sample — all of which the join answers; matches headers after whitespace normalization, so `Bk\xa0Record\xa0Id`, `Booking\xa0Id` and ` Positie-ID` resolve; converts Excel serial numbers to dates; rejects a non-Dutch header set with a clear error rather than mis-mapping; a blank cell is accepted in both the shapes it arrives in — a zero-length shared string as Saxo writes it and an empty cell as the fixture writer produces it — and a test says so. Tested against the rebuilt Saxo fixtures, including that every position-affecting row resolves a `_Transacties` counterpart and that an unjoinable row is a refusal and not a silent absence.
Notes: Revision 3 splits IMP-SAXO-025, the newest-first direction rule, into FIF-083. TST-030 is new in revision 7: it is a known divergence of the fixtures (FIF-003) but its assertion is on this reader, which is why it is owned here.
**Re-scoped in revision 29 by DEC-070 (`c04362c`), which restated IMP-SAXO-001 and added IMP-SAXO-037.** The file is three sheets, not one, and the two that were never opened carry the quantities, the traded values and the tax figures. Nothing built is invalidated — this item has never been started — but two things follow. The dependency moves from FIF-003 to **FIF-093**: the committed fixtures reproduce one sheet, so this item cannot be tested until they are rebuilt, and DEC-070 says so in as many words. And `SpreadsheetReader` (FIF-017, `done`) takes the **first** sheet and checks nothing about it, which its own documentation records as the format's business; reading three named sheets is therefore this item's increment and not a defect in FIF-017.
One thing this item must settle in code and say out loud: what a Saxo **source record** is now. DOM-120 renders one spreadsheet row, and a Saxo event is up to three rows across three sheets. Whether the record is the `Transacties` row with its counterparts folded into the rendering, or one record per physical row with the join re-derived later, is not stated in `design/` and bears on identity (FIF-020, FIF-084), on consumption (FIF-058, blocked) and on what an undo removes. Record the reading chosen and its consequences here; if it cannot be chosen from `importers.md`, that is a `spec-auditor` finding and not an implementer's judgement call.
Revision 32 finds this item **built in the working tree and uncommitted**:
`crates/fifolio-core/src/import/saxo.rs` (903 lines), `import/test_workbook.rs` (372) and
`crates/fifolio-core/tests/saxo_export.rs` (270) are untracked, `import/mod.rs`, `import/reader.rs`
and `Cargo.toml` are modified, and `cargo test --workspace` gives **371 passed, 0 failed**, up from
344. It stays `todo` for the reason revisions 13, 15 and 30 kept FIF-059, FIF-008 and FIF-093 at
`todo`: completion is recorded against a commit, never against a working tree, and uncommitted is
unreviewed. The outstanding work is review against the acceptance above and a commit, not a rebuild.
The open reading the note above demanded **is settled in that tree and stated**: one source record
per `Transacties` row, the detail sheets being joined inputs to a derivation rather than records of
their own, argued from DOM-007 / DOM-120 (a record's rendering is one row's cells), from
IMP-SAXO-026 (neither detail sheet carries `Transactiedatum`) and from IMP-SAXO-007 (a detail row
shares the ledger row's identity columns, so per-row records would collide). The review should
confirm that argument holds rather than re-open it; its consequence — a detail column is reachable
only while the workbook is open — is what FIF-024, FIF-025, FIF-094 and FIF-067 inherit.
**One correction the review must make**, and it is the reason the acceptance above changed this
revision: the tree's `detail_of` documentation and its test
`a_corporate_action_joins_both_its_legs_on_its_group_id` assert a corporate action is "**both** its
legs ... two rows ... distinguished by `Trade Event Type`", which is the DEC-070 phrasing that
DEC-071 (`786cfd2`) withdrew. The join itself is right and needs no change; the claim about leg
count is now false — the sample carries one-leg, two-leg and three-leg groups — and must be restated
before it is committed, or the next reader takes it for the rule. Summing the sides and cancelling
opposing legs is **not** this item's work; it is FIF-096.
Revision 33: **committed as `a97004e` and done.** The tree revision 32 found uncommitted was
reviewed and landed: `crates/fifolio-core/src/import/saxo.rs`, `import/test_workbook.rs` and
`crates/fifolio-core/tests/saxo_export.rs`, with `cargo test --workspace` green. The correction
revision 32 demanded was made before the commit — the `detail_of` documentation now states a group's
legs are its `_Transacties` rows "however many there are", naming the two-`Gekocht` Philips dividend
and the three-leg DeVolksbank tender, and the DEC-070 "both its legs" phrasing is gone. The settled
reading stands as recorded: one source record per `Transacties` row, the detail sheets being joined
inputs to a derivation; FIF-020, FIF-024, FIF-025, FIF-067 and FIF-094 inherit it.


## FIF-083 Saxo: newest-first row direction
Status: done
Requirements: IMP-SAXO-025
Depends on: FIF-019, FIF-006
Acceptance: the reader recognizes that Saxo emits rows newest first and normalizes the file's direction before row positions are used for ordering, so file position as a tie-breaker means oldest-first position.
Notes: Split out of FIF-019 in revision 3. FIF-066's third ordering key ("file position taken in reverse") states the same thing from the ordering side and is only correct once this is settled.
Unblocked in revision 47: OQ-009 closed (DEC-082). FIF-084's ordinal now counts in the order this item establishes, so this item goes first.

Revision 62: **done**, committed as `f9a718c`. `saxo::DIRECTION` declares the file newest first, so
file position breaks an ordering tie in reverse; tested on synthetic rows and on every committed
fixture in `fifolio-core/tests/saxo_export.rs`. `cargo test --workspace`: **612 passed, 0 failed**.

## FIF-066 Saxo: ordering columns
Status: blocked
Requirements: IMP-SAXO-026, IMP-SAXO-027, IMP-SAXO-036
Depends on: FIF-019, FIF-006
Acceptance: rows are ordered on `Transactiedatum` first, and the order **within one date** is chosen per date: if every row of that date populates the same one of `Bk Record Id`, `Booking Id` and `Transactie-ID`, that counter orders the date; otherwise the whole date is ordered by file position taken in reverse and no counter is consulted. The choice is per date and never per pair of rows, so the result is a sort order at all — a pairwise rule is not transitive, `Bk 5`, `Booking 100` and `Bk 7` being the counterexample. `Corporate action-Id` is never an ordering column, because it is not monotonic with date. Tested on a fixture date carrying several rows where only that id is populated, so file position settles them, and on one of the ten fixture dates that carry a mixture of counters, asserting that date falls wholly to file position and that the order does not depend on which column a row happens to populate.
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
Restated again in revision 29 by DEC-069 (`409f76f`), which keeps DEC-064's reason and replaces its
mechanism: the pairwise comparison revision 14 wrote into the acceptance above is **not transitive**
and so is not a sort order, and the choice is now made per date. The acceptance is rewritten
accordingly; the item is still `blocked` and still unbuilt, so nothing is invalidated. `ordering.rs`
(FIF-006) is unaffected a second time — it takes ordering columns in a stated precedence, and a
per-date choice of which column to supply is this item's binding of it, not a change to the
mechanism.

## FIF-020 Saxo: account normalization and row identity
Status: done
Requirements: DOM-003, IMP-SAXO-005, IMP-SAXO-006, IMP-SAXO-007, IMP-SAXO-024
Depends on: FIF-019, FIF-007
Acceptance: `Rekening-ID` currency suffix stripped, so `.../1000000EUR|USD|CAD` collapse onto one account; `Klant-id` never used as the account; identity is the first populated of `Transactie-ID`, `Bk Record Id`, `Booking Id`, `Corporate action-Id`; if two rows still produce one identity the file is rejected rather than deduplicated. Re-importing the fixture twice yields no new records.
Notes: The collision rejection (IMP-SAXO-024) was new in revision 2. Revision 3 splits the `Corporate action-Id` composite identity (IMP-SAXO-008) into FIF-084, it being undecided; until that lands, the corporate-action fallback is the bare id, so the collision refusal will fire on the TransAlta shape rather than distinguishing its rows. That is the safe failure and the refusal test covers it.
Done in revision 34, commit `95fac5e`, in `crates/fifolio-core/src/import/saxo/identity.rs` with
its integration cases in `crates/fifolio-core/tests/saxo_export.rs`. `account` strips the currency
suffix so the EUR, USD and CAD sub-accounts collapse onto one Depot [DOM-003, IMP-SAXO-005] and
never reads `Klant-id` [IMP-SAXO-006]; `identity` takes the first populated of `Transactie-ID`,
`Bk Record Id`, `Booking Id`, `Corporate action-Id` after the whitespace normalization FIF-019
supplies [IMP-SAXO-007]; `check_identities` refuses a file whose rows collide rather than
deduplicating, including one value appearing in two different columns [IMP-SAXO-024]. The safe
failure this item's note predicted is asserted rather than assumed: the test named for the
three-row TransAlta shape shows the refusal firing until FIF-084 lands. `cargo test --workspace`
is green at **386 passed, 0 failed**, up from 371.
That commit also carried revision 33's own PLAN.md edits, which is why the plan it committed still
showed this item as `todo`; the record is corrected here rather than by rewriting that commit.

## FIF-084 Saxo: corporate action row identity
Status: done
Requirements: IMP-SAXO-008
Depends on: FIF-020, FIF-083
Acceptance: when identity falls through to `Corporate action-Id`, that id plus `Acties` plus `Boekingsbedrag` plus **the row's ordinal within its `Corporate action-Id` group in normalized, oldest-first order** (FIF-083), not the file's newest-first order form the identity, so the three TransAlta rows stay distinct across re-imports, and a row a later export adds to a group takes the next ordinal without shifting the existing rows' identities. If two rows still share an identity the file is rejected (IMP-SAXO-024, already built).
Notes: Split out of FIF-020 in revision 3. Without the ordinal, two rows sharing a label and an amount deduplicate as a re-import and money vanishes from the event; DEC-033 records why the ordinal and not a hash of the row.
Unblocked in revision 47: DEC-082 corrects DEC-033's "in file order". In the file's newest-first order, a row added by a later export of the same year shifts every existing ordinal, which breaks SRV-015.
Revision 63: **done**, committed as `18cf73a`. `crates/fifolio-core/src/import/saxo/identity.rs`
composes a `Corporate action-Id` fallback identity from the id, `Acties`, `Boekingsbedrag` and the
row's ordinal within its group counted oldest first (DEC-082); a colliding identity is still refused
[IMP-SAXO-024]. Integration cases in `fifolio-core/tests/saxo_export.rs`. `cargo test --workspace`:
**618 passed, 0 failed**.

## FIF-021 Saxo: money derivation
Status: done
Requirements: IMP-SAXO-009, IMP-SAXO-010, IMP-SAXO-023, IMP-SAXO-029, IMP-SAXO-030, IMP-SAXO-032
Depends on: FIF-019, FIF-008
Acceptance: `Boekingsbedrag` read as native cash movement including costs, `Aantal` as the same amount in EUR and never as a quantity, `Totale kosten` as EUR costs (always negative), `Omrekeningskoers` as the native-to-EUR multiplier whose **reciprocal** is the stored rate; the derivation is sign-aware — EUR gross = |Aantal| − |Totale kosten| on a buy and |Aantal| + |Totale kosten| on a disposal; EUR fees = |Totale kosten|; EUR price = EUR gross / (quantity × factor), so the quoted price is reproduced and the factor is applied once; native figures follow the same shape with costs converted back using `Omrekeningskoers`, not the stored rate, **the subtraction keeping full precision like every other intermediate**, so the sample buy stores a native unit price of 5.750036 against a printed 5.75; rate source `broker`. Unit tests reproduce all three worked examples in `importers.md`: the 40 @ 5.75 USD buy (EUR gross 209.63, EUR price 5.240750), the 60 @ 30.65 sell (EUR gross 1839.24, EUR price 30.654) and the 3000 @ 139.46 bond (EUR price 139.46, not 1.3946).
Notes: The sign-aware derivation (DEC-035) was new in revision 2; revision 1's single formula understated every disposal's proceeds by twice the fee. Blocked in revision 2 on IMP-SAXO-030, which left the undecided list in revision 3. The factor in the EUR-price step is FIF-075 and still undecided, so the bond case in the third worked example lands with that item.
IMP-SAXO-032 is **reversed in revision 8**. Revision 7 read it (DEC-054) as an exception to the intermediate-precision policy of FIF-054, rounding the native fee so the stored price matched the printed one; DEC-059 supersedes DEC-054 and settles it the other way, so IMP-SAXO-032 is now an instance of that policy rather than an exception to it, and the plan's earlier acceptance clause is gone rather than edited into silence. Nothing was built against the old reading — this item has never been started — so no completed work is invalidated. It changes no EUR figure and therefore no tax figure either way.
DOM-039 was restated in the same commit: reconciliation against a broker document is on the **booked amounts**, which are exact, and not on the printed unit price. FIF-055 records that the observable assertion for DOM-039 lands here; that assertion is now "the derived price reproduces the booked gross", not "the stored price equals the printed one", and the sell case (30.654 against a printed 30.65) already in this item's acceptance is exactly it.
Done in revision 35, commit `f34c466`, in `crates/fifolio-core/src/import/saxo/money.rs` (38
functions, 806 lines with its unit tests) plus the integration cases in
`crates/fifolio-core/tests/saxo_export.rs`. The four columns are read as what they are
[IMP-SAXO-009, IMP-SAXO-010]; the direction is an argument rather than the sign of a money cell, so
a disposal is no longer understated by twice the fee [IMP-SAXO-030]; the unit price divides by
`quantity × factor` once [IMP-SAXO-023]; the native costs are converted back with the row's own
`Omrekeningskoers` and the stored rate is its reciprocal [IMP-SAXO-029]; nothing rounds, the
subtraction included, so the sample buy derives 5.750036 against a printed 5.75 and the sample sell
30.654 against 30.65 [IMP-SAXO-032]. Both EUR-quoted worked examples are reproduced; the bond
example waits on FIF-075 as this item's notes said it would.
The **rounded-to-FX-scale half of IMP-SAXO-029 is not here** and is deliberately carried onto
FIF-023 (see its notes): whichever row-to-transaction mapping first persists a Saxo conversion must
round at the storage boundary, and storage refuses an unrounded rate outright, so omitting it is a
failed insert and not a wrong figure.
That commit also carried revision 34's own PLAN.md edits, which is why the plan it committed still
showed this item as `todo` — the sixth time this has happened. The record is corrected here rather
than by rewriting the commit.

## FIF-022 Saxo: `Acties` label parsing
Status: done
Requirements: IMP-SAXO-011, IMP-SAXO-012, IMP-SAXO-038
Depends on: FIF-019
Acceptance: quantity and direction come from `_Transacties` — `Traded Quantity`, signed, and `Trade Event Type` stating `Gekocht` or `Verkocht` — and parsing them out of the free-text label (`Koop 40 @ 5.75 USD`, `Verkoop -60 @ 30.65 EUR`, `Deponering 300 @ 51.40 EUR`) is the fallback for a row with no counterpart, not the normal path; a label whose parsed quantity disagrees with the column **refuses the file** rather than choosing one, and a test asserts that refusal; the label price is exposed only as a 2-decimal display value and is structurally unable to reach any money field, with **no** exception now that `Deponering` reads `Verhandelde waarde` (FIF-024); a test asserts the sell case where 30.65 in the label differs from 30.654 derived from the columns; an unparsable label is a parse failure, not a guess.
Notes: Blocked in revision 2 on IMP-SAXO-012, which left the undecided list in revision 3.
**Re-scoped in revision 29 by DEC-070**, which added IMP-SAXO-038 and restated IMP-SAXO-012: every figure now comes from a column, the quantity and the direction included, so the label is a cross-check and a fallback rather than the source. The item is not started, so nothing is invalidated; what changes is which way the disagreement test points — it used to prove the label's price wrong, and now it also refuses a file whose label and column disagree on the quantity.
The one sanctioned exception this item carried, `Deponering`'s authoritative label price, is **gone**: IMP-SAXO-039 puts that basis on `Verhandelde waarde`. Nothing in the workspace may keep a path by which a label price reaches money.
Done in revision 36, commit `1f38bc3`. Found built, committed and green with the plan still reading
`todo` — the seventh consecutive occurrence, that commit having swept revision 35's plan edits in
alongside `import/saxo/quantity.rs`. Nothing was rebuilt; the module was read against the three
requirements and the status corrected. `cargo test --workspace` gives **425 passed, 0 failed**, up
from 410.
What is there: `traded(label, leg)` is the single precedence point [IMP-SAXO-038] — the column pair
`Traded Quantity` / `Trade Event Type` first, the label only for a row with no counterpart, and a
disagreement between the two refusing the file rather than choosing
(`a_label_disagreeing_with_the_column_refuses_the_file`). The label price is a `LabelPrice` display
type with no conversion into `Money`, so no path carries it to a money field [IMP-SAXO-011,
IMP-SAXO-012], and the 30.65-against-30.654 sell divergence is asserted. An unparsable trade clause
is a parse failure. `Deponering`'s event type is neither of the two IMP-SAXO-038 names, so
`direction()` is an `Option` there and FIF-024 must supply the direction from the classification
rather than from the column.

## FIF-088 Saxo: reversal rows and group summation
Status: done
Requirements: IMP-SAXO-033, IMP-SAXO-034, IMP-SAXO-035
Depends on: FIF-022, FIF-021
Acceptance: `Terugboeking` is matched as a **suffix** and never as a bare `Acties` value, so `Terugkoopaanbod - Terugboeking` and `Dividend - Terugboeking` classify as their prefix, reversing; summing a `Corporate action-Id` group, a reversal's cash and its costs both **subtract**, reproducing the DeVolksbank tender: 3946.14 paid, −1998.07 reversed, 1948.07 net; summation is over the **EUR** figures the rows carry and never over the native ones, because a group's rows may hold different `Omrekeningskoers` values. Unit tested over the reversal rows of the Saxo fixture for the classification, and over synthetic figures for the summation, the fixture's amounts being perturbed.
Notes: New in revision 7 (DEC-056). Cut as its own item rather than folded into FIF-023, which is blocked, so that a decided rule is not frozen behind an undecided one; the group summation it provides is what FIF-067 (cash merger, tender and buyback decomposition) and FIF-025 (dividend grouping) each call, and both are gated on FIF-023 anyway. Its `_Transacties` counterpart is **FIF-096** (revision 32): a reversal shows up as a leg as well as
a labelled cash row, and the two halves are read by different rules. That reversal costs subtract is **chosen, not observed**: both reversal rows in five years of exports carry zero in `Totale kosten`. The code comment must say so and name the check to perform if a costed reversal ever arrives, because nothing in the tests can catch it being wrong.
Done in revision 36, in the same commit as this status flip. `import/saxo/reversal.rs` carries both
halves: `Reversible::read` takes `Terugboeking` off an action as a suffix, requiring the separator
and a non-empty prefix so a bare value is not a reversal of nothing [IMP-SAXO-033]; `group_cash`
sums one `Corporate action-Id` over the EUR figures alone [IMP-SAXO-035], each figure entering as
its magnitude signed by whether its row reverses, so a reversal's cash and its costs both subtract
[IMP-SAXO-034]. The module header states that the costs half is chosen rather than observed and
names the check to perform if a costed reversal ever arrives. `Booked` gained `eur_movement` and
`eur_costs`; the native side deliberately has no accessor. Classification stays IMP-SAXO-013's:
this module answers what a prefix is, never what it means. Integration test
`every_reversal_in_the_corpus_is_a_suffixed_action` asserts the corpus carries exactly the two
suffixed reversals and no bare one; the summation is unit tested on the specification's figures,
the fixture's being perturbed [TST-014]. `cargo test --workspace` gives **436 passed, 0 failed**,
up from 425; patch coverage on `fifolio-core` is 100%.

## FIF-096 Saxo: a corporate action's legs, summed per side and cancelled by label
Status: done
Requirements: IMP-SAXO-044, IMP-SAXO-045, IMP-SAXO-046, IMP-SAXO-047, IMP-SAXO-048
Depends on: FIF-019
Acceptance: given the `_Transacties` legs of one `Corporate action-Id`, two rules applied in this
order and nowhere else: **first**, a leg whose `Acties` carries the `- Terugboeking` suffix cancels
against the leg in the same group with the same absolute quantity, the **same** price and the
opposite-signed traded value, both removed; a group with no suffixed leg cancels nothing, however
its legs are shaped. This reproduces the DeVolksbank tender — the suffixed `Gekocht 2000 @ 999.03 /
-19980.65` cancels `Verkocht -2000 @ 999.03 / 19980.65`, leaving one disposal of 2000 at 99.90 for
1998.07, the figure `Bookings` and the cash ledger both show — and, in the same test, leaves the
sample's one `Omwisseling` (`Gekocht 3 @ 168.63 / -505.89` against `Verkocht -3 @ 168.63 / 505.89`)
**intact**, that group carrying no suffix although its legs have the identical shape
[IMP-SAXO-045, IMP-SAXO-046]. **Second**, each side's quantity and traded value are the **sum** of
that side's remaining legs and are never taken by indexing one of them, reproducing the 2023 Philips
dividend as **2** shares at 34.74 and not 1 [IMP-SAXO-044]; a summed quantity keeps the file's sign,
so a `Verkocht` side is negative [IMP-SAXO-048]. There are **three** sides, not two: `Deponering` is
its own side and is neither bought nor sold, so all 13 transfer legs survive a grouping
[IMP-SAXO-048]. Summing a side's traded value is permitted only because a group carries one
instrument and so one currency; a group whose legs disagree on instrument currency is **refused**
with an error rather than summed [IMP-SAXO-047]. No caller may read "the `Gekocht` leg": the shape
returned offers summed sides and no way to ask for a single leg, so the error DEC-071 closes is
unreachable rather than merely tested for. Unit tests cover every leg shape the table in
`importers.md` lists — one `Gekocht`, one `Verkocht` plus one `Gekocht`, one `Verkocht`, two
`Gekocht`, two `Verkocht` plus one `Gekocht`, one `Deponering` — plus the mixed-currency refusal.
Notes: New in revision 32, carrying the two identifiers `importers.md` gained in `786cfd2`
(DEC-071). Cut as its own item rather than folded into FIF-067, FIF-094 or FIF-025 because all three
read it — the split ratio is a summed `Gekocht` over a summed `Verkocht`, the tender's disposal is a
summed side after cancellation, the stock election's share count is a summed `Gekocht` — and two of
them are gated on the blocked FIF-023 while this rule is decided and reviewable on its own against
the Legs table.
**Rewritten in revision 38 by DEC-072 (`79b82de`), before the item was started, so nothing built is
invalidated.** The acceptance revision 32 wrote cancelled a pair whose quantity, price *and* traded
value were exact negatives; DEC-072 establishes that the price on a cancelling pair is **equal**, so
that rule matched nothing in five years of exports, and that a shape-only reading matches the one
genuine `Omwisseling` and would destroy it. The old wording is replaced rather than annotated,
because building it would silently produce wrong parcels. The item gains IMP-SAXO-046 (the label is
the signal, not the shape), IMP-SAXO-047 (one currency per group, or refuse) and IMP-SAXO-048
(`Deponering` is a third side; a summed quantity keeps its sign) — all three new in that commit and
all three clauses of the same question this item already answers, which is what a group's sides are.
It is the `_Transacties` counterpart of **FIF-088**, which sums a group's *cash* on `Transacties` and
matches `Terugboeking` as an `Acties` suffix. The two are deliberately separate: FIF-088 reads a
label and subtracts money, this item reads the same label and removes rows, and a reversal shows up
in both places. FIF-088 is `done` (`0050f03`), so its suffix matching exists; whether this item
reuses that predicate or states its own is an implementation choice, but the module must say in its
header which half it is.
`Openen/sluiten` (`Te openen` / `Te sluiten`) corroborates a reversal and is explicitly **not** the
rule; a build that keys on it instead of on the suffix does not meet this acceptance.
One discrepancy the implementer will meet and must not silently resolve: the Legs table calls the
DeVolksbank tender "two `Verkocht` + one `Gekocht`", while DEC-072's worked figures give its
reversal as the `Gekocht` leg cancelling a `Verkocht`. Both readings leave the same single disposal
of 2000 at 99.90, so the outcome this acceptance asserts is unaffected; check the fixture rather
than the prose, and if the fixture disagrees with both, stop and report it.
This item does not classify, derive or emit anything: it answers what a group's sides are. What is
done with them is FIF-067's, FIF-094's and FIF-025's, and their acceptance lines now read the sides
from here.
Done in revision 38, in the same commit as this status flip.
`import/saxo/legs.rs` carries `Sides::of`, which takes one `Corporate action-Id`'s `_Transacties`
rows and answers the three sides summed [IMP-SAXO-044], [IMP-SAXO-048]. `Side` is `Acquired` /
`Disposed` / `Deposited`, so the sample's 13 transfer legs land on a side of their own rather than
being dropped; a summed quantity keeps the file's sign. `SideTotal` answers a quantity and a traded
value and holds no leg, and `Leg` is private, so "the `Gekocht` leg" is not a question a caller can
ask — the error DEC-071 closes is unreachable and not merely tested for.
Cancellation reads the `- Terugboeking` suffix through `Reversible::read`, FIF-088's predicate,
reused rather than restated; the module header says which half of a reversal it is
[IMP-SAXO-045], [IMP-SAXO-046]. A cancelling pair matches on equal absolute quantity, **equal**
price and opposite traded value, so the fixture's tender cancels and its one `Omwisseling` of the
identical shape survives, both asserted in the same integration test. `Openen/sluiten` is read by
nothing. A group's legs must agree on `Instrumentvaluta` or the group is refused
[IMP-SAXO-047].
Four points the specification leaves open are decided in the module header rather than in code
comments scattered about: an unmatched reversal survives instead of refusing the file, the 2023
export carrying exactly that; a reversal never cancels another reversal; a fourth `Trade Event Type`
refuses the group; a blank `Instrumentvaluta` refuses it. Each is named as a choice, not as a
reading of the specification.
The Legs table's "two `Verkocht` + one `Gekocht`" against DEC-072's worked figures was checked
against the fixture, as the note above requires: the fixture agrees with DEC-072 — the suffixed leg
is the `Gekocht` — and both readings leave the one disposal of 2000, so nothing is reported.
`cargo test --workspace` gives **461 passed, 0 failed**, up from 436; patch coverage on
`fifolio-core` is 100%.

## FIF-023 Saxo: row classification
Status: blocked
Requirements: IMP-SAXO-013
Depends on: FIF-022, FIF-017, FIF-056
Acceptance: `Transactietype` plus `Acties` map to exactly the table in `importers.md`, onto the transaction variants: `Koop` → `buy`, `Verkoop` → `sell`, both automatic; `Deponering` → `transfer_in`, automatic (FIF-024); `Expiratie` → `expiration`, automatic, pending if nothing remains; `Fusie`, `Terugkoopaanbod` (with `Terugboeking`) → pending `sell` and/or `transfer_out` (FIF-067); `Stock split` → pending `split`; `Omwisseling` → pending `transfer_out`; dividend labels → FIF-025; `Rente`, `Service fee`, `ADR-kosten`, `Storting`, `Opname` → recognized as non-position, not stored. Table-driven test over the fixture, one case per label; an unknown label fails to parse rather than being silently dropped.
Blocked by: IMP-SAXO-013 is on the undecided list, and it is the whole of the item — the classification table is its single requirement, so there is nothing to split off. Everything downstream of it (FIF-024, FIF-025, FIF-067) waits.
Notes: Newly blocked in revision 3; it was `todo` in revision 2. **Three** open questions now reach it: OQ-005 (when an expiration of nothing is detected), OQ-008 (an `Acties` value outside the table, with a live `Overige Corporate Action` instance) and, new in revision 8, OQ-014 — where a `Herbeleggingsdividend`'s share count and price come from, the export carrying neither. All three must close. OQ-014 is the one with scope beyond this item: 19 events in the sample against 12 for every other corporate action combined, so its answer decides how much of the tool's use is data entry, and it bears on FIF-025 and FIF-057.
Revision 29: **OQ-014 is answered but still blocks.** DEC-070 found the figures in the export — the share count on `_Transacties`, the taxable value in `Bookings`, and no shares at all on the nineteen `Herbeleggingsdividend` events — and `open-questions.md` marks the entry answered while keeping it in the file, and therefore on a `Blocks:` line, "until the rule is written into `domain.md` and the entry deleted". This plan takes the `Blocks:` lines literally, as it is required to, so IMP-SAXO-013 stays blocked and this item with it. OQ-005 and OQ-008 are untouched by DEC-070 and would keep it blocked anyway, so the practical cost is nil — but the classification table this item owns was substantially rewritten by DEC-070 (three rows moved from `pending` to `derived`), and the acceptance above still reads the old table. It is **not** rewritten here, because the item is blocked and an acceptance nobody can review is better left showing its age than quietly updated; the derivations themselves are covered by FIF-094, FIF-067 and FIF-025, whose acceptance lines are current.
Carried in from FIF-021: `Booked::conversion` returns the reciprocal of `Omrekeningskoers` at full
precision, which is the correct intermediate (ARC-009, DEC-027), but the *rounded to the FX scale*
half of IMP-SAXO-029 belongs to whichever row-to-transaction mapping first persists a Saxo
conversion. It must apply `conversion.rate().rounded()` at the storage boundary; storage refuses an
unrounded rate outright (`UnscaledValue { field: "conversion_rate", scale: 6 }`), so skipping it is
a failed insert rather than a wrong figure.

## FIF-024 Saxo: `Deponering` transfers in
Status: todo
Requirements: IMP-SAXO-014, IMP-SAXO-015, IMP-SAXO-016, IMP-SAXO-017, IMP-SAXO-028, IMP-SAXO-039, DOM-080
Depends on: FIF-023, FIF-009
Acceptance: a `Deponering` row (zero `Boekingsbedrag` and `Aantal`) produces a complete `transfer_in` with source `broker`, whose cost basis is `_Transacties`' `Verhandelde waarde` and whose quantity is `Traded Quantity`, taken at face value as a split-restated acquisition cost; the label's price is never used for money here either, and a test asserts the widest of the thirteen sample divergences — `300 @ 14.06 CAD` labelled, 4218.00 by the label against 4216.50 traded, 1.50 apart — so a regression to the label fails rather than passes within a tolerance; acquisition date set to the transfer date and date provenance `transfer_date`; the date is **never** corrected afterwards, so grandfathered Altbestand status is not represented; because `Omrekeningskoers` is 1 even for foreign-currency instruments, a non-EUR `Deponering` resolves its EUR cost basis by ECB lookup on the transfer date with source `ecb`; a later split row applies on top with no double counting.
Notes: Changed in this revision: DEC-040 reverses DEC-011's editable acquisition date. The date is now fixed, and the plan no longer carries an edit path for it (SRV-054, in FIF-038).
**Reversed in revision 29 by DEC-070, and this is the costliest of that commit's reversals.** IMP-SAXO-028 said the label price was authoritative here because no column carried the figure; the column exists, on a sheet nobody had opened, and the "half-cent tolerance of 0.15" the plan carried since revision 2 was an artifact of reading one sheet. The real divergence is up to 1.50 on one parcel and 3.30 across the thirteen, on a cost basis that is subtracted from a future gain — an error, not display rounding. The item has never been started, so nothing built is invalidated; the acceptance is rewritten rather than annotated, because the old clause would now produce wrong tax figures. The bond assertion in FIF-026 (3000 nominal at 139.46) reads the same basis and must be re-derived from `Verhandelde waarde` too, not from the label.

## FIF-025 Saxo: dividend grouping by `Positie-ID`
Status: todo
Requirements: IMP-SAXO-018, IMP-SAXO-019, IMP-SAXO-042, IMP-SAXO-043
Depends on: FIF-023, FIF-057
Acceptance: dividend-type rows (`Dividend`, `Keuzedividend`, `Herbeleggingsdividend`) are grouped by `Corporate action-Id` *before* evaluation; if any row in a group carries a `Positie-ID`, the whole group becomes one pending entry requiring an explicit stock-or-cash decision with stock pre-selected; a group is read against its `Bookings` components rather than against the label, so a `Herbeleggingsdividend` decomposing into `Corporate actions - Fracties` and `Corporate actions - Voorheffing` and no share amount is ordinary cash and issues nothing, while a `Keuzedividend` carrying a `Corporate Actions - Share Amount` component issues shares and derives a `buy` with origin `stock_dividend`, its share count from the **summed** `Gekocht` side's `Traded Quantity` (FIF-096) and its taxable value from that component — the 2025 Philips case being 1 share at 20.09 against 28.05 on 33 eligible shares; neither figure is asked of the user. A regression test reproduces the two-row Philips shape — position-marked row carrying zero, cash row carrying the money with no marker — and asserts that row-by-row evaluation would drop the money while grouping does not; a second asserts that all nineteen `Herbeleggingsdividend` events issue no shares and leave the eligible position unchanged.
Notes: Calibrated heuristic; its failure mode is a blocked attribution with a named shortfall, not a wrong figure. Keep that reasoning in the code comment. Blocked in revision 2 on IMP-SAXO-018, which left the undecided list in revision 3; the cost basis of the derived buy (FIF-057) is decided too. Still gated on FIF-023.
**Widened in revision 29 by DEC-070**, which added IMP-SAXO-042 and IMP-SAXO-043. Both are readings of the two sheets nobody had opened, and together they remove what the plan had recorded as the largest block of manual data entry: the share count and the taxable value of a stock election are in the export, and the nineteen reinvestment dividends issue no shares at all. This item has never been started, so nothing is invalidated. What it does change is the heuristic's standing — `Bookings` answers directly what `Positie-ID` was being used to infer — so the code must say which of the two it is trusting where they disagree, and the answer stated by `importers.md` is `Bookings`.
This is also the substance of **OQ-014**, which `open-questions.md` marks answered but keeps on its `Blocks:` line until the rule is written into `domain.md`. Until that entry is deleted, IMP-SAXO-013 is still blocked and **FIF-023** with it, so this item is still gated even though its own requirements are decided. That is a `spec-auditor` matter, not an implementer's.

## FIF-026 Saxo: security mapping and bond quotation
Status: done
Requirements: IMP-SAXO-020, IMP-SAXO-021, IMP-SAXO-022
Depends on: FIF-019, FIF-005
Acceptance: `Instrument ISIN`, `Instrument` and `Type` map onto the security by the explicit table — `Stock` → `stock`, `Bond` → `bond` with quotation `percent_of_par`, `Etf` → `etf`, `MutualFund` → `fund`, `Cash` → no security and a non-position row, anything else rejects the import; ISIN is the key, so a changed or delisting-annotated name (`*Delisted 20231011 (...)`) resolves to the same security and does not create a second one. Test asserts a 3000-nominal bond at 139.46 costs 4183.80.
Notes: The bond assertion is a factor assertion — 4183.80 and not 418,380 — and its **basis** now comes from `_Transacties`' `Verhandelde waarde`, not from the `Acties` label, DEC-070 having removed the one exception that let a label price reach money (FIF-024, IMP-SAXO-039). Whichever figure the rebuilt fixture states for that row is what the test must reproduce; 139.46 stays the quoted price it reconciles against [DOM-039].
Named next to build in revision 39, FIF-096 having landed in `54b9e08`. Both dependencies are
`done` — FIF-019 in `a97004e`, FIF-005 in `9846b5a` — none of IMP-SAXO-020 to IMP-SAXO-022 appears
on a `Blocks:` line, and everything ahead of it in document order is `done`, `blocked`, or waiting
on a `blocked` item.
Concretely for the implementer: the fixture's bond row reads `Deponering 3000 @ 138.00 EUR`, not
139.46 — amounts are perturbed and quantities are not [TST-028] — so the 4183.80 of the acceptance
is the **real** export's figure and cannot be asserted against the fixture. The factor assertion
must be built from the fixture's own quantity and `_Transacties` `Verhandelde waarde`: what the test
has to catch is a cost two orders of magnitude out, which holds whatever the perturbed price is.
This item must **not** settle where the factor is applied in general: `quantity x unit_price x
factor` is ARC-008 / DOM-038, blocked in FIF-075 by OQ-010. What is decided and owned here is that
`Type = Bond` defaults the security's quotation to `percent_of_par` — through FIF-055's
`quotation_for(SecurityType)`, which already answers exactly that and must be called rather than
duplicated — and that the bond's basis is read, not computed.
ISIN as the key is the half with a fixture case of its own: the fixtures carry two delisting
annotations, `*Delisted 20231011 (Fixture Instrument 14)` and `*Delisted 20231002 (Fixture
Instrument 07)`, the instrument names being pseudonyms while the annotation's shape survives, so the
assertion is that one ISIN under two names is one security and not two. Auto-creation's flag is FIF-005's `Security` field [DOM-008] and is set here.
Done in revision 39, in the same commit as this status flip. The acceptance's `3000 @ 139.46 =
4183.80` could not be asserted as written: the fixture states `138.00`, amounts being perturbed and
quantities not [TST-028], so the end-to-end test asserts the *magnitude* — the booked cost is the
nominal at a percentage of par, and lies in the thousands rather than the hundreds of thousands —
which is the factor assertion the acceptance was after and which no perturbation can hide.
Two points the specification left open are decided in the new module's header rather than invented
silently: which name a security keeps when its rows disagree (the first row's, an export being
newest first [IMP-SAXO-026]), and that one ISIN stated with two instrument types refuses the file
rather than keeping either, as a disagreeing quantity [IMP-SAXO-038] and leg currency
[IMP-SAXO-047] already do. No row of the five-year corpus triggers that refusal.

## FIF-067 Saxo: cash merger, tender and buyback decomposition
Status: todo
Requirements: IMP-SAXO-031
Depends on: FIF-023, FIF-063, FIF-100, FIF-096
Acceptance: a `Fusie` or `Terugkoopaanbod` group, reversals included, derives both sides from `_Transacties` — the `Verkocht` side disposing and the `Gekocht` side opening, each summed after cancellation as FIF-096 answers them, under the one `Corporate action-Id` — rather than asking the user for the quantity disposed and the target security, and produces a `sell` and/or a `transfer_out` with the cash and all of the costs on the sell leg; a group whose legs are absent or ambiguous falls back to a pending entry rather than guessing, and a test says which of the two each sample group takes.
Notes: Split out of FIF-023 in revision 2; IMP-SAXO-031 left the undecided list in revision 3. Still gated on FIF-063, whose transfer rules it derives against, and on FIF-023.
**Restated in revision 29 by DEC-070**, which rewrote IMP-SAXO-031's row of the classification table from `pending` to `derived`. The item is not started, so nothing is invalidated; what changes is that the completion-queue shape this was to raise — a disposed quantity with an optional target, which **FIF-092** shipped as `Supplied::Disposal` — is now the fallback rather than the normal path. FIF-092 is `done` and that variant stays: DOM-097 still lists the shape, and a group whose legs cannot be read still needs it.
Restated again in revision 32 by DEC-071: the DeVolksbank tender this item's own sample contains is
three legs with a reversal among them, so "the `Verkocht` leg" was never a thing to read. Sides are
summed after cancellation and that work is **FIF-096**, a new dependency. Still unstarted, so
nothing is invalidated.
Revision 47 adds FIF-100, the decomposition split out of FIF-063, to its dependencies.

## FIF-094 Saxo: split and exchange ratios from the position sheet
Status: todo
Requirements: IMP-SAXO-040, IMP-SAXO-041
Depends on: FIF-019, FIF-023, FIF-061, FIF-096
Acceptance: a `Stock split` group derives its ratio as the **summed** `Gekocht` quantity over the **summed** `Verkocht` quantity of its `_Transacties` legs (FIF-096), kept as the exact integer pair the sheet states rather than reduced or divided — Tesla `45 : 15`, OBAM `20 : 4` — and an `Omwisseling` group derives both quantities the same way, hence its ratio, with the target security taken from the `Gekocht` legs' instrument, which must be a single instrument or the file is refused; neither asks the user for anything; a group with an empty side, or a zero summed `Verkocht` quantity, is a refusal or a pending entry and never a guessed ratio, and a test says which for each shape.
Notes: New in revision 29, carrying the two identifiers `importers.md` gained in `c04362c` for rows the classification table used to mark `pending`. Cut as its own item rather than folded into FIF-023, which is blocked, for the same reason FIF-088 was: the derivation is decided and reviewable on its own once the classification that routes rows to it exists.
The ratio's *representation* is FIF-061's (DOM-113, blocked by OQ-004), and this item must not settle it: what it owns is that the pair comes off the sheet as two integers, which is what makes FIF-061's exact-rational question answerable at all. FIF-092's `Supplied::Split` — a bare ratio for a split — stays in the manual-entry set as the fallback shape; DOM-097 still lists it.
Restated in revision 32 by DEC-071 (`786cfd2`), which also restated IMP-SAXO-041's own sentence: the
sides are summed, not indexed, and "a group with three legs" is no longer a refusal shape — the
DeVolksbank tender is three legs and legitimate. The item is unstarted, so nothing is invalidated.
The summing and cancelling themselves moved to **FIF-096**, which this item now depends on.
Tesla 45:15 is 3:1 and OBAM 20:4 is 5:1, but the sheet states the quantities and not the reduced form; reducing early is where a 45:15 that is really a 3:1 on a partial position would be lost, so the pair is carried and the reduction, if any, is FIF-061's.

## FIF-027 Trade Republic: CSV reading, identity, ordering and sign convention
Status: done
Requirements: IMP-TR-001, IMP-TR-002, IMP-TR-003, IMP-TR-004, IMP-TR-015, IMP-TR-022, IMP-TR-023
Depends on: FIF-017, FIF-003, FIF-007, FIF-006
Acceptance: quoted comma-delimited CSV with dot decimals and one header row of 23 columns; `date` is the effective date and becomes the trade date, `datetime` is a booking timestamp used only as an ordering column after the date and never described as an execution time; the two are independent fields that diverge by up to six days on a corporate action; rows are ordered on `date`, then `datetime`, then file position, because the 2025 export is sorted by neither; `shares` is the quantity and `price` the unit price; `transaction_id` UUID used directly as the identity; cash-flow signs interpreted so buys and fees are negative. Re-importing the fixture twice yields no new records.
Notes: Blocked in revision 2 on IMP-TR-001, which left the undecided list in revision 3. IMP-TR-023 was restated in revision 7 and is sharper than the plan's earlier reading: the 2022 to 2024 exports ascend by both columns, while 2025 ascends by `date` and **descends** by `datetime` within a date. The requirement id and the item are unchanged; the fixture must carry the 2025 shape, which FIF-003 already states.
Named next to build in revision 40, FIF-026 having landed in `4d434ba`. All four dependencies are
`done` — FIF-017 (`bb09bb0`, the import and derivation framework), FIF-003 (`610ca2c`, the Trade
Republic fixtures), FIF-007 (`f9cecac`, identity) and FIF-006 (`8f9837c`, per-file order) — none of
IMP-TR-001 to IMP-TR-004, IMP-TR-015, IMP-TR-022 or IMP-TR-023 appears on a `Blocks:` line, and everything ahead of it in document order is `done`,
`blocked`, or waits on a `blocked` item. It is the **first Trade Republic item**, so it opens the
second format rather than continuing the Saxo chain, which is stalled behind the blocked FIF-023
(IMP-SAXO-013, by OQ-005, OQ-008 and OQ-014).
Concretely for the implementer, three pieces already exist and must be used rather than rebuilt:
`import/reader.rs` carries `DelimitedReader::comma()`, which answers the same `RowReader` trait the
Saxo spreadsheet reader does and already refuses a ragged file, so "CSV reading" here is the
23-column header check and the field mapping, not a parser; `import/mod.rs` carries the `Importer`
trait, `RowIdentity` and the counts, which the Saxo importer fills and this one must fill the same
way [ARC-023]; and `ordering.rs` carries `assign_orders(rows, direction)`, whose ordering columns
this item supplies as (`date`, `datetime`) with `FileDirection::OldestFirst`, which the enum's own
documentation already names as the Trade Republic direction [DOM-040]. Note that the file's first
column is `datetime` and its second `date`: the ordering precedence is the requirement's, not the
header's, so reading them in column order would order the 2025 export wrongly.
Two boundaries this item must not cross. Money is **FIF-028**'s — `price`, `amount`, `fee`, `tax`,
`currency` and the `original_*` triple — and IMP-TR-022 is here only because it says which column
holds the quantity and which the unit price, not what either is worth. Security typing is
**FIF-069**'s and is `blocked` by OQ-006, so a row's `asset_class` must be carried through
untouched; deciding what an unmapped class does here would answer that question in code.
Unlike Saxo's OQ-013, the ordering columns are never absent: all four committed fixtures (7, 17, 21
and 20 rows, 23 columns each) populate both `date` and `datetime` on every row, so no absent-value rule is needed and none should be invented. What the
2025 fixture does carry is the descending-`datetime`-within-`date` shape [IMP-TR-023], which is the
case that makes file position alone wrong and is the one a test must name.

## FIF-028 Trade Republic: money mapping
Status: done
Requirements: IMP-TR-005, IMP-TR-006, IMP-TR-007, IMP-TR-016
Depends on: FIF-027, FIF-008
Acceptance: `price`, `amount`, `fee`, `tax`, `currency` map to the native figures. **`amount` excludes the fee**, the opposite of Saxo, so gross = |amount| and fees = |fee| + |tax| in the settlement currency, checked as that relation against the fixtures (the real export's 35 × 75.09 = 2628.15 with a fee of −1.00 carried separately is the worked case). `original_amount`, `original_currency` and `fx_rate` are read where populated, but **`fx_rate` is never used to value anything**, in either convention: its convention changed in late 2024 and nothing stored needs it.
Notes: IMP-TR-016 and IMP-TR-017 are new in this revision and reverse the previous plan's assumption that a populated `original_*` triple values the trade. Revision 54: this item's partial, **unreviewed** code was committed at the user's instruction so the tree is clean; build on it and review it against the specification as if uncommitted.
Named next to build in revision 41, FIF-027 having landed in `39d98f4`. Both dependencies are
`done` — FIF-027 (`39d98f4`, the container half) and FIF-008 (`9a1e457`, EUR valuation and the
stored gross) — none of IMP-TR-005, IMP-TR-006, IMP-TR-007, IMP-TR-016 or IMP-TR-017 appears on a
`Blocks:` line, and everything ahead of it in document order is `done`, `blocked`, or waits on a
`blocked` item: FIF-060 on FIF-058 (DOM-101), FIF-013 on FIF-076 (DOM-011, DOM-013, DOM-111),
FIF-014 / FIF-015 / FIF-016 behind FIF-013, FIF-061 and FIF-063, and FIF-024 / FIF-025 / FIF-067 /
FIF-094 all behind the blocked FIF-023 (IMP-SAXO-013).
Concretely for the implementer, the shapes to fill already exist and must not be rebuilt.
`valuation.rs` carries `Valued<T>` (the native/EUR pair), `Conversion` (rate, source, rate date) and
`Currency`; `import/saxo/money.rs` is the worked precedent — `Booked::read`, `conversion(trade_date)`
and `DerivedMoney` with `gross()`, `fees()`, `unit_price()` — and this item is its Trade Republic
counterpart. The arithmetic is the **opposite** one: Saxo's booked amount includes the costs and
must have them removed, Trade Republic's `amount` excludes the fee, so `gross = |amount|` and
`fees = |fee| + |tax|` are read straight off the row [IMP-TR-016], with the sign convention FIF-027
already pins [IMP-TR-004]. `fx_rate` is foreign units per EUR, which is the stored convention, so it
is used as stated and never inverted; inverting it is the one mistake this item can make silently.
Two boundaries. Classification stays **FIF-029**'s, and IMP-TR-017's refusal needs only the literal
`category` cell reading `TRADING` beside a populated `original_*` triple — reading that one cell is
not classifying the row, and no mapping of the other categories may be written here. Security typing
stays **FIF-069**'s and is `blocked` (OQ-006), so `asset_class` remains untouched.
The acceptance's `35 x 75.09 = 2628.15` is the **real** export's arithmetic; fixture amounts are
perturbed and quantities are not [TST-028], so against a committed fixture it is asserted as the
relation `gross = |amount|` with the fee carried separately, not as that literal. The fixtures do
populate `original_amount` / `original_currency` / `fx_rate` on at least one **non-trade** row
[TST-013], which is the IMP-TR-006 case; a `TRADING` row carrying them exists in no export, so the
IMP-TR-017 refusal is tested against a constructed row rather than a committed fixture.
**Revision 42: `blocked`, not `done`.** OQ-021 (`b9017d8`) names IMP-TR-006 and IMP-TR-016 on a
`Blocks:` line: `fx_rate` is foreign units per EUR up to 2024-07-02 and its reciprocal after, so
"used as stated and never inverted" above is wrong for later rows. Blocked whole rather than split:
IMP-TR-016 (gross and fees) is the substance of the item, and IMP-TR-005 / IMP-TR-007 are its
column reading and its fee sum, not a separately useful increment. The `done` status came from an
implementation pass in the working tree and was never committed; that tree (`money.rs` under
`import/trade_republic/`, the `trade_republic.rs` error variants, three new fixture tests) is
uncommitted and **unreviewed**. Two points for whoever resumes it after OQ-021 closes:
`the_fixture_dividends_carry_their_conversion_uninverted` asserts the pre-change convention on all
14 dividends, including the five dated after 2024-07-02, so it encodes exactly the error OQ-021
describes; and **IMP-TR-024**, which that pass added to this item's `Requirements:` line, is not a
requirement in `design/`: `b9017d8` reverted the `importers.md` paragraph an implementation wrote to
authorize its own refusal (`PartialConversion`). It is removed here; the refusal has no requirement
behind it until a person writes one.
**Revision 47: unblocked, re-scoped.** DEC-073 closed OQ-021. IMP-TR-017 now rejects *any* row that would be stored with `original_*` populated, not only a `TRADING` one. Whether a row would be stored is classification, so IMP-TR-017 moves to **FIF-029**, and this item no longer reads the `category` cell at all. The uncommitted working tree (`import/trade_republic/money.rs`, the `trade_republic.rs` error variants, the new fixture tests) predates DEC-073 and must be reviewed against it, not committed as is. `the_fixture_dividends_carry_their_conversion_uninverted` asserts a convention the specification now says holds only up to 2024-07-02, so it must go or change. A `TRADING`-only refusal there belongs to FIF-029's broader rule.
Revision 64: **done**, committed as `e395518`. `import/trade_republic/money.rs` maps `price`,
`amount`, `fee`, `tax` and `currency`; gross is `|amount|` and fees `|fee| + |tax|` [IMP-TR-016],
[IMP-TR-007]; the `original_*` triple is read verbatim and values nothing (DEC-073) [IMP-TR-006]. The
obsolete uninverted-convention test was replaced. The foreign-trade refusal left this item for FIF-029
[IMP-TR-017]. Fixture cases in `fifolio-core/tests/trade_republic_export.rs`. `cargo test
--workspace`: **615 passed, 0 failed** (fewer than revision 63's 618 because the pre-DEC-073 cases in
`money.rs` were removed).

## FIF-029 Trade Republic: row classification
Status: done
Requirements: IMP-TR-008, IMP-TR-009, IMP-TR-010, IMP-TR-011, IMP-TR-012, IMP-TR-013, IMP-TR-014, IMP-TR-017
Depends on: FIF-028, FIF-056
Acceptance: `TRADING`/`BUY` and `TRADING`/`SELL` derive automatically; `CORPORATE_ACTION`/`TAX_EXCHANGE` is handled by FIF-068 and `CORPORATE_ACTION`/anything else rejects the import naming the type, the row and the transaction id; `CASH`/`DIVIDEND`, `INTEREST_PAYMENT`, `CUSTOMER_INBOUND`, `TRANSFER_INBOUND`, `STOCKPERK` are recognized as non-position and not stored, the `STOCKPERK` credit specifically without any check that its paired `TRADING`/`BUY` exists; any other type **naming a security** rejects the import, and any other type naming no security is not stored but is counted and named in the summary; nothing is inferred from quantity signs for an unrecognized type; the Saxo `Positie-ID` heuristic is not applied here; a row that would be stored with `original_*` populated **rejects the import naming the row**, while a non-stored foreign-currency row, such as every dividend in the fixtures, imports fine.
Notes: Previously blocked on decision D1 — which `CORPORATE_ACTION` types besides `TAX_EXCHANGE` map to a lot transfer. DEC-019 resolved it: none do, and an unrecognized type rejects the file. D1 is closed.
**An obligation carried over from FIF-027**, recorded here so it is not rediscovered: FIF-027's
acceptance clause "re-importing the fixture twice yields no new records", and the fixture half of
FIF-065's year guard, cannot be asserted through `import::import` until an `impl Importer for
TradeRepublic` exists, which needs the classification this item owns. FIF-027 asserts them at the
reader — the same fixture bytes identify the same records twice, and each fixture holds one calendar
year of trade dates — and this item must additionally drive a committed Trade Republic fixture
through `import::import` twice, asserting the second import stores no new source record and that the
year guard accepts the single-year file.
Revision 47 moves IMP-TR-017 here from FIF-028. DEC-073 extended it from `TRADING` rows to every stored row, and "would be stored" is this item's classification. The stored case exists in no export, so it is tested against a constructed row.
Revision 64: FIF-028 landed in `e395518` without any refusal on `original_*`; IMP-TR-017's
refusal is wholly this item's to write, on top of the `money.rs` reading FIF-028 left.
Revision 65: **done**, committed as `365fefd`. `import/trade_republic/classification.rs` classifies
every row [IMP-TR-008 to IMP-TR-014]; a stored row with `original_*` populated refuses the import
naming the row [IMP-TR-017]; the FIF-027 obligation is met by driving a fixture through
`import::import` twice in `fifolio-core/tests/trade_republic_export.rs`. `cargo test --workspace`:
**634 passed, 0 failed**.

## FIF-068 Trade Republic: `TAX_EXCHANGE`
Status: todo
Requirements: IMP-TR-018, IMP-TR-019
Depends on: FIF-029, FIF-063
Acceptance: the importer derives **only** the `transfer_out`, from the negative-quantity row; the positive row supplies the target security and the ratio and is cited, not turned into a transaction; the matching `transfer_in` records are emitted on approval, one per parcel consumed; the pairing key is same account, same effective `date`, same absolute quantity, opposite signs, with the ratio the **exact integer pair** `|target| : |source|` reduced by its greatest common divisor and never divided out, since a one-for-three ratio has no finite decimal expansion and a rounded one leaves a residue that grows across applications; an unpaired row, or more than two sharing a key, rejects the import.
Notes: Split out of FIF-029 in revision 2 and blocked then; both ids left the undecided list in revision 3. Still gated on FIF-063. DEC-032 records that tax-neutral treatment is inference from the absence of any monetary figure, to be settled against the user's TR tax report; that reasoning belongs in the code comment. IMP-TR-019 was restated in revision 7 (DEC-055): the ratio is an integer pair, the same representation a `split` carries in FIF-061, so the two must agree.

## FIF-069 Trade Republic: security type mapping
Status: done
Requirements: IMP-TR-020, IMP-TR-021
Depends on: FIF-027, FIF-005
Acceptance: `asset_class` `STOCK` → `stock`, `FUND` → `fund`, anything else rejects the import; ETFs arrive as `FUND` and are recorded as funds, flagged auto-created for review; no `asset_class` value maps to `bond`, so a Trade Republic bond is rejected by that catch-all, naming the ISIN; nothing creates it pending review.
Notes: New in this revision.
Unblocked in revision 47: DEC-077 closed OQ-006. It corrects DEC-031, which had a Trade Republic bond created pending review. IMP-TR-021 states why `bond` must never be mapped, and IMP-TR-020's catch-all enforces it. The reason belongs in a code comment.
Revision 66: **done**, committed as `ce6e6d4`. `import/trade_republic/security.rs` maps `STOCK` to
`stock` and `FUND` to `fund`; any other `asset_class`, blank included, refuses the file naming the
ISIN, so no value reaches `bond` [IMP-TR-020, IMP-TR-021]. `cargo test --workspace`: **640 passed,
0 failed**.

## FIF-030 Income tax overview report
Status: done
Requirements: DOM-001, DOM-073, DOM-074, DOM-075, DOM-095, DOM-117, DOM-121
Depends on: FIF-015
Acceptance: gain/loss aggregated per year (the year of the disposal) and per account, with columns year, account, proceeds, sell fees, cost, buy fees, gain/loss and **a count of outstanding disposals**, the count being a column rather than a second block so every output format keeps one record shape, and a non-zero count saying in plain words that the year is incomplete wherever the format has room for a sentence; optional account filter and optional tax year filter, unfiltered meaning all accounts and all years; only attributed disposals contribute, and an unattributed one is reported as outstanding rather than counted at zero; a `transfer_out` realizes nothing and contributes no row; the figures are raw gain/loss with no tax-law treatment applied.
Notes: Blocked in revision 2 on DOM-095 and DOM-117, both of which left the undecided list in revision 3. DOM-121 is new in revision 7 (DEC-058); the count column is what keeps DOM-117 from being a silent understatement, so the two are one item. FIF-046's `insta` snapshots and FIF-040's endpoint both carry the extra column.
Revision 75: named next to build; FIF-015 is done and none of DOM-001, DOM-073, DOM-074, DOM-075, DOM-095, DOM-117, DOM-121 is on a `Blocks:` line. A domain-level report over storage, no HTTP (FIF-040 exposes it). Figures sum `allocation.rs`'s per-allocation derived shares (`proceeds`, `sell_fee`, `cost`, `buy_fee`, `gain`) of stored attributions (`storage/attributions.rs`), never recomputed from pooled totals. "Outstanding" is a closing with no approved attribution; an `expiration` cannot be approved (DEC-102), so it counts as outstanding in its disposal year. A `transfer_out` is excluded from both figures and the count (DOM-095). The plain-words sentence belongs to the formats that render it (FIF-046); this item exposes the count.
Done in revision 75: `income_tax.rs` `overview` sums, per year of disposal and per account, the EUR shares and gains `allocation.rs` derives for every allocation of every attributed disposal, with the outstanding count as a column of the same row (DOM-117, DOM-121); a `transfer_out` adds neither figures nor a count (DOM-095); the account and tax year filters select rows without changing a figure (DOM-073). No new decisions. Integration tests in `crates/fifolio-core/tests/income_tax.rs`.

## FIF-031 Acquisition report
Status: done
Requirements: DOM-076, DOM-077, DOM-079, DOM-096, DOM-118
Depends on: FIF-015, FIF-061
Acceptance: every opening transaction listed with date, account, security, effective quantity, remaining quantity, effective unit price, fees and realized gain/loss, the two quantity columns both **as of today** so they share one scale and compare to a broker statement; a `transfer_in` appears as an opening in its own right and names the opening it inherited from; the year filter selects openings having at least one allocation in that year; the account filter applies as in FIF-030.
Notes: Renamed from "Buy report" in revision 2; the report covers both opening variants and the CLI token is `acquisitions` (DEC-034). Blocked in revision 2 on DOM-076 and DOM-078; DOM-076 is now decided and DOM-078, the per-disposal detail lines, moved to FIF-082 in revision 3. What is left is the opening-level table.
Revision 76: named next to build; FIF-015 and FIF-061 are done and none of DOM-076, DOM-077, DOM-079, DOM-096, DOM-118 is on a `Blocks:` line. A domain-level report over storage, no HTTP (FIF-040 exposes it), alongside `income_tax.rs`. Effective quantity and unit price come from `effective_quantity.rs` as of the latest position in canonical order (DOM-118); remaining quantity is `unattributed_quantity` on the same 8-decimal view; realized gain/loss sums `allocation.rs`'s derived per-allocation gains, never pooled. DOM-096 needs a link storage does not yet hold: `emitted_transfer_in` (migration `0003`) records only the `transfer_out`, while `transfer.rs` `emit` already returns each record paired with its parcel's opening id; store that id (new migration) in `attribution.rs`'s approval transaction and name it in the report. An **imported** `transfer_in` (from another broker) inherits from no opening in the system and names none. A `transfer_out` is not an opening and has no row; the year filter keys on allocation years only (DOM-079), the account filter as in FIF-030. The per-disposal lines are FIF-082's.
Done in revision 76: `acquisitions.rs` lists every opening, `buy` or `transfer_in`, in canonical order with date, account, security, effective and remaining quantity as of today (DOM-118), effective unit price, fees and realized gain/loss summed from `allocation.rs`'s per-allocation gains (DOM-076, DOM-077); an emitted `transfer_in` names its inherited opening, stored by migration `0009` on `emitted_transfer_in` and written in `attribution.rs`'s approval transaction (DOM-096); the account and year filters select rows without changing a figure (DOM-079). Provisional decisions DEC-107 (year filter) and DEC-108 (link nulled when the parent is deleted). Integration tests in `crates/fifolio-core/tests/acquisitions.rs`.

## FIF-082 Acquisition report: per-disposal detail lines
Status: done
Requirements: DOM-078
Depends on: FIF-031, FIF-014
Acceptance: beneath each opening, every attributed disposal is listed with its date, quantity consumed, allocated proceeds, allocated sell fee, allocated cost, allocated buy fee and gain/loss.
Blocked by (until revision 69): DOM-078 is on the undecided list.
Notes: Split out of FIF-031 in revision 3.
Revision 69: unblocked by the provisional DEC-093. Acceptance amended: a `transfer_out` is not a disposal and has no line; the parcels it carried appear under the disposals that later consume their emitted `transfer_in` records, with the inherited cost and buy fee. If DEC-093 is overridden on ratification, this item reopens.
Revision 77: named next to build; FIF-031 and FIF-014 are done and DOM-078 is on no `Blocks:` line (OQ-011 provisionally answered by DEC-093). Extends `acquisitions.rs`: each opening row gains its disposal lines, one per allocation to an attributed disposal, taking every figure from `allocation.rs`'s per-allocation derivation (the same shares FIF-030 and FIF-031 sum), never recomputed. Each line carries its disposal's date, so the year is visible (DEC-107); the year filter still selects rows and changes neither the row nor its lines. Under an emitted `transfer_in`, its lines are the disposals consuming it, with the inherited cost and buy fee it carries; under the parent opening, the allocation to the `transfer_out` has no line (DEC-093). Integration tests extend `crates/fifolio-core/tests/acquisitions.rs`.
Done in revision 77: each `acquisitions.rs` row carries its disposal lines, one per allocation to an attributed disposal in canonical order, with the disposal's date, quantity consumed, and allocated proceeds, sell fee, cost, buy fee and gain/loss taken from `allocation.rs`'s `Figures` (DOM-078); the lines reconcile to the row's gain and to the income tax overview (DOM-125); a `transfer_out` has no line and its parcel's disposals appear under the emitted `transfer_in` (DEC-093); the year filter changes no line (DEC-107). Provisional decision DEC-109 (quantity consumed in the units current at the disposal). Integration tests in `crates/fifolio-core/tests/acquisitions.rs`.

## FIF-032 Server binary, arguments and OpenAPI
Status: done
Requirements: SRV-001, SRV-002, SRV-003, SRV-004, SRV-005, SRV-006, ARC-003, ARC-022
Depends on: FIF-011
Acceptance: `fifolio-server` replaces the stub, binds `127.0.0.1:8000` and nothing else, accepts `--port` and `--database` (default `./fifolio.db`); `fifolio-server openapi` prints the spec and exits without binding; `GET /openapi.json` returns the same spec; the server process is the only one that opens the database.
Notes: Done in revision 42's build. `fifolio-server` is a library (`src/lib.rs`: `Args`, `serve`, `router`, `openapi`) behind a thin `main.rs`. The OpenAPI document is assembled by `utoipa-axum` from the handlers' own annotations, so `fifolio-server openapi` and `GET /openapi.json` read one route list [SRV-005, SRV-006]. The database is opened before the socket is bound; `openapi` opens neither. ARC-022 is asserted end to end (127.0.0.2 and `::1` refuse), ARC-003 by the spawned server creating its database and by `dependency_graph.rs` showing the CLI reaches neither `fifolio-core` nor a SQLite driver.

## FIF-033 Problem+json errors and the HTTP test harness
Status: done
Requirements: ARC-020, ARC-021, TST-005
Depends on: FIF-032
Acceptance: every error response is `application/problem+json` per RFC 9457 with a stable machine-readable `type` per error class, and each core error variant maps to exactly one `type`; an integration harness spins the router over a temporary database in process and asserts status codes and body shape. Later server items add their own cases to this harness.
Notes: Named next to build in revision 43. FIF-032 (`8964f18`) left `fifolio-server` a library (`src/lib.rs`: `Args`, `serve`, `router`, `openapi`) whose OpenAPI document `utoipa-axum` assembles from handler annotations; the problem+json schema belongs in that same document, and the harness should drive `router()` rather than a spawned process. `crates/fifolio-server/tests/http_surface.rs` already exists and is the natural home.
"Each core error variant" is read as each variant of an error a handler can hold: `StorageError`, `ImportError` (with `ReadError` through `ImportError::Read`), `RateError`, `FeedError` and `IngestError`. `SaxoError` and `TradeRepublicError` are format-internal and get no problem type: the server imports through `import::import`, and the `Importer` trait answers only `RowError` and `ImportError`, so a format error reaches the server as one of those or not at all. Consequence for whoever writes the Saxo and Trade Republic `Importer` impls (FIF-023 onward, FIF-029): `ImportError` has no variant yet for a format's whole-file refusals (unrecognized or reordered headers, an unjoined detail row, a duplicate identity, a disagreeing quantity, an unknown instrument type, a mixed security type), so those need `ImportError` variants, and each new variant needs its `ProblemType` in `fifolio-server/src/problem.rs`, which the exhaustive match will demand.

## FIF-034 Accounts and securities endpoints
Status: done
Requirements: SRV-007, SRV-008, SRV-009, SRV-010, SRV-011, SRV-057, DOM-126
Depends on: FIF-033
Acceptance: CRUDL for accounts and securities; deleting an account, or changing its broker or id, is refused while a source record, an import batch, a manual entry or a transaction references it (SRV-008 as amended by DEC-089); deleting a security referenced by any source record is refused; creating a security with an existing ISIN is a conflict; type and quotation are editable, which is how an auto-created security is corrected; a security carries `auto_created` (provenance, never changed) and `needs_review` as separate fields; only marking the security reviewed clears `needs_review`, and an edit leaves it set.
Notes: Named next to build in revision 44. Its cases go into the in-process harness FIF-033 left in `crates/fifolio-server/tests/http_surface.rs`, and each refusal maps to a `ProblemType` in `fifolio-server/src/problem.rs`. Revision 54: this item's partial, **unreviewed** code was committed at the user's instruction so the tree is clean; build on it and review it against the specification as if uncommitted.
Revision 67: SRV-008 was **widened** in `e98430a` (DEC-089): the account guard now also covers batches, manual entries and transactions, and refuses a change of broker or id, not only deletion. Acceptance restated. The working tree holds further uncommitted FIF-034 work (`0005_needs_review.sql`, `entities.rs`, `securities.rs`, tests); it is unreviewed, so review it against the specification like the rest.
Revision 68: **done**, committed as `cca68b9`. The account guard in `storage/entities.rs` counts
source records, batches, manual entries and transactions and refuses delete or a change of key,
while writing an account back to its own key is not refused [SRV-008]; migration `0005` stores
`needs_review` apart from `auto_created`, and `POST /securities/{isin}/reviewed` alone clears it
[DOM-126, SRV-057]. Covered in `fifolio-core/tests/storage.rs` and `http_surface.rs`.
`cargo test --workspace`: **649 passed, 0 failed**.

## FIF-035 Import endpoint
Status: done
Requirements: SRV-012, SRV-013, SRV-014, SRV-015, SRV-016, SRV-018, SRV-049
Depends on: FIF-034, FIF-029
Acceptance: the caller supplies target account, format and file; formats are Saxo NL XLSX and Trade Republic DE CSV; unknown ISINs are auto-created and flagged; re-posting the same file changes nothing; non-position rows are counted and not stored; the response **names every unrecognized row type it met with how many rows carried it**; a sell exceeding holdings imports fine and surfaces only at attribution.
Notes: Revision 3 splits the counted summary (SRV-017) into FIF-085, it being undecided; SRV-049's naming of unrecognized types is decided and stays here. Revision 69 (recorded in `6921150` under a duplicate "Revision 58" label; user: server first, importers later): no longer waits on FIF-023. Until the Saxo importer can classify rows, a Saxo file sent to the endpoint is refused as a format not yet supported, naming FIF-023; Trade Republic imports work end to end.
Revision 47: a failed row now refuses the whole import (SRV-058). FIF-099 builds that in core. Here it is one more refusal mapped to problem+json, naming every failed row.
Done in revision 77: the endpoint takes the target account, format and filename as query parameters and the file as the body (SRV-012); a Trade Republic DE CSV imports end to end through `import_service.rs`, and a Saxo NL XLSX is refused as not supported yet, naming FIF-023 (SRV-013); the unknown securities that stored rows name are auto-created and flagged for review, and one ISIN stated with two types is a failed row (SRV-014, DEC-110); posting a file again writes no record or security, only a batch that owns nothing (SRV-015, DEC-111); non-position rows are counted and not stored (SRV-016); the response names every unrecognized row type with its count (SRV-049); a sell exceeding holdings imports (SRV-018); a file with failed rows is refused as problem+json naming every one (SRV-058). Provisional decisions DEC-110 and DEC-111. Tests in `crates/fifolio-core/tests/import_service.rs` and `crates/fifolio-server/tests/http_surface.rs`.

## FIF-085 Import response summary counts
Status: done
Requirements: SRV-017
Depends on: FIF-035, FIF-099
Acceptance: the import response reports four counts: derived automatically, pending, recognized as non-position, and securities auto-created. They reconcile against the rows of the fixture. There is no failed-to-parse count, because a failed row refuses the import (SRV-058, FIF-099).
Notes: Split out of FIF-035 in revision 3. The same five counts are what a batch records (SRV-020, FIF-036), so the two should be reviewed together once this is settled.
Unblocked in revision 47: DEC-074 closed OQ-012 and removed the fifth count from SRV-017. The batch's counts (SRV-020, FIF-036) should be the same four.
Done in revision 78: the import response carries a `summary` with the derived, pending and non-position row counts, which add up to the file's rows with no failed count, and the securities the import auto-created, counting only those not stored before, so a file posted again counts none (SRV-017, DEC-110). The batch keeps its three row counts; the auto-created count is the response's alone (provisional DEC-112). Tests in `crates/fifolio-server/src/imports.rs` and `crates/fifolio-server/tests/http_surface.rs`.

## FIF-070 Multi-year refusal at the endpoint
Status: done
Requirements: SRV-051
Depends on: FIF-035, FIF-065, FIF-103
Acceptance: posting a file whose rows carry trade dates in more than one calendar year is refused at the endpoint with its own problem type, the refusal reaching the caller rather than being swallowed into a generic parse failure.
Notes: Blocked in revision 2 on SRV-056; it and IMP-003 both left the undecided list in revision 3, and SRV-056 re-entered it in revision 8 (OQ-015), so revision 8 splits it into FIF-090. Renamed accordingly; the year refusal is decided on both sides.
Revision 58: now depends on FIF-103. A refusal carrying the years together with other grounds (SRV-059) must reach the caller with every ground, not only the years' problem type.
Revision 79: the Ground-to-problem-type mapping (`multiple-calendar-years`) already exists in `fifolio-server/src/problem.rs` with unit tests; what remains is an HTTP test in `http_surface.rs` posting a multi-year file (alone and combined with another ground) to the import endpoint and asserting the problem+json reaches the caller.
Done in revision 79: posting a file with trade dates in two calendar years to the import endpoint is refused with a `multiple-calendar-years` problem+json; the same file with a failed row among its rows is refused with one `several-grounds` problem whose detail names the years and the failed row (SRV-051, SRV-059, provisional DEC-113). Tests in `crates/fifolio-server/tests/http_surface.rs`.

## FIF-090 Account-mismatch refusal at the endpoint
Status: done
Requirements: SRV-056
Depends on: FIF-070, FIF-089
Acceptance: where the format states an account id it is checked against the target account, and a mismatch, or a file carrying rows from more than one account, refuses the import naming both, as problem+json. A Trade Republic file, which states none, is not checked.
Notes: Split out of FIF-070 in revision 8.
Unblocked in revision 47 with FIF-089 (DEC-075). It still waits on FIF-070 and FIF-089.
Revision 58: an account refusal combined with other grounds (SRV-059, FIF-103) reaches the caller with every ground; FIF-103 arrives through FIF-070.
Revision 80: named next to build. Both dependencies are done. The Ground-to-problem-type mapping (`account-mismatch`, `multiple-accounts`) exists in `fifolio-server/src/problem.rs` with unit tests; what remains is HTTP tests in `http_surface.rs`. A combined refusal is one `several-grounds` problem naming every ground (provisional DEC-113, as in FIF-070).
Done in revision 80: a Trade Republic file, which states no account, is not checked: it imports into two accounts of different ids over HTTP. The account refusal is pinned one step short of the wire: the endpoint's error path renders `account-mismatch` and `multiple-accounts` as 422 problem+json naming both accounts, and a combined refusal as one `several-grounds` problem (SRV-056, SRV-059, DEC-113). No served format states an account yet (Saxo is refused as not supported until FIF-023), so the HTTP case for a Saxo file naming another account is owed by the item that first serves a Saxo import (provisional DEC-114). Tests in `crates/fifolio-server/src/problem.rs` and `crates/fifolio-server/tests/http_surface.rs`.

## FIF-036 Import batch endpoints
Status: done
Requirements: SRV-019, SRV-020, SRV-022
Depends on: FIF-035
Acceptance: every import creates a batch; batches are readable and listable with account, filename, format, timestamp and counts; deletion is refused, naming the offenders, when any derived transaction participates in an attribution or when a record the batch owns is cited by a transaction the batch did not derive.
Notes: Revision 3 splits the deletion behavior itself (SRV-021) into FIF-086; what remains is batch creation, reading and the two refusals, which is reviewable on its own because a refused deletion never reaches the removal path.
Revision 81: named next to build. Its one dependency, FIF-035, is `done`, and none of SRV-019, SRV-020, SRV-022 is on a `Blocks:` line. Batch creation (SRV-019) already happens on every import, including a file posted again (provisional DEC-111), and the endpoint returns the batch id. `ImportBatchRepository` in `fifolio-core/src/storage/entities.rs` has `insert`, `find` and `delete` but no list; the batch keeps three counts, not four (provisional DEC-112). What remains: read and list endpoints, the delete route with SRV-022's two refusals as problem+json naming the offenders, and HTTP tests in `http_surface.rs`. The removal itself stays FIF-086's.
Done in revision 81: every import creates a batch, and `GET /imports` and `GET /imports/{batch}` read and list each with its account, filename, format, timestamp and three counts; a file posted again has a batch of its own (SRV-019, SRV-020, DEC-111, DEC-112). `DELETE /imports/{batch}` refuses as a 409 problem naming the transactions that hold the batch, on the attribution ground first, then on a record it owns cited by a transaction it did not derive (SRV-022); an absent batch is a 404. Where neither refusal holds it answers 501 `batch-removal-not-supported` and removes nothing until FIF-086 (provisional DEC-115). `ImportBatchRepository` gains a list. Tests in `crates/fifolio-core/tests/storage.rs`, `crates/fifolio-core/tests/invariants.rs` and `crates/fifolio-server/tests/http_surface.rs`.

## FIF-086 Batch deletion removes its records
Status: done
Requirements: SRV-021
Depends on: FIF-036, FIF-071
Acceptance: deleting a batch removes exactly the source records it owns and everything derived from them, and never a manual entry.
Blocked by (until revision 69): SRV-021 is on the undecided list. What "the records it owns" means is itself open (SRV-052, FIF-071).
Notes: Revision 69: unblocked by the provisional DEC-092. Acceptance amended: a record the deleted batch owns that a remaining batch also supplied is **not** removed; ownership returns to the newest remaining supplier, so undoing a re-import restores the previous import. Test: import a year, re-import it, delete the newer batch; every record and its derived transactions survive under the older batch. If DEC-092 is overridden on ratification, this item reopens.
Revision 83: named next to build. Both dependencies, FIF-036 and FIF-071, are `done`, and SRV-021 is on no `Blocks:` line. The provisional DEC-116 (added with FIF-071) amends revision 69's test: a transaction keeps the batch that derived it, so after a re-import the newer batch owns records the older batch's transactions cite, and SRV-022 refuses deleting it, naming them. Revision 69's scenario ("every record and its derived transactions survive under the older batch") is therefore a refusal, not a removal, whenever the year derived any transaction; test it as such. DEC-092's return of ownership is tested on re-supplied records no other batch's transaction cites (pending or non-position records), which the deletion must keep, handing each to the newest remaining supplier in `record_supplier` (DEC-117) and dropping the deleted batch as a supplier. What remains: replace the core removal (`delete from source_record where batch_id = ?`, which drops re-supplied records and removes transactions whose records the batch does not own) with SRV-021's rule, removing a transaction only when a record it cites is removed; replace DEC-115's 501 `batch-removal-not-supported` with the removal; manual entries are never removed (DOM-108 lifecycle, FIF-062); tests in `storage.rs`, `invariants.rs`, `import_service.rs` and `http_surface.rs`.
Done in revision 83's build: `ImportBatchRepository::delete` removes the records the batch owns that no other batch supplied, and a transaction only when a record it cites is removed; a record a remaining batch also supplied stays, owned again by the newest remaining supplier in `record_supplier`, and the deleted batch leaves the suppliers; `first_batch_id` stays as stored (SRV-021, DEC-092, DEC-117). A transaction the batch derived from records that stay is kept and re-placed on the newest batch owning a record it cites (provisional DEC-118). An unstored batch is refused as unknown. `DELETE /imports/{batch}` answers 204 and DEC-115's 501 `batch-removal-not-supported` is gone. Manual entries are never touched. Tests in `crates/fifolio-core/tests/storage.rs`, `invariants.rs`, `import_service.rs` and `crates/fifolio-server/tests/http_surface.rs`.

## FIF-071 Batch ownership on re-import
Status: done
Requirements: SRV-052
Depends on: FIF-036
Acceptance: a source record belongs to every batch that supplied it and the **newest** owns it; re-importing a year transfers ownership to the new batch and the superseded batches then own nothing.
Blocked by (until revision 69): SRV-052 is on the undecided list.
Notes: New in this revision. It decides what "the records a batch owns" in SRV-021 and SRV-022 means, so FIF-036 is only fully reviewable once this is settled.
Revision 69: unblocked; OQ-007 is provisionally answered by DEC-092, which leaves SRV-052's rule as written. Storage needs the supplier relation (every batch that supplied a record), not only the owner in `source_record.batch_id`; the oldest supplier FIF-076 keys on must not move when this item moves ownership.
Revision 82: named next to build. Its one dependency, FIF-036, is `done`, and SRV-052 is on no `Blocks:` line. Storage today keeps only the owner (`source_record.batch_id`) and the first supplier (`first_batch_id`); a re-posted record keeps both (`entities.rs`, insert path), so a re-import's batch owns nothing (DEC-111). What remains: the supplier relation (every batch that supplied a record), moving ownership to the newest supplier on re-import while `first_batch_id` stays put, and tests. `invariants.rs` `re_importing_a_year_leaves_the_canonical_order_unchanged` moves ownership in raw SQL "exactly as FIF-071 will"; it should switch to the real path. DEC-115's 501 stays until FIF-086; the existing core removal (`delete from source_record where batch_id = ?`) is FIF-086's to replace, not this item's.
Done in revision 82's build: migration `0010_record_supplier.sql` adds `record_supplier`, every batch that supplied a record, filled from each stored record's owner (provisional DEC-117). An import now supplies each stated record through `SourceRecordRepository::supply`/`supply_in`: it stores the record if absent, records the batch as a supplier and makes it the owner, leaving `first_batch_id` as stored; `suppliers` lists a record's batches oldest first. Transactions keep the batch that derived them (provisional DEC-116), so SRV-022 refuses deleting a re-import while the replaced batch's transactions cite its records. `re_importing_a_year_leaves_the_canonical_order_unchanged` now goes through `supply`. Tests in `crates/fifolio-core/tests/import_service.rs`, `invariants.rs` and `storage.rs`. The core removal still deletes every record the batch owns, re-supplied or not, and still removes the transactions a batch derived though it owns none of their records; FIF-086 replaces it.

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
Status: todo
Requirements: SRV-034
Depends on: FIF-038, FIF-081
Acceptance: the HTTP surface offers no route that constructs a transaction other than by derivation from source records, asserted by a test over the routing table rather than by convention.
Notes: Split out of FIF-038 in revision 3.
Unblocked in revision 47 (OQ-002 closed by DEC-079), with FIF-081.

## FIF-073 Transfer out approval emits its transfer ins
Status: todo
Requirements: SRV-030
Depends on: FIF-038, FIF-063
Acceptance: approving a `transfer_out` also creates the `transfer_in` records it implies, in the same operation.
Notes: Unblocked in revision 47 (OQ-002 closed by DEC-079). It still waits on FIF-063 and FIF-038.

## FIF-039 Attribution endpoints
Status: todo
Requirements: SRV-035, SRV-036, SRV-037, SRV-038, SRV-039, SRV-040, SRV-041, SRV-042, SRV-043, SRV-050
Depends on: FIF-038, FIF-015
Acceptance: a proposal endpoint returns the closing, the proposed allocations with derived figures and a fingerprint, and is also addressable as "the next closing awaiting attribution" for an account and security; the fingerprint covers the closing, every allocation's opening id and quantity **and every derived money figure displayed**, as a hash of a canonical serialization stable across processes, so a re-rating between display and approval changes it; an uncoverable closing returns the named shortfall instead; a security with pending records is refused, naming what is outstanding; creating an attribution requires the fingerprint the client was shown, and a mismatch is a conflict; attributions can be read, listed and deleted but never updated; deletion is refused when a later attribution exists for that account and security; declining is not an API call.
Notes: DEC-104 (provisional) places DOM-054's "proposal written unchanged" here, not in core's `attribution::approve`, which stores any allocations passing DOM-018/019/020/065 and DEC-103. Creation must recompute the proposal and its fingerprint and refuse a differing fingerprint or allocation set as a conflict; this item carries the test that a valid non-FIFO allocation set (for example, skipping the oldest opening) is refused.

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
Notes: Named next to build in revision 68. The cache, seeding and the rate source it fetches through already exist in `fifolio-core` (`ecb.rs`, `fx.rs`, FIF-010, FIF-095); this item exposes them over HTTP, so a fake source goes into the FIF-033 harness in `crates/fifolio-server/tests/http_surface.rs` and each failure maps to a `ProblemType` in `fifolio-server/src/problem.rs`.

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

* **FIF-097 — The stored rate convention against Trade Republic's changed `fx_rate`.** Retired in revision 47. It existed only to re-check FIF-008's DOM-086 once OQ-021 was answered. DEC-073 answered it without changing DOM-086's rule: no Trade Republic `fx_rate` is ever stored. The check therefore has nothing left to find, and DOM-086 returns to FIF-008, which implemented it. Recorded on FIF-008.

# Revision history

**Revision 83.** A status reconciliation after an interrupted session. `design/` changed since
revision 82 only by adding the provisional DEC-116 (a re-import moves a record's ownership, not a
transaction's deriving batch) and DEC-117 (records stored before the supplier relation have their
owner as their only supplier), both listed under "Provisionally answered" against FIF-071; no
requirement id added or removed. `open-questions.md` names the same **7** ids on `Blocks:` lines,
and the 5 items carrying one are exactly the 5 `blocked`.

* **Completed: FIF-071** (`17b3bb5`). Its item text already recorded this. DEC-116 and DEC-117
  invalidate no completed work. DEC-116 does amend FIF-086's revision-69 test, which it turns into
  an SRV-022 refusal whenever the re-imported year derived a transaction; recorded on FIF-086.
* Flag for `spec-auditor`, not a blocker: under DEC-116 and SRV-022 as written, DEC-092's stated
  purpose ("undoing a re-import restores the previous import") is reachable only for a year that
  derived no transaction. The specification is determinate, so the plan follows it.
* No item added, split, retired, blocked or unblocked. 101 items: **63 `done`, 33 `todo`, 5
  `blocked`**. Coverage unchanged: **352** live ids, each on exactly one item; uncovered only the
  retired DOM-009, 014, 015, 021, 041 and 050 to 053.
* Server scope for this run: FIF-086, 037, 072, 038, 087, 073, 039, 040, 041 remain `todo`; none
  carries a blocked requirement, so each waits only on dependencies inside that list.
* **Next to build: FIF-086.** Its dependencies FIF-036 and FIF-071 are `done`. FIF-037 and FIF-041
  are also ready but come later in document order.

**Revision 82.** A status reconciliation after an interrupted session. `design/` changed since
revision 81 only by adding the provisional DEC-115 (a batch deletion no refusal holds answers 501
`batch-removal-not-supported` and removes nothing until FIF-086), listed under "Provisionally
answered" against FIF-036; no requirement id added or removed. `open-questions.md` names the same
**7** ids on `Blocks:` lines, and the 5 items carrying one are exactly the 5 `blocked`.

* **Completed: FIF-036** (`780bdba`). Its item text already recorded this. DEC-115 invalidates no
  completed work; it constrains FIF-086, which replaces the 501 with the removal.
* No item added, split, retired, blocked or unblocked. 101 items: **62 `done`, 34 `todo`, 5
  `blocked`**. Coverage unchanged: **352** live ids, each on exactly one item; uncovered only the
  retired DOM-009, 014, 015, 021, 041 and 050 to 053.
* Server scope for this run: FIF-071, 086, 037, 072, 038, 087, 073, 039, 040, 041 remain `todo`;
  none carries a blocked requirement, so each waits only on dependencies inside that list.
* **Next to build: FIF-071.** Its dependency FIF-036 is `done`. FIF-086 waits on it; FIF-037
  (dependency FIF-035, `done`) is also ready but comes later in document order.

**Revision 81.** A status reconciliation after an interrupted session. `design/` changed since
revision 80 only by adding the provisional DEC-114 (testing the account refusal while no served
format states an account), listed under "Provisionally answered" against FIF-090; no requirement id
added or removed. `open-questions.md` names the same **7** ids on `Blocks:` lines, and the 5 items
carrying one are exactly the 5 `blocked`.

* **Completed: FIF-090** (`2a60a9d`). Its item text already recorded this; only the history lagged.
  DEC-114 invalidates no completed work.
* No item added, split, retired, blocked or unblocked. 101 items: **61 `done`, 35 `todo`, 5
  `blocked`**. Coverage unchanged: **352** live ids, each on exactly one item; uncovered only the
  retired DOM-009, 014, 015, 021, 041 and 050 to 053.
* Server scope for this run: FIF-036, 071, 086, 037, 072, 038, 087, 073, 039, 040, 041 remain
  `todo`; none carries a blocked requirement, so each waits only on dependencies inside that list.
* **Next to build: FIF-036.** Its dependency FIF-035 is `done`; batch creation already happens on
  every import (DEC-111), so what remains is read, list and the refused delete (SRV-022).

**Revision 80.** A status reconciliation after an interrupted session. `design/` changed in
`c776fa4` only by adding the provisional DEC-113 (a refusal on several grounds is one
`several-grounds` problem whose detail names every ground), listed under "Provisionally answered"
against FIF-070; no requirement id added or removed. `open-questions.md` names the same **7** ids on
`Blocks:` lines, and the 5 items carrying one are exactly the 5 `blocked`.

* **Completed: FIF-070** (`c776fa4`). Its item text already recorded this; only the history lagged.
  DEC-113 invalidates no completed work: FIF-103's core refusal already carries every ground, and
  DEC-113 only fixes how the server renders them.
* No item added, split, retired, blocked or unblocked. 101 items: **60 `done`, 36 `todo`, 5
  `blocked`**. Coverage unchanged: **352** live ids, each on exactly one item; uncovered only the
  retired DOM-009, 014, 015, 021, 041 and 050 to 053.
* Server scope for this run: FIF-090, 036, 071, 086, 037, 072, 038, 087, 073, 039, 040, 041 remain
  `todo`; none carries a blocked requirement, so each waits only on dependencies inside that list.
* **Next to build: FIF-090.** Its dependencies FIF-070 and FIF-089 are `done`; the
  `account-mismatch` and `multiple-accounts` problem types already exist in
  `fifolio-server/src/problem.rs`, so what remains is HTTP tests in `http_surface.rs` (mismatch,
  several accounts, combined with another ground per DEC-113, and a Trade Republic file unchecked).
  Also ready in scope: FIF-036, FIF-037, FIF-041.

**Revision 79.** A status reconciliation after an interrupted session. `design/` changed in
`c3447f7` only by adding the provisional DEC-112 (a batch keeps its three row counts; the
auto-created count is the import response's alone), listed under "Provisionally answered"; no
requirement id added or removed. `open-questions.md` names the same **7** ids on `Blocks:` lines,
and the 5 items carrying one are exactly the 5 `blocked`.

* **Completed: FIF-085** (`c3447f7`). Its item text already recorded this; only the history lagged.
  DEC-112 is consistent with FIF-036's acceptance (batch counts) and invalidates no completed work.
* Checked against the code: `fifolio-server/src/problem.rs` already maps the refusal grounds to the
  `multiple-calendar-years`, `account-mismatch` and `multiple-accounts` problem types, unit tested
  there, but no test in `fifolio-server/tests/http_surface.rs` posts a multi-year or mismatched file
  to the import endpoint, so FIF-070 and FIF-090 stay `todo`.
* No item added, split, retired, blocked or unblocked. 101 items: **59 `done`, 37 `todo`, 5
  `blocked`**. Coverage unchanged: **352** live ids, each on exactly one item; uncovered only the
  retired DOM-009, 014, 015, 021, 041 and 050 to 053.
* Server scope for this run: FIF-070, 090, 036, 071, 086, 037, 072, 038, 087, 073, 039, 040, 041
  remain `todo`; none carries a blocked requirement, so each waits only on dependencies inside that
  list.
* **Next to build: FIF-070.** Its dependencies FIF-035, FIF-065 and FIF-103 are `done`. Also ready
  in scope: FIF-036, FIF-037, FIF-041.

**Revision 78.** A status reconciliation after an interrupted session. `design/` changed in
`8f89f39` only by adding the provisional DEC-110 (an import auto-creates only the securities its
stored rows name; one ISIN of two types is a failed row) and DEC-111 (posting a file again writes a
batch that owns nothing), both listed under "Provisionally answered"; no requirement id added or
removed. `open-questions.md` names the same **7** ids on `Blocks:` lines, and the 5 items carrying
one are exactly the 5 `blocked`.

* **Completed: FIF-082** (`f19b2bc`) and **FIF-035** (`8f89f39`). Their item text already recorded
  this; only the history lagged. Neither provisional decision invalidates completed work.
* Checked against the code: the import response (`fifolio-server/src/imports.rs`) returns the batch
  id and the unrecognized types but none of SRV-017's four counts, so FIF-085 stays `todo`. The
  batch row itself is already written by FIF-035 (DEC-111); FIF-036 exposes and reads it.
* No item added, split, retired, blocked or unblocked. 101 items: **58 `done`, 38 `todo`, 5
  `blocked`**. Coverage unchanged: **352** live ids, each on exactly one item; uncovered only the
  retired DOM-009, 014, 015, 021, 041 and 050 to 053.
* Server scope for this run: FIF-085, 070, 090, 036, 071, 086, 037, 072, 038, 087, 073, 039, 040,
  041 remain `todo`; none carries a blocked requirement, so each waits only on dependencies inside
  that list.
* **Next to build: FIF-085.** Its dependencies FIF-035 and FIF-099 are `done`. Also ready in
  scope: FIF-070, FIF-036, FIF-037, FIF-041.

**Revision 77.** A status reconciliation. `design/` changed in `5f45c9b` only by adding the
provisional DEC-107 (the acquisition report's year filter changes no figure) and DEC-108 (an emitted
`transfer_in`'s parent link is nulled on the parent's deletion), both listed under "Provisionally
answered"; no requirement id added or removed. `open-questions.md` names the same **7** ids on
`Blocks:` lines, and the 5 items carrying one are exactly the 5 `blocked`.

* **Completed: FIF-031**, committed as `5f45c9b` (`acquisitions.rs`, migration `0009`). Its item
  text already recorded this; only the history lagged. DEC-107 and DEC-108 are its provisional
  decisions; neither invalidates completed work.
* No item added, split, retired, blocked or unblocked. 101 items: **56 `done`, 40 `todo`, 5
  `blocked`**. Coverage unchanged: **352** live ids, each on exactly one item; uncovered only the
  retired DOM-009, 014, 015, 021, 041 and 050 to 053.
* Server scope for this run: FIF-082, 035, 085, 070, 090, 036, 071, 086, 037, 072, 038, 087, 073,
  039, 040, 041 remain `todo`; none carries a blocked requirement, so each waits only on
  dependencies inside that list.
* **Next to build: FIF-082.** Its dependencies FIF-031 and FIF-014 are `done`. Also ready in
  scope: FIF-035, FIF-041.

**Revision 76.** A status reconciliation. `design/` did not change in `859a4c7`; no requirement id
added or removed. `open-questions.md` names the same **7** ids on `Blocks:` lines (DOM-092, DOM-114,
IMP-SAXO-013, ARC-008, DOM-038, DOM-104, IMP-SAXO-026), and the 5 items carrying one are exactly the
5 `blocked`.

* **Completed: FIF-030**, committed as `859a4c7` (`income_tax.rs`). Its item text already recorded
  this; only the history lagged.
* No item added, split, retired, blocked or unblocked. 101 items: **55 `done`, 41 `todo`, 5
  `blocked`**. Coverage unchanged: **352** live ids, each on exactly one item; uncovered only the
  retired DOM-009, 014, 015, 021, 041 and 050 to 053.
* Server scope for this run: FIF-031, 082, 035, 085, 070, 090, 036, 071, 086, 037, 072, 038, 087,
  073, 039, 040, 041 remain `todo`; none carries a blocked requirement, so each waits only on
  dependencies inside that list.
* Found while naming FIF-031: DOM-096's inherited-opening link is not stored (`emitted_transfer_in`
  holds only the `transfer_out`); recorded on FIF-031 as part of its work, not a spec change.
* **Next to build: FIF-031.** Its dependencies FIF-015 and FIF-061 are `done`. Also ready in
  scope: FIF-035, FIF-041.

**Revision 75.** A status reconciliation. `design/` changed in `9ba55b3` only by adding the
provisional DEC-105 and DEC-106 and listing them under "Provisionally answered"; no requirement id
added or removed. `open-questions.md` names the same **7** ids on `Blocks:` lines (DOM-092, DOM-114,
IMP-SAXO-013, ARC-008, DOM-038, DOM-104, IMP-SAXO-026), and the 5 items carrying one are exactly the
5 `blocked`.

* **Completed: FIF-063**, committed as `9ba55b3` (`transfer.rs`, `attribution.rs`), on the
  provisional DEC-105 and DEC-106.
* No item added, split, retired, blocked or unblocked. 101 items: **54 `done`, 42 `todo`, 5
  `blocked`**. Coverage unchanged: **352** live ids, each on exactly one item; uncovered only the
  retired DOM-009, 014, 015, 021, 041 and 050 to 053.
* Server scope for this run: FIF-030, 031, 082, 035, 085, 070, 090, 036, 071, 086, 037, 072, 038,
  087, 073, 039, 040, 041 remain `todo`; none carries a blocked requirement, so each waits only on
  dependencies inside that list.
* **Next to build: FIF-030.** Its dependency FIF-015 is `done`. Also ready in scope: FIF-031,
  FIF-035, FIF-041.

**Revision 74.** A status reconciliation. `design/` changed in `07f2433` only by adding the
provisional DEC-102 to DEC-104 and listing them under "Provisionally answered"; no requirement id
added or removed. `open-questions.md` names the same **7** ids on `Blocks:` lines, and the 5 items
carrying one are exactly the 5 `blocked` (checked by script).

* **Completed: FIF-015**, committed as `07f2433` (`attribution.rs`), on the provisional DEC-102 to
  DEC-104. Its item text already recorded this; only the history lagged.
* No item added, split, retired, blocked or unblocked. 101 items: **53 `done`, 43 `todo`, 5
  `blocked`**. Coverage unchanged: **352** live ids, each on exactly one item; uncovered only the
  retired DOM-009, 014, 015, 021, 041 and 050 to 053.
* **Next to build: FIF-063.** Its dependencies FIF-015 and FIF-061 are `done`, and none of DOM-090,
  DOM-106, DOM-107, DOM-115 is blocked. Also ready in scope: FIF-030, FIF-031, FIF-035, FIF-041.

**Revision 73.** A status reconciliation. `design/` changed in `6d9916e` only by adding the
provisional DEC-100 and DEC-101 and listing them under "Provisionally answered"; no requirement id
added or removed. `open-questions.md` names the same **7** ids on `Blocks:` lines, and the 5 items
carrying one are exactly the 5 `blocked`.

* **Completed: FIF-014**, committed as `6d9916e` (`allocation.rs`), on the provisional DEC-100 and
  DEC-101. Its item text already recorded this; only the history lagged.
* No item added, split, retired, blocked or unblocked. 101 items: **52 `done`, 44 `todo`, 5
  `blocked`**. Coverage unchanged: **352** live ids, each on exactly one item; uncovered only the
  retired DOM-009, 014, 015, 021, 041 and 050 to 053.
* **Next to build: FIF-015.** Its dependencies FIF-014 and FIF-012 are `done`, and none of DOM-018,
  DOM-019, DOM-020, DOM-054, DOM-055 is blocked. Also ready in scope: FIF-035, FIF-041.

**Revision 72.** A status reconciliation. `design/` changed in `88597d5` only by adding the
provisional DEC-098 and DEC-099 and listing them under "Provisionally answered"; no requirement id
added or removed. `open-questions.md` names the same **7** ids on `Blocks:` lines, and the 5 items
carrying one are exactly the 5 `blocked` (checked by script).

* **Completed: FIF-013**, committed as `88597d5` (`fifo.rs`), on the provisional DEC-098 and
  DEC-099, relying on DEC-091, DEC-095 and DEC-097.
* No item added, split, retired, blocked or unblocked. 101 items: **51 `done`, 45 `todo`, 5
  `blocked`**. Coverage unchanged: **352** live ids, each on exactly one item; uncovered only the
  retired DOM-009, 014, 015, 021, 041 and 050 to 053.
* **Next to build: FIF-014.** Its dependencies FIF-013, FIF-061, FIF-004 and FIF-054 are `done`,
  and none of its requirements is blocked. Also ready in scope: FIF-035, FIF-041.

**Revision 71.** A status reconciliation. `design/` changed in `aa69414` only by adding the
provisional DEC-097 and listing it under "Provisionally answered"; no requirement id added or
removed. `open-questions.md` names the same **7** ids on `Blocks:` lines, and the 5 items carrying
one are exactly the 5 `blocked` (checked by script).

* **Completed: FIF-061**, committed as `aa69414` (migration `0007_split_ratio.sql`), on the
  provisional DEC-091 and DEC-097.
* No item added, split, retired, blocked or unblocked. 101 items: **50 `done`, 46 `todo`, 5
  `blocked`**. Coverage unchanged: **352** live ids, each on exactly one item; uncovered only the
  retired DOM-009, 014, 015, 021, 041 and 050 to 053.
* **Next to build: FIF-013.** Its dependencies FIF-076, FIF-056 and FIF-061 are `done`, and neither
  DOM-056 nor DOM-057 is blocked. Also ready in scope: FIF-035, FIF-041.

**Revision 70.** A status reconciliation. `design/` changed in `8293c03` only by adding the
provisional DEC-094 to DEC-096 and listing them under "Provisionally answered" in
`open-questions.md`; no requirement id added or removed. `open-questions.md` names the same **7** ids
on `Blocks:` lines, and the 5 items carrying one are exactly the 5 `blocked` (checked by script).

* **Completed: FIF-076**, committed as `8293c03` (migration `0006_canonical_order.sql`), on the
  provisional DEC-090, DEC-092, DEC-094, DEC-095, DEC-096.
* No item added, split, retired, blocked or unblocked. 101 items: **49 `done`, 47 `todo`, 5
  `blocked`**. Coverage unchanged: **352** live ids, each on exactly one item; uncovered only the
  retired DOM-009, 014, 015, 021, 041 and 050 to 053.
* **Next to build: FIF-061.** Its dependencies FIF-076 and FIF-054 are `done`, and none of DOM-089,
  DOM-103, DOM-113 is blocked (DOM-113 rests on the provisional DEC-091). FIF-013 precedes it in
  document order but depends on it. Also ready in scope: FIF-035, FIF-041.

**Revision 69.** `design/` **changed** in `6921150`: OQ-001, OQ-004, OQ-007 and OQ-011 are
provisionally answered (DEC-090 to DEC-093), awaiting the user's ratification; DOM-011, DOM-064,
DOM-065, DOM-078, DOM-111 and SRV-021 were reworded to carry the answers. No id added or removed.
The user put the server first and the importers, CLI and UI later.

That commit first recorded this revision as a second "Revision 58" with no status changes; it is
renumbered here, its text kept in substance: "OQ-001, OQ-004, OQ-007 and OQ-011 are provisionally
answered (DEC-090 to DEC-093), which unblocks FIF-076, FIF-061, FIF-071, FIF-086 and FIF-082 and with
them the engine the attribution and report endpoints need. FIF-035 no longer waits on FIF-023."

* **Unblocked (now `todo`), acceptance amended by note, not rewritten:** FIF-076 (DEC-090, DEC-092),
  FIF-058 (DEC-090), FIF-078 (DEC-091), FIF-061 (DEC-091), FIF-080 (DEC-093), FIF-100 (DEC-090),
  FIF-082 (DEC-093), FIF-086 (DEC-092), FIF-071 (DEC-092). Each records that an override on
  ratification reopens it.
* **Still `blocked`:** FIF-075, FIF-077 (OQ-010), FIF-079 (OQ-005), FIF-066 (OQ-013), FIF-023
  (OQ-005, OQ-008, OQ-014). `open-questions.md` now names **7** ids on `Blocks:` lines, and the 5
  items carrying one are exactly the 5 `blocked` (checked by script).
* **Invalidated completed work:** none. SRV-021's rewording touches FIF-086, never built; DOM-111's
  touches FIF-076, never built. FIF-012's stand-in order and FIF-076's replacing it were already
  recorded.
* No item added, split or retired. 101 items: **48 `done`, 48 `todo`, 5 `blocked`**. Coverage:
  **352** live ids, each on exactly one item; uncovered only the retired DOM-009, 014, 015, 021, 041
  and 050 to 053.
* **Server scope** (user's instruction): FIF-076, 061, 013, 014, 015, 063, 030, 031, 082, 035, 085,
  070, 090, 036, 071, 086, 037, 072, 038, 087, 073, 039, 040, 041. The `todo` engine items outside it
  (FIF-058, FIF-060, FIF-078, FIF-080, FIF-100) are not selected; FIF-039's refusal of a security
  with pending records is DOM-067 in FIF-060, which lies outside that scope.
* **Next to build: FIF-076.** Its dependencies FIF-006 and FIF-056 are `done`, and none of DOM-011,
  DOM-013, DOM-111 is blocked. It is the first ready in-scope `todo` in document order. Also ready
  in scope: FIF-035 and FIF-041. FIF-061 follows once FIF-076 is done.

**Revision 68.** A status reconciliation. `design/` is **unchanged** since `e98430a`.
`open-questions.md` names the same **18** ids, and the 14 items carrying one are exactly the 14
`blocked` (checked by script); none is unblocked.

* **Completed: FIF-034**, committed as `cca68b9`, reviewed against SRV-007 to SRV-011, SRV-057 and
  DOM-126 as widened by DEC-089; suite 649 passed, 0 failed.
* No item added, split, retired, blocked or unblocked. 101 items: **48 `done`, 39 `todo`, 14
  `blocked`**. Coverage: **352** live ids, each on exactly one item; uncovered only the retired
  DOM-009, 014, 015, 021, 041 and 050 to 053.
* **Next to build: FIF-041.** Its dependencies FIF-033 and FIF-010 are `done`, and neither SRV-046
  nor SRV-047 is blocked. Every `todo` before it waits on a `blocked` item, directly or
  transitively (FIF-058, FIF-061, FIF-063 via FIF-015, FIF-023, FIF-076). Also ready: FIF-042.

**Revision 67.** `design/` **changed** in `e98430a` (DEC-089): SRV-008 now refuses deleting an
account, or changing its broker or id, while a source record, batch, manual entry or transaction
references it. No new id. `open-questions.md` unchanged: the same **18** ids, and the 14 items
carrying one are exactly the 14 `blocked` (checked by script).

* **Amended: FIF-034** (`todo`), which carries SRV-008; acceptance restated. No `done` item carries
  SRV-008, so no completed work is invalidated.
* No item added, split, retired, blocked or unblocked. 101 items: **47 `done`, 40 `todo`, 14
  `blocked`**. Coverage: **352** live ids, each on exactly one item.
* **Next to build: FIF-034.** Also ready: FIF-041, FIF-042.

**Revision 66.** A status reconciliation. `design/` is **unchanged** since `951cb67`.
`open-questions.md` names the same **18** ids, and the 14 items carrying one are exactly the 14
`blocked` (checked by script); none is unblocked.

* **Completed: FIF-069**, committed as `ce6e6d4`; suite 640 passed, 0 failed.
* No item added, split, retired, blocked or unblocked. 101 items: **47 `done`, 40 `todo`, 14
  `blocked`**.
* Coverage re-verified by script: **352** live ids, each on exactly one item. Uncovered: only the
  retired DOM-009, 014, 015, 021, 041 and 050 to 053.
* **Next to build: FIF-034.** Its dependency FIF-033 is `done`, and none of its requirements is
  blocked. Every `todo` before it waits on a `blocked` item, directly or transitively (FIF-058,
  FIF-061, FIF-063 via FIF-015, FIF-023, FIF-076). Also ready: FIF-041, FIF-042.

**Revision 65.** A status reconciliation. `design/` is **unchanged** since `951cb67`.
`open-questions.md` names the same **18** ids, and the 14 items carrying one are exactly the 14
`blocked` (checked by script); none is unblocked.

* **Completed: FIF-029**, committed as `365fefd`; suite 634 passed, 0 failed.
* No item added, split, retired, blocked or unblocked. 101 items: **46 `done`, 41 `todo`, 14
  `blocked`**.
* Coverage re-verified by script: **352** live ids, each on exactly one item. Uncovered: only the
  retired DOM-009, 014, 015, 021, 041 and 050 to 053.
* **Next to build: FIF-069.** Its dependencies FIF-027 and FIF-005 are `done`, and neither
  IMP-TR-020 nor IMP-TR-021 is blocked. It is the first ready `todo` in document order. Also ready:
  FIF-034, FIF-041, FIF-042. FIF-068 still waits on FIF-063.

**Revision 64.** A status reconciliation. `design/` is **unchanged** since `951cb67`.
`open-questions.md` names the same **18** ids, and the 14 items carrying one are exactly the 14
`blocked` (checked by script); none is unblocked.

* **Completed: FIF-028**, committed as `e395518`; suite 615 passed, 0 failed.
* **Amended:** FIF-029 note (the IMP-TR-017 refusal is wholly its own).
* No item added, split, retired, blocked or unblocked. 101 items: **45 `done`, 42 `todo`, 14
  `blocked`**.
* Coverage re-verified by script: **352** live ids, each on exactly one item. Uncovered: only the
  retired DOM-009, 014, 015, 021, 041 and 050 to 053.
* **Next to build: FIF-029.** Its dependencies FIF-028 and FIF-056 are `done`, and none of its
  requirements is blocked. It is the first ready `todo` in document order. Also ready: FIF-069,
  FIF-034, FIF-041, FIF-042.

**Revision 63.** A status reconciliation. `design/` is **unchanged** since `951cb67`.
`open-questions.md` names the same **18** ids, and the 14 items carrying one are exactly the 14
`blocked` (checked by script); none is unblocked.

* **Completed: FIF-084**, committed as `18cf73a`; suite 618 passed, 0 failed.
* No item added, split, retired, blocked or unblocked. 101 items: **44 `done`, 43 `todo`, 14
  `blocked`**.
* Coverage re-verified by script: **352** live ids, each on exactly one item. Uncovered: only the
  retired DOM-009, 014, 015, 021, 041 and 050 to 053.
* **Next to build: FIF-028.** Its dependencies are `done` and none of its requirements is blocked.
  It is the first ready `todo` in document order. Also ready: FIF-069, FIF-034, FIF-041, FIF-042.

**Revision 62.** A status reconciliation. `design/` is **unchanged** since `951cb67`.
`open-questions.md` names the same **18** ids, and the 14 items carrying one are exactly the 14
`blocked` (checked by script); none is unblocked.

* **Completed: FIF-083**, committed as `f9a718c`; suite 612 passed, 0 failed.
* No item added, split, retired, blocked or unblocked. 101 items: **43 `done`, 44 `todo`, 14
  `blocked`**.
* Coverage re-verified by script: **352** live ids, each on exactly one item. Uncovered: only the
  retired DOM-009, 014, 015, 021, 041 and 050 to 053.
* **Next to build: FIF-084.** Its dependencies FIF-020 and FIF-083 are `done`, and IMP-SAXO-008 is
  not blocked. It is the first ready `todo` in document order. Also ready: FIF-028, FIF-069,
  FIF-034, FIF-041, FIF-042.

**Revision 61.** A status reconciliation. `design/` is **unchanged** since `951cb67`.
`open-questions.md` names the same **18** ids, and the 14 items carrying one are exactly the 14
`blocked` (checked by script); none is unblocked.

* **Completed: FIF-103**, committed as `5e225eb`; suite 609 passed, 0 failed.
* No item added, split, retired, blocked or unblocked. 101 items: **42 `done`, 45 `todo`, 14
  `blocked`**.
* Coverage re-verified by script: **352** live ids, each on exactly one item. Uncovered: only the
  retired DOM-009, 014, 015, 021, 041 and 050 to 053.
* **Next to build: FIF-083.** Its dependencies FIF-019 and FIF-006 are `done`, and IMP-SAXO-025 is
  not blocked. It is the first ready `todo` in document order. Also ready: FIF-028, FIF-069,
  FIF-034, FIF-041, FIF-042.

**Revision 60.** A status reconciliation. `design/` is **unchanged** since `951cb67`.
`open-questions.md` names the same **18** ids, and the 14 items carrying one are exactly the 14
`blocked` (checked by script); none is unblocked.

* **Completed: FIF-089**, committed as `d086bf8`; suite 602 passed, 0 failed.
* No item added, split, retired, blocked or unblocked. 101 items: **41 `done`, 46 `todo`, 14
  `blocked`**.
* Coverage re-verified by script: **352** live ids, each on exactly one item. Uncovered: only the
  retired DOM-009, 014, 015, 021, 041 and 050 to 053.
* **Next to build: FIF-103.** Its dependencies FIF-099 and FIF-089 are both `done`, and SRV-059 is
  not blocked. It is the first ready `todo` in document order and corrects completed work in
  FIF-065. Also ready: FIF-083, FIF-028, FIF-069, FIF-034, FIF-041, FIF-042.

**Revision 59.** A status reconciliation. `design/` is **unchanged** since `951cb67`.
`open-questions.md` names the same **18** ids, and the 14 items carrying one are exactly the 14
`blocked` (checked by script); none is unblocked.

* **Completed: FIF-099**, committed as `116be0c`; suite 596 passed, 0 failed.
* No item added, split, retired, blocked or unblocked. 101 items: **40 `done`, 47 `todo`, 14
  `blocked`**.
* Coverage re-verified by script: **352** live ids, each on exactly one item. Uncovered: only the
  retired DOM-009, 014, 015, 021, 041 and 050 to 053.
* **Next to build: FIF-089.** Its dependencies FIF-017 and FIF-065 are `done`, and IMP-003 is not
  blocked. It is the remaining dependency of FIF-103. Also ready: FIF-083, FIF-028, FIF-069,
  FIF-034, FIF-041, FIF-042.

**Revision 58.** `design/` **changed** in `951cb67` (DEC-088): new requirement SRV-059 (an import
refused on several grounds reports all of them), and the batch's `failed` count leaves DOM-017.
`open-questions.md` is unchanged: the same **18** ids, and the 14 items carrying one are exactly the
14 `blocked`; none is unblocked.

* **Added: FIF-103** (`todo`), carrying SRV-059. It puts completed work in FIF-065 in question
  (the year guard refuses on the years alone); recorded on both.
* **Amended:** FIF-099 notes (DOM-017's failed count is in its scope; SRV-059 answers revision 57's
  year-guard question), FIF-070 now depends on FIF-103, FIF-090 note.
* No item split, retired, completed, blocked or unblocked. 101 items: **39 `done`, 48 `todo`, 14
  `blocked`**.
* Coverage re-verified by script: **352** live ids, each on exactly one item. Uncovered: only the
  retired DOM-009, 014, 015, 021, 041 and 050 to 053.
* **Next to build: FIF-099.** Its one dependency, FIF-017, is `done`, and SRV-058 is not blocked.
  Also ready: FIF-089, FIF-083, FIF-028, FIF-069, FIF-034, FIF-041, FIF-042.

**Revision 57.** `design/` **changed** in `242ab6a` (DEC-087): SRV-058 now defines a failed row as
one whose ordering key, identity or classification cannot be read. Revision 56's claim that
`design/` was unchanged since `d55a1b5` held when it was written and no longer does.
`open-questions.md` is unchanged: the same **18** ids, and the 14 items carrying one are exactly the
14 `blocked`; none is unblocked.

* **Amended: FIF-099** (still `todo`). Acceptance widened to the ordering key. This further
  invalidates completed work in FIF-017 (`ImportError::Unorderable` as a first-row stop) and
  FIF-033 (its `unorderable-row` mapping); recorded on FIF-099, which already exists to correct
  FIF-017 for SRV-058. Uncommitted partial code for FIF-099 is in the tree and predates DEC-087.
* No item added, split, retired, completed, blocked or unblocked. 100 items: **39 `done`, 47
  `todo`, 14 `blocked`**, counted by script.
* Coverage re-verified by script: **351** live ids, each on exactly one item; no id added or
  removed by DEC-087. Uncovered: only the retired DOM-009, 014, 015, 021, 041 and 050 to 053.
* **Next to build: FIF-099.** Its one dependency, FIF-017, is `done`, and SRV-058 is not blocked.
  Also ready: FIF-089, FIF-083, FIF-028, FIF-069, FIF-034, FIF-041, FIF-042.

**Revision 56.** A status reconciliation. `design/` is **unchanged** since revision 52 (last touched
in `d55a1b5`; no uncommitted change). `open-questions.md` names the same **18** ids, and the 14
items carrying one are exactly the 14 `blocked`; none is unblocked.

* **Completed: FIF-081**, committed as `fdad876`, `6a8b5ba`, `35b3374`, status flip in `1ce1a9f`.
* No item added, split, retired, blocked or unblocked. 100 items: **39 `done`, 47 `todo`, 14
  `blocked`**, counted by script.
* Coverage re-verified by script: every live id on exactly one item's `Requirements:` line, none on
  an item without being in `design/`. Uncovered: only the retired DOM-009, 014, 015, 021, 041 and
  050 to 053.
* **Next to build: FIF-099.** Its one dependency, FIF-017, is `done`, and SRV-058 is not blocked.
  Also ready: FIF-089, FIF-083, FIF-028, FIF-069, FIF-034, FIF-041, FIF-042. FIF-028 and FIF-034
  still carry the unreviewed partial code committed in revision 54.

**Revision 55.** A status reconciliation. `design/` is **unchanged** since revision 52 (last touched
in `d55a1b5`; no uncommitted change). `open-questions.md` names the same **18** ids, and the 14
items carrying one are exactly the 14 `blocked`; none is unblocked.

* No item added, split, retired, completed, blocked or unblocked. 100 items: **38 `done`, 48
  `todo`, 14 `blocked`**, counted by script (revision 54 made FIF-102 `done`).
* Coverage re-verified by script: every live id on exactly one item's `Requirements:` line.
  Uncovered: only the retired DOM-009, 014, 015, 021, 041 and 050 to 053.
* **Next to build: FIF-081.** Its dependencies FIF-017 and FIF-102 are `done`, and DOM-047 is not
  blocked. Everything ahead of it in document order is `done`, `blocked`, or waits on a `blocked`
  item. Its partial, unreviewed code from revision 54 is in `HEAD`; build on it and review it.

**Revision 54.** The working tree held the unreviewed partial work of FIF-028, FIF-034 and
FIF-081 alongside FIF-102, entangled across files. At the user's instruction all of it was committed
in one commit, green (590 tests, fmt, clippy). FIF-102 is `done`; the other three stay `todo` and
must still be reviewed against the specification.

**Revision 53.** A status reconciliation. `design/` is **unchanged** since revision 52: `HEAD` is
`d55a1b5`, the commit that recorded DEC-086 and revision 52, and `design/` has no uncommitted
change. `open-questions.md` names the same **18** ids, and the 14 items carrying one are exactly the
14 `blocked`; none is unblocked. The working tree still carries uncommitted, unreviewed work —
FIF-028 (`import/trade_republic/`), FIF-034 (`accounts.rs`, `securities.rs`) and what looks like the
start of FIF-081 (`Derivation::new` taking a `vec1`, emitted `transfer_in` test fixtures citing
their emitter). None of it is touched, and all three stay `todo`.

* No item added, split, retired, completed, blocked or unblocked. 100 items: **37 `done`, 49
  `todo`, 14 `blocked`**, counted by script. (Revision 51's "99 items" predates FIF-102.)
* Coverage re-verified by script against every id appearing in the six requirement documents:
  **351** live ids each on exactly one item's `Requirements:` line, none on an item without being in
  `design/`. DOM-034 and DOM-119 are also *named* on FIF-101 and FIF-102 as corrections, not
  carried. Uncovered: only the retired DOM-009, 014, 015, 021, 041 and 050 to 053.
* **Next to build: FIF-102.** Its one dependency, FIF-012, is `done`, and it carries no
  requirement id, so none is blocked. It is the first ready `todo` in document order; FIF-081,
  named in revision 51, now waits on it. Also ready: FIF-099, FIF-089, FIF-083, FIF-028, FIF-069,
  FIF-034, FIF-041, FIF-042.

**Revision 52.** DEC-086 (user's decision): an emitted `transfer_in` counts as derived by its
`transfer_out`'s batch for DOM-119. New FIF-102 corrects `foreign_citations`; FIF-081 now depends
on it and resumes after it.

**Revision 51.** A status reconciliation. `design/` is **unchanged** since revision 49:
`git log fca2736..HEAD -- design/` is empty, and `open-questions.md` names the same **18** ids.
`HEAD` is `39eb756`. The working tree still carries the uncommitted, unreviewed FIF-028 and FIF-034
work. None of it is touched, and both stay `todo`.

* **Completed: FIF-095**, committed as `39eb756` with its status flip in the same commit.
* No item added, split, retired, re-scoped, blocked or unblocked. 99 items: **37 `done`, 48
  `todo`, 14 `blocked`**, counted by script. The 14 items carrying a blocked id are exactly the 14
  `blocked`.
* Coverage re-verified by script: **348** live ids each on exactly one item, none on an item without
  being in `design/`. Uncovered: only the retired DOM-009, 014, 015, 021, 041 and 050 to 053.
* **Next to build: FIF-081.** Its one dependency, FIF-017, is `done`, and DOM-047 is not blocked.
  It is the first ready `todo` in document order. Also ready: FIF-099, FIF-089, FIF-083, FIF-028,
  FIF-069, FIF-034, FIF-041, FIF-042.

**Revision 50.** A status reconciliation. `design/` is **unchanged** since revision 49:
`git log fca2736..HEAD -- design/` is empty, and `open-questions.md` names the same **18** ids.
`HEAD` is `4f23d8e`. The working tree still carries the uncommitted, unreviewed FIF-028, FIF-034 and
FIF-095 work. None of it is touched, and all three stay `todo`.

* **Completed: FIF-101**, committed as `4f23d8e` with its status flip in the same commit.
* No item added, split, retired, re-scoped, blocked or unblocked. 99 items: **36 `done`, 49
  `todo`, 14 `blocked`**, counted by script. The 14 items carrying a blocked id are exactly the 14
  `blocked`.
* Coverage re-verified by script: **357** distinct ids in `design/`; **348** live ids each on exactly
  one item, none duplicated, none on an item without being in `design/`. The nine uncovered are the
  retired DOM-009, 014, 015, 021, 041 and 050 to 053. Revision 49's "351" was a miscount of the same
  set; no id moved.
* **Next to build: FIF-095.** Its dependencies FIF-009, FIF-010 and FIF-101 are all `done`, and its
  one requirement, ARC-019, is not blocked. It is the first ready `todo` in document order. Also
  ready: FIF-081, FIF-099, FIF-089, FIF-083, FIF-028, FIF-069, FIF-034, FIF-041, FIF-042.

**Revision 49.** `design/` changed: `fca2736` adds DEC-085 and amends DOM-034. No
requirement id added or retired. `open-questions.md` is unchanged and names the same **18** ids.
`HEAD` is `fca2736`. The working tree still carries the uncommitted, unreviewed FIF-028 and FIF-034
work, and now an uncommitted FIF-095 pass as well (`ecb.rs`, `storage/rates.rs`, `tests/rate_cache.rs`). None of it is
touched, and all three stay `todo`.

* **Added: FIF-101**, a date after the newest publication has no rate yet. DEC-085 invalidates part of the `done`
  FIF-009: it substitutes for a date after the newest publication. That is recorded as its own item,
  placed directly after FIF-009. FIF-095 now depends on it.
* No item split, retired, blocked or unblocked. 99 items: **35 `done`, 50 `todo`, 14 `blocked`**.
* Coverage re-verified by script: **351** ids, each exactly once. The nine ids in `design/` on no item
  are the retired DOM-009, 014, 015, 021, 041 and 050 to 053. Nothing else is uncovered.
* **Next to build: FIF-101.** Its one dependency, FIF-009, is `done`, and it carries no blocked id.
  It is the first `todo` in document order. Also ready: FIF-081, FIF-099, FIF-089, FIF-083, FIF-028,
  FIF-069, FIF-034, FIF-041, FIF-042. FIF-095 becomes ready once FIF-101 is `done`.

**Revision 48.** A status reconciliation. `design/` is **unchanged** since revision 47:
`git log -- design/` still ends at `7981565`, and `open-questions.md` names the same **18** ids.
`HEAD` is `4095318`. The working tree still carries the uncommitted, unreviewed FIF-028 and FIF-034
work revision 47 described. None of it is touched, and both items stay `todo`.

* **Completed: FIF-098**, committed as `4095318` with its status flip in the same commit.
* No item added, split, retired, re-scoped, blocked or unblocked. 98 items: **35 `done`, 49
  `todo`, 14 `blocked`**, counted by script. The 14 items carrying a blocked id are exactly the 14
  `blocked`.
* Coverage re-verified by script: **351** ids, each exactly once. Nothing uncovered, nothing deferred.
* **Next to build: FIF-095**, fetching a rate an import does not have. It is the first `todo` in
  document order whose dependencies (FIF-009, FIF-010) are all `done` and whose requirement (ARC-019)
  is not blocked. Also ready: FIF-081, FIF-099, FIF-089, FIF-083, FIF-028, FIF-069, FIF-034,
  FIF-041, FIF-042.

**Revision 47.** Blocked statuses re-derived from the amended `open-questions.md`, which
now names **18** ids across OQ-001, 004, 005, 007, 008, 010, 011, 013 and 014. `HEAD` is `7981565`.
The working tree carries uncommitted, unreviewed work: the pre-DEC-073 FIF-028 tree, and what reads
as a FIF-034 pass (`accounts.rs`, `securities.rs`, storage and `problem.rs` changes). None of it is
touched, and both items stay `todo`.

* **Unblocked:** FIF-028, FIF-069, FIF-073, FIF-081, FIF-083, FIF-084, FIF-085, FIF-087, FIF-089,
  FIF-090, FIF-095. Each is rewritten where the answering decision changed its acceptance.
* **Split:** FIF-063. The decided emission rules (DOM-090, DOM-106, DOM-107, DOM-115) stay and are
  `todo`. The decomposition (DOM-091, still under OQ-001, with DOM-116) moves to the new, `blocked`
  **FIF-100**. FIF-067 now depends on FIF-100.
* **Added:** **FIF-099**, SRV-058: a failed row rejects the whole import. This reverses completed
  FIF-017 behavior (`Import::failures`), so it is a correction item.
* **Moved:** IMP-TR-017 from FIF-028 to FIF-029 (DEC-073 made it depend on classification). DOM-086
  from FIF-097 back to FIF-008.
* **Retired:** FIF-097, see Retired items.
* **Reordered:** FIF-098 moved to directly after FIF-008, so that it is still the next item built
  as revision 46 intended.
* **Stale text for `spec-auditor`:** DOM-086 still says Trade Republic quotes the stored convention.
  It is not a blocker.
* **Next to build: FIF-098.** It has no dependencies and no requirements, and it is the first
  `todo` in document order.


**Revision 46.** `design/` amended by the user and the specification owner's delegate: OQ-002,
OQ-003, OQ-006, OQ-009, OQ-012, OQ-015, OQ-016, OQ-017, OQ-018, OQ-019, OQ-020 and OQ-021 are
answered (DEC-073 to DEC-083) and deleted from `open-questions.md`; SRV-058 is new and needs an item.
DEC-084 removes the unspecified re-rate operation, carried by the new FIF-098. The planner
re-derives blocked statuses and coverage from the amended `open-questions.md`.

**Revision 45.** `design/` amended by the user: DOM-006 now states that `auto_created` is provenance
only; new DOM-126 adds `needs_review`, a separate field cleared only by marking the security
reviewed; new SRV-057 is that action; SRV-014 now sets both flags on import. FIF-034 gains SRV-057
and DOM-126, and stays next to build. DOM-006 stays on FIF-005, whose stored flag already matches
its new wording. The coverage count is for the planner to re-verify.

**Revision 44 (this run).** A status reconciliation. `design/` is **unchanged** since revision 42:
`git log -- design/` still ends at `b9017d8`, and `open-questions.md` names the same 37 ids. `HEAD`
is `0ddf90d`; the working tree still carries the uncommitted, unreviewed FIF-028 implementation
(`import/trade_republic/money.rs` and its tests), untouched. FIF-028 stays `blocked` by OQ-021.

* **Completed: FIF-033**, problem+json errors and the HTTP test harness, committed as `0ddf90d`
  with its status flip in the same commit.
* No item added, split, retired or re-scoped. Nothing blocked or unblocked. 96 items: **34
  `done`, 36 `todo`, 26 `blocked`**, counted by script.
* Coverage re-verified by script: **348** ids on the `Requirements:` lines, each exactly once, equal
  to the live ids in `design/` less the nine retired. Nothing uncovered, nothing deferred. The 26
  items carrying one of the 37 blocked ids are exactly the 26 `blocked`.
* **Next to build: FIF-034**, accounts and securities endpoints. It is the first `todo` in document
  order whose dependencies are all `done` (FIF-033) and none of whose requirements (SRV-007 to
  SRV-011) is blocked. FIF-041 and FIF-042 are the other ready items.

**Revision 43.** A status reconciliation. `design/` is **unchanged** since revision 42:
`git log -- design/` still ends at `b9017d8`, and `open-questions.md` is byte-identical. `HEAD` is
`8964f18`; the working tree still carries the uncommitted, unreviewed FIF-028 implementation
revision 42 described (`import/trade_republic/money.rs`, error variants, fixture tests), untouched.

* **Completed: FIF-032**, server binary, arguments and OpenAPI, committed as `8964f18` with its
  status flip in the same commit.
* No item added, split, retired or re-scoped. Nothing blocked or unblocked. 96 items: **33
  `done`, 37 `todo`, 26 `blocked`**, counted by script.
* Coverage re-verified by script: **348** ids on the `Requirements:` lines, each exactly once, equal
  to the live ids in `design/` less the nine retired. Nothing uncovered, nothing deferred. **37** ids
  on `Blocks:` lines across OQ-001 to OQ-021; the 26 items carrying them are exactly the 26 `blocked`.
* The Decisions required section was stale since revision 42 (it still said 34 ids, 24 items, and
  omitted OQ-021); corrected there.
* **Next to build: FIF-033**, problem+json errors and the HTTP test harness. It is the first `todo`
  in document order whose dependencies are all `done` (FIF-032) and none of whose requirements
  (ARC-020, ARC-021, TST-005) is blocked. FIF-042 is the only other ready item.

**Revision 42.** `design/` **changed**: `b9017d8` adds **OQ-021** to
`open-questions.md`, blocking `IMP-TR-006`, `IMP-TR-016` and `DOM-086`. No requirement was added,
restated or retired. `HEAD` is `b9017d8`; the working tree carries an uncommitted FIF-028
implementation and revision 41's uncommitted plan text, both kept.

* **Blocked: FIF-028**, Trade Republic money mapping, which the working tree had flipped to `done`
  without a commit. It carries IMP-TR-006 and IMP-TR-016. Blocked whole, reasons on the item.
  **IMP-TR-024** removed from its `Requirements:` line: no such id exists in `design/`.
* **Split: DOM-086 out of FIF-008 into the new FIF-097**, `blocked`. FIF-008 stays `done`. FIF-097
  records plainly that OQ-021 puts completed work (`9a1e457`) in question.
* No item completed. 96 items: **32 `done`, 38 `todo`, 26 `blocked`**, counted by script.
* Coverage re-verified by script: **348** ids on the `Requirements:` lines, each exactly once, equal
  to the live ids in `design/` less the nine retired. Nothing uncovered, nothing deferred. **37**
  ids sit on `Blocks:` lines across OQ-001 to OQ-021; the 26 items carrying them are exactly the 26
  `blocked` items.
* **Next to build: FIF-032**, server binary, arguments and OpenAPI. FIF-029 and everything behind it
  in the Trade Republic chain now waits on FIF-028, so the first ready `todo` in document order is
  the first server item. Its one dependency, FIF-011, is `done`; none of its requirements is blocked.
* Reachable set recomputed: of 38 `todo`, **10** are reachable without closing a question: FIF-032,
  FIF-033, FIF-034, FIF-041, FIF-042, FIF-043, FIF-044, FIF-045, FIF-048, FIF-049. FIF-029 dropped
  out with FIF-028.

**Revision 41.** `design/` is **unchanged** since revision 38: `git log -- design/` still
ends at `79b82de`, so no requirement was added, restated or retired, and `open-questions.md` is
byte-identical to revision 32's. `HEAD` is `39d98f4`; the tracked working tree is clean (`docs/` and
`tmp/` are the user's, untracked); `cargo test --workspace` gives **510 passed, 0 failed**, up from
the 474 revision 40 measured.

* **Completed: FIF-027**, Trade Republic CSV reading, identity, ordering and sign convention,
  committed as `39d98f4` **with its status flip in the same commit** — the fourth time in eleven
  revisions that plan and implementation landed together. Nothing needed correcting. Two points its
  completion note records are worth carrying forward rather than rediscovering: `DelimitedReader`
  gained a `read` that answers the header row alongside the data rows, so a header-only file is
  refused instead of read as a successful empty import; and the re-import idempotency clause of this
  item's own acceptance is asserted at the reader, the `import::import` version of it being recorded
  as an obligation on **FIF-029**, which owns the classification an `impl Importer` needs. Both are
  gap-filling within the item's acceptance, not answers to anything on a `Blocks:` line.
* No item added, split, retired or re-scoped. No status changed but FIF-027's. 95 items: **32
  `done`, 39 `todo`, 24 `blocked`**.
* Coverage re-verified by script in both directions: **348** ids on the `Requirements:` lines, each
  exactly once, together precisely the live ids in `design/` less the nine retired ones (DOM-009,
  DOM-014, DOM-015, DOM-021, DOM-041, DOM-050 to DOM-053). Nothing is uncovered and nothing is
  deferred.
* **34** requirements sit on `Blocks:` lines across OQ-001 to OQ-019 (OQ-020 blocks nothing), and
  the 24 items carrying them are exactly the 24 `blocked` items, checked by script. Nothing was
  blocked or unblocked. OQ-014 is still marked *answered* and still on a `Blocks:` line, so
  IMP-SAXO-013 and FIF-023 with it stay blocked; this plan follows the file's own instruction.
* **Next to build: FIF-028**, Trade Republic money mapping. It is the first `todo` in document order
  whose dependencies are all `done` and none of whose requirements is blocked. It continues the
  Trade Republic chain for the reason revision 40 gave: the Saxo importer is finished as far as the
  open questions allow, four of its remaining items waiting on FIF-023 alone.
* The observation revision 40 closed on stands, and the reachable set was computed this revision
  rather than asserted. Of the 39 `todo` items, **12** are reachable without any open question
  being closed — their whole dependency chain is `done` or itself reachable: FIF-028 and FIF-029 on
  the Trade Republic side, then the surfaces that need no engine, FIF-032, FIF-033, FIF-034,
  FIF-041, FIF-042, FIF-043, FIF-044, FIF-045, FIF-048 and FIF-049. The other 27 wait on a
  `blocked` item. So build-out does not stall after FIF-029: it moves from the importers to the
  server and CLI shells. What stays out of reach is everything that exposes the engine — the
  reports, the attribution and transaction endpoints, the TUI completion and attribution flows —
  because the engine itself is stalled on FIF-076 (OQ-001, OQ-002, OQ-007). Closing OQ-014's
  paperwork and OQ-008 would release the Saxo tail; closing OQ-001, OQ-002 and OQ-007 would release
  the engine and the 27 behind it. That is an observation for `spec-auditor` and a person, not a
  decision this plan may take.

**Revision 40.** `design/` is **unchanged** since revision 38: `git log -- design/` still
ends at `79b82de`, so no requirement was added, restated or retired, and `open-questions.md` is
byte-identical to revision 32's. `HEAD` is `4d434ba`; the tracked working tree is clean (`docs/` and
`tmp/` are the user's, untracked); `cargo test --workspace` gives **474 passed, 0 failed**, up from
the 461 revision 39 measured.

* **Completed: FIF-026**, Saxo security mapping and bond quotation, committed as `4d434ba` **with
  its status flip in the same commit**, which is the third time in ten revisions that plan and
  implementation landed together. Nothing needed correcting: the two points its completion note
  records as decided in the module header — a security keeps its first row's name, and one ISIN
  under two instrument types refuses the file — are gap-filling within the item's own acceptance,
  not answers to anything on a `Blocks:` line, so neither becomes a `decision-required` finding.
  The one acceptance clause it could not meet as written (`3000 @ 139.46 = 4183.80`) was met as a
  magnitude assertion for the reason revision 39 had already recorded on the item: amounts are
  perturbed and the fixture states `138.00` [TST-028].
* No item added, split, retired or re-scoped. No status changed but FIF-026's. 95 items: **31
  `done`, 40 `todo`, 24 `blocked`**.
* Coverage re-verified by script in both directions: **348** ids on the `Requirements:` lines, each
  exactly once, together precisely the live ids in `design/` less the nine retired ones (DOM-009,
  DOM-014, DOM-015, DOM-021, DOM-041, DOM-050 to DOM-053). Nothing is uncovered and nothing is
  deferred.
* **34** requirements sit on `Blocks:` lines across OQ-001 to OQ-019 (OQ-020 blocks nothing), and
  the 24 items carrying them are exactly the 24 `blocked` items, checked by script. Nothing was
  blocked or unblocked. OQ-014 is still marked *answered* and still on a `Blocks:` line, so
  IMP-SAXO-013 and FIF-023 with it stay blocked; this plan follows the file's own instruction.
* **Next to build: FIF-027**, Trade Republic CSV reading, identity, ordering and sign convention.
  It is the first `todo` in document order whose dependencies are all `done` and none of whose
  requirements is blocked. Everything ahead of it waits on something blocked: FIF-060 on FIF-058
  (DOM-101), FIF-013 on FIF-076 (DOM-011, DOM-013, DOM-111), FIF-014 / FIF-015 / FIF-016 behind
  FIF-013, FIF-061 and FIF-063, and FIF-024 / FIF-025 / FIF-067 / FIF-094 all behind the blocked
  FIF-023.
* Worth stating plainly, since it is now visible in the shape of the plan rather than in any one
  item: **the Saxo importer is finished as far as the open questions allow**. Of the remaining Saxo
  items, four wait on FIF-023 alone, and FIF-023 waits on three questions of which one (OQ-014) is
  marked answered but not yet written into `domain.md`. Closing OQ-014's paperwork and OQ-008 would
  release more buildable work than anything else in `design/`. That is an observation for
  `spec-auditor` and a person, not a decision this plan may take.

**Revision 39.** `design/` is **unchanged** since revision 38: `git log -- design/` still
ends at `79b82de`, so no requirement was added, restated or retired. Coverage stays at **348** ids,
each on exactly one `Requirements:` line and together precisely the live ids in `design/` less the
nine retired ones (DOM-009, DOM-014, DOM-015, DOM-021, DOM-041, DOM-050 to DOM-053), checked by
script in both directions. `HEAD` is `54b9e08`; the tracked working tree is clean (`docs/` and
`tmp/` are the user's, untracked); `cargo test --workspace` gives **461 passed, 0 failed**, which is
the figure FIF-096's completion note states, so the committed tree is the tree that note describes.

* **Completed: FIF-096**, a corporate action's legs summed per side and cancelled by label,
  committed as `54b9e08` **with its status flip in the same commit** — the second time in nine
  revisions that plan and implementation landed together, and the pattern revisions 33 to 36 kept
  recording stays stopped. Nothing needed correcting here.
* No item added, split, retired or re-scoped. No status changed but FIF-096's. 95 items: **30
  `done`, 41 `todo`, 24 `blocked`**.
* `open-questions.md` is byte-identical to revision 32's: **34** blocked requirements across OQ-001
  to OQ-019 (OQ-020 blocks nothing), carried by exactly the 24 `blocked` items and no others,
  checked by script. No item was blocked or unblocked, and none blocked in an earlier revision has
  been released. OQ-014 is still marked *answered* and still on a `Blocks:` line, so IMP-SAXO-013
  and FIF-023 with it stay blocked; that is the file's own instruction and this plan follows it.
* **Next to build: FIF-026**, Saxo security mapping and bond quotation. It is the first `todo` in
  document order whose dependencies are both `done` (FIF-019 in `a97004e`, FIF-005 in `9846b5a`) and
  none of whose requirements — IMP-SAXO-020, IMP-SAXO-021, IMP-SAXO-022 — appears on a `Blocks:`
  line. Everything ahead of it is `done`, `blocked`, or waits on a `blocked` item: FIF-060 on
  FIF-058 (DOM-101), FIF-013 on FIF-076 (DOM-011, DOM-013, DOM-111), FIF-014 and FIF-015 behind
  FIF-013 and the blocked FIF-061, FIF-016 behind FIF-061 and FIF-063, FIF-024 / FIF-025 / FIF-067 /
  FIF-094 all behind the blocked FIF-023 (IMP-SAXO-013).
* Two caveats now recorded on that item rather than left to be rediscovered: the acceptance's
  `3000 @ 139.46 = 4183.80` is the **real** export's arithmetic and the fixture states `138.00`,
  amounts being perturbed and quantities not [TST-028]; and the item must not settle where the
  quotation factor is applied, that being ARC-008 / DOM-038 in the blocked FIF-075. What is decided
  here is `Type = Bond` defaulting the quotation, which FIF-055 already answers in
  `quotation_for(SecurityType)`.

**Revision 38.** `design/` **changed**: commit `79b82de` (DEC-072) landed after the
working-tree revision 37 below was written, which is why that entry's claim that `design/` is
unchanged and `HEAD` is `0050f03` is wrong — it was true when written and had stopped being true
before it was read. `HEAD` is now `79b82de`. `cargo test --workspace` gives **436 passed, 0
failed**, unchanged, the commit touching `design/decisions.md` and `design/importers.md` only. The
tracked working tree carries this file's revision 37 and 38 edits and nothing else (`docs/` and
`tmp/` are the user's, untracked).

* **Three new identifiers**, all in `importers.md`: IMP-SAXO-046, IMP-SAXO-047, IMP-SAXO-048.
  Coverage is now **348** ids, each on exactly one `Requirements:` line and together precisely the
  live ids in `design/` less the nine retired ones. Nothing is uncovered.
* **FIF-096 rewritten, not annotated.** DEC-072 corrects DEC-071: a reversal is identified by the
  `- Terugboeking` suffix and not by an opposing shape, because the price on a cancelling pair is
  *equal* and a shape-only rule both matches nothing as literally written and, read charitably,
  destroys the sample's one genuine `Omwisseling`. The item had not been started, so no completed
  work is invalidated and no correction item is needed; the three new ids join it because each is a
  clause of the one question it answers — what a group's sides are. Its title changed with it.
* No item added, split, retired or completed. No status changed. 95 items: **29 `done`, 42 `todo`,
  24 `blocked`**.
* `open-questions.md` is byte-identical to revision 32's: **34** blocked requirements across OQ-001
  to OQ-019 (OQ-020 blocks nothing), carried by exactly the 24 `blocked` items and no others,
  checked by script. None of the three new ids appears on a `Blocks:` line.
* **Next to build: FIF-096**, under its new acceptance. It is still the first `todo` in document
  order whose only dependency, FIF-019, is `done`, and everything ahead of it is `done`, `blocked`,
  or waiting on a `blocked` item: FIF-060 on FIF-058 (DOM-101), FIF-013 on FIF-076 (DOM-011,
  DOM-013, DOM-111), FIF-014 and FIF-015 behind FIF-013 and the blocked FIF-061, FIF-016 behind
  FIF-061 and FIF-063.

**Revision 37 (this run).** `design/` is unchanged since revision 32 (`git diff 786cfd2..HEAD --
design/` is empty), so no requirement was added, restated or retired and coverage is untouched at
**345** ids, each named on exactly one `Requirements:` line and together precisely the live ids in
`design/` less the nine retired ones (DOM-009, DOM-014, DOM-015, DOM-021, DOM-041, DOM-050 to
DOM-053). `HEAD` is `0050f03`; the working tree carries no tracked change (only the user's untracked
`docs/` and `tmp/`); `cargo test --workspace` gives **436 passed, 0 failed**, which is the figure
FIF-088's completion note states, so the committed tree is the tree that note describes.

* **Completed:** none this revision. **FIF-088** was completed in `0050f03` **with its status flip
  in the same commit**, which is the first time in eight revisions that the plan and the
  implementation landed together. The pattern revisions 33 to 36 kept recording has stopped; nothing
  needed correcting here.
* No item added, split, retired or re-scoped. No status changed.
* `open-questions.md` is byte-identical to revision 32's: **34** blocked requirements across OQ-001
  to OQ-019 (OQ-020 blocks nothing), **24** `blocked` items, every one carrying a blocked id and no
  other item carrying one, and no item blocked in an earlier revision has been released. 95 items:
  **29 `done`, 42 `todo`, 24 `blocked`**.
* **Next to build: FIF-096**, a corporate action's legs summed per side and cancelled in pairs. It
  is the first `todo` in document order whose only dependency, FIF-019, is `done` (`a97004e`) and
  neither of whose requirements — IMP-SAXO-044, IMP-SAXO-045 — appears on a `Blocks:` line.
  Everything ahead of it in document order is `done`, `blocked`, or waits on a `blocked` item:
  FIF-060 on FIF-058 (DOM-101), FIF-013 on FIF-076 (DOM-011, DOM-013, DOM-111), FIF-014 and FIF-015
  behind FIF-013 and the blocked FIF-061, FIF-016 behind FIF-061 and FIF-063.
* The caveat for that implementer is already in the item and is worth repeating: this rule reads
  `_Transacties` quantities and removes rows, where the just-finished FIF-088 reads `Transacties`
  labels and subtracts money. A reversal appears in both places, so the new module should say in its
  header which half it is. It classifies nothing — IMP-SAXO-013 stays blocked in FIF-023.

**Revision 36 (this run).** `design/` is unchanged since revision 32 (`git diff 786cfd2..HEAD --
design/` is empty), so no requirement was added, restated or retired and coverage is untouched at
**345** ids, each named on exactly one `Requirements:` line. `HEAD` is `1f38bc3`; the working tree
carries no tracked change (only the user's untracked `docs/` and `tmp/`); `cargo test --workspace`
gives **425 passed, 0 failed**, up from revision 35's 410.

* **Completed:** **FIF-022**, Saxo `Acties` label parsing, committed as `1f38bc3`, with the plan in
  that same commit still reading `todo`. Seventh consecutive occurrence of that pattern; the
  observation revision 35 recorded stands unchanged — the status flip needs to be its own commit —
  and it remains a workflow matter, not an item.
* No item added, split, retired or re-scoped. No status changed but FIF-022's.
* **Next to build: FIF-088**, Saxo reversal rows and group summation. It is the first `todo` in
  document order whose dependencies are both `done` (FIF-022, FIF-021) and none of whose
  requirements — IMP-SAXO-033, IMP-SAXO-034, IMP-SAXO-035 — appears on a `Blocks:` line in
  `open-questions.md`. Everything ahead of it is `done`, `blocked`, or waits on a `blocked` item:
  FIF-060 on FIF-058 (DOM-101), FIF-013 on FIF-076 (DOM-011), FIF-014 and FIF-015 behind FIF-013 and
  the blocked FIF-061, FIF-016 behind FIF-061 and FIF-063.
* `open-questions.md` is byte-identical to revision 32's, so no item was blocked or unblocked.

**Revision 35.** `design/` is unchanged since revision 32 (`git diff 786cfd2..HEAD --
design/` is empty), so no requirement was added, restated or retired and coverage is untouched at
**345** ids, each named on exactly one `Requirements:` line and together precisely the live ids in
`design/` less the nine retired ones. `HEAD` is `f34c466`; the working tree carries no tracked
change (only the user's untracked `docs/` and `tmp/`); `cargo test --workspace` gives **410 passed,
0 failed**, up from revision 34's 386.

* **Completed:** **FIF-021**, Saxo money derivation, committed as `f34c466`. Found built, committed
  and green with the plan still reading `todo`, because that commit swept revision 34's plan edits
  in alongside the implementation. Nothing was rebuilt; `money.rs` was read against the six
  requirements and the status corrected. This is the sixth consecutive occurrence of that pattern
  and the plan can only keep recording it: the status flip needs to be its own commit before the
  session can be interrupted, which is a workflow change and not an item.
* No item added, split, retired or re-scoped. No status changed but FIF-021's.
* One obligation was moved rather than dropped, and it was moved by the implementer, not here: the
  *rounding* clause of IMP-SAXO-029 now sits in FIF-023's notes, the derivation keeping full
  precision per FIF-054. IMP-SAXO-029 stays assigned to FIF-021 alone — the id is covered once, and
  what FIF-023 carries is a construction note, not a second claim on the requirement.
* `design/open-questions.md` is unchanged: thirty-four blocked requirements across OQ-001 to OQ-019
  (OQ-020 blocks nothing), twenty-four `blocked` items, every one carrying a blocked id and no other
  item doing so, and no item blocked in an earlier revision has been released. 95 items:
  **27 `done`, 44 `todo`, 24 `blocked`**.
* **Next ready item: FIF-022**, Saxo `Acties` label parsing — its only dependency FIF-019 is `done`
  (`a97004e`) and none of IMP-SAXO-011, IMP-SAXO-012, IMP-SAXO-038 is on a `Blocks:` line. The five
  `todo` items standing earlier in document order — FIF-060, FIF-013, FIF-014, FIF-015, FIF-016 —
  each still wait on a `blocked` dependency (FIF-058, FIF-076, FIF-061, FIF-063), so none is
  selectable. The caveat for the implementer is the DEC-070 re-scope already written into the item:
  the quantity and the direction come from `_Transacties`, the label is a cross-check and a fallback,
  a disagreement refuses the file, and no path may exist by which a label price reaches a money
  field — `Deponering`'s old exception is gone.

**Revision 34 (this run).** `design/` is unchanged since revision 32 (`git diff 786cfd2..HEAD --
design/` is empty), so no requirement was added, restated or retired and coverage is untouched at
**345** ids, each named on exactly one `Requirements:` line and together precisely the live ids in
`design/` less the nine retired ones. `HEAD` is `95fac5e`; the working tree carries no tracked
change; `cargo test --workspace` gives **386 passed, 0 failed**.

* **Completed:** **FIF-020**, Saxo account normalization and row identity, committed as `95fac5e`.
  This revision found it already built, committed and green, with the plan still reading `todo` —
  that commit swept revision 33's plan edits in alongside the implementation, so the status line was
  written before the work it describes. Nothing was rebuilt; the module was read against the five
  requirements and the status corrected.
* No item added, split, retired or re-scoped. No status changed but FIF-020's.
* `design/open-questions.md` is unchanged: thirty-four blocked requirements, twenty-four `blocked`
  items, and no item blocked in an earlier revision has been released.
* **Next ready item: FIF-021**, Saxo money derivation — both dependencies are `done` (FIF-019 in
  `a97004e`, FIF-008 in `9a1e457`) and none of its six identifiers is on a `Blocks:` line. The three
  `todo` items standing earlier in document order — FIF-013, FIF-014, FIF-015, FIF-016 and FIF-060 —
  each still wait on a `blocked` dependency (FIF-076, FIF-061, FIF-058), so none of them is
  selectable. The one caveat the implementer should carry: the factor in the EUR-price step is
  FIF-075 and still blocked, so the bond worked example (3000 @ 139.46) lands with that item and not
  here; the two EUR-quoted worked examples are fully reviewable now.

**Revision 33 (this run).** `design/` is unchanged since revision 32 (`git diff 786cfd2..HEAD --
design/` is empty), so no requirement was added, restated or retired and coverage is untouched at
**345** ids. `HEAD` is `a97004e`; `cargo test --workspace` is green.

* **Completed:** **FIF-019**, Saxo file reading and header normalization. Revision 32 found it built
  and uncommitted and kept it `todo` on the rule that completion is recorded against a commit; it is
  now committed, the leg-count correction that revision demanded was made first, and it is `done`.
* No item added, split, retired or re-scoped. No status changed but FIF-019's.
* `design/open-questions.md` is unchanged: thirty-four blocked requirements, twenty-four `blocked`
  items, and no item blocked in an earlier revision has been released.
* **Next ready item: FIF-020**, Saxo account normalization and row identity — FIF-019 completing is
  exactly what released it, its other dependency FIF-007 having been `done` since revision 21. Its
  four Saxo identifiers are all decided; the undecided part of that area, the corporate-action
  composite identity IMP-SAXO-008, is already split out into the blocked FIF-084, so the item is
  reviewable on its own.

**Revision 32 (this run).** `design/` changed in `786cfd2` (DEC-071), which corrects DEC-070 one
revision after it landed. `HEAD` is `786cfd2`; the working tree carries **FIF-019 built and
uncommitted** plus the user's untracked `docs/` and `tmp/`, and `cargo test --workspace` gives
**371 passed, 0 failed**, up from revision 31's 344.

* **Added:** **FIF-096**, a corporate action's legs summed per side and cancelled in pairs, carrying
  the two identifiers `importers.md` gained — **IMP-SAXO-044** and **IMP-SAXO-045**. They were the
  only uncovered ids; coverage is again complete. Cut as its own item, not folded into FIF-067,
  FIF-094 or FIF-025, because all three read the rule and two of them are gated on the blocked
  FIF-023 while this rule is decided.
* **Restated, none of it invalidating built work:** **FIF-019**'s acceptance — a corporate action's
  legs are its `_Transacties` rows under one id, *however many*, not two told apart by
  `Trade Event Type`, which is the DEC-070 phrasing DEC-071 withdrew; **FIF-067** and **FIF-094**,
  whose acceptance read "the `Gekocht` leg" and, in FIF-094's case, counted three legs as a refusal
  shape — the DeVolksbank tender is three legs and legitimate; **FIF-025**, whose share count is the
  summed side. All three are unstarted. Each gains FIF-096 as a dependency.
* **FIF-093 is `done` and stays `done`.** Its fixture assertion is `rows.len() >= 2` with both event
  types present, which is true of the sample and assumes no leg count, so DEC-071 does not reach it.
  The Philips two-`Gekocht` and DeVolksbank three-leg groups are real rows of the committed
  fixtures, which is how DEC-071 was found at all.
* **Completed:** none. **Newly blocked:** none. **Unblocked:** none. **Uncovered:** none.
* **345** ids on `Requirements:` lines, each exactly once, precisely the live ids in `design/` less
  the nine retired ones; **34** blocked ids across OQ-001 to OQ-019, unchanged — `open-questions.md`
  is untouched by `786cfd2`; **24** items `blocked`, every one carrying a blocked id and no other
  item doing so. 95 items: **24 `done`, 47 `todo`, 24 `blocked`**.
* **Next to build is FIF-019 again**, and the outstanding work is **review and a commit, not a
  rebuild**: the reader, its three sheets, its joins and 27 new tests are in the tree untracked. Two
  things the review owes. The open reading FIF-019 was told to settle — what a Saxo source record is
  when an event spans three sheets — *is* settled there and argued from DOM-007, DOM-120,
  IMP-SAXO-026 and IMP-SAXO-007: one record per `Transacties` row. And the tree still carries
  DEC-070's withdrawn claim about two legs, in `detail_of`'s documentation and in the test
  `a_corporate_action_joins_both_its_legs_on_its_group_id`; the join is right, the claim about leg
  count is not, and it must be restated before it is committed.
* A fourth consecutive revision has now found substantial work built and uncommitted (FIF-059,
  FIF-008 thrice, FIF-093, now FIF-019). The plan cannot fix that, and records it: if a revision
  finds FIF-019 uncommitted again, what needs attention is the hand-off, not the item.

**Revision 31.** `design/` is unchanged since `c04362c`, so nothing completed is
invalidated, no requirement moved and no item was added, split or dropped. `HEAD` is `7a0d64f`, the
tree is clean apart from the user's untracked `docs/` and `tmp/`, and `cargo test --workspace` gives
**344 passed, 0 failed**.

* **Completed:** **FIF-093** (`7a0d64f`), the Saxo fixtures across all three sheets. This is the
  work revision 30 found built and uncommitted and therefore left at `todo`; the commit is what
  this plan counts, so the status flips now. The suite gained 29 tests since revision 29's 315.
* **Added / split / dropped / re-scoped / renumbered:** none. No dependency moved.
* **Newly blocked:** none. **Unblocked:** none. **Uncovered:** none.
* Re-verified mechanically: **343** ids on `Requirements:` lines, each exactly once, precisely the
  live ids in `design/` less the nine retired ones; **34** blocked ids across OQ-001 to OQ-019
  (OQ-020 blocks nothing); **24** items `blocked`, every one of them carrying a blocked id and no
  other item doing so. 94 items: **24 `done`, 46 `todo`, 24 `blocked`**.
* **A correction to revisions 29 and 30**, which both wrote that "every other unblocked `todo` waits
  on a blocked item". That was wrong. Three items are ready this revision — FIF-019, **FIF-027**
  (Trade Republic CSV reading) and **FIF-032** (server binary and OpenAPI) — and FIF-027 and FIF-032
  were equally ready then, their dependencies having been `done` since revision 29. Nothing was
  mis-ordered by it, since FIF-093 genuinely came first, but the claim was false and is recorded
  rather than quietly dropped.
* **Next to build is FIF-019**, Saxo file reading and header normalization: first in plan order of
  the three ready items, and the one that unblocks the largest amount of downstream Saxo work.
  Its unstarted re-scope by DEC-070 stands — three named sheets and their joins, not the first
  sheet — and it now has the fixtures to be tested against. The open reading it must settle and
  state, per its Notes, is what a Saxo source record is when one event spans three rows on three
  sheets; if `importers.md` does not decide it, that is a `spec-auditor` finding, not an
  implementer's choice.
* `docs/` is untracked and is the user's own; no item covers it and none should.

**Revision 30.** `design/` is unchanged since `c04362c`, so nothing completed is
invalidated, no requirement moved and no item changed status. What changed is the working tree:
**FIF-093 is built there and not committed**, and `cargo test --workspace` gives **340 passed,
0 failed**, up from revision 29's 315. The five Saxo fixtures now carry all three sheets, verified
by reading `xl/workbook.xml` out of one of them rather than by trusting the diff.

* **Added / split / dropped / re-scoped / renumbered:** none. No dependency moved. **Uncovered:** none.
* **Completed:** none. `PLAN.md` itself is also uncommitted, so revision 29's entry and this one will
  land together; the status flip for FIF-093 belongs to whichever revision sees its commit.
* **Newly blocked:** none. **Unblocked:** none.
* Re-verified mechanically against the working tree: **343** ids on `Requirements:` lines, each
  exactly once, precisely the live ids in `design/` less the nine retired ones; **34** blocked ids
  across OQ-001 to OQ-019 (OQ-020 blocks nothing); **24** items `blocked`, every one of them
  carrying a blocked id and no other item doing so. 94 items: 23 `done`, 47 `todo`, 24 `blocked`.
* Next to build is **FIF-093** again, and this is not a repeat of the stale-status pattern revisions
  26 to 29 describe: the work exists but has never been reviewed against its acceptance, which is
  the act this plan counts. Everything else that is ready is behind it — FIF-019 depends on it and
  every other unblocked `todo` waits on a blocked item.
* `docs/` is untracked and is the user's own; no item covers it and none should.

**Revision 29 (this run).** The first revision since revision 14 in which `design/` grew, and the
first in which a specification change **invalidates committed work**. `HEAD` is `c04362c`, the tree
is clean and `cargo test --workspace` gives **315 passed, 0 failed**. Two design commits landed:
`409f76f` (DEC-069, the Saxo same-day rule made an actual sort order; the duplicate ARC-025 /
ARC-026 renumbered; OQ-017 to OQ-019 recorded) and `c04362c` (DEC-070, the Saxo export has three
sheets; OQ-020 recorded). Ten identifiers added, one moved, coverage now **343**.

* **Completed:** FIF-065 (`769bb48`), the one-calendar-year import guard, which the commit itself left at `todo` — the fifth consecutive revision to find a built item unmarked. The guard sits in `import`, reads only trade dates, and names every year met.
* **Added:** **FIF-093** (Saxo fixtures across all three sheets, TST-031), **FIF-094** (split and exchange ratios off `_Transacties`, IMP-SAXO-040 and IMP-SAXO-041) and **FIF-095** (fetching a rate an import lacks, ARC-019, blocked).
* **Invalidated, and this is the entry that matters:** the committed Saxo fixtures reproduce one sheet of a three-sheet workbook, so they are not fixtures of the file, and DEC-070 says in as many words that they are rebuilt before any Saxo importer work continues. FIF-003 keeps its record and its acceptance as written; the correction is FIF-093, which is now what to build next and which FIF-019 depends on in place of FIF-003.
* **Reversed rather than edited into silence:** IMP-SAXO-028 in FIF-024. The label price was authoritative for a transferred parcel because "no column carries the figure"; the column exists on a sheet nobody had opened, and the "half-cent tolerance of 0.15" the plan carried since revision 2 was an artifact. The real divergence is up to 1.50 on one parcel and 3.30 across the thirteen, on a cost basis that is subtracted from a future gain. The acceptance is rewritten, not annotated, because the old clause would now produce wrong tax figures.
* **Re-scoped, none of them started, so nothing built is invalidated:** FIF-019 (three sheets and their joins), FIF-022 (quantity and direction from a column, label as fallback, disagreement refuses the file), FIF-025 (the nineteen reinvestment dividends issue nothing; a stock election's share count and taxable value are both in the export), FIF-067 (both legs derived, the pending entry now the fallback), FIF-066 (DEC-069's per-date rule replacing DEC-064's non-transitive pairwise one).
* **Newly blocked:** ARC-019, split out of the `done` FIF-009 into FIF-095 so that a completed item is not turned `blocked` and its record erased. **Unblocked:** none — OQ-014 is marked answered but is still on a `Blocks:` line, so IMP-SAXO-013 and FIF-023 stay blocked.
* **Blocked ids: thirty-four, up from thirty-two. Blocked items: twenty-four, up from twenty-three.** 94 items: 23 `done`, 47 `todo`, 24 `blocked`. **Uncovered:** none.
* Next to build is **FIF-093**. Everything else that is ready is behind it: FIF-019 now depends on it, and every other unblocked `todo` waits on a blocked item.

**Revision 28.** A resumption after the machine was shut down, and the fourth consecutive
revision of that shape, which revision 27 said would make the hand-off the thing to fix. `HEAD` is
`4b6c89d`, which carries revision 27's own plan entry and the whole of FIF-062 in one commit, so the
plan again said `todo` about an item that was already built and reviewed. **FIF-062 is `done`.**
Verified against the code rather than the commit message: `waiting` and `reconnected(batch)` in
`storage/manual_entries.rs`, both over one presence predicate, with `tests/manual_entry_lifecycle.rs`
as the integration cover. `cargo test --workspace` on the clean tree gives **310 passed, 0 failed**,
up from 300. `design/` is unchanged since `d160fee`, so no completed work is invalidated, no
requirement moved and no item changed status but this one.
Re-verified mechanically: **333** live identifiers in `design/`, each named by exactly one item, the
nine retired ones assigned to nothing; **32** blocked ids across OQ-001 to OQ-016; **23** items
`blocked` and no item blocked on anything else; 91 items, now 22 `done`, 46 `todo`.

* **Added / split / dropped / re-scoped / renumbered:** none. No dependency moved. **Uncovered:** none.
* **Completed:** FIF-062. **Newly blocked:** none. **Unblocked:** none.
* Next to build is **FIF-065**, the one-calendar-year import guard. It is the first item to refuse a
  whole file, and it sits inside `import` rather than in either format, for the reason its notes give.
* One honest amendment recorded on FIF-065 rather than by editing its acceptance: the clause "tested
  against the Saxo and Trade Republic fixtures" cannot be met yet, because reading a fixture for its
  trade dates needs a format importer and both are `todo`. The obligation is written onto FIF-065 and
  falls due in FIF-019 and FIF-027; it is deferred, not dropped.
* On the stale-status pattern, now four revisions old: the cause is that the plan entry and the code
  land in one commit, so the run that would flip the status is the run that gets interrupted, and the
  record is never actually lost — `HEAD` always carries the truth. The cheap fix is a hand-off order
  in which the status flip is its own commit before the session can be interrupted; that is a change
  to the workflow, not to any item, so this plan only names it.

**Revision 27.** A resumption after the machine was shut down, and the third consecutive
revision of that shape: the stop came *after* the work, not before it. `HEAD` is now `3e9fe46`,
which carries revision 26's own plan entry and the whole of FIF-064 in one commit, so the plan again
said `todo` about an item that was already built. **FIF-064 is `done`.** Verified against the code
rather than the commit message: `RowClassification` states DOM-043's set in one place with the
amendment rule, there is no catch-all variant and no `Default`, and the new test pins the set with
`strum::EnumCount` against the outcomes an import reaches and asserts every outcome is reached.
`cargo test --workspace` on the clean tree gives **300 passed, 0 failed**, up from 299. `design/` is
unchanged since `d160fee`, so no completed work is invalidated and no requirement moved.
Re-verified mechanically: **333** live identifiers in `design/`, each named by exactly one item, the
nine retired ones assigned to nothing; **32** blocked ids across OQ-001 to OQ-016; **23** items
`blocked` and no item blocked on anything else; 91 items, now 21 `done`, 47 `todo`.

* **Added / split / dropped / re-scoped / renumbered:** none. No dependency moved. **Uncovered:** none.
* **Completed:** FIF-064. **Newly blocked:** none. **Unblocked:** none.
* Next to build is **FIF-062**, the manual-entry lifecycle across undo and re-import. It is the
  first item to exercise an undo, and the first behavioral item outside the importers whose two
  dependencies are both `done`.
* Worth saying plainly, since it is now a pattern rather than an incident: three revisions running
  have found the previous session's work committed and the plan one revision behind, because the
  plan entry and the code land in the same commit and the run that would flip the status is the one
  that gets interrupted. Nothing is lost — the commit is the record — but a reviewer reading
  `PLAN.md` at `HEAD` sees a stale status. If a fourth revision finds the same, the fix is to the
  hand-off order, not to any item.

**Revision 26.** A resumption after the machine was shut down, and the same shape as
revision 25: the stop came *after* the work. `HEAD` is now `bb09bb0`, which carries revision 25's
plan entry and the whole of FIF-017 in one commit, so the plan again said `todo` about an item that
was already built. **FIF-017 is `done`.** Verified against the code rather than the commit message —
the reader abstraction, the canonical rendering and its re-read test, the three classifications, the
five non-position kinds, and `completion` as the only manual-entry constructor on the import path —
and `cargo test --workspace` on the clean tree gives **299 passed, 0 failed**, up from 254. `design/`
is still unchanged since `d160fee`, so no completed work is invalidated and no requirement moved.
Re-verified mechanically: **333** live identifiers in `design/`, each named by exactly one item, the
nine retired ones assigned to nothing; **32** blocked ids across OQ-001 to OQ-016; **23** items
`blocked` and no item blocked on anything else; 91 items, now 20 `done`, 48 `todo`.

* **Added / split / dropped / re-scoped / renumbered:** none. No dependency moved. **Uncovered:** none.
* **Completed:** FIF-017. **Newly blocked:** none. **Unblocked:** none.
* Next to build is **FIF-064**, the import classification taxonomy. Recorded on that item and worth
  repeating here: FIF-017's commit already ships `RowClassification` and the required `classify`
  method, so FIF-064 may turn out to be a review that closes DOM-043 against `bb09bb0` rather than
  new code. That is a legitimate outcome and is why the item is named rather than quietly merged
  into its predecessor.

**Revision 25.** A resumption after the session was stopped to shut the machine down.
The stop came *after* the work, not before it: `HEAD` is now `3136a8a`, which carries revision 24's
own plan entry and the whole of FIF-012 in one commit, so the plan said `todo` about an item that
was already built. **FIF-012 is `done`.** Verified rather than assumed: the seven requirements each
have a distinguishable `StorageError` variant and an integration test against a real temporary
database, `cargo test -p fifolio-core --test invariants` gives 17 passed, and
`cargo test --workspace` on the clean tree gives **254 passed, 0 failed** — up from 237 at revision
24, the 17 new tests being exactly this item's. `design/` is still unchanged since `d160fee`, so no
completed work is invalidated and no requirement moved. Re-verified mechanically: **333** live
identifiers in `design/`, each named by exactly one item, the nine retired ones assigned to nothing;
**32** blocked ids across OQ-001 to OQ-016; **23** items `blocked` and no item blocked on anything
else; 91 items, now 19 `done`, 49 `todo`.

* **Added / split / dropped / re-scoped / renumbered:** none. No dependency moved. **Uncovered:** none.
* **Completed:** FIF-012. **Newly blocked:** none. **Unblocked:** none.
* Next to build is **FIF-017**, the import and derivation framework. FIF-012's own successor
  FIF-060 waits on FIF-058 and the FIFO chain waits on FIF-076, both `blocked`, so the first
  `todo` whose dependencies are all `done` is the importer trait and the reader abstraction.

**Revision 24.** A resumption check after the previous session was stopped mid-run.
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

**As of revision 69**, `open-questions.md` names **seven** ids on `Blocks:` lines; OQ-001, OQ-004,
OQ-007 and OQ-011 are provisionally answered (DEC-090 to DEC-093) and block nothing. Current mapping:

* **OQ-005** — DOM-092, DOM-114 (FIF-079), IMP-SAXO-013 (FIF-023)
* **OQ-008**, **OQ-014** — IMP-SAXO-013 (FIF-023)
* **OQ-010** — ARC-008, DOM-038 (FIF-075), DOM-104 (FIF-077)
* **OQ-013** — IMP-SAXO-026 (FIF-066)

None of the five `blocked` items is in the server scope, and no in-scope item depends on one, so the
server is not held up by an open question. Items built on DEC-090 to DEC-093 (FIF-076, FIF-061,
FIF-071, FIF-086, FIF-082, and outside scope FIF-058, FIF-078, FIF-080, FIF-100) reopen if the user
overrides the provisional answer on ratification.

The revision 67 state follows, kept for audit.

**As of revision 67** (unchanged since 47), `open-questions.md` names **eighteen** distinct ids, and the items carrying
them are `blocked`. OQ-002, 003, 006, 009, 012, 015 to 021 are closed (DEC-073 to DEC-083). Current
mapping:

* **OQ-001** — DOM-011 (FIF-076), DOM-101 (FIF-058), DOM-091 (FIF-100)
* **OQ-004** — DOM-064, DOM-065 (FIF-078), DOM-113 (FIF-061)
* **OQ-005** — DOM-092, DOM-114 (FIF-079), IMP-SAXO-013 (FIF-023)
* **OQ-007** — DOM-111 (FIF-076), SRV-052 (FIF-071), SRV-021 (FIF-086)
* **OQ-008**, **OQ-014** — IMP-SAXO-013 (FIF-023)
* **OQ-010** — ARC-008, DOM-038 (FIF-075), DOM-104 (FIF-077)
* **OQ-011** — DOM-078 (FIF-082), DOM-112 (FIF-080)
* **OQ-013** — IMP-SAXO-026 (FIF-066)

The OQ-014 paragraph below still applies. The rest of this section is the revision 43 state, kept
for audit.

**Thirty-seven** requirements are named on the `Blocks:` lines of `design/open-questions.md`, and the
**twenty-six** items carrying them are `blocked` (as of revision 43; revision 42's OQ-021 added
IMP-TR-006, IMP-TR-016 and DOM-086, blocking FIF-028 and the new FIF-097). Revision 29 raised both by two and one: OQ-017 to
OQ-020 are new in `409f76f` and `c04362c`, of which OQ-018 names DOM-115 (already in the blocked
FIF-063), OQ-019 names ARC-019 (split out of the `done` FIF-009 into the new **FIF-095**) and OQ-020
names nothing yet. This plan does not resolve any of them; closing a question is a change to
`design/`, not to `PLAN.md`. Mapped from the open question to the owning item:

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
* **OQ-017** a transfer's fee share when the basis total is zero — DOM-107 (FIF-063)
* **OQ-018** an emitted quantity sum that must equal a product not representable at the quantity scale — DOM-115 (FIF-063)
* **OQ-019** who fetches a rate an import needs and does not have — ARC-019 (FIF-095)
* **OQ-020** withholding tax is in the export and in no requirement — blocks nothing yet, and so blocks no item; it is a scoping question, and if it is answered by modelling a withheld tax it will add requirements rather than release any
* **OQ-021** Trade Republic changed its `fx_rate` convention in late 2024 — IMP-TR-006, IMP-TR-016 (FIF-028), DOM-086 (FIF-097)

Several questions reach one item from different directions, and every one of them must close before
that item unblocks: FIF-023 by OQ-005, OQ-008 and OQ-014; FIF-080 by OQ-003, OQ-011 and OQ-016;
FIF-063 by OQ-001, OQ-002, OQ-003, OQ-016, OQ-017 and OQ-018; FIF-089 and FIF-090 by OQ-015 alone.

**OQ-014 is marked answered and still blocks**, by its own words: the entry is kept "until the rule
is written into `domain.md` and the entry deleted". Read literally, as this plan is required to read
it, IMP-SAXO-013 is still undecided and FIF-023 is still blocked. Nothing is lost by that — OQ-005
and OQ-008 block the same requirement independently — but an entry that says it is answered while
sitting on a `Blocks:` line is exactly the ambiguity the list exists to remove, and deleting it is a
`spec-auditor` act.

OQ-014 and OQ-015 are worth singling out because neither can be answered by reading the
specification harder. OQ-014 needs a figure that is in no column of the export and in no Saxo
report; OQ-015 needs a rule for a format that carries no account identifier at all. Both are
questions for a person, and OQ-014 governs the largest block of manual work in the sample.

D1 and D2, raised in revision 1, are closed; see the revision history. The twenty-seven ids blocked
in revision 2 and decided in revision 3 are listed there too.

# Requirement coverage

Revision 83: unchanged. **352** live ids, each exactly once, re-verified by script against `design/`
at `17b3bb5`. One hundred and one items: 63 `done`, 33 `todo`, 5 `blocked`.

Revision 82: unchanged. **352** live ids, each exactly once, re-verified by script against `design/`
at `780bdba`. One hundred and one items: 62 `done`, 34 `todo`, 5 `blocked`.

Revision 81: unchanged. **352** live ids, each exactly once, re-verified by script against `design/`
at `2a60a9d`. One hundred and one items: 61 `done`, 35 `todo`, 5 `blocked`.

Revision 80: unchanged. **352** live ids, each exactly once, re-verified by script against `design/`
at `c776fa4`. One hundred and one items: 60 `done`, 36 `todo`, 5 `blocked`.

Revision 78: unchanged. **352** live ids, each exactly once, re-verified by script against `design/`
at `8f89f39`. One hundred and one items: 58 `done`, 38 `todo`, 5 `blocked`.

Revision 69: unchanged. **352** live ids, each exactly once, re-verified by script against `design/`
at `6921150`. One hundred and one items: 48 `done`, 48 `todo`, 5 `blocked`.

Revision 67: unchanged. **352** live ids, each exactly once, re-verified by script against `design/`
at `e98430a`. One hundred and one items: 47 `done`, 40 `todo`, 14 `blocked`.

Revision 66: unchanged. **352** live ids, each exactly once, re-verified by script against `design/`
at `ce6e6d4`. One hundred and one items: 47 `done`, 40 `todo`, 14 `blocked`.

Revision 65: unchanged. **352** live ids, each exactly once, re-verified by script against `design/`
at `365fefd`. One hundred and one items: 46 `done`, 41 `todo`, 14 `blocked`.

Revision 64: unchanged. **352** live ids, each exactly once, re-verified by script against `design/`
at `e395518`. One hundred and one items: 45 `done`, 42 `todo`, 14 `blocked`.

Revision 63: unchanged. **352** live ids, each exactly once, re-verified by script against `design/`
at `18cf73a`. One hundred and one items: 44 `done`, 43 `todo`, 14 `blocked`.

Revision 62: unchanged. **352** live ids, each exactly once, re-verified by script against `design/`
at `f9a718c`. One hundred and one items: 43 `done`, 44 `todo`, 14 `blocked`.

Revision 61: unchanged. **352** live ids, each exactly once, re-verified by script against `design/`
at `5e225eb`. One hundred and one items: 42 `done`, 45 `todo`, 14 `blocked`.

Revision 60: unchanged. **352** live ids, each exactly once, re-verified by script against `design/`
at `d086bf8`. One hundred and one items: 41 `done`, 46 `todo`, 14 `blocked`.

Revision 59: unchanged. **352** live ids, each exactly once, re-verified by script against `design/`
at `116be0c`. One hundred and one items: 40 `done`, 47 `todo`, 14 `blocked`.

Revision 58: **352** live ids, each exactly once, re-verified by script against `design/` at
`951cb67`; SRV-059 is new and on FIF-103. One hundred and one items: 39 `done`, 48 `todo`, 14
`blocked`.

Revision 57: unchanged. **351** live ids, each exactly once, re-verified by script against `design/`
at `242ab6a`; DEC-087 reworded SRV-058 without adding an id. One hundred items: 39 `done`, 47
`todo`, 14 `blocked`.

Revision 56: unchanged. **351** live ids, each exactly once, re-verified by script against `design/`
at `1ce1a9f`. One hundred items: 39 `done`, 47 `todo`, 14 `blocked`.

Revision 51: unchanged. **348** live ids, each exactly once, re-verified by script against `design/`
at `39eb756`. Ninety-nine items: 37 `done`, 48 `todo`, 14 `blocked`.

Revision 49: unchanged in count. **351** ids, each exactly once, re-verified by script against `design/` at
`fca2736`. FIF-101 carries no id of its own. Ninety-nine items: 35 `done`, 50 `todo`, 14 `blocked`.

Unchanged in revision 48, re-verified by script against `design/` at `7981565`: **351** ids, each
exactly once, nothing uncovered, nothing deferred. Ninety-eight items: 35 `done`, 49 `todo`, 14
`blocked`.

Revision 47, checked by script against `design/` at `7981565`: **351** ids on the `Requirements:`
lines, each exactly once. They are exactly the live ids in `design/` less the nine retired ones.
The additions since revision 44 are DOM-126 and SRV-057 (FIF-034, revision 45) and SRV-058 (the new
FIF-099). Nothing is uncovered and nothing is deferred. The 14 items carrying one of the 18 blocked
ids are exactly the 14 `blocked` items. There are ninety-eight items: 34 `done`, 50 `todo`, 14
`blocked`.

Unchanged in revision 44, re-verified by script against `design/` at `b9017d8`: **348** ids, each
exactly once, nothing uncovered, nothing deferred; the 26 items carrying one of the 37 blocked ids
are exactly the 26 `blocked`, of ninety-six items, thirty-four now `done`.

Re-verified in revision 43 by script against `design/` at `b9017d8`: **348** ids on the
`Requirements:` lines, each exactly once, and together precisely the live ids in `design/` less the
nine retired ones (DOM-009, DOM-014, DOM-015, DOM-021, DOM-041, DOM-050 to DOM-053), which are
assigned to nothing. **Nothing is uncovered and nothing is deferred.** Every item carrying one of
the thirty-seven ids on a `Blocks:` line is `blocked`, and no other item is — twenty-six items of
ninety-six, of which thirty-three are now `done`.

Unchanged in revision 39, re-verified by script against `design/` at `79b82de`: **348** ids on the
`Requirements:` lines, each exactly once, and together precisely the live ids in `design/` less the
nine retired ones (DOM-009, DOM-014, DOM-015, DOM-021, DOM-041, DOM-050 to DOM-053), which are
assigned to nothing. **Nothing is uncovered and nothing is deferred.** Every item carrying one of
the thirty-four ids on a `Blocks:` line is `blocked`, and no other item is — twenty-four items of
ninety-five.

Re-verified in revision 38 against `design/` at `79b82de`, which added three identifiers:
**348** ids on the `Requirements:` lines, each exactly once, and together precisely the live ids in
`design/` less the nine retired ones (DOM-009, DOM-014, DOM-015, DOM-021, DOM-041, DOM-050 to
DOM-053), which are assigned to nothing. The three additions — IMP-SAXO-046, IMP-SAXO-047,
IMP-SAXO-048 — are all on **FIF-096**, whose acceptance DEC-072 rewrote; none of them is on a
`Blocks:` line. **Nothing is uncovered and nothing is deferred.** Every item carrying one of the
thirty-four blocked ids is `blocked`, and no other item is — twenty-four items of ninety-five.

Unchanged in revision 37, verified by script against `design/` at `0050f03`: **345** ids on the
`Requirements:` lines, each exactly once, and together precisely the live ids in `design/` less the
nine retired ones (DOM-009, DOM-014, DOM-015, DOM-021, DOM-041, DOM-050 to DOM-053), which are
assigned to nothing. **Nothing is uncovered and nothing is deferred.** Every item carrying one of the
thirty-four ids on a `Blocks:` line of `open-questions.md` is `blocked`, and no other item is —
twenty-four items of ninety-five.

Unchanged in revision 33: `design/` did not change, so the figures below stand as verified.

Re-verified in revision 32 against `design/` at `786cfd2`: **345** ids on the `Requirements:` lines,
each exactly once, precisely the live ids in `design/` less the nine retired ones. The two ids added
by DEC-071 — **IMP-SAXO-044** (a leg count is never assumed; sides are summed) and **IMP-SAXO-045**
(an exactly opposing pair cancels first) — were uncovered when this revision began and are now
carried by the new **FIF-096**. Nothing is uncovered and nothing is deferred. `open-questions.md` is
unchanged, so the thirty-four blocked ids and the twenty-four `blocked` items carrying them stand as
revision 31 recorded them, of ninety-five items.

Re-verified in revision 31 by the same script, against a `design/` unchanged since `c04362c`: **343**
ids on the `Requirements:` lines, each exactly once, precisely the live ids in `design/` less the
nine retired ones, which are assigned to nothing. Every item carrying one of the thirty-four ids on
a `Blocks:` line of `open-questions.md` is `blocked` and no other item is — twenty-four items of
ninety-four. No placement changed. Nothing is uncovered and nothing is deferred.

Re-verified in revision 30 by the same script, against a `design/` unchanged since `c04362c`: **343**
ids, each named by exactly one item, none uncovered, none deferred; the twenty-four items carrying
one of the thirty-four blocked ids are `blocked` and no others are. No item changed status, so no
assignment moved.

Re-verified in revision 29 against a `design/` that grew for the first time since `d160fee`, in
`409f76f` and `c04362c`: the `Requirements:` lines name **343** distinct ids, each exactly once, and
they are precisely the live ids in `design/` less the nine retired ones, which are assigned to
nothing. Nothing is uncovered and nothing is deferred. The ten additions and the one move:

* **ARC-028** → FIF-010 and **ARC-029** → FIF-011, both `done`. These are not new rules: they are the renumbering of the duplicate `ARC-025` / `ARC-026` the plan flagged in revisions 19 and 21, and both rules were already implemented as acceptance clauses. The collision is now fixed in `design/` and the plan's two flags are retired with it.
* **TST-031** → the new **FIF-093**, the Saxo fixtures rebuilt across all three sheets. Carried away from the `done` FIF-003, which shipped the one-sheet fixtures DEC-070 says are not fixtures of the file.
* **IMP-SAXO-037** → FIF-019, the three sheets and their join keys.
* **IMP-SAXO-038** → FIF-022, the quantity and direction coming from a column with the label as fallback.
* **IMP-SAXO-039** → FIF-024, `Verhandelde waarde` as the transferred parcel's basis.
* **IMP-SAXO-040** and **IMP-SAXO-041** → the new **FIF-094**, split and exchange ratios off the position sheet.
* **IMP-SAXO-042** and **IMP-SAXO-043** → FIF-025, the reinvestment dividends that issue nothing and the stock election that does.
* **ARC-019 moved** from the `done` FIF-009 to the new, blocked **FIF-095**, OQ-019 having made it undecided. The error FIF-009 shipped is unaffected; what moved is the fetch path nothing builds.

Every item carrying one of the **thirty-four** ids on a `Blocks:` line is `blocked` and no other item
is — **twenty-four** items of ninety-four, checked by script.

Re-verified again in revision 28 by the same script, against a `design/` unchanged since `d160fee`:
**333** ids, each named by exactly one item, none uncovered, none deferred; the twenty-three items
carrying one of the thirty-two blocked ids are `blocked` and no others are. FIF-062 moving to `done`
changed no assignment.

Re-verified again in revision 27 by the same script, against a `design/` unchanged since `d160fee`:
**333** ids, each named by exactly one item, none uncovered, none deferred; the twenty-three items
carrying one of the thirty-two blocked ids are `blocked` and no others are. FIF-064 moving to `done`
changed no assignment.

Re-verified again in revision 25 by the same script, against a `design/` unchanged since `d160fee`:
**333** ids, each named by exactly one item, none uncovered, none deferred; the twenty-three items
carrying one of the thirty-two blocked ids are `blocked` and no others are. FIF-012 moving to `done`
changed no assignment.

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
