-- A split's ratio: an integer numerator and denominator, never a decimal, since a one-for-three
-- has no finite decimal expansion [DOM-113]. Reading one back refuses a value `Ratio`
-- cannot hold, as the manual entry's ratio columns do.
--
-- No existing split is backfilled: nothing has derived a split transaction before this item, and
-- a ratio cannot be invented for one.
create table transaction_split (
    transaction_id    integer primary key references transaction_record (id) on delete cascade,
    ratio_numerator   integer not null,
    ratio_denominator integer not null
) strict;
