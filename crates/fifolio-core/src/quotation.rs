//! Choosing a security's quotation when an importer creates it [DOM-036].
//!
//! # Defaulted once, from the instrument type
//!
//! An auto-created security takes its quotation from the broker's instrument type: a bond is
//! quoted as a percentage of par, everything else per unit. That happens once, at creation, and
//! is never revisited — a security already in the database keeps whatever it has, whether that
//! came from this rule or from the user correcting it afterwards [DOM-037].
//!
//! "Never guessed afterwards" is the point. A rule that re-derived the quotation on every import
//! would silently undo a correction the first time the file was read again, and the correction
//! exists because the guess can be wrong. [`quotation_for`] is a function of the type alone and
//! takes no security, so re-deriving one is not something a caller can reach for.
//!
//! # What this deliberately does not decide
//!
//! Whether a *particular format's* bond can be given this default at all. Percent-of-par is
//! confirmed for Saxo against real data — a 7.5% bond entered as `3000 @ 139.46` paid coupons of
//! exactly 225.00, which is 7.5% of a nominal 3000, and redeemed for exactly 3000.00 — and is
//! unconfirmed for Trade Republic, which has never been seen to carry a bond.
//!
//! What a format should do about that is open: `OQ-006` in `design/open-questions.md` records
//! that one requirement rejects such an import and another creates the security pending review,
//! that both are normative, and that the trigger is undefined either way. It blocks IMP-TR-020
//! and IMP-TR-021. Answering it here — by taking a per-format flag and returning "undeterminable"
//! — would settle an open question in code, so this module answers from the type and nothing
//! else, and the importer items that own those requirements decide whether to use the answer.
//!
//! # Prices are stored as the statement shows them
//!
//! A quoted price is stored verbatim: `139.46`, not the `1.3946` that already has the factor in
//! it [DOM-039]. A figure in the application can then be held against the broker document it came
//! from, which is what makes an error findable; and the factor stays applied exactly once, where
//! a quoted price becomes a value, rather than being folded in at import where a second
//! application could not be detected.
//!
//! That rule is carried by types and by the importers, not by anything here:
//! `decimal::QuotedPrice` is what a statement shows and `decimal::EffectivePrice` is what a
//! division produces, and an importer divides a booked total by the quotation factor so the price
//! it stores is the quoted one. This module stores no price and so cannot test it; the importer
//! items assert it against a sample document figure.

use crate::entities::{Quotation, SecurityType};

/// The quotation to give a security being auto-created, from its instrument type [DOM-036].
#[must_use]
pub fn quotation_for(security_type: SecurityType) -> Quotation {
    match security_type {
        SecurityType::Bond => Quotation::PercentOfPar,
        SecurityType::Stock
        | SecurityType::Etf
        | SecurityType::Fund
        | SecurityType::Derivative
        | SecurityType::Other => Quotation::PerUnit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{Isin, Security};

    /// A bond is quoted as a percentage of par [DOM-036].
    #[test]
    fn a_bond_defaults_to_percent_of_par() {
        assert_eq!(
            quotation_for(SecurityType::Bond),
            Quotation::PercentOfPar,
            "a nominal 3000 at 139.46% costs 4183.80, not 418,380"
        );
    }

    /// Everything else is quoted per unit [DOM-036], [DOM-005].
    #[test]
    fn everything_but_a_bond_defaults_to_per_unit() {
        for security_type in [
            SecurityType::Stock,
            SecurityType::Etf,
            SecurityType::Fund,
            SecurityType::Derivative,
            SecurityType::Other,
        ] {
            assert_eq!(
                quotation_for(security_type),
                Quotation::PerUnit,
                "{security_type:?} is not quoted as a percentage of par"
            );
        }
    }

    /// The default is a creation-time choice, so a correction survives [DOM-036], [DOM-037].
    ///
    /// The correction is a real one — a bond moved off the default — so a `with_quotation` that
    /// ignored its argument would fail this.
    #[test]
    fn a_correction_is_not_undone_by_the_default() {
        let corrected = Security::auto_created(
            Isin::new("NL0000102077"),
            "An odd bond",
            SecurityType::Bond,
            quotation_for(SecurityType::Bond),
        )
        .with_quotation(Quotation::PerUnit);

        assert_eq!(
            quotation_for(corrected.security_type()),
            Quotation::PercentOfPar,
            "the rule still says what it said"
        );
        assert_eq!(
            corrected.quotation(),
            Quotation::PerUnit,
            "and the stored quotation is the user's, not the rule's"
        );
    }
}
