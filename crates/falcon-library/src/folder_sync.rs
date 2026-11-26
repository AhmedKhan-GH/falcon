//! Folder synchronization engine for registered watched folders.
//!
//! Supports both Link (Referenced) Mode and Copy (Managed) Mode with:
//! - Recursive and shallow directory scanning
//! - Subfolder hierarchy preservation in Falcon library
//! - In Link Mode: zero-payload reference imports and stale thumbnail invalidation on source modification
//! - In Copy Mode: managed copies without overwriting existing files
//! - Safe offline/unmounted volume handling (never mass-deletes items)

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::error::{Error, Result};
use crate::import::ImportOptions;
use crate::library::Library;
use crate::model::Folder;
use crate::storage_mode::{StorageMode, SyncStatus, WatchedFolder};

fn find_folder_in<'a>(folders: &'a [Folder], id: &str) -> Option<&'a Folder> {
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
    /// True if the source path was offline or unmounted.
    pub offline: bool,
}

/// Checks whether `file_name` should be excluded from sync.
pub fn is_sync_skipped(name: &str, size: u64) -> bool {
    if name.starts_with('.') || name.contains(".tmp") {
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

fn mtime_ms(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as u64)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

impl Library {
    /// Synchronizes a registered watched folder against the current library state.
    pub fn sync_watched_folder(&mut self, wf: &mut WatchedFolder) -> Result<SyncStats> {
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
        let _ = self.upsert_watched_folder(wf.clone());

        // Build index of canonical source path -> item_id for linked assets
        let mut linked_by_source: HashMap<PathBuf, String> = HashMap::new();
        for id in &self.config.linked_ids {
            if let Some(src) = self.last_known_source_path(id) {
                let key = fs::canonicalize(&src).unwrap_or(src);
                linked_by_source.insert(key, id.clone());
            }
        }

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
                    None => self.create_folder(Some(&parent_falcon_id), &dir_name)?,
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
                    if let Some(existing_id) = linked_by_source.get(&canonical) {
                        // Already registered: check if source has changed
                        if let Some(item) = self.item(existing_id).cloned() {
                            let mtime = mtime_ms(&meta);
                            let rec = self.link_record(existing_id).ok().flatten();
                            let is_changed = match &rec {
                                Some(r) => r.identity.size != meta.len() || r.identity.mtime_ms != mtime,
                                None => item.size != meta.len(),
                            };

                            if is_changed {
                                // Source modified: invalidate cached thumbnail and refresh metadata
                                if let Some(thumb) = item.thumbnail_path(self.root()) {
                                    let _ = fs::remove_file(thumb);
                                }
                                if let Some(mut r) = rec {
                                    r.identity.size = meta.len();
                                    r.identity.mtime_ms = mtime;
                                    r.last_verified_at_ms = now_ms();
                                    let _ = crate::linked::write_sidecar(&item, self.root(), &r);
                                }
                                let mut updated = item;
                                updated.size = meta.len();
                                updated.last_modified = now_ms();
                                self.register_item(updated)?;
                                stats.modified += 1;
                            } else {
                                stats.skipped += 1;
                            }
                        } else {
                            stats.skipped += 1;
                        }
                    } else {
                        // New file: import as linked asset
                        let opts = ImportOptions {
                            folders: vec![target_folder_id],
                            storage_mode: Some(StorageMode::Link),
                            ..Default::default()
                        };
                        match self.import_file(&file_path, &opts) {
                            Ok(new_id) => {
                                linked_by_source.insert(canonical, new_id);
                                stats.added += 1;
                            }
                            Err(_) => {
                                stats.skipped += 1;
                            }
                        }
                    }
                }
                StorageMode::Copy => {
                    // One-way copy without overwriting managed files
                    let stem = file_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    let ext = file_path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();

                    let already_exists = self.items().iter().any(|i| {
                        i.folders.contains(&target_folder_id) && i.name == stem && i.ext.eq_ignore_ascii_case(&ext)
                    });

                    if already_exists {
                        stats.skipped += 1;
                    } else {
                        let opts = ImportOptions {
                            folders: vec![target_folder_id],
                            storage_mode: Some(StorageMode::Copy),
                            ..Default::default()
                        };
                        match self.import_file(&file_path, &opts) {
                            Ok(_) => {
                                stats.added += 1;
                            }
                            Err(_) => {
                                stats.skipped += 1;
                            }
                        }
                    }
                }
            }
        }

        wf.last_synced = Some(now_ms());
        wf.status = SyncStatus::Success;
        wf.last_error = None;
        self.upsert_watched_folder(wf.clone())?;

        Ok(stats)
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
}
