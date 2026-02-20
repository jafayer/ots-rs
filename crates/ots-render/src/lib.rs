use ots_core::FieldValue;

pub fn render_line(label: &str, value: &FieldValue) -> String {
    match value {
        FieldValue::Null => format!("{} =", label),
        FieldValue::Number(v) => format!("{} = {:.2}", label, v),
        FieldValue::Integer(v) => format!("{} = {}", label, v),
        FieldValue::Text(v) => format!("{} = {}", label, v),
        FieldValue::Bool(v) => format!("{} = {}", label, if *v { "yes" } else { "no" }),
        FieldValue::List(values) => {
            let rendered = values
                .iter()
                .map(|entry| match entry {
                    FieldValue::Null => "n/a".to_string(),
                    FieldValue::Number(v) => format!("{v:.2}"),
                    FieldValue::Integer(v) => v.to_string(),
                    FieldValue::Text(v) => v.to_string(),
                    FieldValue::Bool(v) => {
                        if *v {
                            "yes".to_string()
                        } else {
                            "no".to_string()
                        }
                    }
                    FieldValue::List(_) => "[nested-list]".to_string(),
                })
                .collect::<Vec<_>>()
                .join(" ");
            format!("{} = {}", label, rendered)
        }
    }
}

pub fn render_computed_line(label: &str, value: f64) -> String {
    render_line(label, &FieldValue::Number(value))
}

/// Render a field line with an optional trailing annotation label (tab-separated).
pub fn render_annotated_line(field_id: &str, value: &FieldValue, annotation: Option<&str>) -> String {
    let base = render_line(field_id, value);
    match annotation {
        Some(ann) if !ann.is_empty() => format!("{}\t\t{}", base, ann),
        _ => base,
    }
}

/// Render a computed numeric field line with an optional trailing annotation label.
pub fn render_computed_annotated_line(field_id: &str, value: f64, annotation: Option<&str>) -> String {
    render_annotated_line(field_id, &FieldValue::Number(value), annotation)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_annotated_line_appends_label() {
        let result = render_annotated_line("L27", &FieldValue::Number(56413.30), Some("Total Income"));
        assert_eq!(result, "L27 = 56413.30\t\tTotal Income");
    }

    #[test]
    fn render_annotated_line_without_annotation_is_plain() {
        let result = render_annotated_line("L27", &FieldValue::Number(56413.30), None);
        assert_eq!(result, "L27 = 56413.30");
    }

    #[test]
    fn render_annotated_line_empty_annotation_is_plain() {
        let result = render_annotated_line("L27", &FieldValue::Number(56413.30), Some(""));
        assert_eq!(result, "L27 = 56413.30");
    }

    #[test]
    fn render_computed_annotated_line_with_label() {
        let result = render_computed_annotated_line("L43", 913.00, Some("TAX"));
        assert_eq!(result, "L43 = 913.00\t\tTAX");
    }
}
