//! Assigning each row of a file its `order` [DOM-040].
//!
//! Order is established when a file is read, from the file's own content, so re-reading the same
//! file reproduces it exactly and the result never depends on what was imported before.
//!
//! Rows are sorted on, in turn:
//!
//! 1. the trade date;
//! 2. every further ordering column the format provides, in the precedence the format states;
//! 3. the row's position in the file, normalized to the file's own direction, so a newest-first
//!    export does not order backwards.
//!
//! The third key is always available and no two rows share a position, so the order within a file
//! is total and an import is never refused for ambiguity [DOM-102].
//!
//! Every row takes part, whatever it turns out to be. Nothing here inspects what a row means, so
//! a split is ordered beside the trades it sits among rather than after them [DOM-088].
//!
//! # What a format supplies
//!
//! An ordering column is an `i64`, and a format converts whatever it has into one: a monotonic
//! booking counter as itself, a timestamp as an instant count.
//!
//! Every row of a file supplies the same number of columns, an absent value as `None`, and every
//! value of one column shares a unit. Both matter. A ragged list would be compared
//! lexicographically, so a row with fewer columns would sort first for a reason nobody stated. A
//! mixed unit misorders: the Trade Republic 2024 export writes `…38.780Z` beside `…55.441487Z`,
//! so a reader that counts fractional digits rather than converting through a single
//! epoch-instant call would place three-digit timestamps wrongly.
//!
//! A column that is not monotonic with time is never an ordering column. Saxo's
//! `Corporate action-Id` is the example: it identifies an event, and sorting by it would
//! interleave years.
//!
//! A column may be absent from a row — Saxo populates its booking counters unevenly — and absent
//! sorts **before** present. The choice matters less than its being fixed: falling through to the
//! next key when either side is absent would not be a total order, because `5` and `3` would each
//! tie with a missing value while differing from each other.
//!
//! # What this does not do
//!
//! Comparing rows across files. `order` is scoped to the file it came from, and the canonical
//! order over an account is a wider key; that is FIF-076's, and is undecided.

use chrono::NaiveDate;

use crate::entities::Order;

/// Which end of the file holds the oldest row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileDirection {
    /// The first row is the oldest. Trade Republic exports read this way.
    OldestFirst,
    /// The first row is the newest. Saxo exports read this way, so their positions are reversed
    /// before use.
    NewestFirst,
}

/// What one row offers for ordering.
///
/// A row's position in the file is not a field: `assign_orders` takes the rows **in file order**
/// and uses each row's index in that slice. Carrying a position separately would let it disagree
/// with the slice — an offset or a line number counted past a header — and the disagreement would
/// reverse a newest-first file wrongly rather than loudly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowOrderingKey {
    /// The trade date, which dominates everything else.
    pub trade_date: NaiveDate,
    /// The format's ordering columns, most significant first. A row may lack any of them.
    pub columns: Vec<Option<i64>>,
}

impl RowOrderingKey {
    /// The key actually sorted on, with `position` normalized to the file's direction.
    fn sortable(
        &self,
        position: usize,
        direction: FileDirection,
        row_count: usize,
    ) -> (NaiveDate, &[Option<i64>], usize) {
        let normalized = match direction {
            FileDirection::OldestFirst => position,
            FileDirection::NewestFirst => row_count - 1 - position,
        };
        (self.trade_date, &self.columns, normalized)
    }
}

/// Assign every row its `order`, returned parallel to the input.
///
/// `rows` must be in file order: index 0 is the first row the file holds, whichever end of the
/// file is the oldest.
///
/// The result is a permutation of `0..rows.len()`: every row gets a distinct order, because the
/// normalized position separates any rows the earlier keys leave equal [DOM-102].
///
/// # Panics
///
/// If the file holds more than `u32::MAX` rows. Returning duplicate orders instead would break
/// the permutation quietly, and no spreadsheet or CSV this reads comes close to the limit.
#[must_use]
pub fn assign_orders(rows: &[RowOrderingKey], direction: FileDirection) -> Vec<Order> {
    let count = rows.len();
    let mut indices: Vec<usize> = (0..count).collect();
    indices.sort_by(|&left, &right| {
        rows[left]
            .sortable(left, direction, count)
            .cmp(&rows[right].sortable(right, direction, count))
    });

    let mut orders = vec![Order::new(0); count];
    for (order, &index) in indices.iter().enumerate() {
        orders[index] =
            Order::new(u32::try_from(order).expect("a file cannot hold more than u32::MAX rows"));
    }
    orders
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2024, 1, day).expect("a valid date")
    }

    fn row(day: u32, columns: Vec<Option<i64>>) -> RowOrderingKey {
        RowOrderingKey {
            trade_date: date(day),
            columns,
        }
    }

    fn orders(rows: &[RowOrderingKey], direction: FileDirection) -> Vec<u32> {
        assign_orders(rows, direction)
            .into_iter()
            .map(Order::get)
            .collect()
    }

    /// Trade date dominates every other key [DOM-040].
    #[test]
    fn trade_date_comes_first() {
        let rows = [
            row(3, vec![Some(1)]),
            row(1, vec![Some(999)]),
            row(2, vec![None]),
        ];

        assert_eq!(orders(&rows, FileDirection::OldestFirst), vec![2, 0, 1]);
    }

    /// An ordering column separates rows of one date [DOM-040].
    #[test]
    fn an_ordering_column_separates_one_date() {
        let rows = [
            row(1, vec![Some(300)]),
            row(1, vec![Some(100)]),
            row(1, vec![Some(200)]),
        ];

        assert_eq!(orders(&rows, FileDirection::OldestFirst), vec![2, 0, 1]);
    }

    /// Columns are compared in the precedence the format gives them [DOM-040].
    #[test]
    fn columns_are_compared_in_order_of_precedence() {
        let rows = [
            row(1, vec![Some(1), Some(9)]),
            row(1, vec![Some(1), Some(2)]),
            row(1, vec![Some(0), Some(9)]),
        ];

        assert_eq!(orders(&rows, FileDirection::OldestFirst), vec![2, 1, 0]);
    }

    /// An absent column sorts before a present one, which keeps the order total.
    #[test]
    fn an_absent_column_sorts_first() {
        let rows = [row(1, vec![Some(1)]), row(1, vec![None])];

        assert_eq!(orders(&rows, FileDirection::OldestFirst), vec![1, 0]);
    }

    /// File position settles rows the earlier keys leave equal, so nothing is ever ambiguous
    /// [DOM-102].
    #[test]
    fn file_position_settles_what_the_columns_cannot() {
        // The two Saxo corporate-action rows that share a date and carry no booking counter.
        let rows = [row(9, vec![None]), row(9, vec![None]), row(9, vec![None])];

        assert_eq!(orders(&rows, FileDirection::OldestFirst), vec![0, 1, 2]);
    }

    /// A newest-first export does not order backwards [DOM-040].
    #[test]
    fn a_newest_first_file_is_not_ordered_backwards() {
        // As Saxo emits them: newest row first, and no booking counter to separate them.
        let rows = [row(3, vec![None]), row(2, vec![None]), row(1, vec![None])];

        assert_eq!(orders(&rows, FileDirection::NewestFirst), vec![2, 1, 0]);

        // Within one date the same reversal applies: the last row of a newest-first file is the
        // oldest booking of that day.
        let same_day = [row(1, vec![None]), row(1, vec![None]), row(1, vec![None])];
        assert_eq!(orders(&same_day, FileDirection::NewestFirst), vec![2, 1, 0]);
    }

    /// Every row gets a distinct order: the result is a permutation [DOM-102].
    #[test]
    fn every_row_gets_a_distinct_order() {
        // Many rows sharing few dates and carrying no column, so every separation comes from
        // the position alone.
        let rows: Vec<_> = (0..25u32)
            .map(|index| row(1 + index % 3, vec![None]))
            .collect();

        let mut assigned = orders(&rows, FileDirection::NewestFirst);
        assigned.sort_unstable();

        assert_eq!(assigned, (0..25).collect::<Vec<_>>());
    }

    /// Reading the same file twice reproduces the order exactly [DOM-040].
    ///
    /// Pinned to a hand-derived vector rather than compared against itself, which any
    /// deterministic function would satisfy including one that ignored every key. Newest-first,
    /// so normalized positions run 3, 2, 1, 0: the day-1 rows come first, the absent column
    /// before the present one, then the day-2 rows by their counters.
    #[test]
    fn re_reading_a_file_reproduces_the_order() {
        let rows = [
            row(2, vec![Some(7)]),
            row(1, vec![None]),
            row(2, vec![Some(3)]),
            row(1, vec![Some(5)]),
        ];

        assert_eq!(orders(&rows, FileDirection::NewestFirst), vec![3, 0, 2, 1]);
    }

    /// Degenerate inputs are ordinary.
    #[test]
    fn an_empty_or_single_row_file_is_ordinary() {
        assert!(orders(&[], FileDirection::OldestFirst).is_empty());
        assert_eq!(
            orders(&[row(1, vec![None])], FileDirection::NewestFirst),
            vec![0]
        );
    }
}
