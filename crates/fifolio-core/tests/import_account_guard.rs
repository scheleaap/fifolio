//! Integration layer [TST-003]: `import`'s whole-file account guard [IMP-003], driven through the
//! public `Importer` trait.
//!
//! Neither format has an `Importer` yet (FIF-029 and the Saxo items after FIF-020), so each
//! fixture is read by a test double built from its format's own public functions: the Saxo double
//! answers the account FIF-020 normalizes [DOM-003], the Trade Republic double leaves the account
//! hook at its default, which states none (DEC-075). A synthetic double covers the shapes no
//! fixture carries: rows from two accounts, and a row whose account cannot be read. No test
//! reaches the network.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;

use fifolio_core::entities::{Account, SourceFormat};
use fifolio_core::import::reader::{DelimitedReader, ReadError, RowReader, SourceRow};
use fifolio_core::import::saxo::{self, SaxoWorkbook};
use fifolio_core::import::{
    Ground, ImportError, Importer, RowClassification, RowError, RowFailure, RowIdentity, import,
    trade_republic,
};
use fifolio_core::ordering::{FileDirection, RowOrderingKey};

/// The Depot every Saxo fixture row names once its currency suffix is stripped, per
/// `fixtures/README.md`.
const SAXO_DEPOT: &str = "40100/9000001";

fn fixture(relative: &str) -> Vec<u8> {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate directory is two levels below the workspace root")
        .join("fixtures")
        .join(relative);
    fs::read(&path).unwrap_or_else(|error| panic!("{} is unreadable: {error}", path.display()))
}

fn saxo_fixture() -> Vec<u8> {
    fixture("saxo-nl/Transactions_10000000_2024-01-01_2024-12-31.xlsx")
}

fn row_error(error: impl ToString) -> RowError {
    RowError::new(error.to_string())
}

/// Every row stored, so that nothing but the guards can refuse a file.
fn all_derived(rows: &[SourceRow]) -> Vec<Result<RowClassification, RowError>> {
    rows.iter()
        .map(|_| Ok(RowClassification::DerivedAutomatically))
        .collect()
}

/// The `Transacties` rows of a Saxo workbook, as a Saxo import reads them (IMP-SAXO-001).
struct SaxoRows;

impl RowReader for SaxoRows {
    fn rows(&self, content: &[u8]) -> Result<Vec<SourceRow>, ReadError> {
        SaxoWorkbook::read(content)
            .map(|workbook| workbook.rows().to_vec())
            .map_err(|error| ReadError::Malformed {
                reason: error.to_string(),
            })
    }
}

/// A Saxo format as far as the account guard needs one: the account is FIF-020's normalized
/// `Rekening-ID` [DOM-003], [IMP-SAXO-005].
struct SaxoDouble;

impl Importer for SaxoDouble {
    fn format(&self) -> SourceFormat {
        SourceFormat::SaxoNlXlsx
    }

    fn reader(&self) -> &dyn RowReader {
        &SaxoRows
    }

    fn direction(&self) -> FileDirection {
        FileDirection::NewestFirst
    }

    fn identity(&self, row: &SourceRow) -> Result<RowIdentity, RowError> {
        saxo::identity::identity(row)
            .map(|reference| RowIdentity::BrokerReference(reference.to_owned()))
            .map_err(row_error)
    }

    fn ordering_key(&self, row: &SourceRow) -> Result<RowOrderingKey, RowError> {
        saxo::date(row, "Transactiedatum")
            .map(|trade_date| RowOrderingKey {
                trade_date,
                columns: Vec::new(),
            })
            .map_err(row_error)
    }

    fn account_id(&self, row: &SourceRow) -> Result<Option<String>, RowError> {
        saxo::identity::account(row)
            .map(|account| Some(account.to_owned()))
            .map_err(row_error)
    }

    fn classify(&self, rows: &[SourceRow]) -> Vec<Result<RowClassification, RowError>> {
        all_derived(rows)
    }
}

/// The Trade Republic reader, header check included (IMP-TR-001).
struct TradeRepublicRows;

impl RowReader for TradeRepublicRows {
    fn rows(&self, content: &[u8]) -> Result<Vec<SourceRow>, ReadError> {
        trade_republic::read(content).map_err(|error| ReadError::Malformed {
            reason: error.to_string(),
        })
    }
}

/// A Trade Republic format as far as the account guard needs one. `account_id` is deliberately
/// not overridden: the export carries no account identifier (DEC-075).
struct TradeRepublicDouble;

impl Importer for TradeRepublicDouble {
    fn format(&self) -> SourceFormat {
        SourceFormat::TradeRepublicDeCsv
    }

    fn reader(&self) -> &dyn RowReader {
        &TradeRepublicRows
    }

    fn direction(&self) -> FileDirection {
        trade_republic::DIRECTION
    }

    fn identity(&self, row: &SourceRow) -> Result<RowIdentity, RowError> {
        trade_republic::identity(row)
            .map(|reference| RowIdentity::BrokerReference(reference.to_owned()))
            .map_err(row_error)
    }

    fn ordering_key(&self, row: &SourceRow) -> Result<RowOrderingKey, RowError> {
        trade_republic::ordering_key(row).map_err(row_error)
    }

    fn classify(&self, rows: &[SourceRow]) -> Vec<Result<RowClassification, RowError>> {
        all_derived(rows)
    }
}

/// A format whose rows are `id,date,account`, every row stored; a blank `account` is one that
/// cannot be read.
struct AccountDouble {
    reader: DelimitedReader,
}

impl AccountDouble {
    fn new() -> Self {
        Self {
            reader: DelimitedReader::comma(),
        }
    }
}

impl Importer for AccountDouble {
    fn format(&self) -> SourceFormat {
        SourceFormat::SaxoNlXlsx
    }

    fn reader(&self) -> &dyn RowReader {
        &self.reader
    }

    fn direction(&self) -> FileDirection {
        FileDirection::OldestFirst
    }

    fn identity(&self, row: &SourceRow) -> Result<RowIdentity, RowError> {
        Ok(RowIdentity::BrokerReference(
            row.field("id").unwrap_or_default().to_owned(),
        ))
    }

    fn ordering_key(&self, row: &SourceRow) -> Result<RowOrderingKey, RowError> {
        let date = row.field("date").unwrap_or_default();
        let trade_date = NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .map_err(|_| RowError::new(format!("{date} is not a date")))?;
        Ok(RowOrderingKey {
            trade_date,
            columns: Vec::new(),
        })
    }

    fn account_id(&self, row: &SourceRow) -> Result<Option<String>, RowError> {
        match row.field("account") {
            Some(account) if !account.is_empty() => Ok(Some(account.to_owned())),
            _ => Err(RowError::new("no account")),
        }
    }

    fn classify(&self, rows: &[SourceRow]) -> Vec<Result<RowClassification, RowError>> {
        all_derived(rows)
    }
}

fn saxo_account(id: &str) -> Account {
    Account::new("Saxo", id)
}

/// A Saxo fixture imported into the Depot it names is not refused [IMP-003].
#[test]
fn a_saxo_file_naming_the_target_account_imports() {
    let import = import(&SaxoDouble, &saxo_account(SAXO_DEPOT), &saxo_fixture())
        .expect("the file names the target account");

    assert!(!import.stored().is_empty());
}

/// A Saxo fixture imported into another account is refused, and the refusal names both the
/// account the file names and the target [IMP-003].
#[test]
fn a_saxo_file_naming_another_account_is_refused_naming_both() {
    let error = import(&SaxoDouble, &saxo_account("40100/9000002"), &saxo_fixture())
        .expect_err("the file names a different account");

    assert_eq!(
        error,
        ImportError::Refused {
            grounds: vec![Ground::AccountMismatch {
                file: SAXO_DEPOT.to_owned(),
                target: "40100/9000002".to_owned(),
            }],
        }
    );
    let message = error.to_string();
    assert!(
        message.contains(SAXO_DEPOT) && message.contains("40100/9000002"),
        "{message}"
    );
}

/// Saxo is compared on the normalized Depot, not the raw suffixed cell [IMP-003], [DOM-003].
/// The fixture's rows carry the EUR, USD and CAD sub-accounts: compared raw, it would be refused
/// as a file of three accounts even when aimed at its own Depot. Compared normalized, it matches
/// the Depot (above), and an import aimed at any raw spelling is a mismatch naming the Depot.
#[test]
fn saxo_is_compared_on_the_normalized_account_not_the_raw_cell() {
    for suffix in ["EUR", "USD", "CAD"] {
        let raw = format!("{SAXO_DEPOT}{suffix}");

        let error = import(&SaxoDouble, &saxo_account(&raw), &saxo_fixture())
            .expect_err("a raw sub-account is not the Depot");

        assert_eq!(
            error,
            ImportError::Refused {
                grounds: vec![Ground::AccountMismatch {
                    file: SAXO_DEPOT.to_owned(),
                    target: raw,
                }],
            }
        );
    }
}

/// A Trade Republic file states no account, so it is not checked and imports into whichever
/// account the caller names [IMP-003] (DEC-075).
#[test]
fn a_trade_republic_file_is_not_checked_and_imports_into_the_named_account() {
    let content = fixture("trade-republic/transactions_2024-01-01_2024-12-31.csv");

    for id in ["DE0001", "any other id"] {
        let import = import(
            &TradeRepublicDouble,
            &Account::new("Trade Republic", id),
            &content,
        )
        .unwrap_or_else(|error| panic!("{id}: an unchecked file is not refused: {error}"));

        assert!(!import.stored().is_empty());
    }
}

/// A file carrying rows from more than one account is refused outright, naming them, even when
/// one of them is the target [IMP-003].
#[test]
fn a_file_carrying_two_accounts_is_refused_outright() {
    let content = "id,date,account\n\
                   a,2024-01-02,40100/9000001\n\
                   b,2024-01-03,40100/9000002\n\
                   c,2024-01-04,40100/9000001\n";

    let error = import(
        &AccountDouble::new(),
        &saxo_account("40100/9000001"),
        content.as_bytes(),
    )
    .expect_err("two accounts in one file are refused");

    assert_eq!(
        error,
        ImportError::Refused {
            grounds: vec![Ground::MultipleAccounts {
                accounts: vec!["40100/9000001".to_owned(), "40100/9000002".to_owned()],
            }],
        }
    );
    assert_eq!(
        error.to_string(),
        "the file carries rows from more than one account: 40100/9000001, 40100/9000002"
    );
}

/// A row whose account cannot be read, in a format that states one, is a failed row: the value
/// the import reads from it cannot be read [SRV-058], and the guard cannot vouch for a row it
/// did not see [IMP-003].
#[test]
fn a_row_whose_account_cannot_be_read_is_a_failed_row() {
    let content = "id,date,account\n\
                   a,2024-01-02,40100/9000001\n\
                   b,2024-01-03,\n";

    let error = import(
        &AccountDouble::new(),
        &saxo_account("40100/9000001"),
        content.as_bytes(),
    )
    .expect_err("an unreadable account refuses the file");

    assert_eq!(
        error,
        ImportError::Refused {
            grounds: vec![Ground::FailedRows {
                failures: vec![RowFailure {
                    position: 1,
                    error: RowError::new("no account"),
                }],
            }],
        }
    );
}
