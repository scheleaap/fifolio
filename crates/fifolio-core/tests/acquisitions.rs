//! Integration layer: the acquisition report over a real temporary SQLite database, in process
//! [TST-003], [TST-004].
//!
//! One scenario, built in code, whose figures are derivable by hand [TST-015]:
//!
//! * Account A, security S. 2023: buys B1 (10 for 1000.00 + 10.00) and B2 (10 for 2000.00 +
//!   20.00). 2024: a sell of 15 for 3000.00 + 10.00 attributed as 10 of B1 and 5 of B2, whose
//!   fee divides with a remainder cent, then a 2-for-1 split. 2025: a one-for-one `transfer_out` of 6 into security T, attributed to B2,
//!   which emits one `transfer_in`, E.
//! * Account A, security T. 2025: sells of 2 of E for 300.00 + 1.00 and of 1 of E for 160.00 +
//!   1.00, both attributed, then a sell of 1 for 170.00 + 1.00 left pending.
//! * Account B, security S. 2024: an imported `transfer_in`, I, of 5 for 500.00 + 2.00.
//!
//! No test reaches the network.

use std::collections::BTreeMap;
use std::num::NonZeroU32;

use chrono::NaiveDate;
use fifolio_core::acquisitions::{Filter, Line, Row, report};
use fifolio_core::attribution::approve;
use fifolio_core::decimal::{Money, Quantity, QuotedPrice, Scaled};
use fifolio_core::entities::{
    Account, ImportBatch, ImportCounts, Isin, Order, Quotation, Security, SecurityType,
    SourceFormat, SourceRecord,
};
use fifolio_core::identity::{IdentitySource, identify};
use fifolio_core::income_tax;
use fifolio_core::manual_entry::Ratio;
use fifolio_core::storage::{Allocation, BatchId, Database, Placement, TransactionId};
use fifolio_core::transaction::{
    Buy, BuyOrigin, DateProvenance, Derivation, Sell, Split, Transaction, TransferIn,
    TransferInSource, TransferOut,
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

fn target() -> Isin {
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

fn ratio(numerator: u32) -> Ratio {
    Ratio::new(
        NonZeroU32::new(numerator).expect("non-zero"),
        NonZeroU32::MIN,
    )
}

struct Scenario {
    _db: TempDb,
    database: Database,
    b1: TransactionId,
    b2: TransactionId,
    emitted: TransactionId,
    imported: TransactionId,
}

impl Scenario {
    async fn open() -> (TempDb, Database, BatchId) {
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
        for isin in [security(), target()] {
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
        (db, database, batch)
    }

    async fn derivation(
        database: &Database,
        batch: BatchId,
        on: NaiveDate,
        reference: &str,
    ) -> Derivation {
        let record = database
            .source_records()
            .insert(
                batch,
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

    async fn store(
        database: &Database,
        batch: BatchId,
        account: Account,
        isin: Isin,
        transaction: Transaction,
    ) -> TransactionId {
        database
            .transactions()
            .insert(&Placement::derived(account, isin, batch), &transaction)
            .await
            .expect("store a transaction")
    }

    async fn approve(
        database: &Database,
        closing: TransactionId,
        allocations: &[(TransactionId, Decimal)],
    ) {
        let allocations: Vec<Allocation> = allocations
            .iter()
            .map(|(opening, quantity)| Allocation::new(*opening, Quantity::new(*quantity)))
            .collect();
        approve(database, closing, &allocations)
            .await
            .expect("the attribution is approved");
    }

    /// The whole scenario of the module comment.
    async fn build() -> Self {
        let (db, database, batch) = Self::open().await;
        let d = &database;
        let derive = |on, reference| Self::derivation(d, batch, on, reference);
        let buy = |derivation, quantity, gross, fees| -> Transaction {
            Buy::new(
                derivation,
                Quantity::new(quantity),
                price(),
                money(gross),
                money(fees),
                BuyOrigin::Purchase,
                Conversion::native(date(2023, 1, 1)),
            )
            .into()
        };
        let sell = |derivation, quantity, gross, fees| -> Transaction {
            Sell::new(
                derivation,
                Quantity::new(quantity),
                price(),
                money(gross),
                money(fees),
                Conversion::native(date(2024, 1, 1)),
            )
            .into()
        };

        let b1 = Self::store(
            d,
            batch,
            account_a(),
            security(),
            buy(
                derive(date(2023, 3, 1), "B1").await,
                dec!(10),
                dec!(1000.00),
                dec!(10.00),
            ),
        )
        .await;
        let b2 = Self::store(
            d,
            batch,
            account_a(),
            security(),
            buy(
                derive(date(2023, 4, 1), "B2").await,
                dec!(10),
                dec!(2000.00),
                dec!(20.00),
            ),
        )
        .await;
        let sold_2024 = Self::store(
            d,
            batch,
            account_a(),
            security(),
            sell(
                derive(date(2024, 2, 1), "S1").await,
                dec!(15),
                dec!(3000.00),
                dec!(10.00),
            ),
        )
        .await;
        Self::approve(d, sold_2024, &[(b1, dec!(10)), (b2, dec!(5))]).await;
        Self::store(
            d,
            batch,
            account_a(),
            security(),
            Split::new(derive(date(2024, 6, 1), "P1").await, ratio(2)).into(),
        )
        .await;

        let moved = Self::store(
            d,
            batch,
            account_a(),
            security(),
            TransferOut::new(
                derive(date(2025, 1, 10), "X1").await,
                Quantity::new(dec!(6)),
                money(dec!(0.00)),
                ratio(1),
                Conversion::native(date(2025, 1, 10)),
                target(),
            )
            .into(),
        )
        .await;
        Self::approve(d, moved, &[(b2, dec!(6))]).await;
        let emitted = TransactionId::new(moved.get() + 1);
        let sold_2025 = Self::store(
            d,
            batch,
            account_a(),
            target(),
            sell(
                derive(date(2025, 3, 1), "S2").await,
                dec!(2),
                dec!(300.00),
                dec!(1.00),
            ),
        )
        .await;
        Self::approve(d, sold_2025, &[(emitted, dec!(2))]).await;
        let sold_later = Self::store(
            d,
            batch,
            account_a(),
            target(),
            sell(
                derive(date(2025, 6, 1), "S3").await,
                dec!(1),
                dec!(160.00),
                dec!(1.00),
            ),
        )
        .await;
        Self::approve(d, sold_later, &[(emitted, dec!(1))]).await;
        // Left pending: it has no allocations, so no line and nothing consumed [DOM-078].
        Self::store(
            d,
            batch,
            account_a(),
            target(),
            sell(
                derive(date(2025, 9, 1), "S4").await,
                dec!(1),
                dec!(170.00),
                dec!(1.00),
            ),
        )
        .await;

        let imported = Self::store(
            d,
            batch,
            account_b(),
            security(),
            TransferIn::new(
                derive(date(2024, 5, 1), "I1").await,
                Quantity::new(dec!(5)),
                money(dec!(500.00)),
                money(dec!(2.00)),
                date(2024, 5, 1),
                DateProvenance::TransferDate,
                TransferInSource::Broker,
                Conversion::native(date(2024, 5, 1)),
            )
            .into(),
        )
        .await;

        Self {
            _db: db,
            database,
            b1,
            b2,
            emitted,
            imported,
        }
    }

    async fn report(&self, filter: Filter) -> Vec<Row> {
        report(&self.database, &filter).await.expect("the report")
    }
}

/// A row as its columns read [DOM-077], with the opening it names [DOM-096].
type Columns = (NaiveDate, Account, Isin, [String; 5], Option<TransactionId>);

fn columns(row: &Row) -> Columns {
    (
        row.date(),
        row.account().clone(),
        row.security().clone(),
        [
            row.effective_quantity().get().to_string(),
            row.remaining_quantity().get().to_string(),
            row.effective_unit_price().get().to_string(),
            row.fees().get().to_string(),
            row.gain().get().to_string(),
        ],
        row.inherited_from(),
    )
}

fn expected(
    on: NaiveDate,
    account: Account,
    isin: Isin,
    figures: [&str; 5],
    inherited_from: Option<TransactionId>,
) -> Columns {
    (
        on,
        account,
        isin,
        figures.map(str::to_owned),
        inherited_from,
    )
}

impl Scenario {
    /// B1 as of today: 10 through the 2-for-1 is 20 at 50, all consumed by the 2024 sell. Its
    /// gain: 10/15 of 3000.00 and 10.00 is 2000.00 and 6.67 (6.666.. rounded); exhausted, it takes
    /// all of 1000.00 and 10.00; 2000 - 6.67 - 1000 - 10 = 983.33.
    fn b1_row(&self) -> Columns {
        expected(
            date(2023, 3, 1),
            account_a(),
            security(),
            ["20", "0", "50", "10.00", "983.33"],
            None,
        )
    }

    /// B2 as of today: 20 at 100, less the 5 sold before the split (10 in today's units) and the
    /// 6 transferred after it, leaves 4 [DOM-118]. Its gain is the sale's alone: the last closing
    /// share takes the remainder, 1000.00 and 10.00 - 6.67 = 3.33; 5 of 10 units as of the sale is
    /// 1000.00 and 10.00; 1000 - 3.33 - 1000 - 10 = -13.33. The transfer realizes nothing
    /// [DOM-093].
    fn b2_row(&self) -> Columns {
        expected(
            date(2023, 4, 1),
            account_a(),
            security(),
            ["20", "4", "100", "20.00", "-13.33"],
            None,
        )
    }

    /// E: 6 of B2 at 6/20 of 2000.00 and 20.00, 600.00 and 6.00, at B2's place and date (DEC-105).
    /// Its sale of 2: 300 - 1 - 200 - 2 = 97.00; its sale of 1: 160 - 1 - 100 - 1 = 58.00; 97.00 +
    /// 58.00 = 155.00. The pending sale consumes nothing, so 3 remain. It names B2 [DOM-096].
    fn emitted_row(&self) -> Columns {
        expected(
            date(2023, 4, 1),
            account_a(),
            target(),
            ["6", "3", "100", "6.00", "155.00"],
            Some(self.b2),
        )
    }

    /// I: imported from another broker, it inherits from no opening stored and names none
    /// [DOM-096]; nothing consumed it.
    fn imported_row(&self) -> Columns {
        expected(
            date(2024, 5, 1),
            account_b(),
            security(),
            ["5", "5", "100", "2.00", "0.00"],
            None,
        )
    }
}

/// Every opening is listed, a `transfer_in` among them, in canonical order, with its columns as
/// of today [DOM-076], [DOM-077], [DOM-118]; the emitted `transfer_in` names the opening it
/// inherited from and the imported one names none [DOM-096]; a `transfer_out` has no row
/// (DEC-093). Unfiltered is every account and year [DOM-073].
#[tokio::test]
async fn every_opening_is_listed_with_its_columns_as_of_today() {
    let s = Scenario::build().await;

    let rows = s.report(Filter::default()).await;

    assert_eq!(
        rows.iter().map(columns).collect::<Vec<_>>(),
        vec![s.b1_row(), s.b2_row(), s.emitted_row(), s.imported_row()]
    );
    assert_eq!(
        rows.iter().map(Row::opening).collect::<Vec<_>>()[..2],
        [s.b1, s.b2]
    );
    assert_eq!(rows[3].opening(), s.imported);
}

/// The year filter selects openings with at least one allocation in that year, a
/// `transfer_out`'s included, and changes no figure of a row it keeps [DOM-079] (DEC-107).
#[tokio::test]
async fn the_year_filter_selects_openings_allocated_in_that_year() {
    let s = Scenario::build().await;
    let year = |year| Filter {
        account: None,
        year: Some(year),
    };

    let in_2024: Vec<Columns> = s.report(year(2024)).await.iter().map(columns).collect();
    assert_eq!(in_2024, vec![s.b1_row(), s.b2_row()]);

    let in_2025: Vec<Columns> = s.report(year(2025)).await.iter().map(columns).collect();
    assert_eq!(in_2025, vec![s.b2_row(), s.emitted_row()]);

    assert!(
        s.report(year(2023)).await.is_empty(),
        "bought, never allocated in 2023"
    );
}

/// The account filter selects that account's openings, as the income tax overview's does, and
/// composes with the year [DOM-073].
#[tokio::test]
async fn the_account_filter_selects_that_accounts_openings() {
    let s = Scenario::build().await;

    let b: Vec<Columns> = s
        .report(Filter {
            account: Some(account_b()),
            year: None,
        })
        .await
        .iter()
        .map(columns)
        .collect();
    assert_eq!(b, vec![s.imported_row()]);

    let a_2025: Vec<Columns> = s
        .report(Filter {
            account: Some(account_a()),
            year: Some(2025),
        })
        .await
        .iter()
        .map(columns)
        .collect();
    assert_eq!(a_2025, vec![s.b2_row(), s.emitted_row()]);
}

/// Both reports sum the same per-allocation gains, so the acquisition report's gains total the
/// income tax overview's to the cent [DOM-125].
#[tokio::test]
async fn the_gains_reconcile_to_the_income_tax_overview() {
    let s = Scenario::build().await;

    let acquired: Decimal = s
        .report(Filter::default())
        .await
        .iter()
        .map(|row| row.gain().get())
        .sum();
    let disposed: Decimal = income_tax::overview(&s.database, &income_tax::Filter::default())
        .await
        .expect("the overview")
        .iter()
        .map(|row| row.gain().get())
        .sum();

    assert_eq!(acquired, dec!(1125.00));
    assert_eq!(acquired, disposed);
}

/// Nothing held, nothing reported.
#[tokio::test]
async fn no_opening_is_no_row() {
    let (_db, database, _batch) = Scenario::open().await;

    assert!(
        report(&database, &Filter::default())
            .await
            .expect("the report")
            .is_empty()
    );
}

/// Once the `transfer_out`'s attribution is deleted, the parcel may be deleted while its emitted
/// record stays; the record then names no opening rather than a row id SQLite may reuse
/// (DEC-108) [DOM-096].
#[tokio::test]
async fn an_emitted_record_whose_parent_is_deleted_names_none() {
    let (_db, database, batch) = Scenario::open().await;
    let parcel = Scenario::store(
        &database,
        batch,
        account_a(),
        security(),
        Buy::new(
            Scenario::derivation(&database, batch, date(2023, 3, 1), "B1").await,
            Quantity::new(dec!(10)),
            price(),
            money(dec!(1000.00)),
            money(dec!(0.00)),
            BuyOrigin::Purchase,
            Conversion::native(date(2023, 3, 1)),
        )
        .into(),
    )
    .await;
    let moved = Scenario::store(
        &database,
        batch,
        account_a(),
        security(),
        TransferOut::new(
            Scenario::derivation(&database, batch, date(2024, 1, 10), "X1").await,
            Quantity::new(dec!(10)),
            money(dec!(0.00)),
            ratio(1),
            Conversion::native(date(2024, 1, 10)),
            target(),
        )
        .into(),
    )
    .await;
    let attribution = approve(
        &database,
        moved,
        &[Allocation::new(parcel, Quantity::new(dec!(10)))],
    )
    .await
    .expect("the transfer is approved");
    let named = report(&database, &Filter::default())
        .await
        .expect("the report");
    assert_eq!(named[1].inherited_from(), Some(parcel));

    database
        .attributions()
        .delete(attribution)
        .await
        .expect("delete the transfer's attribution");
    database
        .transactions()
        .delete(parcel)
        .await
        .expect("the parcel is free once nothing is attributed to it");

    let rows = report(&database, &Filter::default())
        .await
        .expect("the report");
    assert_eq!(rows.len(), 1, "only the emitted record is left");
    assert_eq!(rows[0].security(), &target());
    assert_eq!(rows[0].inherited_from(), None);
}

/// A disposal line as its columns read [DOM-078]: its date, the quantity consumed, then proceeds,
/// sell fee, cost, buy fee and gain/loss.
type LineColumns = (NaiveDate, String, [String; 5]);

fn line_columns(line: &Line) -> LineColumns {
    (
        line.date(),
        line.quantity().get().to_string(),
        [
            line.proceeds(),
            line.sell_fee(),
            line.cost(),
            line.buy_fee(),
            line.gain(),
        ]
        .map(|money| money.get().to_string()),
    )
}

fn lines_of(rows: &[Row]) -> Vec<(TransactionId, Vec<LineColumns>)> {
    rows.iter()
        .map(|row| {
            (
                row.opening(),
                row.disposals().iter().map(line_columns).collect(),
            )
        })
        .collect()
}

impl Scenario {
    /// Every opening's expected lines, the figures derived by hand as in the row comments above.
    ///
    /// * B1: 10 of the 2024 sale, 10/15 of 3000.00 and 10.00, 2000.00 and 6.67; exhausted, all of
    ///   1000.00 and 10.00.
    /// * B2: 5 of the 2024 sale, in the units current then [DOM-103] (DEC-109): the closing's last
    ///   share, 1000.00 and the remainder cent's 3.33; 5 of 10 of 2000.00 and 20.00. The 2025
    ///   `transfer_out` of 6 has no line (DEC-093).
    /// * E: two lines in canonical order. 2 of the March sale, 300.00 and 1.00; 2 of the 6 it
    ///   carries at the inherited 600.00 and 6.00. Then 1 of the June sale, 160.00 and 1.00; 1 of
    ///   the 6, 100.00 and 1.00. The pending September sale has no line.
    /// * I: consumed by nothing, no line.
    fn expected_lines(&self) -> Vec<(TransactionId, Vec<LineColumns>)> {
        let line = |on, quantity: &str, figures: [&str; 5]| {
            (on, quantity.to_owned(), figures.map(str::to_owned))
        };
        vec![
            (
                self.b1,
                vec![line(
                    date(2024, 2, 1),
                    "10",
                    ["2000.00", "6.67", "1000.00", "10.00", "983.33"],
                )],
            ),
            (
                self.b2,
                vec![line(
                    date(2024, 2, 1),
                    "5",
                    ["1000.00", "3.33", "1000.00", "10.00", "-13.33"],
                )],
            ),
            (
                self.emitted,
                vec![
                    line(
                        date(2025, 3, 1),
                        "2",
                        ["300.00", "1.00", "200.00", "2.00", "97.00"],
                    ),
                    line(
                        date(2025, 6, 1),
                        "1",
                        ["160.00", "1.00", "100.00", "1.00", "58.00"],
                    ),
                ],
            ),
            (self.imported, vec![]),
        ]
    }
}

/// Beneath each opening, every attributed disposal with its date, quantity consumed and the
/// allocation's five figures, in canonical order; a pending disposal has none [DOM-078]. A `transfer_out` has no line; the parcel it carried is
/// listed under the sale of its emitted `transfer_in`, at the inherited cost and buy fee
/// (DEC-093).
#[tokio::test]
async fn each_opening_lists_its_attributed_disposals() {
    let s = Scenario::build().await;

    let rows = s.report(Filter::default()).await;

    assert_eq!(lines_of(&rows), s.expected_lines());
}

/// The year filter selects rows and changes no line: B2, selected in 2025 by its transfer, still
/// lists its 2024 sale and nothing for the transfer [DOM-079] (DEC-107, DEC-093).
#[tokio::test]
async fn the_year_filter_changes_no_line() {
    let s = Scenario::build().await;
    let expected = s.expected_lines();

    let in_2025 = s
        .report(Filter {
            account: None,
            year: Some(2025),
        })
        .await;

    assert_eq!(
        lines_of(&in_2025),
        vec![expected[1].clone(), expected[2].clone()]
    );
}

/// The lines drop no allocation the income tax overview counts: every column of the lines totals
/// the overview's to the cent [DOM-078], [DOM-125]. Both read the same per-allocation figures, so
/// this checks coverage, not the figures themselves; `expected_lines` checks those by hand.
#[tokio::test]
async fn the_lines_total_the_income_tax_overview() {
    let s = Scenario::build().await;
    let rows = s.report(Filter::default()).await;
    let overview = income_tax::overview(&s.database, &income_tax::Filter::default())
        .await
        .expect("the overview");

    let lines: Vec<&Line> = rows.iter().flat_map(Row::disposals).collect();
    let total = |figure: fn(&Line) -> Money| -> Decimal {
        lines.iter().map(|line| figure(line).get()).sum()
    };
    let reported = |figure: fn(&income_tax::Row) -> Money| -> Decimal {
        overview.iter().map(|row| figure(row).get()).sum()
    };
    assert_eq!(
        [
            total(Line::proceeds),
            total(Line::sell_fee),
            total(Line::cost),
            total(Line::buy_fee),
            total(Line::gain),
        ],
        [
            reported(income_tax::Row::proceeds),
            reported(income_tax::Row::sell_fees),
            reported(income_tax::Row::cost),
            reported(income_tax::Row::buy_fees),
            reported(income_tax::Row::gain),
        ]
    );
}

/// An opening's lines follow the disposals' canonical order, not the order they were stored in:
/// by trade date, then on one date by the source record's position [DOM-078], [DOM-011]. Each
/// sale is told apart by its date and proceeds.
#[tokio::test]
async fn the_lines_follow_canonical_order_not_insertion_order() {
    let (_db, database, batch) = Scenario::open().await;
    let d = &database;
    // Scenario::derivation places every record at position 0; a same-date pair needs two.
    let derive = |on, reference, position| async move {
        let record = d
            .source_records()
            .insert(
                batch,
                &SourceRecord::new(
                    identify(&account_a(), &IdentitySource::BrokerReference(reference)),
                    Order::new(position),
                    "\"row\"",
                    BTreeMap::new(),
                ),
            )
            .await
            .expect("the record a transaction is derived from");
        Derivation::new(on, vec1![record])
    };
    let parcel = Scenario::store(
        d,
        batch,
        account_a(),
        security(),
        Buy::new(
            derive(date(2024, 1, 1), "B1", 0).await,
            Quantity::new(dec!(10)),
            price(),
            money(dec!(1000.00)),
            money(dec!(0.00)),
            BuyOrigin::Purchase,
            Conversion::native(date(2024, 1, 1)),
        )
        .into(),
    )
    .await;
    // Stored latest date first, and the same-date pair with the later position first, so neither
    // insertion order nor id order is canonical order.
    for (on, reference, position, proceeds) in [
        (date(2024, 6, 1), "S1", 0, dec!(150.00)),
        (date(2024, 3, 1), "S2", 0, dec!(120.00)),
        (date(2024, 4, 1), "S3", 5, dec!(140.00)),
        (date(2024, 4, 1), "S4", 2, dec!(130.00)),
    ] {
        let sale = Scenario::store(
            d,
            batch,
            account_a(),
            security(),
            Sell::new(
                derive(on, reference, position).await,
                Quantity::new(dec!(1)),
                price(),
                money(proceeds),
                money(dec!(0.00)),
                Conversion::native(on),
            )
            .into(),
        )
        .await;
        Scenario::approve(d, sale, &[(parcel, dec!(1))]).await;
    }

    let rows = report(d, &Filter::default()).await.expect("the report");

    assert_eq!(
        rows[0]
            .disposals()
            .iter()
            .map(|line| (line.date(), line.proceeds().get().to_string()))
            .collect::<Vec<_>>(),
        [
            (date(2024, 3, 1), "120.00"),
            (date(2024, 4, 1), "130.00"),
            (date(2024, 4, 1), "140.00"),
            (date(2024, 6, 1), "150.00"),
        ]
        .map(|(on, proceeds)| (on, proceeds.to_owned()))
    );
}
