//! What only Windows can get wrong: libraries from a Mac, library metadata
//! staying free of Windows paths, and paths and files the system treats
//! differently from a Mac or Linux.
#![cfg(windows)]

use std::fs;
use std::path::{Path, PathBuf};

use falcon_library::Library;
use falcon_library::import::ImportOptions;
use image::{ImageFormat, Rgb, RgbImage};

fn make_png(path: &Path) {
    RgbImage::from_fn(8, 8, |x, y| Rgb([x as u8 * 30, y as u8 * 30, 90])).save_with_format(path, ImageFormat::Png).unwrap();
}

/// Large enough to be given a thumbnail.
fn make_large_png(path: &Path) {
    RgbImage::from_fn(2000, 1000, |x, y| Rgb([x as u8, y as u8, 90])).save_with_format(path, ImageFormat::Png).unwrap();
}

fn opts() -> ImportOptions {
    ImportOptions { palette: false, ..Default::default() }
}

/// Every JSON file in a library.
fn json_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            json_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "json") {
            out.push(path);
        }
    }
}

/// A library as a Mac leaves it, with item names a Mac allows and Windows
/// doesn't, and the files named the way copying them to Windows names them.
fn mac_library(root: &Path) -> Vec<(&'static str, &'static str, &'static str)> {
    let items = vec![
        ("M3KVWXDQ2B7CA", "a:b", "a_b.png"),
        ("M3KVWXDQ2B7CB", "what?", "what_.png"),
        ("M3KVWXDQ2B7CC", "CON", "CON_.png"),
        ("M3KVWXDQ2B7CD", r"back\slash", "back_slash.png"),
        ("M3KVWXDQ2B7CE", "plain", "plain.png"),
    ];
    drop(Library::create(root).unwrap());
    fs::write(
        root.join("metadata.json"),
        r#"{"folders":[{"id":"F1","name":"Mac/Folder","description":"","children":[],"modificationTime":1,"tags":[],"password":"","passwordTips":""}],"smartFolders":[],"quickAccess":[],"tagsGroups":[],"modificationTime":5,"applicationVersion":"4.0.0"}"#,
    )
    .unwrap();
    for (id, name, file) in &items {
        let dir = root.join("images").join(format!("{id}.info"));
        fs::create_dir_all(&dir).unwrap();
        make_png(&dir.join(file));
        let meta = serde_json::json!({
            "id": id, "name": name, "size": fs::metadata(dir.join(file)).unwrap().len(), "btime": 1, "mtime": 1, "ext": "png",
            "tags": ["t"], "folders": ["F1"], "isDeleted": false, "url": "https://a.b/c", "annotation": "note",
            "modificationTime": 1, "height": 8, "width": 8, "lastModified": 1, "palettes": [],
        });
        fs::write(dir.join("metadata.json"), serde_json::to_vec(&meta).unwrap()).unwrap();
    }
    items
}

#[test]
fn a_mac_library_opens_and_keeps_its_names_byte_for_byte() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("Mac.library");
    let items = mac_library(&root);
    let lib = Library::open(&root).unwrap();
    assert_eq!(lib.items().len(), items.len());
    for (id, name, file) in &items {
        let item = lib.item(id).unwrap();
        assert_eq!(item.name, *name, "the name in the metadata is untouched");
        let info = item.info_dir(lib.root());
        assert_eq!(item.original_path(lib.root()), info.join(file));
        assert!(item.original_path(lib.root()).is_file(), "{file}");
        assert!(item.thumbnail_path(lib.root()).is_none());
    }
    // Written back, every item and the library are what they were.
    let out = tmp.path().join("Out.library");
    lib.save_to(&out).unwrap();
    for (id, _, file) in &items {
        let rel = format!("images/{id}.info/metadata.json");
        assert_eq!(fs::read_to_string(root.join(&rel)).unwrap(), fs::read_to_string(out.join(&rel)).unwrap(), "{rel}");
        assert!(out.join(format!("images/{id}.info/{file}")).is_file());
    }
    let (before, after): (serde_json::Value, serde_json::Value) =
        (serde_json::from_slice(&fs::read(root.join("metadata.json")).unwrap()).unwrap(), serde_json::from_slice(&fs::read(out.join("metadata.json")).unwrap()).unwrap());
    assert_eq!(before["folders"], after["folders"]);
    assert_eq!(Library::open(&out).unwrap().items(), lib.items());
}

#[test]
fn saving_an_edit_to_a_mac_name_does_not_write_outside_its_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("Mac.library");
    mac_library(&root);
    let mut lib = Library::open(&root).unwrap();
    // Redrawing writes the thumbnail, the file a stream would have caught.
    make_large_png(&lib.item("M3KVWXDQ2B7CA").unwrap().original_path(lib.root()));
    lib.refresh_thumbnail("M3KVWXDQ2B7CA").unwrap();
    let item = lib.item("M3KVWXDQ2B7CA").unwrap();
    assert_eq!(item.name, "a:b");
    assert_eq!(item.thumbnail_path(lib.root()), Some(item.info_dir(lib.root()).join("a_b_thumbnail.png")));
    let names: Vec<_> = fs::read_dir(item.info_dir(lib.root())).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    assert!(!names.iter().any(|n| n == "a"), "a stream of a file called a: {names:?}");
    // Renaming from it moves the real file.
    let mut renamed = item.clone();
    renamed.name = "Renamed: Now?".into();
    lib.update_item(renamed).unwrap();
    let item = lib.item("M3KVWXDQ2B7CA").unwrap();
    assert_eq!(item.name, "Renamed Now");
    assert!(item.original_path(lib.root()).is_file());
    assert!(item.thumbnail_path(lib.root()).is_some());
}

#[test]
fn nothing_written_to_a_library_holds_a_windows_path() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("New.library");
    let mut lib = Library::create(&root).unwrap();
    let src = tmp.path().join("Some Folder").join("sub");
    fs::create_dir_all(&src).unwrap();
    make_png(&src.join("a picture.png"));
    let id = lib.import_file(src.join("a picture.png"), &opts()).unwrap();
    let copy = lib.item(&id).cloned().unwrap();
    lib.update_item(copy).unwrap();
    lib.create_folder(None, "A Folder").unwrap();
    lib.save_metadata().unwrap();
    let mut files = Vec::new();
    json_files(&root, &mut files);
    assert!(files.len() >= 2, "{files:?}");
    let profile = std::env::var("USERPROFILE").unwrap();
    for f in files {
        let text = fs::read_to_string(&f).unwrap();
        assert!(!text.contains('\\'), "{f:?}: {text}");
        assert!(!text.contains(":/"), "{f:?}: {text}");
        assert!(!text.contains(&profile), "{f:?}: {text}");
        assert!(!text.contains('\r'), "{f:?}: {text}");
    }
}

#[test]
fn a_device_name_imports_under_a_name_windows_can_keep() {
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = Library::create(tmp.path().join("New.library")).unwrap();
    let src = tmp.path().join("picture.png");
    make_png(&src);
    let id = lib.import_file(&src, &ImportOptions { name: Some("NUL".into()), ..opts() }).unwrap();
    let item = lib.item(&id).unwrap();
    assert_eq!(item.name, "NUL_");
    assert!(item.original_path(lib.root()).is_file());
}

#[test]
fn a_library_past_max_path_works() {
    let tmp = tempfile::tempdir().unwrap();
    let mut deep = tmp.path().to_path_buf();
    for n in 0..8 {
        deep.push(format!("{n}-a-rather-long-folder-name-to-go-past-max-path"));
    }
    fs::create_dir_all(&deep).unwrap();
    let root = deep.join("Deep.library");
    assert!(root.as_os_str().len() > 260);
    let mut lib = Library::create(&root).unwrap();
    let src = tmp.path().join("picture.png");
    make_large_png(&src);
    let id = lib.import_file(&src, &opts()).unwrap();
    let item = lib.item(&id).cloned().unwrap();
    assert!(item.original_path(lib.root()).is_file());
    assert!(item.thumbnail_path(lib.root()).is_some());
    drop(lib);
    assert_eq!(Library::open(&root).unwrap().items().len(), 1);
}

/// Files replaced whole while another thread of the app reads them.
#[test]
fn replacing_a_file_works_while_it_is_being_read() {
    use std::io::Write;
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target.bin");
    fs::write(&target, "old").unwrap();
    let reader = fs::File::open(&target).unwrap();

    let staged = tmp.path().join("staged.bin");
    fs::write(&staged, "renamed").unwrap();
    fs::rename(&staged, &target).unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(), "renamed");

    let mut temp = tempfile::NamedTempFile::new_in(tmp.path()).unwrap();
    temp.write_all(b"persisted").unwrap();
    temp.persist(&target).map_err(|e| e.error).unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(), "persisted");
    drop(reader);
}

/// The file the app is showing or backing up may be open while it is saved.
#[test]
fn saving_works_while_the_files_are_being_read() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("New.library");
    let mut lib = Library::create(&root).unwrap();
    let src = tmp.path().join("picture.txt");
    fs::write(&src, "first").unwrap();
    let id = lib.import_file(&src, &opts()).unwrap();
    let raw = lib.item(&id).unwrap().original_path(lib.root());
    let _open = [fs::File::open(root.join("metadata.json")).unwrap(), fs::File::open(&raw).unwrap()];
    lib.save_metadata().unwrap();
    lib.save_text(&id, "second").unwrap();
    assert_eq!(fs::read_to_string(&raw).unwrap(), "second");
    falcon_library::backup::backup_library(&root).unwrap();
}
