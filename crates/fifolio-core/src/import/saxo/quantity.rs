//! How many units a Saxo row moved and which way, and the free-text label they fall back to.
//!
//! # The columns are the source, the label is the fallback
//!
//! Quantity and direction are `_Transacties` columns: `Traded Quantity`, signed, and
//! `Trade Event Type`, stating `Gekocht` or `Verkocht`. Parsing them out of the `Transacties`
//! row's free-text `Acties` label is the fallback for a row with no counterpart, not the normal
//! path [IMP-SAXO-038]. [`traded`] is where that precedence lives: given a row's label and its
//! counterpart, it reads the counterpart and only reads the label when there is none.
//!
//! Where both state a quantity and the two disagree, the **file is refused**
//! [`SaxoError::QuantityDisagreement`] rather than one of them chosen: the two figures are
//! quantities of the same trade, and a tool that picked either would book a position the export
//! does not describe.
//!
//! # The label's price is a display value and nothing else
//!
//! The price inside the label is rounded to 2 decimals and must never reach money
//! [IMP-SAXO-011]: the sample sell reads `30.65` where the booked amounts give 30.654, and the
//! sample's thirteen transfers diverge from `Verhandelde waarde` by up to 1.50 on one parcel
//! [IMP-SAXO-039]. Every figure comes from a column [IMP-SAXO-012].
//!
//! That is enforced by construction rather than by care: [`LabelPrice`] wraps the decimal
//! privately and implements [`Display`](std::fmt::Display) alone. There is no accessor, so no
//! caller can obtain a [`Decimal`] from it, and no arithmetic and no
//! [`Money`](crate::decimal::Money) can be built out of it. `Deponering` used to be the one
//! sanctioned exception; since IMP-SAXO-039 put that cost basis on `Verhandelde waarde` there is
//! none.
//!
//! # What a label is
//!
//! Two shapes occur, and both are valid:
//!
//! ```text
//! Koop 40 @ 5.75 USD          an action and a trade clause
//! Verkoop -60 @ 30.65 EUR
//! Deponering 300 @ 51.40 EUR
//! Dividend                    an action alone
//! Terugkoopaanbod - Terugboeking
//! ADR-kosten XF0000000137
//! ```
//!
//! The `@` is what marks a trade clause. A label carrying one that does not fit the grammar is a
//! parse failure [`SaxoError::UnparsableLabel`] and never a guess: the quantity in it is a share
//! count, and a half-read one is a wrong position rather than a missing one. A label carrying no
//! `@` states no quantity, which is not a failure — most of the ledger is dividends, fees and
//! interest, and none of those carry one.
//!
//! What an action *means* — which transaction variant it classifies to, and that `Terugboeking`
//! is a suffix on the action it reverses [IMP-SAXO-033] — is not decided here. This module reads
//! the label; classifying it is IMP-SAXO-013's.

use std::fmt;

use rust_decimal::Decimal;

use super::money::Direction;
use super::{SaxoError, field};
use crate::decimal::{Quantity, Scaled as _};
use crate::import::reader::SourceRow;
use crate::valuation::Currency;

/// The signed quantity of a `_Transacties` leg [IMP-SAXO-038].
const TRADED_QUANTITY: &str = "Traded Quantity";

/// The column stating which way a `_Transacties` leg went [IMP-SAXO-038].
const TRADE_EVENT_TYPE: &str = "Trade Event Type";

/// The `Trade Event Type` of a leg that opened a position [IMP-SAXO-038].
const ACQUIRED: &str = "Gekocht";

/// The `Trade Event Type` of a leg that closed one [IMP-SAXO-038].
const DISPOSED: &str = "Verkocht";

/// The `Acties` action of a purchase, which is the label's own statement of direction.
const BUY: &str = "Koop";

/// The `Acties` action of a sale.
const SELL: &str = "Verkoop";

/// The token separating a trade clause's quantity from its price.
const AT: &str = "@";

/// The length of the ISO 4217 code a trade clause ends in.
const CURRENCY_CODE: usize = 3;

/// The decimals the label's price is printed to. It carries no more [IMP-SAXO-011].
const DISPLAY_DECIMALS: usize = 2;

/// A price as an `Acties` label prints it: a display value, and structurally nothing else
/// [IMP-SAXO-011], [IMP-SAXO-012].
///
/// The decimal is private and no accessor answers it, so this type cannot become an amount, a
/// cost basis or a unit price. It exists to be shown next to a row, which is the only use the
/// specification leaves it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelPrice(Decimal);

impl fmt::Display for LabelPrice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:.*}", DISPLAY_DECIMALS, self.0)
    }
}

/// The quantity clause of an `Acties` label: `40 @ 5.75 USD`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TradeClause {
    quantity: Quantity,
    price: LabelPrice,
    currency: Currency,
}

/// A parsed `Acties` label [IMP-SAXO-038].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    action: String,
    clause: Option<TradeClause>,
}

/// Which of the two stated the quantity a row traded, so that the fallback is observable rather
/// than merely intended [IMP-SAXO-038].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatedBy {
    /// The `_Transacties` columns: the normal path.
    Columns,
    /// The `Acties` label: the fallback, for a row with no counterpart.
    Label,
}

/// How many units a row moved and which way [IMP-SAXO-038].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Traded {
    quantity: Quantity,
    direction: Option<Direction>,
    stated_by: StatedBy,
}

impl Traded {
    /// The signed quantity, as the column or the label states it.
    #[must_use]
    pub fn quantity(&self) -> Quantity {
        self.quantity
    }

    /// Which way the trade went, when what stated it says so.
    ///
    /// `None` for a movement that is neither `Gekocht` nor `Verkocht`: every `Deponering` leg in
    /// the sample carries `Deponering` as its `Trade Event Type`, and IMP-SAXO-038 names only the
    /// two. A direction is **not** inferred from the sign of the quantity, for the reason
    /// [`Direction`] gives — a second statement of the same fact can only contradict the first —
    /// and the specification states none for a transfer, so none is invented here. Nothing is
    /// lost by that today: a `Deponering` books zero cash and takes its cost basis from
    /// `Verhandelde waarde` [IMP-SAXO-039], so no derivation of it consults a direction.
    #[must_use]
    pub fn direction(&self) -> Option<Direction> {
        self.direction
    }

    /// Whether the columns or the label answered, the columns being the normal path.
    #[must_use]
    pub fn stated_by(&self) -> StatedBy {
        self.stated_by
    }
}

impl Label {
    /// Parses the `Acties` value of a row.
    ///
    /// # Errors
    ///
    /// When the label carries an `@`, so states a trade clause, that does not read as
    /// `<action> <quantity> @ <price> <currency>`.
    pub fn parse(acties: &str) -> Result<Self, SaxoError> {
        let tokens: Vec<&str> = acties.split_whitespace().collect();
        let Some(at) = tokens.iter().position(|token| *token == AT) else {
            return Ok(Self {
                action: tokens.join(" "),
                clause: None,
            });
        };

        let unparsable = || SaxoError::UnparsableLabel {
            label: acties.to_owned(),
        };
        // `<action...> <quantity> @ <price> <currency>`: the action is one token or more, the
        // quantity is the token before the `@`, and the two after it end the label.
        let ([action @ .., quantity], [price, currency]) = (&tokens[..at], &tokens[at + 1..])
        else {
            return Err(unparsable());
        };
        if action.is_empty()
            || currency.len() != CURRENCY_CODE
            || !currency.chars().all(|code| code.is_ascii_uppercase())
        {
            return Err(unparsable());
        }

        Ok(Self {
            action: action.join(" "),
            clause: Some(TradeClause {
                quantity: Quantity::new(decimal(quantity).ok_or_else(unparsable)?),
                price: LabelPrice(decimal(price).ok_or_else(unparsable)?),
                currency: Currency::new(currency),
            }),
        })
    }

    /// The action the label names, its trade clause removed: `Koop`, `Dividend`,
    /// `Terugkoopaanbod - Terugboeking`. Classifying it is IMP-SAXO-013's.
    #[must_use]
    pub fn action(&self) -> &str {
        &self.action
    }

    /// The signed quantity the label states, when it states one.
    #[must_use]
    pub fn quantity(&self) -> Option<Quantity> {
        self.clause.as_ref().map(|clause| clause.quantity)
    }

    /// The price the label prints, which is a display value and can be nothing else
    /// [IMP-SAXO-011].
    #[must_use]
    pub fn price(&self) -> Option<&LabelPrice> {
        self.clause.as_ref().map(|clause| &clause.price)
    }

    /// The currency the label's price is quoted in.
    #[must_use]
    pub fn currency(&self) -> Option<&Currency> {
        self.clause.as_ref().map(|clause| &clause.currency)
    }

    /// Which way the label says the trade went, when its action says so [IMP-SAXO-038].
    ///
    /// Matched on the whole action, so a suffixed value such as `Dividend - Terugboeking` states
    /// no direction here; that suffix reverses the action it is attached to [IMP-SAXO-033] and
    /// reading it is FIF-088's, not this module's.
    #[must_use]
    pub fn direction(&self) -> Option<Direction> {
        match self.action.as_str() {
            BUY => Some(Direction::Acquisition),
            SELL => Some(Direction::Disposal),
            _ => None,
        }
    }
}

/// How many units the row moved and which way: from its `_Transacties` counterpart, falling back
/// to its `Acties` label for a row that has none [IMP-SAXO-038].
///
/// `leg` is the row's single counterpart. A corporate action joins **several**, and their sides
/// are summed after cancellation rather than indexed [IMP-SAXO-044], [IMP-SAXO-045]; that is
/// FIF-096's, and no caller may reach it by handing one of a group's legs to this function.
///
/// # Errors
///
/// When the leg's quantity is not a number or it carries no such column; when neither the leg nor
/// the label states a quantity; and when both state one and they disagree, which refuses the file
/// [`SaxoError::QuantityDisagreement`].
pub fn traded(label: &Label, leg: Option<&SourceRow>) -> Result<Traded, SaxoError> {
    let Some(leg) = leg else {
        return label
            .quantity()
            .map(|quantity| Traded {
                quantity,
                direction: label.direction(),
                stated_by: StatedBy::Label,
            })
            .ok_or_else(|| SaxoError::NoQuantity {
                label: label.action().to_owned(),
            });
    };

    let quantity = leg_quantity(leg)?;
    if let Some(stated) = label.quantity()
        && stated != quantity
    {
        return Err(SaxoError::QuantityDisagreement {
            label: stated.get().to_string(),
            column: quantity.get().to_string(),
        });
    }

    Ok(Traded {
        quantity,
        direction: leg_direction(leg),
        stated_by: StatedBy::Columns,
    })
}

/// The signed `Traded Quantity` of a leg [IMP-SAXO-038].
fn leg_quantity(leg: &SourceRow) -> Result<Quantity, SaxoError> {
    let value = field(leg, TRADED_QUANTITY).ok_or_else(|| SaxoError::MissingColumn {
        header: TRADED_QUANTITY.to_owned(),
    })?;
    decimal(value)
        .map(Quantity::new)
        .ok_or_else(|| SaxoError::NotAQuantity {
            header: TRADED_QUANTITY.to_owned(),
            value: value.to_owned(),
        })
}

/// Which way a leg went, `None` for an event type that is neither of the two IMP-SAXO-038 names.
fn leg_direction(leg: &SourceRow) -> Option<Direction> {
    match field(leg, TRADE_EVENT_TYPE)? {
        ACQUIRED => Some(Direction::Acquisition),
        DISPOSED => Some(Direction::Disposal),
        _ => None,
    }
}

/// `text` as a decimal, or `None` when it is not one.
fn decimal(text: &str) -> Option<Decimal> {
    text.parse::<Decimal>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::saxo::Sheet;
    use crate::import::saxo::money::Booked;
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;

    /// A row of `sheet` spelled as the file spells it — non-breaking spaces and all — with only
    /// the named columns populated.
    fn row(sheet: Sheet, values: &[(&str, &str)]) -> SourceRow {
        let columns = sheet
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

    /// A `_Transacties` leg stating a quantity and an event type.
    fn leg(quantity: &str, event: &str) -> SourceRow {
        row(
            Sheet::Detail,
            &[
                ("Traded Quantity", quantity),
                ("Trade Event Type", event),
                ("Acties", "Verkoop -60 @ 30.65 EUR"),
            ],
        )
    }

    /// The three label shapes `importers.md` prints, each read into its parts [IMP-SAXO-038].
    #[test]
    fn a_trade_clause_parses_into_action_quantity_price_and_currency() {
        let cases = [
            ("Koop 40 @ 5.75 USD", "Koop", dec!(40), "5.75", "USD"),
            (
                "Verkoop -60 @ 30.65 EUR",
                "Verkoop",
                dec!(-60),
                "30.65",
                "EUR",
            ),
            (
                "Deponering 300 @ 51.40 EUR",
                "Deponering",
                dec!(300),
                "51.40",
                "EUR",
            ),
        ];

        for (acties, action, quantity, price, currency) in cases {
            let label = Label::parse(acties).expect("the label is one of the three shapes");

            assert_eq!(label.action(), action, "{acties}");
            assert_eq!(label.quantity(), Some(Quantity::new(quantity)), "{acties}");
            assert_eq!(
                label.price().map(ToString::to_string),
                Some(price.to_owned()),
                "{acties}"
            );
            assert_eq!(
                label.currency().map(Currency::code),
                Some(currency),
                "{acties}"
            );
        }
    }

    /// An action carrying no trade clause states no quantity, which is not a failure: most of the
    /// ledger is labelled this way [IMP-SAXO-038].
    #[test]
    fn a_label_without_a_trade_clause_states_no_quantity() {
        for acties in [
            "Dividend",
            "Stock split",
            "Terugkoopaanbod - Terugboeking",
            "ADR-kosten XF0000000137",
        ] {
            let label = Label::parse(acties).expect("an action alone is a label");

            assert_eq!(label.action(), acties, "{acties}");
            assert_eq!(label.quantity(), None, "{acties}");
            assert_eq!(label.price(), None, "{acties}");
        }
    }

    /// A trade clause that does not read is a parse failure and never a guess [IMP-SAXO-038].
    #[test]
    fn an_unparsable_trade_clause_is_a_parse_failure() {
        for acties in [
            "Koop @ 5.75 USD",
            "Koop veertig @ 5.75 USD",
            "Koop 40 @ USD",
            "Koop 40 @ 5.75",
            "Koop 40 @ 5.75 dollar",
            "Koop 40 @ 5.75 USD extra",
            "40 @ 5.75 USD",
        ] {
            assert_eq!(
                Label::parse(acties),
                Err(SaxoError::UnparsableLabel {
                    label: acties.to_owned()
                }),
                "{acties} states a clause that cannot be read"
            );
        }
    }

    /// The columns are the normal path: a row whose label states no quantity still trades what
    /// its counterpart says it did [IMP-SAXO-038].
    #[test]
    fn the_quantity_and_direction_come_from_the_counterpart() {
        let label = Label::parse("Fusie").expect("an action alone is a label");
        let leg = leg("-300", "Verkocht");

        let traded = traded(&label, Some(&leg)).expect("the leg states the quantity");

        assert_eq!(traded.quantity(), Quantity::new(dec!(-300)));
        assert_eq!(traded.direction(), Some(Direction::Disposal));
        assert_eq!(traded.stated_by(), StatedBy::Columns);
    }

    /// `Gekocht` and `Verkocht` are the two directions the column states [IMP-SAXO-038].
    #[test]
    fn the_event_type_states_the_direction() {
        let label = Label::parse("Stock split").expect("an action alone is a label");
        let cases = [
            ("45", "Gekocht", Some(Direction::Acquisition)),
            ("-15", "Verkocht", Some(Direction::Disposal)),
            ("3000", "Deponering", None),
        ];

        for (quantity, event, direction) in cases {
            let leg = leg(quantity, event);

            let traded = traded(&label, Some(&leg)).expect("the leg states the quantity");

            assert_eq!(traded.direction(), direction, "{event}");
        }
    }

    /// The label is the fallback for a row with no counterpart, direction included
    /// [IMP-SAXO-038].
    #[test]
    fn a_row_without_a_counterpart_falls_back_to_its_label() {
        let cases = [
            ("Koop 40 @ 5.75 USD", dec!(40), Some(Direction::Acquisition)),
            (
                "Verkoop -60 @ 30.65 EUR",
                dec!(-60),
                Some(Direction::Disposal),
            ),
            ("Deponering 300 @ 51.40 EUR", dec!(300), None),
        ];

        for (acties, quantity, direction) in cases {
            let label = Label::parse(acties).expect("the label states a clause");

            let traded = traded(&label, None).expect("the label states the quantity");

            assert_eq!(traded.quantity(), Quantity::new(quantity), "{acties}");
            assert_eq!(traded.direction(), direction, "{acties}");
            assert_eq!(traded.stated_by(), StatedBy::Label, "{acties}");
        }
    }

    /// A row with neither a counterpart nor a quantity in its label states no quantity at all,
    /// which is reported rather than assumed to be zero [IMP-SAXO-038].
    #[test]
    fn a_row_stating_no_quantity_anywhere_is_reported() {
        let label = Label::parse("Dividend").expect("an action alone is a label");

        assert_eq!(
            traded(&label, None),
            Err(SaxoError::NoQuantity {
                label: "Dividend".to_owned()
            })
        );
    }

    /// A label disagreeing with the column refuses the file rather than choosing one of them
    /// [IMP-SAXO-038].
    #[test]
    fn a_label_disagreeing_with_the_column_refuses_the_file() {
        let label = Label::parse("Verkoop -60 @ 30.65 EUR").expect("the label states a clause");
        // The sign is part of the disagreement: a `Verkoop` booked as an acquisition of 60 is a
        // different position from a disposal of 60, so it is refused like any other mismatch.
        for quantity in ["-70", "60"] {
            let leg = leg(quantity, "Verkocht");

            assert_eq!(
                traded(&label, Some(&leg)),
                Err(SaxoError::QuantityDisagreement {
                    label: "-60".to_owned(),
                    column: quantity.to_owned(),
                }),
                "the label says -60 and the column {quantity}"
            );
        }
    }

    /// A label agreeing with the column is not a disagreement, however each is spelled
    /// [IMP-SAXO-038].
    #[test]
    fn a_label_agreeing_with_the_column_is_accepted() {
        let label =
            Label::parse("Deponering 3000 @ 138.00 EUR").expect("the label states a clause");
        let leg = leg("3000.00", "Deponering");

        let traded = traded(&label, Some(&leg)).expect("the two agree");

        assert_eq!(traded.quantity(), Quantity::new(dec!(3000.00)));
        assert_eq!(traded.stated_by(), StatedBy::Columns);
    }

    /// A `Traded Quantity` that is not a number is reported rather than read as zero
    /// [IMP-SAXO-038].
    #[test]
    fn a_counterpart_whose_quantity_is_not_a_number_is_reported() {
        let label = Label::parse("Fusie").expect("an action alone is a label");
        let leg = leg("onbekend", "Verkocht");

        assert_eq!(
            traded(&label, Some(&leg)),
            Err(SaxoError::NotAQuantity {
                header: "Traded Quantity".to_owned(),
                value: "onbekend".to_owned(),
            })
        );
    }

    /// A row of the wrong sheet carries no `Traded Quantity` at all, which is named rather than
    /// treated as a blank.
    #[test]
    fn a_row_carrying_no_quantity_column_is_named() {
        let label = Label::parse("Fusie").expect("an action alone is a label");
        let not_a_leg = row(Sheet::Transacties, &[("Acties", "Fusie")]);

        assert_eq!(
            traded(&label, Some(&not_a_leg)),
            Err(SaxoError::MissingColumn {
                header: "Traded Quantity".to_owned(),
            })
        );
    }

    /// The label's price is a display value and the derived price is the money one: the sample
    /// sell prints `30.65` where the booked columns give 30.654 [IMP-SAXO-011], [IMP-SAXO-012],
    /// [IMP-SAXO-032].
    ///
    /// The figures are `importers.md`'s worked example, not the fixture's, whose amounts are
    /// perturbed [TST-014].
    #[test]
    fn the_labels_price_is_a_display_value_and_not_the_derived_one() {
        let label = Label::parse("Verkoop -60 @ 30.65 EUR").expect("the label states a clause");
        let booked = Booked::read(&row(
            Sheet::Transacties,
            &[
                ("Acties", "Verkoop -60 @ 30.65 EUR"),
                ("_Valuta", "EUR"),
                ("Boekingsbedrag", "1833.24"),
                ("Aantal", "1833.24"),
                ("Totale kosten", "-6.00"),
                ("Omrekeningskoers", "1"),
            ],
        ))
        .expect("the row states its money");
        let traded = traded(&label, Some(&leg("-60", "Verkocht"))).expect("the two agree");

        let derived = booked
            .derive(
                traded.direction().expect("a `Verkocht` leg states one"),
                traded.quantity(),
                Decimal::ONE,
                NaiveDate::from_ymd_opt(2022, 5, 12).expect("a real date"),
            )
            .expect("the row's money derives");

        assert_eq!(
            label.price().map(ToString::to_string).as_deref(),
            Some("30.65")
        );
        assert_eq!(derived.unit_price().eur().get(), dec!(30.654));
    }

    /// The display value is printed to 2 decimals whatever the label spells [IMP-SAXO-011].
    #[test]
    fn the_labels_price_prints_to_two_decimals() {
        let label = Label::parse("Deponering 3000 @ 138 EUR").expect("the label states a clause");

        assert_eq!(
            label.price().map(ToString::to_string).as_deref(),
            Some("138.00")
        );
    }
}
