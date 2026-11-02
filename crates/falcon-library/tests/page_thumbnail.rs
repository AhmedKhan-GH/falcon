//! Saving a page screenshot as a `.url` bookmark's thumbnail, on a temp library.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use falcon_library::import::ImportOptions;
use falcon_library::thumbnail::{self, PageThumbnail};
use falcon_library::Library;
use image::{DynamicImage, Rgb, RgbImage};

fn page(w: u32, h: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| Rgb([200, ((x * 255) / w) as u8, ((y * 255) / h) as u8])))
}

fn setup() -> (tempfile::TempDir, Library, String) {
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = Library::create(tmp.path().join("T.library")).unwrap();
    let src = tmp.path().join("example.url");
    fs::write(&src, "[InternetShortcut]\r\nURL=https://example.com/\r\n").unwrap();
    let id = lib.import_file(&src, &ImportOptions::default()).unwrap();
    (tmp, lib, id)
}

fn listing(root: &Path, skip_dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in fs::read_dir(&dir).unwrap() {
            let path = e.unwrap().path();
            if path == skip_dir {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else {
                let name = path.strip_prefix(root).unwrap().to_string_lossy().into_owned();
                // The mtime index and the library record are rewritten by any item edit.
                if name == "mtime.json" || name == "metadata.json" {
                    continue;
                }
                out.insert(name, fs::read(&path).unwrap());
            }
        }
    }
    out
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

fn metadata_keys(lib: &Library, id: &str) -> Vec<String> {
    let item = lib.item(id).unwrap();
    let text = fs::read_to_string(item.info_dir(lib.root()).join("metadata.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    v.as_object().unwrap().keys().cloned().collect()
}

#[test]
fn page_screenshot_becomes_the_thumbnail() {
    let (_tmp, mut lib, id) = setup();
    let before = lib.item(&id).unwrap().clone();
    assert_eq!(before.url, "https://example.com/");
    let shot = PageThumbnail::draw(page(1440, 900)).unwrap();

    assert!(lib.set_page_thumbnail(&id, &shot, 1440, 900).unwrap());
    let item = lib.item(&id).unwrap().clone();
    let thumb = item.info_dir(lib.root()).join(format!("{}_thumbnail.png", item.name));
    assert!(thumb.is_file());
    let t = thumbnail::decode(&thumb).unwrap();
    assert_eq!(t.width().min(t.height()), 320);
    assert_eq!((t.width(), t.height()), (512, 320));
    assert_eq!((item.width, item.height), (1440, 900));
    assert!(!item.palettes.is_empty());
    assert_eq!((&item.url, item.size, item.btime, item.mtime), (&before.url, before.size, before.btime, before.mtime));
    assert_eq!((item.no_thumbnail, item.remove_thumbnail), (None, None));
    assert!(!thumbnail::has_custom_thumbnail(&item));
    assert!(!metadata_keys(&lib, &id).contains(&"customThumbnail".to_string()));
    assert_eq!(item.tags, before.tags);

    let info = item.info_dir(lib.root());
    assert!(names(&info).iter().all(|n| !n.starts_with(".falcon")), "{:?}", names(&info));

    let reopened = Library::open(lib.root()).unwrap();
    let again = reopened.item(&id).unwrap();
    assert_eq!((again.width, again.height), (1440, 900));
    assert_eq!(again.palettes, item.palettes);
    assert_eq!(fs::read(again.thumbnail_path(reopened.root()).unwrap()).unwrap(), fs::read(&thumb).unwrap());
}

#[test]
fn custom_thumbnail_and_unknown_id_are_left_alone() {
    let (tmp, mut lib, id) = setup();
    let cover = tmp.path().join("cover.png");
    page(300, 200).save(&cover).unwrap();
    lib.set_custom_thumbnail(&id, &cover).unwrap();
    let item = lib.item(&id).unwrap().clone();
    let thumb = item.thumbnail_path(lib.root()).unwrap();
    let bytes = fs::read(&thumb).unwrap();
    let info = item.info_dir(lib.root());
    let files = names(&info);

    let shot = PageThumbnail::draw(page(1440, 900)).unwrap();
    assert!(!lib.set_page_thumbnail(&id, &shot, 1440, 900).unwrap());
    assert_eq!(fs::read(&thumb).unwrap(), bytes);
    assert_eq!(names(&info), files);
    assert_eq!((lib.item(&id).unwrap().width, lib.item(&id).unwrap().height), (300, 200));

    assert!(!lib.set_page_thumbnail("NOSUCHID", &shot, 1440, 900).unwrap());
}

#[test]
fn a_renamed_item_writes_under_its_new_name() {
    let (_tmp, mut lib, id) = setup();
    let mut item = lib.item(&id).unwrap().clone();
    item.name = "Renamed".into();
    lib.update_item(item).unwrap();
    let shot = PageThumbnail::draw(page(1440, 900)).unwrap();
    assert!(lib.set_page_thumbnail(&id, &shot, 1440, 900).unwrap());
    let item = lib.item(&id).unwrap().clone();
    assert_eq!(item.name, "Renamed");
    let info = item.info_dir(lib.root());
    assert!(info.join("Renamed_thumbnail.png").is_file());
    assert!(!info.join("example_thumbnail.png").exists());
}

#[test]
fn only_the_items_own_folder_changes() {
    let (tmp, mut lib, id) = setup();
    let other = tmp.path().join("other.url");
    fs::write(&other, "[InternetShortcut]\r\nURL=https://example.org/\r\n").unwrap();
    let other_id = lib.import_file(&other, &ImportOptions::default()).unwrap();
    let info = lib.item(&id).unwrap().info_dir(lib.root());
    let other_info = lib.item(&other_id).unwrap().info_dir(lib.root());
    let before = listing(lib.root(), &info);
    let other_before = names(&other_info);

    let shot = PageThumbnail::draw(page(1440, 900)).unwrap();
    assert!(lib.set_page_thumbnail(&id, &shot, 1440, 900).unwrap());

    assert_eq!(listing(lib.root(), &info), before);
    assert_eq!(names(&other_info), other_before);
}
