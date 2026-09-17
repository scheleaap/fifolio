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

There is no single id column. Identity is the first populated of `Transactie-ID`, `Bk Record Id`, `Booking Id`, `Corporate action-Id`. [IMP-SAXO-007] `Corporate action-Id` is shared by every row of one event, so it alone is not unique: add `Acties` and the amount. [IMP-SAXO-008] Every row in the sample carries at least one of the four.

## Money

| Column | Meaning |
| --- | --- |
| `Boekingsbedrag` | cash movement in the native currency (`_Valuta`), **including** costs |
| `Aantal` | the same amount in EUR |
| `Totale kosten` | costs, **in EUR**, always negative |
| `Omrekeningskoers` | native to EUR multiplier |

Despite its name, `Aantal` is never a quantity. [IMP-SAXO-009]

So both EUR figures are directly derivable and nothing has to be split by guesswork: [IMP-SAXO-010]

    EUR gross  = |Aantal| − |Totale kosten|      (buy: Aantal negative)
    EUR fees   = |Totale kosten|
    EUR price  = EUR gross / quantity

Worked example, the one buy in the sample:

    Koop 40 @ 5.75 USD
    Boekingsbedrag -238.00 USD   Aantal -216.92 EUR
    Totale kosten -7.29 EUR      Omrekeningskoers 0.911413

    238.00 × 0.911413 = 216.92          Aantal confirmed
    7.29 / 0.911413   = 8.00 USD        costs in native
    238.00 − 8.00     = 230.00 = 40 × 5.75
    EUR gross = 216.92 − 7.29 = 209.63  EUR price = 5.240750

## Quantity and direction

Quantity and direction are only present inside the free-text `Acties` string: `Koop 40 @ 5.75 USD`, `Verkoop -60 @ 30.65 EUR`, `Deponering 300 @ 51.40 EUR`.

**The price in that string is rounded to 2 decimals and must never be used for money.** [IMP-SAXO-011] The one sell in the sample reads `30.65`, while 1833.24 + 6.00 over 60 units gives 30.654. Take the quantity and the direction from the label, every figure from the columns. [IMP-SAXO-012]

## Row classification

`Transactietype` is `Transactie`, `Corporate action` or `Geldoverboeking`; `Acties` refines it. Classification: [IMP-SAXO-013]

| `Acties` | Handling |
| --- | --- |
| `Koop`, `Verkoop` | derived automatically: buy, sell |
| `Deponering` | derived automatically: buy (see below) |
| `Expiratie` | sell closing the whole remaining position, proceeds from the row |
| `Fusie`, `Terugkoopaanbod` (+ `Terugboeking`) | pending: sell, and where shares are received also a lot transfer |
| `Stock split`, `Omwisseling` | pending: quantity adjustment or lot transfer |
| `Dividend`, `Keuzedividend`, `Herbeleggingsdividend` | see Dividends |
| `Rente`, `Service fee`, `ADR-kosten`, `Storting`, `Opname` | recognized as non-position, not stored |

### `Deponering`

A transfer in from another broker. `Boekingsbedrag` and `Aantal` are zero; the quantity and price come from the `Acties` label.

The price is a **historical acquisition price restated to the transfer date**, that is, adjusted for any split between purchase and transfer. [IMP-SAXO-014] Two consequences:

* `quantity × price` is split-invariant, so it is the true acquisition cost and can be taken at face value
* splits occurring *after* the transfer are still to be applied, and appear as their own rows. There is no double counting

Import creates a complete buy: quantity and price from the label, acquisition date defaulted to the transfer date. [IMP-SAXO-015] The date stays editable, because it is the one figure that is not real [IMP-SAXO-016] (see Altbestand in `domain.md`).

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

Stock is pre-selected because all six position-marked rows in the sample were stock elections, and because choosing stock requires a share count and so cannot be accepted blindly, whereas choosing cash is a single keystroke that would silently discard an acquisition.

This is a heuristic calibrated on one account and one issuer. It is acceptable because its failure mode is safe: an understated position surfaces later as a blocked attribution with a named shortfall, not as a wrong number.

In the sample it isolates 6 rows out of 74 dividend-type rows. The 19 `Herbeleggingsdividend` rows were verified against declared dividends per share and are ordinary cash despite the label.

## Bonds

`Type` is `Bond`, which is what defaults the security's quotation to `percent_of_par`. [IMP-SAXO-020] Confirmed against the sample: a 7.5% bond with `Deponering 3000 @ 139.46` paid coupons of exactly 225.00 (7.5% of 3000) and redeemed for exactly 3000.00, so 3000 is a nominal amount and the cost is 4183.80, not 418,380.

## Securities

`Instrument ISIN`, `Instrument` (name) and `Type` (`Stock`, `Bond`, `Etf`, `MutualFund`, `Cash`) map onto the security. [IMP-SAXO-021] Names carry delisting annotations such as `*Delisted 20231011 (TransAlta Renewables Inc.)` and change over time for one ISIN; the ISIN is the key. [IMP-SAXO-022]

# Trade Republic DE

## File format

CSV, quoted, comma-delimited, dot decimals, ASCII, single header row, 23 columns. [IMP-TR-001] The header is byte-identical across the 2022 to 2025 exports.

* `datetime` is full ISO-8601 UTC with sub-second precision; `date` is the effective date and can differ from it [IMP-TR-002]
* `transaction_id` is a UUID: a stable broker reference, used directly as the identity [IMP-TR-003]
* Sign convention is cash flow, so buys and fees are negative [IMP-TR-004]

## Money

Native columns are `price`, `amount`, `fee`, `tax`, `currency`. [IMP-TR-005] Where the trade was in another currency, `original_amount`, `original_currency` and `fx_rate` are populated. [IMP-TR-006] `fee` and `tax` are summed into the domain's single `fees` field. [IMP-TR-007]

## Row classification

`category` is `TRADING`, `CASH` or `CORPORATE_ACTION`; `type` refines it. Classification: [IMP-TR-008]

| `category` / `type` | Handling |
| --- | --- |
| `TRADING` / `BUY`, `SELL` | derived automatically |
| `CORPORATE_ACTION` / `TAX_EXCHANGE` | lot transfer; quantities are stated, so nothing is missing |
| `CORPORATE_ACTION` / anything else | the import is rejected [IMP-TR-010] |
| `CASH` / `DIVIDEND`, `INTEREST_PAYMENT`, `CUSTOMER_INBOUND`, `TRANSFER_INBOUND`, `STOCKPERK` | recognized as non-position, not stored |
| anything else naming a security | the import is rejected [IMP-TR-013] |
| anything else naming no security | not stored, but counted and named in the import summary [IMP-TR-014] |

Trade Republic states share quantities on corporate actions, so the Saxo dividend heuristic is neither needed nor applicable here. [IMP-TR-009] The sample's `TAX_EXCHANGE` pair moves −60.00 of one ISIN and +60.00 of another with both quantities present.

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
