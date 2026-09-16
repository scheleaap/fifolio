# Fifolio, FIFO security tracking

Design documents:

* `domain.md`: the problem, the entities, the FIFO rules, the invariants and the reports
* `architecture.md`: crate layout and cross-cutting conventions (numbers, storage, errors, testing)
* `server.md`: `fifolio-server`, the HTTP API
* `importers.md`: the Saxo NL and Trade Republic formats, field by field
* `cli.md`: `fifolio-cli`, the terminal application and the report commands
* `testing.md`: test layers, coverage thresholds, fixtures and properties

`example_exports/` holds the real broker exports the importer mappings were derived from.

## Requirement identifiers

Every testable statement carries a stable id in brackets, e.g. `[DOM-061]`. Prefixes: `DOM`, `ARC`, `SRV`, `CLI`, `IMP-SAXO`, `IMP-TR`, `TST`.

Ids are never reused or renumbered. A removed requirement leaves its number retired, so that a finding or a test naming it stays interpretable. New requirements append.

Tests name the requirements they cover, so that specification coverage is checkable. The convention and the agents that rely on it are described in `.claude/agents/README.md`.
