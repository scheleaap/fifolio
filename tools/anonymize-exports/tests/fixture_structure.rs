//! Integration layer [TST-003]: the committed fixtures, read as the importers will read them.
//!
//! These tests are the standing statement of what a fixture is for [TST-011]. Every property
//! asserted here is one an importer is specified against [TST-013], so a regenerated fixture that
//! lost one fails the suite instead of quietly weakening the importer tests that will be written
//! against it. Nothing here asserts an amount: the amounts are perturbed and the fixtures never
//! test arithmetic [TST-014].
//!
//! They read the committed `fixtures/` only. The real exports are gitignored and absent in CI, so
//! nothing here may reach for them.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anonymize_exports::saxo::{self, Cell, SheetKind};
use anonymize_exports::trade_republic as tr;
use calamine::{Reader, Xlsx, open_workbook};
use chrono::{Datelike as _, Days, NaiveDate};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;
use uuid::Uuid;

fn fixtures() -> PathBuf {
    // `tools/anonymize-exports` up two levels is the workspace root.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the manifest directory is two levels below the workspace root")
        .join("fixtures")
}

fn files_in(directory: &str, extension: &str) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = fs::read_dir(fixtures().join(directory))
        .expect("the fixture directory is committed")
        .map(|entry| entry.expect("a readable directory entry").path())
        .filter(|path| path.extension().is_some_and(|found| found == extension))
        .collect();
    paths.sort();
    assert!(
        !paths.is_empty(),
        "{directory} holds no {extension} fixture"
    );
    paths
}

fn saxo_exports() -> Vec<(PathBuf, saxo::Export)> {
    files_in("saxo-nl", "xlsx")
        .into_iter()
        .map(|path| {
            let export = saxo::read(&path).expect("a fixture Saxo export reads");
            (path, export)
        })
        .collect()
}

/// The `Transacties` sheet of every fixture, which the cash-ledger properties are asserted over.
fn saxo_sheets() -> Vec<(PathBuf, saxo::Sheet)> {
    saxo_exports()
        .into_iter()
        .map(|(path, export)| (path, export.sheet(SheetKind::Transacties).clone()))
        .collect()
}

fn saxo_field(row: &[Cell], header: &str) -> String {
    field(SheetKind::Transacties, row, header)
}

fn field(kind: SheetKind, row: &[Cell], header: &str) -> String {
    saxo::field(kind, row, header).as_text()
}

/// The rows of `sheet` carrying each non-empty value of `header`, by that value: the join index
/// both Saxo joins are built on [IMP-SAXO-037].
fn index_of(sheet: &saxo::Sheet, header: &str) -> BTreeMap<String, Vec<usize>> {
    sheet.rows.iter().enumerate().fold(
        BTreeMap::new(),
        |mut indexed: BTreeMap<String, Vec<usize>>, (index, row)| {
            let key = field(sheet.kind, row, header);
            if !key.is_empty() {
                indexed.entry(key).or_default().push(index);
            }
            indexed
        },
    )
}

/// The `_Transacties` rows a `Transacties` row joins: on `Transactie-ID`, else on
/// `Corporate action-Id` [IMP-SAXO-037].
fn detail_counterparts(export: &saxo::Export, row: &[Cell]) -> Vec<usize> {
    let detail = export.sheet(SheetKind::Detail);
    ["Transactie-ID", "Corporate action-Id"]
        .iter()
        .find_map(|header| {
            index_of(detail, header)
                .get(&saxo_field(row, header))
                .cloned()
        })
        .unwrap_or_default()
}

/// The `Bookings` rows a `Transacties` row joins: on `Bk Record Id`, then `Booking Id`, else on
/// `Corporate action-Id` [IMP-SAXO-037].
fn booking_components(export: &saxo::Export, row: &[Cell]) -> Vec<usize> {
    let bookings = export.sheet(SheetKind::Bookings);
    [
        "Bk\u{a0}Record\u{a0}Id",
        "Booking\u{a0}Id",
        "Corporate action-Id",
    ]
    .iter()
    .find_map(|header| {
        index_of(bookings, header)
            .get(&saxo_field(row, header))
            .cloned()
    })
    .unwrap_or_default()
}

fn decimal_of(value: &str) -> Decimal {
    value.parse::<Decimal>().expect("a numeric column")
}

/// The date a serial number stands for, against the epoch Excel counts from.
fn date_of(serial: &str) -> NaiveDate {
    let days = serial
        .parse::<Decimal>()
        .expect("a date column holds a serial number")
        .trunc()
        .to_u64()
        .expect("a serial after the epoch");
    NaiveDate::from_ymd_opt(1899, 12, 30)
        .expect("the Excel epoch")
        .checked_add_days(Days::new(days))
        .expect("a serial within the calendar")
}

/// A real XLSX container with **all three sheets**, each with its own header row byte for byte
/// and its own row count — the non-breaking spaces and the leading space included
/// [IMP-SAXO-001], [IMP-SAXO-002], [TST-013], [TST-031].
#[test]
fn saxo_fixtures_are_xlsx_with_the_three_sheets_and_their_headers() {
    let mut counted = [0_usize; 3];
    for (path, export) in saxo_exports() {
        // `saxo::read` refuses a header row that is not the sheet's own, so it having parsed is
        // the assertion; this states the sheets and their widths for the reader.
        let workbook: Xlsx<_> = open_workbook(&path).expect("a real XLSX container");
        assert_eq!(
            workbook.sheet_names(),
            vec!["Transacties", "_Transacties", "Bookings"],
            "{} does not carry the three sheets",
            path.display()
        );
        for (index, kind) in SheetKind::ALL.into_iter().enumerate() {
            let sheet = export.sheet(kind);
            assert!(
                !sheet.rows.is_empty(),
                "{} has no {} rows",
                path.display(),
                kind.name()
            );
            for row in &sheet.rows {
                assert_eq!(
                    row.len(),
                    kind.headers().len(),
                    "{} has a short row on {}",
                    path.display(),
                    kind.name()
                );
            }
            counted[index] += sheet.rows.len();
        }
    }
    // The sample's row counts, which `importers.md` states [IMP-SAXO-001]. A corpus that grew is
    // a rerun of the anonymizer and a revision of that table, not a fixture edited by hand.
    assert_eq!(counted, [188, 33, 242]);
}

/// Dates are Excel serial numbers, not text, on every sheet that holds one [IMP-SAXO-003],
/// [TST-013], [TST-031]. `Bookings` also carries `Ex-datum` and `Boekdatum`, which the export
/// writes as text and the fixture keeps as text.
#[test]
fn saxo_dates_are_excel_serial_numbers() {
    for (path, export) in saxo_exports() {
        for (kind, headers) in [
            (
                SheetKind::Transacties,
                &["Transactiedatum", "Valutadatum"][..],
            ),
            (
                SheetKind::Detail,
                &["Aangepaste transactiedatum", "Uitvoeringsdatum transactie"],
            ),
            (SheetKind::Bookings, &["Boekingsdatum"]),
        ] {
            for row in &export.sheet(kind).rows {
                for header in headers {
                    assert!(
                        matches!(saxo::field(kind, row, header), Cell::Number(_)),
                        "{} holds {header} on {} as something other than a serial number",
                        path.display(),
                        kind.name()
                    );
                }
            }
        }
        for row in &export.sheet(SheetKind::Bookings).rows {
            for header in ["Ex-datum", "Boekdatum"] {
                assert!(
                    matches!(
                        saxo::field(SheetKind::Bookings, row, header),
                        Cell::Text(_) | Cell::Empty
                    ),
                    "{} holds {header} as something other than the text the export writes",
                    path.display()
                );
            }
        }
    }
}

/// Rows are emitted newest first [IMP-SAXO-025], [TST-013].
#[test]
fn saxo_rows_run_newest_first() {
    for (path, sheet) in saxo_sheets() {
        let dates: Vec<NaiveDate> = sheet
            .rows
            .iter()
            .map(|row| date_of(&saxo_field(row, "Transactiedatum")))
            .collect();
        let mut descending = dates.clone();
        descending.sort_by(|left, right| right.cmp(left));
        assert_eq!(dates, descending, "{} is not newest first", path.display());
    }
}

/// Every file is confined to a single calendar year of trade dates [IMP-001], [IMP-002].
#[test]
fn a_saxo_fixture_covers_one_calendar_year() {
    for (path, sheet) in saxo_sheets() {
        let years: BTreeSet<i32> = sheet
            .rows
            .iter()
            .map(|row| date_of(&saxo_field(row, "Transactiedatum")).year())
            .collect();
        assert_eq!(years.len(), 1, "{} spans {years:?}", path.display());
    }
}

/// The per-currency `Rekening-ID` suffixes sit on one base account [IMP-SAXO-005], [TST-013].
#[test]
fn saxo_accounts_are_one_depot_in_three_currencies() {
    let accounts: BTreeSet<String> = saxo_sheets()
        .iter()
        .flat_map(|(_, sheet)| sheet.rows.iter().map(|row| saxo_field(row, "Rekening-ID")))
        .collect();
    let bases: BTreeSet<&str> = accounts
        .iter()
        .map(|account| &account[..account.len() - 3])
        .collect();
    assert_eq!(bases.len(), 1, "{accounts:?} are not one Depot");
    for suffix in ["EUR", "USD", "CAD"] {
        assert!(
            accounts.iter().any(|account| account.ends_with(suffix)),
            "no {suffix} sub-account in {accounts:?}"
        );
    }
}

/// At least one row of every `Acties` value the exports carry [TST-013].
///
/// The list is the one `importers.md` classifies [IMP-SAXO-013], plus `Overige Corporate Action`,
/// which the sample carries and that table does not name — the fixture keeps it so that the
/// unknown-label refusal has a real row to fire on.
#[test]
fn every_observed_acties_value_appears() {
    let labels: Vec<String> = saxo_sheets()
        .iter()
        .flat_map(|(_, sheet)| sheet.rows.iter().map(|row| saxo_field(row, "Acties")))
        .collect();
    for expected in [
        "Koop",
        "Verkoop",
        "Deponering",
        "Expiratie",
        "Fusie",
        "Terugkoopaanbod",
        "Terugboeking",
        "Stock split",
        "Omwisseling",
        "Dividend",
        "Keuzedividend",
        "Herbeleggingsdividend",
        "Rente",
        "Service fee",
        "ADR-kosten",
        "Storting",
        "Opname",
        "Overige Corporate Action",
    ] {
        assert!(
            labels.iter().any(
                |label| label.starts_with(expected) || label.contains(&format!("- {expected}"))
            ),
            "no row carries {expected}"
        );
    }
}

/// The free-text labels keep their quantity, their direction and a 2-decimal price
/// [IMP-SAXO-011], [IMP-SAXO-012], [TST-013].
#[test]
fn a_trade_label_states_quantity_direction_and_a_two_decimal_price() {
    let labels: Vec<String> = saxo_sheets()
        .iter()
        .flat_map(|(_, sheet)| sheet.rows.iter().map(|row| saxo_field(row, "Acties")))
        .filter(|label| label.contains(" @ "))
        .collect();
    assert!(
        labels.iter().any(|label| label.starts_with("Koop ")),
        "no buy label"
    );
    assert!(
        labels.iter().any(|label| label.starts_with("Verkoop -")),
        "no sell label with a negative quantity"
    );
    assert!(
        labels.iter().any(|label| label.starts_with("Deponering ")),
        "no transfer-in label"
    );
    for label in &labels {
        let price = label
            .rsplit(" @ ")
            .next()
            .and_then(|tail| tail.split(' ').next())
            .expect("a label states a price");
        assert_eq!(
            price.split('.').next_back().expect("a decimal part").len(),
            2,
            "{label} does not carry a 2-decimal price"
        );
    }
}

/// A corporate action whose rows share a `Corporate action-Id`, where only one carries a
/// `Positie-ID` and another carries the money: the Philips shape the dividend grouping rule
/// exists for [IMP-SAXO-018], [IMP-SAXO-019], [TST-013].
#[test]
fn a_dividend_group_splits_the_position_marker_from_the_money() {
    let found = saxo_sheets().iter().any(|(_, sheet)| {
        groups_by_corporate_action(sheet).values().any(|rows| {
            let marked = rows
                .iter()
                .filter(|row| !saxo_field(row, " Positie-ID").is_empty())
                .count();
            let with_money = rows.iter().any(|row| {
                saxo_field(row, " Positie-ID").is_empty()
                    && saxo_field(row, "Aantal")
                        .parse::<Decimal>()
                        .is_ok_and(|amount| !amount.is_zero())
            });
            rows.len() >= 2 && marked == 1 && with_money
        })
    });
    assert!(found, "no fixture carries the Philips shape");
}

/// A three-row group under one `Corporate action-Id`: the TransAlta shape the within-group
/// ordinal of the identity rule exists for [IMP-SAXO-008], [TST-013].
#[test]
fn a_corporate_action_pays_over_three_rows() {
    let widest = saxo_sheets()
        .iter()
        .flat_map(|(_, sheet)| {
            groups_by_corporate_action(sheet)
                .into_values()
                .map(|rows| rows.len())
                .collect::<Vec<_>>()
        })
        .max()
        .unwrap_or_default();
    assert!(widest >= 3, "the widest corporate action has {widest} rows");
}

/// At least one reversal row [TST-013]. `Terugboeking` is what the merger and tender rules net
/// their cash against [IMP-SAXO-031].
#[test]
fn a_reversal_row_is_present() {
    let found = saxo_sheets().iter().any(|(_, sheet)| {
        sheet
            .rows
            .iter()
            .any(|row| saxo_field(row, "Acties").contains("Terugboeking"))
    });
    assert!(found, "no reversal row");
}

/// At least one `Bond`, which is what defaults a quotation to percent of par [IMP-SAXO-020],
/// [TST-013].
#[test]
fn a_bond_instrument_is_present() {
    let types: BTreeSet<String> = saxo_sheets()
        .iter()
        .flat_map(|(_, sheet)| sheet.rows.iter().map(|row| saxo_field(row, "Type")))
        .collect();
    assert!(types.contains("Bond"), "the instrument types are {types:?}");
    // The other mapped types are what the security mapping is tested against [IMP-SAXO-021].
    for expected in ["Stock", "Etf", "MutualFund", "Cash"] {
        assert!(types.contains(expected), "no {expected} instrument");
    }
}

/// The identities are replaced, on all three sheets [TST-012], [TST-031]. What a fixture cannot
/// prove is what a value used to be, so this asserts the other half: every identifying column
/// carries a generated value.
#[test]
fn saxo_identities_are_generated_ones() {
    for (path, export) in saxo_exports() {
        for sheet in &export.sheets {
            for row in &sheet.rows {
                let where_it_is = format!("{} on {}", path.display(), sheet.kind.name());
                assert!(
                    field(sheet.kind, row, "Rekening-ID").starts_with("40100/"),
                    "{where_it_is} carries an unreplaced account"
                );
                let isin = field(sheet.kind, row, "Instrument ISIN");
                assert!(
                    isin.is_empty() || isin.starts_with("XF"),
                    "{where_it_is} carries an unreplaced ISIN {isin}"
                );
                // A symbol keeps its `:exchange` suffix, which identifies a market and nobody.
                let symbol = field(sheet.kind, row, "Instrumentsymbool");
                assert!(
                    symbol.is_empty() || symbol.starts_with("FXT"),
                    "{where_it_is} carries an unreplaced symbol {symbol}"
                );
                let name = field(sheet.kind, row, "Instrument");
                assert!(
                    name.is_empty()
                        || name.starts_with("Fixture Instrument")
                        || name.starts_with("*Delisted"),
                    "{where_it_is} carries an unreplaced instrument name {name}"
                );
            }
        }
        // `Order-ID` is the one id the two new sheets add. `0` is Saxo's "no order" marker and
        // stays; anything else is a generated order number.
        for row in &export.sheet(SheetKind::Detail).rows {
            let order = field(SheetKind::Detail, row, "Order-ID");
            assert!(
                order == "0" || order.starts_with('6'),
                "{} carries an unreplaced order id {order}",
                path.display()
            );
        }
    }
    // The account holder is named on `Transacties` alone.
    for (path, sheet) in saxo_sheets() {
        for row in &sheet.rows {
            let owner = saxo_field(row, "Naam IBAN-eigenaar");
            assert!(
                owner.is_empty() || owner.starts_with("Fixture Owner"),
                "{} carries an unreplaced name",
                path.display()
            );
        }
    }
}

/// A delisting annotation survives anonymization, so the security mapping can be tested against a
/// name that changed for one ISIN [IMP-SAXO-022], [TST-013].
#[test]
fn a_delisting_annotation_is_present() {
    let found = saxo_sheets().iter().any(|(_, sheet)| {
        sheet
            .rows
            .iter()
            .any(|row| saxo_field(row, "Instrument").starts_with("*Delisted "))
    });
    assert!(found, "no delisting-annotated instrument name");
}

/// Every position-affecting row resolves a `_Transacties` counterpart, and no `_Transacties` row
/// is left unclaimed: the join an importer is specified against survives anonymization, a row
/// that joined before joining the same counterpart after [IMP-SAXO-037], [TST-031].
#[test]
fn every_position_affecting_row_has_a_detail_counterpart() {
    let mut joined = 0_usize;
    let mut labels: Vec<String> = Vec::new();
    for (path, export) in saxo_exports() {
        let detail = export.sheet(SheetKind::Detail);
        let mut claimed: BTreeSet<usize> = BTreeSet::new();
        for row in &export.sheet(SheetKind::Transacties).rows {
            let counterparts = detail_counterparts(&export, row);
            if counterparts.is_empty() {
                continue;
            }
            joined += 1;
            labels.push(saxo_field(row, "Acties"));
            claimed.extend(counterparts);
        }
        assert_eq!(
            claimed.len(),
            detail.rows.len(),
            "{} leaves a _Transacties row joined to nothing",
            path.display()
        );
    }
    // The sample's 32 position-affecting rows, which `importers.md` counts [IMP-SAXO-037].
    assert_eq!(joined, 32, "the joined rows are {labels:?}");
    let family = |name: &str| {
        labels
            .iter()
            .filter(|label| label.starts_with(name))
            .count()
    };
    assert_eq!(family("Deponering"), 13);
    assert_eq!(family("Stock split"), 2);
    assert_eq!(family("Fusie"), 3);
    assert_eq!(family("Keuzedividend"), 3);
    // The tender and the `Terugboeking` that reverses it.
    assert_eq!(family("Terugkoopaanbod"), 2);
    for single in ["Koop ", "Verkoop ", "Omwisseling", "Expiratie"] {
        assert_eq!(
            family(single),
            1,
            "no {single} row joins a _Transacties row"
        );
    }
}

/// A price met on two sheets is the same price after anonymization [IMP-SAXO-011], [TST-031]:
/// the label's price moves on its own magnitude and equal magnitudes move equally, so a joined
/// row states one price and not two. The labels themselves need not match — a reversal leg joins
/// its tender under one `Corporate action-Id` and says so — but a price does.
#[test]
fn a_label_price_reads_the_same_on_both_sheets_of_a_joined_row() {
    let price_in = |label: &str| {
        label
            .split_once(" @ ")
            .map(|(_, tail)| tail.to_owned())
            .filter(|price| !price.is_empty())
    };
    let mut compared = 0_usize;
    for (path, export) in saxo_exports() {
        let detail = export.sheet(SheetKind::Detail);
        for row in &export.sheet(SheetKind::Transacties).rows {
            let Some(price) = price_in(&saxo_field(row, "Acties")) else {
                continue;
            };
            for counterpart in detail_counterparts(&export, row) {
                let Some(counterpart_price) = price_in(&field(
                    SheetKind::Detail,
                    &detail.rows[counterpart],
                    "Acties",
                )) else {
                    continue;
                };
                compared += 1;
                assert_eq!(
                    counterpart_price,
                    price,
                    "{} states one row's price two ways",
                    path.display()
                );
            }
        }
    }
    // The sample's joined rows that carry a price on both sheets.
    assert_eq!(compared, 15, "the joined prices are not the sample's");
}

/// Every `Bookings` row joins the booking it decomposes, and the join keys are the ones
/// `importers.md` names [IMP-SAXO-037], [TST-031].
#[test]
fn every_bookings_row_joins_the_booking_it_decomposes() {
    let mut joined = 0_usize;
    for (path, export) in saxo_exports() {
        let bookings = export.sheet(SheetKind::Bookings);
        let mut claimed: BTreeSet<usize> = BTreeSet::new();
        for row in &export.sheet(SheetKind::Transacties).rows {
            let components = booking_components(&export, row);
            if components.is_empty() {
                continue;
            }
            joined += 1;
            claimed.extend(components);
        }
        assert_eq!(
            claimed.len(),
            bookings.rows.len(),
            "{} leaves a Bookings row joined to nothing",
            path.display()
        );
    }
    // The sample's 171 bookings that decompose; the remaining 17 rows are position-only.
    assert_eq!(joined, 171);
}

/// The sample's `Transacties` rows whose joined `Bookings` components sum to neither the row's
/// `Boekingsbedrag` nor its `Aantal`: a `Bookings` group the export does not state as a
/// decomposition of the row it joins.
const SAMPLE_UNDECOMPOSED_BOOKINGS: usize = 21;

/// A booking's components still sum to the booking after anonymization: the amounts moved
/// together rather than each on its own magnitude [TST-031].
///
/// Which figure they sum to is the export's business and not this fixture's: a booking decomposes
/// either in the currency `Boekingsbedrag` is stated in or in the EUR `Aantal`. A group that sums
/// to neither is one the export itself does not decompose, and its components are moved each on
/// its own magnitude, so it is counted as its own class rather than passed over: a decomposition
/// that stopped adding up shows here as one booking moving out of its class and into that one.
#[test]
fn the_components_of_a_booking_still_sum_to_it() {
    let mut to_booked = 0_usize;
    let mut to_euro = 0_usize;
    let mut to_neither: Vec<String> = Vec::new();
    for (path, export) in saxo_exports() {
        let bookings = export.sheet(SheetKind::Bookings);
        for row in &export.sheet(SheetKind::Transacties).rows {
            let total: Decimal = booking_components(&export, row)
                .iter()
                .map(|index| {
                    decimal_of(&field(
                        SheetKind::Bookings,
                        &bookings.rows[*index],
                        "Boekingsbedrag",
                    ))
                })
                .sum();
            if total.is_zero() {
                continue;
            }
            if total == decimal_of(&saxo_field(row, "Boekingsbedrag")) {
                to_booked += 1;
            } else if total == decimal_of(&saxo_field(row, "Aantal")) {
                to_euro += 1;
            } else {
                to_neither.push(format!(
                    "{} transaction {} booking {} sums to {total}",
                    path.display(),
                    saxo_field(row, "Transactie-ID"),
                    saxo_field(row, "Bk\u{a0}Record\u{a0}Id"),
                ));
            }
        }
    }
    assert_eq!(
        (to_booked, to_euro),
        (100, 50),
        "a booking's components no longer sum to it"
    );
    assert_eq!(
        to_neither.len(),
        SAMPLE_UNDECOMPOSED_BOOKINGS,
        "the bookings summing to neither figure are {to_neither:?}"
    );
}

/// `Verhandelde waarde` is still the traded quantity at the traded price, the quantity never
/// having moved [TST-028] and the value having moved with the price [TST-031].
///
/// It is not exactly the product: the price is rounded to two decimals [IMP-SAXO-039] and a bond
/// quotes in percent of par [IMP-SAXO-020], so the value is within a percent of the product or of
/// a hundredth of it. An independently perturbed value would be up to a fifth out.
#[test]
fn a_traded_value_is_still_its_quantity_at_its_price() {
    let mut checked = 0_usize;
    for (path, export) in saxo_exports() {
        for row in &export.sheet(SheetKind::Detail).rows {
            let quantity = decimal_of(&field(SheetKind::Detail, row, "Traded\u{a0}Quantity"));
            let price = decimal_of(&field(SheetKind::Detail, row, "Prijs"));
            let value = decimal_of(&field(SheetKind::Detail, row, "Verhandelde waarde"));
            if quantity.is_zero() || price.is_zero() {
                continue;
            }
            checked += 1;
            let product = (quantity * price).abs();
            let ratio = value.abs() / product;
            let per_unit = (ratio - Decimal::ONE).abs() < Decimal::new(1, 2);
            let percent_of_par = (ratio - Decimal::new(1, 2)).abs() < Decimal::new(1, 4);
            assert!(
                per_unit || percent_of_par,
                "{} states {value} for {quantity} at {price}",
                path.display()
            );
        }
    }
    assert_eq!(checked, 32, "the sample's priced _Transacties rows");
}

/// A corporate action's two legs are two `_Transacties` rows under one `Corporate action-Id`,
/// distinguished by `Trade Event Type` [IMP-SAXO-037], [TST-013], [TST-031].
#[test]
fn a_corporate_action_carries_both_its_legs_under_one_id() {
    let found = saxo_exports().iter().any(|(_, export)| {
        let detail = export.sheet(SheetKind::Detail);
        index_of(detail, "Corporate action-Id")
            .values()
            .any(|rows| {
                let events: BTreeSet<String> = rows
                    .iter()
                    .map(|index| {
                        field(
                            SheetKind::Detail,
                            &detail.rows[*index],
                            "Trade\u{a0}Event\u{a0}Type",
                        )
                    })
                    .collect();
                rows.len() >= 2 && events.contains("Gekocht") && events.contains("Verkocht")
            })
    });
    assert!(found, "no corporate action with a bought and a sold leg");
}

/// A withholding percentage on a `Bookings` row, which is the tax figure no other sheet holds
/// [IMP-SAXO-042], [TST-031]. It is a rate and not an amount, so it is not perturbed [TST-028].
#[test]
fn a_bookings_row_carries_a_withholding_percentage() {
    let rates: BTreeSet<String> = saxo_exports()
        .iter()
        .flat_map(|(_, export)| {
            export
                .sheet(SheetKind::Bookings)
                .rows
                .iter()
                .filter(|row| {
                    field(SheetKind::Bookings, row, "Amount Type").contains("Voorheffing")
                })
                .map(|row| field(SheetKind::Bookings, row, "Tax\u{a0}Percentage"))
                .collect::<Vec<_>>()
        })
        .filter(|rate| !rate.is_empty())
        .collect();
    assert!(!rates.is_empty(), "no withholding percentage");
    for rate in &rates {
        let percentage = decimal_of(rate);
        assert!(
            percentage > Decimal::ZERO && percentage <= Decimal::ONE_HUNDRED,
            "{rate} is not a percentage"
        );
        assert_eq!(percentage.fract(), Decimal::ZERO, "{rate} was perturbed");
    }
}

fn groups_by_corporate_action(sheet: &saxo::Sheet) -> BTreeMap<String, Vec<&Vec<Cell>>> {
    sheet
        .rows
        .iter()
        .filter(|row| !saxo_field(row, "Corporate action-Id").is_empty())
        .fold(BTreeMap::new(), |mut grouped, row| {
            grouped
                .entry(saxo_field(row, "Corporate action-Id"))
                .or_default()
                .push(row);
            grouped
        })
}

fn trade_republic_files() -> Vec<(PathBuf, tr::Rows)> {
    files_in("trade-republic", "csv")
        .into_iter()
        .map(|path| {
            let rows = tr::read(&path).expect("a fixture Trade Republic export reads");
            (path, rows)
        })
        .collect()
}

fn tr_field<'row>(row: &'row [String], header: &str) -> &'row str {
    let index = tr::HEADERS
        .iter()
        .position(|candidate| *candidate == header)
        .expect("a named header");
    &row[index]
}

/// Quoted CSV with all 23 columns [IMP-TR-001], [TST-013].
#[test]
fn trade_republic_fixtures_are_quoted_csv_with_23_columns() {
    for (path, rows) in trade_republic_files() {
        // `tr::read` refuses a header row that is not the 23, so it having parsed is half of it.
        assert_eq!(tr::HEADERS.len(), 23);
        assert!(!rows.0.is_empty(), "{} has no rows", path.display());
        for row in &rows.0 {
            assert_eq!(row.len(), 23, "{} has a short row", path.display());
        }
        let text = fs::read_to_string(&path).expect("a readable fixture");
        for line in text.lines() {
            assert!(
                line.starts_with('"') && line.ends_with('"'),
                "{} has an unquoted field: {line}",
                path.display()
            );
        }
        assert!(
            text.lines().next().expect("a header line").starts_with(
                "\"datetime\",\"date\",\"account_type\",\"category\",\"type\",\"asset_class\""
            ),
            "{} does not start with the export's own header",
            path.display()
        );
    }
}

/// `datetime` is an ISO-8601 UTC timestamp with sub-second precision and is its own field, not
/// `date` with a time on it [IMP-TR-002], [IMP-TR-015], [TST-013].
#[test]
fn a_datetime_carries_sub_second_precision_and_diverges_from_the_date() {
    let rows: Vec<Vec<String>> = trade_republic_files()
        .into_iter()
        .flat_map(|(_, rows)| rows.0)
        .collect();
    for row in &rows {
        let stamp = tr_field(row, "datetime");
        assert!(stamp.ends_with('Z'), "{stamp} is not UTC");
        assert!(stamp.contains('.'), "{stamp} has no sub-second part");
    }
    assert!(
        rows.iter()
            .any(|row| !tr_field(row, "datetime").starts_with(tr_field(row, "date"))),
        "no row where the booking day differs from the effective date"
    );
}

/// `transaction_id` is a UUID, which is the identity [IMP-TR-003], [TST-013].
#[test]
fn every_transaction_id_is_a_uuid() {
    let ids: Vec<String> = trade_republic_files()
        .into_iter()
        .flat_map(|(_, rows)| rows.0)
        .map(|row| tr_field(&row, "transaction_id").to_owned())
        .collect();
    for id in &ids {
        assert!(Uuid::parse_str(id).is_ok(), "{id} is not a UUID");
    }
    assert_eq!(
        ids.iter().collect::<BTreeSet<_>>().len(),
        ids.len(),
        "two rows share a transaction id"
    );
}

/// The cash-flow sign convention survives perturbation: buys and fees stay negative
/// [IMP-TR-004], [TST-013].
#[test]
fn negative_cash_flows_are_present() {
    let found = trade_republic_files().into_iter().any(|(_, rows)| {
        rows.0.iter().any(|row| {
            tr_field(row, "category") == "TRADING" && tr_field(row, "amount").starts_with('-')
        })
    });
    assert!(found, "no negative trade amount");
}

/// `original_amount`, `original_currency` and `fx_rate` populated on a non-trade row
/// [IMP-TR-006], [TST-013].
#[test]
fn a_non_trade_row_carries_the_original_currency_triple() {
    let found = trade_republic_files().into_iter().any(|(_, rows)| {
        rows.0.iter().any(|row| {
            tr_field(row, "category") != "TRADING"
                && !tr_field(row, "original_amount").is_empty()
                && !tr_field(row, "original_currency").is_empty()
                && !tr_field(row, "fx_rate").is_empty()
        })
    });
    assert!(found, "no non-trade row carries the original_* triple");
}

/// The `TAX_EXCHANGE` pair: two rows, one effective date, equal absolute quantities and opposite
/// signs, which is the pairing key [IMP-TR-018], [IMP-TR-019], [TST-013].
#[test]
fn the_tax_exchange_pair_is_present() {
    let pair: Vec<Vec<String>> = trade_republic_files()
        .into_iter()
        .flat_map(|(_, rows)| rows.0)
        .filter(|row| tr_field(row, "type") == "TAX_EXCHANGE")
        .collect();
    assert_eq!(pair.len(), 2, "a TAX_EXCHANGE is a pair");
    assert_eq!(tr_field(&pair[0], "date"), tr_field(&pair[1], "date"));
    let quantities: Vec<Decimal> = pair
        .iter()
        .map(|row| {
            tr_field(row, "shares")
                .parse::<Decimal>()
                .expect("a quantity")
        })
        .collect();
    assert_eq!(quantities[0].abs(), quantities[1].abs());
    assert!(
        quantities[0].is_sign_negative() != quantities[1].is_sign_negative(),
        "the pair does not carry opposite signs"
    );
    assert_ne!(
        tr_field(&pair[0], "symbol"),
        tr_field(&pair[1], "symbol"),
        "the pair names one security"
    );
}

/// A `STOCKPERK` credit with the `TRADING`/`BUY` it is booked against [IMP-TR-011], [TST-013].
#[test]
fn the_stockperk_credit_and_its_paired_buy_are_present() {
    let rows: Vec<Vec<String>> = trade_republic_files()
        .into_iter()
        .flat_map(|(_, rows)| rows.0)
        .collect();
    let credits: Vec<&Vec<String>> = rows
        .iter()
        .filter(|row| tr_field(row, "type") == "STOCKPERK")
        .collect();
    assert!(!credits.is_empty(), "no STOCKPERK credit");
    for credit in credits {
        assert!(
            rows.iter().any(|row| {
                tr_field(row, "category") == "TRADING"
                    && tr_field(row, "type") == "BUY"
                    && tr_field(row, "date") == tr_field(credit, "date")
                    && tr_field(row, "symbol") == tr_field(credit, "symbol")
            }),
            "a STOCKPERK credit has no buy of the same security on its date"
        );
    }
}

/// Every classification the mapping names is exercised [IMP-TR-008], [TST-013].
#[test]
fn every_observed_category_and_type_appears() {
    let met: BTreeSet<(String, String)> = trade_republic_files()
        .into_iter()
        .flat_map(|(_, rows)| rows.0)
        .map(|row| {
            (
                tr_field(&row, "category").to_owned(),
                tr_field(&row, "type").to_owned(),
            )
        })
        .collect();
    for expected in [
        ("TRADING", "BUY"),
        ("CORPORATE_ACTION", "TAX_EXCHANGE"),
        ("CASH", "DIVIDEND"),
        ("CASH", "INTEREST_PAYMENT"),
        ("CASH", "CUSTOMER_INBOUND"),
        ("CASH", "TRANSFER_INBOUND"),
        ("CASH", "STOCKPERK"),
    ] {
        assert!(
            met.contains(&(expected.0.to_owned(), expected.1.to_owned())),
            "no {expected:?} row"
        );
    }
}

/// A file whose row order is not the `datetime` order, so that file position alone orders it
/// wrongly: the 2025 shape [IMP-TR-023], [TST-013].
#[test]
fn a_fixture_is_not_in_datetime_order() {
    let found = trade_republic_files().into_iter().any(|(_, rows)| {
        let stamps: Vec<&str> = rows.0.iter().map(|row| tr_field(row, "datetime")).collect();
        let mut ascending = stamps.clone();
        ascending.sort_unstable();
        stamps != ascending
    });
    assert!(found, "every fixture is already in datetime order");
}

/// Each Trade Republic export is one calendar year [IMP-001], and its rows are one account's.
#[test]
fn a_trade_republic_fixture_covers_one_calendar_year() {
    for (path, rows) in trade_republic_files() {
        let years: BTreeSet<&str> = rows
            .0
            .iter()
            .map(|row| &tr_field(row, "date")[..4])
            .collect();
        assert_eq!(years.len(), 1, "{} spans {years:?}", path.display());
    }
}

/// The identities are replaced [TST-012], the other half of the Saxo assertion above.
#[test]
fn trade_republic_identities_are_generated_ones() {
    for (path, rows) in trade_republic_files() {
        for row in &rows.0 {
            let symbol = tr_field(row, "symbol");
            assert!(
                symbol.is_empty() || symbol.starts_with("XF"),
                "{} carries an unreplaced ISIN {symbol}",
                path.display()
            );
            let name = tr_field(row, "name");
            assert!(
                name.is_empty()
                    || name.starts_with("Fixture Instrument")
                    || name.starts_with("Fixture Owner"),
                "{} carries an unreplaced name {name}",
                path.display()
            );
            let iban = tr_field(row, "counterparty_iban");
            assert!(
                iban.is_empty() || iban[4..].starts_with("5001051700"),
                "{} carries an unexpected IBAN",
                path.display()
            );
        }
    }
}
