//! Integration layer [TST-003]: the committed Saxo fixtures, read by the Saxo reader.
//!
//! The unit tests in `import::saxo` state each rule against a workbook built for it; these state
//! that the rules hold on the files an import will actually meet [TST-011], [TST-013], [TST-031].
//! Nothing here asserts an amount: the fixtures' amounts are perturbed [TST-014].

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{Datelike as _, NaiveDate};
use fifolio_core::entities::{Account, RecordIdentity};
use fifolio_core::identity::{IdentitySource, identify};
use fifolio_core::import::saxo::identity::{account, check_identities, identity};
use fifolio_core::import::saxo::money::Booked;
use fifolio_core::import::saxo::quantity::{Label, StatedBy, traded};
use fifolio_core::import::saxo::{SaxoError, SaxoWorkbook, Sheet, date, field};

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

/// The fixtures an import can get past the identity check today: every one but the 2023 file,
/// whose corporate action collides on its `Corporate action-Id` [IMP-SAXO-024].
fn importable() -> Vec<(PathBuf, SaxoWorkbook)> {
    exports()
        .into_iter()
        .filter(|(_, export)| check_identities(export.rows()).is_ok())
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

/// Every row of every fixture names the one Depot, its per-currency sub-accounts collapsed
/// [DOM-003], [IMP-SAXO-005].
///
/// The fixtures carry all three suffixes on one base account, so the collapse is what the
/// assertion rests on and not an accident of a single-currency file.
#[test]
fn the_per_currency_sub_accounts_collapse_onto_one_depot() {
    for (path, export) in exports() {
        let sub_accounts: BTreeSet<&str> = export
            .rows()
            .iter()
            .filter_map(|row| field(row, "Rekening-ID"))
            .collect();
        let depots: BTreeSet<&str> = export
            .rows()
            .iter()
            .map(|row| account(row).expect("every fixture row names an account"))
            .collect();

        assert_eq!(
            sub_accounts.len(),
            3,
            "{} carries the EUR, USD and CAD sub-accounts",
            path.display()
        );
        assert_eq!(depots.len(), 1, "{} is one Depot", path.display());
    }
}

/// `Klant-id` is the client and is never the account [IMP-SAXO-006].
///
/// Observable because the fixtures' client id is not any account id: were it read as the
/// account, the Depot above would be the client's number instead.
#[test]
fn the_client_id_is_not_the_account() {
    for (path, export) in exports() {
        for row in export.rows() {
            let client = field(row, "Klant-id").expect("Transacties carries Klant-id");
            let depot = account(row).expect("every fixture row names an account");

            assert_ne!(
                client,
                depot,
                "{} names its client as its account",
                path.display()
            );
        }
    }
}

/// Every row of the sample carries at least one of the four identity columns [IMP-SAXO-007].
#[test]
fn every_fixture_row_is_identified_by_one_of_the_four_columns() {
    for (path, export) in exports() {
        for (index, row) in export.rows().iter().enumerate() {
            let found = identity(row)
                .unwrap_or_else(|error| panic!("{} row {}: {error}", path.display(), index + 2));

            assert!(!found.is_empty());
        }
    }
}

/// Reading a fixture twice yields the same identities, which is what makes a re-import create no
/// new records: a source record is keyed by its identity [DOM-022], [DOM-023], [IMP-SAXO-007].
///
/// The 2023 fixture is not among these — it is refused outright, which the next test states.
#[test]
fn re_reading_a_fixture_identifies_the_same_records() {
    for (path, export) in importable() {
        let account = Account::new("saxo", account(&export.rows()[0]).expect("an account"));
        let identities = |export: &SaxoWorkbook| -> BTreeSet<RecordIdentity> {
            export
                .rows()
                .iter()
                .map(|row| {
                    identify(
                        &account,
                        &IdentitySource::BrokerReference(identity(row).expect("an identity")),
                    )
                })
                .collect()
        };

        let first = identities(&export);
        let content = fs::read(&path).expect("a fixture is readable");
        let second = identities(&SaxoWorkbook::read(&content).expect("the fixture reads"));

        assert_eq!(
            first,
            second,
            "{} identifies differently twice",
            path.display()
        );
        assert_eq!(
            first.len(),
            export.rows().len(),
            "{} has one identity per row",
            path.display()
        );
    }
}

/// A file whose rows do not all identify differently is refused rather than deduplicated
/// [IMP-SAXO-024].
///
/// The 2023 fixture carries a `Terugkoopaanbod` and its `Terugboeking` under one
/// `Corporate action-Id` and no other id column, so both fall through to that id and produce one
/// identity. Deduplicating them would drop the reversal and leave the event's money wrong, so the
/// file is refused. The composite identity that will tell such rows apart is IMP-SAXO-008, which
/// is undecided and is FIF-084's; until it lands this refusal is the safe failure, and this test
/// is what will change when it does.
#[test]
fn a_file_with_two_rows_of_one_identity_is_refused() {
    let refused: Vec<(PathBuf, SaxoError)> = exports()
        .into_iter()
        .filter_map(|(path, export)| {
            check_identities(export.rows())
                .err()
                .map(|error| (path, error))
        })
        .collect();

    let [(path, error)] = refused.as_slice() else {
        panic!("one fixture is refused, not {}", refused.len());
    };
    assert!(path.ends_with("Transactions_10000000_2023-01-01_2023-12-31.xlsx"));
    assert_eq!(
        *error,
        SaxoError::DuplicateIdentity {
            identity: "5000135".to_owned(),
            first: 11,
            second: 12,
        }
    );
}

/// Every row of every fixture states its four money columns and its native currency, so the
/// derivation reads a figure off each one rather than defaulting [IMP-SAXO-009], [IMP-SAXO-010].
///
/// Parsing only: no amount is asserted, the fixtures' amounts being perturbed [TST-014]. A
/// non-EUR row is what makes `Omrekeningskoers` load-bearing, so the corpus is checked to carry
/// one — without it a reader that ignored the column would pass this.
#[test]
fn every_fixture_row_states_the_money_it_booked() {
    let mut foreign = 0_usize;
    for (path, export) in exports() {
        for (index, row) in export.rows().iter().enumerate() {
            let booked = Booked::read(row).unwrap_or_else(|error| {
                panic!(
                    "{} row {} states no money: {error}",
                    path.display(),
                    index + 2
                )
            });
            if !booked.currency().is_eur() {
                foreign += 1;
                booked
                    .conversion(date(row, "Transactiedatum").expect("a row dates its booking"))
                    .expect("a foreign row states an invertible quote");
            }
        }
    }

    assert!(foreign > 0, "the corpus carries foreign-currency rows");
}

/// Every `Acties` value in the corpus reads as a label, in one of its two shapes, and an
/// unparsable one is a failure rather than a guess [IMP-SAXO-038].
///
/// The counts are what keep this from passing on a parser that answered "no trade clause" to
/// everything: the sample's fifteen clause-bearing labels are the thirteen transfers, the one
/// buy and the one sell.
#[test]
fn every_fixture_label_reads_as_a_label() {
    let labels: Vec<Label> = exports()
        .iter()
        .flat_map(|(path, export)| {
            (0..export.rows().len())
                .map(|index| {
                    let acties = action(export, index);
                    Label::parse(acties).unwrap_or_else(|error| {
                        panic!(
                            "{} row {} labelled {acties:?}: {error}",
                            path.display(),
                            index + 2
                        )
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect();

    let with_clause = labels.iter().filter(|label| label.quantity().is_some());
    assert_eq!(with_clause.clone().count(), 15);
    assert_eq!(
        with_clause
            .filter(|label| label.price().is_some() && label.currency().is_some())
            .count(),
        15,
        "a trade clause states a price and a currency as well as a quantity"
    );
    assert!(
        labels.iter().any(|label| label.quantity().is_none()),
        "most of the ledger is labelled with an action alone"
    );
}

/// Every labelled quantity in the corpus agrees with the `Traded Quantity` of the row's
/// counterpart, and the counterpart is what answers [IMP-SAXO-038].
///
/// A disagreement refuses the file, so this is the assertion that the rule does not refuse the
/// real exports; the refusal itself is unit tested, no fixture carrying a mismatch. Rows joining
/// several legs are left out: their sides are summed after cancellation, which is IMP-SAXO-044's
/// and IMP-SAXO-045's, not this rule's.
#[test]
fn every_labelled_quantity_agrees_with_its_counterpart() {
    let mut checked = 0_usize;
    for (path, export) in exports() {
        for index in 0..export.rows().len() {
            let acties = action(&export, index);
            let label = Label::parse(acties).expect("a fixture label reads");
            let legs: Vec<_> = export.detail_of(index).collect();
            let [leg] = legs[..] else { continue };

            let traded = traded(&label, Some(leg)).unwrap_or_else(|error| {
                panic!(
                    "{} row {} labelled {acties:?}: {error}",
                    path.display(),
                    index + 2
                )
            });

            assert_eq!(traded.stated_by(), StatedBy::Columns);
            if let Some(stated) = label.quantity() {
                checked += 1;
                assert_eq!(traded.quantity(), stated, "{acties}");
            }
        }
    }

    // The thirteen transfers, the buy and the sell: every clause-bearing label in the corpus
    // joins exactly one leg.
    assert_eq!(checked, 15);
}
