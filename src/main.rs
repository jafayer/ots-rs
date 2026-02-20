use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process;

use chrono::Datelike;
use ots_core::dag::{build_execution_dag, load_rule_schema_from_file};
use ots_core::dag_exec::{execute_dag, HookRegistry};
use ots_core::{build_initial_numeric_values, find_state_entry_normalized, round_field_value, FieldValue};
use ots_dsl::{detect_round_to_whole_dollars, DslCursor};
use ots_forms::{enrich_initial_values, list_registered_forms, register_form_hooks_from_constants_file, resolve_form};
use ots_render::{render_computed_line, render_line};

const USAGE: &str = "usage:\n  ots run <form-name> <file-path> [--year <year>]";

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let command = parse_command(env::args().skip(1))?;

    match command {
        Command::Run { form_name, file_path, year } => {
            let form = resolve_form(&form_name, year).ok_or_else(|| {
                format!(
                    "unsupported form/year: {form_name} ({year}). Registered forms: {}",
                    list_registered_forms().join(", ")
                )
            })?;

            let input = read_input(Some(&file_path))?;
            let rules_path = PathBuf::from(form.rules_file);
            render_computed_fields(&input, &rules_path, form.canonical_name, form.year)?;
            Ok(())
        }
    }
}

enum Command {
    Run {
        form_name: String,
        file_path: String,
        year: u16,
    },
}

fn parse_command(args: impl Iterator<Item = String>) -> Result<Command, Box<dyn std::error::Error>> {
    let mut args = args;
    let command = args.next().ok_or(USAGE)?;
    if command != "run" {
        return Err(USAGE.into());
    }

    let form_name = args.next().ok_or(USAGE)?;
    let file_path = args.next().ok_or(USAGE)?;

    let mut year = current_year();
    while let Some(arg) = args.next() {
        if arg == "--year" {
            let Some(value) = args.next() else {
                return Err("missing value for --year".into());
            };
            year = value.parse::<u16>()?;
        } else {
            return Err(format!("unknown argument: {arg}").into());
        }
    }

    Ok(Command::Run {
        form_name,
        file_path,
        year,
    })
}

fn current_year() -> u16 {
    (chrono::Utc::now().year() - 1) as u16
}

fn read_input(input_source: Option<&str>) -> Result<String, Box<dyn std::error::Error>> {
    let input = match input_source {
        Some("-") | None => {
            let mut buffer = String::new();
            io::stdin().read_to_string(&mut buffer)?;
            buffer
        }
        Some(path) => fs::read_to_string(path)?,
    };

    Ok(input)
}

fn render_computed_fields(
    input: &str,
    rules_path: &Path,
    form_name: &str,
    year: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let state = DslCursor::parse_form_state(input)?;
    let round_to_whole_dollars = detect_round_to_whole_dollars(&state);

    let schema = load_rule_schema_from_file(rules_path)?;
    let dag = build_execution_dag(&schema)?;

    let meta = schema
        .meta
        .as_ref()
        .ok_or("rules config missing [meta] section")?;
    let constants_file = meta
        .constants_file
        .as_deref()
        .ok_or("rules config missing meta.constants_file")?;

    let mut hook_registry = HookRegistry::new();
    hook_registry.register_schema_hooks(&schema);
    register_form_hooks_from_constants_file(&mut hook_registry, year, form_name, constants_file)?;

    let constants_toml_raw = fs::read_to_string(constants_file)?;
    let constants_toml: toml::Value = toml::from_str(&constants_toml_raw)?;

    let mut initial_values = build_initial_numeric_values(&schema, &state, &constants_toml)?;
    enrich_initial_values(form_name, year, input, &mut initial_values);
    let execution = execute_dag(&schema, &dag, initial_values, &hook_registry)?;

    for field in &schema.fields {
        if let Some(value) = execution.context.get_value(&field.id) {
            let rendered_value = if round_to_whole_dollars {
                value.round()
            } else {
                value
            };
            println!("{}", render_computed_line(&field.id, rendered_value));
            continue;
        }

        if let Some(entry) = find_state_entry_normalized(&state, &field.id) {
            let fallback_value = if round_to_whole_dollars {
                round_field_value(&entry.value)
            } else {
                entry.value.clone()
            };
            println!("{}", render_line(&field.id, &fallback_value));
            continue;
        }

        if let Some(default) = field.default {
            let default_value = if round_to_whole_dollars {
                FieldValue::Number(default.round())
            } else {
                FieldValue::Number(default)
            };
            println!("{}", render_line(&field.id, &default_value));
            continue;
        }

        println!("{}", render_line(&field.id, &FieldValue::Null));
    }

    Ok(())
}
