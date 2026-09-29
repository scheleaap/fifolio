//! Everything that happens to a holding: the transaction sum type and its six variants
//! [DOM-010].
//!
//! Types only. Nothing here persists, derives or computes.
//!
//! # Why a sum type, and why it is nested
//!
//! Each variant carries only the fields it has, so a variant can never be read as another
//! [DOM-010]: there is no shared struct with a kind tag and no optional field standing in for
//! "not applicable here". A `split` has no quantity to read, and an `expiration` has no unit
//! price, because neither type has the field at all.
//!
//! [DOM-081] groups the variants by what they do to holdings: `buy` and `transfer_in` **open** a
//! parcel, `sell`, `expiration` and `transfer_out` **close** one, and `split` does neither. That
//! grouping is the nesting of [`Transaction`] over [`Opening`], [`Closing`] and [`Split`] rather
//! than a method returning a label, so the FIFO engine can take an `&Opening` and be unable to
//! receive a split at all.
//!
//! # What every variant carries
//!
//! A trade date, the source records the transaction was derived from [DOM-016], and its place in
//! the canonical order [DOM-011] — together, its [`Derivation`]. The records are named by their
//! [`RecordIdentity`]; the relations to account, security and those records [DOM-013] are held by
//! storage, as a source record's relation to its batch is, and written with the transaction.
//!
//! The place is an [`OrderKey`]: the trade date, the [`RecordPosition`] of the lowest record the
//! transaction was derived from (DEC-090, DEC-094, provisional), and a [`Leg`] that lets a
//! decomposition's `transfer_out` sort immediately after its `sell`. Until consumption is
//! distinguished from citation (FIF-058), every record derived from counts, which is the same set
//! for every transaction built so far.
//!
//! # Nothing is created from nothing
//!
//! A [`Derivation`] is made from at least one [`RecordHandle`], by type: its one constructor
//! takes a [`Vec1`] of them [DOM-047]. The `Vec1` rules out a derivation citing nothing; the
//! handle rules out one citing a record that does not exist, since only storage issues handles
//! and it issues them for records it holds. A record identity will not do, because
//! [`crate::identity::identify`] computes one for any row, read or not. Every variant
//! constructor takes a `Derivation`, so this is the one gate every construction path passes,
//! whatever surface it starts at, with no check to forget. A `transfer_in` emitted on approval
//! of a `transfer_out` is no exception: it cites the `transfer_out`'s records [DEC-079].
//!
//! A handle proves the record was stored when it was issued, not that it still is: an import
//! undo may have removed it since, or the handle may come from another database. Storing the
//! transaction therefore refuses a citation of a record that is not stored at that moment
//! [DOM-047]; what keeps a stored transaction's citations standing afterwards is the import
//! undo's refusal [DOM-119].
//!
//! The distinction between **consuming** a record and merely **citing** it [DOM-101] is
//! undecided (FIF-058). What every variant holds until then is the citation — the audit trail
//! DOM-016 asks for — and which of those records is consumed is added on top, not carved out.
//!
//! # `fees` is one field
//!
//! Commission, exchange fees and transaction taxes such as the French FTT or stamp duty are
//! summed into a single `fees` figure [DOM-012]. They receive identical treatment in the gain
//! calculation, so modeling them separately would buy nothing and would invite a formula that
//! deducts one and forgets another. The caller sums them; this type holds the total.
//!
//! # A buy's origin
//!
//! A `buy` says how it arose [DOM-082]. An ordinary purchase has a cost basis it paid for; shares
//! issued as a stock dividend paid nothing, so their basis is their taxable value at issue — and
//! that value **is** the buy's EUR gross, held as one stored figure [DOM-123]. The origin carries
//! no figure of its own, so a basis differing from the total every calculation reads is not a
//! state this type can be in. Nothing here computes the value either: the gross is what the
//! caller booked, off a statement or from the user, which is what keeps it sourced. Where the
//! Saxo importer will find it — along with the share count itself — is OQ-014, still open; that
//! question blocks IMP-SAXO-013, not this type.
//!
//! # Every money figure is a native/EUR pair
//!
//! A transaction stores what the statement booked *and* what it is worth in EUR, with the rate
//! that relates them, its source and its date [DOM-028] — the [`Conversion`] every
//! money-bearing variant carries. Each figure is a [`Valued`] pair, so the two halves cannot
//! drift to different scales [DOM-029] and an allocation reads the EUR half exactly as it
//! reads the native one [DOM-084].
//!
//! The date those figures are valued at is the trade date on the [`Derivation`], never a
//! settlement date [DOM-027]; [`Transaction::valuation_date`] is where that is said once.
//! Nothing here holds a currency gain: the movement of the currency between acquisition and
//! disposal is inside the securities gain [DOM-026].
//!
//! Every variant that opens or closes for cash also stores its **gross total**, not only its
//! unit price [DOM-085]. Whether a calculation may read the unit price at all is DOM-104,
//! undecided, and belongs to FIF-077; what this module fixes is that the total is there to be
//! read.
//!
//! # Fields deliberately absent
//!
//! Each is owned by another item, and inventing it here would fix a rule this item does not
//! decide:
//!
//! | Absent | Variant | Owner |
//! | --- | --- | --- |
//! | the EUR gross | `transfer_out` | FIF-080 (DOM-112): it is derived from its own allocations, not stored |
//! | quantity | `expiration` | FIF-079 (DOM-092): it is the unattributed remainder, not a stated figure |
//!
//! The money fields present are the figures the variant states, at the scales of
//! [`crate::decimal`].

use chrono::NaiveDate;
use vec1::Vec1;

use crate::decimal::{Money, Quantity, QuotedPrice};
use crate::entities::{Isin, RecordIdentity};
use crate::manual_entry::Ratio;
use crate::ordering::{Leg, OrderKey, RecordPosition};
use crate::storage::RecordHandle;
use crate::valuation::{Conversion, Valued};

/// What every variant carries: its trade date, the records it was derived from [DOM-016], and
/// its place in the canonical order [DOM-011].
///
/// The trade date is the date of the obligating transaction and never the settlement date
/// [DOM-027]; which column of an export supplies it is each importer's item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Derivation {
    trade_date: NaiveDate,
    cites: Vec1<RecordIdentity>,
    position: RecordPosition,
    leg: Leg,
}

impl Derivation {
    /// A transaction derived from `cites`, in the order the caller states, so that a multi-row
    /// event keeps its audit trail in the shape it had [DOM-016].
    ///
    /// `cites` is non-empty and made of storage's handles, so there is no derivation from nothing
    /// [DOM-047]. From one handle it compiles:
    ///
    /// ```
    /// # use chrono::NaiveDate;
    /// # use fifolio_core::entities::Account;
    /// # use fifolio_core::identity::{IdentitySource, identify};
    /// # use fifolio_core::storage::RecordHandle;
    /// # use fifolio_core::transaction::Derivation;
    /// # use vec1::vec1;
    /// fn derive(date: NaiveDate, record: RecordHandle) -> Derivation {
    ///     Derivation::new(date, vec1![record])
    /// }
    /// // The setup the failing examples below share, so that each fails for the reason named
    /// // and not because an import or a signature drifted.
    /// let account = Account::new("Saxo", "69900/1000000");
    /// let _ = identify(&account, &IdentitySource::BrokerReference("r1"));
    /// ```
    ///
    /// From none it does not. Stable rustdoc does not check the error codes below, so the block
    /// above is what shows the imports and the call are sound and each failure is the one named:
    ///
    /// ```compile_fail,E0308
    /// # use chrono::NaiveDate;
    /// # use fifolio_core::transaction::Derivation;
    /// let date = NaiveDate::from_ymd_opt(2024, 5, 2).unwrap();
    /// let _ = Derivation::new(date, Vec::new());
    /// ```
    ///
    /// Nor from an identity computed for a row nobody read:
    ///
    /// ```compile_fail,E0308
    /// # use chrono::NaiveDate;
    /// # use fifolio_core::entities::Account;
    /// # use fifolio_core::identity::{IdentitySource, identify};
    /// # use fifolio_core::transaction::Derivation;
    /// # use vec1::vec1;
    /// let date = NaiveDate::from_ymd_opt(2024, 5, 2).unwrap();
    /// let account = Account::new("Saxo", "69900/1000000");
    /// let invented = identify(&account, &IdentitySource::BrokerReference("r1"));
    /// let _ = Derivation::new(date, vec1![invented]);
    /// ```
    ///
    /// Nor from a handle made by hand, because only storage makes them:
    ///
    /// ```compile_fail,E0624
    /// # use fifolio_core::entities::Account;
    /// # use fifolio_core::identity::{IdentitySource, identify};
    /// # use fifolio_core::storage::RecordHandle;
    /// let account = Account::new("Saxo", "69900/1000000");
    /// let _ = RecordHandle::new(identify(&account, &IdentitySource::BrokerReference("r1")));
    /// ```
    ///
    /// Its position is the lowest of the records' positions, so the transaction sorts where its
    /// first row does [DOM-011] (DEC-090, provisional). Records from different files are compared
    /// by `order` then batch age, as though they shared the transaction's trade date (DEC-094,
    /// provisional). It leads its position; [`Derivation::trailing`] makes it the second leg.
    #[must_use]
    pub fn new(trade_date: NaiveDate, cites: Vec1<RecordHandle>) -> Self {
        let position = cites
            .iter()
            .map(RecordHandle::position)
            .fold(cites.first().position(), Ord::min);
        Self {
            trade_date,
            cites: cites.mapped(RecordHandle::into_identity),
            position,
            leg: Leg::Lead,
        }
    }

    /// A derivation as storage wrote it, its place read back rather than recomputed: the records
    /// it cites may since have gone [DOM-099], and a place taken from anything but the records a
    /// transaction was derived from (an emitted `transfer_in`'s, DEC-079) cannot be recomputed.
    pub(crate) fn stored(trade_date: NaiveDate, cites: Vec1<RecordHandle>, key: OrderKey) -> Self {
        Self {
            trade_date,
            cites: cites.mapped(RecordHandle::into_identity),
            position: key.position(),
            leg: key.leg(),
        }
    }

    /// The derivation of a `transfer_in` emitted on approval of the `transfer_out` this derives:
    /// it cites the same records, which is what keeps it from being created from nothing
    /// [DEC-079], and takes `parcel`, the stored place of the parcel it carries, as its own trade
    /// date and place, so the parcels sort in the target security as they did before [DOM-090]
    /// (DEC-105, provisional).
    pub(crate) fn carried(&self, parcel: OrderKey) -> Self {
        Self {
            trade_date: parcel.trade_date(),
            cites: self.cites.clone(),
            position: parcel.position(),
            leg: parcel.leg(),
        }
    }

    /// The same derivation as the trailing leg of a decomposition: it sorts immediately after the
    /// leg derived from the same records (DEC-090, provisional). Building the decomposition is
    /// FIF-100's.
    #[must_use]
    pub fn trailing(self) -> Self {
        Self {
            leg: Leg::Trailing,
            ..self
        }
    }

    #[must_use]
    pub fn trade_date(&self) -> NaiveDate {
        self.trade_date
    }

    /// The source records this transaction was derived from [DOM-016].
    #[must_use]
    pub fn cites(&self) -> &[RecordIdentity] {
        &self.cites
    }

    /// Where the transaction sits in the canonical order of its account and security [DOM-011],
    /// [DOM-111].
    #[must_use]
    pub fn order_key(&self) -> OrderKey {
        OrderKey::new(self.trade_date, self.position, self.leg)
    }
}

/// Defines one transaction variant: a [`Derivation`] plus the fields that variant alone has.
///
/// The fields differ per variant and their accessors do not, so the accessors are generated.
/// Every field is private and read-only, which is what makes a `transfer_in`'s source and date
/// provenance non-editable by construction rather than by a check [DOM-083].
///
/// Fields listed in the trailing `borrowed` group are handed back by reference instead of by
/// value; a [`Conversion`] names its currency and so is not `Copy`.
macro_rules! variant {
    ($(#[$meta:meta])* $name:ident { $($(#[$field_meta:meta])* $field:ident: $type:ty),* $(,)? }) => {
        variant!($(#[$meta])* $name { $($(#[$field_meta])* $field: $type,)* } borrowed {});
    };
    (
        $(#[$meta:meta])* $name:ident { $($(#[$field_meta:meta])* $field:ident: $type:ty),* $(,)? }
        borrowed { $($(#[$borrowed_meta:meta])* $borrowed:ident: $borrowed_type:ty),* $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct $name {
            derivation: Derivation,
            $($field: $type,)*
            $($borrowed: $borrowed_type,)*
        }

        impl $name {
            /// A variant's constructor takes exactly the fields that variant has, which is the
            /// point of the type; `transfer_in` has eight of them and grouping them behind a
            /// builder would hide which ones a caller forgot.
            #[allow(clippy::too_many_arguments)]
            #[must_use]
            pub fn new(
                derivation: Derivation,
                $($field: $type,)*
                $($borrowed: $borrowed_type,)*
            ) -> Self {
                Self { derivation, $($field,)* $($borrowed,)* }
            }

            #[must_use]
            pub fn derivation(&self) -> &Derivation {
                &self.derivation
            }

            #[must_use]
            pub fn trade_date(&self) -> NaiveDate {
                self.derivation.trade_date()
            }

            $(
                $(#[$field_meta])*
                #[must_use]
                pub fn $field(&self) -> $type {
                    self.$field
                }
            )*

            $(
                $(#[$borrowed_meta])*
                #[must_use]
                pub fn $borrowed(&self) -> &$borrowed_type {
                    &self.$borrowed
                }
            )*
        }
    };
}

variant!(
    /// Opens a parcel against payment, or against a stock dividend's taxable value when
    /// nothing was paid [DOM-082]. Realizes no gain.
    Buy {
        quantity: Quantity,
        /// The price the statement shows, so it still reconciles against the document
        /// [DOM-039]. Whether it is per unit or a percentage of par is the security's
        /// quotation.
        ///
        /// Display and reconciliation only: what a calculation reads is `gross` [DOM-085].
        unit_price: Valued<QuotedPrice>,
        /// What the parcel cost in total, as booked [DOM-085]. For a buy issued as a stock
        /// dividend it is also the taxable value at issue, stored once [DOM-123]; read it
        /// through [`Buy::taxable_value`].
        gross: Valued<Money>,
        /// Commission, exchange fees and transaction taxes, summed [DOM-012], converted at
        /// the same rate as the leg they belong to [DOM-035].
        fees: Valued<Money>,
        /// How this buy arose [DOM-082].
        origin: BuyOrigin,
    }
    borrowed {
        /// The rate the EUR figures were obtained under, its source and its date [DOM-028].
        conversion: Conversion,
    }
);

variant!(
    /// Opens a parcel with a cost basis carried from elsewhere: another broker, or a corporate
    /// action that replaced the parcel this one continues [DOM-083]. Realizes no gain.
    TransferIn {
        quantity: Quantity,
        /// What the parcel cost where it was acquired, carried onward rather than paid here.
        /// It is this opening's gross [DOM-085].
        cost_basis: Valued<Money>,
        /// Commission, exchange fees and transaction taxes, summed [DOM-012], converted at
        /// the same rate as the leg they belong to [DOM-035].
        fees: Valued<Money>,
        /// When the parcel was acquired, which is what a holding period is counted from — and
        /// what `date_provenance` says the worth of.
        acquisition_date: NaiveDate,
        /// Whether `acquisition_date` is a real acquisition date or a stand-in [DOM-083].
        date_provenance: DateProvenance,
        /// Where the units came from [DOM-083].
        source: TransferInSource,
    }
    borrowed {
        /// The rate the EUR figures were obtained under, its source and its date [DOM-028].
        conversion: Conversion,
    }
);

variant!(
    /// Closes parcels against payment. Realizes a gain.
    Sell {
        quantity: Quantity,
        /// The price the statement shows [DOM-039]. Display and reconciliation only: what a
        /// calculation reads is `gross` [DOM-085].
        unit_price: Valued<QuotedPrice>,
        /// The proceeds in total, as booked [DOM-085].
        gross: Valued<Money>,
        /// Commission, exchange fees and transaction taxes, summed [DOM-012], converted at
        /// the same rate as the leg they belong to [DOM-035].
        fees: Valued<Money>,
    }
    borrowed {
        /// The rate the EUR figures were obtained under, its source and its date [DOM-028].
        conversion: Conversion,
    }
);

variant!(
    /// Closes the whole remaining position at zero value — an option or warrant expiring
    /// worthless. Realizes a gain, which is a loss of the whole basis.
    ///
    /// It carries no quantity: the quantity is the unattributed remainder rather than a stated
    /// figure, which is DOM-092 and belongs to FIF-079.
    Expiration {
        /// What the position paid out, as booked — zero for a worthless expiry, and stored
        /// rather than assumed so that every cash closing answers one formula [DOM-085]. It
        /// has no unit price: there is no price at which nothing was sold.
        gross: Valued<Money>,
        /// Commission, exchange fees and transaction taxes, summed [DOM-012], converted at
        /// the same rate as the leg they belong to [DOM-035].
        fees: Valued<Money>,
    }
    borrowed {
        /// The rate the EUR figures were obtained under, its source and its date [DOM-028].
        conversion: Conversion,
    }
);

variant!(
    /// Closes parcels without payment: the basis carries onward into the records it emits, so
    /// it realizes **no** gain.
    ///
    /// The one money-bearing variant with no `gross`: a transfer has no proceeds, and the
    /// basis it carries onward is derived from its own allocations rather than stored, which
    /// is DOM-112 and belongs to FIF-080. Storing a figure here would be inventing the answer
    /// to a question that is still open (OQ-003, OQ-011, OQ-016).
    TransferOut {
        quantity: Quantity,
        /// Commission, exchange fees and transaction taxes, summed [DOM-012], converted at
        /// the same rate as the leg they belong to [DOM-035]. Stored as booked, but a transfer
        /// carrying one is refused at approval rather than divided [DOM-107].
        fees: Valued<Money>,
        /// Units of `target` received for each unit transferred, as an exact integer pair
        /// (DEC-055), which the emitted quantities are scaled by [DOM-115].
        ratio: Ratio,
    }
    borrowed {
        /// The rate the EUR figures were obtained under, its source and its date [DOM-028].
        conversion: Conversion,
        /// The security the emitted `transfer_in` records open parcels of [DOM-090].
        target: Isin,
    }
);

variant!(
    /// Rescales every open parcel of one security. Neither opens nor closes anything
    /// [DOM-081], and realizes no gain.
    ///
    /// What it does to a parcel is [`crate::effective_quantity`]'s: the parcel's stated figures
    /// stay as booked, and its effective quantity is derived through this ratio [DOM-089].
    Split {
        /// `numerator` new units for every `denominator` held, as an integer pair so that a
        /// one-for-three is never flattened into a decimal that cannot hold it [DOM-113].
        ratio: Ratio,
    }
);

/// How a [`Buy`] arose [DOM-082].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BuyOrigin {
    /// An ordinary purchase. Its cost basis is what it paid: quantity, unit price and fees.
    Purchase,
    /// Shares issued as a stock dividend, whose cost basis is their taxable value at issue
    /// [DOM-082].
    ///
    /// The variant holds nothing: that taxable value is the buy's EUR gross and is stored
    /// there once [DOM-123]. A field here would be a second figure that could drift from the
    /// one every calculation reads, which is a cost basis wrong and silent about it.
    StockDividend,
}

impl Buy {
    /// The taxable value at issue of shares issued as a stock dividend, which is their cost
    /// basis, nothing having been paid for them [DOM-082]. `None` for an ordinary purchase,
    /// which paid for its basis and has no such value.
    ///
    /// It reads the stored EUR gross rather than a field of its own [DOM-123], so there is no
    /// way to state a taxable value and a gross that disagree. It is still never computed here:
    /// the gross is the figure the caller booked, not one derived from quantity and unit price.
    #[must_use]
    pub fn taxable_value(&self) -> Option<Money> {
        match self.origin {
            BuyOrigin::Purchase => None,
            BuyOrigin::StockDividend => Some(self.gross.eur()),
        }
    }
}

/// Where the units of a [`TransferIn`] came from [DOM-083].
///
/// Not editable: it records what the import found, and there is no setter anywhere for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TransferInSource {
    /// A transfer in from another broker.
    Broker,
    /// Units arriving from a corporate action.
    CorporateAction,
}

/// What a [`TransferIn`]'s acquisition date is worth [DOM-083].
///
/// Not editable either, and for a sharper reason: a transfer date presented as an acquisition
/// date that may be corrected is how a holding period silently becomes whatever the user
/// believed. It says what it is instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DateProvenance {
    /// The export gave no real acquisition date, so the transfer date stands in for it.
    TransferDate,
    /// Carried from the parcel this transfer replaced.
    Inherited,
}

/// A transaction that opens a parcel [DOM-081].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opening {
    Buy(Buy),
    TransferIn(TransferIn),
}

impl Opening {
    #[must_use]
    pub fn derivation(&self) -> &Derivation {
        match self {
            Self::Buy(buy) => buy.derivation(),
            Self::TransferIn(transfer_in) => transfer_in.derivation(),
        }
    }

    /// The quantity the statement states, in the units current at the opening. A split never
    /// rewrites it; the quantity at a later position is [`crate::effective_quantity`]'s
    /// [DOM-089].
    #[must_use]
    pub fn quantity(&self) -> Quantity {
        match self {
            Self::Buy(buy) => buy.quantity(),
            Self::TransferIn(transfer_in) => transfer_in.quantity(),
        }
    }

    /// What the parcel cost in total: a buy's gross, a `transfer_in`'s carried cost basis, which
    /// is its gross [DOM-085]. Fees are not in it; they are a separate figure [DOM-059].
    #[must_use]
    pub fn gross(&self) -> Valued<Money> {
        match self {
            Self::Buy(buy) => buy.gross(),
            Self::TransferIn(transfer_in) => transfer_in.cost_basis(),
        }
    }

    /// The opening's fees, which allocation spreads as the buy fee, apart from its cost
    /// [DOM-059].
    #[must_use]
    pub fn fees(&self) -> Valued<Money> {
        match self {
            Self::Buy(buy) => buy.fees(),
            Self::TransferIn(transfer_in) => transfer_in.fees(),
        }
    }

    /// The rate the opening's EUR figures were obtained under [DOM-028].
    #[must_use]
    pub fn conversion(&self) -> &Conversion {
        match self {
            Self::Buy(buy) => buy.conversion(),
            Self::TransferIn(transfer_in) => transfer_in.conversion(),
        }
    }

    #[must_use]
    pub fn trade_date(&self) -> NaiveDate {
        self.derivation().trade_date()
    }
}

/// A transaction that closes parcels [DOM-081].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Closing {
    Sell(Sell),
    Expiration(Expiration),
    TransferOut(TransferOut),
}

impl Closing {
    #[must_use]
    pub fn derivation(&self) -> &Derivation {
        match self {
            Self::Sell(sell) => sell.derivation(),
            Self::Expiration(expiration) => expiration.derivation(),
            Self::TransferOut(transfer_out) => transfer_out.derivation(),
        }
    }

    /// Every closing carries its fees, so one formula spreads them all as the sell fee
    /// [DOM-105]. The gross is not read here: a `transfer_out` stores none (FIF-080, DOM-112).
    #[must_use]
    pub fn fees(&self) -> Valued<Money> {
        match self {
            Self::Sell(sell) => sell.fees(),
            Self::Expiration(expiration) => expiration.fees(),
            Self::TransferOut(transfer_out) => transfer_out.fees(),
        }
    }

    #[must_use]
    pub fn trade_date(&self) -> NaiveDate {
        self.derivation().trade_date()
    }
}

/// Everything that happens to a holding [DOM-010].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transaction {
    Opening(Opening),
    Closing(Closing),
    Split(Split),
}

impl Transaction {
    #[must_use]
    pub fn derivation(&self) -> &Derivation {
        match self {
            Self::Opening(opening) => opening.derivation(),
            Self::Closing(closing) => closing.derivation(),
            Self::Split(split) => split.derivation(),
        }
    }

    /// Every variant carries one [DOM-010].
    #[must_use]
    pub fn trade_date(&self) -> NaiveDate {
        self.derivation().trade_date()
    }

    /// The date this transaction's EUR figures are valued at: the trade date, which is the
    /// obligating transaction, and never the settlement date [DOM-025, DOM-027].
    ///
    /// The same field as [`Transaction::trade_date`] and deliberately so — there is no
    /// settlement date anywhere in this crate to reach for by mistake. It exists so that the
    /// rule is named where a caller asks the question.
    #[must_use]
    pub fn valuation_date(&self) -> NaiveDate {
        self.trade_date()
    }

    /// The source records this transaction was derived from [DOM-016].
    #[must_use]
    pub fn cites(&self) -> &[RecordIdentity] {
        self.derivation().cites()
    }

    /// Where this transaction sits in the canonical order [DOM-011], [DOM-111].
    #[must_use]
    pub fn order_key(&self) -> OrderKey {
        self.derivation().order_key()
    }

    /// The opening this is, if it opens a parcel [DOM-081]. `None` for a split.
    #[must_use]
    pub fn opening(&self) -> Option<&Opening> {
        match self {
            Self::Opening(opening) => Some(opening),
            Self::Closing(_) | Self::Split(_) => None,
        }
    }

    /// The closing this is, if it closes parcels [DOM-081]. `None` for a split.
    #[must_use]
    pub fn closing(&self) -> Option<&Closing> {
        match self {
            Self::Closing(closing) => Some(closing),
            Self::Opening(_) | Self::Split(_) => None,
        }
    }
}

/// Lifts each variant into the group it belongs to, so a caller names the variant and the
/// grouping of [DOM-081] follows rather than being restated at every construction site.
macro_rules! lift {
    ($from:ty => $group:ident::$case:ident) => {
        impl From<$from> for $group {
            fn from(value: $from) -> Self {
                Self::$case(value)
            }
        }

        impl From<$from> for Transaction {
            fn from(value: $from) -> Self {
                Self::$group($group::$case(value))
            }
        }
    };
}

lift!(Buy => Opening::Buy);
lift!(TransferIn => Opening::TransferIn);
lift!(Sell => Closing::Sell);
lift!(Expiration => Closing::Expiration);
lift!(TransferOut => Closing::TransferOut);

impl From<Opening> for Transaction {
    fn from(value: Opening) -> Self {
        Self::Opening(value)
    }
}

impl From<Closing> for Transaction {
    fn from(value: Closing) -> Self {
        Self::Closing(value)
    }
}

impl From<Split> for Transaction {
    fn from(value: Split) -> Self {
        Self::Split(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::num::NonZeroU32;

    use rust_decimal_macros::dec;
    use vec1::vec1;

    use crate::decimal::{FxRate, Scaled};
    use crate::entities::{Account, Order};
    use crate::identity::{IdentitySource, identify};
    use crate::ordering::BatchAge;
    use crate::valuation::{Currency, RateSource};

    fn account() -> Account {
        Account::new("Saxo", "69900/1000000")
    }

    fn cite(reference: &str) -> RecordIdentity {
        identify(&account(), &IdentitySource::BrokerReference(reference))
    }

    /// Storage's handle on the record `reference` names, as it would issue one on insert.
    fn stored(reference: &str) -> RecordHandle {
        stored_at(reference, 0, 1)
    }

    /// A handle on a record at `order` in the file batch `batch` first supplied.
    fn stored_at(reference: &str, order: u32, batch: i64) -> RecordHandle {
        RecordHandle::for_test(
            cite(reference),
            RecordPosition::new(Order::new(order), BatchAge::new(batch)),
        )
    }

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2024, 5, 2).expect("a valid date")
    }

    fn derivation() -> Derivation {
        Derivation::new(date(), vec1![stored("row-1")])
    }

    /// The conversion the worked Saxo buy was booked under: USD figures, the EUR figures the
    /// file states, and the quotient they imply as the stored rate [DOM-030, DOM-086].
    fn usd() -> Conversion {
        Conversion::new(
            Currency::new("USD"),
            FxRate::new(dec!(1.097171)),
            RateSource::Broker,
            date(),
        )
    }

    /// 40 units at 5.75 USD: 230.00 USD of gross, 209.63 EUR at 1.097171 USD per EUR, and
    /// 3.50 USD of fees converted at the same rate [DOM-035].
    fn buy() -> Buy {
        Buy::new(
            derivation(),
            Quantity::new(dec!(40)),
            Valued::new(
                QuotedPrice::new(dec!(5.75)),
                QuotedPrice::new(dec!(5.240750)),
            ),
            Valued::new(Money::new(dec!(230.00)), Money::new(dec!(209.63))),
            Valued::new(Money::new(dec!(3.50)), Money::new(dec!(3.19))),
            BuyOrigin::Purchase,
            usd(),
        )
    }

    /// A `buy` for shares issued as a stock dividend: 3 shares booked at 26.10, so a gross of
    /// 78.30 — which is their taxable value at issue and therefore their cost basis, nothing
    /// having been paid [DOM-082, DOM-123].
    ///
    /// FIF-057's fixture stated 81.00 against this same 3 x 26.10; DEC-061 settled that the two
    /// are one figure, so there is no second number to state here.
    fn stock_dividend_buy() -> Buy {
        Buy::new(
            derivation(),
            Quantity::new(dec!(3)),
            Valued::in_eur(QuotedPrice::new(dec!(26.10))),
            Valued::in_eur(Money::new(dec!(78.30))),
            Valued::in_eur(Money::zero()),
            BuyOrigin::StockDividend,
            Conversion::native(date()),
        )
    }

    fn transfer_in() -> TransferIn {
        TransferIn::new(
            derivation(),
            Quantity::new(dec!(300)),
            Valued::in_eur(Money::new(dec!(15420.00))),
            Valued::in_eur(Money::zero()),
            date(),
            DateProvenance::TransferDate,
            TransferInSource::Broker,
            Conversion::native(date()),
        )
    }

    /// The worked Saxo sell: 60 units, 1839.24 EUR of proceeds, so 30.654 a unit — which is
    /// not the 30.65 the label prints [DOM-085].
    fn sell() -> Sell {
        Sell::new(
            derivation(),
            Quantity::new(dec!(60)),
            Valued::in_eur(QuotedPrice::new(dec!(30.654))),
            Valued::in_eur(Money::new(dec!(1839.24))),
            Valued::in_eur(Money::new(dec!(9.10))),
            Conversion::native(date()),
        )
    }

    fn expiration() -> Expiration {
        Expiration::new(
            derivation(),
            Valued::in_eur(Money::zero()),
            Valued::in_eur(Money::zero()),
            Conversion::native(date()),
        )
    }

    fn transfer_out() -> TransferOut {
        TransferOut::new(
            derivation(),
            Quantity::new(dec!(25)),
            Valued::in_eur(Money::zero()),
            Ratio::new(NonZeroU32::MIN, NonZeroU32::MIN),
            Conversion::native(date()),
            Isin::new("IE000Y77LGG9"),
        )
    }

    fn split() -> Split {
        Split::new(
            derivation(),
            Ratio::new(NonZeroU32::new(2).expect("non-zero"), NonZeroU32::MIN),
        )
    }

    fn every_variant() -> Vec<Transaction> {
        vec![
            buy().into(),
            transfer_in().into(),
            sell().into(),
            expiration().into(),
            transfer_out().into(),
            split().into(),
        ]
    }

    /// The sum type has exactly the six variants the specification names [DOM-010].
    ///
    /// Exhaustive by construction: a seventh variant, or one moved between the groups, makes
    /// these matches fail to compile.
    #[test]
    fn the_sum_type_has_the_six_specified_variants() {
        for transaction in every_variant() {
            match transaction {
                Transaction::Opening(Opening::Buy(_) | Opening::TransferIn(_))
                | Transaction::Closing(
                    Closing::Sell(_) | Closing::Expiration(_) | Closing::TransferOut(_),
                )
                | Transaction::Split(_) => {}
            }
        }

        assert_eq!(every_variant().len(), 6);
    }

    /// Every variant carries a trade date [DOM-010].
    #[test]
    fn every_variant_carries_a_trade_date() {
        for transaction in every_variant() {
            assert_eq!(transaction.trade_date(), date());
        }
    }

    /// Every variant carries the `order` of the record it was derived from, keyed with its trade
    /// date and that record's batch age [DOM-011], [DOM-111].
    #[test]
    fn every_variant_carries_the_order_of_its_record() {
        let expected = OrderKey::new(
            date(),
            RecordPosition::new(Order::new(0), BatchAge::new(1)),
            Leg::Lead,
        );
        for transaction in every_variant() {
            assert_eq!(transaction.order_key(), expected);
        }
    }

    /// A transaction derived from several records takes the lowest of their orders, whatever
    /// order they are cited in (DEC-090) [DOM-011]; across files the lower `order` wins before the
    /// older batch does (DEC-094) [DOM-111].
    #[test]
    fn a_transaction_from_several_records_takes_the_lowest_order() {
        let derivation = Derivation::new(
            date(),
            vec1![
                stored_at("cash-row", 7, 1),
                stored_at("position-row", 4, 3),
                stored_at("reversal-row", 5, 1),
            ],
        );

        assert_eq!(
            derivation.order_key().position(),
            RecordPosition::new(Order::new(4), BatchAge::new(3))
        );
        // The citations keep the caller's shape; only the key takes the minimum [DOM-016].
        assert_eq!(derivation.cites()[0], cite("cash-row"));
    }

    /// Of two transactions derived from the same records, the trailing one sorts immediately after
    /// the other: the key a decomposition's `transfer_out` needs against its `sell` (DEC-090)
    /// [DOM-011].
    #[test]
    fn a_trailing_leg_sorts_immediately_after_the_leg_it_shares_records_with() {
        let rows = vec1![stored_at("merger-row", 2, 1)];
        let sell = Derivation::new(date(), rows.clone());
        let transfer_out = Derivation::new(date(), rows).trailing();

        assert_eq!(sell.cites(), transfer_out.cites());
        assert_eq!(sell.order_key().leg(), Leg::Lead);
        assert!(sell.order_key() < transfer_out.order_key());
        assert!(
            transfer_out.order_key()
                < Derivation::new(date(), vec1![stored_at("next-row", 3, 1)]).order_key(),
            "the next record of the day still sorts after both legs"
        );
    }

    /// A transaction cites the source records it was derived from, so a multi-row event keeps
    /// its audit trail [DOM-016].
    #[test]
    fn a_transaction_cites_the_records_it_was_derived_from() {
        let rows = vec1![stored("philips-position-row"), stored("philips-cash-row")];

        let transaction: Transaction = Buy::new(
            Derivation::new(date(), rows.clone()),
            Quantity::new(dec!(3)),
            Valued::in_eur(QuotedPrice::new(dec!(26.10))),
            Valued::in_eur(Money::new(dec!(78.30))),
            Valued::in_eur(Money::zero()),
            BuyOrigin::Purchase,
            Conversion::native(date()),
        )
        .into();

        assert_eq!(
            transaction.cites(),
            [cite("philips-position-row"), cite("philips-cash-row")]
        );
    }

    /// The citation is by broker-scoped identity rather than by an internal key, which is what
    /// lets it name a record across the deletion and re-import of that record [DOM-016].
    #[test]
    fn citations_name_records_by_identity() {
        let before = cite("bk-record-1");
        let after_a_re_import = cite("bk-record-1");

        assert_eq!(before, after_a_re_import);
        assert_ne!(before, cite("bk-record-2"));
    }

    /// Opening and closing variants are distinguishable, and `split` is neither [DOM-081].
    #[test]
    fn openings_and_closings_are_distinguishable_and_a_split_is_neither() {
        let openings: Vec<Transaction> = vec![buy().into(), transfer_in().into()];
        let closings: Vec<Transaction> =
            vec![sell().into(), expiration().into(), transfer_out().into()];

        for opening in &openings {
            assert!(opening.opening().is_some());
            assert!(opening.closing().is_none());
        }
        for closing in &closings {
            assert!(closing.closing().is_some());
            assert!(closing.opening().is_none());
        }

        let split: Transaction = split().into();
        assert!(split.opening().is_none());
        assert!(split.closing().is_none());
    }

    /// The grouping is in the type system, not in a label: a function over openings takes an
    /// `Opening` and there is no value of that type carrying a sell or a split [DOM-081].
    #[test]
    fn the_grouping_is_structural() {
        fn quantity_opened(opening: &Opening) -> Quantity {
            match opening {
                Opening::Buy(buy) => buy.quantity(),
                Opening::TransferIn(transfer_in) => transfer_in.quantity(),
            }
        }

        assert_eq!(quantity_opened(&buy().into()), Quantity::new(dec!(40)));
        assert_eq!(
            quantity_opened(&transfer_in().into()),
            Quantity::new(dec!(300))
        );
    }

    /// Fees are one field holding the sum of every incidental cost [DOM-012].
    ///
    /// The figures are a commission, an exchange fee and a French FTT; what the variant holds
    /// is their total, and there is no second fee field for a formula to forget.
    #[test]
    fn fees_are_a_single_summed_field() {
        let commission = dec!(5.00);
        let exchange_fee = dec!(1.50);
        let transaction_tax = dec!(0.90);

        let buy = Buy::new(
            derivation(),
            Quantity::new(dec!(100)),
            Valued::in_eur(QuotedPrice::new(dec!(3.00))),
            Valued::in_eur(Money::new(dec!(300.00))),
            Valued::in_eur(Money::new(commission + exchange_fee + transaction_tax)),
            BuyOrigin::Purchase,
            Conversion::native(date()),
        );

        assert_eq!(buy.fees().native(), Money::new(dec!(7.40)));
    }

    /// Every variant that can bear a cost carries that one field, so one formula reads them
    /// all [DOM-012].
    #[test]
    fn every_costed_variant_carries_the_same_fee_field() {
        // The list is the statement: all five expose the same accessor of the same type, and a
        // variant that grew a second fee field would have to be read some other way.
        let [
            buy_fees,
            _transfer_in_fees,
            sell_fees,
            _expiration_fees,
            _transfer_out_fees,
        ] = [
            buy().fees(),
            transfer_in().fees(),
            sell().fees(),
            expiration().fees(),
            transfer_out().fees(),
        ];

        assert_eq!(buy_fees.native(), Money::new(dec!(3.50)));
        assert_eq!(sell_fees.eur(), Money::new(dec!(9.10)));
    }

    /// A `transfer_in` says where its units came from and what its acquisition date is worth
    /// [DOM-083].
    #[test]
    fn a_transfer_in_carries_its_source_and_its_date_provenance() {
        let from_broker = transfer_in();
        assert_eq!(from_broker.source(), TransferInSource::Broker);
        assert_eq!(
            from_broker.date_provenance(),
            DateProvenance::TransferDate,
            "a broker deposit states no real acquisition date"
        );
        assert_eq!(from_broker.acquisition_date(), date());

        let from_corporate_action = TransferIn::new(
            derivation(),
            Quantity::new(dec!(10)),
            Valued::in_eur(Money::new(dec!(1000.00))),
            Valued::in_eur(Money::zero()),
            NaiveDate::from_ymd_opt(2019, 3, 14).expect("a valid date"),
            DateProvenance::Inherited,
            TransferInSource::CorporateAction,
            Conversion::native(date()),
        );
        assert_eq!(
            from_corporate_action.source(),
            TransferInSource::CorporateAction
        );
        assert_eq!(
            from_corporate_action.date_provenance(),
            DateProvenance::Inherited
        );
        assert_ne!(
            from_corporate_action.acquisition_date(),
            from_corporate_action.trade_date(),
            "an inherited date is the replaced parcel's, not this transaction's"
        );
    }

    /// Both values of each are representable, and nothing else is [DOM-083].
    #[test]
    fn the_source_and_provenance_enums_are_closed_two_value_sets() {
        let sources = [TransferInSource::Broker, TransferInSource::CorporateAction];
        for source in sources {
            match source {
                TransferInSource::Broker | TransferInSource::CorporateAction => {}
            }
        }

        let provenances = [DateProvenance::TransferDate, DateProvenance::Inherited];
        for provenance in provenances {
            match provenance {
                DateProvenance::TransferDate | DateProvenance::Inherited => {}
            }
        }

        assert_eq!(sources.len(), 2);
        assert_eq!(provenances.len(), 2);
    }

    /// A group is usable on its own: it answers for the trade date of whichever variant it
    /// holds [DOM-010], and lifts into the sum type without the caller naming the variant
    /// again [DOM-081].
    #[test]
    fn a_group_carries_the_trade_date_and_lifts_into_the_sum_type() {
        let groups: Vec<(Opening, Closing)> = vec![
            (buy().into(), sell().into()),
            (transfer_in().into(), expiration().into()),
            (buy().into(), transfer_out().into()),
        ];

        for (opening, closing) in groups {
            assert_eq!(opening.trade_date(), date());
            assert_eq!(closing.trade_date(), date());

            assert_eq!(
                Transaction::from(opening.clone()),
                Transaction::Opening(opening)
            );
            assert_eq!(
                Transaction::from(closing.clone()),
                Transaction::Closing(closing)
            );
        }
    }

    /// A `buy` records how it arose: an ordinary purchase, or shares issued as a stock dividend
    /// [DOM-082].
    #[test]
    fn a_buy_records_how_it_arose() {
        assert_eq!(buy().origin(), BuyOrigin::Purchase);

        let issued = stock_dividend_buy();
        assert_eq!(issued.origin(), BuyOrigin::StockDividend);
        assert_ne!(issued.origin(), buy().origin());
    }

    /// The two origins are the whole set [DOM-082]: a third makes this match fail to compile.
    #[test]
    fn the_origin_is_a_closed_two_case_set() {
        let origins = [BuyOrigin::Purchase, BuyOrigin::StockDividend];

        for origin in origins {
            match origin {
                BuyOrigin::Purchase | BuyOrigin::StockDividend => {}
            }
        }

        assert_eq!(origins.len(), 2);
    }

    /// A stock dividend's taxable value at issue **is** its stored EUR gross [DOM-123], and is
    /// its cost basis [DOM-082].
    ///
    /// This replaces FIF-057's assertion that the taxable value differs from the buy's own EUR
    /// figures — 81.00 against a stated 3 x 26.10 = 78.30. DEC-061 settled that the two are one
    /// figure, so the divergence that test protected is the failure this one forbids.
    #[test]
    fn a_stock_dividends_taxable_value_is_its_stored_eur_gross() {
        let issued = stock_dividend_buy();

        assert_eq!(issued.taxable_value(), Some(issued.gross().eur()));
        assert_eq!(issued.taxable_value(), Some(Money::new(dec!(78.30))));
    }

    /// Only a stock dividend has a taxable value at issue [DOM-082]: an ordinary purchase paid
    /// for its basis.
    #[test]
    fn an_ordinary_purchase_has_no_taxable_value_at_issue() {
        assert_eq!(buy().taxable_value(), None);
    }

    /// The origin holds no figure, so no caller can set a taxable value differing from the gross
    /// [DOM-123]: two stock dividends with different gross totals have the same origin, and each
    /// taxable value follows its own gross.
    #[test]
    fn the_origin_carries_no_figure_that_could_differ_from_the_gross() {
        let issued = stock_dividend_buy();
        let other_gross = Money::new(dec!(81.00));
        let mut other = issued.clone();
        other.gross = Valued::in_eur(other_gross);

        assert_eq!(issued.origin(), other.origin());
        assert_eq!(other.taxable_value(), Some(other_gross));
        assert_ne!(issued.taxable_value(), other.taxable_value());
    }

    /// The taxable value is the **EUR** half of the gross [DOM-123], not the native one: shares
    /// issued in USD take their EUR figure as the cost basis, never the USD amount.
    ///
    /// The other stock-dividend fixtures are booked in EUR, where the two halves are equal and
    /// a native/EUR mix-up is invisible. This case states them apart on purpose.
    #[test]
    fn a_stock_dividends_taxable_value_is_the_eur_half_of_a_foreign_gross() {
        let gross = Valued::new(Money::new(dec!(230.00)), Money::new(dec!(209.63)));
        let issued = Buy::new(
            derivation(),
            Quantity::new(dec!(40)),
            Valued::new(
                QuotedPrice::new(dec!(5.75)),
                QuotedPrice::new(dec!(5.240750)),
            ),
            gross,
            Valued::new(Money::zero(), Money::zero()),
            BuyOrigin::StockDividend,
            usd(),
        );

        assert_eq!(issued.taxable_value(), Some(Money::new(dec!(209.63))));
        assert_ne!(issued.taxable_value(), Some(gross.native()));
    }

    /// A zero gross is a stated figure, not an absent one [DOM-123]: the origin decides whether
    /// there is a taxable value, the amount decides only what it is.
    #[test]
    fn a_stock_dividend_with_a_zero_gross_has_a_taxable_value_of_zero() {
        let mut issued = stock_dividend_buy();
        issued.gross = Valued::in_eur(Money::zero());

        assert_eq!(issued.taxable_value(), Some(Money::zero()));
    }

    /// Every money-bearing variant stores its native figures, its EUR figures and the
    /// conversion that relates them [DOM-028].
    ///
    /// Written as one list so that every money-bearing variant answers the same assertions. It
    /// does not catch a sixth such variant: one added without a conversion does not appear here
    /// and this test keeps passing, so the list has to be extended by hand.
    #[test]
    fn every_money_bearing_variant_stores_both_currencies_and_the_conversion() {
        let conversions = [
            buy().conversion().clone(),
            transfer_in().conversion().clone(),
            sell().conversion().clone(),
            expiration().conversion().clone(),
            transfer_out().conversion().clone(),
        ];

        for conversion in &conversions {
            assert!(!conversion.currency().code().is_empty());
            assert_eq!(conversion.rate_date(), date());
        }

        let usd_buy = buy();
        assert_eq!(usd_buy.conversion().currency().code(), "USD");
        assert_eq!(usd_buy.conversion().rate(), FxRate::new(dec!(1.097171)));
        assert_eq!(usd_buy.conversion().source(), RateSource::Broker);
        assert_eq!(usd_buy.gross().native(), Money::new(dec!(230.00)));
        assert_eq!(usd_buy.gross().eur(), Money::new(dec!(209.63)));

        // A EUR-denominated leg is rate 1 from source `native`, and its two halves are one
        // figure [DOM-033].
        let eur_sell = sell();
        assert_eq!(eur_sell.conversion().source(), RateSource::Native);
        assert_eq!(eur_sell.gross().native(), eur_sell.gross().eur());
    }

    /// The EUR figures mirror the native ones: a price against a price, an amount against an
    /// amount, at the same scale [DOM-029].
    ///
    /// That an allocation share derives from this pair the same way on both sides and is never
    /// stored independently is DOM-084, and no allocation type exists yet: it is asserted by
    /// FIF-014, not here.
    #[test]
    fn the_eur_figures_mirror_the_native_ones_at_one_scale() {
        let priced = buy();
        let price = priced.unit_price().rounded();
        assert_eq!(price.native(), QuotedPrice::new(dec!(5.75)));
        assert_eq!(price.eur(), QuotedPrice::new(dec!(5.240750)));
        assert!(price.native().get().scale() <= QuotedPrice::SCALE);
        assert!(price.eur().get().scale() <= QuotedPrice::SCALE);

        let fees = priced.fees().rounded();
        assert_eq!(fees.native().get().scale(), Money::SCALE);
        assert_eq!(fees.eur().get().scale(), Money::SCALE);
    }

    /// The EUR gross total is stored as well as the unit price [DOM-085].
    ///
    /// A large nominal against a price cut to its scale is why: 1_234_567.89 EUR over
    /// 1_000_000 units is 1.23456789 a unit, which the price scale keeps as 1.234568
    /// [DEC-028], and multiplying that back gives 1_234_568.00 — eleven cents away from the
    /// booked total *after* rounding to cents, not a residue that vanishes there. The booked
    /// total is therefore not recoverable from the stored price at any scale, which is why it
    /// is stored rather than derived. Whether anything may read the unit price at all is
    /// DOM-104, undecided, and belongs to FIF-077.
    #[test]
    fn the_eur_gross_total_is_stored_as_well_as_the_unit_price() {
        let quantity = Quantity::new(dec!(1_000_000));
        let booked = Money::new(dec!(1_234_567.89));
        let price = QuotedPrice::new(dec!(1.23456789)).rounded();

        let nominal = Buy::new(
            derivation(),
            quantity,
            Valued::in_eur(price),
            Valued::in_eur(booked),
            Valued::in_eur(Money::zero()),
            BuyOrigin::Purchase,
            Conversion::native(date()),
        );

        assert_eq!(nominal.gross().eur(), booked);

        let rebuilt = Money::new(price.get() * quantity.get()).rounded();
        assert_eq!(rebuilt, Money::new(dec!(1_234_568.00)));
        assert_ne!(
            rebuilt, booked,
            "a total rebuilt from the stored unit price differs from the booked one at money scale"
        );
    }

    /// Fees convert at the same rate as the leg they belong to [DOM-035], and every EUR figure
    /// is reproducible from the stored rate, which is what makes it auditable [DOM-028].
    #[test]
    fn the_eur_figures_are_reproducible_from_the_stored_rate() {
        let usd_buy = buy();
        let rate = usd_buy.conversion().rate();

        for (native_and_eur, label) in [(usd_buy.gross(), "gross"), (usd_buy.fees(), "fees")] {
            let recomputed = Valued::converted(native_and_eur.native(), rate)
                .expect("the stored rate is not zero")
                .rounded();

            assert_eq!(
                recomputed.eur(),
                native_and_eur.eur(),
                "{label} does not reproduce from the stored rate"
            );
        }
    }

    /// A transaction is valued at its trade date, the obligating transaction, and there is no
    /// settlement date to value at instead [DOM-025, DOM-027].
    #[test]
    fn every_variant_is_valued_at_its_trade_date() {
        for transaction in every_variant() {
            assert_eq!(transaction.valuation_date(), transaction.trade_date());
            assert_eq!(transaction.valuation_date(), date());
        }

        // Each leg is valued at its **own** date, not at one date for the position [DOM-025]:
        // the acquisition below is five years before the disposal that closes it.
        let acquired = NaiveDate::from_ymd_opt(2019, 3, 14).expect("a valid date");
        let opened: Transaction = Buy::new(
            Derivation::new(acquired, vec1![stored("row-1")]),
            Quantity::new(dec!(10)),
            Valued::in_eur(QuotedPrice::new(dec!(100.00))),
            Valued::in_eur(Money::new(dec!(1000.00))),
            Valued::in_eur(Money::zero()),
            BuyOrigin::Purchase,
            Conversion::native(acquired),
        )
        .into();
        let closed: Transaction = sell().into();

        assert_eq!(opened.valuation_date(), acquired);
        assert_eq!(closed.valuation_date(), date());
        assert_ne!(opened.valuation_date(), closed.valuation_date());

        // A `transfer_in` is the one variant carrying a second date. The figures were converted
        // on the day it was booked here, so the valuation date is its trade date; the
        // acquisition date is what a holding period is counted from [DOM-027].
        let carried: Transaction = TransferIn::new(
            derivation(),
            Quantity::new(dec!(10)),
            Valued::in_eur(Money::new(dec!(1000.00))),
            Valued::in_eur(Money::zero()),
            acquired,
            DateProvenance::Inherited,
            TransferInSource::CorporateAction,
            Conversion::native(date()),
        )
        .into();

        assert_eq!(carried.valuation_date(), date());
        assert_ne!(carried.valuation_date(), acquired);
    }

    /// Neither is editable [DOM-083]: the fields are private and there is no setter, which is a
    /// compile-time property no runtime assertion can make. What is testable is that a
    /// transaction built from one is unchanged by anything a caller can do to it — here, being
    /// cloned and lifted into the sum type.
    #[test]
    fn a_transfer_in_keeps_its_source_and_provenance() {
        let original = transfer_in();
        let lifted: Transaction = original.clone().into();

        assert_eq!(Transaction::from(original), lifted);
    }
}
