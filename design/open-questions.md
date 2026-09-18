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

**OQ-002 — An emitted `transfer_in` has no source record.**
Blocks: `DOM-090`, `DOM-013`, `DOM-047`, `SRV-030`, `SRV-034`
Records emitted on approval are created by the system, not derived from a row, so they have no
`order`, no stated trade date, and no defined position among their siblings — which decides which
of them a later disposal consumes first.

**OQ-003 — Whether a transfer carries the buy fee as well as the cost.**
Blocks: `DOM-106`, `DOM-107`, `DOM-112`
Each emitted record carries its parcel's allocated cost. The allocation also produces an allocated
buy fee, and nothing says whether that travels with it. If it does not, acquisition costs already
incurred are never deducted from any gain, because a transfer realizes nothing.

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

**OQ-006 — A Trade Republic bond.**
Blocks: `IMP-TR-020`, `IMP-TR-021`
One requirement rejects the import; another creates the security pending review. Both are
normative, and the format carries no value that maps to `bond` anyway, so the trigger is undefined.

**OQ-007 — Ownership moves under re-import, but order and deletion depend on it.**
Blocks: `DOM-111`, `SRV-052`, `SRV-021`
Canonical order uses batch age, and batch deletion removes the records a batch owns. Re-importing a
year transfers ownership to the newer batch, so both the tie-break and what a deletion removes
change without any record being edited.

**OQ-008 — A Saxo `Acties` value outside the table.**
Blocks: `IMP-SAXO-013`
The classification table lists the values observed. Nothing says what an unlisted one does — reject
the file, as an unknown Trade Republic type does, or something else.

**OQ-009 — The direction of the corporate-action ordinal.**
Blocks: `IMP-SAXO-008`, `IMP-SAXO-025`
Identity uses a row's ordinal within its group "in file order", while the file is newest-first and
is normalized for ordering. Whether the ordinal counts in file order or normalized order is
unstated, and it changes the identity.

**OQ-010 — `quantity × unit_price × factor` is defined but never invoked.**
Blocks: `ARC-008`, `DOM-038`, `DOM-104`
Trade value has a formula, and every calculation is confined to the booked EUR total. Nothing says
where the formula is used, or whether it is only a reconciliation check.

**OQ-011 — A `transfer_out` row in the acquisition report.**
Blocks: `DOM-078`, `DOM-112`
The per-disposal columns are proceeds, sell fee, cost, buy fee and gain. A transfer realizes no gain
and has no proceeds, and nothing says what those columns hold.

**OQ-012 — What a failed row does to its import.**
Blocks: `SRV-017`
Rows that fail to parse are counted in the summary. Whether the import proceeds with the rest, or
fails as a whole, is unstated.
