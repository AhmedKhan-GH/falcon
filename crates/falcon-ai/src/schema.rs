//! Checks model output against a JSON schema. A provider's own structured
//! output is not trusted: every reply is checked here. This covers the part
//! of JSON Schema falcon's features use: `type`, `properties`, `required`,
//! `additionalProperties: false`, `items`, `enum`, `minItems`, `maxItems`,
//! `minLength` and `maxLength`.

use serde_json::Value;

/// `Err` names the first place that doesn't fit, such as `$.tags[1]: expected string`.
pub fn validate(value: &Value, schema: &Value) -> Result<(), String> {
    check(value, schema, "$")
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn check(v: &Value, schema: &Value, at: &str) -> Result<(), String> {
    if let Some(want) = schema.get("type").and_then(Value::as_str) {
        let fits = match want {
            "integer" => v.is_i64() || v.is_u64(),
            other => type_name(v) == other,
        };
        if !fits {
            return Err(format!("{at}: expected {want}, got {}", type_name(v)));
        }
    }
    if let Some(allowed) = schema.get("enum").and_then(Value::as_array)
        && !allowed.contains(v)
    {
        return Err(format!("{at}: not an allowed value"));
    }
    let limit = |key: &str| schema.get(key).and_then(Value::as_u64);
    match v {
        Value::Object(map) => {
            let empty = serde_json::Map::new();
            let props = schema.get("properties").and_then(Value::as_object).unwrap_or(&empty);
            if let Some(required) = schema.get("required").and_then(Value::as_array) {
                for key in required.iter().filter_map(Value::as_str) {
                    if !map.contains_key(key) {
                        return Err(format!("{at}: missing {key}"));
                    }
                }
            }
            for (key, value) in map {
                match props.get(key) {
                    Some(sub) => check(value, sub, &format!("{at}.{key}"))?,
                    None if schema.get("additionalProperties") == Some(&Value::Bool(false)) => return Err(format!("{at}: unexpected field {key}")),
                    None => {}
                }
            }
        }
        Value::Array(list) => {
            if limit("minItems").is_some_and(|n| (list.len() as u64) < n) {
                return Err(format!("{at}: too few items"));
            }
            if limit("maxItems").is_some_and(|n| (list.len() as u64) > n) {
                return Err(format!("{at}: too many items"));
            }
            if let Some(sub) = schema.get("items") {
                for (i, value) in list.iter().enumerate() {
                    check(value, sub, &format!("{at}[{i}]"))?;
                }
            }
        }
        Value::String(s) => {
            let n = s.chars().count() as u64;
            if limit("minLength").is_some_and(|m| n < m) {
                return Err(format!("{at}: too short"));
            }
            if limit("maxLength").is_some_and(|m| n > m) {
                return Err(format!("{at}: too long"));
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tags() -> Value {
        json!({"type":"object","properties":{"tags":{"type":"array","items":{"type":"string","minLength":1},"maxItems":2}},"required":["tags"],"additionalProperties":false})
    }

    #[test]
    fn accepts_what_fits() {
        assert_eq!(validate(&json!({"tags":["a","b"]}), &tags()), Ok(()));
        assert_eq!(validate(&json!({"tags":[]}), &tags()), Ok(()));
    }

    #[test]
    fn names_what_does_not() {
        for (value, says) in [
            (json!([]), "$: expected object"),
            (json!({}), "$: missing tags"),
            (json!({"tags":"a"}), "$.tags: expected array"),
            (json!({"tags":["a",3]}), "$.tags[1]: expected string"),
            (json!({"tags":["a"],"run":"delete"}), "$: unexpected field run"),
            (json!({"tags":["a","b","c"]}), "$.tags: too many items"),
            (json!({"tags":[""]}), "$.tags[0]: too short"),
        ] {
            let e = validate(&value, &tags()).unwrap_err();
            assert!(e.starts_with(says), "{e}");
        }
    }

    #[test]
    fn enums_and_integers() {
        assert!(validate(&json!("a"), &json!({"enum":["a","b"]})).is_ok());
        assert!(validate(&json!("c"), &json!({"enum":["a","b"]})).is_err());
        assert!(validate(&json!(3), &json!({"type":"integer"})).is_ok());
        assert!(validate(&json!(3.5), &json!({"type":"integer"})).is_err());
    }
}
