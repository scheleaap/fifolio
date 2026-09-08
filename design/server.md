# Name

The project is called `fifolio-server`.

# Requirements

CLI:
* `fifolio-server` starts the server at port 8000. Optional `--port` argument.
* `fifolio-server openapi` prints the server's OpenAPI spec (does not start the server).

API:
* `GET /openapi.json` returns the server's OpenAPI spec.
* CRUDL endpoints for accounts, securities
* Transaction related enpoints:
    * Read, list transactions
    * List unattributed sell transactions (can be a filter on the regular list transactions endpoint)
    * Import transactions
* Attribution related endpoints:
    * Create an attribution (i.e. attribute a sell transaction to >= 1 buy transactions)
    * Read, delete, list attributions (no update)
    * Anything else that is necessary
* Reporting endpoints

Entity managment:
* An account cannot be deleted if it is referenced by any transaction
* A security cannot be deleted if it is referenced by any transaction

Transaction import:
* Supported file formats:
    * Saxo NL CSV
    * Trade Republic DE CSV
* Import must be idempotent. Detect by hash of source record.

Storage:
* Store in local SQLite database


# Constraints

Programming language: Rust
