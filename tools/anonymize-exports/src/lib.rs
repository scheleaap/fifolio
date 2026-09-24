//! Derives the committed test fixtures from the real broker exports [TST-011].
//!
//! The real exports in `design/example_exports/` are personal financial records and are
//! gitignored; the fixtures in `fixtures/` are what the test suites read. This binary is the only
//! thing that turns one into the other, so that a future export shape can be folded in by running
//! it again rather than by editing a fixture by hand.
//!
//! ```text
//! cargo run --package anonymize-exports -- --source design/example_exports --out fixtures
//! ```
//!
//! Every row of every export is carried over — for Saxo that is all three sheets, since a fixture
//! carrying one of them is not a fixture of this file [TST-031]. Which rows exist, on which sheet,
//! in which order, carrying which labels, ids and classification columns, is the real file's
//! [TST-013]; the identities are
//! replaced [TST-012] and the amounts are moved, which is why the fixtures test parsing,
//! classification and idempotency and never arithmetic [TST-014].
//!
//! Amounts, and nothing else: quantities and dates are structural and are left alone [TST-028],
//! and free text naming a security is rebuilt rather than stripped [TST-029].
//!
//! # Why it is a whole-corpus, two-pass run
//!
//! The pseudonym for a value is decided from its rank among all values of its kind, so every file
//! is read before any is written. That is what keeps Saxo's booking counters ascending in the
//! fixture as they ascend in the export, and what makes one ISIN one security across five years.
//!
//! # The leak check
//!
//! Nothing is written until every value of every output file has been checked against every
//! original collected. A column that was collected but not replaced fails the run rather than
//! shipping. It cannot catch an identity in a column nobody thought to collect, which is why the
//! free-text columns are rebuilt rather than filtered — see [`pseudonym::free_text`].

pub mod perturb;
pub mod pseudonym;
pub mod saxo;
pub mod trade_republic;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};

use pseudonym::{Originals, Pseudonyms};

const SAXO_DIRECTORY: &str = "saxo-nl";
const TRADE_REPUBLIC_DIRECTORY: &str = "trade-republic";

/// Reads every export under `source` and writes its fixture under `out`.
///
/// # Errors
///
/// When an export cannot be read, when its shape is not the one the importers are specified
/// against, or when a fixture would carry a real value.
pub fn run(source: &Path, out: &Path) -> Result<()> {
    let saxo_sources = exports_in(&source.join(SAXO_DIRECTORY), "xlsx")?;
    let trade_republic_sources = exports_in(&source.join(TRADE_REPUBLIC_DIRECTORY), "csv")?;

    let saxo_exports = saxo_sources
        .iter()
        .map(|path| saxo::read(path))
        .collect::<Result<Vec<_>>>()?;
    let trade_republic_rows = trade_republic_sources
        .iter()
        .map(|path| trade_republic::read(path))
        .collect::<Result<Vec<_>>>()?;

    let mut originals = Originals::default();
    for export in &saxo_exports {
        saxo::collect(export, &mut originals);
    }
    for rows in &trade_republic_rows {
        trade_republic::collect(rows, &mut originals);
    }
    let pseudonyms = Pseudonyms::build(&originals)?;

    let saxo_out = out.join(SAXO_DIRECTORY);
    let trade_republic_out = out.join(TRADE_REPUBLIC_DIRECTORY);
    fs::create_dir_all(&saxo_out)?;
    fs::create_dir_all(&trade_republic_out)?;

    for (source, export) in saxo_sources.iter().zip(&saxo_exports) {
        let anonymized = saxo::anonymize(export, &pseudonyms)?;
        let target = fixture_path(&saxo_out, source, &pseudonyms)?;
        refuse_leaks(&pseudonyms, &target, saxo_values(&anonymized))?;
        saxo::write(&target, &anonymized)?;
        let counts: Vec<String> = anonymized
            .sheets
            .iter()
            .map(|sheet| format!("{} {}", sheet.rows.len(), sheet.kind.name()))
            .collect();
        println!("{} rows -> {}", counts.join(", "), target.display());
    }

    for (source, rows) in trade_republic_sources.iter().zip(&trade_republic_rows) {
        let anonymized = trade_republic::anonymize(rows, &pseudonyms)?;
        let target = fixture_path(&trade_republic_out, source, &pseudonyms)?;
        refuse_leaks(
            &pseudonyms,
            &target,
            anonymized.0.iter().flat_map(|row| row.iter().cloned()),
        )?;
        trade_republic::write(&target, &anonymized)?;
        println!("{} rows -> {}", anonymized.0.len(), target.display());
    }

    Ok(())
}

/// The export files of one format, in a fixed order so that a rerun assigns the same ranks.
fn exports_in(directory: &Path, extension: &str) -> Result<Vec<PathBuf>> {
    let mut paths: Vec<PathBuf> = fs::read_dir(directory)
        .with_context(|| format!("listing {}", directory.display()))?
        .map(|entry| entry.map(|found| found.path()))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|path| path.extension().is_some_and(|found| found == extension))
        .collect();
    paths.sort();
    if paths.is_empty() {
        bail!("{} holds no .{extension} export", directory.display());
    }
    Ok(paths)
}

/// Where a source file's fixture goes. A Saxo export is named for the client id, so the file name
/// goes through the same substitution its contents do.
fn fixture_path(directory: &Path, source: &Path, pseudonyms: &Pseudonyms) -> Result<PathBuf> {
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("{} has no usable file name", source.display()))?;
    let anonymized = pseudonyms.substitute(name);
    if let Some(leaked) = pseudonyms.leak_in(&anonymized) {
        bail!("the fixture file name {anonymized:?} would carry {leaked:?}");
    }
    Ok(directory.join(anonymized))
}

/// Every value a Saxo fixture would be written from, on all three sheets.
///
/// An original standing on `_Transacties` or on `Bookings` leaks as surely as one standing on
/// `Transacties`, so the leak check sees the whole workbook and not its first sheet [TST-031].
fn saxo_values(anonymized: &saxo::Export) -> impl Iterator<Item = String> {
    anonymized
        .sheets
        .iter()
        .flat_map(|sheet| sheet.rows.iter())
        .flat_map(|row| row.iter().map(saxo::Cell::as_text))
}

/// Refuses to write a file in which any collected original still appears.
fn refuse_leaks(
    pseudonyms: &Pseudonyms,
    target: &Path,
    values: impl Iterator<Item = String>,
) -> Result<()> {
    for value in values {
        if let Some(leaked) = pseudonyms.leak_in(&value) {
            bail!(
                "{} would carry {leaked:?} from the real export",
                target.display()
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use pseudonym::Kind;
    use saxo::{Cell, Export, Sheet, SheetKind};

    /// A sheet of `kind` with one row naming `instrument`, every other column empty.
    fn sheet_naming(kind: SheetKind, instrument: &str) -> Sheet {
        let row = kind
            .headers()
            .iter()
            .map(|header| {
                if *header == "Instrument" {
                    Cell::Text(instrument.to_owned())
                } else {
                    Cell::Empty
                }
            })
            .collect();
        Sheet {
            kind,
            rows: vec![row],
        }
    }

    /// The leak check reads the whole workbook: an original left standing on `_Transacties` or on
    /// `Bookings` stops the run exactly as one on `Transacties` does, since all three sheets are
    /// written and all three carry instrument-level detail [TST-012], [TST-031].
    #[test]
    fn a_collected_original_on_any_of_the_three_sheets_refuses_the_fixture() {
        let original = "Sample Account Holder";
        let mut originals = Originals::default();
        originals.add(Kind::InstrumentName, original);
        let pseudonyms = Pseudonyms::build(&originals).expect("the table builds");
        let target = Path::new("fixture.xlsx");

        for leaking in SheetKind::ALL {
            let export = Export {
                sheets: SheetKind::ALL.map(|kind| {
                    let instrument = if kind == leaking {
                        original
                    } else {
                        "Fixture Instrument 00"
                    };
                    sheet_naming(kind, instrument)
                }),
            };

            let refused = refuse_leaks(&pseudonyms, target, saxo_values(&export))
                .expect_err(&format!("a leak on {} was written", leaking.name()));

            assert!(
                refused.to_string().contains(original),
                "{} does not name what leaked on {}",
                refused,
                leaking.name()
            );
        }

        let clean = Export {
            sheets: SheetKind::ALL.map(|kind| sheet_naming(kind, "Fixture Instrument 00")),
        };
        assert!(refuse_leaks(&pseudonyms, target, saxo_values(&clean)).is_ok());
    }
}
