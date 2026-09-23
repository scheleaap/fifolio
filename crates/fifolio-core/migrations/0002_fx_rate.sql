-- The local cache of ECB daily euro reference rates [ARC-015].
--
-- Keyed by currency and date, which is the key the one query the cache answers needs: the most
-- recent publication for a currency on or before a date [DOM-034]. The primary key is that
-- composite in that order, so the query is a backward range scan over the key itself and needs
-- no index of its own.
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
