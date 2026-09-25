//! Which security a Saxo row names, and how that security is quoted.
//!
//! # Three columns, one security
//!
//! `Instrument ISIN` is the key, `Instrument` the name and `Type` the instrument type
//! [IMP-SAXO-021], [IMP-SAXO-022]. All three sit on `Transacties` and on `_Transacties` under the
//! same spellings, so [`Instrument::read`] reads a row of either sheet.
//!
//! `Type` maps by the explicit table in `importers.md` and by nothing else — no prefix match, no
//! case folding, no fallback to [`SecurityType::Other`]. A value outside it **refuses the file**
//! [`SaxoError::UnknownInstrumentType`]: the type decides the quotation, and a bond quoted per
//! unit costs a hundred times what it cost.
//!
//! `Cash` is the one value that names no security at all. It is a non-position row — a deposit,
//! a fee, interest — so [`Instrument::Cash`] carries nothing, and a caller cannot reach a
//! [`Security`] for such a row by any path.
//!
//! # The quotation is defaulted, not decided here
//!
//! A `Bond` is quoted as a percentage of par, which is what makes `3000 @ 138.00` a nominal 3000
//! costing 4140 rather than 414,000. That rule is [`quotation_for`]'s [DOM-036] and is called
//! rather than repeated: a second copy of it here could drift from the first, and the difference
//! between the two is two orders of magnitude on a cost basis.
//!
//! Where the factor is *applied* — whether a value is ever computed as
//! `quantity x unit_price x factor` — is not settled here either. The importer reads what a
//! parcel cost from `_Transacties`' `Verhandelde waarde` [IMP-SAXO-039], which already has the
//! quotation in it; the formula and its use are ARC-008 and DOM-038, open under OQ-010.
//!
//! # A name changes, an ISIN does not
//!
//! Names carry delisting annotations — `*Delisted 20231011 (...)` — and change over time for one
//! ISIN [IMP-SAXO-022]. [`securities`] therefore keys on the ISIN alone: the corpus holds one
//! instrument under both `*Delisted 20231002 (Fixture Instrument 07)` and its plain name, and
//! that is one security, not two.
//!
//! Two points the specification does not settle, decided here so that nothing is invented:
//!
//! * **which name a security keeps** when its rows disagree: the first row's, in the order the
//!   rows are given. A Saxo export is newest first [IMP-SAXO-026], so on an unsorted file that is
//!   the most recent name the broker used, and re-naming a security on a later row would let the
//!   oldest row in the file decide what the user sees;
//! * **an ISIN stated with two instrument types** refuses the file
//!   [`SaxoError::MixedSecurityType`] rather than keeping the first. One instrument cannot be a
//!   stock and a bond, the two quote differently, and choosing either would book a cost basis
//!   that is wrong by a factor of a hundred. It is the reading this importer already takes where
//!   two cells state the same fact and disagree — a quantity [IMP-SAXO-038], a leg's currency
//!   [IMP-SAXO-047] — and no row of the five-year corpus triggers it.
//!
//! A row whose `Type` is not `Cash` and whose `Instrument ISIN` is blank refuses the file too
//! [`SaxoError::NoIsin`]: the key is what a security is looked up and stored by [DOM-071], so a
//! position row without one has no security to be about.

use super::{SaxoError, field};
use crate::entities::{Isin, Security, SecurityType};
use crate::import::reader::SourceRow;
use crate::quotation::quotation_for;

/// The security's name, which is not its key [IMP-SAXO-022].
const INSTRUMENT: &str = "Instrument";

/// The security's key [IMP-SAXO-022].
const INSTRUMENT_ISIN: &str = "Instrument ISIN";

/// The broker's instrument type [IMP-SAXO-021].
const TYPE: &str = "Type";

/// The one `Type` that names no security [IMP-SAXO-021].
const CASH: &str = "Cash";

/// What a row's `Type` names [IMP-SAXO-021].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instrument {
    /// The row is about a security, auto-created from the row's three instrument columns and
    /// flagged for review [DOM-006].
    Security(Security),
    /// `Type` is `Cash`: no security is created and the row moves no position.
    Cash,
}

impl Instrument {
    /// The instrument a `Transacties` or `_Transacties` row names [IMP-SAXO-021],
    /// [IMP-SAXO-022].
    ///
    /// # Errors
    ///
    /// When the row carries no `Type` column; when its `Type` is outside the table, which
    /// refuses the file; and when a row naming a security carries a blank `Instrument ISIN`.
    pub fn read(row: &SourceRow) -> Result<Self, SaxoError> {
        let value = field(row, TYPE).ok_or_else(|| SaxoError::MissingColumn {
            header: TYPE.to_owned(),
        })?;
        let stated = value.trim();
        if stated == CASH {
            return Ok(Self::Cash);
        }
        let security_type =
            security_type(stated).ok_or_else(|| SaxoError::UnknownInstrumentType {
                value: stated.to_owned(),
            })?;

        let name = field(row, INSTRUMENT).unwrap_or_default().trim();
        let isin = field(row, INSTRUMENT_ISIN).unwrap_or_default().trim();
        if isin.is_empty() {
            return Err(SaxoError::NoIsin {
                name: name.to_owned(),
            });
        }

        Ok(Self::Security(Security::auto_created(
            Isin::new(isin),
            name,
            security_type,
            quotation_for(security_type),
        )))
    }
}

/// The securities `rows` name, one per ISIN, in the order the rows first name them
/// [IMP-SAXO-022].
///
/// A row whose `Type` is `Cash` contributes none, and a second row naming an ISIN already seen
/// contributes none either, however it spells the name.
///
/// # Errors
///
/// When a row's instrument cannot be read, and when two rows state one ISIN with two instrument
/// types.
pub fn securities<'a>(
    rows: impl IntoIterator<Item = &'a SourceRow>,
) -> Result<Vec<Security>, SaxoError> {
    rows.into_iter()
        .try_fold(Vec::new(), |mut found: Vec<Security>, row| {
            let Instrument::Security(security) = Instrument::read(row)? else {
                return Ok(found);
            };
            match found.iter().find(|seen| seen.isin() == security.isin()) {
                Some(seen) if seen.security_type() != security.security_type() => {
                    Err(SaxoError::MixedSecurityType {
                        isin: security.isin().as_str().to_owned(),
                        first: seen.security_type(),
                        second: security.security_type(),
                    })
                }
                Some(_) => Ok(found),
                None => {
                    found.push(security);
                    Ok(found)
                }
            }
        })
}

/// The security type a Saxo `Type` maps to, or `None` for [`CASH`] and for a value outside the
/// table [IMP-SAXO-021].
fn security_type(value: &str) -> Option<SecurityType> {
    match value {
        "Stock" => Some(SecurityType::Stock),
        "Bond" => Some(SecurityType::Bond),
        "Etf" => Some(SecurityType::Etf),
        "MutualFund" => Some(SecurityType::Fund),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::Quotation;
    use crate::import::saxo::Sheet;

    /// A row of `sheet` spelled as the file spells it, with only the named columns populated.
    fn row(sheet: Sheet, values: &[(&str, &str)]) -> SourceRow {
        let columns = sheet
            .headers()
            .iter()
            .map(|header| {
                let value = values
                    .iter()
                    .find(|(name, _)| name.split_whitespace().eq(header.split_whitespace()))
                    .map_or("", |(_, value)| *value);
                ((*header).to_owned(), value.to_owned())
            })
            .collect();
        SourceRow::new(columns, "")
    }

    /// A `Transacties` row naming an instrument.
    fn instrument_row(isin: &str, name: &str, instrument_type: &str) -> SourceRow {
        row(
            Sheet::Transacties,
            &[
                ("Instrument ISIN", isin),
                ("Instrument", name),
                ("Type", instrument_type),
            ],
        )
    }

    /// The security a row names, or a panic naming why it could not be read.
    fn security(row: &SourceRow) -> Security {
        match Instrument::read(row).expect("the row's instrument reads") {
            Instrument::Security(security) => security,
            Instrument::Cash => panic!("the row names a security"),
        }
    }

    /// The four typed rows of the table map onto their security types [IMP-SAXO-021].
    #[test]
    fn each_instrument_type_maps_onto_its_security_type() {
        let mapped = [
            ("Stock", SecurityType::Stock),
            ("Bond", SecurityType::Bond),
            ("Etf", SecurityType::Etf),
            ("MutualFund", SecurityType::Fund),
        ];

        for (stated, expected) in mapped {
            assert_eq!(
                security(&instrument_row("XF0000000103", "An instrument", stated)).security_type(),
                expected,
                "Saxo's {stated} is not mapped by the table"
            );
        }
    }

    /// A security an import creates is flagged for review [DOM-006], [IMP-SAXO-021].
    #[test]
    fn a_row_creates_the_security_it_names_and_flags_it() {
        let created = security(&instrument_row(
            "XF0000000103",
            "Fixture Instrument 04",
            "Stock",
        ));

        assert_eq!(created.isin(), &Isin::new("XF0000000103"));
        assert_eq!(created.name(), "Fixture Instrument 04");
        assert!(
            created.is_auto_created(),
            "an importer created it, not the user"
        );
    }

    /// A bond is quoted as a percentage of par and everything else per unit, through the one
    /// rule that decides it [IMP-SAXO-020], [DOM-036].
    #[test]
    fn a_bond_is_quoted_as_a_percentage_of_par() {
        assert_eq!(
            security(&instrument_row("XF0000000103", "A bond", "Bond")).quotation(),
            quotation_for(SecurityType::Bond),
            "the bond's quotation is not the default its type gives"
        );
        assert_eq!(
            security(&instrument_row("XF0000000103", "A bond", "Bond")).quotation(),
            Quotation::PercentOfPar,
            "a nominal 3000 at 138.00% costs 4140, not 414,000"
        );
        assert_eq!(
            security(&instrument_row("XF0000000145", "A share", "Stock")).quotation(),
            Quotation::PerUnit,
        );
    }

    /// `Cash` names no security and the row moves no position [IMP-SAXO-021].
    #[test]
    fn a_cash_row_names_no_security() {
        let cash = row(Sheet::Transacties, &[("Type", "Cash")]);

        assert_eq!(
            Instrument::read(&cash).expect("a cash row reads"),
            Instrument::Cash
        );
        assert_eq!(
            securities([&cash]).expect("a cash row reads"),
            Vec::new(),
            "a cash row creates nothing"
        );
    }

    /// A `Type` outside the table refuses the file rather than falling back [IMP-SAXO-021].
    #[test]
    fn an_instrument_type_outside_the_table_is_refused() {
        for stated in ["Cfd", "CASH", "stock", ""] {
            assert_eq!(
                Instrument::read(&instrument_row("XF0000000103", "An instrument", stated)),
                Err(SaxoError::UnknownInstrumentType {
                    value: stated.to_owned()
                }),
                "{stated:?} is not a value the table maps"
            );
        }
    }

    /// A row naming a security and no ISIN is refused: the ISIN is the key [IMP-SAXO-022].
    #[test]
    fn a_security_row_without_an_isin_is_refused() {
        assert_eq!(
            Instrument::read(&instrument_row("", "An instrument", "Stock")),
            Err(SaxoError::NoIsin {
                name: "An instrument".to_owned()
            })
        );
    }

    /// One ISIN under two names is one security, and the first name is the one it keeps
    /// [IMP-SAXO-022].
    #[test]
    fn a_renamed_instrument_resolves_to_one_security() {
        let rows = [
            instrument_row(
                "XF0000000145",
                "*Delisted 20231002 (Fixture Instrument 07)",
                "Stock",
            ),
            instrument_row("XF0000000145", "Fixture Instrument 08", "Stock"),
        ];

        let found = securities(&rows).expect("both rows read");

        assert_eq!(found.len(), 1, "one ISIN is one security");
        assert_eq!(
            found[0].name(),
            "*Delisted 20231002 (Fixture Instrument 07)",
            "the first row's name is the one kept"
        );
    }

    /// One ISIN stated as two instrument types refuses the file rather than choosing
    /// [IMP-SAXO-021], [IMP-SAXO-022].
    #[test]
    fn one_isin_with_two_instrument_types_is_refused() {
        let rows = [
            instrument_row("XF0000000103", "An instrument", "Stock"),
            instrument_row("XF0000000103", "An instrument", "Bond"),
        ];

        assert_eq!(
            securities(&rows),
            Err(SaxoError::MixedSecurityType {
                isin: "XF0000000103".to_owned(),
                first: SecurityType::Stock,
                second: SecurityType::Bond,
            })
        );
    }

    /// The same three columns are read off a `_Transacties` row [IMP-SAXO-021].
    #[test]
    fn a_detail_row_names_its_instrument_too() {
        let leg = row(
            Sheet::Detail,
            &[
                ("Instrument ISIN", "XF0000000103"),
                ("Instrument", "Fixture Instrument 04"),
                ("Type", "Bond"),
            ],
        );

        assert_eq!(security(&leg).quotation(), Quotation::PercentOfPar);
    }

    /// A row carrying no `Type` column at all is a caller naming the wrong sheet's columns.
    #[test]
    fn a_row_without_the_type_column_is_reported() {
        let empty = SourceRow::new(Vec::new(), "");

        assert_eq!(
            Instrument::read(&empty),
            Err(SaxoError::MissingColumn {
                header: TYPE.to_owned()
            })
        );
    }
}
