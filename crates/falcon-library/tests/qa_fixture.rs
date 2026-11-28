//! Builds the synthetic libraries and source folders used for hands-on QA of
//! the app. It never touches a real library: everything goes under the folder
//! named by `FALCON_QA_DIR`, which must be an empty or new directory.
//!
//! ```text
//! FALCON_QA_DIR=/path/to/sandbox cargo test -p falcon-library --test qa_fixture -- --ignored --nocapture
//! ```
//!
//! Layout:
//! - `libraries/QA-Copy.library`: managed items in nested folders, tags, a few
//!   formats the grid hides as unsupported, large and extreme-aspect images
//! - `libraries/QA-Link.library`: referenced items, one whose source was
//!   deleted (missing media) and one renamed into `sources/relink-new`, plus an
//!   automatic watched folder over `sources/watched` that is not synced yet
//! - `sources/`: the files those libraries refer to
//! - `autoimport/QA-Copy/`: the folder Auto-Import watches for `QA-Copy`

use std::fs;
use std::path::{Path, PathBuf};

use falcon_library::import::ImportOptions;
use falcon_library::{Library, StorageMode, SyncMode, SyncStatus, WatchedFolder};

fn gradient(path: &Path, w: u32, h: u32, seed: u32) {
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        let r = ((x * 255) / w.max(1)) as u8;
        let g = ((y * 255) / h.max(1)) as u8;
        let b = ((seed * 37) % 256) as u8;
        image::Rgb([r, g, b])
    });
    img.save(path).unwrap();
}

fn opts(folder: Option<&str>, mode: StorageMode) -> ImportOptions {
    ImportOptions { folders: folder.map(|f| vec![f.to_string()]).unwrap_or_default(), storage_mode: Some(mode), palette: false, ..Default::default() }
}

#[test]
#[ignore = "writes a QA sandbox; run by hand with FALCON_QA_DIR"]
fn build_qa_sandbox() {
    let Some(dir) = std::env::var_os("FALCON_QA_DIR").map(PathBuf::from) else {
        panic!("set FALCON_QA_DIR to a new directory")
    };
    assert!(!dir.join("libraries").exists(), "{} already holds a sandbox", dir.display());
    let sources = dir.join("sources");
    for sub in ["linked", "watched/2026/Q4", "relink-new", "copy-src"] {
        fs::create_dir_all(sources.join(sub)).unwrap();
    }
    fs::create_dir_all(dir.join("libraries")).unwrap();
    fs::create_dir_all(dir.join("autoimport").join("QA-Copy")).unwrap();

    // ---- managed library ------------------------------------------------
    let mut copy = Library::create(dir.join("libraries/QA-Copy.library")).unwrap();
    let projects = copy.create_folder(None, "Projects").unwrap();
    let y2026 = copy.create_folder(Some(&projects), "2026").unwrap();
    let q4 = copy.create_folder(Some(&y2026), "Q4").unwrap();
    let textures = copy.create_folder(None, "Textures").unwrap();
    let mut ids = Vec::new();
    for i in 0..40u32 {
        let (w, h) = match i % 5 {
            0 => (640, 480),
            1 => (480, 640),
            2 => (1280, 720),
            3 => (4096, 3072), // high-resolution preview checks
            _ => (3000, 200),  // extreme aspect ratio
        };
        let file = sources.join("copy-src").join(format!("shot-{i:02}.png"));
        gradient(&file, w, h, i);
        let folder = match i % 4 {
            0 => Some(q4.as_str()),
            1 => Some(y2026.as_str()),
            2 => Some(textures.as_str()),
            _ => None,
        };
        ids.push(copy.import_file(&file, &opts(folder, StorageMode::Copy)).unwrap());
    }
    copy.add_tags_to_items(&ids[..10], &["alpha".into(), "hero".into()]).unwrap();
    copy.add_tags_to_items(&ids[10..20], &["beta".into()]).unwrap();
    // Formats the grid hides under "Hide Unsupported Files".
    for (i, ext) in ["xyz", "qaq", "zzz"].into_iter().enumerate() {
        let f = sources.join("copy-src").join(format!("mystery-{i}.{ext}"));
        fs::write(&f, vec![7u8; 2048 + i]).unwrap();
        copy.import_file(&f, &opts(None, StorageMode::Copy)).unwrap();
    }
    let note = sources.join("copy-src").join("notes.txt");
    fs::write(&note, "Falcon QA notes\nsecond line\n").unwrap();
    copy.import_file(&note, &opts(None, StorageMode::Copy)).unwrap();

    // ---- referenced library -----------------------------------------------
    let mut link = Library::create(dir.join("libraries/QA-Link.library")).unwrap();
    link.set_storage_mode(StorageMode::Link).unwrap();
    let linked_folder = link.create_folder(None, "Linked").unwrap();
    let mut linked_ids = Vec::new();
    for i in 0..20u32 {
        let file = sources.join("linked").join(format!("ref-{i:02}.png"));
        gradient(&file, 800 + i * 10, 600, i + 100);
        linked_ids.push(link.import_file(&file, &opts(Some(&linked_folder), StorageMode::Link)).unwrap());
    }
    // Missing media: the source is deleted afterwards.
    let gone = sources.join("linked").join("ref-00.png");
    fs::remove_file(&gone).unwrap();
    // Relink candidate: renamed into another folder with the same bytes.
    let moved_from = sources.join("linked").join("ref-01.png");
    fs::rename(&moved_from, sources.join("relink-new").join("ref-01.png")).unwrap();

    // Automatic watched folder with files not synced yet.
    let watched_folder = link.create_folder(None, "Watched").unwrap();
    for i in 0..30u32 {
        let sub = if i % 3 == 0 { "watched/2026/Q4" } else { "watched" };
        gradient(&sources.join(sub).join(format!("w-{i:02}.png")), 700, 500 + i, i + 200);
    }
    link.upsert_watched_folder(WatchedFolder {
        id: "qa-watched".into(),
        source_path: sources.join("watched"),
        folder_id: watched_folder,
        storage_mode: StorageMode::Link,
        sync_mode: SyncMode::Auto,
        recursive: true,
        last_synced: None,
        status: SyncStatus::Idle,
        last_error: None,
    })
    .unwrap();

    println!("QA sandbox ready under {}", dir.display());
}
