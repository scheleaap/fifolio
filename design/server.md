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
* A file covering more than one calendar year is refused [SRV-051]
* Unknown ISINs are created automatically and flagged as auto-created for later review [SRV-014]
* Import is idempotent, on the identity rules in `domain.md` [SRV-015]
* Rows that carry no position effect are recognized and not stored [SRV-016]
* The response summarizes: derived automatically, pending, recognized as non-position, failed to parse, and securities auto-created [SRV-017]
* The summary names every unrecognized row type it met and how many rows carried it, so a new broker type becomes visible on its first appearance [SRV-049]
* A sell that exceeds the holdings does not block import. It surfaces later, at attribution [SRV-018]

Every import creates a batch, and a batch is the unit of undo: [SRV-019]

* Read and list import batches, with their account, filename, format, timestamp and counts [SRV-020]
* A source record belongs to every batch that supplied it, and the **newest** of those owns it. Re-importing a year transfers ownership to the new batch; the superseded batches then own nothing [SRV-052]
* Delete a batch, which removes exactly the source records it owns and anything derived from them. Manual entries are never removed [SRV-021]
* Deletion is refused, naming the offenders, if any derived transaction participates in an attribution [SRV-022]. Delete those attributions first

## Source records and manual entries

* Read and list source records. List filters: account, batch, security, consumed or pending [SRV-023]
* The pending list is the completion queue, and is the main thing the interactive client works through [SRV-024]
* Create a manual entry. This is the only way information that no export contains enters the system [SRV-025]
* List all manual entries for export (see `cli.md`), and list those whose source records are absent, with what each is waiting for [SRV-026]
* Delete a manual entry. This is how an entry that has become obsolete is removed; nothing else deletes one [SRV-053]
* Creating a manual entry is idempotent on its content and the source record identities it cites, so replaying an exported file cannot duplicate one [SRV-048]
* Importing source records whose identities a waiting manual entry names reconnects it and restores the transaction it completed, without asking [SRV-055]

Source records are never edited. [SRV-027] A mistake is corrected by deleting the derived transaction and the manual entry, then supplying a new one.

There is no endpoint that edits a transaction. A transferred parcel's acquisition date in particular is fixed at import and never corrected. [SRV-054]

## Transactions

* Read and list transactions. List filters: account, security, type, date range [SRV-028]
* List unattributed closing transactions, as a filter on the list endpoint [SRV-029]
* Approving a `transfer_out` also creates the `transfer_in` records it implies, in the same operation [SRV-030]
* Derive a transaction from one or more pending source records, together with whatever the user had to supply. [SRV-031] The supplied part is stored as a manual entry and cited alongside the imported records [SRV-032]
* Delete a derived transaction, returning its source records to pending [SRV-033]

There is no endpoint that creates a transaction from nothing. Everything is derived from source records. [SRV-034]

## Attributions

The server owns the FIFO logic and proposes; the client confirms what it was shown. [SRV-035]

* Get a proposal for a closing transaction: returns it, the proposed allocations with their derived figures, and a fingerprint of the proposal. [SRV-036] Also addressable as "the next closing awaiting attribution" for an account and security [SRV-037]
* The fingerprint covers the closing transaction, every allocation's opening id and quantity, and every derived money figure displayed. It is a hash of a canonical serialization, stable across processes, so a re-rating between display and approval changes it. [SRV-050] A fingerprint over ids and quantities alone would let the money change while the guarantee appeared to hold
* If the closing cannot be covered by the available unattributed openings, the proposal endpoint returns the shortfall instead of a proposal [SRV-038]
* If the security has any pending source record in that account, the proposal endpoint refuses and names what is outstanding [SRV-039]
* Create an attribution: the client posts the closing, the allocations and the fingerprint it was shown. A fingerprint mismatch is a conflict, [SRV-040] so the client can never approve figures other than the ones displayed
* Read, list and delete attributions. There is no update [SRV-041]
* Deletion is refused if a later attribution exists for the same account and security [SRV-042]

Declining a proposal is not an API call. [SRV-043] Nothing is stored, and the unattributed closing continues to block later closings by itself.

## Reports

* Income tax overview and acquisition report, both accepting optional account and year filters [SRV-044]
* Reports are read-only projections; the response carries the same figures the CLI formats [SRV-045]

## FX rates

* List cached rates, and trigger a refresh from the ECB feed [SRV-046]
* The cache is seeded from the ECB's full historical series on first use [SRV-047]; see `architecture.md`
