//! Smart folders: saved rule sets in the library `metadata.json` (see the
//! Smart folders chapter of the docs). Entries stay raw JSON in [`LibraryMetadata`] so they
//! round-trip untouched; this module reads, evaluates and edits them.
//!
//! [`LibraryMetadata`]: crate::model::LibraryMetadata

use std::cell::{OnceCell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use chrono::{Local, NaiveDate, TimeZone};
use regex::{Regex, RegexBuilder};
use serde_json::{Map, Value, json};

use crate::edit::{display_name, now_ms};
use crate::error::Result;
use crate::ids;
use crate::library::Library;
use crate::model::{FolderOrder, Item};
use crate::search::{self, ColorQuery, FolderIndex};

/// A smart folder, read from its JSON entry.
#[derive(Debug, Clone, PartialEq)]
pub struct SmartFolder {
    pub id: String,
    pub name: String,
    pub conditions: Vec<Value>,
    pub children: Vec<SmartFolder>,
    /// Icon name from [`crate::model::FOLDER_ICONS`], if not the default.
    pub icon: Option<String>,
    /// Colour from [`crate::model::ICON_COLORS`], if not the default.
    pub icon_color: Option<String>,
    pub description: String,
    /// The smart folder's own sort, as Eagle's `orderBy` string.
    pub order_by: Option<String>,
    pub sort_increase: Option<bool>,
}

impl SmartFolder {
    fn from_value(v: &Value) -> Option<SmartFolder> {
        let text = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        Some(SmartFolder {
            id: v.get("id")?.as_str()?.to_string(),
            name: text("name").unwrap_or_default(),
            icon: text("icon"),
            icon_color: text("iconColor"),
            description: text("description").unwrap_or_default(),
            order_by: text("orderBy"),
            sort_increase: v.get("sortIncrease").and_then(Value::as_bool),
            conditions: v.get("conditions").and_then(Value::as_array).cloned().unwrap_or_default(),
            children: v
                .get("children")
                .and_then(Value::as_array)
                .map(|c| c.iter().filter_map(SmartFolder::from_value).collect())
                .unwrap_or_default(),
        })
    }

    /// Whether this is a smart folder group: an entry with no conditions,
    /// which shows the union of its children. Eagle stores no flag; its
    /// "new group" command simply makes an entry with `conditions: []`.
    pub fn is_group(&self) -> bool {
        self.conditions.is_empty()
    }

    /// Visits this smart folder and its descendants with their ancestry
    /// (outermost first).
    pub fn walk<'a>(&'a self, path: &mut Vec<&'a SmartFolder>, f: &mut impl FnMut(&'a SmartFolder, &[&'a SmartFolder])) {
        f(self, path);
        path.push(self);
        for c in &self.children {
            c.walk(path, f);
        }
        path.pop();
    }
}

/// What evaluating rules needs besides the item: the folder index (for
/// `folderName`), the fonts activated through Eagle (for `fontActivated`,
/// read on first use) and a cache of compiled regexes.
pub struct Context<'a> {
    pub folders: &'a FolderIndex,
    font_dir: Option<PathBuf>,
    fonts: OnceCell<HashSet<String>>,
    /// Compiled patterns, case-sensitive in `[0]` and case-insensitive in
    /// `[1]`; `None` for a pattern that doesn't compile.
    regexes: [RefCell<HashMap<String, Option<Regex>>>; 2],
}

impl<'a> Context<'a> {
    /// A context reading activated fonts from Eagle's font folder
    /// ([`search::eagle_font_dir`]).
    pub fn new(folders: &'a FolderIndex) -> Context<'a> {
        Context::with_font_dir(folders, search::eagle_font_dir())
    }

    /// A context reading activated fonts from `dir` (none when `None`).
    pub fn with_font_dir(folders: &'a FolderIndex, dir: Option<PathBuf>) -> Context<'a> {
        Context { folders, font_dir: dir, fonts: OnceCell::new(), regexes: Default::default() }
    }

    fn fonts(&self) -> &HashSet<String> {
        self.fonts.get_or_init(|| self.font_dir.as_deref().map(search::installed_font_keys).unwrap_or_default())
    }

    /// Whether `pattern` matches somewhere in `text`. A pattern that doesn't
    /// compile never matches.
    fn test(&self, pattern: &str, ignore_case: bool, text: &str) -> bool {
        let mut cache = self.regexes[usize::from(ignore_case)].borrow_mut();
        if !cache.contains_key(pattern) {
            let re = RegexBuilder::new(pattern).case_insensitive(ignore_case).size_limit(1 << 20).build().ok();
            cache.insert(pattern.to_string(), re);
        }
        cache[pattern].as_ref().is_some_and(|re| re.is_match(text))
    }
}

/// Whether a live item is in a smart folder: it must pass the folder's
/// conditions and every ancestor's. A folder without conditions shows the
/// union of its children (nothing if it has none), as Eagle's view does.
pub fn contains(item: &Item, folder: &SmartFolder, ancestors: &[&SmartFolder], cx: &Context) -> bool {
    if item.is_deleted {
        return false;
    }
    if !ancestors.iter().all(|a| a.conditions.is_empty() || passes(item, &a.conditions, cx)) {
        return false;
    }
    if folder.conditions.is_empty() {
        let mut path: Vec<&SmartFolder> = ancestors.to_vec();
        path.push(folder);
        return folder.children.iter().any(|c| contains(item, c, &path, cx));
    }
    passes(item, &folder.conditions, cx)
}

/// All conditions AND-ed; `boolean: "FALSE"` negates one. A condition
/// Eagle can't evaluate (no `rules`, or a rule with an unknown property
/// that is reached) makes Eagle's whole test fail, negated or not.
fn passes(item: &Item, conditions: &[Value], cx: &Context) -> bool {
    conditions.iter().all(|c| {
        let negate = c.get("boolean").and_then(Value::as_str) == Some("FALSE");
        condition(item, c, cx).is_some_and(|hit| hit != negate)
    })
}

/// One condition, in Eagle's order: `AND` stops at the first miss, `OR` at
/// the first hit, and anything else needs every rule to pass. No rules
/// passes.
fn condition(item: &Item, c: &Value, cx: &Context) -> Option<bool> {
    let mode = c.get("match").and_then(Value::as_str);
    let mut all = true;
    for r in c.get("rules")?.as_array()? {
        let hit = rule(item, r, cx)?;
        if !hit {
            all = false;
            if mode == Some("AND") {
                return Some(false);
            }
        } else if mode == Some("OR") {
            return Some(true);
        }
    }
    Some(all)
}

/// One rule, or `None` for a property Eagle doesn't know.
fn rule(item: &Item, r: &Value, cx: &Context) -> Option<bool> {
    let prop = r.get("property").and_then(Value::as_str).unwrap_or("");
    let method = r.get("method").and_then(Value::as_str).unwrap_or("");
    let value = r.get("value").cloned().unwrap_or(Value::Null);
    let pair = |i: usize| value.as_array().and_then(|a| a.get(i));
    Some(match prop {
        "name" => text_rule(&item.name, method, &value, cx),
        "url" => text_rule(&item.url, method, &value, cx),
        "annotation" => text_rule(&item.annotation, method, &value, cx),
        "camera" => search::camera(item).is_some_and(|c| text_rule(c, method, &value, cx)),
        "comments" => comments_rule(item, method, &value, cx),
        "folderName" => item.folders.iter().any(|f| text_rule(cx.folders.get(f).map_or("", |(name, _)| name), method, &value, cx)),
        "bpm" => search::bpm(item).is_some_and(|b| number_rule(b, method, &value)),
        "width" => number_rule(item.width as f64, method, &value),
        "height" => number_rule(item.height as f64, method, &value),
        "fileSize" => {
            let unit = r.get("unit").and_then(Value::as_str).unwrap_or("mb");
            // Eagle treats any unit other than kb as MB.
            let div = if unit == "kb" { 1024.0 } else { 1_048_576.0 };
            number_rule(item.size as f64 / div, method, &value)
        }
        "duration" => search::duration(item).filter(|d| *d != 0.0).is_some_and(|d| {
            // Seconds unless the unit is `h` or `m`.
            let d = match r.get("unit").and_then(Value::as_str) {
                Some("h") => d / 3600.0,
                Some("m") => d / 60.0,
                _ => d,
            };
            number_rule(d, method, &value)
        }),
        "iso" => raw_meta(item, "isoSpeed").and_then(parse_int).is_some_and(|iso| compare(iso, method, pair(0).and_then(parse_int), pair(1).and_then(parse_int))),
        // The first number in `f/2.8` or `50.0 mm`.
        "aperture" | "focalLength" => raw_meta(item, prop).and_then(|v| nth_number(v, 0)).is_some_and(|x| compare(x, method, pair(0).and_then(parse_float), pair(1).and_then(parse_float))),
        // The second number in `1/250.0 sec`: the exposure's denominator.
        "shutter" => raw_meta(item, "shutter").and_then(|v| nth_number(v, 1)).is_some_and(|x| compare(x, method, pair(0).and_then(parse_float), pair(1).and_then(parse_float))),
        "timestamp" => raw_meta(item, "timestamp").and_then(parse_int).filter(|t| *t != 0.0).is_some_and(|t| date_rule(t, method, pair(0).and_then(parse_int), pair(1).and_then(parse_int))),
        "createTime" => date_value(item.modification_time, method, &value),
        "mtime" => date_value(if item.mtime > 0 { item.mtime } else { item.modification_time }, method, &value),
        "btime" => date_value(if item.btime > 0 { item.btime } else { item.modification_time }, method, &value),
        "tags" => set_rule(&item.tags, method, &value, true),
        "folders" => set_rule(&item.folders, method, &value, false),
        "fontActivated" => {
            // Eagle's key for an activated font; see [`search::font_key`].
            let Some(name) = search::font_name(item, "postScriptName").filter(|n| !n.is_empty()) else { return Some(false) };
            let on = cx.fonts().contains(&format!("{name}_.{}", item.ext));
            match method {
                "activate" => on,
                "deactivate" => !on,
                _ => false,
            }
        }
        "type" => {
            let v = value.as_str().unwrap_or("");
            let hit = item.ext == v || search::type_group(&item.ext) == Some(v.trim_end_matches('s')) || (v == "presentation" && matches!(item.ext.as_str(), "ppt" | "key" | "pptx"));
            if method == "unequal" { !hit } else { hit }
        }
        "rating" => {
            // Eagle reads no star, 0 and "none" all as "unrated".
            let star = item.extra.get("star").and_then(Value::as_f64).filter(|s| *s != 0.0);
            let want = parse_int(&value).filter(|v| *v != 0.0);
            match method {
                // Eagle's dialog stores the stars as text ("35"); any "none"
                // in it matches every item, as in Eagle.
                "contain" => match &value {
                    Value::String(s) => s.contains("none") || star.is_some_and(|st| s.contains(&(st as i64).to_string())),
                    Value::Array(a) => a.iter().any(|v| v == "none") || star.is_some_and(|st| a.iter().any(|v| parse_int(v) == Some(st))),
                    _ => false,
                },
                "equal" => star == want,
                "unequal" => star != want,
                _ => false,
            }
        }
        "shape" => {
            let shape = shape_of(item.width, item.height);
            let hit = match value.as_str() {
                Some("custom") => {
                    let rw = r.get("width").and_then(Value::as_f64);
                    let rh = r.get("height").and_then(Value::as_f64);
                    matches!((rw, rh), (Some(a), Some(b)) if item.height > 0 && a / b == item.width as f64 / item.height as f64)
                }
                Some(v) => shape == Some(v),
                None => false,
            };
            if method == "unequal" { !hit } else { hit }
        }
        "color" => match method {
            "grayscale" => search::Filter { grayscale: true, ..Default::default() }.check(item).is_some(),
            _ => {
                let Some(rgb) = value.as_str().and_then(hex) else { return Some(false) };
                let acc = if method == "accuracy" { 10.0 } else { 20.0 };
                smart_color(item, rgb, acc)
            }
        },
        _ => return None,
    })
}

/// The stricter colour test smart folders use.
fn smart_color(item: &Item, rgb: [u8; 3], acc: f64) -> bool {
    let p = &item.palettes;
    let ratio = |i: usize| p.get(i).and_then(|c| c.ratio.as_f64()).unwrap_or(0.0);
    if p.is_empty() || ratio(0) < 33.0 {
        return false;
    }
    if p[0].color.iter().zip(rgb).any(|(a, b)| (i32::from(*a) - i32::from(b)).abs() > 96) {
        return false;
    }
    if p.iter().take(2).any(|c| c.color == rgb) {
        return true;
    }
    ColorQuery { rgb, accuracy: acc }.distance(item).is_some()
}

fn hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim_start_matches('#');
    let v = u32::from_str_radix(s, 16).ok().filter(|_| s.len() == 6)?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

fn shape_of(w: u64, h: u64) -> Option<&'static str> {
    if w == 0 || h == 0 {
        return None;
    }
    let (wf, hf) = (w as f64, h as f64);
    Some(if w > h {
        if wf / hf >= 2.5 { "panoramic-landscape" } else { "landscape" }
    } else if w < h {
        if hf / wf >= 2.5 { "panoramic-portrait" } else { "portrait" }
    } else {
        "square"
    })
}

/// A text rule on lower-cased text with a lower-cased value, as Eagle's
/// name, URL, note, folder name and camera rules do. An empty value only
/// works with `empty` and `not-empty`.
fn text_rule(text: &str, method: &str, value: &Value, cx: &Context) -> bool {
    let v = value.as_str().unwrap_or("").to_lowercase();
    if v.is_empty() && !matches!(method, "empty" | "not-empty") {
        return false;
    }
    string_method(&text.to_lowercase(), method, &v, cx)
}

/// Eagle's string methods. `startWith` and `endWith` build a
/// case-insensitive regex from `^` + value and value + `$`, so the value is
/// a pattern there too. Eagle runs JavaScript `RegExp`; falcon runs the
/// `regex` crate, which has no look-around or backreferences (such a
/// pattern never matches) and is Unicode-aware for `\w`, `\d`, `\b` and
/// case folding where JavaScript without the `u` flag is ASCII-only. Note
/// that Eagle lower-cases the pattern along with the value for the
/// lower-cased rules, so `\D` becomes `\d`; falcon does the same.
fn string_method(text: &str, method: &str, v: &str, cx: &Context) -> bool {
    match method {
        "equal" => text == v,
        "contain" => text.contains(v),
        "uncontain" => !text.contains(v),
        "startWith" => cx.test(&format!("^{v}"), true, text),
        "endWith" => cx.test(&format!("{v}$"), true, text),
        "empty" => text.is_empty(),
        "not-empty" => !text.is_empty(),
        "regex" => cx.test(v, false, text),
        _ => false,
    }
}

/// The comments rule: `empty` and `not-empty` look at the list; the other
/// methods test every comment's text joined together, case-sensitively
/// (Eagle doesn't lower-case here) and without refusing an empty value.
fn comments_rule(item: &Item, method: &str, value: &Value, cx: &Context) -> bool {
    let list = item.extra.get("comments").and_then(Value::as_array);
    match method {
        "empty" => list.is_none_or(|l| l.is_empty()),
        "not-empty" => list.is_some_and(|l| !l.is_empty()),
        _ => list.is_some_and(|l| {
            let text: String = l.iter().filter_map(|c| c.get("annotation").and_then(Value::as_str)).collect();
            string_method(&text, method, value.as_str().unwrap_or(""), cx)
        }),
    }
}

/// A numeric rule with Eagle's `[a, b]` value (`b` only for `between`).
fn number_rule(x: f64, method: &str, value: &Value) -> bool {
    let n = |i: usize| value.as_array().and_then(|a| a.get(i)).and_then(Value::as_f64);
    compare(x, method, n(0), n(1))
}

fn compare(x: f64, method: &str, a: Option<f64>, b: Option<f64>) -> bool {
    let Some(a) = a else { return false };
    match method {
        "=" => x == a,
        ">=" => x >= a,
        "<=" => x <= a,
        ">" => x > a,
        "<" => x < a,
        "between" => b.is_some_and(|b| x >= a && x <= b),
        _ => false,
    }
}

/// A truthy entry of the item's `rawMetas` (photo metadata).
fn raw_meta<'a>(item: &'a Item, key: &str) -> Option<&'a Value> {
    item.extra.get("rawMetas")?.get(key).filter(|v| match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        _ => true,
    })
}

/// JavaScript's `parseInt` for a number or a string: leading whitespace, a
/// sign and decimal digits; `None` for NaN.
fn parse_int(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64().map(f64::trunc),
        Value::String(s) => {
            let s = s.trim_start();
            let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
            let len = digits.bytes().take_while(u8::is_ascii_digit).count();
            let n: f64 = digits[..len].parse().ok()?;
            Some(if s.starts_with('-') { -n } else { n })
        }
        _ => None,
    }
}

/// JavaScript's `parseFloat` for a number or a string (the longest leading
/// decimal number); `None` for NaN.
fn parse_float(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => {
            let s = s.trim_start();
            (1..=s.len()).rev().filter(|&i| s.is_char_boundary(i)).find_map(|i| {
                let p = &s[..i];
                // Rust also parses "inf", "nan" and a trailing "e"-less exponent
                // form JavaScript doesn't; keep to digits, sign, dot and exponent.
                (p.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.' | b'e' | b'E')) && p.bytes().any(|b| b.is_ascii_digit()))
                    .then(|| p.parse::<f64>().ok())
                    .flatten()
            })
        }
        _ => None,
    }
}

/// The `n`th number (`[+-]?\d+(\.\d+)?`) in a string value, as Eagle reads
/// aperture, focal length and shutter text. Not a string: `None`.
fn nth_number(v: &Value, n: usize) -> Option<f64> {
    let s = v.as_str()?.as_bytes();
    let digits = |i: usize| s[i..].iter().take_while(|b| b.is_ascii_digit()).count();
    let mut found = 0;
    let mut i = 0;
    while i < s.len() {
        let sign = usize::from(matches!(s[i], b'+' | b'-'));
        let int = if i + sign < s.len() { digits(i + sign) } else { 0 };
        if int == 0 {
            i += 1;
            continue;
        }
        let mut end = i + sign + int;
        if end + 1 < s.len() && s[end] == b'.' && s[end + 1].is_ascii_digit() {
            end += 1 + digits(end + 1);
        }
        if found == n {
            return std::str::from_utf8(&s[i..end]).ok()?.parse().ok();
        }
        found += 1;
        i = end;
    }
    None
}

const DAY: f64 = 86_400_000.0;

/// A date rule on a timestamp in ms with Eagle's `[a, b]` value (`a` in
/// days for `within`).
fn date_value(t: u64, method: &str, value: &Value) -> bool {
    let n = |i: usize| value.as_array().and_then(|a| a.get(i)).and_then(Value::as_f64);
    t != 0 && date_rule(t as f64, method, n(0), n(1))
}

fn date_rule(t: f64, method: &str, a: Option<f64>, b: Option<f64>) -> bool {
    let Some(a) = a else { return false };
    match method {
        // Same local calendar day, as Eagle's `toDateString()` compares.
        "on" => local_day(t).is_some_and(|d| local_day(a) == Some(d)),
        "before" => t <= a + DAY,
        "after" => t >= a,
        "between" => b.is_some_and(|b| t >= a && t <= b + DAY),
        "within" => t + a * DAY >= Local::now().timestamp_millis() as f64,
        _ => false,
    }
}

fn local_day(ms: f64) -> Option<NaiveDate> {
    Local.timestamp_millis_opt(ms as i64).single().map(|d| d.date_naive())
}

fn set_rule(list: &[String], method: &str, value: &Value, tags: bool) -> bool {
    let want: Vec<&str> = value.as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    let has = |w: &&str| list.iter().any(|x| x == w);
    match method {
        "union" => want.iter().any(has),
        "intersection" => (if tags { !want.is_empty() } else { true }) && want.iter().all(has),
        "equal" => want.iter().all(has) && list.len() == want.len() && (!tags || !want.is_empty()),
        "identity" => !want.iter().any(has),
        "empty" => list.is_empty(),
        "not-empty" => !list.is_empty(),
        _ => false,
    }
}

impl Library {
    /// The library's smart folders, top level first.
    pub fn smart_folders(&self) -> Vec<SmartFolder> {
        self.metadata.smart_folders.iter().filter_map(SmartFolder::from_value).collect()
    }

    /// Ids of live items in the smart folder `id`.
    pub fn smart_folder_items(&self, id: &str) -> Vec<&Item> {
        let roots = self.smart_folders();
        let mut found: Option<(SmartFolder, Vec<SmartFolder>)> = None;
        for r in &roots {
            r.walk(&mut Vec::new(), &mut |f, path| {
                if f.id == id {
                    found = Some((f.clone(), path.iter().map(|p| (*p).clone()).collect()));
                }
            });
        }
        let Some((folder, ancestors)) = found else { return Vec::new() };
        let refs: Vec<&SmartFolder> = ancestors.iter().collect();
        let fx = search::folder_index(&self.metadata.folders);
        let cx = Context::new(&fx);
        self.items().iter().filter(|i| contains(i, &folder, &refs, &cx)).collect()
    }

    /// Adds a top-level smart folder at the end and returns its id.
    pub fn create_smart_folder(&mut self, name: &str, conditions: Vec<Value>) -> Result<String> {
        self.create_smart_folder_in(None, name, conditions)
    }

    /// Adds a smart folder at the end of `parent`'s children (or at the top
    /// level) and returns its id. A top-level entry takes the shape Eagle's
    /// save rebuilds top-level entries into (`id`, `name`, `description`,
    /// `modificationTime`, `conditions`, `children`). Eagle writes nested
    /// entries as its dialog built them, with the parent's id added
    /// (`id`, `name`, `conditions`, `modificationTime`, `children`, `parent`).
    pub fn create_smart_folder_in(&mut self, parent: Option<&str>, name: &str, conditions: Vec<Value>) -> Result<String> {
        let id = ids::new_id();
        let (name, now) = (display_name(name), now_ms());
        match parent {
            None => self.metadata.smart_folders.push(json!({
                "id": id, "name": name, "description": "", "modificationTime": now, "conditions": conditions, "children": []
            })),
            Some(p) => {
                let entry = json!({
                    "id": id, "name": name, "conditions": conditions, "modificationTime": now, "children": [], "parent": p
                });
                let Some(parent) = find_smart(&mut self.metadata.smart_folders, p) else { return Err(self.no_smart(p)) };
                children_of(parent).push(entry);
            }
        }
        self.save_metadata()?;
        Ok(id)
    }

    /// Adds a smart folder group (no conditions, the `grid` icon) at the end
    /// of `parent`'s children or of the top level, and returns its id.
    /// Eagle's command only makes top-level groups, which its save writes
    /// as `id`, `icon`, `name`, `description`, `modificationTime`,
    /// `conditions`, `children`. A nested group keeps the key order of
    /// Eagle's new group object with `parent` added, as a group dragged into
    /// another smart folder would be written.
    pub fn create_smart_folder_group(&mut self, parent: Option<&str>, name: &str) -> Result<String> {
        let id = ids::new_id();
        let (name, now) = (display_name(name), now_ms());
        match parent {
            None => self.metadata.smart_folders.push(json!({
                "id": id, "icon": "grid", "name": name, "description": "", "modificationTime": now, "conditions": [], "children": []
            })),
            Some(p) => {
                let entry = json!({
                    "id": id, "name": name, "modificationTime": now, "children": [], "conditions": [], "icon": "grid", "parent": p
                });
                let Some(parent) = find_smart(&mut self.metadata.smart_folders, p) else { return Err(self.no_smart(p)) };
                children_of(parent).push(entry);
            }
        }
        self.save_metadata()?;
        Ok(id)
    }

    /// Replaces a smart folder's name and conditions (at any depth) and
    /// stamps its `modificationTime`, as Eagle's edit dialog does.
    pub fn update_smart_folder(&mut self, id: &str, name: &str, conditions: Vec<Value>) -> Result<()> {
        if let Some(v) = find_smart(&mut self.metadata.smart_folders, id) {
            v["name"] = json!(display_name(name));
            v["conditions"] = json!(conditions);
            v["modificationTime"] = json!(now_ms());
        }
        self.save_metadata()
    }

    /// Renames a smart folder (at any depth) in place, as Eagle's sidebar
    /// rename does: only `name` changes (cut to 1024 characters), and
    /// `modificationTime` is left alone.
    pub fn rename_smart_folder(&mut self, id: &str, name: &str) -> Result<()> {
        let Some(v) = find_smart(&mut self.metadata.smart_folders, id) else { return Err(self.no_smart(id)) };
        v["name"] = json!(display_name(name));
        self.save_metadata()
    }

    /// Removes a smart folder (at any depth) with its children.
    pub fn delete_smart_folder(&mut self, id: &str) -> Result<()> {
        remove_smart(&mut self.metadata.smart_folders, id);
        self.save_metadata()
    }

    /// Moves a smart folder under `parent` (or to the top level) at
    /// position `index` among its new siblings, counted after it has left
    /// its old place; past the end appends. A nested entry gets `parent`
    /// set to its parent's id and a top-level one loses it, as Eagle's drag
    /// does. Moving into itself or a descendant is refused. Children keep
    /// narrowing their new ancestors when evaluated.
    pub fn move_smart_folder(&mut self, id: &str, parent: Option<&str>, index: usize) -> Result<()> {
        let Some(entry) = find_smart(&mut self.metadata.smart_folders, id) else { return Err(self.no_smart(id)) };
        if let Some(p) = parent {
            if find_smart(std::slice::from_mut(entry), p).is_some() {
                return Err(self.invalid("cannot move a smart folder into itself"));
            }
            if find_smart(&mut self.metadata.smart_folders, p).is_none() {
                return Err(self.no_smart(p));
            }
        }
        let mut entry = remove_smart(&mut self.metadata.smart_folders, id).ok_or_else(|| self.no_smart(id))?;
        if let Some(m) = entry.as_object_mut() {
            match parent {
                Some(p) => {
                    m.insert("parent".into(), json!(p));
                }
                None => {
                    m.shift_remove("parent");
                }
            }
        }
        let siblings = match parent {
            None => &mut self.metadata.smart_folders,
            Some(p) => children_of(find_smart(&mut self.metadata.smart_folders, p).expect("parent checked above")),
        };
        siblings.insert(index.min(siblings.len()), entry);
        self.save_metadata()
    }

    /// Sets a smart folder's icon and colour; `None` removes the key. Pass
    /// the current value to keep one. A new key goes where Eagle's save
    /// puts it on a top-level entry (right after `id`), or at the end of a
    /// nested one. `modificationTime` is left alone.
    pub fn set_smart_folder_icon(&mut self, id: &str, icon: Option<&str>, color: Option<&str>) -> Result<()> {
        self.check_color(color)?;
        let top = self.metadata.smart_folders.iter().any(|v| v.get("id").and_then(Value::as_str) == Some(id));
        let Some(m) = find_smart(&mut self.metadata.smart_folders, id).and_then(Value::as_object_mut) else { return Err(self.no_smart(id)) };
        put(m, "icon", icon.map(|s| json!(s)), top.then_some(1));
        let after_icon = usize::from(m.contains_key("icon")) + 1;
        put(m, "iconColor", color.map(|s| json!(s)), top.then_some(after_icon));
        self.save_metadata()
    }

    /// Sets a smart folder's own sort, or clears it (`orderBy` and
    /// `sortIncrease` both go).
    pub fn set_smart_folder_order(&mut self, id: &str, order: Option<FolderOrder>, increase: bool) -> Result<()> {
        let Some(m) = find_smart(&mut self.metadata.smart_folders, id).and_then(Value::as_object_mut) else { return Err(self.no_smart(id)) };
        put(m, "orderBy", order.map(|o| json!(o.as_str())), None);
        put(m, "sortIncrease", order.map(|_| json!(increase)), None);
        self.save_metadata()
    }

    fn no_smart(&self, id: &str) -> crate::Error {
        self.invalid(format!("no smart folder {id}"))
    }
}

/// Sets `key` in place if present, else inserts it at `at` (or the end);
/// `None` removes it.
fn put(m: &mut Map<String, Value>, key: &str, value: Option<Value>, at: Option<usize>) {
    match value {
        None => {
            m.shift_remove(key);
        }
        Some(v) if m.contains_key(key) => m[key] = v,
        Some(v) => {
            let at = at.unwrap_or(m.len()).min(m.len());
            m.shift_insert(at, key.to_string(), v);
        }
    }
}

fn find_smart<'a>(list: &'a mut [Value], id: &str) -> Option<&'a mut Value> {
    for v in list {
        if v.get("id").and_then(Value::as_str) == Some(id) {
            return Some(v);
        }
        if let Some(c) = v.get_mut("children").and_then(Value::as_array_mut)
            && let Some(found) = find_smart(c, id)
        {
            return Some(found);
        }
    }
    None
}

fn remove_smart(list: &mut Vec<Value>, id: &str) -> Option<Value> {
    if let Some(pos) = list.iter().position(|v| v.get("id").and_then(Value::as_str) == Some(id)) {
        return Some(list.remove(pos));
    }
    list.iter_mut().find_map(|v| v.get_mut("children").and_then(Value::as_array_mut).and_then(|c| remove_smart(c, id)))
}

/// An entry's `children`, created (at the end) when missing.
fn children_of(entry: &mut Value) -> &mut Vec<Value> {
    if !entry.get("children").is_some_and(Value::is_array) {
        entry["children"] = json!([]);
    }
    entry["children"].as_array_mut().expect("just ensured")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(json: Value) -> Item {
        let mut base = json!({
            "id": "X", "name": "Red Logo", "size": 2_097_152, "btime": 0, "mtime": 0, "ext": "png",
            "tags": ["logo", "red"], "folders": ["F1"], "isDeleted": false, "url": "https://a.example/x",
            "annotation": "", "modificationTime": 1, "height": 100, "width": 300, "lastModified": 1,
            "palettes": [{"color": [250, 20, 20], "ratio": 60}]
        });
        if let (Value::Object(b), Value::Object(o)) = (&mut base, json) {
            b.extend(o);
        }
        Item::from_json_str(&base.to_string()).unwrap()
    }

    fn folder(conditions: Value) -> SmartFolder {
        SmartFolder::from_value(&json!({"id": "S", "name": "s", "conditions": conditions, "children": []})).unwrap()
    }

    #[test]
    fn rules_and_conditions() {
        let it = item(json!({}));
        let fx = FolderIndex::new();
        let cx = Context::with_font_dir(&fx, None);
        let yes = |c: Value| contains(&it, &folder(c), &[], &cx);
        assert!(yes(json!([{"match": "AND", "rules": [
            {"property": "name", "method": "contain", "value": "logo"},
            {"property": "type", "method": "equal", "value": "png"},
            {"property": "fileSize", "method": ">=", "value": [2, 0], "unit": "mb"}
        ]}])));
        assert!(!yes(json!([{"match": "AND", "rules": [{"property": "tags", "method": "intersection", "value": ["logo", "blue"]}]}])));
        assert!(yes(json!([{"match": "OR", "rules": [
            {"property": "tags", "method": "union", "value": ["blue"]},
            {"property": "shape", "method": "equal", "value": "panoramic-landscape"}
        ]}])));
        assert!(!yes(json!([{"match": "OR", "boolean": "FALSE", "rules": [{"property": "name", "method": "startWith", "value": "red"}]}])));
        assert!(yes(json!([{"rules": [{"property": "color", "method": "similar", "value": "#FF1010"}]}])));
        assert!(!yes(json!([{"rules": [{"property": "color", "method": "similar", "value": "#1010FF"}]}])));
        assert!(yes(json!([{"rules": [{"property": "rating", "method": "equal", "value": "none"}]}])));
        assert!(!yes(json!([{"rules": [{"property": "name", "method": "contain", "value": ""}]}])));
        assert!(!yes(json!([{"rules": [{"property": "bpm", "method": ">=", "value": [0, 0]}]}])), "no bpm never matches");
        let song = item(json!({"ext": "mp3", "bpm": 128}));
        let rule = |m: &str, v: Value| contains(&song, &folder(json!([{"rules": [{"property": "bpm", "method": m, "value": v}]}])), &[], &cx);
        assert!(rule("between", json!([120, 130])) && rule("=", json!([128, 0])) && !rule("<", json!([128, 0])));
    }

    /// Whether `it` passes a single rule, with activated fonts read from `fonts`.
    fn one(it: &Item, rule: Value, fonts: Option<PathBuf>) -> bool {
        let fx = FolderIndex::new();
        let cx = Context::with_font_dir(&fx, fonts);
        contains(it, &folder(json!([{"match": "AND", "rules": [rule]}])), &[], &cx)
    }

    #[test]
    fn comments_rule() {
        let it = item(json!({"comments": [{"id": "c1", "annotation": "Fix the Logo"}, {"id": "c2", "annotation": " later"}]}));
        let r = |m: &str, v: &str| one(&it, json!({"property": "comments", "method": m, "value": v}), None);
        assert!(r("contain", "Logo later") && !r("contain", "logo"), "joined text, case-sensitive");
        assert!(r("uncontain", "logo") && r("equal", "Fix the Logo later"));
        assert!(r("startWith", "fix") && r("endWith", "LATER"), "begins/ends ignore case");
        assert!(r("regex", "L.go\\s") && !r("regex", "l.go"), "regex is case-sensitive");
        assert!(r("not-empty", "") && !r("empty", "") && r("contain", ""), "an empty value is not refused");
        let none = item(json!({}));
        let empty = item(json!({"comments": []}));
        let c = |it: &Item, m: &str| one(it, json!({"property": "comments", "method": m, "value": ""}), None);
        assert!(c(&none, "empty") && c(&empty, "empty") && !c(&none, "not-empty"));
        assert!(!c(&none, "contain") && c(&empty, "contain"), "no list fails, an empty list joins to empty text");
    }

    #[test]
    fn camera_and_exposure_rules() {
        let photo = item(json!({"rawMetas": {"timestamp": 1_700_000_000_000_u64, "camera": "Canon EOS R5", "isoSpeed": 400, "focalLength": "50.0 mm", "shutter": "1/250.0 sec", "aperture": "f/2.8"}}));
        let plain = item(json!({}));
        let r = |it: &Item, p: &str, m: &str, v: Value| one(it, json!({"property": p, "method": m, "value": v}), None);
        assert!(r(&photo, "camera", "contain", json!("eos")) && r(&photo, "camera", "startWith", json!("CANON")));
        assert!(r(&photo, "camera", "regex", json!("r\\d$")) && !r(&photo, "camera", "equal", json!("canon")));
        assert!(!r(&plain, "camera", "empty", json!("")), "no camera fails every method, as in Eagle");
        assert!(r(&photo, "iso", ">", json!([100, 0])) && r(&photo, "iso", "=", json!(["400", 0])) && !r(&photo, "iso", "<", json!([400, 0])));
        assert!(r(&photo, "iso", "between", json!([400, 800])) && !r(&plain, "iso", ">", json!([0, 0])));
        assert!(r(&photo, "aperture", "=", json!([2.8, 0])) && r(&photo, "aperture", "<", json!(["4", 0])));
        assert!(r(&photo, "focalLength", ">=", json!([50, 0])) && !r(&photo, "focalLength", ">", json!([50, 0])));
        assert!(r(&photo, "shutter", "=", json!([250, 0])), "the shutter's denominator");
        let long = item(json!({"rawMetas": {"shutter": "2.0 sec", "isoSpeed": "100 ISO"}}));
        assert!(!r(&long, "shutter", ">", json!([0, 0])), "a whole-second exposure has no second number");
        assert!(r(&long, "iso", "=", json!([100, 0])), "parseInt reads the leading digits");
        let day = 1_700_000_000_000_u64;
        assert!(r(&photo, "timestamp", "on", json!([day + 1000])) && r(&photo, "timestamp", "after", json!([day])));
        assert!(r(&photo, "timestamp", "between", json!([day - 1, day])) && !r(&photo, "timestamp", "before", json!([day - 2 * 86_400_000])));
        assert!(!r(&plain, "timestamp", "after", json!([0])));
    }

    #[test]
    fn duration_rule() {
        let clip = item(json!({"ext": "mp4", "duration": 90.0}));
        let r = |m: &str, v: Value, unit: &str| one(&clip, json!({"property": "duration", "method": m, "value": v, "unit": unit}), None);
        assert!(r("=", json!([90, 0]), "s") && r("=", json!([1.5, 0]), "m") && r("<", json!([1, 0]), "h"));
        assert!(r("between", json!([1, 2]), "m") && !r(">", json!([90, 0]), "s"));
        assert!(r("<=", json!([90, 0]), "other"), "an unknown unit is seconds");
        let still = item(json!({}));
        assert!(!one(&still, json!({"property": "duration", "method": ">=", "value": [0, 0], "unit": "s"}), None));
    }

    #[test]
    fn font_activated_rule() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("ArialMT.ttf"), b"").unwrap();
        let font = |ps: &str| item(json!({"ext": "ttf", "fontMetas": {"postScriptName": {"en": ps}}}));
        let r = |it: &Item, m: &str, d: Option<PathBuf>| one(it, json!({"property": "fontActivated", "method": m}), d);
        let d = || Some(dir.path().to_path_buf());
        assert!(r(&font("ArialMT"), "activate", d()) && !r(&font("ArialMT"), "deactivate", d()));
        assert!(!r(&font("Other"), "activate", d()) && r(&font("Other"), "deactivate", d()));
        assert!(r(&font("ArialMT"), "deactivate", None), "no font folder: nothing is activated");
        assert!(!r(&item(json!({})), "deactivate", d()), "not a font fails both");
    }

    #[test]
    fn regex_and_anchored_methods() {
        let it = item(json!({"name": "Report 2024.v2"}));
        let r = |m: &str, v: &str| one(&it, json!({"property": "name", "method": m, "value": v}), None);
        assert!(r("regex", "^report \\d{4}") && !r("regex", "^\\d"), "a real regex, on lower-cased text");
        assert!(r("regex", "REPORT"), "the pattern is lower-cased too");
        assert!(!r("regex", "(") && !r("regex", "(?<=x)y"), "a pattern that doesn't compile never matches");
        assert!(r("startWith", "rep") && r("endWith", "v\\d") && r("startWith", "r.p"), "begins/ends take patterns");
        assert!(!r("regex", ""), "an empty value is refused");
    }

    #[test]
    fn on_compares_local_days() {
        use chrono::{Local, TimeZone};
        let noon = Local.with_ymd_and_hms(2024, 3, 10, 12, 0, 0).single().unwrap().timestamp_millis() as u64;
        let late = Local.with_ymd_and_hms(2024, 3, 10, 23, 30, 0).single().unwrap().timestamp_millis() as u64;
        let early = Local.with_ymd_and_hms(2024, 3, 10, 0, 15, 0).single().unwrap().timestamp_millis() as u64;
        let next = Local.with_ymd_and_hms(2024, 3, 11, 0, 15, 0).single().unwrap().timestamp_millis() as u64;
        let at = |t: u64| item(json!({"modificationTime": t}));
        let on = |t: u64, day: u64| one(&at(t), json!({"property": "createTime", "method": "on", "value": [day]}), None);
        assert!(on(late, noon) && on(early, noon) && on(noon, late), "any time on the same local day");
        assert!(!on(next, noon) && !on(noon, next));
    }

    #[test]
    fn conditions_follow_eagles_order() {
        let it = item(json!({}));
        let fx = FolderIndex::new();
        let cx = Context::with_font_dir(&fx, None);
        let yes = |c: Value| contains(&it, &folder(c), &[], &cx);
        let unknown = json!({"property": "mystery", "method": "equal", "value": "x"});
        let hit = json!({"property": "name", "method": "contain", "value": "logo"});
        let miss = json!({"property": "name", "method": "contain", "value": "zzz"});
        assert!(yes(json!([{"match": "OR", "rules": []}])), "no rules passes");
        assert!(yes(json!([{"match": "OR", "rules": [hit, unknown]}])), "OR stops at the first hit");
        assert!(!yes(json!([{"match": "AND", "rules": [hit, unknown]}])), "an unknown property that is reached fails");
        assert!(!yes(json!([{"match": "AND", "boolean": "FALSE", "rules": [hit, unknown]}])), "even when negated");
        assert!(yes(json!([{"match": "AND", "boolean": "FALSE", "rules": [miss, unknown]}])), "AND stops at the first miss");
        assert!(!yes(json!([{"match": "OR"}])), "a condition without rules fails");
    }

    #[test]
    fn rating_rule() {
        let rated = item(json!({"star": 3}));
        let unrated = item(json!({}));
        let r = |it: &Item, m: &str, v: Value| one(it, json!({"property": "rating", "method": m, "value": v}), None);
        assert!(r(&rated, "equal", json!("3")) && r(&unrated, "equal", json!("none")) && !r(&rated, "equal", json!("none")));
        assert!(r(&rated, "unequal", json!("none")) && !r(&unrated, "unequal", json!("none")));
        assert!(r(&rated, "contain", json!("35")) && !r(&rated, "contain", json!("45")) && !r(&unrated, "contain", json!("12345")));
        assert!(r(&unrated, "contain", json!("none")) && r(&rated, "contain", json!("1none")), "any none matches everything");
        assert!(r(&rated, "contain", json!(["3", "5"])));
    }

    #[test]
    fn children_narrow_their_parent() {
        let it = item(json!({}));
        let parent = folder(json!([{"rules": [{"property": "type", "method": "equal", "value": "jpg"}]}]));
        let child = folder(json!([{"rules": [{"property": "name", "method": "contain", "value": "red"}]}]));
        let fx = FolderIndex::new();
        let cx = Context::with_font_dir(&fx, None);
        assert!(contains(&it, &child, &[], &cx));
        assert!(!contains(&it, &child, &[&parent], &cx));
    }

    #[test]
    fn groups_show_the_union_of_their_children() {
        let png = item(json!({}));
        let jpg = item(json!({"id": "Y", "ext": "jpg", "name": "Blue"}));
        let fx = FolderIndex::new();
        let cx = Context::with_font_dir(&fx, None);
        let by = |ext: &str| folder(json!([{"rules": [{"property": "type", "method": "equal", "value": ext}]}]));
        let group = SmartFolder { id: "G".into(), children: vec![by("png"), by("jpg")], ..folder(json!([])) };
        assert!(group.is_group() && !by("png").is_group());
        assert!(contains(&png, &group, &[], &cx) && contains(&jpg, &group, &[], &cx));
        let empty = folder(json!([]));
        assert!(!contains(&png, &empty, &[], &cx), "a group without children shows nothing");
        // A group inside a group unions its own children.
        let outer = SmartFolder { id: "O".into(), children: vec![SmartFolder { children: vec![by("jpg")], ..folder(json!([])) }], ..folder(json!([])) };
        assert!(contains(&jpg, &outer, &[], &cx) && !contains(&png, &outer, &[], &cx));
        let trashed = item(json!({"isDeleted": true}));
        assert!(!contains(&trashed, &group, &[], &cx));
    }

    #[test]
    fn groups_are_written_in_eagles_shape() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("G.library")).unwrap();
        let g = lib.create_smart_folder_group(None, "Mine").unwrap();
        let inner = lib.create_smart_folder_group(Some(&g), "Inner").unwrap();
        let png = lib.create_smart_folder_in(Some(&g), "PNG", vec![json!({"match": "AND", "rules": [{"property": "type", "method": "equal", "value": "png"}]})]).unwrap();
        assert!(lib.create_smart_folder_group(Some("NOPE"), "x").is_err());
        let text = std::fs::read_to_string(lib.root().join("metadata.json")).unwrap();
        assert!(text.contains(&format!(r#"{{"id":"{g}","icon":"grid","name":"Mine","description":"","modificationTime":"#)), "{text}");
        assert!(text.contains(&format!(r#""conditions":[],"children":[{{"id":"{inner}","name":"Inner","modificationTime":"#)), "{text}");
        assert!(text.contains(&format!(r#""children":[],"conditions":[],"icon":"grid","parent":"{g}"}}"#)), "{text}");
        let sf = lib.smart_folders();
        assert!(sf[0].is_group() && sf[0].children[0].is_group() && !sf[0].children[1].is_group());
        assert_eq!(sf[0].children[1].id, png);
        assert!(lib.smart_folder_items(&g).is_empty(), "an empty library has nothing to show");

        let before = std::fs::read_to_string(lib.root().join("metadata.json")).unwrap();
        lib.rename_smart_folder(&inner, "Renamed").unwrap();
        let after = std::fs::read_to_string(lib.root().join("metadata.json")).unwrap();
        // The library's own modificationTime moves on every save (and can
        // land on a later millisecond), so compare everything but that.
        let strip = |s: &str| {
            let mut v: serde_json::Value = serde_json::from_str(s).unwrap();
            v.as_object_mut().unwrap().shift_remove("modificationTime");
            v.to_string() // keeps key order, so reordering still fails
        };
        assert_eq!(strip(&after), strip(&before.replace(r#""name":"Inner""#, r#""name":"Renamed""#)), "only the name changes");
        assert!(lib.rename_smart_folder("NOPE", "x").is_err());
    }
}
