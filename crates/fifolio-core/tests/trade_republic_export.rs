//! Integration layer [TST-003]: the committed Trade Republic fixtures, read by the Trade
//! Republic reader.
//!
//! The unit tests in `import::trade_republic` state each rule against a file built for it; these
//! state that the rules hold on the files an import will actually meet [TST-011], [TST-013].
//! Nothing here asserts an amount: the fixtures' amounts are perturbed [TST-014]. The sign of an
//! amount is asserted, because perturbation preserves it [TST-028].

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike as _, NaiveDate};
use fifolio_core::entities::{Account, RecordIdentity};
use fifolio_core::identity::{IdentitySource, identify};
use fifolio_core::import::reader::SourceRow;
use fifolio_core::import::trade_republic::{
    DIRECTION, HEADERS, QUANTITY_COLUMN, UNIT_PRICE_COLUMN, booking_instant, field, identity,
    is_outflow, ordering_key, read, trade_date,
};
use fifolio_core::ordering::assign_orders;
use rust_decimal::Decimal;

/// Every committed Trade Republic fixture, with its path.
fn exports() -> Vec<(PathBuf, Vec<SourceRow>)> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate directory is two levels below the workspace root")
        .join("fixtures/trade-republic");
    let mut paths: Vec<PathBuf> = fs::read_dir(directory)
        .expect("the fixture directory is committed")
        .map(|entry| entry.expect("a readable directory entry").path())
        .filter(|path| path.extension().is_some_and(|found| found == "csv"))
        .collect();
    paths.sort();
    assert_eq!(paths.len(), 4, "the four committed Trade Republic fixtures");

    paths
        .into_iter()
        .map(|path| {
            let content = fs::read(&path).expect("a fixture is readable");
            let rows = read(&content)
                .unwrap_or_else(|error| panic!("{} does not read: {error}", path.display()));
            (path, rows)
        })
        .collect()
}

/// The account every fixture is imported into. The export carries no account id of any kind, so
/// the account is the import's and not the file's; whether it can be verified against the file
/// at all is OQ-015 and is not settled here.
fn account() -> Account {
    Account::new("trade-republic", "fixture")
}

/// A value of `column` on `row`, read as a decimal. `None` where the fixture leaves it blank,
/// which is most cells of most rows.
fn figure(row: &SourceRow, column: &str) -> Option<Decimal> {
    let value = field(row, column).expect("the fixture carries the format's columns");
    (!value.is_empty()).then(|| {
        value
            .parse()
            .unwrap_or_else(|_| panic!("{column} holds {value:?}, which is not a number"))
    })
}

/// Every fixture is a single header row of the format's 23 columns, in the file's own order
/// [IMP-TR-001].
#[test]
fn every_fixture_carries_the_twenty_three_headers_in_order() {
    for (path, rows) in exports() {
        assert!(!rows.is_empty(), "{} carries rows", path.display());
        for row in &rows {
            let headers: Vec<&str> = row
                .columns()
                .iter()
                .map(|(name, _)| name.as_str())
                .collect();

            assert_eq!(headers, HEADERS, "{}", path.display());
        }
    }
}

/// Every row carries both date columns, so no absent-value rule is needed for the ordering
/// columns [IMP-TR-002], [IMP-TR-023].
#[test]
fn every_fixture_row_carries_both_date_columns() {
    for (path, rows) in exports() {
        for (index, row) in rows.iter().enumerate() {
            let named = || format!("{} row {}", path.display(), index + 2);

            trade_date(row).unwrap_or_else(|error| panic!("{}: {error}", named()));
            booking_instant(row).unwrap_or_else(|error| panic!("{}: {error}", named()));
        }
    }
}

/// Each fixture is confined to the calendar year its filename names, which is what an import
/// takes a file at a time [IMP-001], [IMP-002], [TST-011]. The year is the **trade dates'**: the
/// 2024 corporate action is booked six days after its effective date, so a file read on its
/// booking timestamps could span two years where its trade dates do not.
#[test]
fn every_fixture_holds_one_calendar_year_of_trade_dates() {
    for (path, rows) in exports() {
        let year: i32 = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.split('_').nth(1))
            .and_then(|start| start.split('-').next())
            .and_then(|year| year.parse().ok())
            .expect("a fixture filename names the year it starts in");
        let years: BTreeSet<i32> = rows
            .iter()
            .map(|row| trade_date(row).expect("a trade date").year())
            .collect();

        assert_eq!(years, BTreeSet::from([year]), "{}", path.display());
    }
}

/// `transaction_id` is a UUID and identifies the row [IMP-TR-003], [TST-013].
#[test]
fn every_fixture_row_is_identified_by_a_uuid_transaction_id() {
    let uuid_shaped = |reference: &str| {
        let groups: Vec<&str> = reference.split('-').collect();
        groups.iter().map(|group| group.len()).eq([8, 4, 4, 4, 12])
            && groups
                .iter()
                .all(|group| group.chars().all(|digit| digit.is_ascii_hexdigit()))
    };

    for (path, rows) in exports() {
        for (index, row) in rows.iter().enumerate() {
            let found = identity(row)
                .unwrap_or_else(|error| panic!("{} row {}: {error}", path.display(), index + 2));

            assert!(
                uuid_shaped(found),
                "{} row {} identifies as {found:?}, which is not a UUID",
                path.display(),
                index + 2
            );
        }
    }
}

/// Reading a fixture twice yields the same identities, which is what makes a re-import create no
/// new records: a source record is keyed by its identity [DOM-022], [DOM-023], [IMP-TR-003].
///
/// This is the reader's half. That a second `import::import` of the same file stores no new source
/// record cannot be asserted yet — no `impl Importer for TradeRepublic` exists, its classification
/// being FIF-029's — and is recorded on that item.
#[test]
fn re_reading_a_fixture_identifies_the_same_records() {
    for (path, rows) in exports() {
        let identities = |rows: &[SourceRow]| -> BTreeSet<RecordIdentity> {
            rows.iter()
                .map(|row| {
                    identify(
                        &account(),
                        &IdentitySource::BrokerReference(identity(row).expect("an identity")),
                    )
                })
                .collect()
        };

        let first = identities(&rows);
        let content = fs::read(&path).expect("a fixture is readable");
        let second = identities(&read(&content).expect("the fixture reads"));

        assert_eq!(
            first,
            second,
            "{} identifies differently twice",
            path.display()
        );
        assert_eq!(
            first.len(),
            rows.len(),
            "{} has one identity per row",
            path.display()
        );
    }
}

/// The orders a fixture yields are the same on a second read and are a permutation of its rows
/// [DOM-040], [IMP-TR-023].
#[test]
fn a_fixtures_orders_reproduce_on_a_second_read() {
    for (path, rows) in exports() {
        let orders = |rows: &[SourceRow]| {
            let keys: Vec<_> = rows
                .iter()
                .map(|row| ordering_key(row).expect("an ordering key"))
                .collect();
            assign_orders(&keys, DIRECTION)
        };

        let first = orders(&rows);
        let content = fs::read(&path).expect("a fixture is readable");
        let second = orders(&read(&content).expect("the fixture reads"));

        assert_eq!(first, second, "{} orders differently twice", path.display());
        assert_eq!(
            first.iter().collect::<BTreeSet<_>>().len(),
            rows.len(),
            "{} orders its rows one apiece",
            path.display()
        );
    }
}

/// The 2025 export ascends by `date` and descends by `datetime` within a date, so file position
/// alone orders it wrongly; the assigned orders follow the timestamp [IMP-TR-023].
#[test]
fn the_2025_fixture_descends_by_timestamp_within_a_date() {
    let (path, rows) = exports()
        .into_iter()
        .find(|(path, _)| path.to_string_lossy().contains("2025"))
        .expect("the 2025 fixture is committed");
    let keys: Vec<_> = rows
        .iter()
        .map(|row| ordering_key(row).expect("an ordering key"))
        .collect();
    let orders = assign_orders(&keys, DIRECTION);

    let descending: Vec<usize> = keys
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| {
            pair[0].trade_date == pair[1].trade_date && pair[0].columns > pair[1].columns
        })
        .map(|(index, _)| index)
        .collect();

    assert!(
        !descending.is_empty(),
        "{} carries the descending-timestamp shape",
        path.display()
    );
    for index in descending {
        assert!(
            orders[index + 1] < orders[index],
            "{} rows {} and {} order by their timestamps, not their positions",
            path.display(),
            index + 2,
            index + 3
        );
    }
}

/// `date` and `datetime` are independent fields: the 2024 corporate action takes effect six days
/// before it is booked, and a dividend's booking crosses midnight UTC the other way
/// [IMP-TR-015].
#[test]
fn the_fixtures_carry_the_divergence_between_the_two_date_columns() {
    let divergences: Vec<i64> = exports()
        .into_iter()
        .flat_map(|(_, rows)| rows)
        .map(|row| {
            let booked =
                DateTime::from_timestamp_nanos(booking_instant(&row).expect("a booking timestamp"))
                    .date_naive();
            let effective: NaiveDate = trade_date(&row).expect("a trade date");
            booked.signed_duration_since(effective).num_days()
        })
        .collect();

    assert_eq!(
        divergences.iter().copied().max(),
        Some(6),
        "the corporate action is booked six days after it takes effect"
    );
    assert_eq!(
        divergences.iter().copied().min(),
        Some(-1),
        "a booking crossing midnight UTC falls on the day before the effective date"
    );
}

/// The signs are cash flow: every fixture trade is a buy and every buy's `amount` and `fee` are
/// negative [IMP-TR-004].
#[test]
fn every_fixture_buy_states_its_money_as_an_outflow() {
    let mut buys = 0;
    for (path, rows) in exports() {
        for row in rows.iter().filter(|row| {
            field(row, "category") == Ok("TRADING") && field(row, "type") == Ok("BUY")
        }) {
            buys += 1;
            let named = |column| format!("{}: {column} of a buy", path.display());

            assert!(
                is_outflow(figure(row, "amount").expect("a buy states an amount")),
                "{}",
                named("amount")
            );
            if let Some(fee) = figure(row, "fee") {
                assert!(is_outflow(fee), "{}", named("fee"));
            }
            // The quantity and the unit price are magnitudes, not cash flows [IMP-TR-022].
            for column in [QUANTITY_COLUMN, UNIT_PRICE_COLUMN] {
                assert!(
                    !is_outflow(figure(row, column).expect("a buy states both")),
                    "{}",
                    named(column)
                );
            }
        }
    }

    assert_eq!(buys, 7, "the fixtures' seven buys");
}

/// The other direction of the same convention: a receipt — a dividend, interest, a deposit, an
/// inbound transfer — states its `amount` positive [IMP-TR-004]. Asserted on the files because
/// perturbation preserves a sign [TST-028], so a sign-flipping anonymizer is what this catches.
#[test]
fn every_fixture_receipt_states_its_money_as_an_inflow() {
    let mut receipts = 0;
    for (path, rows) in exports() {
        for row in rows.iter().filter(|row| {
            field(row, "category") == Ok("CASH")
                && matches!(
                    field(row, "type"),
                    Ok("DIVIDEND" | "INTEREST_PAYMENT" | "CUSTOMER_INBOUND" | "TRANSFER_INBOUND")
                )
        }) {
            receipts += 1;

            assert!(
                !is_outflow(figure(row, "amount").expect("a receipt states an amount")),
                "{}: amount of a receipt",
                path.display()
            );
        }
    }

    assert_eq!(receipts, 55, "the fixtures' 55 cash receipts");
}
