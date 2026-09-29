//! The acquisition report: every opening transaction, what remains of it and what it has realized
//! [DOM-076], [DOM-077], with no tax law applied to it [DOM-001]. The per-disposal lines beneath
//! each opening are FIF-082's [DOM-078].
//!
//! # Rows
//!
//! One per opening, `buy` or `transfer_in`, in canonical order. A `transfer_in` is an opening in its
//! own right; an emitted one names the opening whose parcel it carries, so a holding can be traced
//! back to the purchase it descends from [DOM-096]. An imported one inherits from nothing stored
//! and names none. A `transfer_out` is not an opening and has no row (DEC-093).
//!
//! # Quantities as of today
//!
//! Effective quantity and remaining quantity are both taken at the latest position in canonical
//! order, through every split of the opening's account and security, so the two share one scale
//! and compare to a broker statement [DOM-118]. Both are the 8-decimal view every effective
//! quantity is compared and shown in (DEC-091, provisional), and the remainder is what
//! [`unattributed_quantity`] says remains, every allocation rescaled to today first [DOM-064].
//! The unit price is the opening's EUR total over the exact effective quantity [DOM-089].
//!
//! # Realized gain/loss
//!
//! The sum of the gains of the opening's allocations to disposals, each derived by
//! [`crate::allocation`] from the parents exactly as the income tax overview derives it [DOM-058],
//! never from pooled totals; each gain is arithmetic on its own rounded shares, so the column is a
//! sum of rounded rows [DOM-125]. An allocation to a `transfer_out` realizes nothing [DOM-093]: its
//! basis travels with the emitted `transfer_in`, whose own row realizes it when it is sold.
//!
//! # Filters
//!
//! Optional account, as the income tax overview's [DOM-073]; optional year, selecting the openings
//! with at least one allocation to a closing of that year, a `transfer_out` included [DOM-079]. A
//! filter selects rows and never changes a figure: a selected row's gain is every realized gain
//! of that opening, and its quantities are today's (DEC-107, provisional).

use std::collections::{HashMap, hash_map::Entry};

use chrono::{Datelike, NaiveDate};
use sqlx::SqliteConnection;
use thiserror::Error;

use crate::decimal::{EffectivePrice, Money, Quantity, Scaled};
use crate::effective_quantity::{effective_quantity, unattributed_quantity};
use crate::entities::{Account, Isin, Order};
use crate::income_tax;
use crate::ordering::{BatchAge, Leg, OrderKey, RecordPosition};
use crate::storage::{
    AttributionRepository, Database, StorageError, StoredOpening, TransactionId,
    TransactionRepository,
};
use crate::transaction::{Opening, Split, Transaction};

/// Which rows the report holds [DOM-073]. The default is unfiltered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// Only this account's openings; `None` for every account.
    pub account: Option<Account>,
    /// Only openings with at least one allocation in this year [DOM-079]; `None` for every year.
    pub year: Option<i32>,
}

/// One opening of the report [DOM-077].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    opening: TransactionId,
    date: NaiveDate,
    account: Account,
    security: Isin,
    effective_quantity: Quantity,
    remaining_quantity: Quantity,
    effective_unit_price: EffectivePrice,
    fees: Money,
    gain: Money,
    inherited_from: Option<TransactionId>,
}

impl Row {
    #[must_use]
    pub fn opening(&self) -> TransactionId {
        self.opening
    }

    /// The opening's trade date. An emitted `transfer_in` takes its parcel's (DEC-105), which is
    /// also the acquisition date it inherits.
    #[must_use]
    pub fn date(&self) -> NaiveDate {
        self.date
    }

    #[must_use]
    pub fn account(&self) -> &Account {
        &self.account
    }

    #[must_use]
    pub fn security(&self) -> &Isin {
        &self.security
    }

    /// The quantity as of today, at the quantity scale [DOM-118].
    #[must_use]
    pub fn effective_quantity(&self) -> Quantity {
        self.effective_quantity
    }

    /// What no closing has consumed, as of today and in the same scale as
    /// [`Self::effective_quantity`] [DOM-118].
    #[must_use]
    pub fn remaining_quantity(&self) -> Quantity {
        self.remaining_quantity
    }

    /// The EUR total cost over the effective quantity as of today, unrounded until display
    /// [DOM-118], [ARC-009].
    #[must_use]
    pub fn effective_unit_price(&self) -> EffectivePrice {
        self.effective_unit_price
    }

    /// The opening's EUR fees as booked [DOM-059].
    #[must_use]
    pub fn fees(&self) -> Money {
        self.fees
    }

    /// The sum of the opening's allocation gains [DOM-125]: raw gain/loss, no tax law [DOM-001].
    #[must_use]
    pub fn gain(&self) -> Money {
        self.gain
    }

    /// The opening an emitted `transfer_in` inherited from [DOM-096]; `None` for a buy and for an
    /// imported `transfer_in`.
    #[must_use]
    pub fn inherited_from(&self) -> Option<TransactionId> {
        self.inherited_from
    }
}

/// Why the report cannot be produced. It is refused whole rather than produced with a row
/// missing or a figure guessed.
#[derive(Debug, Error)]
pub enum ReportError {
    /// A disposal's allocation figures cannot be derived.
    #[error(transparent)]
    Figures(#[from] income_tax::ReportError),
    /// The opening's quantity or unit price as of today lies beyond what a decimal holds, or an
    /// allocation against it sits before it in canonical order.
    #[error("the quantity of opening {opening} cannot be measured as of today")]
    Unmeasurable { opening: TransactionId },
    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// The report's rows under `filter`, in canonical order.
///
/// Read in one SQLite transaction, so every figure is taken from one snapshot.
///
/// # Errors
///
/// [`ReportError::Figures`] naming the disposal whose figures cannot be derived,
/// [`ReportError::Unmeasurable`] naming the opening, or [`ReportError::Storage`].
pub async fn report(database: &Database, filter: &Filter) -> Result<Vec<Row>, ReportError> {
    let mut tx = database.begin().await?;
    let gains = realized(&mut tx, filter.account.as_ref()).await?;
    let openings = TransactionRepository::openings_in(&mut tx, filter.account.as_ref()).await?;

    let mut splits: HashMap<(Account, Isin), Vec<Split>> = HashMap::new();
    let mut rows = Vec::with_capacity(openings.len());
    for stored in openings {
        let against = AttributionRepository::against_opening_in(&mut tx, stored.id).await?;
        let allocated: Vec<(Quantity, OrderKey)> = against
            .into_iter()
            .map(|(_, closed_at, quantity)| (quantity, closed_at))
            .collect();
        if !selected(filter.year, &allocated) {
            continue;
        }
        let opening = match TransactionRepository::find_in(&mut tx, stored.id).await? {
            Some(Transaction::Opening(opening)) => opening,
            _ => {
                return Err(StorageError::CorruptValue {
                    field: "kind",
                    value: stored.id.to_string(),
                }
                .into());
            }
        };
        let pair_splits = match splits.entry((stored.account.clone(), stored.security.clone())) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(
                TransactionRepository::splits_in(&mut tx, &stored.account, &stored.security)
                    .await?,
            ),
        };
        let gain = gains.get(&stored.id).copied().unwrap_or_else(Money::zero);
        rows.push(row(stored, &opening, pair_splits, &allocated, gain)?);
    }
    Ok(rows)
}

/// The realized gain of every opening of `account`, or of every account, that an attributed
/// disposal consumed: the sum of its allocations' gains [DOM-125]. Every disposal is read, not
/// only those of a filtered year, since the filter never changes a figure (DEC-107).
///
/// Only disposals: a `transfer_out` realizes nothing [DOM-093], and an unattributed disposal
/// has no allocations yet.
async fn realized(
    connection: &mut SqliteConnection,
    account: Option<&Account>,
) -> Result<HashMap<TransactionId, Money>, ReportError> {
    let mut gains: HashMap<TransactionId, Money> = HashMap::new();
    for disposal in AttributionRepository::disposals_in(&mut *connection, account).await? {
        let Some(attribution) = disposal.attribution else {
            continue;
        };
        for (opening, figures) in
            income_tax::figures(&mut *connection, &disposal, attribution).await?
        {
            let total = gains.entry(opening).or_insert_with(Money::zero);
            *total = Money::new(total.get() + figures.gain().get());
        }
    }
    Ok(gains)
}

/// Whether an opening with allocations to closings at `allocated` belongs in a report of `year`:
/// at least one of them is in it [DOM-079]. Every opening belongs when no year is given.
fn selected(year: Option<i32>, allocated: &[(Quantity, OrderKey)]) -> bool {
    year.is_none_or(|year| {
        allocated
            .iter()
            .any(|(_, closed_at)| closed_at.trade_date().year() == year)
    })
}

/// A position after every stored one, so every split of the pair applies: the latest position in
/// canonical order, which is "today" [DOM-118]. Nothing is stored here, so no split is excluded
/// by the strict bound `effective_quantity` applies (DEC-097).
fn today() -> OrderKey {
    OrderKey::new(
        NaiveDate::MAX,
        RecordPosition::new(Order::new(u32::MAX), BatchAge::new(i64::MAX)),
        Leg::Trailing,
    )
}

/// The row of `opening`, stored as `stored`, through `splits` of its pair, with `allocated`
/// against it and `gain` realized [DOM-077], [DOM-118].
fn row(
    stored: StoredOpening,
    opening: &Opening,
    splits: &[Split],
    allocated: &[(Quantity, OrderKey)],
    gain: Money,
) -> Result<Row, ReportError> {
    let unmeasurable = || ReportError::Unmeasurable { opening: stored.id };
    let now = today();
    let effective = effective_quantity(opening, splits, now).ok_or_else(unmeasurable)?;
    let effective_quantity = effective.at_quantity_scale().ok_or_else(unmeasurable)?;
    let remaining_quantity = unattributed_quantity(opening, splits, now, allocated.iter().copied())
        .ok_or_else(unmeasurable)?;
    let effective_unit_price = effective
        .unit_price(opening.gross().eur())
        .ok_or_else(unmeasurable)?;
    Ok(Row {
        opening: stored.id,
        date: opening.trade_date(),
        account: stored.account,
        security: stored.security,
        effective_quantity,
        remaining_quantity,
        effective_unit_price,
        fees: opening.fees().eur(),
        // At the money scale, so an opening that has realized nothing reads `0.00`.
        gain: gain.rounded(),
        inherited_from: stored.inherited_from,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::num::NonZeroU32;

    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use vec1::vec1;

    use crate::decimal::QuotedPrice;
    use crate::identity::{IdentitySource, identify};
    use crate::manual_entry::Ratio;
    use crate::storage::RecordHandle;
    use crate::transaction::{Buy, BuyOrigin, Derivation};
    use crate::valuation::{Conversion, Valued};

    fn account() -> Account {
        Account::new("Saxo", "69900/1000000")
    }

    fn isin() -> Isin {
        Isin::new("NL0000009538")
    }

    fn date(year: i32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, 3, day).expect("a valid date")
    }

    fn key(year: i32, day: u32) -> OrderKey {
        OrderKey::new(
            date(year, day),
            RecordPosition::new(Order::new(0), BatchAge::new(1)),
            Leg::Lead,
        )
    }

    fn on(year: i32, day: u32) -> Derivation {
        let record = RecordHandle::for_test(
            identify(
                &account(),
                &IdentitySource::BrokerReference(&format!("row-{year}-{day}")),
            ),
            RecordPosition::new(Order::new(0), BatchAge::new(1)),
        );
        Derivation::new(date(year, day), vec1![record])
    }

    /// `quantity` bought on 1 March 2024 for `gross` EUR plus `fees`.
    fn buy(quantity: Decimal, gross: Decimal, fees: Decimal) -> Opening {
        Opening::Buy(Buy::new(
            on(2024, 1),
            Quantity::new(quantity),
            Valued::in_eur(QuotedPrice::new(gross / quantity)),
            Valued::in_eur(Money::new(gross)),
            Valued::in_eur(Money::new(fees)),
            BuyOrigin::Purchase,
            Conversion::native(date(2024, 1)),
        ))
    }

    fn split(year: i32, day: u32, numerator: u32, denominator: u32) -> Split {
        Split::new(
            on(year, day),
            Ratio::new(
                NonZeroU32::new(numerator).expect("non-zero"),
                NonZeroU32::new(denominator).expect("non-zero"),
            ),
        )
    }

    fn stored(inherited_from: Option<TransactionId>) -> StoredOpening {
        StoredOpening {
            id: TransactionId::new(7),
            account: account(),
            security: isin(),
            inherited_from,
        }
    }

    /// Buy 10 for 1000, sell 4, split 2:1: as of today the parcel is 20 at 50 with 12 remaining,
    /// the 4 sold rescaled to 8 in today's units, so the two quantities share one scale [DOM-118],
    /// [DOM-077], [DOM-064].
    #[test]
    fn both_quantities_and_the_price_are_as_of_today() {
        let opening = buy(dec!(10), dec!(1000.00), dec!(8.00));
        let splits = [split(2024, 20, 2, 1)];
        let allocated = [(Quantity::new(dec!(4)), key(2024, 10))];

        let row = row(
            stored(None),
            &opening,
            &splits,
            &allocated,
            Money::new(dec!(12.34)),
        )
        .expect("measurable");

        assert_eq!(row.effective_quantity(), Quantity::new(dec!(20)));
        assert_eq!(row.remaining_quantity(), Quantity::new(dec!(12)));
        assert_eq!(row.effective_unit_price().get(), dec!(50));
        assert_eq!(row.fees(), Money::new(dec!(8.00)));
        assert_eq!(row.gain(), Money::new(dec!(12.34)));
        assert_eq!(row.date(), date(2024, 1));
        assert_eq!(row.account(), &account());
        assert_eq!(row.security(), &isin());
    }

    /// A split on the far side of every disposal still applies: "today" is after everything
    /// stored [DOM-118].
    #[test]
    fn today_is_after_every_split() {
        let opening = buy(dec!(9), dec!(900.00), dec!(0.00));
        let splits = [split(2099, 31, 1, 3)];

        let row = row(stored(None), &opening, &splits, &[], Money::zero()).expect("measurable");

        assert_eq!(row.effective_quantity(), Quantity::new(dec!(3)));
        assert_eq!(row.remaining_quantity(), Quantity::new(dec!(3)));
        assert_eq!(row.effective_unit_price().get(), dec!(300));
    }

    /// An opening that has realized nothing reads `0.00`, and names the opening it inherited
    /// from when it has one [DOM-096].
    #[test]
    fn nothing_realized_reads_zero_and_the_parent_is_named() {
        let opening = buy(dec!(1), dec!(10.00), dec!(0.00));
        let parent = TransactionId::new(3);

        let row = row(stored(Some(parent)), &opening, &[], &[], Money::zero()).expect("measurable");

        assert_eq!(row.gain().get().to_string(), "0.00");
        assert_eq!(row.inherited_from(), Some(parent));
    }

    /// An allocation placed before its opening has no remainder to report, and the report says
    /// which opening rather than showing a figure (DEC-097).
    #[test]
    fn an_allocation_before_the_opening_is_unmeasurable() {
        let opening = buy(dec!(10), dec!(1000.00), dec!(0.00));
        let allocated = [(Quantity::new(dec!(1)), key(2023, 1))];

        match row(stored(None), &opening, &[], &allocated, Money::zero()) {
            Err(ReportError::Unmeasurable { opening }) => {
                assert_eq!(opening, TransactionId::new(7))
            }
            other => panic!("expected the opening named as unmeasurable, got {other:?}"),
        }
    }

    /// The year filter keeps an opening with at least one allocation in that year, whatever its
    /// others; no year keeps every opening, allocated or not [DOM-079].
    #[test]
    fn the_year_filter_keys_on_allocation_years() {
        let allocated = [
            (Quantity::new(dec!(1)), key(2023, 5)),
            (Quantity::new(dec!(1)), key(2025, 5)),
        ];

        assert!(selected(Some(2023), &allocated));
        assert!(selected(Some(2025), &allocated));
        assert!(!selected(Some(2024), &allocated));
        assert!(!selected(Some(2024), &[]));
        assert!(selected(None, &[]));
    }
}
