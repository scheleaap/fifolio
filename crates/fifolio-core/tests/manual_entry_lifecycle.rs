//! Integration layer [TST-003], [TST-004]: what happens to a manual entry when the import it
//! answered is undone and the same file is imported again, against a real temporary SQLite
//! database.
//!
//! The rows come through the real import framework rather than being hand-built, because the
//! reconnection matches on the identity an import computes [DOM-022], [DOM-024]: an entry that
//! reconnects against identities the test wrote itself would prove nothing about a re-import.
//!
//! What is deliberately not here: the storage refusal that a manual entry is never deleted by an
//! undo [DOM-110], which `tests/invariants.rs` owns, and the property that an undo followed by a
//! re-import restores exactly the transactions that existed before [TST-010], which is a
//! `proptest` and FIF-016's. No test reaches the network.

use chrono::{DateTime, NaiveDate, Utc};
use fifolio_core::decimal::{Money, Quantity, QuotedPrice};
use fifolio_core::entities::{
    Account, ImportBatch, Isin, Quotation, RecordIdentity, Security, SecurityType, SourceFormat,
    SourceRecord,
};
use fifolio_core::identity::{IdentitySource, identify};
use fifolio_core::import::reader::{DelimitedReader, RowReader, SourceRow};
use fifolio_core::import::{
    Import, Importer, NonPositionKind, NonPositionReason, RowClassification, RowError, RowIdentity,
    StoredAs, completion, import,
};
use fifolio_core::manual_entry::{Election, ManualEntry, Supplied};
use fifolio_core::ordering::{BatchAge, FileDirection, RowOrderingKey};
use fifolio_core::storage::{BatchId, Database, Placement, RecordHandle, TransactionId};
use fifolio_core::transaction::{Buy, BuyOrigin, Derivation, Transaction};
use fifolio_core::valuation::{Conversion, Valued};
use fifolio_test_support::TempDb;
use rust_decimal_macros::dec;
use vec1::Vec1;

/// A stand-in for a real importer: a comma-delimited file whose `id` is the broker reference,
/// whose `date` orders the row and whose `kind` states the classification. Each format's own
/// rules are its own item; what this file exercises is the framework every format shares.
struct FakeImporter {
    reader: DelimitedReader,
}

impl FakeImporter {
    fn new() -> Self {
        Self {
            reader: DelimitedReader::comma(),
        }
    }
}

impl Importer for FakeImporter {
    fn format(&self) -> SourceFormat {
        SourceFormat::TradeRepublicDeCsv
    }

    fn reader(&self) -> &dyn RowReader {
        &self.reader
    }

    fn direction(&self) -> FileDirection {
        FileDirection::OldestFirst
    }

    fn identity(&self, row: &SourceRow) -> Result<RowIdentity, RowError> {
        match row.field("id") {
            Some(id) if !id.is_empty() => Ok(RowIdentity::BrokerReference(id.to_owned())),
            _ => Err(RowError::new("no id")),
        }
    }

    fn ordering_key(&self, row: &SourceRow) -> Result<RowOrderingKey, RowError> {
        let date = row.field("date").unwrap_or_default();
        Ok(RowOrderingKey {
            trade_date: NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .map_err(|_| RowError::new(format!("{date} is not a date")))?,
            columns: Vec::new(),
        })
    }

    fn classify(&self, rows: &[SourceRow]) -> Vec<Result<RowClassification, RowError>> {
        rows.iter()
            .map(|row| match row.field("kind") {
                Some("buy") => Ok(RowClassification::DerivedAutomatically),
                // A dividend that issues shares is a position event, and the count is in no
                // column, so it waits for the user [DOM-045], [DOM-124].
                Some("stock dividend") => Ok(RowClassification::Pending),
                Some("interest") => Ok(RowClassification::NonPosition(
                    NonPositionReason::Recognized(NonPositionKind::Interest),
                )),
                other => Err(RowError::new(format!("unknown kind {other:?}"))),
            })
            .collect()
    }
}

/// One file, one account: a row that derives on its own, a row that waits for the user, and a
/// row that is not stored at all.
const FILE: &str = "id,date,kind\n\
                    a,2024-01-02,buy\n\
                    b,2024-01-03,stock dividend\n\
                    c,2024-01-04,interest\n";

/// A second file of the same account, so that an undo and a reconnection can be watched with
/// another import standing beside them. Its two pending rows are one corporate action spread over
/// two bookings, which is why an entry names several records [DOM-098].
const OTHER_FILE: &str = "id,date,kind\n\
                          d,2024-01-05,buy\n\
                          e,2024-01-06,stock dividend\n\
                          f,2024-01-07,stock dividend\n";

fn account() -> Account {
    Account::new("Trade Republic", "DE0001")
}

fn isin() -> Isin {
    Isin::new("NL0000009538")
}

fn imported_at() -> DateTime<Utc> {
    "2024-05-03T09:00:00Z".parse().expect("a valid timestamp")
}

fn day(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2024, 1, day).expect("a valid date")
}

fn money(amount: rust_decimal::Decimal) -> Valued<Money> {
    Valued::new(Money::new(amount), Money::new(amount))
}

fn price(amount: rust_decimal::Decimal) -> Valued<QuotedPrice> {
    Valued::new(QuotedPrice::new(amount), QuotedPrice::new(amount))
}

/// A database with the account and the security a placed transaction needs.
async fn open() -> (TempDb, Database) {
    let db = TempDb::new();
    let database = Database::open(db.path())
        .await
        .expect("open the temporary database");
    database
        .accounts()
        .insert(&account())
        .await
        .expect("the account");
    database
        .securities()
        .insert(&Security::auto_created(
            isin(),
            "NN Group",
            SecurityType::Stock,
            Quotation::PerUnit,
        ))
        .await
        .expect("the security");
    (db, database)
}

/// Imports `FILE` into the account and stores what it yielded, which is what an import does,
/// handing back the handles storage issued for the records it wrote.
async fn import_file(database: &Database) -> (BatchId, Import, Vec<RecordHandle>) {
    import_bytes(database, "2024.csv", FILE).await
}

async fn import_bytes(
    database: &Database,
    filename: &str,
    content: &str,
) -> (BatchId, Import, Vec<RecordHandle>) {
    let imported = import(&FakeImporter::new(), &account(), content.as_bytes()).expect("the file");
    let batch = database
        .import_batches()
        .insert(&ImportBatch::new(
            account(),
            filename,
            SourceFormat::TradeRepublicDeCsv,
            imported_at(),
            imported.counts(),
        ))
        .await
        .expect("the batch");
    let mut issued = Vec::new();
    for stored in imported.stored() {
        issued.push(
            database
                .source_records()
                .insert(batch, stored.record())
                .await
                .expect("the source record"),
        );
    }
    (batch, imported, issued)
}

/// The handles among `issued` on `records`, in the order of `records`: a derivation is made from
/// at least one record storage holds [DOM-047].
fn handles(issued: &[RecordHandle], records: &[SourceRecord]) -> Vec1<RecordHandle> {
    records
        .iter()
        .map(|record| {
            issued
                .iter()
                .find(|handle| handle.identity() == record.identity())
                .cloned()
                .expect("the import stored the record")
        })
        .collect::<Vec<_>>()
        .try_into()
        .expect("a transaction derives from at least one record")
}

/// The transaction a row that needs nothing from the user derives to.
fn purchase(records: Vec1<RecordHandle>) -> Transaction {
    Buy::new(
        Derivation::new(day(2), records),
        Quantity::new(dec!(100.00000000)),
        price(dec!(10.000000)),
        money(dec!(1000.00)),
        money(dec!(8.00)),
        BuyOrigin::Purchase,
        Conversion::native(day(2)),
    )
    .into()
}

/// The transaction a pending row derives to once the user has supplied the share count: a buy
/// whose origin is a stock dividend [DOM-124].
///
/// This stands in for a format's own derivation rule (IMP-SAXO-018), which is each importer's
/// item. What matters here is that it is a function of the entry and the records alone, so that
/// running it again after a re-import needs nothing from the user.
fn stock_dividend(entry: &ManualEntry, records: Vec1<RecordHandle>) -> Transaction {
    let Supplied::Election(Election::Stock { shares }) = entry.supplied() else {
        panic!("the entry supplies a stock election")
    };
    Buy::new(
        Derivation::new(day(3), records),
        *shares,
        price(dec!(4.000000)),
        money(dec!(12.00)),
        money(dec!(0.00)),
        BuyOrigin::StockDividend,
        Conversion::native(day(3)),
    )
    .into()
}

async fn store(database: &Database, batch: BatchId, transaction: &Transaction) -> TransactionId {
    database
        .transactions()
        .insert(&Placement::derived(account(), isin(), batch), transaction)
        .await
        .expect("store the transaction")
}

/// The records an import stored as pending, which is the completion queue [DOM-045].
fn pending(imported: &Import) -> Vec<SourceRecord> {
    imported
        .stored()
        .iter()
        .filter(|stored| stored.stored_as() == StoredAs::Pending)
        .map(|stored| stored.record().clone())
        .collect()
}

fn derived(imported: &Import) -> Vec<SourceRecord> {
    imported
        .stored()
        .iter()
        .filter(|stored| stored.stored_as() == StoredAs::DerivedAutomatically)
        .map(|stored| stored.record().clone())
        .collect()
}

/// What the user answered the pending row with.
fn entry_for(records: &[SourceRecord]) -> ManualEntry {
    completion(
        account(),
        isin(),
        Supplied::Election(Election::Stock {
            shares: Quantity::new(dec!(3.00000000)),
        }),
        records,
    )
}

/// Undoing an import removes the source records it owns and the transactions derived from them,
/// leaves another import's records and transactions standing, and leaves every manual entry
/// standing [DOM-108], [TST-004].
///
/// "Derived from them" is read through the records a transaction cites [DOM-013], together with
/// the batch its placement names as the one that derived it.
///
/// The second import is what makes "**its** source records" an assertion: with one batch present
/// an undo that deleted every record and every transaction in the account would pass.
#[tokio::test]
async fn an_undo_removes_the_imports_records_and_transactions_and_leaves_the_entry_standing() {
    let (_db, database) = open().await;
    let (batch, imported, issued) = import_file(&database).await;
    let (other_batch, other, other_issued) = import_bytes(&database, "other.csv", OTHER_FILE).await;
    let survivor = store(
        &database,
        other_batch,
        &purchase(handles(&other_issued, &derived(&other))),
    )
    .await;
    let answered = pending(&imported);
    let entry = entry_for(&answered);
    let stored_entry = database
        .manual_entries()
        .insert(&entry)
        .await
        .expect("the manual entry");
    let purchase = store(
        &database,
        batch,
        &purchase(handles(&issued, &derived(&imported))),
    )
    .await;
    let completed = store(
        &database,
        batch,
        &stock_dividend(&entry, handles(&issued, &answered)),
    )
    .await;

    database
        .import_batches()
        .delete(batch)
        .await
        .expect("undo the import");

    for record in imported.stored() {
        assert_eq!(
            database
                .source_records()
                .find(record.record().identity())
                .await
                .expect("read back"),
            None,
            "the undo removes every record the batch owned"
        );
    }
    for transaction in [purchase, completed] {
        assert_eq!(
            database
                .transactions()
                .find(transaction)
                .await
                .expect("read back"),
            None,
            "the undo removes the transactions the batch derived"
        );
    }
    for record in other.stored() {
        assert!(
            database
                .source_records()
                .find(record.record().identity())
                .await
                .expect("read back")
                .is_some(),
            "the undo leaves the other import's records standing"
        );
    }
    assert!(
        database
            .transactions()
            .find(survivor)
            .await
            .expect("read back")
            .is_some(),
        "the undo leaves the transactions another batch derived standing"
    );
    assert_eq!(
        database
            .manual_entries()
            .find(stored_entry)
            .await
            .expect("read back"),
        Some(entry),
        "the entry the user supplied stands, with the identities it named"
    );
}

/// An entry whose records are absent is listed as waiting, naming what it expects: the account,
/// the security and the identities it is waiting for [DOM-109], [TST-004].
#[tokio::test]
async fn an_entry_whose_records_are_absent_is_listed_as_waiting_naming_what_it_expects() {
    let (_db, database) = open().await;
    let (batch, imported, _) = import_file(&database).await;
    let answered = pending(&imported);
    let entry = entry_for(&answered);
    let stored_entry = database
        .manual_entries()
        .insert(&entry)
        .await
        .expect("the manual entry");

    assert!(
        database
            .manual_entries()
            .waiting()
            .await
            .expect("list the waiting entries")
            .is_empty(),
        "an entry whose records are present is not waiting for anything"
    );

    database
        .import_batches()
        .delete(batch)
        .await
        .expect("undo the import");

    let waiting = database
        .manual_entries()
        .waiting()
        .await
        .expect("list the waiting entries");

    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].id(), stored_entry);
    assert_eq!(waiting[0].entry().account(), &account());
    assert_eq!(waiting[0].entry().security(), &isin());
    assert_eq!(
        waiting[0].missing(),
        &[identify(&account(), &IdentitySource::BrokerReference("b"))],
        "it names the row it is waiting for, as the entry named it"
    );
}

/// Re-importing the same rows reconnects each entry by the record identities it names, and the
/// records come back with it so the transaction it completed can be derived again [DOM-108],
/// [TST-004].
#[tokio::test]
async fn a_re_import_reconnects_the_entry_by_the_identities_it_names() {
    let (_db, database) = open().await;
    let (batch, imported, _) = import_file(&database).await;
    let answered = pending(&imported);
    let stored_entry = database
        .manual_entries()
        .insert(&entry_for(&answered))
        .await
        .expect("the manual entry");
    database
        .import_batches()
        .delete(batch)
        .await
        .expect("undo the import");

    let (reimported_batch, _, _) = import_file(&database).await;

    let reconnected = database
        .manual_entries()
        .reconnected(reimported_batch)
        .await
        .expect("list what the import reconnected");
    assert_eq!(reconnected.len(), 1);
    assert_eq!(reconnected[0].id(), stored_entry);
    assert_eq!(
        reconnected[0].records(),
        answered.as_slice(),
        "the records return as the file holds them, matched by identity alone"
    );
    assert!(
        database
            .manual_entries()
            .waiting()
            .await
            .expect("list the waiting entries")
            .is_empty(),
        "an entry that reconnected is no longer waiting"
    );
}

/// An undo followed by a re-import returns the account exactly where it was: the entry
/// reconnects without being asked again, and the transaction it completed is the transaction
/// that was there before [DOM-108], [TST-004].
///
/// The derivation itself is a format's and belongs to the derive path (SRV-028); what this
/// asserts is that it needs nothing the re-import does not already hand it.
#[tokio::test]
async fn an_undo_followed_by_a_re_import_returns_the_account_where_it_was() {
    let (_db, database) = open().await;
    let (batch, imported, issued) = import_file(&database).await;
    let answered = pending(&imported);
    let entry = entry_for(&answered);
    database
        .manual_entries()
        .insert(&entry)
        .await
        .expect("the manual entry");
    let before: Vec<Transaction> = vec![
        purchase(handles(&issued, &derived(&imported))),
        stock_dividend(&entry, handles(&issued, &answered)),
    ];
    for transaction in &before {
        store(&database, batch, transaction).await;
    }

    database
        .import_batches()
        .delete(batch)
        .await
        .expect("undo the import");
    let (reimported_batch, reimported, reissued) = import_file(&database).await;

    let mut after = vec![purchase(handles(&reissued, &derived(&reimported)))];
    for reconnection in database
        .manual_entries()
        .reconnected(reimported_batch)
        .await
        .expect("list what the import reconnected")
    {
        // The reconnection's own handles, since storage has just read those records back.
        let restored = stock_dividend(reconnection.entry(), reconnection.handles());
        let id = store(&database, reimported_batch, &restored).await;
        after.push(
            database
                .transactions()
                .find(id)
                .await
                .expect("read back")
                .expect("the restored transaction is stored"),
        );
    }

    assert_eq!(after, before, "the account is where it was before the undo");
}

/// With a newer import standing, an undo followed by a re-import restores the same transactions
/// except their batch age, which is the re-import's: the undone batch was the records' oldest
/// supplier and is gone (DEC-096, provisional) [TST-010], [DOM-111], [DEC-092], [TST-004].
///
/// The newer import's purchase ties with this file's on trade date and `order`, so the age is
/// what orders them, and the order between them turns.
#[tokio::test]
async fn an_undo_and_re_import_beside_a_newer_import_takes_the_re_imports_age() {
    let (_db, database) = open().await;
    let (batch, imported, issued) = import_file(&database).await;
    let (other_batch, other, other_issued) = import_bytes(&database, "other.csv", OTHER_FILE).await;
    let answered = pending(&imported);
    let entry = entry_for(&answered);
    database
        .manual_entries()
        .insert(&entry)
        .await
        .expect("the manual entry");
    let before = [
        purchase(handles(&issued, &derived(&imported))),
        stock_dividend(&entry, handles(&issued, &answered)),
    ];
    for transaction in &before {
        store(&database, batch, transaction).await;
    }
    let tied = purchase(handles(&other_issued, &derived(&other)));
    store(&database, other_batch, &tied).await;
    assert!(before[0].order_key() < tied.order_key());

    database
        .import_batches()
        .delete(batch)
        .await
        .expect("undo the import");
    let (reimported_batch, reimported, reissued) = import_file(&database).await;
    assert!(reimported_batch.get() > other_batch.get());

    let mut rebuilt = vec![purchase(handles(&reissued, &derived(&reimported)))];
    let mut after = Vec::new();
    for reconnection in database
        .manual_entries()
        .reconnected(reimported_batch)
        .await
        .expect("list what the import reconnected")
    {
        rebuilt.push(stock_dividend(reconnection.entry(), reconnection.handles()));
    }
    for transaction in &rebuilt {
        let id = store(&database, reimported_batch, transaction).await;
        after.push(
            database
                .transactions()
                .find(id)
                .await
                .expect("read back")
                .expect("the restored transaction is stored"),
        );
    }
    assert_eq!(after, rebuilt, "what was derived is what is stored");

    assert_eq!(after.len(), before.len());
    for (was, is) in before.iter().zip(&after) {
        assert_eq!(is.cites(), was.cites(), "the same records");
        let (was, is) = (was.order_key(), is.order_key());
        assert_eq!(is.trade_date(), was.trade_date());
        assert_eq!(is.position().order(), was.position().order());
        assert_eq!(is.leg(), was.leg());
        assert_eq!(was.position().batch_age(), BatchAge::new(batch.get()));
        assert_eq!(
            is.position().batch_age(),
            BatchAge::new(reimported_batch.get()),
            "the re-import is now the oldest supplier"
        );
    }
    assert!(
        tied.order_key() < after[0].order_key(),
        "the newer import's tied purchase now comes first"
    );
}

/// An entry that names a record the re-import did not bring back stays waiting, and names only
/// what is still absent [DOM-108], [DOM-109], [TST-004].
#[tokio::test]
async fn an_entry_naming_a_record_no_import_restored_stays_waiting() {
    let (_db, database) = open().await;
    let (batch, imported, _) = import_file(&database).await;
    // A second row of the same event, from a file this database never saw: a corporate action
    // spread over two exports is why an entry names several records [DOM-098].
    let elsewhere = identify(&account(), &IdentitySource::BrokerReference("z"));
    let entry = ManualEntry::new(
        account(),
        isin(),
        Supplied::Election(Election::Stock {
            shares: Quantity::new(dec!(3.00000000)),
        }),
        pending(&imported)
            .iter()
            .map(|record| record.identity().clone())
            .chain([elsewhere.clone()]),
    );
    database
        .manual_entries()
        .insert(&entry)
        .await
        .expect("the manual entry");
    database
        .import_batches()
        .delete(batch)
        .await
        .expect("undo the import");

    let (reimported_batch, _, _) = import_file(&database).await;

    assert!(
        database
            .manual_entries()
            .reconnected(reimported_batch)
            .await
            .expect("list what the import reconnected")
            .is_empty(),
        "an entry is reconnected only when every record it names is present"
    );
    let waiting = database
        .manual_entries()
        .waiting()
        .await
        .expect("list the waiting entries");
    assert_eq!(waiting.len(), 1);
    assert_eq!(
        waiting[0].missing(),
        &[elsewhere],
        "it waits for what is absent and not for what returned"
    );
}

/// An entry of another account is untouched by an import into this one, because an identity is
/// scoped to its account [DOM-024], [DOM-109], [TST-004].
#[tokio::test]
async fn an_entry_of_another_account_is_not_reconnected() {
    let (_db, database) = open().await;
    let elsewhere = Account::new("Trade Republic", "DE0002");
    database
        .accounts()
        .insert(&elsewhere)
        .await
        .expect("the other account");
    // The same broker reference as the pending row of `FILE`, scoped to the other account.
    let named: Vec<RecordIdentity> =
        vec![identify(&elsewhere, &IdentitySource::BrokerReference("b"))];
    database
        .manual_entries()
        .insert(&ManualEntry::new(
            elsewhere,
            isin(),
            Supplied::Election(Election::Cash),
            named,
        ))
        .await
        .expect("the manual entry");

    let (batch, _, _) = import_file(&database).await;

    assert!(
        database
            .manual_entries()
            .reconnected(batch)
            .await
            .expect("list what the import reconnected")
            .is_empty(),
        "the same row in another account is another record"
    );
    assert_eq!(
        database
            .manual_entries()
            .waiting()
            .await
            .expect("list the waiting entries")
            .len(),
        1,
        "the other account's entry is still waiting for its own rows"
    );
}

/// An import reconnects only the entries its own records answer: an entry whose records another
/// file of the same account brought is none of this batch's business [DOM-108], [TST-004].
///
/// Without the batch scope the listing would read "every entry whose records are all present",
/// and importing one file would hand back the entries of every other, to be derived again against
/// a batch that never held their rows.
#[tokio::test]
async fn an_import_reconnects_only_the_entries_its_own_records_answer() {
    let (_db, database) = open().await;
    let (batch, imported, _) = import_file(&database).await;
    let stored_entry = database
        .manual_entries()
        .insert(&entry_for(&pending(&imported)))
        .await
        .expect("the manual entry");

    let (other_batch, _, _) = import_bytes(&database, "other.csv", OTHER_FILE).await;

    assert!(
        database
            .manual_entries()
            .reconnected(other_batch)
            .await
            .expect("list what the import reconnected")
            .is_empty(),
        "an import reconnects nothing it did not bring the records for"
    );
    let reconnected = database
        .manual_entries()
        .reconnected(batch)
        .await
        .expect("list what the import reconnected");
    assert_eq!(reconnected.len(), 1);
    assert_eq!(
        reconnected[0].id(),
        stored_entry,
        "the batch that owns the records it names is the one that reconnects it"
    );
}

/// An entry answering several records reconnects with all of them, in the order it names them and
/// not the order they are stored in, and with a handle on each in that order [DOM-108], [DOM-098],
/// [DOM-047], [TST-004].
#[tokio::test]
async fn an_entry_naming_several_records_reconnects_with_them_in_the_order_it_names() {
    let (_db, database) = open().await;
    let (batch, imported, _) = import_bytes(&database, "other.csv", OTHER_FILE).await;
    // Reversed against the file, so a listing that returned the records in storage order rather
    // than the entry's would fail here.
    let named: Vec<SourceRecord> = pending(&imported).into_iter().rev().collect();
    assert_eq!(named.len(), 2, "the file spreads one event over two rows");
    database
        .manual_entries()
        .insert(&entry_for(&named))
        .await
        .expect("the manual entry");
    database
        .import_batches()
        .delete(batch)
        .await
        .expect("undo the import");

    let (reimported_batch, _, _) = import_bytes(&database, "other.csv", OTHER_FILE).await;

    let reconnected = database
        .manual_entries()
        .reconnected(reimported_batch)
        .await
        .expect("list what the import reconnected");
    assert_eq!(reconnected.len(), 1);
    assert_eq!(
        reconnected[0].records(),
        named.as_slice(),
        "every record it names returns, in the order it names them"
    );
    assert_eq!(
        reconnected[0]
            .handles()
            .iter()
            .map(RecordHandle::identity)
            .collect::<Vec<_>>(),
        named.iter().map(SourceRecord::identity).collect::<Vec<_>>(),
        "a handle for every record it names, in the order it names them [DOM-047]"
    );
}

/// An entry that names no record is waiting for nothing and is not listed as waiting [DOM-109],
/// [TST-004].
///
/// What such an entry means is CLI-039's and SRV-026's; this pins only that the listing does not
/// decide it by reporting the entry as waiting for an empty set.
#[tokio::test]
async fn an_entry_that_names_no_record_is_not_waiting() {
    let (_db, database) = open().await;
    database
        .manual_entries()
        .insert(&ManualEntry::new(
            account(),
            isin(),
            Supplied::Election(Election::Cash),
            Vec::<RecordIdentity>::new(),
        ))
        .await
        .expect("the manual entry");

    assert!(
        database
            .manual_entries()
            .waiting()
            .await
            .expect("list the waiting entries")
            .is_empty(),
        "an entry with no answers waits for nothing"
    );
}

/// Several entries wait at once, each naming its own absent identities and not another's
/// [DOM-109], [TST-004].
#[tokio::test]
async fn each_waiting_entry_names_its_own_missing_identities() {
    let (_db, database) = open().await;
    let (batch, imported, _) = import_file(&database).await;
    let first = database
        .manual_entries()
        .insert(&entry_for(&pending(&imported)))
        .await
        .expect("the first manual entry");
    // An entry answering a file this database never saw, so it waits for both its identities.
    let elsewhere: Vec<RecordIdentity> = ["y", "z"]
        .into_iter()
        .map(|reference| identify(&account(), &IdentitySource::BrokerReference(reference)))
        .collect();
    let second = database
        .manual_entries()
        .insert(&ManualEntry::new(
            account(),
            isin(),
            Supplied::Election(Election::Cash),
            elsewhere.clone(),
        ))
        .await
        .expect("the second manual entry");
    database
        .import_batches()
        .delete(batch)
        .await
        .expect("undo the import");

    let waiting = database
        .manual_entries()
        .waiting()
        .await
        .expect("list the waiting entries");

    assert_eq!(waiting.len(), 2);
    assert_eq!(waiting[0].id(), first);
    assert_eq!(
        waiting[0].missing(),
        &[identify(&account(), &IdentitySource::BrokerReference("b"))],
        "the first waits for the row the undo took and for nothing of the second's"
    );
    assert_eq!(waiting[1].id(), second);
    assert_eq!(
        waiting[1].missing(),
        elsewhere.as_slice(),
        "the second waits for both its own identities"
    );
}
