//! Integration layer [TST-003]: the committed Saxo fixtures, read by the Saxo reader.
//!
//! The unit tests in `import::saxo` state each rule against a workbook built for it; these state
//! that the rules hold on the files an import will actually meet [TST-011], [TST-013], [TST-031].
//! Nothing here asserts an amount: the fixtures' amounts are perturbed [TST-014].

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{Datelike as _, NaiveDate};
use fifolio_core::import::saxo::{SaxoWorkbook, Sheet, date, field};

/// Every committed Saxo fixture, with the year its filename names.
fn exports() -> Vec<(PathBuf, SaxoWorkbook)> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate directory is two levels below the workspace root")
        .join("fixtures/saxo-nl");
    let mut paths: Vec<PathBuf> = fs::read_dir(directory)
        .expect("the fixture directory is committed")
        .map(|entry| entry.expect("a readable directory entry").path())
        .filter(|path| path.extension().is_some_and(|found| found == "xlsx"))
        .collect();
    paths.sort();
    assert_eq!(paths.len(), 5, "the five committed Saxo fixtures");

    paths
        .into_iter()
        .map(|path| {
            let content = fs::read(&path).expect("a fixture is readable");
            let export = SaxoWorkbook::read(&content)
                .unwrap_or_else(|error| panic!("{} does not read: {error}", path.display()));
            (path, export)
        })
        .collect()
}

/// The `Acties` label of a `Transacties` row.
fn action(export: &SaxoWorkbook, index: usize) -> &str {
    field(&export.sheet(Sheet::Transacties)[index], "Acties").expect("Transacties carries Acties")
}

/// All three sheets are read, each with its one header row and its own width
/// [IMP-SAXO-001], [IMP-SAXO-002].
#[test]
fn the_three_sheets_of_every_fixture_are_read() {
    let mut counted = [0_usize; 3];
    for (path, export) in exports() {
        for (index, sheet) in Sheet::ALL.into_iter().enumerate() {
            let rows = export.sheet(sheet);
            assert!(
                !rows.is_empty(),
                "{} carries no {} row",
                path.display(),
                sheet.name()
            );
            for row in rows {
                assert_eq!(
                    row.columns().len(),
                    sheet.headers().len(),
                    "{} has a short row on {}",
                    path.display(),
                    sheet.name()
                );
            }
            counted[index] += rows.len();
        }
    }
    // The sample's row counts, which `importers.md` states [IMP-SAXO-001].
    assert_eq!(counted, [188, 33, 242]);
    assert_eq!(
        [
            Sheet::Transacties.headers().len(),
            Sheet::Detail.headers().len(),
            Sheet::Bookings.headers().len(),
        ],
        [31, 24, 21]
    );
}

/// The rows an import stores are the `Transacties` rows, the detail sheets being joined inputs
/// rather than records of their own [DOM-007], [IMP-SAXO-001].
#[test]
fn the_rows_an_import_reads_are_the_cash_ledger() {
    for (_, export) in exports() {
        assert_eq!(export.rows(), export.sheet(Sheet::Transacties));
    }
}

/// The headers the export spells with non-breaking spaces and with a leading space resolve on
/// the real files, under their ordinary spellings [IMP-SAXO-002].
#[test]
fn a_fixture_row_answers_its_headers_after_normalization() {
    for (path, export) in exports() {
        for row in export.rows() {
            for header in ["Bk Record Id", "Booking Id", "Positie-ID"] {
                assert!(
                    field(row, header).is_some(),
                    "{} has a row answering nothing for {header}",
                    path.display()
                );
            }
        }
        for row in export.sheet(Sheet::Detail) {
            assert!(field(row, "Traded Quantity").is_some());
            assert!(field(row, "Trade Event Type").is_some());
        }
        for row in export.sheet(Sheet::Bookings) {
            assert!(field(row, "Amount Type Id").is_some());
            assert!(field(row, "Tax Percentage").is_some());
        }
    }
}

/// Every date column converts from its Excel serial, and lands in the calendar year the file is
/// confined to [IMP-SAXO-003], [IMP-001].
#[test]
fn every_transaction_date_converts_from_its_serial() {
    for (path, export) in exports() {
        let name = path
            .file_name()
            .expect("a fixture filename")
            .to_string_lossy();
        let year: i32 = name
            .split('_')
            .nth(2)
            .and_then(|start| start.get(..4))
            .and_then(|year| year.parse().ok())
            .expect("a fixture filename names the year it starts in");

        let dates: Vec<NaiveDate> = export
            .rows()
            .iter()
            .map(|row| {
                date(row, "Transactiedatum")
                    .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
            })
            .collect();

        assert!(
            dates.iter().all(|date| date.year() == year),
            "{} carries a trade date outside {year}",
            path.display()
        );
        for row in export.sheet(Sheet::Bookings) {
            date(row, "Boekingsdatum").expect("a Bookings row carries a booking date");
        }
        for row in export.sheet(Sheet::Detail) {
            date(row, "Aangepaste transactiedatum").expect("a _Transacties row carries its date");
        }
    }
}

/// Every position-affecting row resolves a `_Transacties` counterpart, and no `_Transacties` row
/// is left over — the latter by construction, since an unjoined one is a refusal and the
/// fixtures read [IMP-SAXO-037].
#[test]
fn every_position_affecting_row_resolves_its_counterpart() {
    let labels: Vec<String> = exports()
        .iter()
        .flat_map(|(_, export)| {
            (0..export.rows().len())
                .filter(|index| export.detail_of(*index).next().is_some())
                .map(|index| action(export, index).to_owned())
                .collect::<Vec<_>>()
        })
        .collect();

    // The sample's 32 position-affecting rows, as `importers.md` counts them [IMP-SAXO-037].
    assert_eq!(labels.len(), 32, "the joined rows are {labels:?}");
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
    // The tender offer and the `Terugboeking` that reverses it.
    assert_eq!(family("Terugkoopaanbod"), 2);
    for single in ["Koop ", "Verkoop ", "Omwisseling", "Expiratie"] {
        assert_eq!(
            family(single),
            1,
            "no {single} row joins a _Transacties row"
        );
    }
}

/// A corporate action's legs are `_Transacties` rows under one `Corporate action-Id`, each
/// carrying a `Trade Event Type` of `Gekocht` or `Verkocht`, and the join answers all of them
/// [IMP-SAXO-037].
///
/// The shapes are the sample's, and two of them are not the `Verkocht` / `Gekocht` pair, which
/// is why every shape is asserted rather than a leg count assumed [DEC-071]: the 2023 tender
/// joins three legs — two `Verkocht` and the reversal's `Gekocht` — and the 2023 stock dividend
/// joins two `Gekocht` and no `Verkocht`. Both shapes are in the real export as well as in the
/// fixture. Summing a side and cancelling an opposing pair belongs to the rules that read the
/// legs [IMP-SAXO-044], [IMP-SAXO-045]; what is asserted here is that the reader hands over
/// every leg rather than some of them.
#[test]
fn a_corporate_action_answers_every_leg_it_has() {
    let mut shapes: Vec<Vec<String>> = Vec::new();
    for (path, export) in &exports() {
        for index in 0..export.rows().len() {
            let legs: Vec<String> = export
                .detail_of(index)
                .map(|row| {
                    field(row, "Trade Event Type")
                        .expect("a leg states its event type")
                        .to_owned()
                })
                .collect();
            if legs.len() < 2 {
                continue;
            }
            assert!(
                legs.iter()
                    .all(|event| event == "Gekocht" || event == "Verkocht"),
                "{} joins {} to a leg that is neither Gekocht nor Verkocht: {legs:?}",
                path.display(),
                action(export, index)
            );
            assert!(
                !field(
                    &export.sheet(Sheet::Transacties)[index],
                    "Corporate action-Id"
                )
                .expect("Transacties carries the group id")
                .is_empty(),
                "{} joins several legs to a row with no Corporate action-Id",
                path.display()
            );
            shapes.push(legs);
        }
    }

    let of_shape = |wanted: &[&str]| {
        shapes
            .iter()
            .filter(|legs| legs.iter().map(String::as_str).eq(wanted.iter().copied()))
            .count()
    };
    assert_eq!(shapes.len(), 10, "the sample's multi-leg corporate actions");
    // The two splits, the exchange and the three merger rows: the pair the specification states.
    assert_eq!(of_shape(&["Verkocht", "Gekocht"]), 6);
    // The tender and the row that reverses it, each joining the group's three legs.
    assert_eq!(of_shape(&["Verkocht", "Verkocht", "Gekocht"]), 2);
    // The stock dividend, whose two legs are both openings.
    assert_eq!(of_shape(&["Gekocht", "Gekocht"]), 2);
}

/// A cash movement resolves the `Bookings` components it decomposes into, on the keys
/// `importers.md` names [IMP-SAXO-037].
#[test]
fn a_cash_movement_resolves_its_components() {
    let joined: usize = exports()
        .iter()
        .map(|(_, export)| {
            (0..export.rows().len())
                .filter(|index| export.bookings_of(*index).next().is_some())
                .count()
        })
        .sum();

    // The sample's 171 bookings that decompose; the remaining 17 rows are position-only.
    assert_eq!(joined, 171);
}
