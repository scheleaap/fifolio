//! Turning domain values into the columns that hold them, and back.
//!
//! # Scale is enforced here, not hoped for
//!
//! Persistence is one of the two boundaries at which a value must already be at its scale
//! [ARC-009, ARC-010]. [`at_scale`] is the only way a decimal reaches a column, and it refuses a
//! value carrying more decimals than its kind: a price at 8 decimals silently truncated on the
//! way in is a cost basis that no longer reconciles with the document it came from. Refusing is
//! what makes the boundary real rather than documented.
//!
//! The text it writes is the value at its kind's scale, padded where the scale is exact
//! [ARC-007], so `1825.5` is stored as `1825.50` and every stored amount reads alike.

use std::str::FromStr;

use rust_decimal::Decimal;

use crate::decimal::{FxRate, Money, Quantity, QuotedPrice, Scaled};
use crate::entities::{Quotation, SecurityType, SourceFormat};
use crate::storage::StorageError;
use crate::transaction::{BuyOrigin, DateProvenance, TransferInSource};
use crate::valuation::{RateSource, Valued};

/// `value` as the text a column holds, or an error if it is not already at its kind's scale.
pub(super) fn at_scale<T: Scaled>(field: &'static str, value: T) -> Result<String, StorageError> {
    let rounded = value.rounded();
    if rounded.get() != value.get() {
        return Err(StorageError::UnscaledValue {
            field,
            value: value.get().to_string(),
            scale: T::SCALE,
        });
    }
    Ok(rounded.get().to_string())
}

/// Both halves of a pair, native first [DOM-029].
pub(super) fn pair_at_scale<T: Scaled>(
    field: &'static str,
    value: Valued<T>,
) -> Result<(String, String), StorageError> {
    Ok((
        at_scale(field, value.native())?,
        at_scale(field, value.eur())?,
    ))
}

fn decimal(field: &'static str, stored: &str) -> Result<Decimal, StorageError> {
    Decimal::from_str(stored).map_err(|_| StorageError::CorruptValue {
        field,
        value: stored.to_owned(),
    })
}

/// Rebuilds a decimal kind from its column.
pub(super) fn from_stored<T: Scaled>(field: &'static str, stored: &str) -> Result<T, StorageError> {
    Ok(T::from_decimal(decimal(field, stored)?))
}

/// Rebuilds a pair from its two columns.
pub(super) fn pair_from_stored<T: Scaled>(
    field: &'static str,
    native: &str,
    eur: &str,
) -> Result<Valued<T>, StorageError> {
    Ok(Valued::new(
        from_stored(field, native)?,
        from_stored(field, eur)?,
    ))
}

/// Type-annotated aliases, so a call site says which kind it is reading back.
pub(super) fn quantity(field: &'static str, stored: &str) -> Result<Quantity, StorageError> {
    from_stored(field, stored)
}

pub(super) fn money_pair(
    field: &'static str,
    native: &str,
    eur: &str,
) -> Result<Valued<Money>, StorageError> {
    pair_from_stored(field, native, eur)
}

pub(super) fn price_pair(
    field: &'static str,
    native: &str,
    eur: &str,
) -> Result<Valued<QuotedPrice>, StorageError> {
    pair_from_stored(field, native, eur)
}

pub(super) fn rate(field: &'static str, stored: &str) -> Result<FxRate, StorageError> {
    from_stored(field, stored)
}

/// Defines the two directions of an enum's stored code.
///
/// The codes are written out rather than derived from the variant name, because a rename in Rust
/// must not silently reinterpret rows already on disk.
macro_rules! coded_enum {
    ($to:ident, $from:ident, $type:ty, $field:literal, { $($variant:path => $code:literal),+ $(,)? }) => {
        pub(super) fn $to(value: $type) -> &'static str {
            match value {
                $($variant => $code,)+
            }
        }

        pub(super) fn $from(stored: &str) -> Result<$type, StorageError> {
            match stored {
                $($code => Ok($variant),)+
                other => Err(StorageError::CorruptValue {
                    field: $field,
                    value: other.to_owned(),
                }),
            }
        }
    };
}

coded_enum!(security_type_code, security_type, SecurityType, "security_type", {
    SecurityType::Stock => "stock",
    SecurityType::Bond => "bond",
    SecurityType::Etf => "etf",
    SecurityType::Fund => "fund",
    SecurityType::Derivative => "derivative",
    SecurityType::Other => "other",
});

coded_enum!(quotation_code, quotation, Quotation, "quotation", {
    Quotation::PerUnit => "per_unit",
    Quotation::PercentOfPar => "percent_of_par",
});

coded_enum!(source_format_code, source_format, SourceFormat, "format", {
    SourceFormat::SaxoNlXlsx => "saxo_nl_xlsx",
    SourceFormat::TradeRepublicDeCsv => "trade_republic_de_csv",
});

coded_enum!(buy_origin_code, buy_origin, BuyOrigin, "origin", {
    BuyOrigin::Purchase => "purchase",
    BuyOrigin::StockDividend => "stock_dividend",
});

coded_enum!(transfer_in_source_code, transfer_in_source, TransferInSource, "source", {
    TransferInSource::Broker => "broker",
    TransferInSource::CorporateAction => "corporate_action",
});

coded_enum!(date_provenance_code, date_provenance, DateProvenance, "date_provenance", {
    DateProvenance::TransferDate => "transfer_date",
    DateProvenance::Inherited => "inherited",
});

coded_enum!(rate_source_code, rate_source, RateSource, "conversion_source", {
    RateSource::Broker => "broker",
    RateSource::Ecb => "ecb",
    RateSource::Native => "native",
});
