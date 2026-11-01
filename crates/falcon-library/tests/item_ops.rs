//! Duplicate, Pin to Top, primary colour, Change Extension and Replace with
//! File on a temp library: what lands on disk.

use std::fs;
use std::io::Cursor;
use std::path::Path;

use falcon_library::import::ImportOptions;
use falcon_library::item_ops::{self, PINNED_KEY};
use falcon_library::undo::WriteMode;
use falcon_library::Library;
use image::{ImageFormat, Rgb, RgbImage};

fn write(w: u32, h: u32, path: &Path, fmt: ImageFormat) {
    let img = RgbImage::from_fn(w, h, |x, y| Rgb([(x * 7) as u8, (y * 5) as u8, 90]));
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), fmt).unwrap();
    fs::write(path, out).unwrap();
}

fn setup() -> (tempfile::TempDir, Library, String) {
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = Library::create(tmp.path().join("T.library")).unwrap();
    let folder = lib.create_folder(None, "Pins").unwrap();
    let src = tmp.path().join("pic.png");
    write(64, 48, &src, ImageFormat::Png);
    let opts = ImportOptions { folders: vec![folder], tags: vec!["red".into()], url: "https://example.com/a".into(), ..Default::default() };
    let id = lib.import_file(&src, &opts).unwrap();
    (tmp, lib, id)
}

fn metadata(lib: &Library, id: &str) -> String {
    let item = lib.item(id).unwrap();
    fs::read_to_string(item.info_dir(lib.root()).join("metadata.json")).unwrap()
}

fn files(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

#[test]
fn a_duplicate_is_the_same_folder_with_a_new_id() {
    let (_tmp, mut lib, id) = setup();
    let before = metadata(&lib, &id);
    let copy = lib.duplicate_item(&id).unwrap();
    assert_ne!(copy, id);
    // Byte for byte the original's metadata with only the id swapped.
    assert_eq!(metadata(&lib, &copy), before.replace(&id, &copy));
    let (a, b) = (lib.item(&id).unwrap().clone(), lib.item(&copy).unwrap().clone());
    assert_eq!(files(&a.info_dir(lib.root())), files(&b.info_dir(lib.root())));
    assert_eq!(fs::read(a.original_path(lib.root())).unwrap(), fs::read(b.original_path(lib.root())).unwrap());
    assert_eq!(metadata(&lib, &id), before, "the original is untouched");
    // Reopening finds both, and mtime.json counts them.
    let lib2 = Library::open(lib.root()).unwrap();
    assert_eq!(lib2.items().len(), 2);
    let mtime: serde_json::Value = serde_json::from_slice(&fs::read(lib.root().join("mtime.json")).unwrap()).unwrap();
    assert_eq!(mtime["all"], 2);
    assert!(mtime.get(&copy).is_some());
}

#[test]
fn duplicating_in_batched_mode_never_leaves_the_old_id_on_disk() {
    let (_tmp, mut lib, id) = setup();
    lib.set_write_mode(WriteMode::Batched).unwrap();
    let copy = lib.duplicate_item(&id).unwrap();
    let text = metadata(&lib, &copy);
    assert!(text.contains(&format!("\"id\":\"{copy}\"")) && !text.contains(&id));
    lib.flush().unwrap();
    assert_eq!(metadata(&lib, &copy), text);
}

#[test]
fn pins_are_stamped_per_folder_and_removed_with_the_last() {
    let (tmp, mut lib, id) = setup();
    let folder = lib.item(&id).unwrap().folders[0].clone();
    let other = lib.create_folder(None, "Other").unwrap();
    let src = tmp.path().join("second.png");
    write(10, 10, &src, ImageFormat::Png);
    let id2 = lib.import_file(&src, &ImportOptions { folders: vec![folder.clone()], ..Default::default() }).unwrap();
    let ids = vec![id.clone(), id2.clone()];

    assert_eq!(lib.set_pinned(&ids, &[folder.clone(), other.clone()], true, 1_000).unwrap(), 2);
    assert!(metadata(&lib, &id).ends_with(&format!(",\"{PINNED_KEY}\":{{\"{folder}\":1000}}}}")), "{}", metadata(&lib, &id));
    assert_eq!(item_ops::pinned_at(lib.item(&id2).unwrap(), &folder), Some(999.0));
    assert_eq!(item_ops::pinned_at(lib.item(&id).unwrap(), &other), None, "not pinned where it isn't");
    assert!(item_ops::all_pinned(ids.iter().map(|i| lib.item(i).unwrap()), &folder));

    assert_eq!(lib.set_pinned(&ids, std::slice::from_ref(&folder), false, 2_000).unwrap(), 2);
    assert!(!metadata(&lib, &id).contains(PINNED_KEY));
    assert_eq!(lib.set_pinned(&ids, &[folder], false, 3_000).unwrap(), 0, "nothing left to unpin");
}

#[test]
fn a_primary_colour_is_marked_then_set_false() {
    let (_tmp, mut lib, id) = setup();
    assert!(!lib.item(&id).unwrap().palettes.is_empty());
    lib.set_palette_marked(&id, 0, true).unwrap();
    let text = metadata(&lib, &id);
    assert!(text.contains(",\"marked\":true}"), "{text}");
    lib.set_palette_marked(&id, 0, false).unwrap();
    assert!(metadata(&lib, &id).contains(",\"marked\":false}"));
    assert!(lib.set_palette_marked(&id, 99, true).is_err());
}

#[test]
fn changing_the_extension_renames_only_the_original() {
    let (_tmp, mut lib, id) = setup();
    let item = lib.item(&id).unwrap().clone();
    let bytes = fs::read(item.original_path(lib.root())).unwrap();
    let thumbs_before = files(&item.info_dir(lib.root()));
    lib.set_extension(&id, "JPEG").unwrap();
    let after = lib.item(&id).unwrap().clone();
    assert_eq!(after.ext, "jpeg");
    assert_eq!(fs::read(after.original_path(lib.root())).unwrap(), bytes);
    assert!(!item.original_path(lib.root()).exists());
    let expect: Vec<String> = thumbs_before.iter().map(|f| f.replace("pic.png", "pic.jpeg")).collect();
    assert_eq!(files(&after.info_dir(lib.root())), expect);
    assert!(metadata(&lib, &id).contains("\"ext\":\"jpeg\""));
    assert!(lib.set_extension(&id, "a.b").is_err());
    assert_eq!(lib.item(&id).unwrap().ext, "jpeg");
}

#[test]
fn replacing_the_file_keeps_the_item_and_redraws_it() {
    let (tmp, mut lib, id) = setup();
    let before = lib.item(&id).unwrap().clone();
    let src = tmp.path().join("new.JPG");
    write(200, 100, &src, ImageFormat::Jpeg);
    lib.replace_original(&id, &src).unwrap();
    let after = lib.item(&id).unwrap().clone();
    assert_eq!(after.ext, "jpg");
    assert_eq!((after.width, after.height), (200, 100));
    assert_eq!(after.size, fs::metadata(&src).unwrap().len());
    assert_eq!(fs::read(after.original_path(lib.root())).unwrap(), fs::read(&src).unwrap());
    assert_eq!((&after.name, &after.tags, &after.folders, &after.url), (&before.name, &before.tags, &before.folders, &before.url));
    assert_eq!(after.modification_time, before.modification_time);
    // Only the new original and the item's own files remain: no backup or
    // staging file, and the old original is gone.
    let left = files(&after.info_dir(lib.root()));
    assert!(left.iter().all(|f| !f.ends_with(".bk") && !f.contains("falcon-replace")), "{left:?}");
    assert!(!before.original_path(lib.root()).exists());
    assert!(left.contains(&"pic.jpg".to_string()));
    // The same extension replaces in place.
    let src2 = tmp.path().join("again.jpg");
    write(30, 20, &src2, ImageFormat::Jpeg);
    lib.replace_original(&id, &src2).unwrap();
    assert_eq!(lib.item(&id).unwrap().width, 30);
    assert!(lib.replace_original(&id, &tmp.path().join("missing.png")).is_err());
    assert_eq!(lib.item(&id).unwrap().width, 30, "a failed replace changes nothing");
}
