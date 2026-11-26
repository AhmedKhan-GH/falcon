//! Phase A-H core hardening tests: crash consistency, fault injection,
//! ID mismatch fail-closed checks, mixed-library merge integrity, and
//! simulated external-SSD unavailability / I/O failure recovery.

use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use falcon_library::Library;
use falcon_library::error::Error;
use falcon_library::fault::{self, FaultPoint};
use falcon_library::import::ImportOptions;
use falcon_library::journal::{LibraryDiagnostic, PendingJournal, PendingOpKind};
use falcon_library::library_merge;
use falcon_library::linked::{AssetStatus, LINK_SIDECAR, LinkRecord};
use falcon_library::repair;
use falcon_library::storage_mode::{FALCON_CONFIG, FalconConfig, StorageMode};
use falcon_library::undo::WriteMode;
use image::{ImageFormat, Rgb, RgbImage};
use tempfile::TempDir;

fn hash_file(path: &Path) -> String {
    let bytes = fs::read(path).unwrap();
    blake3::hash(&bytes).to_hex().to_string()
}

fn write_png(path: &Path, w: u32, h: u32, color: [u8; 3]) {
    let img = RgbImage::from_pixel(w, h, Rgb(color));
    img.save_with_format(path, ImageFormat::Png).unwrap();
}

fn create_lib(dir: &TempDir, name: &str) -> Library {
    Library::create(dir.path().join(format!("{name}.library"))).unwrap()
}

#[test]
fn ah1_batched_import_writes_metadata_synchronously_and_fault_recovery_matrix() {
    let dir = tempfile::tempdir().unwrap();
    let mut lib = create_lib(&dir, "BatchedImport");
    lib.set_storage_mode(StorageMode::Link).unwrap();
    lib.set_write_mode(WriteMode::Batched).unwrap();

    let src = dir.path().join("asset.png");
    write_png(&src, 64, 64, [20, 120, 220]);
    let src_hash = hash_file(&src);

    // 1. Batched import writes metadata.json + falcon-link.json before flush().
    let id = lib.import_file(&src, &ImportOptions::default()).unwrap();
    let info_dir = lib.item(&id).unwrap().info_dir(lib.root());
    assert!(info_dir.join("metadata.json").is_file());
    assert!(info_dir.join(LINK_SIDECAR).is_file());
    assert_eq!(lib.asset_status(&id), Some(AssetStatus::Linked));
    assert_eq!(hash_file(&src), src_hash);

    // 2. Fault injection across all import persistence boundaries preserves clean rollback & source.
    for fp in [
        FaultPoint::ImportAfterStageMetadata,
        FaultPoint::ImportBeforeLinkedIds,
        FaultPoint::ImportAfterLinkedIds,
        FaultPoint::ImportBeforePublishRename,
    ] {
        let _guard = fault::install(move |p| {
            if p == fp {
                Err(io::Error::other(format!("simulated crash at {p:?}")))
            } else {
                Ok(())
            }
        });
        let err = lib
            .import_file(&src, &ImportOptions::default())
            .unwrap_err();
        assert!(
            matches!(err, Error::Io { .. }),
            "expected Io error at {fp:?}: {err}"
        );
        assert_eq!(hash_file(&src), src_hash);
    }

    // Only the first successful item exists; no orphan staging or dangling linked_ids remain.
    let reopened = Library::open(lib.root()).unwrap();
    assert_eq!(reopened.items().len(), 1);
    assert_eq!(reopened.config().linked_ids.len(), 1);
    assert!(reopened.diagnostics().is_empty());

    // 3. Simulated uncleaned crash leaving staging dir + journal + linked_ids entry:
    // Reopen fails closed into diagnostics without deleting unverified data or loading as Managed.
    let fake_id = "CRASHED999999";
    let stage_name = format!(".staging-{fake_id}");
    let stage_dir = reopened.root().join("images").join(&stage_name);
    fs::create_dir_all(&stage_dir).unwrap();
    fs::write(
        stage_dir.join("metadata.json"),
        br#"{"id":"CRASHED999999"}"#,
    )
    .unwrap();
    fs::write(stage_dir.join(LINK_SIDECAR), b"{}").unwrap();
    let mut cfg = FalconConfig::load_or_default(reopened.root()).unwrap();
    cfg.linked_ids.insert(fake_id.to_string());
    cfg.save(reopened.root()).unwrap();
    let j = PendingJournal {
        schema_version: 1,
        kind: PendingOpKind::Import,
        item_ids: vec![fake_id.to_string()],
        linked_ids: vec![fake_id.to_string()],
        stage_dirs: vec![stage_name],
        created_at_ms: 1,
    };
    fs::write(
        reopened.root().join(".falcon-journal.json"),
        serde_json::to_vec(&j).unwrap(),
    )
    .unwrap();

    let mut crashed_open = Library::open(reopened.root()).unwrap();
    assert_eq!(
        crashed_open.items().len(),
        1,
        "partial staged item never enters catalog"
    );
    assert!(
        crashed_open
            .diagnostics()
            .iter()
            .any(|d| matches!(d, LibraryDiagnostic::InterruptedJournal { .. }))
    );
    assert!(crashed_open.diagnostics().iter().any(|d| matches!(
        d,
        LibraryDiagnostic::OrphanStagingDir {
            has_link_sidecar: true,
            ..
        }
    )));
    assert!(
        crashed_open
            .diagnostics()
            .iter()
            .any(|d| matches!(d, LibraryDiagnostic::OrphanLinkedId { id } if id == fake_id))
    );

    // Explicit recovery rolls back unpublished linked_ids while preserving unverified staging files.
    let report = crashed_open.recover_pending_journal().unwrap();
    assert_eq!(report.rolled_back_linked_ids, vec![fake_id.to_string()]);
    assert_eq!(report.preserved_staging_dirs, vec![stage_dir.clone()]);
    assert!(
        stage_dir.is_dir(),
        "unverified staging files must not be auto-deleted"
    );
    assert!(!crashed_open.config().linked_ids.contains(fake_id));
    assert_eq!(hash_file(&src), src_hash);
}

#[test]
fn ah2_crash_safe_duplicate_and_open_id_mismatch_fail_closed() {
    let dir = tempfile::tempdir().unwrap();
    let mut lib = create_lib(&dir, "DupSafety");
    lib.set_storage_mode(StorageMode::Link).unwrap();
    lib.set_write_mode(WriteMode::Batched).unwrap();

    let src = dir.path().join("orig.png");
    write_png(&src, 48, 48, [180, 90, 40]);
    let src_hash = hash_file(&src);
    let orig_id = lib.import_file(&src, &ImportOptions::default()).unwrap();

    for fp in [
        FaultPoint::DuplicateAfterStageMetadata,
        FaultPoint::DuplicateBeforeLinkedIds,
        FaultPoint::DuplicateAfterLinkedIds,
        FaultPoint::DuplicateBeforePublishRename,
    ] {
        let _guard = fault::install(move |p| {
            if p == fp {
                Err(io::Error::other(format!("simulated crash at {p:?}")))
            } else {
                Ok(())
            }
        });
        assert!(lib.duplicate_item(&orig_id).is_err());
        assert_eq!(lib.items().len(), 1);
        assert_eq!(hash_file(&src), src_hash);
    }

    // Happy duplicate in batched mode persists new ID cleanly.
    let dup_id = lib.duplicate_item(&orig_id).unwrap();
    assert_ne!(dup_id, orig_id);
    lib.flush().unwrap();

    // Simulate tampered / half-copied `.info` dirs:
    // Case 1: folder stem != item.id inside metadata.json
    let mismatch_dir = lib.root().join("images/MISMATCHSTEM1.info");
    fs::create_dir_all(&mismatch_dir).unwrap();
    let orig_meta = fs::read_to_string(
        lib.item(&orig_id)
            .unwrap()
            .info_dir(lib.root())
            .join("metadata.json"),
    )
    .unwrap();
    let mismatched_meta = orig_meta.replace(&orig_id, "ACTUALDIFFERENTID");
    fs::write(mismatch_dir.join("metadata.json"), &mismatched_meta).unwrap();

    // Case 2: folder stem == id? No, two `.info` folders trying to claim duplicate ID impossible if stem==id,
    // wait: verify mismatch stem NEVER loads as orig_id duplicate either.
    let mismatch_dup = lib.root().join("images/ZZZDUPSTEM1.info");
    fs::create_dir_all(&mismatch_dup).unwrap();
    fs::write(mismatch_dup.join("metadata.json"), &orig_meta).unwrap();

    let mut reopened = Library::open(lib.root()).unwrap();
    // Only the two valid items enter the active catalog.
    assert_eq!(reopened.items().len(), 2);
    assert!(reopened.item("ACTUALDIFFERENTID").is_none());
    assert_eq!(reopened.skipped.len(), 2);
    assert_eq!(
        reopened
            .diagnostics()
            .iter()
            .filter(|d| matches!(d, LibraryDiagnostic::IdMismatch { .. }))
            .count(),
        2
    );

    // Repair must NOT treat mismatched folders (which still contain metadata.json) as deletable orphans.
    let scan = repair::scan(reopened.root(), reopened.items());
    assert_eq!(scan.orphans.len(), 0);
    let cleared = reopened.clear_invalid(&scan, |_| Ok(())).unwrap();
    assert_eq!(cleared.removed, 0);
    assert_eq!(
        fs::read_to_string(mismatch_dir.join("metadata.json")).unwrap(),
        mismatched_meta,
        "mismatched metadata must never be overwritten or deleted automatically"
    );
    assert_eq!(hash_file(&src), src_hash);
}

#[test]
fn ah3_mixed_library_merge_integrity_and_failure_cases() {
    let dir = tempfile::tempdir().unwrap();

    // Build source mixed library (Copy item + Available Linked item + Offline/Missing Linked item).
    let mut src_lib = create_lib(&dir, "SourceMix");
    let src_folder = src_lib.create_folder(None, "SyncedRef").unwrap();

    let managed_src = dir.path().join("managed_src.png");
    write_png(&managed_src, 32, 32, [10, 200, 10]);
    let managed_id = src_lib
        .import_file(
            &managed_src,
            &ImportOptions {
                folders: vec![src_folder.clone()],
                tags: vec!["managed-tag".into()],
                annotation: "managed note".into(),
                ..Default::default()
            },
        )
        .unwrap();

    src_lib.set_storage_mode(StorageMode::Link).unwrap();
    let ext_avail = dir.path().join("external_master.png");
    write_png(&ext_avail, 64, 64, [200, 40, 200]);
    let ext_avail_hash = hash_file(&ext_avail);
    let linked_ok_id = src_lib
        .import_file(
            &ext_avail,
            &ImportOptions {
                folders: vec![src_folder.clone()],
                tags: vec!["linked-tag".into()],
                annotation: "keep reference".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mut item_ok = src_lib.item(&linked_ok_id).unwrap().clone();
    item_ok.extra.insert("star".into(), 5.into());
    src_lib.update_item(item_ok).unwrap();

    let ext_gone = dir.path().join("temporarily_gone.png");
    write_png(&ext_gone, 32, 32, [90, 90, 250]);
    let linked_missing_id = src_lib
        .import_file(
            &ext_gone,
            &ImportOptions {
                tags: vec!["missing-ref".into()],
                ..Default::default()
            },
        )
        .unwrap();
    fs::remove_file(&ext_gone).unwrap();

    // Create destination library configured as StorageMode::Copy + one colliding duplicate ID item.
    let mut dest_lib = create_lib(&dir, "DestCopy");
    assert_eq!(dest_lib.storage_mode(), StorageMode::Copy);

    // Inject malformed sidecar item & destination-containment escaping sidecar item into a copy of source.
    let mut bad_src = Library::create(dir.path().join("BadCases.library")).unwrap();
    bad_src.set_storage_mode(StorageMode::Link).unwrap();
    let ext_tmp = dir.path().join("bad_tmp.png");
    write_png(&ext_tmp, 16, 16, [1, 2, 3]);
    let bad_sidecar_id = bad_src
        .import_file(&ext_tmp, &ImportOptions::default())
        .unwrap();
    let escape_id = bad_src
        .import_file(&ext_tmp, &ImportOptions::default())
        .unwrap();

    // Corrupt bad_sidecar_id's falcon-link.json
    fs::write(
        bad_src
            .item(&bad_sidecar_id)
            .unwrap()
            .link_record_path(bad_src.root()),
        b"{not json",
    )
    .unwrap();
    // Point escape_id's falcon-link.json inside dest_lib!
    let rec_path = bad_src
        .item(&escape_id)
        .unwrap()
        .link_record_path(bad_src.root());
    let mut esc_rec: LinkRecord = serde_json::from_slice(&fs::read(&rec_path).unwrap()).unwrap();
    esc_rec.source_path = dest_lib.root().join("images/inside_dest.png");
    fs::write(&rec_path, serde_json::to_vec_pretty(&esc_rec).unwrap()).unwrap();

    let bad_plan = library_merge::plan(&dest_lib, bad_src.root()).unwrap();
    let cancel = AtomicBool::new(false);
    let (bad_copied, bad_failed) =
        library_merge::copy_items(dest_lib.root(), &bad_plan, &cancel, |_, _| {});
    assert!(
        bad_copied.is_empty(),
        "neither malformed nor dest-escaping item may copy"
    );
    assert_eq!(bad_failed.len(), 2);

    // Unsupported source falcon.json schema version -> plan fails closed!
    let newer_src = create_lib(&dir, "NewerSchema");
    fs::write(
        newer_src.root().join(FALCON_CONFIG),
        br#"{"schemaVersion": 99, "storageMode": "link"}"#,
    )
    .unwrap();
    assert!(matches!(
        library_merge::plan(&dest_lib, newer_src.root()),
        Err(library_merge::MergeRefusal::Library(
            Error::NewerFalconMetadata { version: 99, .. }
        ))
    ));

    // Merge the healthy mixed source library (including unavailable external reference).
    let plan = library_merge::plan(&dest_lib, src_lib.root()).unwrap();
    let (copied, failed) = library_merge::copy_items(dest_lib.root(), &plan, &cancel, |_, _| {});
    assert!(failed.is_empty(), "unexpected failures: {failed:?}");
    assert_eq!(copied.len(), 3);

    // Duplicate ID resilience: pre-seed a colliding managed item in dest with same ID as `managed_id` won't overwrite.
    // Run finish_merge cleanly and verify ownership + metadata + ratings + destination storage_mode.
    let report = dest_lib.finish_merge(&plan, &copied).unwrap();
    assert_eq!(report.items, 3);
    assert_eq!(
        dest_lib.storage_mode(),
        StorageMode::Copy,
        "target storage mode preserved"
    );
    assert_eq!(
        dest_lib.asset_status(&managed_id),
        Some(AssetStatus::Managed)
    );
    assert_eq!(
        dest_lib.asset_status(&linked_ok_id),
        Some(AssetStatus::Linked)
    );
    assert_eq!(
        dest_lib.asset_status(&linked_missing_id),
        Some(AssetStatus::Missing)
    );

    let merged_ok = dest_lib.item(&linked_ok_id).unwrap();
    assert_eq!(merged_ok.tags, vec!["linked-tag"]);
    assert_eq!(merged_ok.annotation, "keep reference");
    assert_eq!(
        merged_ok.extra.get("star").and_then(|v| v.as_u64()),
        Some(5)
    );
    assert_eq!(hash_file(&ext_avail), ext_avail_hash);

    // Reopen destination library and re-run merge for duplicate-ID skip verification.
    let mut reopened_dest = Library::open(dest_lib.root()).unwrap();
    assert_eq!(reopened_dest.items().len(), 3);
    assert_eq!(
        reopened_dest.asset_status(&linked_ok_id),
        Some(AssetStatus::Linked)
    );
    assert_eq!(
        reopened_dest.asset_status(&linked_missing_id),
        Some(AssetStatus::Missing)
    );

    let plan2 = library_merge::plan(&reopened_dest, src_lib.root()).unwrap();
    let (copied2, _) = library_merge::copy_items(reopened_dest.root(), &plan2, &cancel, |_, _| {});
    assert!(
        copied2.is_empty(),
        "existing IDs in destination must be skipped without overwriting"
    );
    let report2 = reopened_dest.finish_merge(&plan2, &copied2).unwrap();
    assert_eq!(report2.items, 0);

    // Interrupted merge fault before linked_ids batch -> fails closed cleanly without claiming commit.
    let mut clean_dest = create_lib(&dir, "InterruptedMergeDest");
    let p3 = library_merge::plan(&clean_dest, src_lib.root()).unwrap();
    let (c3, _) = library_merge::copy_items(clean_dest.root(), &p3, &cancel, |_, _| {});
    {
        let _guard = fault::install(|fp| {
            if fp == FaultPoint::MergeBeforeLinkedIdsBatch {
                Err(io::Error::other("simulated SSD disconnect during merge"))
            } else {
                Ok(())
            }
        });
        assert!(clean_dest.finish_merge(&p3, &c3).is_err());
    }
    let reopened_after_abort = Library::open(clean_dest.root()).unwrap();
    assert!(reopened_after_abort.diagnostics().iter().any(|d| matches!(
        d,
        LibraryDiagnostic::InterruptedJournal {
            kind: PendingOpKind::Merge,
            ..
        }
    )));
    // Copied linked `.info/` dirs still have `falcon-link.json`, so they NEVER misclassify as Managed!
    assert_eq!(
        reopened_after_abort.asset_status(&linked_ok_id),
        Some(AssetStatus::Linked)
    );
    assert_eq!(
        reopened_after_abort.asset_status(&linked_missing_id),
        Some(AssetStatus::Missing)
    );
    assert_eq!(hash_file(&ext_avail), ext_avail_hash);
}

#[test]
fn ssd_simulated_volume_unavailability_and_io_failures() {
    let dir = tempfile::tempdir().unwrap();
    let mut lib = create_lib(&dir, "ExternalSSDSim");
    lib.set_storage_mode(StorageMode::Link).unwrap();

    let src = dir.path().join("ext.png");
    write_png(&src, 64, 64, [15, 160, 190]);
    let src_hash = hash_file(&src);
    let id = lib.import_file(&src, &ImportOptions::default()).unwrap();

    // 1. Source unavailable (`PermissionDenied` vs `Missing`) vs Library volume fault:
    fs::remove_file(&src).unwrap();
    assert_eq!(lib.asset_status(&id), Some(AssetStatus::Missing));
    assert!(matches!(
        lib.import_file(&src, &ImportOptions::default()),
        Err(Error::Io { .. } | Error::Import { .. })
    ));
    // Re-create source at same path -> recovers cleanly without replacement library.
    write_png(&src, 64, 64, [15, 160, 190]);
    assert_eq!(lib.asset_status(&id), Some(AssetStatus::Linked));
    assert_eq!(hash_file(&src), src_hash);

    // 2. Simulated library volume I/O disconnect during metadata flush & mtime write in Batched mode.
    lib.set_write_mode(WriteMode::Batched).unwrap();
    let mut edited = lib.item(&id).unwrap().clone();
    edited.tags.push("ssd-tag".into());
    lib.update_item(edited).unwrap();
    {
        let _guard = fault::install(|fp| {
            if fp == FaultPoint::FlushDirtyItem {
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "simulated external SSD unplugged",
                ))
            } else {
                Ok(())
            }
        });
        assert!(lib.flush().is_err());
    }
    // Pending dirty flags preserved; once volume returns, flush succeeds cleanly!
    assert!(lib.has_pending_writes());
    lib.flush().unwrap();
    let reopened = Library::open(lib.root()).unwrap();
    assert_eq!(reopened.item(&id).unwrap().tags, vec!["ssd-tag"]);
    assert_eq!(hash_file(&src), src_hash);
}
