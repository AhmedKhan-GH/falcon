//! Smart folders: saved rule sets in the library `metadata.json` (see the
//! Smart folders chapter of the docs). Entries stay raw JSON in [`LibraryMetadata`] so they
//! round-trip untouched; this module reads and evaluates them.
//!
//! [`LibraryMetadata`]: crate::model::LibraryMetadata

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::error::Result;
use crate::ids;
use crate::library::{Library, write_json};
use crate::model::Item;
use crate::search::{self, ColorQuery, FolderIndex};

/// A smart folder, read from its JSON entry.
#[derive(Debug, Clone, PartialEq)]
pub struct SmartFolder {
    pub id: String,
    pub name: String,
    pub conditions: Vec<Value>,
    pub children: Vec<SmartFolder>,
}

impl SmartFolder {
    fn from_value(v: &Value) -> Option<SmartFolder> {
        Some(SmartFolder {
            id: v.get("id")?.as_str()?.to_string(),
            name: v.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
            conditions: v.get("conditions").and_then(Value::as_array).cloned().unwrap_or_default(),
            children: v
                .get("children")
                .and_then(Value::as_array)
                .map(|c| c.iter().filter_map(SmartFolder::from_value).collect())
                .unwrap_or_default(),
        })
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

/// Whether a live item is in a smart folder: it must pass the folder's
/// conditions and every ancestor's. A folder without conditions shows the
/// union of its children (nothing if it has none), as Eagle's view does.
pub fn contains(item: &Item, folder: &SmartFolder, ancestors: &[&SmartFolder], fx: &FolderIndex) -> bool {
    if item.is_deleted {
        return false;
    }
    if !ancestors.iter().all(|a| a.conditions.is_empty() || passes(item, &a.conditions, fx)) {
        return false;
    }
    if folder.conditions.is_empty() {
        let mut path: Vec<&SmartFolder> = ancestors.to_vec();
        path.push(folder);
        return folder.children.iter().any(|c| contains(item, c, &path, fx));
    }
    passes(item, &folder.conditions, fx)
}

/// All conditions AND-ed; `boolean: "FALSE"` negates one.
fn passes(item: &Item, conditions: &[Value], fx: &FolderIndex) -> bool {
    conditions.iter().all(|c| {
        let rules = c.get("rules").and_then(Value::as_array).cloned().unwrap_or_default();
        let any = c.get("match").and_then(Value::as_str) == Some("OR");
        let hit = if any { rules.iter().any(|r| rule(item, r, fx)) } else { rules.iter().all(|r| rule(item, r, fx)) };
        let negate = c.get("boolean").and_then(Value::as_str) == Some("FALSE");
        hit != negate
    })
}

fn rule(item: &Item, r: &Value, fx: &FolderIndex) -> bool {
    let prop = r.get("property").and_then(Value::as_str).unwrap_or("");
    let method = r.get("method").and_then(Value::as_str).unwrap_or("");
    let value = r.get("value").cloned().unwrap_or(Value::Null);
    match prop {
        "name" => string_rule(&item.name, method, &value),
        "url" => string_rule(&item.url, method, &value),
        "annotation" => string_rule(&item.annotation, method, &value),
        "folderName" => item.folders.iter().filter_map(|f| fx.get(f)).any(|(name, _)| string_rule(name, method, &value)),
        "width" => number_rule(item.width as f64, method, &value),
        "height" => number_rule(item.height as f64, method, &value),
        "fileSize" => {
            let unit = r.get("unit").and_then(Value::as_str).unwrap_or("mb");
            // Eagle treats any unit other than kb as MB.
            let div = if unit == "kb" { 1024.0 } else { 1_048_576.0 };
            number_rule(item.size as f64 / div, method, &value)
        }
        "createTime" => date_rule(item.modification_time, method, &value),
        "mtime" => date_rule(if item.mtime > 0 { item.mtime } else { item.modification_time }, method, &value),
        "btime" => date_rule(if item.btime > 0 { item.btime } else { item.modification_time }, method, &value),
        "tags" => set_rule(&item.tags, method, &value, true),
        "folders" => set_rule(&item.folders, method, &value, false),
        "type" => {
            let v = value.as_str().unwrap_or("");
            let hit = item.ext == v || search::type_group(&item.ext) == Some(v.trim_end_matches('s')) || (v == "presentation" && matches!(item.ext.as_str(), "ppt" | "key" | "pptx"));
            if method == "unequal" { !hit } else { hit }
        }
        "rating" => {
            let star = item.extra.get("star").and_then(Value::as_u64).unwrap_or(0);
            let want = |v: &Value| match v.as_str() {
                Some("none") => Some(0),
                Some(s) => s.parse::<u64>().ok(),
                None => v.as_u64(),
            };
            match method {
                "contain" => value.as_array().is_some_and(|a| a.iter().any(|v| want(v) == Some(star))),
                "unequal" => want(&value) != Some(star),
                _ => want(&value) == Some(star),
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
                let Some(rgb) = value.as_str().and_then(hex) else { return false };
                let acc = if method == "accuracy" { 10.0 } else { 20.0 };
                smart_color(item, rgb, acc)
            }
        },
        _ => false,
    }
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

fn string_rule(text: &str, method: &str, value: &Value) -> bool {
    let t = text.to_lowercase();
    let v = value.as_str().unwrap_or("").to_lowercase();
    match method {
        "empty" => t.is_empty(),
        "not-empty" => !t.is_empty(),
        _ if v.is_empty() => false,
        "equal" => t == v,
        "contain" => t.contains(&v),
        "uncontain" => !t.contains(&v),
        "startWith" => t.starts_with(&v),
        "endWith" => t.ends_with(&v),
        // Eagle compiles the lower-cased value as a regex; falcon has no
        // regex engine, so it falls back to a substring test.
        "regex" => t.contains(&v),
        _ => false,
    }
}

fn number_rule(x: f64, method: &str, value: &Value) -> bool {
    let arr = value.as_array();
    let n = |i: usize| arr.and_then(|a| a.get(i)).and_then(Value::as_f64);
    let Some(a) = n(0) else { return false };
    match method {
        "=" => x == a,
        ">=" => x >= a,
        "<=" => x <= a,
        ">" => x > a,
        "<" => x < a,
        "between" => n(1).is_some_and(|b| x >= a && x <= b),
        _ => false,
    }
}

const DAY: u64 = 86_400_000;

fn date_rule(t: u64, method: &str, value: &Value) -> bool {
    if t == 0 {
        return false;
    }
    let arr = value.as_array();
    let n = |i: usize| arr.and_then(|a| a.get(i)).and_then(Value::as_f64).map(|v| v as u64);
    let Some(a) = n(0) else { return false };
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    match method {
        "on" => t / DAY == a / DAY, // UTC day; Eagle uses the local day
        "before" => t <= a + DAY,
        "after" => t >= a,
        "between" => n(1).is_some_and(|b| t >= a && t <= b + DAY),
        "within" => t + a * DAY >= now,
        _ => false,
    }
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
        self.items().iter().filter(|i| contains(i, &folder, &refs, &fx)).collect()
    }

    /// Adds a top-level smart folder in the shape Eagle's dialog writes and
    /// returns its id.
    pub fn create_smart_folder(&mut self, name: &str, conditions: Vec<Value>) -> Result<String> {
        let id = ids::new_id();
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        self.metadata.smart_folders.push(json!({
            "id": id, "name": name, "conditions": conditions, "modificationTime": now, "children": []
        }));
        self.save_metadata()?;
        Ok(id)
    }

    /// Replaces a smart folder's name and conditions (at any depth) and
    /// stamps its `modificationTime`, as Eagle's edit dialog does.
    pub fn update_smart_folder(&mut self, id: &str, name: &str, conditions: Vec<Value>) -> Result<()> {
        fn find<'a>(list: &'a mut [Value], id: &str) -> Option<&'a mut Value> {
            for v in list {
                if v.get("id").and_then(Value::as_str) == Some(id) {
                    return Some(v);
                }
                if let Some(c) = v.get_mut("children").and_then(Value::as_array_mut)
                    && let Some(found) = find(c, id)
                {
                    return Some(found);
                }
            }
            None
        }
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        if let Some(v) = find(&mut self.metadata.smart_folders, id) {
            v["name"] = json!(name);
            v["conditions"] = json!(conditions);
            v["modificationTime"] = json!(now);
        }
        self.save_metadata()
    }

    /// Removes a smart folder (at any depth) with its children.
    pub fn delete_smart_folder(&mut self, id: &str) -> Result<()> {
        fn remove(list: &mut Vec<Value>, id: &str) -> bool {
            if let Some(pos) = list.iter().position(|v| v.get("id").and_then(Value::as_str) == Some(id)) {
                list.remove(pos);
                return true;
            }
            list.iter_mut().any(|v| v.get_mut("children").and_then(Value::as_array_mut).is_some_and(|c| remove(c, id)))
        }
        remove(&mut self.metadata.smart_folders, id);
        let path = self.root().join("metadata.json");
        write_json(&path, &self.metadata)
    }
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
        let yes = |c: Value| contains(&it, &folder(c), &[], &fx);
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
    }

    #[test]
    fn children_narrow_their_parent() {
        let it = item(json!({}));
        let parent = folder(json!([{"rules": [{"property": "type", "method": "equal", "value": "jpg"}]}]));
        let child = folder(json!([{"rules": [{"property": "name", "method": "contain", "value": "red"}]}]));
        let fx = FolderIndex::new();
        assert!(contains(&it, &child, &[], &fx));
        assert!(!contains(&it, &child, &[&parent], &fx));
    }
}
