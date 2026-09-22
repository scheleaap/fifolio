//! What the user supplied because no export contains it [DOM-097].
//!
//! Types only. Nothing here persists, derives or computes.
//!
//! # Why this is not a source record
//!
//! A source record is reproducible: import the file again. A manual entry is not — it is the
//! only thing in the system that cannot be recovered from a broker file, which is why undoing
//! an import can never destroy one [DOM-100]. That difference is structural rather than
//! remembered (DEC-038): a [`ManualEntry`] belongs to an [`Account`], carries no import batch
//! and holds no [`crate::entities::SourceRecord`], so the undo that removes a batch's records
//! has nothing here to remove. Like [DOM-008] on a source record, that is a compile-time
//! property; no runtime test can name it, and the absence of one is deliberate.
//!
//! # Why it names records by identity
//!
//! The records an entry answers are held as [`RecordIdentity`] values — the broker's own
//! identity for the row, scoped to the account — and never as an internal key [DOM-099]. A
//! foreign key would be dangling the moment the batch was undone; an identity is a value, so it
//! survives the deletion and matches again when the same rows are imported, because
//! [`crate::identity::identify`] recomputes the same string from the same row. The reconnection
//! itself, and the listing of an entry whose records are absent, are DOM-108 and DOM-109 and
//! belong to FIF-047; this type is what makes them possible.
//!
//! # What "what was supplied" covers
//!
//! [`Supplied`] is the set [DOM-097] names, and DEC-060 states that set is closed: a share
//! count, a stock-or-cash election, a target security with a ratio. An acquisition date is not
//! among them [DOM-122] — a transferred parcel's date is fixed at import and never corrected
//! (SRV-054, IMP-SAXO-016) — so `cli.md`'s listing of one among the manual shapes is the wording
//! DEC-060 overrules, and there is no variant for it and no accessor that could set one.
//!
//! The completion queue in `cli.md` also asks for a split's ratio on its own, and for a disposed
//! quantity with an *optional* target security. Neither shape is in the closed set, so neither is
//! modeled here: inventing a variant would reopen a set DEC-060 shut, and widening DOM-097 is a
//! decision for a person, not for this module. The queue item is FIF-046.

use std::num::NonZeroU32;

use crate::decimal::Quantity;
use crate::entities::{Account, Isin, RecordIdentity};

/// Which of a stock-or-cash dividend the user elected [DOM-097].
///
/// What each choice derives — stock a `buy` whose origin is a stock dividend, cash nothing at
/// all — is the Saxo importer's rule (IMP-SAXO-018), not this type's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Election {
    Stock,
    Cash,
}

/// A ratio, as an exact integer numerator and denominator (DEC-055, DOM-113).
///
/// A one-for-three ratio has no finite decimal expansion, so a decimal would leave a residue
/// that grows across applications and the requirement that emitted quantities sum exactly would
/// stop being well defined. Both halves are non-zero: a zero denominator is not a ratio, and a
/// zero numerator would scale every parcel out of existence. The arithmetic that applies one —
/// effective quantity (FIF-061) and transfer emission (FIF-063) — is not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ratio {
    numerator: NonZeroU32,
    denominator: NonZeroU32,
}

impl Ratio {
    /// `numerator` new units for every `denominator` held.
    #[must_use]
    pub fn new(numerator: NonZeroU32, denominator: NonZeroU32) -> Self {
        Self {
            numerator,
            denominator,
        }
    }

    #[must_use]
    pub fn numerator(self) -> NonZeroU32 {
        self.numerator
    }

    #[must_use]
    pub fn denominator(self) -> NonZeroU32 {
        self.denominator
    }
}

/// What the user supplied, because the export does not carry it [DOM-097].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Supplied {
    /// How many shares an event issued, where no column states them.
    ShareCount(Quantity),
    /// Whether a dividend was taken in stock or in cash.
    Election(Election),
    /// The security a holding was exchanged into, and at what ratio.
    Exchange { target: Isin, ratio: Ratio },
}

/// Information the user supplied, kept as traceably as imported information [DOM-048].
///
/// Read-only after construction, for the reason a source record is: what was supplied is a
/// record of what was said, and a mistake is corrected by deleting the entry and supplying a
/// new one (SRV-053, SRV-027), not by editing this one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualEntry {
    account: Account,
    security: Isin,
    supplied: Supplied,
    answers: Vec<RecordIdentity>,
}

impl ManualEntry {
    /// An entry against `security` in `account`, answering the records named by `answers`.
    ///
    /// The identities are taken in the caller's order, which is the order the queue showed the
    /// rows in. The cardinality is not constrained here: what an entry attached to no row means
    /// is CLI-039's and SRV-026's, and refusing an empty list would decide it.
    #[must_use]
    pub fn new(
        account: Account,
        security: Isin,
        supplied: Supplied,
        answers: impl IntoIterator<Item = RecordIdentity>,
    ) -> Self {
        Self {
            account,
            security,
            supplied,
            answers: answers.into_iter().collect(),
        }
    }

    #[must_use]
    pub fn account(&self) -> &Account {
        &self.account
    }

    /// The security this is about, by its natural key.
    #[must_use]
    pub fn security(&self) -> &Isin {
        &self.security
    }

    #[must_use]
    pub fn supplied(&self) -> &Supplied {
        &self.supplied
    }

    /// The identities of the source records this entry answers [DOM-098], [DOM-099].
    #[must_use]
    pub fn answers(&self) -> &[RecordIdentity] {
        &self.answers
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use rust_decimal_macros::dec;

    use super::*;
    use crate::decimal::Scaled;
    use crate::entities::{Order, SourceRecord};
    use crate::identity::{IdentitySource, identify};

    fn account() -> Account {
        Account::new("Saxo", "69900/1000000")
    }

    fn isin() -> Isin {
        Isin::new("NL0000009538")
    }

    fn parsed() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("Acties".to_owned(), "Keuzedividend".to_owned()),
            ("Positie-ID".to_owned(), "1234567".to_owned()),
        ])
    }

    /// An imported row, as an importer would have produced it.
    fn record() -> SourceRecord {
        SourceRecord::new(
            identify(&account(), &IdentitySource::BrokerReference("BK-9001")),
            Order::new(3),
            "Keuzedividend",
            parsed(),
        )
    }

    /// An entry holds the account, the security, what was supplied and the records it answers
    /// [DOM-097], [DOM-098].
    #[test]
    fn an_entry_holds_its_account_security_supplied_value_and_records() {
        let entry = ManualEntry::new(
            account(),
            isin(),
            Supplied::ShareCount(Quantity::new(dec!(12.5))),
            [record().identity().clone()],
        );

        assert_eq!(entry.account(), &account());
        assert_eq!(entry.security(), &isin());
        assert_eq!(
            entry.supplied(),
            &Supplied::ShareCount(Quantity::new(dec!(12.5)))
        );
        assert_eq!(entry.answers(), &[record().identity().clone()]);
    }

    /// The supplied value is the set DOM-097 names, and each shape carries only its own fields
    /// [DOM-097].
    #[test]
    fn the_supplied_value_is_the_specified_set() {
        let all = [
            Supplied::ShareCount(Quantity::new(dec!(0.43))),
            Supplied::Election(Election::Stock),
            Supplied::Exchange {
                target: Isin::new("US8816242098"),
                ratio: ratio(1, 3),
            },
        ];

        // Exhaustive by construction: a new variant makes this match fail to compile.
        for supplied in &all {
            match supplied {
                Supplied::ShareCount(quantity) => assert!(!quantity.is_zero()),
                Supplied::Election(election) => {
                    assert!(matches!(election, Election::Stock | Election::Cash));
                }
                Supplied::Exchange { target, ratio } => {
                    assert_eq!(target.as_str(), "US8816242098");
                    assert_eq!(ratio.numerator().get(), 1);
                }
            }
        }
        assert_eq!(all.len(), 3);
    }

    fn ratio(numerator: u32, denominator: u32) -> Ratio {
        Ratio::new(
            NonZeroU32::new(numerator).expect("a non-zero numerator"),
            NonZeroU32::new(denominator).expect("a non-zero denominator"),
        )
    }

    /// A ratio is an exact integer pair and keeps both halves as stated, so a one-for-three is
    /// never flattened to a decimal (DEC-055, DOM-113).
    #[test]
    fn a_ratio_keeps_its_integer_pair() {
        let one_for_three = ratio(1, 3);

        assert_eq!(one_for_three.numerator().get(), 1);
        assert_eq!(one_for_three.denominator().get(), 3);
        assert_ne!(one_for_three, ratio(2, 6));
    }

    /// The entry names its records by broker identity, not by an internal key, so dropping the
    /// records an import created leaves the entry naming exactly what it named before
    /// [DOM-099], [DOM-100].
    #[test]
    fn an_entry_outlives_the_records_it_answers() {
        let imported = vec![record(), record()];
        let entry = ManualEntry::new(
            account(),
            isin(),
            Supplied::Election(Election::Stock),
            imported.iter().map(|row| row.identity().clone()),
        );
        let named: Vec<String> = entry
            .answers()
            .iter()
            .map(|identity| identity.as_str().to_owned())
            .collect();

        drop(imported); // the undo of the import that created them [DOM-100]

        assert_eq!(
            entry
                .answers()
                .iter()
                .map(|identity| identity.as_str().to_owned())
                .collect::<Vec<_>>(),
            named
        );
    }

    /// Re-importing the same row reproduces the identity the entry holds, which is what lets it
    /// reconnect; the reconnection itself is FIF-047's [DOM-099].
    #[test]
    fn a_reimported_row_carries_the_identity_the_entry_named() {
        let entry = ManualEntry::new(
            account(),
            isin(),
            Supplied::Exchange {
                target: Isin::new("CA8935841014"),
                ratio: ratio(1, 4),
            },
            [record().identity().clone()],
        );

        let reimported = record();

        assert_eq!(entry.answers(), &[reimported.identity().clone()]);
    }

    /// The same row in another account is another record, so an entry does not reconnect to it
    /// [DOM-099], [DOM-024].
    #[test]
    fn an_entry_does_not_reconnect_across_accounts() {
        let entry = ManualEntry::new(
            account(),
            isin(),
            Supplied::ShareCount(Quantity::new(dec!(1))),
            [record().identity().clone()],
        );

        let elsewhere = identify(
            &Account::new("Saxo", "69900/2000000"),
            &IdentitySource::BrokerReference("BK-9001"),
        );

        assert_ne!(entry.answers(), &[elsewhere]);
    }
}
