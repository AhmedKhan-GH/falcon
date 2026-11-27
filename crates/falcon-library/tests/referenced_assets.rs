//! Phase A acceptance tests for Referenced Assets (`Copy` vs `Link` storage mode).

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use falcon_library::error::Error;
use falcon_library::export::{self, Collision};
use falcon_library::import::{self, ImportOptions};
use falcon_library::linked::{AssetStatus, LINK_SIDECAR, LinkRecord};
use falcon_library::pack::{self, PackImportOptions, PackSource};
use falcon_library::repair;
use falcon_library::storage_mode::{FALCON_CONFIG, StorageMode};
use falcon_library::transform::{Flip, PixelRect};
use falcon_library::{Library, webp_convert};
use image::{DynamicImage, Rgb, RgbImage};
use tempfile::TempDir;

fn hash_file(path: &Path) -> String {
    let bytes = fs::read(path).unwrap();
    blake3::hash(&bytes).to_hex().to_string()
}

fn write_png(path: &Path, w: u32, h: u32, color: [u8; 3]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let img = RgbImage::from_pixel(w, h, Rgb(color));
    DynamicImage::ImageRgb8(img).save(path).unwrap();
}

fn write_jpg(path: &Path, w: u32, h: u32, color: [u8; 3]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let img = RgbImage::from_pixel(w, h, Rgb(color));
    DynamicImage::ImageRgb8(img)
        .save_with_format(path, image::ImageFormat::Jpeg)
        .unwrap();
}

fn create_lib(dir: &TempDir, name: &str) -> (PathBuf, Library) {
    let root = dir.path().join(format!("{name}.library"));
    let lib = Library::create(&root).unwrap();
    (root, lib)
}

#[test]
fn test_01_default_storage_mode_is_copy_and_legacy_has_no_falcon_json() {
    let dir = tempfile::tempdir().unwrap();
    let (root, mut lib) = create_lib(&dir, "Legacy");
    assert_eq!(lib.storage_mode(), StorageMode::Copy);
    assert!(
        !root.join(FALCON_CONFIG).exists(),
        "opening/creating default Copy library writes no premature falcon.json"
    );

    let src = dir.path().join("sample.png");
    write_png(&src, 64, 64, [20, 120, 200]);
    let id = lib
        .import_file(
            &src,
            &ImportOptions {
                palette: false,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(lib.asset_status(&id), Some(AssetStatus::Managed));
    assert!(lib.item(&id).unwrap().managed_storage_path(&root).is_file());
    assert!(!lib.item(&id).unwrap().link_record_path(&root).exists());
}

#[test]
fn test_02_mode_persistence_and_no_auto_migration_on_switch() {
    let dir = tempfile::tempdir().unwrap();
    let (root, mut lib) = create_lib(&dir, "Mixed");

    let managed_src = dir.path().join("managed.png");
    write_png(&managed_src, 64, 64, [200, 30, 30]);
    let m_id = lib
        .import_file(
            &managed_src,
            &ImportOptions {
                palette: false,
                ..Default::default()
            },
        )
        .unwrap();

    // Switch to Link; existing managed item must remain Managed (zero auto-migration).
    lib.set_storage_mode(StorageMode::Link).unwrap();
    assert_eq!(lib.storage_mode(), StorageMode::Link);
    assert_eq!(lib.asset_status(&m_id), Some(AssetStatus::Managed));

    let linked_src = dir.path().join("linked.png");
    write_png(&linked_src, 64, 64, [30, 200, 30]);
    let l_id = lib
        .import_file(
            &linked_src,
            &ImportOptions {
                palette: false,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(lib.asset_status(&l_id), Some(AssetStatus::Linked));

    // Switch back to Copy; linked item stays Linked, next import is Managed.
    lib.set_storage_mode(StorageMode::Copy).unwrap();
    let lib = Library::open(&root).unwrap();
    assert_eq!(lib.storage_mode(), StorageMode::Copy);
    assert_eq!(lib.asset_status(&m_id), Some(AssetStatus::Managed));
    assert_eq!(lib.asset_status(&l_id), Some(AssetStatus::Linked));
}

#[test]
fn test_03_mixed_library_reopen_and_save_to_preserve_both_modes() {
    let dir = tempfile::tempdir().unwrap();
    let (_root, mut lib) = create_lib(&dir, "Src");
    let src_m = dir.path().join("m.png");
    let src_l = dir.path().join("l.png");
    write_png(&src_m, 40, 40, [10, 10, 200]);
    write_png(&src_l, 40, 40, [200, 200, 10]);

    let m_id = lib
        .import_file(
            &src_m,
            &ImportOptions {
                palette: false,
                ..Default::default()
            },
        )
        .unwrap();
    lib.set_storage_mode(StorageMode::Link).unwrap();
    let l_id = lib
        .import_file(
            &src_l,
            &ImportOptions {
                palette: false,
                ..Default::default()
            },
        )
        .unwrap();

    let moved = dir.path().join("Moved.library");
    lib.save_to(&moved).unwrap();
    let reopened = Library::open(&moved).unwrap();
    assert_eq!(reopened.storage_mode(), StorageMode::Link);
    assert_eq!(reopened.asset_status(&m_id), Some(AssetStatus::Managed));
    assert_eq!(reopened.asset_status(&l_id), Some(AssetStatus::Linked));
    assert_eq!(
        reopened.resolved_source_path(&l_id).unwrap(),
        fs::canonicalize(&src_l).unwrap()
    );
}

#[test]
fn test_04_link_import_zero_payload_copy_and_clean_eagle_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let (root, mut lib) = create_lib(&dir, "ZeroCopy");
    lib.set_storage_mode(StorageMode::Link).unwrap();

    // Large image (> 960px) generates thumbnail, yet still copies zero original payload.
    let src = dir.path().join("big.png");
    write_png(&src, 1200, 800, [50, 90, 140]);
    let src_hash = hash_file(&src);

    let id = lib
        .import_file(
            &src,
            &ImportOptions {
                palette: false,
                ..Default::default()
            },
        )
        .unwrap();
    let item = lib.item(&id).unwrap();
    assert_eq!(lib.asset_status(&id), Some(AssetStatus::Linked));
    assert!(
        !item.managed_storage_path(&root).exists(),
        "no payload copied into .info dir"
    );
    assert!(
        item.thumbnail_path(&root).is_some(),
        "cached thumbnail generated cleanly"
    );
    assert!(
        item.link_record_path(&root).is_file(),
        "falcon-link.json created"
    );

    // metadata.json must stay strictly Eagle-compatible (no Falcon link keys injected).
    let meta_raw = fs::read_to_string(item.info_dir(&root).join("metadata.json")).unwrap();
    assert!(
        !meta_raw.contains("sourcePath")
            && !meta_raw.contains("falconLink")
            && !meta_raw.contains("storageMode")
    );
    assert_eq!(hash_file(&src), src_hash);
}

#[test]
fn test_05_disallow_linking_files_inside_library_root_including_symlinks_and_traversals() {
    let dir = tempfile::tempdir().unwrap();
    let (root, mut lib) = create_lib(&dir, "Containment");
    lib.set_storage_mode(StorageMode::Link).unwrap();

    // 1. Direct file inside library/images
    let inside = root.join("images").join("inside.png");
    write_png(&inside, 32, 32, [255, 0, 0]);
    assert!(lib.import_file(&inside, &ImportOptions::default()).is_err());

    // 2. Traversal via `..` back into library
    let outside_dir = dir.path().join("outside");
    fs::create_dir_all(&outside_dir).unwrap();
    let traversal = outside_dir
        .join("..")
        .join("Containment.library")
        .join("images")
        .join("inside.png");
    assert!(
        lib.import_file(&traversal, &ImportOptions::default())
            .is_err()
    );

    // 3. Symlink outside library pointing inside library
    #[cfg(unix)]
    {
        let link_to_inside = outside_dir.join("alias.png");
        std::os::unix::fs::symlink(&inside, &link_to_inside).unwrap();
        assert!(
            lib.import_file(&link_to_inside, &ImportOptions::default())
                .is_err()
        );

        // 4. Symlink inside library pointing outside library traversed through library_root
        let real_ext = outside_dir.join("real.png");
        write_png(&real_ext, 32, 32, [0, 255, 0]);
        let sym_in_lib = root.join("link_out");
        std::os::unix::fs::symlink(&outside_dir, &sym_in_lib).unwrap();
        assert!(
            lib.import_file(sym_in_lib.join("real.png"), &ImportOptions::default())
                .is_err()
        );
    }

    assert!(lib.items().is_empty(), "zero silent fallback to Copy");
}

#[test]
fn test_06_link_import_errors_without_silent_fallback_to_copy() {
    let dir = tempfile::tempdir().unwrap();
    let (_root, mut lib) = create_lib(&dir, "NoFallback");
    lib.set_storage_mode(StorageMode::Link).unwrap();

    // Missing file
    assert!(
        lib.import_file(dir.path().join("missing.png"), &ImportOptions::default())
            .is_err()
    );
    // Directory instead of regular file
    let folder = dir.path().join("folder.png");
    fs::create_dir_all(&folder).unwrap();
    assert!(lib.import_file(&folder, &ImportOptions::default()).is_err());
    // Unrecognized extension
    let no_ext = dir.path().join("noext");
    fs::write(&no_ext, b"hello").unwrap();
    assert!(lib.import_file(&no_ext, &ImportOptions::default()).is_err());
    assert!(lib.items().is_empty());
}

#[test]
fn test_07_generated_assets_stay_managed_even_in_link_mode_library() {
    let dir = tempfile::tempdir().unwrap();
    let (root, mut lib) = create_lib(&dir, "Generated");
    lib.set_storage_mode(StorageMode::Link).unwrap();

    let generated = dir.path().join("pasted-temp.png");
    write_png(&generated, 64, 64, [90, 90, 90]);
    let opts = ImportOptions {
        palette: false,
        storage_mode: Some(StorageMode::Copy),
        ..Default::default()
    };
    let id = lib.import_file(&generated, &opts).unwrap();
    fs::remove_file(&generated).unwrap();

    assert_eq!(lib.asset_status(&id), Some(AssetStatus::Managed));
    assert!(lib.resolved_source_path(&id).unwrap().is_file());
    assert!(!lib.item(&id).unwrap().link_record_path(&root).exists());
}

#[test]
fn test_08_commit_rollback_leaves_no_orphan_sidecar_or_source_damage() {
    let dir = tempfile::tempdir().unwrap();
    let (root, mut lib) = create_lib(&dir, "Rollback");
    lib.set_storage_mode(StorageMode::Link).unwrap();

    let src = dir.path().join("temp_vanish.png");
    write_png(&src, 48, 48, [12, 34, 56]);
    let staged = import::stage_for(
        &src,
        &ImportOptions {
            palette: false,
            ..Default::default()
        },
        Some(&root),
        StorageMode::Link,
    )
    .unwrap();
    assert!(staged.is_linked());

    // Delete source between stage and commit so validation fails in commit_import.
    fs::remove_file(&src).unwrap();
    assert!(lib.commit_import(staged).is_err());

    assert!(lib.items().is_empty());
    assert!(lib.config().linked_ids.is_empty());
    let entries: Vec<_> = fs::read_dir(root.join("images")).unwrap().collect();
    assert!(entries.is_empty(), "rolled back .info directory completely");
}

#[test]
fn test_09_rename_and_undo_update_display_name_only_source_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let (root, mut lib) = create_lib(&dir, "RenameSafe");
    lib.set_storage_mode(StorageMode::Link).unwrap();

    let src = dir.path().join("MasterPhoto.png");
    write_png(&src, 1200, 900, [100, 150, 200]);
    let src_hash = hash_file(&src);

    let id = lib
        .import_file(
            &src,
            &ImportOptions {
                palette: false,
                ..Default::default()
            },
        )
        .unwrap();
    lib.begin_transaction("Rename");
    let mut item = lib.item(&id).unwrap().clone();
    item.name = "RenamedInFalcon".into();
    lib.update_item(item).unwrap();
    let tx = lib.end_transaction().unwrap();

    assert!(src.is_file(), "external filename untouched");
    assert_eq!(hash_file(&src), src_hash);
    assert_eq!(lib.item(&id).unwrap().name, "RenamedInFalcon");
    assert!(
        lib.item(&id)
            .unwrap()
            .thumbnail_path(&root)
            .unwrap()
            .ends_with("RenamedInFalcon_thumbnail.png")
    );

    // Undo rename must also keep external source file intact.
    lib.revert(tx).unwrap();
    assert!(src.is_file());
    assert_eq!(hash_file(&src), src_hash);
    assert_eq!(lib.item(&id).unwrap().name, "MasterPhoto");
    assert!(
        lib.item(&id)
            .unwrap()
            .thumbnail_path(&root)
            .unwrap()
            .ends_with("MasterPhoto_thumbnail.png")
    );
}

#[test]
fn test_10_delete_and_empty_trash_remove_library_entry_only_never_source() {
    let dir = tempfile::tempdir().unwrap();
    let (root, mut lib) = create_lib(&dir, "DeleteSafe");
    lib.set_storage_mode(StorageMode::Link).unwrap();

    let src1 = dir.path().join("keep1.png");
    let src2 = dir.path().join("keep2.png");
    write_png(&src1, 64, 64, [10, 200, 10]);
    write_png(&src2, 64, 64, [200, 10, 200]);
    let h1 = hash_file(&src1);
    let h2 = hash_file(&src2);

    let id1 = lib
        .import_file(
            &src1,
            &ImportOptions {
                palette: false,
                ..Default::default()
            },
        )
        .unwrap();
    let id2 = lib
        .import_file(
            &src2,
            &ImportOptions {
                palette: false,
                ..Default::default()
            },
        )
        .unwrap();

    lib.delete_permanently(std::slice::from_ref(&id1)).unwrap();
    assert!(lib.item(&id1).is_none());
    assert!(!lib.config().linked_ids.contains(&id1));
    assert!(!root.join("images").join(format!("{id1}.info")).exists());
    assert_eq!(
        hash_file(&src1),
        h1,
        "external source untouched after permanent delete"
    );

    lib.set_trashed(std::slice::from_ref(&id2), true).unwrap();
    assert_eq!(lib.empty_trash().unwrap(), 1);
    assert!(lib.item(&id2).is_none());
    assert!(!lib.config().linked_ids.contains(&id2));
    assert_eq!(
        hash_file(&src2),
        h2,
        "external source untouched after empty trash"
    );
}

#[test]
fn test_11_destructive_payload_mutations_refused_safely_on_linked_assets() {
    let dir = tempfile::tempdir().unwrap();
    let (root, mut lib) = create_lib(&dir, "MutationGuards");
    lib.set_storage_mode(StorageMode::Link).unwrap();

    let jpg = dir.path().join("photo.jpg");
    write_jpg(&jpg, 64, 64, [210, 40, 40]);
    let jpg_hash = hash_file(&jpg);
    let jpg_id = lib
        .import_file(
            &jpg,
            &ImportOptions {
                palette: false,
                ..Default::default()
            },
        )
        .unwrap();

    let webp_src = dir.path().join("pic.webp");
    DynamicImage::ImageRgb8(RgbImage::from_pixel(32, 32, Rgb([40, 210, 40])))
        .save_with_format(&webp_src, image::ImageFormat::WebP)
        .unwrap();
    let webp_hash = hash_file(&webp_src);
    let webp_id = lib
        .import_file(
            &webp_src,
            &ImportOptions {
                palette: false,
                ..Default::default()
            },
        )
        .unwrap();

    let txt_src = dir.path().join("note.txt");
    fs::write(&txt_src, "original note content").unwrap();
    let txt_hash = hash_file(&txt_src);
    let txt_id = lib
        .import_file(
            &txt_src,
            &ImportOptions {
                palette: false,
                ..Default::default()
            },
        )
        .unwrap();

    let replacement = dir.path().join("other.png");
    write_png(&replacement, 16, 16, [0, 0, 0]);

    assert!(matches!(
        lib.rotate_item(&jpg_id, 1),
        Err(Error::UnsupportedForLinkedAsset { .. })
    ));
    assert!(matches!(
        lib.flip_item(&jpg_id, Flip::Horizontal),
        Err(Error::UnsupportedForLinkedAsset { .. })
    ));
    assert!(matches!(
        lib.crop_item(
            &jpg_id,
            PixelRect {
                x: 0,
                y: 0,
                width: 16,
                height: 16
            }
        ),
        Err(Error::UnsupportedForLinkedAsset { .. })
    ));
    assert!(matches!(
        lib.set_extension(&jpg_id, "png"),
        Err(Error::UnsupportedForLinkedAsset { .. })
    ));
    assert!(matches!(
        lib.replace_original(&jpg_id, &replacement),
        Err(Error::UnsupportedForLinkedAsset { .. })
    ));
    assert!(matches!(
        lib.convert_webp(&webp_id, webp_convert::Format::Png, |_| Ok(())),
        Err(Error::UnsupportedForLinkedAsset { .. })
    ));
    assert!(matches!(
        lib.save_text(&txt_id, "modified"),
        Err(Error::UnsupportedForLinkedAsset { .. })
    ));

    // Non-destructive crop Save As produces a new Managed copy while leaving Linked source intact.
    let cropped_copy = lib
        .crop_item_as_new(
            &jpg_id,
            PixelRect {
                x: 0,
                y: 0,
                width: 32,
                height: 32,
            },
        )
        .unwrap();
    assert_eq!(lib.asset_status(&cropped_copy), Some(AssetStatus::Managed));
    assert!(
        lib.item(&cropped_copy)
            .unwrap()
            .managed_storage_path(&root)
            .is_file()
    );

    assert_eq!(hash_file(&jpg), jpg_hash);
    assert_eq!(hash_file(&webp_src), webp_hash);
    assert_eq!(hash_file(&txt_src), txt_hash);
}

#[test]
fn test_12_duplicate_linked_item_clones_sidecar_cleanly_without_copying_payload() {
    let dir = tempfile::tempdir().unwrap();
    let (root, mut lib) = create_lib(&dir, "DupLinked");
    lib.set_storage_mode(StorageMode::Link).unwrap();

    let src = dir.path().join("asset.png");
    write_png(&src, 64, 64, [15, 75, 195]);
    let src_hash = hash_file(&src);

    let id = lib
        .import_file(
            &src,
            &ImportOptions {
                palette: false,
                ..Default::default()
            },
        )
        .unwrap();
    let dup_id = lib.duplicate_item(&id).unwrap();
    assert_ne!(dup_id, id);
    assert_eq!(lib.asset_status(&dup_id), Some(AssetStatus::Linked));
    assert_eq!(lib.link_record(&dup_id).unwrap().unwrap().item_id, dup_id);
    assert!(
        !lib.item(&dup_id)
            .unwrap()
            .managed_storage_path(&root)
            .exists()
    );
    assert!(lib.config().linked_ids.contains(&dup_id));
    assert_eq!(hash_file(&src), src_hash);

    // InvalidReference duplicate must fail closed.
    fs::remove_file(lib.item(&dup_id).unwrap().link_record_path(&root)).unwrap();
    assert_eq!(
        lib.asset_status(&dup_id),
        Some(AssetStatus::InvalidReference)
    );
    assert!(lib.duplicate_item(&dup_id).is_err());
}

#[test]
fn test_13_fail_closed_status_transitions_linked_missing_offline_invalid_reference() {
    let dir = tempfile::tempdir().unwrap();
    let (root, mut lib) = create_lib(&dir, "States");
    lib.set_storage_mode(StorageMode::Link).unwrap();

    let src = dir.path().join("media.png");
    write_png(&src, 48, 48, [120, 60, 180]);
    let id = lib
        .import_file(
            &src,
            &ImportOptions {
                palette: false,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(lib.asset_status(&id), Some(AssetStatus::Linked));

    // Delete source -> Missing (never Managed)
    fs::remove_file(&src).unwrap();
    assert_eq!(lib.asset_status(&id), Some(AssetStatus::Missing));
    assert!(lib.resolved_source_path(&id).is_err());
    assert!(lib.last_known_source_path(&id).is_some());

    // Simulate unmounted external volume in sidecar -> Offline
    let sidecar = lib.item(&id).unwrap().link_record_path(&root);
    let mut rec: LinkRecord = serde_json::from_slice(&fs::read(&sidecar).unwrap()).unwrap();
    rec.volume.mount_point = Some(dir.path().join("UnmountedDrive"));
    rec.volume.volume_uuid = Some("00000000-0000-0000-0000-DEAD00000001".into());
    fs::write(&sidecar, serde_json::to_vec(&rec).unwrap()).unwrap();
    assert_eq!(lib.asset_status(&id), Some(AssetStatus::Offline));

    // Fail-closed InvalidReference cases:
    // (a) Future schema version
    let mut bad_ver = rec.clone();
    bad_ver.schema_version = 99;
    fs::write(&sidecar, serde_json::to_vec(&bad_ver).unwrap()).unwrap();
    assert_eq!(lib.asset_status(&id), Some(AssetStatus::InvalidReference));

    // (b) Mismatched item_id
    let mut bad_id = rec.clone();
    bad_id.item_id = "OTHER_ITEM".into();
    fs::write(&sidecar, serde_json::to_vec(&bad_id).unwrap()).unwrap();
    assert_eq!(lib.asset_status(&id), Some(AssetStatus::InvalidReference));

    // (c) Containment violation inside library_root
    let mut bad_path = rec.clone();
    bad_path.source_path = root.join("images").join("hack.png");
    fs::write(&sidecar, serde_json::to_vec(&bad_path).unwrap()).unwrap();
    assert_eq!(lib.asset_status(&id), Some(AssetStatus::InvalidReference));

    // (d) Corrupt JSON
    fs::write(&sidecar, b"{corrupt json").unwrap();
    assert_eq!(lib.asset_status(&id), Some(AssetStatus::InvalidReference));

    // (e) Deleted sidecar while tracked in falcon.json -> STILL InvalidReference (never silently Managed!)
    fs::remove_file(&sidecar).unwrap();
    assert_eq!(lib.asset_status(&id), Some(AssetStatus::InvalidReference));
}

#[test]
fn test_14_repair_never_treats_linked_offline_missing_or_invalid_as_broken() {
    let dir = tempfile::tempdir().unwrap();
    let (root, mut lib) = create_lib(&dir, "RepairSafe");
    lib.set_storage_mode(StorageMode::Link).unwrap();

    let s_ok = dir.path().join("ok.png");
    let s_miss = dir.path().join("miss.png");
    write_png(&s_ok, 32, 32, [10, 10, 10]);
    write_png(&s_miss, 32, 32, [20, 20, 20]);

    let id_ok = lib
        .import_file(
            &s_ok,
            &ImportOptions {
                palette: false,
                ..Default::default()
            },
        )
        .unwrap();
    let id_miss = lib
        .import_file(
            &s_miss,
            &ImportOptions {
                palette: false,
                ..Default::default()
            },
        )
        .unwrap();
    fs::remove_file(&s_miss).unwrap();

    let scan = repair::scan(&root, lib.items());
    assert!(
        scan.broken.is_empty(),
        "linked/missing referenced assets are never flagged broken: {scan:?}"
    );
    let cleared = lib.clear_invalid(&scan, |_| Ok(())).unwrap();
    assert_eq!(cleared.trashed, 0);
    assert!(!lib.item(&id_ok).unwrap().is_deleted);
    assert!(!lib.item(&id_miss).unwrap().is_deleted);
    assert!(
        lib.item(&id_miss)
            .unwrap()
            .link_record_path(&root)
            .is_file()
    );
}

#[test]
fn test_15_eaglepack_bundles_available_linked_as_managed_and_fails_atomically_when_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    let (_root, mut lib) = create_lib(&dir, "PackTest");
    lib.set_storage_mode(StorageMode::Link).unwrap();

    let ext_file = dir.path().join("bundle_me.png");
    write_png(&ext_file, 64, 64, [240, 140, 40]);
    let ext_hash = hash_file(&ext_file);
    let id = lib
        .import_file(
            &ext_file,
            &ImportOptions {
                palette: false,
                ..Default::default()
            },
        )
        .unwrap();

    // 1. Available Linked item -> .eaglepack carries Managed payload without falcon-link.json
    let out_pack = dir.path().join("clean.eaglepack");
    assert_eq!(
        pack::export_pack(
            &lib,
            PackSource::Items(std::slice::from_ref(&id)),
            &out_pack
        )
        .unwrap(),
        1
    );

    let archive = zip::ZipArchive::new(File::open(&out_pack).unwrap()).unwrap();
    let names: Vec<String> = (0..archive.len())
        .map(|i| archive.name_for_index(i).unwrap().unwrap().into_owned())
        .collect();
    assert!(
        !names.iter().any(|n| n.ends_with(LINK_SIDECAR)),
        "no falcon-link.json leaked into .eaglepack: {names:?}"
    );
    drop(archive);

    let mut manifest = String::new();
    zip::ZipArchive::new(File::open(&out_pack).unwrap())
        .unwrap()
        .by_name("pack.json")
        .unwrap()
        .read_to_string(&mut manifest)
        .unwrap();
    assert!(
        !manifest.contains(&ext_file.display().to_string()),
        "no external source path leaked in pack.json"
    );

    // Importing pack into another library produces a pure Managed item with matching payload hash.
    let (_r2, mut lib2) = create_lib(&dir, "Dest");
    let imported = pack::import_pack(&mut lib2, &out_pack, &PackImportOptions::default()).unwrap();
    assert_eq!(imported.len(), 1);
    assert_eq!(lib2.asset_status(&imported[0]), Some(AssetStatus::Managed));
    assert_eq!(
        hash_file(&lib2.resolved_source_path(&imported[0]).unwrap()),
        ext_hash
    );

    // 2. Missing Linked item -> pack fails explicitly BEFORE finalizing any .eaglepack output.
    fs::remove_file(&ext_file).unwrap();
    let failed_pack = dir.path().join("must_not_exist.eaglepack");
    assert!(
        pack::export_pack(
            &lib,
            PackSource::Items(std::slice::from_ref(&id)),
            &failed_pack
        )
        .is_err()
    );
    assert!(
        !failed_pack.exists(),
        "no partial .eaglepack left behind when asset is missing"
    );
}

#[test]
fn test_16_export_and_read_apis_use_external_source_and_never_mutate_source_hash() {
    let dir = tempfile::tempdir().unwrap();
    let (_root, mut lib) = create_lib(&dir, "ExportSafe");
    lib.set_storage_mode(StorageMode::Link).unwrap();

    let src = dir.path().join("master.png");
    write_png(&src, 64, 64, [33, 66, 99]);
    let src_hash = hash_file(&src);

    let id = lib
        .import_file(
            &src,
            &ImportOptions {
                palette: false,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        lib.resolved_source_path(&id).unwrap(),
        fs::canonicalize(&src).unwrap()
    );
    assert!(matches!(
        lib.managed_writable_path(&id, "test"),
        Err(Error::UnsupportedForLinkedAsset { .. })
    ));

    let export_dir = dir.path().join("Exported");
    let plan = export::plan_items(
        &lib,
        std::slice::from_ref(&id),
        &export_dir,
        Collision::KeepBoth,
    );
    assert_eq!(plan.steps[0].from, fs::canonicalize(&src).unwrap());
    let written =
        export::run(&plan, &std::sync::atomic::AtomicBool::new(false), |_, _| {}).unwrap();
    assert_eq!(written.len(), 1);
    assert_eq!(hash_file(&written[0]), src_hash);
    assert_eq!(
        export::paths_text(&lib, &[lib.item(&id).unwrap()]),
        fs::canonicalize(&src).unwrap().display().to_string()
    );
    assert!(export::data_url(&lib, lib.item(&id).unwrap()).is_some());
    assert!(export::thumbnail_image(&lib, lib.item(&id).unwrap()).is_some());

    // Duplicate candidates & exact duplicates check resolved paths cleanly.
    let cands = lib.duplicate_candidates(lib.item(&id).unwrap().size, "png");
    assert_eq!(cands.len(), 1);
    assert_eq!(cands[0].1, fs::canonicalize(&src).unwrap());

    // Source hash strictly preserved after all read/export/plan operations.
    assert_eq!(hash_file(&src), src_hash);
}
