//! Merging another Eagle library into the open one (Eagle's Library ▸
//! Import Library).
//!
//! Eagle refuses the library that is open, asks with the other library's
//! top-level folder count and the number of entries in its `images/`, then:
//! - folders: a top-level folder whose id the open library already has is
//!   not added again; its subfolders go through the same test inside the
//!   existing one. New folders are added with their whole subtree.
//! - smart folders and tag groups whose id is already present are skipped,
//!   the rest are appended.
//! - every `images/<id>.info` folder the open library lacks is copied
//!   verbatim; ones it has are left alone. The items keep their folder ids,
//!   which the merged tree now contains.
//!
//! falcon differs in two small ways: it copies only `*.info` folders (not
//! stray files in `images/`), and a new folder's descendants whose ids
//! already exist elsewhere in the open library are merged into those
//! instead of being added a second time, so the tree never holds one id
//! twice. Each item folder is copied under a temporary name and renamed
//! into place, so an interrupted merge never leaves half an item.
//!
//! The copy ([`copy_items`]) needs no lock on the open library and runs on
//! a background thread; [`Library::finish_merge`] then merges the folder
//! tree and registers the copied items.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{Error, Result};
use crate::library::Library;
use crate::model::{Folder, Item, LibraryMetadata};

/// What merging `source` would bring in, read before asking.
#[derive(Debug, Clone)]
pub struct MergePlan {
    pub source: PathBuf,
    /// The other library's `metadata.json`.
    pub metadata: LibraryMetadata,
    /// Its `images/<id>.info` folder names, sorted.
    pub item_dirs: Vec<String>,
}

impl MergePlan {
    /// The counts Eagle's confirmation shows: top-level folders, and items.
    pub fn counts(&self) -> (usize, usize) {
        (self.metadata.folders.len(), self.item_dirs.len())
    }
}

/// Why a merge can't start.
#[derive(Debug, thiserror::Error)]
pub enum MergeRefusal {
    #[error("this is the library that is open; choose another one")]
    SameLibrary,
    #[error("no Eagle library data (metadata.json) was found there")]
    NotALibrary,
    #[error(transparent)]
    Library(#[from] Error),
}

/// Reads `source` to merge into `dest`.
pub fn plan(dest: &Library, source: &Path) -> std::result::Result<MergePlan, MergeRefusal> {
    let same = |a: &Path, b: &Path| a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y);
    if same(dest.root(), source) {
        return Err(MergeRefusal::SameLibrary);
    }
    let meta_path = source.join("metadata.json");
    if !meta_path.is_file() {
        return Err(MergeRefusal::NotALibrary);
    }
    let bytes = fs::read(&meta_path).map_err(|e| Error::io(&meta_path, e))?;
    let metadata: LibraryMetadata = serde_json::from_slice(&bytes).map_err(|_| MergeRefusal::NotALibrary)?;
    if crate::library::is_newer_than_app(&metadata.application_version) {
        return Err(Error::NewerLibrary { path: source.to_path_buf(), version: metadata.application_version }.into());
    }
    let images = source.join("images");
    let mut item_dirs = Vec::new();
    if images.is_dir() {
        for entry in fs::read_dir(&images).map_err(|e| Error::io(&images, e))? {
            let entry = entry.map_err(|e| Error::io(&images, e))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".info") && entry.path().is_dir() {
                item_dirs.push(name);
            }
        }
    }
    item_dirs.sort();
    Ok(MergePlan { source: source.to_path_buf(), metadata, item_dirs })
}

/// Copies the plan's item folders that `dest_root` lacks into its
/// `images/`, calling `progress(done, total)` after each, and stops early
/// when `cancel` is set. Returns the folder names copied. A folder that
/// fails to copy is skipped (its partial copy removed) and reported in the
/// second list with the reason.
pub fn copy_items(dest_root: &Path, plan: &MergePlan, cancel: &AtomicBool, mut progress: impl FnMut(usize, usize)) -> (Vec<String>, Vec<(String, String)>) {
    let images = dest_root.join("images");
    let total = plan.item_dirs.len();
    let (mut copied, mut failed) = (Vec::new(), Vec::new());
    for (i, name) in plan.item_dirs.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let target = images.join(name);
        if !target.exists() {
            let tmp = images.join(format!(".{name}.merging"));
            let _ = fs::remove_dir_all(&tmp);
            match copy_dir(&plan.source.join("images").join(name), &tmp).and_then(|()| fs::rename(&tmp, &target)) {
                Ok(()) => copied.push(name.clone()),
                Err(e) => {
                    let _ = fs::remove_dir_all(&tmp);
                    failed.push((name.clone(), e.to_string()));
                }
            }
        }
        progress(i + 1, total);
    }
    (copied, failed)
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

/// What a finished merge added.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MergeReport {
    pub folders: usize,
    pub smart_folders: usize,
    pub tag_groups: usize,
    pub items: usize,
}

impl Library {
    /// Merges the plan's folders, smart folders and tag groups into this
    /// library (saving `metadata.json`), and registers the item folders
    /// `copied` by [`copy_items`] in memory and `mtime.json`. Copied items
    /// that can't be read are left on disk and counted out.
    pub fn finish_merge(&mut self, plan: &MergePlan, copied: &[String]) -> Result<MergeReport> {
        let mut report = MergeReport::default();
        let mut known: HashSet<String> = self.folder_ids().into_iter().map(String::from).collect();
        for folder in &plan.metadata.folders {
            report.folders += merge_folder(&mut self.metadata.folders, None, folder.clone(), &mut known);
        }
        let smart_id = |v: &serde_json::Value| v.get("id").and_then(|i| i.as_str()).map(String::from);
        let have: HashSet<String> = self.metadata.smart_folders.iter().filter_map(smart_id).collect();
        for sf in &plan.metadata.smart_folders {
            if smart_id(sf).is_none_or(|id| !have.contains(&id)) {
                self.metadata.smart_folders.push(sf.clone());
                report.smart_folders += 1;
            }
        }
        let have: HashSet<String> = self.metadata.tags_groups.iter().map(|g| g.id.clone()).collect();
        for g in &plan.metadata.tags_groups {
            if !have.contains(&g.id) {
                self.metadata.tags_groups.push(g.clone());
                report.tag_groups += 1;
            }
        }
        self.save_metadata()?;

        for name in copied {
            let id = name.trim_end_matches(".info");
            let file = self.root().join("images").join(name).join("metadata.json");
            let Ok(item) = fs::read_to_string(&file).map_err(|_| ()).and_then(|t| Item::from_json_str(&t).map_err(|_| ())) else { continue };
            if item.id != id || self.item(id).is_some() {
                continue;
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

/// Merges `folder` into `tree` under `parent` (None: top level). A folder
/// whose id the tree already has is not added; its children are merged
/// into the existing folder of that id, wherever it is. A new folder is
/// added with its subtree, minus descendants whose ids are already known,
/// which are merged the same way. Returns how many folders were added (a
/// subtree counts as one).
fn merge_folder(tree: &mut Vec<Folder>, parent: Option<&str>, mut folder: Folder, known: &mut HashSet<String>) -> usize {
    if known.contains(&folder.id) {
        let id = folder.id.clone();
        return std::mem::take(&mut folder.children).into_iter().map(|c| merge_folder(tree, Some(&id), c, known)).sum();
    }
    let mut displaced = Vec::new();
    strip_known(&mut folder, known, &mut displaced);
    folder.walk(&mut |f| {
        known.insert(f.id.clone());
    });
    match parent.and_then(|p| find_mut(tree, p)) {
        Some(p) => p.children.push(folder),
        None => tree.push(folder),
    }
    for d in displaced {
        merge_folder(tree, None, d, known);
    }
    1
}

fn strip_known(folder: &mut Folder, known: &HashSet<String>, displaced: &mut Vec<Folder>) {
    let (dup, keep): (Vec<Folder>, Vec<Folder>) = std::mem::take(&mut folder.children).into_iter().partition(|c| known.contains(&c.id));
    displaced.extend(dup);
    folder.children = keep;
    for c in &mut folder.children {
        strip_known(c, known, displaced);
    }
}

fn find_mut<'a>(folders: &'a mut [Folder], id: &str) -> Option<&'a mut Folder> {
    for f in folders {
        if f.id == id {
            return Some(f);
        }
        if let Some(found) = find_mut(&mut f.children, id) {
            return Some(found);
        }
    }
    None
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

    #[test]
    fn refuses_itself_and_non_libraries() {
        let d = tempfile::tempdir().unwrap();
        let lib = library(d.path(), "A.library");
        assert!(matches!(plan(&lib, lib.root()), Err(MergeRefusal::SameLibrary)));
        fs::create_dir(d.path().join("plain")).unwrap();
        assert!(matches!(plan(&lib, &d.path().join("plain")), Err(MergeRefusal::NotALibrary)));
    }

    #[test]
    fn merges_folders_items_and_groups_by_id() {
        let d = tempfile::tempdir().unwrap();
        let mut dest = library(d.path(), "Dest.library");
        let shared = dest.create_folder(None, "Shared").unwrap();
        let mine = import(&mut dest, "mine.txt", vec![shared.clone()]);

        // The other library started as a copy (same folder id), then grew.
        dest.save_to(d.path().join("Other.library")).unwrap();
        let mut other = Library::open(d.path().join("Other.library")).unwrap();
        let sub = other.create_folder(Some(&shared), "Sub").unwrap();
        let fresh = other.create_folder(None, "Fresh").unwrap();
        let theirs = import(&mut other, "theirs.txt", vec![sub.clone(), fresh.clone()]);

        let plan = plan(&dest, other.root()).unwrap();
        assert_eq!(plan.counts(), (2, 2));
        let cancel = AtomicBool::new(false);
        let mut seen = Vec::new();
        let (copied, failed) = copy_items(dest.root(), &plan, &cancel, |a, b| seen.push((a, b)));
        assert!(failed.is_empty());
        assert_eq!(copied, vec![format!("{theirs}.info")], "the shared item stays as it is");
        assert_eq!(seen.last(), Some(&(2, 2)));

        let report = dest.finish_merge(&plan, &copied).unwrap();
        assert_eq!((report.folders, report.items), (2, 1), "Sub inside Shared, and Fresh");
        let shared_f = dest.all_folders().into_iter().find(|f| f.id == shared).unwrap().clone();
        assert_eq!(shared_f.children.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), vec![sub.as_str()]);
        assert_eq!(dest.metadata.folders.len(), 2);
        assert_eq!(dest.item(&theirs).unwrap().folders, vec![sub, fresh]);
        assert!(dest.item(&mine).is_some());
        // Reopening reads the same library.
        let again = Library::open(dest.root()).unwrap();
        assert_eq!(again.items().len(), 2);
        assert_eq!(again.mtime.all, Some(2));
        assert!(!dest.root().join("images").read_dir().unwrap().any(|e| e.unwrap().file_name().to_string_lossy().ends_with(".merging")));
    }

    #[test]
    fn a_new_folder_never_duplicates_an_id() {
        let mut known: HashSet<String> = ["B".to_string()].into();
        let mut tree = vec![Folder::new("X", "X", 0)];
        tree[0].children.push(Folder::new("B", "B", 0));
        let mut a = Folder::new("A", "A", 0);
        let mut b = Folder::new("B", "B again", 0);
        b.children.push(Folder::new("C", "C", 0));
        a.children.push(b);
        assert_eq!(merge_folder(&mut tree, None, a, &mut known), 1);
        let mut ids = Vec::new();
        for f in &tree {
            f.walk(&mut |x| ids.push(x.id.clone()));
        }
        ids.sort();
        assert_eq!(ids, vec!["A", "B", "C", "X"]);
        // C went into the existing B.
        assert_eq!(tree[0].children[0].children[0].id, "C");
    }
}
