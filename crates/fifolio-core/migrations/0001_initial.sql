-- The initial schema: the types built so far and no more.
--
-- Deliberately absent, because the rules that would shape them are undecided: any relation from
-- a transaction to an account, a security or a source record (DOM-013, FIF-076); ownership of a
-- source record by an import batch (OQ-007); allocations and attributions (FIF-014, FIF-015).
-- A later migration that adds a column is cheaper than a column guessed now.
--
-- Every decimal is TEXT, written at its kind's scale by the repository that stores it: SQLite
-- has no exact decimal type and REAL would be the floating point ARC-006 forbids.
--
-- The only foreign keys to a natural key are `import_batch` and `manual_entry` to `account`: a
-- batch is an import into an account [DOM-017] and an entry answers a record of one [DOM-024],
-- so neither exists without its account and both refuse a write that names an unstored one.
-- `manual_entry.security_isin` deliberately carries no such constraint: an entry may name a
-- security the import has not created yet, and which of the two is written first is FIF-012's
-- to decide. The remaining references are internal parent/child links, cascading on delete.

create table account (
    broker text not null,
    id     text not null,
    primary key (broker, id)
) strict;

create table security (
    -- The ISIN is the natural key, so uniqueness is the primary key itself [DOM-071].
    isin          text not null primary key,
    name          text not null,
    security_type text not null,
    quotation     text not null,
    auto_created  integer not null
) strict;

create table source_record (
    -- Already scoped to its account [DOM-024], so re-importing the same row finds the same key
    -- and writes no second record [DOM-023].
    identity text not null primary key,
    ordering integer not null,
    raw      text not null,
    -- The parsed fields as a JSON object; a BTreeMap serializes in key order, so the stored
    -- text is deterministic.
    parsed   text not null
) strict;

create table import_batch (
    id             integer primary key,
    account_broker text not null,
    account_id     text not null,
    filename       text not null,
    format         text not null,
    imported_at    text not null,
    derived        integer not null,
    pending        integer not null,
    non_position   integer not null,
    failed         integer not null,
    foreign key (account_broker, account_id) references account (broker, id)
) strict;

-- One header row per transaction, with the fields of its variant in a table of that variant's
-- own. A single wide table with nullable columns would be the shape `transaction.rs` refuses:
-- one variant readable as another.
create table transaction_record (
    id         integer primary key,
    kind       text not null,
    trade_date text not null
) strict;

create table transaction_citation (
    transaction_id  integer not null references transaction_record (id) on delete cascade,
    -- The caller's order is the shape a multi-row event had [DOM-016], so it is stored.
    ordinal         integer not null,
    -- The record's identity as a value, not a foreign key: a transaction may cite a record a
    -- later import undo removed, exactly as a manual entry may [DOM-099].
    record_identity text not null,
    primary key (transaction_id, ordinal)
) strict;

create table transaction_buy (
    transaction_id       integer primary key references transaction_record (id) on delete cascade,
    quantity             text not null,
    unit_price_native    text not null,
    unit_price_eur       text not null,
    gross_native         text not null,
    gross_eur            text not null,
    fees_native          text not null,
    fees_eur             text not null,
    origin               text not null,
    conversion_currency  text not null,
    conversion_rate      text not null,
    conversion_source    text not null,
    conversion_rate_date text not null
) strict;

create table transaction_transfer_in (
    transaction_id       integer primary key references transaction_record (id) on delete cascade,
    quantity             text not null,
    cost_basis_native    text not null,
    cost_basis_eur       text not null,
    fees_native          text not null,
    fees_eur             text not null,
    acquisition_date     text not null,
    date_provenance      text not null,
    source               text not null,
    conversion_currency  text not null,
    conversion_rate      text not null,
    conversion_source    text not null,
    conversion_rate_date text not null
) strict;

create table transaction_sell (
    transaction_id       integer primary key references transaction_record (id) on delete cascade,
    quantity             text not null,
    unit_price_native    text not null,
    unit_price_eur       text not null,
    gross_native         text not null,
    gross_eur            text not null,
    fees_native          text not null,
    fees_eur             text not null,
    conversion_currency  text not null,
    conversion_rate      text not null,
    conversion_source    text not null,
    conversion_rate_date text not null
) strict;

create table transaction_expiration (
    transaction_id       integer primary key references transaction_record (id) on delete cascade,
    gross_native         text not null,
    gross_eur            text not null,
    fees_native          text not null,
    fees_eur             text not null,
    conversion_currency  text not null,
    conversion_rate      text not null,
    conversion_source    text not null,
    conversion_rate_date text not null
) strict;

create table transaction_transfer_out (
    transaction_id       integer primary key references transaction_record (id) on delete cascade,
    quantity             text not null,
    fees_native          text not null,
    fees_eur             text not null,
    conversion_currency  text not null,
    conversion_rate      text not null,
    conversion_source    text not null,
    conversion_rate_date text not null
) strict;

-- A split has no fields of its own yet: its ratio is DOM-113 and belongs to FIF-061, so it has
-- no detail table rather than an empty one.

create table manual_entry (
    id               integer primary key,
    account_broker   text not null,
    account_id       text not null,
    security_isin    text not null,
    supplied_kind    text not null,
    -- Populated per shape, which is why these are the schema's only nullable columns: a stock
    -- election has shares, a disposal a quantity and an optional target, a split and an
    -- exchange a ratio.
    shares           text,
    quantity         text,
    ratio_numerator  integer,
    ratio_denominator integer,
    target_isin      text,
    foreign key (account_broker, account_id) references account (broker, id)
) strict;

create table manual_entry_answer (
    manual_entry_id integer not null references manual_entry (id) on delete cascade,
    ordinal         integer not null,
    -- A broker identity rather than an internal key, so the entry survives the deletion of the
    -- records it answers [DOM-099].
    record_identity text not null,
    primary key (manual_entry_id, ordinal)
) strict;
