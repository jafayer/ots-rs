# ots-pdf-markup

MarkupPDF command parsing and serialization layer.

## Purpose

Provides structures and parsing helpers for PDF field/tag overlay instructions used by tax form output workflows.

## Current state

- Contains placeholder `MarkupCommand` and `parse_markup` API.

## Planned direction

- Parse legacy MarkupPDF command syntax into typed command structures.
- Support round-trip serialization.
- Provide mapping helpers for output values to PDF tags.