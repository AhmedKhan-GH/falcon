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
