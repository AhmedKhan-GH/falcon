//! Picking up changes other apps make to an open library: a sync tool
//! (Dropbox, iCloud, Syncthing), Eagle on another machine, or the falcon
//! CLI.
//!
//! Eagle polls `metadata.json` and `mtime.json` every 4 seconds
//! (`fs.watchFile`). When `metadata.json` changed and its
//! `modificationTime` is later than the one in memory, it takes the new
//! folder tree. When `mtime.json` changed, it compares it with the stamps
//! it knows: ids it hasn't seen are new items and larger stamps are edited
//! ones, whose `metadata.json` it reads again; when the `all` count no
//! longer matches the items it holds, it lists `images/` to find items
//! that were deleted (and new folders missing from the index).
//!
//! Here the polling and file reading happen on the app's thread of choice
//! ([`Probe`], [`read_metadata`], [`read_mtime`]); applying is quick and
//! done with the library locked ([`Library::take_external_metadata`],
//! [`Library::sync_items`]). Applying never writes: the changes are already
//! on disk. falcon's own writes come back through the same path and change
//! nothing, since their stamps are the ones in memory.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::time::SystemTime;

use crate::error::{Error, Result};
use crate::library::Library;
use crate::model::{Item, LibraryMetadata, MtimeIndex};

/// How often Eagle polls the two files.
pub const POLL: std::time::Duration = std::time::Duration::from_secs(4);

/// What a poll of the library folder found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Probe {
    /// `metadata.json` exists (if not, the library is gone or broken).
    pub present: bool,
    /// Modification times of `metadata.json` and `mtime.json`.
    pub metadata: Option<SystemTime>,
    /// `mtime.json`'s time, `None` while it is missing or empty (Eagle
    /// ignores an empty `mtime.json`: it is being rewritten).
    pub mtime: Option<SystemTime>,
}

impl Probe {
    /// Looks at the two files under `root`.
    pub fn of(root: &Path) -> Probe {
        let meta = fs::metadata(root.join("metadata.json")).ok().filter(|m| m.is_file());
        let mtime = fs::metadata(root.join("mtime.json")).ok().filter(|m| m.is_file() && m.len() > 0);
        Probe { present: meta.is_some(), metadata: meta.and_then(|m| m.modified().ok()), mtime: mtime.and_then(|m| m.modified().ok()) }
    }
}

/// Reads and parses `metadata.json` (an unfinished or damaged file is an
/// error; poll again later).
pub fn read_metadata(root: &Path) -> Result<LibraryMetadata> {
    read(&root.join("metadata.json"))
}

/// Reads and parses `mtime.json`.
pub fn read_mtime(root: &Path) -> Result<MtimeIndex> {
    read(&root.join("mtime.json"))
}

fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).map_err(|e| Error::io(path, e))?;
    serde_json::from_slice(&bytes).map_err(|e| Error::json(path, e))
}

/// What [`Library::sync_items`] changed in memory.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ItemSync {
    pub added: Vec<String>,
    pub changed: Vec<String>,
    pub removed: Vec<String>,
}

impl ItemSync {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.changed.is_empty() && self.removed.is_empty()
    }
}

impl Library {
    /// Takes `metadata` read from disk if it is newer than what falcon
    /// last read or wrote (its `modificationTime` is later), as Eagle
    /// does; returns whether it did. A newer folder tree replaces the one
    /// in memory, including edits still waiting in batched mode, and
    /// becomes the base later writes are checked against.
    pub fn take_external_metadata(&mut self, metadata: LibraryMetadata) -> bool {
        if metadata.modification_time <= self.disk_stamp {
            return false;
        }
        self.disk_stamp = metadata.modification_time;
        self.metadata = metadata;
        self.committed.metadata = self.metadata.clone();
        self.dirty.metadata = false;
        true
    }

    /// Brings the items in memory up to date with `index`, the
    /// `mtime.json` just read from disk: items it lists that falcon
    /// doesn't hold are read and added, items with a later stamp are read
    /// again, and when its `all` count differs from the number of items
    /// held, `images/` is listed to drop items whose folder is gone and add
    /// folders the index missed. Items that can't be read yet (a sync still
    /// copying them) are left for the next poll. Nothing is written.
    pub fn sync_items(&mut self, index: &MtimeIndex) -> ItemSync {
        let mut out = ItemSync::default();
        let known: HashMap<String, u64> = self.mtime.entries.iter().cloned().collect();
        for (id, stamp) in &index.entries {
            match known.get(id) {
                Some(&mine) if *stamp <= mine => {}
                had => {
                    if let Some(item) = self.read_item(id) {
                        let new = had.is_none() && self.item(id).is_none();
                        self.adopt(item, *stamp);
                        if new { out.added.push(id.clone()) } else { out.changed.push(id.clone()) }
                    }
                }
            }
        }
        let held = self.items().len() as u64;
        if index.all.is_some_and(|all| all != held) || index.entries.len() as u64 != held {
            let on_disk = self.item_dirs();
            let gone: Vec<String> = self.items().iter().filter(|i| !on_disk.contains(&i.id)).map(|i| i.id.clone()).collect();
            for id in gone {
                self.drop_item(&id);
                out.removed.push(id);
            }
            let mut fresh: Vec<&String> = on_disk.iter().filter(|id| self.item(id).is_none()).collect();
            fresh.sort();
            for id in fresh.into_iter().cloned().collect::<Vec<_>>() {
                if let Some(item) = self.read_item(&id) {
                    let stamp = item.last_modified;
                    self.adopt(item, stamp);
                    out.added.push(id);
                }
            }
        }
        if !out.is_empty() {
            self.mtime.all = Some(self.items().len() as u64);
        }
        out
    }

    /// Drops every write batched mode is holding back, for a library whose
    /// folder is gone: writing them would fail, or put files where the
    /// library used to be.
    pub fn discard_pending_writes(&mut self) {
        self.dirty = Default::default();
    }

    /// The item at `images/<id>.info/metadata.json`, if it reads and is
    /// that id (Eagle's ids are 13 or 36 characters).
    fn read_item(&self, id: &str) -> Option<Item> {
        if id.len() != 13 && id.len() != 36 {
            return None;
        }
        let file = self.root().join("images").join(format!("{id}.info")).join("metadata.json");
        let text = fs::read_to_string(file).ok()?;
        Item::from_json_str(&text).ok().filter(|i| i.id == id)
    }

    /// Ids of the item folders in `images/`.
    fn item_dirs(&self) -> HashSet<String> {
        let Ok(dir) = fs::read_dir(self.root().join("images")) else { return HashSet::new() };
        dir.filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".info").map(String::from))
            .collect()
    }

    /// Puts an item read from disk in memory with its `mtime.json` stamp.
    /// A pending write of the old copy is dropped: the disk copy is newer.
    fn adopt(&mut self, item: Item, stamp: u64) {
        self.dirty.items.remove(&item.id);
        self.skipped.retain(|(dir, _)| !dir.ends_with(format!("{}.info", item.id)));
        match self.mtime.entries.iter_mut().find(|(id, _)| *id == item.id) {
            Some(e) => e.1 = stamp,
            None => self.mtime.entries.push((item.id.clone(), stamp)),
        }
        let items = self.items_mut();
        match items.binary_search_by(|i| i.id.cmp(&item.id)) {
            Ok(pos) => items[pos] = item,
            Err(pos) => items.insert(pos, item),
        }
    }

    /// Forgets an item deleted elsewhere (its folder is already gone).
    fn drop_item(&mut self, id: &str) {
        self.dirty.items.remove(id);
        self.items_mut().retain(|i| i.id != id);
        self.mtime.entries.retain(|(e, _)| e != id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::ImportOptions;

    fn library() -> (tempfile::TempDir, Library) {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::create(dir.path().join("S.library")).unwrap();
        (dir, lib)
    }

    fn import(lib: &mut Library, name: &str) -> String {
        let src = lib.root().parent().unwrap().join(name);
        fs::write(&src, name.as_bytes()).unwrap();
        lib.import_file(&src, &ImportOptions::default()).unwrap()
    }

    #[test]
    fn probe_sees_both_files() {
        let (_d, lib) = library();
        let p = Probe::of(lib.root());
        assert!(p.present && p.metadata.is_some() && p.mtime.is_some());
        fs::write(lib.root().join("mtime.json"), b"").unwrap();
        assert_eq!(Probe::of(lib.root()).mtime, None, "an empty mtime.json is being rewritten");
        assert!(!Probe::of(&lib.root().join("nope")).present);
    }

    #[test]
    fn newer_metadata_from_disk_replaces_the_folder_tree() {
        let (_d, mut lib) = library();
        lib.create_folder(None, "Here").unwrap();
        // Our own write read back changes nothing.
        let mine = read_metadata(lib.root()).unwrap();
        assert!(!lib.take_external_metadata(mine.clone()));
        // Another device adds a folder, later.
        let mut other = Library::open(lib.root()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        other.create_folder(None, "There").unwrap();
        let theirs = read_metadata(lib.root()).unwrap();
        assert!(lib.take_external_metadata(theirs));
        assert_eq!(lib.metadata.folders.len(), 2);
        // Now our next write is not a conflict.
        lib.create_folder(None, "Again").unwrap();
        // An older copy (a stale read) is ignored.
        assert!(!lib.take_external_metadata(mine));
        assert_eq!(lib.metadata.folders.len(), 3);
    }

    #[test]
    fn items_added_edited_and_deleted_elsewhere_are_picked_up() {
        let (_d, mut lib) = library();
        let kept = import(&mut lib, "a.txt");
        let doomed = import(&mut lib, "b.txt");
        // Our own index read back changes nothing.
        assert!(lib.sync_items(&read_mtime(lib.root()).unwrap()).is_empty());

        let mut other = Library::open(lib.root()).unwrap();
        let added = import(&mut other, "c.txt");
        let mut item = other.item(&kept).unwrap().clone();
        item.annotation = "edited elsewhere".into();
        std::thread::sleep(std::time::Duration::from_millis(2));
        other.update_item(item).unwrap();
        other.delete_permanently(std::slice::from_ref(&doomed)).unwrap();

        let got = lib.sync_items(&read_mtime(lib.root()).unwrap());
        assert_eq!(got.added, vec![added.clone()]);
        assert_eq!(got.changed, vec![kept.clone()]);
        assert_eq!(got.removed, vec![doomed.clone()]);
        assert_eq!(lib.item(&kept).unwrap().annotation, "edited elsewhere");
        assert!(lib.item(&doomed).is_none());
        assert_eq!(lib.mtime, other.mtime, "the index in memory matches the disk");
        assert!(lib.sync_items(&read_mtime(lib.root()).unwrap()).is_empty());
    }

    #[test]
    fn a_folder_the_index_missed_is_found_by_the_count() {
        let (_d, mut lib) = library();
        import(&mut lib, "a.txt");
        let mut other = Library::open(lib.root()).unwrap();
        let id = import(&mut other, "b.txt");
        // The sync tool delivered the item folder but an old mtime.json.
        let mut stale = read_mtime(lib.root()).unwrap();
        stale.entries.retain(|(e, _)| *e != id);
        let got = lib.sync_items(&stale);
        assert_eq!(got.added, vec![id]);
        // A half-copied item (unreadable) waits for a later poll.
        let mut third = Library::open(lib.root()).unwrap();
        let late = import(&mut third, "c.txt");
        fs::write(lib.root().join(format!("images/{late}.info/metadata.json")), b"{\"id\":").unwrap();
        assert!(lib.sync_items(&read_mtime(lib.root()).unwrap()).added.is_empty());
    }
}
