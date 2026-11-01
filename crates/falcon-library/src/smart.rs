//! Smart folders: saved rule sets in the library `metadata.json` (see the
//! Smart folders chapter of the docs). Entries stay raw JSON in [`LibraryMetadata`] so they
//! round-trip untouched; this module reads, evaluates and edits them.
//!
//! [`LibraryMetadata`]: crate::model::LibraryMetadata

use std::time::{SystemTime, UNIX_EPOCH};

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
        "bpm" => search::bpm(item).is_some_and(|b| number_rule(b, method, &value)),
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
        assert!(!yes(json!([{"rules": [{"property": "bpm", "method": ">=", "value": [0, 0]}]}])), "no bpm never matches");
        let song = item(json!({"ext": "mp3", "bpm": 128}));
        let rule = |m: &str, v: Value| contains(&song, &folder(json!([{"rules": [{"property": "bpm", "method": m, "value": v}]}])), &[], &fx);
        assert!(rule("between", json!([120, 130])) && rule("=", json!([128, 0])) && !rule("<", json!([128, 0])));
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

    #[test]
    fn groups_show_the_union_of_their_children() {
        let png = item(json!({}));
        let jpg = item(json!({"id": "Y", "ext": "jpg", "name": "Blue"}));
        let fx = FolderIndex::new();
        let by = |ext: &str| folder(json!([{"rules": [{"property": "type", "method": "equal", "value": ext}]}]));
        let group = SmartFolder { id: "G".into(), children: vec![by("png"), by("jpg")], ..folder(json!([])) };
        assert!(group.is_group() && !by("png").is_group());
        assert!(contains(&png, &group, &[], &fx) && contains(&jpg, &group, &[], &fx));
        let empty = folder(json!([]));
        assert!(!contains(&png, &empty, &[], &fx), "a group without children shows nothing");
        // A group inside a group unions its own children.
        let outer = SmartFolder { id: "O".into(), children: vec![SmartFolder { children: vec![by("jpg")], ..folder(json!([])) }], ..folder(json!([])) };
        assert!(contains(&jpg, &outer, &[], &fx) && !contains(&png, &outer, &[], &fx));
        let trashed = item(json!({"isDeleted": true}));
        assert!(!contains(&trashed, &group, &[], &fx));
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
        assert_eq!(after, before.replace(r#""name":"Inner""#, r#""name":"Renamed""#), "only the name changes");
        assert!(lib.rename_smart_folder("NOPE", "x").is_err());
    }
}
