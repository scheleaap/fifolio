//! Reading a real Saxo NL export and writing the fixture derived from it.
//!
//! What the fixture keeps, because the importer is specified against it [TST-013]:
//!
//! * a real XLSX container with one sheet and one header row of 31 Dutch headers, written
//!   byte for byte from the constant below — the non-breaking spaces in `Bk Record Id` and
//!   `Booking Id` and the leading space in ` Positie-ID` included [IMP-SAXO-001], [IMP-SAXO-002]
//! * dates as Excel serial numbers under the export's own `dd-mmm-yyyy` format [IMP-SAXO-003]
//! * the rows in file order, which Saxo emits newest first [IMP-SAXO-025]
//! * the per-currency `Rekening-ID` suffixes over one base account [IMP-SAXO-005]
//! * the free-text `Acties` labels, with their quantity, direction and 2-decimal price
//!   [IMP-SAXO-011], [IMP-SAXO-012]
//! * every id column's digit width, its blank rows and which rows share an id
//!
//! Only the identities are replaced and the amounts moved. Which rows exist, in which order,
//! carrying which labels and which ids, is the real file's.

use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use calamine::{Data, Reader, Xlsx, open_workbook};
use regex::Regex;
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive as _, ToPrimitive as _};
use rust_xlsxwriter::{DocProperties, ExcelDateTime, Format, Workbook};

use crate::perturb::perturb;
use crate::pseudonym::{Kind, Originals, Pseudonyms, free_text};

/// The one sheet the importer reads [IMP-SAXO-001].
pub const SHEET_NAME: &str = "Transacties";

/// The 31 headers, byte for byte, `\u{a0}` and the leading space included [IMP-SAXO-002].
pub const HEADERS: [&str; 31] = [
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

/// The columns holding a monetary figure or a conversion rate, all of which are perturbed.
const AMOUNT_HEADERS: [&str; 6] = [
    "Aantal",
    "Boekingsbedrag",
    "Omrekeningskoers",
    "Omwisselkosten",
    "Totale kosten",
    "Gerealiseerd rendement",
];

/// The two date columns, written under the export's own number format so that a reader sees
/// serial numbers where the real file has them [IMP-SAXO-003].
const DATE_HEADERS: [&str; 2] = ["Transactiedatum", "Valutadatum"];

/// One cell, in the only three shapes the sheet uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cell {
    Empty,
    Text(String),
    Number(Decimal),
}

impl Cell {
    /// The cell as text, an empty cell included, for matching and for the leak check.
    #[must_use]
    pub fn as_text(&self) -> String {
        match self {
            Self::Empty => String::new(),
            Self::Text(text) => text.clone(),
            Self::Number(number) => number.to_string(),
        }
    }
}

/// The data rows of the one sheet; the header row is [`HEADERS`].
#[derive(Debug, Clone)]
pub struct Sheet {
    pub rows: Vec<Vec<Cell>>,
}

/// The value of `header` in `row`.
fn cell<'row>(row: &'row [Cell], header: &str) -> &'row Cell {
    &row[column(header)]
}

fn column(header: &str) -> usize {
    HEADERS
        .iter()
        .position(|candidate| *candidate == header)
        .expect("a header this crate names is one of the 31")
}

/// Reads the single sheet of a real export.
///
/// # Errors
///
/// When the workbook cannot be read, when its header row is not the expected 31 — a changed
/// export shape must stop the run rather than produce a fixture nothing is specified against —
/// or when a cell holds something other than text, a number or nothing.
pub fn read(path: &Path) -> Result<Sheet> {
    let mut workbook: Xlsx<_> =
        open_workbook(path).with_context(|| format!("opening {}", path.display()))?;
    let sheet_name = workbook
        .sheet_names()
        .first()
        .cloned()
        .ok_or_else(|| anyhow!("{} has no sheet", path.display()))?;
    let range = workbook
        .worksheet_range(&sheet_name)
        .with_context(|| format!("reading sheet {sheet_name} of {}", path.display()))?;

    let mut rows = range.rows();
    let header_row = rows
        .next()
        .ok_or_else(|| anyhow!("{} has no header row", path.display()))?;
    let headers: Vec<String> = header_row.iter().map(read_text).collect();
    if headers != HEADERS {
        bail!(
            "{} does not carry the 31 Dutch Saxo headers: {headers:?}",
            path.display()
        );
    }

    let rows = rows
        .map(|row| row.iter().map(read_cell).collect::<Result<Vec<Cell>>>())
        .collect::<Result<Vec<_>>>()?;
    Ok(Sheet { rows })
}

fn read_text(data: &Data) -> String {
    match data {
        Data::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn read_cell(data: &Data) -> Result<Cell> {
    match data {
        Data::Empty => Ok(Cell::Empty),
        Data::String(text) if text.is_empty() => Ok(Cell::Empty),
        Data::String(text) => Ok(Cell::Text(text.clone())),
        Data::Int(number) => Ok(Cell::Number(Decimal::from(*number))),
        Data::Float(number) => decimal_from(*number),
        Data::DateTime(serial) => decimal_from(serial.as_f64()),
        other => bail!("unexpected cell {other:?}"),
    }
}

/// A spreadsheet holds every number as a binary double, so the scale a column is written at is
/// recovered rather than read: `from_f64` rounds to the 15 significant digits a double actually
/// carries and `normalize` then drops the zeros that padding left, so `12.15` comes back at scale
/// 2 and a date serial at scale 0. Without it a perturbed amount is rounded at the scale of the
/// binary noise and the fixture shows `12.897225000000002`.
fn decimal_from(number: f64) -> Result<Cell> {
    Decimal::from_f64(number)
        .map(|value| Cell::Number(value.normalize()))
        .ok_or_else(|| anyhow!("{number} is not representable as a decimal"))
}

/// Records every identifying value the sheet carries.
pub fn collect(sheet: &Sheet, originals: &mut Originals) {
    for row in &sheet.rows {
        let text = |header: &str| cell(row, header).as_text();
        originals.add(Kind::ClientId, &text("Klant-id"));
        originals.add(Kind::AccountBase, account_base(&text("Rekening-ID")));
        originals.add(Kind::TransactieId, &text("Transactie-ID"));
        originals.add(Kind::PositieId, &text(" Positie-ID"));
        originals.add(Kind::CorporateActionId, &text("Corporate action-Id"));
        originals.add(Kind::BkRecordId, &text("Bk\u{a0}Record\u{a0}Id"));
        originals.add(Kind::BookingId, &text("Booking\u{a0}Id"));
        originals.add(Kind::Iban, &text("IBAN"));
        originals.add(Kind::Person, &text("Naam IBAN-eigenaar"));
        originals.add(Kind::Isin, &text("Instrument ISIN"));
        originals.add(Kind::Symbol, &text("Instrumentsymbool"));
        originals.add(
            Kind::InstrumentName,
            &Pseudonyms::core_instrument_name(&text("Instrument")),
        );
    }
}

/// The `Rekening-ID` without its per-currency suffix: `69900/1000000EUR` is a sub-account of
/// `69900/1000000` [IMP-SAXO-005]. The suffix is a currency code and identifies nobody, so it
/// stays and the fixture keeps the three sub-accounts of one Depot.
fn account_base(account: &str) -> &str {
    account
        .strip_suffix(currency_suffix(account))
        .unwrap_or(account)
}

fn currency_suffix(account: &str) -> &str {
    let tail_starts_at = account.len().saturating_sub(3);
    let tail = &account[tail_starts_at..];
    if tail.len() == 3 && tail.chars().all(|character| character.is_ascii_uppercase()) {
        tail
    } else {
        ""
    }
}

/// The fixture form of the sheet.
///
/// # Errors
///
/// When a value was not collected before it was replaced, or when a numeric column holds
/// something a decimal cannot carry.
pub fn anonymize(sheet: &Sheet, pseudonyms: &Pseudonyms) -> Result<Sheet> {
    let rows = sheet
        .rows
        .iter()
        .map(|row| anonymize_row(row, pseudonyms))
        .collect::<Result<Vec<_>>>()?;
    Ok(Sheet { rows })
}

fn anonymize_row(row: &[Cell], pseudonyms: &Pseudonyms) -> Result<Vec<Cell>> {
    let text = |header: &str| cell(row, header).as_text();
    let names_a_security = !text("Instrument ISIN").is_empty();
    let isin = pseudonyms.of(Kind::Isin, &text("Instrument ISIN"))?;

    HEADERS
        .iter()
        .zip(row)
        .map(|(header, cell)| -> Result<Cell> {
            let original = cell.as_text();
            let replaced = match *header {
                "Klant-id" => pseudonyms.of(Kind::ClientId, &original)?,
                "Rekening-ID" => format!(
                    "{}{}",
                    pseudonyms.of(Kind::AccountBase, account_base(&original))?,
                    currency_suffix(&original)
                ),
                "Transactie-ID" => pseudonyms.of(Kind::TransactieId, &original)?,
                " Positie-ID" => pseudonyms.of(Kind::PositieId, &original)?,
                "Corporate action-Id" => pseudonyms.of(Kind::CorporateActionId, &original)?,
                "Bk\u{a0}Record\u{a0}Id" => pseudonyms.of(Kind::BkRecordId, &original)?,
                "Booking\u{a0}Id" => pseudonyms.of(Kind::BookingId, &original)?,
                "IBAN" => pseudonyms.of(Kind::Iban, &original)?,
                "Naam IBAN-eigenaar" => pseudonyms.of(Kind::Person, &original)?,
                "Instrument" => pseudonyms.instrument_name(&original)?,
                "Instrumentsymbool" => symbol(pseudonyms, &original)?,
                "Instrument ISIN" => isin.clone(),
                "Acties" => relabel(pseudonyms, &original),
                "Opmerking" => free_text(
                    pseudonyms,
                    &original,
                    names_a_security.then_some(isin.as_str()),
                    &text("Acties"),
                ),
                _ => return rewritten_amount(header, cell),
            };
            Ok(same_shape_as(cell, &replaced))
        })
        .collect()
}

/// A replacement keeps the cell shape it replaces: `Bk Record Id` is a number in the export and
/// the other id columns are text, and an importer's header-to-type mapping is tested against
/// that.
fn same_shape_as(cell: &Cell, replacement: &str) -> Cell {
    match cell {
        Cell::Empty => Cell::Empty,
        Cell::Text(_) => Cell::Text(replacement.to_owned()),
        Cell::Number(_) => replacement
            .parse::<Decimal>()
            .map_or_else(|_| Cell::Text(replacement.to_owned()), Cell::Number),
    }
}

fn rewritten_amount(header: &str, cell: &Cell) -> Result<Cell> {
    match (AMOUNT_HEADERS.contains(&header), cell) {
        (true, Cell::Number(amount)) => Ok(Cell::Number(perturb(*amount))),
        _ => Ok(cell.clone()),
    }
}

/// An instrument symbol, keeping the `:exchange` suffix that says where it trades.
fn symbol(pseudonyms: &Pseudonyms, original: &str) -> Result<String> {
    match original.split_once(':') {
        Some((_, exchange)) => Ok(format!(
            "{}:{exchange}",
            pseudonyms.of(Kind::Symbol, original)?
        )),
        None => pseudonyms.of(Kind::Symbol, original),
    }
}

/// `Koop 40 @ 5.75 USD` with the price perturbed and everything else kept.
///
/// The label is the only place a quantity and a direction appear [IMP-SAXO-011], so both stay;
/// the price is an amount and moves with the rest, keeping its 2 decimals.
fn relabel(pseudonyms: &Pseudonyms, label: &str) -> String {
    static PATTERN: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let pattern = PATTERN.get_or_init(|| {
        Regex::new(
            r"^(?<label>.+?) (?<quantity>-?[\d.]+) @ (?<price>\d+\.\d{2}) (?<currency>[A-Z]{3})$",
        )
        .expect("the Acties label pattern compiles")
    });
    match pattern.captures(label) {
        Some(captured) => {
            let price = captured["price"]
                .parse::<Decimal>()
                .map(perturb)
                .map_or_else(|_| captured["price"].to_owned(), |moved| moved.to_string());
            format!(
                "{} {} @ {price} {}",
                &captured["label"], &captured["quantity"], &captured["currency"]
            )
        }
        None => pseudonyms.substitute(label),
    }
}

/// Writes the fixture workbook.
///
/// # Errors
///
/// When the file cannot be written, or when an amount is not representable in a spreadsheet.
pub fn write(path: &Path, sheet: &Sheet) -> Result<()> {
    let mut workbook = Workbook::new();
    // The container otherwise records the moment it was written, which would make every rerun a
    // diff even where no row changed. A fixed creation time and the zip's own fixed entry times
    // make the same exports produce the same bytes.
    workbook.set_properties(
        &DocProperties::new().set_creation_datetime(&ExcelDateTime::from_ymd(2000, 1, 1)?),
    );
    let dates = Format::new().set_num_format("dd-mmm-yyyy");
    let worksheet = workbook.add_worksheet();
    worksheet.set_name(SHEET_NAME)?;

    for (index, header) in HEADERS.iter().enumerate() {
        worksheet.write_string(0, column_index(index)?, *header)?;
    }
    for (row_index, row) in sheet.rows.iter().enumerate() {
        let row_number = u32::try_from(row_index + 1)?;
        for (index, cell) in row.iter().enumerate() {
            let column_number = column_index(index)?;
            match cell {
                // An empty cell is written as an empty string, which is what the real export
                // holds: a shared string of zero length rather than a blank cell.
                Cell::Empty => worksheet.write_string(row_number, column_number, "")?,
                Cell::Text(text) => worksheet.write_string(row_number, column_number, text)?,
                Cell::Number(number) => {
                    let value = number
                        .to_f64()
                        .ok_or_else(|| anyhow!("{number} is not representable in a spreadsheet"))?;
                    if DATE_HEADERS.contains(&HEADERS[index]) {
                        worksheet.write_number_with_format(
                            row_number,
                            column_number,
                            value,
                            &dates,
                        )?
                    } else {
                        worksheet.write_number(row_number, column_number, value)?
                    }
                }
            };
        }
    }

    workbook.save(path)?;
    Ok(())
}

fn column_index(index: usize) -> Result<u16> {
    u16::try_from(index).context("a column index fits a spreadsheet column")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The header set is the one the importer matches against [IMP-SAXO-002].
    #[test]
    fn the_headers_carry_their_unusual_whitespace() {
        assert_eq!(HEADERS.len(), 31);
        assert!(HEADERS.contains(&"Bk\u{a0}Record\u{a0}Id"));
        assert!(HEADERS.contains(&"Booking\u{a0}Id"));
        assert!(HEADERS.contains(&" Positie-ID"));
    }

    /// The per-currency suffix marks a sub-account of one Depot [IMP-SAXO-005].
    #[test]
    fn an_account_splits_into_a_base_and_a_currency() {
        assert_eq!(account_base("69900/1000000EUR"), "69900/1000000");
        assert_eq!(account_base("69900/1000000CAD"), "69900/1000000");
        assert_eq!(currency_suffix("69900/1000000USD"), "USD");
        assert_eq!(currency_suffix("69900/1000000"), "");
    }

    /// The label keeps its quantity and its direction; only the price moves [IMP-SAXO-011],
    /// [TST-028]. The label is the only place a Saxo quantity appears, `Aantal` being a cash
    /// movement and never a quantity [IMP-SAXO-009], so this is where TST-028 bites for Saxo.
    #[test]
    fn a_label_keeps_its_quantity_and_direction() {
        let pseudonyms = Pseudonyms::build(&Originals::default()).unwrap();
        let relabeled = relabel(&pseudonyms, "Verkoop -60 @ 30.65 EUR");
        assert!(relabeled.starts_with("Verkoop -60 @ "), "{relabeled}");
        assert!(relabeled.ends_with(" EUR"), "{relabeled}");
        assert!(!relabeled.contains("30.65"), "{relabeled}");
        assert_eq!(relabel(&pseudonyms, "Stock split"), "Stock split");
        assert!(
            relabel(&pseudonyms, "Deponering 3000 @ 139.46 EUR").starts_with("Deponering 3000 @ ")
        );
    }

    /// A replacement is written in the shape of the cell it replaces.
    #[test]
    fn a_number_stays_a_number_and_a_blank_stays_blank() {
        assert_eq!(
            same_shape_as(&Cell::Number(Decimal::ONE), "3000000011"),
            Cell::Number(Decimal::from(3_000_000_011_u64))
        );
        assert_eq!(
            same_shape_as(&Cell::Text("x".into()), "40000000013"),
            Cell::Text("40000000013".into())
        );
        assert_eq!(same_shape_as(&Cell::Empty, ""), Cell::Empty);
    }

    /// Amounts move, everything else in an unmapped column does not. A date serial is a number
    /// too and stays exactly where it was [TST-028]: a moved date would break the per-file
    /// calendar-year boundary and the ordering cases the fixtures exist for.
    #[test]
    fn only_the_amount_columns_are_perturbed() {
        let amount = Cell::Number(Decimal::new(181_324, 2));
        assert_ne!(rewritten_amount("Aantal", &amount).unwrap(), amount);
        assert_eq!(rewritten_amount("Type", &amount).unwrap(), amount);
        for date_header in DATE_HEADERS {
            let serial = Cell::Number(Decimal::from(45_000));
            assert_eq!(rewritten_amount(date_header, &serial).unwrap(), serial);
        }
    }
}
