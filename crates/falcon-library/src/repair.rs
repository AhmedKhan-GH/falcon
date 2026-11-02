//! Clear Invalid Content: finding item folders that are empty or broken,
//! after Eagle's repair tool of that name.
//!
//! Eagle walks `images/` and counts as invalid:
//! - an `<id>.info` folder that isn't a loaded item and has no
//!   `metadata.json`: the folder is removed;
//! - a loaded item whose original file is gone (whether or not its folder
//!   holds anything else): the item is moved to the Trash (if it isn't
//!   there already). An item whose original exists is never invalid, however
//!   few files its folder holds;
//! - a loaded item whose folder has gone altogether: it drops out when the
//!   library is reloaded.
//!
//! It asks first, warning that a sync tool may not have finished, then
//! cleans up and reloads the library. falcon follows this, with one
//! difference: an orphaned folder is handed to the caller's `discard` (the
//! app moves it to the Trash) instead of being deleted. A folder with a
//! `metadata.json` that can't be read is left alone, as Eagle leaves it
//! (falcon reports such items when the library opens).
//!
//! [`scan`] needs only a copy of the items, so it runs without the library
//! lock; [`Library::clear_invalid`] applies a scan.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::library::Library;
use crate::model::Item;

/// What a scan found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InvalidScan {
    /// `*.info` folders looked at.
    pub scanned: usize,
    /// Folders that are no item and have no `metadata.json`.
    pub orphans: Vec<PathBuf>,
    /// Items whose original is gone, not yet in
    /// the Trash.
    pub broken: Vec<String>,
    /// Items whose folder has gone.
    pub gone: Vec<String>,
}

impl InvalidScan {
    /// How many things the cleanup would deal with.
    pub fn total(&self) -> usize {
        self.orphans.len() + self.broken.len() + self.gone.len()
    }
}

/// Scans the library at `root`, whose loaded items are `items`.
pub fn scan(root: &Path, items: &[Item]) -> InvalidScan {
    let mut out = InvalidScan::default();
    let known: HashSet<&str> = items.iter().map(|i| i.id.as_str()).collect();
    let images = root.join("images");
    if let Ok(entries) = fs::read_dir(&images) {
        let mut dirs: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".info") && e.path().is_dir())
            .map(|e| e.path())
            .collect();
        dirs.sort();
        for dir in dirs {
            out.scanned += 1;
            let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let id = name.trim_end_matches(".info");
            if !known.contains(id) && !dir.join("metadata.json").exists() {
                out.orphans.push(dir);
            }
        }
    }
    for item in items {
        let dir = item.info_dir(root);
        if !dir.is_dir() {
            out.gone.push(item.id.clone());
        } else if !item.is_deleted && !item.original_path(root).exists() {
            out.broken.push(item.id.clone());
        }
    }
    out
}

/// What [`Library::clear_invalid`] did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cleared {
    pub trashed: usize,
    pub removed: usize,
    pub forgotten: usize,
    /// Folders `discard` refused, with the reason.
    pub failed: Vec<(PathBuf, String)>,
}

impl Library {
    /// Applies `scan`: moves its broken items to the Trash (one undoable
    /// step when a transaction is open), forgets items whose folder has
    /// gone, and hands each orphaned folder still there to `discard`.
    pub fn clear_invalid(&mut self, scan: &InvalidScan, mut discard: impl FnMut(&Path) -> std::io::Result<()>) -> Result<Cleared> {
        let mut out = Cleared::default();
        let broken: Vec<String> = scan.broken.iter().filter(|id| self.item(id).is_some_and(|i| !i.is_deleted)).cloned().collect();
        if !broken.is_empty() {
            self.set_trashed(&broken, true)?;
            out.trashed = broken.len();
        }
        for id in &scan.gone {
            if let Some(item) = self.item(id)
                && !item.info_dir(self.root()).exists()
            {
                self.forget_item(id)?;
                out.forgotten += 1;
            }
        }
        for dir in &scan.orphans {
            if !dir.is_dir() || dir.join("metadata.json").exists() || !dir.starts_with(self.root().join("images")) {
                continue;
            }
            match discard(dir) {
                Ok(()) => out.removed += 1,
                Err(e) => out.failed.push((dir.clone(), e.to_string())),
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::ImportOptions;

    fn setup() -> (tempfile::TempDir, Library, Vec<String>) {
        let d = tempfile::tempdir().unwrap();
        let mut lib = Library::create(d.path().join("L.library")).unwrap();
        let mut ids = Vec::new();
        for n in ["a.txt", "b.txt", "c.txt", "d.txt"] {
            let p = d.path().join(n);
            fs::write(&p, n).unwrap();
            ids.push(lib.import_file(&p, &ImportOptions::default()).unwrap());
        }
        (d, lib, ids)
    }

    #[test]
    fn a_healthy_library_has_nothing_invalid() {
        let (_d, lib, _) = setup();
        let s = scan(lib.root(), lib.items());
        assert_eq!(s.scanned, 4);
        assert_eq!(s.total(), 0);
    }

    #[test]
    fn finds_and_clears_each_kind() {
        let (_d, mut lib, ids) = setup();
        let images = lib.root().join("images");
        // A folder that is no item and has no metadata.json.
        fs::create_dir_all(images.join("ORPHAN.info")).unwrap();
        fs::write(images.join("ORPHAN.info/x.png"), b"x").unwrap();
        // A folder with an unreadable metadata.json is left alone.
        fs::create_dir_all(images.join("DAMAGED.info")).unwrap();
        fs::write(images.join("DAMAGED.info/metadata.json"), b"{").unwrap();
        // A missing original.
        let a = lib.item(&ids[0]).unwrap().clone();
        fs::remove_file(a.original_path(lib.root())).unwrap();
        // A folder that has gone.
        let b = lib.item(&ids[1]).unwrap().clone();
        fs::remove_dir_all(b.info_dir(lib.root())).unwrap();
        // A missing original already in the Trash is left alone.
        lib.set_trashed(std::slice::from_ref(&ids[2]), true).unwrap();
        let c = lib.item(&ids[2]).unwrap().clone();
        fs::remove_file(c.original_path(lib.root())).unwrap();

        let s = scan(lib.root(), lib.items());
        assert_eq!(s.orphans, [images.join("ORPHAN.info")]);
        assert_eq!(s.broken, [ids[0].clone()]);
        assert_eq!(s.gone, [ids[1].clone()]);
        assert_eq!(s.total(), 3);

        let mut discarded = Vec::new();
        lib.begin_transaction("Clear Invalid Content");
        let done = lib
            .clear_invalid(&s, |p| {
                discarded.push(p.to_path_buf());
                fs::remove_dir_all(p)
            })
            .unwrap();
        let tx = lib.end_transaction().unwrap();
        assert_eq!(done, Cleared { trashed: 1, removed: 1, forgotten: 1, failed: vec![] });
        assert_eq!(discarded, [images.join("ORPHAN.info")]);
        assert!(lib.item(&ids[0]).unwrap().is_deleted);
        assert!(lib.item(&ids[1]).is_none());
        assert!(images.join("DAMAGED.info").exists());
        assert!(!lib.item(&ids[3]).unwrap().is_deleted);
        // Forgetting a vanished item can't be undone.
        assert!(tx.irreversible);
        let again = scan(lib.root(), lib.items());
        assert_eq!(again.total(), 0);
    }

    #[test]
    fn an_empty_item_folder_counts_as_broken() {
        let (_d, lib, ids) = setup();
        let item = lib.item(&ids[3]).unwrap().clone();
        for e in fs::read_dir(item.info_dir(lib.root())).unwrap() {
            let p = e.unwrap().path();
            if p.file_name().unwrap() != "metadata.json" {
                fs::remove_file(p).unwrap();
            }
        }
        fs::write(item.info_dir(lib.root()).join(".DS_Store"), b"").unwrap();
        assert_eq!(scan(lib.root(), lib.items()).broken, [ids[3].clone()]);
    }

    #[test]
    fn an_item_whose_original_exists_is_never_invalid() {
        let (_d, lib, ids) = setup();
        let item = lib.item(&ids[3]).unwrap().clone();
        let dir = item.info_dir(lib.root());
        let original = item.original_path(lib.root());
        for e in fs::read_dir(&dir).unwrap() {
            let p = e.unwrap().path();
            if p != original {
                fs::remove_file(p).unwrap();
            }
        }
        // Only the original is left: one file, but the original exists.
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        let s = scan(lib.root(), lib.items());
        assert!(s.broken.is_empty());
        assert_eq!(s.total(), 0);
    }

    #[test]
    fn a_refused_discard_is_reported() {
        let (_d, mut lib, _) = setup();
        let orphan = lib.root().join("images/ORPHAN.info");
        fs::create_dir_all(&orphan).unwrap();
        let s = scan(lib.root(), lib.items());
        let done = lib.clear_invalid(&s, |_| Err(std::io::Error::other("no Trash"))).unwrap();
        assert_eq!(done.removed, 0);
        assert_eq!(done.failed.len(), 1);
        assert!(orphan.exists());
    }
}
