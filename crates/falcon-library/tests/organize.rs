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
fn folder_passwords() {
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = new_library(tmp.path());
    let a = lib.create_folder(None, "A").unwrap();
    assert!(!lib.metadata.folders[0].is_locked());
    assert!(!lib.verify_folder_password(&a, ""));

    lib.set_folder_password(&a, Some(("123", "the usual"))).unwrap();
    assert!(metadata(&lib).contains(r#""password":"MTIz","passwordTips":"the usual""#));
    let reopened = Library::open(lib.root()).unwrap();
    assert!(reopened.metadata.folders[0].is_locked());
    assert!(reopened.verify_folder_password(&a, "123"));
    assert!(!reopened.verify_folder_password(&a, "124"));

    assert!(lib.set_folder_password(&a, Some(("密码", ""))).is_err());
    assert!(lib.set_folder_password(&a, Some(("", ""))).is_err());
    assert!(lib.verify_folder_password(&a, "123"), "refused changes keep the old password");

    lib.set_folder_password(&a, None).unwrap();
    assert!(metadata(&lib).contains(r#""password":"","passwordTips":"""#));
    assert!(!lib.verify_folder_password(&a, "123"));
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn tag_groups() {
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = new_library(tmp.path());
    let mood = lib.create_tag_group("Mood", &strings(&["calm", "loud", "calm"]), Some("aqua")).unwrap();
    let style = lib.create_tag_group("Style", &strings(&["flat"]), None).unwrap();
    assert_eq!(mood.len(), 13);
    let text = metadata(&lib);
    assert!(text.contains(&format!(r#""tagsGroups":[{{"id":"{mood}","name":"Mood","tags":["calm","loud"],"color":"aqua"}},{{"id":"{style}","name":"Style","tags":["flat"]}}]"#)), "{text}");

    // Adding moves a tag out of its other groups.
    lib.add_tags_to_group(&style, &strings(&["loud\n", "flat"])).unwrap();
    assert_eq!(lib.metadata.tags_groups[0].tags, ["calm"]);
    assert_eq!(lib.metadata.tags_groups[1].tags, ["flat", "loud"]);
    lib.remove_tags_from_group(&style, &strings(&["flat"])).unwrap();
    lib.set_tag_group_tags(&mood, &strings(&["b", "a", "b"])).unwrap();
    lib.rename_tag_group(&mood, "Feel").unwrap();
    lib.set_tag_group_color(&mood, None).unwrap();
    assert!(lib.set_tag_group_color(&mood, Some("grey")).is_err());
    lib.reorder_tag_group(&style, 0).unwrap();

    let reopened = Library::open(lib.root()).unwrap();
    let g = &reopened.metadata.tags_groups;
    assert_eq!((g[0].id.as_str(), g[0].tags.clone()), (style.as_str(), strings(&["loud"])));
    assert_eq!((g[1].name.as_str(), g[1].tags.clone(), g[1].color.clone()), ("Feel", strings(&["b", "a"]), None));

    // Tag renames and deletes reach the groups.
    lib.rename_tag("b", "a").unwrap();
    assert_eq!(lib.metadata.tags_groups[1].tags, ["a"]);
    lib.delete_tag("loud").unwrap();
    assert!(lib.metadata.tags_groups[0].tags.is_empty());

    lib.delete_tag_group(&style, false).unwrap();
    assert_eq!(Library::open(lib.root()).unwrap().metadata.tags_groups.len(), 1);
    assert!(lib.rename_tag_group(&style, "x").is_err());
}

#[test]
fn deleting_a_group_can_take_its_tags_off_items() {
    let Some(src) = fixture() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("Groups.library");
    Library::open(&src).unwrap().save_to(&dest).unwrap();
    let mut lib = Library::open(&dest).unwrap();
    let tag = lib.items().iter().find_map(|i| i.tags.first().cloned()).expect("fixture has tags");
    let g = lib.create_tag_group("G", std::slice::from_ref(&tag), None).unwrap();
    lib.delete_tag_group(&g, true).unwrap();
    let reopened = Library::open(&dest).unwrap();
    assert!(reopened.metadata.tags_groups.is_empty());
    assert!(reopened.items().iter().all(|i| !i.tags.contains(&tag)));
}

fn fixture() -> Option<std::path::PathBuf> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/Assets.library");
    p.is_dir().then_some(p)
}

#[test]
fn rename_normalises_and_drops_stale_pinyin() {
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = new_library(tmp.path());
    let a = lib.create_folder(None, "A").unwrap();
    lib.metadata.folders[0].pinyin = Some("A".into());
    lib.rename_folder(&a, "Cafe\u{301}").unwrap();
    let f = &lib.metadata.folders[0];
    assert_eq!((f.name.as_str(), f.pinyin.as_deref()), ("Café", None));
    lib.rename_folder(&a, &"x".repeat(2000)).unwrap();
    assert_eq!(lib.metadata.folders[0].name.len(), 1024);
}
