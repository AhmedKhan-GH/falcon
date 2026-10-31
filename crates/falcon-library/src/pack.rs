//! `.eaglepack` export and import.
//!
//! An `.eaglepack` is a zip archive with no compression (Eagle's archiver
//! runs with `store: true`). At its root is `pack.json`,
//! `{"images": [...], "folder": {...}}`, followed by each item's whole
//! `<ID>.info/` folder. `images` holds the item records (their
//! `metadata.json` contents), newest `modificationTime` first; `folder` is
//! present only when a folder was exported and carries its subtree. On
//! import every item and folder gets a new id, so a pack can be imported
//! into the library it came from.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{Datelike, Local, NaiveDate, TimeZone, Timelike};
use filetime::FileTime;
use serde_json::{Map, Value, json};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::error::{Error, Result};
use crate::ids;
use crate::library::Library;
use crate::model::{Folder, Item};

const PACK_JSON: &str = "pack.json";

/// What to put in a pack.
#[derive(Debug, Clone, Copy)]
pub enum PackSource<'a> {
    /// These items, with their folder memberships cleared (Eagle's "export
    /// selected"). Trashed items are allowed and come out untrashed.
    Items(&'a [String]),
    /// A folder, its subfolders and every untrashed item in any of them.
    Folder(&'a str),
}

/// Settings for [`import_pack`].
#[derive(Debug, Clone, Default)]
pub struct PackImportOptions {
    /// Put every item in this folder instead of the pack's own folders
    /// (Eagle does this when a pack of items is dropped on a folder). Ignored
    /// for a folder pack, whose folder tree is recreated.
    pub folder: Option<String>,
    /// Leave out items that look like ones already in the library, by
    /// Eagle's size-and-dimensions key.
    pub skip_duplicates: bool,
}

/// Writes an `.eaglepack` of `source` to `dest` and returns the number of
/// items in it. The archive is checked after writing and removed if it
/// can't be read back.
pub fn export_pack(lib: &Library, source: PackSource<'_>, dest: &Path) -> Result<usize> {
    let refuse = |reason: String| Error::Pack { path: dest.to_path_buf(), reason };
    let mut images: Vec<(u64, Value)> = Vec::new();
    let mut folder_record = None;
    match source {
        PackSource::Folder(id) => {
            let folder = lib.all_folders().into_iter().find(|f| f.id == id).ok_or_else(|| refuse(format!("no folder {id}")))?;
            let mut subtree = HashSet::new();
            folder.walk(&mut |f| {
                subtree.insert(f.id.as_str());
            });
            for item in lib.items().iter().filter(|i| !i.is_deleted && i.folders.iter().any(|f| subtree.contains(f.as_str()))) {
                images.push((item.modification_time, to_record(item)?));
            }
            folder_record = Some(clone_tree(folder));
        }
        PackSource::Items(ids) => {
            let mut seen = HashSet::new();
            for id in ids.iter().filter(|id| seen.insert(id.as_str())) {
                let item = lib.item(id).ok_or_else(|| refuse(format!("no item {id}")))?;
                let mut record = to_record(item)?;
                if let Value::Object(m) = &mut record {
                    m.insert("folders".into(), json!([]));
                    if item.is_deleted {
                        m.shift_remove("isDeleted");
                    }
                }
                images.push((item.modification_time, record));
            }
        }
    }
    // Stable, like Eagle's sort, so ties keep library order.
    images.sort_by_key(|(t, _)| std::cmp::Reverse(*t));

    let mut pack = Map::new();
    pack.insert("images".into(), Value::Array(images.iter().map(|(_, v)| v.clone()).collect()));
    if let Some(f) = folder_record {
        pack.insert("folder".into(), f);
    }
    let pack_json = serde_json::to_vec(&pack).map_err(|e| Error::json(dest, e))?;

    let parent = dest.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent).map_err(|e| Error::io(parent, e))?;
    let entries = write_zip(lib, tmp.as_file_mut(), &pack_json, images.iter().map(|(_, v)| record_id(v)))
        .map_err(|e| refuse(e.to_string()))?;
    verify(tmp.path(), entries).map_err(refuse)?;
    tmp.persist(dest).map_err(|e| Error::io(dest, e.error))?;
    Ok(images.len())
}

/// Imports the pack at `pack` into `lib` and returns the new item ids.
/// Items and folders get new ids; a folder pack's tree is added at the root.
pub fn import_pack(lib: &mut Library, pack: &Path, opts: &PackImportOptions) -> Result<Vec<String>> {
    let refuse = |reason: String| Error::Pack { path: pack.to_path_buf(), reason };
    if let Some(f) = &opts.folder
        && !lib.folder_ids().contains(f.as_str())
    {
        return Err(refuse(format!("no folder {f}")));
    }
    let extracted = tempfile::Builder::new().prefix("falcon-eaglepack").tempdir().map_err(|e| Error::io(pack, e))?;
    extract(pack, extracted.path()).map_err(|e| refuse(e.to_string()))?;
    let manifest = extracted.path().join(PACK_JSON);
    let text = fs::read(&manifest).map_err(|_| refuse("pack.json is missing".into()))?;
    let mut manifest: Value = serde_json::from_slice(&text).map_err(|e| Error::json(&manifest, e))?;
    let records = manifest.get_mut("images").and_then(Value::as_array_mut).map(std::mem::take).ok_or_else(|| refuse("pack.json has no images list".into()))?;

    // Eagle walks the list oldest first.
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for mut record in records.into_iter().rev() {
        let Some(id) = record.get("id").and_then(Value::as_str).map(str::to_string) else { continue };
        if !seen.insert(id.clone()) || !extracted.path().join(format!("{id}.info")).is_dir() {
            continue;
        }
        if let Value::Object(m) = &mut record
            && !m.contains_key("isDeleted")
        {
            m.insert("isDeleted".into(), Value::Bool(false));
        }
        let item = Item::from_json_str(&record.to_string()).map_err(|e| refuse(format!("item {id}: {e}")))?;
        items.push(item);
    }
    if opts.skip_duplicates {
        let existing: HashSet<String> = lib.items().iter().filter(|i| !i.is_deleted).filter_map(duplicate_key).collect();
        items.retain(|i| duplicate_key(i).is_none_or(|k| !existing.contains(&k)));
    }

    let mut taken: HashSet<String> = lib.items().iter().map(|i| i.id.clone()).collect();
    let mut fresh_id = || loop {
        let id = ids::new_id();
        if taken.insert(id.clone()) && !lib.root().join("images").join(format!("{id}.info")).exists() {
            return id;
        }
    };
    let new_ids: HashMap<String, String> = items.iter().map(|i| (i.id.clone(), fresh_id())).collect();

    let mut folder = None;
    if let Some(record) = manifest.get_mut("folder").filter(|f| f.is_object()) {
        let old_root = record.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
        let mut folder_ids = HashMap::new();
        remap_folder(record, &mut folder_ids, &new_ids, &mut fresh_id);
        let tree: Folder = serde_json::from_value(record.take()).map_err(|e| refuse(format!("folder: {e}")))?;
        // Keep the folder's custom order, else oldest first.
        let rank = |i: &Item| i.extra.get("order").and_then(|o| o.get(&old_root)).and_then(as_number).unwrap_or(i.modification_time as f64);
        items.sort_by(|a, b| rank(a).total_cmp(&rank(b)));
        for item in &mut items {
            item.folders = item.folders.iter().filter_map(|f| folder_ids.get(f).cloned()).collect();
        }
        folder = Some(tree);
    } else {
        let known = lib.folder_ids();
        for item in &mut items {
            match &opts.folder {
                Some(f) => item.folders = vec![f.clone()],
                None => item.folders.retain(|f| known.contains(f.as_str())),
            }
        }
    }
    if let Some(tree) = folder {
        lib.metadata.folders.push(tree);
        lib.save_metadata()?;
    }

    let now = now_ms();
    let mut out = Vec::new();
    for (index, mut item) in items.into_iter().enumerate() {
        let from = extracted.path().join(format!("{}.info", item.id));
        item.id = new_ids[&item.id].clone();
        item.modification_time = lib.next_import_time(now + index as u64);
        let to = item.info_dir(lib.root());
        move_dir(&from, &to)?;
        lib.register_item(item.clone())?;
        out.push(item.id);
    }
    Ok(out)
}

/// An item's record as Eagle writes it into `pack.json`.
fn to_record(item: &Item) -> Result<Value> {
    serde_json::to_value(item).map_err(|e| Error::json(&item.id, e))
}

fn record_id(v: &Value) -> String {
    v.get("id").and_then(Value::as_str).unwrap_or_default().to_string()
}

/// A folder and its subtree with the keys Eagle copies into a pack, in its
/// order; keys a folder doesn't have are left out.
fn clone_tree(f: &Folder) -> Value {
    let mut m = Map::new();
    m.insert("id".into(), json!(f.id));
    m.insert("name".into(), json!(f.name));
    m.insert("description".into(), json!(f.description));
    m.insert("children".into(), Value::Array(f.children.iter().map(clone_tree).collect()));
    m.insert("modificationTime".into(), json!(f.modification_time));
    m.insert("tags".into(), json!(f.tags));
    for key in ["extendTags", "icon", "iconColor", "pinyin"] {
        if let Some(v) = f.extra.get(key).filter(|v| !v.is_null()) {
            m.insert(key.into(), v.clone());
        }
    }
    m.insert("password".into(), json!(f.password));
    m.insert("passwordTips".into(), json!(f.password_tips));
    if let Some(v) = f.extra.get("coverId").filter(|v| !v.is_null()) {
        m.insert("coverId".into(), v.clone());
    }
    if let Some(order_by) = f.extra.get("orderBy").filter(|v| truthy(v)) {
        m.insert("orderBy".into(), order_by.clone());
        if let Some(v) = f.extra.get("sortIncrease") {
            m.insert("sortIncrease".into(), v.clone());
        }
    }
    Value::Object(m)
}

/// Gives a pack folder and its children new ids, expands them and points
/// their covers at the new item ids, filling keys falcon's model requires.
fn remap_folder(v: &mut Value, folder_ids: &mut HashMap<String, String>, item_ids: &HashMap<String, String>, fresh_id: &mut impl FnMut() -> String) {
    let Value::Object(m) = v else { return };
    let new = fresh_id();
    if let Some(old) = m.get("id").and_then(Value::as_str) {
        folder_ids.insert(old.to_string(), new.clone());
    }
    m.insert("id".into(), json!(new));
    for (key, default) in [("name", json!("")), ("description", json!("")), ("children", json!([])), ("modificationTime", json!(now_ms())), ("tags", json!([])), ("password", json!("")), ("passwordTips", json!(""))] {
        if m.get(key).is_none_or(Value::is_null) {
            m.insert(key.into(), default);
        }
    }
    m.insert("isExpand".into(), Value::Bool(true));
    let cover = m.get("coverId").and_then(Value::as_str).and_then(|c| item_ids.get(c)).cloned();
    match cover {
        Some(c) => {
            m.insert("coverId".into(), json!(c));
        }
        None => {
            m.shift_remove("coverId");
        }
    }
    if let Some(Value::Array(children)) = m.get_mut("children") {
        children.retain(Value::is_object);
        for c in children {
            remap_folder(c, folder_ids, item_ids, fresh_id);
        }
    }
}

/// Eagle's duplicate key for pack imports, simplified: videos by size and
/// duration, other files by size, dimensions and extension. SVG, TIFF and
/// BMP are never treated as duplicates.
fn duplicate_key(item: &Item) -> Option<String> {
    const VIDEO: &[&str] = &["mp4", "mov", "m4v", "webm", "avi", "mkv", "wmv", "flv", "3gp", "ogv"];
    match item.ext.as_str() {
        "svg" | "tif" | "tiff" | "bmp" | "url" => None,
        ext if VIDEO.contains(&ext) => Some(format!("v-{}{}", item.size, item.extra.get("duration").map(Value::to_string).unwrap_or_default())),
        ext => Some(format!("image-{}{}{}{}", item.size, item.width, item.height, ext)),
    }
}

fn write_zip(lib: &Library, file: &mut File, pack_json: &[u8], ids: impl Iterator<Item = String>) -> io::Result<usize> {
    let mut zip = ZipWriter::new(file);
    let base = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let mut entries = 1;
    zip.start_file(PACK_JSON, base.last_modified_time(zip_time(SystemTime::now())).unix_permissions(0o644))?;
    zip.write_all(pack_json)?;
    for id in ids {
        let dir = lib.root().join("images").join(format!("{id}.info"));
        if !dir.is_dir() {
            continue;
        }
        entries += add_dir(&mut zip, base, &dir, &format!("{id}.info/"))?;
    }
    zip.finish()?;
    Ok(entries)
}

/// Adds `dir` (as `name`, ending in `/`) and everything under it; returns
/// the number of entries written.
fn add_dir<W: Write + io::Seek>(zip: &mut ZipWriter<W>, base: SimpleFileOptions, dir: &Path, name: &str) -> io::Result<usize> {
    let meta = fs::metadata(dir)?;
    zip.add_directory(name, base.last_modified_time(zip_time(meta.modified()?)).unix_permissions(mode(&meta, 0o755)))?;
    let mut count = 1;
    let mut children: Vec<_> = fs::read_dir(dir)?.collect::<io::Result<_>>()?;
    children.sort_by_key(|e| e.file_name());
    for entry in children {
        let path = entry.path();
        let child = format!("{name}{}", entry.file_name().to_string_lossy());
        if path.is_dir() {
            count += add_dir(zip, base, &path, &format!("{child}/"))?;
            continue;
        }
        let meta = fs::metadata(&path)?;
        let opts = base
            .last_modified_time(zip_time(meta.modified()?))
            .unix_permissions(mode(&meta, 0o644))
            .large_file(meta.len() >= u32::MAX as u64);
        zip.start_file(child, opts)?;
        io::copy(&mut File::open(&path)?, zip)?;
        count += 1;
    }
    Ok(count)
}

/// Eagle's check is the zip signature; falcon also reads the archive back.
fn verify(path: &Path, expected: usize) -> std::result::Result<(), String> {
    let mut head = [0u8; 4];
    File::open(path).and_then(|mut f| f.read_exact(&mut head)).map_err(|e| e.to_string())?;
    if head != *b"PK\x03\x04" {
        return Err("written file is not a zip archive".into());
    }
    let mut zip = ZipArchive::new(File::open(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    if zip.len() != expected {
        return Err(format!("archive has {} entries, expected {expected}", zip.len()));
    }
    let mut manifest = String::new();
    zip.by_name(PACK_JSON).map_err(|e| e.to_string())?.read_to_string(&mut manifest).map_err(|e| e.to_string())?;
    serde_json::from_str::<Value>(&manifest).map_err(|e| e.to_string())?;
    Ok(())
}

/// Unpacks `pack` into `dest`, keeping each file's time as `unzip` does.
fn extract(pack: &Path, dest: &Path) -> std::result::Result<(), String> {
    let mut zip = ZipArchive::new(File::open(pack).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = entry.enclosed_name() else { continue };
        let out = dest.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(p) = out.parent() {
            fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        let mut file = File::create(&out).map_err(|e| e.to_string())?;
        io::copy(&mut entry, &mut file).map_err(|e| e.to_string())?;
        drop(file);
        if let Some(t) = entry.last_modified().and_then(from_zip_time) {
            let _ = filetime::set_file_mtime(&out, FileTime::from_system_time(t));
        }
    }
    Ok(())
}

fn move_dir(from: &Path, to: &Path) -> Result<()> {
    if let Some(p) = to.parent() {
        fs::create_dir_all(p).map_err(|e| Error::io(p, e))?;
    }
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    copy_tree(from, to)
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to).map_err(|e| Error::io(to, e))?;
    for entry in fs::read_dir(from).map_err(|e| Error::io(from, e))? {
        let entry = entry.map_err(|e| Error::io(from, e))?;
        let dst: PathBuf = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &dst)?;
        } else {
            fs::copy(entry.path(), &dst).map_err(|e| Error::io(&dst, e))?;
            if let Ok(m) = entry.metadata() {
                let _ = filetime::set_file_mtime(&dst, FileTime::from_last_modification_time(&m));
            }
        }
    }
    Ok(())
}

/// Zip entries store local time with two-second precision.
fn zip_time(t: SystemTime) -> zip::DateTime {
    let local: chrono::DateTime<Local> = t.into();
    zip::DateTime::from_date_and_time(
        local.year().clamp(1980, 2107) as u16,
        local.month() as u8,
        local.day() as u8,
        local.hour() as u8,
        local.minute() as u8,
        local.second() as u8,
    )
    .unwrap_or_default()
}

fn from_zip_time(t: zip::DateTime) -> Option<SystemTime> {
    let naive = NaiveDate::from_ymd_opt(t.year().into(), t.month().into(), t.day().into())?.and_hms_opt(t.hour().into(), t.minute().into(), t.second().into())?;
    Some(Local.from_local_datetime(&naive).earliest()?.into())
}

#[cfg(unix)]
fn mode(meta: &fs::Metadata, _default: u32) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o777
}

#[cfg(not(unix))]
fn mode(_meta: &fs::Metadata, default: u32) -> u32 {
    default
}

fn as_number(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::ImportOptions;

    fn png(path: &Path, w: u32, h: u32, shade: u8) {
        image::RgbImage::from_fn(w, h, |x, y| image::Rgb([shade, (x * 7) as u8, (y * 5) as u8])).save(path).unwrap();
    }

    struct Setup {
        _dir: tempfile::TempDir,
        root: PathBuf,
        lib: Library,
        folder: String,
        child: String,
        ids: Vec<String>,
    }

    /// A library with a folder, a subfolder and two imported PNGs, one in
    /// each, plus a third untouched item.
    fn setup() -> Setup {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let mut lib = Library::create(root.join("A.library")).unwrap();
        let folder = lib.create_folder(None, "Refs").unwrap();
        let child = lib.create_folder(Some(&folder), "Inner").unwrap();
        let mut ids = Vec::new();
        for (i, f) in [&folder, &child].into_iter().enumerate() {
            let src = root.join(format!("pic{i}.png"));
            png(&src, 40 + i as u32, 30, 50 * i as u8);
            let opts = ImportOptions { folders: vec![f.clone()], tags: vec![format!("t{i}")], palette: false, ..Default::default() };
            ids.push(lib.import_file(&src, &opts).unwrap());
        }
        let src = root.join("loose.png");
        png(&src, 10, 10, 200);
        ids.push(lib.import_file(&src, &ImportOptions { palette: false, ..Default::default() }).unwrap());
        Setup { _dir: dir, root, lib, folder, child, ids }
    }

    fn read_pack(path: &Path) -> (Value, Vec<String>) {
        let mut zip = ZipArchive::new(File::open(path).unwrap()).unwrap();
        let names = (0..zip.len()).map(|i| zip.by_index(i).unwrap().name().unwrap().to_string()).collect();
        let mut text = String::new();
        zip.by_name(PACK_JSON).unwrap().read_to_string(&mut text).unwrap();
        (serde_json::from_str(&text).unwrap(), names)
    }

    #[test]
    fn items_pack_matches_eagle_layout() {
        let mut s = setup();
        let mut trashed = s.lib.item(&s.ids[1]).unwrap().clone();
        trashed.is_deleted = true;
        s.lib.update_item(trashed).unwrap();
        let out = s.root.join("two.eaglepack");
        assert_eq!(export_pack(&s.lib, PackSource::Items(&s.ids[..2]), &out).unwrap(), 2);

        let (pack, names) = read_pack(&out);
        assert_eq!(names[0], "pack.json");
        assert_eq!(pack.as_object().unwrap().keys().collect::<Vec<_>>(), ["images"]);
        let images = pack["images"].as_array().unwrap();
        // Newest modificationTime first, folders cleared.
        assert_eq!(images[0]["id"], json!(s.ids[1]));
        assert_eq!(images[1]["id"], json!(s.ids[0]));
        assert!(images.iter().all(|i| i["folders"] == json!([])));
        // A trashed item loses isDeleted; the other keeps Eagle's key order.
        assert!(images[0].get("isDeleted").is_none());
        let keys: Vec<_> = images[1].as_object().unwrap().keys().take(9).cloned().collect();
        assert_eq!(keys, ["id", "name", "size", "btime", "mtime", "ext", "tags", "folders", "isDeleted"]);
        for id in &s.ids[..2] {
            let item = s.lib.item(id).unwrap();
            assert!(names.contains(&format!("{id}.info/")));
            assert!(names.contains(&format!("{id}.info/metadata.json")));
            assert!(names.contains(&format!("{id}.info/{}.png", item.name)));
        }
        let mut zip = ZipArchive::new(File::open(&out).unwrap()).unwrap();
        assert!((0..zip.len()).all(|i| zip.by_index(i).unwrap().compression() == CompressionMethod::Stored));
    }

    #[test]
    fn folder_pack_round_trips() {
        let s = setup();
        let out = s.root.join("refs.eaglepack");
        assert_eq!(export_pack(&s.lib, PackSource::Folder(&s.folder), &out).unwrap(), 2);
        let (pack, _) = read_pack(&out);
        let folder = &pack["folder"];
        assert_eq!(folder["id"], json!(s.folder));
        assert_eq!(folder["children"][0]["id"], json!(s.child));
        let keys: Vec<_> = folder.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["id", "name", "description", "children", "modificationTime", "tags", "password", "passwordTips"]);

        let mut other = Library::create(s.root.join("B.library")).unwrap();
        let new = import_pack(&mut other, &out, &PackImportOptions::default()).unwrap();
        assert_eq!(new.len(), 2);
        let reopened = Library::open(other.root()).unwrap();
        assert_eq!(reopened.items().len(), 2);
        let tree = &reopened.metadata.folders[0];
        assert_eq!(tree.name, "Refs");
        assert_eq!(tree.children[0].name, "Inner");
        assert_ne!(tree.id, s.folder);
        assert_eq!(tree.extra.get("isExpand"), Some(&json!(true)));
        // Oldest first: the item from the parent folder was imported first.
        for (new_id, old_id) in new.iter().zip(&s.ids) {
            let a = reopened.item(new_id).unwrap();
            let b = s.lib.item(old_id).unwrap();
            assert_ne!(a.id, b.id);
            assert_eq!((&a.name, &a.ext, a.size, a.width, a.height, &a.tags), (&b.name, &b.ext, b.size, b.width, b.height, &b.tags));
            assert_eq!(fs::read(a.original_path(reopened.root())).unwrap(), fs::read(b.original_path(s.lib.root())).unwrap());
            assert_eq!(a.thumbnail_path(reopened.root()).is_some(), b.thumbnail_path(s.lib.root()).is_some());
        }
        assert_eq!(reopened.item(&new[0]).unwrap().folders, vec![tree.id.clone()]);
        assert_eq!(reopened.item(&new[1]).unwrap().folders, vec![tree.children[0].id.clone()]);
        assert!(reopened.mtime.entries.iter().any(|(id, _)| *id == new[0]));
    }

    #[test]
    fn items_pack_into_a_folder_and_duplicates() {
        let mut s = setup();
        let out = s.root.join("all.eaglepack");
        export_pack(&s.lib, PackSource::Items(&s.ids), &out).unwrap();

        // Importing into its own library: everything is a duplicate.
        let opts = PackImportOptions { skip_duplicates: true, ..Default::default() };
        assert!(import_pack(&mut s.lib, &out, &opts).unwrap().is_empty());

        // Without the check the items come in again with new ids.
        let opts = PackImportOptions { folder: Some(s.child.clone()), ..Default::default() };
        let new = import_pack(&mut s.lib, &out, &opts).unwrap();
        assert_eq!(new.len(), 3);
        assert_eq!(s.lib.items().len(), 6);
        assert!(new.iter().all(|id| s.lib.item(id).unwrap().folders == vec![s.child.clone()]));
        assert!(new.iter().all(|id| !s.ids.contains(id)));

        let bad = PackImportOptions { folder: Some("NOPE".into()), ..Default::default() };
        assert!(import_pack(&mut s.lib, &out, &bad).is_err());
    }

    #[test]
    fn rejects_unknown_sources_and_bad_packs() {
        let s = setup();
        let out = s.root.join("x.eaglepack");
        assert!(export_pack(&s.lib, PackSource::Folder("NOPE"), &out).is_err());
        assert!(export_pack(&s.lib, PackSource::Items(&["NOPE".into()]), &out).is_err());
        assert!(!out.exists());

        let mut lib = Library::create(s.root.join("C.library")).unwrap();
        fs::write(&out, b"not a zip").unwrap();
        assert!(import_pack(&mut lib, &out, &PackImportOptions::default()).is_err());
        let mut zip = ZipWriter::new(File::create(&out).unwrap());
        zip.start_file("other.txt", SimpleFileOptions::default()).unwrap();
        zip.finish().unwrap();
        assert!(import_pack(&mut lib, &out, &PackImportOptions::default()).is_err());
        assert!(lib.items().is_empty());
    }

    #[test]
    fn zip_times_round_trip_to_two_seconds() {
        let t = UNIX_EPOCH + std::time::Duration::from_secs(1_793_005_755);
        let back = from_zip_time(zip_time(t)).unwrap();
        let diff = t.duration_since(back).unwrap_or_else(|e| e.duration());
        assert!(diff.as_secs() <= 1);
    }
}
