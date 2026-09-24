//! The substitution table: what every identifying value in a real export becomes [TST-012].
//!
//! # How a pseudonym is chosen
//!
//! Every original is collected first, then the whole table is built at once: within a kind the
//! originals are sorted and each gets a replacement derived from its **rank**, never from its
//! content. A hash of the original would be equally deterministic but would carry the real value
//! into the fixture in a recoverable form for a value space small enough to enumerate — an
//! eight-digit client id is 10^8 guesses.
//!
//! Rank ordering is what keeps the broker's counters usable: `Transactie-ID`, `Bk Record Id` and
//! `Booking Id` ascend with date, which is the whole reason Saxo's ordering key names them
//! (`IMP-SAXO-026`). A rank-ordered replacement ascends with them, so the fixture orders the same
//! way the real file does. It is also why the table is built in one pass over all files rather
//! than lazily as rows are met.
//!
//! Sharing is preserved by construction: one original always maps to one pseudonym, so the rows
//! of a corporate action keep sharing a `Corporate action-Id` and one ISIN stays one security.
//!
//! # What is not replaced here
//!
//! Dates, quantities, currencies, labels and classification columns are structure, not identity,
//! and the importers are tested against them [TST-028]. Amounts are perturbed instead — see
//! [`crate::perturb`].

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use regex::Regex;
use uuid::Uuid;

/// What an identifying value is, which decides the shape of its replacement.
///
/// The id columns are separate kinds rather than one, because each has its own digit width and
/// the fixture keeps it: a format's own sanity checks read widths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    ClientId,
    /// A Saxo `Rekening-ID` with its per-currency suffix already stripped [IMP-SAXO-005].
    AccountBase,
    TransactieId,
    BkRecordId,
    /// A Saxo `_Transacties` `Order-ID`, the broker's own order number. Not a column any
    /// requirement reads, but a real id of a real order, so it is replaced like the rest.
    OrderId,
    BookingId,
    CorporateActionId,
    PositieId,
    Isin,
    /// An instrument name with any `*Delisted` annotation already stripped [IMP-SAXO-022].
    InstrumentName,
    Symbol,
    Person,
    Iban,
    /// A UUID: Trade Republic's `transaction_id`, and the ids its descriptions embed.
    Uuid,
}

/// The numeric id kinds, with the first value of the fixture's range and the step between
/// consecutive ranks. The ranges are wide enough for many times the sample's row count and keep
/// the digit width of the column they stand in.
const NUMERIC_LAYOUT: [(Kind, u64, u64); 6] = [
    (Kind::TransactieId, 7_000_000_000, 7),
    (Kind::OrderId, 6_000_000_000, 19),
    (Kind::BkRecordId, 3_000_000_000, 11),
    (Kind::BookingId, 40_000_000_000, 13),
    (Kind::CorporateActionId, 5_000_000, 3),
    (Kind::PositieId, 2_500_000_000, 17),
];

/// Every identifying value met while reading the real exports.
#[derive(Debug, Default)]
pub struct Originals {
    values: BTreeSet<(Kind, String)>,
}

impl Originals {
    /// Records `value` as identifying. An empty value is not an identity and is ignored, so
    /// callers can hand over a column that is blank on most rows.
    pub fn add(&mut self, kind: Kind, value: &str) {
        if !value.is_empty() {
            self.values.insert((kind, value.to_owned()));
        }
    }
}

/// The finished table, and the only thing the writers consult.
#[derive(Debug)]
pub struct Pseudonyms {
    by_original: BTreeMap<(Kind, String), String>,
    /// Every (original, pseudonym) pair, longest original first, for free-text substitution.
    substitutions: Vec<(String, String)>,
}

impl Pseudonyms {
    /// Builds the table from everything collected.
    ///
    /// # Errors
    ///
    /// When a value collected for a numeric id kind is not a number, which would mean the export
    /// shape changed and the replacement can no longer preserve its order.
    pub fn build(originals: &Originals) -> Result<Self> {
        let mut by_original = BTreeMap::new();
        for kind in KINDS {
            let values: Vec<&String> = originals
                .values
                .iter()
                .filter(|(value_kind, _)| *value_kind == kind)
                .map(|(_, value)| value)
                .collect();
            let ordered = order_for(kind, values)?;
            for (rank, original) in ordered.into_iter().enumerate() {
                by_original.insert((kind, original.clone()), pseudonym(kind, rank));
            }
        }

        let mut substitutions: Vec<(String, String)> = by_original
            .iter()
            .map(|((_, original), pseudonym)| (original.clone(), pseudonym.clone()))
            .collect();
        // Longest first, so that a value containing another does not leave the shorter one's
        // replacement stranded inside it.
        substitutions
            .sort_by(|left, right| right.0.len().cmp(&left.0.len()).then(left.0.cmp(&right.0)));

        Ok(Self {
            by_original,
            substitutions,
        })
    }

    /// The replacement for `original`, or `original` itself when it is empty.
    ///
    /// # Errors
    ///
    /// When `original` was never collected, which means the collecting pass and the writing pass
    /// disagree about which columns identify someone — a leak, so it stops the run.
    pub fn of(&self, kind: Kind, original: &str) -> Result<String> {
        if original.is_empty() {
            return Ok(String::new());
        }
        self.by_original
            .get(&(kind, original.to_owned()))
            .cloned()
            .with_context(|| format!("{kind:?} was not collected before it was replaced"))
    }

    /// An instrument name, keeping any `*Delisted 20231011 (...)` annotation [IMP-SAXO-022].
    ///
    /// The annotation is structure — the importer must resolve an annotated name onto the same
    /// security — so only the name inside it is replaced. The core name is what is mapped, so an
    /// instrument met both plain and annotated keeps one identity across the two.
    ///
    /// # Errors
    ///
    /// As [`Pseudonyms::of`].
    pub fn instrument_name(&self, original: &str) -> Result<String> {
        match delisting_annotation().captures(original) {
            Some(captured) => Ok(format!(
                "*Delisted {} ({})",
                &captured["date"],
                self.of(Kind::InstrumentName, &captured["name"])?
            )),
            None => self.of(Kind::InstrumentName, original),
        }
    }

    /// The core name inside a `*Delisted` annotation, or the name itself. What is collected must
    /// be what is looked up, so this is the one place that strips.
    #[must_use]
    pub fn core_instrument_name(original: &str) -> String {
        delisting_annotation().captures(original).map_or_else(
            || original.to_owned(),
            |captured| captured["name"].to_owned(),
        )
    }

    /// `text` with every collected original replaced by its pseudonym.
    ///
    /// For free-text columns, where identities appear inside a sentence. Replacement is
    /// sequential and longest-first; a pseudonym is never itself an original, so no pass can undo
    /// an earlier one.
    #[must_use]
    pub fn substitute(&self, text: &str) -> String {
        self.substitutions
            .iter()
            .fold(text.to_owned(), |carried, (original, pseudonym)| {
                carried.replace(original, pseudonym)
            })
    }

    /// The first collected original that `text` still contains, if any.
    ///
    /// The last line of defence: every value written to a fixture goes through this, so a column
    /// that was collected but not replaced fails the run instead of shipping.
    #[must_use]
    pub fn leak_in(&self, text: &str) -> Option<&str> {
        self.substitutions
            .iter()
            .map(|(original, _)| original.as_str())
            .find(|original| text.contains(original))
    }
}

const KINDS: [Kind; 14] = [
    Kind::ClientId,
    Kind::AccountBase,
    Kind::TransactieId,
    Kind::OrderId,
    Kind::BkRecordId,
    Kind::BookingId,
    Kind::CorporateActionId,
    Kind::PositieId,
    Kind::Isin,
    Kind::InstrumentName,
    Kind::Symbol,
    Kind::Person,
    Kind::Iban,
    Kind::Uuid,
];

/// The originals of one kind in the order their ranks are assigned in.
///
/// Numeric ids sort numerically so that the replacement ascends exactly where the original does;
/// everything else sorts as text, which `BTreeSet` already did.
fn order_for(kind: Kind, values: Vec<&String>) -> Result<Vec<&String>> {
    if !is_numeric(kind) {
        return Ok(values);
    }
    let mut keyed: Vec<(u64, &String)> = values
        .into_iter()
        .map(|value| {
            value
                .parse::<u64>()
                .map(|number| (number, value))
                .with_context(|| format!("{kind:?} value {value:?} is not a number"))
        })
        .collect::<Result<_>>()?;
    keyed.sort_unstable();
    Ok(keyed.into_iter().map(|(_, value)| value).collect())
}

fn is_numeric(kind: Kind) -> bool {
    NUMERIC_LAYOUT.iter().any(|(layout, _, _)| *layout == kind)
}

/// The replacement for the `rank`-th original of `kind`.
fn pseudonym(kind: Kind, rank: usize) -> String {
    let rank_as_u64 = u64::try_from(rank).expect("a rank fits a u64");
    if let Some((_, base, stride)) = NUMERIC_LAYOUT.iter().find(|(layout, _, _)| *layout == kind) {
        return (base + rank_as_u64 * stride).to_string();
    }
    match kind {
        Kind::ClientId => (10_000_000 + rank_as_u64).to_string(),
        // The five-then-seven digit shape of a Saxo Depot id, suffix excluded.
        Kind::AccountBase => format!("{:05}/{:07}", 40_100 + rank_as_u64, 9_000_001 + rank_as_u64),
        Kind::Isin => fixture_isin(rank_as_u64),
        Kind::InstrumentName => format!("Fixture Instrument {rank_as_u64:02}"),
        Kind::Symbol => format!("FXT{rank_as_u64:02}"),
        Kind::Person => format!("Fixture Owner {rank_as_u64:02}"),
        Kind::Iban => fixture_iban(rank_as_u64),
        Kind::Uuid => fixture_uuid(rank_as_u64),
        _ => unreachable!("every numeric kind is in NUMERIC_LAYOUT"),
    }
}

/// An ISIN in the reserved `XF` prefix, so no fixture instrument can collide with a real one,
/// with a correct check digit in case anything ever validates it.
fn fixture_isin(rank: u64) -> String {
    let body = format!("XF{rank:09}");
    format!("{body}{}", isin_check_digit(&body))
}

/// The ISO 6166 check digit: expand letters to two digits, then Luhn from the right.
fn isin_check_digit(body: &str) -> u32 {
    let digits: Vec<u32> = body
        .chars()
        .flat_map(|character| {
            let value = character
                .to_digit(36)
                .expect("an ISIN body is letters and digits");
            // A letter expands to its two decimal digits; a digit is itself.
            if value >= 10 {
                vec![value / 10, value % 10]
            } else {
                vec![value]
            }
        })
        .collect();
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(position, digit)| {
            // Every second digit counted from the right, starting at the rightmost, doubles.
            let weighted = if position % 2 == 0 { digit * 2 } else { *digit };
            weighted / 10 + weighted % 10
        })
        .sum();
    (10 - sum % 10) % 10
}

/// A German IBAN of the right length, with correct ISO 13616 check digits.
fn fixture_iban(rank: u64) -> String {
    let body = format!("5001051700{rank:08}");
    format!("DE{:02}{body}", iban_check_digits("DE", &body))
}

/// ISO 13616: move the country code and `00` to the end, read letters as numbers, take 98 minus
/// the remainder modulo 97.
fn iban_check_digits(country: &str, body: &str) -> u32 {
    let rearranged = format!("{body}{country}00");
    let remainder = rearranged.chars().fold(0_u32, |carried, character| {
        let value = character
            .to_digit(36)
            .expect("an IBAN is letters and digits");
        if value >= 10 {
            (carried * 100 + value) % 97
        } else {
            (carried * 10 + value) % 97
        }
    });
    98 - remainder
}

/// A UUID derived from the rank alone, so that the fixture's ids are as well distributed as the
/// real ones without deriving from them.
fn fixture_uuid(rank: u64) -> String {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("fifolio-fixture-{rank}").as_bytes(),
    )
    .to_string()
}

/// `*Delisted 20231011 (TransAlta Renewables Inc.)`, and the spaceless variant Saxo also emits.
fn delisting_annotation() -> &'static Regex {
    static PATTERN: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"^\*Delisted (?<date>\d{8})\s*\((?<name>.*)\)$")
            .expect("the delisting annotation pattern compiles")
    })
}

/// A free-text column — Saxo's `Opmerking`, Trade Republic's `description` — in fixture form.
///
/// Substitution alone is not enough where the row names a security. A description such as
/// `Buy trade IE000Y77LGG9 Amundi MSCI World SRI Climate Paris Aligned UCITS ETF Acc` carries a
/// marketing name that appears in no column, so there is nothing to substitute it with and it
/// would ship verbatim. Those descriptions are therefore **rebuilt** from the row's own label and
/// its replaced ISIN rather than filtered. No requirement reads either column, so nothing is lost
/// but realism.
///
/// Where the row names no security the text is kept, with every collected identity — a person's
/// name, a payout collection id — substituted out of it. Rebuilding rather than stripping is what
/// TST-029 asks for: the label survives, so classification still has something to read [TST-029].
#[must_use]
pub fn free_text(
    pseudonyms: &Pseudonyms,
    text: &str,
    security: Option<&str>,
    label: &str,
) -> String {
    match (text.is_empty(), security) {
        (true, _) => String::new(),
        (false, Some(isin)) => format!("{label} {isin}"),
        (false, None) => pseudonyms.substitute(text),
    }
}

/// Collects every UUID `text` contains, which is how the ids Trade Republic embeds in a
/// description are replaced rather than passed through.
pub fn uuids_in(text: &str) -> Vec<&str> {
    static PATTERN: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let pattern = PATTERN.get_or_init(|| {
        Regex::new(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}")
            .expect("the UUID pattern compiles")
    });
    pattern
        .find_iter(text)
        .map(|found| found.as_str())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn built(pairs: &[(Kind, &str)]) -> Pseudonyms {
        let mut originals = Originals::default();
        for (kind, value) in pairs {
            originals.add(*kind, value);
        }
        Pseudonyms::build(&originals).expect("the table builds")
    }

    /// Account ids, client ids, names, IBANs and instrument detail are replaced [TST-012].
    #[test]
    fn every_kind_of_identity_is_replaced() {
        let table = built(&[
            (Kind::ClientId, "88123456"),
            (Kind::AccountBase, "69900/1000000"),
            (Kind::Person, "Sample Account Holder"),
            (Kind::Iban, "DE02120300000000202051"),
            (Kind::Isin, "US6541061031"),
            (Kind::InstrumentName, "Nike (B)"),
            (Kind::Symbol, "RNW:xtse"),
            (Kind::Uuid, "3f2b7c10-9d44-4a61-8f0e-2c6b51d9a704"),
        ]);
        for (kind, original) in [
            (Kind::ClientId, "88123456"),
            (Kind::AccountBase, "69900/1000000"),
            (Kind::Person, "Sample Account Holder"),
            (Kind::Iban, "DE02120300000000202051"),
            (Kind::Isin, "US6541061031"),
            (Kind::InstrumentName, "Nike (B)"),
            (Kind::Symbol, "RNW:xtse"),
            (Kind::Uuid, "3f2b7c10-9d44-4a61-8f0e-2c6b51d9a704"),
        ] {
            let replacement = table.of(kind, original).expect("a replacement");
            assert_ne!(replacement, original, "{kind:?} was passed through");
            assert!(!replacement.is_empty());
        }
    }

    /// A rank-ordered replacement ascends where the original does, so Saxo's monotonic booking
    /// counters still order the fixture [TST-013], and it keeps the digit width of the column it
    /// stands in [TST-012], [TST-031]. Every numeric kind, since each has its own range and step.
    #[test]
    fn numeric_ids_keep_their_order() {
        // Three samples per kind at that column's width in the real export, offered out of order.
        for originals in [
            [
                (Kind::TransactieId, "5057936890"),
                (Kind::TransactieId, "4913220117"),
                (Kind::TransactieId, "5020114455"),
            ],
            [
                (Kind::OrderId, "6127884310"),
                (Kind::OrderId, "5993410277"),
                (Kind::OrderId, "6011238844"),
            ],
            [
                (Kind::BkRecordId, "1621581503"),
                (Kind::BkRecordId, "1424145358"),
                (Kind::BkRecordId, "1452109983"),
            ],
            [
                (Kind::BookingId, "22516735552"),
                (Kind::BookingId, "14836444374"),
                (Kind::BookingId, "19004122871"),
            ],
            [
                (Kind::CorporateActionId, "8957416"),
                (Kind::CorporateActionId, "8909094"),
                (Kind::CorporateActionId, "8931200"),
            ],
            [
                (Kind::PositieId, "3241778890"),
                (Kind::PositieId, "3105449201"),
                (Kind::PositieId, "3188220145"),
            ],
        ] {
            let kind = originals[0].0;
            let table = built(&originals);
            let mut ascending: Vec<&str> = originals.iter().map(|(_, value)| *value).collect();
            ascending.sort_by_key(|value| value.parse::<u64>().expect("a numeric id"));
            let replacements: Vec<String> = ascending
                .iter()
                .map(|value| table.of(kind, value).unwrap())
                .collect();
            assert!(
                replacements
                    .windows(2)
                    .all(|pair| pair[0].parse::<u64>().unwrap() < pair[1].parse::<u64>().unwrap()),
                "{kind:?} no longer ascends: {replacements:?}"
            );
            for (original, replacement) in ascending.iter().zip(&replacements) {
                assert_eq!(
                    replacement.len(),
                    original.len(),
                    "{kind:?} left {original} as {replacement}, changing the column's digit width"
                );
            }
        }
    }

    /// One original is one pseudonym, so rows of one corporate action keep sharing an id and one
    /// ISIN stays one security [TST-013].
    #[test]
    fn distinct_originals_stay_distinct_and_equal_ones_stay_equal() {
        let table = built(&[
            (Kind::CorporateActionId, "8957416"),
            (Kind::CorporateActionId, "8909094"),
        ]);
        assert_eq!(
            table.of(Kind::CorporateActionId, "8957416").unwrap(),
            table.of(Kind::CorporateActionId, "8957416").unwrap()
        );
        assert_ne!(
            table.of(Kind::CorporateActionId, "8957416").unwrap(),
            table.of(Kind::CorporateActionId, "8909094").unwrap()
        );
    }

    /// Re-running the script against the same exports produces the same fixtures.
    #[test]
    fn the_table_is_reproducible() {
        let pairs = [(Kind::Isin, "US6541061031"), (Kind::Isin, "LU1861134382")];
        let first = built(&pairs);
        let second = built(&pairs);
        assert_eq!(
            first.of(Kind::Isin, "LU1861134382").unwrap(),
            second.of(Kind::Isin, "LU1861134382").unwrap()
        );
    }

    /// A delisting annotation is structure and survives; only the name inside it is replaced
    /// [IMP-SAXO-022], [TST-013].
    #[test]
    fn a_delisting_annotation_keeps_its_shape() {
        let annotated = "*Delisted 20231011 (TransAlta Renewables Inc.)";
        assert_eq!(
            Pseudonyms::core_instrument_name(annotated),
            "TransAlta Renewables Inc."
        );
        let table = built(&[(Kind::InstrumentName, "TransAlta Renewables Inc.")]);
        let replaced = table.instrument_name(annotated).unwrap();
        assert!(replaced.starts_with("*Delisted 20231011 ("), "{replaced}");
        assert!(!replaced.contains("TransAlta"), "{replaced}");
        // The plain name and the annotated one are one instrument, so one replacement.
        assert!(
            replaced.contains(
                &table
                    .of(Kind::InstrumentName, "TransAlta Renewables Inc.")
                    .unwrap()
            )
        );
    }

    /// Saxo also writes the annotation without a space before the bracket.
    #[test]
    fn the_spaceless_annotation_is_recognized() {
        assert_eq!(
            Pseudonyms::core_instrument_name("*Delisted 20231002(Microsoft Corp SPL-Reduce Only)"),
            "Microsoft Corp SPL-Reduce Only"
        );
    }

    /// Identities inside free text are replaced too [TST-012].
    #[test]
    fn free_text_is_substituted() {
        let table = built(&[
            (Kind::Person, "Sample Account Holder"),
            (Kind::Uuid, "7c41e8a2-63b5-4d90-bb17-0e5a9f2c3d88"),
        ]);
        let substituted = table.substitute(
            "Incoming transfer from Sample Account Holder, 7c41e8a2-63b5-4d90-bb17-0e5a9f2c3d88",
        );
        assert!(substituted.starts_with("Incoming transfer from Fixture Owner"));
        assert_eq!(table.leak_in(&substituted), None);
    }

    /// A description naming a security is rebuilt rather than stripped, because the instrument
    /// name it carries is in no column and so cannot be substituted [TST-012], [TST-029].
    #[test]
    fn a_description_naming_a_security_is_rebuilt() {
        let table = built(&[(Kind::Isin, "IE000Y77LGG9")]);
        let replacement = table.of(Kind::Isin, "IE000Y77LGG9").unwrap();
        assert_eq!(
            free_text(
                &table,
                "Buy trade IE000Y77LGG9 Amundi MSCI World SRI Climate Paris Aligned UCITS ETF Acc",
                Some(&replacement),
                "BUY"
            ),
            format!("BUY {replacement}")
        );
        assert_eq!(free_text(&table, "", Some(&replacement), "BUY"), "");
        assert_eq!(
            free_text(&table, "Interest payment Booking", None, "INTEREST_PAYMENT"),
            "Interest payment Booking"
        );
    }

    /// The leak check is what stops an unreplaced column from shipping.
    #[test]
    fn a_leak_is_reported() {
        let table = built(&[(Kind::Person, "Sample Account Holder")]);
        assert_eq!(
            table.leak_in("Naam: Sample Account Holder"),
            Some("Sample Account Holder")
        );
    }

    /// The check digits are real ones, so a fixture ISIN or IBAN passes validation if anything
    /// ever validates them.
    #[test]
    fn check_digits_are_computed_correctly() {
        // Published examples: Apple's ISIN and the ECBS IBAN example.
        assert_eq!(isin_check_digit("US037833100"), 5);
        assert_eq!(isin_check_digit("AU000000BHP"), 4);
        assert_eq!(iban_check_digits("DE", "370400440532013000"), 89);
        let isin = fixture_isin(7);
        assert_eq!(isin.len(), 12);
        assert_eq!(
            isin_check_digit(&isin[..11]),
            isin[11..].parse::<u32>().unwrap()
        );
        let iban = fixture_iban(3);
        assert_eq!(iban.len(), 22);
        assert_eq!(
            iban_check_digits("DE", &iban[4..]),
            iban[2..4].parse().unwrap()
        );
    }

    /// A fixture UUID is a UUID, which the Trade Republic identity rule depends on [IMP-TR-003].
    #[test]
    fn a_fixture_uuid_parses_as_one() {
        let generated = fixture_uuid(4);
        assert!(Uuid::parse_str(&generated).is_ok(), "{generated}");
        assert_ne!(generated, fixture_uuid(5));
    }

    /// Every UUID in a free-text field is found, including the payout collection ids Trade
    /// Republic writes into interest descriptions.
    #[test]
    fn uuids_are_found_in_free_text() {
        assert_eq!(
            uuids_in("Interest payment for payout collection 7c41e8a2-63b5-4d90-bb17-0e5a9f2c3d88"),
            vec!["7c41e8a2-63b5-4d90-bb17-0e5a9f2c3d88"]
        );
        assert!(uuids_in("Interest payment Booking").is_empty());
    }

    /// An empty column is not an identity.
    #[test]
    fn empty_values_are_neither_collected_nor_replaced() {
        let table = built(&[(Kind::Isin, "")]);
        assert_eq!(table.of(Kind::Isin, "").unwrap(), "");
        assert_eq!(table.leak_in("anything"), None);
    }

    /// A value the collecting pass missed stops the run rather than shipping.
    #[test]
    fn an_uncollected_value_is_an_error() {
        let table = built(&[(Kind::Isin, "US6541061031")]);
        assert!(table.of(Kind::Isin, "LU1861134382").is_err());
    }

    /// A numeric id column that stopped being numeric stops the run.
    #[test]
    fn a_non_numeric_id_is_an_error() {
        let mut originals = Originals::default();
        originals.add(Kind::BookingId, "22516735552x");
        assert!(Pseudonyms::build(&originals).is_err());
    }
}
