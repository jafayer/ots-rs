# ots-core

Core shared library for the tax engine.

## Responsibilities

- Generic domain model (`FieldValue`, `FormField`, `FormState`).
- Rule schema model and TOML loading.
- Execution DAG builder with dependency validation.
- DAG runtime executor (built-ins, branching, hook dispatch).

## Key modules

- `src/lib.rs`: base value/state types and helpers.
- `src/dag.rs`: schema structs + DAG build/topological ordering.
- `src/dag_exec.rs`: execution context, hook registry, and rule evaluation.

## Consumers

- `ots` CLI uses this crate to load schemas, build the DAG, execute rules, and collect computed outputs.
- `ots-forms` uses runtime hook types to register form-specific calculations.