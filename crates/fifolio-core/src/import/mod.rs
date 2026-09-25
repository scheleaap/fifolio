//! Turning a broker file into source records [DOM-042].
//!
//! Import creates source records; transactions are derived from one or more of them afterwards,
//! because one real event is often several rows. This module owns the half every format shares:
//! reading the file [ARC-023], identifying and ordering its rows, and classifying each one.
//! What a column means, which rows group together and what transaction comes out is each
//! format's own item.
//!
//! # The three outcomes
//!
//! A row is classified as [derived automatically](RowClassification::DerivedAutomatically) when
//! every field its variant needs is present and unambiguous [DOM-044]; as
//! [pending](RowClassification::Pending) when it affects holdings but the export does not carry
//! everything needed, which is the completion queue [DOM-045]; or as
//! [non-position](RowClassification::NonPosition) — a **cash** dividend, interest, a deposit, a
//! withdrawal, an account fee — which is counted and not stored [DOM-002], [DOM-046].
//!
//! Not stored is structural here: [`Import`] holds a [`SourceRecord`] only for a row it stored,
//! so there is no non-position record for a later step to pick up. A dividend that issues
//! shares is **not** one of these: it is a position event, so its rows are stored and the buy it
//! produces carries [`BuyOrigin::StockDividend`](crate::transaction::BuyOrigin::StockDividend)
//! [DOM-124]. Which rows those are is a format's heuristic — Saxo's is IMP-SAXO-018 — and the
//! framework only guarantees that a classification that is not non-position is kept.
//!
//! Those three are the whole taxonomy [DOM-043], stated on [`RowClassification`] and nowhere
//! else.
//!
//! # What the user supplies
//!
//! Nothing here asks the user anything. When a pending row is answered, the answer becomes a
//! [`ManualEntry`] and nothing else [DOM-048]: [`completion`] is the only constructor this
//! module offers over user input, and it reads the identities it references off the records
//! being answered rather than taking them from a caller.
//!
//! # Failures
//!
//! A row whose ordering key, account, identity or classification cannot be read is a failed
//! row, and any failed row refuses the import as a whole [SRV-058] (DEC-074, DEC-087): skipping
//! it would leave a batch owning part of a file, and without a row's key the file has no total
//! order, so every row's `order` would depend on which rows were dropped. Every row is still
//! tried first, so that the refusal names every failed row and one round of fixes suffices.

pub mod reader;
pub mod saxo;
#[cfg(test)]
mod test_workbook;
pub mod trade_republic;

use std::collections::BTreeSet;

use chrono::Datelike;

use crate::entities::{Account, ImportCounts, Order, SourceFormat, SourceRecord};
use crate::identity::{IdentitySource, identify};
use crate::manual_entry::{ManualEntry, Supplied};
use crate::ordering::{FileDirection, RowOrderingKey, assign_orders};
use reader::{ReadError, RowReader, SourceRow};

use thiserror::Error;

use crate::entities::Isin;

/// What a row turned out to be [DOM-044], [DOM-045], [DOM-046].
///
/// This enum *is* the classification taxonomy of DOM-043, and it is the only statement of it:
/// the set is closed because [`Importer::classify`] can answer nothing outside these variants.
/// There is deliberately no catch-all variant and no [`Default`], so a format cannot land a row
/// somewhere without deciding what it is, and an importer written later has no escape hatch
/// from the decision.
///
/// A new classification is an amendment to `domain.md` first and to this enum second. It is
/// never a format's invention: a broker's own vocabulary maps *into* this set, and a row that
/// does not map is a [`RowError`], not a fourth outcome.
///
/// [`EnumCount`](strum::EnumCount) is derived so the size of the set is readable at compile
/// time: the test that pins DOM-043 counts the outcomes an import reaches against it, and a
/// variant added here fails that count rather than passing unnoticed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumCount)]
pub enum RowClassification {
    /// Every field the variant needs is present and unambiguous [DOM-044].
    DerivedAutomatically,
    /// The row affects holdings, but the export does not carry everything needed. It waits for
    /// the user: this is the completion queue [DOM-045].
    Pending,
    /// Income that is not a disposal. Counted and not stored [DOM-002], [DOM-046].
    NonPosition(NonPositionKind),
}

/// The kinds of row that are recognized as carrying no position effect [DOM-002].
///
/// `CashDividend` is spelled cash deliberately: a dividend taken in shares issues a parcel and
/// is a position event, so it is stored [DOM-124] and has no kind here to be classified as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NonPositionKind {
    CashDividend,
    Interest,
    Deposit,
    Withdrawal,
    AccountFee,
}

/// Why a stored row was stored, which is the classification minus the outcome that stores
/// nothing [DOM-046].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredAs {
    DerivedAutomatically,
    Pending,
}

/// A source record and what the importer made of the row it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRecord {
    record: SourceRecord,
    stored_as: StoredAs,
}

impl StoredRecord {
    #[must_use]
    pub fn record(&self) -> &SourceRecord {
        &self.record
    }

    #[must_use]
    pub fn stored_as(&self) -> StoredAs {
        self.stored_as
    }
}

/// What a format offers to identify a row, owned rather than borrowed.
///
/// [`IdentitySource`] borrows the fields it hashes, which an importer computing them per row
/// cannot return; this is the same two cases in a shape a trait method can produce. The account
/// scoping stays in `identity::identify` [DOM-024], so an importer still cannot build an
/// unscoped identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowIdentity {
    /// The broker's own stable reference for the row [DOM-023].
    BrokerReference(String),
    /// The parsed business fields, in the order the format states, when it has no reference.
    ParsedFields(Vec<String>),
}

/// Why one row could not be read as the format expects. The message is the format's own.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct RowError(String);

impl RowError {
    #[must_use]
    pub fn new(reason: impl Into<String>) -> Self {
        Self(reason.into())
    }

    #[must_use]
    pub fn reason(&self) -> &str {
        &self.0
    }
}

/// A row whose ordering key, identity or classification could not be read, by its position in
/// the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowFailure {
    pub position: usize,
    pub error: RowError,
}

/// Why an import could not be performed at all.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ImportError {
    #[error("the file could not be read: {0}")]
    Read(#[from] ReadError),
    /// Broker exports are taken a year at a time, so a file spanning two is refused whole
    /// [IMP-001]. The years are those of the rows' **trade dates** [IMP-002], ascending.
    #[error("the file carries trade dates in more than one calendar year: {}", years.iter().map(i32::to_string).collect::<Vec<_>>().join(", "))]
    MultipleCalendarYears { years: Vec<i32> },
    /// The file names one account and the import targets another [IMP-003]: the mis-aimed
    /// import batch deletion exists to undo, refused before anything is stored.
    #[error("the file names account {file}, but the import targets account {target}")]
    AccountMismatch { file: String, target: String },
    /// The file carries rows from more than one account, refused outright [IMP-003] rather than
    /// compared, since no one target could be right for all of them. Ascending; at least two.
    #[error("the file carries rows from more than one account: {}", accounts.join(", "))]
    MultipleAccounts { accounts: Vec<String> },
    /// An importer that answers a different number of rows than it was given has lost the
    /// correspondence between row and classification; pairing them anyway would file one row's
    /// outcome against another's record.
    #[error("the importer classified {classified} of {rows} rows")]
    ClassificationCount { rows: usize, classified: usize },
    /// Rows whose ordering key, identity or classification could not be read refuse the file
    /// whole [SRV-058]. Every one is named, in file order, not only the first (DEC-074,
    /// DEC-087); never empty.
    #[error("{} rows could not be read: {}", failures.len(), failures.iter().map(|failure| format!("row {}: {}", failure.position, failure.error)).collect::<Vec<_>>().join("; "))]
    FailedRows { failures: Vec<RowFailure> },
}

/// What one file yielded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    format: SourceFormat,
    stored: Vec<StoredRecord>,
    counts: ImportCounts,
}

impl Import {
    #[must_use]
    pub fn format(&self) -> SourceFormat {
        self.format
    }

    /// Every row that was stored, in file order. A non-position row is not among them
    /// [DOM-002], [DOM-046].
    #[must_use]
    pub fn stored(&self) -> &[StoredRecord] {
        &self.stored
    }

    /// The records awaiting the user, which is the completion queue [DOM-045].
    pub fn pending(&self) -> impl Iterator<Item = &SourceRecord> {
        self.stored
            .iter()
            .filter(|stored| stored.stored_as == StoredAs::Pending)
            .map(StoredRecord::record)
    }

    /// How the import classified what it read [DOM-046], as an import batch records it.
    #[must_use]
    pub fn counts(&self) -> ImportCounts {
        self.counts
    }
}

/// One broker export format, read row by row [DOM-042].
///
/// A format supplies the container it arrives in, the direction its rows run, and, per row, what
/// identifies it, what orders it and what it is. Everything else — scoping the identity to the
/// account, assigning orders over the file, discarding what is not stored and counting — is
/// [`import`]'s, so that no format can forget it.
pub trait Importer {
    /// The format an import batch records [DOM-017].
    fn format(&self) -> SourceFormat;

    /// How this format's files are read: a spreadsheet or a delimited file [ARC-023].
    fn reader(&self) -> &dyn RowReader;

    /// Which end of the file holds the oldest row [DOM-040].
    fn direction(&self) -> FileDirection;

    /// What identifies `row`, before it is scoped to the account [DOM-023].
    ///
    /// # Errors
    ///
    /// When the row carries nothing the format identifies it by.
    fn identity(&self, row: &SourceRow) -> Result<RowIdentity, RowError>;

    /// What `row` is ordered by: its trade date and the format's ordering columns [DOM-040].
    ///
    /// # Errors
    ///
    /// When the trade date or an ordering column cannot be read, which makes it a failed row
    /// [SRV-058].
    fn ordering_key(&self, row: &SourceRow) -> Result<RowOrderingKey, RowError>;

    /// The account `row` names, as the format normalizes it, or `None` when the format states
    /// no account id [IMP-003].
    ///
    /// The answer is compared with [`Account::id`], which carries the normalized id, so a
    /// format whose file spells the account in several ways answers the one it normalizes to:
    /// Saxo's is the `Rekening-ID` without its currency suffix [DOM-003], never the raw cell.
    ///
    /// The default states none, as Trade Republic's export does: such a file has nothing to
    /// check and is imported into the account the caller names (DEC-075).
    ///
    /// # Errors
    ///
    /// When the format states an account id and this row's cannot be read, which makes it a
    /// failed row [SRV-058].
    fn account_id(&self, _row: &SourceRow) -> Result<Option<String>, RowError> {
        Ok(None)
    }

    /// What each row of the file is [DOM-043], one answer per row in the order given.
    ///
    /// It takes the whole file because a classification can depend on a row's neighbours: a
    /// Saxo dividend is classified by its `Corporate action-Id` group and not by its own cells,
    /// and evaluating such a row alone drops the money (IMP-SAXO-019).
    fn classify(&self, rows: &[SourceRow]) -> Vec<Result<RowClassification, RowError>>;
}

/// Reads `content` as `importer`'s format and yields the source records it stores [DOM-042].
///
/// # Errors
///
/// [`ImportError`] when the file cannot be read; when its trade dates span more than one year
/// [IMP-001]; when it names an account other than `account`, or more than one [IMP-003]; or
/// when any row's ordering key, account, identity or classification cannot be read, in which
/// case every such row is named [SRV-058].
pub fn import(
    importer: &dyn Importer,
    account: &Account,
    content: &[u8],
) -> Result<Import, ImportError> {
    let rows = importer.reader().rows(content)?;

    // Every row's key is read before any refusal, so that an unreadable one is named with every
    // other failed row rather than stopping the import at the first [SRV-058] (DEC-087).
    let keys: Vec<Result<RowOrderingKey, RowError>> =
        rows.iter().map(|row| importer.ordering_key(row)).collect();
    // The year guard reads the ordering keys' trade dates and nothing else [IMP-002]: not a
    // booking timestamp, which a format may also put in its ordering columns, and not the
    // filename, which `import` never sees. A partial year is a single year and passes. A row
    // whose key is unreadable has no trade date to contribute, and refuses the file below, so no
    // file is imported with a trade date the guard has not seen. Reporting the years and the
    // failed rows together [SRV-059] is FIF-103's, not this refusal's.
    let years: BTreeSet<i32> = keys
        .iter()
        .flatten()
        .map(|key| key.trade_date.year())
        .collect();
    if years.len() > 1 {
        return Err(ImportError::MultipleCalendarYears {
            years: years.into_iter().collect(),
        });
    }

    // The account guard [IMP-003], like the year guard, reads every row first and judges only
    // the ids it could read; a row whose id is unreadable is a failed row and refuses the file
    // below, so no file is imported with an account the guard has not seen. Combining this
    // refusal with the others [SRV-059] is FIF-103's.
    let named: Vec<Result<Option<String>, RowError>> =
        rows.iter().map(|row| importer.account_id(row)).collect();
    let accounts: BTreeSet<&str> = named
        .iter()
        .flatten()
        .flatten()
        .map(String::as_str)
        .collect();
    if accounts.len() > 1 {
        return Err(ImportError::MultipleAccounts {
            accounts: accounts.into_iter().map(str::to_owned).collect(),
        });
    }
    if let Some(file) = accounts.first().filter(|file| **file != account.id()) {
        return Err(ImportError::AccountMismatch {
            file: (*file).to_owned(),
            target: account.id().to_owned(),
        });
    }

    let classifications = importer.classify(&rows);
    if classifications.len() != rows.len() {
        return Err(ImportError::ClassificationCount {
            rows: rows.len(),
            classified: classifications.len(),
        });
    }

    // A row is still classified and identified when its key is unreadable, but it is named once,
    // for the first value that failed in reading order: its key, its account, its
    // classification, then its identity, which is only asked of a row that is stored.
    let outcomes: Vec<Result<(RowOrderingKey, Outcome), RowError>> = rows
        .iter()
        .zip(keys)
        .zip(named)
        .zip(classifications)
        .map(|(((row, key), named), classification)| {
            let outcome = classification.and_then(|classification| match classification {
                RowClassification::NonPosition(_) => Ok(Outcome::NonPosition),
                RowClassification::DerivedAutomatically => importer
                    .identity(row)
                    .map(|identity| Outcome::Store(StoredAs::DerivedAutomatically, identity)),
                RowClassification::Pending => importer
                    .identity(row)
                    .map(|identity| Outcome::Store(StoredAs::Pending, identity)),
            });
            key.and_then(|key| named.and_then(|_| outcome.map(|outcome| (key, outcome))))
        })
        .collect();

    let failures: Vec<RowFailure> = outcomes
        .iter()
        .enumerate()
        .filter_map(|(position, outcome)| {
            outcome.as_ref().err().map(|error| RowFailure {
                position,
                error: error.clone(),
            })
        })
        .collect();
    if !failures.is_empty() {
        return Err(ImportError::FailedRows { failures });
    }

    // Every outcome is `Ok` past the refusal above, so flattening drops nothing and the keys
    // stay aligned with the rows.
    let (keys, outcomes): (Vec<RowOrderingKey>, Vec<Outcome>) =
        outcomes.into_iter().flatten().unzip();

    // Orders are assigned over every row the file holds, including the ones that will not be
    // stored, so that what a row's `order` is depends on the file alone and not on how its
    // neighbours were classified [DOM-040].
    let orders = assign_orders(&keys, importer.direction());

    let mut stored = Vec::new();
    let mut counts = ImportCounts::default();

    for ((row, order), outcome) in rows.iter().zip(orders).zip(outcomes) {
        match outcome {
            Outcome::NonPosition => counts.non_position += 1,
            Outcome::Store(stored_as, identity) => {
                stored.push(StoredRecord {
                    record: record(account, order, row, &identity),
                    stored_as,
                });
                match stored_as {
                    StoredAs::DerivedAutomatically => counts.derived += 1,
                    StoredAs::Pending => counts.pending += 1,
                }
            }
        }
    }

    Ok(Import {
        format: importer.format(),
        stored,
        counts,
    })
}

/// What one row came to, once its identity was needed.
///
/// The non-position kind is not carried: a batch counts non-position rows without distinguishing
/// them [DOM-046], and the kind is what the importer stated rather than something derived here.
enum Outcome {
    NonPosition,
    Store(StoredAs, RowIdentity),
}

fn record(
    account: &Account,
    order: Order,
    row: &SourceRow,
    identity: &RowIdentity,
) -> SourceRecord {
    let identity = match identity {
        RowIdentity::BrokerReference(reference) => {
            identify(account, &IdentitySource::BrokerReference(reference))
        }
        RowIdentity::ParsedFields(fields) => {
            let fields: Vec<&str> = fields.iter().map(String::as_str).collect();
            identify(account, &IdentitySource::ParsedFields(&fields))
        }
    };
    SourceRecord::new(identity, order, row.raw(), row.parsed())
}

/// The manual entry that records what the user supplied to complete `answered` [DOM-048].
///
/// Everything the user supplies becomes one of these, so that hand-entered information is as
/// traceable as imported information. The identities are read off the records being answered
/// [DOM-098], which is what stops an entry naming rows it was not shown.
#[must_use]
pub fn completion(
    account: Account,
    security: Isin,
    supplied: Supplied,
    answered: &[SourceRecord],
) -> ManualEntry {
    ManualEntry::new(
        account,
        security,
        supplied,
        answered.iter().map(|record| record.identity().clone()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decimal::Quantity;
    use crate::manual_entry::Election;
    use chrono::NaiveDate;
    use reader::DelimitedReader;
    use rust_decimal_macros::dec;
    use strum::EnumCount;

    /// A stand-in for a real importer: a comma-delimited file whose `date` is the trade date,
    /// whose `id` is the broker reference and whose `kind` column states the classification, so
    /// that the framework can be tested without either real format's rules.
    ///
    /// Both of a format's free choices are parameters here rather than constants, because each
    /// is a hook the framework could silently stop calling: the direction its file runs in, and
    /// whether it identifies a row by a broker reference or by its parsed fields.
    struct FakeImporter {
        reader: DelimitedReader,
        direction: FileDirection,
        identity: IdentityStyle,
    }

    /// What the fake offers to identify a row: the two cases [`RowIdentity`] has.
    #[derive(Clone, Copy)]
    enum IdentityStyle {
        BrokerReference,
        ParsedFields,
    }

    impl FakeImporter {
        fn new() -> Self {
            Self {
                reader: DelimitedReader::comma(),
                direction: FileDirection::OldestFirst,
                identity: IdentityStyle::BrokerReference,
            }
        }

        /// A format whose file runs newest first, as Saxo's does (IMP-SAXO-025).
        fn newest_first() -> Self {
            Self {
                direction: FileDirection::NewestFirst,
                ..Self::new()
            }
        }

        /// A format with no broker reference, which identifies a row by the fields it parsed.
        fn by_parsed_fields() -> Self {
            Self {
                identity: IdentityStyle::ParsedFields,
                ..Self::new()
            }
        }

        /// The fields this fake states as a row's identity, in the order it states them.
        fn identity_fields(row: &SourceRow) -> Vec<String> {
            ["date", "kind"]
                .iter()
                .map(|name| row.field(name).unwrap_or_default().to_owned())
                .collect()
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
            self.direction
        }

        fn identity(&self, row: &SourceRow) -> Result<RowIdentity, RowError> {
            match self.identity {
                IdentityStyle::ParsedFields => {
                    Ok(RowIdentity::ParsedFields(Self::identity_fields(row)))
                }
                IdentityStyle::BrokerReference => match row.field("id") {
                    Some(id) if !id.is_empty() => Ok(RowIdentity::BrokerReference(id.to_owned())),
                    _ => Err(RowError::new("no id")),
                },
            }
        }

        fn ordering_key(&self, row: &SourceRow) -> Result<RowOrderingKey, RowError> {
            let date = row.field("date").unwrap_or_default();
            let trade_date = NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .map_err(|_| RowError::new(format!("{date} is not a date")))?;
            Ok(RowOrderingKey {
                trade_date,
                columns: Vec::new(),
            })
        }

        fn classify(&self, rows: &[SourceRow]) -> Vec<Result<RowClassification, RowError>> {
            rows.iter()
                .map(|row| match row.field("kind") {
                    Some("buy") => Ok(RowClassification::DerivedAutomatically),
                    Some("split") => Ok(RowClassification::Pending),
                    Some("cash dividend") => Ok(RowClassification::NonPosition(
                        NonPositionKind::CashDividend,
                    )),
                    Some("interest") => {
                        Ok(RowClassification::NonPosition(NonPositionKind::Interest))
                    }
                    Some("deposit") => Ok(RowClassification::NonPosition(NonPositionKind::Deposit)),
                    Some("withdrawal") => {
                        Ok(RowClassification::NonPosition(NonPositionKind::Withdrawal))
                    }
                    Some("account fee") => {
                        Ok(RowClassification::NonPosition(NonPositionKind::AccountFee))
                    }
                    Some("stock dividend") => Ok(RowClassification::Pending),
                    other => Err(RowError::new(format!("unknown kind {other:?}"))),
                })
                .collect()
        }
    }

    const FILE: &str = "id,date,kind\n\
                        a,2024-01-02,buy\n\
                        b,2024-01-03,cash dividend\n\
                        c,2024-01-04,split\n";

    fn account() -> Account {
        Account::new("Trade Republic", "DE0001")
    }

    fn run(content: &str) -> Import {
        import(&FakeImporter::new(), &account(), content.as_bytes()).expect("the sample imports")
    }

    /// An import yields a source record per row it stores, carrying the row as the file held it
    /// [DOM-042], [DOM-007].
    #[test]
    fn an_import_yields_a_source_record_per_stored_row() {
        let import = run(FILE);

        assert_eq!(import.format(), SourceFormat::TradeRepublicDeCsv);
        assert_eq!(import.stored().len(), 2);
        assert_eq!(import.stored()[0].record().raw(), "a,2024-01-02,buy");
        assert_eq!(import.stored()[0].record().field("kind"), Some("buy"));
    }

    /// A row with everything present is derived automatically; one that affects holdings but
    /// lacks what only the user knows is pending, and pending is the completion queue
    /// [DOM-044], [DOM-045].
    #[test]
    fn rows_are_derived_or_pending_by_what_the_file_carries() {
        let import = run(FILE);

        assert_eq!(
            import.stored()[0].stored_as(),
            StoredAs::DerivedAutomatically
        );
        assert_eq!(import.stored()[1].stored_as(), StoredAs::Pending);
        let queue: Vec<&str> = import.pending().map(SourceRecord::raw).collect();
        assert_eq!(queue, ["c,2024-01-04,split"]);
        assert_eq!(import.counts().derived, 1);
        assert_eq!(import.counts().pending, 1);
    }

    /// A cash dividend is recognized as non-position: counted, and no record of it kept
    /// [DOM-002], [DOM-046].
    #[test]
    fn a_non_position_row_is_counted_and_not_stored() {
        let import = run(FILE);

        assert_eq!(import.counts().non_position, 1);
        assert!(
            import
                .stored()
                .iter()
                .all(|stored| stored.record().field("kind") != Some("cash dividend")),
            "a recognized non-position row must leave no source record"
        );
    }

    /// The framework discards a row on its classification alone, never on the word dividend:
    /// two rows that both say dividend diverge because the importer classified them differently
    /// [DOM-124]. That a share-issuing dividend's buy carries the stock-dividend origin is
    /// asserted where that buy is built, in `transaction.rs`; recognizing such a row is a
    /// format's heuristic (IMP-SAXO-018, FIF-025).
    #[test]
    fn a_dividend_row_is_discarded_by_its_classification_and_not_by_its_word() {
        let content = "id,date,kind\n\
                       d,2024-02-01,cash dividend\n\
                       e,2024-02-02,stock dividend\n";

        let import = run(content);

        assert_eq!(import.counts().non_position, 1);
        assert_eq!(import.counts().pending, 1);
        let stored: Vec<&str> = import
            .stored()
            .iter()
            .map(|stored| stored.record().raw())
            .collect();
        assert_eq!(stored, ["e,2024-02-02,stock dividend"]);
    }

    /// The taxonomy is the three outcomes of DOM-043, and an import reaches every one of them
    /// [DOM-043].
    ///
    /// A fourth variant fails this test three ways: `RowClassification::COUNT` no longer
    /// matches the outcomes an import counts, `slot` stops compiling until the variant is
    /// named, and there is no fourth slot for it to take. A classification nothing reaches is
    /// an unamended `domain.md`, not a new outcome.
    #[test]
    fn the_classification_taxonomy_is_the_three_outcomes_of_dom_043() {
        let import = run(FILE);
        // One slot per outcome of DOM-043, holding what the import path itself reached.
        let reached = [
            import.counts().derived,
            import.counts().pending,
            import.counts().non_position,
        ];

        assert_eq!(
            RowClassification::COUNT,
            reached.len(),
            "the classification set has grown past the outcomes an import reaches"
        );

        // The only route from a classification into those slots, and exhaustive over the enum.
        let slot = |classification: &RowClassification| match classification {
            RowClassification::DerivedAutomatically => 0,
            RowClassification::Pending => 1,
            // The payload is irrelevant here; the five kinds are their own test [DOM-002].
            RowClassification::NonPosition(_) => 2,
        };

        let importer = FakeImporter::new();
        let rows = importer
            .reader()
            .rows(FILE.as_bytes())
            .expect("the sample reads");
        let mut answered = [0; 3];
        for classification in importer.classify(&rows) {
            answered[slot(&classification.expect("the sample classifies"))] += 1;
        }

        assert_eq!(
            answered, reached,
            "each classification an importer answers must reach its own outcome of the import"
        );
        assert!(
            reached.iter().all(|count| *count > 0),
            "an outcome no row reaches is a taxonomy member nothing maps to: {reached:?}"
        );
    }

    /// The non-position kinds are the five `domain.md` names and no others [DOM-002].
    #[test]
    fn the_non_position_kinds_are_the_specified_five() {
        let all = [
            NonPositionKind::CashDividend,
            NonPositionKind::Interest,
            NonPositionKind::Deposit,
            NonPositionKind::Withdrawal,
            NonPositionKind::AccountFee,
        ];

        // Exhaustive by construction: a new variant makes this match fail to compile.
        for kind in all {
            match kind {
                NonPositionKind::CashDividend
                | NonPositionKind::Interest
                | NonPositionKind::Deposit
                | NonPositionKind::Withdrawal
                | NonPositionKind::AccountFee => {}
            }
        }
    }

    /// Every one of the five kinds is counted and left unstored, not only the dividend
    /// [DOM-002], [DOM-046].
    #[test]
    fn a_row_of_each_non_position_kind_is_counted_and_not_stored() {
        let content = "id,date,kind\n\
                       a,2024-01-02,cash dividend\n\
                       b,2024-01-03,interest\n\
                       c,2024-01-04,deposit\n\
                       d,2024-01-05,withdrawal\n\
                       e,2024-01-06,account fee\n";

        let import = run(content);

        assert_eq!(import.counts().non_position, 5);
        assert!(import.stored().is_empty());
    }

    /// Order comes from the file's own content, so a row's order does not move because a row
    /// beside it was discarded [DOM-040]: the pending row is the file's third and keeps the
    /// third position, the discarded dividend included.
    #[test]
    fn discarding_a_row_does_not_renumber_the_rest() {
        let import = run(FILE);

        let orders: Vec<u32> = import
            .stored()
            .iter()
            .map(|stored| stored.record().order().get())
            .collect();
        assert_eq!(orders, [0, 2]);
    }

    /// The importer's file direction decides the order, so a newest-first file is numbered from
    /// its last row back [DOM-040]. The rows share a trade date deliberately: with distinct
    /// dates the date dominates and both directions produce the same orders, so only a same-day
    /// file can tell whether the direction was consulted at all.
    #[test]
    fn the_file_direction_decides_which_end_of_the_file_is_oldest() {
        let content = "id,date,kind\n\
                       a,2024-01-02,buy\n\
                       b,2024-01-02,cash dividend\n\
                       c,2024-01-02,split\n";

        let orders = |importer: &dyn Importer| -> Vec<u32> {
            import(importer, &account(), content.as_bytes())
                .expect("the sample imports")
                .stored()
                .iter()
                .map(|stored| stored.record().order().get())
                .collect()
        };

        assert_eq!(orders(&FakeImporter::new()), [0, 2]);
        assert_eq!(orders(&FakeImporter::newest_first()), [2, 0]);
    }

    /// A format with no broker reference identifies a row by the fields it states, scoped to the
    /// account like any other identity [DOM-023], [DOM-024], and reproduces it on a re-import.
    #[test]
    fn a_row_is_identified_by_its_parsed_fields_when_the_format_has_no_reference() {
        let importer = FakeImporter::by_parsed_fields();

        let import = import(&importer, &account(), FILE.as_bytes()).expect("the sample imports");

        let expected = identify(
            &account(),
            &IdentitySource::ParsedFields(&["2024-01-02", "buy"]),
        );
        assert_eq!(import.stored()[0].record().identity(), &expected);
        assert_ne!(
            import.stored()[0].record().identity(),
            run(FILE).stored()[0].record().identity(),
            "a parsed-fields identity is not the broker-reference one for the same row"
        );
        assert_eq!(
            import,
            self::import(&importer, &account(), FILE.as_bytes()).expect("the sample imports"),
            "a re-import reproduces the identity, which is what makes it add nothing"
        );
    }

    /// A file carrying a header and no rows is an ordinary case — a year with no transactions —
    /// and yields nothing, not a failure [DOM-042], [DOM-046].
    #[test]
    fn a_file_with_no_data_rows_yields_nothing() {
        let import = run("id,date,kind\n");

        assert!(import.stored().is_empty());
        assert_eq!(import.pending().count(), 0);
        assert_eq!(import.counts(), ImportCounts::default());
    }

    /// Identity is scoped to the account, so the same file imported into two accounts yields
    /// distinct records [DOM-024].
    #[test]
    fn identity_is_scoped_to_the_account() {
        let here = run(FILE);
        let elsewhere = import(
            &FakeImporter::new(),
            &Account::new("Trade Republic", "DE0002"),
            FILE.as_bytes(),
        )
        .expect("the sample imports");

        assert_ne!(
            here.stored()[0].record().identity(),
            elsewhere.stored()[0].record().identity()
        );
    }

    /// Re-reading the same file produces the same identities and the same raw content, which is
    /// what makes a re-import add nothing [DOM-022], [DOM-120].
    #[test]
    fn re_importing_the_same_file_reproduces_the_same_records() {
        assert_eq!(run(FILE), run(FILE));
    }

    /// Failed rows of all three kinds among good rows — two whose ordering key cannot be read,
    /// one that cannot be classified and one that cannot be identified — refuse the whole
    /// import, and the refusal names every one by position with its reason rather than stopping
    /// at the first [SRV-058] (DEC-074, DEC-087). No `Import` is returned, so nothing the good
    /// rows yielded can be stored.
    #[test]
    fn every_failed_row_refuses_the_import_and_is_named() {
        let content = "id,date,kind\n\
                       a,2024-01-02,buy\n\
                       b,not-a-date,buy\n\
                       c,2024-01-03,nonsense\n\
                       d,2024-01-04,cash dividend\n\
                       ,2024-01-05,buy\n\
                       f,,split\n\
                       g,2024-01-06,split\n";

        let error = import(&FakeImporter::new(), &account(), content.as_bytes())
            .expect_err("a failed row refuses the file");

        assert_eq!(
            error,
            ImportError::FailedRows {
                failures: vec![
                    RowFailure {
                        position: 1,
                        error: RowError::new("not-a-date is not a date"),
                    },
                    RowFailure {
                        position: 2,
                        error: RowError::new("unknown kind Some(\"nonsense\")"),
                    },
                    RowFailure {
                        position: 4,
                        error: RowError::new("no id"),
                    },
                    RowFailure {
                        position: 5,
                        error: RowError::new(" is not a date"),
                    },
                ],
            }
        );
        assert_eq!(
            error.to_string(),
            "4 rows could not be read: row 1: not-a-date is not a date; \
             row 2: unknown kind Some(\"nonsense\"); row 4: no id; row 5:  is not a date"
        );
    }

    /// A row failing on more than one value is named once, for its ordering key, which is read
    /// first; the refusal is per row, not per value [SRV-058].
    #[test]
    fn a_row_failing_twice_is_named_once() {
        let content = "id,date,kind\n\
                       ,not-a-date,nonsense\n";

        let error = import(&FakeImporter::new(), &account(), content.as_bytes())
            .expect_err("a failed row refuses the file");

        assert_eq!(
            error,
            ImportError::FailedRows {
                failures: vec![RowFailure {
                    position: 0,
                    error: RowError::new("not-a-date is not a date"),
                }],
            }
        );
    }

    /// An importer answering the wrong number of rows is refused rather than zipped to the
    /// shorter of the two, on either side of the count: too few and too many both lose the
    /// correspondence between row and classification.
    #[test]
    fn a_classification_per_row_is_required() {
        struct MiscountingImporter {
            reader: DelimitedReader,
            classifications: usize,
        }

        impl Importer for MiscountingImporter {
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
                FakeImporter::new().identity(row)
            }
            fn ordering_key(&self, row: &SourceRow) -> Result<RowOrderingKey, RowError> {
                FakeImporter::new().ordering_key(row)
            }
            fn classify(&self, _rows: &[SourceRow]) -> Vec<Result<RowClassification, RowError>> {
                vec![Ok(RowClassification::DerivedAutomatically); self.classifications]
            }
        }

        let refusal = |classifications: usize| {
            import(
                &MiscountingImporter {
                    reader: DelimitedReader::comma(),
                    classifications,
                },
                &account(),
                FILE.as_bytes(),
            )
            .expect_err("a miscounted classification is refused")
        };

        assert_eq!(
            refusal(1),
            ImportError::ClassificationCount {
                rows: 3,
                classified: 1
            }
        );
        assert_eq!(
            refusal(4),
            ImportError::ClassificationCount {
                rows: 3,
                classified: 4
            }
        );
    }

    /// A file that cannot be read at all is a whole-file error, not a row failure [ARC-023].
    #[test]
    fn an_unreadable_file_refuses_the_import() {
        let error = import(&FakeImporter::new(), &account(), &[0xff, 0xfe])
            .expect_err("invalid UTF-8 refuses the file");

        assert!(matches!(error, ImportError::Read(_)), "{error:?}");
    }

    /// What the user supplies becomes a manual entry naming the records it answers, and
    /// nothing else [DOM-048], [DOM-098].
    #[test]
    fn what_the_user_supplies_becomes_a_manual_entry() {
        let import = run(FILE);
        let answered: Vec<SourceRecord> = import.pending().cloned().collect();

        let entry = completion(
            account(),
            Isin::new("NL0000009538"),
            Supplied::Election(Election::Stock {
                shares: Quantity::new(dec!(3)),
            }),
            &answered,
        );

        assert_eq!(entry.account(), &account());
        assert_eq!(entry.security(), &Isin::new("NL0000009538"));
        assert_eq!(
            entry.answers(),
            [answered[0].identity().clone()],
            "an entry names the records it answers by their broker identity"
        );
    }

    /// The identities are read off every record answered, in the order given, and off nothing
    /// else: an entry answering two records names both, and one answering none names none
    /// [DOM-048], [DOM-098].
    #[test]
    fn an_entry_names_every_record_it_answers_and_no_other() {
        let content = "id,date,kind\n\
                       a,2024-01-02,split\n\
                       b,2024-01-03,split\n";
        let import = run(content);
        let answered: Vec<SourceRecord> = import.pending().cloned().collect();
        let entry = |answered: &[SourceRecord]| {
            completion(
                account(),
                Isin::new("NL0000009538"),
                Supplied::Election(Election::Stock {
                    shares: Quantity::new(dec!(3)),
                }),
                answered,
            )
        };

        assert_eq!(answered.len(), 2);
        assert_eq!(
            entry(&answered).answers(),
            [
                answered[0].identity().clone(),
                answered[1].identity().clone()
            ]
        );
        assert!(entry(&[]).answers().is_empty());
    }
}
