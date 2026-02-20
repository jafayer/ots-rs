# ots-rs

Rust implementation of an OpenTaxSolver-style pipeline using:
- a DSL parser for legacy form inputs,
- declarative TOML rule schemas,
- a DAG execution runtime with pluggable hooks,
- and a stable text renderer.

## Build and run

Build the CLI binary named `ots`:

```bash
cargo build --release --bin ots
```

Run a return:

```bash
target/release/ots run US1040 test/example_us_1040.txt [--year 2025]
```

Default year behavior: if `--year` is omitted, the CLI uses `current year - 1`.

## Workspace layout

- `src/main.rs`: CLI orchestration (`run <form-name> <file-path> [--year]`).
- `crates/ots-core`: shared domain model, schema loading, DAG construction, and DAG execution runtime.
- `crates/ots-dsl`: parser/tokenizer and cursor helpers for OTS-style input files.
- `crates/ots-forms`: form/year registry and hook implementations (currently `US1040` for `2025`).
- `crates/ots-render`: output formatting for line/value emission.
- `crates/ots-import`: placeholder crate for prior-return imports.
- `crates/ots-pdf-markup`: placeholder crate for MarkupPDF parsing/serialization.
- `crates/ots-tax-tables`: placeholder crate for loading tax tables.

## Current execution pipeline

1. Parse DSL input into generic `FormState`.
2. Resolve form/year and load rules TOML.
3. Build execution DAG from schema.
4. Register schema hooks and form-specific runtime hooks.
5. Execute DAG to compute derived values.
6. Render all schema fields in canonical output format.

## Data and config

- `data/2025/us1040.rules.toml`: declarative fields/rules/hooks schema.
- `data/2025/us1040.toml`: constants consumed by form hooks.

## Status

- Implemented: end-to-end `US1040` 2025 run flow via `ots run`.
- In progress: additional forms/years and fuller feature parity with legacy OTS behavior.