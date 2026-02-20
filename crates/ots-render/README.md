# ots-render

Formatting helpers for canonical line-oriented output.

## Responsibilities

- Render parsed `FieldValue` values into stable text lines.
- Render computed numeric results in a normalized decimal format.
- Keep output deterministic for diffing and regression checks.

## Key API

- `render_line(label, value) -> String`
- `render_computed_line(label, value) -> String`

## Output format

Examples:
- `L1 = 123.00`
- `status = 2.00`
- `SomeField =` (for null/empty values)