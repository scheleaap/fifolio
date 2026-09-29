//! Integration layer: approving an attribution through the service, against a real temporary
//! SQLite database, in process [TST-003], [TST-004].
//!
//! Every refusal is followed by the approval that should succeed, because a refusal that stored
//! part of its attribution would leave the closing attributed and refuse the second approval.
//! Every test opens its own file through the shared helper. No test reaches the network.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use fifolio_core::attribution::{AttributionError, approve};
use fifolio_core::decimal::{Money, Quantity, QuotedPrice};
use fifolio_core::entities::{
    Account, ImportBatch, ImportCounts, Isin, Order, Quotation, Security, SecurityType,
    SourceFormat, SourceRecord,
};
use fifolio_core::fifo::{Proposal, propose};
use fifolio_core::identity::{IdentitySource, identify};
use fifolio_core::storage::{
    Allocation, BatchId, Database, Placement, RecordHandle, StorageError, TransactionId,
};
use fifolio_core::transaction::{
    Buy, BuyOrigin, Derivation, Expiration, Sell, Transaction, TransferOut,
};
use fifolio_core::valuation::{Conversion, Valued};
use fifolio_test_support::TempDb;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use vec1::vec1;

fn account() -> Account {
    Account::new("Saxo", "69900/1000000")
}

fn other_account() -> Account {
    Account::new("Saxo", "69900/2000000")
}

fn isin() -> Isin {
    Isin::new("NL0000009538")
}

fn other_isin() -> Isin {
    Isin::new("NL0011821202")
}

fn day(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2024, 5, day).expect("a valid date")
}

fn money(amount: Decimal) -> Valued<Money> {
    Valued::new(Money::new(amount), Money::new(amount))
}

fn buy(on: NaiveDate, record: RecordHandle, quantity: Decimal) -> Transaction {
    Buy::new(
        Derivation::new(on, vec1![record]),
        Quantity::new(quantity),
        Valued::new(QuotedPrice::new(dec!(10)), QuotedPrice::new(dec!(10))),
        money(quantity * dec!(10)),
        money(dec!(8.00)),
        BuyOrigin::Purchase,
        Conversion::native(on),
    )
    .into()
}

fn sell(on: NaiveDate, record: RecordHandle, quantity: Decimal) -> Transaction {
    Sell::new(
        Derivation::new(on, vec1![record]),
        Quantity::new(quantity),
        Valued::new(QuotedPrice::new(dec!(12)), QuotedPrice::new(dec!(12))),
        money(quantity * dec!(12)),
        money(dec!(8.00)),
        Conversion::native(on),
    )
    .into()
}

fn expiration(on: NaiveDate, record: RecordHandle) -> Transaction {
    Expiration::new(
        Derivation::new(on, vec1![record]),
        money(dec!(0.00)),
        money(dec!(0.00)),
        Conversion::native(on),
    )
    .into()
}

fn transfer_out(on: NaiveDate, record: RecordHandle, quantity: Decimal) -> Transaction {
    TransferOut::new(
        Derivation::new(on, vec1![record]),
        Quantity::new(quantity),
        money(dec!(0.00)),
        Conversion::native(on),
    )
    .into()
}

struct Fixture {
    _db: TempDb,
    database: Database,
    batch: BatchId,
}

impl Fixture {
    /// A database holding both accounts, both securities and one import.
    async fn open() -> Self {
        let db = TempDb::new();
        let database = Database::open(db.path())
            .await
            .expect("open the temporary database");
        for account in [account(), other_account()] {
            database
                .accounts()
                .insert(&account)
                .await
                .expect("the account");
        }
        for isin in [isin(), other_isin()] {
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
                account(),
                "2024.xlsx",
                SourceFormat::SaxoNlXlsx,
                "2024-05-31T09:00:00Z".parse().expect("a valid timestamp"),
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

    /// Stores the record `reference` names at `order` in its file and hands back its handle.
    async fn record(&self, reference: &str, order: u32) -> RecordHandle {
        self.database
            .source_records()
            .insert(
                self.batch,
                &SourceRecord::new(
                    identify(&account(), &IdentitySource::BrokerReference(reference)),
                    Order::new(order),
                    "\"2024-05-02\",\"BUY\"",
                    BTreeMap::new(),
                ),
            )
            .await
            .expect("the record a transaction is derived from")
    }

    async fn store_in(
        &self,
        account: Account,
        security: Isin,
        transaction: &Transaction,
    ) -> TransactionId {
        self.database
            .transactions()
            .insert(
                &Placement::derived(account, security, self.batch),
                transaction,
            )
            .await
            .expect("store a transaction")
    }

    async fn store(&self, transaction: &Transaction) -> TransactionId {
        self.store_in(account(), isin(), transaction).await
    }

    /// The FIFO proposal for `closing` over `openings`, read back from storage, with nothing yet
    /// allocated [DOM-056].
    async fn proposal(
        &self,
        closing: TransactionId,
        openings: &[TransactionId],
    ) -> Vec<Allocation> {
        let find = |id| async move {
            self.database
                .transactions()
                .find(id)
                .await
                .expect("read a transaction")
                .expect("a stored transaction")
        };
        let closing = find(closing).await;
        let mut stored = Vec::new();
        for id in openings {
            stored.push((*id, find(*id).await));
        }
        let openings = stored
            .iter()
            .map(|(id, transaction)| (*id, transaction.opening().expect("an opening")));
        match propose(closing.closing().expect("a closing"), openings, &[], &[])
            .expect("a proposal")
        {
            Proposal::Allocate(allocations) => allocations,
            shortfall => panic!("expected allocations, got {shortfall:?}"),
        }
    }

    async fn allocations(
        &self,
        attribution: fifolio_core::storage::AttributionId,
    ) -> Vec<Allocation> {
        self.database
            .attributions()
            .find(attribution)
            .await
            .expect("read the attribution")
            .expect("a stored attribution")
            .allocations()
            .to_vec()
    }
}

fn allocate(opening: TransactionId, quantity: Decimal) -> Allocation {
    Allocation::new(opening, Quantity::new(quantity))
}

/// The FIFO proposal is written as it was proposed: the same openings, quantities and order
/// [DOM-018], [DOM-054].
#[tokio::test]
async fn an_approved_proposal_is_stored_unchanged() {
    let f = Fixture::open().await;
    let first = f
        .store(&buy(day(1), f.record("B1", 0).await, dec!(100)))
        .await;
    let second = f
        .store(&buy(day(2), f.record("B2", 0).await, dec!(100)))
        .await;
    let closing = f
        .store(&sell(day(3), f.record("S1", 0).await, dec!(150)))
        .await;

    let proposal = f.proposal(closing, &[second, first]).await;
    assert_eq!(
        proposal,
        vec![allocate(first, dec!(100)), allocate(second, dec!(50))]
    );

    let attribution = approve(&f.database, closing, &proposal)
        .await
        .expect("the proposal is approved");

    assert_eq!(f.allocations(attribution).await, proposal);
}

/// Allocations given out of canonical order are stored in the order given, which is the order
/// DOM-063 rounds in, not re-sorted by opening or order key [DOM-054].
#[tokio::test]
async fn allocations_are_stored_in_the_order_given() {
    let f = Fixture::open().await;
    let first = f
        .store(&buy(day(1), f.record("B1", 0).await, dec!(100)))
        .await;
    let second = f
        .store(&buy(day(2), f.record("B2", 0).await, dec!(100)))
        .await;
    let closing = f
        .store(&sell(day(3), f.record("S1", 0).await, dec!(10)))
        .await;

    let allocations = [allocate(second, dec!(4)), allocate(first, dec!(6))];
    let attribution = approve(&f.database, closing, &allocations)
        .await
        .expect("allocations out of canonical order are approved");

    assert_eq!(f.allocations(attribution).await, allocations);
}

/// A `transfer_out` is covered by the quantity it states: one quantum off is refused, the exact
/// sum approved [DOM-018], [DOM-065] (DEC-091).
#[tokio::test]
async fn a_transfer_out_is_covered_by_its_quantity() {
    let f = Fixture::open().await;
    let opening = f
        .store(&buy(day(1), f.record("B1", 0).await, dec!(100)))
        .await;
    let closing = f
        .store(&transfer_out(day(2), f.record("T1", 0).await, dec!(40)))
        .await;

    match approve(
        &f.database,
        closing,
        &[allocate(opening, dec!(39.99999999))],
    )
    .await
    {
        Err(AttributionError::Uncovered(uncovered)) => {
            assert_eq!(uncovered.allocated, dec!(39.99999999));
            assert_eq!(uncovered.closed, dec!(40));
        }
        other => panic!("expected the DOM-065 refusal, got {other:?}"),
    }

    let attribution = approve(&f.database, closing, &[allocate(opening, dec!(40))])
        .await
        .expect("allocations summing to the transferred quantity are approved");
    assert_eq!(
        f.allocations(attribution).await,
        [allocate(opening, dec!(40))]
    );
}

/// A non-positive allocation and an opening allocated twice are refused though the sum holds,
/// and nothing is stored (DEC-103, provisional).
#[tokio::test]
async fn non_positive_and_repeated_allocations_are_refused() {
    let f = Fixture::open().await;
    let first = f
        .store(&buy(day(1), f.record("B1", 0).await, dec!(100)))
        .await;
    let second = f
        .store(&buy(day(2), f.record("B2", 0).await, dec!(100)))
        .await;
    let closing = f
        .store(&sell(day(3), f.record("S1", 0).await, dec!(10)))
        .await;

    assert!(matches!(
        approve(
            &f.database,
            closing,
            &[allocate(first, dec!(15)), allocate(second, dec!(-5))]
        )
        .await,
        Err(AttributionError::NotPositive { opening }) if opening == second
    ));
    assert!(matches!(
        approve(
            &f.database,
            closing,
            &[allocate(first, dec!(5)), allocate(first, dec!(5))]
        )
        .await,
        Err(AttributionError::RepeatedOpening { opening }) if opening == first
    ));

    approve(
        &f.database,
        closing,
        &[allocate(first, dec!(6)), allocate(second, dec!(4))],
    )
    .await
    .expect("nothing was stored by the refusals");
}

/// Declining is not an operation: nothing is stored, the closing stays unattributed and blocks
/// the later closing of its pair until it is approved [DOM-055], [DOM-066].
#[tokio::test]
async fn a_declined_closing_blocks_later_closings_until_approved() {
    let f = Fixture::open().await;
    let opening = f
        .store(&buy(day(1), f.record("B1", 0).await, dec!(100)))
        .await;
    let declined = f
        .store(&sell(day(2), f.record("S1", 0).await, dec!(10)))
        .await;
    let later = f
        .store(&sell(day(3), f.record("S2", 0).await, dec!(10)))
        .await;

    let _declined_proposal = f.proposal(declined, &[opening]).await;

    match approve(&f.database, later, &[allocate(opening, dec!(10))]).await {
        Err(AttributionError::Storage(StorageError::EarlierClosingUnattributed {
            closing,
            earlier,
        })) => {
            assert_eq!(closing, later);
            assert_eq!(earlier, declined);
        }
        other => panic!("expected the DOM-066 refusal, got {other:?}"),
    }

    approve(&f.database, declined, &[allocate(opening, dec!(10))])
        .await
        .expect("the declined closing is approved later");
    approve(&f.database, later, &[allocate(opening, dec!(10))])
        .await
        .expect("the block is lifted");
}

/// An opening of another account or another security is refused and nothing is stored
/// [DOM-019].
#[tokio::test]
async fn an_opening_of_another_account_or_security_is_refused() {
    let f = Fixture::open().await;
    let own = f
        .store(&buy(day(1), f.record("B1", 0).await, dec!(100)))
        .await;
    let other_account_opening = f
        .store_in(
            other_account(),
            isin(),
            &buy(day(1), f.record("B2", 1).await, dec!(100)),
        )
        .await;
    let other_security_opening = f
        .store_in(
            account(),
            other_isin(),
            &buy(day(1), f.record("B3", 2).await, dec!(100)),
        )
        .await;
    let closing = f
        .store(&sell(day(2), f.record("S1", 0).await, dec!(10)))
        .await;

    for elsewhere in [other_account_opening, other_security_opening] {
        let allocations = [allocate(own, dec!(5)), allocate(elsewhere, dec!(5))];
        match approve(&f.database, closing, &allocations).await {
            Err(AttributionError::OtherAccountOrSecurity {
                opening,
                closing: named,
            }) => {
                assert_eq!(opening, elsewhere);
                assert_eq!(named, closing);
            }
            other => panic!("expected the DOM-019 refusal, got {other:?}"),
        }
    }

    let attribution = approve(&f.database, closing, &[allocate(own, dec!(10))])
        .await
        .expect("nothing was stored by the refusals");
    assert_eq!(f.allocations(attribution).await, [allocate(own, dec!(10))]);
}

/// An opening that does not strictly precede the closing in the stored canonical order is
/// refused: a later date, a later record of the same date, and on a shared order key a higher
/// row id [DOM-020] (DEC-095).
#[tokio::test]
async fn an_opening_not_before_the_closing_is_refused() {
    let f = Fixture::open().await;
    let earlier = f
        .store(&buy(day(1), f.record("B1", 0).await, dec!(100)))
        .await;
    let shared = f.record("S1", 5).await;
    // Derived from the closing's own record, so it shares the closing's order key; stored first,
    // it has the lower row id and precedes the closing.
    let tied_before = f.store(&buy(day(2), shared.clone(), dec!(100))).await;
    let closing = f.store(&sell(day(2), shared.clone(), dec!(10))).await;
    let tied_after = f.store(&buy(day(2), shared, dec!(100))).await;
    let same_day_later = f
        .store(&buy(day(2), f.record("B2", 6).await, dec!(100)))
        .await;
    let next_day = f
        .store(&buy(day(3), f.record("B3", 0).await, dec!(100)))
        .await;

    for late in [tied_after, same_day_later, next_day] {
        let allocations = [allocate(earlier, dec!(5)), allocate(late, dec!(5))];
        match approve(&f.database, closing, &allocations).await {
            Err(AttributionError::NotBeforeClosing {
                opening,
                closing: named,
            }) => {
                assert_eq!(opening, late);
                assert_eq!(named, closing);
            }
            other => panic!("expected the DOM-020 refusal, got {other:?}"),
        }
    }

    let allocations = [allocate(earlier, dec!(5)), allocate(tied_before, dec!(5))];
    let attribution = approve(&f.database, closing, &allocations)
        .await
        .expect("a tie broken by the lower row id precedes");
    assert_eq!(f.allocations(attribution).await, allocations);
}

/// Allocations one quantum short or over the closed quantity are refused and nothing is stored
/// [DOM-065] (DEC-091).
#[tokio::test]
async fn allocations_not_summing_to_the_closing_are_refused() {
    let f = Fixture::open().await;
    let first = f
        .store(&buy(day(1), f.record("B1", 0).await, dec!(100)))
        .await;
    let second = f
        .store(&buy(day(2), f.record("B2", 0).await, dec!(100)))
        .await;
    let closing = f
        .store(&sell(day(3), f.record("S1", 0).await, dec!(10)))
        .await;

    for last in [dec!(3.99999999), dec!(4.00000001)] {
        let allocations = [allocate(first, dec!(6)), allocate(second, last)];
        match approve(&f.database, closing, &allocations).await {
            Err(AttributionError::Uncovered(uncovered)) => {
                assert_eq!(uncovered.allocated, dec!(6) + last);
                assert_eq!(uncovered.closed, dec!(10));
            }
            other => panic!("expected the DOM-065 refusal, got {other:?}"),
        }
    }

    approve(
        &f.database,
        closing,
        &[allocate(first, dec!(6)), allocate(second, dec!(4))],
    )
    .await
    .expect("nothing was stored by the refusals");
}

/// A closing linked to no opening is refused [DOM-018], so it cannot count as attributed and
/// unblock later closings while consuming nothing.
#[tokio::test]
async fn a_closing_allocated_to_nothing_is_refused() {
    let f = Fixture::open().await;
    let opening = f
        .store(&buy(day(1), f.record("B1", 0).await, dec!(100)))
        .await;
    let closing = f
        .store(&sell(day(2), f.record("S1", 0).await, dec!(10)))
        .await;

    assert!(matches!(
        approve(&f.database, closing, &[]).await,
        Err(AttributionError::NoAllocations { closing: named }) if named == closing
    ));
    approve(&f.database, closing, &[allocate(opening, dec!(10))])
        .await
        .expect("nothing was stored by the refusal");
}

/// Only a closing is attributed, and only to openings [DOM-018]; an unknown transaction, opening
/// or closing, is storage's refusal.
#[tokio::test]
async fn only_a_closing_is_attributed_to_openings() {
    let f = Fixture::open().await;
    let opening = f
        .store(&buy(day(1), f.record("B1", 0).await, dec!(100)))
        .await;
    let other_closing = f
        .store(&sell(day(2), f.record("S1", 0).await, dec!(10)))
        .await;
    let closing = f
        .store(&sell(day(3), f.record("S2", 0).await, dec!(10)))
        .await;

    assert!(matches!(
        approve(&f.database, opening, &[allocate(opening, dec!(10))]).await,
        Err(AttributionError::NotAClosing { transaction }) if transaction == opening
    ));
    assert!(matches!(
        approve(&f.database, closing, &[allocate(other_closing, dec!(10))]).await,
        Err(AttributionError::NotAnOpening { transaction }) if transaction == other_closing
    ));
    let unknown = TransactionId::new(999);
    assert!(matches!(
        approve(&f.database, closing, &[allocate(unknown, dec!(10))]).await,
        Err(AttributionError::Storage(StorageError::UnknownTransaction { transaction }))
            if transaction == unknown
    ));
    assert!(matches!(
        approve(&f.database, unknown, &[allocate(opening, dec!(10))]).await,
        Err(AttributionError::Storage(StorageError::UnknownTransaction { transaction }))
            if transaction == unknown
    ));
}

/// An expiration states no quantity for its allocations to sum to, so it is refused rather than
/// stored unchecked (DEC-102, provisional).
#[tokio::test]
async fn an_expiration_is_refused() {
    let f = Fixture::open().await;
    let opening = f
        .store(&buy(day(1), f.record("B1", 0).await, dec!(100)))
        .await;
    let closing = f.store(&expiration(day(2), f.record("E1", 0).await)).await;

    assert!(matches!(
        approve(&f.database, closing, &[allocate(opening, dec!(100))]).await,
        Err(AttributionError::QuantityNotStated { closing: named }) if named == closing
    ));
}

/// Storage's own refusals stay behind the service: a second approval of the same closing is
/// refused as already attributed [DOM-066].
#[tokio::test]
async fn a_second_approval_is_storages_refusal() {
    let f = Fixture::open().await;
    let opening = f
        .store(&buy(day(1), f.record("B1", 0).await, dec!(100)))
        .await;
    let closing = f
        .store(&sell(day(2), f.record("S1", 0).await, dec!(10)))
        .await;

    let first = approve(&f.database, closing, &[allocate(opening, dec!(10))])
        .await
        .expect("the first approval");
    match approve(&f.database, closing, &[allocate(opening, dec!(10))]).await {
        Err(AttributionError::Storage(StorageError::ClosingAlreadyAttributed {
            closing: named,
            attribution,
        })) => {
            assert_eq!(named, closing);
            assert_eq!(attribution, first);
        }
        other => panic!("expected the already-attributed refusal, got {other:?}"),
    }
}
