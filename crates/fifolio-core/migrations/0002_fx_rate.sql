-- The local cache of ECB daily euro reference rates [ARC-015].
--
-- Keyed by currency and date: the composite primary key is what makes one publication per
-- currency per day the table's own rule, so a restated day is a conflict the repository resolves
-- rather than a second row. That order -- currency first -- is also the order the in-memory
-- snapshot keys on, where the backward range scan for the most recent publication on or before a
-- date happens [DOM-034]; the read here is the whole table at once and needs no index of its own.
--
-- `rate` is foreign units per EUR [ARC-027, DOM-086], written as TEXT at scale 6 by the
-- repository that stores it, as every other decimal column is.
--
-- No column records which feed a row came from: the full historical series and the rolling
-- 90-day window publish the same figures for the same days [ARC-017, ARC-018], so a provenance
-- column would distinguish two paths to one fact.

create table fx_rate (
    currency  text not null,
    rate_date text not null,
    rate      text not null,
    primary key (currency, rate_date)
) strict;
