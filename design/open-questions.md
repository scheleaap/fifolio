# Open questions

Points the specification does not determine, each naming the requirements it blocks.

This file is the **authoritative list of what is undecided**. The planner reads it and marks every
item covering a blocked requirement as `blocked`; nothing else decides that. It is maintained by
hand, deliberately: an earlier design had the auditor regenerate the list each run, and on an
unchanged specification two consecutive runs disagreed about most of it, so the plan churned and
items were split on a boundary that moved. A list that only changes when a person changes it is
stable by construction.

`spec-auditor` still reports ambiguities it finds. A finding becomes an entry here only when a
person puts it here.

Closing a question means answering it, writing the answer into the requirement documents and into
`decisions.md`, and deleting the entry.

---

**OQ-005 — When an expiration of nothing is detected.**
Blocks: `DOM-092`, `DOM-114`, `IMP-SAXO-013`
The rule says such a row stays pending, but it is stated as an import-time check, and at import the
remaining position is not yet known — it depends on attributions not yet approved.

**OQ-008 — A Saxo `Acties` value outside the table.**
Blocks: `IMP-SAXO-013`
The classification table lists the values observed. Nothing says what an unlisted one does — reject
the file, as an unknown Trade Republic type does, or something else.

There is a live instance, so this is not hypothetical. The 2025 export carries one
`Overige Corporate Action` row — Saxo's own junk drawer — on Eco Wave Power Global AB, 5 December
2025: quantity `-1.03`, cash `-1.20 USD`, and `Totale kosten` `-1.03`. The share change and the
cost are the same number, so the row is either a small fee or a write-off of shares booked as a
cost, and those attribute differently. It needs an answer from the broker, not from the
specification. Roughly one euro is at stake.

**OQ-010 — `quantity × unit_price × factor` is defined but never invoked.**
Blocks: `ARC-008`, `DOM-038`, `DOM-104`
Trade value has a formula, and every calculation is confined to the booked EUR total. Nothing says
where the formula is used, or whether it is only a reconciliation check.

**OQ-013 — Where a row with no ordering column sorts.**
Blocks: `IMP-SAXO-026`
An ordering column may be absent from a row, and Saxo's is: its counter is the first populated of
three booking ids, so the counter-less rows are precisely the corporate actions. A fixed choice is
forced — falling through to the next key when either side is absent is not transitive, since `5`
and `3` would each tie with a missing value while differing from each other — but which fixed
choice is not determined. Absent-first places those rows before every counter-carrying row of their
date; absent-last places them after; the alternative is to let file position stand for a missing
value, which orders them where the export puts them.

The mechanism in `ordering.rs` is unaffected either way and currently sorts absent first. What is
blocked is the Saxo importer's binding of it.

**OQ-014 — A stock dividend's cost basis.** *(answered; kept until the rule is written into
`domain.md` and the entry deleted)* `Bookings` carries the taxable value as a
`Corporate Actions - Share Amount` component and `_Transacties` carries the share count as the
`Gekocht` leg's quantity, so neither is a manual entry. The 19 `Herbeleggingsdividend` events issue
no shares at all. See IMP-SAXO-042 and IMP-SAXO-043.
Blocks: `IMP-SAXO-013`
A `Herbeleggingsdividend` row carries a cash amount and no share count: `0.43 EUR` of ING, `0.12` of
KPN. The shares it bought are in no column of the export, and Saxo's separate corporate-action
report does not carry them either — its `Gekozen aantal` is the position the election was made on,
not the shares received. So both the quantity and the price of the acquired parcel are absent, and
the specification says nothing about where either comes from. This is the **largest** block of
manual work in the sample: 19 events across four years, against 12 for every other corporate action
combined. Whether the user supplies the share count per event, or the amount is treated as cash and
the shares picked up elsewhere, decides how much of the tool's use is data entry.

---

## Provisionally answered, awaiting ratification

Answered on a best-effort basis on 2026-09-29 so the server could be built without halting. They
block nothing any more; the user ratifies or overrides each, and an override reopens the items
built on it.

* **OQ-001** (order of a multi-record transaction and of decomposition legs) → DEC-090
* **OQ-004** (exact rationals against 8-decimal allocations) → DEC-091
* **OQ-007** (ownership moving under re-import) → DEC-092
* **OQ-011** (a `transfer_out` in the acquisition report) → DEC-093
* **FIF-076** (a transaction's `order` from records of several files; batch age) → DEC-094, DEC-095
* **FIF-076** (whether undo-then-re-import restores batch age as well) → DEC-096
* **FIF-061** (whether "between" an opening and a position includes either end) → DEC-097
* **FIF-013** (a closing of nothing; a parcel already over-allocated) → DEC-098
* **FIF-013** (remaining quantity: rounded exact difference, or difference of rounded sides) → DEC-099
* **FIF-014** (closing-side divisor: stated quantity, or its view at the quantity scale) → DEC-100
* **FIF-014** (whether a last share may be negative, or a running total pass the parent) → DEC-101
* **FIF-015** (approving an `expiration`, whose quantity is undecided) → DEC-102
* **FIF-015** (allocations of zero or less; one opening allocated twice in one attribution) → DEC-103
* **FIF-015** (which layer holds DOM-054's "proposal written unchanged") → DEC-104
* **FIF-063** (an emitted record's trade date, conversion, placement; reaching back) → DEC-105
* **FIF-063** (an emitted record of zero or fewer units) → DEC-106
* **FIF-031** (whether a year-filtered row's gain is that year's or the opening's whole) → DEC-107
* **FIF-031** (what an emitted record names once its parent opening is deleted) → DEC-108
* **FIF-082** (the units a disposal line's quantity consumed is shown in) → DEC-109
* **FIF-035** (which rows supply auto-created securities; one ISIN of two types) → DEC-110
* **FIF-035** (whether posting a file again creates a batch) → DEC-111
* **FIF-085** (whether a batch stores the auto-created count too) → DEC-112
* **FIF-070** (how a refusal on several grounds carries each ground to the caller) → DEC-113
* **FIF-090** (testing the account refusal while no served format states an account) → DEC-114
* **FIF-036** (what deleting a batch answers while batch removal is not built) → DEC-115
