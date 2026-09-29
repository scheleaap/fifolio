-- A `transfer_out`'s target security and ratio: what the `transfer_in` records it emits on
-- approval open, and how their quantities are scaled [DOM-090], [DOM-115]. The ratio is an integer
-- numerator and denominator, never a decimal (DEC-055), read back as the split's ratio is.
--
-- A table of its own rather than columns on `transaction_transfer_out`: `alter table` can add a
-- `not null` column only with a default, and a default target security or ratio would be an
-- invented one. No existing `transfer_out` is backfilled: nothing has derived one before this
-- item, and one without its target reads back as corrupt rather than as a guess.
create table transaction_transfer_out_target (
    transaction_id       integer primary key
                         references transaction_transfer_out (transaction_id) on delete cascade,
    target_security_isin text not null references security (isin),
    ratio_numerator      integer not null,
    ratio_denominator    integer not null
) strict;
