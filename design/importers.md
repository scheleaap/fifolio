# Importers

Format-specific detail. The domain model is in `domain.md`; nothing here belongs in it.

Both mappings were derived from real exports in `example_exports/`: five Saxo files covering 2021-11-29 to 2025-12-31 (188 rows), and one Trade Republic file covering 2024 (21 rows).

# Saxo NL

## File format

**XLSX, not CSV.** One sheet, one header row, 31 columns, identical across all five files. [IMP-SAXO-001]

* Headers are Dutch, and several contain non-breaking spaces (`Bk\xa0Record\xa0Id`, `Booking\xa0Id`) or a leading space (` Positie-ID`). Normalize whitespace before matching [IMP-SAXO-002]
* Dates are Excel serial numbers, not text [IMP-SAXO-003]
* The export language follows the account's Saxo interface language, so header and label matching is language-dependent. Only Dutch is supported [IMP-SAXO-004]

## Account

`Rekening-ID` carries a currency suffix: `69900/1000000EUR`, `...USD`, `...CAD` are three sub-accounts of one Depot. **Strip the suffix**, because FIFO applies per Depot. [IMP-SAXO-005] `Klant-id` is the client, not the account. [IMP-SAXO-006]

## Identity

There is no single id column. Identity is the first populated of `Transactie-ID`, `Bk Record Id`, `Booking Id`, `Corporate action-Id`. [IMP-SAXO-007]

`Corporate action-Id` is shared by every row of one event, so it alone is not unique. The fallback
identity is that id, plus `Acties`, plus `Boekingsbedrag`, plus **the row's ordinal within its
`Corporate action-Id` group in file order**. [IMP-SAXO-008] The ordinal is what makes it collision-proof:
the TransAlta merger pays over three rows under one id, and without it two rows sharing a label and
an amount would be deduplicated as a re-import, silently losing money from the event. If two rows
still produce one identity, the file is rejected rather than deduplicated [IMP-SAXO-024]. Every row in the sample carries at least one of the four.

## Money

| Column | Meaning |
| --- | --- |
| `Boekingsbedrag` | cash movement in the native currency (`_Valuta`), **including** costs |
| `Aantal` | the same amount in EUR |
| `Totale kosten` | costs, **in EUR**, always negative |
| `Omrekeningskoers` | native to EUR multiplier |

Despite its name, `Aantal` is never a quantity. [IMP-SAXO-009]

So both EUR figures are directly derivable and nothing has to be split by guesswork: [IMP-SAXO-010]

`Aantal` is the cash movement, so on a buy it is gross **plus** costs and on a disposal it is gross
**minus** costs. The derivation is therefore sign-aware; one formula for both directions understates
every disposal's proceeds by twice the fee.

    EUR gross  = |Aantal| − |Totale kosten|      for a buy   (cash out includes costs)
    EUR gross  = |Aantal| + |Totale kosten|      for a disposal (cash in is net of costs)
    EUR fees   = |Totale kosten|
    EUR price  = EUR gross / (quantity × factor)

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

**The price in that string is rounded to 2 decimals and must never be used for money.** [IMP-SAXO-011] The one sell in the sample reads `30.65`, while 1833.24 + 6.00 over 60 units gives 30.654. Take the quantity and the direction from the label, every figure from the columns. [IMP-SAXO-012]

## Row classification

`Transactietype` is `Transactie`, `Corporate action` or `Geldoverboeking`; `Acties` refines it. Classification: [IMP-SAXO-013]

| `Acties` | Variant | Handling |
| --- | --- | --- |
| `Koop` | `buy` | derived automatically |
| `Verkoop` | `sell` | derived automatically |
| `Deponering` | `transfer_in` | derived automatically, source `broker`, date provenance `stated` (see below) |
| `Expiratie` | `expiration` | derived automatically; quantity is the remaining position, proceeds from the row |
| `Fusie`, `Terugkoopaanbod` (+ `Terugboeking`) | `sell` and/or `transfer_out` | pending: the quantity disposed, and any target security |
| `Stock split` | `split` | pending: the ratio |
| `Omwisseling` | `transfer_out` | pending: the target security and the ratio |
| `Dividend`, `Keuzedividend`, `Herbeleggingsdividend` | `buy` or none | see Dividends |
| `Rente`, `Service fee`, `ADR-kosten`, `Storting`, `Opname` | none | recognized as non-position, not stored |

### `Deponering`

A transfer in from another broker. `Boekingsbedrag` and `Aantal` are zero; the quantity and price come from the `Acties` label.

The price is a **historical acquisition price restated to the transfer date**, that is, adjusted for any split between purchase and transfer. [IMP-SAXO-014] Two consequences:

* `quantity × price` is split-invariant, so it is the true acquisition cost and can be taken at face value
* splits occurring *after* the transfer are still to be applied, and appear as their own rows. There is no double counting

Import creates a complete `transfer_in` with source `broker`: quantity and price from the label, acquisition date defaulted to the transfer date, date provenance `stated`. [IMP-SAXO-015] The date stays editable, because it is the one figure that is not real [IMP-SAXO-016] (see Altbestand in `domain.md`).

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

* `date` is the effective date and becomes the **trade date**. `datetime` is a booking timestamp, used only as a deterministic tie-break within a day; it is not an execution time and should not be described as one [IMP-TR-002]
* The two are independent fields, not derivable from each other. Across four years they agree on every trade, diverge by a day on three dividends (two of which cross midnight UTC) and by **six days** on the corporate action, which is booked after it takes effect [IMP-TR-015]
* `transaction_id` is a UUID: a stable broker reference, used directly as the identity [IMP-TR-003]
* Sign convention is cash flow, so buys and fees are negative [IMP-TR-004]

## Money

Native columns are `price`, `amount`, `fee`, `tax`, `currency`. [IMP-TR-005] Where the row was in another currency, `original_amount`, `original_currency` and `fx_rate` are populated. [IMP-TR-006] `fee` and `tax` are summed into the domain's single `fees` field. [IMP-TR-007]

**`amount` excludes the fee**, the opposite of Saxo's `Boekingsbedrag`: the sample's `35 × 75.09 = 2628.15` equals `|amount|` with `fee` of −1.00 carried separately. So: [IMP-TR-016]

    gross = |amount|
    fees  = |fee| + |tax|          both in the settlement currency

`fx_rate` is foreign units per EUR, which is already the stored convention, so no inversion is needed.

No foreign-currency **trade** appears in four years of exports, so it is not specified: a `TRADING` row with `original_*` populated **rejects the import** rather than being guessed at, consistent with the unknown-type rule. [IMP-TR-017] `tax` has likewise never appeared on a trade.

## Row classification

`category` is `TRADING`, `CASH` or `CORPORATE_ACTION`; `type` refines it. Classification: [IMP-TR-008]

| `category` / `type` | Handling |
| --- | --- |
| `TRADING` / `BUY`, `SELL` | derived automatically |
| `CORPORATE_ACTION` / `TAX_EXCHANGE` | a `transfer_out` and its `transfer_in`; quantities are stated |
| `CORPORATE_ACTION` / anything else | the import is rejected [IMP-TR-010] |
| `CASH` / `DIVIDEND`, `INTEREST_PAYMENT`, `CUSTOMER_INBOUND`, `TRANSFER_INBOUND`, `STOCKPERK` | recognized as non-position, not stored |
| anything else naming a security | the import is rejected [IMP-TR-013] |
| anything else naming no security | not stored, but counted and named in the import summary [IMP-TR-014] |

Trade Republic states share quantities on corporate actions, so the Saxo dividend heuristic is neither needed nor applicable here. [IMP-TR-009]

### `TAX_EXCHANGE`

A fund merger. The sample's pair, effective 2024-01-18, moves −60.00 of `LU1861134382` out and
+60.00 of `IE000Y77LGG9` in: an Amundi MSCI World SRI ETF redomiciled from Luxembourg to Ireland,
substituted one for one with no price, no amount and no tax.

Treated as tax-neutral: a `transfer_out` of the negative side and a `transfer_in` of the positive,
basis and acquisition dates carrying across. [IMP-TR-018] The evidence supports it — a German broker
must withhold on a realized gain, and the rows carry no monetary figure of any kind — but the label
is TR's and the reading is inference from absence. The user's Trade Republic tax report for the year
settles it, and a different answer there means revisiting this.

Pairing key: same account, same effective `date`, same absolute quantity, opposite signs. The ratio
is `|target quantity| / |source quantity|`. An unpaired row, or more than two rows sharing a key,
**rejects the import**. [IMP-TR-019]

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
**not** assumed here: a security that would map to `bond` from this format is created pending review
rather than given a quotation, and the import summary says so. [IMP-TR-021]

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
