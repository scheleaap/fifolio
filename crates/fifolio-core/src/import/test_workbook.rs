//! Writing an XLSX workbook whose cells have exactly the shapes a broker file has.
//!
//! Test support, compiled only under `cfg(test)`.
//!
//! `rust_xlsxwriter` is what the fixture generator uses, and it writes a zero-length string as a
//! blank cell: the one known divergence between a fixture and a real Saxo export [TST-030]. So a
//! test that a blank cell reads the same in both its shapes cannot be written with it, and this
//! module emits the parts of the container by hand — every text cell a shared string, as Excel
//! and Saxo write them, and a blank either a shared string of length zero or no cell element at
//! all.
//!
//! It is the smallest workbook `calamine` will open: the content types, the two relationship
//! parts, the workbook, one worksheet per sheet and the shared string table. No styles, so a
//! number is a number and not a formatted date — which is what the reader wants anyway, an Excel
//! serial staying a serial [IMP-SAXO-003].

use std::io::{Cursor, Write as _};

use quick_xml::escape::escape;
use zip::write::SimpleFileOptions;

/// One cell, in the shape the file holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cell {
    /// A shared string. Zero-length is how Saxo writes a blank [TST-030].
    Text(String),
    /// A numeric cell, written verbatim: an Excel serial date is one of these.
    Number(String),
    /// No cell element at all, which is how a blank round-trips through the fixture writer
    /// [TST-030].
    Blank,
}

impl Cell {
    /// A non-empty text cell.
    pub fn text(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

/// One sheet: its name, its header row and its data rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetContent {
    pub name: String,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<Cell>>,
}

/// An XLSX workbook carrying `sheets`, in the order given.
///
/// # Panics
///
/// When the parts cannot be written to the in-memory archive, which nothing here can cause.
pub fn workbook(sheets: &[SheetContent]) -> Vec<u8> {
    let strings = shared_strings(sheets);

    let mut parts: Vec<(String, String)> = vec![
        ("[Content_Types].xml".to_owned(), content_types(sheets)),
        ("_rels/.rels".to_owned(), root_relationships()),
        ("xl/workbook.xml".to_owned(), workbook_part(sheets)),
        (
            "xl/_rels/workbook.xml.rels".to_owned(),
            workbook_relationships(sheets),
        ),
        (
            "xl/sharedStrings.xml".to_owned(),
            shared_string_part(&strings),
        ),
    ];
    parts.extend(sheets.iter().enumerate().map(|(index, sheet)| {
        (
            format!("xl/worksheets/sheet{}.xml", index + 1),
            worksheet(sheet, &strings),
        )
    }));

    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, body) in parts {
        archive
            // Stored rather than deflated, so the builder needs no compression feature of the
            // `zip` crate; the parts are a few kilobytes and live in memory.
            .start_file(
                name,
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
            )
            .expect("an in-memory archive accepts a part");
        archive
            .write_all(body.as_bytes())
            .expect("an in-memory archive accepts a part's bytes");
    }
    archive
        .finish()
        .expect("an in-memory archive closes")
        .into_inner()
}

/// Every distinct string the workbook holds, in first-seen order: the shared string table.
fn shared_strings(sheets: &[SheetContent]) -> Vec<String> {
    sheets
        .iter()
        .flat_map(|sheet| {
            sheet.headers.iter().cloned().chain(
                sheet
                    .rows
                    .iter()
                    .flatten()
                    .filter_map(|cell| match cell {
                        Cell::Text(value) => Some(value.clone()),
                        Cell::Number(_) | Cell::Blank => None,
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .fold(Vec::new(), |mut table, value| {
            if !table.contains(&value) {
                table.push(value);
            }
            table
        })
}

fn content_types(sheets: &[SheetContent]) -> String {
    let overrides = (1..=sheets.len())
        .map(|index| {
            format!(
                "<Override PartName=\"/xl/worksheets/sheet{index}.xml\" \
                 ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>"
            )
        })
        .collect::<String>();
    format!(
        "{DECLARATION}\
         <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
         <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
         <Default Extension=\"xml\" ContentType=\"application/xml\"/>\
         <Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>\
         {overrides}\
         <Override PartName=\"/xl/sharedStrings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml\"/>\
         </Types>"
    )
}

fn root_relationships() -> String {
    format!(
        "{DECLARATION}\
         <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
         <Relationship Id=\"rId1\" \
         Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" \
         Target=\"xl/workbook.xml\"/></Relationships>"
    )
}

fn workbook_part(sheets: &[SheetContent]) -> String {
    let entries = sheets
        .iter()
        .enumerate()
        .map(|(index, sheet)| {
            format!(
                "<sheet name=\"{}\" sheetId=\"{}\" r:id=\"rId{}\"/>",
                escape(&sheet.name),
                index + 1,
                index + 1
            )
        })
        .collect::<String>();
    format!(
        "{DECLARATION}\
         <workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" \
         xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">\
         <sheets>{entries}</sheets></workbook>"
    )
}

fn workbook_relationships(sheets: &[SheetContent]) -> String {
    let entries = sheets
        .iter()
        .enumerate()
        .map(|(index, _)| {
            format!(
                "<Relationship Id=\"rId{id}\" \
                 Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" \
                 Target=\"worksheets/sheet{id}.xml\"/>",
                id = index + 1
            )
        })
        .collect::<String>();
    format!(
        "{DECLARATION}\
         <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
         {entries}\
         <Relationship Id=\"rId{}\" \
         Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings\" \
         Target=\"sharedStrings.xml\"/></Relationships>",
        sheets.len() + 1
    )
}

fn shared_string_part(strings: &[String]) -> String {
    let entries = strings
        .iter()
        // `xml:space="preserve"` is what keeps a leading space in a header, ` Positie-ID` being
        // the one the Saxo export carries [IMP-SAXO-002].
        .map(|value| {
            format!(
                "<si><t xml:space=\"preserve\">{}</t></si>",
                escape(value.as_str())
            )
        })
        .collect::<String>();
    format!(
        "{DECLARATION}\
         <sst xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" count=\"{count}\" \
         uniqueCount=\"{count}\">{entries}</sst>",
        count = strings.len()
    )
}

fn worksheet(sheet: &SheetContent, strings: &[String]) -> String {
    let header_cells: Vec<Cell> = sheet.headers.iter().cloned().map(Cell::Text).collect();
    let body = std::iter::once(&header_cells)
        .chain(&sheet.rows)
        .enumerate()
        .map(|(index, cells)| row(index + 1, cells, strings))
        .collect::<String>();
    format!(
        "{DECLARATION}\
         <worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">\
         <sheetData>{body}</sheetData></worksheet>"
    )
}

fn row(number: usize, cells: &[Cell], strings: &[String]) -> String {
    let body = cells
        .iter()
        .enumerate()
        .map(|(column, cell)| {
            let reference = format!("{}{number}", column_name(column));
            match cell {
                Cell::Text(value) => {
                    let index = strings
                        .iter()
                        .position(|candidate| candidate == value)
                        .expect("every text cell is in the shared string table");
                    format!("<c r=\"{reference}\" t=\"s\"><v>{index}</v></c>")
                }
                Cell::Number(value) => format!("<c r=\"{reference}\"><v>{value}</v></c>"),
                // No element at all: the cell exists in the sheet's rectangle and holds nothing.
                Cell::Blank => String::new(),
            }
        })
        .collect::<String>();
    format!("<row r=\"{number}\">{body}</row>")
}

/// A zero-based column index as a spreadsheet column name: `0` is `A`, `26` is `AA`.
fn column_name(column: usize) -> String {
    let mut name = String::new();
    let mut remaining = column + 1;
    while remaining > 0 {
        let digit = (remaining - 1) % 26;
        name.insert(
            0,
            char::from(b'A' + u8::try_from(digit).expect("a remainder below 26")),
        );
        remaining = (remaining - 1) / 26;
    }
    name
}

const DECLARATION: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::reader::{ReadError, SourceRow, SpreadsheetReader};

    /// The builder writes a workbook the reader opens, with the sheets, the headers and the
    /// values it was given.
    #[test]
    fn the_builder_writes_a_readable_workbook() {
        let content = workbook(&[
            SheetContent {
                name: "Transacties".to_owned(),
                headers: vec![" Positie-ID".to_owned(), "Aantal".to_owned()],
                rows: vec![vec![Cell::text("x"), Cell::Number("45208".to_owned())]],
            },
            SheetContent {
                name: "_Transacties".to_owned(),
                headers: vec!["Prijs".to_owned()],
                rows: vec![vec![Cell::Number("5.75".to_owned())]],
            },
        ]);

        let sheets = SpreadsheetReader
            .sheets(&content, &["Transacties", "_Transacties"])
            .expect("the workbook reads");

        assert_eq!(sheets[0].headers, [" Positie-ID", "Aantal"]);
        assert_eq!(sheets[0].rows[0].field(" Positie-ID"), Some("x"));
        assert_eq!(sheets[0].rows[0].field("Aantal"), Some("45208"));
        assert_eq!(sheets[1].rows[0].field("Prijs"), Some("5.75"));
    }

    /// A sheet the workbook does not carry is the reader's refusal, which is what makes the
    /// missing-sheet test above a test of the reader and not of the builder.
    #[test]
    fn a_sheet_the_builder_did_not_write_is_missing() {
        let content = workbook(&[SheetContent {
            name: "Transacties".to_owned(),
            headers: vec!["Aantal".to_owned()],
            rows: Vec::new(),
        }]);

        let error = SpreadsheetReader
            .sheets(&content, &["Bookings"])
            .expect_err("an absent sheet is refused");

        assert_eq!(
            error,
            ReadError::MissingSheet {
                name: "Bookings".to_owned()
            }
        );
    }

    /// The two shapes of a blank are written as two different things — a shared string of length
    /// zero and no cell element — so a test that they read alike is testing the reader.
    #[test]
    fn the_two_shapes_of_a_blank_are_written_differently() {
        let sheet = |blank: Cell| SheetContent {
            name: "Transacties".to_owned(),
            headers: vec!["a".to_owned(), "b".to_owned()],
            rows: vec![vec![blank, Cell::text("x")]],
        };
        let shared = worksheet(
            &sheet(Cell::Text(String::new())),
            &shared_strings(&[sheet(Cell::Text(String::new()))]),
        );
        let empty = worksheet(&sheet(Cell::Blank), &shared_strings(&[sheet(Cell::Blank)]));

        assert!(shared.contains("<c r=\"A2\" t=\"s\">"), "{shared}");
        assert!(!empty.contains("r=\"A2\""), "{empty}");
    }

    /// The column names run past `Z`, a Saxo sheet being 31 columns wide.
    #[test]
    fn column_names_run_past_z() {
        assert_eq!(column_name(0), "A");
        assert_eq!(column_name(25), "Z");
        assert_eq!(column_name(26), "AA");
        assert_eq!(column_name(30), "AE");
    }

    /// A value carrying XML's own characters survives, so a header or a label is not a way to
    /// break the container.
    #[test]
    fn a_value_carrying_markup_is_escaped() {
        let content = workbook(&[SheetContent {
            name: "Transacties".to_owned(),
            headers: vec!["a & <b>".to_owned()],
            rows: vec![vec![Cell::text("x < y & \"z\"")]],
        }]);

        let sheets = SpreadsheetReader
            .sheets(&content, &["Transacties"])
            .expect("the workbook reads");

        let row: &SourceRow = &sheets[0].rows[0];
        assert_eq!(sheets[0].headers, ["a & <b>"]);
        assert_eq!(row.field("a & <b>"), Some("x < y & \"z\""));
    }
}
