//! What makes a source record the same record twice [DOM-022].
//!
//! Import is idempotent: reading a file that has already been read produces no new records. That
//! rests entirely on identity, so identity is here rather than inside any one importer.
//!
//! # Two sources, one shape
//!
//! A format either has a stable reference of its own or it does not [DOM-023]:
//!
//! * **A broker reference.** Trade Republic's `transaction_id` is a UUID; Saxo has a tuple of
//!   booking ids. Preferred, because it survives a re-export that changes formatting, and
//!   because two genuinely distinct trades can share date, security, quantity, price and fees —
//!   which a hash of those fields cannot tell apart.
//! * **A hash of the parsed business fields.** The fallback when a format offers nothing stable.
//!
//! # Always scoped to the account
//!
//! Both are scoped to the account [DOM-024], so the same row imported into two accounts yields
//! two records rather than one. Without it, a stock transferred between your own accounts would
//! silently deduplicate against itself.
//!
//! The scoping is applied here rather than left to each importer, because an importer that
//! forgot it would produce a bug visible only when a second account existed.
//!
//! # Which fields a hash covers
//!
//! The caller chooses, and each format's rule is its own item's. This module fixes only how the
//! chosen fields become a digest: in the caller's order, length-prefixed so that two different
//! field lists cannot collide by running together, and hashed with SHA-256.
//!
//! **The digest string is persisted.** It is what a source record stores and what a manual entry
//! names to find its rows again [DOM-099], so the algorithm, the hex casing and the framing are
//! part of the on-disk format. Changing any of them turns every row already imported into a new
//! row on the next import and reconnects no manual entry; it is a migration, not a refactor.
//!
//! A caller that supplies an empty reference or an empty field list gets one identity for every
//! row of the file, which is data loss rather than idempotency: 400 rows become one record,
//! silently. A format that can produce one has a mapping defect, and rejecting it belongs to that
//! format's item.

use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use crate::entities::{Account, RecordIdentity};

/// What a format offers to identify a row [DOM-023].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentitySource<'a> {
    /// The broker's own stable reference for the row.
    BrokerReference(&'a str),
    /// The parsed business fields, in the order the format states, when it has no reference.
    ParsedFields(&'a [&'a str]),
}

/// The identity of a row in `account` [DOM-023], [DOM-024].
#[must_use]
pub fn identify(account: &Account, source: &IdentitySource<'_>) -> RecordIdentity {
    let scope = account_scope(account);
    match source {
        IdentitySource::BrokerReference(reference) => {
            RecordIdentity::new(format!("{scope}:ref:{reference}"))
        }
        IdentitySource::ParsedFields(fields) => {
            RecordIdentity::new(format!("{scope}:hash:{}", digest(fields)))
        }
    }
}

/// The account part of an identity.
///
/// Length-prefixed so that a broker and an id cannot run together into the same string as a
/// different pair would. Colon-joined alone, `("a", "b:c")` and `("a:b", "c")` both render
/// `a:b:c`; an account id like Saxo's `69900/1000000` shows punctuation here is ordinary.
fn account_scope(account: &Account) -> String {
    let broker = account.broker();
    let id = account.id();
    format!("{}:{broker}:{}:{id}", broker.len(), id.len())
}

/// A digest of `fields`, in the given order and length-prefixed for the same reason.
fn digest(fields: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for field in fields {
        hasher.update(field.len().to_string().as_bytes());
        hasher.update(b":");
        hasher.update(field.as_bytes());
    }
    hasher
        .finalize()
        .iter()
        .fold(String::new(), |mut digest, byte| {
            let _ = write!(digest, "{byte:02x}");
            digest
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn saxo() -> Account {
        Account::new("Saxo", "69900/1000000")
    }

    fn trade_republic() -> Account {
        Account::new("Trade Republic", "DEFAULT")
    }

    /// A broker reference identifies a row [DOM-023].
    #[test]
    fn a_broker_reference_identifies_a_row() {
        let uuid = IdentitySource::BrokerReference("c016ee6f-7c1e-4a8b-a39c-f7d253fe1841");

        let once = identify(&trade_republic(), &uuid);
        let again = identify(&trade_republic(), &uuid);

        assert_eq!(once, again, "the same row must identify the same way");
    }

    /// So do the parsed fields, when a format has no reference [DOM-023].
    #[test]
    fn parsed_fields_identify_a_row_when_there_is_no_reference() {
        let fields = ["2024-05-02", "BUY", "IE000Y77LGG9", "55", "90.24"];
        let source = IdentitySource::ParsedFields(&fields);

        assert_eq!(identify(&saxo(), &source), identify(&saxo(), &source));
    }

    /// The same rows identify the same way every time, which is what lets an import recognize
    /// a row it already holds. That an import then stores nothing new is demonstrated by the
    /// importer items, which have a store to count records in.
    #[test]
    fn re_importing_the_same_rows_produces_no_new_identities() {
        let rows = [
            IdentitySource::BrokerReference("a"),
            IdentitySource::BrokerReference("b"),
            IdentitySource::BrokerReference("c"),
        ];

        let first: BTreeSet<_> = rows.iter().map(|row| identify(&saxo(), row)).collect();
        let second: BTreeSet<_> = rows.iter().map(|row| identify(&saxo(), row)).collect();

        assert_eq!(first.len(), 3);
        assert_eq!(first, second);
    }

    /// A wider re-export identifies the rows it shares exactly as the narrower one did, which
    /// is the property a year-to-date re-import rests on.
    #[test]
    fn a_wider_re_export_keeps_the_identities_it_shares() {
        let march = ["a", "b"];
        let december = ["a", "b", "c", "d"];

        let from = |refs: &[&str]| -> BTreeSet<_> {
            refs.iter()
                .map(|r| identify(&saxo(), &IdentitySource::BrokerReference(r)))
                .collect()
        };

        assert!(from(&march).is_subset(&from(&december)));
        assert_eq!(from(&december).len(), 4);
    }

    /// Identical rows in two accounts stay distinct [DOM-024].
    #[test]
    fn identical_rows_in_two_accounts_are_distinct() {
        let row = IdentitySource::BrokerReference("shared-reference");

        assert_ne!(identify(&saxo(), &row), identify(&trade_republic(), &row));
    }

    /// And the same for the hashed path, which is where forgetting the scope would be easiest
    /// [DOM-024].
    #[test]
    fn identical_hashed_rows_in_two_accounts_are_distinct() {
        let fields = ["2024-05-02", "BUY", "IE000Y77LGG9"];
        let row = IdentitySource::ParsedFields(&fields);

        assert_ne!(identify(&saxo(), &row), identify(&trade_republic(), &row));
    }

    /// Two accounts whose broker and id flatten to the same string are still distinct, because
    /// the scope is length-prefixed [DOM-024].
    ///
    /// The values carry the separator on purpose: without a length prefix `("a", "b:c")` and
    /// `("a:b", "c")` both render `a:b:c`, and an account id like Saxo's `69900/1000000` shows
    /// that punctuation in these fields is ordinary. A pair that does not contain the separator
    /// cannot collide however the scope is built, so it would not test anything.
    #[test]
    fn account_scoping_cannot_collide_by_flattening() {
        let row = IdentitySource::BrokerReference("x");

        assert_ne!(
            identify(&Account::new("a", "b:c"), &row),
            identify(&Account::new("a:b", "c"), &row)
        );
    }

    /// Different rows identify differently [DOM-023].
    #[test]
    fn different_rows_identify_differently() {
        let one = ["2024-05-02", "BUY", "55"];
        let other = ["2024-05-02", "BUY", "15"];

        assert_ne!(
            identify(&saxo(), &IdentitySource::ParsedFields(&one)),
            identify(&saxo(), &IdentitySource::ParsedFields(&other))
        );
    }

    /// Field order is part of the identity, so two rows whose values are permuted differ.
    #[test]
    fn field_order_is_part_of_the_identity() {
        let one = ["BUY", "55"];
        let other = ["55", "BUY"];

        assert_ne!(
            identify(&saxo(), &IdentitySource::ParsedFields(&one)),
            identify(&saxo(), &IdentitySource::ParsedFields(&other))
        );
    }

    /// Fields cannot collide by running together.
    ///
    /// Two pairs, because the digest is framed as `{len}:{field}` and each half of that framing
    /// needs its own witness — a pair that only collides when both are removed would let either
    /// one be dropped unnoticed.
    ///
    /// Drop the length and `["a:b"]` and `["a","b"]` both render `:a:b`. Drop the separator and
    /// `["9abcdefghi"]` renders `10` + the field while `["0","abcdefghi"]` renders `1` + `0` +
    /// `9` + the field — the same string.
    #[test]
    fn fields_cannot_collide_by_flattening() {
        let identity_of =
            |fields: &[&str]| identify(&saxo(), &IdentitySource::ParsedFields(fields));

        assert_ne!(
            identity_of(&["a:b"]),
            identity_of(&["a", "b"]),
            "the length prefix is what keeps these apart"
        );
        assert_ne!(
            identity_of(&["9abcdefghi"]),
            identity_of(&["0", "abcdefghi"]),
            "the separator is what keeps these apart"
        );
    }

    /// A reference and a hash never collide, so a format that gains a reference later cannot
    /// alias a row it identified by hash before [DOM-023].
    #[test]
    fn a_reference_and_a_hash_never_collide() {
        let fields = ["anything"];

        assert_ne!(
            identify(&saxo(), &IdentitySource::BrokerReference("anything")),
            identify(&saxo(), &IdentitySource::ParsedFields(&fields))
        );
    }

    /// An empty field list yields one identity for every row, which is why a format that can
    /// produce one has a defect its own item must reject. Recorded here rather than guarded,
    /// because this module cannot tell an empty list from a deliberate one.
    #[test]
    fn an_empty_field_list_collapses_every_row_to_one_identity() {
        let empty: [&str; 0] = [];
        let source = IdentitySource::ParsedFields(&empty);

        assert_eq!(identify(&saxo(), &source), identify(&saxo(), &source));
        assert!(identify(&saxo(), &source).as_str().contains(":hash:"));
    }
}
