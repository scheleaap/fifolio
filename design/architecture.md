# Workspace

A single Cargo workspace with three crates: [ARC-001]

* `fifolio-core`: domain model, FIFO engine, reports, importers, FX rate resolution, SQLite storage. No HTTP, no terminal [ARC-002]
* `fifolio-server`: HTTP API over `fifolio-core`. The only process that opens the database [ARC-003]
* `fifolio-cli`: HTTP client and terminal UI. Holds no domain logic [ARC-004]

The CLI reaches data exclusively over HTTP. [ARC-005] This keeps a single writer to the database and a single implementation of the FIFO rules.

# Numbers

Arbitrary-precision decimals throughout (`rust_decimal`). Floating point is never used for money or quantities. [ARC-006]

Scales: [ARC-007]

| Kind | Decimals | Notes |
| --- | --- | --- |
| Quantity | up to 8 | Fractional shares are supported (savings plans, fractional purchases) |
| Unit price | up to 6 | |
| Monetary amount (fees, totals, gains) | 2 | |
| FX rate | 6 | foreign units per EUR, the ECB convention; see `domain.md` |

Trade value is `quantity × unit_price × factor`, where the factor comes from the security's quotation: 1 per unit, 0.01 for percent of par. [ARC-008] See `domain.md`.

Intermediate arithmetic keeps full precision. [ARC-009] Rounding to the scales above happens **before** the storage and presentation boundaries, half away from zero, and is the caller's act rather than the boundary's: storage refuses a value that is not already at its scale instead of quietly rounding it. [ARC-010] A store that rounds on the way in cannot tell a figure that was meant to be rounded from one that arrived wrong, and the second is the case worth catching. [ARC-025] "Half-up" is ambiguous for negative amounts, and a realized loss is negative. The one place rounding is load-bearing is allocation shares, which follow the drift rule in `domain.md`.

# Storage

Local SQLite database, one file. [ARC-011] Schema migrations are versioned and applied on startup. [ARC-012]

The database is `./fifolio.db` in the working directory unless `--database` says otherwise. [ARC-013] The file is created on first run. [ARC-014]

# FX rates

ECB daily euro reference rates are cached in a local table, keyed by currency and date. [ARC-015] Once cached, imports work offline. [ARC-016]

The ECB publishes the current rates and a rolling 90-day window, which is not enough for a first import of several years of history. The cache is therefore seeded once from the ECB's complete historical series (all currencies back to 1999, a few megabytes) [ARC-017] and topped up from the 90-day feed afterwards. [ARC-018]

An import that needs a rate which is neither cached nor fetchable fails with a clear error naming the currency and date, rather than guessing. [ARC-019]

The fallback to an earlier rate is bounded: no rate exists before the series begins in 1999, and a substitution more than seven days stale is an error rather than a silent approximation. [ARC-027]

# Errors

The API returns RFC 9457 `application/problem+json` for all error responses, [ARC-020] with a stable machine-readable `type` per error class. [ARC-021]

# Security model

Local, single user, no authentication. The server binds `127.0.0.1` only. [ARC-022] Exposing it to a network is an explicit non-goal: the data is financial and entirely unprotected.

# Reading source files

Saxo exports are XLSX, Trade Republic CSV, so the importers need a spreadsheet reader as well as a CSV reader. [ARC-023] Format detail is in `importers.md`.

# Testing

Domain rules, the FIFO engine, rounding behavior, importers and report shaping live in `fifolio-core` and are tested without a server or a terminal. [ARC-024] The terminal UI is a thin render-and-dispatch layer over a UI-agnostic client layer, so that everything except drawing is testable headlessly. [ARC-025]

Layers, coverage thresholds, fixtures and the property tests are in `testing.md`.

The CLI is localized (Dutch and English); see `cli.md`. Translation is confined to the CLI, so `fifolio-core` and `fifolio-server` carry no locale concerns. [ARC-026]
