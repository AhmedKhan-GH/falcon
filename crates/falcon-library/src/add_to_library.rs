//! Adding items, a folder, a smart folder or a tag group of the open library
//! to another library (Eagle's "Add to Other Library").
//!
//! What Eagle does, which falcon follows:
//! - items: each `images/<id>.info` folder is copied whole into the other
//!   library. One whose id the other library already has is left alone (and
//!   counted as already there). The copy's `metadata.json` loses its
//!   `folders` (they belong to this library), and its `modificationTime`
//!   becomes now plus the item's position, so the items show up as newly
//!   added in their order.
//! - a folder: the folder (with its subfolders, ids kept) is put at the top
//!   of the other library's tree unless that library already has its id;
//!   subfolders whose ids it already has are not added a second time. Its
//!   items and its subfolders' items are copied as above but keep their
//!   `folders`, and in a folder sorted by hand they also keep their
//!   `modificationTime`.
//! - a smart folder or a tag group: only the definition is added, at the
//!   top of the list, under new ids (a smart folder's subfolders too).
//!
//! falcon differs in three small ways: an item copied with a folder keeps
//! only the folder ids the other library has once the tree is merged (so no
//! item points at a folder that isn't there); a new folder's descendants
//! whose ids exist elsewhere in the other library are merged into those, as
//! in [`crate::library_merge`]; and each item folder is copied under a
//! temporary name, its metadata rewritten there, then renamed into place, so
//! the other library never sees half an item. Eagle leaves the other
//! library's `tags.json` alone, and so does falcon.
//!
//! The copy ([`copy_items`]) needs no lock on either library and runs on a
//! background thread; [`Library::finish_add`] then merges the definitions
//! into the other library and registers the copied items there. The caller
//! checks the other library with [`check_target`] first (which applies the
//! sandbox guard).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{Error, Result};
use crate::library::Library;
use crate::model::{Folder, Item, TagGroup};

/// What is added besides the items.
#[derive(Debug, Clone, PartialEq)]
pub enum Extra {
    /// Only the items.
    None,
    /// A folder with its subtree; the items came from it.
    Folder(Box<Folder>),
    /// A smart folder's definition (the raw `smartFolders` entry).
    SmartFolder(serde_json::Value),
    /// A tag group.
    TagGroup(TagGroup),
}

/// Everything one Add to Other Library copies.
#[derive(Debug, Clone)]
pub struct AddPlan {
    /// This library's root.
    pub source: PathBuf,
    /// The other library's root.
    pub target: PathBuf,
    /// Item ids in the order they were chosen.
    pub items: Vec<String>,
    pub extra: Extra,
    /// The time the copies are stamped with (ms).
    pub now: u64,
}

impl AddPlan {
    /// Selected items.
    pub fn items(source: &Library, target: &Path, ids: Vec<String>) -> AddPlan {
        AddPlan { source: source.root().to_path_buf(), target: target.to_path_buf(), items: ids, extra: Extra::None, now: crate::edit::now_ms() }
    }

    /// Folder `id` with its subfolders, and the items in any of them, as
    /// Eagle takes them (trashed items left out). None if there is no such
    /// folder.
    pub fn folder(source: &Library, target: &Path, id: &str) -> Option<AddPlan> {
        let folder = source.all_folders().into_iter().find(|f| f.id == id)?.clone();
        let mut ids = HashSet::new();
        folder.walk(&mut |f| {
            ids.insert(f.id.clone());
        });
        let items = source.items().iter().filter(|i| !i.is_deleted && i.folders.iter().any(|f| ids.contains(f))).map(|i| i.id.clone()).collect();
        Some(AddPlan { source: source.root().to_path_buf(), target: target.to_path_buf(), items, extra: Extra::Folder(Box::new(folder)), now: crate::edit::now_ms() })
    }

    /// Smart folder `id`'s definition. None if there is no such smart folder.
    pub fn smart_folder(source: &Library, target: &Path, id: &str) -> Option<AddPlan> {
        let def = find_smart(&source.metadata.smart_folders, id)?.clone();
        Some(AddPlan { source: source.root().to_path_buf(), target: target.to_path_buf(), items: Vec::new(), extra: Extra::SmartFolder(def), now: crate::edit::now_ms() })
    }

    /// Tag group `id`. None if there is no such group.
    pub fn tag_group(source: &Library, target: &Path, id: &str) -> Option<AddPlan> {
        let group = source.metadata.tags_groups.iter().find(|g| g.id == id)?.clone();
        Some(AddPlan { source: source.root().to_path_buf(), target: target.to_path_buf(), items: Vec::new(), extra: Extra::TagGroup(group), now: crate::edit::now_ms() })
    }

    /// Whether copied items keep their `folders` and (in a folder sorted by
    /// hand) their `modificationTime`.
    fn with_folder(&self) -> Option<&Folder> {
        match &self.extra {
            Extra::Folder(f) => Some(f),
            _ => None,
        }
    }
}

fn find_smart<'a>(list: &'a [serde_json::Value], id: &str) -> Option<&'a serde_json::Value> {
    for v in list {
        if v.get("id").and_then(|i| i.as_str()) == Some(id) {
            return Some(v);
        }
        if let Some(found) = v.get("children").and_then(|c| c.as_array()).and_then(|c| find_smart(c, id)) {
            return Some(found);
        }
    }
    None
}

/// Why the other library can't take the items.
#[derive(Debug, thiserror::Error)]
pub enum AddRefusal {
    #[error("this is the library that is open; choose another one")]
    SameLibrary,
    #[error("no Eagle library was found there")]
    NotALibrary,
    #[error(transparent)]
    Library(#[from] Error),
}

/// Checks that `target` is another library falcon may write: inside the
/// sandbox, not `current`, holding `metadata.json` and `images/`, and not
/// made by a newer Eagle.
pub fn check_target(current: &Path, target: &Path) -> std::result::Result<(), AddRefusal> {
    crate::sandbox::guard(target)?;
    let same = current == target || matches!((current.canonicalize(), target.canonicalize()), (Ok(x), Ok(y)) if x == y);
    if same {
        return Err(AddRefusal::SameLibrary);
    }
    let meta = target.join("metadata.json");
    if !meta.is_file() || !target.join("images").is_dir() {
        return Err(AddRefusal::NotALibrary);
    }
    #[derive(serde::Deserialize)]
    struct Version {
        #[serde(rename = "applicationVersion", default)]
        application_version: String,
    }
    let bytes = fs::read(&meta).map_err(|e| Error::io(&meta, e))?;
    let v: Version = serde_json::from_slice(&bytes).map_err(|_| AddRefusal::NotALibrary)?;
    if crate::library::is_newer_than_app(&v.application_version) {
        return Err(Error::NewerLibrary { path: target.to_path_buf(), version: v.application_version }.into());
    }
    Ok(())
}

/// How the item copies went.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Copied {
    /// Ids copied into the other library, in order.
    pub copied: Vec<String>,
    /// Ids the other library already had (left alone).
    pub present: Vec<String>,
    /// Ids that couldn't be copied, with the reason (nothing was left behind).
    pub failed: Vec<(String, String)>,
}

/// Copies the plan's item folders into the other library's `images/`,
/// rewriting each copy's `metadata.json` as Eagle does before it is renamed
/// into place. Calls `progress(done, total)` after each item and stops early
/// when `cancel` is set (what was copied stays). Needs no library lock.
pub fn copy_items(plan: &AddPlan, cancel: &AtomicBool, mut progress: impl FnMut(usize, usize)) -> Copied {
    let mut out = Copied::default();
    let images = plan.target.join("images");
    let total = plan.items.len();
    let folder = plan.with_folder();
    let manual = folder.is_some_and(|f| f.order_by.as_deref() == Some("MANUAL"));
    for (index, id) in plan.items.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let name = format!("{id}.info");
        let target = images.join(&name);
        if target.exists() {
            out.present.push(id.clone());
        } else {
            let tmp = images.join(format!(".{name}.adding"));
            let _ = fs::remove_dir_all(&tmp);
            let src = plan.source.join("images").join(&name);
            let done = copy_dir(&src, &tmp)
                .map_err(|e| e.to_string())
                .and_then(|()| restamp(&tmp.join("metadata.json"), id, folder.is_some(), manual, plan.now + index as u64))
                .and_then(|()| fs::rename(&tmp, &target).map_err(|e| e.to_string()));
            match done {
                Ok(()) => out.copied.push(id.clone()),
                Err(e) => {
                    let _ = fs::remove_dir_all(&tmp);
                    out.failed.push((id.clone(), e));
                }
            }
        }
        progress(index + 1, total);
    }
    out
}

/// Rewrites a copied item's `metadata.json`: without a folder its `folders`
/// go; unless it came from a folder sorted by hand its `modificationTime`
/// becomes `stamp`. Everything else is written back as read.
fn restamp(file: &Path, id: &str, keep_folders: bool, manual: bool, stamp: u64) -> std::result::Result<(), String> {
    let text = fs::read_to_string(file).map_err(|e| e.to_string())?;
    let mut item = Item::from_json_str(&text).map_err(|e| format!("its metadata.json can't be read ({e})"))?;
    if item.id != id {
        return Err("its metadata.json names another item".into());
    }
    if !keep_folders {
        item.folders.clear();
    }
    if !manual {
        item.modification_time = stamp;
    }
    crate::library::write_json(file, &item).map_err(|e| e.to_string())
}

fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else {
            fs::copy(entry.path(), &to)?;
            if let Ok(meta) = entry.metadata() {
                let _ = filetime::set_file_mtime(&to, filetime::FileTime::from_last_modification_time(&meta));
            }
        }
    }
    Ok(())
}

/// What a finished Add to Other Library added.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AddReport {
    pub items: usize,
    /// Whether the folder, smart folder or tag group was added.
    pub definition: bool,
}

impl Library {
    /// The second half of Add to Other Library, on the other library: adds
    /// the plan's folder, smart folder or tag group (saving `metadata.json`
    /// only then), and registers the items [`copy_items`] copied, in memory
    /// and in `mtime.json`. With a folder, a copied item's `folders` keep
    /// only ids this library has once the tree is merged. A copied item
    /// that can't be read is left on disk and counted out.
    pub fn finish_add(&mut self, plan: &AddPlan, copied: &[String]) -> Result<AddReport> {
        let mut report = AddReport::default();
        match &plan.extra {
            Extra::None => {}
            Extra::Folder(folder) => {
                let mut known: HashSet<String> = self.folder_ids().into_iter().map(String::from).collect();
                if !known.contains(&folder.id) {
                    let before = self.metadata.folders.len();
                    crate::library_merge::merge_folder_into(&mut self.metadata.folders, (**folder).clone(), &mut known);
                    // Eagle puts the new folder at the top of the tree.
                    if let Some(pos) = self.metadata.folders.iter().skip(before).position(|f| f.id == folder.id) {
                        let f = self.metadata.folders.remove(before + pos);
                        self.metadata.folders.insert(0, f);
                    }
                    report.definition = true;
                    self.save_metadata()?;
                }
            }
            Extra::SmartFolder(def) => {
                let mut def = def.clone();
                renew_smart_ids(&mut def);
                self.metadata.smart_folders.insert(0, def);
                report.definition = true;
                self.save_metadata()?;
            }
            Extra::TagGroup(group) => {
                let mut group = group.clone();
                group.id = crate::ids::new_id();
                self.metadata.tags_groups.insert(0, group);
                report.definition = true;
                self.save_metadata()?;
            }
        }
        let folders: HashSet<String> = self.folder_ids().into_iter().map(String::from).collect();
        let with_folder = plan.with_folder().is_some();
        for id in copied {
            let file = self.root().join("images").join(format!("{id}.info")).join("metadata.json");
            let Ok(mut item) = fs::read_to_string(&file).map_err(|_| ()).and_then(|t| Item::from_json_str(&t).map_err(|_| ())) else { continue };
            if item.id != *id || self.item(id).is_some() {
                continue;
            }
            if with_folder && item.folders.iter().any(|f| !folders.contains(f)) {
                item.folders.retain(|f| folders.contains(f));
                crate::library::write_json(&file, &item)?;
            }
            let stamp = item.last_modified;
            self.adopt(item, stamp);
            report.items += 1;
        }
        if report.items > 0 {
            self.mtime.all = Some(self.items().len() as u64);
            self.write_mtime()?;
        }
        Ok(report)
    }
}

/// Gives a smart folder definition and all its subfolders new ids, as Eagle
/// does when it adds one to another library.
fn renew_smart_ids(v: &mut serde_json::Value) {
    if let Some(obj) = v.as_object_mut() {
        obj.insert("id".into(), crate::ids::new_id().into());
        if let Some(children) = obj.get_mut("children").and_then(|c| c.as_array_mut()) {
            for c in children {
                renew_smart_ids(c);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::ImportOptions;

    fn library(dir: &Path, name: &str) -> Library {
        Library::create(dir.join(name)).unwrap()
    }

    fn import(lib: &mut Library, file: &str, folders: Vec<String>) -> String {
        let src = lib.root().parent().unwrap().join(file);
        fs::write(&src, file.as_bytes()).unwrap();
        lib.import_file(&src, &ImportOptions { folders, ..Default::default() }).unwrap()
    }

    fn run(plan: &AddPlan, target: &mut Library) -> (Copied, AddReport) {
        let copied = copy_items(plan, &AtomicBool::new(false), |_, _| {});
        let report = target.finish_add(plan, &copied.copied).unwrap();
        (copied, report)
    }

    #[test]
    fn refuses_itself_and_non_libraries() {
        let d = tempfile::tempdir().unwrap();
        let a = library(d.path(), "A.library");
        let _b = library(d.path(), "B.library");
        assert!(matches!(check_target(a.root(), a.root()), Err(AddRefusal::SameLibrary)));
        fs::create_dir_all(d.path().join("Plain")).unwrap();
        assert!(matches!(check_target(a.root(), &d.path().join("Plain")), Err(AddRefusal::NotALibrary)));
        assert!(check_target(a.root(), &d.path().join("B.library")).is_ok());
    }

    #[test]
    fn items_are_copied_without_folders_and_restamped() {
        let d = tempfile::tempdir().unwrap();
        let mut a = library(d.path(), "A.library");
        let mut b = library(d.path(), "B.library");
        let f = a.create_folder(None, "F").unwrap();
        let one = import(&mut a, "one.txt", vec![f.clone()]);
        let two = import(&mut a, "two.txt", vec![]);
        let before = fs::read_dir(a.root().join("images").join(format!("{one}.info"))).unwrap().count();
        let mut plan = AddPlan::items(&a, b.root(), vec![two.clone(), one.clone()]);
        plan.now = 1_000;
        let (copied, report) = run(&plan, &mut b);
        assert_eq!(copied.copied, [two.clone(), one.clone()]);
        assert_eq!(report.items, 2);
        let got = b.item(&one).unwrap();
        assert!(got.folders.is_empty());
        assert_eq!(got.modification_time, 1_001);
        assert_eq!(b.item(&two).unwrap().modification_time, 1_000);
        assert_eq!(fs::read_dir(b.root().join("images").join(format!("{one}.info"))).unwrap().count(), before);
        // The source is untouched.
        assert_eq!(a.item(&one).unwrap().folders, [f]);
        // Reopened from disk, the other library has the same items.
        let reopened = Library::open(b.root()).unwrap();
        assert_eq!(reopened.items().len(), 2);
        assert!(reopened.item(&one).unwrap().folders.is_empty());
        // No temporary folders are left.
        assert!(fs::read_dir(b.root().join("images")).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().starts_with('.')));
    }

    #[test]
    fn items_already_there_are_left_alone() {
        let d = tempfile::tempdir().unwrap();
        let mut a = library(d.path(), "A.library");
        let mut b = library(d.path(), "B.library");
        let one = import(&mut a, "one.txt", vec![]);
        let plan = AddPlan::items(&a, b.root(), vec![one.clone()]);
        run(&plan, &mut b);
        let stamp = b.item(&one).unwrap().modification_time;
        let mut again = AddPlan::items(&a, b.root(), vec![one.clone()]);
        again.now += 50_000;
        let (copied, report) = run(&again, &mut b);
        assert_eq!(copied.present, std::slice::from_ref(&one));
        assert_eq!(report.items, 0);
        assert_eq!(b.item(&one).unwrap().modification_time, stamp);
    }

    #[test]
    fn cancel_stops_before_the_next_item() {
        let d = tempfile::tempdir().unwrap();
        let mut a = library(d.path(), "A.library");
        let b = library(d.path(), "B.library");
        let one = import(&mut a, "one.txt", vec![]);
        let two = import(&mut a, "two.txt", vec![]);
        let plan = AddPlan::items(&a, b.root(), vec![one.clone(), two]);
        let cancel = AtomicBool::new(false);
        let copied = copy_items(&plan, &cancel, |_, _| cancel.store(true, Ordering::Relaxed));
        assert_eq!(copied.copied, [one]);
    }

    #[test]
    fn a_damaged_item_fails_without_leaving_anything() {
        let d = tempfile::tempdir().unwrap();
        let mut a = library(d.path(), "A.library");
        let mut b = library(d.path(), "B.library");
        let one = import(&mut a, "one.txt", vec![]);
        fs::write(a.root().join("images").join(format!("{one}.info")).join("metadata.json"), b"{").unwrap();
        let plan = AddPlan::items(&a, b.root(), vec![one.clone()]);
        let (copied, report) = run(&plan, &mut b);
        assert_eq!(copied.failed.len(), 1);
        assert_eq!(report.items, 0);
        assert_eq!(fs::read_dir(b.root().join("images")).unwrap().count(), 0);
    }

    #[test]
    fn a_folder_goes_to_the_top_with_its_items_and_their_folders() {
        let d = tempfile::tempdir().unwrap();
        let mut a = library(d.path(), "A.library");
        let mut b = library(d.path(), "B.library");
        let existing = b.create_folder(None, "Existing").unwrap();
        let top = a.create_folder(None, "Top").unwrap();
        let sub = a.create_folder(Some(&top), "Sub").unwrap();
        let other = a.create_folder(None, "Other").unwrap();
        let in_sub = import(&mut a, "sub.txt", vec![sub.clone(), other.clone()]);
        let outside = import(&mut a, "out.txt", vec![other.clone()]);
        let trashed = import(&mut a, "gone.txt", vec![top.clone()]);
        a.set_trashed(std::slice::from_ref(&trashed), true).unwrap();
        let plan = AddPlan::folder(&a, b.root(), &top).unwrap();
        assert_eq!(plan.items, std::slice::from_ref(&in_sub));
        let (_, report) = run(&plan, &mut b);
        assert!(report.definition);
        assert_eq!(report.items, 1);
        let ids: Vec<&str> = b.metadata.folders.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ids, [top.as_str(), existing.as_str()]);
        assert_eq!(b.metadata.folders[0].children[0].id, sub);
        // `Other` isn't in the other library, so the item drops it.
        assert_eq!(b.item(&in_sub).unwrap().folders, std::slice::from_ref(&sub));
        assert!(b.item(&outside).is_none());
        // Adding the same folder again adds no second copy.
        let again = AddPlan::folder(&a, b.root(), &top).unwrap();
        let (_, report) = run(&again, &mut b);
        assert!(!report.definition);
        assert_eq!(b.metadata.folders.len(), 2);
        let reopened = Library::open(b.root()).unwrap();
        assert_eq!(reopened.metadata.folders[0].id, top);
    }

    #[test]
    fn a_manually_sorted_folder_keeps_the_items_times() {
        let d = tempfile::tempdir().unwrap();
        let mut a = library(d.path(), "A.library");
        let mut b = library(d.path(), "B.library");
        let top = a.create_folder(None, "Top").unwrap();
        let one = import(&mut a, "one.txt", vec![top.clone()]);
        let stamp = a.item(&one).unwrap().modification_time;
        let mut plan = AddPlan::folder(&a, b.root(), &top).unwrap();
        if let Extra::Folder(f) = &mut plan.extra {
            f.order_by = Some("MANUAL".into());
        }
        plan.now = stamp + 99_999;
        run(&plan, &mut b);
        assert_eq!(b.item(&one).unwrap().modification_time, stamp);
    }

    #[test]
    fn smart_folders_and_tag_groups_get_new_ids_at_the_top() {
        let d = tempfile::tempdir().unwrap();
        let a = {
            let mut a = library(d.path(), "A.library");
            a.metadata.smart_folders.push(serde_json::json!({"id": "S1", "name": "Big", "conditions": [], "children": [{"id": "S2", "name": "Kid", "conditions": [], "children": []}]}));
            a.metadata.tags_groups.push(TagGroup { id: "G1".into(), name: "Group".into(), tags: vec!["x".into()], color: None, description: None, extra: Default::default() });
            a.save_metadata().unwrap();
            a
        };
        let mut b = library(d.path(), "B.library");
        b.metadata.tags_groups.push(TagGroup { id: "G0".into(), name: "Mine".into(), tags: vec![], color: None, description: None, extra: Default::default() });
        b.save_metadata().unwrap();
        let plan = AddPlan::smart_folder(&a, b.root(), "S1").unwrap();
        assert!(run(&plan, &mut b).1.definition);
        let sf = &b.metadata.smart_folders[0];
        assert_eq!(sf["name"], "Big");
        assert_ne!(sf["id"], "S1");
        assert_ne!(sf["children"][0]["id"], "S2");
        assert_eq!(sf["children"][0]["name"], "Kid");
        let plan = AddPlan::tag_group(&a, b.root(), "G1").unwrap();
        run(&plan, &mut b);
        assert_eq!(b.metadata.tags_groups[0].name, "Group");
        assert_ne!(b.metadata.tags_groups[0].id, "G1");
        assert_eq!(b.metadata.tags_groups[1].id, "G0");
        assert!(AddPlan::smart_folder(&a, b.root(), "nope").is_none());
        // A subfolder can be added on its own.
        assert!(AddPlan::smart_folder(&a, b.root(), "S2").is_some());
    }
}
