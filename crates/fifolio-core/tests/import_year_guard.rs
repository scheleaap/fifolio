//! Integration layer [TST-003]: `import`'s whole-file calendar-year guard [IMP-001], [IMP-002],
//! driven through the public `Importer` trait by a test-double format over synthetic files.
//!
//! A test double rather than a fixture: reading a Saxo or Trade Republic export for its trade
//! dates needs a format importer, and neither exists yet (FIF-019, FIF-027). The fixture half of
//! the acceptance is deferred to those items. No test reaches the network.

use chrono::NaiveDate;

use fifolio_core::entities::{Account, SourceFormat};
use fifolio_core::import::reader::{DelimitedReader, RowReader, SourceRow};
use fifolio_core::import::{
    Ground, ImportError, Importer, RowClassification, RowError, RowIdentity, import,
};
use fifolio_core::ordering::{FileDirection, RowOrderingKey};

/// A format whose rows are `id,date[,booked]`, every row stored. `booked` stands for the
/// booking timestamp a real format may put in its ordering columns, and is deliberately *not*
/// the trade date.
struct YearDouble {
    reader: DelimitedReader,
}

impl YearDouble {
    fn new() -> Self {
        Self {
            reader: DelimitedReader::comma(),
        }
    }
}

impl Importer for YearDouble {
    fn format(&self) -> SourceFormat {
        SourceFormat::TradeRepublicDeCsv
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
        let booked = row
            .field("booked")
            .and_then(|year| year.parse::<i64>().ok());
        Ok(RowOrderingKey {
            trade_date,
            columns: vec![booked],
        })
    }

    fn classify(&self, rows: &[SourceRow]) -> Vec<Result<RowClassification, RowError>> {
        rows.iter()
            .map(|_| Ok(RowClassification::DerivedAutomatically))
            .collect()
    }
}

fn account() -> Account {
    Account::new("Trade Republic", "DE0001")
}

fn run(content: &str) -> Result<usize, ImportError> {
    import(&YearDouble::new(), &account(), content.as_bytes()).map(|result| result.stored().len())
}

/// A file whose trade dates fall in two calendar years is refused whole, and the refusal names
/// both years [IMP-001].
#[test]
fn a_file_spanning_two_calendar_years_is_refused_naming_the_years() {
    let error = run("id,date\n\
                     a,2023-12-31\n\
                     b,2024-01-02\n")
    .expect_err("a two-year file is refused");

    assert_eq!(
        error,
        ImportError::Refused {
            grounds: vec![Ground::MultipleCalendarYears {
                years: vec![2023, 2024]
            }],
        }
    );
    let message = error.to_string();
    assert!(
        message.contains("2023") && message.contains("2024"),
        "{message}"
    );
}

/// The refusal names every year met, not merely the first two [IMP-001].
#[test]
fn the_refusal_names_every_year_the_file_carries() {
    let error = run("id,date\n\
                     a,2022-06-01\n\
                     b,2024-01-02\n\
                     c,2023-03-04\n\
                     d,2024-12-31\n")
    .expect_err("a three-year file is refused");

    assert_eq!(
        error,
        ImportError::Refused {
            grounds: vec![Ground::MultipleCalendarYears {
                years: vec![2022, 2023, 2024]
            }],
        }
    );
}

/// A partial year is a single year and passes — the first Saxo export runs 2021-11-29 to
/// 2021-12-31 [IMP-001].
#[test]
fn a_partial_calendar_year_is_accepted() {
    assert_eq!(
        run("id,date\n\
             a,2021-11-29\n\
             b,2021-12-31\n"),
        Ok(2)
    );
}

/// A year boundary inside one year, from the file's first day to its last, is still one year
/// [IMP-001].
#[test]
fn a_full_calendar_year_is_accepted() {
    assert_eq!(
        run("id,date\n\
             a,2024-01-01\n\
             b,2024-12-31\n"),
        Ok(2)
    );
}

/// The trade date decides, not a booking timestamp: rows whose ordering columns carry another
/// year import unrefused [IMP-002].
#[test]
fn a_booking_timestamp_in_another_year_does_not_refuse_the_file() {
    assert_eq!(
        run("id,date,booked\n\
             a,2024-12-30,2024\n\
             b,2024-12-31,2025\n"),
        Ok(2)
    );
}
