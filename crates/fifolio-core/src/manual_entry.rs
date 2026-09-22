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
//! [`Supplied`] is the set [DOM-097] names, which is exactly what the completion queue asks for
//! (CLI-020), one shape per row of its table: a stock-or-cash election, carrying the share count
//! when the answer is stock (`Keuzedividend`, IMP-SAXO-018); a split's ratio on its own (`Stock
//! split`); an exchange's target security and ratio (`Omwisseling`); and a disposed quantity with
//! an *optional* target security, for a cash merger, tender or partial buyback (`Fusie`,
//! `Terugkoopaanbod`, IMP-SAXO-031). The queue's remaining row, a transfer out, asks for nothing:
//! it is approved like any disposal. DEC-062 withdrew DEC-060's claim that the set closed at three
//! shapes, having found those pending rows with no shape to answer them.
//!
//! The share count is a field of the stock election rather than a shape of its own, so a count
//! with no election behind it cannot be constructed [DOM-097]. DEC-060 stands on its own subject:
//! an acquisition date is not among these shapes [DOM-122] — a transferred parcel's date is fixed
//! at import and never corrected (SRV-054, IMP-SAXO-016) — so there is no variant for it and no
//! accessor that could set one.

use std::num::NonZeroU32;

use crate::decimal::Quantity;
use crate::entities::{Account, Isin, RecordIdentity};

/// Which of a stock-or-cash dividend the user elected [DOM-097].
///
/// The share count belongs to the stock branch rather than standing beside the election, because
/// DOM-097 asks for it only "if stock": a count with no election behind it is not a shape the
/// queue offers, and making it a field is what stops one being recorded.
///
/// What each choice derives — stock a `buy` whose origin is a stock dividend, cash nothing at
/// all — is the Saxo importer's rule (IMP-SAXO-018), not this type's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Election {
    /// Taken in shares, `shares` of them, because no column states the count.
    Stock {
        shares: Quantity,
    },
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
///
/// One variant per completion-queue case that asks for something (CLI-020) and no others.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Supplied {
    /// Whether a dividend was taken in stock or in cash, and how many shares if stock.
    Election(Election),
    /// The ratio a split applied, which no column states.
    Split(Ratio),
    /// The security a holding was exchanged into, and at what ratio.
    Exchange { target: Isin, ratio: Ratio },
    /// How much a cash merger, tender or partial buyback disposed of, and the security received
    /// in return where there was one — a tender paid entirely in cash receives none.
    Disposal {
        quantity: Quantity,
        target: Option<Isin>,
    },
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
            Supplied::Election(Election::Stock {
                shares: Quantity::new(dec!(12.5)),
            }),
            [record().identity().clone()],
        );

        assert_eq!(entry.account(), &account());
        assert_eq!(entry.security(), &isin());
        assert_eq!(
            entry.supplied(),
            &Supplied::Election(Election::Stock {
                shares: Quantity::new(dec!(12.5)),
            })
        );
        assert_eq!(entry.answers(), &[record().identity().clone()]);
    }

    /// Completion queue, "Stock or cash dividend", answered stock (CLI-020, IMP-SAXO-018): the
    /// election and the share count the `Keuzedividend` row does not state [DOM-097].
    #[test]
    fn a_stock_or_cash_dividend_taken_in_stock_supplies_the_election_and_the_count() {
        let supplied = Supplied::Election(Election::Stock {
            shares: Quantity::new(dec!(0.43)),
        });

        let Supplied::Election(Election::Stock { shares }) = supplied else {
            panic!("a stock election")
        };
        assert_eq!(shares.get(), dec!(0.43));
    }

    /// Completion queue, "Stock or cash dividend", answered cash (CLI-020, IMP-SAXO-018): the
    /// election alone, because a cash dividend issues no shares to count [DOM-097].
    #[test]
    fn a_stock_or_cash_dividend_taken_in_cash_supplies_the_election_alone() {
        assert_eq!(
            Supplied::Election(Election::Cash),
            Supplied::Election(Election::Cash)
        );
        assert_ne!(
            Supplied::Election(Election::Cash),
            Supplied::Election(Election::Stock {
                shares: Quantity::zero()
            })
        );
    }

    /// Completion queue, "Split" (CLI-020), the `Stock split` row the Saxo importer marks
    /// pending: the ratio and nothing else [DOM-097].
    #[test]
    fn a_split_supplies_the_ratio_alone() {
        let supplied = Supplied::Split(ratio(3, 1));

        let Supplied::Split(applied) = supplied else {
            panic!("a split ratio")
        };
        assert_eq!(applied, ratio(3, 1));
    }

    /// Completion queue, "Exchange or share-class swap" (CLI-020), the `Omwisseling` row: the
    /// target security and the ratio [DOM-097].
    #[test]
    fn an_exchange_supplies_a_target_security_and_a_ratio() {
        let supplied = Supplied::Exchange {
            target: Isin::new("US8816242098"),
            ratio: ratio(1, 3),
        };

        let Supplied::Exchange { target, ratio } = supplied else {
            panic!("an exchange")
        };
        assert_eq!(target.as_str(), "US8816242098");
        assert_eq!(ratio.numerator().get(), 1);
    }

    /// Completion queue, "Cash merger, tender, partial buyback" (CLI-020), the `Fusie` and
    /// `Terugkoopaanbod` rows (IMP-SAXO-031): the quantity disposed, with a target security
    /// where one was received and none where the payout was all cash [DOM-097].
    #[test]
    fn a_cash_merger_tender_or_buyback_supplies_a_quantity_and_an_optional_target() {
        let all_cash = Supplied::Disposal {
            quantity: Quantity::new(dec!(40)),
            target: None,
        };
        let part_stock = Supplied::Disposal {
            quantity: Quantity::new(dec!(40)),
            target: Some(Isin::new("CA8935841014")),
        };

        assert_ne!(part_stock, all_cash);
        let Supplied::Disposal { quantity, target } = all_cash else {
            panic!("a disposal")
        };
        assert_eq!(quantity.get(), dec!(40));
        assert_eq!(target, None);
    }

    /// The supplied value is the set DOM-097 names and no more: one shape per completion-queue
    /// case that asks for something, with a share count reachable only through a stock election
    /// [DOM-097].
    #[test]
    fn the_supplied_value_is_the_specified_set() {
        let all = [
            Supplied::Election(Election::Stock {
                shares: Quantity::new(dec!(0.43)),
            }),
            Supplied::Election(Election::Cash),
            Supplied::Split(ratio(3, 1)),
            Supplied::Exchange {
                target: Isin::new("US8816242098"),
                ratio: ratio(1, 3),
            },
            Supplied::Disposal {
                quantity: Quantity::new(dec!(40)),
                target: None,
            },
        ];

        // Exhaustive by construction: a new variant makes these matches fail to compile, and a
        // share count outside a stock election has nowhere to appear.
        let shapes: Vec<&str> = all
            .iter()
            .map(|supplied| match supplied {
                Supplied::Election(Election::Stock { .. }) => "dividend elected in stock",
                Supplied::Election(Election::Cash) => "dividend elected in cash",
                Supplied::Split(_) => "split",
                Supplied::Exchange { .. } => "exchange or share-class swap",
                Supplied::Disposal { .. } => "cash merger, tender, partial buyback",
            })
            .collect();

        assert_eq!(
            shapes,
            [
                "dividend elected in stock",
                "dividend elected in cash",
                "split",
                "exchange or share-class swap",
                "cash merger, tender, partial buyback",
            ]
        );
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
            Supplied::Election(Election::Cash),
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
            Supplied::Split(ratio(2, 1)),
            [record().identity().clone()],
        );

        let elsewhere = identify(
            &Account::new("Saxo", "69900/2000000"),
            &IdentitySource::BrokerReference("BK-9001"),
        );

        assert_ne!(entry.answers(), &[elsewhere]);
    }
}
