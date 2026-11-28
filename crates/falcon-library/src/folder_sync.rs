//! Folder synchronization engine for registered watched folders.
//!
//! Supports both Link (Referenced) Mode and Copy (Managed) Mode with:
//! - Recursive and shallow directory scanning
//! - Subfolder hierarchy preservation in Falcon library
//! - In Link Mode: zero-payload reference imports and, when a source changes,
//!   a refresh of everything derived from it (size, times, dimensions,
//!   thumbnail, palette, embedded metadata, identity) that keeps the item's
//!   id, tags, notes and folders
//! - In Copy Mode: managed copies, never overwritten; a stable record of which
//!   source file each item came from keeps renamed items from importing again
//!   and reports a changed source as a conflict instead of replacing anything
//! - Safe offline/unmounted volume handling (never mass-deletes items)
//! - A status that always ends in a terminal state, also after a failure or a
//!   crash (see [`SyncStatus`])

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::error::{Error, Result};
use crate::fault::{self, FaultPoint};
use crate::import::ImportOptions;
use crate::library::Library;
use crate::linked;
use crate::model::Folder;
use crate::storage_mode::{FalconConfig, StorageMode, SyncSource, SyncStatus, WatchedFolder};

/// How many Copy-mode imports go by between saves of the provenance record.
const PROVENANCE_SAVE_EVERY: usize = 50;

pub(crate) fn find_folder_in<'a>(folders: &'a [Folder], id: &str) -> Option<&'a Folder> {
    for f in folders {
        if f.id == id {
            return Some(f);
        }
        if let Some(found) = find_folder_in(&f.children, id) {
            return Some(found);
        }
    }
    None
}

/// Summary of work done during a folder synchronization pass.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SyncStats {
    /// Number of newly imported assets.
    pub added: usize,
    /// Number of existing linked assets whose modified source was detected and updated.
    pub modified: usize,
    /// Number of unchanged or skipped files.
    pub skipped: usize,
    /// Copy mode: source files that changed after they were imported. The
    /// managed copy is left as it is; nothing is overwritten.
    pub conflicts: usize,
    /// Ids of the items imported in this pass.
    pub added_ids: Vec<String>,
    /// Ids of linked items refreshed in this pass.
    pub refreshed_ids: Vec<String>,
    /// Ids of imported or refreshed items whose thumbnail still has to be
    /// drawn (a background pass defers it to the thumbnail worker).
    pub pending_thumbnail_ids: Vec<String>,
    /// The pass was stopped on request; what was done is kept.
    pub cancelled: bool,
    /// Files left for a later pass because they were modified too recently to
    /// be trusted as complete.
    pub unsettled: usize,
    /// True if the source path was offline or unmounted.
    pub offline: bool,
}

/// File extensions (lower case) of temporary, partial and lock files that are
/// never synchronized.
const TEMPORARY_EXTENSIONS: [&str; 10] =
    ["tmp", "temp", "crdownload", "part", "partial", "download", "lock", "lck", "swp", "swo"];

/// Whether `name` is a temporary, partial, lock or system artifact: a hidden
/// name, an Office lock (`~$…`), an editor backup (`…~`), a download in
/// progress, or one of the usual operating system files. Only the final
/// extension counts, so `design.tmpl.png` is an ordinary file.
pub fn is_temporary_name(name: &str) -> bool {
    if name.starts_with('.') || name.starts_with("~$") || name.ends_with('~') {
        return true;
    }
    let lower = name.to_lowercase();
    if lower == "thumbs.db" || lower == "desktop.ini" {
        return true;
    }
    Path::new(&lower).extension().is_some_and(|e| TEMPORARY_EXTENSIONS.contains(&e.to_string_lossy().as_ref()))
}

/// Checks whether `file_name` should be excluded from sync.
pub fn is_sync_skipped(name: &str, size: u64) -> bool {
    if is_temporary_name(name) {
        return true;
    }
    let ext = Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if ext.is_empty() || ext == "url" || size == 0 {
        return true;
    }
    !crate::import::is_recognized(&ext)
}

pub(crate) fn mtime_ms(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as u64)
}

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Key of a source file in [`FalconConfig::sync_sources`].
pub(crate) fn source_key(wf: &WatchedFolder, file: &Path) -> String {
    let rel = file.strip_prefix(&wf.source_path).unwrap_or(file);
    let rel: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    format!("{}|{}", wf.id, rel.join("/"))
}

impl Library {
    /// Synchronizes a registered watched folder against the current library state.
    ///
    /// A pass that fails part-way, for any reason, leaves the folder marked
    /// [`SyncStatus::Error`] with the reason, never stuck at `Syncing`; running
    /// it again continues without duplicating what was already done.
    pub fn sync_watched_folder(&mut self, wf: &mut WatchedFolder) -> Result<SyncStats> {
        let result = self.sync_watched_folder_pass(wf);
        if let Err(e) = &result
            && wf.status == SyncStatus::Syncing
        {
            wf.status = SyncStatus::Error;
            wf.last_error = Some(e.to_string());
            let _ = self.upsert_watched_folder(wf.clone());
        }
        result
    }

    fn sync_watched_folder_pass(&mut self, wf: &mut WatchedFolder) -> Result<SyncStats> {
        let mut stats = SyncStats::default();

        // Safe offline detection: unmounted volumes or missing directories must
        // never delete or invalidate existing library items.
        if !wf.source_path.exists() || !wf.source_path.is_dir() {
            wf.status = SyncStatus::Offline;
            wf.last_error = Some("Source folder is offline or unreachable".into());
            let _ = self.upsert_watched_folder(wf.clone());
            stats.offline = true;
            return Ok(stats);
        }

        // Validate target library folder exists
        if find_folder_in(&self.metadata.folders, &wf.folder_id).is_none() {
            wf.status = SyncStatus::Error;
            wf.last_error = Some(format!("Target library folder {} not found", wf.folder_id));
            let _ = self.upsert_watched_folder(wf.clone());
            return Err(Error::Refused(format!("Target library folder {} not found", wf.folder_id)));
        }

        wf.status = SyncStatus::Syncing;
        self.upsert_watched_folder(wf.clone())?;
        fault::check(FaultPoint::SyncAfterMarkedSyncing, &wf.source_path)?;

        // Build index of canonical source path -> item_id for linked assets
        let mut linked_by_source: HashMap<PathBuf, String> = HashMap::new();
        for id in &self.config.linked_ids {
            if let Some(src) = self.last_known_source_path(id) {
                let key = fs::canonicalize(&src).unwrap_or(src);
                linked_by_source.insert(key, id.clone());
            }
        }

        // Copy-mode provenance of this watched folder.
        let prefix = format!("{}|", wf.id);
        let mut provenance: BTreeMap<String, SyncSource> =
            self.config.sync_sources.iter().filter(|(k, _)| k.starts_with(&prefix)).map(|(k, v)| (k.clone(), v.clone())).collect();
        let mut claimed: HashSet<String> = provenance.values().map(|s| s.item_id.clone()).collect();
        let mut unsaved = 0usize;

        // Map relative directory paths to Falcon library folder IDs
        let mut dir_map: HashMap<PathBuf, String> = HashMap::new();
        dir_map.insert(PathBuf::new(), wf.folder_id.clone());

        // Traverse directories and replicate hierarchy if recursive
        let mut queue: Vec<PathBuf> = vec![wf.source_path.clone()];
        let mut all_files: Vec<(PathBuf, String)> = Vec::new();

        while let Some(current_dir) = queue.pop() {
            let rel_dir = match current_dir.strip_prefix(&wf.source_path) {
                Ok(r) => r.to_path_buf(),
                Err(_) => PathBuf::new(),
            };

            let current_falcon_id = if let Some(fid) = dir_map.get(&rel_dir) {
                fid.clone()
            } else {
                // Determine parent relative dir and current directory name
                let parent_rel = rel_dir.parent().unwrap_or(Path::new("")).to_path_buf();
                let parent_falcon_id = dir_map.get(&parent_rel).cloned().unwrap_or_else(|| wf.folder_id.clone());
                let dir_name = current_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();

                // Look for existing subfolder in parent
                let existing = find_folder_in(&self.metadata.folders, &parent_falcon_id)
                    .and_then(|p| p.children.iter().find(|c| c.name == dir_name))
                    .map(|c| c.id.clone());

                let fid = match existing {
                    Some(id) => id,
                    None => {
                        fault::check(FaultPoint::SyncBeforeFolderCreate, &current_dir)?;
                        self.create_folder(Some(&parent_falcon_id), &dir_name)?
                    }
                };
                dir_map.insert(rel_dir.clone(), fid.clone());
                fid
            };

            let Ok(entries) = fs::read_dir(&current_dir) else {
                continue;
            };

            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(file_type) = entry.file_type() else { continue };

                if file_type.is_dir() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if wf.recursive && !name.starts_with('.') {
                        queue.push(path);
                    }
                } else if file_type.is_file() {
                    all_files.push((path, current_falcon_id.clone()));
                }
            }
        }
        all_files.sort();

        // Process files
        for (file_path, target_folder_id) in all_files {
            let file_name = file_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let Ok(meta) = fs::metadata(&file_path) else { continue };

            if is_sync_skipped(&file_name, meta.len()) {
                stats.skipped += 1;
                continue;
            }

            match wf.storage_mode {
                StorageMode::Link => {
                    let canonical = fs::canonicalize(&file_path).unwrap_or_else(|_| file_path.clone());
                    if let Some(existing_id) = linked_by_source.get(&canonical).cloned() {
                        // Already registered: check if source has changed
                        let Some(item) = self.item(&existing_id).cloned() else {
                            stats.skipped += 1;
                            continue;
                        };
                        let mtime = mtime_ms(&meta);
                        let rec = self.link_record(&existing_id).ok().flatten();
                        let is_changed = match &rec {
                            Some(r) => r.identity.size != meta.len() || r.identity.mtime_ms != mtime,
                            None => item.size != meta.len(),
                        };
                        if !is_changed {
                            stats.skipped += 1;
                        } else if self.refresh_linked_source(&existing_id, &meta)? {
                            stats.modified += 1;
                            stats.refreshed_ids.push(existing_id);
                        } else {
                            // Changed again while being read: next pass.
                            stats.skipped += 1;
                        }
                    } else {
                        // New file: import as linked asset
                        fault::check(FaultPoint::SyncBeforeImport, &file_path)?;
                        let opts = ImportOptions {
                            folders: vec![target_folder_id],
                            storage_mode: Some(StorageMode::Link),
                            ..Default::default()
                        };
                        match self.import_file(&file_path, &opts) {
                            Ok(new_id) => {
                                linked_by_source.insert(canonical, new_id.clone());
                                stats.added += 1;
                                stats.added_ids.push(new_id);
                            }
                            Err(_) => {
                                stats.skipped += 1;
                            }
                        }
                    }
                }
                StorageMode::Copy => {
                    // One-way copy: a managed original is never overwritten.
                    let key = source_key(wf, &file_path);
                    let mtime = mtime_ms(&meta);
                    match provenance.get(&key).cloned() {
                        Some(prev) => {
                            let alive = self.item(&prev.item_id).is_some_and(|i| !i.is_deleted);
                            if !alive {
                                // Removed from Falcon on purpose: not brought back.
                                stats.skipped += 1;
                            } else if prev.size == meta.len() && prev.mtime_ms == mtime {
                                stats.skipped += 1;
                            } else {
                                let fingerprint = linked::compute_fast_fingerprint(&file_path, meta.len()).ok();
                                if prev.size == meta.len() && fingerprint.is_some() && fingerprint == prev.fingerprint {
                                    // Touched, not changed: remember the new time.
                                    provenance.insert(key, SyncSource { mtime_ms: mtime, ..prev });
                                    unsaved += 1;
                                    stats.skipped += 1;
                                } else {
                                    stats.conflicts += 1;
                                }
                            }
                        }
                        None => {
                            let stem = file_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                            let ext = file_path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                            let fingerprint = linked::compute_fast_fingerprint(&file_path, meta.len()).ok();

                            // An item from before provenance was recorded (or
                            // from a copy of falcon.json without it): adopt it.
                            let adopt = self
                                .items()
                                .iter()
                                .find(|i| {
                                    i.folders.contains(&target_folder_id)
                                        && i.name == stem
                                        && i.ext.eq_ignore_ascii_case(&ext)
                                        && !claimed.contains(&i.id)
                                })
                                .map(|i| i.id.clone());

                            if let Some(id) = adopt {
                                claimed.insert(id.clone());
                                provenance.insert(key, SyncSource { item_id: id, size: meta.len(), mtime_ms: mtime, fingerprint });
                                unsaved += 1;
                                stats.skipped += 1;
                            } else {
                                fault::check(FaultPoint::SyncBeforeImport, &file_path)?;
                                let opts = ImportOptions {
                                    folders: vec![target_folder_id],
                                    storage_mode: Some(StorageMode::Copy),
                                    ..Default::default()
                                };
                                match self.import_file(&file_path, &opts) {
                                    Ok(id) => {
                                        claimed.insert(id.clone());
                                        provenance.insert(key, SyncSource { item_id: id.clone(), size: meta.len(), mtime_ms: mtime, fingerprint });
                                        unsaved += 1;
                                        stats.added += 1;
                                        stats.added_ids.push(id);
                                    }
                                    Err(_) => {
                                        stats.skipped += 1;
                                    }
                                }
                            }
                        }
                    }
                    if unsaved >= PROVENANCE_SAVE_EVERY {
                        self.store_sync_sources(&prefix, &provenance)?;
                        unsaved = 0;
                    }
                }
            }
        }

        if unsaved > 0 {
            self.store_sync_sources(&prefix, &provenance)?;
        }
        fault::check(FaultPoint::SyncBeforeFinalize, &wf.source_path)?;
        wf.last_synced = Some(now_ms());
        wf.status = SyncStatus::Success;
        wf.last_error = None;
        self.upsert_watched_folder(wf.clone())?;

        Ok(stats)
    }

    /// Writes this watched folder's provenance (the entries starting with
    /// `prefix`) to `falcon.json`.
    pub(crate) fn store_sync_sources(&mut self, prefix: &str, provenance: &BTreeMap<String, SyncSource>) -> Result<()> {
        fault::check(FaultPoint::SyncBeforeProvenanceSave, self.root())?;
        let mut next: FalconConfig = self.config.clone();
        next.sync_sources.retain(|k, _| !k.starts_with(prefix));
        next.sync_sources.extend(provenance.iter().map(|(k, v)| (k.clone(), v.clone())));
        if next == self.config {
            return Ok(());
        }
        if self.write_mode == crate::undo::WriteMode::Batched {
            self.config = next;
            self.dirty.config = true;
        } else {
            next.save(self.root())?;
            self.config = next;
        }
        Ok(())
    }

    /// Brings a linked item up to date with its changed source file: size,
    /// times, identity and fingerprint, dimensions, thumbnail, palette and the
    /// metadata read from the file. The item keeps its id, tags, notes, folders
    /// and rating. Returns `Ok(false)`, writing nothing, if the source changed
    /// again while it was being read, so the next pass takes it.
    ///
    /// The item is written before the sidecar. A failure in between leaves the
    /// old identity on record, so the change is found and refreshed again.
    fn refresh_linked_source(&mut self, id: &str, meta: &fs::Metadata) -> Result<bool> {
        fault::check(FaultPoint::SyncBeforeRefresh, self.root())?;
        let item = self.item(id).cloned().ok_or_else(|| Error::Refused(format!("unknown item {id}")))?;
        let record = self.link_record(id)?.ok_or_else(|| Error::Refused(format!("{id} has no link record")))?;
        let source = record.source_path.clone();
        let now = now_ms();

        let mut updated = item.clone();
        updated.size = meta.len();
        updated.mtime = mtime_ms(meta);
        updated.last_modified = now.max(item.last_modified);
        let thumb = item.info_dir(self.root()).join(item.thumbnail_name());
        if !crate::import::redraw(&mut updated, &source, &thumb) {
            // The new content can't be drawn now. The old picture must not
            // stay up for it; the background worker tries again.
            let _ = fs::remove_file(&thumb);
            updated.no_thumbnail = None;
        }
        crate::import::refresh_source_metadata(&mut updated, &source);

        // Changed again while it was read: write nothing this time.
        let Ok(after) = fs::metadata(&source) else { return Ok(false) };
        if after.len() != meta.len() || mtime_ms(&after) != mtime_ms(meta) {
            return Ok(false);
        }

        let draft = linked::build_draft(&source, self.root(), &after, now);
        let mut next = record;
        next.identity = draft.identity;
        next.volume = draft.volume;
        if draft.bookmark_base64.is_some() {
            next.bookmark_base64 = draft.bookmark_base64;
        }
        next.last_verified_at_ms = now;

        self.register_item(updated.clone())?;
        fault::check(FaultPoint::SyncBeforeRefreshSidecar, &source)?;
        linked::write_sidecar(&updated, self.root(), &next)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage_mode::SyncMode;

    #[test]
    fn skip_filter_allows_valid_small_files() {
        assert!(is_sync_skipped("empty.png", 0), "0-byte file skipped");
        assert!(!is_sync_skipped("small.png", 50), "small 50-byte file allowed");
        assert!(!is_sync_skipped("doc.pdf", 1024), "normal file allowed");
        assert!(is_sync_skipped(".hidden.png", 100), "hidden file skipped");
        assert!(is_sync_skipped("temp.tmp", 100), "temp file skipped");
        assert!(is_sync_skipped("noext", 100), "missing extension skipped");
    }

    #[test]
    fn offline_folder_sets_status_without_error() {
        let tmp = tempfile::tempdir().unwrap();
        let lib_dir = tmp.path().join("Test.library");
        let mut lib = Library::create(&lib_dir).unwrap();
        let fid = lib.create_folder(None, "Assets").unwrap();

        let mut wf = WatchedFolder {
            id: "wf-1".into(),
            source_path: tmp.path().join("does-not-exist"),
            folder_id: fid,
            storage_mode: StorageMode::Link,
            sync_mode: SyncMode::Manual,
            recursive: true,
            last_synced: None,
            status: SyncStatus::Idle,
            last_error: None,
        };

        let stats = lib.sync_watched_folder(&mut wf).unwrap();
        assert!(stats.offline);
        assert_eq!(wf.status, SyncStatus::Offline);
        assert!(wf.last_error.is_some());
    }

    #[test]
    fn link_mode_sync_imports_and_detects_modification() {
        let tmp = tempfile::tempdir().unwrap();
        let lib_dir = tmp.path().join("Test.library");
        let mut lib = Library::create(&lib_dir).unwrap();
        let fid = lib.create_folder(None, "External").unwrap();

        let src_dir = tmp.path().join("ExternalSource");
        fs::create_dir_all(&src_dir).unwrap();
        let file_path = src_dir.join("photo.jpg");
        fs::write(&file_path, b"initial photo data").unwrap();

        let mut wf = WatchedFolder {
            id: "wf-link".into(),
            source_path: src_dir.clone(),
            folder_id: fid.clone(),
            storage_mode: StorageMode::Link,
            sync_mode: SyncMode::Manual,
            recursive: true,
            last_synced: None,
            status: SyncStatus::Idle,
            last_error: None,
        };

        // First sync: imports photo.jpg as linked asset
        let stats = lib.sync_watched_folder(&mut wf).unwrap();
        assert_eq!(stats.added, 1);
        assert_eq!(stats.modified, 0);
        assert_eq!(wf.status, SyncStatus::Success);
        assert_eq!(lib.items().len(), 1);

        // Second sync without changes: skipped
        let stats2 = lib.sync_watched_folder(&mut wf).unwrap();
        assert_eq!(stats2.added, 0);
        assert_eq!(stats2.modified, 0);
        assert_eq!(stats2.skipped, 1);

        // Modify source file
        fs::write(&file_path, b"updated photo data with larger size").unwrap();

        // Third sync: detects modification
        let stats3 = lib.sync_watched_folder(&mut wf).unwrap();
        assert_eq!(stats3.added, 0);
        assert_eq!(stats3.modified, 1);
    }

    #[test]
    fn folder_hierarchy_is_replicated_recursively() {
        let tmp = tempfile::tempdir().unwrap();
        let lib_dir = tmp.path().join("TestHierarchy.library");
        let mut lib = Library::create(&lib_dir).unwrap();
        let fid = lib.create_folder(None, "Projects").unwrap();

        let src_dir = tmp.path().join("SourceTree");
        let deep_dir = src_dir.join("2026").join("Q4");
        fs::create_dir_all(&deep_dir).unwrap();
        let file_path = deep_dir.join("diagram.png");
        fs::write(&file_path, b"png data in deep folder").unwrap();

        let mut wf = WatchedFolder {
            id: "wf-tree".into(),
            source_path: src_dir.clone(),
            folder_id: fid.clone(),
            storage_mode: StorageMode::Link,
            sync_mode: SyncMode::Manual,
            recursive: true,
            last_synced: None,
            status: SyncStatus::Idle,
            last_error: None,
        };

        let stats = lib.sync_watched_folder(&mut wf).unwrap();
        assert_eq!(stats.added, 1);

        // Verify folder hierarchy in Falcon
        let roots = &lib.metadata.folders;
        let p_folder = find_folder_in(roots, &fid).expect("parent folder");
        let y_folder = p_folder.children.iter().find(|c| c.name == "2026").expect("2026 child");
        let q_folder = y_folder.children.iter().find(|c| c.name == "Q4").expect("Q4 grandchild");

        let item = &lib.items()[0];
        assert!(item.folders.contains(&q_folder.id), "item is inside grandchild folder");
    }

    #[test]
    fn copy_mode_sync_does_not_overwrite_managed_files() {
        let tmp = tempfile::tempdir().unwrap();
        let lib_dir = tmp.path().join("TestCopy.library");
        let mut lib = Library::create(&lib_dir).unwrap();
        let fid = lib.create_folder(None, "ManagedSync").unwrap();

        let src_dir = tmp.path().join("CopySource");
        fs::create_dir_all(&src_dir).unwrap();
        let file_path = src_dir.join("logo.png");
        fs::write(&file_path, b"managed logo payload").unwrap();

        let mut wf = WatchedFolder {
            id: "wf-copy".into(),
            source_path: src_dir.clone(),
            folder_id: fid.clone(),
            storage_mode: StorageMode::Copy,
            sync_mode: SyncMode::Manual,
            recursive: false,
            last_synced: None,
            status: SyncStatus::Idle,
            last_error: None,
        };

        // First pass: imports 1 file
        let stats = lib.sync_watched_folder(&mut wf).unwrap();
        assert_eq!(stats.added, 1);
        assert_eq!(lib.items().len(), 1);

        // Second pass: skips existing file without duplicating or overwriting
        let stats2 = lib.sync_watched_folder(&mut wf).unwrap();
        assert_eq!(stats2.added, 0);
        assert_eq!(stats2.skipped, 1);
        assert_eq!(lib.items().len(), 1);
    }

    fn wf_for(id: &str, src: &Path, folder: &str, mode: StorageMode) -> WatchedFolder {
        WatchedFolder {
            id: id.into(),
            source_path: src.to_path_buf(),
            folder_id: folder.into(),
            storage_mode: mode,
            sync_mode: SyncMode::Manual,
            recursive: true,
            last_synced: None,
            status: SyncStatus::Idle,
            last_error: None,
        }
    }

    fn write_png(path: &Path, w: u32, h: u32, shade: u8) {
        image::RgbImage::from_pixel(w, h, image::Rgb([shade, 10, 200])).save(path).unwrap();
    }

    fn persisted_status(lib: &Library) -> String {
        lib.config().linked_folders[0]["status"].as_str().unwrap().to_string()
    }

    #[test]
    fn temporary_names_are_matched_precisely() {
        for kept in ["design.tmpl.png", "photo.png", "template.tmp.png", "report (draft).pdf", "a~b.png"] {
            assert!(!is_temporary_name(kept), "{kept} is an ordinary file");
            assert!(!is_sync_skipped(kept, 10), "{kept} stays eligible");
        }
        for skipped in [
            "photo.png.tmp",
            "photo.TMP",
            "~$budget.xlsx",
            "photo.png~",
            ".hidden.png",
            "movie.mp4.crdownload",
            "movie.mp4.part",
            "Thumbs.db",
            "desktop.ini",
            "file.lock",
            "x.swp",
        ] {
            assert!(is_sync_skipped(skipped, 10), "{skipped} is a temporary or system file");
        }
    }

    #[test]
    fn copy_mode_survives_renames_and_reports_changed_sources_without_overwriting() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("L.library")).unwrap();
        let fid = lib.create_folder(None, "Managed").unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).unwrap();
        let file = src.join("logo.png");
        write_png(&file, 4, 4, 50);
        let mut wf = wf_for("wf-copy2", &src, &fid, StorageMode::Copy);

        let first = lib.sync_watched_folder(&mut wf).unwrap();
        assert_eq!(first.added, 1);
        let id = first.added_ids[0].clone();

        // Renaming the item inside Falcon must not import the source again.
        let mut item = lib.item(&id).unwrap().clone();
        item.name = "Renamed".into();
        lib.update_item(item).unwrap();
        let again = lib.sync_watched_folder(&mut wf).unwrap();
        assert_eq!((again.added, again.conflicts), (0, 0));
        assert_eq!(lib.items().len(), 1, "idempotent after a rename");

        // A restart of the library (reopen) keeps the record.
        let root = lib.root().to_path_buf();
        drop(lib);
        let mut lib = Library::open(&root).unwrap();
        assert_eq!(lib.sync_watched_folder(&mut wf).unwrap().added, 0);

        // A touched file (same bytes, new time) is not a conflict.
        let bytes = fs::read(&file).unwrap();
        let managed_before = fs::read(lib.managed_writable_path(&id, "test").unwrap()).unwrap();
        let later = filetime::FileTime::from_unix_time(1_900_000_000, 0);
        filetime::set_file_mtime(&file, later).unwrap();
        let touched = lib.sync_watched_folder(&mut wf).unwrap();
        assert_eq!((touched.added, touched.conflicts), (0, 0));

        // A changed source is reported, and the managed original stays as it was.
        write_png(&file, 9, 9, 90);
        assert_ne!(fs::read(&file).unwrap(), bytes);
        let changed = lib.sync_watched_folder(&mut wf).unwrap();
        assert_eq!((changed.added, changed.conflicts), (0, 1));
        assert_eq!(fs::read(lib.managed_writable_path(&id, "test").unwrap()).unwrap(), managed_before);
        assert_eq!(lib.items().len(), 1);
    }

    #[test]
    fn copy_mode_adopts_items_imported_before_provenance_and_never_resurrects_deleted_ones() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("L.library")).unwrap();
        let fid = lib.create_folder(None, "Managed").unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).unwrap();
        let file = src.join("old.png");
        write_png(&file, 4, 4, 70);
        let existing = lib
            .import_file(&file, &ImportOptions { folders: vec![fid.clone()], storage_mode: Some(StorageMode::Copy), ..Default::default() })
            .unwrap();
        let mut wf = wf_for("wf-adopt", &src, &fid, StorageMode::Copy);
        let stats = lib.sync_watched_folder(&mut wf).unwrap();
        assert_eq!((stats.added, lib.items().len()), (0, 1), "the earlier item is adopted, not duplicated");
        assert!(lib.config().sync_sources.values().any(|s| s.item_id == existing));

        lib.delete_permanently(std::slice::from_ref(&existing)).unwrap();
        let after = lib.sync_watched_folder(&mut wf).unwrap();
        assert_eq!(after.added, 0, "an item removed on purpose is not brought back");
        assert!(lib.items().is_empty());
    }

    #[test]
    fn link_refresh_updates_derived_data_and_keeps_user_data() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("L.library")).unwrap();
        let fid = lib.create_folder(None, "Linked").unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).unwrap();
        let file = src.join("pic.png");
        write_png(&file, 640, 480, 30);
        let mut wf = wf_for("wf-refresh", &src, &fid, StorageMode::Link);
        let id = lib.sync_watched_folder(&mut wf).unwrap().added_ids.remove(0);
        lib.add_tags_to_items(std::slice::from_ref(&id), &["keep-me".to_string()]).unwrap();
        let mut item = lib.item(&id).unwrap().clone();
        item.annotation = "my note".into();
        lib.update_item(item).unwrap();
        let before = lib.item(&id).unwrap().clone();
        let old_identity = lib.link_record(&id).unwrap().unwrap().identity;
        assert_eq!((before.width, before.height), (640, 480));

        write_png(&file, 1280, 720, 200);
        let stats = lib.sync_watched_folder(&mut wf).unwrap();
        assert_eq!((stats.modified, stats.refreshed_ids.clone()), (1, vec![id.clone()]));

        let after = lib.item(&id).unwrap();
        assert_eq!((after.width, after.height), (1280, 720), "dimensions follow the new file");
        assert_eq!(after.size, fs::metadata(&file).unwrap().len());
        assert_eq!(after.id, id);
        assert!(after.tags.contains(&"keep-me".to_string()) && after.annotation == "my note", "user data is kept");
        assert_eq!(after.folders, before.folders);
        let thumb = after.thumbnail_path(lib.root()).expect("thumbnail redrawn");
        let drawn = crate::thumbnail::decode(&thumb).unwrap();
        assert!(drawn.width() * 9 == drawn.height() * 16 || (drawn.width() as f32 / drawn.height() as f32 - 16.0 / 9.0).abs() < 0.02, "the thumbnail shows the new 16:9 picture");
        let new_identity = lib.link_record(&id).unwrap().unwrap().identity;
        assert_ne!(new_identity.size, old_identity.size);
        assert_ne!(new_identity.content_fingerprint, old_identity.content_fingerprint);
        // Idempotent afterwards.
        assert_eq!(lib.sync_watched_folder(&mut wf).unwrap().modified, 0);
        // The referenced original was never written to.
        assert_eq!(image::open(&file).unwrap().width(), 1280);
    }

    #[test]
    fn a_deferred_thumbnail_staged_from_an_old_source_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("L.library")).unwrap();
        let src = tmp.path().join("pic.png");
        write_png(&src, 4, 4, 30);
        let opts = ImportOptions { storage_mode: Some(StorageMode::Link), defer_thumbnail: true, ..Default::default() };
        let id = lib.import_file(&src, &opts).unwrap();
        let item = lib.item(&id).unwrap().clone();
        let staged = crate::import::stage_deferred_thumbnail(&item, &src, &item.info_dir(lib.root()), true).unwrap().unwrap();
        write_png(&src, 20, 20, 1);
        let err = lib.commit_deferred_thumbnail(&id, staged).unwrap_err();
        assert!(err.to_string().contains("source changed"), "{err}");
        assert!(lib.item(&id).unwrap().thumbnail_path(lib.root()).is_none());
    }

    #[test]
    fn an_interrupted_sync_never_shows_as_syncing_forever() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("L.library");
        let mut lib = Library::create(&root).unwrap();
        let fid = lib.create_folder(None, "F").unwrap();
        let mut wf = wf_for("wf-int", tmp.path(), &fid, StorageMode::Link);
        wf.status = SyncStatus::Syncing;
        lib.upsert_watched_folder(wf).unwrap();
        assert_eq!(lib.watched_folders()[0].status, SyncStatus::Syncing, "a pass in progress keeps its state");
        drop(lib);
        // Found at open, the pass was cut short.
        let lib = Library::open(&root).unwrap();
        let shown = &lib.watched_folders()[0];
        assert_eq!(shown.status, SyncStatus::Error);
        assert!(shown.last_error.as_deref().is_some_and(|e| e.contains("interrupted")));
        let again = Library::open(&root).unwrap();
        assert_eq!(again.watched_folders()[0].status, SyncStatus::Error, "and it was saved");
    }

    /// Each persistence and registration boundary of a pass, failed in turn:
    /// the status ends in `Error` (never `Syncing`), and a second pass finishes
    /// the work without duplicating anything.
    #[test]
    fn failure_at_every_boundary_ends_in_error_and_the_next_pass_recovers() {
        use crate::fault::{FaultPoint as P, install};
        let scenarios: [(P, StorageMode, bool); 8] = [
            (P::SyncAfterMarkedSyncing, StorageMode::Link, false),
            (P::SyncBeforeFolderCreate, StorageMode::Link, false),
            (P::SyncBeforeImport, StorageMode::Link, false),
            (P::SyncBeforeImport, StorageMode::Copy, false),
            (P::SyncBeforeProvenanceSave, StorageMode::Copy, false),
            (P::SyncBeforeFinalize, StorageMode::Copy, false),
            (P::SyncBeforeRefresh, StorageMode::Link, true),
            (P::SyncBeforeRefreshSidecar, StorageMode::Link, true),
        ];
        for (point, mode, refresh) in scenarios {
            let tmp = tempfile::tempdir().unwrap();
            let mut lib = Library::create(tmp.path().join("L.library")).unwrap();
            let fid = lib.create_folder(None, "F").unwrap();
            let src = tmp.path().join("src");
            fs::create_dir_all(src.join("sub")).unwrap();
            write_png(&src.join("top.png"), 4, 4, 10);
            write_png(&src.join("sub").join("deep.png"), 5, 5, 20);
            let mut wf = wf_for("wf-fault", &src, &fid, mode);

            if refresh {
                lib.sync_watched_folder(&mut wf).unwrap();
                write_png(&src.join("top.png"), 8, 8, 99);
            }
            let id_before: Vec<String> = lib.items().iter().map(|i| i.id.clone()).collect();
            let old_identity = id_before.first().and_then(|id| lib.link_record(id).ok().flatten()).map(|r| r.identity);

            {
                let _guard = install(move |p| if p == point { Err(std::io::Error::other("injected")) } else { Ok(()) });
                let res = lib.sync_watched_folder(&mut wf);
                assert!(res.is_err(), "{point:?} {mode:?}: the pass fails");
            }
            assert_eq!(wf.status, SyncStatus::Error, "{point:?}: in memory");
            assert_eq!(persisted_status(&lib), "error", "{point:?}: persisted");
            if point == P::SyncBeforeRefreshSidecar {
                let now = lib.link_record(&id_before[0]).unwrap().unwrap().identity;
                assert_eq!(Some(now), old_identity, "the old identity stays so the change is found again");
            }

            let stats = lib.sync_watched_folder(&mut wf).unwrap();
            assert_eq!(wf.status, SyncStatus::Success, "{point:?}: recovered");
            assert_eq!(persisted_status(&lib), "success");
            assert_eq!(lib.items().len(), 2, "{point:?} {mode:?}: no duplicates after recovery");
            if refresh {
                assert_eq!(stats.modified, 1, "{point:?}: the change is picked up on retry");
            }
            let third = lib.sync_watched_folder(&mut wf).unwrap();
            assert_eq!((third.added, third.modified, third.conflicts), (0, 0, 0), "{point:?}: idempotent");
        }
    }
}
