//! Which security a Trade Republic row names, and what type it is [IMP-TR-020], [IMP-TR-021].
//!
//! `symbol` is the ISIN and the key, `name` the name, and `asset_class` the broker's class.
//! A row with a blank `symbol` names no security (DEC-022), whatever else it carries, so
//! [`Instrument::None`] carries nothing: a deposit or an interest payment states no
//! `asset_class`, and reading that blank against the table would refuse every export.
//!
//! `asset_class` maps by the explicit table in `importers.md` and by nothing else — no case
//! folding, no fallback to [`SecurityType::Other`]. A value outside it, blank included,
//! **refuses the file** [`TradeRepublicError::UnknownAssetClass`], naming the ISIN.
//!
//! Trade Republic states an ETF as `FUND`, so an ETF is recorded as a fund. Nothing in v1 turns
//! on the distinction, and the security is auto-created and so flagged for review, which is
//! where the user corrects it [IMP-TR-020], [DOM-006].

use super::{TradeRepublicError, field};
use crate::entities::{Isin, Security, SecurityType};
use crate::import::reader::SourceRow;
use crate::quotation::quotation_for;

/// The broker's class of instrument [IMP-TR-020].
const ASSET_CLASS: &str = "asset_class";

/// The security's name, which is not its key.
const NAME: &str = "name";

/// The security's ISIN, blank on a row that names none (DEC-022).
const SYMBOL: &str = "symbol";

/// What a row's `symbol` names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instrument {
    /// The row is about a security, auto-created from the row and flagged for review [DOM-006].
    Security(Security),
    /// `symbol` is blank: the row names no security and no type is read.
    None,
}

impl Instrument {
    /// The instrument `row` names [IMP-TR-020], [IMP-TR-021].
    ///
    /// # Errors
    ///
    /// When a row naming a security states an `asset_class` outside the table, which refuses
    /// the file; and when a column is missing.
    pub fn read(row: &SourceRow) -> Result<Self, TradeRepublicError> {
        let isin = field(row, SYMBOL)?.trim();
        if isin.is_empty() {
            return Ok(Self::None);
        }
        let stated = field(row, ASSET_CLASS)?;
        let security_type =
            security_type(stated).ok_or_else(|| TradeRepublicError::UnknownAssetClass {
                value: stated.to_owned(),
                isin: isin.to_owned(),
            })?;

        Ok(Self::Security(Security::auto_created(
            Isin::new(isin),
            field(row, NAME)?.trim(),
            security_type,
            quotation_for(security_type),
        )))
    }
}

/// The security type an `asset_class` maps to, or `None` for a value outside the table
/// [IMP-TR-020].
///
/// No value maps to [`SecurityType::Bond`], and none may be added (DEC-077, IMP-TR-021). A bond
/// is quoted as a percentage of par, but that convention is confirmed only for Saxo: no Trade
/// Republic bond has been observed, so its quotation cannot be established, and guessing either
/// way books a cost basis wrong by a factor of a hundred. Quotation has no unknown state that
/// could park such a bond pending review (DEC-041), so the catch-all below rejecting the import
/// is the rule, not a gap.
fn security_type(value: &str) -> Option<SecurityType> {
    match value {
        "STOCK" => Some(SecurityType::Stock),
        "FUND" => Some(SecurityType::Fund),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::row;
    use super::*;
    use crate::entities::Quotation;

    const ISIN: &str = "XF0000000152";

    fn instrument_row(isin: &str, asset_class: &str) -> SourceRow {
        row(&[
            ("category", "TRADING"),
            ("type", "BUY"),
            (SYMBOL, isin),
            (NAME, "Fixture Instrument 09"),
            (ASSET_CLASS, asset_class),
        ])
    }

    fn security(row: &SourceRow) -> Security {
        match Instrument::read(row).expect("the row's instrument reads") {
            Instrument::Security(security) => security,
            Instrument::None => panic!("the row names a security"),
        }
    }

    /// `STOCK` maps to a stock and `FUND` to a fund, both quoted per unit [IMP-TR-020].
    #[test]
    fn each_asset_class_maps_onto_its_security_type() {
        for (stated, expected) in [("STOCK", SecurityType::Stock), ("FUND", SecurityType::Fund)] {
            let mapped = security(&instrument_row(ISIN, stated));
            assert_eq!(mapped.security_type(), expected, "{stated}");
            assert_eq!(mapped.quotation(), Quotation::PerUnit, "{stated}");
        }
    }

    /// An ETF arrives as `FUND`, is recorded as a fund and is flagged as auto-created, which is
    /// what puts it in front of the user for review [IMP-TR-020], [DOM-006].
    #[test]
    fn an_etf_is_recorded_as_a_fund_and_flagged() {
        let etf = security(&instrument_row("IE000Y77LGG9", "FUND"));

        assert_eq!(etf.isin(), &Isin::new("IE000Y77LGG9"));
        assert_eq!(etf.name(), "Fixture Instrument 09");
        assert_eq!(etf.security_type(), SecurityType::Fund);
        assert!(
            etf.is_auto_created(),
            "an importer created it, not the user"
        );
    }

    /// Any other `asset_class` refuses the file, naming the ISIN [IMP-TR-020].
    #[test]
    fn an_asset_class_outside_the_table_is_refused() {
        for stated in ["CRYPTO", "DERIVATIVE", "ETF", "stock", "Fund", ""] {
            assert_eq!(
                Instrument::read(&instrument_row(ISIN, stated)),
                Err(TradeRepublicError::UnknownAssetClass {
                    value: stated.to_owned(),
                    isin: ISIN.to_owned(),
                }),
                "{stated:?} is not a value the table maps"
            );
        }
    }

    /// A bond is refused by the catch-all, naming its ISIN, and is not created pending review:
    /// no `asset_class` maps to `bond` [IMP-TR-021], (DEC-077).
    #[test]
    fn a_bond_is_refused_and_never_created() {
        for stated in ["BOND", "BONDS", "Bond"] {
            let read = Instrument::read(&instrument_row("XF0000000103", stated));
            assert_eq!(
                read,
                Err(TradeRepublicError::UnknownAssetClass {
                    value: stated.to_owned(),
                    isin: "XF0000000103".to_owned(),
                }),
                "{stated:?}"
            );
            assert!(
                read.unwrap_err().to_string().contains("XF0000000103"),
                "the refusal names the ISIN"
            );
        }
    }

    /// A row with a blank `symbol` names no security, so its blank `asset_class` refuses nothing
    /// (DEC-022).
    #[test]
    fn a_row_naming_no_security_reads_no_type() {
        let deposit = row(&[("category", "CASH"), ("type", "CUSTOMER_INBOUND")]);

        assert_eq!(Instrument::read(&deposit), Ok(Instrument::None));
    }
}
