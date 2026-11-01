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

#[test]
fn saved_filters() {
    use falcon_library::model::SavedFilter;
    use falcon_library::search::Filter;
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = new_library(tmp.path());
    let path = lib.root().join("saved-filters.json");
    assert_eq!(fs::read_to_string(&path).unwrap(), "[]");

    let mut f = Filter::default();
    f.tags.includes.push("red".into());
    let rule = SavedFilter::rule_from_filter(&f, None, |_| None);
    assert_eq!(lib.create_saved_filter("Red", rule.clone()).unwrap(), 0);
    assert_eq!(lib.create_saved_filter("Plain", serde_json::json!({"keyword": "cat"})).unwrap(), 1);
    assert!(lib.create_saved_filter("", rule.clone()).is_err());
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.starts_with(r#"[{"name":"Red","rule":{"type":"#) && text.ends_with(r#"{"name":"Plain","rule":{"keyword":"cat"}}]"#), "{text}");

    lib.rename_saved_filter(1, "Cats").unwrap();
    lib.update_saved_filter(1, serde_json::json!({"keyword": "dog"})).unwrap();
    lib.reorder_saved_filter(1, 0).unwrap();
    let reopened = Library::open(lib.root()).unwrap();
    assert_eq!(reopened.saved_filters[0].name, "Cats");
    assert_eq!(reopened.saved_filters[0].to_filter().keyword.as_deref(), Some("dog"));
    assert_eq!(reopened.saved_filters[1].to_filter().filter, f);

    lib.delete_saved_filter(0).unwrap();
    assert!(lib.delete_saved_filter(5).is_err());
    assert_eq!(Library::open(lib.root()).unwrap().saved_filters.len(), 1);
}

#[test]
fn nested_smart_folders_icons_and_moves() {
    use serde_json::json;
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = new_library(tmp.path());
    let rule = |prop: &str, method: &str, value: serde_json::Value| vec![json!({"match": "AND", "rules": [{"property": prop, "method": method, "value": value}]})];
    let images = lib.create_smart_folder("Images", rule("type", "equal", json!("png"))).unwrap();
    let wide = lib.create_smart_folder_in(Some(&images), "Wide", rule("shape", "equal", json!("landscape"))).unwrap();
    let other = lib.create_smart_folder("Other", vec![]).unwrap();
    assert!(lib.create_smart_folder_in(Some("NOPE"), "x", vec![]).is_err());

    let text = metadata(&lib);
    assert!(text.contains(&format!(r#"{{"id":"{images}","name":"Images","description":"","modificationTime":"#)), "{text}");
    assert!(text.contains(&format!(r#""children":[],"parent":"{images}"}}"#)), "{text}");

    lib.set_smart_folder_icon(&images, Some("star"), Some("pink")).unwrap();
    lib.set_smart_folder_icon(&wide, Some("film"), None).unwrap();
    lib.set_smart_folder_order(&images, Some(FolderOrder::Random), true).unwrap();
    assert!(lib.set_smart_folder_icon(&images, None, Some("beige")).is_err());
    let text = metadata(&lib);
    assert!(text.contains(&format!(r#"{{"id":"{images}","icon":"star","iconColor":"pink","name":"Images""#)), "{text}");
    assert!(text.contains(&format!(r#""parent":"{images}","icon":"film"}}],"orderBy":"RANDOM","sortIncrease":true}}"#)), "{text}");

    let sf = lib.smart_folders();
    assert_eq!((sf[0].icon.as_deref(), sf[0].icon_color.as_deref(), sf[0].order_by.as_deref()), (Some("star"), Some("pink"), Some("RANDOM")));
    assert_eq!(sf[0].children[0].icon.as_deref(), Some("film"));

    // Moving: refuse cycles, set and clear `parent`, honour the index.
    assert!(lib.move_smart_folder(&images, Some(&wide), 0).is_err());
    lib.move_smart_folder(&other, Some(&images), 0).unwrap();
    let ids: Vec<String> = lib.smart_folders()[0].children.iter().map(|c| c.id.clone()).collect();
    assert_eq!(ids, [other.clone(), wide.clone()]);
    lib.move_smart_folder(&wide, None, 0).unwrap();
    let reopened = Library::open(lib.root()).unwrap();
    let top: Vec<&str> = reopened.metadata.smart_folders.iter().map(|v| v["id"].as_str().unwrap()).collect();
    assert_eq!(top, [wide.as_str(), images.as_str()]);
    assert!(reopened.metadata.smart_folders[0].get("parent").is_none());
    assert_eq!(reopened.metadata.smart_folders[1]["children"][0]["parent"], json!(images));

    lib.set_smart_folder_icon(&images, None, None).unwrap();
    lib.set_smart_folder_order(&images, None, true).unwrap();
    assert!(lib.metadata.smart_folders[1].get("icon").is_none() && lib.metadata.smart_folders[1].get("orderBy").is_none());
    lib.delete_smart_folder(&images).unwrap();
    assert_eq!(Library::open(lib.root()).unwrap().smart_folders().len(), 1);
}

#[test]
fn nested_smart_folders_narrow_their_parents() {
    use serde_json::json;
    let Some(src) = fixture() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("Smart.library");
    Library::open(&src).unwrap().save_to(&dest).unwrap();
    let mut lib = Library::open(&dest).unwrap();
    let ext = lib.items().iter().find(|i| !i.is_deleted).unwrap().ext.clone();
    let parent = lib.create_smart_folder("By type", vec![json!({"match": "AND", "rules": [{"property": "type", "method": "equal", "value": ext}]})]).unwrap();
    let child = lib.create_smart_folder_in(Some(&parent), "Big", vec![json!({"match": "AND", "rules": [{"property": "width", "method": ">=", "value": [1, 0]}]})]).unwrap();
    let loose = lib.create_smart_folder("Big anywhere", vec![json!({"match": "AND", "rules": [{"property": "width", "method": ">=", "value": [1, 0]}]})]).unwrap();
    let (p, c, l) = (lib.smart_folder_items(&parent).len(), lib.smart_folder_items(&child).len(), lib.smart_folder_items(&loose).len());
    assert!(c <= p && c <= l && c > 0, "{p} {c} {l}");
    lib.move_smart_folder(&child, None, 99).unwrap();
    assert_eq!(lib.smart_folder_items(&child).len(), l, "no longer narrowed once moved out");
}

#[test]
fn comments_on_a_fixture_copy() {
    use falcon_library::model::Comment;
    let Some(src) = fixture() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("Comments.library");
    Library::open(&src).unwrap().save_to(&dest).unwrap();
    let mut lib = Library::open(&dest).unwrap();
    let item = lib.items()[0].clone();
    let file = item.info_dir(lib.root()).join("metadata.json");
    let before = fs::read_to_string(&file).unwrap();

    let region = lib.add_comment(&item.id, Comment::region(10, 20, 300, 150, "logo")).unwrap();
    let late = lib.add_comment(&item.id, Comment::at_time(9.0, "late")).unwrap();
    let early = lib.add_comment(&item.id, Comment::at_time(2.5, "early")).unwrap();
    let order: Vec<String> = lib.item(&item.id).unwrap().comments().into_iter().map(|c| c.id).collect();
    assert_eq!(order, [region.clone(), early.clone(), late.clone()]);

    let mut edited = lib.item(&item.id).unwrap().comments()[0].clone();
    let stamp = edited.last_modified;
    edited.annotation = "logo<br>mark".into();
    lib.update_comment(&item.id, edited).unwrap();
    lib.delete_comment(&item.id, &late).unwrap();
    assert!(lib.delete_comment(&item.id, &late).is_err());
    assert!(lib.add_comment("NOPE", Comment::default()).is_err());

    let reopened = Library::open(&dest).unwrap();
    let saved = reopened.item(&item.id).unwrap();
    let list = saved.comments();
    assert_eq!(list.len(), 2);
    assert_eq!((list[0].annotation.as_str(), list[0].last_modified), ("logo<br>mark", stamp));
    assert_eq!(list[1].duration.as_ref().and_then(|d| d.as_f64()), Some(2.5));
    assert!(saved.last_modified > item.last_modified, "edits bump the item's lastModified");

    // Everything before `comments` is unchanged apart from lastModified.
    let after = fs::read_to_string(&file).unwrap();
    let cut = |s: &str| s.split(r#","lastModified":"#).next().unwrap().to_string();
    assert_eq!(cut(&before), cut(&after));
    assert!(after.contains(r#","comments":[{"id":""#));

    lib.delete_comment(&item.id, &region).unwrap();
    lib.delete_comment(&item.id, &early).unwrap();
    assert!(fs::read_to_string(&file).unwrap().contains(r#""comments":[]"#), "an empty list stays, as in Eagle");
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

#[test]
fn tag_renames_reach_folder_auto_tags_and_smart_rules_in_one_undo_step() {
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = new_library(tmp.path());
    let a = lib.create_folder(None, "A").unwrap();
    let b = lib.create_folder(Some(&a), "B").unwrap();
    lib.set_folder_auto_tags(&a, &strings(&["old", "keep"])).unwrap();
    lib.set_folder_auto_tags(&b, &strings(&["new", "old"])).unwrap();
    let rule = serde_json::json!([{"match": "AND", "boolean": "TRUE", "rules": [{"property": "tags", "method": "union", "value": ["old", "x"]}, {"property": "name", "method": "contains", "value": "old"}]}]);
    let parent = lib.create_smart_folder("S", rule.as_array().unwrap().clone()).unwrap();
    lib.create_smart_folder_in(Some(&parent), "C", serde_json::from_value(serde_json::json!([{"match": "OR", "rules": [{"property": "tags", "method": "intersection", "value": ["new", "old"]}]}])).unwrap()).unwrap();
    let before = metadata(&lib);

    lib.begin_transaction("Rename tag");
    lib.rename_tag("old", "new").unwrap();
    let tx = lib.end_transaction().unwrap();
    let text = metadata(&lib);
    assert_eq!(lib.metadata.folders[0].tags, ["new", "keep"]);
    assert_eq!(lib.metadata.folders[0].children[0].tags, ["new"]);
    assert!(text.contains(r#""rules":[{"property":"tags","method":"union","value":["new","x"]},{"property":"name","method":"contains","value":"old"}]"#), "{text}");
    assert!(text.contains(r#""value":["new"]"#), "{text}");

    // One step puts every list back as it was.
    lib.revert(tx).unwrap();
    let back = metadata(&lib);
    let strip = |s: &str| s.split(r#""modificationTime""#).next().unwrap().to_string();
    assert_eq!(strip(&back), strip(&before));
    assert_eq!(lib.metadata.folders[0].tags, ["old", "keep"]);

    // Deleting a tag takes it off folders' auto-tags, not off rules.
    lib.delete_tag("old").unwrap();
    assert_eq!(lib.metadata.folders[0].tags, ["keep"]);
    assert_eq!(lib.metadata.folders[0].children[0].tags, ["new"]);
    assert!(metadata(&lib).contains(r#""value":["old","x"]"#));
}
