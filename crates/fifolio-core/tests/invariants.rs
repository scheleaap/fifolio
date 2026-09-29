//! Integration layer: the lifecycle invariants storage refuses to let a caller break, against a
//! real temporary SQLite database, in process [TST-003], [TST-004].
//!
//! One test per invariant. Each drives the repository the way a service would — approve, delete —
//! and asserts both the refusal and that the state the refusal protected is untouched,
//! because a refusal that nonetheless wrote half of its change is not a refusal.
//!
//! Every test opens its own file through the shared helper. No test reaches the network.

use std::collections::BTreeMap;
use std::num::NonZeroU32;

use chrono::NaiveDate;
use fifolio_core::decimal::{Money, Quantity, QuotedPrice};
use fifolio_core::entities::{
    Account, ImportBatch, ImportCounts, Isin, Order, Quotation, RecordIdentity, Security,
    SecurityType, SourceFormat, SourceRecord,
};
use fifolio_core::identity::{IdentitySource, identify};
use fifolio_core::manual_entry::{Election, ManualEntry, Ratio, Supplied};
use fifolio_core::ordering::Leg;
use fifolio_core::storage::{
    Allocation, AttributionId, BatchId, Database, Placement, RecordHandle, StorageError,
    TransactionId,
};
use fifolio_core::transaction::DateProvenance;
use fifolio_core::transaction::{
    Buy, BuyOrigin, Derivation, Expiration, Sell, Split, Transaction, TransferIn, TransferInSource,
    TransferOut,
};
use fifolio_core::valuation::{Conversion, Valued};
use fifolio_test_support::TempDb;
use rust_decimal_macros::dec;
use vec1::vec1;

fn account() -> Account {
    Account::new("Saxo", "69900/1000000")
}

/// A second account holding the same security, to check that an invariant keyed on the pair does
/// not block across it either [DOM-066], [DOM-068].
fn other_account() -> Account {
    Account::new("Saxo", "69900/2000000")
}

fn isin() -> Isin {
    Isin::new("NL0000009538")
}

/// A second security in the same account, to check that an invariant keyed on the pair does not
/// block across it [DOM-066].
fn other_isin() -> Isin {
    Isin::new("NL0011821202")
}

fn day(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2024, 5, day).expect("a valid date")
}

/// A date before every `day`, for the transaction written last and dated first: the 2024 export
/// imported after the 2025 one, where row-id order and trade-date order disagree [DOM-066],
/// [DOM-068].
fn day_before_all() -> NaiveDate {
    NaiveDate::from_ymd_opt(2024, 4, 30).expect("a valid date")
}

fn cite(reference: &str) -> RecordIdentity {
    identify(&account(), &IdentitySource::BrokerReference(reference))
}

fn money(amount: rust_decimal::Decimal) -> Valued<Money> {
    Valued::new(Money::new(amount), Money::new(amount))
}

fn price(amount: rust_decimal::Decimal) -> Valued<QuotedPrice> {
    Valued::new(QuotedPrice::new(amount), QuotedPrice::new(amount))
}

fn conversion() -> Conversion {
    Conversion::native(day(2))
}

/// Each `_at` constructor takes the conversion; the unsuffixed helpers fix it to the native one.
fn buy_at(on: NaiveDate, record: RecordHandle, conversion: Conversion) -> Transaction {
    Buy::new(
        Derivation::new(on, vec1![record]),
        Quantity::new(dec!(100.00000000)),
        price(dec!(10.000000)),
        money(dec!(1000.00)),
        money(dec!(8.00)),
        BuyOrigin::Purchase,
        conversion,
    )
    .into()
}

fn buy(on: NaiveDate, record: RecordHandle) -> Transaction {
    buy_at(on, record, conversion())
}

fn sell_at(on: NaiveDate, record: RecordHandle, conversion: Conversion) -> Transaction {
    Sell::new(
        Derivation::new(on, vec1![record]),
        Quantity::new(dec!(10.00000000)),
        price(dec!(12.000000)),
        money(dec!(120.00)),
        money(dec!(8.00)),
        conversion,
    )
    .into()
}

fn sell(on: NaiveDate, record: RecordHandle) -> Transaction {
    sell_at(on, record, conversion())
}

fn transfer_out_at(on: NaiveDate, record: RecordHandle, conversion: Conversion) -> Transaction {
    TransferOut::new(
        Derivation::new(on, vec1![record]),
        Quantity::new(dec!(10.00000000)),
        money(dec!(0.00)),
        Ratio::new(NonZeroU32::MIN, NonZeroU32::MIN),
        conversion,
        other_isin(),
    )
    .into()
}

fn transfer_out(on: NaiveDate, record: RecordHandle) -> Transaction {
    transfer_out_at(on, record, conversion())
}

fn expiration_at(on: NaiveDate, record: RecordHandle, conversion: Conversion) -> Transaction {
    Expiration::new(
        Derivation::new(on, vec1![record]),
        money(dec!(0.00)),
        money(dec!(0.00)),
        conversion,
    )
    .into()
}

fn expiration(on: NaiveDate, record: RecordHandle) -> Transaction {
    expiration_at(on, record, conversion())
}

fn transfer_in_at(on: NaiveDate, record: RecordHandle, conversion: Conversion) -> Transaction {
    TransferIn::new(
        Derivation::new(on, vec1![record]),
        Quantity::new(dec!(10.00000000)),
        money(dec!(100.00)),
        money(dec!(0.00)),
        on,
        DateProvenance::Inherited,
        TransferInSource::Broker,
        conversion,
    )
    .into()
}

/// A split carries no money and so no conversion [DOM-105].
fn split(on: NaiveDate, record: RecordHandle) -> Transaction {
    Split::new(
        Derivation::new(on, vec1![record]),
        Ratio::new(
            NonZeroU32::new(2).expect("a non-zero numerator"),
            NonZeroU32::MIN,
        ),
    )
    .into()
}

/// The `transfer_in` a `transfer_out` emits on approval: derived from no row of its own
/// [DOM-090], it cites the records of the `transfer_out` that emitted it [DEC-079].
fn emitted_transfer_in(on: NaiveDate, emitter: RecordHandle) -> Transaction {
    TransferIn::new(
        Derivation::new(on, vec1![emitter]),
        Quantity::new(dec!(10.00000000)),
        money(dec!(100.00)),
        money(dec!(0.00)),
        on,
        DateProvenance::Inherited,
        TransferInSource::CorporateAction,
        conversion(),
    )
    .into()
}

fn record(reference: &str) -> SourceRecord {
    record_at(reference, 1)
}

/// A record at `order` in the file it was read from [DOM-040].
fn record_at(reference: &str, order: u32) -> SourceRecord {
    SourceRecord::new(
        cite(reference),
        Order::new(order),
        "\"2024-05-02\",\"BUY\"",
        BTreeMap::new(),
    )
}

/// Stores the record `reference` names in `batch` and hands back storage's handle on it, which is
/// the only thing a transaction is derived from [DOM-047].
async fn stored_record(database: &Database, batch: BatchId, reference: &str) -> RecordHandle {
    database
        .source_records()
        .insert(batch, &record(reference))
        .await
        .expect("the record a transaction is derived from")
}

/// As [`stored_record`], at `order` in its file.
async fn stored_record_at(
    database: &Database,
    batch: BatchId,
    reference: &str,
    order: u32,
) -> RecordHandle {
    database
        .source_records()
        .insert(batch, &record_at(reference, order))
        .await
        .expect("the record a transaction is derived from")
}

fn import(filename: &str) -> ImportBatch {
    ImportBatch::new(
        account(),
        filename,
        SourceFormat::SaxoNlXlsx,
        "2024-05-03T09:00:00Z".parse().expect("a valid timestamp"),
        ImportCounts {
            derived: 1,
            pending: 0,
            non_position: 0,
        },
    )
}

/// A database holding both accounts, both securities and one import, which is the least a placed
/// transaction needs.
async fn open() -> (TempDb, Database, BatchId) {
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
        .insert(&import("2024.xlsx"))
        .await
        .expect("the import");

    (db, database, batch)
}

async fn store(
    database: &Database,
    batch: BatchId,
    security: Isin,
    transaction: &Transaction,
) -> TransactionId {
    store_in(database, batch, account(), security, transaction).await
}

async fn store_in(
    database: &Database,
    batch: BatchId,
    account: Account,
    security: Isin,
    transaction: &Transaction,
) -> TransactionId {
    database
        .transactions()
        .insert(&Placement::derived(account, security, batch), transaction)
        .await
        .expect("store a transaction")
}

/// Approves `closing`, expecting the DOM-066 refusal, and hands back the closing it named.
async fn blocked_by(
    database: &Database,
    closing: TransactionId,
    allocations: &[Allocation],
) -> TransactionId {
    match database.attributions().approve(closing, allocations).await {
        Err(StorageError::EarlierClosingUnattributed {
            closing: refused,
            earlier,
        }) => {
            assert_eq!(refused, closing, "the refusal names the closing it refused");
            earlier
        }
        other => panic!("the later closing must be refused, got {other:?}"),
    }
}

/// Closings are attributed in canonical order per account and security: the later closing is
/// refused while an earlier one is unattributed, whichever of the three closing kinds that
/// earlier one is [DOM-066], [DOM-081], [TST-004].
///
/// The closings of another account and of another security are what make the transaction's
/// stored relations to account and security the ones read [DOM-013].
#[tokio::test]
async fn a_later_closing_is_refused_while_an_earlier_one_is_unattributed() {
    let (_db, database, batch) = open().await;
    // Earliest of all, and in the other account: if the comparison stopped keying on the account
    // this would be the closing every refusal below named.
    let across_accounts = store_in(
        &database,
        batch,
        other_account(),
        isin(),
        &sell(day(1), stored_record(&database, batch, "s0").await),
    )
    .await;
    let opening = store(
        &database,
        batch,
        isin(),
        &buy(day(1), stored_record(&database, batch, "b1").await),
    )
    .await;
    // One of each closing kind, so that narrowing the kinds the order is taken over is visible.
    let expiry = store(
        &database,
        batch,
        isin(),
        &expiration(day(2), stored_record(&database, batch, "e1").await),
    )
    .await;
    let out = store(
        &database,
        batch,
        isin(),
        &transfer_out(day(3), stored_record(&database, batch, "t1").await),
    )
    .await;
    let earlier = store(
        &database,
        batch,
        isin(),
        &sell(day(4), stored_record(&database, batch, "s1").await),
    )
    .await;
    let later = store(
        &database,
        batch,
        isin(),
        &sell(day(5), stored_record(&database, batch, "s2").await),
    )
    .await;
    // Written after `later` and dated before everything: the order is the trade date's, not the
    // one the rows happened to be written in, so this is the first closing named below.
    let earliest = store(
        &database,
        batch,
        isin(),
        &sell(
            day_before_all(),
            stored_record(&database, batch, "s4").await,
        ),
    )
    .await;
    // Same account, different security: FIFO never crosses the pair, so this must not block.
    let elsewhere = store(
        &database,
        batch,
        other_isin(),
        &sell(day(2), stored_record(&database, batch, "s3").await),
    )
    .await;

    let allocation = [Allocation::new(opening, Quantity::new(dec!(10.00000000)))];

    database
        .attributions()
        .approve(elsewhere, &allocation)
        .await
        .expect("another security is not in the same order");

    // Each earlier closing in turn is the one named, and approving it lifts the block, which is
    // what DOM-055 relies on.
    for blocking in [earliest, expiry, out, earlier] {
        assert_eq!(
            blocked_by(&database, later, &allocation).await,
            blocking,
            "the earliest unattributed closing is named"
        );
        database
            .attributions()
            .approve(blocking, &allocation)
            .await
            .expect("the earliest unattributed closing");
    }

    database
        .attributions()
        .approve(later, &allocation)
        .await
        .expect("the block is lifted by approving every earlier closing");
    // It was unattributed throughout the approvals above and blocked none of them; it is still
    // attributable now, so nothing about it was consumed by them either.
    database
        .attributions()
        .approve(across_accounts, &allocation)
        .await
        .expect("another account is not in the same order");
}

/// Two closings on one trade date are ordered by the `order` of the records they were derived
/// from, not by the order they were written in [DOM-066], [DOM-111], [TST-004].
///
/// Two sells on one day is the ordinary case. The one written first carries the higher `order`,
/// so a comparison that fell back on the row id would name the wrong closing.
#[tokio::test]
async fn closings_on_one_trade_date_are_ordered_by_their_records_order() {
    let (_db, database, batch) = open().await;
    let opening = store(
        &database,
        batch,
        isin(),
        &buy(day(1), stored_record_at(&database, batch, "b1", 0).await),
    )
    .await;
    let second = store(
        &database,
        batch,
        isin(),
        &sell(day(2), stored_record_at(&database, batch, "s2", 5).await),
    )
    .await;
    let first = store(
        &database,
        batch,
        isin(),
        &sell(day(2), stored_record_at(&database, batch, "s1", 4).await),
    )
    .await;
    let allocation = [Allocation::new(opening, Quantity::new(dec!(10.00000000)))];

    assert_eq!(
        blocked_by(&database, second, &allocation).await,
        first,
        "the closing whose record comes first in the file is the earlier one"
    );

    database
        .attributions()
        .approve(first, &allocation)
        .await
        .expect("the earlier of the two");
    database
        .attributions()
        .approve(second, &allocation)
        .await
        .expect("the block is lifted");
}

/// Two closings of one trade date and one `order`, from different files, are ordered by the age
/// of the batch that first supplied each record: the older batch first [DOM-066], [DOM-111],
/// [TST-004].
///
/// The newer batch's closing is written first, so the row id disagrees with the batch age.
#[tokio::test]
async fn closings_of_one_date_and_order_from_two_files_are_ordered_by_batch_age() {
    let (_db, database, older) = open().await;
    let newer = database
        .import_batches()
        .insert(&import("2024-q2.xlsx"))
        .await
        .expect("the newer import");
    let opening = store(
        &database,
        older,
        isin(),
        &buy(day(1), stored_record_at(&database, older, "b1", 0).await),
    )
    .await;
    let from_newer = store(
        &database,
        newer,
        isin(),
        &sell(day(2), stored_record_at(&database, newer, "s2", 3).await),
    )
    .await;
    let from_older = store(
        &database,
        older,
        isin(),
        &sell(day(2), stored_record_at(&database, older, "s1", 3).await),
    )
    .await;
    let allocation = [Allocation::new(opening, Quantity::new(dec!(10.00000000)))];

    assert_eq!(
        blocked_by(&database, from_newer, &allocation).await,
        from_older,
        "the older batch's closing is the earlier one"
    );
}

/// Approves `earlier` then `later`, and expects deleting the first attribution to be refused
/// naming the second, which is the DOM-068 half of the order the DOM-066 refusal reads.
async fn deletion_is_refused_in_order(
    database: &Database,
    earlier: TransactionId,
    later: TransactionId,
    allocation: &[Allocation],
) {
    let first = database
        .attributions()
        .approve(earlier, allocation)
        .await
        .expect("the earlier closing");
    let second = database
        .attributions()
        .approve(later, allocation)
        .await
        .expect("the later closing");
    match database.attributions().delete(first).await {
        Err(StorageError::LaterAttributionExists {
            later: blocking, ..
        }) => assert_eq!(blocking, second, "the later attribution is named"),
        other => panic!("deleting the earlier attribution must be refused, got {other:?}"),
    }
}

/// On one trade date the record's `order` outranks the batch age: a newer file's earlier row
/// comes before an older file's later row [DOM-111], [DOM-066], [DOM-068], [TST-004].
///
/// Keying on the batch age before the order would name the older batch's closing; the lower
/// order is also written last, so the row id disagrees too.
#[tokio::test]
async fn on_one_trade_date_the_order_outranks_the_batch_age() {
    let (_db, database, older) = open().await;
    let newer = database
        .import_batches()
        .insert(&import("2024-q2.xlsx"))
        .await
        .expect("the newer import");
    let opening = store(
        &database,
        older,
        isin(),
        &buy(day(1), stored_record_at(&database, older, "b1", 0).await),
    )
    .await;
    let from_older = store(
        &database,
        older,
        isin(),
        &sell(day(2), stored_record_at(&database, older, "s1", 5).await),
    )
    .await;
    let from_newer = store(
        &database,
        newer,
        isin(),
        &sell(day(2), stored_record_at(&database, newer, "s2", 2).await),
    )
    .await;
    let allocation = [Allocation::new(opening, Quantity::new(dec!(10.00000000)))];

    assert_eq!(
        blocked_by(&database, from_older, &allocation).await,
        from_newer,
        "the lower order is the earlier closing, whatever its batch"
    );
    deletion_is_refused_in_order(&database, from_newer, from_older, &allocation).await;
}

/// The trade date outranks the record's `order`: an earlier date's late row comes before a later
/// date's early row [DOM-111], [DOM-066], [DOM-068], [TST-004].
///
/// Keying on the order before the date would name the later date's closing; the earlier date is
/// also written last, so the row id disagrees too.
#[tokio::test]
async fn the_trade_date_outranks_the_order() {
    let (_db, database, batch) = open().await;
    let opening = store(
        &database,
        batch,
        isin(),
        &buy(day(1), stored_record_at(&database, batch, "b1", 0).await),
    )
    .await;
    let later_date = store(
        &database,
        batch,
        isin(),
        &sell(day(3), stored_record_at(&database, batch, "s2", 1).await),
    )
    .await;
    let earlier_date = store(
        &database,
        batch,
        isin(),
        &sell(day(2), stored_record_at(&database, batch, "s1", 9).await),
    )
    .await;
    let allocation = [Allocation::new(opening, Quantity::new(dec!(10.00000000)))];

    assert_eq!(
        blocked_by(&database, later_date, &allocation).await,
        earlier_date,
        "the earlier trade date is the earlier closing, whatever its order"
    );
    deletion_is_refused_in_order(&database, earlier_date, later_date, &allocation).await;
}

/// Re-importing a year into a newer batch leaves every record's canonical order unchanged:
/// ownership moves to the newest supplier [SRV-052], the batch age the order reads is the oldest
/// supplier's and does not move (DEC-092) [DOM-111], [DOM-066], [DOM-068], [TST-004].
///
/// The move is made in SQL, exactly as FIF-071 will make it, since no repository moves ownership
/// yet. Transactions store their position when written, so the stored key and the attribution
/// order hold whether or not the record's age moves; what would flip, were the age read from the
/// owner, is the reread handle and the closing a re-import derives from it, which would sort
/// after the one the middle batch supplied.
#[tokio::test]
async fn re_importing_a_year_leaves_the_canonical_order_unchanged() {
    let (db, database, first) = open().await;
    let middle = database
        .import_batches()
        .insert(&import("2024-q2.xlsx"))
        .await
        .expect("the middle import");
    let opening = store(
        &database,
        first,
        isin(),
        &buy(day(1), stored_record_at(&database, first, "b1", 0).await),
    )
    .await;
    let re_imported_handle = stored_record_at(&database, first, "s1", 3).await;
    let re_imported = store(
        &database,
        first,
        isin(),
        &sell(day(2), re_imported_handle.clone()),
    )
    .await;
    let untouched = store(
        &database,
        middle,
        isin(),
        &sell(day(2), stored_record_at(&database, middle, "s2", 3).await),
    )
    .await;
    let allocation = [Allocation::new(opening, Quantity::new(dec!(10.00000000)))];
    let earlier = database
        .attributions()
        .approve(re_imported, &allocation)
        .await
        .expect("the earlier closing");
    let later = database
        .attributions()
        .approve(untouched, &allocation)
        .await
        .expect("the later closing");

    let newest = database
        .import_batches()
        .insert(&import("2024.xlsx"))
        .await
        .expect("the re-import");
    let pool = sqlx::SqlitePool::connect(&format!("sqlite:{}", db.path().display()))
        .await
        .expect("open the database file directly");
    sqlx::query("update source_record set batch_id = ? where batch_id = ?")
        .bind(newest.get())
        .bind(first.get())
        .execute(&pool)
        .await
        .expect("move ownership to the re-import");
    pool.close().await;

    let reread = database
        .source_records()
        .handle(re_imported_handle.identity())
        .await
        .expect("read the handle")
        .expect("the record is still stored");
    assert_eq!(
        reread.position(),
        re_imported_handle.position(),
        "the record's position survives the ownership move"
    );
    let untouched_key = database
        .transactions()
        .find(untouched)
        .await
        .expect("read back")
        .expect("the middle batch's closing is stored")
        .order_key();
    assert!(
        sell(day(2), reread).order_key() < untouched_key,
        "a closing the re-import derives from the reread record still sorts first"
    );
    let stored = database
        .transactions()
        .find(re_imported)
        .await
        .expect("read back")
        .expect("the transaction is still stored");
    assert_eq!(
        stored.order_key().position(),
        re_imported_handle.position(),
        "the transaction keeps the position it was derived at"
    );
    match database.attributions().delete(earlier).await {
        Err(StorageError::LaterAttributionExists {
            later: blocking, ..
        }) => assert_eq!(blocking, later, "the order between the two closings stands"),
        other => panic!("deleting out of order must still be refused, got {other:?}"),
    }
}

/// A decomposition's `transfer_out` sorts immediately after its `sell`: both derived from the same
/// record, the trailing leg is later than the sell and earlier than the next record of the day
/// (DEC-090) [DOM-011], [DOM-066], [TST-004].
///
/// The legs are written in reverse, and the next record's closing before both, so neither the row
/// id nor the kind can be what orders them. Deleting the sell leg's attribution is then refused
/// while the `transfer_out` leg's stands, which is the same order read by DOM-068's query.
#[tokio::test]
async fn a_trailing_leg_sorts_immediately_after_its_sell() {
    let (_db, database, batch) = open().await;
    let opening = store(
        &database,
        batch,
        isin(),
        &buy(day(1), stored_record_at(&database, batch, "b1", 0).await),
    )
    .await;
    let merger_row = stored_record_at(&database, batch, "m1", 2).await;
    let next = store(
        &database,
        batch,
        isin(),
        &sell(day(2), stored_record_at(&database, batch, "s2", 3).await),
    )
    .await;
    let transfer_leg: Transaction = TransferOut::new(
        Derivation::new(day(2), vec1![merger_row.clone()]).trailing(),
        Quantity::new(dec!(10.00000000)),
        money(dec!(0.00)),
        Ratio::new(NonZeroU32::MIN, NonZeroU32::MIN),
        conversion(),
        other_isin(),
    )
    .into();
    let transfer_leg = store(&database, batch, isin(), &transfer_leg).await;
    let sell_leg = store(&database, batch, isin(), &sell(day(2), merger_row)).await;
    let allocation = [Allocation::new(opening, Quantity::new(dec!(10.00000000)))];

    assert_eq!(
        database
            .transactions()
            .find(transfer_leg)
            .await
            .expect("read back")
            .expect("stored")
            .order_key()
            .leg(),
        Leg::Trailing,
        "the leg round-trips"
    );
    assert_eq!(blocked_by(&database, next, &allocation).await, sell_leg);
    assert_eq!(
        blocked_by(&database, transfer_leg, &allocation).await,
        sell_leg,
        "the sell comes before the transfer_out it shares its record with"
    );
    let sell_attribution = database
        .attributions()
        .approve(sell_leg, &allocation)
        .await
        .expect("the sell leg");
    assert_eq!(
        blocked_by(&database, next, &allocation).await,
        transfer_leg,
        "the transfer_out comes before the next record of the day"
    );

    // [DOM-068]: the transfer_out leg has the lower row id, so only its leg makes it later.
    let transfer_attribution = database
        .attributions()
        .approve(transfer_leg, &allocation)
        .await
        .expect("the transfer_out leg");
    match database.attributions().delete(sell_attribution).await {
        Err(StorageError::LaterAttributionExists { attribution, later }) => {
            assert_eq!(attribution, sell_attribution);
            assert_eq!(
                later, transfer_attribution,
                "the trailing leg is the later one"
            );
        }
        other => panic!("deleting the sell leg's attribution must be refused, got {other:?}"),
    }
}

/// What was approved is what is stored: the allocations read back carrying their own openings and
/// their own quantities, in the order they were approved in [DOM-054], [DOM-058], [TST-004].
///
/// The two quantities differ from each other and neither is the closing's own, so a constant bound
/// in place of the caller's quantity, a dropped allocation or a lost ordinal each read back as
/// something other than what went in. These rows are what FIF-014 derives every monetary figure
/// from, so a silent zero here is a silent zero in a tax figure.
#[tokio::test]
async fn an_approved_attribution_reads_back_with_its_allocations() {
    let (_db, database, batch) = open().await;
    let first_opening = store(
        &database,
        batch,
        isin(),
        &buy(day(1), stored_record(&database, batch, "b1").await),
    )
    .await;
    let second_opening = store(
        &database,
        batch,
        isin(),
        &buy(day(2), stored_record(&database, batch, "b2").await),
    )
    .await;
    let closing = store(
        &database,
        batch,
        isin(),
        &sell(day(3), stored_record(&database, batch, "s1").await),
    )
    .await;

    // The later opening is allocated first, so the stored order is the approved one rather than
    // an id order the query might fall back on.
    let allocations = [
        Allocation::new(second_opening, Quantity::new(dec!(7.50000000))),
        Allocation::new(first_opening, Quantity::new(dec!(2.50000000))),
    ];
    let attribution = database
        .attributions()
        .approve(closing, &allocations)
        .await
        .expect("approve");

    let stored = database
        .attributions()
        .find(attribution)
        .await
        .expect("read back")
        .expect("the attribution just approved");
    assert_eq!(stored.closing(), closing);
    assert_eq!(
        stored.allocations(),
        allocations.as_slice(),
        "every allocation reads back as approved, in the order it was approved in"
    );
}

/// An attribution approved with no allocations at all is stored, with no allocation rows
/// [DOM-054], [DOM-058], [TST-004].
///
/// This item's invariants are silent on the empty set: what would forbid it is DOM-065, that a
/// closing's allocations sum exactly to its quantity, which is FIF-078's and blocked. Until then
/// an empty attribution counts as attributed and unblocks later closings under DOM-066, so the
/// behavior is pinned here and a change to it is a change to this test.
#[tokio::test]
async fn an_attribution_approved_with_no_allocations_is_stored_empty() {
    let (_db, database, batch) = open().await;
    let closing = store(
        &database,
        batch,
        isin(),
        &sell(day(2), stored_record(&database, batch, "s1").await),
    )
    .await;

    let attribution = database
        .attributions()
        .approve(closing, &[])
        .await
        .expect("nothing in this item refuses an empty allocation set");

    let stored = database
        .attributions()
        .find(attribution)
        .await
        .expect("read back")
        .expect("the attribution just approved");
    assert_eq!(stored.closing(), closing);
    assert!(
        stored.allocations().is_empty(),
        "an empty approval stored no allocation rows"
    );

    // The closing counts as attributed, which is the consequence DOM-065 will have to answer for.
    let later = store(
        &database,
        batch,
        isin(),
        &sell(day(3), stored_record(&database, batch, "s2").await),
    )
    .await;
    database
        .attributions()
        .approve(later, &[])
        .await
        .expect("the empty attribution unblocks the later closing under DOM-066");
}

/// An allocation quantity carrying more decimals than a quantity holds is refused rather than
/// truncated, and the refusal leaves no part of the attribution stored [ARC-010], [DOM-058],
/// [TST-004].
#[tokio::test]
async fn an_allocation_quantity_not_at_its_scale_is_refused() {
    let (_db, database, batch) = open().await;
    let first_opening = store(
        &database,
        batch,
        isin(),
        &buy(day(1), stored_record(&database, batch, "b1").await),
    )
    .await;
    let second_opening = store(
        &database,
        batch,
        isin(),
        &buy(day(1), stored_record(&database, batch, "b2").await),
    )
    .await;
    let closing = store(
        &database,
        batch,
        isin(),
        &sell(day(2), stored_record(&database, batch, "s1").await),
    )
    .await;

    // The unscaled quantity is the second of the two: a check made per insert rather than up
    // front would already have written the first allocation by the time it refused.
    match database
        .attributions()
        .approve(
            closing,
            &[
                Allocation::new(first_opening, Quantity::new(dec!(5.00000000))),
                Allocation::new(second_opening, Quantity::new(dec!(5.000000005))),
            ],
        )
        .await
    {
        Err(StorageError::UnscaledValue {
            field,
            value,
            scale,
        }) => {
            assert_eq!(field, "quantity");
            assert_eq!(value, "5.000000005");
            assert_eq!(scale, 8);
        }
        other => panic!("an unscaled allocation quantity must be refused, got {other:?}"),
    }

    // No attribution exists at all: the refused approval would have taken the first rowid.
    assert_eq!(
        database
            .attributions()
            .find(AttributionId::new(1))
            .await
            .expect("read back"),
        None,
        "the refusal stored neither the attribution nor its first allocation"
    );
}

/// Both attribution invariants are stated over closings: an opening cannot be attributed, and a
/// closing is attributed once [DOM-066], [DOM-081], [TST-004].
#[tokio::test]
async fn only_a_closing_is_attributed_and_only_once() {
    let (_db, database, batch) = open().await;
    let opening = store(
        &database,
        batch,
        isin(),
        &buy(day(1), stored_record(&database, batch, "b1").await),
    )
    .await;
    let allocation = [Allocation::new(opening, Quantity::new(dec!(10.00000000)))];

    let openings = [
        (opening, "buy"),
        (
            store(
                &database,
                batch,
                isin(),
                &transfer_in_at(
                    day(1),
                    stored_record(&database, batch, "i1").await,
                    conversion(),
                ),
            )
            .await,
            "transfer_in",
        ),
        (
            store(
                &database,
                batch,
                isin(),
                &split(day(1), stored_record(&database, batch, "p1").await),
            )
            .await,
            "split",
        ),
    ];
    for (not_a_closing, expected_kind) in openings {
        match database
            .attributions()
            .approve(not_a_closing, &allocation)
            .await
        {
            Err(StorageError::NotAClosing { transaction, kind }) => {
                assert_eq!(transaction, not_a_closing);
                assert_eq!(kind, expected_kind, "the refusal names the kind it met");
            }
            other => panic!("attributing a {expected_kind} must be refused, got {other:?}"),
        }
    }

    let closing = store(
        &database,
        batch,
        isin(),
        &sell(day(2), stored_record(&database, batch, "s1").await),
    )
    .await;
    let attribution = database
        .attributions()
        .approve(closing, &allocation)
        .await
        .expect("the closing");
    match database.attributions().approve(closing, &allocation).await {
        Err(StorageError::ClosingAlreadyAttributed {
            closing: refused,
            attribution: named,
        }) => {
            assert_eq!(refused, closing);
            assert_eq!(
                named, attribution,
                "the attribution it already has is named"
            );
        }
        other => panic!("a second approval of the same closing must be refused, got {other:?}"),
    }
}

/// An attribution may only be deleted if no later attribution exists for the same account and
/// security [DOM-068], [TST-004].
#[tokio::test]
async fn an_attribution_with_a_later_one_is_not_deleted() {
    let (_db, database, batch) = open().await;
    let opening = store(
        &database,
        batch,
        isin(),
        &buy(day(1), stored_record(&database, batch, "b1").await),
    )
    .await;
    let allocation = [Allocation::new(opening, Quantity::new(dec!(10.00000000)))];

    // The later-dated closing is written first, so row-id order and trade-date order disagree:
    // the order the deletion is refused in is the trade date's, not the one they were written in.
    let later_closing = store(
        &database,
        batch,
        isin(),
        &sell(day(3), stored_record(&database, batch, "s2").await),
    )
    .await;
    let earlier_closing = store(
        &database,
        batch,
        isin(),
        &sell(day(2), stored_record(&database, batch, "s1").await),
    )
    .await;
    let earlier = database
        .attributions()
        .approve(earlier_closing, &allocation)
        .await
        .expect("approve the earlier");
    let later = database
        .attributions()
        .approve(later_closing, &allocation)
        .await
        .expect("approve the later");

    match database.attributions().delete(earlier).await {
        Err(StorageError::LaterAttributionExists {
            attribution,
            later: blocking,
        }) => {
            assert_eq!(attribution, earlier);
            assert_eq!(blocking, later, "the blocking attribution is named");
        }
        other => panic!("deleting out of order must be refused, got {other:?}"),
    }
    assert!(
        database
            .attributions()
            .find(earlier)
            .await
            .expect("read back")
            .is_some(),
        "a refused deletion leaves the attribution standing"
    );

    database
        .attributions()
        .delete(later)
        .await
        .expect("the latest attribution");
    database
        .attributions()
        .delete(earlier)
        .await
        .expect("the block is lifted by deleting the later one");
    assert_eq!(
        database
            .attributions()
            .find(earlier)
            .await
            .expect("read back"),
        None
    );
}

/// Two attributions whose closings share a trade date are ordered by the `order` of the records
/// the closings were derived from [DOM-068], [DOM-111], [TST-004].
///
/// The second closing is written first, so a comparison falling back on the row id would let the
/// first attribution be deleted.
#[tokio::test]
async fn attributions_of_one_trade_date_are_ordered_by_their_records_order() {
    let (_db, database, batch) = open().await;
    let opening = store(
        &database,
        batch,
        isin(),
        &buy(day(1), stored_record(&database, batch, "b1").await),
    )
    .await;
    let allocation = [Allocation::new(opening, Quantity::new(dec!(10.00000000)))];

    let second_closing = store(
        &database,
        batch,
        isin(),
        &sell(day(2), stored_record_at(&database, batch, "s2", 6).await),
    )
    .await;
    let first_closing = store(
        &database,
        batch,
        isin(),
        &sell(day(2), stored_record_at(&database, batch, "s1", 5).await),
    )
    .await;
    let first = database
        .attributions()
        .approve(first_closing, &allocation)
        .await
        .expect("approve the first of the day");
    let second = database
        .attributions()
        .approve(second_closing, &allocation)
        .await
        .expect("approve the second of the day");
    // A later attribution in another account, and one on another security in this account: the
    // key is the pair, so neither half of it may be dropped and neither blocks here.
    let across_accounts = store_in(
        &database,
        batch,
        other_account(),
        isin(),
        &sell(day(3), stored_record(&database, batch, "s3").await),
    )
    .await;
    let across_securities = store(
        &database,
        batch,
        other_isin(),
        &sell(day(3), stored_record(&database, batch, "s4").await),
    )
    .await;
    for elsewhere in [across_accounts, across_securities] {
        database
            .attributions()
            .approve(elsewhere, &allocation)
            .await
            .expect("approve a closing outside the pair");
    }

    match database.attributions().delete(first).await {
        Err(StorageError::LaterAttributionExists {
            attribution,
            later: blocking,
        }) => {
            assert_eq!(attribution, first);
            assert_eq!(
                blocking, second,
                "the later attribution of that day is named"
            );
        }
        other => panic!("deleting out of order must be refused, got {other:?}"),
    }

    database
        .attributions()
        .delete(second)
        .await
        .expect("the latest attribution");
    database
        .attributions()
        .delete(first)
        .await
        .expect("the block is lifted, the later attributions outside the pair notwithstanding");
}

/// Deleting an attribution or a transaction nothing is stored under is already true [DOM-068],
/// [DOM-069], [TST-004].
///
/// A deletion states an end state that already holds, so it is idempotent on both repositories.
/// The specification does not decide this; it is pinned here so that a change is a change to a
/// test.
#[tokio::test]
async fn an_unknown_attribution_and_an_unknown_transaction() {
    let (_db, database, _batch) = open().await;

    database
        .attributions()
        .delete(AttributionId::new(404))
        .await
        .expect("deleting what is not there is already true");

    let unknown = TransactionId::new(404);
    database
        .transactions()
        .delete(unknown)
        .await
        .expect("deleting what is not there is already true");
}

/// A transaction that participates in an attribution is immutable: it cannot be deleted while
/// that attribution exists [DOM-069], [TST-004].
///
/// DOM-069 forbids three operations — edited, re-rated, deleted. Only deletion is asserted here:
/// no operation edits or re-rates a stored transaction (DEC-084), so those clauses hold by
/// construction and have nothing to refuse. The edit clause is carried on FIF-038 in the plan,
/// which is where an edit surface would appear, rather than only here.
#[tokio::test]
async fn an_attributed_transaction_is_not_deleted() {
    let (_db, database, batch) = open().await;
    let b1 = stored_record(&database, batch, "b1").await;
    let opening = store(&database, batch, isin(), &buy(day(1), b1.clone())).await;
    let closing = store(
        &database,
        batch,
        isin(),
        &sell(day(2), stored_record(&database, batch, "s1").await),
    )
    .await;
    let attribution = database
        .attributions()
        .approve(
            closing,
            &[Allocation::new(opening, Quantity::new(dec!(10.00000000)))],
        )
        .await
        .expect("approve");

    // Both sides participate: the closing the attribution is of, and the opening it allocates
    // against.
    for participant in [opening, closing] {
        match database.transactions().delete(participant).await {
            Err(StorageError::TransactionAttributed {
                transaction,
                attribution: named,
            }) => {
                assert_eq!(transaction, participant);
                assert_eq!(
                    named, attribution,
                    "the attribution to delete first is named"
                );
            }
            other => panic!("deleting an attributed transaction must be refused, got {other:?}"),
        }
    }

    assert_eq!(
        database
            .transactions()
            .find(opening)
            .await
            .expect("read back"),
        Some(buy(day(1), b1)),
        "a refused deletion leaves the transaction as it was"
    );

    // Deleting the attribution first is what the rule tells the user to do.
    database
        .attributions()
        .delete(attribution)
        .await
        .expect("delete the attribution");
    database
        .transactions()
        .delete(closing)
        .await
        .expect("delete once nothing is attributed");
}

/// A `transfer_in` emitted by a `transfer_out` may not be deleted independently of it, and an
/// attributed emitted record freezes the `transfer_out` that emitted it [DOM-094], [DOM-069],
/// [TST-004].
#[tokio::test]
async fn an_emitted_transfer_in_is_not_deleted_independently_of_its_transfer_out() {
    let (_db, database, batch) = open().await;
    let t1 = stored_record(&database, batch, "t1").await;
    let out = store(&database, batch, isin(), &transfer_out(day(2), t1.clone())).await;
    // DOM-090 emits one transfer_in per consumed parcel, so two is the ordinary shape.
    let mut emitted = Vec::new();
    for _ in 0..2 {
        let record = database
            .transactions()
            .insert(
                &Placement::emitted(account(), isin()),
                &emitted_transfer_in(day(1), t1.clone()),
            )
            .await
            .expect("the emitted record");
        database
            .transactions()
            .record_emission(out, record)
            .await
            .expect("record the emission");
        emitted.push(record);
    }

    for &record in &emitted {
        match database.transactions().delete(record).await {
            Err(StorageError::EmittedTransferIn {
                transfer_in,
                transfer_out: emitter,
            }) => {
                assert_eq!(transfer_in, record);
                assert_eq!(emitter, out, "the transfer_out that emitted it is named");
            }
            other => panic!("deleting an emitted transfer_in alone must be refused, got {other:?}"),
        }
        assert!(
            database
                .transactions()
                .find(record)
                .await
                .expect("read back")
                .is_some(),
            "the refused deletion leaves the emitted record standing"
        );
    }

    // An emitted record is an opening like any other, and while it is attributed the transfer_out
    // cannot take it down with it [DOM-069]. The closing is dated before the transfer_out, so
    // DOM-066 does not stand in the way of approving it.
    let closing = store(
        &database,
        batch,
        isin(),
        &sell(day(1), stored_record(&database, batch, "s1").await),
    )
    .await;
    let attribution = database
        .attributions()
        .approve(
            closing,
            &[Allocation::new(
                emitted[0],
                Quantity::new(dec!(10.00000000)),
            )],
        )
        .await
        .expect("attribute the emitted record as an opening");
    match database.transactions().delete(out).await {
        Err(StorageError::TransactionAttributed {
            transaction,
            attribution: named,
        }) => {
            assert_eq!(
                transaction, emitted[0],
                "the attributed member of the group is named, not the transfer_out"
            );
            assert_eq!(named, attribution);
        }
        other => panic!(
            "deleting a transfer_out whose emission is attributed must be refused, got {other:?}"
        ),
    }
    assert!(
        database
            .transactions()
            .find(out)
            .await
            .expect("read back")
            .is_some(),
        "the refused deletion leaves the whole group standing"
    );
    database
        .attributions()
        .delete(attribution)
        .await
        .expect("delete the attribution first");

    // Deleting the transfer_out takes its emitted records with it, which is what "not
    // independently of it" leaves allowed.
    database
        .transactions()
        .delete(out)
        .await
        .expect("delete the transfer_out");
    for record in emitted {
        assert_eq!(
            database
                .transactions()
                .find(record)
                .await
                .expect("read back"),
            None,
            "every emitted record goes with the transfer_out that emitted it"
        );
    }
}

/// An emission link is recorded only between a `transfer_out` and a `transfer_in` [DOM-094],
/// [TST-004].
///
/// The link is what makes its `transfer_in` undeletable on its own, so recording it over any
/// other pair would freeze an unrelated transaction behind a transaction that never emitted it.
#[tokio::test]
async fn an_emission_is_recorded_only_between_a_transfer_out_and_a_transfer_in() {
    let (_db, database, batch) = open().await;
    let t1 = stored_record(&database, batch, "t1").await;
    let out = store(&database, batch, isin(), &transfer_out(day(2), t1.clone())).await;
    let opening = store(
        &database,
        batch,
        isin(),
        &buy(day(1), stored_record(&database, batch, "b1").await),
    )
    .await;
    let emitted = database
        .transactions()
        .insert(
            &Placement::emitted(account(), isin()),
            &emitted_transfer_in(day(1), t1.clone()),
        )
        .await
        .expect("the emitted record");

    // Once as the emitter, once as the emitted record: neither end takes a transaction of
    // another kind.
    for (emitter, emitted_end, subject, expected_kind) in [
        (opening, emitted, opening, "transfer_out"),
        (out, opening, opening, "transfer_in"),
    ] {
        match database
            .transactions()
            .record_emission(emitter, emitted_end)
            .await
        {
            Err(StorageError::NotOfKind {
                transaction,
                expected,
                kind,
            }) => {
                assert_eq!(transaction, subject, "the refusal names the wrong-kind end");
                assert_eq!(expected, expected_kind);
                assert_eq!(kind, "buy", "the kind it met is named");
            }
            other => panic!("recording an emission over a buy must be refused, got {other:?}"),
        }
    }

    let unknown = TransactionId::new(404);
    match database
        .transactions()
        .record_emission(unknown, emitted)
        .await
    {
        Err(StorageError::UnknownTransaction { transaction }) => assert_eq!(transaction, unknown),
        other => panic!("an emission from an unknown transaction must be refused, got {other:?}"),
    }

    // No link was written by any of the three, so the buy is still deletable on its own.
    database
        .transactions()
        .delete(opening)
        .await
        .expect("the refused emissions froze nothing");
}

/// An import batch may only be deleted if none of the transactions it derived participates in an
/// attribution, and the deletion that is allowed takes the records those transactions emitted with
/// it [DOM-072], [DOM-094], [TST-004].
#[tokio::test]
async fn a_batch_whose_transaction_is_attributed_is_not_deleted() {
    let (_db, database, batch) = open().await;
    // The record the transfer_out and its emission cite, owned by this batch, so that the undo
    // meets the emission's citation of it and must not read it as foreign [DOM-119], [DEC-086].
    let t1 = stored_record(&database, batch, "t1").await;
    let opening = store(
        &database,
        batch,
        isin(),
        &buy(day(1), stored_record(&database, batch, "b1").await),
    )
    .await;
    let closing = store(
        &database,
        batch,
        isin(),
        &sell(day(2), stored_record(&database, batch, "s1").await),
    )
    .await;
    // A transfer_out the batch derived, with the record it emitted. The emitted record belongs to
    // no batch [DOM-090], so an undo reaches it only through the group its transfer_out heads; it
    // is dated after the closing below so that DOM-066 does not stand in the way of approving it.
    let out = store(&database, batch, isin(), &transfer_out(day(3), t1.clone())).await;
    let emitted = database
        .transactions()
        .insert(
            &Placement::emitted(account(), isin()),
            &emitted_transfer_in(day(1), t1.clone()),
        )
        .await
        .expect("the emitted record");
    database
        .transactions()
        .record_emission(out, emitted)
        .await
        .expect("record the emission");
    let attribution = database
        .attributions()
        .approve(
            closing,
            &[Allocation::new(opening, Quantity::new(dec!(10.00000000)))],
        )
        .await
        .expect("approve");

    match database.import_batches().delete(batch).await {
        Err(StorageError::BatchTransactionAttributed {
            batch: refused,
            transactions,
        }) => {
            assert_eq!(refused, batch);
            assert_eq!(
                transactions,
                vec![opening, closing],
                "both attributed transactions are named"
            );
        }
        other => panic!("deleting the batch must be refused, got {other:?}"),
    }
    assert!(
        database
            .import_batches()
            .find(batch)
            .await
            .expect("read back")
            .is_some(),
        "the refused deletion leaves the batch and its transactions standing"
    );

    database
        .attributions()
        .delete(attribution)
        .await
        .expect("delete the attribution");
    database
        .import_batches()
        .delete(batch)
        .await
        .expect("an unattributed batch is deletable");
    assert_eq!(
        database
            .transactions()
            .find(closing)
            .await
            .expect("read back"),
        None,
        "deleting the batch removes the transactions it derived"
    );
    for gone in [out, emitted] {
        assert_eq!(
            database.transactions().find(gone).await.expect("read back"),
            None,
            "the undo takes the emitted record with the transfer_out that emitted it"
        );
    }
}

/// An import undo is refused while a record the batch's `transfer_out` emitted is attributed
/// [DOM-072], [DOM-094], [DOM-069], [DEC-086], [TST-004].
///
/// An emitted record belongs to no batch [DOM-090], but it is derived from the `transfer_out`'s
/// records, which it cites [DEC-079], and DOM-072 is read through the records a transaction was
/// derived from. So the up-front check names it, as it would any transaction of the batch's own.
/// That refusal is all that stands between an undo and the destruction of an opening the user
/// approved an attribution against, so it is asserted on its own.
#[tokio::test]
async fn a_batch_whose_emitted_record_is_attributed_is_not_deleted() {
    let (_db, database, batch) = open().await;
    // The record the transfer_out and its emission cite, owned by this batch, so that the refusal
    // below is SRV-022's attribution refusal and not DOM-119 reading the emission's citation of
    // it as foreign [DEC-086].
    let t1 = stored_record(&database, batch, "t1").await;
    let out = store(&database, batch, isin(), &transfer_out(day(3), t1.clone())).await;
    let emitted = database
        .transactions()
        .insert(
            &Placement::emitted(account(), isin()),
            &emitted_transfer_in(day(1), t1.clone()),
        )
        .await
        .expect("the emitted record");
    database
        .transactions()
        .record_emission(out, emitted)
        .await
        .expect("record the emission");

    // The closing belongs to a second import, so no transaction placed on the batch under test is
    // attributed: only the emitted record holds the batch.
    let second = database
        .import_batches()
        .insert(&import("2025.xlsx"))
        .await
        .expect("the second import");
    let closing = store(
        &database,
        second,
        isin(),
        &sell(day(1), stored_record(&database, second, "s1").await),
    )
    .await;
    let attribution = database
        .attributions()
        .approve(
            closing,
            &[Allocation::new(emitted, Quantity::new(dec!(10.00000000)))],
        )
        .await
        .expect("attribute the emitted record as an opening");

    match database.import_batches().delete(batch).await {
        Err(StorageError::BatchTransactionAttributed {
            batch: refused,
            transactions,
        }) => {
            assert_eq!(refused, batch);
            assert_eq!(
                transactions,
                vec![emitted],
                "the emitted record the undo would have taken is named"
            );
        }
        other => panic!("undoing the import must be refused, got {other:?}"),
    }
    assert!(
        database
            .import_batches()
            .find(batch)
            .await
            .expect("read back")
            .is_some(),
        "the refused undo leaves the batch standing"
    );
    for standing in [out, emitted] {
        assert!(
            database
                .transactions()
                .find(standing)
                .await
                .expect("read back")
                .is_some(),
            "the refused undo leaves the whole group standing"
        );
    }

    database
        .attributions()
        .delete(attribution)
        .await
        .expect("delete the attribution first");
    database
        .import_batches()
        .delete(batch)
        .await
        .expect("the undo is allowed once nothing is attributed");
    assert_eq!(
        database
            .transactions()
            .find(emitted)
            .await
            .expect("read back"),
        None,
        "the permitted undo takes the emitted record with its transfer_out"
    );
}

/// DOM-072 answers for every batch whose records a transaction was derived from, not only for the
/// batch that derived it [DOM-072], [DOM-013], [TST-004].
///
/// The sell is placed on the second import and derived from a record of each; attributed, it holds
/// the first import too, and the refusal is DOM-072's, naming it as attributed.
#[tokio::test]
async fn a_batch_one_of_whose_records_an_attributed_transaction_was_derived_from_is_not_deleted() {
    let (_db, database, batch) = open().await;
    let second = database
        .import_batches()
        .insert(&import("2025.xlsx"))
        .await
        .expect("the second import");
    let opening = store(
        &database,
        second,
        isin(),
        &buy(day(1), stored_record_at(&database, second, "b1", 0).await),
    )
    .await;
    let across_files: Transaction = Sell::new(
        Derivation::new(
            day(2),
            vec1![
                stored_record_at(&database, second, "s1-cash", 1).await,
                stored_record_at(&database, batch, "s1-position", 7).await,
            ],
        ),
        Quantity::new(dec!(10.00000000)),
        price(dec!(12.000000)),
        money(dec!(120.00)),
        money(dec!(8.00)),
        conversion(),
    )
    .into();
    let closing = store(&database, second, isin(), &across_files).await;
    database
        .attributions()
        .approve(
            closing,
            &[Allocation::new(opening, Quantity::new(dec!(10.00000000)))],
        )
        .await
        .expect("approve");

    match database.import_batches().delete(batch).await {
        Err(StorageError::BatchTransactionAttributed {
            batch: refused,
            transactions,
        }) => {
            assert_eq!(refused, batch);
            assert_eq!(transactions, vec![closing]);
        }
        other => panic!("deleting the first import must be refused, got {other:?}"),
    }
}

/// An import batch may only be deleted if none of the records it owns is cited by a transaction
/// the batch did not derive, and the refusal names those transactions [DOM-119], [TST-004].
#[tokio::test]
async fn a_batch_whose_records_a_foreign_transaction_cites_is_not_deleted() {
    let (_db, database, batch) = open().await;
    let r1 = stored_record(&database, batch, "r1").await;

    // A second import whose transaction cites the first import's record: the shape of a corporate
    // action booked across two files.
    let second = database
        .import_batches()
        .insert(&import("2025.xlsx"))
        .await
        .expect("the second import");
    let citing = store(&database, second, isin(), &sell(day(2), r1.clone())).await;
    // A second foreign citer, so that the refusal is seen naming more than one transaction.
    let also_citing = store(&database, second, isin(), &sell(day(3), r1.clone())).await;
    // A citer no batch derived at all — the emitted shape of DOM-090 — is a transaction this
    // batch did not derive just as much as one another batch derived, and holds it just as hard.
    let emitted_citer = database
        .transactions()
        .insert(
            &Placement::emitted(account(), isin()),
            &transfer_in_at(day(4), r1.clone(), conversion()),
        )
        .await
        .expect("the emitted citer");
    // The batch's own transaction cites the same record and must not be named.
    let own = store(&database, batch, isin(), &buy(day(1), r1)).await;

    match database.import_batches().delete(batch).await {
        Err(StorageError::BatchRecordsCited {
            batch: refused,
            transactions,
        }) => {
            assert_eq!(refused, batch);
            assert_eq!(
                transactions,
                vec![citing, also_citing, emitted_citer],
                "only the transactions the batch did not derive hold it, in id order"
            );
            let message = StorageError::BatchRecordsCited {
                batch: refused,
                transactions,
            }
            .to_string();
            assert!(
                message.ends_with(&format!("{citing}, {also_citing}, {emitted_citer}")),
                "the refusal names every one of them in id order: {message}"
            );
        }
        other => panic!("deleting the cited batch must be refused, got {other:?}"),
    }

    for foreign in [citing, also_citing, emitted_citer] {
        database
            .transactions()
            .delete(foreign)
            .await
            .expect("delete the foreign transaction");
    }
    database
        .import_batches()
        .delete(batch)
        .await
        .expect("nothing foreign cites the batch's records now");
    assert_eq!(
        database.transactions().find(own).await.expect("read back"),
        None,
        "the batch's own transaction goes with it"
    );
}

/// A transaction a batch derived answers to that batch even when it cites none of the records the
/// batch owns: undoing the batch takes it, and while it is attributed the undo is refused naming
/// it [DOM-072], [DOM-119], [DOM-013], [TST-004].
///
/// Read through the citations alone, such a transaction would be neither derived from the batch's
/// records nor a foreign citer of them, and the undo would reach the batch row with a placement
/// still pointing at it.
#[tokio::test]
async fn a_batch_answers_for_a_transaction_it_derived_from_another_imports_records() {
    let (_db, database, batch) = open().await;
    let r1 = stored_record(&database, batch, "r1").await;
    let second = database
        .import_batches()
        .insert(&import("2025.xlsx"))
        .await
        .expect("the second import");
    let opening = store(&database, second, isin(), &buy(day(1), r1)).await;
    let closing = store(
        &database,
        batch,
        isin(),
        &sell(day(2), stored_record(&database, batch, "s1").await),
    )
    .await;
    let attribution = database
        .attributions()
        .approve(
            closing,
            &[Allocation::new(opening, Quantity::new(dec!(10.00000000)))],
        )
        .await
        .expect("approve");

    match database.import_batches().delete(second).await {
        Err(StorageError::BatchTransactionAttributed {
            batch: refused,
            transactions,
        }) => {
            assert_eq!(refused, second);
            assert_eq!(transactions, vec![opening]);
        }
        other => {
            panic!("deleting the batch that derived the opening must be refused, got {other:?}")
        }
    }

    database
        .attributions()
        .delete(attribution)
        .await
        .expect("delete the attribution");
    database
        .import_batches()
        .delete(second)
        .await
        .expect("the batch's own transaction no longer holds it");
    assert_eq!(
        database
            .transactions()
            .find(opening)
            .await
            .expect("read back"),
        None,
        "the undo takes the transaction the batch derived"
    );
    assert!(
        database
            .source_records()
            .handle(&cite("r1"))
            .await
            .expect("read the handle")
            .is_some(),
        "the record another import owns stays"
    );
}

/// A `transfer_in` a batch's `transfer_out` emitted counts as derived by that batch, so its
/// citation of the batch's records does not hold the batch; one emitted by another batch's
/// `transfer_out` still does [DOM-119], [DEC-086], [TST-004].
#[tokio::test]
async fn a_batch_whose_records_only_its_own_emissions_cite_is_deleted() {
    let (_db, database, batch) = open().await;
    let t1 = stored_record(&database, batch, "t1").await;
    let out = store(&database, batch, isin(), &transfer_out(day(3), t1.clone())).await;
    // DOM-090 emits one transfer_in per consumed parcel, each citing the transfer_out's records.
    let mut emitted = Vec::new();
    for _ in 0..2 {
        let record = database
            .transactions()
            .insert(
                &Placement::emitted(account(), isin()),
                &emitted_transfer_in(day(1), t1.clone()),
            )
            .await
            .expect("the emitted record");
        database
            .transactions()
            .record_emission(out, record)
            .await
            .expect("record the emission");
        emitted.push(record);
    }

    // A transfer_out a second import derived, whose emission cites this batch's record: that
    // emission belongs to the second batch, so to this one it is foreign.
    let second = database
        .import_batches()
        .insert(&import("2025.xlsx"))
        .await
        .expect("the second import");
    let other_out = store(
        &database,
        second,
        isin(),
        &transfer_out(day(4), stored_record(&database, second, "t2").await),
    )
    .await;
    let foreign = database
        .transactions()
        .insert(
            &Placement::emitted(account(), isin()),
            &emitted_transfer_in(day(1), t1.clone()),
        )
        .await
        .expect("the foreign emitted record");
    database
        .transactions()
        .record_emission(other_out, foreign)
        .await
        .expect("record the foreign emission");

    match database.import_batches().delete(batch).await {
        Err(StorageError::BatchRecordsCited {
            batch: refused,
            transactions,
        }) => {
            assert_eq!(refused, batch);
            assert_eq!(
                transactions,
                vec![foreign],
                "only the emission of another batch's transfer_out holds the batch"
            );
        }
        other => panic!("deleting the batch must be refused, got {other:?}"),
    }

    database
        .transactions()
        .delete(other_out)
        .await
        .expect("delete the other batch's transfer_out with its emission");
    database
        .import_batches()
        .delete(batch)
        .await
        .expect("the batch's own emissions do not hold it");
    for gone in std::iter::once(out).chain(emitted) {
        assert_eq!(
            database.transactions().find(gone).await.expect("read back"),
            None,
            "the undo takes the transfer_out and every record it emitted"
        );
    }
}

/// A manual entry is never deleted by an import undo: it belongs to no batch, and the records it
/// answers are broker identities rather than foreign keys [DOM-110], [TST-004].
#[tokio::test]
async fn an_import_undo_leaves_a_manual_entry_standing() {
    let (_db, database, batch) = open().await;
    database
        .source_records()
        .insert(batch, &record("r1"))
        .await
        .expect("the record the entry answers");
    let entry = ManualEntry::new(
        account(),
        isin(),
        Supplied::Election(Election::Stock {
            shares: Quantity::new(dec!(3.00000000)),
        }),
        [cite("r1")],
    );
    let stored = database
        .manual_entries()
        .insert(&entry)
        .await
        .expect("the manual entry");

    database
        .import_batches()
        .delete(batch)
        .await
        .expect("undo the import");

    assert_eq!(
        database
            .source_records()
            .find(&cite("r1"))
            .await
            .expect("read back"),
        None,
        "the undo removes the records the batch owned"
    );
    assert_eq!(
        database
            .manual_entries()
            .find(stored)
            .await
            .expect("read back"),
        Some(entry),
        "the entry and the identities it answers survive the undo [DOM-099]"
    );
}
