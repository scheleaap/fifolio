# Name

The project is called `fifolio-cli`.

See `architecture.md` for the workspace layout and `domain.md` for the attribution rules.

# Requirements

CLI:
* `fifolio-cli` starts the interactive application [CLI-001]
* `fifolio-cli report <report name>` prints the corresponding report (see domain description). [CLI-002] Optional `--format` to produce human-readable, CSV or JSON output. [CLI-003] Optional `--account` and `--year` filters [CLI-004]
* `fifolio-cli export-manual-information` writes every `manual` source record to a file [CLI-005]
* `--server-url` selects the server, defaulting to `http://127.0.0.1:8000` [CLI-006]
* `--lang` selects the interface language, `en` or `nl` [CLI-007]

Reports are deliberately not part of the interactive application [CLI-008]: as a subcommand they can be piped, redirected and diffed.

## Exporting manual information

Everything else in the database can be rebuilt by importing the broker files again. Manual source records cannot: they are the acquisition dates, share counts, stock-or-cash elections and exchange details that no export contains, and reconstructing them means the research was done twice.

`export-manual-information` writes them all to a file, so they can be kept alongside the broker exports. The file records, per entry, the account, the security, what was supplied, and which imported source records it was attached to. [CLI-009]

Open: whether a matching import exists, which would make the file a restore path rather than only a record.

# Interactive application

## Layout

A single full-width view at a time, with a breadcrumb at the top and a context hint bar at the bottom. [CLI-010]

```
Accounts › Saxo NL › IE00B4L5Y983
┌─────────────────────────────────────────────────────┐
│ Date        Type  Qty     Price    Fees   EUR       │
│ 2024-03-04  BUY    10.00  182.30    2.50  1825.50   │
│ 2024-05-11  BUY     5.00  191.00    2.50   957.50   │
│ 2025-01-20  SELL   12.00  210.55    2.50  2524.10   │
└─────────────────────────────────────────────────────┘
 Esc Back   a Attribute   i Import   q Quit
```

Enter descends into the selected row, Esc ascends. [CLI-011] The view stack is Accounts › Securities › Transactions, with the attribution flow and the completion queue reachable from an account or security. [CLI-012]

## Hint bar

A single dim line listing only the keys that are valid in the current view, each paired with a verb. [CLI-013] No fixed function-key row: F-keys are frequently intercepted by terminal emulators, and an always-visible row spends a line on actions that mostly do not apply.

## Dialogs

Dialog buttons name their action rather than answering a question, so the consequence is readable without reading the prompt. [CLI-014]

```
Delete attribution for the sale of 12.00 IE00B4L5Y983 on 2025-01-20?

                                    [ Cancel ]  [ Delete ]
```

The safe choice is leftmost and focused by default. [CLI-015] Destructive actions are visually distinct. [CLI-016]

## Scope

The interactive application covers browsing, account and security management, import, the completion queue and the attribution flow. [CLI-017] Reports are the `report` subcommand.

Imports are browsable as batches, with their file, account, timestamp and counts, and a batch can be deleted to undo it. [CLI-018] Deleting a batch whose transactions are attributed is refused, and the dialog names what stands in the way. [CLI-019]

## Completion queue

Import leaves source records that affect holdings but lack something only the user knows. The queue is where they are resolved, and a security with anything outstanding is blocked from attribution, so the queue is the first thing to clear after an import.

An entry shows the imported rows it covers, every figure the file does state, and the one part that is missing. [CLI-020] What is asked depends on the event:

| Event | Supplied by the user |
| --- | --- |
| Stock or cash dividend | which was elected, and if stock, the share count |
| Split | the ratio |
| Exchange or share-class swap | the target security and the ratio |
| Cash merger, tender, partial buyback | the quantity disposed, and any shares received |

Nothing is typed twice: the money, dates, currency and rate always come from the file. [CLI-021] What the user supplies is stored as a `manual` source record and cited beside the imported rows, so a derived transaction always names every source it rests on.

Where a group contains a reversal, the entry shows it as a reversal rather than folding it into a total, [CLI-022] so a corrected booking is never read as an additional one.

## Sale attribution

1. The user selects an account, and optionally a security
2. The application requests the next sell awaiting attribution, and displays it with its proposed buys and the derived figures per allocation: quantity consumed, allocated cost, allocated buy fee, allocated proceeds, allocated sell fee, gain or loss
3. The user approves or declines. Approving posts the proposal back for confirmation; declining stores nothing [CLI-033]
4. On approval the application moves to the next pending sell. On decline it stops, because later sells for that account and security are blocked until this one is resolved
5. If the sell cannot be covered by the available buys, the shortfall is shown instead of a proposal, with the missing quantity named. The usual remedy is importing the missing history, or completing a corporate action that created units
6. If the security has anything outstanding in the completion queue, the application says so and links to it rather than offering a proposal [CLI-034]

# Localization

The interactive application is available in Dutch and English.

## Language selection

`--lang` wins when given. [CLI-023] Otherwise the language is derived from the environment (`LC_ALL`, `LC_MESSAGES`, `LANG`): a locale resolving to `nl` selects Dutch, anything else selects English. [CLI-024] An unrecognized `--lang` value is an error listing the supported languages, rather than a silent fallback. [CLI-025]

## What is translated

Interface text only: view titles, breadcrumbs, column headers in the interactive views, hint bar verbs, dialog prompts and buttons, and user-facing error and status messages. [CLI-026]

Deliberately not translated:

* Report output in any format, including the human-readable one. [CLI-035] Reports are documents that get filed, shared and diffed
* CSV and JSON output of any kind
* Stored data: account names, security names, import source filenames
* The API's machine-readable error types. The CLI maps those to translated messages; the wire format stays stable [CLI-036]

## Formatting

Dates and numbers follow the language in the interactive application only. [CLI-027]

| | English | Dutch |
| --- | --- | --- |
| Date | 2025-01-20 | 20-01-2025 |
| Amount | 1825.50 | 1.825,50 |

Everywhere else, including all report output, dates are ISO 8601 and decimals use a dot. [CLI-028] A comma decimal separator in CSV is a recurring source of breakage, and ISO dates sort correctly.

## Implementation

Translations live in message catalogs, one per language, keyed by identifier rather than by English source string, so that changing English wording does not silently invalidate the Dutch translation. Plurals and interpolation are handled by the catalog format, not by string concatenation. Project Fluent (`fluent`, `unic-langid`) covers this; `rust_decimal` and `chrono`/`jiff` formatting are driven by the selected locale.

A missing Dutch key falls back to English rather than showing the raw key, [CLI-029] and the build fails on any key present in one catalog but absent from the other. [CLI-030]

## Layout consequence

Dutch strings are typically longer than their English counterparts. Views must lay out from measured string widths, never from hardcoded column positions, [CLI-031] and the hint bar must degrade gracefully when the terminal is too narrow for every hint. [CLI-032]
