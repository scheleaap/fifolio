//! Reading a real Saxo NL export and writing the fixture derived from it.
//!
//! What the fixture keeps, because the importer is specified against it [TST-013]:
//!
//! * a real XLSX container with **all three sheets** — `Transacties` (31 columns), `_Transacties`
//!   (24) and `Bookings` (21) — each with its own header row, written byte for byte from the
//!   constants below, the non-breaking spaces in `Bk Record Id`, `Booking Id`, `Trade Event Type`,
//!   `Traded Quantity`, `Amount Type Id` and `Tax Percentage` and the leading space in
//!   ` Positie-ID` included [IMP-SAXO-001], [IMP-SAXO-002], [TST-031]
//! * the join keys between the sheets, untouched as keys: one original is one pseudonym, so a row
//!   that joined its counterpart in the export joins the same counterpart in the fixture
//!   [IMP-SAXO-037], [TST-031]
//! * dates as Excel serial numbers under the export's own `dd-mmm-yyyy` format, and the
//!   `_Transacties` execution stamp under its `dd-mmm-yyyy hh:mm:ss` [IMP-SAXO-003]
//! * the rows in file order, which Saxo emits newest first [IMP-SAXO-025]
//! * the per-currency `Rekening-ID` suffixes over one base account [IMP-SAXO-005]
//! * the free-text `Acties` labels, with their quantity, direction and 2-decimal price
//!   [IMP-SAXO-011], [IMP-SAXO-012]
//! * every id column's digit width, its blank rows and which rows share an id
//!
//! Only the identities are replaced and the amounts moved. Which rows exist, on which sheet, in
//! which order, carrying which labels and which ids, is the real file's.
//!
//! Two figures are not moved on their own magnitude but with the figure they belong to, because
//! the relation is the thing an importer reads: `Verhandelde waarde` moves with its row's `Prijs`,
//! and a booking's `Bookings` components move with the booking so that they still sum to it
//! [TST-031]. See [`crate::perturb`].

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use calamine::{Data, Reader, Xlsx, open_workbook};
use regex::Regex;
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive as _, ToPrimitive as _};
use rust_xlsxwriter::{DocProperties, ExcelDateTime, Format, Workbook};

use crate::perturb::{perturb, perturb_like, perturb_shares};
use crate::pseudonym::{Kind, Originals, Pseudonyms, free_text};

/// One of the three sheets a Saxo export carries [IMP-SAXO-001].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetKind {
    /// The cash ledger: one row per booked movement.
    Transacties,
    /// The position side: signed quantity, price, traded value, direction. Its name in the file
    /// starts with an underscore.
    Detail,
    /// The components a cash movement decomposes into, and the tax figures.
    Bookings,
}

impl SheetKind {
    /// The three sheets in the order the export carries them.
    pub const ALL: [Self; 3] = [Self::Transacties, Self::Detail, Self::Bookings];

    /// The sheet's name in the workbook, as the importer looks it up [IMP-SAXO-001].
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Transacties => "Transacties",
            Self::Detail => "_Transacties",
            Self::Bookings => "Bookings",
        }
    }

    /// The sheet's header row, byte for byte [IMP-SAXO-002].
    #[must_use]
    pub fn headers(self) -> &'static [&'static str] {
        match self {
            Self::Transacties => &TRANSACTIES_HEADERS,
            Self::Detail => &DETAIL_HEADERS,
            Self::Bookings => &BOOKINGS_HEADERS,
        }
    }

    /// The index of `header` in this sheet.
    ///
    /// # Panics
    ///
    /// When `header` is not one of the sheet's own, which only a caller naming the wrong sheet's
    /// column can produce.
    #[must_use]
    pub fn column(self, header: &str) -> usize {
        self.headers()
            .iter()
            .position(|candidate| *candidate == header)
            .unwrap_or_else(|| panic!("{header:?} is not a column of {}", self.name()))
    }
}

/// The 31 `Transacties` headers, `\u{a0}` and the leading space included [IMP-SAXO-002].
pub const TRANSACTIES_HEADERS: [&str; 31] = [
    "Klant-id",
    "Transactiedatum",
    "Valutadatum",
    "Rekening-ID",
    "Transactie-ID",
    " Positie-ID",
    "Corporate action-Id",
    "Bk\u{a0}Record\u{a0}Id",
    "Booking\u{a0}Id",
    "Transactietype",
    "Acties",
    "Aantal",
    "Valuta",
    "Boekingsbedrag",
    "_Valuta",
    "Omrekeningskoers",
    "Omwisselkosten",
    "Totale kosten",
    "Gerealiseerd rendement",
    "IBAN",
    "Naam IBAN-eigenaar",
    "Opmerking",
    "Reden van correctie",
    "Instrument",
    "Instrumentsymbool",
    "Instrument ISIN",
    "Instrumentvaluta",
    "Type",
    "Uitwisselingsbeschrijving",
    "Van derivaat",
    "Onderliggend instrumenttype",
];

/// The 24 `_Transacties` headers [IMP-SAXO-001].
pub const DETAIL_HEADERS: [&str; 24] = [
    "Rekening-ID",
    "Transactie-ID",
    "Bk\u{a0}Record\u{a0}Id",
    "Booking\u{a0}Id",
    "Corporate action-Id",
    "Acties",
    "Order-ID",
    "Aangepaste transactiedatum",
    "Uitvoeringsdatum transactie",
    "Trade\u{a0}Event\u{a0}Type",
    "Trade Type",
    "Openen/sluiten",
    "Traded\u{a0}Quantity",
    "Prijs",
    "Verhandelde waarde",
    "Spreadkosten",
    "Van derivaat",
    "Onderliggend instrumenttype",
    "Instrument",
    "Instrumentsymbool",
    "Instrument ISIN",
    "Instrumentvaluta",
    "Type",
    "Uitwisselingsbeschrijving",
];

/// The 21 `Bookings` headers [IMP-SAXO-001].
pub const BOOKINGS_HEADERS: [&str; 21] = [
    "Rekening-ID",
    "Transactie-ID",
    "Bk\u{a0}Record\u{a0}Id",
    "Booking\u{a0}Id",
    "Corporate action-Id",
    "Acties",
    "Amount Type",
    "Amount\u{a0}Type\u{a0}Id",
    "Boekingsbedrag",
    "Omwisselkosten",
    "Omrekeningskoers",
    "Boekingsdatum",
    "Ex-datum",
    "Boekdatum",
    "In aanmerking komend aantal",
    "Dividend per aandeel",
    "Tax\u{a0}Percentage",
    "Instrument",
    "Instrumentsymbool",
    "Instrument ISIN",
    "Instrumentvaluta",
];

/// The columns holding a monetary figure or a conversion rate, all of which are perturbed.
///
/// A quantity is not among them and never moves [TST-028]: `Traded Quantity` and
/// `In aanmerking komend aantal` are share counts an importer recognizes a corporate action by,
/// and `Tax Percentage` is a withholding rate, not an amount. `Aantal` is here because on
/// `Transacties` it is a cash movement and never a quantity [IMP-SAXO-009].
const AMOUNT_HEADERS: [&str; 9] = [
    "Aantal",
    "Boekingsbedrag",
    "Omrekeningskoers",
    "Omwisselkosten",
    "Totale kosten",
    "Gerealiseerd rendement",
    "Prijs",
    "Verhandelde waarde",
    "Spreadkosten",
];

/// The date columns, written under the export's own number format so that a reader sees serial
/// numbers where the real file has them [IMP-SAXO-003]. `Ex-datum` and `Boekdatum` are not here:
/// the export writes those two as text, and so does the fixture.
const DATE_HEADERS: [&str; 4] = [
    "Transactiedatum",
    "Valutadatum",
    "Aangepaste transactiedatum",
    "Boekingsdatum",
];

/// The one column holding a date **and a time of day**, under the export's own format for it.
const TIMESTAMP_HEADERS: [&str; 1] = ["Uitvoeringsdatum transactie"];

/// The `Bookings` join keys, in the priority the join takes them in [IMP-SAXO-037].
const BOOKING_JOIN_KEYS: [&str; 3] = [
    "Bk\u{a0}Record\u{a0}Id",
    "Booking\u{a0}Id",
    "Corporate action-Id",
];

/// One cell, in the only three shapes the sheets use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cell {
    Empty,
    Text(String),
    Number(Decimal),
}

impl Cell {
    /// The cell as text, an empty cell included, for matching and for the leak check.
    #[must_use]
    pub fn as_text(&self) -> String {
        match self {
            Self::Empty => String::new(),
            Self::Text(text) => text.clone(),
            Self::Number(number) => number.to_string(),
        }
    }
}

/// The data rows of one sheet; the header row is the kind's [`SheetKind::headers`].
#[derive(Debug, Clone)]
pub struct Sheet {
    pub kind: SheetKind,
    pub rows: Vec<Vec<Cell>>,
}

/// One export: its three sheets, in file order.
#[derive(Debug, Clone)]
pub struct Export {
    pub sheets: [Sheet; 3],
}

impl Export {
    /// The sheet of `kind`.
    ///
    /// # Panics
    ///
    /// Never: an export holds each kind exactly once, which [`read`] establishes.
    #[must_use]
    pub fn sheet(&self, kind: SheetKind) -> &Sheet {
        self.sheets
            .iter()
            .find(|sheet| sheet.kind == kind)
            .unwrap_or_else(|| panic!("an export carries {}", kind.name()))
    }
}

/// The value of `header` in `row`, a row of `kind`.
///
/// # Panics
///
/// As [`SheetKind::column`].
#[must_use]
pub fn field<'row>(kind: SheetKind, row: &'row [Cell], header: &str) -> &'row Cell {
    &row[kind.column(header)]
}

/// The numeric value of `header` in `row`, where it holds one.
fn number(kind: SheetKind, row: &[Cell], header: &str) -> Option<Decimal> {
    match field(kind, row, header) {
        Cell::Number(value) => Some(*value),
        _ => None,
    }
}

/// Reads the three sheets of a real export.
///
/// # Errors
///
/// When the workbook cannot be read, when a sheet is missing — the file is three sheets and a
/// fixture of one is a fixture of nothing [TST-031] — when a header row is not the one expected,
/// which a changed export shape must stop the run over rather than produce a fixture nothing is
/// specified against, or when a cell holds something other than text, a number or nothing.
pub fn read(path: &Path) -> Result<Export> {
    let mut workbook: Xlsx<_> =
        open_workbook(path).with_context(|| format!("opening {}", path.display()))?;
    let sheets = SheetKind::ALL
        .iter()
        .map(|kind| read_sheet(&mut workbook, path, *kind))
        .collect::<Result<Vec<_>>>()?;
    Ok(Export {
        sheets: sheets
            .try_into()
            .map_err(|_| anyhow!("{} does not hold the three sheets", path.display()))?,
    })
}

fn read_sheet<R: std::io::Read + std::io::Seek>(
    workbook: &mut Xlsx<R>,
    path: &Path,
    kind: SheetKind,
) -> Result<Sheet> {
    let range = workbook
        .worksheet_range(kind.name())
        .with_context(|| format!("reading sheet {} of {}", kind.name(), path.display()))?;

    let mut rows = range.rows();
    let header_row = rows
        .next()
        .ok_or_else(|| anyhow!("{} of {} has no header row", kind.name(), path.display()))?;
    let headers: Vec<String> = header_row.iter().map(read_text).collect();
    if headers != kind.headers() {
        bail!(
            "{} of {} does not carry the {} Dutch Saxo headers: {headers:?}",
            kind.name(),
            path.display(),
            kind.headers().len()
        );
    }

    let rows = rows
        .map(|row| row.iter().map(read_cell).collect::<Result<Vec<Cell>>>())
        .collect::<Result<Vec<_>>>()?;
    Ok(Sheet { kind, rows })
}

fn read_text(data: &Data) -> String {
    match data {
        Data::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn read_cell(data: &Data) -> Result<Cell> {
    match data {
        Data::Empty => Ok(Cell::Empty),
        Data::String(text) if text.is_empty() => Ok(Cell::Empty),
        Data::String(text) => Ok(Cell::Text(text.clone())),
        Data::Int(number) => Ok(Cell::Number(Decimal::from(*number))),
        Data::Float(number) => decimal_from(*number),
        Data::DateTime(serial) => decimal_from(serial.as_f64()),
        other => bail!("unexpected cell {other:?}"),
    }
}

/// A spreadsheet holds every number as a binary double, so the scale a column is written at is
/// recovered rather than read: `from_f64` rounds to the 15 significant digits a double actually
/// carries and `normalize` then drops the zeros that padding left, so `12.15` comes back at scale
/// 2 and a date serial at scale 0. Without it a perturbed amount is rounded at the scale of the
/// binary noise and the fixture shows `12.897225000000002`.
fn decimal_from(number: f64) -> Result<Cell> {
    Decimal::from_f64(number)
        .map(|value| Cell::Number(value.normalize()))
        .ok_or_else(|| anyhow!("{number} is not representable as a decimal"))
}

/// Records every identifying value the export carries, on all three sheets.
pub fn collect(export: &Export, originals: &mut Originals) {
    for sheet in &export.sheets {
        for row in &sheet.rows {
            for (header, cell) in sheet.kind.headers().iter().zip(row) {
                let value = cell.as_text();
                match *header {
                    "Rekening-ID" => originals.add(Kind::AccountBase, account_base(&value)),
                    "Instrument" => originals.add(
                        Kind::InstrumentName,
                        &Pseudonyms::core_instrument_name(&value),
                    ),
                    named => {
                        if let Some(kind) = identity_kind(named, &value) {
                            originals.add(kind, &value);
                        }
                    }
                }
            }
        }
    }
}

/// The identity a column carries, if it carries one.
///
/// `Rekening-ID` and `Instrument` are not here: each is rewritten around a part that stays — the
/// currency suffix, the delisting annotation — so each is handled in its own right.
fn identity_kind(header: &str, value: &str) -> Option<Kind> {
    Some(match header {
        "Klant-id" => Kind::ClientId,
        "Transactie-ID" => Kind::TransactieId,
        " Positie-ID" => Kind::PositieId,
        "Corporate action-Id" => Kind::CorporateActionId,
        "Bk\u{a0}Record\u{a0}Id" => Kind::BkRecordId,
        "Booking\u{a0}Id" => Kind::BookingId,
        // `0` is Saxo's "no order" on a row no order produced, a marker rather than an id.
        // Replacing it would make the fixture claim an order that never existed.
        "Order-ID" if value != "0" => Kind::OrderId,
        "IBAN" => Kind::Iban,
        "Naam IBAN-eigenaar" => Kind::Person,
        "Instrument ISIN" => Kind::Isin,
        "Instrumentsymbool" => Kind::Symbol,
        _ => return None,
    })
}

/// The `Rekening-ID` without its per-currency suffix: `69900/1000000EUR` is a sub-account of
/// `69900/1000000` [IMP-SAXO-005]. The suffix is a currency code and identifies nobody, so it
/// stays and the fixture keeps the three sub-accounts of one Depot.
fn account_base(account: &str) -> &str {
    account
        .strip_suffix(currency_suffix(account))
        .unwrap_or(account)
}

fn currency_suffix(account: &str) -> &str {
    let tail_starts_at = account.len().saturating_sub(3);
    let tail = &account[tail_starts_at..];
    if tail.len() == 3 && tail.chars().all(|character| character.is_ascii_uppercase()) {
        tail
    } else {
        ""
    }
}

/// An amount whose fixture value is decided by the figure it belongs to rather than by its own
/// magnitude: the column it stands in, and the value it takes.
type LinkedAmount = Option<(&'static str, Decimal)>;

/// One entry per data row of a sheet, in row order.
type Linked = Vec<LinkedAmount>;

/// The fixture form of the export.
///
/// # Errors
///
/// When a value was not collected before it was replaced, or when a numeric column holds
/// something a decimal cannot carry.
pub fn anonymize(export: &Export, pseudonyms: &Pseudonyms) -> Result<Export> {
    let linked = [
        vec![None; export.sheet(SheetKind::Transacties).rows.len()],
        linked_traded_values(export.sheet(SheetKind::Detail)),
        linked_components(export),
    ];
    let sheets = export
        .sheets
        .iter()
        .zip(linked)
        .map(|(sheet, linked)| anonymize_sheet(sheet, &linked, pseudonyms))
        .collect::<Result<Vec<_>>>()?;
    Ok(Export {
        sheets: sheets
            .try_into()
            .map_err(|_| anyhow!("an anonymized export carries the three sheets"))?,
    })
}

/// `Verhandelde waarde` moved by the factor its own row's `Prijs` moved by [TST-031].
///
/// The traded value is the quantity times the price, and the quantity never moves [TST-028], so
/// the value has to follow the price. It is not exactly the product — the price is rounded to two
/// decimals and a bond quotes in percent of par [IMP-SAXO-039] — and scaling by the price's factor
/// is what keeps the relation the export states, whatever that relation is.
fn linked_traded_values(detail: &Sheet) -> Linked {
    detail
        .rows
        .iter()
        .map(|row| {
            match (
                number(detail.kind, row, "Prijs"),
                number(detail.kind, row, "Verhandelde waarde"),
            ) {
                (Some(price), Some(value)) if !price.is_zero() => Some((
                    "Verhandelde waarde",
                    perturb_like(value, price, perturb(price)),
                )),
                _ => None,
            }
        })
        .collect()
}

/// The `Bookings` components of a booking, moved together with the booking, so that they still
/// sum to it [TST-031].
///
/// The booking is the `Transacties` row the components join [IMP-SAXO-037], taken in file order,
/// and the figure they sum to is that row's `Boekingsbedrag` where they summed to it in the export
/// and its `Aantal` — the same movement stated in EUR — where they summed to that instead. Where
/// they summed to neither, each component moves on its own magnitude: there is no sum to keep, and
/// inventing one would state a decomposition the export does not.
fn linked_components(export: &Export) -> Linked {
    let bookings = export.sheet(SheetKind::Bookings);
    let indexes: Vec<BTreeMap<String, Vec<usize>>> = BOOKING_JOIN_KEYS
        .iter()
        .map(|header| index_by(bookings, header))
        .collect();

    let mut linked: Linked = vec![None; bookings.rows.len()];
    for row in &export.sheet(SheetKind::Transacties).rows {
        let components = BOOKING_JOIN_KEYS
            .iter()
            .zip(&indexes)
            .find_map(|(header, index)| {
                index.get(&field(SheetKind::Transacties, row, header).as_text())
            });
        let Some(components) = components else {
            continue;
        };
        // A `Corporate action-Id` group is shared by several bookings; the first of them in file
        // order carries the components, so no component is moved twice.
        if components.iter().any(|index| linked[*index].is_some()) {
            continue;
        }
        let amounts: Vec<Decimal> = components
            .iter()
            .map(|index| {
                number(
                    SheetKind::Bookings,
                    &bookings.rows[*index],
                    "Boekingsbedrag",
                )
                .unwrap_or_default()
            })
            .collect();
        let total: Decimal = amounts.iter().sum();
        let parent = ["Boekingsbedrag", "Aantal"]
            .iter()
            .filter_map(|header| number(SheetKind::Transacties, row, header))
            .find(|parent| !parent.is_zero() && *parent == total);
        let Some(parent) = parent else {
            continue;
        };
        for (index, moved) in components.iter().zip(perturb_shares(&amounts, parent)) {
            linked[*index] = Some(("Boekingsbedrag", moved));
        }
    }
    linked
}

/// The rows of `sheet` carrying each non-empty value of `header`, by that value.
fn index_by(sheet: &Sheet, header: &str) -> BTreeMap<String, Vec<usize>> {
    sheet.rows.iter().enumerate().fold(
        BTreeMap::new(),
        |mut indexed: BTreeMap<String, Vec<usize>>, (index, row)| {
            let key = field(sheet.kind, row, header).as_text();
            if !key.is_empty() {
                indexed.entry(key).or_default().push(index);
            }
            indexed
        },
    )
}

fn anonymize_sheet(sheet: &Sheet, linked: &Linked, pseudonyms: &Pseudonyms) -> Result<Sheet> {
    let rows = sheet
        .rows
        .iter()
        .zip(linked)
        .map(|(row, linked)| anonymize_row(sheet.kind, row, linked.as_ref(), pseudonyms))
        .collect::<Result<Vec<_>>>()?;
    Ok(Sheet {
        kind: sheet.kind,
        rows,
    })
}

fn anonymize_row(
    kind: SheetKind,
    row: &[Cell],
    linked: Option<&(&'static str, Decimal)>,
    pseudonyms: &Pseudonyms,
) -> Result<Vec<Cell>> {
    let text = |header: &str| field(kind, row, header).as_text();
    let names_a_security = !text("Instrument ISIN").is_empty();
    let isin = pseudonyms.of(Kind::Isin, &text("Instrument ISIN"))?;

    kind.headers()
        .iter()
        .zip(row)
        .map(|(header, cell)| -> Result<Cell> {
            let original = cell.as_text();
            let replaced = match *header {
                "Rekening-ID" => format!(
                    "{}{}",
                    pseudonyms.of(Kind::AccountBase, account_base(&original))?,
                    currency_suffix(&original)
                ),
                "Instrument" => pseudonyms.instrument_name(&original)?,
                "Instrumentsymbool" => symbol(pseudonyms, &original)?,
                "Instrument ISIN" => isin.clone(),
                "Acties" => relabel(pseudonyms, &original),
                "Dividend per aandeel" => dividend_per_share(&original),
                "Opmerking" => free_text(
                    pseudonyms,
                    &original,
                    names_a_security.then_some(isin.as_str()),
                    &text("Acties"),
                ),
                named => match identity_kind(named, &original) {
                    Some(identity) => pseudonyms.of(identity, &original)?,
                    None => return Ok(rewritten_amount(named, cell, linked)),
                },
            };
            Ok(same_shape_as(cell, &replaced))
        })
        .collect()
}

/// A replacement keeps the cell shape it replaces: `Bk Record Id` is a number in the export and
/// the other id columns are text, and an importer's header-to-type mapping is tested against
/// that.
fn same_shape_as(cell: &Cell, replacement: &str) -> Cell {
    match cell {
        Cell::Empty => Cell::Empty,
        Cell::Text(_) => Cell::Text(replacement.to_owned()),
        Cell::Number(_) => replacement
            .parse::<Decimal>()
            .map_or_else(|_| Cell::Text(replacement.to_owned()), Cell::Number),
    }
}

fn rewritten_amount(header: &str, cell: &Cell, linked: Option<&(&'static str, Decimal)>) -> Cell {
    match (cell, linked) {
        (Cell::Number(_), Some((column, moved))) if *column == header => Cell::Number(*moved),
        (Cell::Number(amount), _) if AMOUNT_HEADERS.contains(&header) => {
            Cell::Number(perturb(*amount))
        }
        _ => cell.clone(),
    }
}

/// An instrument symbol, keeping the `:exchange` suffix that says where it trades.
fn symbol(pseudonyms: &Pseudonyms, original: &str) -> Result<String> {
    match original.split_once(':') {
        Some((_, exchange)) => Ok(format!(
            "{}:{exchange}",
            pseudonyms.of(Kind::Symbol, original)?
        )),
        None => pseudonyms.of(Kind::Symbol, original),
    }
}

/// `Koop 40 @ 5.75 USD` with the price perturbed and everything else kept.
///
/// The label is the only place a quantity and a direction appear on `Transacties`
/// [IMP-SAXO-011], so both stay; the price is an amount and moves with the rest, keeping its 2
/// decimals. The same label appears on `_Transacties` and on `Bookings`, and equal amounts move
/// equally, so the three sheets still agree on the price.
fn relabel(pseudonyms: &Pseudonyms, label: &str) -> String {
    static PATTERN: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let pattern = PATTERN.get_or_init(|| {
        Regex::new(
            r"^(?<label>.+?) (?<quantity>-?[\d.]+) @ (?<price>\d+\.\d{2}) (?<currency>[A-Z]{3})$",
        )
        .expect("the Acties label pattern compiles")
    });
    match pattern.captures(label) {
        Some(captured) => {
            let price = captured["price"]
                .parse::<Decimal>()
                .map(perturb)
                .map_or_else(|_| captured["price"].to_owned(), |moved| moved.to_string());
            format!(
                "{} {} @ {price} {}",
                &captured["label"], &captured["quantity"], &captured["currency"]
            )
        }
        None => pseudonyms.substitute(label),
    }
}

/// `Bookings` states a dividend per share as `0.83 USD`, and `-` where the row has none. The
/// figure is an amount and moves; the currency and the `-` marker are structure and stay.
fn dividend_per_share(original: &str) -> String {
    match original.split_once(' ') {
        Some((amount, currency)) => amount.parse::<Decimal>().map_or_else(
            |_| original.to_owned(),
            |value| format!("{} {currency}", perturb(value)),
        ),
        None => original.to_owned(),
    }
}

/// Writes the fixture workbook: the three sheets, each under its own name and headers.
///
/// # Errors
///
/// When the file cannot be written, or when an amount is not representable in a spreadsheet.
pub fn write(path: &Path, export: &Export) -> Result<()> {
    let mut workbook = Workbook::new();
    // The container otherwise records the moment it was written, which would make every rerun a
    // diff even where no row changed. A fixed creation time and the zip's own fixed entry times
    // make the same exports produce the same bytes.
    workbook.set_properties(
        &DocProperties::new().set_creation_datetime(&ExcelDateTime::from_ymd(2000, 1, 1)?),
    );
    let dates = Format::new().set_num_format("dd-mmm-yyyy");
    let stamps = Format::new().set_num_format("dd-mmm-yyyy hh:mm:ss");

    for sheet in &export.sheets {
        let worksheet = workbook.add_worksheet();
        worksheet.set_name(sheet.kind.name())?;

        for (index, header) in sheet.kind.headers().iter().enumerate() {
            worksheet.write_string(0, column_index(index)?, *header)?;
        }
        for (row_index, row) in sheet.rows.iter().enumerate() {
            let row_number = u32::try_from(row_index + 1)?;
            for (index, cell) in row.iter().enumerate() {
                let column_number = column_index(index)?;
                let header = sheet.kind.headers()[index];
                match cell {
                    // The real export holds a zero-length shared string where a cell is blank;
                    // `rust_xlsxwriter` writes no cell at all for one, which is the divergence
                    // `testing.md` records and the reader accepts either way [TST-030].
                    Cell::Empty => worksheet.write_string(row_number, column_number, "")?,
                    Cell::Text(text) => worksheet.write_string(row_number, column_number, text)?,
                    Cell::Number(number) => {
                        let value = number.to_f64().ok_or_else(|| {
                            anyhow!("{number} is not representable in a spreadsheet")
                        })?;
                        if DATE_HEADERS.contains(&header) {
                            worksheet.write_number_with_format(
                                row_number,
                                column_number,
                                value,
                                &dates,
                            )?
                        } else if TIMESTAMP_HEADERS.contains(&header) {
                            worksheet.write_number_with_format(
                                row_number,
                                column_number,
                                value,
                                &stamps,
                            )?
                        } else {
                            worksheet.write_number(row_number, column_number, value)?
                        }
                    }
                };
            }
        }
    }

    workbook.save(path)?;
    Ok(())
}

fn column_index(index: usize) -> Result<u16> {
    u16::try_from(index).context("a column index fits a spreadsheet column")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sheet of `kind` built from rows of `(header, value)` pairs, every other column empty.
    fn sheet(kind: SheetKind, rows: &[&[(&str, &str)]]) -> Sheet {
        let rows = rows
            .iter()
            .map(|stated| {
                kind.headers()
                    .iter()
                    .map(|header| {
                        stated.iter().find(|(named, _)| named == header).map_or(
                            Cell::Empty,
                            |(_, value)| {
                                value
                                    .parse::<Decimal>()
                                    .map_or_else(|_| Cell::Text((*value).to_owned()), Cell::Number)
                            },
                        )
                    })
                    .collect()
            })
            .collect();
        Sheet { kind, rows }
    }

    /// The header sets are the ones the importer matches against [IMP-SAXO-001],
    /// [IMP-SAXO-002], [TST-031].
    #[test]
    fn the_three_sheets_carry_their_headers_and_their_unusual_whitespace() {
        assert_eq!(
            SheetKind::ALL.map(SheetKind::name),
            ["Transacties", "_Transacties", "Bookings"]
        );
        assert_eq!(
            SheetKind::ALL.map(|kind| kind.headers().len()),
            [31, 24, 21]
        );
        assert!(TRANSACTIES_HEADERS.contains(&"Bk\u{a0}Record\u{a0}Id"));
        assert!(TRANSACTIES_HEADERS.contains(&"Booking\u{a0}Id"));
        assert!(TRANSACTIES_HEADERS.contains(&" Positie-ID"));
        assert!(DETAIL_HEADERS.contains(&"Trade\u{a0}Event\u{a0}Type"));
        assert!(DETAIL_HEADERS.contains(&"Traded\u{a0}Quantity"));
        assert!(BOOKINGS_HEADERS.contains(&"Tax\u{a0}Percentage"));
        assert!(BOOKINGS_HEADERS.contains(&"Amount\u{a0}Type\u{a0}Id"));
    }

    /// A workbook of `(sheet name, header row)` written straight rather than through [`write`],
    /// so that a sheet can be left out or its header row stated wrong.
    fn workbook_with(path: &Path, sheets: &[(&str, Vec<String>)]) -> Result<()> {
        let mut workbook = Workbook::new();
        for (name, headers) in sheets {
            let worksheet = workbook.add_worksheet();
            worksheet.set_name(*name)?;
            for (index, header) in headers.iter().enumerate() {
                worksheet.write_string(0, column_index(index)?, header.as_str())?;
            }
        }
        workbook.save(path)?;
        Ok(())
    }

    fn stated_headers(kind: SheetKind) -> Vec<String> {
        kind.headers()
            .iter()
            .map(|header| (*header).to_owned())
            .collect()
    }

    /// A header row that is not the sheet's own stops the run [IMP-SAXO-002], [TST-031]: it is
    /// this refusal that makes every other test's "the fixture parsed" an assertion about the
    /// headers, so a changed export shape must fail here rather than produce a fixture nothing is
    /// specified against.
    #[test]
    fn a_header_row_that_is_not_the_sheets_own_stops_the_run() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("export.xlsx");
        let mut bookings = stated_headers(SheetKind::Bookings);
        // The export writes this one with non-breaking spaces; an ordinary space is the smallest
        // difference a changed export could arrive as.
        bookings[SheetKind::Bookings.column("Amount\u{a0}Type\u{a0}Id")] =
            "Amount Type Id".to_owned();
        workbook_with(
            &path,
            &[
                (
                    SheetKind::Transacties.name(),
                    stated_headers(SheetKind::Transacties),
                ),
                (SheetKind::Detail.name(), stated_headers(SheetKind::Detail)),
                (SheetKind::Bookings.name(), bookings),
            ],
        )
        .expect("the test workbook is written");

        let refusal = read(&path)
            .expect_err("a header row that is not the sheet's own is refused")
            .to_string();

        assert!(refusal.contains("Bookings"), "{refusal}");
    }

    /// A missing sheet stops the run: the file is three sheets and a fixture of two is a fixture
    /// of nothing [TST-031].
    #[test]
    fn a_missing_sheet_stops_the_run() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("export.xlsx");
        workbook_with(
            &path,
            &[
                (
                    SheetKind::Transacties.name(),
                    stated_headers(SheetKind::Transacties),
                ),
                (SheetKind::Detail.name(), stated_headers(SheetKind::Detail)),
            ],
        )
        .expect("the test workbook is written");

        let refusal = format!(
            "{:#}",
            read(&path).expect_err("an export without Bookings is refused")
        );

        assert!(refusal.contains("Bookings"), "{refusal}");
    }

    /// What [`write`] puts on disk, read back: the sheet names, the cells in the shapes they were
    /// written in, the serial dates under a date format and the blank cell as the zero-length
    /// string the export holds [IMP-SAXO-002], [IMP-SAXO-003], [TST-030]. Twice written is byte
    /// for byte the same file, which is what makes a regenerated fixture a diff only where a row
    /// changed.
    #[test]
    fn a_written_fixture_reads_back_as_it_was_written_and_is_byte_reproducible() {
        let export = Export {
            sheets: [
                sheet(
                    SheetKind::Transacties,
                    &[&[
                        ("Transactiedatum", "45000"),
                        ("Acties", "Koop 40 @ 5.75 USD"),
                        ("Boekingsbedrag", "-238.00"),
                    ]],
                ),
                sheet(
                    SheetKind::Detail,
                    &[&[
                        ("Uitvoeringsdatum transactie", "45000.5"),
                        ("Prijs", "51.4"),
                    ]],
                ),
                sheet(
                    SheetKind::Bookings,
                    &[&[("Boekingsdatum", "45000"), ("Boekingsbedrag", "2.38")]],
                ),
            ],
        };
        let directory = tempfile::tempdir().expect("a temporary directory");
        let first = directory.path().join("first.xlsx");
        let second = directory.path().join("second.xlsx");

        write(&first, &export).expect("the fixture is written");
        write(&second, &export).expect("the fixture is written again");

        assert_eq!(
            std::fs::read(&first).expect("the first file"),
            std::fs::read(&second).expect("the second file"),
            "two runs of the writer produced different bytes"
        );
        let read_back = read(&first).expect("the written fixture reads back");
        for kind in SheetKind::ALL {
            assert_eq!(
                read_back.sheet(kind).rows,
                export.sheet(kind).rows,
                "{} did not survive the round trip",
                kind.name()
            );
        }

        // The fixed creation time is the whole of byte-reproducibility: without it the container
        // records the moment of the run and every rerun is a diff.
        let mut container =
            zip::ZipArchive::new(std::fs::File::open(&first).expect("the written file"))
                .expect("a zip container");
        let mut properties = String::new();
        std::io::Read::read_to_string(
            &mut container
                .by_name("docProps/core.xml")
                .expect("the container's properties"),
            &mut properties,
        )
        .expect("the properties read");
        assert!(
            properties.contains("2000-01-01T00:00:00Z"),
            "the container records the moment it was written: {properties}"
        );

        let mut workbook: Xlsx<_> = open_workbook(&first).expect("a real XLSX container");
        assert_eq!(
            workbook.sheet_names(),
            vec!["Transacties", "_Transacties", "Bookings"]
        );
        let range = workbook
            .worksheet_range("Transacties")
            .expect("the cash ledger");
        let row: Vec<Data> = range.rows().nth(1).expect("the one data row").to_vec();
        assert!(
            matches!(
                row[SheetKind::Transacties.column("Transactiedatum")],
                Data::DateTime(_)
            ),
            "a date is not written under a date format: {row:?}"
        );
        assert!(
            matches!(
                row[SheetKind::Transacties.column("Boekingsbedrag")],
                Data::Float(_)
            ),
            "an amount is not written as a plain number: {row:?}"
        );
        // The one known divergence from the real file: a blank round-trips as an empty cell
        // rather than as a zero-length shared string, and a reader must accept both [TST-030].
        assert_eq!(row[SheetKind::Transacties.column("Aantal")], Data::Empty);
        let detail = workbook
            .worksheet_range("_Transacties")
            .expect("the position side");
        let stamp = detail.rows().nth(1).expect("the one data row")
            [SheetKind::Detail.column("Uitvoeringsdatum transactie")]
        .clone();
        assert!(
            matches!(stamp, Data::DateTime(_)),
            "an execution stamp is not written under a date-and-time format: {stamp:?}"
        );
    }

    /// The per-currency suffix marks a sub-account of one Depot [IMP-SAXO-005].
    #[test]
    fn an_account_splits_into_a_base_and_a_currency() {
        assert_eq!(account_base("69900/1000000EUR"), "69900/1000000");
        assert_eq!(account_base("69900/1000000CAD"), "69900/1000000");
        assert_eq!(currency_suffix("69900/1000000USD"), "USD");
        assert_eq!(currency_suffix("69900/1000000"), "");
    }

    /// The label keeps its quantity and its direction; only the price moves [IMP-SAXO-011],
    /// [TST-028]. The label is the only place a Saxo quantity appears on `Transacties`, `Aantal`
    /// being a cash movement and never a quantity [IMP-SAXO-009], so this is where TST-028 bites
    /// for that sheet.
    #[test]
    fn a_label_keeps_its_quantity_and_direction() {
        let pseudonyms = Pseudonyms::build(&Originals::default()).unwrap();
        let relabeled = relabel(&pseudonyms, "Verkoop -60 @ 30.65 EUR");
        assert!(relabeled.starts_with("Verkoop -60 @ "), "{relabeled}");
        assert!(relabeled.ends_with(" EUR"), "{relabeled}");
        assert!(!relabeled.contains("30.65"), "{relabeled}");
        assert_eq!(relabel(&pseudonyms, "Stock split"), "Stock split");
        assert!(
            relabel(&pseudonyms, "Deponering 3000 @ 139.46 EUR").starts_with("Deponering 3000 @ ")
        );
    }

    /// A dividend per share moves, its currency and Saxo's `-` marker do not [TST-012].
    #[test]
    fn a_dividend_per_share_keeps_its_currency_and_its_blank_marker() {
        let moved = dividend_per_share("0.83 USD");
        assert!(moved.ends_with(" USD"), "{moved}");
        assert_ne!(moved, "0.83 USD");
        assert_eq!(dividend_per_share("-"), "-");
        assert_eq!(dividend_per_share(""), "");
    }

    /// A replacement is written in the shape of the cell it replaces.
    #[test]
    fn a_number_stays_a_number_and_a_blank_stays_blank() {
        assert_eq!(
            same_shape_as(&Cell::Number(Decimal::ONE), "3000000011"),
            Cell::Number(Decimal::from(3_000_000_011_u64))
        );
        assert_eq!(
            same_shape_as(&Cell::Text("x".into()), "40000000013"),
            Cell::Text("40000000013".into())
        );
        assert_eq!(same_shape_as(&Cell::Empty, ""), Cell::Empty);
    }

    /// Amounts move, everything else in an unmapped column does not. A date serial is a number
    /// too and stays exactly where it was [TST-028]: a moved date would break the per-file
    /// calendar-year boundary and the ordering cases the fixtures exist for. So does a quantity
    /// and so does a withholding percentage, on either of the two detail sheets.
    #[test]
    fn only_the_amount_columns_are_perturbed() {
        let amount = Cell::Number(Decimal::new(181_324, 2));
        assert_ne!(rewritten_amount("Aantal", &amount, None), amount);
        assert_ne!(
            rewritten_amount("Verhandelde waarde", &amount, None),
            amount
        );
        assert_eq!(rewritten_amount("Type", &amount, None), amount);
        for structural in [
            "Traded\u{a0}Quantity",
            "In aanmerking komend aantal",
            "Tax\u{a0}Percentage",
        ] {
            assert_eq!(rewritten_amount(structural, &amount, None), amount);
        }
        for date_header in DATE_HEADERS.iter().chain(&TIMESTAMP_HEADERS) {
            let serial = Cell::Number(Decimal::from(45_000));
            assert_eq!(rewritten_amount(date_header, &serial, None), serial);
        }
    }

    /// A linked amount takes the value its own booking decided, not its own magnitude's.
    #[test]
    fn a_linked_amount_overrides_the_magnitude_rule() {
        let amount = Cell::Number(Decimal::new(238, 2));
        let linked = ("Boekingsbedrag", Decimal::new(251, 2));
        assert_eq!(
            rewritten_amount("Boekingsbedrag", &amount, Some(&linked)),
            Cell::Number(Decimal::new(251, 2))
        );
        // The link names one column; another amount on the same row is unaffected.
        assert_ne!(
            rewritten_amount("Omwisselkosten", &amount, Some(&linked)),
            Cell::Number(Decimal::new(251, 2))
        );
    }

    /// A booking's components still sum to the booking they belong to [TST-031], and a group
    /// summing to the row's EUR `Aantal` rather than to its `Boekingsbedrag` is carried too.
    #[test]
    fn the_components_of_a_booking_are_moved_with_it() {
        let export = Export {
            sheets: [
                sheet(
                    SheetKind::Transacties,
                    &[
                        &[
                            ("Bk\u{a0}Record\u{a0}Id", "1424145358"),
                            ("Aantal", "2.03"),
                            ("Boekingsbedrag", "2.12"),
                        ],
                        &[
                            ("Bk\u{a0}Record\u{a0}Id", "1424145359"),
                            ("Aantal", "-5.47"),
                            ("Boekingsbedrag", "-5.47"),
                        ],
                    ],
                ),
                sheet(SheetKind::Detail, &[]),
                sheet(
                    SheetKind::Bookings,
                    &[
                        &[
                            ("Bk\u{a0}Record\u{a0}Id", "1424145358"),
                            ("Boekingsbedrag", "2.38"),
                        ],
                        &[
                            ("Bk\u{a0}Record\u{a0}Id", "1424145358"),
                            ("Boekingsbedrag", "-0.35"),
                        ],
                        &[
                            ("Bk\u{a0}Record\u{a0}Id", "1424145359"),
                            ("Boekingsbedrag", "-5.47"),
                        ],
                    ],
                ),
            ],
        };

        let linked = linked_components(&export);
        let moved = |index: usize| linked[index].expect("a carried component").1;
        assert_eq!(
            moved(0) + moved(1),
            perturb(Decimal::new(203, 2)),
            "the components no longer sum to the EUR amount of their booking"
        );
        assert_eq!(moved(2), perturb(Decimal::new(-547, 2)));
    }

    /// A `Bookings` row belongs to one booking only: the first `Transacties` row that joins it in
    /// file order moves it, and a later row joining the wider `Corporate action-Id` group leaves
    /// it alone. Moved twice, the components would stop summing to the booking they belong to
    /// [TST-031].
    #[test]
    fn a_component_already_moved_is_not_moved_again_by_a_wider_group() {
        let export = Export {
            sheets: [
                sheet(
                    SheetKind::Transacties,
                    &[
                        &[
                            ("Bk\u{a0}Record\u{a0}Id", "1424145358"),
                            ("Corporate action-Id", "8909094"),
                            ("Boekingsbedrag", "2.03"),
                        ],
                        &[("Corporate action-Id", "8909094"), ("Aantal", "4.06")],
                    ],
                ),
                sheet(SheetKind::Detail, &[]),
                sheet(
                    SheetKind::Bookings,
                    &[
                        &[
                            ("Bk\u{a0}Record\u{a0}Id", "1424145358"),
                            ("Corporate action-Id", "8909094"),
                            ("Boekingsbedrag", "2.03"),
                        ],
                        &[
                            ("Corporate action-Id", "8909094"),
                            ("Boekingsbedrag", "61.40"),
                        ],
                        &[
                            ("Corporate action-Id", "8909094"),
                            ("Boekingsbedrag", "-59.37"),
                        ],
                    ],
                ),
            ],
        };

        let linked = linked_components(&export);

        assert_eq!(
            linked[0],
            Some(("Boekingsbedrag", perturb(Decimal::new(203, 2)))),
            "the component was moved again against the corporate action's total"
        );
        assert_eq!((linked[1], linked[2]), (None, None));
    }

    /// Where a group sums to neither figure of the row it joins there is no sum to keep, so each
    /// component moves on its own magnitude rather than staying as the export states it
    /// [TST-012], [TST-031].
    #[test]
    fn components_that_sum_to_neither_figure_move_on_their_own_magnitude() {
        let export = Export {
            sheets: [
                sheet(
                    SheetKind::Transacties,
                    &[&[
                        ("Bk\u{a0}Record\u{a0}Id", "1424145358"),
                        ("Aantal", "2.03"),
                        ("Boekingsbedrag", "2.12"),
                    ]],
                ),
                sheet(SheetKind::Detail, &[]),
                sheet(
                    SheetKind::Bookings,
                    &[
                        &[
                            ("Bk\u{a0}Record\u{a0}Id", "1424145358"),
                            ("Boekingsbedrag", "9.99"),
                        ],
                        &[
                            ("Bk\u{a0}Record\u{a0}Id", "1424145358"),
                            ("Boekingsbedrag", "-0.35"),
                        ],
                    ],
                ),
            ],
        };

        let linked = linked_components(&export);

        assert_eq!(linked, vec![None, None], "a sum was invented");
        for (index, stated) in [Decimal::new(999, 2), Decimal::new(-35, 2)]
            .iter()
            .enumerate()
        {
            let cell = Cell::Number(*stated);
            assert_eq!(
                rewritten_amount("Boekingsbedrag", &cell, linked[index].as_ref()),
                Cell::Number(perturb(*stated))
            );
        }
    }

    /// The join is taken on `Bk Record Id` first and only falls through to `Corporate action-Id`
    /// [IMP-SAXO-037]: a row carrying both joins the `Bk Record Id` group, not the other one.
    #[test]
    fn the_booking_join_prefers_the_record_id_over_the_corporate_action() {
        let export = Export {
            sheets: [
                sheet(
                    SheetKind::Transacties,
                    &[&[
                        ("Bk\u{a0}Record\u{a0}Id", "1424145358"),
                        ("Corporate action-Id", "8909094"),
                        ("Boekingsbedrag", "2.03"),
                    ]],
                ),
                sheet(SheetKind::Detail, &[]),
                sheet(
                    SheetKind::Bookings,
                    &[
                        &[
                            ("Bk\u{a0}Record\u{a0}Id", "1424145358"),
                            ("Boekingsbedrag", "2.03"),
                        ],
                        &[
                            ("Corporate action-Id", "8909094"),
                            ("Boekingsbedrag", "61.40"),
                        ],
                        &[
                            ("Corporate action-Id", "8909094"),
                            ("Boekingsbedrag", "-59.37"),
                        ],
                    ],
                ),
            ],
        };

        let linked = linked_components(&export);

        assert_eq!(
            linked[0],
            Some(("Boekingsbedrag", perturb(Decimal::new(203, 2)))),
            "the record-id group was not the one moved"
        );
        assert_eq!(
            (linked[1], linked[2]),
            (None, None),
            "the corporate-action group was moved by a row that joins on its record id"
        );
    }

    /// The middle key decides where the first is absent [IMP-SAXO-037]: a row with no
    /// `Bk Record Id` joins its `Booking Id` group rather than its `Corporate action-Id` one.
    #[test]
    fn the_booking_join_falls_through_to_the_booking_id_before_the_corporate_action() {
        let export = Export {
            sheets: [
                sheet(
                    SheetKind::Transacties,
                    &[&[
                        ("Booking\u{a0}Id", "14836444374"),
                        ("Corporate action-Id", "8909094"),
                        ("Boekingsbedrag", "2.03"),
                    ]],
                ),
                sheet(SheetKind::Detail, &[]),
                sheet(
                    SheetKind::Bookings,
                    &[
                        &[
                            ("Booking\u{a0}Id", "14836444374"),
                            ("Boekingsbedrag", "2.03"),
                        ],
                        &[
                            ("Corporate action-Id", "8909094"),
                            ("Boekingsbedrag", "61.40"),
                        ],
                        &[
                            ("Corporate action-Id", "8909094"),
                            ("Boekingsbedrag", "-59.37"),
                        ],
                    ],
                ),
            ],
        };

        let linked = linked_components(&export);

        assert_eq!(
            linked[0],
            Some(("Boekingsbedrag", perturb(Decimal::new(203, 2)))),
            "the booking-id group was not the one moved"
        );
        assert_eq!(
            (linked[1], linked[2]),
            (None, None),
            "the corporate-action group was moved by a row that joins on its booking id"
        );
    }

    /// A traded value moves with its price, so the fixture still states a value the quantity and
    /// the price produce [TST-031]; the quantity itself never moves [TST-028].
    #[test]
    fn a_traded_value_is_moved_with_its_price() {
        let detail = sheet(
            SheetKind::Detail,
            &[&[
                ("Traded\u{a0}Quantity", "300"),
                ("Prijs", "51.40"),
                ("Verhandelde waarde", "-15419.46"),
            ]],
        );
        let linked = linked_traded_values(&detail);
        let (column, value) = linked[0].expect("a linked traded value");

        assert_eq!(column, "Verhandelde waarde");
        let stated = Decimal::new(-1_541_946, 2);
        assert_ne!(value, stated, "the traded value came through unchanged");
        // The relation is what must survive, not any particular factor: the moved value over the
        // moved price is still the row's `Traded Quantity`, as far out as the export's own pair
        // was plus the cent the value is rounded at.
        let quantity = Decimal::new(300, 0);
        let price = Decimal::new(5140, 2);
        let before = (stated.abs() / price - quantity).abs();
        let after = (value.abs() / perturb(price) - quantity).abs();
        assert!(
            after <= before + Decimal::new(1, 3),
            "{value} at {} is {} shares, not {quantity}",
            perturb(price),
            value.abs() / perturb(price)
        );
    }

    /// A row that joined its counterpart before anonymization joins the same counterpart after
    /// [TST-031]: one original is one pseudonym, on every sheet.
    #[test]
    fn the_join_keys_survive_anonymization() {
        let export = Export {
            sheets: [
                sheet(
                    SheetKind::Transacties,
                    &[&[
                        ("Klant-id", "15853040"),
                        ("Rekening-ID", "69900/1000000EUR"),
                        ("Transactie-ID", "5057936890"),
                        ("Corporate action-Id", "8909094"),
                        ("Bk\u{a0}Record\u{a0}Id", "1424145358"),
                        ("Booking\u{a0}Id", "14836444374"),
                    ]],
                ),
                sheet(
                    SheetKind::Detail,
                    &[&[
                        ("Rekening-ID", "69900/1000000EUR"),
                        ("Transactie-ID", "5057936890"),
                        ("Corporate action-Id", "8909094"),
                        ("Order-ID", "0"),
                    ]],
                ),
                sheet(
                    SheetKind::Bookings,
                    &[&[
                        ("Rekening-ID", "69900/1000000EUR"),
                        ("Bk\u{a0}Record\u{a0}Id", "1424145358"),
                        ("Booking\u{a0}Id", "14836444374"),
                        ("Corporate action-Id", "8909094"),
                    ]],
                ),
            ],
        };
        let mut originals = Originals::default();
        collect(&export, &mut originals);
        let pseudonyms = Pseudonyms::build(&originals).expect("the table builds");

        let anonymized = anonymize(&export, &pseudonyms).expect("the export is anonymized");

        for (header, left, right) in [
            ("Transactie-ID", SheetKind::Transacties, SheetKind::Detail),
            (
                "Corporate action-Id",
                SheetKind::Transacties,
                SheetKind::Detail,
            ),
            (
                "Bk\u{a0}Record\u{a0}Id",
                SheetKind::Transacties,
                SheetKind::Bookings,
            ),
            (
                "Booking\u{a0}Id",
                SheetKind::Transacties,
                SheetKind::Bookings,
            ),
            (
                "Corporate action-Id",
                SheetKind::Transacties,
                SheetKind::Bookings,
            ),
        ] {
            let key =
                |kind: SheetKind| field(kind, &anonymized.sheet(kind).rows[0], header).as_text();
            assert_eq!(key(left), key(right), "{header} no longer joins");
            assert_ne!(
                key(left),
                field(left, &export.sheet(left).rows[0], header).as_text(),
                "{header} was not replaced"
            );
        }
        // `0` is a marker rather than an order, so it is the one id value that passes through.
        assert_eq!(
            field(
                SheetKind::Detail,
                &anonymized.sheet(SheetKind::Detail).rows[0],
                "Order-ID"
            )
            .as_text(),
            "0"
        );
    }
}
