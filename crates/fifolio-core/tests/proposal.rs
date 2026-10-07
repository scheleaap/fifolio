//! Integration layer: the attribution proposal as it is shown, read from a real temporary SQLite
//! database, in process [TST-003], [TST-004].
//!
//! Every test opens its own file. No test reaches the network.

use std::collections::BTreeMap;
use std::num::NonZeroU32;

use chrono::NaiveDate;
use fifolio_core::attribution::approve;
use fifolio_core::decimal::{Money, Quantity, QuotedPrice, Scaled};
use fifolio_core::entities::{
    Account, ImportBatch, ImportCounts, Isin, Order, Quotation, Security, SecurityType,
    SourceFormat, SourceRecord,
};
use fifolio_core::fifo::ProposalError;
use fifolio_core::identity::{IdentitySource, identify};
use fifolio_core::manual_entry::Ratio;
use fifolio_core::proposal::{Offer, OfferedFigures, ProposalRefusal, propose_for, propose_next};
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

fn buy(
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

fn sell(
    on: NaiveDate,
    record: RecordHandle,
    quantity: Decimal,
    gross: Decimal,
    fees: Decimal,
) -> Transaction {
    Sell::new(
        Derivation::new(on, vec1![record]),
        Quantity::new(quantity),
        Valued::new(QuotedPrice::new(dec!(12)), QuotedPrice::new(dec!(12))),
        money(gross),
        money(fees),
        Conversion::native(on),
    )
    .into()
}

fn allocate(opening: TransactionId, quantity: Decimal) -> Allocation {
    Allocation::new(opening, Quantity::new(quantity))
}

struct Fixture {
    _db: TempDb,
    database: Database,
    batch: BatchId,
    other_batch: BatchId,
}

impl Fixture {
    /// A database holding both accounts, both securities and one import into each account.
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
                    "Philips",
                    SecurityType::Stock,
                    Quotation::PerUnit,
                ))
                .await
                .expect("the security");
        }
        let mut batches = Vec::new();
        for account in [account(), other_account()] {
            batches.push(
                database
                    .import_batches()
                    .insert(&ImportBatch::new(
                        account,
                        "2024.xlsx",
                        SourceFormat::SaxoNlXlsx,
                        "2024-05-31T09:00:00Z".parse().expect("a valid timestamp"),
                        ImportCounts::default(),
                    ))
                    .await
                    .expect("the import"),
            );
        }
        Self {
            _db: db,
            database,
            batch: batches[0],
            other_batch: batches[1],
        }
    }

    /// Stores a record of the first account at `order`, naming `fields`, and hands back its
    /// handle.
    async fn record_with(
        &self,
        reference: &str,
        order: u32,
        fields: &[(&str, &str)],
    ) -> RecordHandle {
        self.database
            .source_records()
            .insert(
                self.batch,
                &SourceRecord::new(
                    identify(&account(), &IdentitySource::BrokerReference(reference)),
                    Order::new(order),
                    "raw",
                    fields
                        .iter()
                        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                        .collect::<BTreeMap<_, _>>(),
                ),
            )
            .await
            .expect("the record")
    }

    /// A record naming no security, so it is never pending under the security's name.
    async fn record(&self, reference: &str, order: u32) -> RecordHandle {
        self.record_with(reference, order, &[]).await
    }

    async fn store(&self, transaction: &Transaction) -> TransactionId {
        self.database
            .transactions()
            .insert(
                &Placement::derived(account(), isin(), self.batch),
                transaction,
            )
            .await
            .expect("store a transaction")
    }

    async fn buy(
        &self,
        on: u32,
        quantity: Decimal,
        gross: Decimal,
        fees: Decimal,
    ) -> TransactionId {
        let record = self.record(&format!("buy-{on}"), on).await;
        self.store(&buy(day(on), record, quantity, gross, fees))
            .await
    }

    async fn sell(
        &self,
        on: u32,
        quantity: Decimal,
        gross: Decimal,
        fees: Decimal,
    ) -> TransactionId {
        let record = self.record(&format!("sell-{on}"), on).await;
        self.store(&sell(day(on), record, quantity, gross, fees))
            .await
    }

    async fn proposal(&self, closing: TransactionId) -> Offer {
        propose_for(&self.database, closing)
            .await
            .expect("a proposal")
    }
}

/// The five EUR figures of each offered allocation of a sell, in the order shown.
fn disposal_figures(offer: &Offer) -> Vec<(TransactionId, Decimal, [Decimal; 5])> {
    offer
        .allocations()
        .iter()
        .map(|offered| {
            let OfferedFigures::Disposal(figures) = offered.figures() else {
                panic!("a sell shows disposal figures: {offered:?}");
            };
            (
                offered.allocation().opening(),
                offered.allocation().quantity().get(),
                [
                    figures.cost().get(),
                    figures.buy_fee().get(),
                    figures.proceeds().get(),
                    figures.sell_fee().get(),
                    figures.gain().get(),
                ],
            )
        })
        .collect()
}

/// The oldest parcel is taken whole and the next in part, each with its cost, buy fee,
/// proceeds, sell fee and gain in EUR: the exhausted parcel's cost whole, the partial one's
/// rounded on its own, the closing's last allocation absorbing the proceeds and fee drift
/// [SRV-036], [DOM-056], [DOM-059], [DOM-061], [DOM-062], [DOM-063], [DOM-125].
#[tokio::test]
async fn a_sell_is_offered_its_fifo_allocations_with_their_figures() {
    let f = Fixture::open().await;
    let first = f.buy(1, dec!(10), dec!(100.00), dec!(3.00)).await;
    let second = f.buy(2, dec!(3), dec!(10.00), dec!(1.00)).await;
    let closing = f.sell(5, dec!(12), dec!(100.00), dec!(7.00)).await;

    let offer = f.proposal(closing).await;

    assert_eq!(offer.closing().id(), closing);
    assert_eq!(
        disposal_figures(&offer),
        [
            // 100 × 10/12 = 83.33, 7 × 10/12 = 5.83.
            (
                first,
                dec!(10),
                [
                    dec!(100.00),
                    dec!(3.00),
                    dec!(83.33),
                    dec!(5.83),
                    dec!(-25.50)
                ]
            ),
            // 10 × 2/3 = 6.67, 1 × 2/3 = 0.67; proceeds and fee are the remainders.
            (
                second,
                dec!(2),
                [dec!(6.67), dec!(0.67), dec!(16.67), dec!(1.17), dec!(8.16)]
            ),
        ]
    );
}

/// A parcel already sold from in part is offered what is left, and the allocation that
/// exhausts it carries its cost drift, as approving would store it: 10.00 over three units is
/// 3.33, 3.33 and then 3.34 [DOM-062], [DOM-064], [SRV-036].
#[tokio::test]
async fn the_allocation_exhausting_a_parcel_absorbs_its_drift() {
    let f = Fixture::open().await;
    let opening = f.buy(1, dec!(3), dec!(10.00), dec!(0.00)).await;
    for on in [2, 3] {
        let closing = f.sell(on, dec!(1), dec!(12.00), dec!(0.00)).await;
        approve(&f.database, closing, &[allocate(opening, dec!(1))])
            .await
            .expect("approve the earlier sale");
    }
    let last = f.sell(4, dec!(1), dec!(12.00), dec!(0.00)).await;

    assert_eq!(
        disposal_figures(&f.proposal(last).await),
        [(
            opening,
            dec!(1),
            [dec!(3.34), dec!(0.00), dec!(12.00), dec!(0.00), dec!(8.66)]
        )]
    );
}

/// A `transfer_out` is offered the parcels it consumes with the cost and buy fee each carries
/// onward, and no proceeds or gain [DOM-093], [DOM-106], [SRV-036].
#[tokio::test]
async fn a_transfer_out_is_offered_only_the_basis_it_carries() {
    let f = Fixture::open().await;
    let opening = f.buy(1, dec!(4), dec!(10.00), dec!(1.00)).await;
    let record = f.record("exchange", 5).await;
    let closing = f
        .store(
            &TransferOut::new(
                Derivation::new(day(5), vec1![record]),
                Quantity::new(dec!(3)),
                money(dec!(0.00)),
                Ratio::new(NonZeroU32::MIN, NonZeroU32::MIN),
                Conversion::native(day(5)),
                other_isin(),
            )
            .into(),
        )
        .await;

    let offer = f.proposal(closing).await;

    let [offered] = offer.allocations() else {
        panic!("one allocation: {offer:?}");
    };
    assert_eq!(offered.allocation(), allocate(opening, dec!(3)));
    let OfferedFigures::Transfer(shares) = offered.figures() else {
        panic!("a transfer shows its basis only: {offered:?}");
    };
    // 10.00 × 3/4 and 1.00 × 3/4.
    assert_eq!(
        (shares.cost().get(), shares.buy_fee().get()),
        (dec!(7.50), dec!(0.75))
    );
}

/// Too little open: the missing quantity instead of a proposal [SRV-038], [DOM-057].
#[tokio::test]
async fn an_uncoverable_closing_names_its_shortfall() {
    let f = Fixture::open().await;
    f.buy(1, dec!(5), dec!(50.00), dec!(0.00)).await;
    let closing = f.sell(5, dec!(7.5), dec!(90.00), dec!(0.00)).await;

    match propose_for(&f.database, closing).await {
        Err(ProposalRefusal::Shortfall {
            closing: short,
            missing,
        }) => {
            assert_eq!((short, missing.get()), (closing, dec!(2.5)));
        }
        other => panic!("expected the shortfall, got {other:?}"),
    }
}

/// A record of the closing's account that names the security and no transaction answers blocks
/// the proposal, naming the record; one in another account, or naming nothing of this security,
/// does not [SRV-039], [DOM-049].
#[tokio::test]
async fn pending_records_of_the_account_refuse_the_proposal() {
    let f = Fixture::open().await;
    f.buy(1, dec!(5), dec!(50.00), dec!(0.00)).await;
    let closing = f.sell(5, dec!(2), dec!(24.00), dec!(0.00)).await;
    // Pending in the other account, and pending in this one under another security only.
    f.database
        .source_records()
        .insert(
            f.other_batch,
            &SourceRecord::new(
                identify(
                    &other_account(),
                    &IdentitySource::BrokerReference("elsewhere"),
                ),
                Order::new(1),
                "raw",
                BTreeMap::from([("isin".to_owned(), isin().as_str().to_owned())]),
            ),
        )
        .await
        .expect("a record of the other account");
    f.record_with("other-security", 7, &[("isin", other_isin().as_str())])
        .await;
    assert!(propose_for(&f.database, closing).await.is_ok());

    let pending = f
        .record_with("stock-dividend", 8, &[("isin", isin().as_str())])
        .await;

    for refused in [
        propose_for(&f.database, closing).await,
        propose_next(&f.database, &account(), &isin()).await,
    ] {
        match refused {
            Err(ProposalRefusal::PendingRecords {
                account: of,
                security,
                records,
            }) => {
                assert_eq!((of, security), (account(), isin()));
                assert_eq!(records, [pending.identity().clone()]);
            }
            other => panic!("expected the pending refusal, got {other:?}"),
        }
    }
}

/// An opening, an absent id and an expiration have no proposal, each for its own reason
/// [SRV-036], [DOM-054], [DOM-092].
#[tokio::test]
async fn only_a_stored_closing_with_a_stated_quantity_is_proposed_for() {
    let f = Fixture::open().await;
    let opening = f.buy(1, dec!(5), dec!(50.00), dec!(0.00)).await;
    let record = f.record("expiry", 5).await;
    let expiration = f
        .store(
            &Expiration::new(
                Derivation::new(day(5), vec1![record]),
                money(dec!(0.00)),
                money(dec!(0.00)),
                Conversion::native(day(5)),
            )
            .into(),
        )
        .await;

    assert!(matches!(
        propose_for(&f.database, opening).await,
        Err(ProposalRefusal::NotAClosing { transaction }) if transaction == opening
    ));
    assert!(matches!(
        propose_for(&f.database, TransactionId::new(999)).await,
        Err(ProposalRefusal::Storage(
            StorageError::UnknownTransaction { .. }
        ))
    ));
    assert!(matches!(
        propose_for(&f.database, expiration).await,
        Err(ProposalRefusal::Engine {
            closing,
            source: ProposalError::QuantityNotStated,
        }) if closing == expiration
    ));
}

/// A closing already attributed, or one behind an earlier unattributed closing, is refused
/// rather than offered figures that approving could never store [DOM-066] (DEC-127,
/// provisional).
#[tokio::test]
async fn only_the_first_unattributed_closing_is_proposed_for() {
    let f = Fixture::open().await;
    let opening = f.buy(1, dec!(5), dec!(50.00), dec!(0.00)).await;
    let first = f.sell(2, dec!(1), dec!(12.00), dec!(0.00)).await;
    let second = f.sell(3, dec!(1), dec!(12.00), dec!(0.00)).await;

    assert!(matches!(
        propose_for(&f.database, second).await,
        Err(ProposalRefusal::EarlierUnattributed { closing, earlier })
            if (closing, earlier) == (second, first)
    ));

    approve(&f.database, first, &[allocate(opening, dec!(1))])
        .await
        .expect("approve the first sale");

    assert!(matches!(
        propose_for(&f.database, first).await,
        Err(ProposalRefusal::AlreadyAttributed { closing }) if closing == first
    ));
    assert_eq!(
        f.proposal(second).await.allocations()[0].allocation(),
        allocate(opening, dec!(1))
    );
}

/// "The next closing awaiting attribution" is the first unattributed closing of the account and
/// security in canonical order, the same proposal as asking for it by id; with every closing
/// attributed there is nothing to propose, and an unknown account or security is refused as
/// such [SRV-037].
#[tokio::test]
async fn the_next_closing_awaiting_attribution_is_proposed() {
    let f = Fixture::open().await;
    let opening = f.buy(1, dec!(5), dec!(50.00), dec!(0.00)).await;
    let first = f.sell(2, dec!(1), dec!(12.00), dec!(0.00)).await;
    let second = f.sell(3, dec!(2), dec!(24.00), dec!(0.00)).await;

    let next = propose_next(&f.database, &account(), &isin())
        .await
        .expect("a proposal");
    assert_eq!(next, f.proposal(first).await);

    approve(&f.database, first, &[allocate(opening, dec!(1))])
        .await
        .expect("approve the first sale");
    let next = propose_next(&f.database, &account(), &isin())
        .await
        .expect("a proposal");
    assert_eq!(next.closing().id(), second);

    approve(&f.database, second, &[allocate(opening, dec!(2))])
        .await
        .expect("approve the second sale");
    assert!(matches!(
        propose_next(&f.database, &account(), &isin()).await,
        Err(ProposalRefusal::NothingAwaiting { account: of, security })
            if (of.clone(), security.clone()) == (account(), isin())
    ));
    assert!(matches!(
        propose_next(&f.database, &other_account(), &isin()).await,
        Err(ProposalRefusal::NothingAwaiting { .. })
    ));
    assert!(matches!(
        propose_next(&f.database, &Account::new("Saxo", "absent"), &isin()).await,
        Err(ProposalRefusal::Storage(
            StorageError::UnknownAccount { .. }
        ))
    ));
    assert!(matches!(
        propose_next(&f.database, &account(), &Isin::new("US0378331005")).await,
        Err(ProposalRefusal::Storage(
            StorageError::UnknownSecurity { .. }
        ))
    ));
}
