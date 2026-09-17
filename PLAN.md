# PLAN

Ordered work items derived from `design/`. One item is one coherent, testable increment.
Item ids (`FIF-nnn`) are stable and never reused. Ordering is by dependency first, then by risk:
domain types, arithmetic, FIFO and storage come before anything that merely exposes them.

Statuses: `todo` | `in-progress` | `done` | `blocked`.

Requirement coverage report is at the bottom.

---

## FIF-001 Workspace and CI baseline
Status: done
Requirements: ARC-001, ARC-002, TST-008, TST-024
Depends on: none
Acceptance: Cargo workspace (edition 2024, resolver 3) with `fifolio-core`, `fifolio-server`, `fifolio-cli`; core carries no HTTP or terminal dependency; CI runs `cargo test`, `cargo fmt --check`, `cargo clippy -- -D warnings` and `cargo llvm-cov` with per-crate thresholds 95 / 90 / 75.
Notes: Already present and green before this plan was written. Listed so the requirements it satisfies are not orphaned. The stub binaries printing "not implemented yet" are *not* part of this item; replacing them is FIF-032 and FIF-042.

## FIF-002 Test layer conventions and harness
Status: todo
Requirements: TST-001, TST-002, TST-003, TST-006, ARC-024
Depends on: FIF-001
Acceptance: the repository documents and demonstrates the three layers — unit (one module, no I/O), integration (crate against real deps in process, temp SQLite / real HTTP surface), end-to-end (real binaries, real db, free port); a shared test-support module provides a temporary-database helper and a free-port helper; every test names the requirement ids it covers, in the convention `.claude/agents/README.md` describes.
Notes: TST-002 enumerates what must end up unit tested in core; the individual items below each carry their own share. This item owns the convention and the harness, not the coverage of every rule.

## FIF-003 Anonymized broker fixtures
Status: todo
Requirements: TST-011, TST-012, TST-013, TST-014
Depends on: FIF-001
Acceptance: committed fixtures under a fixtures directory, derived from `design/example_exports/` (gitignored, present locally):
* Saxo: a real XLSX container, one sheet, the 31 Dutch headers byte-for-byte including `Bk\xa0Record\xa0Id`, `Booking\xa0Id` and the leading space in ` Positie-ID`; Excel serial dates; `Rekening-ID` values with `EUR`/`USD`/`CAD` suffixes on one base account; free-text `Acties` strings with their 2-decimal prices; at least one row of every observed `Acties` value (`Koop`, `Verkoop`, `Deponering`, `Expiratie`, `Fusie`, `Terugkoopaanbod`, `Terugboeking`, `Stock split`, `Omwisseling`, `Dividend`, `Keuzedividend`, `Herbeleggingsdividend`, `Rente`, `Service fee`, `ADR-kosten`, `Storting`, `Opname`); at least one multi-row corporate action sharing a `Corporate action-Id` where only one row carries a `Positie-ID` and another carries the money (the Philips shape); at least one reversal row; at least one `Bond` instrument.
* Trade Republic: quoted CSV, all 23 columns, ISO-8601 UTC timestamps with sub-second precision, UUID `transaction_id`, negative cash-flow amounts, populated `original_amount` / `original_currency` / `fx_rate` on at least one row, and one `TAX_EXCHANGE` pair.
* Account ids, client ids, personal names, IBANs and instrument-level identifying detail replaced; amounts perturbed.
* A committed, runnable anonymization script plus a README stating that fixtures test parsing, classification and idempotency only, never arithmetic.
Notes: This blocks every importer test (FIF-019 .. FIF-029), which is why it is third. The generator must be re-runnable against the real exports so a future export shape can be folded in; it must not embed the real values.

## FIF-004 Money primitives: scales, rounding, quotation factor
Status: todo
Requirements: ARC-006, ARC-007, ARC-008, ARC-009, ARC-010, DOM-038, TST-015, TST-016
Depends on: FIF-001
Acceptance: newtypes or wrappers over `rust_decimal` for quantity (8), unit price (6), monetary amount (2) and FX rate (6); no `f32`/`f64` anywhere in core; a single rounding function, half-up, applied only at storage and presentation boundaries; `trade_value(quantity, unit_price, factor)` with factor 1 or 0.01; unit tests with hand-derivable synthetic inputs covering per-unit vs percent-of-par (including that confusing them is a factor of 100) and fractional quantities at full scale.
Notes: TST-016's other boundary cases (EUR derivation, fee distribution remainders) are asserted in FIF-021 and FIF-014 respectively; this item owns the convention and the two cases that are purely about scale and factor.

## FIF-005 Core domain types
Status: todo
Requirements: DOM-004, DOM-005, DOM-006, DOM-007, DOM-008, DOM-009, DOM-010, DOM-011, DOM-012, DOM-013, DOM-014, DOM-017, DOM-018, DOM-036, DOM-037, DOM-039
Depends on: FIF-004
Acceptance: `Account`, `Security`, `SourceRecord`, `Transaction`, `CorporateAction`, `ImportBatch`, `SaleAttribution` and `Allocation` types with the fixed enums (`SecurityType`, `Quotation` defaulting to `per_unit`, `SourceRecordKind`, `TransactionType` = buy|sell only, `CorporateActionKind` = quantity adjustment | lot transfer); source records hold raw content plus parsed fields and are constructible but not mutable; securities carry an auto-created flag; quotation is defaulted at auto-creation and settable afterwards, independently of type; transactions carry a per-account monotonic import sequence and a single `fees` field summing all incidental costs; an allocation holds only buy, sell and quantity; prices are stored exactly as read.
Notes: Types only, no persistence and no engine. Reviewable against the Entities section of `domain.md` on its own.

## FIF-006 Canonical ordering
Status: todo
Requirements: DOM-040, DOM-041
Depends on: FIF-005
Acceptance: a total order over a security's transactions within an account: trade date, then execution time with time-less transactions sorting before timed ones on the same date, then import sequence; unit tested including the tie cases.

## FIF-007 Source record identity and idempotency
Status: todo
Requirements: DOM-022, DOM-023, DOM-024
Depends on: FIF-005
Acceptance: an identity abstraction that takes either a broker reference or a hash of the parsed business fields, always scoped to the account; identical rows in two accounts produce distinct identities; re-importing the same rows into the same account produces no new source records. Format-specific identity rules live in FIF-020 and FIF-027.

## FIF-008 EUR valuation rules
Status: todo
Requirements: DOM-025, DOM-026, DOM-027, DOM-028, DOM-029
Depends on: FIF-004, FIF-005
Acceptance: every transaction stores native unit price and fees plus EUR unit price and EUR fees at the same scales, together with rate, rate source and rate date; the valuation date is the trade date, never settlement; no separate currency-gain figure exists anywhere in the model; all gross and allocation figures derive from the stored pair.

## FIF-009 FX rate resolution
Status: todo
Requirements: DOM-030, DOM-031, DOM-032, DOM-033, DOM-034, DOM-035, ARC-019, TST-019, TST-020, TST-021
Depends on: FIF-008
Acceptance: rate source precedence `broker` > `ecb` > `native`; broker-stated EUR figures used verbatim with the implied quotient stored as the rate; EUR-denominated transactions get rate 1 and source `native`; ECB lookup falls back to the most recent published rate before the trade date and stores that rate's own date; fees convert at the leg's rate; a missing, unfetchable rate fails with an error naming currency and date. The rate source is an injected trait; unit tests use a fake table with weekend and holiday gaps. No test opens a socket.

## FIF-010 ECB rate cache and seeding
Status: todo
Requirements: ARC-015, ARC-016, ARC-017, ARC-018
Depends on: FIF-009, FIF-011
Acceptance: a rate table keyed by currency and date; a seeding path that ingests the ECB full historical series (1999 onward) and a top-up path for the rolling 90-day feed; once seeded, imports resolve rates with no outbound call. Seeding is tested against a committed recorded fragment of the series, never a live fetch.

## FIF-011 SQLite storage and migrations
Status: todo
Requirements: ARC-011, ARC-012, ARC-013, ARC-014, DOM-071, TST-004
Depends on: FIF-005
Acceptance: one SQLite file, default `./fifolio.db`, created on first run and overridable; versioned `sqlx` migrations applied on startup; repositories for every entity in FIF-005; a unique constraint on security ISIN; integration tests against a real temporary database covering migration from empty and the ISIN uniqueness failure.

## FIF-012 Storage-enforced invariants
Status: todo
Requirements: DOM-064, DOM-065, DOM-066, DOM-067, DOM-068, DOM-069, DOM-070, DOM-072
Depends on: FIF-011
Acceptance: each invariant is refused at the persistence/service boundary with a distinguishable error: allocations against a buy never exceed its quantity; a sale's allocations sum exactly to its quantity; a sell may only be attributed if every earlier sell of the same account and security is attributed; a security with any pending source record may not be attributed; an attribution may only be deleted if no later attribution exists for that account and security; a transaction in an attribution cannot be edited, re-rated or deleted; a source record is consumed by at most one transaction or corporate action; a batch may only be deleted if no transaction derived from it participates in an attribution. Integration tested against a real temporary database, one test per invariant.

## FIF-013 FIFO proposal engine
Status: todo
Requirements: DOM-056, DOM-057
Depends on: FIF-006, FIF-005
Acceptance: given an account, a security and a sell, the engine consumes the oldest buys with unattributed quantity remaining, in canonical order, until the sold quantity is covered, splitting the final buy; if the available unattributed quantity is short, it returns a shortfall naming the missing quantity instead of a proposal. Pure function over transactions and existing allocations, unit tested with no database.

## FIF-014 Allocation figure derivation and the drift rule
Status: todo
Requirements: DOM-058, DOM-059, DOM-060, DOM-061, DOM-062, DOM-063
Depends on: FIF-013, FIF-004
Acceptance: allocated cost, buy fee, proceeds, sell fee and gain computed on demand from the parent transactions and the quotation factor, exactly as the formulas in `domain.md` state; each share rounded to 2 decimals independently with drift absorbed by the last share; buy-side last share is the allocation that exhausts the lot, sell-side last share is the last allocation of that sale in canonical order; sell fees never spread beyond their own sale. Unit tests include a division that is exact, one leaving one cent, and one leaving many.

## FIF-015 Attribution service
Status: todo
Requirements: DOM-019, DOM-020, DOM-021, DOM-049, DOM-054, DOM-055
Depends on: FIF-014, FIF-012
Acceptance: approve-or-decline only, no partial edit; creating an attribution validates same account and same security, every buy dated on or before the sell in canonical order, and quantities summing exactly to the sell quantity; a security with any pending source record is refused; declining writes nothing and leaves the sell blocking later sells by construction.

## FIF-016 Property tests for the engine
Status: todo
Requirements: TST-010
Depends on: FIF-014, FIF-018
Acceptance: `proptest` suites asserting all seven properties in `testing.md`: no buy over-consumed; sale allocations sum exactly; shares sum exactly to the parent figure for every split and remainder; a buy's fees fully distributed at its last unit sold and not before; a quantity adjustment leaves total cost per lot unchanged; a lot transfer preserves total cost basis and lot count; attributing a sequence of sells in canonical order never over-consumes a buy.

## FIF-017 Import and derivation framework
Status: todo
Requirements: DOM-002, DOM-042, DOM-043, DOM-044, DOM-045, DOM-046, DOM-047, DOM-048, ARC-023
Depends on: FIF-007, FIF-011
Acceptance: an importer trait over a source file that yields source records, each classified as derived automatically, pending, or recognized as non-position; plain trades with unambiguous quantity, price and fees become buys and sells; non-position rows (dividends, interest, deposits, withdrawals, account fees) are counted and not stored; every import creates a batch with its counts; nothing is created without a source record; user-supplied information always becomes a `manual` source record cited alongside the imported ones. XLSX reading via `calamine` and CSV via `csv` sit behind the same reader abstraction.
Notes: This is the seam both importers plug into; cut so the Saxo and TR items below can each be reviewed alone.

## FIF-018 Corporate action engine
Status: todo
Requirements: DOM-015, DOM-016, DOM-050, DOM-051, DOM-052, DOM-053
Depends on: FIF-013, FIF-017
Acceptance: a quantity adjustment rescales every open lot by a ratio, leaving each lot's total cost unchanged and its acquisition date untouched, so cost per unit moves inversely; a lot transfer closes lots in one security and opens one new lot per old lot in another, carrying acquisition dates and costs; cash-for-units events are a sell, stock dividends are a buy, and both cite the corporate action rows they came from; a mixed cash-and-exchange event decomposes into a sell plus a lot transfer citing the same source records.

## FIF-019 Saxo: file reading and header normalization
Status: todo
Requirements: IMP-SAXO-001, IMP-SAXO-002, IMP-SAXO-003, IMP-SAXO-004
Depends on: FIF-017, FIF-003
Acceptance: reads the single sheet with its one header row and 31 columns; matches headers after whitespace normalization, so `Bk\xa0Record\xa0Id`, `Booking\xa0Id` and ` Positie-ID` resolve; converts Excel serial numbers to dates; rejects a non-Dutch header set with a clear error rather than mis-mapping. Tested against the Saxo fixture.

## FIF-020 Saxo: account normalization and row identity
Status: todo
Requirements: DOM-003, IMP-SAXO-005, IMP-SAXO-006, IMP-SAXO-007, IMP-SAXO-008
Depends on: FIF-019, FIF-007
Acceptance: `Rekening-ID` currency suffix stripped, so `.../1000000EUR|USD|CAD` collapse onto one account; `Klant-id` never used as the account; identity is the first populated of `Transactie-ID`, `Bk Record Id`, `Booking Id`, `Corporate action-Id`; when it falls through to `Corporate action-Id`, `Acties` and the amount are folded in so sibling rows of one event stay distinct. Re-importing the fixture twice yields no new records.

## FIF-021 Saxo: money derivation
Status: todo
Requirements: IMP-SAXO-009, IMP-SAXO-010
Depends on: FIF-019, FIF-008
Acceptance: `Boekingsbedrag` read as native cash movement including costs, `Aantal` as the same amount in EUR and never as a quantity, `Totale kosten` as EUR costs (always negative), `Omrekeningskoers` as the native-to-EUR multiplier; EUR gross = |Aantal| − |Totale kosten|, EUR fees = |Totale kosten|, EUR unit price = EUR gross / quantity; rate source `broker`. Unit test reproduces the worked example in `importers.md` (40 @ 5.75 USD → EUR gross 209.63, EUR price 5.240750).

## FIF-022 Saxo: `Acties` label parsing
Status: todo
Requirements: IMP-SAXO-011, IMP-SAXO-012
Depends on: FIF-019
Acceptance: quantity and direction parsed out of the free-text label (`Koop 40 @ 5.75 USD`, `Verkoop -60 @ 30.65 EUR`, `Deponering 300 @ 51.40 EUR`); the label price is exposed only as a 2-decimal display value and is structurally unable to reach any money field — a test asserts the sell case where 30.65 in the label differs from 30.654 derived from the columns; an unparsable label is a parse failure, not a guess.
Notes: Deliberately separate from FIF-021 so "labels give quantity, columns give money" is reviewable as one rule.

## FIF-023 Saxo: row classification
Status: todo
Requirements: IMP-SAXO-013
Depends on: FIF-022, FIF-017, FIF-018
Acceptance: `Transactietype` plus `Acties` map to exactly the table in `importers.md`: `Koop`/`Verkoop` → automatic buy/sell; `Deponering` → automatic buy (FIF-024); `Expiratie` → sell closing the whole remaining position with proceeds from the row; `Fusie`, `Terugkoopaanbod` (with `Terugboeking`) → pending sell, plus a lot transfer where shares are received; `Stock split`, `Omwisseling` → pending quantity adjustment or lot transfer; dividend labels → FIF-025; `Rente`, `Service fee`, `ADR-kosten`, `Storting`, `Opname` → recognized as non-position, not stored. Table-driven test over the fixture, one case per label; an unknown label fails to parse rather than being silently dropped.

## FIF-024 Saxo: `Deponering` transfers in
Status: todo
Requirements: IMP-SAXO-014, IMP-SAXO-015, IMP-SAXO-016, IMP-SAXO-017, DOM-080
Depends on: FIF-023, FIF-009
Acceptance: a `Deponering` row (zero `Boekingsbedrag` and `Aantal`) produces a complete buy with quantity and price from the label, taken at face value as a split-restated acquisition cost; acquisition date defaults to the transfer date and remains editable afterwards (the Altbestand case); because `Omrekeningskoers` is 1 even for foreign-currency instruments, a non-EUR `Deponering` resolves its EUR cost basis by ECB lookup on the transfer date with source `ecb`; a later split row applies on top with no double counting.

## FIF-025 Saxo: dividend grouping by `Positie-ID`
Status: todo
Requirements: IMP-SAXO-018, IMP-SAXO-019
Depends on: FIF-023
Acceptance: dividend-type rows (`Dividend`, `Keuzedividend`, `Herbeleggingsdividend`) are grouped by `Corporate action-Id` *before* evaluation; if any row in a group carries a `Positie-ID`, the whole group becomes one pending entry requiring an explicit stock-or-cash decision with stock pre-selected; otherwise the group is recognized as non-position and not stored. A regression test reproduces the two-row Philips shape — position-marked row carrying zero, cash row carrying the money with no marker — and asserts that row-by-row evaluation would drop the money while grouping does not.
Notes: Calibrated heuristic; its failure mode is a blocked attribution with a named shortfall, not a wrong figure. Keep that reasoning in the code comment.

## FIF-026 Saxo: security mapping and bond quotation
Status: todo
Requirements: IMP-SAXO-020, IMP-SAXO-021, IMP-SAXO-022
Depends on: FIF-019, FIF-005
Acceptance: `Instrument ISIN`, `Instrument` and `Type` (`Stock`, `Bond`, `Etf`, `MutualFund`, `Cash`) map onto the security; `Bond` defaults quotation to `percent_of_par`, everything else to `per_unit`; ISIN is the key, so a changed or delisting-annotated name (`*Delisted 20231011 (...)`) resolves to the same security and does not create a second one. Test asserts a 3000-nominal bond at 139.46 costs 4183.80.

## FIF-027 Trade Republic: CSV reading, identity and sign convention
Status: todo
Requirements: IMP-TR-001, IMP-TR-002, IMP-TR-003, IMP-TR-004
Depends on: FIF-017, FIF-003, FIF-007
Acceptance: quoted comma-delimited CSV with dot decimals and one header row of 23 columns; `datetime` parsed as ISO-8601 UTC with sub-second precision and kept distinct from `date`, which is the effective date; `transaction_id` UUID used directly as the identity; cash-flow signs interpreted so buys and fees are negative. Re-importing the fixture twice yields no new records.

## FIF-028 Trade Republic: money mapping
Status: todo
Requirements: IMP-TR-005, IMP-TR-006, IMP-TR-007
Depends on: FIF-027, FIF-008
Acceptance: `price`, `amount`, `fee`, `tax`, `currency` map to the native figures; `fee` + `tax` sum into the single `fees` field; where `original_amount`, `original_currency` and `fx_rate` are populated, the trade is valued from them with rate source `broker`; otherwise `native` or `ecb` per FIF-009.

## FIF-029 Trade Republic: row classification
Status: blocked
Requirements: IMP-TR-008, IMP-TR-009
Depends on: FIF-028, FIF-018
Acceptance: `TRADING`/`BUY` and `TRADING`/`SELL` derive automatically; `CORPORATE_ACTION`/`TAX_EXCHANGE` derives a lot transfer directly, with both quantities taken from the row and nothing left pending; `CASH`/`DIVIDEND`, `INTEREST_PAYMENT`, `CUSTOMER_INBOUND` are recognized as non-position; the Saxo `Positie-ID` heuristic is not applied here. An unrecognized `category`/`type` pair fails to parse rather than being dropped.
Blocked by: `importers.md` says `CORPORATE_ACTION` / `TAX_EXCHANGE` **and similar** without enumerating the other types. Which `type` values map to a lot transfer is a specification decision, not an implementation choice. See decision-required D1.
Notes: The `TRADING` and `CASH` halves are fully specified; if the decision is slow, split this item rather than guessing at the enumeration.

## FIF-030 Income tax overview report
Status: todo
Requirements: DOM-001, DOM-073, DOM-074, DOM-075
Depends on: FIF-015
Acceptance: gain/loss aggregated per year (the year of the sale) and per account, with columns year, account, proceeds, sell fees, cost, buy fees, gain/loss; optional account filter and optional tax year filter, unfiltered meaning all accounts and all years; the figures are raw gain/loss with no tax-law treatment applied (no Teilfreistellung, no loss pots, no allowances).

## FIF-031 Buy report
Status: todo
Requirements: DOM-076, DOM-077, DOM-078, DOM-079
Depends on: FIF-015
Acceptance: every buy listed with date, account, security, quantity, remaining unsold quantity, unit price, fees and realized gain/loss, and beneath it each attributed sell with sell date, quantity consumed, allocated proceeds, allocated sell fee, allocated cost, allocated buy fee and gain/loss; the year filter selects buys having at least one allocation in that year; the account filter applies as in FIF-030.

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
Requirements: SRV-012, SRV-013, SRV-014, SRV-015, SRV-016, SRV-017, SRV-018
Depends on: FIF-034, FIF-023, FIF-029
Acceptance: the caller supplies target account, format and file; formats are Saxo NL XLSX and Trade Republic DE CSV; unknown ISINs are auto-created and flagged; re-posting the same file changes nothing; non-position rows are counted and not stored; the response reports derived automatically, pending, recognized as non-position, failed to parse, and securities auto-created; a sell exceeding holdings imports fine and surfaces only at attribution.

## FIF-036 Import batch endpoints
Status: todo
Requirements: SRV-019, SRV-020, SRV-021, SRV-022
Depends on: FIF-035
Acceptance: every import creates a batch; batches are readable and listable with account, filename, format, timestamp and counts; deleting a batch removes exactly its source records and everything derived from them; deletion is refused, naming the offending transactions, when any derived transaction participates in an attribution.

## FIF-037 Source record endpoints
Status: todo
Requirements: SRV-023, SRV-024, SRV-025, SRV-026, SRV-027
Depends on: FIF-035
Acceptance: source records readable and listable with filters on account, batch, security, kind and consumed-or-pending; the pending filter is the completion queue; a `manual` source record can be created; all `manual` records can be listed for export; there is no update endpoint.

## FIF-038 Transaction and corporate action endpoints
Status: todo
Requirements: SRV-028, SRV-029, SRV-030, SRV-031, SRV-032, SRV-033, SRV-034
Depends on: FIF-037, FIF-018
Acceptance: transactions readable and listable with filters on account, security, type and date range, including an unattributed-sells filter; corporate actions readable and listable; a derive endpoint builds a transaction or a corporate action from one or more pending source records plus what the user supplied, storing the supplied part as a `manual` record cited alongside the imported ones; deleting a derived transaction or corporate action returns its source records to pending; no endpoint creates a transaction from nothing.

## FIF-039 Attribution endpoints
Status: todo
Requirements: SRV-035, SRV-036, SRV-037, SRV-038, SRV-039, SRV-040, SRV-041, SRV-042, SRV-043
Depends on: FIF-038, FIF-015
Acceptance: a proposal endpoint returns the sell, the proposed allocations with derived figures and a fingerprint, and is also addressable as "the next sell awaiting attribution" for an account and security; an uncoverable sell returns the named shortfall instead; a security with pending records is refused, naming what is outstanding; creating an attribution requires the fingerprint the client was shown, and a mismatch is a conflict; attributions can be read, listed and deleted but never updated; deletion is refused when a later attribution exists for that account and security; declining is not an API call.

## FIF-040 Report endpoints
Status: todo
Requirements: SRV-044, SRV-045
Depends on: FIF-039, FIF-030, FIF-031
Acceptance: read-only endpoints for the income tax overview and the buy report, both accepting optional account and year filters, returning exactly the figures the CLI formats with no additional computation client-side.

## FIF-041 FX rate endpoints
Status: todo
Requirements: SRV-046, SRV-047
Depends on: FIF-033, FIF-010
Acceptance: list cached rates with filters, and trigger a refresh from the ECB feed; the cache seeds from the full historical series on first use. Integration tests inject a fake rate source; nothing reaches the network.

## FIF-042 CLI binary, arguments and HTTP-only access
Status: todo
Requirements: CLI-001, CLI-006, ARC-004, ARC-005
Depends on: FIF-032
Acceptance: `fifolio-cli` replaces the stub and starts the interactive application; `--server-url` defaults to `http://127.0.0.1:8000`; the crate has no database dependency and no domain rule of its own — every figure it shows comes from the server.

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
Acceptance: `fifolio-cli report <name>` prints the corresponding report with `--format` human-readable | csv | json and optional `--account` and `--year`; output is untranslated, ISO dates, dot decimals, and pipeable; reports are absent from the interactive application; `insta` snapshots over a fixed scenario built in code, one snapshot per format, so a layout change cannot silently alter the CSV.

## FIF-047 `export-manual-information` subcommand
Status: todo
Requirements: CLI-005, CLI-009
Depends on: FIF-045, FIF-037
Acceptance: writes every `manual` source record to a file, recording per entry the account, the security, what was supplied, and which imported source records it was attached to.
Notes: `cli.md` specifies the content but not the file format, and leaves open whether a matching import exists. See decision-required D2; the item is implementable only once the format is fixed.

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
Requirements: CLI-020, CLI-021, CLI-022
Depends on: FIF-050, FIF-038
Acceptance: an entry shows the imported rows it covers, every figure the file does state, and the one missing part; what is asked matches the event — stock-or-cash election plus share count for dividends, ratio for splits, target security and ratio for exchanges, quantity disposed and any shares received for cash mergers, tenders and partial buybacks; money, dates, currency and rate are never retyped; a group containing a reversal shows it as a reversal instead of folding it into a total.

## FIF-052 TUI attribution flow
Status: todo
Requirements: CLI-033, CLI-034
Depends on: FIF-051, FIF-039
Acceptance: after selecting an account and optionally a security, the application requests the next sell awaiting attribution and shows it with proposed buys and per-allocation figures; approving posts the proposal back with its fingerprint and moves to the next sell; declining stores nothing and stops, because later sells are blocked; a shortfall is displayed with the missing quantity named instead of a proposal; when the security has anything outstanding in the completion queue, the application says so and links there rather than offering a proposal.

## FIF-053 End-to-end tests
Status: todo
Requirements: TST-007
Depends on: FIF-052, FIF-046
Acceptance: tests spawn the real `fifolio-server` binary on a temporary database and a free port and drive it with the real `fifolio-cli` binary: import a fixture, resolve the completion queue, approve an attribution, run a report, asserting on what a user would see. Kept to those paths; no network access.

---

# Decisions required

These are for `spec-auditor`; this plan does not resolve them.

* **D1 — Trade Republic corporate action types.** `importers.md` [IMP-TR-008] maps `CORPORATE_ACTION` / `TAX_EXCHANGE` "and similar" to a lot transfer without enumerating what else qualifies, and the sample contains only `TAX_EXCHANGE`. Blocks FIF-029.
* **D2 — `export-manual-information` file format.** `cli.md` [CLI-009] states the content per entry but not the format (JSON, CSV, something re-importable), and explicitly leaves open whether a matching import exists. FIF-047 cannot be reviewed against the specification until this is fixed.

# Requirement coverage

All 244 requirement identifiers in `design/` are assigned to exactly one item. Nothing is uncovered
and nothing is deliberately deferred.

Requirements whose coverage is worth calling out because the item that owns them is not the obvious one:

* DOM-003 (per-currency sub-accounts normalize onto one Depot) sits in FIF-020 with the Saxo suffix rule, because that is the only place it is observable.
* DOM-049 and DOM-067 state the same block from two sides; DOM-049 is owned by the attribution service (FIF-015), DOM-067 by the invariant enforcement item (FIF-012).
* DOM-002 and DOM-046 likewise: both are owned by FIF-017, which is where non-position rows are recognized and discarded.
* DOM-080 (editable acquisition date, Altbestand) is owned by FIF-024, because `Deponering` is what creates the defaulted date.
* TST-002 enumerates what core must unit test; FIF-002 owns the convention, and the rules it lists are asserted inside the items that implement them.
* ARC-023 (spreadsheet reader plus CSV reader) is owned by FIF-017, where the reader abstraction lives.
