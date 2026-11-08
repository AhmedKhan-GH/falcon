//! The v2 routes for comments, tag groups, smart folders, tag and item
//! edits, folder edits and library switching.
//!
//! v2 handlers either return a value (`false`, `undefined`, an object) or
//! throw; a throw becomes [`Reply::Error`], HTTP 500 with the text in
//! `message`. Lookups that find nothing mostly return `false` or nothing
//! rather than throwing, and each handler keeps Eagle's choice.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use falcon_library::model::{Comment, Folder, Item};
use falcon_library::{Library, ids};
use serde_json::{Map, Number, Value, json};

use super::{Reply, Shared, folder_json, item_json, paginate, s, strings, tag_objects};
use crate::rules::{self, truthy};

/// Eagle's cap on names, notes and descriptions, in UTF-16 units.
const MAX_STRING: usize = 10000;
/// Eagle's cap on batch arrays.
const MAX_ARRAY: usize = 1000;

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// Whether a string value is over [`MAX_STRING`] (JavaScript's `length`).
fn too_long(args: &Map<String, Value>, key: &str) -> bool {
    args.get(key).and_then(Value::as_str).is_some_and(|v| v.encode_utf16().count() > MAX_STRING)
}

fn fail(msg: impl Into<String>) -> Reply {
    Reply::Error(msg.into())
}

/// A strict JSON array of strings (a query-string value is not an array).
fn array<'a>(args: &'a Map<String, Value>, key: &str) -> Option<Vec<&'a str>> {
    match args.get(key) {
        Some(Value::Array(a)) => Some(a.iter().filter_map(Value::as_str).collect()),
        _ => None,
    }
}

fn owned(v: Vec<&str>) -> Vec<String> {
    v.into_iter().map(String::from).collect()
}

/// `Math.round`, kept as an integer when it fits.
fn round(v: &Value) -> Value {
    let f = v.as_f64().unwrap_or(0.0);
    let r = (f + 0.5).floor();
    if r.abs() < 9.0e15 { json!(r as i64) } else { Number::from_f64(r).map_or(Value::Null, Value::Number) }
}

// ---------------------------------------------------------------------------
// Comments

/// The item behind `id`, or Eagle's error text.
fn comment_item(lib: &Library, args: &Map<String, Value>) -> Result<Item, Reply> {
    let Some(id) = s(args, "id") else { return Err(fail("id is required")) };
    lib.item(id).cloned().ok_or_else(|| fail(format!("item not found: {id}")))
}

pub(crate) fn get_comments(lib: &Library, args: &Map<String, Value>) -> Reply {
    match comment_item(lib, args) {
        Ok(item) => Reply::Data(item.extra.get("comments").filter(|c| !c.is_null()).cloned().unwrap_or(json!([]))),
        Err(r) => r,
    }
}

fn has_number(args: &Map<String, Value>, key: &str) -> bool {
    args.get(key).is_some_and(Value::is_number)
}

/// Sorts timed notes by time (stable), when every note has a time.
fn sort_timed(list: &mut [Comment]) {
    let time = |c: &Comment| c.duration.as_ref().and_then(Number::as_f64);
    if list.iter().all(|c| time(c).is_some()) {
        list.sort_by(|a, b| time(a).partial_cmp(&time(b)).unwrap_or(std::cmp::Ordering::Equal));
    }
}

pub(crate) fn add_comment(lib: &mut Library, args: &Map<String, Value>) -> Reply {
    let mut item = match comment_item(lib, args) {
        Ok(i) => i,
        Err(r) => return r,
    };
    let timed = has_number(args, "duration");
    let rect = ["x", "y", "width", "height"].iter().any(|k| has_number(args, k));
    if timed && rect {
        return fail("cannot have both duration and x/y/width/height");
    }
    if !timed && !rect {
        return fail("must provide either duration (video) or x/y/width/height (image)");
    }
    let mut comment = Comment { id: ids::new_id(), last_modified: Some(now_ms()), ..Default::default() };
    let mut reply = Map::new();
    reply.insert("id".into(), json!(comment.id));
    reply.insert("annotation".into(), json!(""));
    reply.insert("lastModified".into(), json!(comment.last_modified));
    if timed {
        let d = &args["duration"];
        if d.as_f64().is_some_and(|d| d < 0.0) {
            return fail("duration must be >= 0");
        }
        comment.duration = d.as_number().cloned();
        reply.insert("duration".into(), d.clone());
    } else {
        if !["x", "y", "width", "height"].iter().all(|k| has_number(args, k)) {
            return fail("x, y, width, height must all be numbers");
        }
        if args["width"].as_f64().is_some_and(|w| w <= 0.0) || args["height"].as_f64().is_some_and(|h| h <= 0.0) {
            return fail("width and height must be > 0");
        }
        let [x, y, w, h] = ["x", "y", "width", "height"].map(|k| round(&args[k]));
        (comment.x, comment.y, comment.width, comment.height) =
            (x.as_number().cloned(), y.as_number().cloned(), w.as_number().cloned(), h.as_number().cloned());
        for (k, v) in [("x", x), ("y", y), ("width", w), ("height", h)] {
            reply.insert(k.into(), v);
        }
    }
    if let Some(text) = args.get("annotation").and_then(Value::as_str) {
        if too_long(args, "annotation") {
            return fail(format!("annotation exceeds max length ({MAX_STRING})"));
        }
        comment.annotation = text.to_string();
        reply.insert("annotation".into(), json!(text));
    }
    let mut list = item.comments();
    list.push(comment);
    if timed {
        sort_timed(&mut list);
    }
    item.set_comments(&list);
    match lib.update_item(item) {
        Ok(()) => Reply::Data(Value::Object(reply)),
        Err(e) => fail(e.to_string()),
    }
}

/// The item and the index of `commentId` in its comments.
fn find_comment(lib: &Library, args: &Map<String, Value>) -> Result<(Item, Vec<Comment>, usize), Reply> {
    if s(args, "id").is_none() {
        return Err(fail("id is required"));
    }
    let Some(cid) = s(args, "commentId") else { return Err(fail("commentId is required")) };
    let item = comment_item(lib, args)?;
    let list = item.comments();
    if list.is_empty() {
        return Err(fail("no comments found"));
    }
    let at = list.iter().position(|c| c.id == cid).ok_or_else(|| fail(format!("comment not found: {cid}")))?;
    Ok((item, list, at))
}

pub(crate) fn update_comment(lib: &mut Library, args: &Map<String, Value>) -> Reply {
    let (mut item, mut list, at) = match find_comment(lib, args) {
        Ok(f) => f,
        Err(r) => return r,
    };
    let mut c = list[at].clone();
    if let Some(text) = args.get("annotation").and_then(Value::as_str) {
        if too_long(args, "annotation") {
            return fail(format!("annotation exceeds max length ({MAX_STRING})"));
        }
        c.annotation = text.to_string();
    }
    let timed = c.duration.as_ref().is_some_and(|d| d.as_f64().is_some());
    let rect = ["x", "y", "width", "height"].iter().any(|k| has_number(args, k));
    if timed {
        if let Some(d) = args.get("duration").filter(|d| d.is_number()) {
            if d.as_f64().is_some_and(|d| d < 0.0) {
                return fail("duration must be >= 0");
            }
            c.duration = d.as_number().cloned();
        }
        if rect {
            return fail("cannot set x/y/width/height on a video comment");
        }
    } else {
        if has_number(args, "duration") {
            return fail("cannot set duration on an image comment");
        }
        for (k, slot) in [("x", &mut c.x), ("y", &mut c.y)] {
            if has_number(args, k) {
                *slot = round(&args[k]).as_number().cloned();
            }
        }
        for (k, slot) in [("width", &mut c.width), ("height", &mut c.height)] {
            if has_number(args, k) {
                if args[k].as_f64().is_some_and(|v| v <= 0.0) {
                    return fail(format!("{k} must be > 0"));
                }
                *slot = round(&args[k]).as_number().cloned();
            }
        }
    }
    c.last_modified = Some(now_ms());
    list[at] = c.clone();
    if timed && has_number(args, "duration") {
        sort_timed(&mut list);
    }
    item.set_comments(&list);
    match lib.update_item(item) {
        Ok(()) => Reply::Data(serde_json::to_value(&c).unwrap_or(Value::Null)),
        Err(e) => fail(e.to_string()),
    }
}

pub(crate) fn remove_comment(lib: &mut Library, args: &Map<String, Value>) -> Reply {
    let (mut item, mut list, at) = match find_comment(lib, args) {
        Ok(f) => f,
        Err(r) => return r,
    };
    list.remove(at);
    item.set_comments(&list);
    match lib.update_item(item) {
        Ok(()) => Reply::Data(json!(true)),
        Err(e) => fail(e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Items

/// `/api/v2/item/update`: Eagle's `item.save`. Unknown items give `false`.
pub(crate) fn update_item(lib: &mut Library, args: &Map<String, Value>) -> Reply {
    if too_long(args, "name") {
        return fail("name too long (max 10000)");
    }
    if too_long(args, "annotation") {
        return fail("annotation too long (max 10000)");
    }
    let Some(mut item) = s(args, "id").and_then(|id| lib.item(id)).cloned() else { return Reply::Data(json!(false)) };
    if let Some(name) = s(args, "name") {
        item.name = name.to_string();
    }
    if let Some(v) = args.get("annotation").and_then(Value::as_str) {
        item.annotation = v.to_string();
    }
    if let Some(v) = args.get("url").and_then(Value::as_str) {
        item.url = v.to_string();
    }
    if let Some(v) = args.get("ext").and_then(Value::as_str) {
        item.ext = v.to_string();
    }
    if let Some(v) = args.get("height").and_then(Value::as_f64) {
        item.height = v.max(0.0) as u64;
    }
    if let Some(v) = args.get("width").and_then(Value::as_f64) {
        item.width = v.max(0.0) as u64;
    }
    if let Some(tags) = array(args, "tags") {
        item.tags = owned(tags);
    }
    if let Some(folders) = array(args, "folders") {
        item.folders = owned(folders);
    }
    if let Some(star) = args.get("star").filter(|v| v.as_f64().is_some_and(|f| (0.0..=5.0).contains(&f))) {
        item.extra.insert("star".into(), star.clone());
    }
    if let Some(t) = args.get("modificationTime").and_then(Value::as_u64).filter(|t| *t > 0) {
        item.modification_time = t;
    }
    if let Some(b) = args.get("noThumbnail").and_then(Value::as_bool) {
        item.no_thumbnail = Some(b);
    }
    if let Some(b) = args.get("noPreview").and_then(Value::as_bool) {
        item.extra.insert("noPreview".into(), json!(b));
    }
    if let Some(b) = args.get("isDeleted").and_then(Value::as_bool) {
        item.is_deleted = b;
    }
    let id = item.id.clone();
    match lib.update_item(item) {
        Ok(()) => Reply::Data(lib.item(&id).map(item_json).unwrap_or(Value::Null)),
        Err(e) => fail(e.to_string()),
    }
}

/// One `item/add` entry rewritten into the keys the v1 add helpers read.
fn add_entry(e: &Map<String, Value>, batch: bool) -> Map<String, Value> {
    let mut m = e.clone();
    if let Some(f) = e.get("folders") {
        m.insert("folderIds".into(), f.clone());
    }
    m.remove("folderId");
    m.remove("folderID");
    if !batch {
        // The single form ignores these.
        m.remove("star");
        m.remove("modificationTime");
    }
    m
}

fn add_one(lib: &Shared, e: &Map<String, Value>, batch: bool) -> Result<String, String> {
    let m = add_entry(e, batch);
    let none = Map::new();
    if e.contains_key("bookmarkURL") {
        Err("bookmarks are not supported".into())
    } else if let Some(b) = e.get("base64") {
        let b = b.as_str().unwrap_or_default();
        let url = if b.starts_with("data:") { b.to_string() } else { format!("data:image/png;base64,{b}") };
        super::add_url(lib, &url, &m, &none)
    } else if let Some(u) = e.get("url") {
        super::add_url(lib, u.as_str().unwrap_or_default(), &m, &none)
    } else if let Some(p) = e.get("path") {
        let p = PathBuf::from(p.as_str().unwrap_or_default());
        if p.is_dir() {
            return Err("not support for directory".into());
        }
        super::add_path(lib, p, &m, &none, false)
    } else {
        Err("item.add requires at least one of: url, path, base64, bookmarkURL".into())
    }
}

/// `/api/v2/item/add`: `{id}` for one entry, `{ids}` for `items`.
pub(crate) fn add_item(lib: &Shared, args: &Map<String, Value>) -> Reply {
    if let Some(Value::Array(items)) = args.get("items") {
        if items.len() > MAX_ARRAY {
            return fail(format!("Batch limit exceeded: max {MAX_ARRAY} items per request, got {}", items.len()));
        }
        let ids: Vec<String> = items.iter().filter_map(Value::as_object).filter_map(|e| add_one(lib, e, true).ok()).collect();
        return Reply::Data(json!({"ids": ids}));
    }
    match add_one(lib, args, false) {
        Ok(id) => Reply::Data(json!({"id": id})),
        Err(e) => fail(e),
    }
}

/// The `smartFolders` filter of `item/get`: ids of the items in any of
/// them, or Eagle's error for an unknown one. `None` when not asked.
pub(crate) fn smart_filter(lib: &Library, args: &Map<String, Value>) -> Result<Option<std::collections::HashSet<String>>, String> {
    let ids: Vec<String> = match args.get("smartFolders") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Array(a)) => a.iter().map(|v| v.as_str().map_or_else(|| v.to_string(), String::from)).collect(),
        Some(Value::String(v)) => match serde_json::from_str::<Value>(v) {
            Ok(Value::Array(a)) => a.iter().map(|v| v.as_str().map_or_else(|| v.to_string(), String::from)).collect(),
            _ => vec![v.clone()],
        },
        Some(other) => vec![other.to_string()],
    };
    let mut out = std::collections::HashSet::new();
    for id in ids {
        if find_smart(&lib.metadata.smart_folders, &id).is_none() {
            return Err(format!("smart folder not found: {id}"));
        }
        out.extend(lib.smart_folder_items(&id).into_iter().map(|i| i.id.clone()));
    }
    Ok(Some(out))
}

// ---------------------------------------------------------------------------
// Folders

fn find_folder_mut<'a>(folders: &'a mut [Folder], id: &str) -> Option<&'a mut Folder> {
    for f in folders {
        if f.id == id {
            return Some(f);
        }
        if let Some(found) = find_folder_mut(&mut f.children, id) {
            return Some(found);
        }
    }
    None
}

/// The id of a folder's parent (`None` at the root or when unknown).
fn parent_of(folders: &[Folder], id: &str) -> Option<String> {
    fn walk(list: &[Folder], id: &str, parent: Option<&str>) -> Option<Option<String>> {
        for f in list {
            if f.id == id {
                return Some(parent.map(String::from));
            }
            if let Some(found) = walk(&f.children, id, Some(&f.id)) {
                return Some(found);
            }
        }
        None
    }
    walk(folders, id, None).flatten()
}

fn folder_reply(lib: &Library, id: &str) -> Reply {
    match lib.all_folders().into_iter().find(|f| f.id == id) {
        Some(f) => Reply::Data(folder_json(f)),
        None => Reply::Empty,
    }
}

/// A colour argument for falcon's palette check: `""` or `null` mean the
/// default (no key).
fn colour(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str).filter(|c| !c.is_empty())
}

/// `/api/v2/folder/create`. No name gives `false`; an unknown parent puts
/// the folder at the root, as Eagle does.
pub(crate) fn create_folder(lib: &mut Library, args: &Map<String, Value>) -> Reply {
    let Some(name) = s(args, "name") else { return Reply::Data(json!(false)) };
    if too_long(args, "name") {
        return fail("name too long (max 10000)");
    }
    let parent = s(args, "parent").filter(|p| lib.folder_ids().contains(p));
    let color = colour(args.get("iconColor"));
    let id = match lib.create_folder(parent, name) {
        Ok(id) => id,
        Err(e) => return fail(e.to_string()),
    };
    // The API's folder takes no icon from its parent, unlike the app's
    // new-subfolder command.
    let result = lib.set_folder_icon(&id, None, color).and_then(|()| match args.get("description").and_then(Value::as_str) {
        Some(d) if !d.is_empty() => lib.set_folder_description(&id, d),
        _ => Ok(()),
    });
    match result {
        Ok(()) => folder_reply(lib, &id),
        Err(e) => fail(e.to_string()),
    }
}

/// `/api/v2/folder/update`: Eagle's `folder.save`, quirks included. An
/// unknown folder gives no `data`. `iconColor` and `parent` are compared
/// with the folder's own values, so leaving them out clears the colour and
/// moves a subfolder to the root.
pub(crate) fn update_folder(lib: &mut Library, args: &Map<String, Value>) -> Reply {
    if too_long(args, "name") {
        return fail("name too long (max 10000)");
    }
    if too_long(args, "description") {
        return fail("description too long (max 10000)");
    }
    let Some(id) = s(args, "id").filter(|id| lib.folder_ids().contains(id)).map(String::from) else { return Reply::Empty };
    let mut steps: Vec<falcon_library::Result<()>> = Vec::new();
    if let Some(name) = s(args, "name") {
        steps.push(lib.rename_folder(&id, name));
    }
    if let Some(d) = args.get("description").and_then(Value::as_str) {
        steps.push(lib.set_folder_description(&id, d));
    }
    if let Some(tags) = array(args, "tags")
        && let Some(f) = find_folder_mut(&mut lib.metadata.folders, &id)
    {
        f.tags = owned(tags);
        steps.push(lib.save_metadata());
    }
    let icon = lib.all_folders().into_iter().find(|f| f.id == id).and_then(|f| f.icon.clone());
    steps.push(lib.set_folder_icon(&id, icon.as_deref(), colour(args.get("iconColor"))));
    let new_parent = s(args, "parent").map(String::from);
    let old_parent = parent_of(&lib.metadata.folders, &id);
    if new_parent != old_parent && new_parent.as_deref() != Some(id.as_str()) {
        let ok = new_parent.as_deref().is_none_or(|p| lib.folder_ids().contains(p));
        // A move into the folder's own subtree is refused by the library and
        // skipped silently here, as Eagle skips it.
        if ok {
            let _ = lib.move_folder(&id, new_parent.as_deref());
        }
    }
    if let Some(Err(e)) = steps.into_iter().find(Result::is_err) {
        return fail(e.to_string());
    }
    folder_reply(lib, &id)
}

// ---------------------------------------------------------------------------
// Tags

/// Whether Eagle's tag manager knows a tag: on a live item or in a group.
fn tag_known(lib: &Library, tag: &str) -> bool {
    lib.items().iter().any(|i| !i.is_deleted && i.tags.iter().any(|t| t == tag)) || lib.metadata.tags_groups.iter().any(|g| g.tags.iter().any(|t| t == tag))
}

fn tag_object(lib: &Library, tag: &str) -> Value {
    tag_objects(lib).into_iter().find(|o| o["name"] == tag).unwrap_or_else(|| json!({"name": tag, "imageCount": 0}))
}

/// `tag/get`, with the optional case-insensitive `name` filter.
pub(crate) fn get_tags(lib: &Library, args: &Map<String, Value>) -> Reply {
    let mut tags = tag_objects(lib);
    if let Some(name) = s(args, "name").map(str::to_lowercase) {
        tags.retain(|t| t["name"].as_str().is_some_and(|n| n.to_lowercase().contains(&name)));
    }
    Reply::Data(paginate(tags, args))
}

/// `tag/getRecentTags` and `getStarredTags`: the listed tags that are on
/// live items, in list order.
pub(crate) fn listed_tags(lib: &Library, list: &[String], args: &Map<String, Value>) -> Reply {
    let all = tag_objects(lib);
    let tags: Vec<Value> = list.iter().filter_map(|t| all.iter().find(|o| o["name"] == *t).cloned()).collect();
    Reply::Data(paginate(tags, args))
}

/// `/api/v2/tag/update`: renames a tag. Missing or unknown tags give no
/// `data`.
pub(crate) fn update_tag(lib: &mut Library, args: &Map<String, Value>) -> Reply {
    let Some(from) = s(args, "originalName").map(String::from) else { return Reply::Empty };
    if too_long(args, "name") {
        return fail("name too long (max 10000)");
    }
    if !tag_known(lib, &from) {
        return Reply::Empty;
    }
    let to = match s(args, "name") {
        Some(to) if to != from => to.to_string(),
        _ => return Reply::Data(tag_object(lib, &from)),
    };
    match lib.rename_tag(&from, &to) {
        Ok(_) => Reply::Data(tag_object(lib, &to)),
        Err(e) => fail(e.to_string()),
    }
}

/// `/api/v2/tag/merge`: `source` becomes `target` everywhere.
pub(crate) fn merge_tags(lib: &mut Library, args: &Map<String, Value>) -> Reply {
    let (Some(source), Some(target)) = (s(args, "source").map(String::from), s(args, "target").map(String::from)) else {
        return fail("tag.merge requires \"source\" and \"target\"");
    };
    if source == target {
        return fail("tag.merge: source and target must be different");
    }
    if !tag_known(lib, &source) || !tag_known(lib, &target) {
        return Reply::Empty;
    }
    match lib.rename_tag(&source, &target) {
        Ok(n) => Reply::Data(json!({"affectedItems": n, "sourceRemoved": true})),
        Err(e) => fail(e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Tag groups

fn group_json(lib: &Library, id: &str) -> Value {
    lib.metadata.tags_groups.iter().find(|g| g.id == id).map_or(Value::Null, |g| serde_json::to_value(g).unwrap_or(Value::Null))
}

fn has_group(lib: &Library, id: &str) -> bool {
    lib.metadata.tags_groups.iter().any(|g| g.id == id)
}

pub(crate) fn get_tag_groups(lib: &Library, args: &Map<String, Value>) -> Reply {
    let groups: Vec<Value> = lib.metadata.tags_groups.iter().filter_map(|g| serde_json::to_value(g).ok()).collect();
    Reply::Data(paginate(groups, args))
}

pub(crate) fn create_tag_group(lib: &mut Library, args: &Map<String, Value>) -> Reply {
    let Some(name) = s(args, "name") else { return Reply::Data(json!(false)) };
    if too_long(args, "name") {
        return fail("name too long (max 10000)");
    }
    let tags = array(args, "tags").map(owned).unwrap_or_default();
    let id = match lib.create_tag_group(name, &tags, colour(args.get("color"))) {
        Ok(id) => id,
        Err(e) => return fail(e.to_string()),
    };
    if let Some(d) = args.get("description").filter(|d| !d.is_null()) {
        set_group_description(lib, &id, d);
        if let Err(e) = lib.save_metadata() {
            return fail(e.to_string());
        }
    }
    Reply::Data(group_json(lib, &id))
}

fn set_group_description(lib: &mut Library, id: &str, d: &Value) {
    if let Some(g) = lib.metadata.tags_groups.iter_mut().find(|g| g.id == id) {
        g.description = Some(d.as_str().map_or_else(|| d.to_string(), String::from));
    }
}

/// `/api/v2/tagGroup/update`: each given key replaces the group's value.
pub(crate) fn update_tag_group(lib: &mut Library, args: &Map<String, Value>) -> Reply {
    let Some(id) = s(args, "id").map(String::from) else { return Reply::Empty };
    if too_long(args, "name") {
        return fail("name too long (max 10000)");
    }
    if too_long(args, "description") {
        return fail("description too long (max 10000)");
    }
    if !has_group(lib, &id) {
        return Reply::Empty;
    }
    let mut steps = Vec::new();
    if let Some(name) = args.get("name").and_then(Value::as_str) {
        steps.push(lib.rename_tag_group(&id, name));
    }
    if let Some(tags) = array(args, "tags") {
        steps.push(lib.set_tag_group_tags(&id, &owned(tags)));
    }
    if let Some(c) = args.get("color").filter(|c| !c.is_null()) {
        steps.push(lib.set_tag_group_color(&id, colour(Some(c))));
    }
    if let Some(d) = args.get("description").filter(|d| !d.is_null()) {
        set_group_description(lib, &id, d);
        steps.push(lib.save_metadata());
    }
    if let Some(Err(e)) = steps.into_iter().find(Result::is_err) {
        return fail(e.to_string());
    }
    Reply::Data(group_json(lib, &id))
}

pub(crate) fn remove_tag_group(lib: &mut Library, args: &Map<String, Value>) -> Reply {
    let Some(id) = s(args, "id").filter(|id| has_group(lib, id)).map(String::from) else { return Reply::Data(json!(false)) };
    match lib.delete_tag_group(&id, false) {
        Ok(()) => Reply::Data(json!(true)),
        Err(e) => fail(e.to_string()),
    }
}

/// `tagGroup/addTags`: tags are added; with `removeFromSource` they also
/// leave the other groups they were in.
pub(crate) fn add_group_tags(lib: &mut Library, args: &Map<String, Value>) -> Reply {
    let Some(tags) = array(args, "tags").map(owned) else { return fail("tagGroup.addTags requires \"tags\" array") };
    if tags.len() > MAX_ARRAY {
        return fail("tags array too large (max 1000)");
    }
    let Some(id) = s(args, "groupId").filter(|id| has_group(lib, id)).map(String::from) else { return Reply::Empty };
    let result = if tags.is_empty() {
        Ok(())
    } else if truthy(args.get("removeFromSource")) {
        lib.add_tags_to_group(&id, &tags)
    } else {
        let mut all = lib.metadata.tags_groups.iter().find(|g| g.id == id).map(|g| g.tags.clone()).unwrap_or_default();
        all.extend(tags);
        lib.set_tag_group_tags(&id, &all)
    };
    match result {
        Ok(()) => Reply::Data(group_json(lib, &id)),
        Err(e) => fail(e.to_string()),
    }
}

pub(crate) fn remove_group_tags(lib: &mut Library, args: &Map<String, Value>) -> Reply {
    let Some(tags) = array(args, "tags").map(owned) else { return fail("tagGroup.removeTags requires \"tags\" array") };
    if tags.len() > MAX_ARRAY {
        return fail("tags array too large (max 1000)");
    }
    let Some(id) = s(args, "groupId").filter(|id| has_group(lib, id)).map(String::from) else { return Reply::Empty };
    match lib.remove_tags_from_group(&id, &tags) {
        Ok(()) => Reply::Data(group_json(lib, &id)),
        Err(e) => fail(e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Smart folders

fn find_smart<'a>(list: &'a [Value], id: &str) -> Option<&'a Value> {
    list.iter().find_map(|v| {
        if v.get("id").and_then(Value::as_str) == Some(id) {
            return Some(v);
        }
        v.get("children").and_then(Value::as_array).and_then(|c| find_smart(c, id))
    })
}

fn find_smart_mut<'a>(list: &'a mut [Value], id: &str) -> Option<&'a mut Value> {
    for v in list {
        if v.get("id").and_then(Value::as_str) == Some(id) {
            return Some(v);
        }
        if let Some(found) = v.get_mut("children").and_then(Value::as_array_mut).and_then(|c| find_smart_mut(c, id)) {
            return Some(found);
        }
    }
    None
}

/// A smart folder entry with `imageCount` on it and on every descendant.
fn with_counts(lib: &Library, v: &Value) -> Value {
    let mut v = v.clone();
    if let Value::Object(m) = &mut v {
        if let Some(Value::Array(children)) = m.get("children") {
            let children: Vec<Value> = children.iter().map(|c| with_counts(lib, c)).collect();
            m.insert("children".into(), Value::Array(children));
        }
        let n = m.get("id").and_then(Value::as_str).map_or(0, |id| lib.smart_folder_items(id).len());
        m.insert("imageCount".into(), json!(n));
    }
    v
}

fn smart_reply(lib: &Library, id: &str) -> Reply {
    match find_smart(&lib.metadata.smart_folders, id) {
        Some(v) => Reply::Data(with_counts(lib, v)),
        None => fail(format!("smart folder not found: {id}")),
    }
}

/// `smartFolder/get`: the whole tree, or a flat list for `id` / `ids`
/// (`ids` only as a JSON array, so only through POST).
pub(crate) fn get_smart_folders(lib: &Library, args: &Map<String, Value>) -> Reply {
    let id = s(args, "id");
    let ids = array(args, "ids");
    let list: Vec<Value> = if id.is_none() && ids.is_none() {
        lib.metadata.smart_folders.iter().map(|v| with_counts(lib, v)).collect()
    } else {
        let mut flat = Vec::new();
        fn walk<'a>(list: &'a [Value], out: &mut Vec<&'a Value>) {
            for v in list {
                out.push(v);
                if let Some(c) = v.get("children").and_then(Value::as_array) {
                    walk(c, out);
                }
            }
        }
        walk(&lib.metadata.smart_folders, &mut flat);
        flat.into_iter()
            .filter(|v| {
                let vid = v.get("id").and_then(Value::as_str).unwrap_or_default();
                match (id, &ids) {
                    (Some(id), _) => vid == id,
                    (None, Some(ids)) => ids.contains(&vid),
                    _ => false,
                }
            })
            .map(|v| with_counts(lib, v))
            .collect()
    };
    Reply::Data(paginate(list, args))
}

fn check_name(args: &Map<String, Value>, required: bool) -> Result<(), Reply> {
    match args.get("name") {
        Some(Value::String(n)) if n.is_empty() && !required => Err(fail("name cannot be empty")),
        Some(Value::String(n)) if !n.is_empty() => {
            if too_long(args, "name") { Err(fail("name too long (max 10000)")) } else { Ok(()) }
        }
        _ if required => Err(fail("name is required")),
        _ => Ok(()),
    }
}

/// Sets a key on a smart folder entry in place, or adds it at the end.
fn set_key(lib: &mut Library, id: &str, key: &str, value: Value) {
    if let Some(Value::Object(m)) = find_smart_mut(&mut lib.metadata.smart_folders, id) {
        m.insert(key.into(), value);
    }
}

pub(crate) fn create_smart_folder(lib: &mut Library, args: &Map<String, Value>) -> Reply {
    if let Err(r) = check_name(args, true) {
        return r;
    }
    if !truthy(args.get("conditions")) {
        return fail("conditions is required");
    }
    if let Err(e) = rules::check_conditions(&args["conditions"]) {
        return fail(e);
    }
    if too_long(args, "description") {
        return fail("description too long (max 10000)");
    }
    let parent = s(args, "parent");
    if let Some(p) = parent
        && find_smart(&lib.metadata.smart_folders, p).is_none()
    {
        return fail(format!("parent smart folder not found: {p}"));
    }
    let name = s(args, "name").unwrap_or_default();
    let description = s(args, "description").unwrap_or("").to_string();
    let icon = s(args, "icon").map(String::from);
    let color = colour(args.get("iconColor")).map(String::from);
    let conditions = args["conditions"].as_array().cloned().unwrap_or_default();
    let created = (|| {
        let id = match parent {
            None => lib.create_smart_folder(name, conditions)?,
            Some(p) => {
                // A nested entry is written as the API builds it.
                let id = ids::new_id();
                let entry = json!({
                    "id": id, "name": name, "conditions": conditions, "description": description,
                    "icon": icon.clone().unwrap_or_default(), "iconColor": color.clone().unwrap_or_default(),
                    "children": [], "modificationTime": now_ms(),
                });
                if let Some(Value::Object(m)) = find_smart_mut(&mut lib.metadata.smart_folders, p) {
                    match m.entry("children").or_insert_with(|| json!([])) {
                        Value::Array(c) => c.push(entry),
                        other => *other = json!([entry]),
                    }
                }
                lib.save_metadata()?;
                return Ok(id);
            }
        };
        if !description.is_empty() {
            set_key(lib, &id, "description", json!(description));
        }
        lib.set_smart_folder_icon(&id, icon.as_deref(), color.as_deref())?;
        Ok::<_, falcon_library::Error>(id)
    })();
    match created {
        Ok(id) => {
            // Eagle's reply is its in-memory object, in this key order.
            let stored = find_smart(&lib.metadata.smart_folders, &id).cloned().unwrap_or(Value::Null);
            let count = lib.smart_folder_items(&id).len();
            Reply::Data(json!({
                "id": id, "name": stored["name"], "conditions": stored["conditions"], "description": description,
                "icon": icon.unwrap_or_default(), "iconColor": color.unwrap_or_default(), "children": [],
                "modificationTime": stored["modificationTime"], "imageCount": count,
            }))
        }
        Err(e) => fail(e.to_string()),
    }
}

pub(crate) fn update_smart_folder(lib: &mut Library, args: &Map<String, Value>) -> Reply {
    let Some(id) = s(args, "id").map(String::from) else { return fail("id is required") };
    let Some(current) = find_smart(&lib.metadata.smart_folders, &id).cloned() else { return fail(format!("smart folder not found: {id}")) };
    if let Err(r) = check_name(args, false) {
        return r;
    }
    if let Some(c) = args.get("conditions")
        && let Err(e) = rules::check_conditions(c)
    {
        return fail(e);
    }
    if too_long(args, "description") {
        return fail("description too long (max 10000)");
    }
    let name = args.get("name").and_then(Value::as_str).map_or_else(|| current["name"].as_str().unwrap_or_default().to_string(), String::from);
    let conditions = args.get("conditions").or(current.get("conditions")).and_then(Value::as_array).cloned().unwrap_or_default();
    let result = (|| {
        lib.update_smart_folder(&id, &name, conditions)?;
        if let Some(d) = args.get("description").and_then(Value::as_str) {
            set_key(lib, &id, "description", json!(d));
        }
        if args.contains_key("icon") || args.contains_key("iconColor") {
            let keep = |k: &str| current.get(k).and_then(Value::as_str).filter(|v| !v.is_empty()).map(String::from);
            let icon = if args.contains_key("icon") { s(args, "icon").map(String::from) } else { keep("icon") };
            let color = if args.contains_key("iconColor") { colour(args.get("iconColor")).map(String::from) } else { keep("iconColor") };
            lib.set_smart_folder_icon(&id, icon.as_deref(), color.as_deref())?;
        } else {
            lib.save_metadata()?;
        }
        Ok::<_, falcon_library::Error>(())
    })();
    match result {
        Ok(()) => smart_reply(lib, &id),
        Err(e) => fail(e.to_string()),
    }
}

pub(crate) fn remove_smart_folder(lib: &mut Library, args: &Map<String, Value>) -> Reply {
    let Some(id) = s(args, "id").map(String::from) else { return fail("id is required") };
    let Some(entry) = find_smart(&lib.metadata.smart_folders, &id).cloned() else { return fail(format!("smart folder not found: {id}")) };
    let mut gone = Vec::new();
    fn walk(v: &Value, out: &mut Vec<String>) {
        if let Some(id) = v.get("id").and_then(Value::as_str) {
            out.push(id.to_string());
        }
        for c in v.get("children").and_then(Value::as_array).into_iter().flatten() {
            walk(c, out);
        }
    }
    walk(&entry, &mut gone);
    // Quick Access entries go with it, as in Eagle.
    lib.metadata.quick_access.retain(|q| q.get("id").and_then(Value::as_str).is_none_or(|q| !gone.iter().any(|g| g == q)));
    match lib.delete_smart_folder(&id) {
        Ok(()) => Reply::Data(json!(true)),
        Err(e) => fail(e.to_string()),
    }
}

/// `smartFolder/getItems`: live items in the folder, optionally sorted and
/// projected (fields in the order asked, missing ones left out).
pub(crate) fn smart_folder_items(lib: &Library, args: &Map<String, Value>) -> Reply {
    let Some(id) = s(args, "smartFolderId") else { return fail("smartFolderId is required") };
    if find_smart(&lib.metadata.smart_folders, id).is_none() {
        return fail(format!("smart folder not found: {id}"));
    }
    let mut items: Vec<&Item> = lib.smart_folder_items(id).into_iter().filter(|i| !i.is_deleted).collect();
    if let Some(order) = s(args, "orderBy") {
        super::sort_v1(&mut items, order);
    }
    let fields = strings(args, "fields");
    let list: Vec<Value> = items
        .into_iter()
        .map(item_json)
        .map(|v| {
            if fields.is_empty() {
                return v;
            }
            let mut m = Map::new();
            for f in &fields {
                if let Some(x) = v.get(f) {
                    m.insert(f.clone(), x.clone());
                }
            }
            Value::Object(m)
        })
        .collect();
    Reply::Data(paginate(list, args))
}

// ---------------------------------------------------------------------------
// Library

/// Why a path cannot be switched to, in Eagle's words where Eagle has any.
pub(crate) fn open_library(path: &str) -> Result<Library, String> {
    let p = Path::new(path.trim_end_matches('/'));
    if path.is_empty() || !p.exists() {
        return Err("Library does not exist.".into());
    }
    let is_library = p.is_dir() && p.extension().is_some_and(|e| e == "library") && p.join("metadata.json").is_file();
    if !is_library {
        return Err("Not an Eagle library.".into());
    }
    Library::open(p).map_err(|e| e.to_string())
}

/// `library/icon`: `icon.png` in a library folder, as Node serialises a
/// `Buffer` (`{"type":"Buffer","data":[bytes]}`).
pub(crate) fn library_icon(path: Option<&str>) -> Result<Value, String> {
    let Some(p) = path.map(Path::new).filter(|p| p.exists()) else { return Err("Library does not exist.".into()) };
    let icon = p.join("icon.png");
    let bytes = std::fs::read(&icon).map_err(|_| "Library icon does not exist.".to_string())?;
    Ok(json!({"type": "Buffer", "data": bytes}))
}
