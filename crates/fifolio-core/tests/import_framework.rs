//! Integration layer [TST-003]: the two readers against the committed broker fixtures, which
//! is the only place the XLSX container and a real quoted CSV are exercised end to end.
//!
//! These assert parsing and reproducibility only, never an amount: the fixtures' amounts are
//! perturbed [TST-014]. No test reaches the network.

use std::fs;
use std::path::PathBuf;

use fifolio_core::import::reader::{DelimitedReader, RowReader, SpreadsheetReader};

fn fixture(directory: &str, name: &str) -> Vec<u8> {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "fixtures",
        directory,
        name,
    ]
    .iter()
    .collect();
    fs::read(&path).unwrap_or_else(|_| panic!("read the fixture {}", path.display()))
}

fn saxo() -> Vec<u8> {
    fixture(
        "saxo-nl",
        "Transactions_10000000_2024-01-01_2024-12-31.xlsx",
    )
}

fn trade_republic() -> Vec<u8> {
    fixture("trade-republic", "transactions_2024-01-01_2024-12-31.csv")
}

/// A spreadsheet row has no verbatim line, so it stores the canonical rendering: a JSON object
/// of column name to cell value, keys in sheet column order, the headers spelled exactly as the
/// file spells them [DOM-120], [ARC-023].
#[test]
fn a_saxo_row_renders_as_json_keyed_by_the_sheet_headers() {
    let rows = SpreadsheetReader
        .rows(&saxo())
        .expect("the Saxo fixture reads");

    assert!(!rows.is_empty(), "the fixture carries data rows");
    let first = &rows[0];
    let names: Vec<&str> = first
        .columns()
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(names.len(), 31, "the sheet's 31 columns, in sheet order");
    assert_eq!(names[0], "Klant-id");
    assert_eq!(names[5], " Positie-ID");
    assert_eq!(names[7], "Bk\u{a0}Record\u{a0}Id");

    let raw = first.raw();
    assert!(raw.starts_with(r#"{"Klant-id":"#), "{raw}");
    assert!(raw.ends_with('}'), "{raw}");
    assert!(
        raw.contains("\" Positie-ID\":"),
        "the leading space is part of the key: {raw}"
    );
    assert!(
        raw.contains("\"Bk\u{a0}Record\u{a0}Id\":"),
        "the non-breaking spaces are part of the key: {raw}"
    );
    // The whitespace rule is asserted on the separators rather than on the whole text: the
    // `Acties` column is free text that anonymization rebuilds (TST-029), so a value carrying a
    // colon and a space must not fail a claim about the serializer [DOM-120].
    for (index, (name, _)) in first.columns().iter().enumerate() {
        let key = serde_json::to_string(name).expect("a header serializes");
        let separator = if index == 0 {
            format!("{{{key}:\"")
        } else {
            format!(",{key}:\"")
        };
        assert!(
            raw.contains(&separator),
            "no insignificant whitespace around {separator}: {raw}"
        );
    }

    let parsed: serde_json::Value = serde_json::from_str(raw).expect("the rendering is valid JSON");
    assert_eq!(
        parsed["Transactietype"].as_str(),
        first.field("Transactietype")
    );
}

/// An Excel serial date stays the serial the file holds rather than becoming a formatted date
/// [DOM-120], [IMP-SAXO-003].
#[test]
fn a_saxo_date_stays_an_excel_serial() {
    let rows = SpreadsheetReader
        .rows(&saxo())
        .expect("the Saxo fixture reads");

    let date = rows[0]
        .field("Transactiedatum")
        .expect("the fixture carries a trade date");

    let serial: u32 = date
        .parse()
        .unwrap_or_else(|_| panic!("{date} is an Excel serial, not a rendered date"));
    // 2024-01-01 is serial 45292 and 2024-12-31 is 45657; this file is confined to 2024.
    assert!((45292..=45657).contains(&serial), "{serial}");
}

/// The rendering is part of the on-disk format: reading the same file again reproduces it byte
/// for byte [DOM-120] (DEC-065).
#[test]
fn re_reading_the_saxo_fixture_reproduces_every_rendering() {
    let content = saxo();

    let once = SpreadsheetReader.rows(&content).expect("the fixture reads");
    let twice = SpreadsheetReader.rows(&content).expect("the fixture reads");

    assert_eq!(once, twice);
}

/// A delimited row stores its verbatim line, quotes included, as the file holds it [DOM-007],
/// [ARC-023].
#[test]
fn a_trade_republic_row_stores_its_verbatim_line() {
    let content = trade_republic();
    let text = String::from_utf8(content.clone()).expect("the fixture is UTF-8");
    let lines: Vec<&str> = text.lines().collect();

    let rows = DelimitedReader::comma()
        .rows(&content)
        .expect("the Trade Republic fixture reads");

    assert_eq!(
        rows.len(),
        lines.len() - 1,
        "one row per line but the header"
    );
    for (row, line) in rows.iter().zip(lines.iter().skip(1)) {
        assert_eq!(row.raw(), *line);
    }
    assert_eq!(rows[0].columns().len(), 23, "the format's 23 columns");
    assert_eq!(rows[0].columns()[0].0, "datetime");
    assert!(rows.iter().any(|row| row.field("category") == Some("CASH")));
}

/// Re-reading reproduces the same rows here too, which is what makes a re-import idempotent
/// [DOM-022], [DOM-120].
#[test]
fn re_reading_the_trade_republic_fixture_reproduces_every_row() {
    let content = trade_republic();
    let reader = DelimitedReader::comma();

    assert_eq!(
        reader.rows(&content).expect("the fixture reads"),
        reader.rows(&content).expect("the fixture reads")
    );
}
