# Name

The project is called `fifolio-server`. [SRV-001]

See `architecture.md` for the workspace layout, number handling, storage, error format and security model, and `importers.md` for the format mappings.

# Requirements

CLI:
* `fifolio-server` starts the server on `127.0.0.1:8000`. [SRV-002] Optional `--port` argument [SRV-003]
* `--database` selects the SQLite file, defaulting to `./fifolio.db` [SRV-004]
* `fifolio-server openapi` prints the server's OpenAPI spec (does not start the server) [SRV-005]

# API

## Meta

* `GET /openapi.json` returns the server's OpenAPI spec [SRV-006]

## Accounts and securities

* CRUDL endpoints for accounts and securities [SRV-007]
* An account cannot be deleted if it is referenced by any source record [SRV-008]
* A security cannot be deleted if it is referenced by any source record [SRV-009]
* A security's ISIN is unique; creating a duplicate is a conflict [SRV-010]
* A security's type and quotation are editable, which is how an auto-created record is corrected [SRV-011]

## Import

* Caller supplies the target account explicitly, the file format, and the file [SRV-012]. Source files rarely identify the account reliably
* Supported formats: Saxo NL XLSX, Trade Republic DE CSV [SRV-013]
* Unknown ISINs are created automatically and flagged as auto-created for later review [SRV-014]
* Import is idempotent, on the identity rules in `domain.md` [SRV-015]
* Rows that carry no position effect are recognized and not stored [SRV-016]
* The response summarizes: derived automatically, pending, recognized as non-position, failed to parse, and securities auto-created [SRV-017]
* A sell that exceeds the holdings does not block import. It surfaces later, at attribution [SRV-018]

Every import creates a batch, and a batch is the unit of undo: [SRV-019]

* Read and list import batches, with their account, filename, format, timestamp and counts [SRV-020]
* Delete a batch, which removes exactly the source records it created and anything derived from them [SRV-021]
* Deletion is refused, naming the offenders, if any derived transaction participates in an attribution [SRV-022]. Delete those attributions first

## Source records

* Read and list source records. List filters: account, batch, security, kind, consumed or pending [SRV-023]
* The pending list is the completion queue, and is the main thing the interactive client works through [SRV-024]
* Create a `manual` source record. This is the only way information that no export contains enters the system [SRV-025]
* List all `manual` records for export (see `cli.md`) [SRV-026]

Source records are never edited. [SRV-027] A mistake is corrected by deleting the derived transaction and the manual record, then supplying a new one.

## Transactions and corporate actions

* Read and list transactions. List filters: account, security, type, date range [SRV-028]
* List unattributed sell transactions, as a filter on the list endpoint [SRV-029]
* Read and list corporate actions [SRV-030]
* Derive a transaction or a corporate action from one or more pending source records, together with whatever the user had to supply. [SRV-031] The supplied part is stored as a `manual` source record and cited alongside the imported ones [SRV-032]
* Delete a derived transaction or corporate action, returning its source records to pending [SRV-033]

There is no endpoint that creates a transaction from nothing. Everything is derived from source records. [SRV-034]

## Attributions

The server owns the FIFO logic and proposes; the client confirms what it was shown. [SRV-035]

* Get a proposal for a sell: returns the sell, the proposed allocations with their derived figures, and a fingerprint of the proposal. [SRV-036] Also addressable as "the next sell awaiting attribution" for an account and security [SRV-037]
* If the sell cannot be covered by the available unattributed buys, the proposal endpoint returns the shortfall instead of a proposal [SRV-038]
* If the security has any pending source record, the proposal endpoint refuses and names what is outstanding [SRV-039]
* Create an attribution: the client posts the sell, the allocations and the fingerprint it was shown. A fingerprint mismatch is a conflict, [SRV-040] so the client can never approve figures other than the ones displayed
* Read, list and delete attributions. There is no update [SRV-041]
* Deletion is refused if a later attribution exists for the same account and security [SRV-042]

Declining a proposal is not an API call. [SRV-043] Nothing is stored, and the unattributed sell continues to block later sells by itself.

## Reports

* Income tax overview and buy report, both accepting optional account and year filters [SRV-044]
* Reports are read-only projections; the response carries the same figures the CLI formats [SRV-045]

## FX rates

* List cached rates, and trigger a refresh from the ECB feed [SRV-046]
* The cache is seeded from the ECB's full historical series on first use [SRV-047]; see `architecture.md`
