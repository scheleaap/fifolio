# Goal

An application that helps to calculate gains and losses for securities for German income tax declarations.

# Background

In Germany, the FIFO principle is applied when calculating gains and losses. The principle is applied on a per-account basis ("Depot").

German brokers apply the principle and calculate taxes for their customers. Brokers outside Germany don't, requiring one to do it themselves.

# Scope

In scope: determining, per account, which buys a sale consumes under FIFO, and the resulting raw gain or loss in EUR.

Explicit non-goals (v1). The reports produce raw gain/loss figures; applying tax law to them is the user's job. [DOM-001]

* Teilfreistellung for funds and ETFs
* Separate loss pots (Verlustverrechnungstöpfe) for stocks vs. other capital income
* Vorabpauschale, Sparer-Pauschbetrag, Kapitalertragsteuer, Solidaritätszuschlag, church tax
* Gains on foreign currency cash balances (potentially a separate private disposal under § 23 EStG)
* Income that is not a disposal: dividends, interest, deposits, withdrawals, account fees. Such rows are recognized at import and deliberately not stored [DOM-002]

Corporate actions are **in scope**. Examination of real broker exports showed they dominate the data rather than sitting at its edges: splits, cash mergers, share-class exchanges, bond redemptions and tender offers all occur, and ignoring them makes holdings and cost bases wrong.

# Entities

The fields listed for an entity are a suggested, non-exhaustive list.

Account
* Fields: broker name, account id
* One account is one Depot. Where a broker splits a Depot into per-currency sub-accounts, the importer normalizes them onto the single account, because FIFO applies per Depot [DOM-003]

Security
* Fields: ISIN (natural key), name, type, quotation
* Type is a fixed enum: `stock`, `bond`, `etf`, `fund`, `derivative`, `other` [DOM-004]
* Quotation is a fixed enum: `per_unit` (default) or `percent_of_par` [DOM-005] (see Quotation)
* Securities may be created automatically during import; such records are flagged so the user can review and correct them [DOM-006]

Source record
* One parsed row of source data, stored verbatim alongside its parsed fields. [DOM-007] Never edited after creation [DOM-008]
* Kind is a fixed enum: [DOM-009]
    * `imported`: produced by reading a broker export
    * `manual`: information the user supplied because no export contains it
* Fields: kind, raw content, parsed fields, identity (see Identity), consumed or not
* Relations: belongs to: import batch (imported) or account (manual); consumed by: 0 or 1 transaction or corporate action

Transaction
* Type is a fixed enum: `buy` or `sell`. Nothing else is stored [DOM-010]
* Fields:
    * trade date, optional execution time, type, quantity, unit price, fees
    * currency, FX rate, FX rate source, FX rate date, EUR unit price, EUR fees (see Currency)
    * import sequence: a monotonic number assigned per account, used to break ordering ties [DOM-011]
* `fees` is the sum of all incidental costs: commission, exchange fees, and transaction taxes such as the French FTT or stamp duty. [DOM-012] They receive identical treatment in the gain calculation, so they are not modeled separately
* Relations: part of: account, relates to: security, derived from: >= 1 source records [DOM-013]

Corporate action
* An event that changes holdings without being a purchase or a sale
* Kind is a fixed enum: [DOM-014]
    * `quantity adjustment`: rescales the quantity of every open lot of one security by a ratio, leaving total cost unchanged. Splits and reverse splits
    * `lot transfer`: closes lots in one security and opens corresponding lots in another, carrying cost basis and acquisition dates. Share-class exchanges, ISIN changes, share-for-share mergers
* Events that pay cash for units are **not** corporate actions: a redemption, tender or cash merger is a sell, and shares issued as a stock dividend are a buy. [DOM-015] Both cite the corporate action rows they came from, so the audit trail survives [DOM-016]
* Fields: kind, effective date, ratio or lot mapping
* Relations: relates to: security (and, for a lot transfer, a target security), derived from: >= 1 source records

Import batch
* Records one import of one file into one account, so that an import can be undone as a unit [DOM-017]
* Fields: account, source filename, format, timestamp, counts (derived, pending, recognized as non-position, failed)
* Relations: has: >= 0 source records

Sale attribution
* Links a sell to one or more buys
* Relations: 1 sell transaction, >= 1 allocations
* An allocation records a buy transaction and a quantity, and nothing else. All money figures are derived [DOM-018] (see Deriving allocation figures)
* Requirements:
    * All transactions must belong to the same account and the same security [DOM-019]
    * Every allocated buy must be dated on or before the sell (in canonical order) [DOM-020]
    * The allocated quantities must sum exactly to the sell quantity [DOM-021]

# Identity

Import must be idempotent. [DOM-022] A source record's identity is the broker's own transaction reference where the format provides a stable one, and otherwise a hash of the parsed business fields. [DOM-023] Either is scoped to the account, so identical rows in different accounts stay distinct. [DOM-024]

Broker references are preferred because they survive re-exports that change formatting, and because two genuinely distinct trades can share date, security, quantity, price and fees.

# Currency

German income tax requires each leg of a trade to be valued in EUR at its own date: acquisition cost at the acquisition date, proceeds at the disposal date. [DOM-025] The currency movement between the two therefore falls inside the securities gain and is never reported separately. [DOM-026]

The relevant date is the trade date (the obligating transaction), not the settlement date. [DOM-027]

Every transaction stores both its native figures and its EUR figures, together with the rate used, so the EUR figures are reproducible and auditable rather than opaque. [DOM-028] The EUR figures mirror the native ones exactly: an EUR unit price and EUR fees, at the same scales. [DOM-029] Everything else, gross amounts and allocation shares alike, derives from that pair the same way it derives from the native pair.

Rate source, in order of preference: [DOM-030]

* `broker`: the source file states the EUR figures actually booked. They are used verbatim, because they are what was actually paid, and the stored rate is the implied quotient [DOM-031]
* `ecb`: the ECB daily euro reference rate for the trade date. Used whenever the file carries no EUR figure [DOM-032]
* `native`: the transaction is already denominated in EUR. Rate is 1 [DOM-033]

If the ECB published no rate for the trade date (weekend, holiday), the most recent published rate before that date is used, and the rate's own date is stored alongside it, so the substitution is visible. [DOM-034]

Fees are converted at the same rate as the leg they belong to. [DOM-035]

# Quotation

Most securities are quoted per unit, so the trade value is `quantity × unit_price`. Bonds are normally quoted as a percentage of par, where the quantity is a nominal amount and the price is a percentage. Treating the latter as the former overstates cost by a factor of 100.

The quotation is a property of the security, defaulted from the broker's instrument type when the security is auto-created [DOM-036] and editable afterwards. [DOM-037] It is kept separate from the type so that a mis-quoted instrument can be corrected without reclassifying what it is.

    trade value = quantity × unit_price × factor      factor = 1 or 0.01     [DOM-038]

Prices are stored exactly as the statement shows them, so a figure in the application can always be reconciled against a broker document. [DOM-039]

# Ordering

FIFO requires a total order over a security's transactions within an account. The canonical order is: [DOM-040]

1. trade date
2. execution time, where the source provides one; transactions without a time sort before those with one on the same date [DOM-041]
3. import sequence

# Processes

## Import and derivation

Import creates source records. Transactions and corporate actions are then derived from one or more of them. [DOM-042] The two are separate because a single event is often several rows: a cash merger can pay out over three bookings, and a tender offer can be a payment plus a reversal.

At import each source record is classified: [DOM-043]

* **Derived automatically.** Plain trades, where quantity, price and fees are all present and unambiguous, become buys and sells directly [DOM-044]
* **Pending.** The record affects holdings but the export does not contain everything needed. It waits for the user, who supplies the missing part. This is the completion queue [DOM-045]
* **Recognized as non-position.** Dividends, interest, deposits, withdrawals and account fees. Not stored; the import summary reports how many were recognized as such [DOM-046]

Nothing is invented. The user completes records that exist; there is no creation of a transaction from nothing. [DOM-047] Everything the user supplies becomes a `manual` source record, [DOM-048] so that hand-entered information is as traceable as imported information, and so that it can be exported and kept.

A security with any pending source record is blocked from attribution, because its holdings are known to be incomplete. [DOM-049]

## Corporate actions

A `quantity adjustment` rescales every open lot of a security by a ratio. Total cost per lot is unchanged, so cost per unit moves inversely. [DOM-050] Acquisition dates are untouched. [DOM-051]

A `lot transfer` closes lots in one security and opens corresponding lots in another, one new lot per old lot, **carrying the original acquisition dates and costs**. [DOM-052] This matches the treatment German law generally applies to a qualifying reorganization, where the new holding steps into the old one's place, and it keeps FIFO order meaningful across the exchange.

Where an event pays cash for part of a holding and exchanges the rest, it decomposes: a sell for the cash portion and a lot transfer for the remainder, both citing the same source records. [DOM-053]

## Sale attribution

This is where the FIFO principle is applied.

This is a manual approval process where the system shows a sell transaction and the proposed one or more buy transactions to attribute it to. The user only has the choice to approve or not to approve. [DOM-054] If the user does not approve, no newer sell transactions may be approved (per account, per security).

Declining stores nothing. [DOM-055] Because sells must be attributed in canonical order per account and security, an unattributed sell blocks every later sell for that account and security by construction. The block is lifted by approving that sell.

Given a sale transaction, the proposal consumes the oldest buys with unattributed quantity remaining, in canonical order, until the sold quantity is covered. [DOM-056] The final buy consumed is normally consumed only partially.

If the available unattributed buy quantity is less than the sold quantity, no proposal is produced. The shortfall is reported, naming the missing quantity. [DOM-057] This usually means historical buys have not been imported yet, or a corporate action that created units has not been completed. It is the system's safety net: a missed acquisition surfaces as a blocked attribution rather than as a wrong figure in a tax return.

## Deriving allocation figures

An allocation stores only a buy, a sell and a quantity. Everything monetary is computed from the parent transactions on demand, so that the figures can never drift away from their sources. [DOM-058]

For an allocation of quantity `q` against buy `b` and sell `s`, all in EUR, with `f` the security's quotation factor: [DOM-059]

* allocated cost = `b.unit_price × q × f`
* allocated buy fee = `b.fees × q / b.quantity`
* allocated proceeds = `s.unit_price × q × f`
* allocated sell fee = `s.fees × q / s.quantity`
* gain = allocated proceeds − allocated sell fee − allocated cost − allocated buy fee

Sell fees belong wholly to the sale that incurred them and are never spread onto lots that the sale did not touch. [DOM-060] Splitting a sale's fee across that sale's own allocations is a presentation of the same total, not a change of rule. Within a single sale every unit has the same unit price, so splitting by quantity and splitting by proceeds share are identical.

Rounding: each share is rounded to 2 decimals independently, and the resulting drift is absorbed by the last share, so that the shares always sum exactly to the parent figure. [DOM-061]

* buy-side shares (cost, buy fee): the last share is the allocation that exhausts the buy lot. [DOM-062] A lot that is not yet fully sold therefore carries its remainder in its unsold units, which satisfies the requirement that a buy's fees are completely distributed once all its units are sold
* sell-side shares (proceeds, sell fee): the last share is the last allocation of that sale in canonical order [DOM-063]

# Invariants

* The sum of allocated quantities against a buy must never exceed that buy's quantity [DOM-064]
* The allocations of a sale must sum exactly to that sale's quantity [DOM-065]
* Sells are attributed in canonical order per account and security: a sell may only be attributed if every earlier sell of the same account and security is already attributed [DOM-066]
* A security with any pending source record may not be attributed [DOM-067]
* An attribution may only be deleted if no later attribution exists for the same account and security [DOM-068]
* A transaction that participates in an attribution is immutable: it cannot be edited, re-rated or deleted while that attribution exists. [DOM-069] Delete the attribution first. This is what keeps derived allocation figures faithful to what the user approved
* A source record is consumed by at most one transaction or corporate action [DOM-070]
* A security's ISIN is unique [DOM-071]
* An import batch may only be deleted if none of the transactions derived from its source records participates in an attribution [DOM-072]

# Reports

Both reports accept an optional account filter and an optional tax year filter. Unfiltered means all accounts and all years. [DOM-073]

Income tax overview
* Gain/loss per year, per account. The year is the year of the sale [DOM-074]
* Columns: year, account, proceeds, sell fees, cost, buy fees, gain/loss [DOM-075]

Buy report
* Lists every buy with the sells it was attributed to, its remaining unsold quantity and its realized gain/loss [DOM-076]
* Buy columns: date, account, security, quantity, remaining quantity, unit price, fees, realized gain/loss [DOM-077]
* Per attributed sell: sell date, quantity consumed, allocated proceeds, allocated sell fee, allocated cost, allocated buy fee, gain/loss [DOM-078]
* The year filter selects buys that have at least one allocation in that year [DOM-079]

# Known limitations

* **Accrued interest.** A bond bought between coupon dates includes Stückzinsen paid to the seller, which under German law is not acquisition cost but negative investment income. This is not modeled
* **Averaged transferred lots.** A position transferred in from another broker usually arrives with a single weighted-average price, so several original purchases collapse into one lot with one acquisition date
* **Altbestand.** Securities acquired before 2009-01-01, and fund gains accrued to 2017-12-31 on units acquired before 2009, are grandfathered. Where a transferred lot's acquisition date defaults to the transfer date, that status is lost. The date is editable for this reason [DOM-080]
