//! Referenced (linked) asset metadata, path containment checks, volume identity,
//! and fail-closed status resolution.

use std::fmt;
use std::fs::{self, File};
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::model::Item;
use crate::storage_mode::{FalconConfig, atomic_write_json, read_json};

/// Per-item sidecar storing referenced asset location, volume identity,
/// bookmark, and file identity metadata (`<library>/images/<id>.info/falcon-link.json`).
pub const LINK_SIDECAR: &str = "falcon-link.json";

/// Current supported schema version for `falcon-link.json`.
pub const LINK_SCHEMA_VERSION: u32 = 1;

const FINGERPRINT_SAMPLE_BYTES: usize = 64 * 1024;

/// Authoritative ownership & availability state of an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetStatus {
    /// Original payload lives inside `<library>/images/<id>.info/`.
    Managed,
    /// Original payload is referenced externally and currently reachable.
    Linked,
    /// Referenced volume/drive is currently disconnected or unmounted.
    Offline,
    /// Referenced volume is online, but the source file at `source_path` is missing.
    Missing,
    /// Sidecar is missing, corrupted, unsupported schema version, or points inside the library.
    InvalidReference,
}

impl fmt::Display for AssetStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AssetStatus::Managed => write!(f, "managed"),
            AssetStatus::Linked => write!(f, "linked"),
            AssetStatus::Offline => write!(f, "offline"),
            AssetStatus::Missing => write!(f, "missing"),
            AssetStatus::InvalidReference => write!(f, "invalid reference"),
        }
    }
}

/// Source availability classification tailored for safe AI embedding/indexing
/// (never conflating offline/missing/permission/transient errors with unsupported/decode failures).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiSourceCondition {
    /// Asset file is readable at the returned path.
    Available(PathBuf),
    /// Referenced volume/mount is offline.
    Offline,
    /// Referenced (or managed) source file is missing on an online volume.
    Missing,
    /// Linked sidecar is damaged, missing when expected, or fails containment validation.
    InvalidReference,
    /// Access refused by OS filesystem permissions.
    PermissionDenied,
    /// Transient OS I/O error (`Interrupted`, `WouldBlock`, `TimedOut`, etc.).
    TemporarilyUnavailable,
}

/// Volume identification persisted in `falcon-link.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume_uuid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mount_point: Option<PathBuf>,
    #[serde(default)]
    pub is_removable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_internal: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<u64>,
}

/// Source file identity metadata persisted in `falcon-link.json`.
///
/// `content_fingerprint` uses bounded head/tail hashing for fast screening only,
/// never as an authoritative proof of full-file equality.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileIdentity {
    pub size: u64,
    pub mtime_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub btime_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inode: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_resource_id_base64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_fingerprint: Option<String>,
}

/// Per-item referenced asset sidecar stored at `<id>.info/falcon-link.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkRecord {
    pub schema_version: u32,
    pub item_id: String,
    pub source_path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bookmark_base64: Option<String>,
    #[serde(default)]
    pub volume: VolumeIdentity,
    pub identity: FileIdentity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative_hint: Option<String>,
    pub linked_at_ms: u64,
    pub last_verified_at_ms: u64,
}

/// Uncommitted link metadata gathered while staging a link import (zero payload copy).
#[derive(Debug, Clone)]
pub(crate) struct LinkDraft {
    pub source_path: PathBuf,
    pub bookmark_base64: Option<String>,
    pub volume: VolumeIdentity,
    pub identity: FileIdentity,
    pub relative_hint: Option<String>,
    pub linked_at_ms: u64,
}

impl LinkDraft {
    pub fn into_record(self, item_id: String) -> LinkRecord {
        LinkRecord {
            schema_version: LINK_SCHEMA_VERSION,
            item_id,
            source_path: self.source_path,
            bookmark_base64: self.bookmark_base64,
            volume: self.volume,
            identity: self.identity,
            relative_hint: self.relative_hint,
            linked_at_ms: self.linked_at_ms,
            last_verified_at_ms: self.linked_at_ms,
        }
    }
}

/// Computes `<id>.info/falcon-link.json`.
pub fn sidecar_path(item: &Item, library_root: &Path) -> PathBuf {
    item.info_dir(library_root).join(LINK_SIDECAR)
}

/// Atomically writes `record` to `<id>.info/falcon-link.json`.
pub fn write_sidecar(item: &Item, library_root: &Path, record: &LinkRecord) -> Result<()> {
    atomic_write_json(&sidecar_path(item, library_root), record)
}

/// Validates that `src` is an existing regular file strictly outside `library_root`
/// (blocking symlinks, aliases, relative traversals, and canonical containment),
/// returning the canonicalized external source path.
pub fn validate_link_source(src: &Path, library_root: &Path) -> Result<PathBuf> {
    let refuse = |reason: &str| Error::Import {
        path: src.to_path_buf(),
        reason: reason.into(),
    };

    let abs_src = make_absolute_normalized(src)?;
    if lexical_contained_in(&abs_src, library_root)? {
        return Err(refuse("cannot link a file stored inside the library"));
    }

    // Check every ancestor prefix of `src` in case an intermediate symlink inside the
    // library points outside.
    let abs_lib = make_absolute_normalized(library_root)?;
    let can_lib = fs::canonicalize(&abs_lib).unwrap_or(abs_lib.clone());
    if ancestor_hits_library(&abs_src, &abs_lib, &can_lib) {
        return Err(refuse("cannot link a file traversed through the library"));
    }

    let meta = fs::metadata(src).map_err(|e| Error::io(src, e))?;
    if !meta.is_file() {
        return Err(refuse("linked source must be an existing regular file"));
    }

    let can_src = fs::canonicalize(src).map_err(|e| Error::io(src, e))?;
    if can_src.starts_with(&can_lib) || can_src.starts_with(&abs_lib) {
        return Err(refuse(
            "cannot link a file whose real location is inside the library",
        ));
    }
    Ok(can_src)
}

/// Builds uncommitted [`LinkDraft`] metadata from a validated external file.
pub(crate) fn build_draft(
    canonical_src: &Path,
    library_root: &Path,
    meta: &fs::Metadata,
    now_ms: u64,
) -> LinkDraft {
    let volume = platform::volume_identity(canonical_src, meta);
    let mtime_ms = to_ms(meta.modified().ok());
    let btime_ms = meta.created().ok().map(|t| to_ms(Some(t)));
    let (device_id, inode) = platform::dev_and_inode(meta);
    let file_resource_id_base64 = platform::file_resource_id(canonical_src);
    let bookmark_base64 = platform::create_bookmark(canonical_src);
    let content_fingerprint = compute_fast_fingerprint(canonical_src, meta.len()).ok();
    let relative_hint = compute_relative_hint(canonical_src, library_root, &volume);

    LinkDraft {
        source_path: canonical_src.to_path_buf(),
        bookmark_base64,
        volume,
        identity: FileIdentity {
            size: meta.len(),
            mtime_ms,
            btime_ms,
            device_id,
            inode,
            file_resource_id_base64,
            content_fingerprint,
        },
        relative_hint,
        linked_at_ms: now_ms,
    }
}

/// Fast head+tail content fingerprint for quick screening only (max 128 KB + size).
pub fn compute_fast_fingerprint(path: &Path, size: u64) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(&size.to_le_bytes());

    let mut buf = vec![0u8; FINGERPRINT_SAMPLE_BYTES];
    let head = read_exact_up_to(&mut file, &mut buf)?;
    hasher.update(&buf[..head]);

    if size > FINGERPRINT_SAMPLE_BYTES as u64 {
        let tail_start = size.saturating_sub(FINGERPRINT_SAMPLE_BYTES as u64);
        file.seek(SeekFrom::Start(tail_start))?;
        let tail = read_exact_up_to(&mut file, &mut buf)?;
        hasher.update(&buf[..tail]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn read_exact_up_to(file: &mut File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        let n = file.read(&mut buf[total..])?;
        if n == 0 {
            break;
        }
        total += n;
    }
    Ok(total)
}

/// Whether `item` is expected to be linked (either listed in `config.linked_ids`
/// or having a `falcon-link.json` sidecar in its `.info` directory).
pub fn is_linked_item(config: &FalconConfig, item: &Item, library_root: &Path) -> bool {
    config.linked_ids.contains(&item.id) || sidecar_path(item, library_root).exists()
}

/// Reads and validates `<id>.info/falcon-link.json`. Returns `Ok(None)` only when
/// the item is neither registered in `config.linked_ids` nor has a sidecar file.
/// Fails closed if tracked in `config.linked_ids` with missing/invalid sidecar,
/// unsupported `schema_version`, mismatched `item_id`, or library containment.
pub fn load_validated_sidecar(
    config: &FalconConfig,
    item: &Item,
    library_root: &Path,
) -> Result<Option<LinkRecord>> {
    let sidecar = sidecar_path(item, library_root);
    load_validated_sidecar_at(
        &sidecar,
        &item.id,
        config.linked_ids.contains(&item.id),
        library_root,
    )
}

fn load_validated_sidecar_at(
    sidecar: &Path,
    item_id: &str,
    expected_linked: bool,
    library_root: &Path,
) -> Result<Option<LinkRecord>> {
    let sidecar_exists = sidecar.exists();
    if !expected_linked && !sidecar_exists {
        return Ok(None);
    }
    if !sidecar.is_file() {
        return Err(Error::AssetUnavailable {
            id: item_id.to_string(),
            status: AssetStatus::InvalidReference,
            path: None,
            reason: "missing falcon-link.json sidecar for linked asset".into(),
        });
    }
    let record: LinkRecord = read_json(sidecar).map_err(|e| Error::AssetUnavailable {
        id: item_id.to_string(),
        status: AssetStatus::InvalidReference,
        path: Some(sidecar.to_path_buf()),
        reason: format!("corrupt falcon-link.json: {e}"),
    })?;
    if record.schema_version == 0 || record.schema_version > LINK_SCHEMA_VERSION {
        return Err(Error::AssetUnavailable {
            id: item_id.to_string(),
            status: AssetStatus::InvalidReference,
            path: Some(sidecar.to_path_buf()),
            reason: format!(
                "unsupported link sidecar schema version {}",
                record.schema_version
            ),
        });
    }
    if record.item_id != item_id {
        return Err(Error::AssetUnavailable {
            id: item_id.to_string(),
            status: AssetStatus::InvalidReference,
            path: Some(sidecar.to_path_buf()),
            reason: format!(
                "sidecar itemId '{}' does not match item '{item_id}'",
                record.item_id
            ),
        });
    }
    if !record.source_path.is_absolute()
        || path_violates_library_containment(&record.source_path, library_root)
    {
        return Err(Error::AssetUnavailable {
            id: item_id.to_string(),
            status: AssetStatus::InvalidReference,
            path: Some(record.source_path),
            reason: "linked source path is invalid or points inside library".into(),
        });
    }
    Ok(Some(record))
}

/// Resolves the read-only payload path of `item` directly from disk when only
/// `library_root` and `item` are held (loading `falcon.json` safely).
/// Fails closed on `Offline`, `Missing`, and `InvalidReference`.
pub(crate) fn resolve_item_read_path(library_root: &Path, item: &Item) -> Result<PathBuf> {
    let config = FalconConfig::load_or_default(library_root)?;
    let status = evaluate_status(&config, item, library_root);
    match status {
        AssetStatus::Managed => Ok(item.managed_storage_path(library_root)),
        AssetStatus::Linked => {
            let record = load_validated_sidecar(&config, item, library_root)?.ok_or_else(|| {
                Error::AssetUnavailable {
                    id: item.id.clone(),
                    status: AssetStatus::InvalidReference,
                    path: None,
                    reason: "linked sidecar missing".into(),
                }
            })?;
            Ok(record.source_path)
        }
        AssetStatus::Offline | AssetStatus::Missing | AssetStatus::InvalidReference => {
            let last = load_validated_sidecar(&config, item, library_root)
                .ok()
                .flatten()
                .map(|r| r.source_path);
            Err(Error::AssetUnavailable {
                id: item.id.clone(),
                status,
                path: last,
                reason: format!("referenced source is {status}"),
            })
        }
    }
}

/// Authoritative fail-closed [`AssetStatus`] evaluation.
pub fn evaluate_status(config: &FalconConfig, item: &Item, library_root: &Path) -> AssetStatus {
    let expected_linked = config.linked_ids.contains(&item.id);
    let sidecar = sidecar_path(item, library_root);
    if !expected_linked && !sidecar.exists() {
        return AssetStatus::Managed;
    }
    let Ok(Some(record)) = load_validated_sidecar(config, item, library_root) else {
        return AssetStatus::InvalidReference;
    };
    if platform::volume_is_offline(&record.volume) {
        return AssetStatus::Offline;
    }
    match fs::metadata(&record.source_path) {
        Ok(meta) => {
            if !meta.is_file()
                || path_violates_library_containment(&record.source_path, library_root)
            {
                AssetStatus::InvalidReference
            } else {
                AssetStatus::Linked
            }
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {
            if platform::volume_is_offline(&record.volume) {
                AssetStatus::Offline
            } else {
                AssetStatus::Missing
            }
        }
        Err(e) if e.kind() == ErrorKind::PermissionDenied || is_transient_io_error(&e) => {
            // Reference itself is intact; OS permission or temporary I/O prevented verification.
            AssetStatus::Linked
        }
        Err(_) => AssetStatus::InvalidReference,
    }
}

/// Lock-free snapshot of an item's storage ownership for off-lock AI workers.
#[derive(Debug, Clone)]
pub struct AiSourceSnapshot {
    pub item_id: String,
    pub library_root: PathBuf,
    pub managed_path: PathBuf,
    pub sidecar_path: PathBuf,
    pub config_linked: bool,
}

impl AiSourceSnapshot {
    pub fn is_linked_hint(&self) -> bool {
        self.config_linked || self.sidecar_path.exists()
    }
}

/// Safe read-only AI source state classification distinguishing offline/missing/invalid
/// /permission/transient errors cleanly without misclassifying any of them.
pub fn evaluate_ai_source(
    config: &FalconConfig,
    item: &Item,
    library_root: &Path,
) -> AiSourceCondition {
    let snap = AiSourceSnapshot {
        item_id: item.id.clone(),
        library_root: library_root.to_path_buf(),
        managed_path: item.managed_storage_path(library_root),
        sidecar_path: sidecar_path(item, library_root),
        config_linked: config.linked_ids.contains(&item.id),
    };
    evaluate_ai_source_snapshot(&snap)
}

/// Evaluates [`AiSourceCondition`] outside the library lock from an [`AiSourceSnapshot`].
pub fn evaluate_ai_source_snapshot(snap: &AiSourceSnapshot) -> AiSourceCondition {
    if !snap.config_linked && !snap.sidecar_path.exists() {
        return classify_open_readability(&snap.managed_path, None);
    }
    let Ok(Some(record)) = load_validated_sidecar_at(
        &snap.sidecar_path,
        &snap.item_id,
        snap.config_linked,
        &snap.library_root,
    ) else {
        return AiSourceCondition::InvalidReference;
    };
    if platform::volume_is_offline(&record.volume) {
        return AiSourceCondition::Offline;
    }
    classify_open_readability(
        &record.source_path,
        Some((&record.volume, &snap.library_root)),
    )
}

fn classify_open_readability(
    path: &Path,
    linked_ctx: Option<(&VolumeIdentity, &Path)>,
) -> AiSourceCondition {
    match fs::metadata(path) {
        Ok(meta) => {
            if !meta.is_file() {
                return if linked_ctx.is_some() {
                    AiSourceCondition::InvalidReference
                } else {
                    AiSourceCondition::Missing
                };
            }
            if let Some((_, lib_root)) = linked_ctx
                && path_violates_library_containment(path, lib_root)
            {
                return AiSourceCondition::InvalidReference;
            }
        }
        Err(e) => return classify_io_err(&e, linked_ctx.map(|(v, _)| v)),
    }
    match File::open(path) {
        Ok(_) => AiSourceCondition::Available(path.to_path_buf()),
        Err(e) => classify_io_err(&e, linked_ctx.map(|(v, _)| v)),
    }
}

fn classify_io_err(err: &std::io::Error, volume: Option<&VolumeIdentity>) -> AiSourceCondition {
    match err.kind() {
        ErrorKind::PermissionDenied => AiSourceCondition::PermissionDenied,
        ErrorKind::NotFound => {
            if volume.is_some_and(platform::volume_is_offline) {
                AiSourceCondition::Offline
            } else {
                AiSourceCondition::Missing
            }
        }
        _ if is_transient_io_error(err) => AiSourceCondition::TemporarilyUnavailable,
        _ => {
            if volume.is_some() {
                AiSourceCondition::TemporarilyUnavailable
            } else {
                AiSourceCondition::Missing
            }
        }
    }
}

fn is_transient_io_error(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        ErrorKind::Interrupted | ErrorKind::WouldBlock | ErrorKind::TimedOut
    )
}

fn path_violates_library_containment(path: &Path, library_root: &Path) -> bool {
    let norm_path = normalize_lexical(path);
    let abs_lib =
        make_absolute_normalized(library_root).unwrap_or_else(|_| normalize_lexical(library_root));
    let can_lib = fs::canonicalize(&abs_lib).unwrap_or_else(|_| abs_lib.clone());
    if norm_path.starts_with(&abs_lib) || norm_path.starts_with(&can_lib) {
        return true;
    }
    if ancestor_hits_library(&norm_path, &abs_lib, &can_lib) {
        return true;
    }
    if let Ok(can_path) = fs::canonicalize(path) {
        return can_path.starts_with(&can_lib) || can_path.starts_with(&abs_lib);
    }
    false
}

fn make_absolute_normalized(path: &Path) -> Result<PathBuf> {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| Error::io(path, e))?
            .join(path)
    };
    Ok(normalize_lexical(&abs))
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if !matches!(
                    out.components().next_back(),
                    Some(Component::RootDir | Component::Prefix(_)) | None
                ) {
                    out.pop();
                }
            }
            c => out.push(c.as_os_str()),
        }
    }
    out
}

fn lexical_contained_in(abs_src: &Path, library_root: &Path) -> Result<bool> {
    let abs_lib = make_absolute_normalized(library_root)?;
    let can_lib = fs::canonicalize(&abs_lib).unwrap_or_else(|_| abs_lib.clone());
    Ok(abs_src.starts_with(&abs_lib) || abs_src.starts_with(&can_lib))
}

fn ancestor_hits_library(abs_src: &Path, abs_lib: &Path, can_lib: &Path) -> bool {
    let mut cur = Some(abs_src);
    while let Some(p) = cur {
        if p == abs_lib || p == can_lib {
            return true;
        }
        if let Ok(can_p) = fs::canonicalize(p)
            && (can_p == *can_lib || can_p == *abs_lib)
        {
            return true;
        }
        cur = p.parent();
    }
    false
}

fn compute_relative_hint(
    canonical_src: &Path,
    library_root: &Path,
    volume: &VolumeIdentity,
) -> Option<String> {
    if let Some(mp) = &volume.mount_point
        && let Ok(rel) = canonical_src.strip_prefix(mp)
    {
        return Some(rel.to_string_lossy().into_owned());
    }
    let can_lib = fs::canonicalize(library_root).ok()?;
    let lib_parent = can_lib.parent()?;
    canonical_src
        .strip_prefix(lib_parent)
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

fn to_ms(time: Option<std::time::SystemTime>) -> u64 {
    time.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as u64)
}

#[cfg(target_os = "macos")]
mod platform {
    use std::fs;
    use std::os::unix::fs::MetadataExt;
    use std::path::{Path, PathBuf};

    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use objc2_foundation::{
        NSArray, NSData, NSFileManager, NSNumber, NSString, NSURL, NSURLBookmarkCreationOptions,
        NSURLFileResourceIdentifierKey, NSURLVolumeIsInternalKey, NSURLVolumeIsRemovableKey,
        NSURLVolumeNameKey, NSURLVolumeUUIDStringKey, NSVolumeEnumerationOptions,
    };

    use super::VolumeIdentity;

    pub fn dev_and_inode(meta: &fs::Metadata) -> (Option<u64>, Option<u64>) {
        (Some(meta.dev()), Some(meta.ino()))
    }

    pub fn file_resource_id(path: &Path) -> Option<String> {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        let mut value = None;
        // SAFETY: NSURLFileResourceIdentifierKey yields an NSSecureCoding NSData/id object or nil.
        unsafe { url.getResourceValue_forKey_error(&mut value, NSURLFileResourceIdentifierKey) }
            .ok()?;
        let data = value?.downcast::<NSData>().ok()?;
        Some(STANDARD.encode(data.to_vec()))
    }

    pub fn create_bookmark(path: &Path) -> Option<String> {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        let data = url
            .bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
                NSURLBookmarkCreationOptions::empty(),
                None,
                None,
            )
            .ok()?;
        Some(STANDARD.encode(data.to_vec()))
    }

    pub fn volume_identity(path: &Path, meta: &fs::Metadata) -> VolumeIdentity {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        let volume_uuid = string_resource(&url, unsafe { NSURLVolumeUUIDStringKey });
        let volume_name = string_resource(&url, unsafe { NSURLVolumeNameKey });
        let is_removable =
            bool_resource(&url, unsafe { NSURLVolumeIsRemovableKey }).unwrap_or(false);
        let is_internal = bool_resource(&url, unsafe { NSURLVolumeIsInternalKey });
        let mount_point = volume_uuid
            .as_deref()
            .and_then(find_mount_point_for_uuid)
            .or_else(|| infer_mount_point(path));
        VolumeIdentity {
            volume_uuid,
            volume_name,
            mount_point,
            is_removable,
            is_internal,
            device_id: Some(meta.dev()),
        }
    }

    pub fn volume_is_offline(vol: &VolumeIdentity) -> bool {
        if let Some(uuid) = vol.volume_uuid.as_deref() {
            let mounted = mounted_volume_uuids();
            if !mounted.is_empty() && !mounted.iter().any(|m| m.eq_ignore_ascii_case(uuid)) {
                return true;
            }
        }
        if let Some(mp) = vol.mount_point.as_deref()
            && mp != Path::new("/")
            && !mp.exists()
        {
            return true;
        }
        false
    }

    fn string_resource(url: &NSURL, key: &objc2_foundation::NSURLResourceKey) -> Option<String> {
        let mut value = None;
        unsafe { url.getResourceValue_forKey_error(&mut value, key) }.ok()?;
        value?.downcast::<NSString>().ok().map(|s| s.to_string())
    }

    fn bool_resource(url: &NSURL, key: &objc2_foundation::NSURLResourceKey) -> Option<bool> {
        let mut value = None;
        unsafe { url.getResourceValue_forKey_error(&mut value, key) }.ok()?;
        value?.downcast::<NSNumber>().ok().map(|n| n.as_bool())
    }

    fn mounted_volumes() -> Option<objc2::rc::Retained<NSArray<NSURL>>> {
        let fm = NSFileManager::defaultManager();
        fm.mountedVolumeURLsIncludingResourceValuesForKeys_options(
            None,
            NSVolumeEnumerationOptions::empty(),
        )
    }

    fn mounted_volume_uuids() -> Vec<String> {
        let Some(urls) = mounted_volumes() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for url in urls.iter() {
            if let Some(uuid) = string_resource(&url, unsafe { NSURLVolumeUUIDStringKey }) {
                out.push(uuid);
            }
        }
        out
    }

    fn find_mount_point_for_uuid(uuid: &str) -> Option<PathBuf> {
        let urls = mounted_volumes()?;
        for url in urls.iter() {
            if string_resource(&url, unsafe { NSURLVolumeUUIDStringKey })
                .is_some_and(|u| u.eq_ignore_ascii_case(uuid))
                && let Some(p) = url.path()
            {
                return Some(PathBuf::from(p.to_string()));
            }
        }
        None
    }

    fn infer_mount_point(path: &Path) -> Option<PathBuf> {
        let mut comps = path.components();
        if matches!(comps.next(), Some(std::path::Component::RootDir))
            && matches!(comps.next(), Some(std::path::Component::Normal(s)) if s == "Volumes")
            && let Some(std::path::Component::Normal(vol)) = comps.next()
        {
            return Some(PathBuf::from("/Volumes").join(vol));
        }
        Some(PathBuf::from("/"))
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::VolumeIdentity;

    pub fn dev_and_inode(meta: &fs::Metadata) -> (Option<u64>, Option<u64>) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            (Some(meta.dev()), Some(meta.ino()))
        }
        #[cfg(not(unix))]
        {
            let _ = meta;
            (None, None)
        }
    }

    pub fn file_resource_id(_: &Path) -> Option<String> {
        None
    }

    pub fn create_bookmark(_: &Path) -> Option<String> {
        None
    }

    pub fn volume_identity(path: &Path, meta: &fs::Metadata) -> VolumeIdentity {
        let (device_id, _) = dev_and_inode(meta);
        VolumeIdentity {
            volume_uuid: None,
            volume_name: None,
            mount_point: infer_mount_point(path),
            is_removable: false,
            is_internal: None,
            device_id,
        }
    }

    pub fn volume_is_offline(vol: &VolumeIdentity) -> bool {
        if let Some(mp) = vol.mount_point.as_deref()
            && mp != Path::new("/")
            && !mp.exists()
        {
            return true;
        }
        false
    }

    fn infer_mount_point(path: &Path) -> Option<PathBuf> {
        #[cfg(windows)]
        {
            use std::path::Component;
            let mut comps = path.components();
            if let Some(Component::Prefix(prefix)) = comps.next() {
                return Some(PathBuf::from(prefix.as_os_str()).join("\\"));
            }
            None
        }
        #[cfg(not(windows))]
        {
            let _ = path;
            Some(PathBuf::from("/"))
        }
    }
}
