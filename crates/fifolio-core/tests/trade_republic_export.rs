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

use chrono::{DateTime, Datelike as _, NaiveDate, Utc};
use fifolio_core::decimal::Scaled;
use fifolio_core::entities::{Account, ImportBatch, RecordIdentity, SecurityType, SourceFormat};
use fifolio_core::identity::{IdentitySource, identify};
use fifolio_core::import::reader::SourceRow;
use fifolio_core::import::trade_republic::money::Booked;
use fifolio_core::import::trade_republic::security::Instrument;
use fifolio_core::import::trade_republic::{
    DIRECTION, HEADERS, QUANTITY_COLUMN, TradeRepublic, UNIT_PRICE_COLUMN, booking_instant, field,
    identity, is_outflow, ordering_key, read, trade_date,
};
use fifolio_core::import::{Ground, Import, ImportError, StoredAs, import};
use fifolio_core::ordering::assign_orders;
use fifolio_core::storage::Database;
use fifolio_test_support::TempDb;
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
/// This is the reader's half; the import's is
/// `a_second_import_of_a_fixture_stores_no_new_source_record`.
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

/// Every row of every fixture states money this reader understands [IMP-TR-005]: a blank cell is
/// an absent figure, and a populated one parses.
#[test]
fn every_fixture_row_states_money_the_reader_understands() {
    for (path, rows) in exports() {
        for row in &rows {
            Booked::read(row).unwrap_or_else(|error| {
                panic!(
                    "{}: {} does not state readable money: {error}",
                    path.display(),
                    identity(row).expect("a fixture row is identified")
                )
            });
        }
    }
}

/// A buy's gross is its `amount`, fee excluded, and the fee is carried beside it [IMP-TR-016].
/// A mapping, not an arithmetic claim: the fixtures' amounts are perturbed [TST-014], so the
/// assertion is that the columns land where the requirement says and never that they multiply
/// out.
#[test]
fn every_fixture_buy_grosses_its_amount_with_the_fee_beside_it() {
    let mut buys = 0;
    for (path, rows) in exports() {
        for row in rows.iter().filter(|row| {
            field(row, "category") == Ok("TRADING") && field(row, "type") == Ok("BUY")
        }) {
            buys += 1;
            let booked = Booked::read(row).expect("a buy states readable money");
            let amount = figure(row, "amount").expect("a buy states an amount");
            let fee = figure(row, "fee").unwrap_or_default();

            assert_eq!(
                booked.gross().map(Scaled::get),
                Some(amount.abs()),
                "{}: the gross is the movement itself",
                path.display()
            );
            assert_eq!(
                booked.fees().map(Scaled::get),
                Ok(fee.abs()),
                "{}: the fee is beside the gross",
                path.display()
            );
        }
    }

    assert_eq!(buys, 7, "the fixtures' seven buys");
}

/// On every row stating an amount, gross = |amount| and fees = |fee| + |tax|, both in the row's
/// `currency` [IMP-TR-016], [IMP-TR-007]. Asserted as the relation and not as figures: the
/// fixtures' amounts are perturbed [TST-014].
#[test]
fn every_fixture_row_grosses_its_amount_and_sums_its_fee_and_tax() {
    let mut rows_with_tax = 0;
    for (path, rows) in exports() {
        for row in rows.iter().filter(|row| figure(row, "amount").is_some()) {
            let booked = Booked::read(row).expect("a fixture row states readable money");
            let magnitude = |column| figure(row, column).unwrap_or_default().abs();
            rows_with_tax += usize::from(figure(row, "tax").is_some());

            assert_eq!(
                booked.gross().map(Scaled::get),
                Some(magnitude("amount")),
                "{}: {} grosses its amount",
                path.display(),
                identity(row).expect("a fixture row is identified")
            );
            assert_eq!(
                booked.fees().map(Scaled::get),
                Ok(magnitude("fee") + magnitude("tax")),
                "{}: {} sums its fee and tax",
                path.display(),
                identity(row).expect("a fixture row is identified")
            );
            assert_eq!(
                booked.currency().map(|currency| currency.code()),
                Some(field(row, "currency").expect("the column is carried")),
                "{}: the figures are in the row's own currency",
                path.display()
            );
        }
    }

    // Without a taxed row the tax half of the relation would be asserted on nothing.
    assert!(rows_with_tax > 0, "the fixtures carry taxed rows");
}

/// The foreign dividends' `original_*` triple is read as the file states it, `fx_rate`
/// verbatim, and none of it reaches the settlement figures: the gross stays `|amount|` in the
/// row's EUR `currency` [IMP-TR-006], (DEC-073). This replaces an assertion that the rate was
/// already foreign units per EUR, which DEC-073 makes true only up to 2024-07-02.
#[test]
fn the_fixture_dividends_carry_their_foreign_side_as_stated() {
    let mut dividends = 0;
    for (path, rows) in exports() {
        for row in rows
            .iter()
            .filter(|row| field(row, "type") == Ok("DIVIDEND"))
        {
            dividends += 1;
            let booked = Booked::read(row).expect("a dividend states readable money");

            assert_eq!(
                booked.original_amount(),
                figure(row, "original_amount"),
                "{}: the foreign amount is the file's",
                path.display()
            );
            assert_eq!(
                booked.original_currency().map(|currency| currency.code()),
                Some(field(row, "original_currency").expect("the column is carried")),
                "{}: the foreign currency is the file's",
                path.display()
            );
            assert_eq!(
                booked.fx_rate(),
                figure(row, "fx_rate"),
                "{}: the rate is the file's, untouched",
                path.display()
            );
            assert_eq!(
                booked.gross().map(Scaled::get),
                figure(row, "amount").map(|amount| amount.abs()),
                "{}: the foreign side values nothing",
                path.display()
            );
            assert!(booked.fx_rate().is_some() && booked.original_currency().is_some());
        }
    }

    assert_eq!(dividends, 14, "the fixtures' 14 dividends");
}

/// The fixtures an import accepts today: every one but 2024, whose `TAX_EXCHANGE` pair is
/// FIF-068's and refuses the file until it lands.
fn importable() -> Vec<(PathBuf, Vec<SourceRow>)> {
    exports()
        .into_iter()
        .filter(|(path, _)| !path.to_string_lossy().contains("2024"))
        .collect()
}

fn import_fixture(path: &Path) -> Result<Import, ImportError> {
    let content = fs::read(path).expect("a fixture is readable");
    import(&TradeRepublic, &account(), &content)
}

/// Each importable fixture passes the year guard and every other ground, and stores exactly its
/// `TRADING` / `BUY` rows, derived automatically; every other row is a recognized cash type,
/// counted and not stored, the foreign dividends included, and no type goes unrecognized
/// [IMP-001], [IMP-TR-008], [IMP-TR-011], [IMP-TR-012], [IMP-TR-017].
#[test]
fn every_importable_fixture_stores_its_buys_and_counts_the_rest() {
    let mut buys = 0;
    for (path, rows) in importable() {
        let import = import_fixture(&path)
            .unwrap_or_else(|error| panic!("{} does not import: {error}", path.display()));
        let expected: BTreeSet<&str> = rows
            .iter()
            .filter(|row| {
                field(row, "category") == Ok("TRADING") && field(row, "type") == Ok("BUY")
            })
            .map(|row| identity(row).expect("an identity"))
            .collect();
        buys += expected.len();

        let stored: BTreeSet<&str> = import
            .stored()
            .iter()
            .map(|stored| {
                assert_eq!(stored.stored_as(), StoredAs::DerivedAutomatically);
                stored
                    .record()
                    .field("transaction_id")
                    .expect("a stored id")
            })
            .collect();
        assert_eq!(stored, expected, "{}", path.display());
        assert_eq!(
            usize::try_from(import.counts().non_position).expect("a count"),
            rows.len() - expected.len(),
            "{}",
            path.display()
        );
        assert!(
            import.unrecognized_types().is_empty(),
            "{}: {:?}",
            path.display(),
            import.unrecognized_types()
        );
    }

    // 2024's two buys are in the file that is not importable yet.
    assert_eq!(buys, 5, "the importable fixtures' five buys");
}

/// The 2024 fixture is refused on its `TAX_EXCHANGE` pair alone, both rows named, and not on its
/// year: its corporate action is booked in the same year it takes effect, and the guard reads the
/// trade date regardless [IMP-001], [IMP-002].
#[test]
fn the_2024_fixture_is_refused_on_its_tax_exchange_pair_alone() {
    let (path, rows) = exports()
        .into_iter()
        .find(|(path, _)| path.to_string_lossy().contains("2024"))
        .expect("the 2024 fixture is committed");

    let refusal = import_fixture(&path).expect_err("TAX_EXCHANGE is not imported yet");

    let ImportError::Refused { grounds } = refusal else {
        panic!("a refusal, not {refusal}");
    };
    let [Ground::FailedRows { failures }] = grounds.as_slice() else {
        panic!("only failed rows, not {grounds:?}");
    };
    let refused: Vec<usize> = failures.iter().map(|failure| failure.position).collect();
    let exchanges: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| field(row, "type") == Ok("TAX_EXCHANGE"))
        .map(|(position, _)| position)
        .collect();
    assert_eq!(refused, exchanges);
    assert_eq!(refused.len(), 2);
}

/// A fixture imported twice stores no new source record on the second pass: the second import
/// yields the very records of the first, and each is one storage already holds [DOM-022],
/// [SRV-015], [IMP-TR-003]. Both passes pass the year guard [IMP-001]. This is the obligation
/// FIF-027 carried to FIF-029.
#[tokio::test]
async fn a_second_import_of_a_fixture_stores_no_new_source_record() {
    let db = TempDb::new();
    let database = Database::open(db.path())
        .await
        .expect("open the temporary database");
    database
        .accounts()
        .insert(&account())
        .await
        .expect("the account");
    let imported_at: DateTime<Utc> = "2026-01-02T09:00:00Z".parse().expect("a timestamp");

    for (path, _) in importable() {
        let first = import_fixture(&path).expect("the first pass imports");
        let batch = database
            .import_batches()
            .insert(&ImportBatch::new(
                account(),
                path.display().to_string(),
                SourceFormat::TradeRepublicDeCsv,
                imported_at,
                first.counts(),
            ))
            .await
            .expect("the batch");
        for stored in first.stored() {
            database
                .source_records()
                .insert(batch, stored.record())
                .await
                .expect("the source record");
        }

        let second = import_fixture(&path).expect("the second pass imports");

        assert_eq!(second, first, "{}", path.display());
        assert!(!second.stored().is_empty(), "{}", path.display());
        for stored in second.stored() {
            assert_eq!(
                database
                    .source_records()
                    .find(stored.record().identity())
                    .await
                    .expect("the lookup"),
                Some(stored.record().clone()),
                "{}: the second pass's record is already stored",
                path.display()
            );
        }
    }
}

/// Every fixture row naming a security states an `asset_class` the table maps, and the fixtures
/// carry both of its rows; every row naming none reads no type [IMP-TR-020].
#[test]
fn every_fixture_row_maps_onto_the_asset_class_table() {
    let mut types = BTreeSet::new();
    let mut none = 0_usize;
    for (path, rows) in exports() {
        for (index, row) in rows.iter().enumerate() {
            match Instrument::read(row).unwrap_or_else(|error| {
                panic!("{} row {} is refused: {error}", path.display(), index + 2)
            }) {
                Instrument::Security(security) => {
                    assert!(security.is_auto_created());
                    types.insert(security.security_type());
                }
                Instrument::None => none += 1,
            }
        }
    }

    assert_eq!(
        types,
        BTreeSet::from([SecurityType::Stock, SecurityType::Fund])
    );
    assert_eq!(
        none, 41,
        "the deposits and interest payments name no security"
    );
}
