//! The reference entities: accounts, securities, source records and import batches.
//!
//! Types only. Nothing here persists, derives or computes; the rules that populate these fields
//! live with the items that own them.
//!
//! # Two things worth knowing
//!
//! [DOM-008] — a source record is never edited after creation — is enforced by construction:
//! [`SourceRecord`]'s fields are private, there is no setter and no `&mut` accessor. That is a
//! compile-time property, so no runtime test names DOM-008 and the absence of one is deliberate
//! rather than an omission.
//!
//! [`SourceRecord::raw`] is a `String`, which suits a CSV line. An XLSX row is a set of cells
//! and has no verbatim string form, so the Saxo importer will have to synthesize one — and a
//! synthesized line is the importer's invention, not the row verbatim. [DOM-007] calls raw
//! content an audit trail, so that item owes a rule for what it writes there.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};

/// A broker account, which is one Depot.
///
/// Where a broker splits a Depot into per-currency sub-accounts, the importer normalizes them
/// onto one of these, because FIFO applies per Depot [DOM-003].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Account {
    broker: String,
    id: String,
}

impl Account {
    #[must_use]
    pub fn new(broker: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            broker: broker.into(),
            id: id.into(),
        }
    }

    #[must_use]
    pub fn broker(&self) -> &str {
        &self.broker
    }

    /// The broker's own identifier for the account, with any per-currency suffix already
    /// stripped [DOM-003].
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
}

/// An ISIN, a security's natural key.
///
/// Not validated here: no requirement asks for a check digit, and a broker file that names an
/// instrument is the authority on what it is called. Trimmed and uppercased on construction,
/// as a [`crate::valuation::Currency`] is, because the ISIN is a security's uniqueness key
/// [DOM-071] and storage compares it byte for byte: ` nl0000009538` reaching a column as a
/// second spelling would be a second security for one instrument.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Isin(String);

impl Isin {
    #[must_use]
    pub fn new(value: impl AsRef<str>) -> Self {
        Self(value.as_ref().trim().to_ascii_uppercase())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What kind of instrument a security is [DOM-004].
///
/// It drives the default quotation and nothing else in this version: Teilfreistellung, where
/// `etf` against `fund` would matter, is an explicit non-goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SecurityType {
    Stock,
    Bond,
    Etf,
    Fund,
    Derivative,
    Other,
}

/// How a security's price is expressed [DOM-005].
///
/// A stock is quoted per unit, so a trade value is quantity times price. A bond is quoted as a
/// percentage of par, where the quantity is a nominal amount: `3000 @ 139.46` is a nominal 3000
/// at 139.46% of face, costing 4183.80 and not 418,380.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Quotation {
    /// Price is per unit. The default, and right for everything but a bond.
    #[default]
    PerUnit,
    /// Price is a percentage of par.
    PercentOfPar,
}

/// A tradable instrument, keyed by its ISIN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Security {
    isin: Isin,
    name: String,
    security_type: SecurityType,
    quotation: Quotation,
    auto_created: bool,
    needs_review: bool,
}

impl Security {
    /// A security the user entered, with its quotation stated.
    #[must_use]
    pub fn new(
        isin: Isin,
        name: impl Into<String>,
        security_type: SecurityType,
        quotation: Quotation,
    ) -> Self {
        Self {
            isin,
            name: name.into(),
            security_type,
            quotation,
            auto_created: false,
            needs_review: false,
        }
    }

    /// A security an importer created from a file, flagged as auto-created [DOM-006] and as
    /// needing review, so the user can check and correct it [DOM-126].
    ///
    /// The defaulting of `quotation` from the broker's instrument type belongs to FIF-055; this
    /// constructor takes whatever the caller decided.
    #[must_use]
    pub fn auto_created(
        isin: Isin,
        name: impl Into<String>,
        security_type: SecurityType,
        quotation: Quotation,
    ) -> Self {
        Self {
            auto_created: true,
            needs_review: true,
            ..Self::new(isin, name, security_type, quotation)
        }
    }

    /// A security as storage holds it, with both flags as stored. Needs review is independent
    /// of provenance [DOM-126], so every combination is a security: only imports set it
    /// [SRV-014], but nothing in the specification makes a user-entered one needing review
    /// invalid. Crate-private so that, outside storage, [`Self::reviewed`] stays the only way
    /// the flag is cleared.
    #[must_use]
    pub(crate) fn stored(
        isin: Isin,
        name: impl Into<String>,
        security_type: SecurityType,
        quotation: Quotation,
        auto_created: bool,
        needs_review: bool,
    ) -> Self {
        Self {
            auto_created,
            needs_review,
            ..Self::new(isin, name, security_type, quotation)
        }
    }

    #[must_use]
    pub fn isin(&self) -> &Isin {
        &self.isin
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn security_type(&self) -> SecurityType {
        self.security_type
    }

    #[must_use]
    pub fn quotation(&self) -> Quotation {
        self.quotation
    }

    /// Whether an importer created this record rather than the user [DOM-006].
    #[must_use]
    pub fn is_auto_created(&self) -> bool {
        self.auto_created
    }

    /// Whether the user has yet to mark this security reviewed [DOM-126].
    #[must_use]
    pub fn needs_review(&self) -> bool {
        self.needs_review
    }

    /// The same security marked reviewed by the user [SRV-057].
    ///
    /// The only way `needs_review` is cleared: [`Self::with_quotation`] and
    /// [`Self::with_security_type`] leave it as it is, because correcting a security is not the
    /// user saying it is right [DOM-126]. Provenance is untouched [DOM-006].
    #[must_use]
    pub fn reviewed(self) -> Self {
        Self {
            needs_review: false,
            ..self
        }
    }

    /// The same security with a different quotation [DOM-037].
    ///
    /// Quotation is kept separate from type precisely so a mis-quoted instrument can be
    /// corrected without reclassifying what it is.
    #[must_use]
    pub fn with_quotation(self, quotation: Quotation) -> Self {
        Self { quotation, ..self }
    }

    /// The same security with a different type, its quotation untouched.
    ///
    /// Here because an auto-created record is flagged for the user to review and correct
    /// [DOM-006], and because it is what shows that type and quotation move independently
    /// [DOM-037]. The endpoint that exposes it is FIF-034's.
    #[must_use]
    pub fn with_security_type(self, security_type: SecurityType) -> Self {
        Self {
            security_type,
            ..self
        }
    }
}

/// What a source record is identified by.
///
/// An opaque carrier. Which value goes in — a broker's own reference where the format has a
/// stable one, otherwise a hash of the parsed business fields — and the account scoping that
/// keeps identical rows in different accounts distinct, both belong to FIF-007. This type
/// decides neither, and satisfies neither requirement on its own.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordIdentity(String);

impl RecordIdentity {
    /// Crate-visible on purpose: `identity::identify` is the only way to obtain one from
    /// outside, so an importer cannot build an unscoped identity by hand [DOM-024]. Storage
    /// reads one back through the same constructor.
    #[must_use]
    pub(crate) fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A record's position in the file it was read from.
///
/// How it is computed belongs to the item that owns ordering; this type carries the answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Order(u32);

impl Order {
    #[must_use]
    pub fn new(position: u32) -> Self {
        Self(position)
    }

    #[must_use]
    pub fn get(self) -> u32 {
        self.0
    }
}

/// A broker export format.
///
/// Present because an import batch records the format it read [DOM-017]. Which formats are
/// supported is SRV-013, owned by FIF-035; this enum will close that set rather than define it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceFormat {
    SaxoNlXlsx,
    TradeRepublicDeCsv,
}

/// One parsed row of a broker export, kept verbatim beside its parsed fields [DOM-007].
///
/// Never edited after creation [DOM-008], which is why it has no setters and no public fields: a
/// mistake is corrected by deleting what was derived from it and importing again, not by
/// changing the record of what the file said.
///
/// The parsed fields are an ordered map so that a hash taken over them is deterministic. Their
/// names are the format's own column names, which keeps a record readable next to the file it
/// came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRecord {
    identity: RecordIdentity,
    order: Order,
    raw: String,
    parsed: BTreeMap<String, String>,
}

impl SourceRecord {
    #[must_use]
    pub fn new(
        identity: RecordIdentity,
        order: Order,
        raw: impl Into<String>,
        parsed: BTreeMap<String, String>,
    ) -> Self {
        Self {
            identity,
            order,
            raw: raw.into(),
            parsed,
        }
    }

    #[must_use]
    pub fn identity(&self) -> &RecordIdentity {
        &self.identity
    }

    #[must_use]
    pub fn order(&self) -> Order {
        self.order
    }

    /// The row exactly as the file held it [DOM-007].
    #[must_use]
    pub fn raw(&self) -> &str {
        &self.raw
    }

    #[must_use]
    pub fn parsed(&self) -> &BTreeMap<String, String> {
        &self.parsed
    }

    /// One parsed field, by the file's own name for it.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&str> {
        self.parsed.get(name).map(String::as_str)
    }
}

/// What an import did, counted by how each row was classified [DOM-043].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ImportCounts {
    /// Rows that became a transaction directly.
    pub derived: u32,
    /// Rows that affect holdings but need something the file does not carry.
    pub pending: u32,
    /// Rows recognized as carrying no position effect, and so not stored.
    pub non_position: u32,
    // No failed count: a row that fails to parse refuses the import, so no batch has one
    // [SRV-058] (DEC-074).
}

/// One import of one file into one account, so that an import can be undone as a unit
/// [DOM-017].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportBatch {
    account: Account,
    filename: String,
    format: SourceFormat,
    imported_at: DateTime<Utc>,
    counts: ImportCounts,
}

impl ImportBatch {
    #[must_use]
    pub fn new(
        account: Account,
        filename: impl Into<String>,
        format: SourceFormat,
        imported_at: DateTime<Utc>,
        counts: ImportCounts,
    ) -> Self {
        Self {
            account,
            filename: filename.into(),
            format,
            imported_at,
            counts,
        }
    }

    #[must_use]
    pub fn account(&self) -> &Account {
        &self.account
    }

    #[must_use]
    pub fn filename(&self) -> &str {
        &self.filename
    }

    #[must_use]
    pub fn format(&self) -> SourceFormat {
        self.format
    }

    #[must_use]
    pub fn imported_at(&self) -> DateTime<Utc> {
        self.imported_at
    }

    #[must_use]
    pub fn counts(&self) -> ImportCounts {
        self.counts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn isin() -> Isin {
        Isin::new("NL0000102077")
    }

    /// One instrument is one key: a variant spelling normalizes to the same ISIN, so the
    /// uniqueness constraint over it cannot be sidestepped by case or stray whitespace
    /// [DOM-071].
    #[test]
    fn an_isin_is_trimmed_and_uppercased() {
        assert_eq!(Isin::new(" nl0000102077 "), isin());
        assert_eq!(Isin::new("NL0000102077").as_str(), "NL0000102077");
    }

    /// The security type enum is the fixed set the specification names, and nothing else
    /// [DOM-004].
    #[test]
    fn the_security_type_enum_is_the_specified_set() {
        let all = [
            SecurityType::Stock,
            SecurityType::Bond,
            SecurityType::Etf,
            SecurityType::Fund,
            SecurityType::Derivative,
            SecurityType::Other,
        ];

        // Exhaustive by construction: a new variant makes this match fail to compile.
        for kind in all {
            match kind {
                SecurityType::Stock
                | SecurityType::Bond
                | SecurityType::Etf
                | SecurityType::Fund
                | SecurityType::Derivative
                | SecurityType::Other => {}
            }
        }
        assert_eq!(all.len(), 6);
    }

    /// Quotation is a two-value enum whose default is per unit [DOM-005].
    #[test]
    fn quotation_defaults_to_per_unit() {
        assert_eq!(Quotation::default(), Quotation::PerUnit);

        let both = [Quotation::PerUnit, Quotation::PercentOfPar];
        for quotation in both {
            match quotation {
                Quotation::PerUnit | Quotation::PercentOfPar => {}
            }
        }
        assert_eq!(both.len(), 2);
    }

    /// Quotation is editable, and editing it leaves the type alone [DOM-037].
    #[test]
    fn quotation_is_editable_independently_of_type() {
        let security = Security::new(
            isin(),
            "NL 7.5% 2023",
            SecurityType::Bond,
            Quotation::PerUnit,
        );

        let corrected = security.clone().with_quotation(Quotation::PercentOfPar);

        assert_eq!(corrected.quotation(), Quotation::PercentOfPar);
        assert_eq!(
            corrected.security_type(),
            security.security_type(),
            "correcting a quotation must not reclassify the instrument"
        );
        assert_eq!(corrected.isin(), security.isin());
        assert_eq!(corrected.name(), security.name());
    }

    /// And the converse: changing the type leaves the quotation alone [DOM-037].
    #[test]
    fn type_is_editable_independently_of_quotation() {
        let security = Security::new(isin(), "Some fund", SecurityType::Fund, Quotation::PerUnit);

        let corrected = security.clone().with_security_type(SecurityType::Etf);

        assert_eq!(corrected.security_type(), SecurityType::Etf);
        assert_eq!(corrected.quotation(), Quotation::PerUnit);
        assert_eq!(corrected.isin(), security.isin());
        assert_eq!(corrected.name(), security.name());
    }

    /// A type and a quotation that disagree are representable, because a bond quoted per unit
    /// is a real possibility the model must not rule out [DOM-005], [DOM-037].
    #[test]
    fn a_bond_may_be_quoted_per_unit() {
        let security = Security::new(
            isin(),
            "An odd bond",
            SecurityType::Bond,
            Quotation::PerUnit,
        );

        assert_eq!(security.security_type(), SecurityType::Bond);
        assert_eq!(security.quotation(), Quotation::PerUnit);
    }

    /// A security an importer made is flagged; one the user made is not [DOM-006].
    #[test]
    fn an_auto_created_security_is_flagged() {
        let by_user = Security::new(isin(), "Philips", SecurityType::Stock, Quotation::PerUnit);
        let by_import =
            Security::auto_created(isin(), "Philips", SecurityType::Stock, Quotation::PerUnit);

        assert!(!by_user.is_auto_created());
        assert!(by_import.is_auto_created());
    }

    /// Editing an auto-created security does not clear its flag: the flag records where the
    /// record came from, not whether it has been looked at [DOM-006].
    #[test]
    fn editing_does_not_clear_the_auto_created_flag() {
        let security =
            Security::auto_created(isin(), "A bond", SecurityType::Bond, Quotation::PerUnit)
                .with_quotation(Quotation::PercentOfPar);

        assert!(security.is_auto_created());
    }

    /// An imported security needs review and a user-entered one does not; correcting type and
    /// quotation leaves it needing review, and only marking it reviewed clears it, provenance
    /// untouched [DOM-126], [DOM-006].
    #[test]
    fn only_marking_reviewed_clears_needs_review() {
        let by_user = Security::new(isin(), "Philips", SecurityType::Stock, Quotation::PerUnit);
        assert!(!by_user.needs_review());

        let corrected =
            Security::auto_created(isin(), "A bond", SecurityType::Other, Quotation::PerUnit)
                .with_security_type(SecurityType::Bond)
                .with_quotation(Quotation::PercentOfPar);
        assert!(corrected.needs_review());

        let reviewed = corrected.reviewed();
        assert!(!reviewed.needs_review());
        assert!(reviewed.is_auto_created());
        assert_eq!(
            (reviewed.security_type(), reviewed.quotation()),
            (SecurityType::Bond, Quotation::PercentOfPar)
        );
    }

    /// A source record keeps the row verbatim beside its parsed fields [DOM-007].
    #[test]
    fn a_source_record_keeps_the_row_and_its_fields() {
        let raw = "\"2024-05-02\",\"BUY\",\"IE000Y77LGG9\",\"55\"";
        let parsed = BTreeMap::from([
            ("date".to_owned(), "2024-05-02".to_owned()),
            ("type".to_owned(), "BUY".to_owned()),
        ]);

        let record = SourceRecord::new(
            RecordIdentity::new("c016ee6f-7c1e-4a8b-a39c-f7d253fe1841"),
            Order::new(12),
            raw,
            parsed,
        );

        assert_eq!(record.raw(), raw);
        assert_eq!(record.field("type"), Some("BUY"));
        assert_eq!(record.field("absent"), None);
        assert_eq!(record.order().get(), 12);
        assert_eq!(
            record.identity().as_str(),
            "c016ee6f-7c1e-4a8b-a39c-f7d253fe1841"
        );
    }

    /// Parsed fields iterate in a stable order, whatever order they were inserted in, so
    /// anything computed over them is deterministic. Identity itself belongs to FIF-007.
    #[test]
    fn parsed_fields_have_a_stable_order() {
        let one = BTreeMap::from([
            ("b".to_owned(), "2".to_owned()),
            ("a".to_owned(), "1".to_owned()),
        ]);
        let other = BTreeMap::from([
            ("a".to_owned(), "1".to_owned()),
            ("b".to_owned(), "2".to_owned()),
        ]);

        let keys = |map: BTreeMap<String, String>| {
            SourceRecord::new(RecordIdentity::new("x"), Order::new(0), "", map)
                .parsed()
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        };

        assert_eq!(keys(one), keys(other));
    }

    /// A batch records what it imported and how each row was classified [DOM-017], [DOM-043].
    #[test]
    fn a_batch_records_its_file_and_its_counts() {
        let counts = ImportCounts {
            derived: 7,
            pending: 2,
            non_position: 48,
        };
        let at = DateTime::from_timestamp(1_714_608_000, 0).expect("a valid timestamp");

        let batch = ImportBatch::new(
            Account::new("Saxo", "69900/1000000"),
            "Transactions_2024.xlsx",
            SourceFormat::SaxoNlXlsx,
            at,
            counts,
        );

        assert_eq!(batch.account().broker(), "Saxo");
        assert_eq!(batch.account().id(), "69900/1000000");
        assert_eq!(batch.filename(), "Transactions_2024.xlsx");
        assert_eq!(batch.format(), SourceFormat::SaxoNlXlsx);
        assert_eq!(batch.imported_at(), at);
        assert_eq!(batch.counts(), counts);
    }

    /// The counts start at zero, so a batch that classified nothing says so rather than
    /// leaving a field unset [DOM-017].
    #[test]
    fn counts_default_to_zero() {
        let counts = ImportCounts::default();

        assert_eq!(counts.derived, 0);
        assert_eq!(counts.pending, 0);
        assert_eq!(counts.non_position, 0);
    }
}
