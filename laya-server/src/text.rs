//! Python's text forms, where the model or a client sees them: `json.dumps(v, ensure_ascii=False)`
//! for structured instructions, criteria and states, and `repr()` for names in error messages.

use serde_json::Value;

/// `json.dumps(value, ensure_ascii=False)` with Python's default separators: ", " between
/// members and ": " before a value. Strings escape like serde_json, which matches Python for
/// quotes, backslashes and control characters and leaves non-ASCII as is.
pub fn python_json(value: &Value) -> String {
    match value {
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(python_json).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Object(members) => {
            let parts: Vec<String> = members
                .iter()
                .map(|(k, v)| format!("{}: {}", json_string(k), python_json(v)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
        Value::String(s) => json_string(s),
        other => other.to_string(),
    }
}

/// laya's `render_criterion`: a string as itself, anything else as its JSON text.
pub fn render_criterion(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => python_json(other),
    }
}

/// Python's `repr()` of a value as it appears in laya's `%r` error messages.
pub fn python_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::String(s) => repr_str(s),
        other => python_json(other),
    }
}

/// Python's `repr()` of a string: single quotes, unless it holds one and no double quote.
pub fn repr_str(s: &str) -> String {
    if s.contains('\'') && !s.contains('"') {
        return format!("\"{}\"", s.replace('\\', "\\\\"));
    }
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn json_string(s: &str) -> String {
    Value::String(s.to_string()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dumps_with_python_separators() {
        assert_eq!(
            python_json(&json!({"a": [1, "ø"], "b": null})),
            r#"{"a": [1, "ø"], "b": null}"#
        );
    }

    #[test]
    fn repr_quotes_like_python() {
        assert_eq!(repr_str("meets"), "'meets'");
        assert_eq!(repr_str("it's"), "\"it's\"");
        assert_eq!(python_repr(&json!(null)), "None");
    }
}
