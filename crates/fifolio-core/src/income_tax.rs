//! The income tax overview: raw gain/loss per year of disposal and per account [DOM-074],
//! [DOM-075], with no tax law applied to it [DOM-001].
//!
//! # What a row sums
//!
//! The four shares and the gain of every allocation of every attributed disposal in the row, as
//! [`crate::allocation`] derives them from the parents [DOM-058]: never a figure recomputed from
//! pooled totals. A row total is therefore the sum of rounded allocation figures, and its gain
//! the sum of their gains, each of which is arithmetic on its own rounded shares [DOM-125]; so a
//! row reconciles to its columns, and to the allocations beneath it, to the cent.
//!
//! The figures are the EUR half: the tax figures are the EUR ones [DOM-084].
//!
//! # What is outstanding
//!
//! A disposal with no approved attribution adds nothing to the figures and one to the row's
//! outstanding count [DOM-117]: it is never counted at zero, and the count is a column of the
//! same row, so every format keeps one record shape [DOM-121]. Saying in plain words that a year
//! is incomplete belongs to the formats that have room for a sentence (FIF-046); this module
//! exposes the count. An `expiration` cannot yet be approved (DEC-102), so every one is
//! outstanding in its year.
//!
//! A `transfer_out` realizes nothing: it adds neither figures nor a count, and a year and
//! account with nothing else has no row [DOM-095].
//!
//! # Filters
//!
//! Optional account and optional tax year; unfiltered means all accounts and all years [DOM-073].
//! A filter selects rows and never changes a figure: which allocation absorbs an opening's drift
//! depends on every allocation against it [DOM-062], including those of disposals filtered out,
//! and those are read regardless.

use std::collections::BTreeMap;

use chrono::Datelike;
use sqlx::SqliteConnection;
use thiserror::Error;

use crate::allocation::{
    AgainstOpening, AllocationError, Figures, Half, OfClosing, closing_shares, opening_shares,
};
use crate::decimal::{Money, Scaled};
use crate::entities::Account;
use crate::storage::{
    AttributionId, AttributionRepository, Database, Disposal, StorageError, TransactionId,
    TransactionRepository,
};
use crate::transaction::Transaction;

/// Which rows the report holds [DOM-073]. The default is unfiltered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// Only this account's rows; `None` for every account.
    pub account: Option<Account>,
    /// Only this year's rows, the year of the disposal [DOM-074]; `None` for every year.
    pub year: Option<i32>,
}

/// One year and account of the overview [DOM-075].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    year: i32,
    account: Account,
    proceeds: Money,
    sell_fees: Money,
    cost: Money,
    buy_fees: Money,
    gain: Money,
    outstanding: usize,
}

impl Row {
    fn empty(year: i32, account: Account) -> Self {
        Self {
            year,
            account,
            proceeds: Money::zero(),
            sell_fees: Money::zero(),
            cost: Money::zero(),
            buy_fees: Money::zero(),
            gain: Money::zero(),
            outstanding: 0,
        }
    }

    fn add(mut self, figures: &Figures) -> Self {
        let plus = |total: Money, share: Money| Money::new(total.get() + share.get());
        self.proceeds = plus(self.proceeds, figures.proceeds());
        self.sell_fees = plus(self.sell_fees, figures.sell_fee());
        self.cost = plus(self.cost, figures.cost());
        self.buy_fees = plus(self.buy_fees, figures.buy_fee());
        self.gain = plus(self.gain, figures.gain());
        self
    }

    /// Every figure at the money scale, so a row of nothing but outstanding disposals reads
    /// `0.00` like any other.
    fn padded(self) -> Self {
        Self {
            proceeds: self.proceeds.rounded(),
            sell_fees: self.sell_fees.rounded(),
            cost: self.cost.rounded(),
            buy_fees: self.buy_fees.rounded(),
            gain: self.gain.rounded(),
            ..self
        }
    }

    /// The calendar year the row's disposals took place in [DOM-074].
    #[must_use]
    pub fn year(&self) -> i32 {
        self.year
    }

    #[must_use]
    pub fn account(&self) -> &Account {
        &self.account
    }

    #[must_use]
    pub fn proceeds(&self) -> Money {
        self.proceeds
    }

    #[must_use]
    pub fn sell_fees(&self) -> Money {
        self.sell_fees
    }

    #[must_use]
    pub fn cost(&self) -> Money {
        self.cost
    }

    #[must_use]
    pub fn buy_fees(&self) -> Money {
        self.buy_fees
    }

    /// The sum of the row's allocation gains [DOM-125]: raw gain/loss, no tax law [DOM-001].
    #[must_use]
    pub fn gain(&self) -> Money {
        self.gain
    }

    /// Disposals of this year and account with no approved attribution [DOM-117]. Non-zero means
    /// the row's figures are incomplete [DOM-121].
    #[must_use]
    pub fn outstanding(&self) -> usize {
        self.outstanding
    }
}

/// Why the overview cannot be produced. It is refused whole rather than produced with a disposal
/// missing, which would be the silent understatement DOM-117 rules out.
#[derive(Debug, Error)]
pub enum ReportError {
    /// An attributed disposal's allocation figures cannot be derived.
    #[error("the figures of disposal {closing} cannot be derived: {source}")]
    Underivable {
        closing: TransactionId,
        #[source]
        source: AllocationError,
    },
    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// What one disposal adds to its row: its allocations' figures when attributed, `None` when
/// outstanding.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Contribution {
    year: i32,
    account: Account,
    figures: Option<Vec<Figures>>,
}

/// The overview's rows under `filter`, ordered by year and then account.
///
/// Read in one SQLite transaction, so every figure is taken from one snapshot.
///
/// # Errors
///
/// [`ReportError::Underivable`] naming the disposal whose figures cannot be derived, or
/// [`ReportError::Storage`].
pub async fn overview(database: &Database, filter: &Filter) -> Result<Vec<Row>, ReportError> {
    let mut tx = database.begin().await?;
    let disposals = AttributionRepository::disposals_in(&mut tx, filter.account.as_ref()).await?;

    let mut contributions = Vec::with_capacity(disposals.len());
    for disposal in disposals.into_iter().filter(|disposal| {
        filter
            .year
            .is_none_or(|year| disposal.trade_date.year() == year)
    }) {
        let figures = match disposal.attribution {
            Some(attribution) => Some(figures(&mut tx, &disposal, attribution).await?),
            None => None,
        };
        contributions.push(Contribution {
            year: disposal.trade_date.year(),
            account: disposal.account,
            figures,
        });
    }
    Ok(tabulate(contributions))
}

/// One row per year and account that has a disposal, attributed or not.
fn tabulate(contributions: impl IntoIterator<Item = Contribution>) -> Vec<Row> {
    contributions
        .into_iter()
        .fold(BTreeMap::new(), |mut rows, contribution| {
            let key = (contribution.year, contribution.account);
            let row = rows
                .remove(&key)
                .unwrap_or_else(|| Row::empty(key.0, key.1.clone()));
            let row = match &contribution.figures {
                Some(figures) => figures.iter().fold(row, Row::add),
                None => Row {
                    outstanding: row.outstanding + 1,
                    ..row
                },
            };
            rows.insert(key, row);
            rows
        })
        .into_values()
        .map(Row::padded)
        .collect()
}

/// The figures of each allocation of an attributed disposal, both sides derived as
/// [`crate::attribution`] derives the basis a `transfer_out` carries.
async fn figures(
    connection: &mut SqliteConnection,
    disposal: &Disposal,
    attribution: AttributionId,
) -> Result<Vec<Figures>, ReportError> {
    let underivable = |source| ReportError::Underivable {
        closing: disposal.closing,
        source,
    };
    let unmeasurable = AllocationError::Unmeasurable {
        closing: disposal.closing,
    };
    let closing = match TransactionRepository::find_in(&mut *connection, disposal.closing).await? {
        Some(Transaction::Closing(closing)) => closing,
        Some(_) => return Err(corrupt(disposal.closing).into()),
        None => {
            return Err(StorageError::UnknownTransaction {
                transaction: disposal.closing,
            }
            .into());
        }
    };
    let splits =
        TransactionRepository::splits_in(&mut *connection, &disposal.account, &disposal.security)
            .await?;

    let allocations = AttributionRepository::allocations_in(&mut *connection, attribution).await?;
    let mut opening_side = Vec::with_capacity(allocations.len());
    let mut of_closing = Vec::with_capacity(allocations.len());
    for allocation in &allocations {
        let opening =
            match TransactionRepository::find_in(&mut *connection, allocation.opening()).await? {
                Some(Transaction::Opening(opening)) => opening,
                _ => return Err(corrupt(allocation.opening()).into()),
            };
        let against: Vec<AgainstOpening> =
            AttributionRepository::against_opening_in(&mut *connection, allocation.opening())
                .await?
                .into_iter()
                .map(|(closing, closed_at, quantity)| {
                    AgainstOpening::new(closing, closed_at, quantity)
                })
                .collect();
        // Each opening appears once per attribution (DEC-103), so the one share keyed by this
        // closing is this allocation's.
        let shares = opening_shares(&opening, &splits, &against, Half::Eur)
            .map_err(underivable)?
            .into_iter()
            .find_map(|(of, shares)| (of == disposal.closing).then_some(shares))
            .ok_or_else(|| underivable(unmeasurable.clone()))?;
        opening_side.push((allocation.opening(), shares));
        of_closing.push(OfClosing::new(
            allocation.opening(),
            opening.derivation().order_key(),
            allocation.quantity(),
        ));
    }

    closing_shares(&closing, &of_closing, Half::Eur)
        .map_err(underivable)?
        .into_iter()
        .map(|(opening, closing_side)| {
            opening_side
                .iter()
                .find_map(|(of, shares)| {
                    (*of == opening).then_some(Figures::new(*shares, closing_side))
                })
                .ok_or_else(|| underivable(unmeasurable.clone()))
        })
        .collect()
}

/// A stored row whose kind is not the one its place requires: a disposal that is not a closing,
/// or an allocation against something that is not an opening.
fn corrupt(transaction: TransactionId) -> StorageError {
    StorageError::CorruptValue {
        field: "kind",
        value: transaction.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    use crate::allocation::{ClosingShares, OpeningShares};

    fn account() -> Account {
        Account::new("Saxo", "69900/1000000")
    }

    fn other_account() -> Account {
        Account::new("Trade Republic", "TR-1")
    }

    fn money(value: Decimal) -> Money {
        Money::new(value)
    }

    /// Figures of one allocation: `proceeds`, `sell_fee`, `cost`, `buy_fee`.
    fn allocation(
        proceeds: Decimal,
        sell_fee: Decimal,
        cost: Decimal,
        buy_fee: Decimal,
    ) -> Figures {
        Figures::new(
            OpeningShares::for_test(money(cost), money(buy_fee)),
            ClosingShares::for_test(money(proceeds), money(sell_fee)),
        )
    }

    fn attributed(year: i32, account: Account, figures: Vec<Figures>) -> Contribution {
        Contribution {
            year,
            account,
            figures: Some(figures),
        }
    }

    fn outstanding(year: i32, account: Account) -> Contribution {
        Contribution {
            year,
            account,
            figures: None,
        }
    }

    /// A row sums its allocations' rounded shares and gains, per year of disposal and per
    /// account [DOM-074], [DOM-075], [DOM-125].
    #[test]
    fn a_row_sums_the_allocations_of_its_year_and_account() {
        let rows = tabulate([
            attributed(
                2024,
                account(),
                vec![
                    allocation(dec!(600.00), dec!(4.00), dec!(500.00), dec!(3.33)),
                    allocation(dec!(600.00), dec!(4.00), dec!(510.00), dec!(3.34)),
                ],
            ),
            attributed(
                2024,
                account(),
                vec![allocation(
                    dec!(90.00),
                    dec!(1.00),
                    dec!(100.00),
                    dec!(0.50),
                )],
            ),
        ]);

        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.year(), 2024);
        assert_eq!(row.account(), &account());
        assert_eq!(row.proceeds(), money(dec!(1290.00)));
        assert_eq!(row.sell_fees(), money(dec!(9.00)));
        assert_eq!(row.cost(), money(dec!(1110.00)));
        assert_eq!(row.buy_fees(), money(dec!(7.17)));
        // 92.67 + 82.66 - 11.50: the sum of the allocation gains.
        assert_eq!(row.gain(), money(dec!(163.83)));
        assert_eq!(row.outstanding(), 0);
    }

    /// A row's gain reconciles to its own columns [DOM-125].
    #[test]
    fn a_row_gain_is_its_columns_arithmetic() {
        let rows = tabulate([attributed(
            2023,
            account(),
            vec![
                allocation(dec!(33.33), dec!(0.33), dec!(40.00), dec!(0.34)),
                allocation(dec!(33.34), dec!(0.34), dec!(20.00), dec!(0.33)),
            ],
        )]);

        let row = &rows[0];
        assert_eq!(
            row.gain().get(),
            row.proceeds().get() - row.sell_fees().get() - row.cost().get() - row.buy_fees().get()
        );
    }

    /// An unattributed disposal counts as outstanding in the same row and adds nothing to the
    /// figures, rather than counting at zero silently [DOM-117], [DOM-121].
    #[test]
    fn an_unattributed_disposal_is_counted_as_outstanding() {
        let rows = tabulate([
            attributed(
                2024,
                account(),
                vec![allocation(
                    dec!(120.00),
                    dec!(1.00),
                    dec!(100.00),
                    dec!(1.00),
                )],
            ),
            outstanding(2024, account()),
            outstanding(2024, account()),
        ]);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].outstanding(), 2);
        assert_eq!(rows[0].proceeds(), money(dec!(120.00)));
        assert_eq!(rows[0].gain(), money(dec!(18.00)));
    }

    /// A year with only unattributed disposals still has its row, figures at `0.00` and the count
    /// saying it is incomplete [DOM-117], [DOM-121].
    #[test]
    fn a_year_of_only_outstanding_disposals_has_a_row() {
        let rows = tabulate([outstanding(2022, account())]);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].outstanding(), 1);
        for figure in [
            rows[0].proceeds(),
            rows[0].sell_fees(),
            rows[0].cost(),
            rows[0].buy_fees(),
            rows[0].gain(),
        ] {
            assert_eq!(figure.get().to_string(), "0.00");
        }
    }

    /// Rows are one per year and account, ordered by year and then account [DOM-074].
    #[test]
    fn rows_are_per_year_and_account_in_order() {
        let rows = tabulate([
            outstanding(2024, other_account()),
            outstanding(2023, account()),
            outstanding(2024, account()),
            outstanding(2023, account()),
        ]);

        let keys: Vec<(i32, &Account, usize)> = rows
            .iter()
            .map(|row| (row.year(), row.account(), row.outstanding()))
            .collect();
        assert_eq!(
            keys,
            vec![
                (2023, &account(), 2),
                (2024, &account(), 1),
                (2024, &other_account(), 1),
            ]
        );
    }

    /// Nothing disposed of, nothing reported.
    #[test]
    fn no_disposal_is_no_row() {
        assert!(tabulate([]).is_empty());
    }
}
