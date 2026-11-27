//! Recovery journal and deterministic reopen diagnostics for multi-file
//! library operations (import, duplicate, merge).
//!
//! Individual JSON writes use `atomic_write_json` / `atomic_write_bytes`,
//! which does not make multi-file sequences atomic by itself. A transient
//! `<library>/.falcon-journal.json` sidecar + same-volume staging directories
//! allow interrupted operations to be diagnosed fail-closed on reopen without
//! misclassifying linked assets as managed or silently deleting unverified data.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::fault::{self, FaultPoint};
use crate::linked::LINK_SIDECAR;
use crate::model::{Item, MtimeIndex};
use crate::storage_mode::{FalconConfig, atomic_create_json_exclusive, read_json, sync_dir};

/// Transient operation journal persisted at `<library>/.falcon-journal.json`.
pub const JOURNAL_FILE: &str = ".falcon-journal.json";

/// Schema version of `.falcon-journal.json`.
pub const JOURNAL_SCHEMA_VERSION: u32 = 1;

/// Multi-file operation kind tracked by the recovery journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingOpKind {
    Import,
    Duplicate,
    Merge,
}

/// Persisted intent record written before multi-file publication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingJournal {
    pub schema_version: u32,
    pub kind: PendingOpKind,
    pub item_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub linked_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stage_dirs: Vec<String>,
    pub created_at_ms: u64,
}

/// Non-destructive structural/recovery diagnostics discovered when opening a library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryDiagnostic {
    /// `<library>/.falcon-journal.json` remained from an interrupted operation.
    InterruptedJournal {
        kind: PendingOpKind,
        item_ids: Vec<String>,
        linked_ids: Vec<String>,
        stage_dirs: Vec<String>,
    },
    /// `.falcon-journal.json` existed on disk but could not be parsed cleanly.
    CorruptJournal { path: PathBuf, reason: String },
    /// Uncommitted hidden staging folder inside `images/`.
    OrphanStagingDir {
        path: PathBuf,
        has_link_sidecar: bool,
    },
    /// `<stem>.info/metadata.json` had `item.id != stem` (excluded fail-closed).
    IdMismatch {
        dir: PathBuf,
        folder_stem: String,
        item_id: String,
    },
    /// Duplicate `item.id` across multiple `.info` folders (excluded fail-closed).
    DuplicateItemId { dir: PathBuf, id: String },
    /// Corrupt or unreadable item `metadata.json` (in `Library.skipped`).
    CorruptItem { dir: PathBuf, reason: String },
    /// Linked ID in `falcon.json` has no loaded `.info` catalog item.
    OrphanLinkedId { id: String },
}

/// Result of explicitly running `Library::recover_pending_journal`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    /// Published `.info` items confirmed consistent with `falcon.json` and `mtime.json`.
    pub finalized_published: Vec<String>,
    /// Unpublished linked IDs removed from `falcon.json` when no `.info` existed.
    pub rolled_back_linked_ids: Vec<String>,
    /// Incomplete staging directories left preserved (never silently deleted).
    pub preserved_staging_dirs: Vec<PathBuf>,
    /// Whether `.falcon-journal.json` was cleared cleanly.
    pub journal_cleared: bool,
}

pub(crate) fn journal_path(library_root: &Path) -> PathBuf {
    library_root.join(JOURNAL_FILE)
}

pub(crate) fn write_journal(library_root: &Path, journal: &PendingJournal) -> Result<()> {
    let path = journal_path(library_root);
    if fs::symlink_metadata(&path).is_ok() {
        return Err(Error::Refused(format!(
            "cannot start {:?} while an unresolved recovery journal exists at {}; recover first",
            journal.kind,
            path.display()
        )));
    }
    fault::check(FaultPoint::JournalWrite, &path)?;
    atomic_create_json_exclusive(&path, journal)
}

pub(crate) fn remove_journal(library_root: &Path) -> Result<()> {
    let path = journal_path(library_root);
    match fs::remove_file(&path) {
        Ok(()) => {
            sync_dir(library_root);
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io(&path, e)),
    }
}

pub(crate) fn read_journal(
    library_root: &Path,
) -> std::result::Result<Option<PendingJournal>, String> {
    let path = journal_path(library_root);
    if !path.exists() {
        return Ok(None);
    }
    let parsed: PendingJournal = read_json(&path).map_err(|e| e.to_string())?;
    if parsed.schema_version == 0 || parsed.schema_version > JOURNAL_SCHEMA_VERSION {
        return Err(format!(
            "unsupported journal schema version {}",
            parsed.schema_version
        ));
    }
    Ok(Some(parsed))
}

/// Checks if a directory name inside `images/` is a hidden Falcon staging directory.
pub(crate) fn is_staging_dir_name(name: &str) -> bool {
    name.starts_with(".staging-") || name.starts_with(".duplicating-") || name.ends_with(".merging")
}

/// Checks whether a hidden directory contains `falcon-link.json`.
pub(crate) fn dir_has_link_sidecar(dir: &Path) -> bool {
    dir.join(LINK_SIDECAR).is_file()
}

/// Deterministic explicit recovery for an interrupted `.falcon-journal.json`.
/// Idempotent: verifies published items (keeping them intact and repairing stale
/// `falcon.json::linked_ids` + `mtime.json`), rolls back unpublished `linked_ids`,
/// and preserves unverified staging files and external originals.
pub(crate) fn reconcile_journal(
    library_root: &Path,
    config: &mut FalconConfig,
    items: &[Item],
    mtime: &mut MtimeIndex,
    write_mtime: impl FnOnce(&MtimeIndex) -> Result<()>,
) -> Result<RecoveryReport> {
    let mut report = RecoveryReport::default();
    let Some(journal) = (match read_journal(library_root) {
        Ok(j) => j,
        Err(reason) => {
            return Err(Error::Refused(format!(
                "cannot automatically recover corrupt journal: {reason}"
            )));
        }
    }) else {
        return Ok(report);
    };

    let loaded_ids: BTreeSet<String> = items.iter().map(|i| i.id.clone()).collect();
    let mut cfg_next = config.clone();
    let mut cfg_dirty = false;
    let mut mtime_dirty = false;
    let images = library_root.join("images");

    for id in &journal.item_ids {
        let info_dir = images.join(format!("{id}.info"));
        let published = loaded_ids.contains(id) && info_dir.is_dir();
        if published {
            let has_sidecar = dir_has_link_sidecar(&info_dir);
            if (journal.linked_ids.contains(id) || has_sidecar)
                && cfg_next.linked_ids.insert(id.clone())
            {
                cfg_dirty = true;
            }
            if let Some(item) = items.iter().find(|i| &i.id == id) {
                match mtime.entries.iter_mut().find(|(eid, _)| eid == id) {
                    Some((_, stamp)) if *stamp != item.last_modified => {
                        *stamp = item.last_modified;
                        mtime_dirty = true;
                    }
                    Some(_) => {}
                    None => {
                        mtime.entries.push((id.clone(), item.last_modified));
                        mtime_dirty = true;
                    }
                }
            }
            report.finalized_published.push(id.clone());
        } else if cfg_next.linked_ids.remove(id) {
            cfg_dirty = true;
            report.rolled_back_linked_ids.push(id.clone());
        }
    }

    if mtime.all != Some(items.len() as u64) {
        mtime.all = Some(items.len() as u64);
        mtime_dirty = true;
    }

    for stage_name in &journal.stage_dirs {
        let stage_path = images.join(stage_name);
        if stage_path.is_dir() {
            report.preserved_staging_dirs.push(stage_path);
        }
    }

    if cfg_dirty {
        cfg_next.save(library_root)?;
        *config = cfg_next;
    }
    if mtime_dirty {
        write_mtime(mtime)?;
    }

    remove_journal(library_root)?;
    report.journal_cleared = true;
    Ok(report)
}
