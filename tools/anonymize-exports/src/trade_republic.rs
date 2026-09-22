//! Reading a real Trade Republic DE export and writing the fixture derived from it.
//!
//! What the fixture keeps, because the importer is specified against it [TST-013]:
//!
//! * a quoted, comma-delimited CSV with one header row of all 23 columns [IMP-TR-001]
//! * `datetime` as an ISO-8601 UTC timestamp with sub-second precision, distinct from `date`
//!   [IMP-TR-002], [IMP-TR-015]
//! * `transaction_id` as a UUID [IMP-TR-003]
//! * the cash-flow sign convention, so buys and fees stay negative [IMP-TR-004]
//! * `original_amount`, `original_currency` and `fx_rate` populated where they were [IMP-TR-006]
//! * the row order of the file, which in the 2025 export is not the `datetime` order
//!   [IMP-TR-023]
//! * `category`, `type`, `asset_class` and `shares` verbatim, which is everything classification
//!   reads [IMP-TR-008]
//!
//! Only the identities are replaced and the amounts moved.

use std::path::Path;

use anyhow::{Context, Result, bail};
use rust_decimal::Decimal;

use crate::perturb::perturb;
use crate::pseudonym::{Kind, Originals, Pseudonyms, free_text, uuids_in};

/// The 23 columns, in the order the export writes them [IMP-TR-001].
pub const HEADERS: [&str; 23] = [
    "datetime",
    "date",
    "account_type",
    "category",
    "type",
    "asset_class",
    "name",
    "symbol",
    "shares",
    "price",
    "amount",
    "fee",
    "tax",
    "currency",
    "original_amount",
    "original_currency",
    "fx_rate",
    "description",
    "transaction_id",
    "counterparty_name",
    "counterparty_iban",
    "payment_reference",
    "mcc_code",
];

/// The columns holding a monetary figure or a rate, all of which are perturbed. `shares` is a
/// quantity and not an amount, so it stays: the classification rules read it [IMP-TR-019].
const AMOUNT_HEADERS: [&str; 6] = [
    "price",
    "amount",
    "fee",
    "tax",
    "original_amount",
    "fx_rate",
];

/// The rows of one export, each in the column order of [`HEADERS`].
#[derive(Debug, Clone)]
pub struct Rows(pub Vec<Vec<String>>);

/// Reads a real export.
///
/// # Errors
///
/// When the file cannot be read, or when its header row is not the expected 23 — a changed export
/// shape must stop the run rather than produce a fixture nothing is specified against.
pub fn read(path: &Path) -> Result<Rows> {
    let mut reader =
        csv::Reader::from_path(path).with_context(|| format!("opening {}", path.display()))?;
    let headers: Vec<String> = reader
        .headers()
        .with_context(|| format!("reading the header row of {}", path.display()))?
        .iter()
        .map(str::to_owned)
        .collect();
    if headers != HEADERS {
        bail!(
            "{} does not carry the 23 Trade Republic columns: {headers:?}",
            path.display()
        );
    }

    let rows = reader
        .records()
        .map(|record| {
            record
                .map(|fields| fields.iter().map(str::to_owned).collect())
                .with_context(|| format!("reading a row of {}", path.display()))
        })
        .collect::<Result<Vec<Vec<String>>>>()?;
    Ok(Rows(rows))
}

fn field<'row>(row: &'row [String], header: &str) -> &'row str {
    &row[HEADERS
        .iter()
        .position(|candidate| *candidate == header)
        .expect("a header this crate names is one of the 23")]
}

/// Records every identifying value the export carries.
pub fn collect(rows: &Rows, originals: &mut Originals) {
    for row in &rows.0 {
        let value = |header: &str| field(row, header);
        originals.add(Kind::Isin, value("symbol"));
        // `name` holds the instrument where the row names one and the account holder where it
        // does not: a cash transfer is booked with the sender's name in it.
        if value("symbol").is_empty() {
            originals.add(Kind::Person, value("name"));
        } else {
            originals.add(Kind::InstrumentName, value("name"));
        }
        originals.add(Kind::Uuid, value("transaction_id"));
        originals.add(Kind::Person, value("counterparty_name"));
        originals.add(Kind::Iban, value("counterparty_iban"));
        // Interest rows name a payout collection by its own UUID inside the description.
        for embedded in uuids_in(value("description")) {
            originals.add(Kind::Uuid, embedded);
        }
    }
}

/// The fixture form of the export.
///
/// # Errors
///
/// When a value was not collected before it was replaced, or when an amount column holds
/// something that is not a number.
pub fn anonymize(rows: &Rows, pseudonyms: &Pseudonyms) -> Result<Rows> {
    let rows = rows
        .0
        .iter()
        .map(|row| anonymize_row(row, pseudonyms))
        .collect::<Result<Vec<_>>>()?;
    Ok(Rows(rows))
}

fn anonymize_row(row: &[String], pseudonyms: &Pseudonyms) -> Result<Vec<String>> {
    let value = |header: &str| field(row, header);
    let names_a_security = !value("symbol").is_empty();
    let isin = pseudonyms.of(Kind::Isin, value("symbol"))?;

    HEADERS
        .iter()
        .zip(row)
        .map(|(header, original)| match *header {
            "symbol" => Ok(isin.clone()),
            "name" if names_a_security => pseudonyms.of(Kind::InstrumentName, original),
            "name" => pseudonyms.of(Kind::Person, original),
            "transaction_id" => pseudonyms.of(Kind::Uuid, original),
            "counterparty_name" => pseudonyms.of(Kind::Person, original),
            "counterparty_iban" => pseudonyms.of(Kind::Iban, original),
            "description" => Ok(free_text(
                pseudonyms,
                original,
                names_a_security.then_some(isin.as_str()),
                value("type"),
            )),
            header if AMOUNT_HEADERS.contains(&header) => rewritten_amount(original),
            _ => Ok(original.clone()),
        })
        .collect()
}

/// An amount keeps the decimal places the export wrote it with, which is why it goes back out
/// through `Decimal` rather than being reformatted.
fn rewritten_amount(original: &str) -> Result<String> {
    if original.is_empty() {
        return Ok(String::new());
    }
    let amount = Decimal::from_str_exact(original)
        .with_context(|| format!("{original:?} is not an amount"))?;
    Ok(perturb(amount).to_string())
}

/// Writes the fixture, quoting every field as the export does [IMP-TR-001].
///
/// # Errors
///
/// When the file cannot be written.
pub fn write(path: &Path, rows: &Rows) -> Result<()> {
    let mut writer = csv::WriterBuilder::new()
        .quote_style(csv::QuoteStyle::Always)
        .from_path(path)
        .with_context(|| format!("writing {}", path.display()))?;
    writer.write_record(HEADERS)?;
    for row in &rows.0 {
        writer.write_record(row)?;
    }
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All 23 columns, in the export's order [IMP-TR-001].
    #[test]
    fn the_column_set_is_complete() {
        assert_eq!(HEADERS.len(), 23);
        assert_eq!(HEADERS[0], "datetime");
        assert_eq!(HEADERS[1], "date");
        assert_eq!(HEADERS[22], "mcc_code");
    }

    /// An amount keeps its scale and its sign; an empty column stays empty [IMP-TR-004].
    #[test]
    fn an_amount_keeps_its_shape() {
        assert_eq!(rewritten_amount("").unwrap(), "");
        let moved = rewritten_amount("-2628.15").unwrap();
        assert!(moved.starts_with('-'), "{moved}");
        assert_eq!(moved.split('.').next_back().unwrap().len(), 2, "{moved}");
        assert_eq!(
            rewritten_amount("0.030000")
                .unwrap()
                .split('.')
                .next_back()
                .unwrap()
                .len(),
            6
        );
        assert!(rewritten_amount("not a number").is_err());
    }

    /// Quantities and dates are structural and come through untouched, only the amounts move
    /// [TST-028]: the `TAX_EXCHANGE` pairing is recognized by equal absolute `shares`
    /// [IMP-TR-019], and `date` carries both the per-file calendar year and the ordering cases.
    #[test]
    fn quantities_and_dates_are_not_perturbed() {
        for structural in ["shares", "date", "datetime"] {
            assert!(!AMOUNT_HEADERS.contains(&structural));
        }

        let row: Vec<String> = [
            "2025-03-01T03:42:15.115349Z",
            "2025-02-28",
            "SECURITIES",
            "TRADING",
            "BUY",
            "STOCK",
            "Example Instrument",
            "NL0000000001",
            "12.345678",
            "10.25",
            "-126.56",
            "-1.00",
            "",
            "EUR",
            "",
            "",
            "",
            "Buy trade NL0000000001 Example Instrument",
            "3f2b7c10-9d44-4a61-8f0e-2c6b51d9a704",
            "",
            "",
            "",
            "",
        ]
        .iter()
        .map(|value| (*value).to_owned())
        .collect();
        let rows = Rows(vec![row]);
        let mut originals = Originals::default();
        collect(&rows, &mut originals);
        let anonymized = anonymize(&rows, &Pseudonyms::build(&originals).unwrap()).unwrap();
        let written = &anonymized.0[0];

        assert_eq!(field(written, "shares"), "12.345678");
        assert_eq!(field(written, "date"), "2025-02-28");
        assert_eq!(field(written, "datetime"), "2025-03-01T03:42:15.115349Z");
        assert_ne!(
            field(written, "amount"),
            "-126.56",
            "the amount stood still"
        );
    }
}
