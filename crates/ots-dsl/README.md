# ots-dsl

Parser and cursor utilities for OpenTaxSolver-style input files.

## Responsibilities

- Tokenize OTS DSL input with Pest grammar.
- Read typed values (numbers, booleans, text, semicolon lists).
- Support common legacy forms behavior (comments, pragmas, optional values).
- Convert parsed statements into generic `ots-core::FormState`.

## Key API

- `DslCursor::new`: tokenize input.
- `DslCursor::parse_form_state`: parse full file into `FormState`.
- `DslCursor` typed readers (`read_float`, `read_bool`, `read_line_text`, etc.).

## Output

This crate produces a neutral parsed representation that downstream execution and rendering layers consume.