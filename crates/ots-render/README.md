# ots-render

Formatting helpers for canonical line-oriented output.

## Responsibilities

- Render parsed `FieldValue` values into stable text lines.
- Render computed numeric results in a normalized decimal format.
- Keep output deterministic for diffing and regression checks.
- Support optional inline annotations (labels) on field lines, matching the OpenTaxSolver output format.

## Key API

- `render_line(label, value) -> String`
- `render_computed_line(label, value) -> String`
- `render_annotated_line(field_id, value, annotation) -> String`
- `render_computed_annotated_line(field_id, value, annotation) -> String`

## Output format

Examples:
- `L1 = 123.00`
- `status = 2.00`
- `SomeField =` (for null/empty values)
- `L27 = 56413.30		Total Income` (with annotation, using tab-separated label)
