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
use crate::storage_mode::{FalconConfig, atomic_write_json, read_json};

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
    fault::check(FaultPoint::JournalWrite, &path)?;
    atomic_write_json(&path, journal)
}

pub(crate) fn remove_journal(library_root: &Path) -> Result<()> {
    let path = journal_path(library_root);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
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
/// Never deletes unverified staging files or external originals.
pub(crate) fn reconcile_journal(
    library_root: &Path,
    config: &mut FalconConfig,
    loaded_ids: &BTreeSet<String>,
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

    let mut cfg_next = config.clone();
    let mut cfg_dirty = false;
    let images = library_root.join("images");

    for id in &journal.item_ids {
        let published = loaded_ids.contains(id) && images.join(format!("{id}.info")).is_dir();
        if published {
            if journal.linked_ids.contains(id) && cfg_next.linked_ids.insert(id.clone()) {
                cfg_dirty = true;
            }
            report.finalized_published.push(id.clone());
        } else if cfg_next.linked_ids.remove(id) {
            cfg_dirty = true;
            report.rolled_back_linked_ids.push(id.clone());
        }
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

    remove_journal(library_root)?;
    report.journal_cleared = true;
    Ok(report)
}
