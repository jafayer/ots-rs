# ots-forms

Form-specific hook registration and form resolution for the OTS engine.

## Add a new form

With the centralized registry, adding a new form is intentionally small and local.

1. Add a form implementation file under `src/<year>/`.
   - Example: `src/2025/us8863.rs`
   - Expose a function with this shape:
     - `pub fn register_hooks_from_constants_file<P: AsRef<Path>>(registry: &mut HookRegistry, path: P) -> Result<(), YourError>`
2. Add a module import in `src/lib.rs` using `#[path = "<year>/<file>.rs"]`.
3. Add one `form_registration!(...)` entry to `FORM_REGISTRATIONS` in `src/lib.rs`.

### Registration template

```rust
#[path = "2025/us8863.rs"]
mod us8863_2025;

const FORM_REGISTRATIONS: &[FormRegistration] = &[
    form_registration!(
        canonical: "US8863",
        year: 2025,
        rules_file: "data/2025/us8863.rules.toml",
        aliases: ["us8863", "8863"],
        register_hooks: us8863_2025::register_hooks_from_constants_file
    ),
    // existing forms...
];
```

## Notes

- Keep aliases normalized and simple (for example: `"US_8863"`, `"us-8863"`, and `"8863"` all resolve through normalization).
- `list_registered_forms()` is derived from `FORM_REGISTRATIONS`, so no separate update is needed.
- Unsupported form/year combinations return `FormRegistryError::UnsupportedForm`.
