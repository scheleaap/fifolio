//! Integration layer: approving an attribution through the service, against a real temporary
//! SQLite database, in process [TST-003], [TST-004].
//!
//! Every refusal is followed by the approval that should succeed, because a refusal that stored
//! part of its attribution would leave the closing attributed and refuse the second approval.
//! Every test opens its own file through the shared helper. No test reaches the network.

use std::collections::BTreeMap;
use std::num::NonZeroU32;

use chrono::NaiveDate;
use fifolio_core::allocation::{AgainstOpening, Half, OpeningShares, opening_shares};
use fifolio_core::attribution::{AttributionError, approve};
use fifolio_core::decimal::{FxRate, Money, Quantity, QuotedPrice, Scaled};
use fifolio_core::entities::{
    Account, ImportBatch, ImportCounts, Isin, Order, Quotation, Security, SecurityType,
    SourceFormat, SourceRecord,
};
use fifolio_core::fifo::{Proposal, propose};
use fifolio_core::identity::{IdentitySource, identify};
use fifolio_core::manual_entry::Ratio;
use fifolio_core::storage::{
    Allocation, BatchId, Database, Placement, RecordHandle, StorageError, TransactionId,
};
use fifolio_core::transaction::{
    Buy, BuyOrigin, DateProvenance, Derivation, Expiration, Opening, Sell, Split, Transaction,
    TransferInSource, TransferOut,
};
use fifolio_core::transfer::EmissionError;
use fifolio_core::valuation::{Conversion, Currency, RateSource, Valued};
use fifolio_test_support::TempDb;
use proptest::prelude::*;
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
        Ratio::new(NonZeroU32::MIN, NonZeroU32::MIN),
        Conversion::native(on),
        other_isin(),
    )
    .into()
}

/// A buy of `quantity` for `gross` plus `fees`, so two parcels of one size can differ in cost.
fn buy_for(
    on: NaiveDate,
    record: RecordHandle,
    quantity: Decimal,
    gross: Decimal,
    fees: Decimal,
) -> Transaction {
    Buy::new(
        Derivation::new(on, vec1![record]),
        Quantity::new(quantity),
        Valued::new(QuotedPrice::new(dec!(10)), QuotedPrice::new(dec!(10))),
        money(gross),
        money(fees),
        BuyOrigin::Purchase,
        Conversion::native(on),
    )
    .into()
}

/// An exchange of `quantity` into [`other_isin`] at `numerator` for `denominator`, carrying
/// `fees` of its own.
fn exchange(
    on: NaiveDate,
    record: RecordHandle,
    quantity: Decimal,
    (numerator, denominator): (u32, u32),
    fees: Decimal,
) -> Transaction {
    TransferOut::new(
        Derivation::new(on, vec1![record]),
        Quantity::new(quantity),
        money(fees),
        Ratio::new(
            NonZeroU32::new(numerator).expect("non-zero"),
            NonZeroU32::new(denominator).expect("non-zero"),
        ),
        Conversion::native(on),
        other_isin(),
    )
    .into()
}

struct Fixture {
    db: TempDb,
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
            db,
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

    async fn find(&self, id: TransactionId) -> Transaction {
        self.database
            .transactions()
            .find(id)
            .await
            .expect("read back")
            .expect("stored")
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

/// Approving a `transfer_out` emits one `transfer_in` per consumed parcel, in the target security
/// of the same account. Two parcels of one size bought at different prices emerge at their own
/// costs, the one an earlier sale took part of with what that sale left, and each buy fee travels
/// as fees; each keeps its acquisition date as `inherited`, cites the transfer's record and takes
/// its parcel's place; the quantities, at one-for-three, sum to the product rounded once. Here the
/// last record's own rounding is also what the others leave, so which of the two it takes is
/// shown by `transfer`'s unit tests, not by this one [DOM-090], [DOM-106], [DOM-115], TST-010.
#[tokio::test]
async fn a_transfer_out_emits_one_record_per_parcel_at_its_own_cost() {
    let f = Fixture::open().await;
    let cheap = f
        .store(&buy_for(
            day(1),
            f.record("B1", 0).await,
            dec!(10),
            dec!(1000.00),
            dec!(5.00),
        ))
        .await;
    let dear = f
        .store(&buy_for(
            day(2),
            f.record("B2", 0).await,
            dec!(10),
            dec!(2000.00),
            dec!(7.00),
        ))
        .await;
    let sale = f
        .store(&sell(day(3), f.record("S1", 0).await, dec!(5)))
        .await;
    approve(&f.database, sale, &[allocate(cheap, dec!(5))])
        .await
        .expect("the earlier sale is approved");
    let exchange_row = f.record("X1", 0).await;
    let transfer = f
        .store(&exchange(
            day(4),
            exchange_row.clone(),
            dec!(15),
            (1, 3),
            dec!(0.00),
        ))
        .await;

    approve(
        &f.database,
        transfer,
        &[allocate(dear, dec!(10)), allocate(cheap, dec!(5))],
    )
    .await
    .expect("the transfer is approved");

    // Row ids are assigned in insert order, and the emission is the only insert after the
    // transfer, in the canonical order of the parcels.
    let emitted = [
        TransactionId::new(transfer.get() + 1),
        TransactionId::new(transfer.get() + 2),
    ];

    let expected = [
        // The sale took half of the cheap parcel's cost and fee; the transfer exhausts it and
        // carries the rest [DOM-062]. Halves come out even here, so these figures do not tell
        // the exhausting remainder from a plain share; the test after this one does.
        (cheap, day(1), dec!(1.66666667), dec!(500.00), dec!(2.50)),
        (dear, day(2), dec!(3.33333333), dec!(2000.00), dec!(7.00)),
    ];
    let mut total = Decimal::ZERO;
    for (id, (parcel, acquired, quantity, cost, fees)) in emitted.iter().zip(expected) {
        let Transaction::Opening(Opening::TransferIn(transfer_in)) = f.find(*id).await else {
            panic!("expected an emitted transfer_in at {id}");
        };
        let parcel = f.find(parcel).await;
        assert_eq!(transfer_in.quantity(), Quantity::new(quantity));
        assert_eq!(transfer_in.cost_basis(), money(cost));
        assert_eq!(transfer_in.fees(), money(fees));
        assert_eq!(transfer_in.acquisition_date(), acquired);
        assert_eq!(transfer_in.date_provenance(), DateProvenance::Inherited);
        assert_eq!(transfer_in.source(), TransferInSource::CorporateAction);
        assert_eq!(
            transfer_in.derivation().cites(),
            [exchange_row.identity().clone()]
        );
        assert_eq!(transfer_in.derivation().order_key(), parcel.order_key());
        total += transfer_in.quantity().get();

        match f.database.transactions().delete(*id).await {
            Err(StorageError::EmittedTransferIn { transfer_out, .. }) => {
                assert_eq!(
                    transfer_out, transfer,
                    "linked to the transfer that emitted it"
                );
            }
            other => panic!("expected the DOM-094 refusal, got {other:?}"),
        }
    }
    assert_eq!(total, dec!(5), "15 at one-for-three, rounded once");

    // The records are openings of the target security in the same account: a sale there is
    // attributed to them, the older parcel first [DOM-019], [DOM-056].
    let target_sale = f
        .store_in(
            account(),
            other_isin(),
            &sell(day(5), f.record("S2", 0).await, dec!(2)),
        )
        .await;
    let proposal = f.proposal(target_sale, &emitted).await;
    assert_eq!(
        proposal,
        [
            allocate(emitted[0], dec!(1.66666667)),
            allocate(emitted[1], dec!(0.33333333))
        ]
    );
    approve(&f.database, target_sale, &proposal)
        .await
        .expect("the emitted parcels are sold in the target security");
}

/// A parcel two earlier sales took part of reaches the transfer with what they left, in each half
/// from that half: 6 units bought for USD 10.00 (EUR 9.09) plus USD 1.00 (EUR 0.91) fees, two
/// sales of 1 taking 1.67 (1.52) and 0.17 (0.15) each, and the transfer of the last 4 exhausting
/// the parcel, so it takes the rest: 6.66 (6.05) and 0.66 (0.61). A plain share of 4 would be
/// 6.67 (6.06) and 0.67, either sale's own share 1.67, and swapped halves 6.05 (6.66) [DOM-062],
/// [DOM-106], [DOM-028].
#[tokio::test]
async fn a_transfer_out_carries_what_earlier_sales_left_of_each_half() {
    let f = Fixture::open().await;
    let usd = Conversion::new(
        Currency::new("USD"),
        FxRate::new(dec!(1.100000)),
        RateSource::Ecb,
        day(1),
    );
    let parcel = f
        .store(
            &Buy::new(
                Derivation::new(day(1), vec1![f.record("B1", 0).await]),
                Quantity::new(dec!(6)),
                Valued::new(
                    QuotedPrice::new(dec!(1.6667)),
                    QuotedPrice::new(dec!(1.515)),
                ),
                Valued::new(Money::new(dec!(10.00)), Money::new(dec!(9.09))),
                Valued::new(Money::new(dec!(1.00)), Money::new(dec!(0.91))),
                BuyOrigin::Purchase,
                usd.clone(),
            )
            .into(),
        )
        .await;
    for (on, reference) in [(day(2), "S1"), (day(3), "S2")] {
        let sale = f
            .store(&sell(on, f.record(reference, 0).await, dec!(1)))
            .await;
        approve(&f.database, sale, &[allocate(parcel, dec!(1))])
            .await
            .expect("the earlier sale is approved");
    }
    let transfer = f
        .store(&exchange(
            day(4),
            f.record("X1", 0).await,
            dec!(4),
            (1, 1),
            dec!(0.00),
        ))
        .await;

    approve(&f.database, transfer, &[allocate(parcel, dec!(4))])
        .await
        .expect("the transfer is approved");

    let emitted = TransactionId::new(transfer.get() + 1);
    let Transaction::Opening(Opening::TransferIn(transfer_in)) = f.find(emitted).await else {
        panic!("expected an emitted transfer_in at {emitted}");
    };
    assert_eq!(transfer_in.quantity(), Quantity::new(dec!(4)));
    assert_eq!(
        transfer_in.cost_basis(),
        Valued::new(Money::new(dec!(6.66)), Money::new(dec!(6.05)))
    );
    assert_eq!(
        transfer_in.fees(),
        Valued::new(Money::new(dec!(0.66)), Money::new(dec!(0.61)))
    );
    assert_eq!(transfer_in.conversion(), &usd);
}

/// A parcel the transfer consumes only in part carries the plain share of the quantity allocated,
/// in each half from that half, whatever an earlier sale took: 10 units bought for USD 10.00
/// (EUR 9.09) plus USD 1.00 (EUR 0.91) fees, a sale of 3 taking 3.00 (2.73) and 0.30 (0.27), and
/// the transfer of 4 taking 4.00 (3.64) and 0.40 (0.36). The parcel's remaining 7 would give 7.00,
/// and the earlier sale's share 3.00. A later sale of the last 3 exhausts the parcel and receives
/// the rest, 3.00 (2.72) and 0.30 (0.28), so the three shares sum to the buy's figures [DOM-059],
/// [DOM-061], [DOM-062], [DOM-106].
#[tokio::test]
async fn a_transfer_out_carries_a_plain_share_of_a_parcel_it_consumes_in_part() {
    let f = Fixture::open().await;
    let usd = Conversion::new(
        Currency::new("USD"),
        FxRate::new(dec!(1.100000)),
        RateSource::Ecb,
        day(1),
    );
    let parcel = f
        .store(
            &Buy::new(
                Derivation::new(day(1), vec1![f.record("B1", 0).await]),
                Quantity::new(dec!(10)),
                Valued::new(QuotedPrice::new(dec!(1)), QuotedPrice::new(dec!(0.909))),
                Valued::new(Money::new(dec!(10.00)), Money::new(dec!(9.09))),
                Valued::new(Money::new(dec!(1.00)), Money::new(dec!(0.91))),
                BuyOrigin::Purchase,
                usd,
            )
            .into(),
        )
        .await;
    let earlier_sale = f
        .store(&sell(day(2), f.record("S1", 0).await, dec!(3)))
        .await;
    approve(&f.database, earlier_sale, &[allocate(parcel, dec!(3))])
        .await
        .expect("the earlier sale is approved");
    let transfer = f
        .store(&exchange(
            day(3),
            f.record("X1", 0).await,
            dec!(4),
            (1, 1),
            dec!(0.00),
        ))
        .await;

    approve(&f.database, transfer, &[allocate(parcel, dec!(4))])
        .await
        .expect("the transfer is approved");

    let emitted = TransactionId::new(transfer.get() + 1);
    let Transaction::Opening(Opening::TransferIn(transfer_in)) = f.find(emitted).await else {
        panic!("expected an emitted transfer_in at {emitted}");
    };
    assert_eq!(transfer_in.quantity(), Quantity::new(dec!(4)));
    assert_eq!(
        transfer_in.cost_basis(),
        Valued::new(Money::new(dec!(4.00)), Money::new(dec!(3.64)))
    );
    assert_eq!(
        transfer_in.fees(),
        Valued::new(Money::new(dec!(0.40)), Money::new(dec!(0.36)))
    );

    let later_sale = f
        .store(&sell(day(4), f.record("S2", 0).await, dec!(3)))
        .await;
    approve(&f.database, later_sale, &[allocate(parcel, dec!(3))])
        .await
        .expect("the later sale takes what the transfer left");

    let opening = f.find(parcel).await;
    let mut against = Vec::new();
    for (closing, quantity) in [
        (earlier_sale, dec!(3)),
        (transfer, dec!(4)),
        (later_sale, dec!(3)),
    ] {
        against.push(AgainstOpening::new(
            closing,
            f.find(closing).await.order_key(),
            Quantity::new(quantity),
        ));
    }
    let shares = |half| {
        opening_shares(opening.opening().expect("an opening"), &[], &against, half)
            .expect("the shares")
    };
    let figures = |shares: &[(TransactionId, OpeningShares)], of| {
        shares
            .iter()
            .find_map(|(closing, share)| {
                (*closing == of).then(|| (share.cost().get(), share.buy_fee().get()))
            })
            .expect("a share of each closing")
    };
    let (native, eur) = (shares(Half::Native), shares(Half::Eur));
    assert_eq!(figures(&native, earlier_sale), (dec!(3.00), dec!(0.30)));
    assert_eq!(figures(&eur, earlier_sale), (dec!(2.73), dec!(0.27)));
    assert_eq!(
        (figures(&native, transfer), figures(&eur, transfer)),
        (
            (
                transfer_in.cost_basis().native().get(),
                transfer_in.fees().native().get()
            ),
            (
                transfer_in.cost_basis().eur().get(),
                transfer_in.fees().eur().get()
            )
        ),
        "the record carries the transfer's own share"
    );
    assert_eq!(figures(&native, later_sale), (dec!(3.00), dec!(0.30)));
    assert_eq!(figures(&eur, later_sale), (dec!(2.72), dec!(0.28)));
}

proptest! {
    // Each case opens its own database, so fewer cases than the pure properties run.
    #![proptest_config(ProptestConfig::with_cases(16))]

    /// A transfer out preserves total cost basis and parcel count: after earlier sales took part
    /// of some buys, a transfer of everything they left emits one record per buy, and each
    /// carries, in each half, that buy's cost and fees less what the earlier sale took, so the
    /// records sum per half to what the openings had left [DOM-062], [DOM-106], TST-010.
    ///
    /// The oracle rounds a sale's share in integers of cents, `(2cs + q) div 2q` for `c * s / q`,
    /// half away from zero, rather than through the allocation module it checks.
    #[test]
    fn a_transfer_out_carries_what_earlier_sales_left_of_every_buy(
        buys in prop::collection::vec(
            (
                2i64..=20,
                0i64..20,
                [1i64..=1_000_000, 1..=1_000_000, 0..=10_000, 0..=10_000],
            ),
            1..=3,
        ),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        runtime.block_on(async {
            let f = Fixture::open().await;
            let usd = |on| {
                Conversion::new(Currency::new("USD"), FxRate::new(dec!(1.100000)), RateSource::Ecb, on)
            };
            let cents = |amount| Decimal::new(amount, 2);
            let mut stored = Vec::with_capacity(buys.len());
            for (k, (quantity, _, [cost, cost_eur, fee, fee_eur])) in (1..).zip(&buys) {
                let id = f
                    .store(
                        &Buy::new(
                            Derivation::new(day(k), vec1![f.record(&format!("B{k}"), 0).await]),
                            Quantity::new(Decimal::from(*quantity)),
                            Valued::new(QuotedPrice::new(dec!(1)), QuotedPrice::new(dec!(1))),
                            Valued::new(Money::new(cents(*cost)), Money::new(cents(*cost_eur))),
                            Valued::new(Money::new(cents(*fee)), Money::new(cents(*fee_eur))),
                            BuyOrigin::Purchase,
                            usd(day(k)),
                        )
                        .into(),
                    )
                    .await;
                stored.push(id);
            }
            let sold_of = |quantity: i64, seed: i64| seed % quantity;
            for (k, (id, (quantity, seed, _))) in (10..).zip(stored.iter().zip(&buys)) {
                let sold = sold_of(*quantity, *seed);
                if sold > 0 {
                    let sale = f
                        .store(&sell(day(k), f.record(&format!("S{k}"), 0).await, Decimal::from(sold)))
                        .await;
                    approve(&f.database, sale, &[allocate(*id, Decimal::from(sold))])
                        .await
                        .expect("the earlier sale is approved");
                }
            }
            let allocations: Vec<_> = stored
                .iter()
                .zip(&buys)
                .map(|(id, (quantity, seed, _))| allocate(*id, Decimal::from(quantity - sold_of(*quantity, *seed))))
                .collect();
            let transferred = allocations.iter().map(|allocation| allocation.quantity().get()).sum();
            let transfer = f
                .store(&exchange(day(20), f.record("X1", 0).await, transferred, (1, 1), dec!(0.00)))
                .await;

            approve(&f.database, transfer, &allocations)
                .await
                .expect("the transfer is approved");

            let left = |figure: i64, quantity: i64, sold: i64| {
                cents(figure - (2 * figure * sold + quantity).div_euclid(2 * quantity))
            };
            for (k, (quantity, seed, [cost, cost_eur, fee, fee_eur])) in (1..).zip(&buys) {
                let sold = sold_of(*quantity, *seed);
                let emitted = TransactionId::new(transfer.get() + k);
                let Transaction::Opening(Opening::TransferIn(transfer_in)) = f.find(emitted).await
                else {
                    panic!("expected an emitted transfer_in at {emitted}");
                };
                prop_assert_eq!(transfer_in.quantity().get(), Decimal::from(quantity - sold));
                prop_assert_eq!(
                    transfer_in.cost_basis(),
                    Valued::new(
                        Money::new(left(*cost, *quantity, sold)),
                        Money::new(left(*cost_eur, *quantity, sold))
                    )
                );
                prop_assert_eq!(
                    transfer_in.fees(),
                    Valued::new(
                        Money::new(left(*fee, *quantity, sold)),
                        Money::new(left(*fee_eur, *quantity, sold))
                    )
                );
            }
            let after = TransactionId::new(transfer.get() + i64::try_from(buys.len()).expect("few") + 1);
            prop_assert_eq!(
                f.database.transactions().find(after).await.expect("read"),
                None,
                "one record per buy"
            );
            Ok(())
        })?;
    }
}

/// Approving a `transfer_out` again is storage's refusal, and emits nothing more: the attribution
/// is refused before any record is written. The parcel is not exhausted, so the service's own
/// over-allocation check does not answer first [DOM-066], [DOM-090].
#[tokio::test]
async fn a_second_approval_of_a_transfer_out_emits_nothing() {
    let f = Fixture::open().await;
    let opening = f
        .store(&buy(day(1), f.record("B1", 0).await, dec!(20)))
        .await;
    let transfer = f
        .store(&exchange(
            day(2),
            f.record("X1", 0).await,
            dec!(10),
            (1, 1),
            dec!(0.00),
        ))
        .await;
    let first = approve(&f.database, transfer, &[allocate(opening, dec!(10))])
        .await
        .expect("the first approval");

    match approve(&f.database, transfer, &[allocate(opening, dec!(10))]).await {
        Err(AttributionError::Storage(StorageError::ClosingAlreadyAttributed {
            closing,
            attribution,
        })) => {
            assert_eq!(closing, transfer);
            assert_eq!(attribution, first);
        }
        other => panic!("expected the already-attributed refusal, got {other:?}"),
    }
    assert!(matches!(
        f.find(TransactionId::new(transfer.get() + 1)).await,
        Transaction::Opening(Opening::TransferIn(_))
    ));
    assert_eq!(
        f.database
            .transactions()
            .find(TransactionId::new(transfer.get() + 2))
            .await
            .expect("read"),
        None,
        "no record was emitted by the second approval"
    );
}

/// A parcel split in the source security before the transfer is carried in its current units:
/// 5 bought for 50.00 plus 3.00 fees, split two-for-one, transfer out as 10, which is the whole
/// parcel, so the record carries the full cost and fee. A split of the same security in another
/// account, which would make the 10 half of 20 and carry 25.00 and 1.50, and a split of the target
/// after the transfer, which reading the target's splits instead would leave 5 units to cover 10
/// with, must not count [DOM-106], [DOM-103].
#[tokio::test]
async fn a_transfer_out_carries_a_parcel_split_before_it_in_current_units() {
    let f = Fixture::open().await;
    let two_for_one = || Ratio::new(NonZeroU32::new(2).expect("non-zero"), NonZeroU32::MIN);
    let parcel = f
        .store(&buy_for(
            day(1),
            f.record("B1", 0).await,
            dec!(5),
            dec!(50.00),
            dec!(3.00),
        ))
        .await;
    for (account, security, on, reference) in [
        (account(), isin(), day(2), "P1"),
        (other_account(), isin(), day(2), "P2"),
        (account(), other_isin(), day(4), "P3"),
    ] {
        f.store_in(
            account,
            security,
            &Split::new(
                Derivation::new(on, vec1![f.record(reference, 0).await]),
                two_for_one(),
            )
            .into(),
        )
        .await;
    }
    let transfer = f
        .store(&exchange(
            day(3),
            f.record("X1", 0).await,
            dec!(10),
            (1, 1),
            dec!(0.00),
        ))
        .await;

    approve(&f.database, transfer, &[allocate(parcel, dec!(10))])
        .await
        .expect("the transfer is approved");

    let emitted = TransactionId::new(transfer.get() + 1);
    let Transaction::Opening(Opening::TransferIn(transfer_in)) = f.find(emitted).await else {
        panic!("expected an emitted transfer_in at {emitted}");
    };
    assert_eq!(transfer_in.quantity(), Quantity::new(dec!(10)));
    assert_eq!(transfer_in.cost_basis(), money(dec!(50.00)));
    assert_eq!(transfer_in.fees(), money(dec!(3.00)));
    assert_eq!(transfer_in.acquisition_date(), day(1));
}

/// A transfer carrying a fee of its own is refused naming its row, and neither the attribution
/// nor any record is stored [DOM-107], DEC-080.
#[tokio::test]
async fn a_transfer_with_a_fee_of_its_own_is_refused_and_stores_nothing() {
    let f = Fixture::open().await;
    let opening = f
        .store(&buy(day(1), f.record("B1", 0).await, dec!(10)))
        .await;
    let row = f.record("X1", 0).await;
    let transfer = f
        .store(&exchange(day(2), row.clone(), dec!(10), (1, 1), dec!(2.50)))
        .await;
    let allocations = [allocate(opening, dec!(10))];

    match approve(&f.database, transfer, &allocations).await {
        Err(AttributionError::Emission {
            transfer_out,
            source: EmissionError::CarriesFee { records },
        }) => {
            assert_eq!(transfer_out, transfer);
            assert_eq!(records, [row.identity().clone()]);
        }
        other => panic!("expected the DOM-107 refusal, got {other:?}"),
    }

    assert_nothing_stored(&f, transfer, &allocations).await;
}

/// Records that would sort before a closing already in the target security are refused, and
/// nothing is stored (DEC-105, provisional).
#[tokio::test]
async fn a_transfer_reaching_back_past_the_targets_history_is_refused() {
    let f = Fixture::open().await;
    let opening = f
        .store(&buy(day(1), f.record("B1", 0).await, dec!(10)))
        .await;
    let target_sale = f
        .store_in(
            account(),
            other_isin(),
            &sell(day(2), f.record("S1", 0).await, dec!(1)),
        )
        .await;
    let transfer = f
        .store(&exchange(
            day(3),
            f.record("X1", 0).await,
            dec!(10),
            (1, 1),
            dec!(0.00),
        ))
        .await;
    let allocations = [allocate(opening, dec!(10))];

    match approve(&f.database, transfer, &allocations).await {
        Err(AttributionError::ReachesBackPast {
            transfer_out,
            target,
            transaction,
        }) => {
            assert_eq!(transfer_out, transfer);
            assert_eq!(target, other_isin());
            assert_eq!(transaction, target_sale);
        }
        other => panic!("expected the DEC-105 refusal, got {other:?}"),
    }

    assert_nothing_stored(&f, transfer, &allocations).await;
}

/// Nothing of a refused approval stands: storage still takes the attribution, which it would
/// refuse had one been written, and no record follows the transfer.
async fn assert_nothing_stored(f: &Fixture, transfer: TransactionId, allocations: &[Allocation]) {
    let next = TransactionId::new(transfer.get() + 1);
    assert_eq!(
        f.database.transactions().find(next).await.expect("read"),
        None,
        "no record was emitted"
    );
    f.database
        .attributions()
        .approve(transfer, allocations)
        .await
        .expect("no attribution was stored");
}

/// A split of the target sorting between the parcel and the transfer is refused: it would rescale
/// units that did not yet exist there (DEC-105, provisional).
#[tokio::test]
async fn a_split_of_the_target_between_parcel_and_transfer_is_refused() {
    let f = Fixture::open().await;
    let opening = f
        .store(&buy(day(1), f.record("B1", 0).await, dec!(10)))
        .await;
    let split = f
        .store_in(
            account(),
            other_isin(),
            &Split::new(
                Derivation::new(day(2), vec1![f.record("P1", 0).await]),
                Ratio::new(NonZeroU32::new(2).expect("non-zero"), NonZeroU32::MIN),
            )
            .into(),
        )
        .await;
    let transfer = f
        .store(&exchange(
            day(3),
            f.record("X1", 0).await,
            dec!(10),
            (1, 1),
            dec!(0.00),
        ))
        .await;
    let allocations = [allocate(opening, dec!(10))];

    match approve(&f.database, transfer, &allocations).await {
        Err(AttributionError::ReachesBackPast { transaction, .. }) => {
            assert_eq!(transaction, split);
        }
        other => panic!("expected the DEC-105 refusal, got {other:?}"),
    }

    assert_nothing_stored(&f, transfer, &allocations).await;
}

/// With several parcels the window opens at the earliest of them, whatever order they are given
/// in: a sale of the target between the two parcels is refused, though it precedes the later
/// parcel, which is given first (DEC-105, provisional).
#[tokio::test]
async fn a_closing_of_the_target_after_the_earliest_of_several_parcels_is_refused() {
    let f = Fixture::open().await;
    let earlier = f
        .store(&buy(day(1), f.record("B1", 0).await, dec!(10)))
        .await;
    let target_sale = f
        .store_in(
            account(),
            other_isin(),
            &sell(day(2), f.record("S1", 0).await, dec!(1)),
        )
        .await;
    let later = f
        .store(&buy(day(3), f.record("B2", 0).await, dec!(10)))
        .await;
    let transfer = f
        .store(&exchange(
            day(4),
            f.record("X1", 0).await,
            dec!(20),
            (1, 1),
            dec!(0.00),
        ))
        .await;
    let allocations = [allocate(later, dec!(10)), allocate(earlier, dec!(10))];

    match approve(&f.database, transfer, &allocations).await {
        Err(AttributionError::ReachesBackPast { transaction, .. }) => {
            assert_eq!(transaction, target_sale);
        }
        other => panic!("expected the DEC-105 refusal, got {other:?}"),
    }

    assert_nothing_stored(&f, transfer, &allocations).await;
}

/// An expiration or a `transfer_out` of the target between parcel and transfer is a closing the
/// records would reach back past, as a sale is (DEC-105, provisional).
#[tokio::test]
async fn an_expiration_or_transfer_out_of_the_target_between_is_refused() {
    for kind in ["expiration", "transfer_out"] {
        let f = Fixture::open().await;
        let opening = f
            .store(&buy(day(1), f.record("B1", 0).await, dec!(10)))
            .await;
        let row = f.record("C1", 0).await;
        let closing = match kind {
            "expiration" => expiration(day(2), row),
            _ => transfer_out(day(2), row, dec!(1)),
        };
        let target_closing = f.store_in(account(), other_isin(), &closing).await;
        let transfer = f
            .store(&exchange(
                day(3),
                f.record("X1", 0).await,
                dec!(10),
                (1, 1),
                dec!(0.00),
            ))
            .await;
        let allocations = [allocate(opening, dec!(10))];

        match approve(&f.database, transfer, &allocations).await {
            Err(AttributionError::ReachesBackPast { transaction, .. }) => {
                assert_eq!(transaction, target_closing, "{kind}");
            }
            other => panic!("expected the DEC-105 refusal for {kind}, got {other:?}"),
        }

        assert_nothing_stored(&f, transfer, &allocations).await;
    }
}

/// An attributed closing of the target after the transfer is refused as well: it was attributed
/// without the parcels the transfer would place before it (DEC-105, provisional).
#[tokio::test]
async fn an_attributed_closing_of_the_target_after_the_transfer_is_refused() {
    let f = Fixture::open().await;
    let opening = f
        .store(&buy(day(1), f.record("B1", 0).await, dec!(10)))
        .await;
    let target_buy = f
        .store_in(
            account(),
            other_isin(),
            &buy(day(1), f.record("B2", 1).await, dec!(10)),
        )
        .await;
    let target_sale = f
        .store_in(
            account(),
            other_isin(),
            &sell(day(4), f.record("S1", 0).await, dec!(1)),
        )
        .await;
    approve(&f.database, target_sale, &[allocate(target_buy, dec!(1))])
        .await
        .expect("the later target sale is approved");
    // Stored last, though it sorts before the sale, so the next row id is the emission's.
    let transfer = f
        .store(&exchange(
            day(3),
            f.record("X1", 0).await,
            dec!(10),
            (1, 1),
            dec!(0.00),
        ))
        .await;
    let allocations = [allocate(opening, dec!(10))];

    match approve(&f.database, transfer, &allocations).await {
        Err(AttributionError::ReachesBackPast { transaction, .. }) => {
            assert_eq!(transaction, target_sale);
        }
        other => panic!("expected the DEC-105 refusal, got {other:?}"),
    }

    assert_nothing_stored(&f, transfer, &allocations).await;
}

/// What the refusal does not reach is approved: a buy of the target between parcel and transfer,
/// a closing of the target at exactly the parcel's key or the transfer's, an unattributed closing
/// of the target after the transfer, and a closing of the same security in another account
/// (DEC-105, provisional).
#[tokio::test]
async fn a_transfer_past_only_what_the_refusal_does_not_reach_is_approved() {
    let f = Fixture::open().await;
    let parcel_row = f.record("B1", 0).await;
    let opening = f.store(&buy(day(1), parcel_row.clone(), dec!(10))).await;
    let exchange_row = f.record("X1", 0).await;
    for transaction in [
        buy(day(2), f.record("B2", 0).await, dec!(10)),
        sell(day(1), parcel_row, dec!(1)),
        sell(day(3), exchange_row.clone(), dec!(1)),
        sell(day(4), f.record("S1", 0).await, dec!(1)),
    ] {
        f.store_in(account(), other_isin(), &transaction).await;
    }
    f.store_in(
        other_account(),
        other_isin(),
        &sell(day(2), f.record("S2", 1).await, dec!(1)),
    )
    .await;
    let transfer = f
        .store(&exchange(
            day(3),
            exchange_row,
            dec!(10),
            (1, 1),
            dec!(0.00),
        ))
        .await;

    approve(&f.database, transfer, &[allocate(opening, dec!(10))])
        .await
        .expect("nothing the refusal reaches lies in the way");
    assert!(matches!(
        f.find(TransactionId::new(transfer.get() + 1)).await,
        Transaction::Opening(Opening::TransferIn(_))
    ));
}

/// A failure after the attribution is written takes the attribution with it: a trigger refuses
/// the emission link, the last write, and neither the attribution nor the emitted record stands
/// [DOM-090].
#[tokio::test]
async fn a_failure_after_the_attribution_is_written_rolls_it_back() {
    let f = Fixture::open().await;
    let opening = f
        .store(&buy(day(1), f.record("B1", 0).await, dec!(10)))
        .await;
    let transfer = f
        .store(&exchange(
            day(2),
            f.record("X1", 0).await,
            dec!(10),
            (1, 1),
            dec!(0.00),
        ))
        .await;
    let pool = sqlx::SqlitePool::connect(&format!("sqlite:{}", f.db.path().display()))
        .await
        .expect("open the database file directly");
    sqlx::query(
        "create trigger refuse_emission before insert on emitted_transfer_in
         begin select raise(abort, 'emission refused by the test'); end",
    )
    .execute(&pool)
    .await
    .expect("install the failing trigger");
    let allocations = [allocate(opening, dec!(10))];

    match approve(&f.database, transfer, &allocations).await {
        Err(AttributionError::Storage(_)) => {}
        other => panic!("expected the injected storage failure, got {other:?}"),
    }

    sqlx::query("drop trigger refuse_emission")
        .execute(&pool)
        .await
        .expect("remove the failing trigger");
    pool.close().await;
    assert_nothing_stored(&f, transfer, &allocations).await;
}
