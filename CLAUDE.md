# Domain

Read design/domain.md to understand the domain.

# General behavior

Make reasonable and sensible decisions

# Implementation

Prefer functional approaches over imperative
Prefer libraries over hand-rolling code

# Testing

Read design/testing.md before writing tests.

Every change ships with tests. Unit tests for logic, integration tests for storage and HTTP, end-to-end tests for the paths a user actually takes.
Write the failing test before fixing a bug.
Run the suite before claiming anything works. Report failures with their output; never describe untested code as working.
Never ignore or delete a test to make the suite pass.
Never reach the network in a test.
