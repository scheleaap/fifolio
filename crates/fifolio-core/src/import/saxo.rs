//! Reading a Saxo NL export: its three sheets, their headers, their dates and their joins.
//!
//! This module is the container half of the Saxo importer. What a column *means* — the account
//! suffix, the identity, the ordering key, the `Acties` classification — is not here.
//!
//! # Three sheets, or no import
//!
//! A Saxo export is one workbook of three sheets, each with one header row: `Transacties`
//! (31 columns, the cash ledger), `_Transacties` (24, the position side) and `Bookings`
//! (21, the components of a cash movement) [IMP-SAXO-001]. A workbook missing any of them is
//! refused rather than imported as the cash ledger alone: the quantities, the traded values and
//! the tax figures are on the two detail sheets, so an import that read only the first would
//! succeed while reading nothing of what it needs.
//!
//! # What a Saxo source record is
//!
//! [`SaxoWorkbook::rows`] answers the `Transacties` rows and only those, so an import of a Saxo
//! file creates **one source record per `Transacties` row**; the detail sheets are joined inputs
//! to the derivation of a transaction, not records of their own. Three things in `design/` force
//! that reading rather than the alternatives — a record folding its counterparts in, or a record
//! per physical row on all three sheets:
//!
//! * a source record is "one parsed row of a broker export" [DOM-007], and its stored rendering
//!   is that row's cells "keyed by column name in sheet column order" [DOM-120]; a record
//!   spanning three sheets has no rendering the specification defines;
//! * every row is ordered on `Transactiedatum` [IMP-SAXO-026], which neither detail sheet
//!   carries — `_Transacties` has `Aangepaste transactiedatum` and `Bookings` `Boekingsdatum` —
//!   and a row whose ordering key cannot be read stops the import;
//! * identity is the first populated of four `Transacties` columns [IMP-SAXO-007], and a detail
//!   row shares those values with the ledger row it belongs to, so records per physical row
//!   would collide by construction.
//!
//! The consequence, recorded here because the next Saxo items meet it: a detail column is
//! reachable while the workbook is open and not from a stored record afterwards. Every rule that
//! reads one — the transferred cost basis [IMP-SAXO-028], the split ratio [IMP-SAXO-040], the
//! quantity and direction [IMP-SAXO-038] — is derived during the import that read the file.
//!
//! # Headers
//!
//! Headers are matched after whitespace normalization [IMP-SAXO-002], so the export's
//! `Bk\u{a0}Record\u{a0}Id`, `Booking\u{a0}Id` and ` Positie-ID` resolve under their ordinary
//! spellings. The rows keep the file's own spelling, because that spelling is part of the stored
//! rendering [DOM-120]; only the *matching* is normalized. A sheet whose normalized header row is
//! not its expected one is refused naming what is missing and what is unexpected, which is what
//! an export in another language produces: only Dutch is supported [IMP-SAXO-004].
//!
//! # Blank cells
//!
//! A blank cell arrives in two shapes — a zero-length shared string, as Saxo writes it, and an
//! empty cell, which is what the fixture writer produces — and both read as an empty field
//! [TST-030]. A column the row does not carry at all is `None` instead, and the two are not the
//! same: an empty `Transactie-ID` falls through to the next join key, an absent one is a sheet
//! that is not the sheet we think it is.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{Days, NaiveDate};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;
use thiserror::Error;

use super::reader::{ReadError, SheetRows, SourceRow, SpreadsheetReader};

/// One of the three sheets a Saxo export carries [IMP-SAXO-001].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Sheet {
    /// The cash ledger: one row per booked movement, and one source record per row.
    Transacties,
    /// The position side: signed quantity, price, traded value, direction. Its name in the file
    /// starts with an underscore.
    Detail,
    /// The components a cash movement decomposes into, and the tax figures.
    Bookings,
}

impl Sheet {
    /// The three sheets, in the order the export carries them.
    pub const ALL: [Self; 3] = [Self::Transacties, Self::Detail, Self::Bookings];

    /// The sheet's name in the workbook [IMP-SAXO-001].
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Transacties => "Transacties",
            Self::Detail => "_Transacties",
            Self::Bookings => "Bookings",
        }
    }

    /// The sheet's header row, as the export spells it [IMP-SAXO-002].
    ///
    /// Held verbatim, non-breaking spaces and the leading space included, so that the file this
    /// reads is legible from the source; matching normalizes both sides.
    #[must_use]
    pub fn headers(self) -> &'static [&'static str] {
        match self {
            Self::Transacties => &TRANSACTIES_HEADERS,
            Self::Detail => &DETAIL_HEADERS,
            Self::Bookings => &BOOKINGS_HEADERS,
        }
    }
}

/// The 31 `Transacties` headers [IMP-SAXO-001], [IMP-SAXO-002].
const TRANSACTIES_HEADERS: [&str; 31] = [
    "Klant-id",
    "Transactiedatum",
    "Valutadatum",
    "Rekening-ID",
    "Transactie-ID",
    " Positie-ID",
    "Corporate action-Id",
    "Bk\u{a0}Record\u{a0}Id",
    "Booking\u{a0}Id",
    "Transactietype",
    "Acties",
    "Aantal",
    "Valuta",
    "Boekingsbedrag",
    "_Valuta",
    "Omrekeningskoers",
    "Omwisselkosten",
    "Totale kosten",
    "Gerealiseerd rendement",
    "IBAN",
    "Naam IBAN-eigenaar",
    "Opmerking",
    "Reden van correctie",
    "Instrument",
    "Instrumentsymbool",
    "Instrument ISIN",
    "Instrumentvaluta",
    "Type",
    "Uitwisselingsbeschrijving",
    "Van derivaat",
    "Onderliggend instrumenttype",
];

/// The 24 `_Transacties` headers [IMP-SAXO-001], [IMP-SAXO-002].
const DETAIL_HEADERS: [&str; 24] = [
    "Rekening-ID",
    "Transactie-ID",
    "Bk\u{a0}Record\u{a0}Id",
    "Booking\u{a0}Id",
    "Corporate action-Id",
    "Acties",
    "Order-ID",
    "Aangepaste transactiedatum",
    "Uitvoeringsdatum transactie",
    "Trade\u{a0}Event\u{a0}Type",
    "Trade Type",
    "Openen/sluiten",
    "Traded\u{a0}Quantity",
    "Prijs",
    "Verhandelde waarde",
    "Spreadkosten",
    "Van derivaat",
    "Onderliggend instrumenttype",
    "Instrument",
    "Instrumentsymbool",
    "Instrument ISIN",
    "Instrumentvaluta",
    "Type",
    "Uitwisselingsbeschrijving",
];

/// The 21 `Bookings` headers [IMP-SAXO-001], [IMP-SAXO-002].
const BOOKINGS_HEADERS: [&str; 21] = [
    "Rekening-ID",
    "Transactie-ID",
    "Bk\u{a0}Record\u{a0}Id",
    "Booking\u{a0}Id",
    "Corporate action-Id",
    "Acties",
    "Amount Type",
    "Amount\u{a0}Type\u{a0}Id",
    "Boekingsbedrag",
    "Omwisselkosten",
    "Omrekeningskoers",
    "Boekingsdatum",
    "Ex-datum",
    "Boekdatum",
    "In aanmerking komend aantal",
    "Dividend per aandeel",
    "Tax\u{a0}Percentage",
    "Instrument",
    "Instrumentsymbool",
    "Instrument ISIN",
    "Instrumentvaluta",
];

/// A `Transacties` row joins its `_Transacties` counterpart on `Transactie-ID`, else on
/// `Corporate action-Id` [IMP-SAXO-037].
///
/// Written with ordinary spaces here and on [`BOOKING_JOIN_KEYS`]: the file spells two of these
/// with non-breaking spaces, and the lookup normalizes [IMP-SAXO-002].
const DETAIL_JOIN_KEYS: [&str; 2] = ["Transactie-ID", "Corporate action-Id"];

/// A `Transacties` row joins its `Bookings` components on `Bk Record Id`, then `Booking Id`,
/// else on `Corporate action-Id` [IMP-SAXO-037].
const BOOKING_JOIN_KEYS: [&str; 3] = ["Bk Record Id", "Booking Id", "Corporate action-Id"];

/// The epoch Excel counts serial dates from, on the 1900 system Saxo writes.
const EXCEL_EPOCH: (i32, u32, u32) = (1899, 12, 30);

/// The lowest serial this reads as a date: 1900-03-01.
///
/// Excel's 1900 date system includes the fictitious 1900-02-29, so serials up to 60 are a day
/// out under any real calendar. A broker export does not carry one — Saxo's earliest is 2021 —
/// so such a serial is refused rather than silently shifted by a day.
const FIRST_UNAMBIGUOUS_SERIAL: u64 = 61;

/// Why a Saxo export could not be read.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SaxoError {
    /// The container, or a sheet the export must carry, or a cell with no defined rendering.
    #[error("the Saxo export could not be read: {0}")]
    Container(#[from] ReadError),
    /// The header row is not the sheet's Dutch one. Only Dutch is supported [IMP-SAXO-004], and
    /// a header set that is not recognized is refused rather than matched column by column,
    /// which would map a value under a name that means something else.
    #[error(
        "the {sheet} sheet of the Saxo export does not carry the expected Dutch headers \
         (missing {missing:?}, unexpected {unexpected:?}); only a Dutch export is supported"
    )]
    Headers {
        sheet: &'static str,
        missing: Vec<String>,
        unexpected: Vec<String>,
    },
    /// A detail row belonging to no `Transacties` row. It is a refusal rather than a silent
    /// absence: that row carries a quantity or a tax figure for some booking, and an import
    /// that dropped it would be short by exactly what it dropped [IMP-SAXO-037].
    #[error("{sheet} row {row} of the Saxo export joins no Transacties row")]
    Unjoined { sheet: &'static str, row: usize },
    /// A date column holding something that is not an Excel serial number [IMP-SAXO-003].
    #[error("the column {header} holds {value:?}, which is not an Excel serial date")]
    NotADate { header: String, value: String },
    /// A column asked for that the row does not carry, which a header check makes unreachable
    /// for a column of the sheet and reachable for a caller naming the wrong sheet's.
    #[error("the row carries no column {header}")]
    MissingColumn { header: String },
}

/// A Saxo export: its three sheets, read and joined [IMP-SAXO-001], [IMP-SAXO-037].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaxoWorkbook {
    transacties: Vec<SourceRow>,
    detail: Vec<SourceRow>,
    bookings: Vec<SourceRow>,
    /// Per `Transacties` row, the `_Transacties` rows it joins, by index.
    detail_joins: Vec<Vec<usize>>,
    /// Per `Transacties` row, the `Bookings` rows it joins, by index.
    booking_joins: Vec<Vec<usize>>,
}

impl SaxoWorkbook {
    /// Reads `content` as a Saxo NL export.
    ///
    /// # Errors
    ///
    /// When the workbook does not carry all three sheets, when a sheet's headers are not its
    /// Dutch ones, or when a detail row joins no `Transacties` row.
    pub fn read(content: &[u8]) -> Result<Self, SaxoError> {
        let names: Vec<&str> = Sheet::ALL.iter().map(|sheet| sheet.name()).collect();
        let read = SpreadsheetReader.sheets(content, &names)?;
        for (sheet, rows) in Sheet::ALL.into_iter().zip(&read) {
            check_headers(sheet, rows)?;
        }

        let [transacties, detail, bookings]: [SheetRows; 3] = read
            .try_into()
            .unwrap_or_else(|_| unreachable!("one sheet was read per name, and there are three"));
        let (transacties, detail, bookings) = (transacties.rows, detail.rows, bookings.rows);

        let detail_joins = joins(&transacties, &detail, &DETAIL_JOIN_KEYS);
        let booking_joins = joins(&transacties, &bookings, &BOOKING_JOIN_KEYS);
        check_claimed(Sheet::Detail, detail.len(), &detail_joins)?;
        check_claimed(Sheet::Bookings, bookings.len(), &booking_joins)?;

        Ok(Self {
            transacties,
            detail,
            bookings,
            detail_joins,
            booking_joins,
        })
    }

    /// The rows an import turns into source records: the `Transacties` rows, in file order.
    #[must_use]
    pub fn rows(&self) -> &[SourceRow] {
        &self.transacties
    }

    /// The rows of one sheet, in file order.
    #[must_use]
    pub fn sheet(&self, sheet: Sheet) -> &[SourceRow] {
        match sheet {
            Sheet::Transacties => &self.transacties,
            Sheet::Detail => &self.detail,
            Sheet::Bookings => &self.bookings,
        }
    }

    /// The `_Transacties` rows the `Transacties` row at `index` joins [IMP-SAXO-037].
    ///
    /// Empty for a row with no position side, which is most of the ledger. A corporate action's
    /// legs are its `_Transacties` rows under one `Corporate action-Id`, **however many there
    /// are** — the sample carries groups of one, two and three — and every one of them is
    /// answered here [IMP-SAXO-037]. A leg count is never assumed and a side is never indexed:
    /// the 2023 Philips stock dividend is two `Gekocht` legs and the DeVolksbank tender three,
    /// one of which is the `Terugboeking` reversing another [DEC-071]. Summing a side and
    /// cancelling an opposing pair is the reader's caller's work [IMP-SAXO-044],
    /// [IMP-SAXO-045], not this join's.
    ///
    /// # Panics
    ///
    /// When `index` is not a row of the `Transacties` sheet.
    pub fn detail_of(&self, index: usize) -> impl Iterator<Item = &SourceRow> {
        self.detail_joins[index]
            .iter()
            .map(|joined| &self.detail[*joined])
    }

    /// The `Bookings` rows the `Transacties` row at `index` joins: the components that movement
    /// decomposes into [IMP-SAXO-037].
    ///
    /// # Panics
    ///
    /// When `index` is not a row of the `Transacties` sheet.
    pub fn bookings_of(&self, index: usize) -> impl Iterator<Item = &SourceRow> {
        self.booking_joins[index]
            .iter()
            .map(|joined| &self.bookings[*joined])
    }
}

/// One column of `row`, matched after whitespace normalization [IMP-SAXO-002].
///
/// `None` when the row carries no such column; `Some("")` when it carries it blank, in either of
/// the two shapes a blank arrives in [TST-030].
#[must_use]
pub fn field<'a>(row: &'a SourceRow, header: &str) -> Option<&'a str> {
    let wanted = normalize(header);
    row.columns()
        .iter()
        .find(|(name, _)| normalize(name) == wanted)
        .map(|(_, value)| value.as_str())
}

/// The date a date column holds, the Excel serial number converted [IMP-SAXO-003].
///
/// A serial carrying a time — `_Transacties` stamps its execution time — is truncated to its
/// day, the date being what the domain orders and reports on.
///
/// # Errors
///
/// When the row carries no such column, or the column holds something that is not a serial
/// number on or after 1900-03-01.
pub fn date(row: &SourceRow, header: &str) -> Result<NaiveDate, SaxoError> {
    let value = field(row, header).ok_or_else(|| SaxoError::MissingColumn {
        header: header.to_owned(),
    })?;
    date_of_serial(value).ok_or_else(|| SaxoError::NotADate {
        header: header.to_owned(),
        value: value.to_owned(),
    })
}

/// A header with every run of whitespace collapsed to one space and the ends trimmed, which is
/// what the export's non-breaking spaces and its leading space need [IMP-SAXO-002].
fn normalize(header: &str) -> String {
    // `char::is_whitespace` is the Unicode `White_Space` property, so U+00A0 is whitespace here.
    header.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The date an Excel serial number stands for, or `None` when the text is not one.
fn date_of_serial(serial: &str) -> Option<NaiveDate> {
    let days = serial.parse::<Decimal>().ok()?.trunc().to_u64()?;
    if days < FIRST_UNAMBIGUOUS_SERIAL {
        return None;
    }
    let (year, month, day) = EXCEL_EPOCH;
    NaiveDate::from_ymd_opt(year, month, day)?.checked_add_days(Days::new(days))
}

/// Refuses a sheet whose header row is not its Dutch one [IMP-SAXO-004].
///
/// Compared as sorted lists rather than as sets, so a sheet repeating one header in place of
/// another is caught too.
fn check_headers(sheet: Sheet, read: &SheetRows) -> Result<(), SaxoError> {
    let sorted = |headers: &mut Vec<String>| headers.sort();
    let mut found: Vec<String> = read.headers.iter().map(|name| normalize(name)).collect();
    let mut expected: Vec<String> = sheet.headers().iter().map(|name| normalize(name)).collect();
    sorted(&mut found);
    sorted(&mut expected);
    if found == expected {
        return Ok(());
    }

    let missing = difference(&expected, &found);
    let unexpected = difference(&found, &expected);
    Err(SaxoError::Headers {
        sheet: sheet.name(),
        missing,
        unexpected,
    })
}

/// The members of `left` that `right` does not carry.
fn difference(left: &[String], right: &[String]) -> Vec<String> {
    let right: BTreeSet<&String> = right.iter().collect();
    left.iter()
        .filter(|name| !right.contains(name))
        .cloned()
        .collect()
}

/// For each row of `rows`, the indexes into `target` it joins: the first of `keys` the row
/// populates **and** that resolves, so an unpopulated id falls through to the next [IMP-SAXO-037].
fn joins(rows: &[SourceRow], target: &[SourceRow], keys: &[&str]) -> Vec<Vec<usize>> {
    let indexes: Vec<BTreeMap<&str, Vec<usize>>> =
        keys.iter().map(|key| index_by(target, key)).collect();

    rows.iter()
        .map(|row| {
            keys.iter()
                .zip(&indexes)
                .find_map(|(key, index)| index.get(field(row, key)?).cloned())
                .unwrap_or_default()
        })
        .collect()
}

/// The rows of `target` carrying each non-empty value of `key`, by that value.
fn index_by<'a>(target: &'a [SourceRow], key: &str) -> BTreeMap<&'a str, Vec<usize>> {
    target.iter().enumerate().fold(
        BTreeMap::new(),
        |mut indexed: BTreeMap<&'a str, Vec<usize>>, (index, row)| {
            match field(row, key) {
                Some(value) if !value.is_empty() => indexed.entry(value).or_default().push(index),
                _ => {}
            }
            indexed
        },
    )
}

/// Refuses a detail sheet holding a row no `Transacties` row joins [IMP-SAXO-037].
fn check_claimed(sheet: Sheet, rows: usize, joins: &[Vec<usize>]) -> Result<(), SaxoError> {
    let claimed: BTreeSet<usize> = joins.iter().flatten().copied().collect();
    (0..rows)
        .find(|index| !claimed.contains(index))
        .map_or(Ok(()), |index| {
            Err(SaxoError::Unjoined {
                sheet: sheet.name(),
                // The file's own row number, its header row counted, so the refusal names what
                // a spreadsheet shows.
                row: index + 2,
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::test_workbook::{Cell, SheetContent, workbook};

    /// A `Transacties` row as the export writes one, with only the columns a test varies filled
    /// in; the rest are blank cells, which is what most of the ledger holds anyway.
    fn transacties_row(values: &[(&str, Cell)]) -> Vec<Cell> {
        row(Sheet::Transacties, values)
    }

    fn row(sheet: Sheet, values: &[(&str, Cell)]) -> Vec<Cell> {
        sheet
            .headers()
            .iter()
            .map(|header| {
                values
                    .iter()
                    .find(|(name, _)| normalize(name) == normalize(header))
                    .map_or(Cell::Blank, |(_, cell)| cell.clone())
            })
            .collect()
    }

    /// A workbook of the three sheets, each with its own header row and the given data rows.
    fn saxo(
        transacties: Vec<Vec<Cell>>,
        detail: Vec<Vec<Cell>>,
        bookings: Vec<Vec<Cell>>,
    ) -> Vec<u8> {
        workbook(&[
            sheet_content(Sheet::Transacties, transacties),
            sheet_content(Sheet::Detail, detail),
            sheet_content(Sheet::Bookings, bookings),
        ])
    }

    fn sheet_content(sheet: Sheet, rows: Vec<Vec<Cell>>) -> SheetContent {
        SheetContent {
            name: sheet.name().to_owned(),
            headers: sheet
                .headers()
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
            rows,
        }
    }

    /// One buy, its position counterpart and its one booking component: the smallest export
    /// that joins on both sides.
    fn one_of_each() -> Vec<u8> {
        saxo(
            vec![transacties_row(&[
                ("Transactie-ID", Cell::text("3000000001")),
                ("Bk Record Id", Cell::text("3000000002")),
                ("Transactiedatum", Cell::Number("45208".to_owned())),
                ("Acties", Cell::text("Koop 40 @ 5.75 USD")),
            ])],
            vec![row(
                Sheet::Detail,
                &[
                    ("Transactie-ID", Cell::text("3000000001")),
                    ("Trade Event Type", Cell::text("Gekocht")),
                ],
            )],
            vec![row(
                Sheet::Bookings,
                &[
                    ("Bk Record Id", Cell::text("3000000002")),
                    ("Amount Type", Cell::text("Trade - Handelswaarde")),
                ],
            )],
        )
    }

    /// The three sheets are read, each with its one header row [IMP-SAXO-001].
    #[test]
    fn all_three_sheets_are_read() {
        let export = SaxoWorkbook::read(&one_of_each()).expect("the sample reads");

        assert_eq!(export.sheet(Sheet::Transacties).len(), 1);
        assert_eq!(export.sheet(Sheet::Detail).len(), 1);
        assert_eq!(export.sheet(Sheet::Bookings).len(), 1);
        assert_eq!(export.rows(), export.sheet(Sheet::Transacties));
    }

    /// A workbook missing a sheet is refused rather than imported as the cash ledger alone
    /// [IMP-SAXO-001].
    #[test]
    fn a_workbook_missing_a_sheet_is_refused() {
        for absent in Sheet::ALL {
            let content = workbook(
                &Sheet::ALL
                    .into_iter()
                    .filter(|sheet| *sheet != absent)
                    .map(|sheet| sheet_content(sheet, Vec::new()))
                    .collect::<Vec<_>>(),
            );

            let error = SaxoWorkbook::read(&content).expect_err("a missing sheet is refused");

            assert_eq!(
                error,
                SaxoError::Container(ReadError::MissingSheet {
                    name: absent.name().to_owned()
                })
            );
        }
    }

    /// The headers the export spells with non-breaking spaces and with a leading space resolve
    /// under their ordinary spellings [IMP-SAXO-002].
    #[test]
    fn a_header_resolves_after_whitespace_normalization() {
        let export = SaxoWorkbook::read(&one_of_each()).expect("the sample reads");
        let row = &export.rows()[0];

        assert_eq!(field(row, "Bk Record Id"), Some("3000000002"));
        assert_eq!(field(row, "Bk\u{a0}Record\u{a0}Id"), Some("3000000002"));
        assert_eq!(field(row, "Positie-ID"), Some(""));
        assert_eq!(field(row, " Positie-ID"), Some(""));
        assert_eq!(field(row, "Booking Id"), Some(""));
        assert_eq!(field(row, "Booking\u{a0}Id"), Some(""));
        assert_eq!(field(row, "Traded Quantity"), None, "a _Transacties column");
    }

    /// The row keeps the file's own header spelling: normalization is for matching, and the
    /// stored rendering is the file's [DOM-120], [IMP-SAXO-002].
    #[test]
    fn the_stored_rendering_keeps_the_file_spelling() {
        let export = SaxoWorkbook::read(&one_of_each()).expect("the sample reads");

        assert!(
            export.rows()[0]
                .raw()
                .contains("\"Bk\u{a0}Record\u{a0}Id\""),
            "{}",
            export.rows()[0].raw()
        );
        assert!(
            export.rows()[0].raw().contains("\" Positie-ID\""),
            "{}",
            export.rows()[0].raw()
        );
    }

    /// An export in another language is refused naming what it carries instead, rather than
    /// matched column by column [IMP-SAXO-004].
    #[test]
    fn a_non_dutch_header_set_is_refused() {
        let english: Vec<String> = Sheet::Transacties
            .headers()
            .iter()
            .enumerate()
            .map(|(index, header)| {
                if index == 1 {
                    "Trade Date".to_owned()
                } else {
                    (*header).to_owned()
                }
            })
            .collect();
        let content = workbook(&[
            SheetContent {
                name: Sheet::Transacties.name().to_owned(),
                headers: english,
                rows: Vec::new(),
            },
            sheet_content(Sheet::Detail, Vec::new()),
            sheet_content(Sheet::Bookings, Vec::new()),
        ]);

        let error = SaxoWorkbook::read(&content).expect_err("a foreign header set is refused");

        assert_eq!(
            error,
            SaxoError::Headers {
                sheet: "Transacties",
                missing: vec!["Transactiedatum".to_owned()],
                unexpected: vec!["Trade Date".to_owned()],
            }
        );
        assert!(
            error
                .to_string()
                .contains("only a Dutch export is supported"),
            "{error}"
        );
    }

    /// The header check runs on the header row, so it holds for a sheet carrying no data row:
    /// a wrong-language export with an empty sheet is still the wrong export [IMP-SAXO-004].
    #[test]
    fn a_sheet_with_no_rows_is_still_header_checked() {
        let content = workbook(&[
            sheet_content(Sheet::Transacties, Vec::new()),
            sheet_content(Sheet::Detail, Vec::new()),
            SheetContent {
                name: Sheet::Bookings.name().to_owned(),
                headers: vec!["Account ID".to_owned()],
                rows: Vec::new(),
            },
        ]);

        let error = SaxoWorkbook::read(&content).expect_err("a foreign header set is refused");

        assert!(
            matches!(
                error,
                SaxoError::Headers {
                    sheet: "Bookings",
                    ..
                }
            ),
            "{error:?}"
        );
    }

    /// A blank cell reads as an empty field in both the shapes it arrives in: the zero-length
    /// shared string Saxo writes, and the empty cell the fixture writer produces [TST-030].
    #[test]
    fn a_blank_cell_reads_empty_in_both_its_shapes() {
        let shapes = |positie: Cell| {
            let content = saxo(
                vec![transacties_row(&[
                    ("Transactie-ID", Cell::text("3000000001")),
                    (" Positie-ID", positie),
                ])],
                vec![row(
                    Sheet::Detail,
                    &[("Transactie-ID", Cell::text("3000000001"))],
                )],
                Vec::new(),
            );
            let export = SaxoWorkbook::read(&content).expect("the sample reads");
            export.rows()[0].clone()
        };

        let shared_string = shapes(Cell::Text(String::new()));
        let empty_cell = shapes(Cell::Blank);

        assert_eq!(field(&shared_string, " Positie-ID"), Some(""));
        assert_eq!(field(&empty_cell, " Positie-ID"), Some(""));
        assert_eq!(
            shared_string, empty_cell,
            "the two shapes of a blank must read as one row"
        );
    }

    /// A `Transacties` row joins its counterpart on `Transactie-ID` and its components on
    /// `Bk Record Id` [IMP-SAXO-037].
    #[test]
    fn a_row_joins_its_counterpart_and_its_components() {
        let export = SaxoWorkbook::read(&one_of_each()).expect("the sample reads");

        let detail: Vec<&str> = export
            .detail_of(0)
            .filter_map(|row| field(row, "Trade Event Type"))
            .collect();
        let components: Vec<&str> = export
            .bookings_of(0)
            .filter_map(|row| field(row, "Amount Type"))
            .collect();

        assert_eq!(detail, ["Gekocht"]);
        assert_eq!(components, ["Trade - Handelswaarde"]);
    }

    /// A corporate action joins on `Corporate action-Id` where it carries no transaction id, and
    /// answers every leg under that id, however many there are [IMP-SAXO-037], [DEC-071].
    ///
    /// The three shapes are the sample's: one leg, the `Verkocht` / `Gekocht` pair, and the
    /// three of the DeVolksbank tender, whose middle leg is the `Terugboeking` reversing the
    /// first. A reader answering two would lose a share of the two-`Gekocht` dividend.
    #[test]
    fn a_corporate_action_answers_every_leg_under_its_group_id() {
        let leg = |event: &str| {
            row(
                Sheet::Detail,
                &[
                    ("Corporate action-Id", Cell::text("8000000001")),
                    ("Trade Event Type", Cell::text(event)),
                ],
            )
        };
        let joined = |events: &[&str]| {
            let content = saxo(
                vec![transacties_row(&[
                    ("Corporate action-Id", Cell::text("8000000001")),
                    ("Acties", Cell::text("Fusie")),
                ])],
                events.iter().map(|event| leg(event)).collect(),
                Vec::new(),
            );
            let export = SaxoWorkbook::read(&content).expect("the sample reads");
            export
                .detail_of(0)
                .filter_map(|row| field(row, "Trade\u{a0}Event\u{a0}Type"))
                .map(str::to_owned)
                .collect::<Vec<String>>()
        };

        for shape in [
            vec!["Gekocht"],
            vec!["Verkocht", "Gekocht"],
            vec!["Gekocht", "Gekocht"],
            vec!["Verkocht", "Gekocht", "Verkocht"],
        ] {
            assert_eq!(joined(&shape), shape, "a leg of the group went unanswered");
        }
    }

    /// A detail row that joins no `Transacties` row is a refusal, not a silent absence: it
    /// carries a quantity or a tax figure for a booking, so dropping it loses exactly that
    /// [IMP-SAXO-037].
    #[test]
    fn a_detail_row_joining_nothing_is_refused() {
        let content = saxo(
            vec![transacties_row(&[(
                "Transactie-ID",
                Cell::text("3000000001"),
            )])],
            vec![
                row(
                    Sheet::Detail,
                    &[("Transactie-ID", Cell::text("3000000001"))],
                ),
                row(
                    Sheet::Detail,
                    &[("Transactie-ID", Cell::text("9999999999"))],
                ),
            ],
            Vec::new(),
        );

        let error = SaxoWorkbook::read(&content).expect_err("an orphan detail row is refused");

        assert_eq!(
            error,
            SaxoError::Unjoined {
                sheet: "_Transacties",
                row: 3,
            }
        );
    }

    /// The same refusal on the `Bookings` side, and on its own join keys [IMP-SAXO-037].
    #[test]
    fn a_bookings_row_joining_nothing_is_refused() {
        let content = saxo(
            vec![transacties_row(&[(
                "Bk Record Id",
                Cell::text("3000000002"),
            )])],
            Vec::new(),
            vec![row(
                Sheet::Bookings,
                &[("Booking Id", Cell::text("4000000001"))],
            )],
        );

        let error = SaxoWorkbook::read(&content).expect_err("an orphan booking is refused");

        assert_eq!(
            error,
            SaxoError::Unjoined {
                sheet: "Bookings",
                row: 2,
            }
        );
    }

    /// An empty id falls through to the next join key rather than matching every other row that
    /// leaves it empty [IMP-SAXO-037].
    #[test]
    fn an_empty_id_falls_through_to_the_next_key() {
        let content = saxo(
            vec![transacties_row(&[
                ("Transactie-ID", Cell::Text(String::new())),
                ("Corporate action-Id", Cell::text("8000000001")),
            ])],
            vec![row(
                Sheet::Detail,
                &[
                    ("Transactie-ID", Cell::Text(String::new())),
                    ("Corporate action-Id", Cell::text("8000000001")),
                ],
            )],
            Vec::new(),
        );

        let export = SaxoWorkbook::read(&content).expect("the sample reads");

        assert_eq!(export.detail_of(0).count(), 1);
    }

    /// An Excel serial number is converted to the date it stands for [IMP-SAXO-003], a serial
    /// carrying a time truncating to its day.
    #[test]
    fn an_excel_serial_converts_to_its_date() {
        let export = SaxoWorkbook::read(&one_of_each()).expect("the sample reads");

        assert_eq!(
            date(&export.rows()[0], "Transactiedatum"),
            // 45208 is 2023-10-09, which a spreadsheet shows as 09-Oct-2023.
            Ok(NaiveDate::from_ymd_opt(2023, 10, 9).expect("a real date"))
        );
        assert_eq!(
            date_of_serial("45208.5138888889"),
            NaiveDate::from_ymd_opt(2023, 10, 9)
        );
    }

    /// A date column holding text, or a serial from Excel's fictitious start of 1900, is a
    /// refusal rather than a date a day out [IMP-SAXO-003].
    #[test]
    fn a_column_that_is_not_a_serial_date_is_refused() {
        let content = saxo(
            vec![transacties_row(&[(
                "Transactiedatum",
                Cell::text("09-Oct-2023"),
            )])],
            Vec::new(),
            Vec::new(),
        );
        let export = SaxoWorkbook::read(&content).expect("the sample reads");

        assert_eq!(
            date(&export.rows()[0], "Transactiedatum"),
            Err(SaxoError::NotADate {
                header: "Transactiedatum".to_owned(),
                value: "09-Oct-2023".to_owned(),
            })
        );
        assert_eq!(date_of_serial("60"), None, "Excel's fictitious 1900-02-29");
        assert_eq!(date_of_serial("-1"), None);
        assert_eq!(
            date_of_serial("61"),
            NaiveDate::from_ymd_opt(1900, 3, 1),
            "the first serial that is a real date on both calendars"
        );
    }

    /// A column the row does not carry is named rather than read as a missing date.
    #[test]
    fn a_date_column_the_row_does_not_carry_is_named() {
        let export = SaxoWorkbook::read(&one_of_each()).expect("the sample reads");

        assert_eq!(
            date(&export.rows()[0], "Boekingsdatum"),
            Err(SaxoError::MissingColumn {
                header: "Boekingsdatum".to_owned(),
            })
        );
    }
}
