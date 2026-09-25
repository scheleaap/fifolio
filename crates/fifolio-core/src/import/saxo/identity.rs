//! Which account a Saxo row belongs to, and which row it is.
//!
//! Both questions are answered off the `Transacties` row alone, which is the sheet a source
//! record is made from; neither detail sheet is consulted.
//!
//! # The account is the Depot, not the sub-account
//!
//! `Rekening-ID` names a per-currency sub-account: `69900/1000000EUR`, `...USD` and `...CAD` are
//! three spellings of one Depot. The suffix is stripped, because FIFO applies per Depot
//! [DOM-003], [IMP-SAXO-005]: left on, a security bought in USD and sold after the position moved
//! to the EUR sub-account would find no parcel to consume.
//!
//! `Klant-id` is the client and never the account [IMP-SAXO-006]. It is read nowhere in this
//! module, which is the whole of what that requirement asks.
//!
//! # Identity is the first populated of four columns
//!
//! There is no single id column. Identity is the first populated of `Transactie-ID`,
//! `Bk Record Id`, `Booking Id` and `Corporate action-Id` [IMP-SAXO-007], and every row in the
//! sample carries at least one.
//!
//! `Corporate action-Id` is shared by every row of one event, so the fallback is not unique on
//! its own. A row identified by it is identified by that id, its `Acties`, its `Boekingsbedrag`
//! and its ordinal within its `Corporate action-Id` group [IMP-SAXO-008] — see [`identities`].
//! Should two rows still produce one identity, the file is **refused** [IMP-SAXO-024] rather than
//! deduplicated: deduplicating would drop a row, and money would leave the event silently.
//!
//! # The ordinal counts oldest first
//!
//! The ordinal is counted in the file's normalized, oldest-first order, not in file order
//! (DEC-082). Saxo writes newest first [IMP-SAXO-025], so a row a later export adds to a group
//! lands *above* the rows already in it; counted in file order it would shift every existing
//! ordinal and change identities that must stay stable across re-imports [SRV-015]. Counted
//! oldest first, it takes the next ordinal and the existing rows keep theirs.
//!
//! Normalized means the direction [`DIRECTION`](super::DIRECTION) declares (FIF-083), and
//! nothing more: the full ordering key is FIF-066's and is blocked (OQ-013). DEC-082's premise is
//! that a row a later export adds to a group lands at the newest end, and direction alone is what
//! puts that end last.
//!
//! The group is every row carrying the `Corporate action-Id`, including the rows another column
//! identifies: `importers.md` names the group by the id alone. Counting only the rows that fall
//! through would renumber them whenever a sibling gained or lost a booking id between exports.
//!
//! # Where the refusal fires
//!
//! [`check_identities`] is separate from [`SaxoWorkbook::read`](super::SaxoWorkbook::read)
//! deliberately. Reading a workbook answers what the file holds and is what the fixture tests and
//! any later inspection rest on; refusing a file for a duplicate identity is a judgement about
//! importing it. An import calls both, so the file is still rejected as a whole, but a workbook
//! that cannot be imported today can still be read.

use std::collections::BTreeMap;

use super::{DIRECTION, SaxoError, field};
use crate::import::reader::SourceRow;
use crate::ordering::FileDirection;

/// The column naming the account [IMP-SAXO-005].
const ACCOUNT_COLUMN: &str = "Rekening-ID";

/// The identity columns, in the precedence `importers.md` states [IMP-SAXO-007].
///
/// Spelled with ordinary spaces; the file spells two of them with non-breaking spaces and the
/// lookup normalizes [IMP-SAXO-002].
const IDENTITY_COLUMNS: [&str; 4] = [
    "Transactie-ID",
    "Bk Record Id",
    "Booking Id",
    CORPORATE_ACTION_COLUMN,
];

/// The identity column shared by every row of one corporate action [IMP-SAXO-008].
const CORPORATE_ACTION_COLUMN: &str = "Corporate action-Id";

/// The length of the currency code `Rekening-ID` is suffixed with.
const CURRENCY_CODE: usize = 3;

/// The Depot `row` belongs to: its `Rekening-ID` without the per-currency suffix
/// [DOM-003], [IMP-SAXO-005].
///
/// # Errors
///
/// When the row carries no `Rekening-ID` column, or carries it blank: a row that names no
/// account cannot be scoped to one, and guessing the file's other rows' account for it would
/// file a movement against a Depot the file did not say it belonged to.
pub fn account(row: &SourceRow) -> Result<&str, SaxoError> {
    let account = field(row, ACCOUNT_COLUMN).ok_or_else(|| SaxoError::MissingColumn {
        header: ACCOUNT_COLUMN.to_owned(),
    })?;
    if account.is_empty() {
        return Err(SaxoError::NoAccount);
    }
    Ok(without_currency_suffix(account))
}

/// The broker reference `row` carries: the first of the four identity columns it populates
/// [IMP-SAXO-007].
///
/// This is not yet the row's identity when the column is `Corporate action-Id`, which the
/// whole-file [`identities`] composes [IMP-SAXO-008]; a row alone cannot know its ordinal.
///
/// # Errors
///
/// When the row populates none of them. That is a row that cannot be identified, which the
/// import framework reports as a failed row, refusing the file with every other failed row
/// named [SRV-058]: every row of the sample carries one, so such a row is a shape neither
/// `design/` nor the sample describes.
pub fn reference(row: &SourceRow) -> Result<&str, SaxoError> {
    populated(row).map(|(_, value)| value)
}

/// Every row's identity, parallel to `rows`, which are in file order [IMP-SAXO-007],
/// [IMP-SAXO-008].
///
/// A row identified by `Corporate action-Id` is identified by
/// `{id}|{Acties}|{Boekingsbedrag}|{ordinal}`, the ordinal counting from 1 within the group in
/// oldest-first order. The ordinal comes last and holds no `|`, so two rows of one group, whose
/// ordinals differ, can never spell one identity whatever their labels hold; and a composite
/// always holds a `|`, so it never equals a bare id from another column.
///
/// # Errors
///
/// Per row: when it populates none of the four columns, or when it falls through to
/// `Corporate action-Id` and does not carry `Acties` or `Boekingsbedrag`.
#[must_use]
pub fn identities(rows: &[SourceRow]) -> Vec<Result<String, SaxoError>> {
    let ordinals = group_ordinals(rows);
    rows.iter()
        .zip(ordinals)
        .map(|(row, ordinal)| match populated(row)? {
            (CORPORATE_ACTION_COLUMN, group) => {
                let ordinal = ordinal.expect("a row carrying a group id has an ordinal in it");
                Ok(format!(
                    "{group}|{}|{}|{ordinal}",
                    required(row, "Acties")?,
                    required(row, "Boekingsbedrag")?
                ))
            }
            (_, value) => Ok(value.to_owned()),
        })
        .collect()
}

/// Refuses a file in which two rows produce one identity [IMP-SAXO-024].
///
/// Rows whose identity cannot be read are not compared: they fail one at a time and do not make
/// each other a duplicate.
///
/// # Errors
///
/// When two rows of `rows` identify the same, naming both and the identity they share.
pub fn check_identities(rows: &[SourceRow]) -> Result<(), SaxoError> {
    identities(rows)
        .into_iter()
        .enumerate()
        .filter_map(|(index, found)| found.ok().map(|found| (index, found)))
        .try_fold(BTreeMap::new(), |mut seen, (index, found)| {
            match seen.insert(found.clone(), index) {
                Some(first) => Err(SaxoError::DuplicateIdentity {
                    identity: found,
                    first: file_row(first),
                    second: file_row(index),
                }),
                None => Ok(seen),
            }
        })
        .map(|_| ())
}

/// An account id with its per-currency suffix removed, and unchanged when it carries none
/// [IMP-SAXO-005].
///
/// A suffix is three ASCII uppercase letters at the end of an otherwise numeric id — an ISO 4217
/// code, which is what the export writes: `69900/1000000` and a currency. Recognizing the *shape*
/// rather than a list of the three codes the sample carries is deliberate: an unlisted currency
/// would otherwise stand as a Depot of its own and split that security's FIFO queue in two, which
/// is a wrong gain rather than a refusal. The digit before the code is what keeps the rule from
/// eating an account id that genuinely ends in letters.
fn without_currency_suffix(account: &str) -> &str {
    let split = account.len().saturating_sub(CURRENCY_CODE);
    match account.split_at_checked(split) {
        Some((base, suffix))
            if base.ends_with(|character: char| character.is_ascii_digit())
                && suffix.len() == CURRENCY_CODE
                && suffix.chars().all(|letter| letter.is_ascii_uppercase()) =>
        {
            base
        }
        _ => account,
    }
}

/// The first identity column `row` populates, and its value [IMP-SAXO-007].
fn populated(row: &SourceRow) -> Result<(&'static str, &str), SaxoError> {
    IDENTITY_COLUMNS
        .iter()
        .find_map(|&column| {
            field(row, column)
                .filter(|value| !value.is_empty())
                .map(|value| (column, value))
        })
        .ok_or(SaxoError::NoIdentity)
}

/// A column `row` must carry, blank or not.
fn required<'a>(row: &'a SourceRow, header: &str) -> Result<&'a str, SaxoError> {
    field(row, header).ok_or_else(|| SaxoError::MissingColumn {
        header: header.to_owned(),
    })
}

/// Each row's 1-based ordinal within its `Corporate action-Id` group, parallel to `rows`, counted
/// in normalized, oldest-first order (DEC-082), and `None` for a row carrying no such id.
fn group_ordinals(rows: &[SourceRow]) -> Vec<Option<usize>> {
    let oldest_first: Vec<usize> = match DIRECTION {
        FileDirection::OldestFirst => (0..rows.len()).collect(),
        FileDirection::NewestFirst => (0..rows.len()).rev().collect(),
    };
    let mut ordinals = vec![None; rows.len()];
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for index in oldest_first {
        if let Some(group) =
            field(&rows[index], CORPORATE_ACTION_COLUMN).filter(|id| !id.is_empty())
        {
            let count = counts.entry(group).or_default();
            *count += 1;
            ordinals[index] = Some(*count);
        }
    }
    ordinals
}

/// The file's own row number for a `Transacties` row index, its header row counted, so a refusal
/// names what a spreadsheet shows.
fn file_row(index: usize) -> usize {
    index + 2
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::entities::Account;
    use crate::identity::{IdentitySource, identify};
    use crate::import::saxo::Sheet;

    /// A `Transacties` row spelled as the file spells it — non-breaking spaces and all — with
    /// only the named columns populated.
    fn transacties_row(values: &[(&str, &str)]) -> SourceRow {
        let columns = Sheet::Transacties
            .headers()
            .iter()
            .map(|header| {
                let value = values
                    .iter()
                    .find(|(name, _)| name.split_whitespace().eq(header.split_whitespace()))
                    .map_or("", |(_, value)| *value);
                ((*header).to_owned(), value.to_owned())
            })
            .collect();
        SourceRow::new(columns, "")
    }

    /// The three per-currency sub-accounts are one Depot [DOM-003], [IMP-SAXO-005].
    #[test]
    fn the_currency_suffix_is_stripped_from_the_account() {
        let accounts: Vec<String> = ["EUR", "USD", "CAD"]
            .iter()
            .map(|currency| {
                transacties_row(&[("Rekening-ID", &format!("69900/1000000{currency}"))])
            })
            .map(|row| account(&row).expect("the row names an account").to_owned())
            .collect();

        assert_eq!(
            accounts,
            ["69900/1000000", "69900/1000000", "69900/1000000"]
        );
    }

    /// Only a three-letter uppercase tail is a currency code; anything else is the account
    /// itself and is left alone [IMP-SAXO-005].
    #[test]
    fn an_account_without_a_currency_suffix_is_unchanged() {
        for id in [
            "69900/1000000",
            "69900/1000000eur",
            "69900/10000EURO",
            "EUR",
        ] {
            let row = transacties_row(&[("Rekening-ID", id)]);

            assert_eq!(account(&row), Ok(id), "{id} carries no suffix");
        }
    }

    /// `Klant-id` is the client, and a row carrying one but no account still names no account
    /// [IMP-SAXO-006].
    #[test]
    fn the_client_id_is_never_the_account() {
        let row = transacties_row(&[("Klant-id", "12345678"), ("Rekening-ID", "")]);

        assert_eq!(account(&row), Err(SaxoError::NoAccount));

        // Two clients on one Depot — a joint account — are still one account.
        let first = transacties_row(&[
            ("Klant-id", "12345678"),
            ("Rekening-ID", "69900/1000000EUR"),
        ]);
        let second = transacties_row(&[
            ("Klant-id", "87654321"),
            ("Rekening-ID", "69900/1000000USD"),
        ]);

        assert_eq!(account(&first), account(&second));
    }

    /// Identity is the first *populated* of the four, so a blank column falls through
    /// [IMP-SAXO-007].
    #[test]
    fn identity_is_the_first_populated_of_the_four_columns() {
        let columns = [
            ("Transactie-ID", "3000000001"),
            ("Bk Record Id", "3000000002"),
            ("Booking Id", "40000000003"),
            ("Corporate action-Id", "900004"),
        ];

        for skipped in 0..columns.len() {
            let row = transacties_row(&columns[skipped..]);

            assert_eq!(reference(&row), Ok(columns[skipped].1));
        }
    }

    /// The non-breaking spellings of `Bk Record Id` and `Booking Id` are matched
    /// [IMP-SAXO-002], [IMP-SAXO-007].
    #[test]
    fn an_identity_column_spelled_with_non_breaking_spaces_is_found() {
        let row = transacties_row(&[("Bk\u{a0}Record\u{a0}Id", "3000000002")]);

        assert_eq!(reference(&row), Ok("3000000002"));
    }

    /// A row populating none of the four cannot be identified [IMP-SAXO-007].
    #[test]
    fn a_row_carrying_none_of_the_four_has_no_identity() {
        let row = transacties_row(&[("Rekening-ID", "69900/1000000EUR"), ("Acties", "Koop")]);

        assert_eq!(reference(&row), Err(SaxoError::NoIdentity));
    }

    /// Distinct identities pass, and an unidentifiable row does not make another one a duplicate
    /// [IMP-SAXO-024].
    #[test]
    fn distinct_identities_are_accepted() {
        let rows = [
            transacties_row(&[("Transactie-ID", "3000000001")]),
            transacties_row(&[("Bk Record Id", "3000000002")]),
            transacties_row(&[("Acties", "Koop")]),
            transacties_row(&[("Acties", "Verkoop")]),
        ];

        assert_eq!(check_identities(&rows), Ok(()));
    }

    /// Two rows producing one identity reject the file rather than deduplicating it
    /// [IMP-SAXO-024].
    #[test]
    fn two_rows_with_one_identity_reject_the_file() {
        let rows = [
            transacties_row(&[("Bk Record Id", "3000000002")]),
            transacties_row(&[("Transactie-ID", "3000000001")]),
            transacties_row(&[("Transactie-ID", "3000000001")]),
        ];

        assert_eq!(
            check_identities(&rows),
            Err(SaxoError::DuplicateIdentity {
                identity: "3000000001".to_owned(),
                first: 3,
                second: 4,
            })
        );
    }

    /// A row of one corporate action, identified by nothing but its group id.
    fn corporate_action(group: &str, acties: &str, amount: &str) -> SourceRow {
        transacties_row(&[
            ("Corporate action-Id", group),
            ("Acties", acties),
            ("Boekingsbedrag", amount),
        ])
    }

    /// The TransAlta shape at its worst: three rows under one `Corporate action-Id`, with the
    /// label and the amount repeated so only the ordinal tells two of them apart. All three stay
    /// distinct and the file is accepted [IMP-SAXO-008], [IMP-SAXO-024].
    #[test]
    fn rows_of_one_corporate_action_identify_distinctly() {
        let rows = [
            corporate_action("900004", "Fusie", "9.54"),
            corporate_action("900004", "Fusie", "9.54"),
            corporate_action("900004", "Fusie", "0"),
        ];

        let found: BTreeSet<String> = identities(&rows)
            .into_iter()
            .map(|identity| identity.expect("a corporate-action row is identified"))
            .collect();

        assert_eq!(found.len(), 3);
        assert_eq!(check_identities(&rows), Ok(()));
    }

    /// The composite is the group id, `Acties`, `Boekingsbedrag` and the ordinal, and the
    /// ordinal counts from the file's *last* row, the oldest, not its first [IMP-SAXO-008],
    /// [IMP-SAXO-025], DEC-082.
    #[test]
    fn the_ordinal_counts_oldest_first() {
        let rows = [
            corporate_action("900004", "Fusie", "9.54"),
            corporate_action("900004", "Fusie", "1493.11"),
            corporate_action("900004", "Fusie", "0"),
        ];

        assert_eq!(
            identities(&rows),
            [
                Ok("900004|Fusie|9.54|3".to_owned()),
                Ok("900004|Fusie|1493.11|2".to_owned()),
                Ok("900004|Fusie|0|1".to_owned()),
            ]
        );
    }

    /// A later export of the same year adds a newer row to the group, at the top of the file.
    /// It takes the next ordinal and every row already imported keeps its identity, so a
    /// re-import finds them [IMP-SAXO-008], DEC-082, [SRV-015].
    #[test]
    fn a_row_added_to_a_group_later_shifts_no_existing_identity() {
        let earlier = [
            transacties_row(&[("Transactie-ID", "3000000001")]),
            corporate_action("900004", "Fusie", "9.54"),
            corporate_action("900004", "Fusie", "9.54"),
        ];
        let later: Vec<SourceRow> = [corporate_action("900004", "Fusie", "9.54")]
            .into_iter()
            .chain(earlier.iter().cloned())
            .collect();

        let before = identities(&earlier);
        let after = identities(&later);

        assert_eq!(after[1..], before[..]);
        assert_eq!(after[0], Ok("900004|Fusie|9.54|3".to_owned()));
    }

    /// The group counts every row carrying the id, the ones a booking id identifies included,
    /// while those rows keep their booking id as identity [IMP-SAXO-007], [IMP-SAXO-008].
    #[test]
    fn a_group_member_identified_by_another_column_is_counted() {
        let rows = [
            transacties_row(&[
                ("Corporate action-Id", "900004"),
                ("Bk Record Id", "3000000847"),
                ("Acties", "Fusie"),
                ("Boekingsbedrag", "9.54"),
            ]),
            corporate_action("900004", "Fusie", "0"),
            transacties_row(&[
                ("Corporate action-Id", "900004"),
                ("Bk Record Id", "3000000836"),
                ("Acties", "Fusie"),
                ("Boekingsbedrag", "1493.11"),
            ]),
        ];

        assert_eq!(
            identities(&rows),
            [
                Ok("3000000847".to_owned()),
                Ok("900004|Fusie|0|2".to_owned()),
                Ok("3000000836".to_owned()),
            ]
        );
    }

    /// Groups count apart, and a row with no identity at all is still reported as such
    /// [IMP-SAXO-007], [IMP-SAXO-008].
    #[test]
    fn each_group_counts_its_own_rows() {
        let rows = [
            corporate_action("900005", "Dividend", "7.63"),
            corporate_action("900004", "Terugkoopaanbod", "3927.59"),
            transacties_row(&[("Acties", "Koop")]),
            corporate_action("900005", "Dividend", "0"),
        ];

        assert_eq!(
            identities(&rows),
            [
                Ok("900005|Dividend|7.63|2".to_owned()),
                Ok("900004|Terugkoopaanbod|3927.59|1".to_owned()),
                Err(SaxoError::NoIdentity),
                Ok("900005|Dividend|0|1".to_owned()),
            ]
        );
    }

    /// The columns are not part of the identity, so one value in two of them is one identity and
    /// is refused too [IMP-SAXO-007], [IMP-SAXO-024].
    #[test]
    fn one_value_in_two_different_columns_is_one_identity() {
        let rows = [
            transacties_row(&[("Transactie-ID", "3000000001")]),
            transacties_row(&[("Booking Id", "3000000001")]),
        ];

        assert!(matches!(
            check_identities(&rows),
            Err(SaxoError::DuplicateIdentity { .. })
        ));
    }

    /// The identity a record stores is the row's reference scoped to the Depot, so the same row
    /// read twice is the same record and the same row in two accounts is two [DOM-023],
    /// [DOM-024], [IMP-SAXO-007].
    #[test]
    fn the_same_row_read_twice_identifies_the_same_record() {
        let row = transacties_row(&[
            ("Rekening-ID", "69900/1000000USD"),
            ("Transactie-ID", "3000000001"),
        ]);
        let record = |row: &SourceRow| {
            let account = Account::new("saxo", account(row).expect("the row names an account"));
            identify(
                &account,
                &IdentitySource::BrokerReference(reference(row).unwrap()),
            )
        };

        assert_eq!(
            record(&row),
            record(&transacties_row(&[
                ("Rekening-ID", "69900/1000000EUR"),
                ("Transactie-ID", "3000000001"),
            ]))
        );
        assert_ne!(
            record(&row),
            record(&transacties_row(&[
                ("Rekening-ID", "69900/2000000EUR"),
                ("Transactie-ID", "3000000001"),
            ]))
        );
    }
}
