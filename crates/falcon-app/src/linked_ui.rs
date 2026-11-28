//! Pure presentation, availability classification, and safety-gating rules
//! for Managed (`Copy`) vs Referenced (`Link (Place)`) assets.

use falcon_library::Library;
use falcon_library::linked::{AiSourceCondition, LinkRecord};

use crate::locale::{tr, tr_n, trf};

/// Granular asset storage and source availability state for UI rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UiAvailability {
    Managed,
    Linked,
    Offline,
    Missing,
    InvalidReference,
    PermissionDenied,
    TemporarilyUnavailable,
}

impl UiAvailability {
    /// Whether this item is tracked as a referenced asset rather than Managed.
    pub(crate) fn is_linked(self) -> bool {
        !matches!(self, Self::Managed)
    }

    /// Whether the linked external original cannot currently be opened.
    pub(crate) fn is_unavailable(self) -> bool {
        !matches!(self, Self::Managed | Self::Linked)
    }

    /// Concise localized status string shown in the Inspector Status row.
    pub(crate) fn status_label(self) -> &'static str {
        match self {
            Self::Managed => tr("Managed (In Library)"),
            Self::Linked => tr("Available (Linked)"),
            Self::Offline => tr("Offline (Volume Disconnected)"),
            Self::Missing => tr("Missing Original"),
            Self::InvalidReference => tr("Invalid Reference"),
            Self::PermissionDenied => tr("Permission Denied"),
            Self::TemporarilyUnavailable => tr("Temporarily Unavailable"),
        }
    }

    /// Hover tooltip for compact grid/list status indicators.
    pub(crate) fn badge_tooltip(self) -> &'static str {
        match self {
            Self::Managed => tr("Managed — File is copied inside this library"),
            Self::Linked => tr("Linked — Original file stays at its source location"),
            Self::Offline => {
                tr("Offline — Volume or drive is not connected (showing cached preview)")
            }
            Self::Missing => {
                tr("Missing — Referenced original file was not found (showing cached preview)")
            }
            Self::InvalidReference => tr("Invalid Reference — Link sidecar is corrupt or unsafe"),
            Self::PermissionDenied => {
                tr("Permission Denied — Original file cannot be read with current permissions")
            }
            Self::TemporarilyUnavailable => {
                tr("Temporarily Unavailable — Original file could not be accessed right now")
            }
        }
    }

    /// Distinct viewer notice when a linked source file cannot be opened.
    pub(crate) fn viewer_notice(self) -> Option<&'static str> {
        match self {
            Self::Managed | Self::Linked => None,
            Self::Offline => Some(tr(
                "Original volume or drive is offline. Reconnect it to view or open the original file.",
            )),
            Self::Missing => Some(tr(
                "Referenced original file could not be found at its last known location.",
            )),
            Self::InvalidReference => Some(tr("Link reference record is invalid or unreadable.")),
            Self::PermissionDenied => Some(tr(
                "Permission denied when trying to read the referenced original file.",
            )),
            Self::TemporarilyUnavailable => {
                Some(tr("Referenced original file is temporarily unavailable."))
            }
        }
    }
}

/// Classifies an item's storage ownership and current readability without
/// performing any network operations or opening payload contents.
pub(crate) fn classify_asset(lib: &Library, id: &str) -> UiAvailability {
    if !lib.is_linked_item(id) {
        return UiAvailability::Managed;
    }
    // Checking a linked source touches the disk (an external or network volume
    // can take long), and cards ask every frame, so the answer is kept for a few seconds.
    thread_local! {
        static SEEN: std::cell::RefCell<std::collections::HashMap<(std::path::PathBuf, String), (std::time::Instant, UiAvailability)>> = Default::default();
    }
    if cfg!(test) {
        return classify_linked_now(lib, id);
    }
    let key = (lib.root().to_path_buf(), id.to_string());
    // A reachable source is rechecked rarely; a problem is rechecked sooner so a reconnected drive shows up.
    let fresh = |at: &std::time::Instant, v: &UiAvailability| at.elapsed() < std::time::Duration::from_secs(if *v == UiAvailability::Linked { 30 } else { 5 });
    if let Some(v) = SEEN.with(|s| s.borrow().get(&key).filter(|(at, v)| fresh(at, v)).map(|(_, v)| *v)) {
        return v;
    }
    let v = classify_linked_now(lib, id);
    SEEN.with(|s| {
        let mut s = s.borrow_mut();
        if s.len() > 20_000 {
            s.clear();
        }
        s.insert(key, (std::time::Instant::now(), v));
    });
    v
}

fn classify_linked_now(lib: &Library, id: &str) -> UiAvailability {
    match lib.ai_source_condition(id) {
        AiSourceCondition::Available(_) => UiAvailability::Linked,
        AiSourceCondition::Offline => UiAvailability::Offline,
        AiSourceCondition::Missing => UiAvailability::Missing,
        AiSourceCondition::InvalidReference => UiAvailability::InvalidReference,
        AiSourceCondition::PermissionDenied => UiAvailability::PermissionDenied,
        AiSourceCondition::TemporarilyUnavailable => UiAvailability::TemporarilyUnavailable,
    }
}

/// Breakdown of managed vs linked items across a selection or Trash scope.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SelectionStorageSummary {
    pub managed: usize,
    pub linked: usize,
}

impl SelectionStorageSummary {
    pub(crate) fn is_all_linked(self) -> bool {
        self.linked > 0 && self.managed == 0
    }

    pub(crate) fn is_mixed(self) -> bool {
        self.linked > 0 && self.managed > 0
    }
}

/// Counts managed vs linked items for a list of item ids.
pub(crate) fn summarize_ids(lib: &Library, ids: &[String]) -> SelectionStorageSummary {
    let mut summary = SelectionStorageSummary::default();
    for id in ids {
        if lib.item(id).is_none() {
            continue;
        }
        if lib.is_linked_item(id) {
            summary.linked += 1;
        } else {
            summary.managed += 1;
        }
    }
    summary
}

/// Filters `ids` to Managed items only and returns `(managed_ids, skipped_linked_count)`
/// so destructive original-file operations never alter linked originals.
pub(crate) fn partition_managed_targets(lib: &Library, ids: &[String]) -> (Vec<String>, usize) {
    let mut managed = Vec::with_capacity(ids.len());
    let mut linked = 0;
    for id in ids {
        if lib.item(id).is_none() {
            continue;
        }
        if lib.is_linked_item(id) {
            linked += 1;
        } else {
            managed.push(id.clone());
        }
    }
    (managed, linked)
}

/// Formats confirmation copy explaining permanent removal clearly for
/// Managed-only, Linked-only, and Mixed selections.
pub(crate) fn permanent_delete_detail(summary: SelectionStorageSummary) -> String {
    if summary.is_all_linked() {
        return tr("Only the library entry, link record, and cached preview are removed. The original files on disk remain untouched.").to_string();
    }
    if summary.is_mixed() {
        let managed = tr_n(summary.managed, "{n} managed file", "{n} managed files");
        let linked = tr_n(
            summary.linked,
            "{n} linked reference",
            "{n} linked references",
        );
        return trf(
            "{managed} will be removed from the library and cannot be restored. {linked} will only remove library metadata and cached previews — original files on disk remain untouched.",
            &[("managed", &managed), ("linked", &linked)],
        );
    }
    tr("The files are removed from the library and cannot be restored.").to_string()
}

/// Formats confirmation copy when moving items to Trash so linked items explain
/// clearly that original files on disk remain untouched.
pub(crate) fn trash_linked_note(summary: SelectionStorageSummary) -> Option<&'static str> {
    if summary.linked == 0 {
        None
    } else if summary.is_all_linked() {
        Some(tr("Linked original files on disk stay untouched."))
    } else {
        Some(tr(
            "For linked items, their original files on disk stay untouched.",
        ))
    }
}

/// Formats human-friendly source volume information from a link sidecar.
pub(crate) fn format_volume_summary(record: &LinkRecord) -> Option<String> {
    let name = record
        .volume
        .volume_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let mount = record
        .volume
        .mount_point
        .as_ref()
        .map(|p| p.display().to_string());
    let base = match (name, mount) {
        (Some(n), _) => n.to_string(),
        (None, Some(m)) => m,
        (None, None) => return None,
    };
    if record.volume.is_removable {
        Some(trf("{volume} (External)", &[("volume", &base)]))
    } else {
        Some(base)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use falcon_library::Library;
    use falcon_library::import::ImportOptions;
    use falcon_library::linked::{FileIdentity, LINK_SCHEMA_VERSION, VolumeIdentity};
    use falcon_library::storage_mode::StorageMode;

    use super::*;

    fn temp_dir(label: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("falcon-linked-ui-{label}-"))
            .tempdir()
            .expect("tempdir")
    }

    fn tiny_png(path: &std::path::Path) {
        let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([20, 120, 220, 255]));
        img.save(path).expect("save png");
    }

    #[test]
    fn classifies_managed_linked_missing_and_counts() {
        let dir = temp_dir("classify");
        let lib_dir = dir.path().join("Test.library");
        let mut lib = Library::create(&lib_dir).expect("create");
        let src_a = dir.path().join("managed.png");
        let src_b = dir.path().join("linked.png");
        tiny_png(&src_a);
        tiny_png(&src_b);

        lib.set_storage_mode(StorageMode::Copy).expect("copy mode");
        let managed_id = lib
            .import_file(&src_a, &ImportOptions::default())
            .expect("copy import");
        lib.set_storage_mode(StorageMode::Link).expect("link mode");
        let linked_id = lib
            .import_file(&src_b, &ImportOptions::default())
            .expect("link import");

        assert_eq!(classify_asset(&lib, &managed_id), UiAvailability::Managed);
        assert_eq!(classify_asset(&lib, &linked_id), UiAvailability::Linked);

        let mix = [managed_id.clone(), linked_id.clone()];
        assert_eq!(
            summarize_ids(&lib, &mix),
            SelectionStorageSummary {
                managed: 1,
                linked: 1
            }
        );
        let (only_managed, skipped) = partition_managed_targets(&lib, &mix);
        assert_eq!(only_managed, vec![managed_id.clone()]);
        assert_eq!(skipped, 1);

        // Removing the linked original makes status Missing cleanly, while Managed stays unaffected.
        fs::remove_file(&src_b).expect("remove source");
        assert_eq!(classify_asset(&lib, &linked_id), UiAvailability::Missing);
        assert_eq!(classify_asset(&lib, &managed_id), UiAvailability::Managed);
    }

    #[test]
    fn formats_removable_volume_summary() {
        let record = LinkRecord {
            schema_version: LINK_SCHEMA_VERSION,
            item_id: "TEST".into(),
            source_path: PathBuf::from("/Volumes/SSD/photo.jpg"),
            bookmark_base64: None,
            volume: VolumeIdentity {
                volume_uuid: None,
                volume_name: Some("SanDisk SSD".into()),
                mount_point: Some(PathBuf::from("/Volumes/SanDisk SSD")),
                is_removable: true,
                is_internal: Some(false),
                device_id: None,
            },
            identity: FileIdentity::default(),
            relative_hint: None,
            linked_at_ms: 0,
            last_verified_at_ms: 0,
        };
        assert_eq!(
            format_volume_summary(&record).as_deref(),
            Some("SanDisk SSD (External)")
        );
    }
}
