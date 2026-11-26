//! Per-library asset storage mode and Falcon-native configuration sidecar (`falcon.json`).
//!
//! Eagle's `metadata.json` remains untouched for compatibility; Falcon-specific
//! configuration, linked item membership, and reserved future fields (stable
//! library UUID, linked folders, sync tombstones, configurable storage locations)
//! live strictly in `<library>/falcon.json`.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::error::{Error, Result};

/// File name of the root Falcon configuration sidecar inside `.library`.
pub const FALCON_CONFIG: &str = "falcon.json";

/// Current supported schema version for `falcon.json`.
pub const FALCON_SCHEMA_VERSION: u32 = 1;

/// How newly imported external files are stored by default in a library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageMode {
    /// Managed assets: copy original file into `<library>/images/<id>.info/`.
    #[default]
    Copy,
    /// Referenced assets: link original file in place without duplicating payload.
    Link,
}

impl StorageMode {
    /// Human-readable label matching Owner terminology.
    pub fn label(self) -> &'static str {
        match self {
            StorageMode::Copy => "Copy to Library",
            StorageMode::Link => "Link (Place)",
        }
    }
}

/// How a watched folder synchronizes with its source directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncMode {
    #[default]
    Auto,
    Manual,
    Disabled,
}

impl SyncMode {
    pub fn label(self) -> &'static str {
        match self {
            SyncMode::Auto => "Automatic",
            SyncMode::Manual => "Manual",
            SyncMode::Disabled => "Disabled",
        }
    }
}

/// The current status of a synchronized watched folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncStatus {
    #[default]
    Idle,
    Syncing,
    Success,
    Offline,
    Error,
}

impl SyncStatus {
    pub fn label(self) -> &'static str {
        match self {
            SyncStatus::Idle => "Idle",
            SyncStatus::Syncing => "Syncing…",
            SyncStatus::Success => "Synced",
            SyncStatus::Offline => "Offline",
            SyncStatus::Error => "Error",
        }
    }
}

/// Configuration for a watched source folder mapped into a Falcon library folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchedFolder {
    /// Stable identifier for this watched folder configuration.
    pub id: String,
    /// Absolute path to the source folder on disk.
    pub source_path: PathBuf,
    /// Corresponding Falcon library folder ID.
    pub folder_id: String,
    /// Storage mode for imported files: Link (reference in place) or Copy.
    pub storage_mode: StorageMode,
    /// Synchronization mode: Auto, Manual, or Disabled.
    pub sync_mode: SyncMode,
    /// Whether to recursively synchronize nested subfolders.
    #[serde(default = "default_true")]
    pub recursive: bool,
    /// Epoch timestamp in milliseconds of the last successful synchronization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_synced: Option<u64>,
    /// Last known synchronization status.
    #[serde(default)]
    pub status: SyncStatus,
    /// Description of the last error encountered (if any).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

fn default_true() -> bool {
    true
}

/// Where a Copy-mode synchronized item came from, so that renaming the item
/// in Falcon never makes the same source file import again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncSource {
    /// The library item made from the source file.
    pub item_id: String,
    /// Size of the source when it was imported (or last seen unchanged).
    pub size: u64,
    /// Modification time of the source then, in milliseconds.
    pub mtime_ms: u64,
    /// Bounded head and tail hash of the source then, to tell a touched file
    /// from a changed one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
}

/// Root configuration persisted at `<library>/falcon.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FalconConfig {
    /// Monotonic schema version checked fail-closed on open.
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    /// Default import mode for external assets imported into this library.
    #[serde(default)]
    pub storage_mode: StorageMode,
    /// Authoritative set of item IDs imported as linked assets.
    ///
    /// Keeping this in `falcon.json` ensures a deleted or unreadable
    /// `falcon-link.json` never silently reclassifies a linked item as `Managed`.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub linked_ids: BTreeSet<String>,
    /// Reserved stable UUID for portable library identification across paths/volumes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library_id: Option<String>,
    /// Reserved forward-compatible linked folder sync definitions (Phase B2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub linked_folders: Vec<serde_json::Value>,
    /// Reserved forward-compatible delete-sync tombstones (Phase B2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded_sources: Vec<String>,
    /// Copy-mode sync provenance, keyed by `<watched folder id>|<path relative
    /// to the watched folder, with / separators>`. Older Falcon versions
    /// ignore it, and a copy of falcon.json without it only loses the
    /// shortcut: the next sync adopts matching items instead of importing.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub sync_sources: std::collections::BTreeMap<String, SyncSource>,
    /// Reserved forward-compatible storage location overrides (Portable Library).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_locations: Option<serde_json::Value>,
}

fn default_schema_version() -> u32 {
    FALCON_SCHEMA_VERSION
}

impl Default for FalconConfig {
    fn default() -> Self {
        Self {
            schema_version: FALCON_SCHEMA_VERSION,
            storage_mode: StorageMode::Copy,
            linked_ids: BTreeSet::new(),
            library_id: None,
            linked_folders: Vec::new(),
            excluded_sources: Vec::new(),
            sync_sources: Default::default(),
            storage_locations: None,
        }
    }
}

impl FalconConfig {
    /// Path of `falcon.json` under `library_root`.
    pub fn path(library_root: &Path) -> PathBuf {
        library_root.join(FALCON_CONFIG)
    }

    /// Loads `<library>/falcon.json` if present, or returns defaults when absent
    /// (preserving legacy Eagle libraries without writing sidecars prematurely).
    /// Fails closed if the file exists but cannot be read, parsed, or has a newer
    /// `schema_version` than supported.
    pub fn load_or_default(library_root: &Path) -> Result<Self> {
        let path = Self::path(library_root);
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(Error::io(&path, e)),
        };
        let cfg: Self = serde_json::from_slice(&bytes).map_err(|e| Error::json(&path, e))?;
        if cfg.schema_version == 0 || cfg.schema_version > FALCON_SCHEMA_VERSION {
            return Err(Error::NewerFalconMetadata {
                path,
                version: cfg.schema_version,
            });
        }
        Ok(cfg)
    }

    /// Atomically persists `<library>/falcon.json`.
    pub fn save(&self, library_root: &Path) -> Result<()> {
        let path = Self::path(library_root);
        atomic_write_json(&path, self)
    }

    /// Returns the parsed watched folders list from `linked_folders`.
    ///
    /// A folder stored as `Syncing` can only be left over from a pass that was
    /// cut short (a pass runs under `&mut Library`, so nobody reads the list
    /// while one is in progress); it is reported as an interrupted error and
    /// is never shown as syncing forever.
    pub fn watched_folders(&self) -> Vec<WatchedFolder> {
        self.linked_folders
            .iter()
            .filter_map(|v| serde_json::from_value::<WatchedFolder>(v.clone()).ok())
            .map(|mut w| {
                if w.status == SyncStatus::Syncing {
                    w.status = SyncStatus::Error;
                    w.last_error = Some("The last sync was interrupted before it finished".into());
                }
                w
            })
            .collect()
    }

    /// Serializes and stores `folders` into `linked_folders`.
    pub fn set_watched_folders(&mut self, folders: &[WatchedFolder]) {
        self.linked_folders = folders
            .iter()
            .filter_map(|f| serde_json::to_value(f).ok())
            .collect();
    }

    /// Finds a watched folder definition by its stable ID.
    pub fn find_watched_folder(&self, id: &str) -> Option<WatchedFolder> {
        self.watched_folders().into_iter().find(|w| w.id == id)
    }

    /// Finds a watched folder mapped to a specific library folder ID.
    pub fn find_watched_by_folder_id(&self, folder_id: &str) -> Option<WatchedFolder> {
        self.watched_folders().into_iter().find(|w| w.folder_id == folder_id)
    }
}

/// Serializes `value` cleanly and writes it atomically to `path` via a temp file
/// in the same directory (`flush` + `sync_all` + atomic rename).
pub fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| Error::json(path, e))?;
    atomic_write_bytes(path, &bytes)
}

/// Atomically writes `bytes` to `path` in the same parent directory.
pub fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::io(path, std::io::Error::other("missing parent directory")))?;
    let mut tmp = NamedTempFile::new_in(parent).map_err(|e| Error::io(path, e))?;
    tmp.write_all(bytes).map_err(|e| Error::io(path, e))?;
    tmp.flush().map_err(|e| Error::io(path, e))?;
    tmp.as_file().sync_all().map_err(|e| Error::io(path, e))?;
    tmp.persist(path).map_err(|e| Error::io(path, e.error))?;
    sync_dir(parent);
    Ok(())
}

/// Reads and parses JSON from `path`.
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).map_err(|e| Error::io(path, e))?;
    serde_json::from_slice(&bytes).map_err(|e| Error::json(path, e))
}

/// Serializes `value` without clobbering an existing target (`persist_noclobber`).
/// Used for multi-file intent journals so concurrent writers or unresolved
/// transactions can never be silently overwritten.
pub(crate) fn atomic_create_json_exclusive<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| Error::json(path, e))?;
    let parent = path
        .parent()
        .ok_or_else(|| Error::io(path, std::io::Error::other("missing parent directory")))?;
    let mut tmp = NamedTempFile::new_in(parent).map_err(|e| Error::io(path, e))?;
    tmp.write_all(&bytes).map_err(|e| Error::io(path, e))?;
    tmp.flush().map_err(|e| Error::io(path, e))?;
    tmp.as_file().sync_all().map_err(|e| Error::io(path, e))?;
    tmp.persist_noclobber(path).map_err(|e| {
        if e.error.kind() == std::io::ErrorKind::AlreadyExists {
            Error::Refused(format!(
                "an unresolved transaction journal already exists at {}",
                path.display()
            ))
        } else {
            Error::io(path, e.error)
        }
    })?;
    sync_dir(parent);
    Ok(())
}

/// Best-effort parent directory sync after atomic rename.
pub(crate) fn sync_dir(dir: &Path) {
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
}

/// Best-effort sync of all regular files inside `dir` and the directory itself.
pub(crate) fn sync_dir_contents(dir: &Path) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && let Ok(f) = File::open(&path)
            {
                let _ = f.sync_all();
            }
        }
    }
    sync_dir(dir);
}
