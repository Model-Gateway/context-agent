//! Minimal JSON-schema argument validation. Covers what the built-in tool
//! schemas use: object types, required properties, primitive types, enums,
//! arrays of objects, and `additionalProperties: false`.

use serde_json::Value;

pub fn validate(schema: &Value, args: &Value) -> Result<(), String> {
    validate_at(schema, args, "arguments")
}

fn type_matches(expected: &str, value: &Value) -> bool {
    match expected {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some() || value.as_f64().map(|f| f.fract() == 0.0).unwrap_or(false),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => true,
    }
}

fn validate_at(schema: &Value, value: &Value, path: &str) -> Result<(), String> {
    if let Some(t) = schema.get("type") {
        let types: Vec<&str> = match t {
            Value::String(s) => vec![s.as_str()],
            Value::Array(a) => a.iter().filter_map(|v| v.as_str()).collect(),
            _ => vec![],
        };
        if !types.is_empty() && !types.iter().any(|t| type_matches(t, value)) {
            return Err(format!("{path} must be of type {} (got {})", types.join(" or "), kind_name(value)));
        }
    }
    if let Some(allowed) = schema.get("enum").and_then(|e| e.as_array()) {
        if !allowed.contains(value) {
            return Err(format!("{path} must be one of {}", allowed.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", ")));
        }
    }
    if let Some(obj) = value.as_object() {
        let props = schema.get("properties").and_then(|p| p.as_object());
        if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
            for r in required.iter().filter_map(|r| r.as_str()) {
                if !obj.contains_key(r) || obj.get(r).map(|v| v.is_null()).unwrap_or(true) {
                    return Err(format!("{path}.{r} is required"));
                }
            }
        }
        if let Some(props) = props {
            for (k, v) in obj {
                match props.get(k) {
                    Some(sub) => validate_at(sub, v, &format!("{path}.{k}"))?,
                    None => {
                        if schema.get("additionalProperties").map(|a| a == &Value::Bool(false)).unwrap_or(false) {
                            return Err(format!("{path}.{k} is not a recognised parameter (allowed: {})", props.keys().cloned().collect::<Vec<_>>().join(", ")));
                        }
                    }
                }
            }
        }
    }
    if let Some(items) = value.as_array() {
        if let Some(item_schema) = schema.get("items") {
            for (i, item) in items.iter().enumerate() {
                validate_at(item_schema, item, &format!("{path}[{i}]"))?;
            }
        }
        if let Some(min) = schema.get("minItems").and_then(|m| m.as_u64()) {
            if (items.len() as u64) < min {
                return Err(format!("{path} must have at least {min} item(s)"));
            }
        }
    }
    if let Some(s) = value.as_str() {
        if let Some(min) = schema.get("minLength").and_then(|m| m.as_u64()) {
            if (s.chars().count() as u64) < min {
                return Err(format!("{path} must not be empty"));
            }
        }
    }
    if let Some(n) = value.as_f64() {
        if let Some(min) = schema.get("minimum").and_then(|m| m.as_f64()) {
            if n < min {
                return Err(format!("{path} must be >= {min}"));
            }
        }
        if let Some(max) = schema.get("maximum").and_then(|m| m.as_f64()) {
            if n > max {
                return Err(format!("{path} must be <= {max}"));
            }
        }
    }
    Ok(())
}

fn kind_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validates_required_types_enums_and_unknown_keys() {
        let schema = json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "minLength": 1},
                "mode": {"type": "string", "enum": ["a", "b"]},
                "edits": {"type": "array", "minItems": 1, "items": {"type": "object", "properties": {"old_text": {"type": "string"}}, "required": ["old_text"]}},
                "limit": {"type": "integer", "minimum": 1}
            },
            "required": ["path"],
            "additionalProperties": false
        });
        assert!(validate(&schema, &json!({"path": "a", "mode": "a", "edits": [{"old_text": "x"}], "limit": 3})).is_ok());
        assert!(validate(&schema, &json!({})).unwrap_err().contains("path is required"));
        assert!(validate(&schema, &json!({"path": "a", "mode": "zzz"})).unwrap_err().contains("one of"));
        assert!(validate(&schema, &json!({"path": "a", "bogus": 1})).unwrap_err().contains("not a recognised"));
        assert!(validate(&schema, &json!({"path": "a", "edits": [{}]})).unwrap_err().contains("old_text is required"));
        assert!(validate(&schema, &json!({"path": "a", "limit": 2.5})).unwrap_err().contains("integer"));
        assert!(validate(&schema, &json!({"path": "a", "limit": 0})).unwrap_err().contains(">= 1"));
    }
}
