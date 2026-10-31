//! Folder, tag-group, saved-filter, smart-folder and comment edits, checked
//! against what lands on disk.

use std::fs;
use std::path::Path;

use falcon_library::Library;
use falcon_library::model::{Folder, FolderOrder};
use falcon_library::search::OrderBy;

fn new_library(dir: &Path) -> Library {
    Library::create(dir.join("Test.library")).unwrap()
}

fn metadata(lib: &Library) -> String {
    fs::read_to_string(lib.root().join("metadata.json")).unwrap()
}

fn ids(folders: &[Folder]) -> Vec<&str> {
    folders.iter().map(|f| f.id.as_str()).collect()
}

#[test]
fn folder_order_icon_and_cover_are_written_in_eagle_order() {
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = new_library(tmp.path());
    let a = lib.create_folder(None, "A").unwrap();
    let stamp = lib.metadata.folders[0].modification_time;

    lib.set_folder_order(&a, Some(OrderBy::Name.into()), false).unwrap();
    lib.set_folder_icon(&a, Some("palette"), Some("blue")).unwrap();
    lib.set_folder_cover(&a, Some("ITEMID0000001")).unwrap();
    let text = metadata(&lib);
    assert!(
        text.contains(r#""tags":[],"icon":"palette","iconColor":"blue","password":"","passwordTips":"","coverId":"ITEMID0000001","orderBy":"NAME","sortIncrease":false}"#),
        "{text}"
    );
    let reopened = Library::open(lib.root()).unwrap();
    let f = &reopened.metadata.folders[0];
    assert_eq!(f.order(), Some(FolderOrder::By(OrderBy::Name)));
    assert_eq!(f.modification_time, stamp, "folder edits leave modificationTime alone");

    // A subfolder inherits the icon and colour.
    let b = lib.create_folder(Some(&a), "B").unwrap();
    let child = &lib.metadata.folders[0].children[0];
    assert_eq!((child.id.as_str(), child.icon.as_deref(), child.icon_color.as_deref()), (b.as_str(), Some("palette"), Some("blue")));

    lib.set_folder_order(&a, None, true).unwrap();
    lib.set_folder_icon(&a, None, None).unwrap();
    lib.set_folder_cover(&a, None).unwrap();
    assert!(!metadata(&lib).contains("orderBy"));
    assert!(lib.set_folder_icon(&a, None, Some("teal")).is_err(), "colour outside the palette");
}

#[test]
fn reorder_places_folders_among_siblings() {
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = new_library(tmp.path());
    let a = lib.create_folder(None, "A").unwrap();
    let b = lib.create_folder(None, "B").unwrap();
    let c = lib.create_folder(None, "C").unwrap();

    lib.reorder_folder(&c, None, 0).unwrap();
    assert_eq!(ids(&lib.metadata.folders), [&c, &a, &b]);
    lib.reorder_folder(&c, None, 99).unwrap();
    assert_eq!(ids(&lib.metadata.folders), [&a, &b, &c]);

    lib.reorder_folder(&b, Some(&a), 0).unwrap();
    lib.reorder_folder(&c, Some(&a), 0).unwrap();
    assert_eq!(ids(&lib.metadata.folders), [&a]);
    assert_eq!(ids(&lib.metadata.folders[0].children), [&c, &b]);

    assert!(lib.reorder_folder(&a, Some(&b), 0).is_err(), "into a descendant");
    assert!(lib.reorder_folder(&a, Some(&a), 0).is_err(), "into itself");
    assert!(lib.reorder_folder(&b, Some("NOPE"), 0).is_err(), "unknown parent");
    assert_eq!(ids(&lib.metadata.folders[0].children), [&c, &b], "failed moves change nothing");

    // The old append behaviour stays.
    lib.move_folder(&c, None).unwrap();
    assert_eq!(ids(&Library::open(lib.root()).unwrap().metadata.folders), [&a, &c]);
}

#[test]
fn rename_keeps_pinyin_in_step() {
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = new_library(tmp.path());
    let a = lib.create_folder(None, "A").unwrap();
    lib.rename_folder(&a, "Plain").unwrap();
    assert_eq!(lib.metadata.folders[0].pinyin, None, "not added when absent");
    lib.metadata.folders[0].pinyin = Some("Plain".into());
    lib.rename_folder(&a, "Café").unwrap();
    assert_eq!(lib.metadata.folders[0].pinyin.as_deref(), Some("Café"));
    lib.rename_folder(&a, "设计").unwrap();
    assert_eq!(lib.metadata.folders[0].pinyin, None, "left for Eagle to rebuild");
}
