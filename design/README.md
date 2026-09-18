# Fifolio, FIFO security tracking

Design documents:

* `domain.md`: the problem, the entities, the FIFO rules, the invariants and the reports
* `architecture.md`: crate layout and cross-cutting conventions (numbers, storage, errors, testing)
* `server.md`: `fifolio-server`, the HTTP API
* `importers.md`: the Saxo NL and Trade Republic formats, field by field
* `cli.md`: `fifolio-cli`, the terminal application and the report commands
* `testing.md`: test layers, coverage thresholds, fixtures and properties
* `decisions.md`: resolved ambiguities — what was open, what was chosen, and why
* `open-questions.md`: what is still undecided, and which requirements each question blocks

`example_exports/` holds the real broker exports the importer mappings were derived from.

## Requirement identifiers

Every testable statement carries a stable id in brackets, of the form `[DOM-nnn]`. Prefixes: `DOM`, `ARC`, `SRV`, `CLI`, `IMP-SAXO`, `IMP-TR`, `TST`.

A bracketed id **declares** a requirement and appears exactly once. Prose elsewhere **cites** one
without brackets, as `` `IMP-TR-007` ``, so that a reference is never mistaken for a second
declaration.

Ids are never reused or renumbered. A removed requirement leaves its number retired, so that a finding or a test naming it stays interpretable. New requirements append.

Tests name the requirements they cover, so that specification coverage is checkable. The convention and the agents that rely on it are described in `.claude/agents/README.md`.
