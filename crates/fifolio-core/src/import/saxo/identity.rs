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
//! its own; the composite that makes it unique [IMP-SAXO-008] is undecided and is FIF-084's.
//! Until it lands, a corporate action spanning several rows produces one identity several times
//! and the file is **refused** [IMP-SAXO-024] rather than deduplicated: deduplicating would drop
//! every row of the event but one, and money would leave the event silently. A refusal is the
//! safe failure of the two.
//!
//! # Where the refusal fires
//!
//! [`check_identities`] is separate from [`SaxoWorkbook::read`](super::SaxoWorkbook::read)
//! deliberately. Reading a workbook answers what the file holds and is what the fixture tests and
//! any later inspection rest on; refusing a file for a duplicate identity is a judgement about
//! importing it. An import calls both, so the file is still rejected as a whole, but a workbook
//! that cannot be imported today can still be read.

use std::collections::BTreeMap;

use super::{SaxoError, field};
use crate::import::reader::SourceRow;

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
    "Corporate action-Id",
];

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

/// The broker reference identifying `row`: the first of the four identity columns it populates
/// [IMP-SAXO-007].
///
/// # Errors
///
/// When the row populates none of them. That is a row that cannot be identified, which the
/// import framework counts as a failed row rather than a refusal of the file: every row of the
/// sample carries one, so such a row is a shape neither `design/` nor the sample describes.
pub fn identity(row: &SourceRow) -> Result<&str, SaxoError> {
    IDENTITY_COLUMNS
        .iter()
        .find_map(|column| field(row, column).filter(|value| !value.is_empty()))
        .ok_or(SaxoError::NoIdentity)
}

/// Refuses a file in which two rows produce one identity [IMP-SAXO-024].
///
/// Rows that carry no identity at all are not compared: they fail one at a time and do not make
/// each other a duplicate.
///
/// # Errors
///
/// When two rows of `rows` identify the same, naming both and the identity they share.
pub fn check_identities(rows: &[SourceRow]) -> Result<(), SaxoError> {
    rows.iter()
        .enumerate()
        .filter_map(|(index, row)| identity(row).ok().map(|found| (index, found)))
        .try_fold(BTreeMap::new(), |mut seen, (index, found)| {
            match seen.insert(found, index) {
                Some(first) => Err(SaxoError::DuplicateIdentity {
                    identity: found.to_owned(),
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

/// The file's own row number for a `Transacties` row index, its header row counted, so a refusal
/// names what a spreadsheet shows.
fn file_row(index: usize) -> usize {
    index + 2
}

#[cfg(test)]
mod tests {
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

            assert_eq!(identity(&row), Ok(columns[skipped].1));
        }
    }

    /// The non-breaking spellings of `Bk Record Id` and `Booking Id` are matched
    /// [IMP-SAXO-002], [IMP-SAXO-007].
    #[test]
    fn an_identity_column_spelled_with_non_breaking_spaces_is_found() {
        let row = transacties_row(&[("Bk\u{a0}Record\u{a0}Id", "3000000002")]);

        assert_eq!(identity(&row), Ok("3000000002"));
    }

    /// A row populating none of the four cannot be identified [IMP-SAXO-007].
    #[test]
    fn a_row_carrying_none_of_the_four_has_no_identity() {
        let row = transacties_row(&[("Rekening-ID", "69900/1000000EUR"), ("Acties", "Koop")]);

        assert_eq!(identity(&row), Err(SaxoError::NoIdentity));
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
    /// [IMP-SAXO-024]. This is the shape the three-row TransAlta merger has until FIF-084's
    /// composite identity (IMP-SAXO-008) lands: one `Corporate action-Id`, no other id column.
    #[test]
    fn two_rows_with_one_identity_reject_the_file() {
        let rows = [
            transacties_row(&[("Transactie-ID", "3000000001")]),
            transacties_row(&[("Corporate action-Id", "900004"), ("Acties", "Fusie")]),
            transacties_row(&[("Corporate action-Id", "900004"), ("Acties", "Fusie")]),
        ];

        assert_eq!(
            check_identities(&rows),
            Err(SaxoError::DuplicateIdentity {
                identity: "900004".to_owned(),
                first: 3,
                second: 4,
            })
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
                &IdentitySource::BrokerReference(identity(row).unwrap()),
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
