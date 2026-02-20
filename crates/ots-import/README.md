# ots-import

Prior-return import abstractions.

## Purpose

This crate is the integration point for loading data from previous returns and mapping it into current-year inputs.

## Current state

- Contains placeholder types/APIs (`ImportedValue`, `import_prior_return`).
- Intended to become the Rust equivalent of legacy OTS import definitions.

## Planned direction

- Add file readers/parsers for prior-return formats.
- Add mapping rules from prior-year fields to current schema fields.
- Integrate with CLI run flow as optional prefill stage.