# Goal

An application that helps to calculate gains and losses for securities for German income tax declarations.

# Background

In Germany, the FIFO principle is applied when calculating gains and losses. The principle is applied on a per-account basis ("Depot").

German brokers apply the principle and calculate taxes for their customers. Brokers outside Germany don't, requiring one to do it themselves. A German broker's account is still worth holding here, for a complete portfolio and as an independent check on figures the broker already reported.

# Scope

In scope: determining, per account, which acquisitions a disposal consumes under FIFO, and the resulting raw gain or loss in EUR.

Explicit non-goals (v1). The reports produce raw gain/loss figures; applying tax law to them is the user's job. [DOM-001]

* Teilfreistellung for funds and ETFs
* Separate loss pots (Verlustverrechnungstöpfe) for stocks vs. other capital income
* Vorabpauschale, Sparer-Pauschbetrag, Kapitalertragsteuer, Solidaritätszuschlag, church tax
* Gains on foreign currency cash balances (potentially a separate private disposal under § 23 EStG)
* Income that is not a disposal: **cash** dividends, interest, deposits, withdrawals, account fees. Such rows are recognized at import and deliberately not stored [DOM-002] A dividend taken in shares is the exception: it issues a parcel, so its rows are stored and its buy carries a stock-dividend origin [DOM-124]

Corporate actions are **in scope**. Examination of real broker exports showed they dominate the data rather than sitting at its edges: splits, cash mergers, share-class exchanges, bond redemptions and tender offers all occur, and ignoring them makes holdings and cost bases wrong. They are modeled as transaction variants rather than as a separate entity, because what a corporate action does to a holding is the same kind of thing a trade does.

# Entities

The fields listed for an entity are a suggested, non-exhaustive list.

Account
* Fields: broker name, account id
* One account is one Depot. Where a broker splits a Depot into per-currency sub-accounts, the importer normalizes them onto the single account, because FIFO applies per Depot [DOM-003]

Security
* Fields: ISIN (natural key), name, type, quotation, auto-created, needs review
* Type is a fixed enum: `stock`, `bond`, `etf`, `fund`, `derivative`, `other` [DOM-004]
* Quotation is a fixed enum: `per_unit` (default) or `percent_of_par` [DOM-005] (see Quotation)
* Securities may be created automatically during import; such records are flagged as auto-created. The flag records provenance only and never changes [DOM-006]
* A security auto-created during import also needs review, so the user can check and correct it. Needs review is separate from auto-created and is cleared only by the user marking the security reviewed; editing it does not clear it [DOM-126]

Source record
* One parsed row of a broker export, stored verbatim alongside its parsed fields. [DOM-007] Never edited after creation [DOM-008]
* A delimited format has a verbatim line and stores it. A spreadsheet row has none — it is typed cells — so it stores a **canonical rendering**: a JSON object mapping column name to cell value as a string, keys in sheet column order, serialized without insignificant whitespace. An Excel serial date stays `"45208"`; an empty cell is `""` and an absent one is omitted; the key is the header exactly as the file spells it, non-breaking spaces and leading spaces included. JSON is named rather than a delimited form because escaping, empties and absences then need no rules of our own. This is a rendering, not the row, and it is part of the on-disk format: a re-import must reproduce the string byte for byte [DOM-120]
* Fields: raw content, parsed fields, identity (see Identity), order (see Ordering), consumed or not
* Relations: belongs to: >= 1 import batches; consumed by: 0 or 1 transaction; cited by: >= 0 transactions

Manual entry
* Information the user supplied because no export contains it. The shapes are exactly those the completion queue asks for: a stock-or-cash election with a share count if stock; a ratio alone, for a split; a target security with a ratio, for an exchange; a quantity disposed with an optional target security, for a cash merger, tender or partial buyback [DOM-097]
* An acquisition date is **not** among them: it is fixed at import and never corrected by hand [DOM-122]
* Fields: account, security, what was supplied, and **the identities of the source records it answers** [DOM-098]
* It references those records by their broker identity rather than by an internal key, so it survives their deletion and reconnects when the same rows are imported again [DOM-099]
* Relations: belongs to: account; cited by: 0 or 1 transaction

The two are separate entities because their lifecycles differ and that difference must be structural rather than remembered. A source record is reproducible — import the file again. A manual entry is not: it is the only thing in the system that cannot be recovered from a broker file, which is why undoing an import can never destroy one [DOM-100]

Transaction
* Everything that happens to a holding. A sum type: each variant carries only the fields it has, so a variant can never be read as another [DOM-010]
* Common to every variant: trade date, and the `order` of the source record it consumes [DOM-011], and a relation to account, security and >= 1 source records [DOM-013]
* A transaction **consumes** the source records that are answered by its existence, and may **cite** further records without consuming them. Consumption is what clears a record from the completion queue; citation is what preserves the audit trail. A decomposition names which of its transactions consumes [DOM-101]
* Variants, grouped by what they do to holdings:

| Variant | Effect | Realizes a gain | Own fields |
| --- | --- | --- | --- |
| `buy` | opens a parcel | no | quantity, unit price, fees, EUR gross, origin |
| `transfer_in` | opens a parcel with a carried cost basis | no | quantity, cost basis, fees, acquisition date, date provenance, source |
| `sell` | closes parcels | yes | quantity, unit price, fees, EUR gross |
| `expiration` | closes the whole remaining position | yes | EUR gross, EUR fees |
| `transfer_out` | closes parcels, basis carries onward | **no** | quantity, target security, ratio, EUR fees; EUR gross derived |
| `split` | rescales every open parcel of one security | no | ratio, as an integer numerator and denominator |

* **Opening** variants are `buy` and `transfer_in`; **closing** variants are `sell`, `expiration` and `transfer_out`. `split` is neither [DOM-081]
* A `buy`'s origin records how it arose: an ordinary purchase, or shares issued as a stock dividend, whose cost basis is their taxable value at issue [DOM-082]
* For such a buy the taxable value **is** the EUR gross: one figure, not two that might disagree. The type stores it once, so that the rule that every calculation reads the gross cannot pick up a number other than the basis [DOM-123]
* A `transfer_in` covers both a transfer from another broker and units arriving from a corporate action. Its `source` says which, and its date provenance records what its acquisition date is worth: `transfer_date` when the export gave no real acquisition date, `inherited` when it was carried from the parcel it replaced. **Neither is editable** [DOM-083]
* `fees` is the sum of all incidental costs: commission, exchange fees, and transaction taxes such as the French FTT or stamp duty. [DOM-012] They receive identical treatment in the gain calculation, so they are not modeled separately
* A transaction cites the source records it was derived from, so a multi-row event keeps its audit trail [DOM-016]

Import batch
* Records one import of one file into one account, so that an import can be undone as a unit [DOM-017]
* Fields: account, source filename, format, timestamp, counts (derived, pending, recognized as non-position, failed)
* Relations: has: >= 0 source records

Attribution
* Links one closing transaction to one or more opening transactions [DOM-018]
* Relations: 1 closing transaction, >= 1 allocations
* An allocation records an opening transaction and a quantity, and nothing else. All money figures are derived (see Deriving allocation figures)
* Requirements:
    * All transactions must belong to the same account and the same security [DOM-019]
    * Every allocated opening must precede the closing in canonical order [DOM-020]
    * The allocated quantities must sum exactly to the closing quantity (see Invariants)

# Identity

Import must be idempotent. [DOM-022] A source record's identity is the broker's own transaction reference where the format provides a stable one, and otherwise a hash of the parsed business fields. [DOM-023] Either is scoped to the account, so identical rows in different accounts stay distinct. [DOM-024]

Broker references are preferred because they survive re-exports that change formatting, and because two genuinely distinct trades can share date, security, quantity, price and fees.

# Currency

German income tax requires each leg of a trade to be valued in EUR at its own date: acquisition cost at the acquisition date, proceeds at the disposal date. [DOM-025] The currency movement between the two therefore falls inside the securities gain and is never reported separately. [DOM-026]

The relevant date is the trade date (the obligating transaction), not the settlement date. [DOM-027]

Every transaction stores both its native figures and its EUR figures, together with the rate used, so the EUR figures are reproducible and auditable rather than opaque. [DOM-028] The EUR figures mirror the native ones: an EUR unit price and EUR fees, at the same scales. [DOM-029] Allocation shares derive from that pair the same way they derive from the native pair, and are never stored independently. [DOM-084]

**The EUR gross total is stored as well as the unit price, and it is the figure every calculation uses.** [DOM-085] Dividing a booked total by a quantity and multiplying back does not round-trip at fractional quantities, so allocation shares are taken pro-rata from the booked total and never rebuilt from the unit price; the drift rule already guarantees they sum back to it exactly. The unit price is kept for display and for reconciliation against the statement, and nothing computes with it. [DOM-104]

Rate source, in order of preference: [DOM-030]

* `broker`: the source file states the EUR figures actually booked. They are used verbatim, because they are what was actually paid. The stored rate is derived as each format specifies and is **informational only** — nothing computes with it, since every calculation reads the booked EUR total [DOM-031]
* `ecb`: the ECB daily euro reference rate for the trade date. Used whenever the file carries no EUR figure [DOM-032]
* `native`: the transaction is already denominated in EUR. Rate is 1 [DOM-033]

**The stored rate is foreign units per EUR**, the ECB convention: `EUR = native / rate`. [DOM-086] Brokers differ — Trade Republic and the ECB quote it this way, Saxo quotes its inverse — so an importer meeting the other convention inverts at full precision from the figures the file states, never from the rounded stored rate.

If the ECB published no rate for the trade date (weekend, holiday), the most recent published rate before that date is used, and the rate's own date is stored alongside it, so the substitution is visible. [DOM-034]

Fees are converted at the same rate as the leg they belong to. [DOM-035]

# Quotation

Most securities are quoted per unit, so the trade value is `quantity × unit_price`. Bonds are normally quoted as a percentage of par, where the quantity is a nominal amount and the price is a percentage. Treating the latter as the former overstates cost by a factor of 100.

The quotation is a property of the security, defaulted from the broker's instrument type when the security is auto-created [DOM-036] and editable afterwards. [DOM-037] It is kept separate from the type so that a mis-quoted instrument can be corrected without reclassifying what it is.

    trade value = quantity × unit_price × factor      factor = 1 or 0.01     [DOM-038]

**The factor is applied exactly once**, when a quoted price becomes a value. [DOM-087] A unit price derived by dividing a value by a quantity is already an effective price and the factor must not be applied to it again.

Prices are **derived from the booked amounts**, not copied from the figure a statement prints. The two agree to the cent in the ordinary case and diverge in the trailing decimals when the broker's display rounds: a sell booked at 1833.24 over 60 units is `30.654`, printed as `30.65`. Reconciliation against a broker document is therefore on the **booked amounts**, which are exact, and not on the printed unit price. [DOM-039]

# Ordering

FIFO requires a total order over a security's transactions within an account.

Order is established **when a file is read**, from the file's own content, and stored on each source record as an integer `order`. [DOM-040] It is computed by sorting the file's rows on, in turn:

1. the trade date
2. every further ordering column the format provides, in a stated precedence — a monotonic booking counter, an execution or booking timestamp. A column that is not monotonic with time is never an ordering column
3. the row's position in the file, normalized to the file's own direction, so a newest-first export does not order backwards

The third key is always available, so the order within a file is total and no import is ever refused for ambiguity. [DOM-102]

`order` is scoped to the file it was computed from. The canonical order over an account and security is therefore **(trade date, order, batch age)**: trade date first, `order` to separate rows of one date, and the age of the owning batch to settle the rare case of two records from different files sharing a date. [DOM-111] Because it is derived from file content alone, re-importing the same file reproduces it exactly, and the order no longer depends on which files were imported first.

Every variant takes part in this order, `split` included. [DOM-088]

# Processes

## Import and derivation

Import creates source records. Transactions are then derived from one or more of them. [DOM-042] The two are separate because a single real event is often several rows: a cash merger can pay out over three bookings, and a tender offer can be a payment plus a reversal.

At import each source record is classified: [DOM-043]

* **Derived automatically.** Rows where every field the variant needs is present and unambiguous [DOM-044]
* **Pending.** The record affects holdings but the export does not contain everything needed. It waits for the user, who supplies the missing part. This is the completion queue [DOM-045]
* **Recognized as non-position.** Cash dividends, interest, deposits, withdrawals and account fees. Not stored; the import summary reports how many were recognized as such [DOM-046] A dividend that issues shares is a position event and is stored [DOM-124]

Nothing is invented. The user completes records that exist; there is no creation of a transaction from nothing. [DOM-047] Everything the user supplies becomes a manual entry, [DOM-048] so that hand-entered information is as traceable as imported information, and so that it can be exported and kept.

A manual entry outlives the import it answered. Undoing an import removes its source records and the transactions derived from them, and leaves every manual entry standing. When the same rows are imported again the entry reconnects by their identities and its transaction is restored automatically, so an undo followed by a re-import returns the account exactly where it was. [DOM-108] An entry whose records are absent is listed as waiting, naming what it expects, so a completion that vanished from the queue has an explanation. [DOM-109]

A security with any pending source record is blocked from attribution **in the account that record belongs to**, because that account's holdings are known to be incomplete. Other accounts holding the same security are unaffected, since FIFO never crosses accounts. [DOM-049]

## Splits

A `split` rescales every parcel of one security that is open at its position in the canonical order, by its ratio. Total cost per parcel is unchanged, so cost per unit moves inversely. Acquisition dates are untouched.

The ratio is an **integer numerator and denominator**, and effective quantity is computed as an exact rational, rounded only for display. [DOM-113] A one-for-three reverse split has no finite decimal expansion, so rounding at each application would leave a residue that grows across successive splits and could stop a parcel exhausting exactly.

An opening transaction's **effective quantity as of a position in the canonical order** is its stated quantity multiplied by the ratios of every split for that security that falls between the opening and that position. [DOM-089] Its effective unit price at that position is its total cost divided by that effective quantity. The stated figures are never rewritten, so the transaction still reconciles against the statement.

The position matters. A disposal states its quantity in the units current when it happened, so a cost basis must be divided by the opening's effective quantity **as of that disposal**, not as of today. Buy 10 at 100, sell 5, then split 2:1: the basis of those 5 units is 500, and dividing by the post-split 20 would give 250 — halving the cost of everything sold before any split. [DOM-103]

## Transfers

A `transfer_out` closes parcels without realizing a gain, because the basis carries onward. It runs through the same attribution process as any other closing transaction: the user approves which parcels it consumes, and the system then emits **one `transfer_in` per consumed parcel**, each carrying that parcel's acquisition date and its share of the cost basis, with date provenance `inherited`. Each emitted record relates to the `transfer_out`'s source records and takes the `order` of the parcel it carries, so the parcels sort in the receiving account exactly as they sorted before the transfer (DEC-079). [DOM-090]

Each emitted record carries **that parcel's own allocated cost**, computed by the allocation formula, never a share of a pooled total, and that parcel's allocated buy fee as its own fees, kept separate from the cost (DEC-081). [DOM-106] Pooling and dividing by quantity would give every emitted parcel the same unit cost — two equal parcels bought at 100 and 200 would both emerge at 150 — destroying the per-parcel basis that one record per parcel exists to preserve. The total would be right and every individual figure wrong, surfacing only when those parcels are disposed of in different years.

Each emitted record's quantity is the consumed quantity multiplied by the transfer's ratio, at the quantity scale, with the last emitted record absorbing the rounding remainder so the emitted quantities sum exactly to the transferred quantity times the ratio, rounded once to the quantity scale, half away from zero, where the exact product is not representable (DEC-083). [DOM-115]

A transfer carrying a fee of its own rejects the import, naming the row, rather than choosing whether that fee is basis or fees. No transfer, exchange or merger in five years of exports carries one, so this blocks nothing; the treatment is decided when one first appears (DEC-080). [DOM-107]

One `transfer_in` per parcel rather than one per event is what preserves separate acquisition dates through an exchange, which German law requires of a qualifying reorganization.

Where an event pays cash for part of a holding and exchanges the rest, it decomposes: a `sell` for the cash portion and a `transfer_out` for the remainder, both citing the same source records. [DOM-091] The sell leg takes the group's summed cash net of any reversal, and **all** of the group's costs; the transfer leg carries no money of its own, only the basis it inherits. [DOM-116] The fee was charged on the payout, and attaching it to the sale deducts it now rather than deferring it to a disposal that may be years away.

## Attribution

This is where the FIFO principle is applied.

It is a manual approval process. The system shows a closing transaction and the proposed opening transactions to attribute it to. The user only has the choice to approve or not to approve. [DOM-054]

Declining stores nothing. [DOM-055] Because closings must be attributed in canonical order per account and security, an unattributed closing blocks every later closing for that account and security by construction. The block is lifted by approving it.

Given a closing transaction, the proposal consumes the oldest openings with unattributed effective quantity remaining, in canonical order, until the closed quantity is covered. [DOM-056] The final opening consumed is normally consumed only partially.

An `expiration` closes the whole remaining position, so its quantity is the unattributed remainder rather than a stated figure. [DOM-092] A row that would produce an expiration of a position with nothing remaining is not a transaction but a data problem: it stays pending rather than being derived, so the division by a zero quantity cannot arise. [DOM-114]

If the available unattributed quantity is less than the closed quantity, no proposal is produced. The shortfall is reported, naming the missing quantity. [DOM-057] This usually means historical acquisitions have not been imported yet, or a corporate action that created units has not been completed. It is the system's safety net: a missed acquisition surfaces as a blocked attribution rather than as a wrong figure in a tax return.

## Deriving allocation figures

An allocation stores only an opening, a closing and a quantity. Everything monetary is computed from the parent transactions on demand, so that the figures can never drift away from their sources. [DOM-058]

Every **closing** variant carries an `eur_gross` and an `eur_fees`, so one formula reads them all; `split` is never a closing and carries neither. [DOM-105] A `transfer_out`'s `eur_gross` is the basis it carries onward rather than proceeds: it is derived from its own allocations, not stored, so the opening side is computed first and its gain is never computed at all. [DOM-112]

For an allocation of quantity `q` against opening `o` and closing `c`, all in EUR: [DOM-059]

* allocated cost = `o.eur_gross × q / o.effective_quantity_at(c)`
* allocated buy fee = `o.eur_fees × q / o.effective_quantity_at(c)`
* allocated proceeds = `c.eur_gross × q / c.quantity`
* allocated sell fee = `c.eur_fees × q / c.quantity`
* gain = allocated proceeds − allocated sell fee − allocated cost − allocated buy fee

A `transfer_out` realizes no gain. Its allocations exist to record which parcels were consumed and to carry their basis into the emitted `transfer_in` records. [DOM-093]

Sell fees belong wholly to the disposal that incurred them and are never spread onto parcels that the disposal did not touch. [DOM-060] Splitting a disposal's fee across its own allocations is a presentation of the same total, not a change of rule. Within a single disposal every unit has the same unit price, so splitting by quantity and splitting by proceeds share are identical.

Rounding: each share is rounded to 2 decimals independently, and the resulting drift is absorbed by the last share, so that the shares always sum exactly to the parent figure. [DOM-061]

A gain is computed from the four **rounded** shares, not exactly and then rounded, and a report total is the sum of the rounded rows. Every row therefore reconciles to its own columns and every total to the rows above it, which is the first thing a reader checks. The cost is up to two cents against the exact figure per allocation; the alternative buys that back and loses the arithmetic a reader can follow. [DOM-125]

* opening-side shares (cost, buy fee): the last share is the allocation that exhausts the parcel. [DOM-062] A parcel not yet fully consumed therefore carries its remainder in its unsold units, which satisfies the requirement that fees are completely distributed once all units are disposed of
* closing-side shares (proceeds, sell fee): the last share is the last allocation of that closing in canonical order [DOM-063]

# Invariants

* The sum of allocated quantities against an opening, each scaled to a common position in the canonical order, must never exceed its effective quantity at that position. Comparing quantities stated at different positions compares different unit scales [DOM-064]
* The allocations of a closing must sum exactly to that closing's quantity [DOM-065]
* Closings are attributed in canonical order per account and security: a closing may only be attributed if every earlier closing of the same account and security is already attributed [DOM-066]
* A security with any pending source record may not be attributed in that account [DOM-067]
* An attribution may only be deleted if no later attribution exists for the same account and security [DOM-068]
* A transaction that participates in an attribution is immutable: it cannot be edited, re-rated or deleted while that attribution exists. [DOM-069] Delete the attribution first. This is what keeps derived allocation figures faithful to what the user approved
* A `transfer_in` emitted by a `transfer_out` may not be deleted independently of it [DOM-094]
* A source record is consumed by at most one transaction, though it may be cited by several [DOM-070]
* A manual entry is never deleted by an import undo [DOM-110]
* A security's ISIN is unique [DOM-071]
* An import batch may only be deleted if none of the transactions derived from its source records participates in an attribution [DOM-072]
* An import batch may only be deleted if none of the records it owns is cited by a transaction the batch did not derive; the deletion is refused naming those transactions [DOM-119]

# Reports

Both reports accept an optional account filter and an optional tax year filter. Unfiltered means all accounts and all years. [DOM-073]

Income tax overview
* Gain/loss per year, per account. The year is the year of the disposal [DOM-074]
* Only attributed disposals contribute. An unattributed disposal is reported as outstanding rather than counted at zero, so a year is never understated silently [DOM-117]
* Columns: year, account, proceeds, sell fees, cost, buy fees, gain/loss, and a count of outstanding disposals [DOM-075]
* The count is a column rather than a second block, so every output format keeps one record shape. A non-zero count means the year's figures are incomplete and the report says so in plain words wherever it has room for a sentence [DOM-121]
* A `transfer_out` realizes nothing and contributes no row [DOM-095]

Acquisition report
* Lists every opening transaction with the disposals it was attributed to, its remaining quantity and its realized gain/loss [DOM-076]
* Opening columns: date, account, security, effective quantity, remaining quantity, effective unit price, fees, realized gain/loss [DOM-077]
* Effective quantity and effective unit price are reported **as of today**, the latest position in canonical order, so the two quantity columns are in one scale and comparable to a broker statement [DOM-118]
* Per attributed disposal: date, quantity consumed, allocated proceeds, allocated sell fee, allocated cost, allocated buy fee, gain/loss [DOM-078]
* The year filter selects openings that have at least one allocation in that year [DOM-079]
* A `transfer_in` appears as an opening in its own right, and names the opening it inherited from, so a holding can be traced back to the purchase it descends from [DOM-096]

# Known limitations

* **Accrued interest.** A bond bought between coupon dates includes Stückzinsen paid to the seller, which under German law is not acquisition cost but negative investment income. This is not modeled
* **Averaged transferred parcels.** A position transferred in from another broker usually arrives with a single weighted-average price, so several original purchases collapse into one parcel with one acquisition date
* **Altbestand is lost on transferred parcels.** Securities acquired before 2009-01-01, and fund gains accrued to 2017-12-31 on units acquired before 2009, are grandfathered. A parcel transferred in from another broker takes the transfer date as its acquisition date, and that date is never corrected, so any grandfathered status it carried is not represented. [DOM-080] Several Saxo positions price to the mid-2000s and are likely affected
* **A backdated row in a re-issued year.** A source record's `order` is fixed when it is first read and never edited. If a broker re-issues a year with a row inserted before existing ones, that row's computed position can equal a stored one. Trade date still separates them unless they also share a date
* **A corporate action straddling a year boundary.** Imports are refused when a file covers more than one calendar year, so a group of rows belonging to one event is normally whole within a file. An event whose rows fall either side of 31 December is split across two imports, and the identity collision rule rejects the file if that produces a clash

# Retired identifiers

Never reused. Listed so that an older finding or test naming one stays interpretable.

* `DOM-014`, `DOM-015`, `DOM-050`, `DOM-051`, `DOM-052`, `DOM-053` — the separate corporate-action entity, replaced by transaction variants
* `DOM-021` — duplicated the invariant now carried by DOM-065 alone
* `DOM-009` — the imported/manual kind enum, replaced by two entities
* `DOM-041` — the execution-time ordering key, replaced by the format's own ordering columns
