//! Loading and saving whole libraries.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::{Error, Result};
use crate::model::{Folder, Item, LibraryMetadata, MtimeIndex, SavedFilter, Tags};
use crate::undo::{Committed, Dirty, Transaction, WriteMode};

const METADATA: &str = "metadata.json";
const MTIME: &str = "mtime.json";
const TAGS: &str = "tags.json";
const FILTERS: &str = "saved-filters.json";
const IMAGES: &str = "images";

/// The Eagle release whose library format falcon reads and writes. A
/// library whose `applicationVersion` is newer is refused, as that Eagle
/// refuses it.
pub const APP_VERSION: &str = "4.0.0";

/// An Eagle library loaded into memory (item payload files stay on disk).
#[derive(Debug, Clone)]
pub struct Library {
    root: PathBuf,
    /// `metadata.json`.
    pub metadata: LibraryMetadata,
    /// `mtime.json`.
    pub mtime: MtimeIndex,
    /// `tags.json`.
    pub tags: Tags,
    /// `saved-filters.json`, in menu order.
    pub saved_filters: Vec<SavedFilter>,
    items: Vec<Item>,
    /// Item folders whose `metadata.json` couldn't be read or parsed (for
    /// example zeroed by a sync tool), with the reason. They are left out
    /// and never written to.
    pub skipped: Vec<(PathBuf, String)>,
    /// Last `modificationTime` handed out by an import, to keep them strictly increasing.
    last_import_ms: u64,
    /// The open undo transaction, if any (see [`Library::begin_transaction`]).
    pub(crate) journal: Option<Transaction>,
    /// `metadata`, `tags` and `saved_filters` as last saved, so a transaction
    /// can record what an edit replaced.
    pub(crate) committed: Committed,
    pub(crate) write_mode: WriteMode,
    /// Writes held back in [`WriteMode::Batched`] until [`Library::flush`].
    pub(crate) dirty: Dirty,
    /// `modificationTime` of `metadata.json` as falcon last read or wrote
    /// it. A file on disk with a later one was changed by someone else.
    pub(crate) disk_stamp: u64,
    /// Set when a `metadata.json` write was refused (see
    /// [`Library::take_conflict`]).
    conflict: bool,
}

impl Library {
    /// Loads the library at `path`. Items are sorted by id.
    pub fn open(path: impl AsRef<Path>) -> Result<Library> {
        Library::open_with_progress(path, &|_, _| {})
    }

    /// [`Library::open`], calling `progress(read, total)` as item folders
    /// are read (first with 0 once the folder list is known), for a
    /// loading screen.
    pub fn open_with_progress(path: impl AsRef<Path>, progress: &dyn Fn(usize, usize)) -> Result<Library> {
        let root = path.as_ref().to_path_buf();
        if !root.join(METADATA).is_file() || !root.join(IMAGES).is_dir() {
            return Err(Error::NotALibrary(root));
        }
        let metadata: LibraryMetadata = read_json(&root.join(METADATA))?;
        if is_newer_than_app(&metadata.application_version) {
            return Err(Error::NewerLibrary { path: root, version: metadata.application_version });
        }
        let mtime = read_json(&root.join(MTIME))?;
        let tags: Tags = read_json(&root.join(TAGS))?;
        let saved_filters: Vec<SavedFilter> = read_json(&root.join(FILTERS))?;

        let images = root.join(IMAGES);
        let mut dirs = Vec::new();
        for entry in fs::read_dir(&images).map_err(|e| Error::io(&images, e))? {
            let entry = entry.map_err(|e| Error::io(&images, e))?;
            if entry.file_name().to_string_lossy().ends_with(".info") && entry.path().is_dir() {
                dirs.push(entry.path());
            }
        }
        let total = dirs.len();
        progress(0, total);
        let mut items = Vec::new();
        let mut skipped = Vec::new();
        for (n, dir) in dirs.into_iter().enumerate() {
            // One damaged item must not make the whole library unreadable.
            let file = dir.join(METADATA);
            match fs::read_to_string(&file).map_err(|e| e.to_string()).and_then(|t| Item::from_json_str(&t).map_err(|e| e.to_string())) {
                Ok(item) => items.push(item),
                Err(e) => skipped.push((dir, e)),
            }
            if (n + 1) % 200 == 0 || n + 1 == total {
                progress(n + 1, total);
            }
        }
        skipped.sort();
        items.sort_by(|a, b| a.id.cmp(&b.id));
        let committed = Committed { metadata: metadata.clone(), tags: tags.clone(), saved_filters: saved_filters.clone() };
        let disk_stamp = metadata.modification_time;
        Ok(Library {
            root,
            metadata,
            mtime,
            tags,
            saved_filters,
            items,
            skipped,
            last_import_ms: 0,
            journal: None,
            committed,
            write_mode: WriteMode::Immediate,
            dirty: Dirty::default(),
            disk_stamp,
            conflict: false,
        })
    }

    /// Creates an empty library at `path` (normally ending in `.library`)
    /// with the files Eagle expects, and opens it. Refuses a folder that
    /// already exists.
    pub fn create(path: impl AsRef<Path>) -> Result<Library> {
        let root = path.as_ref().to_path_buf();
        if root.exists() {
            return Err(Error::Io {
                path: root.clone(),
                source: std::io::Error::new(std::io::ErrorKind::AlreadyExists, "already exists"),
            });
        }
        mkdir(&root.join(IMAGES))?;
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        let metadata = LibraryMetadata {
            folders: Vec::new(),
            smart_folders: Vec::new(),
            quick_access: Vec::new(),
            tags_groups: Vec::new(),
            modification_time: now,
            application_version: APP_VERSION.into(),
            extra: Default::default(),
        };
        write_json(&root.join(METADATA), &metadata)?;
        write_json(&root.join(MTIME), &MtimeIndex { entries: Vec::new(), all: Some(0) })?;
        write_json(&root.join(TAGS), &Tags::default())?;
        write_json(&root.join(FILTERS), &Vec::<SavedFilter>::new())?;
        Library::open(root)
    }

    /// The library's root folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// All items (including trashed ones), sorted by id.
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// Looks up an item by id.
    pub fn item(&self, id: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }

    /// Mutable access to the items, for edits before [`Library::save_to`].
    pub fn items_mut(&mut self) -> &mut Vec<Item> {
        &mut self.items
    }

    /// Every folder in the tree, depth first.
    pub fn all_folders(&self) -> Vec<&Folder> {
        let mut out = Vec::new();
        for f in &self.metadata.folders {
            f.walk(&mut |x| out.push(x));
        }
        out
    }

    /// Ids of all folders in the tree.
    pub fn folder_ids(&self) -> HashSet<&str> {
        self.all_folders().into_iter().map(|f| f.id.as_str()).collect()
    }

    /// Info directory of `item` in this library.
    pub fn info_dir(&self, item: &Item) -> PathBuf {
        item.info_dir(&self.root)
    }

    /// Writes the library to `dest` (created if needed), copying each item's
    /// files and any other root entries (`backup/`, `Icon`, ...) unchanged,
    /// and regenerating the JSON files from the in-memory model.
    pub fn save_to(&self, dest: impl AsRef<Path>) -> Result<()> {
        let dest = dest.as_ref();
        mkdir(dest)?;
        let same = |a: &Path, b: &Path| matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b);
        if same(dest, &self.root) {
            return Err(Error::SaveOntoSelf(dest.to_path_buf()));
        }

        // Unmodelled root entries are copied verbatim.
        let modelled = [METADATA, MTIME, TAGS, FILTERS, IMAGES];
        for entry in fs::read_dir(&self.root).map_err(|e| Error::io(&self.root, e))? {
            let entry = entry.map_err(|e| Error::io(&self.root, e))?;
            if !modelled.iter().any(|m| entry.file_name() == *m) {
                copy_recursive(&entry.path(), &dest.join(entry.file_name()))?;
            }
        }

        write_json(&dest.join(METADATA), &self.metadata)?;
        write_json(&dest.join(MTIME), &self.mtime)?;
        write_json(&dest.join(TAGS), &self.tags)?;
        write_json(&dest.join(FILTERS), &self.saved_filters)?;

        let images = dest.join(IMAGES);
        mkdir(&images)?;
        for item in &self.items {
            let src = self.info_dir(item);
            let out = item.info_dir(dest);
            mkdir(&out)?;
            if src.is_dir() {
                for entry in fs::read_dir(&src).map_err(|e| Error::io(&src, e))? {
                    let entry = entry.map_err(|e| Error::io(&src, e))?;
                    if entry.file_name() != METADATA {
                        copy_recursive(&entry.path(), &out.join(entry.file_name()))?;
                    }
                }
            }
            write_json(&out.join(METADATA), item)?;
        }
        // Items that couldn't be read are copied as they are.
        for (dir, _) in &self.skipped {
            if let Some(name) = dir.file_name() {
                copy_recursive(dir, &images.join(name))?;
            }
        }
        Ok(())
    }
}

impl Library {
    /// An import timestamp (ms) that is at least `now` and strictly greater
    /// than the previous one handed out.
    pub(crate) fn next_import_time(&mut self, now: u64) -> u64 {
        self.last_import_ms = now.max(self.last_import_ms + 1);
        self.last_import_ms
    }

    /// Drops an item from memory and `mtime.json` (its files are already
    /// gone). This can't be undone, so an open transaction is marked
    /// irreversible.
    pub(crate) fn forget_item(&mut self, id: &str) -> Result<()> {
        if let Some(j) = &mut self.journal {
            j.irreversible = true;
        }
        self.dirty.items.remove(id);
        self.items.retain(|i| i.id != id);
        self.mtime.entries.retain(|(e, _)| e != id);
        self.mtime.all = Some(self.items.len() as u64);
        self.write_mtime()
    }

    /// Writes `item`'s `metadata.json` in place, adds it to the in-memory
    /// list (kept sorted by id) and records it in `mtime.json`, whose `all`
    /// becomes the item count. An open transaction records the item as it
    /// was (or as new).
    pub(crate) fn register_item(&mut self, item: Item) -> Result<()> {
        self.journal_item(&item.id);
        match self.write_mode {
            WriteMode::Immediate => write_json(&item.info_dir(&self.root).join(METADATA), &item)?,
            WriteMode::Batched => {
                self.dirty.items.insert(item.id.clone());
            }
        }
        let stamp = item.last_modified;
        match self.mtime.entries.iter_mut().find(|(id, _)| *id == item.id) {
            Some(entry) => entry.1 = stamp,
            None => self.mtime.entries.push((item.id.clone(), stamp)),
        }
        match self.items.binary_search_by(|i| i.id.cmp(&item.id)) {
            Ok(pos) => self.items[pos] = item,
            Err(pos) => self.items.insert(pos, item),
        }
        self.mtime.all = Some(self.items.len() as u64);
        self.write_mtime()
    }

    /// Writes the in-memory `metadata` to `metadata.json`, the one place
    /// that file is written while the library is open. Like Eagle, it
    /// first reads the file on disk and refuses (writing nothing) when it
    /// was made by a newer Eagle or its `modificationTime` is later than
    /// the one falcon last read or wrote, which means another app or a
    /// sync tool changed it meanwhile ([`Error::SyncConflict`]). A file
    /// that can't be read or parsed is overwritten.
    pub(crate) fn write_metadata_file(&mut self) -> Result<()> {
        let path = self.root.join(METADATA);
        if let Some(disk) = fs::read(&path).ok().and_then(|b| serde_json::from_slice::<DiskStamp>(&b).ok()) {
            if disk.application_version.as_deref().is_some_and(is_newer_than_app) {
                self.conflict = true;
                return Err(Error::NewerLibrary { path: self.root.clone(), version: disk.application_version.unwrap_or_default() });
            }
            if disk.modification_time.is_some_and(|t| t > self.disk_stamp) {
                self.conflict = true;
                return Err(Error::SyncConflict(self.root.clone()));
            }
        }
        write_json(&path, &self.metadata)?;
        self.disk_stamp = self.metadata.modification_time;
        Ok(())
    }

    /// Whether a `metadata.json` write was refused since the last call
    /// because the file on disk changed under falcon (or a newer Eagle
    /// wrote it). The app then reloads the library from disk, as Eagle
    /// does; any caller of the library (the HTTP API included) can trip it.
    pub fn take_conflict(&mut self) -> bool {
        std::mem::take(&mut self.conflict)
    }

    /// Writes `mtime.json`, or marks it dirty in batched mode.
    pub(crate) fn write_mtime(&mut self) -> Result<()> {
        match self.write_mode {
            WriteMode::Immediate => write_json(&self.root.join(MTIME), &self.mtime),
            WriteMode::Batched => {
                self.dirty.mtime = true;
                Ok(())
            }
        }
    }
}

impl Item {
    /// `<library>/images/<id>.info`.
    pub fn info_dir(&self, library_root: &Path) -> PathBuf {
        library_root.join(IMAGES).join(format!("{}.info", self.id))
    }

    /// Path of the original file, `<name>.<ext>` inside the info dir.
    pub fn original_path(&self, library_root: &Path) -> PathBuf {
        let file = if self.ext.is_empty() {
            self.name.clone()
        } else {
            format!("{}.{}", self.name, self.ext)
        };
        self.info_dir(library_root).join(file)
    }

    /// Path of `<name>_thumbnail.png`, if that file exists.
    pub fn thumbnail_path(&self, library_root: &Path) -> Option<PathBuf> {
        let p = self.info_dir(library_root).join(format!("{}_thumbnail.png", self.name));
        p.is_file().then_some(p)
    }
}

/// The two keys of `metadata.json` checked before it is overwritten.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DiskStamp {
    modification_time: Option<u64>,
    application_version: Option<String>,
}

/// Whether a library written by Eagle `version` is too new for falcon.
/// Unparseable versions are not refused.
pub fn is_newer_than_app(version: &str) -> bool {
    compare_versions(APP_VERSION, version) == Some(std::cmp::Ordering::Less)
}

/// Orders two version strings the way Eagle's version check does: an
/// optional leading `v`, dot-separated numbers (missing ones count as 0),
/// then an optional `-prerelease` that sorts before the release itself
/// (its dot-separated parts compare as numbers when both are numeric, else
/// as text, and fewer parts sort first). `+build` is ignored. `None` when
/// either isn't a version.
pub fn compare_versions(a: &str, b: &str) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    fn parse(v: &str) -> Option<(Vec<u64>, Option<Vec<&str>>)> {
        let v = v.trim();
        let v = v.strip_prefix('v').unwrap_or(v);
        let v = v.split('+').next()?;
        let (main, pre) = match v.split_once('-') {
            Some((m, p)) => (m, Some(p.split('.').collect::<Vec<_>>())),
            None => (v, None),
        };
        let nums = main.split('.').map(|p| p.parse::<u64>().ok()).collect::<Option<Vec<_>>>()?;
        (!nums.is_empty() && nums.len() <= 4).then_some((nums, pre))
    }
    let (na, pa) = parse(a)?;
    let (nb, pb) = parse(b)?;
    for i in 0..na.len().max(nb.len()) {
        match na.get(i).unwrap_or(&0).cmp(nb.get(i).unwrap_or(&0)) {
            Ordering::Equal => {}
            o => return Some(o),
        }
    }
    Some(match (pa, pb) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => {
            for (p, q) in x.iter().zip(&y) {
                let o = match (p.parse::<u64>(), q.parse::<u64>()) {
                    (Ok(m), Ok(n)) => m.cmp(&n),
                    _ => p.cmp(q),
                };
                if o != Ordering::Equal {
                    return Some(o);
                }
            }
            x.len().cmp(&y.len())
        }
    })
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).map_err(|e| Error::io(path, e))?;
    serde_json::from_slice(&bytes).map_err(|e| Error::json(path, e))
}

/// Eagle writes compact JSON with no trailing newline.
pub(crate) fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|e| Error::json(path, e))?;
    fs::write(path, bytes).map_err(|e| Error::io(path, e))
}

fn mkdir(p: &Path) -> Result<()> {
    fs::create_dir_all(p).map_err(|e| Error::io(p, e))
}

fn copy_recursive(src: &Path, dst: &Path) -> Result<()> {
    if src.is_dir() {
        mkdir(dst)?;
        for entry in fs::read_dir(src).map_err(|e| Error::io(src, e))? {
            let entry = entry.map_err(|e| Error::io(src, e))?;
            copy_recursive(&entry.path(), &dst.join(entry.file_name()))?;
        }
    } else {
        fs::copy(src, dst).map_err(|e| Error::io(src, e))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering::*;

    #[test]
    fn versions_compare_like_eagle() {
        assert_eq!(compare_versions("4.0.0", "4.0.0"), Some(Equal));
        assert_eq!(compare_versions("4.0.0", "4.0.1"), Some(Less));
        assert_eq!(compare_versions("4.0.0", "4.1"), Some(Less));
        assert_eq!(compare_versions("4.0.0", "4"), Some(Equal));
        assert_eq!(compare_versions("4.0.0", "3.0.10"), Some(Greater));
        assert_eq!(compare_versions("4.0.0", "10.0.0"), Some(Less), "numeric, not text");
        assert_eq!(compare_versions("4.0.0", "4.0.0-beta.2"), Some(Greater));
        assert_eq!(compare_versions("4.0.0-beta.10", "4.0.0-beta.2"), Some(Greater));
        assert_eq!(compare_versions("4.0.0-alpha", "4.0.0-beta"), Some(Less));
        assert_eq!(compare_versions("v4.0.0", "4.0.0+build7"), Some(Equal));
        assert_eq!(compare_versions("4.0.0", "build 20"), None);
        assert_eq!(compare_versions("4.0.0", ""), None);
        assert!(is_newer_than_app("4.0.1"));
        assert!(is_newer_than_app("5.0.0-beta"));
        assert!(!is_newer_than_app("4.0.0"));
        assert!(!is_newer_than_app("3.0.0"));
        assert!(!is_newer_than_app("nonsense"));
    }

    fn set_disk_metadata(root: &Path, edit: impl FnOnce(&mut serde_json::Value)) {
        let path = root.join(METADATA);
        let mut v: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        edit(&mut v);
        fs::write(path, serde_json::to_vec(&v).unwrap()).unwrap();
    }

    #[test]
    fn reports_loading_progress() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("P.library");
        let mut lib = Library::create(&root).unwrap();
        for n in 0..3 {
            let src = dir.path().join(format!("{n}.txt"));
            fs::write(&src, b"x").unwrap();
            lib.import_file(&src, &crate::import::ImportOptions::default()).unwrap();
        }
        let seen = std::sync::Mutex::new(Vec::new());
        let again = Library::open_with_progress(&root, &|a, b| seen.lock().unwrap().push((a, b))).unwrap();
        assert_eq!(again.items().len(), 3);
        assert_eq!(seen.into_inner().unwrap(), vec![(0, 3), (3, 3)]);
    }

    #[test]
    fn refuses_a_library_from_a_newer_eagle() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("New.library");
        Library::create(&root).unwrap();
        set_disk_metadata(&root, |v| v["applicationVersion"] = "4.1.0".into());
        match Library::open(&root) {
            Err(Error::NewerLibrary { version, .. }) => assert_eq!(version, "4.1.0"),
            other => panic!("expected NewerLibrary, got {other:?}"),
        }
        set_disk_metadata(&root, |v| v["applicationVersion"] = "3.0.5".into());
        assert!(Library::open(&root).is_ok());
    }

    #[test]
    fn metadata_writes_refuse_a_newer_copy_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Sync.library");
        let mut lib = Library::create(&root).unwrap();
        lib.create_folder(None, "Mine").unwrap();
        // Our own writes never conflict with themselves.
        lib.create_folder(None, "Mine too").unwrap();
        // Another device saves later.
        let later = lib.metadata.modification_time + 60_000;
        set_disk_metadata(&root, |v| {
            v["modificationTime"] = later.into();
            v["folders"] = serde_json::json!([]);
        });
        let before = fs::read(root.join(METADATA)).unwrap();
        assert!(matches!(lib.create_folder(None, "Lost"), Err(Error::SyncConflict(_))));
        assert!(lib.take_conflict());
        assert!(!lib.take_conflict(), "taken once");
        assert_eq!(fs::read(root.join(METADATA)).unwrap(), before, "nothing written");

        // Batched writes are refused at flush, and not retried.
        let mut lib = Library::open(&root).unwrap();
        lib.set_write_mode(WriteMode::Batched).unwrap();
        lib.create_folder(None, "Pending").unwrap();
        set_disk_metadata(&root, |v| v["modificationTime"] = (later + 60_000).into());
        assert!(matches!(lib.flush(), Err(Error::SyncConflict(_))));
        assert!(!lib.has_pending_writes());

        // A newer Eagle's file is never overwritten either.
        let mut lib = Library::open(&root).unwrap();
        set_disk_metadata(&root, |v| v["applicationVersion"] = "9.0.0".into());
        assert!(matches!(lib.create_folder(None, "No"), Err(Error::NewerLibrary { .. })));
    }
}
