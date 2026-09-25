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

**OQ-001 — A transaction's `order` when it consumes no record, or several.**
Blocks: `DOM-011`, `DOM-101`, `DOM-091`
Every variant takes the `order` of the record it consumes, and a decomposition produces two
transactions from one group of rows. Which record each consumes, and what orders the two legs
relative to each other, is unstated.

**OQ-004 — Exact-rational quantities against 8-decimal allocations.**
Blocks: `DOM-064`, `DOM-065`, `DOM-113`
Effective quantity is an exact rational so that a one-for-three split leaves no residue, but
allocations are decimals required to sum exactly to the closing quantity. After such a split no sum
of decimals equals the remaining quantity, so a parcel can never be exactly exhausted and the drift
rule that depends on exhaustion never fires.

**OQ-005 — When an expiration of nothing is detected.**
Blocks: `DOM-092`, `DOM-114`, `IMP-SAXO-013`
The rule says such a row stays pending, but it is stated as an import-time check, and at import the
remaining position is not yet known — it depends on attributions not yet approved.

**OQ-007 — Ownership moves under re-import, but order and deletion depend on it.**
Blocks: `DOM-111`, `SRV-052`, `SRV-021`
Canonical order uses batch age, and batch deletion removes the records a batch owns. Re-importing a
year transfers ownership to the newer batch, so both the tie-break and what a deletion removes
change without any record being edited.

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

**OQ-011 — A `transfer_out` row in the acquisition report.**
Blocks: `DOM-078`, `DOM-112`
The per-disposal columns are proceeds, sell fee, cost, buy fee and gain. A transfer realizes no gain
and has no proceeds, and nothing says what those columns hold.

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
