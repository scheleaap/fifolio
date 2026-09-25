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

There is a live instance, so this is not hypothetical. The 2025 export carries one
`Overige Corporate Action` row — Saxo's own junk drawer — on Eco Wave Power Global AB, 5 December
2025: quantity `-1.03`, cash `-1.20 USD`, and `Totale kosten` `-1.03`. The share change and the
cost are the same number, so the row is either a small fee or a write-off of shares booked as a
cost, and those attribute differently. It needs an answer from the broker, not from the
specification. Roughly one euro is at stake.

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

**OQ-015 — Verifying the account id against a Trade Republic file.**
Blocks: `IMP-003`, `SRV-056`
An import is required to check that the file belongs to the account it is being imported into. The
Trade Republic export carries no account identifier of any kind, so the check has nothing to read
and cannot run. Either the requirement admits formats that cannot be checked, or Trade Republic
files are matched some other way.

**OQ-016 — Whether a transfer's own fee is basis or fees.**
Blocks: `DOM-106`, `DOM-112`
A transfer can itself cost money. Whether that fee joins the cost basis travelling to the new parcel,
or is recorded as fees on the emitted record, is unstated, and the two produce different gains on the
eventual disposal. Related to OQ-003, which asks the same of the *allocated* buy fee rather than the
transfer's own.

**OQ-017 — A transfer's fee share when the basis total is zero.**
Blocks: `DOM-107`
The fee a transfer itself costs is divided across the emitted records in proportion to each one's
basis. A parcel whose basis is zero, or a set of them summing to zero, makes that division
undefined. A stock dividend whose taxable value was zero, or a fully written-down holding, produces
exactly that.

**OQ-018 — A transfer's emitted quantities must sum to a product that may not exist.**
Blocks: `DOM-115`
The emitted quantities must sum exactly to the transferred quantity times the ratio. With the ratio
now an exact integer pair the product is exact, but it need not be representable at the 8-decimal
quantity scale, and nothing says what the sum targets when it is not.

**OQ-019 — Who fetches a rate an import needs and does not have.**
Blocks: `ARC-019`
The error names the currency and date when a rate is "neither cached nor fetchable", which assumes
an import can fetch. No requirement assigns that path: seeding and top-up are separate acts, and
whether an import reaches the network at all is unstated. It decides whether an import can fail for
want of a network connection.

**OQ-020 — Withholding tax is in the export and in no requirement.**
Blocks: nothing yet
`Bookings` states a `Tax Percentage` (15 on the Dutch dividends) and isolates the withheld amount as
a `Corporate actions - Voorheffing` component. Foreign withholding is creditable against German tax,
so this is a figure with a use, and 70 of the 242 booking rows carry one. Nothing in `domain.md`
models a tax withheld, and DEC-001 puts tax treatment out of scope while the reports exist to feed a
tax return. Whether the withheld amount is stored and reported, or deliberately ignored, is a
scoping decision rather than a gap.

**OQ-021 — Trade Republic changed the `fx_rate` convention in late 2024.**
Blocks: `IMP-TR-006`, `IMP-TR-016`, `DOM-086`
`importers.md` states `fx_rate` is foreign units per EUR, so a native figure divides by it. That
holds for every foreign row up to 2024-07-02 and stops holding after it. The rate series across the
four exports, all USD dividends:

    2022-07-01 .. 2024-07-02   1.0451  0.97862  1.0640  1.0890  1.0905  1.05171  1.1128  1.08326  1.0710
    2024-10-01 .. 2025-10-01   0.893176  0.962557  0.924642  0.853242  0.851716

The later values are the reciprocals of the market USD-per-EUR rates for those dates: on 2024-10-01
the euro bought about 1.1196 dollars, and 1 / 0.893176 = 1.1196. Two rows settle it from the file
alone, without appealing to any external rate, because their native amount is large enough to
discriminate: 2025-07-01 states `original_amount 0.04 USD`, `fx_rate 0.853242` and `amount 0.03`.
Dividing gives 0.047, which rounds to 0.05. Multiplying gives 0.034, which rounds to 0.03. The
stated amount is 0.03. The 2025-10-01 row behaves identically. Every earlier row states 0.03 against
a rate near 1, where both readings round to the same figure and so decide nothing.

Nothing is wrong today: every foreign row in the sample is a dividend, and dividends are not stored.
But a foreign-currency row valued after the change at the documented convention is wrong by the
square of the rate — about 30% on USD — which is exactly the error `money.rs` documents itself as
guarding against.

What a person must decide is not the fact but the response. Keying on the date is fragile if the
change was gradual or account-specific. Keying on whether the rate is above or below 1 fails for a
currency near parity. Refusing a foreign row until the convention is confirmed is safe and blocks
nothing today. Ignoring the stated rate and taking the ECB rate for the date is the most robust and
contradicts DEC-003's preference for a broker-stated figure.

