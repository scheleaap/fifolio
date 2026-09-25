//! What a Trade Republic row is: derived, refused, or not stored [IMP-TR-008].
//!
//! `category` is `TRADING`, `CASH` or `CORPORATE_ACTION`, and `type` refines it. The table is
//! keyed on the pair as the file states it, never on a quantity's sign or on the row's shape
//! [IMP-TR-009]: an event resembling a lot transfer is not thereby one, and a miscategorized
//! position event corrupts a cost basis permanently (DEC-019).
//!
//! | `category` / `type` | Handling |
//! | --- | --- |
//! | `TRADING` / `BUY`, `SELL` | derived automatically |
//! | `CORPORATE_ACTION` / `TAX_EXCHANGE` | FIF-068's; refused until it lands |
//! | `CORPORATE_ACTION` / anything else | refused [IMP-TR-010] |
//! | `CASH` / `DIVIDEND`, `INTEREST_PAYMENT`, `CUSTOMER_INBOUND`, `TRANSFER_INBOUND`, `STOCKPERK` | non-position, not stored [IMP-TR-011], [IMP-TR-012] |
//! | anything else naming a security | refused [IMP-TR-013] |
//! | anything else naming no security | not stored, named in the import's summary [IMP-TR-014] |
//!
//! "Naming a security" is a populated `symbol`, which is what made `STOCKPERK` dangerous despite
//! arriving as a `CASH` row (DEC-022).
//!
//! Trade Republic states share quantities on its corporate actions, so the Saxo dividend
//! heuristic keyed on `Positie-ID` has nothing to do here and is not applied [IMP-TR-009].
//!
//! # A stored row carries no foreign side
//!
//! `fx_rate` changed convention in late 2024 and has none a reader could apply (DEC-073). Every
//! foreign-currency row observed is a dividend, which is not stored, so a row that *would* be
//! stored with any of `original_amount`, `original_currency` or `fx_rate` populated refuses the
//! import rather than being valued on a guess [IMP-TR-017]. A non-stored row is never read for
//! its money at all, so the fixtures' foreign dividends import untouched.

use super::money::Booked;
use super::{IDENTITY_COLUMN, TradeRepublicError, field};
use crate::import::reader::SourceRow;
use crate::import::{NonPositionKind, NonPositionReason, RowClassification};

const CATEGORY: &str = "category";
const TYPE: &str = "type";

/// The instrument a row names, blank on a row that names none (DEC-022).
const SYMBOL: &str = "symbol";

/// What `row` is [IMP-TR-008].
///
/// A row on its own suffices: the one classification that needs a neighbour, the `STOCKPERK`
/// credit's paired `TRADING` / `BUY`, is deliberately not checked [IMP-TR-011].
///
/// # Errors
///
/// When the row is of a type the table refuses — a corporate action other than `TAX_EXCHANGE`
/// [IMP-TR-010], or an unrecognized type naming a security [IMP-TR-013] — or is a `TAX_EXCHANGE`,
/// which is not imported yet (FIF-068); when a row that would be stored carries a foreign side
/// [IMP-TR-017] or money that cannot be read [IMP-TR-005]; or when a column is missing.
pub fn classify(row: &SourceRow) -> Result<RowClassification, TradeRepublicError> {
    let category = field(row, CATEGORY)?;
    let kind = field(row, TYPE)?;
    let transaction_id = || field(row, IDENTITY_COLUMN).unwrap_or_default().to_owned();
    let recognized = |kind| {
        Ok(RowClassification::NonPosition(
            NonPositionReason::Recognized(kind),
        ))
    };

    match (category, kind) {
        ("TRADING", "BUY" | "SELL") => {
            refuse_foreign_side(row)?;
            Ok(RowClassification::DerivedAutomatically)
        }
        // Refused rather than stored as pending: pending means the export lacks something only
        // the user knows, which is not true of this pair, and FIF-068 decides what it becomes.
        ("CORPORATE_ACTION", "TAX_EXCHANGE") => Err(TradeRepublicError::TaxExchangeNotImported {
            transaction_id: transaction_id(),
        }),
        ("CORPORATE_ACTION", other) => Err(TradeRepublicError::UnknownCorporateAction {
            kind: other.to_owned(),
            transaction_id: transaction_id(),
        }),
        ("CASH", "DIVIDEND") => recognized(NonPositionKind::CashDividend),
        ("CASH", "INTEREST_PAYMENT") => recognized(NonPositionKind::Interest),
        // TRANSFER_INBOUND is cash arriving from another account, indistinguishable in effect
        // from a deposit [IMP-TR-012].
        ("CASH", "CUSTOMER_INBOUND" | "TRANSFER_INBOUND") => recognized(NonPositionKind::Deposit),
        // A promotional credit that funds its paired buy, so it nets to zero cash; the buy
        // carries the acquisition [IMP-TR-011]. Of DOM-002's kinds, cash arriving is a deposit.
        ("CASH", "STOCKPERK") => recognized(NonPositionKind::Deposit),
        (category, kind) => match field(row, SYMBOL)? {
            "" => Ok(RowClassification::NonPosition(
                NonPositionReason::UnrecognizedType(format!("{category}/{kind}")),
            )),
            symbol => Err(TradeRepublicError::UnrecognizedTypeNamingSecurity {
                category: category.to_owned(),
                kind: kind.to_owned(),
                symbol: symbol.to_owned(),
                transaction_id: transaction_id(),
            }),
        },
    }
}

/// Refuses a row that would be stored carrying a foreign side [IMP-TR-017].
///
/// Any one of the three columns refuses it: the rate is the one that cannot be trusted, but a
/// foreign amount or currency on a stored row means a figure in another currency that a later
/// step would need that rate to value.
fn refuse_foreign_side(row: &SourceRow) -> Result<(), TradeRepublicError> {
    let booked = Booked::read(row)?;
    let foreign = booked.original_amount().is_some()
        || booked.original_currency().is_some()
        || booked.fx_rate().is_some();
    if foreign {
        return Err(TradeRepublicError::ForeignSideOnStoredRow {
            transaction_id: field(row, IDENTITY_COLUMN)?.to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::tests::row;
    use super::*;

    const ID: &str = "bf751ce3-33c9-539c-96d7-1428cc7bdde9";

    fn classified(values: &[(&str, &str)]) -> Result<RowClassification, TradeRepublicError> {
        classify(&row(values))
    }

    fn non_position(kind: NonPositionKind) -> Result<RowClassification, TradeRepublicError> {
        Ok(RowClassification::NonPosition(
            NonPositionReason::Recognized(kind),
        ))
    }

    /// A buy and a sell derive automatically [IMP-TR-008].
    #[test]
    fn a_buy_and_a_sell_derive_automatically() {
        for kind in ["BUY", "SELL"] {
            assert_eq!(
                classified(&[
                    (CATEGORY, "TRADING"),
                    (TYPE, kind),
                    (SYMBOL, "XF0000000152"),
                    ("shares", "35.0000000000"),
                    ("amount", "-2628.150000"),
                    ("currency", "EUR"),
                    (IDENTITY_COLUMN, ID),
                ]),
                Ok(RowClassification::DerivedAutomatically),
                "{kind}"
            );
        }
    }

    /// The five cash types are non-position and stored nowhere, each as the kind of DOM-002 it
    /// is [IMP-TR-008], [IMP-TR-012], [DOM-002].
    #[test]
    fn the_five_cash_types_are_non_position() {
        let cases = [
            ("DIVIDEND", NonPositionKind::CashDividend),
            ("INTEREST_PAYMENT", NonPositionKind::Interest),
            ("CUSTOMER_INBOUND", NonPositionKind::Deposit),
            ("TRANSFER_INBOUND", NonPositionKind::Deposit),
            ("STOCKPERK", NonPositionKind::Deposit),
        ];
        for (kind, expected) in cases {
            assert_eq!(
                classified(&[(CATEGORY, "CASH"), (TYPE, kind), (IDENTITY_COLUMN, ID)]),
                non_position(expected),
                "{kind}"
            );
        }
    }

    /// A `STOCKPERK` credit names a security and is still not stored, alone in its file: the
    /// paired buy is not looked for [IMP-TR-011].
    #[test]
    fn a_stockperk_without_its_paired_buy_is_non_position() {
        assert_eq!(
            classified(&[
                (CATEGORY, "CASH"),
                (TYPE, "STOCKPERK"),
                (SYMBOL, "XF0000000152"),
                ("amount", "16.200000"),
                ("currency", "EUR"),
                (IDENTITY_COLUMN, ID),
            ]),
            non_position(NonPositionKind::Deposit)
        );
    }

    /// A foreign dividend is not stored, so its foreign side refuses nothing [IMP-TR-017].
    #[test]
    fn a_foreign_dividend_is_non_position() {
        assert_eq!(
            classified(&[
                (CATEGORY, "CASH"),
                (TYPE, "DIVIDEND"),
                (SYMBOL, "XF0000000152"),
                ("amount", "0.032946"),
                ("currency", "EUR"),
                ("original_amount", "0.05"),
                ("original_currency", "USD"),
                ("fx_rate", "0.860751"),
                (IDENTITY_COLUMN, ID),
            ]),
            non_position(NonPositionKind::CashDividend)
        );
    }

    /// A corporate action of any type but `TAX_EXCHANGE` refuses the import, naming the type and
    /// the transaction id — including one shaped exactly like a lot transfer, since nothing is
    /// inferred from quantity signs [IMP-TR-010], [IMP-TR-009].
    #[test]
    fn an_unknown_corporate_action_is_refused_whatever_its_quantities() {
        for shares in ["-60.0000000000", "60.0000000000", ""] {
            assert_eq!(
                classified(&[
                    (CATEGORY, "CORPORATE_ACTION"),
                    (TYPE, "SPIN_OFF"),
                    (SYMBOL, "LU1861134382"),
                    ("shares", shares),
                    (IDENTITY_COLUMN, ID),
                ]),
                Err(TradeRepublicError::UnknownCorporateAction {
                    kind: "SPIN_OFF".to_owned(),
                    transaction_id: ID.to_owned(),
                }),
                "shares {shares:?}"
            );
        }
        // A corporate action naming no security is refused too: the category rule comes before
        // the one keyed on the symbol [IMP-TR-010].
        assert!(matches!(
            classified(&[
                (CATEGORY, "CORPORATE_ACTION"),
                (TYPE, "SPIN_OFF"),
                (IDENTITY_COLUMN, ID)
            ]),
            Err(TradeRepublicError::UnknownCorporateAction { .. })
        ));
    }

    /// A `TAX_EXCHANGE` is FIF-068's, and is refused rather than stored in a shape that item
    /// would have to undo.
    #[test]
    fn a_tax_exchange_is_not_imported_yet() {
        assert_eq!(
            classified(&[
                (CATEGORY, "CORPORATE_ACTION"),
                (TYPE, "TAX_EXCHANGE"),
                (SYMBOL, "LU1861134382"),
                ("shares", "-60.0000000000"),
                (IDENTITY_COLUMN, ID),
            ]),
            Err(TradeRepublicError::TaxExchangeNotImported {
                transaction_id: ID.to_owned(),
            })
        );
    }

    /// An unrecognized type naming a security refuses the import, whatever its category and
    /// whatever its quantity says [IMP-TR-013], [IMP-TR-009]. The table is keyed on the pair: a
    /// recognized type under another category is not recognized.
    #[test]
    fn an_unrecognized_type_naming_a_security_is_refused() {
        for (category, kind) in [
            ("CASH", "SAVEBACK"),
            ("TRADING", "SAVINGS_PLAN"),
            ("TRADING", "DIVIDEND"),
            ("CASH", "BUY"),
        ] {
            assert_eq!(
                classified(&[
                    (CATEGORY, category),
                    (TYPE, kind),
                    (SYMBOL, "XF0000000152"),
                    ("shares", "1.0000000000"),
                    (IDENTITY_COLUMN, ID),
                ]),
                Err(TradeRepublicError::UnrecognizedTypeNamingSecurity {
                    category: category.to_owned(),
                    kind: kind.to_owned(),
                    symbol: "XF0000000152".to_owned(),
                    transaction_id: ID.to_owned(),
                }),
                "{category}/{kind}"
            );
        }
    }

    /// An unrecognized type naming no security is taken as cash and named by its category and
    /// type, a populated quantity notwithstanding [IMP-TR-014], [IMP-TR-009].
    #[test]
    fn an_unrecognized_type_naming_no_security_is_named() {
        for shares in ["", "3.0000000000", "-3.0000000000"] {
            assert_eq!(
                classified(&[
                    (CATEGORY, "CASH"),
                    (TYPE, "CARD_TRANSACTION"),
                    ("shares", shares),
                    ("amount", "-12.50"),
                    (IDENTITY_COLUMN, ID),
                ]),
                Ok(RowClassification::NonPosition(
                    NonPositionReason::UnrecognizedType("CASH/CARD_TRANSACTION".to_owned())
                )),
                "shares {shares:?}"
            );
        }
    }

    /// A buy or a sell carrying any one of the three foreign columns refuses the import naming
    /// the row, on a constructed row: no export carries one [IMP-TR-017].
    #[test]
    fn a_stored_row_with_a_foreign_side_is_refused() {
        for kind in ["BUY", "SELL"] {
            for (column, value) in [
                ("original_amount", "-3000.00"),
                ("original_currency", "USD"),
                ("fx_rate", "1.0876"),
            ] {
                assert_eq!(
                    classified(&[
                        (CATEGORY, "TRADING"),
                        (TYPE, kind),
                        (SYMBOL, "XF0000000152"),
                        ("shares", "35.0000000000"),
                        ("amount", "-2628.150000"),
                        ("currency", "EUR"),
                        (column, value),
                        (IDENTITY_COLUMN, ID),
                    ]),
                    Err(TradeRepublicError::ForeignSideOnStoredRow {
                        transaction_id: ID.to_owned(),
                    }),
                    "{kind} with {column}"
                );
            }
        }
    }

    /// A stored row whose money cannot be read is a failed row, not a derived one [IMP-TR-005].
    #[test]
    fn a_stored_row_with_unreadable_money_is_refused() {
        assert!(matches!(
            classified(&[
                (CATEGORY, "TRADING"),
                (TYPE, "BUY"),
                ("amount", "2.628,15"),
                (IDENTITY_COLUMN, ID),
            ]),
            Err(TradeRepublicError::NotAnAmount { .. })
        ));
    }
}
