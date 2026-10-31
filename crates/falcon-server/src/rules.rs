//! The smart-folder rule schema behind `/api/v2/smartFolder/getRules`, and
//! the strict check `smartFolder/create` and `update` run on `conditions`.
//! Error texts follow Eagle's so clients can match on them.

use serde_json::{Map, Value, json};

const STRING: &[&str] = &["equal", "startWith", "endWith", "contain", "uncontain", "empty", "not-empty", "regex"];
const NUMERIC: &[&str] = &["=", ">=", "<=", ">", "<", "between"];
const DATE: &[&str] = &["on", "before", "after", "between", "within"];
const SET: &[&str] = &["union", "intersection", "equal", "identity", "empty", "not-empty"];
const EMPTY_CHECKS: &[&str] = &["empty", "not-empty"];

#[derive(Clone, Copy)]
enum Kind {
    Text,
    Numbers,
    Strings,
    /// No value at all (`fontActivated`).
    Nothing,
}

#[derive(Clone, Copy)]
enum Extra {
    None,
    /// A unit that must be one of these.
    Unit(&'static [&'static str]),
    /// `width` and `height` numbers, needed when the value is `custom`.
    CustomSize,
}

struct Property {
    name: &'static str,
    category: &'static str,
    methods: &'static [&'static str],
    kind: Kind,
    no_value: &'static [&'static str],
    options: &'static [&'static str],
    extra: Extra,
}

const fn prop(name: &'static str, category: &'static str, methods: &'static [&'static str], kind: Kind, no_value: &'static [&'static str]) -> Property {
    Property { name, category, methods, kind, no_value, options: &[], extra: Extra::None }
}

const TYPES: &[&str] = &[
    "video", "audio", "font", "jpg", "png", "gif", "svg", "psd", "ai", "pdf", "bmp", "ico", "tif", "webp", "avif", "base", "raw", "riff", "office",
];
const SHAPES: &[&str] = &["landscape", "portrait", "square", "panoramic-landscape", "panoramic-portrait", "custom"];

const PROPERTIES: &[Property] = &[
    prop("name", "STRING", STRING, Kind::Text, EMPTY_CHECKS),
    prop("url", "STRING", STRING, Kind::Text, EMPTY_CHECKS),
    prop("annotation", "STRING", STRING, Kind::Text, EMPTY_CHECKS),
    prop("comments", "STRING", STRING, Kind::Text, EMPTY_CHECKS),
    prop("camera", "STRING", STRING, Kind::Text, EMPTY_CHECKS),
    prop("folderName", "STRING", STRING, Kind::Text, EMPTY_CHECKS),
    prop("width", "NUMERIC", NUMERIC, Kind::Numbers, &[]),
    prop("height", "NUMERIC", NUMERIC, Kind::Numbers, &[]),
    prop("iso", "NUMERIC", NUMERIC, Kind::Numbers, &[]),
    prop("aperture", "NUMERIC", NUMERIC, Kind::Numbers, &[]),
    prop("focalLength", "NUMERIC", NUMERIC, Kind::Numbers, &[]),
    prop("shutter", "NUMERIC", NUMERIC, Kind::Numbers, &[]),
    prop("bpm", "NUMERIC", NUMERIC, Kind::Numbers, &[]),
    Property { extra: Extra::Unit(&["kb", "mb"]), ..prop("fileSize", "NUMERIC_WITH_UNIT", NUMERIC, Kind::Numbers, &[]) },
    Property { extra: Extra::Unit(&["s", "m", "h"]), ..prop("duration", "NUMERIC_WITH_UNIT", NUMERIC, Kind::Numbers, &[]) },
    prop("createTime", "DATE", DATE, Kind::Numbers, &[]),
    prop("mtime", "DATE", DATE, Kind::Numbers, &[]),
    prop("btime", "DATE", DATE, Kind::Numbers, &[]),
    prop("timestamp", "DATE", DATE, Kind::Numbers, &[]),
    prop("tags", "SET", SET, Kind::Strings, EMPTY_CHECKS),
    prop("folders", "SET", SET, Kind::Strings, EMPTY_CHECKS),
    Property { options: TYPES, ..prop("type", "SPECIAL", &["equal", "unequal"], Kind::Text, &[]) },
    Property { options: &["none", "1", "2", "3", "4", "5"], ..prop("rating", "SPECIAL", &["equal", "unequal", "contain"], Kind::Text, &[]) },
    Property { options: SHAPES, extra: Extra::CustomSize, ..prop("shape", "SPECIAL", &["equal", "unequal"], Kind::Text, &[]) },
    prop("color", "SPECIAL", &["similar", "accuracy", "grayscale"], Kind::Text, &["grayscale"]),
    prop("fontActivated", "SPECIAL", &["activate", "deactivate"], Kind::Nothing, &[]),
];

/// The schema as `getRules` returns it.
pub fn schema() -> Value {
    let mut out = Map::new();
    for p in PROPERTIES {
        let value_type = match p.kind {
            Kind::Text => json!("string"),
            Kind::Numbers => json!("number_pair"),
            Kind::Strings => json!("string_array"),
            Kind::Nothing => Value::Null,
        };
        let mut m = Map::new();
        m.insert("category".into(), json!(p.category));
        m.insert("methods".into(), json!(p.methods));
        m.insert("valueType".into(), value_type);
        if !p.no_value.is_empty() {
            m.insert("noValueMethods".into(), json!(p.no_value));
        }
        if !p.options.is_empty() {
            m.insert("options".into(), json!(p.options));
        }
        match p.extra {
            Extra::None => {}
            Extra::Unit(units) => {
                m.insert("extraFields".into(), json!({"unit": {"options": units, "required": true}}));
            }
            Extra::CustomSize => {
                m.insert(
                    "extraFields".into(),
                    json!({"width": {"type": "number", "requiredWhen": "custom"}, "height": {"type": "number", "requiredWhen": "custom"}}),
                );
            }
        }
        out.insert(p.name.into(), Value::Object(m));
    }
    Value::Object(out)
}

/// JavaScript's `typeof` for a JSON value.
fn type_of(v: &Value) -> &'static str {
    match v {
        Value::String(_) => "string",
        Value::Number(_) => "number",
        Value::Bool(_) => "boolean",
        _ => "object",
    }
}

/// How JavaScript would print a value inside a message (`undefined` when
/// missing).
fn show(v: Option<&Value>) -> String {
    match v {
        None => "undefined".into(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) => "null".into(),
        Some(other) => other.to_string(),
    }
}

/// JavaScript truthiness.
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(_) => true,
    }
}

fn check_rule(rule: &Value) -> Result<(), String> {
    let get = |k: &str| rule.get(k).filter(|v| !v.is_null());
    let property = rule.get("property");
    let def = property.and_then(Value::as_str).and_then(|n| PROPERTIES.iter().find(|p| p.name == n));
    let Some(def) = def else {
        let names: Vec<&str> = PROPERTIES.iter().map(|p| p.name).collect();
        return Err(format!("invalid property: {}. Valid properties: {}", show(property), names.join(", ")));
    };
    let method = rule.get("method");
    if !method.and_then(Value::as_str).is_some_and(|m| def.methods.contains(&m)) {
        return Err(format!(
            "invalid method '{}' for property '{}'. Valid methods: {}",
            show(method),
            def.name,
            def.methods.join(", ")
        ));
    }
    let method = method.and_then(Value::as_str).unwrap_or_default();
    if def.no_value.contains(&method) || matches!(def.kind, Kind::Nothing) {
        return Ok(());
    }
    let Some(value) = get("value") else {
        return Err(format!("value is required for property '{}' with method '{method}'", def.name));
    };
    let name = def.name;
    match def.kind {
        Kind::Text => {
            if !value.is_string() {
                return Err(format!("value must be a string for property '{name}', got {}", type_of(value)));
            }
        }
        Kind::Numbers => match value {
            Value::Number(_) => {}
            Value::Array(list) => {
                if list.is_empty() {
                    return Err(format!("value array must have at least 1 element for property '{name}'"));
                }
                if method == "between" && list.len() < 2 {
                    return Err(format!("value must be [number, number] for property '{name}' with method 'between'"));
                }
                if let Some((i, v)) = list.iter().enumerate().find(|(_, v)| !v.is_number()) {
                    return Err(format!("value[{i}] must be a number for property '{name}', got {}", type_of(v)));
                }
            }
            other => return Err(format!("value must be [number, ...] for property '{name}', got {}", type_of(other))),
        },
        Kind::Strings => {
            let Value::Array(list) = value else {
                return Err(format!("value must be an array of strings for property '{name}', got {}", type_of(value)));
            };
            if let Some((i, v)) = list.iter().enumerate().find(|(_, v)| !v.is_string()) {
                return Err(format!("value[{i}] must be a string for property '{name}', got {}", type_of(v)));
            }
        }
        Kind::Nothing => {}
    }
    match def.extra {
        Extra::None => {}
        Extra::Unit(units) => {
            let Some(unit) = get("unit") else {
                return Err(format!("'unit' is required for property '{name}'. Valid values: {}", units.join(", ")));
            };
            if !unit.as_str().is_some_and(|u| units.contains(&u)) {
                return Err(format!("invalid 'unit' value '{}' for property '{name}'. Valid values: {}", show(Some(unit)), units.join(", ")));
            }
        }
        Extra::CustomSize => {
            if value.as_str() == Some("custom") {
                for field in ["width", "height"] {
                    if !rule.get(field).is_some_and(Value::is_number) {
                        return Err(format!("'{field}' (number) is required for property '{name}' when value is 'custom'"));
                    }
                }
            }
        }
    }
    Ok(())
}

/// Checks a whole `conditions` value, failing on the first problem.
pub fn check_conditions(conditions: &Value) -> Result<(), String> {
    let list = match conditions {
        Value::Array(l) if !l.is_empty() => l,
        _ => return Err("conditions must be a non-empty array".into()),
    };
    if list.len() > 30 {
        return Err("max 30 conditions".into());
    }
    for c in list {
        let rules = match c.get("rules") {
            Some(Value::Array(r)) if !r.is_empty() => r,
            _ => return Err("each condition must have non-empty rules array".into()),
        };
        if rules.len() > 30 {
            return Err("max 30 rules per condition".into());
        }
        if truthy(c.get("match")) && !matches!(c.get("match").and_then(Value::as_str), Some("AND" | "OR")) {
            return Err("match must be \"AND\" or \"OR\"".into());
        }
        if truthy(c.get("boolean")) && !matches!(c.get("boolean").and_then(Value::as_str), Some("TRUE" | "FALSE")) {
            return Err("boolean must be \"TRUE\" or \"FALSE\"".into());
        }
        for r in rules {
            check_rule(r)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(rule: Value) -> Result<(), String> {
        check_conditions(&json!([{"match": "AND", "rules": [rule]}]))
    }

    #[test]
    fn schema_lists_every_property_in_order() {
        let s = schema();
        let keys: Vec<&String> = s.as_object().unwrap().keys().collect();
        assert_eq!(keys.first().unwrap().as_str(), "name");
        assert_eq!(keys.last().unwrap().as_str(), "fontActivated");
        assert_eq!(keys.len(), 26);
        assert_eq!(s["fontActivated"]["valueType"], Value::Null);
        assert!(s["createTime"].get("noValueMethods").is_none());
        assert_eq!(s["fileSize"]["extraFields"]["unit"]["required"], true);
        assert_eq!(s["shape"]["extraFields"]["width"]["requiredWhen"], "custom");
        let first: Vec<&String> = s["rating"].as_object().unwrap().keys().collect();
        assert_eq!(first, ["category", "methods", "valueType", "options"]);
    }

    #[test]
    fn rules_are_checked_like_eagle() {
        assert!(one(json!({"property": "tags", "method": "union", "value": ["a"]})).is_ok());
        assert!(one(json!({"property": "tags", "method": "empty"})).is_ok());
        assert!(one(json!({"property": "fontActivated", "method": "activate"})).is_ok());
        assert!(one(json!({"property": "width", "method": ">", "value": 3})).is_ok());
        assert!(one(json!({"property": "nope", "method": "x"})).unwrap_err().starts_with("invalid property: nope. Valid properties: name, url,"));
        assert!(one(json!({"method": "x"})).unwrap_err().starts_with("invalid property: undefined."));
        assert_eq!(
            one(json!({"property": "type", "method": "x"})).unwrap_err(),
            "invalid method 'x' for property 'type'. Valid methods: equal, unequal"
        );
        assert_eq!(one(json!({"property": "name", "method": "contain"})).unwrap_err(), "value is required for property 'name' with method 'contain'");
        assert_eq!(one(json!({"property": "name", "method": "contain", "value": 1})).unwrap_err(), "value must be a string for property 'name', got number");
        assert_eq!(
            one(json!({"property": "width", "method": "between", "value": [1]})).unwrap_err(),
            "value must be [number, number] for property 'width' with method 'between'"
        );
        assert_eq!(one(json!({"property": "width", "method": "=", "value": [1, "2"]})).unwrap_err(), "value[1] must be a number for property 'width', got string");
        assert_eq!(one(json!({"property": "tags", "method": "union", "value": "a"})).unwrap_err(), "value must be an array of strings for property 'tags', got string");
        assert_eq!(
            one(json!({"property": "fileSize", "method": ">", "value": [1]})).unwrap_err(),
            "'unit' is required for property 'fileSize'. Valid values: kb, mb"
        );
        assert_eq!(
            one(json!({"property": "fileSize", "method": ">", "value": [1], "unit": "gb"})).unwrap_err(),
            "invalid 'unit' value 'gb' for property 'fileSize'. Valid values: kb, mb"
        );
        assert_eq!(
            one(json!({"property": "shape", "method": "equal", "value": "custom", "width": 2})).unwrap_err(),
            "'height' (number) is required for property 'shape' when value is 'custom'"
        );
    }

    #[test]
    fn conditions_are_checked_like_eagle() {
        assert_eq!(check_conditions(&json!([])).unwrap_err(), "conditions must be a non-empty array");
        assert_eq!(check_conditions(&json!({})).unwrap_err(), "conditions must be a non-empty array");
        assert_eq!(check_conditions(&json!([{"rules": []}])).unwrap_err(), "each condition must have non-empty rules array");
        let rule = json!({"property": "tags", "method": "empty"});
        assert_eq!(check_conditions(&json!([{"match": "XOR", "rules": [rule]}])).unwrap_err(), "match must be \"AND\" or \"OR\"");
        assert_eq!(check_conditions(&json!([{"boolean": "yes", "rules": [rule]}])).unwrap_err(), "boolean must be \"TRUE\" or \"FALSE\"");
        assert!(check_conditions(&json!([{"match": "", "rules": [rule]}])).is_ok());
        let many: Vec<Value> = (0..31).map(|_| json!({"rules": [rule]})).collect();
        assert_eq!(check_conditions(&Value::Array(many)).unwrap_err(), "max 30 conditions");
    }
}
