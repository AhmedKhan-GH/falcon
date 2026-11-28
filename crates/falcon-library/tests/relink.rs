//! Comprehensive tests for the Missing Media & Relink engine.

use std::fs;
use std::path::PathBuf;

use falcon_library::Library;
use falcon_library::import::ImportOptions;
use falcon_library::linked::AssetStatus;
use falcon_library::relink::MatchConfidence;
use falcon_library::storage_mode::StorageMode;
use tempfile::TempDir;

struct TestContext {
    _tmp: TempDir,
    root: PathBuf,
    lib_path: PathBuf,
    src_dir: PathBuf,
}

impl TestContext {
    fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        let lib_path = root.join("TestRelink.library");
        let src_dir = root.join("external_sources");
        fs::create_dir_all(&src_dir).unwrap();
        Library::create(&lib_path).unwrap();
        Self {
            _tmp: tmp,
            root,
            lib_path,
            src_dir,
        }
    }

    fn open_lib(&self) -> Library {
        Library::open(&self.lib_path).unwrap()
    }
}

#[test]
fn test_relink_single_moved_file() {
    let ctx = TestContext::new();
    let file_a = ctx.src_dir.join("photo.jpg");
    fs::write(&file_a, b"PHOTO-CONTENT-ABC").unwrap();

    let mut lib = ctx.open_lib();
    lib.set_storage_mode(StorageMode::Link).unwrap();
    let opts = ImportOptions::default();
    let item_id = lib.import_file(&file_a, &opts).unwrap();
    assert_eq!(lib.asset_status(&item_id), Some(AssetStatus::Linked));

    // Move file externally
    let moved_dir = ctx.root.join("new_location");
    fs::create_dir_all(&moved_dir).unwrap();
    let moved_file = moved_dir.join("photo.jpg");
    fs::rename(&file_a, &moved_file).unwrap();

    assert_eq!(lib.asset_status(&item_id), Some(AssetStatus::Missing));

    // Relink single file
    let rec = lib.relink_item_to_file(&item_id, &moved_file).unwrap();
    assert_eq!(rec.source_path, moved_file.canonicalize().unwrap());
    assert_eq!(lib.asset_status(&item_id), Some(AssetStatus::Linked));

    // ID, tags, ratings preserved
    let it = lib.item(&item_id).unwrap();
    assert_eq!(it.id, item_id);
    assert_eq!(it.name, "photo");
}

#[test]
fn test_relink_renamed_file_with_verified_content() {
    let ctx = TestContext::new();
    let orig = ctx.src_dir.join("sunset.png");
    fs::write(&orig, b"SUNSET-BYTES-999").unwrap();

    let mut lib = ctx.open_lib();
    lib.set_storage_mode(StorageMode::Link).unwrap();
    let opts = ImportOptions::default();
    let item_id = lib.import_file(&orig, &opts).unwrap();

    // Rename file
    let renamed = ctx.src_dir.join("sunset_final_v2.png");
    fs::rename(&orig, &renamed).unwrap();
    assert_eq!(lib.asset_status(&item_id), Some(AssetStatus::Missing));

    // Relink
    lib.relink_item_to_file(&item_id, &renamed).unwrap();
    assert_eq!(lib.asset_status(&item_id), Some(AssetStatus::Linked));
    assert_eq!(lib.resolved_source_path(&item_id).unwrap(), renamed.canonicalize().unwrap());
}

#[test]
fn test_reject_incompatible_format_and_containment() {
    let ctx = TestContext::new();
    let orig = ctx.src_dir.join("doc.pdf");
    fs::write(&orig, b"%PDF-1.4").unwrap();

    let mut lib = ctx.open_lib();
    lib.set_storage_mode(StorageMode::Link).unwrap();
    let opts = ImportOptions::default();
    let item_id = lib.import_file(&orig, &opts).unwrap();

    // 1. Incompatible format (e.g. .mp4 for .pdf without override)
    let bad_ext = ctx.src_dir.join("video.mp4");
    fs::write(&bad_ext, b"MP4-DATA").unwrap();
    assert!(lib.relink_item_to_file(&item_id, &bad_ext).is_err());

    // 2. Reject target inside library
    let inside_lib = ctx.lib_path.join("images").join("sneaky.pdf");
    fs::write(&inside_lib, b"%PDF-INSIDE").unwrap();
    assert!(lib.relink_item_to_file(&item_id, &inside_lib).is_err());
}

#[test]
fn test_relink_folder_batch_discovery() {
    let ctx = TestContext::new();
    let folder_a = ctx.src_dir.join("ProjectA");
    fs::create_dir_all(&folder_a).unwrap();
    let f1 = folder_a.join("hero.jpg");
    let f2 = folder_a.join("icon.svg");
    fs::write(&f1, b"HERO-IMAGE-DATA").unwrap();
    fs::write(&f2, b"<svg>ICON</svg>").unwrap();

    let mut lib = ctx.open_lib();
    lib.set_storage_mode(StorageMode::Link).unwrap();
    let opts = ImportOptions::default();
    let id1 = lib.import_file(&f1, &opts).unwrap();
    let id2 = lib.import_file(&f2, &opts).unwrap();

    // Relocate ProjectA to another volume/dir
    let relocated = ctx.root.join("RelocatedProjects").join("ProjectA");
    fs::create_dir_all(relocated.parent().unwrap()).unwrap();
    fs::rename(&folder_a, &relocated).unwrap();

    assert_eq!(lib.asset_status(&id1), Some(AssetStatus::Missing));
    assert_eq!(lib.asset_status(&id2), Some(AssetStatus::Missing));

    // Preview relink in relocated folder
    let preview = lib.preview_relink_folder(lib.items(), &relocated);
    assert_eq!(preview.matched.len(), 2);
    assert_eq!(preview.ambiguous.len(), 0);
    assert_eq!(preview.unmatched.len(), 0);

    // Apply batch
    let pairs: Vec<_> = preview
        .matched
        .into_iter()
        .map(|s| (s.item_id, s.candidate.unwrap().candidate_path))
        .collect();
    let committed = lib.apply_relink_batch(&pairs).unwrap();
    assert_eq!(committed.len(), 2);

    assert_eq!(lib.asset_status(&id1), Some(AssetStatus::Linked));
    assert_eq!(lib.asset_status(&id2), Some(AssetStatus::Linked));
}

#[test]
fn test_relink_batch_rollback_on_failure() {
    let ctx = TestContext::new();
    let f1 = ctx.src_dir.join("file1.jpg");
    let f2 = ctx.src_dir.join("file2.jpg");
    fs::write(&f1, b"DATA-1").unwrap();
    fs::write(&f2, b"DATA-2").unwrap();

    let mut lib = ctx.open_lib();
    lib.set_storage_mode(StorageMode::Link).unwrap();
    let opts = ImportOptions::default();
    let id1 = lib.import_file(&f1, &opts).unwrap();
    let id2 = lib.import_file(&f2, &opts).unwrap();

    let rec1_orig = lib.link_record(&id1).unwrap().unwrap();
    let rec2_orig = lib.link_record(&id2).unwrap().unwrap();

    // Valid target for 1, invalid target for 2 (inside library)
    let good_target = ctx.root.join("good.jpg");
    fs::write(&good_target, b"GOOD-DATA").unwrap();
    let bad_target = ctx.lib_path.join("bad.jpg");
    fs::write(&bad_target, b"BAD-DATA").unwrap();

    let pairs = vec![
        (id1.clone(), good_target),
        (id2.clone(), bad_target),
    ];

    assert!(lib.apply_relink_batch(&pairs).is_err());

    // Both link records must be intact at their original paths
    assert_eq!(lib.link_record(&id1).unwrap().unwrap().source_path, rec1_orig.source_path);
    assert_eq!(lib.link_record(&id2).unwrap().unwrap().source_path, rec2_orig.source_path);
}

#[test]
fn test_ambiguous_matches_require_confirmation() {
    let ctx = TestContext::new();
    let f = ctx.src_dir.join("banner.jpg");
    fs::write(&f, b"ORIGINAL-BANNER-12345").unwrap();

    let mut lib = ctx.open_lib();
    lib.set_storage_mode(StorageMode::Link).unwrap();
    let opts = ImportOptions::default();
    let id = lib.import_file(&f, &opts).unwrap();

    fs::remove_file(&f).unwrap();
    assert_eq!(lib.asset_status(&id), Some(AssetStatus::Missing));

    // Folder containing a banner.jpg with DIFFERENT content/size
    let diff_dir = ctx.root.join("other_folder");
    fs::create_dir_all(&diff_dir).unwrap();
    fs::write(diff_dir.join("banner.jpg"), b"DIFFERENT-CONTENT-LONGER-DATA-999").unwrap();

    let preview = lib.preview_relink_folder(lib.items(), &diff_dir);
    assert_eq!(preview.matched.len(), 0);
    assert_eq!(preview.ambiguous.len(), 1);
    assert_eq!(preview.ambiguous[0].candidate.as_ref().unwrap().confidence, MatchConfidence::Ambiguous);
}
