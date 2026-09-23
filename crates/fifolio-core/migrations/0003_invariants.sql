-- The relations the storage-enforced invariants are stated over, and nothing beyond them.
--
-- Still deliberately absent: a transaction's relation to the source records it *consumes*, which
-- is DOM-013 and undecided (OQ-002), and the canonical order across files, which is DOM-111 and
-- undecided (OQ-007). `transaction_citation` already records what a transaction cites, and
-- citation is not consumption; `transaction_placement.derived_by_batch` below is the coarser fact
-- DOM-072 and DOM-119 are stated over — which import produced a transaction — and says nothing
-- about which of that import's records it consumed, so it decides neither question.

-- Which import owns a source record, which is what "the records it owns" means in DOM-119.
-- Nullable because SQLite may only add a column carrying a foreign key when its default is null;
-- the repository takes a batch on every insert, so no row is written without one.
alter table source_record add column batch_id integer references import_batch (id);

-- A transaction's account and security [DOM-066, DOM-068, DOM-072], and the import that derived
-- it. A table of its own rather than columns on `transaction_record`, because `alter table`
-- cannot add the composite foreign key to `account`. Written in the same SQLite transaction as
-- the header row, so a stored transaction is always placed.
create table transaction_placement (
    transaction_id   integer primary key references transaction_record (id) on delete cascade,
    account_broker   text not null,
    account_id       text not null,
    security_isin    text not null references security (isin),
    -- Null for a transaction no import derived: a `transfer_in` emitted on approval is derived
    -- from no row at all [DOM-090].
    derived_by_batch integer references import_batch (id),
    foreign key (account_broker, account_id) references account (broker, id)
) strict;

-- Attribution of one closing to the openings it consumes, as the user approved it [DOM-054].
--
-- The account and security the invariants key on [DOM-066, DOM-068] are the closing's own, read
-- through `transaction_placement`, rather than copied here where the two could disagree.
create table attribution (
    id                     integer primary key,
    -- Unique as a backstop only: the repository refuses a second approval of the same closing
    -- with a named error, this constraint being unreachable through it.
    closing_transaction_id integer not null unique references transaction_record (id)
) strict;

-- An allocation stores only an opening, a closing and a quantity [DOM-058]; the closing is the
-- attribution's. Every monetary figure is derived on demand and is FIF-014's.
create table attribution_allocation (
    attribution_id         integer not null references attribution (id) on delete cascade,
    -- The caller's order, which is the order the shares of a divided figure are rounded in
    -- [DOM-062, DOM-063].
    ordinal                integer not null,
    opening_transaction_id integer not null references transaction_record (id),
    quantity               text not null,
    primary key (attribution_id, ordinal)
) strict;

-- The `transfer_in` records a `transfer_out` emitted on approval, one per consumed parcel
-- [DOM-090]. The link is what makes an emitted record recognizable as emitted, which is what
-- DOM-094 refuses to break.
create table emitted_transfer_in (
    transfer_in_id  integer primary key references transaction_record (id) on delete cascade,
    transfer_out_id integer not null references transaction_record (id) on delete cascade
) strict;
