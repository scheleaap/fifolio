# Fixtures

Anonymized broker exports, derived from the real files in `design/example_exports/` and committed
here because those are personal financial records and are gitignored [TST-011].

    fixtures/saxo-nl/Transactions_10000000_<from>_<to>.xlsx   5 files, 188 rows, 2021 to 2025
    fixtures/trade-republic/transactions_<from>_<to>.csv      4 files,  65 rows, 2022 to 2025

## What they are for

**Parsing, classification and idempotency. Never arithmetic.** [TST-014]

Anonymization perturbs every amount, so nothing in a fixture reconciles with anything else in it:
`Aantal`, `Boekingsbedrag` and `Totale kosten` no longer add up, and a label's price is not the
price its columns derive. A test that asserts a figure derived from a fixture is asserting a
number the anonymizer invented.

The money rules are tested separately, with synthetic inputs whose expected results are derivable
by hand [TST-015]. The worked examples in `design/importers.md` are where a real figure belongs.

## What is replaced

Every identity, each one consistently, so that what two rows share they still share [TST-012]:

| In the export | In the fixture |
| --- | --- |
| `Klant-id` | `10000000` |
| `Rekening-ID` base, per-currency suffix kept | `40100/9000001` + `EUR`/`USD`/`CAD` |
| `Transactie-ID`, ` Positie-ID`, `Corporate action-Id`, `Bk Record Id`, `Booking Id` | generated ids of the same digit width, in the same ascending order |
| `transaction_id` and the ids inside a description | generated UUIDs |
| `Instrument ISIN`, `symbol` | `XF`-prefixed ISINs with a correct check digit |
| `Instrument`, `name` | `Fixture Instrument nn`, any `*Delisted 20231011 (...)` annotation kept |
| `Instrumentsymbool` | `FXTnn`, any `:exchange` suffix kept |
| personal names, `IBAN`, `counterparty_iban` | `Fixture Owner nn`, generated German IBANs |
| every monetary column and conversion rate | moved by up to ten percent |

Dates, quantities, currencies, the `Acties` and `category`/`type`/`asset_class` labels, the row
order and which columns are blank are **kept**: they are what the importers are specified against
[TST-028]. Free text that names a security is rebuilt from the row's label and its replaced ISIN
rather than stripped, because the name it carries appears in no column [TST-029].

The id replacements keep the ascending order of the originals, because Saxo's ordering key rests
on those counters ascending with date [IMP-SAXO-026]. Zero stays zero and a conversion rate of
exactly 1 stays 1, because a `Deponering` row is recognized by both [IMP-SAXO-014], [IMP-SAXO-017].

## Regenerating

    cargo run --package anonymize-exports -- --source design/example_exports --out fixtures

Runs against the real exports, which only the account holder has. It is deterministic: the same
exports produce the same fixtures, byte for byte, so a rerun that changes nothing shows no diff.
The script embeds no real value — every replacement comes from the rank of the original among the
values of its kind — and it refuses to write a file in which any collected original still appears.

A new export year is folded in by dropping it into `design/example_exports/` and running the
script again. Ranks are assigned over the whole corpus, so a new file renumbers the pseudonyms of
the old ones; that is a fixture change to review, not a mistake.

`tools/anonymize-exports/tests/fixture_structure.rs` is the standing statement of what these files
must carry [TST-013]. A regenerated fixture that lost a property fails there.

## Known divergences from a real export

Three, all deliberate and all recorded here rather than discovered later:

* **Blank cells.** Saxo writes an empty cell as a zero-length shared string, which `calamine`
  reports as `Data::String("")`; the fixture writer cannot emit one, so the same cell reads as
  `Data::Empty`. An importer must treat both as blank — a fixture cannot prove it does, so the
  Saxo reader owes that assertion its own test against the two variants.
* **`Overige Corporate Action`.** The sample carries this `Acties` value and the classification
  table in `design/importers.md` does not name it. The fixture keeps the row rather than dropping
  it, so the unknown-label refusal has a real row to fire on. The table needs to either classify
  it or state that it is unknown.
* **Trade Republic 2025 row order.** `design/importers.md` describes it as sorted by neither
  `date` nor `datetime`; it is in fact in `date` order, with `datetime` descending inside a date.
  The property the ordering rule needs — file position alone orders the ties wrongly — holds
  either way, and the fixture reproduces the real order exactly.
