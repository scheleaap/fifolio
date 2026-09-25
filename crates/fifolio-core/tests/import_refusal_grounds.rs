//! Integration layer [TST-003]: one refusal reports every ground an import is refused on
//! [SRV-059] (DEC-088), driven through the public `Importer` trait by a test-double format over
//! synthetic files.
//!
//! The three grounds are trade dates in more than one year [IMP-002], a file account other than
//! the target or several accounts in one file [IMP-003], and failed rows [SRV-058]. Each pair and
//! all three together are refused with every ground named, and every failed row by position.
//! A refusal is an `Err`, so no `Import` exists whose records could be stored. No test reaches
//! the network.

use chrono::NaiveDate;

use fifolio_core::entities::{Account, SourceFormat};
use fifolio_core::import::reader::{DelimitedReader, RowReader, SourceRow};
use fifolio_core::import::{
    Ground, ImportError, Importer, RowClassification, RowError, RowFailure, RowIdentity, import,
};
use fifolio_core::ordering::{FileDirection, RowOrderingKey};

const TARGET: &str = "40100/9000001";
const OTHER: &str = "40100/9000002";

/// A format whose rows are `id,date,account`, every row stored. A blank `id` cannot be
/// identified, an unparseable `date` has no ordering key and a blank `account` cannot be read,
/// which gives each value the import reads its own way to fail.
struct GroundsDouble {
    reader: DelimitedReader,
}

impl Importer for GroundsDouble {
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
        match row.field("id") {
            Some(id) if !id.is_empty() => Ok(RowIdentity::BrokerReference(id.to_owned())),
            _ => Err(RowError::new("no id")),
        }
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
        rows.iter()
            .map(|_| Ok(RowClassification::DerivedAutomatically))
            .collect()
    }
}

fn refusal(content: &str) -> ImportError {
    let double = GroundsDouble {
        reader: DelimitedReader::comma(),
    };
    import(&double, &Account::new("Saxo", TARGET), content.as_bytes())
        .expect_err("the file is refused")
}

fn refused(grounds: Vec<Ground>) -> ImportError {
    ImportError::Refused { grounds }
}

fn years(years: &[i32]) -> Ground {
    Ground::MultipleCalendarYears {
        years: years.to_vec(),
    }
}

fn mismatch() -> Ground {
    Ground::AccountMismatch {
        file: OTHER.to_owned(),
        target: TARGET.to_owned(),
    }
}

fn two_accounts() -> Ground {
    Ground::MultipleAccounts {
        accounts: vec![TARGET.to_owned(), OTHER.to_owned()],
    }
}

fn failed(rows: &[(usize, &str)]) -> Ground {
    Ground::FailedRows {
        failures: rows
            .iter()
            .map(|(position, reason)| RowFailure {
                position: *position,
                error: RowError::new(*reason),
            })
            .collect(),
    }
}

/// Two years and a file naming another account: both grounds, neither guard cutting the other
/// short [SRV-059], [IMP-002], [IMP-003].
#[test]
fn two_years_and_another_account_are_both_reported() {
    let content = format!(
        "id,date,account\n\
         a,2023-12-31,{OTHER}\n\
         b,2024-01-02,{OTHER}\n"
    );

    assert_eq!(
        refusal(&content),
        refused(vec![years(&[2023, 2024]), mismatch()])
    );
}

/// Two years and two accounts in one file: both grounds [SRV-059], [IMP-002], [IMP-003].
#[test]
fn two_years_and_two_accounts_are_both_reported() {
    let content = format!(
        "id,date,account\n\
         a,2023-12-31,{TARGET}\n\
         b,2024-01-02,{OTHER}\n"
    );

    assert_eq!(
        refusal(&content),
        refused(vec![years(&[2023, 2024]), two_accounts()])
    );
}

/// Two years and failed rows: the defect DEC-088 names, where the years alone were reported.
/// Every failed row is named, whichever value it failed on; the row with no trade date adds no
/// year [SRV-059], [IMP-002], [SRV-058].
#[test]
fn two_years_and_failed_rows_are_both_reported() {
    let content = format!(
        "id,date,account\n\
         a,2023-12-31,{TARGET}\n\
         ,2024-01-02,{TARGET}\n\
         c,not-a-date,{TARGET}\n\
         d,2024-01-03,\n"
    );

    assert_eq!(
        refusal(&content),
        refused(vec![
            years(&[2023, 2024]),
            failed(&[
                (1, "no id"),
                (2, "not-a-date is not a date"),
                (3, "no account"),
            ]),
        ])
    );
}

/// Another account and failed rows: both grounds, every failed row named [SRV-059], [IMP-003],
/// [SRV-058].
#[test]
fn another_account_and_failed_rows_are_both_reported() {
    let content = format!(
        "id,date,account\n\
         a,2024-01-02,{OTHER}\n\
         ,2024-01-03,{OTHER}\n\
         c,2024-01-04,\n"
    );

    assert_eq!(
        refusal(&content),
        refused(vec![mismatch(), failed(&[(1, "no id"), (2, "no account")])])
    );
}

/// Two accounts and failed rows: both grounds, every failed row named [SRV-059], [IMP-003],
/// [SRV-058].
#[test]
fn two_accounts_and_failed_rows_are_both_reported() {
    let content = format!(
        "id,date,account\n\
         a,2024-01-02,{TARGET}\n\
         b,2024-01-03,{OTHER}\n\
         c,,{OTHER}\n\
         ,2024-01-05,{TARGET}\n"
    );

    assert_eq!(
        refusal(&content),
        refused(vec![
            two_accounts(),
            failed(&[(2, " is not a date"), (3, "no id")]),
        ])
    );
}

/// All three grounds, with the account ground a mismatch [SRV-059], [IMP-002], [IMP-003],
/// [SRV-058].
#[test]
fn two_years_another_account_and_failed_rows_are_all_reported() {
    let content = format!(
        "id,date,account\n\
         a,2023-12-31,{OTHER}\n\
         b,2024-01-02,{OTHER}\n\
         ,2024-01-03,{OTHER}\n\
         d,2024-13-01,{OTHER}\n\
         e,2024-01-05,\n"
    );

    assert_eq!(
        refusal(&content),
        refused(vec![
            years(&[2023, 2024]),
            mismatch(),
            failed(&[
                (2, "no id"),
                (3, "2024-13-01 is not a date"),
                (4, "no account"),
            ]),
        ])
    );
}

/// All three grounds, with the account ground two accounts; the message names every ground and
/// every failed row, so a caller reading only the text learns everything to fix [SRV-059],
/// [IMP-002], [IMP-003], [SRV-058].
#[test]
fn two_years_two_accounts_and_failed_rows_are_all_reported() {
    let content = format!(
        "id,date,account\n\
         a,2023-12-31,{TARGET}\n\
         b,2024-01-02,{OTHER}\n\
         ,2024-01-03,{TARGET}\n\
         d,2024-01-04,\n"
    );

    let error = refusal(&content);

    assert_eq!(
        error,
        refused(vec![
            years(&[2023, 2024]),
            two_accounts(),
            failed(&[(2, "no id"), (3, "no account")]),
        ])
    );
    assert_eq!(
        error.to_string(),
        format!(
            "the file carries trade dates in more than one calendar year: 2023, 2024. \
             the file carries rows from more than one account: {TARGET}, {OTHER}. \
             2 rows could not be read: row 2: no id; row 3: no account"
        )
    );
}
