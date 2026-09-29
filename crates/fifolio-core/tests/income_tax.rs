//! Integration layer: the income tax overview over a real temporary SQLite database, in process
//! [TST-003], [TST-004].
//!
//! One scenario, built in code, whose figures are derivable by hand [TST-015]:
//!
//! * Account A, security S. 2023: a buy of 10 and a `transfer_out` of those 10, attributed.
//!   2024: buys B1 (100 for 1000.00 + 10.00) and B2 (100 for 1200.00 + 5.00), a 2-for-1 split,
//!   and a sell of 300 for 1800.00 + 9.00 attributed as 200 of B1 and 100 of B2. 2025: an
//!   unattributed sell and an `expiration`.
//! * Account B, security S. 2024: a buy of 10 for 100.00 + 1.00 and its sell for 150.00 + 1.00,
//!   attributed. 2022, security T: an unattributed `transfer_out`.
//!
//! No test reaches the network.

use std::collections::BTreeMap;
use std::num::NonZeroU32;

use chrono::NaiveDate;
use fifolio_core::attribution::approve;
use fifolio_core::decimal::{Money, Quantity, QuotedPrice, Scaled};
use fifolio_core::entities::{
    Account, ImportBatch, ImportCounts, Isin, Order, Quotation, Security, SecurityType,
    SourceFormat, SourceRecord,
};
use fifolio_core::identity::{IdentitySource, identify};
use fifolio_core::income_tax::{Filter, Row, overview};
use fifolio_core::manual_entry::Ratio;
use fifolio_core::storage::{Allocation, BatchId, Database, Placement, TransactionId};
use fifolio_core::transaction::{
    Buy, BuyOrigin, Derivation, Expiration, Sell, Split, Transaction, TransferOut,
};
use fifolio_core::valuation::{Conversion, Valued};
use fifolio_test_support::TempDb;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use vec1::vec1;

fn account_a() -> Account {
    Account::new("Saxo", "69900/1000000")
}

fn account_b() -> Account {
    Account::new("Saxo", "69900/2000000")
}

fn security() -> Isin {
    Isin::new("NL0000009538")
}

fn other_security() -> Isin {
    Isin::new("NL0011821202")
}

fn date(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).expect("a valid date")
}

fn money(amount: Decimal) -> Valued<Money> {
    Valued::new(Money::new(amount), Money::new(amount))
}

fn price() -> Valued<QuotedPrice> {
    Valued::new(QuotedPrice::new(dec!(10)), QuotedPrice::new(dec!(10)))
}

fn one_for_one() -> Ratio {
    Ratio::new(NonZeroU32::MIN, NonZeroU32::MIN)
}

struct Scenario {
    _db: TempDb,
    database: Database,
    batch: BatchId,
}

impl Scenario {
    async fn open() -> Self {
        let db = TempDb::new();
        let database = Database::open(db.path())
            .await
            .expect("open the temporary database");
        for account in [account_a(), account_b()] {
            database
                .accounts()
                .insert(&account)
                .await
                .expect("the account");
        }
        for isin in [security(), other_security()] {
            database
                .securities()
                .insert(&Security::auto_created(
                    isin,
                    "NN Group",
                    SecurityType::Stock,
                    Quotation::PerUnit,
                ))
                .await
                .expect("the security");
        }
        let batch = database
            .import_batches()
            .insert(&ImportBatch::new(
                account_a(),
                "export.xlsx",
                SourceFormat::SaxoNlXlsx,
                "2025-12-31T09:00:00Z".parse().expect("a valid timestamp"),
                ImportCounts {
                    derived: 1,
                    pending: 0,
                    non_position: 0,
                },
            ))
            .await
            .expect("the import");
        Self {
            _db: db,
            database,
            batch,
        }
    }

    async fn derivation(&self, on: NaiveDate, reference: &str) -> Derivation {
        let record = self
            .database
            .source_records()
            .insert(
                self.batch,
                &SourceRecord::new(
                    identify(&account_a(), &IdentitySource::BrokerReference(reference)),
                    Order::new(0),
                    "\"row\"",
                    BTreeMap::new(),
                ),
            )
            .await
            .expect("the record a transaction is derived from");
        Derivation::new(on, vec1![record])
    }

    async fn store(&self, account: Account, isin: Isin, transaction: Transaction) -> TransactionId {
        self.database
            .transactions()
            .insert(&Placement::derived(account, isin, self.batch), &transaction)
            .await
            .expect("store a transaction")
    }

    async fn buy(
        &self,
        account: Account,
        on: NaiveDate,
        reference: &str,
        quantity: Decimal,
        (gross, fees): (Decimal, Decimal),
    ) -> TransactionId {
        let derivation = self.derivation(on, reference).await;
        self.store(
            account,
            security(),
            Buy::new(
                derivation,
                Quantity::new(quantity),
                price(),
                money(gross),
                money(fees),
                BuyOrigin::Purchase,
                Conversion::native(on),
            )
            .into(),
        )
        .await
    }

    async fn sell(
        &self,
        account: Account,
        on: NaiveDate,
        reference: &str,
        quantity: Decimal,
        (gross, fees): (Decimal, Decimal),
    ) -> TransactionId {
        let derivation = self.derivation(on, reference).await;
        self.store(
            account,
            security(),
            Sell::new(
                derivation,
                Quantity::new(quantity),
                price(),
                money(gross),
                money(fees),
                Conversion::native(on),
            )
            .into(),
        )
        .await
    }

    async fn transfer_out(
        &self,
        account: Account,
        isin: Isin,
        on: NaiveDate,
        reference: &str,
        quantity: Decimal,
    ) -> TransactionId {
        let derivation = self.derivation(on, reference).await;
        self.store(
            account,
            isin,
            TransferOut::new(
                derivation,
                Quantity::new(quantity),
                money(dec!(0.00)),
                one_for_one(),
                Conversion::native(on),
                other_security(),
            )
            .into(),
        )
        .await
    }

    async fn approve(&self, closing: TransactionId, allocations: &[(TransactionId, Decimal)]) {
        let allocations: Vec<Allocation> = allocations
            .iter()
            .map(|(opening, quantity)| Allocation::new(*opening, Quantity::new(*quantity)))
            .collect();
        approve(&self.database, closing, &allocations)
            .await
            .expect("the attribution is approved");
    }

    /// The whole scenario of the module comment.
    async fn build() -> Self {
        let s = Self::open().await;

        let parcel = s
            .buy(
                account_a(),
                date(2023, 3, 1),
                "A0",
                dec!(10),
                (dec!(100.00), dec!(1.00)),
            )
            .await;
        let moved = s
            .transfer_out(account_a(), security(), date(2023, 6, 1), "X0", dec!(10))
            .await;
        s.approve(moved, &[(parcel, dec!(10))]).await;

        let first = s
            .buy(
                account_a(),
                date(2024, 2, 1),
                "B1",
                dec!(100),
                (dec!(1000.00), dec!(10.00)),
            )
            .await;
        let second = s
            .buy(
                account_a(),
                date(2024, 3, 1),
                "B2",
                dec!(100),
                (dec!(1200.00), dec!(5.00)),
            )
            .await;
        let split = s.derivation(date(2024, 4, 1), "P1").await;
        s.store(
            account_a(),
            security(),
            Split::new(
                split,
                Ratio::new(NonZeroU32::new(2).expect("non-zero"), NonZeroU32::MIN),
            )
            .into(),
        )
        .await;
        let sold = s
            .sell(
                account_a(),
                date(2024, 5, 1),
                "S1",
                dec!(300),
                (dec!(1800.00), dec!(9.00)),
            )
            .await;
        s.approve(sold, &[(first, dec!(200)), (second, dec!(100))])
            .await;

        s.sell(
            account_a(),
            date(2025, 1, 10),
            "S2",
            dec!(50),
            (dec!(400.00), dec!(2.00)),
        )
        .await;
        let expired = s.derivation(date(2025, 6, 30), "E1").await;
        s.store(
            account_a(),
            security(),
            Expiration::new(
                expired,
                money(dec!(0.00)),
                money(dec!(0.00)),
                Conversion::native(date(2025, 6, 30)),
            )
            .into(),
        )
        .await;

        let bought = s
            .buy(
                account_b(),
                date(2024, 7, 1),
                "C1",
                dec!(10),
                (dec!(100.00), dec!(1.00)),
            )
            .await;
        let sold = s
            .sell(
                account_b(),
                date(2024, 8, 1),
                "C2",
                dec!(10),
                (dec!(150.00), dec!(1.00)),
            )
            .await;
        s.approve(sold, &[(bought, dec!(10))]).await;
        s.transfer_out(
            account_b(),
            other_security(),
            date(2022, 9, 1),
            "Y1",
            dec!(5),
        )
        .await;

        s
    }

    async fn report(&self, filter: Filter) -> Vec<Row> {
        overview(&self.database, &filter)
            .await
            .expect("the overview")
    }
}

/// A row as its columns read: year, account, proceeds, sell fees, cost, buy fees, gain and the
/// outstanding count [DOM-075].
fn columns(row: &Row) -> (i32, Account, [String; 5], usize) {
    (
        row.year(),
        row.account().clone(),
        [
            row.proceeds(),
            row.sell_fees(),
            row.cost(),
            row.buy_fees(),
            row.gain(),
        ]
        .map(|figure| figure.get().to_string()),
        row.outstanding(),
    )
}

fn expected(
    year: i32,
    account: Account,
    figures: [&str; 5],
    outstanding: usize,
) -> (i32, Account, [String; 5], usize) {
    (year, account, figures.map(str::to_owned), outstanding)
}

/// Account A's 2024 row. Opening side, as of the sell, after the split: B1 is exhausted and takes
/// all of 1000.00 and 10.00; B2 gives 100 of its 200 units, 600.00 and 2.50. Closing side:
/// 200/300 of 1800.00 and 9.00 is 1200.00 and 6.00, and B2 as the last takes 600.00 and 3.00.
/// Gains 1200 - 6 - 1000 - 10 = 184.00 and 600 - 3 - 600 - 2.50 = -5.50.
fn a_2024() -> (i32, Account, [String; 5], usize) {
    expected(
        2024,
        account_a(),
        ["1800.00", "9.00", "1600.00", "12.50", "178.50"],
        0,
    )
}

/// Account A's 2025: an unattributed sell and an expiration, both outstanding, neither counted at
/// zero into figures [DOM-117], DEC-102.
fn a_2025() -> (i32, Account, [String; 5], usize) {
    expected(
        2025,
        account_a(),
        ["0.00", "0.00", "0.00", "0.00", "0.00"],
        2,
    )
}

/// Account B's 2024: 150 - 1 - 100 - 1 = 48.00.
fn b_2024() -> (i32, Account, [String; 5], usize) {
    expected(
        2024,
        account_b(),
        ["150.00", "1.00", "100.00", "1.00", "48.00"],
        0,
    )
}

/// Unfiltered is every account and every year [DOM-073]; a row per year of disposal and account
/// [DOM-074], [DOM-075]; the attributed disposals' allocation figures summed [DOM-125]; the
/// unattributed ones counted as outstanding [DOM-117], [DOM-121]; and a `transfer_out`,
/// attributed or not, adds no row and no count: account A has no 2023 row and account B no 2022
/// row [DOM-095]. Raw figures, no tax law [DOM-001].
#[tokio::test]
async fn the_unfiltered_overview_is_every_year_and_account() {
    let s = Scenario::build().await;

    let rows: Vec<_> = s
        .report(Filter::default())
        .await
        .iter()
        .map(columns)
        .collect();

    assert_eq!(rows, vec![a_2024(), b_2024(), a_2025()]);
}

/// The account filter keeps that account's rows and the year filter that year's; together they
/// keep one row [DOM-073].
#[tokio::test]
async fn the_filters_select_rows() {
    let s = Scenario::build().await;
    let rows = |filter| async {
        s.report(filter)
            .await
            .iter()
            .map(columns)
            .collect::<Vec<_>>()
    };

    assert_eq!(
        rows(Filter {
            account: Some(account_a()),
            year: None,
        })
        .await,
        vec![a_2024(), a_2025()]
    );
    assert_eq!(
        rows(Filter {
            account: None,
            year: Some(2024),
        })
        .await,
        vec![a_2024(), b_2024()]
    );
    assert_eq!(
        rows(Filter {
            account: Some(account_b()),
            year: Some(2024),
        })
        .await,
        vec![b_2024()]
    );
}

/// A year holding only a `transfer_out` has no row even when asked for by name [DOM-095].
#[tokio::test]
async fn a_year_of_only_transfers_out_is_empty() {
    let s = Scenario::build().await;

    for (account, year) in [(account_a(), 2023), (account_b(), 2022)] {
        assert!(
            s.report(Filter {
                account: Some(account),
                year: Some(year),
            })
            .await
            .is_empty()
        );
    }
}

/// A later disposal that exhausts a parcel takes its drift; filtering to the earlier year does
/// not move that drift onto the earlier disposal [DOM-062], [DOM-073]. A parcel of 3 for 10.00
/// sold one unit a year: 3.33, 3.33 and the exhausting 3.34.
#[tokio::test]
async fn a_year_filter_changes_no_figure() {
    let s = Scenario::open().await;
    let parcel = s
        .buy(
            account_a(),
            date(2023, 1, 2),
            "B1",
            dec!(3),
            (dec!(10.00), dec!(0.00)),
        )
        .await;
    for (year, reference) in [(2023, "S1"), (2024, "S2"), (2025, "S3")] {
        let sold = s
            .sell(
                account_a(),
                date(year, 6, 1),
                reference,
                dec!(1),
                (dec!(5.00), dec!(0.00)),
            )
            .await;
        s.approve(sold, &[(parcel, dec!(1))]).await;
    }

    let unfiltered: Vec<_> = s
        .report(Filter::default())
        .await
        .iter()
        .map(columns)
        .collect();
    assert_eq!(
        unfiltered,
        vec![
            expected(
                2023,
                account_a(),
                ["5.00", "0.00", "3.33", "0.00", "1.67"],
                0
            ),
            expected(
                2024,
                account_a(),
                ["5.00", "0.00", "3.33", "0.00", "1.67"],
                0
            ),
            expected(
                2025,
                account_a(),
                ["5.00", "0.00", "3.34", "0.00", "1.66"],
                0
            ),
        ]
    );
    for row in &unfiltered {
        let filtered: Vec<_> = s
            .report(Filter {
                account: None,
                year: Some(row.0),
            })
            .await
            .iter()
            .map(columns)
            .collect();
        assert_eq!(filtered, vec![row.clone()]);
    }
}

/// An account filter naming an account with no disposal gives no row, not an error [DOM-073].
#[tokio::test]
async fn an_account_without_disposals_has_no_rows() {
    let s = Scenario::open().await;

    assert!(
        s.report(Filter {
            account: Some(account_b()),
            year: None,
        })
        .await
        .is_empty()
    );
}
