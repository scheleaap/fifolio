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

use anonymize_exports::saxo::{self, Cell};
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

fn saxo_sheets() -> Vec<(PathBuf, saxo::Sheet)> {
    files_in("saxo-nl", "xlsx")
        .into_iter()
        .map(|path| {
            let sheet = saxo::read(&path).expect("a fixture Saxo export reads");
            (path, sheet)
        })
        .collect()
}

fn saxo_field(row: &[Cell], header: &str) -> String {
    let index = saxo::HEADERS
        .iter()
        .position(|candidate| *candidate == header)
        .expect("a named header");
    row[index].as_text()
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

/// A real XLSX container with one sheet and the 31 Dutch headers byte for byte — the
/// non-breaking spaces and the leading space included [IMP-SAXO-001], [IMP-SAXO-002], [TST-013].
#[test]
fn saxo_fixtures_are_xlsx_with_one_sheet_and_the_31_headers() {
    for (path, sheet) in saxo_sheets() {
        // `saxo::read` refuses a header row that is not the 31, so it having parsed is the
        // assertion; this states which 31 for the reader.
        let workbook: Xlsx<_> = open_workbook(&path).expect("a real XLSX container");
        assert_eq!(
            workbook.sheet_names(),
            vec![saxo::SHEET_NAME],
            "{} carries more than the one sheet",
            path.display()
        );
        assert!(!sheet.rows.is_empty(), "{} has no rows", path.display());
        for row in &sheet.rows {
            assert_eq!(row.len(), 31, "{} has a short row", path.display());
        }
    }
    assert!(saxo::HEADERS.contains(&"Bk\u{a0}Record\u{a0}Id"));
    assert!(saxo::HEADERS.contains(&"Booking\u{a0}Id"));
    assert!(saxo::HEADERS.contains(&" Positie-ID"));
}

/// Dates are Excel serial numbers, not text [IMP-SAXO-003], [TST-013].
#[test]
fn saxo_dates_are_excel_serial_numbers() {
    for (path, sheet) in saxo_sheets() {
        for row in &sheet.rows {
            for header in ["Transactiedatum", "Valutadatum"] {
                let index = saxo::HEADERS
                    .iter()
                    .position(|candidate| *candidate == header)
                    .expect("a date header");
                assert!(
                    matches!(row[index], Cell::Number(_)),
                    "{} holds {header} as something other than a serial number",
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

/// The identities are replaced [TST-012]. What a fixture cannot prove is what a value used to be,
/// so this asserts the other half: every identifying column carries a generated value.
#[test]
fn saxo_identities_are_generated_ones() {
    for (path, sheet) in saxo_sheets() {
        for row in &sheet.rows {
            let where_it_is = format!("{}", path.display());
            assert!(
                saxo_field(row, "Rekening-ID").starts_with("40100/"),
                "{where_it_is} carries an unreplaced account"
            );
            let isin = saxo_field(row, "Instrument ISIN");
            assert!(
                isin.is_empty() || isin.starts_with("XF"),
                "{where_it_is} carries an unreplaced ISIN {isin}"
            );
            let name = saxo_field(row, "Instrument");
            assert!(
                name.is_empty()
                    || name.starts_with("Fixture Instrument")
                    || name.starts_with("*Delisted"),
                "{where_it_is} carries an unreplaced instrument name {name}"
            );
            let owner = saxo_field(row, "Naam IBAN-eigenaar");
            assert!(
                owner.is_empty() || owner.starts_with("Fixture Owner"),
                "{where_it_is} carries an unreplaced name"
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
