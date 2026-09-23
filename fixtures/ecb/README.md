# ECB euro reference-rate fragments

Two documents in the shape the ECB publishes its daily euro reference rates in, committed so that
the cache-seeding path is tested against a recorded fragment of the series and never against a live
fetch [TST-019].

    eurofxref-hist-fragment.xml   stands in for the complete historical series [ARC-017]
    eurofxref-90d-fragment.xml    stands in for the rolling 90-day window [ARC-018]

## What they hold

Both are the `gesmes` envelope with the namespaces, the header elements and the newest-day-first
ordering of the real documents, trimmed to a handful of currencies and three publication days:

| Day | Currencies | Why it is here |
| --- | --- | --- |
| 1999-01-04 | USD, JPY | the first publication of the series, so the 1999 bound has a real day behind it |
| 2024-03-28 | USD, CAD | the Thursday before Good Friday, published |
| 2024-04-02 | USD | the Tuesday after Easter Monday, published, and absent from the historical fragment so the top-up has something to add |

Good Friday, the weekend and Easter Monday are simply absent, as they are in the published series:
a day the ECB did not publish for has no `Cube` of its own. That is what exercises the
previous-publication fallback of `fx.rs` against a real gap rather than an invented one.

The 90-day fragment restates 2024-03-28 deliberately. The real window overlaps whatever the
historical series already deposited, and a top-up must leave those rows alone.

## What they are for

**Parsing and ingestion. Never arithmetic.** The rates are the published figures for those days
and the fragments are small enough to read, but nothing asserts a converted amount against them:
the money rules are tested with synthetic inputs whose results are derivable by hand [TST-015].

The values are trimmed from the published series rather than generated, so the documents are not
anonymized and need no generator: a public reference rate identifies nobody.
