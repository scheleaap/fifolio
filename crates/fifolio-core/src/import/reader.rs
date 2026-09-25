//! Reading the rows of a broker file, whatever container it arrives in [ARC-023].
//!
//! Saxo exports are XLSX and Trade Republic CSV, so there are two readers; both answer the same
//! trait and both yield the same [`SourceRow`], which is what keeps the importers above them
//! free of container detail.
//!
//! # What a row carries
//!
//! A row is its columns, keyed by the file's own header spelling and **in file column order**,
//! plus the raw content a source record stores [DOM-007]:
//!
//! * a delimited row has a verbatim line and stores it, quotes and all;
//! * a spreadsheet row has none — it is typed cells — so it stores the canonical rendering
//!   [DOM-120] defines: a JSON object of column name to cell value as a string, keys in sheet
//!   column order, no insignificant whitespace, an Excel serial date left as `45208`, an empty
//!   cell `""` and an absent one omitted.
//!
//! # The one place a double is unavoidable
//!
//! calamine exposes a numeric cell only as `f64`, so a monetary or quantity cell is rendered
//! through `Decimal::from_f64` rather than from the file's own digits. Nothing is computed with
//! it — the double is converted once and never used in arithmetic — and the conversion recovers
//! what was written up to the ~15 significant digits a double carries. Beyond that, and for a
//! magnitude outside `Decimal`'s range (refused as an unsupported cell), the rendering is not the
//! file's own text. That is the boundary of ARC-006 here, and it follows from the container
//! library the acceptance names rather than from a choice made in this module.
//!
//! That rendering is part of the on-disk format, not a display: re-reading the same file must
//! reproduce it byte for byte (DEC-065). Two consequences for anyone editing this module. The
//! object is assembled here rather than through [`serde_json::Map`], because that map sorts its
//! keys unless the `preserve_order` feature is on and DOM-120 asks for sheet order; escaping is
//! still `serde_json`'s, which is the part worth borrowing. And a number is rendered from the
//! decimal the file holds rather than from the binary double calamine returns, so `45208` does
//! not come back as `45208.000000000004`.
//!
//! # Empty against absent
//!
//! A cell that exists and holds nothing renders as `""`; a column the row does not reach is
//! omitted. calamine hands back a rectangular range, so the second case is unreachable through
//! [`SpreadsheetReader`] today and the CSV reader refuses a ragged file outright; the rule is
//! nonetheless implemented and unit tested against the rendering directly, because it is a
//! clause of the on-disk format rather than a property of one library's padding.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use calamine::{Data, Reader, Xlsx};
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive as _;
use thiserror::Error;

/// One row of a broker file, as the file holds it.
///
/// The columns are the file's own, in its own order, so nothing here interprets what a value
/// means: that is each format's importer, and this type is what it reads from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRow {
    columns: Vec<(String, String)>,
    raw: String,
}

impl SourceRow {
    /// A row with its columns in file order and the raw content a source record stores.
    #[must_use]
    pub fn new(columns: Vec<(String, String)>, raw: impl Into<String>) -> Self {
        Self {
            columns,
            raw: raw.into(),
        }
    }

    /// One column, by the file's own name for it. `None` when the row does not carry it.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&str> {
        self.columns
            .iter()
            .find(|(column, _)| column == name)
            .map(|(_, value)| value.as_str())
    }

    /// Every column the row carries, in file column order.
    #[must_use]
    pub fn columns(&self) -> &[(String, String)] {
        &self.columns
    }

    /// The row exactly as the file held it, or the canonical rendering where the file holds no
    /// line [DOM-007], [DOM-120].
    #[must_use]
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// The columns as a source record's parsed fields, which are keyed rather than ordered.
    #[must_use]
    pub fn parsed(&self) -> BTreeMap<String, String> {
        self.columns.iter().cloned().collect()
    }
}

/// Why a file could not be read.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ReadError {
    #[error("the file is not valid UTF-8: {reason}")]
    NotUtf8 { reason: String },
    #[error("the workbook holds no sheet")]
    NoSheet,
    /// A sheet a format names is not in the workbook. Saxo's three are IMP-SAXO-001, and a
    /// workbook carrying only the cash ledger is refused rather than imported as one.
    #[error("the workbook holds no sheet named {name}")]
    MissingSheet { name: String },
    #[error("the file holds no header row")]
    NoHeaderRow,
    /// Two columns sharing a name: `field` would answer the first and the parsed fields the
    /// last, and the canonical rendering would emit the key twice, so the same row would read
    /// differently depending on which of the two was asked [DOM-120].
    #[error("the header row carries the column {name} twice")]
    DuplicateColumn { name: String },
    /// The container is broken, or its rows do not agree on how many columns they have.
    #[error("the file could not be read: {reason}")]
    Malformed { reason: String },
    /// A cell holding something other than text, a number, a date or nothing. Refused rather
    /// than coerced: a broker file is not expected to carry one, and guessing at a rendering
    /// for it would put a value in the audit trail that no re-read reproduces.
    #[error("row {row} column {column} holds an unsupported cell: {cell}")]
    UnsupportedCell {
        row: usize,
        column: usize,
        cell: String,
    },
}

/// Reads the data rows of a file, the header row consumed [ARC-023].
pub trait RowReader {
    /// Every data row of `content`, in file order.
    ///
    /// # Errors
    ///
    /// When the container cannot be read, or a cell holds something no rendering is defined for.
    fn rows(&self, content: &[u8]) -> Result<Vec<SourceRow>, ReadError>;
}

/// A delimited text file, one header line and one line per row.
///
/// Only the comma is offered, that being the one delimiter a specified format uses
/// [IMP-TR-001]; the field is there because the delimiter is the only thing that would vary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DelimitedReader {
    delimiter: u8,
}

/// One delimited file as read: its header row, and its data rows.
///
/// The headers are carried separately as well as on every row for the same reason
/// [`SheetRows`] carries them: a file with no data row still has a header row and a format
/// checks it, so a wholly different file carrying only a header is refused rather than read as
/// a successful, empty import [IMP-TR-001].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelimitedRows {
    /// The header row, as the file spells it, in file column order.
    pub headers: Vec<String>,
    /// The data rows, in file order.
    pub rows: Vec<SourceRow>,
}

impl DelimitedReader {
    /// A comma-delimited file, which is what Trade Republic exports [IMP-TR-001].
    #[must_use]
    pub fn comma() -> Self {
        Self { delimiter: b',' }
    }

    /// The file's header row and its data rows.
    ///
    /// # Errors
    ///
    /// As [`RowReader::rows`].
    pub fn read(&self, content: &[u8]) -> Result<DelimitedRows, ReadError> {
        let text = std::str::from_utf8(content).map_err(|error| ReadError::NotUtf8 {
            reason: error.to_string(),
        })?;
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(self.delimiter)
            .from_reader(text.as_bytes());
        let headers: Vec<String> = reader
            .headers()
            .map_err(malformed)?
            .iter()
            .map(str::to_owned)
            .collect();
        // An empty file has no header row, which the spreadsheet reader refuses too [ARC-023];
        // reading it as zero rows would import a wholly wrong file as a successful, empty one.
        if headers.is_empty() {
            return Err(ReadError::NoHeaderRow);
        }
        check_unique(&headers)?;

        // The verbatim line is recovered by slicing the file between record starts: the `csv`
        // crate parses fields and does not keep the line, and re-joining the parsed fields
        // would invent a quoting style the file need not have used [DOM-007].
        let mut records = Vec::new();
        for record in reader.records() {
            let record = record.map_err(malformed)?;
            let start = record
                .position()
                .expect("a record read from a reader carries its position")
                .byte();
            let start = usize::try_from(start).map_err(|_| ReadError::Malformed {
                reason: "the file is larger than this platform can address".to_owned(),
            })?;
            records.push((start, record));
        }

        let rows = records
            .iter()
            .enumerate()
            .map(|(index, (start, record))| {
                let end = records.get(index + 1).map_or(text.len(), |(next, _)| *next);
                // Both ends are trimmed: on a CRLF file, and after a blank line, the `csv`
                // crate reports a record as starting at the terminator that precedes it, and a
                // record never legitimately begins with one.
                let raw = text[*start..end].trim_matches(['\r', '\n']);
                let columns = headers
                    .iter()
                    .cloned()
                    .zip(record.iter().map(str::to_owned))
                    .collect();
                SourceRow::new(columns, raw)
            })
            .collect();
        Ok(DelimitedRows { headers, rows })
    }
}

impl RowReader for DelimitedReader {
    fn rows(&self, content: &[u8]) -> Result<Vec<SourceRow>, ReadError> {
        self.read(content).map(|read| read.rows)
    }
}

fn malformed(error: csv::Error) -> ReadError {
    ReadError::Malformed {
        reason: error.to_string(),
    }
}

/// Refuses a header row carrying the same column name twice, which neither specified format does
/// and which no row shape here can represent unambiguously.
fn check_unique(headers: &[String]) -> Result<(), ReadError> {
    let mut seen = BTreeSet::new();
    headers
        .iter()
        .find(|name| !seen.insert(*name))
        .map_or(Ok(()), |name| {
            Err(ReadError::DuplicateColumn { name: name.clone() })
        })
}

/// An XLSX workbook, whose first sheet holds a header row and one row per record.
///
/// Which sheet a format reads, and what its header row must contain, is that format's rule:
/// Saxo's three sheets and their Dutch headers are IMP-SAXO-001 and IMP-SAXO-002, and
/// [`crate::import::saxo`] is where they are named. This reader takes a sheet and the first row
/// of it, and checks neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpreadsheetReader;

/// One sheet as read: its header row, and its data rows.
///
/// The headers are carried separately as well as on every row, because a sheet with no data row
/// still has a header row and a format checks it: an export in the wrong language is refused on
/// its headers whether or not it happens to carry rows [IMP-SAXO-004].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetRows {
    /// The header row, as the file spells it, in sheet column order.
    pub headers: Vec<String>,
    /// The data rows, in file order.
    pub rows: Vec<SourceRow>,
}

impl SpreadsheetReader {
    /// Each named sheet, in the order named, the header rows consumed.
    ///
    /// The whole set is asked for at once because a format reading several sheets reads them
    /// from one workbook: they are joined to each other, so a missing one is a refusal of the
    /// file rather than a sheet read separately and found absent [IMP-SAXO-001].
    ///
    /// # Errors
    ///
    /// [`ReadError::MissingSheet`] when the workbook does not carry one of `names`, and
    /// otherwise as [`RowReader::rows`].
    pub fn sheets(&self, content: &[u8], names: &[&str]) -> Result<Vec<SheetRows>, ReadError> {
        let mut workbook = open(content)?;
        let present: BTreeSet<String> = workbook.sheet_names().into_iter().collect();
        names
            .iter()
            .map(|name| {
                if present.contains(*name) {
                    rows_of(&mut workbook, name)
                } else {
                    Err(ReadError::MissingSheet {
                        name: (*name).to_owned(),
                    })
                }
            })
            .collect()
    }
}

impl RowReader for SpreadsheetReader {
    fn rows(&self, content: &[u8]) -> Result<Vec<SourceRow>, ReadError> {
        let mut workbook = open(content)?;
        let sheet = workbook
            .sheet_names()
            .first()
            .cloned()
            .ok_or(ReadError::NoSheet)?;
        rows_of(&mut workbook, &sheet).map(|sheet| sheet.rows)
    }
}

fn open(content: &[u8]) -> Result<Xlsx<Cursor<&[u8]>>, ReadError> {
    Xlsx::new(Cursor::new(content)).map_err(|error| ReadError::Malformed {
        reason: error.to_string(),
    })
}

/// One sheet of an opened workbook: its header row, and the data rows under it.
fn rows_of(workbook: &mut Xlsx<Cursor<&[u8]>>, sheet: &str) -> Result<SheetRows, ReadError> {
    let range = workbook
        .worksheet_range(sheet)
        .map_err(|error| ReadError::Malformed {
            reason: error.to_string(),
        })?;

    let mut rows = range.rows();
    let header_row = rows.next().ok_or(ReadError::NoHeaderRow)?;
    let headers = header_row
        .iter()
        .enumerate()
        .map(|(column, cell)| cell_text(cell, 0, column))
        .collect::<Result<Vec<_>, _>>()?;
    check_unique(&headers)?;

    let data = rows
        .enumerate()
        .map(|(index, cells)| {
            let columns = headers
                .iter()
                .enumerate()
                // A column the row does not reach is absent, and an absent column is
                // omitted from the rendering [DOM-120].
                .filter_map(|(column, name)| cells.get(column).map(|cell| (column, name, cell)))
                .map(|(column, name, cell)| {
                    cell_text(cell, index + 1, column).map(|text| (name.clone(), text))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let raw = canonical_rendering(&columns);
            Ok(SourceRow::new(columns, raw))
        })
        .collect::<Result<Vec<_>, ReadError>>()?;

    Ok(SheetRows {
        headers,
        rows: data,
    })
}

/// A cell as the file holds it [DOM-120].
fn cell_text(cell: &Data, row: usize, column: usize) -> Result<String, ReadError> {
    let unsupported = || ReadError::UnsupportedCell {
        row,
        column,
        cell: format!("{cell:?}"),
    };
    match cell {
        Data::Empty => Ok(String::new()),
        Data::String(text) => Ok(text.clone()),
        Data::Int(number) => Ok(number.to_string()),
        Data::Float(number) => decimal_text(*number).ok_or_else(unsupported),
        // An Excel date is a serial number and stays one: converting it here would put a
        // rendering in the audit trail that the file does not hold [DOM-120], [IMP-SAXO-003].
        Data::DateTime(serial) => decimal_text(serial.as_f64()).ok_or_else(unsupported),
        Data::DateTimeIso(text) | Data::DurationIso(text) => Ok(text.clone()),
        Data::Bool(_) | Data::Error(_) => Err(unsupported()),
    }
}

/// The decimal a spreadsheet's binary double stands for.
///
/// A sheet holds every number as a double, so the scale it was written at is recovered rather
/// than read: `from_f64` rounds to the 15 significant digits a double carries and `normalize`
/// drops the padding, so a date serial comes back as `45208` and an amount as `12.15`. Reading
/// the double's own decimal expansion instead renders `12.897225000000002`.
fn decimal_text(number: f64) -> Option<String> {
    Decimal::from_f64(number).map(|value| value.normalize().to_string())
}

/// The canonical rendering of a spreadsheet row: a JSON object of column name to cell value,
/// keys in sheet column order [DOM-120].
fn canonical_rendering(columns: &[(String, String)]) -> String {
    let body = columns
        .iter()
        .map(|(name, value)| format!("{}:{}", json_string(name), json_string(value)))
        .collect::<Vec<_>>()
        .join(",");
    format!("{{{body}}}")
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("a string is always serializable as JSON")
}

#[cfg(test)]
mod tests {
    use super::*;

    const CSV: &str = "\"date\",\"type\",\"amount\"\n\
                       \"2024-05-02\",\"BUY\",\"-2628.15\"\n\
                       \"2024-05-03\",\"SELL\",\"1833.24\"\n";

    /// A delimited row keeps its verbatim line, quotes and separators included [DOM-007].
    #[test]
    fn a_delimited_row_stores_its_verbatim_line() {
        let rows = DelimitedReader::comma()
            .rows(CSV.as_bytes())
            .expect("the sample reads");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].raw(), "\"2024-05-02\",\"BUY\",\"-2628.15\"");
        assert_eq!(rows[1].raw(), "\"2024-05-03\",\"SELL\",\"1833.24\"");
    }

    /// The last line of a file that ends without a newline is still the whole line [DOM-007].
    #[test]
    fn a_final_line_without_a_newline_is_kept_whole() {
        let content = "a,b\n1,2";

        let rows = DelimitedReader::comma()
            .rows(content.as_bytes())
            .expect("the sample reads");

        assert_eq!(rows[0].raw(), "1,2");
    }

    /// Columns are keyed by the header and keep the file's order [ARC-023].
    #[test]
    fn a_delimited_row_is_keyed_by_its_headers_in_file_order() {
        let rows = DelimitedReader::comma()
            .rows(CSV.as_bytes())
            .expect("the sample reads");

        let names: Vec<&str> = rows[0]
            .columns()
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(names, ["date", "type", "amount"]);
        assert_eq!(rows[0].field("type"), Some("BUY"));
        assert_eq!(rows[0].field("absent"), None);
    }

    /// Re-reading the same bytes reproduces the same raw content [DOM-120].
    #[test]
    fn re_reading_a_delimited_file_reproduces_its_rows() {
        let reader = DelimitedReader::comma();

        assert_eq!(
            reader.rows(CSV.as_bytes()).expect("the sample reads"),
            reader.rows(CSV.as_bytes()).expect("the sample reads")
        );
    }

    /// A row with a different column count than the header is a malformed file, not a row with
    /// absent columns: guessing which column is missing would misalign every value after it.
    #[test]
    fn a_ragged_delimited_file_is_refused() {
        let content = "a,b,c\n1,2\n";

        let error = DelimitedReader::comma()
            .rows(content.as_bytes())
            .expect_err("a short row is refused");

        assert!(matches!(error, ReadError::Malformed { .. }), "{error:?}");
    }

    /// A CRLF file's line is the line, without either terminator character: the `csv` crate
    /// reports such a record as starting at the `\n` that ends the line before it [DOM-007].
    #[test]
    fn a_crlf_delimited_row_stores_its_line_without_the_terminators() {
        let content = "\"date\",\"type\"\r\n\"2024-05-02\",\"BUY\"\r\n\"2024-05-03\",\"SELL\"\r\n";

        let rows = DelimitedReader::comma()
            .rows(content.as_bytes())
            .expect("the sample reads");

        assert_eq!(rows[0].raw(), "\"2024-05-02\",\"BUY\"");
        assert_eq!(rows[1].raw(), "\"2024-05-03\",\"SELL\"");
    }

    /// A quoted field may span two physical lines, and the verbatim content is both of them: the
    /// row is the record, not the line [DOM-007].
    #[test]
    fn a_quoted_field_spanning_two_lines_is_kept_whole() {
        let content = "a,b\n\"first\nsecond\",2\n3,4\n";

        let rows = DelimitedReader::comma()
            .rows(content.as_bytes())
            .expect("the sample reads");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].raw(), "\"first\nsecond\",2");
        assert_eq!(rows[0].field("a"), Some("first\nsecond"));
        assert_eq!(rows[1].raw(), "3,4");
    }

    /// A file of a header and nothing else is an ordinary case — a broker year with no
    /// transactions — and reads as no rows at all [DOM-042].
    #[test]
    fn a_header_only_delimited_file_yields_no_rows() {
        let rows = DelimitedReader::comma()
            .rows(b"a,b\n")
            .expect("a header alone reads");

        assert!(rows.is_empty());
    }

    /// The header row is answered whether or not the file carries a data row, so a format can
    /// check it on a file it would otherwise read as empty [IMP-TR-001].
    #[test]
    fn a_header_only_delimited_file_still_answers_its_headers() {
        let read = DelimitedReader::comma()
            .read(b"a,b\n")
            .expect("a header alone reads");

        assert_eq!(read.headers, ["a", "b"]);
        assert!(read.rows.is_empty());
    }

    /// A file with no header row is refused rather than read as no rows, as it is in the
    /// spreadsheet reader: an empty or wholly wrong file is not a successful, empty import
    /// [ARC-023].
    #[test]
    fn a_delimited_file_with_no_header_row_is_refused() {
        let error = DelimitedReader::comma()
            .rows(b"")
            .expect_err("an empty file is refused");

        assert_eq!(error, ReadError::NoHeaderRow);
    }

    /// A header naming the same column twice is refused: `field` answers the first and the
    /// parsed fields the last, so the row would read two ways [DOM-120].
    #[test]
    fn a_repeated_delimited_column_name_is_refused() {
        let error = DelimitedReader::comma()
            .rows(b"a,a,b\n1,2,3\n")
            .expect_err("a repeated column is refused");

        assert_eq!(
            error,
            ReadError::DuplicateColumn {
                name: "a".to_owned()
            }
        );
    }

    #[test]
    fn a_delimited_file_that_is_not_utf8_is_refused() {
        let error = DelimitedReader::comma()
            .rows(&[0xff, 0xfe, b'a'])
            .expect_err("invalid UTF-8 is refused");

        assert!(matches!(error, ReadError::NotUtf8 { .. }), "{error:?}");
    }

    /// The rendering is a JSON object keyed in sheet column order, without insignificant
    /// whitespace [DOM-120].
    #[test]
    fn a_spreadsheet_row_renders_as_json_in_sheet_column_order() {
        let columns = vec![
            ("Transactiedatum".to_owned(), "45208".to_owned()),
            ("Acties".to_owned(), "Koop 40 @ 5.75 USD".to_owned()),
            ("Aantal".to_owned(), "-216.92".to_owned()),
        ];

        assert_eq!(
            canonical_rendering(&columns),
            r#"{"Transactiedatum":"45208","Acties":"Koop 40 @ 5.75 USD","Aantal":"-216.92"}"#
        );
    }

    /// The key is the header exactly as the file spells it, a non-breaking space and a leading
    /// space included [DOM-120], [IMP-SAXO-002].
    #[test]
    fn the_rendering_keys_are_the_headers_verbatim() {
        let columns = vec![
            ("Bk\u{a0}Record\u{a0}Id".to_owned(), "3012345678".to_owned()),
            (" Positie-ID".to_owned(), String::new()),
        ];

        assert_eq!(
            canonical_rendering(&columns),
            "{\"Bk\u{a0}Record\u{a0}Id\":\"3012345678\",\" Positie-ID\":\"\"}"
        );
    }

    /// An empty cell renders as `""`; a column the row does not carry is omitted [DOM-120].
    #[test]
    fn an_empty_cell_renders_empty_and_an_absent_one_is_omitted() {
        let empty = vec![
            ("a".to_owned(), String::new()),
            ("b".to_owned(), "1".to_owned()),
        ];
        let absent = vec![("b".to_owned(), "1".to_owned())];

        assert_eq!(canonical_rendering(&empty), r#"{"a":"","b":"1"}"#);
        assert_eq!(canonical_rendering(&absent), r#"{"b":"1"}"#);
    }

    /// Escaping is JSON's, so a value carrying a quote or a backslash reproduces exactly
    /// [DOM-120]: that is why the rendering is JSON rather than a delimited form of our own.
    #[test]
    fn the_rendering_escapes_by_json_rules() {
        let columns = vec![(
            "Opmerking".to_owned(),
            "a \"quoted\" \\ value\twith a tab".to_owned(),
        )];

        let rendered = canonical_rendering(&columns);

        assert_eq!(
            rendered,
            r#"{"Opmerking":"a \"quoted\" \\ value\twith a tab"}"#
        );
        let parsed: serde_json::Value =
            serde_json::from_str(&rendered).expect("the rendering is valid JSON");
        assert_eq!(parsed["Opmerking"], "a \"quoted\" \\ value\twith a tab");
    }

    /// An Excel serial date stays the serial the file holds [DOM-120], [IMP-SAXO-003], and an
    /// amount keeps the scale it was written at rather than the double's noise.
    #[test]
    fn a_numeric_cell_renders_as_the_decimal_the_file_holds() {
        let serial = cell_text(&Data::Float(45208.0), 1, 0).expect("a float renders");
        let amount = cell_text(&Data::Float(12.897_225), 1, 1).expect("a float renders");
        let count = cell_text(&Data::Int(40), 1, 2).expect("an integer renders");

        assert_eq!(serial, "45208");
        assert_eq!(amount, "12.897225");
        assert_eq!(count, "40");
    }

    /// A value no decimal stands for is an unsupported cell rather than a rendering of its own:
    /// `NaN` is the reachable case, `Decimal::from_f64` refusing it [DOM-120].
    #[test]
    fn a_number_no_decimal_stands_for_is_refused() {
        let error = cell_text(&Data::Float(f64::NAN), 2, 4).expect_err("NaN is refused");

        assert!(
            matches!(
                error,
                ReadError::UnsupportedCell {
                    row: 2,
                    column: 4,
                    ..
                }
            ),
            "{error:?}"
        );
    }

    /// Bytes that are not a workbook are a whole-file refusal, not a row failure [ARC-023].
    #[test]
    fn bytes_that_are_not_a_workbook_are_refused() {
        let error = SpreadsheetReader
            .rows(b"not a workbook at all")
            .expect_err("a non-workbook is refused");

        assert!(matches!(error, ReadError::Malformed { .. }), "{error:?}");
    }

    /// A legitimate workbook whose first sheet holds nothing has no header row, which is the
    /// same refusal the delimited reader gives an empty file [ARC-023].
    #[test]
    fn a_workbook_whose_first_sheet_is_empty_is_refused() {
        let error = SpreadsheetReader
            .rows(&workbook(&[]))
            .expect_err("an empty sheet is refused");

        assert_eq!(error, ReadError::NoHeaderRow);
    }

    /// The duplicate-column refusal is the reader abstraction's, so it holds on both sides of it
    /// [DOM-120], [ARC-023].
    #[test]
    fn a_repeated_sheet_column_name_is_refused() {
        let content = workbook(&[vec!["a", "a", "b"], vec!["1", "2", "3"]]);

        let error = SpreadsheetReader
            .rows(&content)
            .expect_err("a repeated column is refused");

        assert_eq!(
            error,
            ReadError::DuplicateColumn {
                name: "a".to_owned()
            }
        );
    }

    /// A workbook of one sheet holding `rows` as strings, which is the smallest input that
    /// reaches the spreadsheet reader's whole-file refusals.
    fn workbook(rows: &[Vec<&str>]) -> Vec<u8> {
        let mut workbook = rust_xlsxwriter::Workbook::new();
        let sheet = workbook.add_worksheet();
        for (row, cells) in rows.iter().enumerate() {
            for (column, cell) in cells.iter().enumerate() {
                let row = u32::try_from(row).expect("a test sheet is small");
                let column = u16::try_from(column).expect("a test sheet is narrow");
                sheet
                    .write_string(row, column, *cell)
                    .expect("a string cell writes");
            }
        }
        workbook.save_to_buffer().expect("the workbook writes")
    }

    /// A cell no rendering is defined for stops the read rather than being coerced.
    #[test]
    fn an_unsupported_cell_is_refused() {
        let error = cell_text(&Data::Bool(true), 3, 7).expect_err("a boolean is refused");

        assert!(
            matches!(
                error,
                ReadError::UnsupportedCell {
                    row: 3,
                    column: 7,
                    ..
                }
            ),
            "{error:?}"
        );
    }
}
