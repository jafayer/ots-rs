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
