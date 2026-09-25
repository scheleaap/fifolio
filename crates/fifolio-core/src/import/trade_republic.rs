//! Reading a Trade Republic DE export: its header, its two date columns, its identity and its
//! signs.
//!
//! This module is the container half of the Trade Republic importer. What a money column is
//! worth is not here — that is FIF-028's — and neither is what a row *is*, which is FIF-029's.
//!
//! # One header row, 23 columns
//!
//! The export is quoted, comma-delimited, dot-decimal ASCII with a single header row of 23
//! columns, byte-identical across the 2022 to 2025 exports [IMP-TR-001]. So the header is
//! matched against that list verbatim, with none of the whitespace normalization a Saxo sheet
//! needs, and a file whose header is not it is refused rather than mapped column by column.
//!
//! # `date` and `datetime` are two different things
//!
//! `date` is the effective date and becomes the trade date. `datetime` is a **booking
//! timestamp**: it is used as an ordering column after the date and is not an execution time
//! [IMP-TR-002]. The two are independent fields, neither derivable from the other: across four
//! years they agree on every trade, diverge by a day on three dividends — two of which cross
//! midnight UTC — and by six days on the corporate action, which is booked after it takes
//! effect [IMP-TR-015].
//!
//! Rows are ordered on `date`, then `datetime`, then file position [IMP-TR-023]. The last key is
//! [`assign_orders`](crate::ordering::assign_orders)'s own and is never enough on its own here:
//! the 2022 to 2024 exports ascend by both columns, but the 2025 export ascends by `date` while
//! **descending** by `datetime` within a date, so file position alone would order it wrongly.
//! Note that the file writes `datetime` first and `date` second; the precedence above is the
//! requirement's, not the header's.
//!
//! # The timestamp is compared as one instant
//!
//! `datetime` is ISO-8601 with sub-second precision, and the file mixes widths — the 2024 export
//! writes `…38.780Z` beside `…55.441487Z`. Both are converted through one epoch-instant call, so
//! a three-digit timestamp is not mistaken for a smaller number than a six-digit one, which is
//! what [`ordering`](crate::ordering) means by every value of a column sharing a unit.
//! Nanoseconds, the finest unit RFC 3339 states, so that no difference the file writes is
//! truncated into a tie: a tie falls back to file position, which is exactly what orders the 2025
//! export backwards. A timestamp outside the range nanoseconds can hold is refused rather than
//! wrapped.

use chrono::{DateTime, NaiveDate};
use rust_decimal::Decimal;
use thiserror::Error;

use super::reader::{DelimitedReader, ReadError, SourceRow};
use crate::ordering::{FileDirection, RowOrderingKey};

/// The 23 headers, in the order the export writes them [IMP-TR-001].
pub const HEADERS: [&str; 23] = [
    "datetime",
    "date",
    "account_type",
    "category",
    "type",
    "asset_class",
    "name",
    "symbol",
    "shares",
    "price",
    "amount",
    "fee",
    "tax",
    "currency",
    "original_amount",
    "original_currency",
    "fx_rate",
    "description",
    "transaction_id",
    "counterparty_name",
    "counterparty_iban",
    "payment_reference",
    "mcc_code",
];

/// Which end of the file holds the oldest row [DOM-040], [IMP-TR-023].
pub const DIRECTION: FileDirection = FileDirection::OldestFirst;

/// The effective date, which becomes the trade date [IMP-TR-002].
const TRADE_DATE_COLUMN: &str = "date";

/// The booking timestamp, which orders rows within a date and is not an execution time
/// [IMP-TR-002].
const BOOKING_TIMESTAMP_COLUMN: &str = "datetime";

/// The stable broker reference a row is identified by [IMP-TR-003].
const IDENTITY_COLUMN: &str = "transaction_id";

/// The column carrying the quantity [IMP-TR-022]. What it is worth is FIF-028's.
pub const QUANTITY_COLUMN: &str = "shares";

/// The column carrying the unit price [IMP-TR-022]. What it is worth is FIF-028's.
pub const UNIT_PRICE_COLUMN: &str = "price";

/// Why a Trade Republic export could not be read.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TradeRepublicError {
    /// The container: not UTF-8, no header row, a repeated column, or a ragged row.
    #[error("the Trade Republic export could not be read: {0}")]
    Container(#[from] ReadError),
    /// The header row is not the format's. It is refused whole rather than matched column by
    /// column [IMP-TR-001], which would map a value under a name that means something else.
    #[error(
        "the Trade Republic export does not carry the expected 23 headers \
         (missing {missing:?}, unexpected {unexpected:?})"
    )]
    Headers {
        missing: Vec<String>,
        unexpected: Vec<String>,
    },
    /// The 23 columns are the format's, but not in the order it writes them. The header is
    /// byte-identical across the observed exports [IMP-TR-001], so a permutation is another
    /// format whose names happen to coincide, and is refused rather than read by name.
    #[error("the Trade Republic export writes its 23 headers in another order: {found:?}")]
    HeaderOrder { found: Vec<String> },
    /// A column asked for that the row does not carry, which the header check makes unreachable
    /// for a column of the format and reachable for a caller naming another format's.
    #[error("the row carries no column {header}")]
    MissingColumn { header: String },
    /// A row carrying no `transaction_id`, so carrying nothing it is identified by
    /// [IMP-TR-003]. Falling back to the parsed fields would give two genuinely identical rows
    /// one identity, and a re-import would then drop one of them.
    #[error("the row carries a blank transaction_id, so it cannot be identified")]
    NoIdentity,
    /// A `date` that is not an ISO-8601 calendar date [IMP-TR-002].
    #[error("the column date holds {value:?}, which is not an ISO-8601 date")]
    NotADate { value: String },
    /// A `datetime` that is not an ISO-8601 timestamp [IMP-TR-002].
    #[error("the column datetime holds {value:?}, which is not an ISO-8601 timestamp")]
    NotATimestamp { value: String },
    /// A `datetime` outside the range nanoseconds since the epoch can hold, roughly 1677 to
    /// 2262. No export carries such a date; it is refused rather than wrapped, because a
    /// wrapped instant would order its row silently and wrongly [IMP-TR-023].
    #[error("the column datetime holds {value:?}, which is outside the range this reader orders")]
    TimestampOutOfRange { value: String },
}

/// Reads `content` as a Trade Republic DE export, answering its rows in file order.
///
/// This, and not [`DelimitedReader`] on its own, is where the header refusal of IMP-TR-001 lives:
/// the bare reader yields rows of whatever header it finds. So the `Importer` impl FIF-029 adds
/// must reach the file through here — its `reader()` answering a wrapper that checks the header —
/// or an import would read another format's columns by name.
///
/// # Errors
///
/// When the file cannot be read as comma-delimited UTF-8, or its header row is not this
/// format's.
pub fn read(content: &[u8]) -> Result<Vec<SourceRow>, TradeRepublicError> {
    // The header is read from the header row itself and not from a data row's column names, so
    // a file of another format's header and no rows is refused rather than read as a
    // successful, empty import — a header-only file being an ordinary shape here, a broker year
    // with no transactions.
    let read = DelimitedReader::comma().read(content)?;
    check_headers(&read.headers)?;
    Ok(read.rows)
}

/// One column of `row`, by the file's own name for it.
///
/// # Errors
///
/// When the row does not carry that column. A column carried blank is `Ok("")`, which is most
/// of a Trade Republic row: a cash movement names no instrument, a trade no counterparty.
pub fn field<'a>(row: &'a SourceRow, header: &str) -> Result<&'a str, TradeRepublicError> {
    row.field(header)
        .ok_or_else(|| TradeRepublicError::MissingColumn {
            header: header.to_owned(),
        })
}

/// The broker reference identifying `row`: its `transaction_id` [IMP-TR-003].
///
/// Used as the identity directly, the UUID being stable across exports; scoping it to the
/// account is [`import`](super::import)'s.
///
/// # Errors
///
/// When the row carries no `transaction_id` column, or carries it blank.
pub fn identity(row: &SourceRow) -> Result<&str, TradeRepublicError> {
    match field(row, IDENTITY_COLUMN)? {
        "" => Err(TradeRepublicError::NoIdentity),
        reference => Ok(reference),
    }
}

/// The trade date of `row`: its effective `date`, never its booking timestamp [IMP-TR-002].
///
/// # Errors
///
/// When the row carries no `date` column, or holds something that is not an ISO-8601 date.
pub fn trade_date(row: &SourceRow) -> Result<NaiveDate, TradeRepublicError> {
    let value = field(row, TRADE_DATE_COLUMN)?;
    NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| TradeRepublicError::NotADate {
        value: value.to_owned(),
    })
}

/// The booking timestamp of `row`, as nanoseconds since the Unix epoch [IMP-TR-002].
///
/// An instant, so timestamps of different sub-second widths compare on the same unit, and an
/// offset other than `Z` would still compare against a UTC one correctly. Nanoseconds because
/// truncating a finer difference would make two rows tie and fall back to file position, which
/// the 2025 export's descending order makes wrong [IMP-TR-023]; the exports observed write three
/// or six fractional digits, so this only matters if the export ever widens.
///
/// # Errors
///
/// When the row carries no `datetime` column, holds something that is not an ISO-8601 timestamp,
/// or holds one outside the range nanoseconds can express.
pub fn booking_instant(row: &SourceRow) -> Result<i64, TradeRepublicError> {
    let value = field(row, BOOKING_TIMESTAMP_COLUMN)?;
    DateTime::parse_from_rfc3339(value)
        .map_err(|_| TradeRepublicError::NotATimestamp {
            value: value.to_owned(),
        })
        .and_then(|stamp| {
            stamp
                .timestamp_nanos_opt()
                .ok_or_else(|| TradeRepublicError::TimestampOutOfRange {
                    value: value.to_owned(),
                })
        })
}

/// What `row` is ordered by: its trade date, then its booking timestamp [IMP-TR-023].
///
/// The timestamp is stated as present rather than optional because every row of every observed
/// export carries one; a row that does not is an error, not an absent ordering column, so no
/// absent-value rule is needed here.
///
/// # Errors
///
/// When either column cannot be read, which stops the import: a row that cannot be placed would
/// change the `order` of every row after it.
pub fn ordering_key(row: &SourceRow) -> Result<RowOrderingKey, TradeRepublicError> {
    Ok(RowOrderingKey {
        trade_date: trade_date(row)?,
        columns: vec![Some(booking_instant(row)?)],
    })
}

/// Whether `figure` is money leaving the account [IMP-TR-004].
///
/// Trade Republic states its figures as cash flow, so a buy and a fee are negative and a sale, a
/// dividend and a deposit positive. A zero figure moves no cash in either direction and is
/// therefore not an outflow. Which columns carry money, and what the magnitudes come to, is
/// FIF-028's.
#[must_use]
pub fn is_outflow(figure: Decimal) -> bool {
    figure.is_sign_negative() && !figure.is_zero()
}

/// Refuses a file whose header row is not this format's [IMP-TR-001].
///
/// Matched verbatim, order included: the header is byte-identical across the 2022 to 2025
/// exports, so a file writing the 23 columns in another order is a format this reader has not
/// seen, and accepting it would rest on the hope that every one of its columns still means what
/// its name meant here. Which columns differ is reported by name; a permutation differs in
/// neither direction, so it is named as what it is.
fn check_headers(found: &[String]) -> Result<(), TradeRepublicError> {
    if found.iter().eq(HEADERS.iter()) {
        return Ok(());
    }

    let difference = |left: &[&str], right: &[&str]| -> Vec<String> {
        left.iter()
            .filter(|name| !right.contains(name))
            .map(|name| (*name).to_owned())
            .collect()
    };
    let found: Vec<&str> = found.iter().map(String::as_str).collect();
    let missing = difference(&HEADERS, &found);
    let unexpected = difference(&found, &HEADERS);
    // A repeated column is already refused by the reader, so names differing in neither
    // direction mean the same 23 columns, and hence a reordering rather than a wrong set.
    if missing.is_empty() && unexpected.is_empty() {
        return Err(TradeRepublicError::HeaderOrder {
            found: found.iter().map(|name| (*name).to_owned()).collect(),
        });
    }
    Err(TradeRepublicError::Headers {
        missing,
        unexpected,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::Account;
    use crate::identity::{IdentitySource, identify};
    use crate::ordering::assign_orders;
    use rust_decimal_macros::dec;

    /// A file of the format's header row and the given lines, quoted as the export quotes them.
    fn export(lines: &[&[&str]]) -> String {
        let quoted = |values: &[&str]| format!("\"{}\"\n", values.join("\",\""));
        let body: String = lines.iter().map(|values| quoted(values)).collect();
        format!("{}{body}", quoted(&HEADERS))
    }

    /// One row with the named columns populated and every other column blank.
    fn row(values: &[(&str, &str)]) -> SourceRow {
        let cells: Vec<&str> = HEADERS
            .iter()
            .map(|header| {
                values
                    .iter()
                    .find(|(name, _)| name == header)
                    .map_or("", |(_, value)| *value)
            })
            .collect();
        let content = export(&[&cells]);
        read(content.as_bytes())
            .expect("the export reads")
            .pop()
            .expect("the export carries its one row")
    }

    /// The quoted, comma-delimited, dot-decimal export reads, and its 23 columns arrive under
    /// the file's own names [IMP-TR-001].
    #[test]
    fn the_export_reads_as_quoted_comma_delimited_csv() {
        let content = export(&[&[
            "2024-05-02T06:01:14.891Z",
            "2024-05-02",
            "DEFAULT",
            "TRADING",
            "BUY",
            "STOCK",
            "Fixture Instrument 09",
            "XF0000000152",
            "35.0000000000",
            "75.090000",
            "-2628.150000",
            "-1.00",
            "",
            "EUR",
            "",
            "",
            "",
            "",
            "bf751ce3-33c9-539c-96d7-1428cc7bdde9",
            "",
            "",
            "",
            "",
        ]]);

        let rows = read(content.as_bytes()).expect("the export reads");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].columns().len(), 23);
        assert_eq!(field(&rows[0], QUANTITY_COLUMN), Ok("35.0000000000"));
        assert_eq!(field(&rows[0], UNIT_PRICE_COLUMN), Ok("75.090000"));
    }

    /// `shares` is the quantity and `price` the unit price, and they are two different columns
    /// [IMP-TR-022].
    #[test]
    fn the_quantity_and_the_unit_price_are_their_own_columns() {
        let read = row(&[
            (QUANTITY_COLUMN, "35.0000000000"),
            (UNIT_PRICE_COLUMN, "75.090000"),
        ]);

        // Named, not read back through the same two constants: reading them back would hold
        // just as well with the two swapped.
        assert_eq!(QUANTITY_COLUMN, "shares");
        assert_eq!(UNIT_PRICE_COLUMN, "price");
        assert_eq!(field(&read, "shares"), Ok("35.0000000000"));
        assert_eq!(field(&read, "price"), Ok("75.090000"));
    }

    /// A header row that is not the format's refuses the file, naming both differences
    /// [IMP-TR-001].
    #[test]
    fn a_header_row_that_is_not_the_formats_refuses_the_file() {
        let content = "\"datum\",\"date\"\n\"2024-05-02\",\"2024-05-02\"\n";

        let refusal = read(content.as_bytes()).expect_err("the header is not the format's");

        let TradeRepublicError::Headers {
            missing,
            unexpected,
        } = refusal
        else {
            panic!("a header refusal, not {refusal}");
        };
        assert_eq!(missing.len(), 22, "every column but date is missing");
        assert_eq!(unexpected, ["datum"]);
    }

    /// A file of 23 columns under other names is refused too: the header is matched, never
    /// mapped by position [IMP-TR-001].
    #[test]
    fn twenty_three_columns_under_other_names_are_refused() {
        let renamed: Vec<&str> = HEADERS
            .iter()
            .map(|header| {
                if *header == "shares" {
                    "quantity"
                } else {
                    *header
                }
            })
            .collect();
        let content = format!(
            "\"{}\"\n\"{}\"\n",
            renamed.join("\",\""),
            vec![""; HEADERS.len()].join("\",\"")
        );

        assert_eq!(
            read(content.as_bytes()),
            Err(TradeRepublicError::Headers {
                missing: vec!["shares".to_owned()],
                unexpected: vec!["quantity".to_owned()],
            })
        );
    }

    /// A file carrying another format's header and no data row is refused, not read as a
    /// successful, empty import: the header is checked whether or not the file carries rows
    /// [IMP-TR-001].
    #[test]
    fn a_header_only_file_of_another_format_is_refused() {
        let content = "\"a\",\"b\"\n";

        assert_eq!(
            read(content.as_bytes()),
            Err(TradeRepublicError::Headers {
                missing: HEADERS.iter().map(|name| (*name).to_owned()).collect(),
                unexpected: vec!["a".to_owned(), "b".to_owned()],
            })
        );
    }

    /// This format's header and no data row is an ordinary file — a broker year with no
    /// transactions — and reads as no rows [IMP-TR-001].
    #[test]
    fn a_header_only_file_of_this_format_reads_as_no_rows() {
        let content = export(&[]);

        assert_eq!(read(content.as_bytes()), Ok(vec![]));
    }

    /// A file carrying all 23 columns plus a 24th is refused: the extra column is a format this
    /// reader has not seen, not a column to ignore [IMP-TR-001].
    #[test]
    fn the_twenty_three_headers_plus_another_column_are_refused() {
        let mut headers: Vec<&str> = HEADERS.to_vec();
        headers.push("isin");
        let content = format!("\"{}\"\n", headers.join("\",\""));

        assert_eq!(
            read(content.as_bytes()),
            Err(TradeRepublicError::Headers {
                missing: vec![],
                unexpected: vec!["isin".to_owned()],
            })
        );
    }

    /// The 23 columns in another order are refused too: the header is matched verbatim, the
    /// exports writing it byte-identically [IMP-TR-001].
    #[test]
    fn the_twenty_three_headers_in_another_order_are_refused() {
        let mut headers: Vec<&str> = HEADERS.to_vec();
        headers.swap(0, 1);
        let content = format!("\"{}\"\n", headers.join("\",\""));

        let refusal = read(content.as_bytes()).expect_err("the header is reordered");

        let TradeRepublicError::HeaderOrder { found } = refusal else {
            panic!("an order refusal, not {refusal}");
        };
        assert_eq!(found[..2], ["date".to_owned(), "datetime".to_owned()]);
    }

    /// `date` is the trade date, and the booking timestamp is never read for it [IMP-TR-002].
    #[test]
    fn the_trade_date_is_the_effective_date_and_not_the_booking_timestamp() {
        let read = row(&[
            ("date", "2024-01-18"),
            ("datetime", "2024-01-24T23:05:38.780Z"),
        ]);

        assert_eq!(
            trade_date(&read),
            Ok(NaiveDate::from_ymd_opt(2024, 1, 18).expect("a real date"))
        );
    }

    /// The two fields are independent: neither is derivable from the other, so the row the
    /// importer reads both from can disagree in either direction [IMP-TR-015]. How far the
    /// committed exports actually diverge is asserted over the files themselves, in
    /// `tests/trade_republic_export.rs`.
    #[test]
    fn the_effective_date_and_the_booking_timestamp_diverge() {
        // The corporate action, booked after it takes effect; and a dividend whose booking
        // crosses midnight UTC, which is the other direction.
        let booked_late = row(&[
            ("date", "2024-01-18"),
            ("datetime", "2024-01-24T23:05:38.780Z"),
        ]);
        let booked_early = row(&[
            ("date", "2022-10-01"),
            ("datetime", "2022-09-30T23:47:28.997910Z"),
        ]);
        let booking_date = |read: &SourceRow| {
            DateTime::from_timestamp_nanos(booking_instant(read).expect("a timestamp")).date_naive()
        };

        assert!(booking_date(&booked_late) > trade_date(&booked_late).expect("a date"));
        assert!(booking_date(&booked_early) < trade_date(&booked_early).expect("a date"));
    }

    /// A `date` that is not an ISO-8601 date stops the row rather than being guessed at
    /// [IMP-TR-002].
    #[test]
    fn a_date_that_is_not_iso_8601_is_refused() {
        for value in ["02/05/2024", "2024-05-02T06:01:14.891Z", "", "2024-13-02"] {
            let read = row(&[("date", value)]);

            assert_eq!(
                trade_date(&read),
                Err(TradeRepublicError::NotADate {
                    value: value.to_owned()
                }),
                "{value:?} is not a date"
            );
        }
    }

    /// Timestamps of different sub-second widths convert to one unit, so a three-digit stamp is
    /// not read as a smaller number than a six-digit one [IMP-TR-023].
    #[test]
    fn timestamps_of_different_widths_compare_on_one_unit() {
        let narrow = row(&[("datetime", "2024-08-06T09:01:52.959Z")]);
        let wide = row(&[("datetime", "2024-08-06T10:22:32.557625Z")]);

        let (narrow, wide) = (
            booking_instant(&narrow).expect("a timestamp"),
            booking_instant(&wide).expect("a timestamp"),
        );

        assert!(narrow < wide, "{narrow} is the earlier instant");
        // The hazard itself: same date, same second, different widths. Compared as the digits
        // they are written with, 557625 would sort before 959; as instants, `.959` is the later
        // of the two.
        let (three_digits, six_digits) = (
            booking_instant(&row(&[("datetime", "2024-08-06T09:01:52.959Z")])).expect("a stamp"),
            booking_instant(&row(&[("datetime", "2024-08-06T09:01:52.557625Z")])).expect("a stamp"),
        );
        assert!(
            six_digits < three_digits,
            "{six_digits} is the earlier instant"
        );
        // The same instant written with an offset rather than as UTC is the same number.
        let offset = row(&[("datetime", "2024-08-06T11:22:32.557625+01:00")]);
        assert_eq!(booking_instant(&offset), Ok(wide));
    }

    /// A sub-microsecond difference is a difference, not a tie that falls back to file position
    /// [IMP-TR-023]. No observed export writes more than six fractional digits; this pins that a
    /// widened one would still order by its timestamps.
    #[test]
    fn a_sub_microsecond_difference_is_not_a_tie() {
        let keys: Vec<RowOrderingKey> = [
            ("2025-04-01", "2025-04-01T10:56:47.137863999Z"),
            ("2025-04-01", "2025-04-01T10:56:47.137863001Z"),
        ]
        .map(|(date, datetime)| row(&[("date", date), ("datetime", datetime)]))
        .iter()
        .map(|read| ordering_key(read).expect("an ordering key"))
        .collect();

        assert_ne!(keys[0].columns, keys[1].columns);
        let orders = assign_orders(&keys, DIRECTION);
        assert!(orders[1] < orders[0]);
    }

    /// A timestamp outside the range nanoseconds can hold is refused rather than wrapped into a
    /// wrong instant [IMP-TR-023].
    #[test]
    fn a_timestamp_out_of_the_orderable_range_is_refused() {
        let read = row(&[("datetime", "2300-01-01T00:00:00.000Z")]);

        assert_eq!(
            booking_instant(&read),
            Err(TradeRepublicError::TimestampOutOfRange {
                value: "2300-01-01T00:00:00.000Z".to_owned()
            })
        );
    }

    /// A `datetime` that is not an ISO-8601 timestamp stops the row [IMP-TR-002].
    #[test]
    fn a_datetime_that_is_not_iso_8601_is_refused() {
        for value in ["2024-08-06", "", "yesterday"] {
            let read = row(&[("datetime", value)]);

            assert_eq!(
                booking_instant(&read),
                Err(TradeRepublicError::NotATimestamp {
                    value: value.to_owned()
                }),
                "{value:?} is not a timestamp"
            );
        }
    }

    /// Rows sort on `date`, then `datetime`, then file position [IMP-TR-023]. This is the 2025
    /// shape: one date whose rows descend by timestamp, which file position alone orders
    /// backwards.
    #[test]
    fn rows_descending_by_timestamp_within_a_date_are_ordered_by_the_timestamp() {
        let rows = [
            ("2025-04-01", "2025-04-01T10:56:47.137863Z"),
            ("2025-04-01", "2025-04-01T09:11:49.917013Z"),
            ("2025-04-03", "2025-04-03T08:59:14.848Z"),
        ]
        .map(|(date, datetime)| row(&[("date", date), ("datetime", datetime)]));
        let keys: Vec<RowOrderingKey> = rows
            .iter()
            .map(|read| ordering_key(read).expect("an ordering key"))
            .collect();

        let orders = assign_orders(&keys, DIRECTION);

        // The second row is booked first, so it takes the first order though it is written
        // second.
        assert!(orders[1] < orders[0]);
        assert!(orders[0] < orders[2]);
    }

    /// The date dominates the timestamp, so the row booked six days late still sorts on the day
    /// it took effect [IMP-TR-002], [IMP-TR-015], [IMP-TR-023].
    #[test]
    fn the_effective_date_dominates_the_booking_timestamp() {
        let keys: Vec<RowOrderingKey> = [
            ("2024-01-18", "2024-01-24T23:05:38.780Z"),
            ("2024-01-20", "2024-01-20T10:00:00.000Z"),
        ]
        .map(|(date, datetime)| row(&[("date", date), ("datetime", datetime)]))
        .iter()
        .map(|read| ordering_key(read).expect("an ordering key"))
        .collect();

        let orders = assign_orders(&keys, DIRECTION);

        assert!(orders[0] < orders[1]);
    }

    /// The file runs oldest first, so file position is used as it stands [DOM-040].
    #[test]
    fn the_file_runs_oldest_first() {
        assert_eq!(DIRECTION, FileDirection::OldestFirst);
    }

    /// `transaction_id` is the identity, used directly [IMP-TR-003].
    #[test]
    fn the_transaction_id_is_the_identity() {
        let read = row(&[("transaction_id", "bf751ce3-33c9-539c-96d7-1428cc7bdde9")]);

        assert_eq!(identity(&read), Ok("bf751ce3-33c9-539c-96d7-1428cc7bdde9"));
    }

    /// A row carrying no `transaction_id` cannot be identified, and falls back to nothing
    /// [IMP-TR-003].
    #[test]
    fn a_row_without_a_transaction_id_cannot_be_identified() {
        let read = row(&[("date", "2024-05-02"), ("type", "BUY")]);

        assert_eq!(identity(&read), Err(TradeRepublicError::NoIdentity));
    }

    /// The identity a record stores is the row's `transaction_id` scoped to the account, so the
    /// same row read twice is the same record and the same row in two accounts is two
    /// [DOM-022], [DOM-023], [DOM-024], [IMP-TR-003].
    #[test]
    fn the_same_row_read_twice_identifies_the_same_record() {
        let read = row(&[("transaction_id", "bf751ce3-33c9-539c-96d7-1428cc7bdde9")]);
        let record = |account: &Account, read: &SourceRow| {
            identify(
                account,
                &IdentitySource::BrokerReference(identity(read).expect("an identity")),
            )
        };
        let mine = Account::new("trade-republic", "mine");
        let yours = Account::new("trade-republic", "yours");

        assert_eq!(
            record(&mine, &read),
            record(
                &mine,
                &row(&[("transaction_id", "bf751ce3-33c9-539c-96d7-1428cc7bdde9")])
            )
        );
        assert_ne!(record(&mine, &read), record(&yours, &read));
    }

    /// The signs are cash flow: a buy and a fee are negative, a sale and a dividend positive
    /// [IMP-TR-004].
    #[test]
    fn the_signs_are_cash_flow() {
        assert!(is_outflow(dec!(-2628.150000)), "a buy is an outflow");
        assert!(is_outflow(dec!(-1.00)), "a fee is an outflow");
        assert!(!is_outflow(dec!(1833.240000)), "a sale is an inflow");
        assert!(!is_outflow(dec!(0.032946)), "a dividend is an inflow");
    }

    /// A figure of zero moves no cash, in either direction, however it is signed
    /// [IMP-TR-004].
    #[test]
    fn a_zero_figure_is_not_an_outflow() {
        assert!(!is_outflow(Decimal::ZERO));
        assert!(!is_outflow(dec!(-0.00)));
    }

    /// A column of another format is reported as missing rather than read as blank.
    #[test]
    fn a_column_the_format_does_not_carry_is_reported() {
        let read = row(&[("date", "2024-05-02")]);

        assert_eq!(
            field(&read, "Transactiedatum"),
            Err(TradeRepublicError::MissingColumn {
                header: "Transactiedatum".to_owned()
            })
        );
    }

    /// A file that is not comma-delimited UTF-8 is refused by the container, not by the header
    /// check [IMP-TR-001].
    #[test]
    fn a_file_that_is_not_utf_8_is_refused() {
        assert!(matches!(
            read(&[0xff, 0xfe]),
            Err(TradeRepublicError::Container(ReadError::NotUtf8 { .. }))
        ));
    }
}
