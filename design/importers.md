# Importers

A file whose rows carry **trade dates** in more than one calendar year is refused. [IMP-001] The trade date decides, not a booking timestamp and not the filename: the trade date is what the domain already treats as authoritative, so a file named for 2024 can still be refused if it carries a 2023 trade date. [IMP-002] Broker exports are taken a year at
a time, which keeps a corporate action's rows inside one file and makes the ownership of a
re-imported row unambiguous. A partial year is fine — the first Saxo export runs 2021-11-29 to
2021-12-31.

An account records its broker account id when it is created. An import whose file names a different
account is refused, naming both, and a file carrying rows from more than one account is refused
outright. A format that states no account id, as Trade Republic's does not, has nothing to check and
is imported into the account the caller names (DEC-075). [IMP-003] This is the guard against the mis-aimed import that batch deletion exists to undo.

Format-specific detail. The domain model is in `domain.md`; nothing here belongs in it.

Both mappings were derived from real exports in `example_exports/`: five Saxo files covering 2021-11-29 to 2025-12-31 (188 rows), and one Trade Republic file covering 2024 (21 rows).

# Saxo NL

## File format

**XLSX, not CSV.** **Three sheets**, each with one header row, identical across all five files:
`Transacties` (31 columns), `_Transacties` (24 columns) and `Bookings` (21 columns).
[IMP-SAXO-001]

`Transacties` is the cash ledger: one row per booked movement, and the sheet every other rule here
was originally written against. The other two are detail sheets, and between them they carry the
quantities, prices and tax figures that `Transacties` does not.

| Sheet | Rows in the sample | What it holds | Joined by |
| --- | --- | --- | --- |
| `Transacties` | 188 | the booked cash movement, the instrument, the dates | — |
| `_Transacties` | 33 | the **position** side: signed quantity, price, traded value, buy/sell direction, open/close, order id, an execution timestamp | `Transactie-ID`, else `Corporate action-Id` |
| `Bookings` | 242 | the **components** of a cash movement: amount type, eligible quantity, dividend per share, ex-date, book date, withholding percentage | `Bk Record Id`, `Booking Id`, else `Corporate action-Id` |

Every position-affecting row of `Transacties` has a `_Transacties` counterpart: all 13 `Deponering`,
both `Stock split`, all three `Fusie`, all three `Keuzedividend`, the `Koop`, the `Verkoop`, the
`Omwisseling`, the `Terugkoopaanbod` with its reversal and the `Expiratie` — 32 rows in all. The
remaining 156 are cash movements and appear in `Bookings` instead. A row may join on
`Corporate action-Id` rather than on a transaction id, and a corporate action's legs are its
`_Transacties` rows under one `Corporate action-Id`. [IMP-SAXO-037]

### Legs

There are not always two. Every shape observed in the sample:

| Legs | Count | Example |
| --- | --- | --- |
| one `Gekocht` | 4 | a stock election issuing one share |
| one `Verkocht` + one `Gekocht` | 4 | both splits, the merger, the exchange |
| one `Verkocht` | 3 | the expiration |
| **two `Gekocht`** | 1 | the 2023 Philips dividend, **two** shares at 34.74 each |
| **two `Verkocht` + one `Gekocht`** | 1 | the DeVolksbank tender, containing a reversal |
| one `Deponering` | 13 | a transfer in; the event type is neither `Gekocht` nor `Verkocht` |

Two rules follow, and neither is optional.

**A leg count is never assumed.** The quantity of a side is the **sum** of that side's legs. The
2023 Philips dividend issues 2 shares, not 1, and a rule reading "the `Gekocht` leg" loses one of
them — a share never acquired, never disposed and never taxed. [IMP-SAXO-044]

**A reversal cancels before anything else is read, and it is identified by its label.** A leg whose
`Acties` carries the `- Terugboeking` suffix is a reversal. It cancels against the leg in the same
group with the same absolute quantity, the **same** price, and the opposite-signed traded value.
Both are removed and the event is what remains. A group containing no `- Terugboeking` leg never
cancels anything. [IMP-SAXO-045]

The label is the signal because the shape is not sufficient. The DeVolksbank tender's reversal is
`Gekocht 2000 @ 999.03 / -19980.65` against `Verkocht -2000 @ 999.03 / 19980.65`, and the sample's
one `Omwisseling` is `Gekocht 3 @ 168.63 / -505.89` against `Verkocht -3 @ 168.63 / 505.89`. On
quantity, price and traded value the two are the same shape. One is a booking being undone; the
other is a genuine exchange whose `transfer_out` IMP-SAXO-041 requires. Cancelling on shape alone
destroys the exchange and leaves its group with no legs at all. `Openen/sluiten` corroborates — a
real opening leg reads `Te openen` and the reversal's reads `Te sluiten` — but the suffix is the
rule, in keeping with this document's refusal to classify a row by its shape. [IMP-SAXO-046]

Note also that the price on a cancelling pair is **equal**, not negated: both DeVolksbank legs read
999.03. An earlier wording here said "the exact negatives" of quantity, price and traded value,
which matches nothing in five years of exports — the tender it was written for would not have
cancelled.

After cancellation the tender is one disposal of 2000 at 99.90 for 1998.07, which is what `Bookings`
and the cash ledger both show. Read as an opening, its `Gekocht` invents a 19,980.65 acquisition
that never happened.

A side's summed traded value is available because a group's legs carry one instrument and so one
currency; a group whose legs disagree on instrument currency is refused rather than summed.
[IMP-SAXO-047]

`Deponering` is a **third side**, neither buying nor selling: all 13 transfer legs carry it, and a
rule that recognizes only `Gekocht` and `Verkocht` drops every transferred-in parcel. A side's
summed quantity keeps the file's sign, so a `Verkocht` side is negative. [IMP-SAXO-048]

* Headers are Dutch, and several contain non-breaking spaces (`Bk\xa0Record\xa0Id`, `Booking\xa0Id`) or a leading space (` Positie-ID`). Normalize whitespace before matching [IMP-SAXO-002]
* Dates are Excel serial numbers, not text [IMP-SAXO-003]
* Rows are emitted **newest first**, so the file's direction must be normalized before its row positions are used for ordering [IMP-SAXO-025]
* The export language follows the account's Saxo interface language, so header and label matching is language-dependent. Only Dutch is supported [IMP-SAXO-004]

## Ordering

Rows are sorted on `Transactiedatum`, then on `Bk Record Id`, then on `Booking Id`, then on
`Transactie-ID`, each compared **only against itself**, and finally on file position taken in
reverse, since the export is newest-first. [IMP-SAXO-026]

Each of the three is a monotonic counter that ascends with date, but they are three disjoint
counters and their magnitudes are unrelated — roughly 3.0e9, 4.0e10 and 7.0e9 in the fixtures — so
folding them into one key would order a date carrying a mixture by which column a row happens to
populate, which is arbitrary with respect to time. Ten dates in the fixtures carry such a mixture,
and same-day order decides which parcel a same-day sell consumes, so it is a cost-basis input.

The choice is therefore made **per date**, not per pair of rows, so that the result is a sort order
at all. Within one `Transactiedatum`: if every row of that date populates the same one of the three
columns, the date is ordered by that counter. Otherwise the whole date is ordered by file position
taken in reverse, and no counter is consulted. A pairwise rule — compare two rows by a counter when
they share one, fall back to file position when they do not — is **not** transitive and so is not a
sort order: given `Bk 5`, `Booking 100` and `Bk 7`, the first and third compare by counter while
each compares to the second by position, and the three cannot be laid in a line. [IMP-SAXO-036]

`Corporate action-Id` is **not** monotonic with date and is never an ordering column. [IMP-SAXO-027]
Across the sample the id columns separate every row on 32 of the 33 dates carrying more than one;
the two exceptions are corporate-action rows where only that id is populated, and file position
settles them.

## Account

`Rekening-ID` carries a currency suffix: `69900/1000000EUR`, `...USD`, `...CAD` are three sub-accounts of one Depot. **Strip the suffix**, because FIFO applies per Depot. [IMP-SAXO-005] `Klant-id` is the client, not the account. [IMP-SAXO-006]

## Identity

There is no single id column. Identity is the first populated of `Transactie-ID`, `Bk Record Id`, `Booking Id`, `Corporate action-Id`. [IMP-SAXO-007]

`Corporate action-Id` is shared by every row of one event, so it alone is not unique. The fallback
identity is that id, plus `Acties`, plus `Boekingsbedrag`, plus **the row's ordinal within its
`Corporate action-Id` group in normalized, oldest-first order** (IMP-SAXO-025), not in the file's newest-first order (DEC-082). [IMP-SAXO-008] The ordinal is what makes it collision-proof:
the TransAlta merger pays over three rows under one id, and without it two rows sharing a label and
an amount would be deduplicated as a re-import, silently losing money from the event. If two rows
still produce one identity, the file is rejected rather than deduplicated [IMP-SAXO-024]. Every row in the sample carries at least one of the four.

## Money

| Column | Meaning |
| --- | --- |
| `Boekingsbedrag` | cash movement in the native currency (`_Valuta`), **including** costs |
| `Aantal` | the same amount in EUR |
| `Totale kosten` | costs, **in EUR**, always negative |
| `Omrekeningskoers` | native to EUR multiplier. The stored rate is its reciprocal, rounded to the FX scale [IMP-SAXO-029] |

Despite its name, `Aantal` is never a quantity. [IMP-SAXO-009]

So both EUR figures are directly derivable and nothing has to be split by guesswork: [IMP-SAXO-010]

`Aantal` is the cash movement, so on a buy it is gross **plus** costs and on a disposal it is gross
**minus** costs. The derivation is therefore sign-aware; one formula for both directions understates
every disposal's proceeds by twice the fee.

    EUR gross  = |Aantal| − |Totale kosten|      for a buy   (cash out includes costs)
    EUR gross  = |Aantal| + |Totale kosten|      for a disposal (cash in is net of costs)
    EUR fees   = |Totale kosten|
    EUR price  = EUR gross / (quantity × factor)

The native-side figures follow the same shape, with the costs converted back from EUR using the
file's own `Omrekeningskoers` — not the stored rate, which is its reciprocal:
`native fees = EUR fees / Omrekeningskoers`, and `native gross = |Boekingsbedrag| − native fees` for
a buy, `+` for a disposal. [IMP-SAXO-030]

That subtraction keeps full precision, like every other intermediate. The stored native unit price
therefore need not equal the figure the statement prints: the sample buy stores `5.750036` against
a printed `5.75`, exactly as the sample sell stores `30.654` against a printed `30.65`. The printed
price is a rounded display, the derived one is what the booked amounts imply, and the derived one is
authoritative. [IMP-SAXO-032]

`factor` is the security's quotation factor: 1 per unit, 0.01 percent of par. Dividing by it here is
what makes `EUR price` the quoted price the statement shows, so that the domain's
`quantity × unit_price × factor` reproduces the gross exactly and the factor is applied once rather
than twice [IMP-SAXO-023].

Worked example, the one buy in the sample:

    Koop 40 @ 5.75 USD
    Boekingsbedrag -238.00 USD   Aantal -216.92 EUR
    Totale kosten -7.29 EUR      Omrekeningskoers 0.911413

    238.00 × 0.911413 = 216.92          Aantal confirmed
    7.29 / 0.911413   = 8.00 USD        costs in native
    238.00 − 8.00     = 230.00 = 40 × 5.75
    EUR gross = 216.92 − 7.29 = 209.63  EUR price = 5.240750

And the sample's only disposal, which goes the other way:

    Verkoop -60 @ 30.65 EUR
    Aantal 1833.24 EUR   Totale kosten -6.00 EUR

    EUR gross = 1833.24 + 6.00 = 1839.24   EUR price = 30.654

The bond case, where the factor matters:

    Deponering 3000 @ 139.46 EUR   cost 4183.80
    EUR price = 4183.80 / (3000 × 0.01) = 139.46      the quoted price, not 1.3946

## Quantity and direction

Quantity and direction are only present inside the free-text `Acties` string: `Koop 40 @ 5.75 USD`, `Verkoop -60 @ 30.65 EUR`, `Deponering 300 @ 51.40 EUR`.

**The price in that string is rounded to 2 decimals and must never be used for money.** [IMP-SAXO-011] The one sell in the sample reads `30.65`, while 1833.24 + 6.00 over 60 units gives 30.654. Every figure comes from a column. [IMP-SAXO-012]

The quantity and the direction come from a column too, on `_Transacties`: `Traded Quantity` is signed and `Trade Event Type` states `Gekocht` or `Verkocht`. Parsing them out of the label is a fallback for a row with no `_Transacties` counterpart, not the normal path, and a label whose parsed quantity disagrees with the column **refuses the file** rather than choosing one. [IMP-SAXO-038]

## Row classification

`Transactietype` is `Transactie`, `Corporate action` or `Geldoverboeking`; `Acties` refines it. Classification: [IMP-SAXO-013]

| `Acties` | Variant | Handling |
| --- | --- | --- |
| `Koop` | `buy` | derived automatically |
| `Verkoop` | `sell` | derived automatically |
| `Deponering` | `transfer_in` | derived automatically, source `broker`, date provenance `transfer_date` (see below) |
| `Expiratie` | `expiration` | derived automatically; quantity is the remaining position, proceeds from the row. Pending if nothing remains |
| `Fusie`, `Terugkoopaanbod` (+ `Terugboeking`) | `sell` and/or `transfer_out` | derived: both legs are `_Transacties` rows under the one `Corporate action-Id`, the `Verkocht` legs disposing and the `Gekocht` legs opening, each side summed after cancellation [IMP-SAXO-044, IMP-SAXO-045]. Cash and costs go to the sell leg [IMP-SAXO-031] |
| `Stock split` | `split` | derived: the ratio is the summed `Gekocht` quantity over the summed `Verkocht` quantity, an exact integer pair — Tesla `45 : 15` is 3:1, OBAM `20 : 4` is 5:1 [IMP-SAXO-040] |
| `Omwisseling` | `transfer_out` | derived: the two `_Transacties` legs give both quantities, hence the ratio; the target security is the `Gekocht` legs' instrument, which must be a single instrument or the file is refused [IMP-SAXO-041] |
| `Dividend`, `Keuzedividend`, `Herbeleggingsdividend` | `buy` or none | see Dividends |
| `Rente`, `Service fee`, `ADR-kosten`, `Storting`, `Opname` | none | recognized as non-position, not stored |

### Reversals

`Terugboeking` is never an `Acties` value on its own. It appears as a **suffix** on the action it
reverses, and the value must be matched as such: the exports carry `Terugkoopaanbod - Terugboeking`
and `Dividend - Terugboeking`. A suffixed value classifies as its prefix, reversing. [IMP-SAXO-033]

A reversal row's cash **subtracts** from its group's summed cash; it is a claw-back, not a second
payment. The sample is the DeVolksbank tender: `3946.14` paid, `-1998.07` reversed, `1948.07` net.

Its costs also subtract. Both reversal rows in five years of exports carry **zero** in
`Totale kosten`, so the rule is untested by the data and is chosen to be arithmetically consistent
with the cash: whatever the reversal undoes, it undoes whole. If a reversal ever arrives carrying a
non-zero cost, that is the point to check against a statement rather than trust this
sentence. [IMP-SAXO-034]

Summation happens on the **EUR** figures the rows carry, not on the native ones before conversion,
because a group's rows may hold different `Omrekeningskoers` values and only booked EUR totals
compute. [IMP-SAXO-035]

### `Deponering`

A transfer in from another broker. `Boekingsbedrag` and `Aantal` are zero; the quantity and price come from the `Acties` label.

The price is a **historical acquisition price restated to the transfer date**, that is, adjusted for any split between purchase and transfer. [IMP-SAXO-014] Two consequences:

* `quantity × price` is split-invariant, so it is the true acquisition cost and can be taken at face value
* splits occurring *after* the transfer are still to be applied, and appear as their own rows. There is no double counting

Import creates a complete `transfer_in` with source `broker`: quantity and price from the label, acquisition date set to the transfer date, date provenance `transfer_date`. [IMP-SAXO-015] The date is never corrected, so any grandfathered status the parcel carried is not represented [IMP-SAXO-016] (see Altbestand in `domain.md`).

`Boekingsbedrag` and `Aantal` are zero on these rows, so the cost basis is not on `Transacties` at
all. It is on `_Transacties`: `Verhandelde waarde` is the traded value and is authoritative, with
`Traded Quantity` the quantity. [IMP-SAXO-028]

This matters by more than a rounding tolerance. `Verhandelde waarde` is **not** the label price
times the quantity, because the label price is rounded to 2 decimals while the value is exact. All
thirteen transfers in the sample:

| Label | quantity x label price | `Verhandelde waarde` | error |
| --- | --- | --- | --- |
| `300 @ 14.06 CAD` | 4218.00 | 4216.50 | 1.50 |
| `345 @ 7.29 EUR` | 2515.05 | 2514.15 | 0.90 |
| `300 @ 51.40 EUR` | 15420.00 | 15419.46 | 0.54 |
| `60 @ 32.31 EUR` | 1938.60 | 1938.30 | 0.30 |
| `30 @ 36.73 EUR` | 1101.90 | 1101.95 | 0.05 |
| `15 @ 146.08 USD` | 2191.20 | 2191.21 | 0.01 |
| the other seven | — | — | exact |

3.30 in total, on a cost basis that is subtracted from a future gain. An earlier version of this
document accepted a "half-cent tolerance" of 0.15 on the bond; that figure was derived from the
`Transacties` sheet alone and is wrong by an order of magnitude on the worst row. The label's price
is never used for money here either. [IMP-SAXO-039]

`Omrekeningskoers` is 1 on these rows even for foreign-currency instruments, so a non-EUR transferred lot has **no** EUR cost basis in the file and needs an ECB rate lookup for the transfer date. [IMP-SAXO-017]

### Dividends

`Acties` is not a reliable signal: across five years Saxo used `Dividend`, `Keuzedividend` and `Herbeleggingsdividend` for both cash and stock events, and `Keuzedividend` appears once with no position effect at all.

`Positie-ID` is the signal that tracked reality. The rule: [IMP-SAXO-018]

    Group dividend-type rows by Corporate action-Id.
      If any row in the group carries a Positie-ID
          → pending: requires an explicit stock-or-cash decision, stock pre-selected
      Otherwise
          → recognized as non-position, not stored

Grouping first is not optional. [IMP-SAXO-019] The 2022 Philips event is two rows under one `Corporate action-Id`: the position-marked row carries zero and the cash row carries the money with no marker. Evaluated row by row, the money is silently dropped.

Choosing stock derives a `buy` whose origin is `stock_dividend` and whose cost basis is the taxable value of the shares issued; choosing cash stores nothing.

Stock is pre-selected because all six position-marked rows in the sample were stock elections, and because choosing stock requires a share count and so cannot be accepted blindly, whereas choosing cash is a single keystroke that would silently discard an acquisition.

This is a heuristic calibrated on one account and one issuer. It is acceptable because its failure mode is safe: an understated position surfaces later as a blocked attribution with a named shortfall, not as a wrong number.

In the sample it isolates 6 rows out of 74 dividend-type rows. The 19 `Herbeleggingsdividend` rows were verified against declared dividends per share and are ordinary cash despite the label.

`Bookings` settles that directly rather than by inference. Each of the 19 decomposes into exactly
two components, `Corporate actions - Fracties` and `Corporate actions - Voorheffing` — the cash paid
for a fraction too small to issue, and withholding tax on it — and never a share amount. The
eligible position on those rows is 3 throughout, and across five years and eleven bookings it never
grows. No shares were issued, so there is no share count to ask for. [IMP-SAXO-042]

Where a stock election **does** issue shares, both sheets say so. The 2025 Philips
`Keuzedividend` carries one `_Transacties` leg `Gekocht 1 @ 20.09` and a `Bookings` component
`Corporate Actions - Share Amount` of `-20.09` against a `Corporate Actions - Cashdividenden` of
`28.05` on 33 eligible shares. The share count and the taxable value are both derivable, so this is
not a manual entry either. [IMP-SAXO-043]

## Bonds

`Type` is `Bond`, which is what defaults the security's quotation to `percent_of_par`. [IMP-SAXO-020] Confirmed against the sample: a 7.5% bond with `Deponering 3000 @ 139.46` paid coupons of exactly 225.00 (7.5% of 3000) and redeemed for exactly 3000.00, so 3000 is a nominal amount and the cost is 4183.80, not 418,380.

## Securities

`Instrument ISIN` is the key; `Instrument` is the name. Names carry delisting annotations such as `*Delisted 20231011 (TransAlta Renewables Inc.)` and change over time for one ISIN. [IMP-SAXO-022]

`Type` maps onto the security type: [IMP-SAXO-021]

| Saxo `Type` | Security type |
| --- | --- |
| `Stock` | `stock` |
| `Bond` | `bond`, quotation `percent_of_par` |
| `Etf` | `etf` |
| `MutualFund` | `fund` |
| `Cash` | no security is created; the row is non-position |
| anything else | the import is rejected |

# Trade Republic DE

## File format

CSV, quoted, comma-delimited, dot decimals, ASCII, single header row, 23 columns. [IMP-TR-001] The header is byte-identical across the 2022 to 2025 exports.

* `date` is the effective date and becomes the **trade date**. `datetime` is a booking timestamp, used as an ordering column after the date; it is not an execution time and should not be described as one [IMP-TR-002]
* `shares` carries the quantity; `price` the unit price [IMP-TR-022]
* Rows are ordered on `date`, then `datetime`, then file position. The 2022 to 2024 exports ascend by both. The 2025 export ascends by `date` but **descends** by `datetime` within a date, so file position alone would order it wrongly [IMP-TR-023]
* The two are independent fields, not derivable from each other. Across four years they agree on every trade, diverge by a day on three dividends (two of which cross midnight UTC) and by **six days** on the corporate action, which is booked after it takes effect [IMP-TR-015]
* `transaction_id` is a UUID: a stable broker reference, used directly as the identity [IMP-TR-003]
* Sign convention is cash flow, so buys and fees are negative [IMP-TR-004]

## Money

Native columns are `price`, `amount`, `fee`, `tax`, `currency`. [IMP-TR-005] Where the row was in another currency, `original_amount`, `original_currency` and `fx_rate` are populated. [IMP-TR-006] `fee` and `tax` are summed into the domain's single `fees` field. [IMP-TR-007]

**`amount` excludes the fee**, the opposite of Saxo's `Boekingsbedrag`: the sample's `35 × 75.09 = 2628.15` equals `|amount|` with `fee` of −1.00 carried separately. So: [IMP-TR-016]

    gross = |amount|
    fees  = |fee| + |tax|          both in the settlement currency

`fx_rate` has no reliable convention: it was foreign units per EUR up to 2024-07-02 and its reciprocal from 2024-10-01 on (see DEC-073). Every foreign-currency row in four years of exports is a dividend, which is not stored, so nothing needs the rate: a row that would be stored with `original_*` populated **rejects the import**, naming the row, rather than being guessed at, consistent with the unknown-type rule. [IMP-TR-017] `tax` has likewise never appeared on a trade.

## Row classification

`category` is `TRADING`, `CASH` or `CORPORATE_ACTION`; `type` refines it. Classification: [IMP-TR-008]

| `category` / `type` | Handling |
| --- | --- |
| `TRADING` / `BUY`, `SELL` | derived automatically |
| `CORPORATE_ACTION` / `TAX_EXCHANGE` | a `transfer_out` only; the inbound row supplies the target security and the ratio |
| `CORPORATE_ACTION` / anything else | the import is rejected [IMP-TR-010] |
| `CASH` / `DIVIDEND`, `INTEREST_PAYMENT`, `CUSTOMER_INBOUND`, `TRANSFER_INBOUND`, `STOCKPERK` | recognized as non-position, not stored |
| anything else naming a security | the import is rejected [IMP-TR-013] |
| anything else naming no security | not stored, but counted and named in the import summary [IMP-TR-014] |

Trade Republic states share quantities on corporate actions, so the Saxo dividend heuristic is neither needed nor applicable here. [IMP-TR-009]

### `TAX_EXCHANGE`

A fund merger. The sample's pair, effective 2024-01-18, moves −60.00 of `LU1861134382` out and
+60.00 of `IE000Y77LGG9` in: an Amundi MSCI World SRI ETF redomiciled from Luxembourg to Ireland,
substituted one for one with no price, no amount and no tax.

Treated as tax-neutral. The importer derives **only the `transfer_out`**, from the negative-quantity
row; the positive row supplies the target security and the ratio and is cited, not turned into a
transaction. [IMP-TR-018] The matching `transfer_in` records are emitted on approval, one per parcel
consumed, which is the only way a position built from several purchases keeps its separate
acquisition dates. The evidence supports it — a German broker
must withhold on a realized gain, and the rows carry no monetary figure of any kind — but the label
is TR's and the reading is inference from absence. The user's Trade Republic tax report for the year
settles it, and a different answer there means revisiting this.

Pairing key: same account, same effective `date`, same absolute quantity, opposite signs. The ratio
is the **integer pair** `|target quantity| : |source quantity|`, reduced by their greatest common
divisor and never divided out, for the reason a split's ratio is a pair: a one-for-three ratio has
no finite decimal expansion, and rounding it leaves a residue that grows across applications. An
unpaired row, or more than two rows sharing a key, **rejects the import**. [IMP-TR-019]

The quantity sign decides which security closes and which opens. This narrows rather than
contradicts the rule against inferring from shape: for a **recognized** type the sign is stated
data, and it is an *unrecognized* type that is never classified by its shape.

### Security type

`asset_class` maps onto the security type: [IMP-TR-020]

| `asset_class` | Security type |
| --- | --- |
| `STOCK` | `stock` |
| `FUND` | `fund` |
| anything else | the import is rejected |

Trade Republic carries ETFs as `FUND`, so `IE000Y77LGG9` is recorded as a fund though it is an ETF.
Nothing in v1 turns on that distinction — Teilfreistellung is a non-goal — and auto-created
securities are flagged for review, so it is visible and correctable.

No Trade Republic bond has been observed, so the percent-of-par convention confirmed for Saxo is
**not** assumed here: a row whose instrument would map to `bond` from this format **rejects the
import**, naming the ISIN, because its quotation cannot be established and guessing is a
hundredfold error either way. No `asset_class` value maps to `bond`, so in practice IMP-TR-020's
catch-all is what rejects such a row; this rule states why it must not be mapped (DEC-077). [IMP-TR-021]

`TAX_EXCHANGE` is the only `CORPORATE_ACTION` type observed, so it is the only one mapped. An
unrecognized type **rejects the whole file**, naming the type, the row and the transaction id, and
nothing is imported. Nothing is inferred from quantity signs: an event whose shape resembles a lot
transfer is not thereby one, and a silently miscategorized corporate action corrupts a cost basis
permanently.

The remedy is to specify the new type here and import again. That is deliberate — the file is the
evidence of what the type means, and the specification is where it gets decided.

The same rule extends past `CORPORATE_ACTION`: **a populated `symbol` is what makes a row
dangerous, not its category.** An unrecognized type that names a security rejects the file
(`IMP-TR-013`), because it may create or remove units and guessing corrupts a cost basis
permanently. An unrecognized type that names no security is almost certainly cash, so it is not
stored, but the import summary names it and counts it (`IMP-TR-014`) — a new type then becomes
visible the first time it appears rather than years later.

### `STOCKPERK`

A promotional free share. Trade Republic books it as two rows on the same date: a `CASH` /
`STOCKPERK` credit, and a `TRADING` / `BUY` of the same ISIN for the same amount, carrying the
quantity and the price. Net cash is zero and the buy row is the acquisition, at the share's value.

The `STOCKPERK` row is therefore treated as cash and not stored [IMP-TR-011], with no check that
the paired buy exists. The pairing is observed once, so if Trade Republic ever books one without a
buy the acquisition is lost — but that surfaces as a blocked attribution with a named shortfall the
first time those units are sold, never as a wrong figure in a report.

### `TRANSFER_INBOUND`

Cash arriving from another account, carrying no security and no quantity. Indistinguishable in
effect from `CUSTOMER_INBOUND`, and treated the same [IMP-TR-012].

# Open items

* **Trade Republic bonds.** None in the sample. The quotation convention is unverified, so the Saxo default must not be assumed to carry over
* **Trade Republic sells.** None in four years of exports; the sell mapping is inferred from the buy rows
* **Trade Republic trade costs.** `tax` is populated only on `DIVIDEND` and `INTEREST_PAYMENT`
  rows, never on a trade, so `IMP-TR-007`'s summing of `fee` and `tax` has no observed trade case
* **Trade Republic unused columns.** `payment_reference` and `mcc_code` are empty in all 65 sample
  rows, and `account_type` is always `DEFAULT`
* **Trade Republic corporate action types.** Only `TAX_EXCHANGE` is known. Each new type met in
  practice needs specifying here before its file will import
* **Saxo language.** Only Dutch exports are supported, and no other language has been seen
