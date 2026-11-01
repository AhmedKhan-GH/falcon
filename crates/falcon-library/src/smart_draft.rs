//! The smart folder dialog's editable form of `conditions`, kept free of
//! any UI so it can be tested on its own.
//!
//! [`drafts`] turns stored conditions into [`ConditionDraft`]s and
//! [`conditions`] turns them back. Whatever the user didn't change comes
//! back byte for byte: an untouched condition or rule is the stored JSON
//! itself, and an edited one is the stored object with only the changed
//! keys replaced, so key order and keys falcon doesn't know survive. A rule
//! the dialog can't show (an unknown property, or a method its property
//! doesn't have) is kept verbatim as [`RuleDraft::Unsupported`].

use chrono::{Local, NaiveDate, TimeZone};
use serde_json::{Map, Value, json};

use crate::search;

/// How a property's value is entered and stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A string.
    Text,
    /// `[a, b]`, `b` only used by `between`.
    Number,
    /// `[a, b]` with a `unit` of `kb` or `mb`.
    Size,
    /// `[a, b]` with a `unit` of `s`, `m` or `h`.
    Duration,
    /// `[ms]`, `[ms, ms]` for `between`, `[days]` for `within`.
    Date,
    /// A list of tag names.
    Tags,
    /// A list of folder ids.
    Folders,
    /// An extension or type group.
    Type,
    /// `"1"`..`"5"` or `"none"`; stars as text for `contain`.
    Rating,
    /// A shape name; `custom` adds `width` and `height`.
    Shape,
    /// `#RRGGBB`.
    Color,
    /// No value (`fontActivated`).
    Font,
}

/// A property the dialog offers.
#[derive(Debug, Clone, Copy)]
pub struct Property {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: Kind,
    /// Eagle's method names, the first being the default.
    pub methods: &'static [&'static str],
}

const TEXT: &[&str] = &["contain", "uncontain", "startWith", "endWith", "equal", "empty", "not-empty", "regex"];
const NUMBER: &[&str] = &[">", ">=", "=", "<", "<=", "between"];
const DATE: &[&str] = &["before", "after", "between", "on", "within"];
const SET: &[&str] = &["intersection", "union", "equal", "identity", "empty", "not-empty"];
const IS: &[&str] = &["equal", "unequal"];

const fn p(key: &'static str, label: &'static str, kind: Kind, methods: &'static [&'static str]) -> Property {
    Property { key, label, kind, methods }
}

/// Every property the evaluator knows, in the dialog's order.
pub const PROPERTIES: &[Property] = &[
    p("name", "Name", Kind::Text, TEXT),
    p("type", "Type", Kind::Type, IS),
    p("tags", "Tags", Kind::Tags, SET),
    p("folders", "Folders", Kind::Folders, SET),
    p("folderName", "Folder name", Kind::Text, TEXT),
    p("color", "Colour", Kind::Color, &["similar", "accuracy", "grayscale"]),
    p("createTime", "Date added", Kind::Date, DATE),
    p("mtime", "Date modified", Kind::Date, DATE),
    p("btime", "Date created", Kind::Date, DATE),
    p("url", "URL", Kind::Text, TEXT),
    p("annotation", "Notes", Kind::Text, TEXT),
    p("width", "Width", Kind::Number, NUMBER),
    p("height", "Height", Kind::Number, NUMBER),
    p("fileSize", "File size", Kind::Size, NUMBER),
    p("shape", "Shape", Kind::Shape, IS),
    p("rating", "Rating", Kind::Rating, &["equal", "unequal", "contain"]),
    p("comments", "Comments", Kind::Text, TEXT),
    p("duration", "Duration", Kind::Duration, NUMBER),
    p("bpm", "BPM", Kind::Number, NUMBER),
    p("fontActivated", "Font", Kind::Font, &["activate", "deactivate"]),
    p("camera", "Camera", Kind::Text, TEXT),
    p("iso", "ISO", Kind::Number, NUMBER),
    p("aperture", "Aperture (f/)", Kind::Number, NUMBER),
    p("focalLength", "Focal length (mm)", Kind::Number, NUMBER),
    p("shutter", "Shutter (1/x s)", Kind::Number, NUMBER),
    p("timestamp", "Date taken", Kind::Date, DATE),
];

/// Shape values, with falcon's labels.
pub const SHAPES: &[(&str, &str)] = &[
    ("landscape", "Landscape"),
    ("portrait", "Portrait"),
    ("square", "Square"),
    ("panoramic-landscape", "Wide landscape"),
    ("panoramic-portrait", "Tall portrait"),
    ("custom", "Ratio W:H"),
];

/// Rating values for `equal` and `unequal`.
pub const RATINGS: &[(&str, &str)] = &[("none", "Unrated"), ("1", "1 star"), ("2", "2 stars"), ("3", "3 stars"), ("4", "4 stars"), ("5", "5 stars")];

pub fn property(key: &str) -> Option<&'static Property> {
    PROPERTIES.iter().find(|p| p.key == key)
}

/// Units of a property with one, the first being the default when a rule
/// has none (what Eagle's evaluator assumes).
pub fn units(kind: Kind) -> &'static [&'static str] {
    match kind {
        Kind::Size => &["mb", "kb"],
        Kind::Duration => &["s", "m", "h"],
        _ => &[],
    }
}

/// Falcon's wording for a method of a property kind.
pub fn method_label(kind: Kind, method: &str) -> &'static str {
    match (kind, method) {
        (_, "contain") if kind == Kind::Rating => "is one of",
        (_, "contain") => "contains",
        (_, "uncontain") => "doesn't contain",
        (_, "startWith") => "starts with",
        (_, "endWith") => "ends with",
        (Kind::Tags | Kind::Folders, "equal") => "are exactly",
        (_, "equal") => "is",
        (_, "unequal") => "is not",
        (_, "empty") => "is empty",
        (_, "not-empty") => "isn't empty",
        (_, "regex") => "matches regex",
        (_, ">") => ">",
        (_, ">=") => "≥",
        (_, "=") => "=",
        (_, "<") => "<",
        (_, "<=") => "≤",
        (_, "between") => "between",
        (_, "before") => "before",
        (_, "after") => "after",
        (_, "on") => "on",
        (_, "within") => "in the last",
        (_, "intersection") => "include all of",
        (_, "union") => "include any of",
        (_, "identity") => "include none of",
        (_, "similar") => "similar to",
        (_, "accuracy") => "very close to",
        (_, "grayscale") => "is greyscale",
        (_, "activate") => "is installed",
        (_, "deactivate") => "isn't installed",
        _ => "?",
    }
}

/// Whether a method takes no value.
pub fn no_value(kind: Kind, method: &str) -> bool {
    kind == Kind::Font || matches!(method, "empty" | "not-empty" | "grayscale")
}

/// A rule's editable fields, all as text.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleFields {
    pub property: &'static str,
    pub method: &'static str,
    /// The value, or the first number, date (`YYYY-MM-DD`, local) or day
    /// count. Lists are comma-separated.
    pub value: String,
    /// The second number or date, for `between`.
    pub value2: String,
    /// `""` for a property without units.
    pub unit: &'static str,
    /// The custom shape's ratio.
    pub width: String,
    pub height: String,
}

impl Default for RuleFields {
    /// Eagle's new rule: name contains nothing yet.
    fn default() -> Self {
        RuleFields::new("name")
    }
}

impl RuleFields {
    /// A fresh rule on `property` with its default method and value.
    pub fn new(property: &'static str) -> RuleFields {
        let prop = self::property(property).unwrap_or(&PROPERTIES[0]);
        let value = match prop.kind {
            Kind::Shape => "landscape",
            Kind::Rating => "5",
            Kind::Color => "#0087EF",
            _ => "",
        };
        RuleFields {
            property: prop.key,
            method: prop.methods[0],
            value: value.into(),
            value2: String::new(),
            unit: units(prop.kind).first().copied().unwrap_or(""),
            width: String::new(),
            height: String::new(),
        }
    }

    pub fn kind(&self) -> Kind {
        property(self.property).map_or(Kind::Text, |p| p.kind)
    }

    /// Switches to another property, resetting the method and value as
    /// Eagle's dialog does.
    pub fn set_property(&mut self, property: &'static str) {
        if self.property != property {
            *self = RuleFields::new(property);
        }
    }

    /// Switches the method. A date rule moving to or from `within` swaps a
    /// date for a day count, so its value starts over (30 days, as in
    /// Eagle); other values carry over.
    pub fn set_method(&mut self, method: &'static str) {
        if self.kind() == Kind::Date && (method == "within") != (self.method == "within") {
            self.value = if method == "within" { "30".into() } else { String::new() };
            self.value2.clear();
        }
        self.method = method;
    }

    /// Reads a stored rule, or `None` when the dialog can't show it.
    fn from_json(r: &Value) -> Option<RuleFields> {
        let prop = property(r.get("property")?.as_str()?)?;
        let m = r.get("method")?.as_str()?;
        let method = *prop.methods.iter().find(|x| **x == m)?;
        let v = r.get("value").unwrap_or(&Value::Null);
        let item = |i: usize| v.as_array().and_then(|a| a.get(i));
        let num_at = |i: usize| item(i).map(num_text);
        let text = |v: &Value| match v {
            Value::String(s) => Some(s.clone()),
            Value::Null => Some(String::new()),
            _ => None,
        };
        let (mut value, mut value2) = (String::new(), String::new());
        match prop.kind {
            Kind::Number | Kind::Size | Kind::Duration => {
                if !v.is_array() {
                    return None;
                }
                value = num_at(0).unwrap_or_default();
                value2 = num_at(1).unwrap_or_default();
            }
            Kind::Date => match v {
                Value::Array(_) if method == "within" => value = num_at(0).unwrap_or_default(),
                Value::Array(_) => {
                    value = item(0).map(date_text).unwrap_or_default();
                    value2 = item(1).map(date_text).unwrap_or_default();
                }
                // Eagle's dialog leaves "" while a range is being picked.
                Value::String(s) if s.is_empty() => {}
                Value::Null => {}
                _ => return None,
            },
            Kind::Tags | Kind::Folders => match v {
                Value::Array(a) => {
                    let names: Option<Vec<&str>> = a.iter().map(Value::as_str).collect();
                    value = names?.join(", ");
                }
                Value::Null => {}
                _ => return None,
            },
            Kind::Rating if method == "contain" && v.is_array() => {
                let stars: Option<Vec<String>> = v.as_array()?.iter().map(|x| x.as_str().map(str::to_string).or_else(|| x.as_u64().map(|n| n.to_string()))).collect();
                value = stars?.concat();
            }
            Kind::Rating => value = text(v).or_else(|| v.as_u64().map(|n| n.to_string()))?,
            Kind::Font => {}
            Kind::Text | Kind::Type | Kind::Shape | Kind::Color => value = text(v)?,
        }
        let unit = match r.get("unit").and_then(Value::as_str) {
            Some(u) => units(prop.kind).iter().find(|x| **x == u).copied(),
            None => units(prop.kind).first().copied(),
        };
        // A unit falcon can't show (Eagle reads it as MB or seconds) would be
        // rewritten on edit, so keep such a rule as it is.
        let unit = if units(prop.kind).is_empty() { "" } else { unit? };
        let dim = |k: &str| r.get(k).map(num_text).unwrap_or_default();
        Some(RuleFields { property: prop.key, method, value, value2, unit, width: dim("width"), height: dim("height") })
    }

    /// The rule's `value` in Eagle's form.
    fn value_json(&self) -> Result<Value, String> {
        let label = property(self.property).map_or(self.property, |p| p.label);
        let number = |s: &str| -> Result<Value, String> { parse_number(s).map(js_number).ok_or_else(|| format!("{label} needs a number")) };
        let date = |s: &str| -> Result<Value, String> { parse_date(s).map(|ms| json!(ms)).ok_or_else(|| format!("{label} needs a date as YYYY-MM-DD")) };
        let list = || json!(self.value.split(',').map(str::trim).filter(|x| !x.is_empty()).collect::<Vec<_>>());
        Ok(match self.kind() {
            Kind::Number | Kind::Size | Kind::Duration => {
                let b = if self.method == "between" || !self.value2.trim().is_empty() { number(&self.value2)? } else { json!(0) };
                json!([number(&self.value)?, b])
            }
            Kind::Date => match self.method {
                "within" => json!([number(&self.value)?]),
                "between" => json!([date(&self.value)?, date(&self.value2)?]),
                _ => json!([date(&self.value)?]),
            },
            Kind::Tags | Kind::Folders => list(),
            Kind::Type => json!(self.value.trim()),
            Kind::Color if self.method != "grayscale" && !is_hex(self.value.trim()) => return Err("Colour needs a value like #FF0000".into()),
            Kind::Color => json!(self.value.trim()),
            Kind::Text | Kind::Rating | Kind::Shape | Kind::Font => json!(self.value),
        })
    }

    /// Which reading of the value text a method uses, so a method change that
    /// alters it rebuilds the value.
    fn form(&self) -> u8 {
        match (self.kind(), self.method) {
            (Kind::Date, "within") => 1,
            (Kind::Date, "between") => 2,
            (Kind::Rating, "contain") => 1,
            _ => 0,
        }
    }

    /// Writes the fields into `rule`, touching only what differs from
    /// `before` (all of it for a new rule).
    fn write(&self, rule: &mut Map<String, Value>, before: Option<&RuleFields>) -> Result<(), String> {
        let same_prop = before.is_some_and(|b| b.property == self.property);
        set(rule, "property", json!(self.property), before.is_none_or(|b| b.property != self.property));
        set(rule, "method", json!(self.method), before.is_none_or(|b| b.method != self.method));
        let value_changed = !same_prop || before.is_some_and(|b| b.value != self.value || b.value2 != self.value2 || b.form() != self.form());
        if self.kind() == Kind::Font {
            // No value; a new rule carries Eagle's empty one.
            if !rule.contains_key("value") {
                rule.insert("value".into(), json!(""));
            }
        } else if value_changed || before.is_none() {
            rule.insert("value".into(), self.value_json()?);
        }
        if !self.unit.is_empty() {
            set(rule, "unit", json!(self.unit), !same_prop || before.is_some_and(|b| b.unit != self.unit));
        }
        if self.kind() == Kind::Shape && self.value == "custom" {
            let ratio = |s: &str| parse_number(s).filter(|n| *n > 0.0).map(js_number).ok_or_else(|| "Ratio W:H needs two numbers above 0".to_string());
            let (w, h) = (ratio(&self.width)?, ratio(&self.height)?);
            let moved = !same_prop || before.is_some_and(|b| b.width != self.width || b.height != self.height || b.value != self.value);
            set(rule, "width", w, moved);
            set(rule, "height", h, moved);
        }
        Ok(())
    }
}

/// Sets `key` when `when` holds or the key is missing; an existing key keeps
/// its place.
fn set(m: &mut Map<String, Value>, key: &str, v: Value, when: bool) {
    if when || !m.contains_key(key) {
        m.insert(key.into(), v);
    }
}

/// One rule row in the dialog.
#[derive(Debug, Clone, PartialEq)]
pub enum RuleDraft {
    /// A rule the dialog edits. `stored` holds the rule as read, with its
    /// fields then, for writing back only what changed.
    Edit { fields: RuleFields, stored: Option<Box<(RuleFields, Value)>> },
    /// A rule the dialog can't show, kept exactly as stored.
    Unsupported(Value),
}

impl Default for RuleDraft {
    fn default() -> Self {
        RuleDraft::Edit { fields: RuleFields::default(), stored: None }
    }
}

impl RuleDraft {
    pub fn from_json(r: &Value) -> RuleDraft {
        match RuleFields::from_json(r) {
            Some(f) => RuleDraft::Edit { fields: f.clone(), stored: Some(Box::new((f, r.clone()))) },
            None => RuleDraft::Unsupported(r.clone()),
        }
    }

    pub fn to_json(&self) -> Result<Value, String> {
        match self {
            RuleDraft::Unsupported(v) => Ok(v.clone()),
            RuleDraft::Edit { fields, stored: Some(s) } if s.0 == *fields => Ok(s.1.clone()),
            RuleDraft::Edit { fields, stored } => {
                let (mut rule, before) = match stored.as_deref() {
                    Some((b, Value::Object(m))) => (m.clone(), Some(b)),
                    _ => (Map::new(), None),
                };
                fields.write(&mut rule, before)?;
                Ok(Value::Object(rule))
            }
        }
    }
}

/// One condition in the dialog: its rules combined by `any` (Eagle's
/// `match: "OR"`, else `"AND"`), the result inverted by `negate` (Eagle's
/// `boolean: "FALSE"`).
#[derive(Debug, Clone, PartialEq)]
pub struct ConditionDraft {
    pub any: bool,
    pub negate: bool,
    pub rules: Vec<RuleDraft>,
    stored: Option<Value>,
}

impl Default for ConditionDraft {
    /// Eagle's new condition: any of one empty name rule, not inverted.
    fn default() -> Self {
        ConditionDraft { any: true, negate: false, rules: vec![RuleDraft::default()], stored: None }
    }
}

impl ConditionDraft {
    pub fn from_json(c: &Value) -> ConditionDraft {
        ConditionDraft {
            any: c.get("match").and_then(Value::as_str) == Some("OR"),
            negate: c.get("boolean").and_then(Value::as_str) == Some("FALSE"),
            rules: c.get("rules").and_then(Value::as_array).map(|r| r.iter().map(RuleDraft::from_json).collect()).unwrap_or_default(),
            stored: Some(c.clone()),
        }
    }

    /// The condition in Eagle's form. A new one is `{rules, match, boolean}`
    /// as Eagle's dialog builds it; a stored one changes only the keys that
    /// differ.
    pub fn to_json(&self) -> Result<Value, String> {
        let list: Vec<Value> = self.rules.iter().map(RuleDraft::to_json).collect::<Result<_, _>>()?;
        let mode = json!(if self.any { "OR" } else { "AND" });
        let boolean = json!(if self.negate { "FALSE" } else { "TRUE" });
        let Some(Value::Object(stored)) = &self.stored else {
            return Ok(json!({"rules": list, "match": mode, "boolean": boolean}));
        };
        let mut c = stored.clone();
        let was = ConditionDraft::from_json(&self.stored.clone().unwrap_or_default());
        // A missing or unreadable `rules` stays as it is while still empty.
        let same = match c.get("rules") {
            Some(Value::Array(a)) => *a == list,
            _ => list.is_empty(),
        };
        if !same {
            c.insert("rules".into(), Value::Array(list));
        }
        if was.any != self.any {
            c.insert("match".into(), mode);
        }
        if was.negate != self.negate {
            c.insert("boolean".into(), boolean);
        }
        Ok(Value::Object(c))
    }
}

/// The dialog's form of a smart folder's `conditions`.
pub fn drafts(conditions: &[Value]) -> Vec<ConditionDraft> {
    conditions.iter().map(ConditionDraft::from_json).collect()
}

/// `conditions` back from the dialog, or the first value that can't be
/// stored (as a message for the user).
pub fn conditions(drafts: &[ConditionDraft]) -> Result<Vec<Value>, String> {
    drafts.iter().map(ConditionDraft::to_json).collect()
}

/// A number as JavaScript would write it: whole numbers without `.0`.
fn js_number(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 { json!(n as i64) } else { json!(n) }
}

fn num_text(v: &Value) -> String {
    match v {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}

fn parse_number(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok().filter(|n| n.is_finite())
}

fn is_hex(s: &str) -> bool {
    s.strip_prefix('#').is_some_and(|h| h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// A stored ms timestamp as a local `YYYY-MM-DD`.
fn date_text(v: &Value) -> String {
    v.as_f64()
        .and_then(|ms| Local.timestamp_millis_opt(ms as i64).single())
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

/// A local `YYYY-MM-DD` as the ms of its midnight, as Eagle's date picker
/// stores a day.
fn parse_date(s: &str) -> Option<u64> {
    let d = NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()?;
    use chrono::Datelike;
    search::local_day_start(d.year(), d.month(), d.day())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(conditions: &Value) -> Value {
        let list = conditions.as_array().unwrap();
        Value::Array(super::conditions(&drafts(list)).unwrap())
    }

    #[test]
    fn untouched_conditions_come_back_verbatim() {
        let c = json!([
            {"match": "AND", "rules": [{"property": "name", "method": "contain", "value": "x", "extra": 1}]},
            {"boolean": "FALSE", "match": "OR", "note": "kept", "rules": [
                {"property": "mystery", "method": "equal", "value": {"a": 1}},
                {"property": "fileSize", "method": ">", "value": [1.50, 0], "unit": "kb"},
                {"property": "width", "method": "teleport", "value": [1, 2]},
                {"property": "createTime", "method": "between", "value": [1_700_000_123_456_u64, 1_700_100_000_000_u64]}
            ]},
            {"rules": []},
            {"match": "OR"}
        ]);
        assert_eq!(round_trip(&c), c);
        let d = drafts(c.as_array().unwrap());
        assert!(!d[0].any && d[1].any && d[1].negate && !d[0].negate);
        assert!(matches!(d[1].rules[0], RuleDraft::Unsupported(_)), "unknown property");
        assert!(matches!(d[1].rules[2], RuleDraft::Unsupported(_)), "unknown method");
        assert!(matches!(&d[1].rules[1], RuleDraft::Edit { fields, .. } if fields.unit == "kb" && fields.value == "1.5"));
    }

    #[test]
    fn edits_touch_only_what_changed() {
        let c = json!([{"note": 1, "match": "AND", "rules": [
            {"x": true, "property": "createTime", "method": "on", "value": [1_700_000_123_456_u64]},
            {"property": "width", "method": ">", "value": [480, 7]}
        ]}]);
        let mut d = drafts(c.as_array().unwrap());
        d[0].negate = true;
        let RuleDraft::Edit { fields, .. } = &mut d[0].rules[0] else { panic!() };
        fields.method = "before"; // same date text: the stored ms stays
        let RuleDraft::Edit { fields, .. } = &mut d[0].rules[1] else { panic!() };
        fields.value = "640".into();
        let out = Value::Array(conditions(&d).unwrap());
        assert_eq!(
            out,
            json!([{"note": 1, "match": "AND", "rules": [
                {"x": true, "property": "createTime", "method": "before", "value": [1_700_000_123_456_u64]},
                {"property": "width", "method": ">", "value": [640, 7]}
            ], "boolean": "FALSE"}])
        );
        // Switching on → within reads the text as days, so it must be redone.
        let mut d = drafts(c.as_array().unwrap());
        let RuleDraft::Edit { fields, .. } = &mut d[0].rules[0] else { panic!() };
        fields.method = "within";
        assert!(conditions(&d).is_err());
        let RuleDraft::Edit { fields, .. } = &mut d[0].rules[0] else { panic!() };
        fields.value = "7".into();
        assert_eq!(conditions(&d).unwrap()[0]["rules"][0]["value"], json!([7]));
        // The dialog's method switch starts the day count at 30.
        let mut d = drafts(c.as_array().unwrap());
        let RuleDraft::Edit { fields, .. } = &mut d[0].rules[0] else { panic!() };
        fields.set_method("within");
        assert_eq!(conditions(&d).unwrap()[0]["rules"][0]["value"], json!([30]));
        let RuleDraft::Edit { fields, .. } = &mut d[0].rules[0] else { panic!() };
        fields.set_method("after");
        assert!(conditions(&d).is_err(), "back to a date, which must be entered");
    }

    #[test]
    fn new_conditions_and_rules_take_eagles_shape() {
        let mut c = ConditionDraft { any: false, ..Default::default() };
        c.rules.push(RuleDraft::Edit { fields: RuleFields { value: "2".into(), ..RuleFields::new("fileSize") }, stored: None });
        c.rules.push(RuleDraft::Edit { fields: RuleFields { value: "1.5".into(), unit: "m", ..RuleFields::new("duration") }, stored: None });
        c.rules.push(RuleDraft::Edit { fields: RuleFields { value: "custom".into(), width: "16".into(), height: "9".into(), ..RuleFields::new("shape") }, stored: None });
        c.rules.push(RuleDraft::Edit { fields: RuleFields::new("fontActivated"), stored: None });
        c.rules.push(RuleDraft::Edit { fields: RuleFields { value: "a, b ,".into(), ..RuleFields::new("tags") }, stored: None });
        c.rules.push(RuleDraft::Edit { fields: RuleFields { method: "between", value: "2024-03-10".into(), value2: "2024-03-12".into(), ..RuleFields::new("timestamp") }, stored: None });
        let text = serde_json::to_string(&c.to_json().unwrap()).unwrap();
        let day = |d: u32| search::local_day_start(2024, 3, d).unwrap();
        assert_eq!(
            text,
            format!(
                concat!(
                    r#"{{"rules":[{{"property":"name","method":"contain","value":""}},"#,
                    r#"{{"property":"fileSize","method":">","value":[2,0],"unit":"mb"}},"#,
                    r#"{{"property":"duration","method":">","value":[1.5,0],"unit":"m"}},"#,
                    r#"{{"property":"shape","method":"equal","value":"custom","width":16,"height":9}},"#,
                    r#"{{"property":"fontActivated","method":"activate","value":""}},"#,
                    r#"{{"property":"tags","method":"intersection","value":["a","b"]}},"#,
                    r#"{{"property":"timestamp","method":"between","value":[{},{}]}}],"match":"AND","boolean":"TRUE"}}"#
                ),
                day(10),
                day(12)
            )
        );
    }

    /// A smart folder made in Eagle, with two conditions, a NOT flag, a rule
    /// falcon can't show and keys it doesn't know, survives the dialog and
    /// `update_smart_folder` byte for byte apart from what was edited.
    #[test]
    fn eagle_smart_folder_survives_an_edit() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = crate::Library::create(tmp.path().join("S.library")).unwrap();
        let stored = json!([
            {"rules": [
                {"property": "name", "method": "regex", "value": "^ref", "$$extra": "x"},
                {"property": "someFutureProperty", "method": "near", "value": [1, {"deep": true}]}
            ], "match": "OR", "boolean": "TRUE"},
            {"rules": [
                {"property": "fileSize", "method": "between", "value": [0.5, 2.25], "unit": "kb"},
                {"property": "tags", "method": "union", "value": ["draft", "wip"]}
            ], "match": "AND", "boolean": "FALSE", "future": 3}
        ]);
        let id = lib.create_smart_folder("Refs", stored.as_array().unwrap().clone()).unwrap();
        let file = lib.root().join("metadata.json");
        // Everything but the modification times, which move on every save.
        let strip = |s: &str| {
            let mut v: Value = serde_json::from_str(s).unwrap();
            v.as_object_mut().unwrap().shift_remove("modificationTime");
            for f in v["smartFolders"].as_array_mut().unwrap() {
                f.as_object_mut().unwrap().shift_remove("modificationTime");
            }
            serde_json::to_string(&v).unwrap()
        };
        let before = std::fs::read_to_string(&file).unwrap();
        let load = |lib: &crate::Library| drafts(&lib.smart_folders()[0].conditions);

        // Saving untouched writes the same bytes.
        lib.update_smart_folder(&id, "Refs", conditions(&load(&lib)).unwrap()).unwrap();
        assert_eq!(strip(&std::fs::read_to_string(&file).unwrap()), strip(&before));

        // Editing one tag list and the NOT flag changes only those.
        let mut d = load(&lib);
        assert!(matches!(d[0].rules[1], RuleDraft::Unsupported(_)) && d[1].negate && !d[0].negate);
        d[1].negate = false;
        let RuleDraft::Edit { fields, .. } = &mut d[1].rules[1] else { panic!() };
        fields.value.push_str(", todo");
        lib.update_smart_folder(&id, "Refs", conditions(&d).unwrap()).unwrap();
        let want = before.replace(r#"["draft","wip"]"#, r#"["draft","wip","todo"]"#).replace(r#""boolean":"FALSE""#, r#""boolean":"TRUE""#);
        assert_ne!(want, before);
        assert_eq!(strip(&std::fs::read_to_string(&file).unwrap()), strip(&want));

        // Deleting the unsupported rule is the only way it goes.
        let mut d = load(&lib);
        d[0].rules.remove(1);
        lib.update_smart_folder(&id, "Refs", conditions(&d).unwrap()).unwrap();
        assert_eq!(lib.smart_folders()[0].conditions[0]["rules"], json!([{"property": "name", "method": "regex", "value": "^ref", "$$extra": "x"}]));
    }

    #[test]
    fn bad_values_are_refused() {
        let bad = |f: RuleFields| RuleDraft::Edit { fields: f, stored: None }.to_json().is_err();
        assert!(bad(RuleFields { value: "abc".into(), ..RuleFields::new("width") }));
        assert!(bad(RuleFields { method: "between", value: "1".into(), ..RuleFields::new("height") }));
        assert!(bad(RuleFields { value: "red".into(), ..RuleFields::new("color") }));
        assert!(bad(RuleFields { value: "10/3/2024".into(), ..RuleFields::new("btime") }));
        assert!(bad(RuleFields { value: "custom".into(), width: "0".into(), height: "1".into(), ..RuleFields::new("shape") }));
        assert!(!bad(RuleFields::new("name")), "an empty text value is stored, as in Eagle");
        assert!(!bad(RuleFields { method: "grayscale", value: String::new(), ..RuleFields::new("color") }));
    }

    #[test]
    fn every_property_reads_back_its_own_output() {
        for p in PROPERTIES {
            for m in p.methods {
                let mut f = RuleFields { method: m, ..RuleFields::new(p.key) };
                match (p.kind, *m) {
                    (Kind::Number | Kind::Size | Kind::Duration, _) => (f.value, f.value2) = ("3".into(), "4".into()),
                    (Kind::Date, "within") => f.value = "7".into(),
                    (Kind::Date, "between") => (f.value, f.value2) = ("2024-01-02".into(), "2024-01-05".into()),
                    (Kind::Date, _) => f.value = "2024-01-02".into(),
                    (Kind::Tags | Kind::Folders, _) => f.value = "a, b".into(),
                    _ => {}
                }
                let v = RuleDraft::Edit { fields: f.clone(), stored: None }.to_json().unwrap();
                let RuleDraft::Edit { fields: back, .. } = RuleDraft::from_json(&v) else { panic!("{v}") };
                assert_eq!(back, f, "{v}");
            }
        }
    }
}
