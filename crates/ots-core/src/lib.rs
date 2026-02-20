pub mod dag;
pub mod dag_exec;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use toml::Value;

use crate::dag::RuleSchema;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ReturnContext {
    pub state: FormState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FieldValue {
    Null,
    Number(f64),
    Integer(i64),
    Text(String),
    Bool(bool),
    List(Vec<FieldValue>),
}

impl Default for FieldValue {
    fn default() -> Self {
        Self::Null
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormField {
    pub label: String,
    pub value: FieldValue,
    pub raw_tokens: Vec<String>,
    pub terminated_by_semicolon: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FormState {
    entries: Vec<FormField>,
}

impl FormState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, field: FormField) {
        self.entries.push(field);
    }

    pub fn entries(&self) -> &[FormField] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn get_first(&self, label: &str) -> Option<&FormField> {
        self.entries.iter().find(|entry| entry.label == label)
    }

    pub fn get_all<'a>(&'a self, label: &'a str) -> impl Iterator<Item = &'a FormField> {
        self.entries.iter().filter(move |entry| entry.label == label)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineRef(pub String);

pub fn round_currency(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

pub fn normalize_name(raw: &str) -> String {
    raw.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}

pub fn find_state_entry_normalized<'a>(state: &'a FormState, field_id: &str) -> Option<&'a FormField> {
    if let Some(entry) = state.get_first(field_id) {
        return Some(entry);
    }

    let normalized_field = normalize_name(field_id);
    state
        .entries()
        .iter()
        .find(|entry| normalize_name(&entry.label) == normalized_field)
}

pub fn field_value_to_bool(value: &FieldValue) -> bool {
    match value {
        FieldValue::Bool(v) => *v,
        FieldValue::Integer(v) => *v != 0,
        FieldValue::Number(v) => *v != 0.0,
        FieldValue::Text(text) => {
            let normalized = text.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "true" | "yes" | "y" | "1")
        }
        FieldValue::List(values) => values.first().map(field_value_to_bool).unwrap_or(false),
        FieldValue::Null => false,
    }
}

pub fn round_field_value(value: &FieldValue) -> FieldValue {
    match value {
        FieldValue::Number(v) => FieldValue::Number(v.round()),
        FieldValue::Integer(v) => FieldValue::Integer(*v),
        FieldValue::Text(text) => {
            if let Ok(parsed) = text.replace(',', "").parse::<f64>() {
                FieldValue::Number(parsed.round())
            } else {
                FieldValue::Text(text.clone())
            }
        }
        FieldValue::Bool(v) => FieldValue::Bool(*v),
        FieldValue::List(items) => FieldValue::List(items.iter().map(round_field_value).collect()),
        FieldValue::Null => FieldValue::Null,
    }
}

pub fn field_value_to_number(
    value: &FieldValue,
    enum_ref: Option<&str>,
    constants_toml: &Value,
) -> Result<f64, Box<dyn std::error::Error>> {
    match value {
        FieldValue::Null => Ok(0.0),
        FieldValue::Number(v) => Ok(*v),
        FieldValue::Integer(v) => Ok(*v as f64),
        FieldValue::Bool(v) => Ok(if *v { 1.0 } else { 0.0 }),
        FieldValue::Text(text) => {
            if let Some(reference) = enum_ref
                && let Some(mapped) = map_enum_value(reference, text, constants_toml)
            {
                return Ok(mapped);
            }

            let normalized = text.replace(',', "");
            normalized
                .parse::<f64>()
                .map_err(|_| format!("cannot convert '{text}' to numeric value").into())
        }
        FieldValue::List(items) => {
            let mut sum = 0.0;
            for item in items {
                sum += field_value_to_number(item, enum_ref, constants_toml)?;
            }
            Ok(sum)
        }
    }
}

pub fn build_initial_numeric_values(
    schema: &RuleSchema,
    state: &FormState,
    constants_toml: &Value,
) -> Result<BTreeMap<String, f64>, Box<dyn std::error::Error>> {
    let mut values = BTreeMap::new();

    seed_constants_from_toml(constants_toml, &mut values, "");

    for field in &schema.fields {
        let should_seed = field.source == "input" || field.source == "input_list";
        if !should_seed {
            continue;
        }

        if let Some(entry) = find_state_entry_normalized(state, &field.id) {
            let value = field_value_to_number(&entry.value, field.enum_ref.as_deref(), constants_toml)?;
            values.insert(field.id.clone(), value);
            continue;
        }

        if let Some(default) = field.default {
            values.insert(field.id.clone(), default);
        }
    }

    for entry in state.entries() {
        if values.contains_key(&entry.label) {
            continue;
        }

        if let Ok(value) = field_value_to_number(&entry.value, None, constants_toml) {
            values.insert(entry.label.clone(), value);
        }
    }

    Ok(values)
}

/// Recursively flatten a TOML value into dotted-path keys, inserting only
/// numeric leaf values.  Top-level keys use `prefix = ""` (no leading dot).
fn seed_constants_from_toml(value: &Value, out: &mut BTreeMap<String, f64>, prefix: &str) {
    match value {
        Value::Table(table) => {
            for (key, child) in table {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                seed_constants_from_toml(child, out, &path);
            }
        }
        Value::Float(v) => {
            out.entry(prefix.to_string()).or_insert(*v);
        }
        Value::Integer(v) => {
            out.entry(prefix.to_string()).or_insert(*v as f64);
        }
        _ => {}
    }
}

fn map_enum_value(enum_ref: &str, text: &str, constants_toml: &Value) -> Option<f64> {
    let enum_table = constants_toml.get(enum_ref)?.as_table()?;

    let needle = normalize_name(text);
    let input_tokens = tokenize_enum_value(text);
    for (key, value) in enum_table {
        let key_normalized = normalize_name(key);
        if key_normalized == needle
            || key_normalized.contains(&needle)
            || needle.contains(&key_normalized)
            || enum_tokens_match(&input_tokens, &key_normalized)
        {
            return value.as_float().or_else(|| value.as_integer().map(|v| v as f64));
        }
    }

    None
}

fn tokenize_enum_value(raw: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    if tokens.is_empty() {
        vec![normalize_name(raw)]
    } else {
        tokens
    }
}

fn enum_tokens_match(input_tokens: &[String], key_normalized: &str) -> bool {
    if input_tokens.is_empty() {
        return false;
    }

    input_tokens.iter().all(|token| {
        if token.len() <= 1 {
            return true;
        }
        key_normalized.contains(token)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_constants_loads_nested_numeric_values() {
        let toml_str = r#"
            [worksheet_f]
            medical_floor_rate = 0.02

            [exemptions]
            qualified_dependent_children_multiplier = 1500.0
            other_dependents_multiplier = 1500.0
        "#;
        let toml: toml::Value = toml::from_str(toml_str).unwrap();
        let mut values = std::collections::BTreeMap::new();
        seed_constants_from_toml(&toml, &mut values, "");

        assert_eq!(
            values.get("worksheet_f.medical_floor_rate").copied(),
            Some(0.02)
        );
        assert_eq!(
            values.get("exemptions.qualified_dependent_children_multiplier").copied(),
            Some(1500.0)
        );
    }

    #[test]
    fn seed_constants_does_not_overwrite_existing_values() {
        let toml_str = r#"
            [section]
            rate = 0.05
        "#;
        let toml: toml::Value = toml::from_str(toml_str).unwrap();
        let mut values = std::collections::BTreeMap::new();
        values.insert("section.rate".to_string(), 0.99);
        seed_constants_from_toml(&toml, &mut values, "");
        // Existing value must not be overwritten
        assert_eq!(values.get("section.rate").copied(), Some(0.99));
    }
}
