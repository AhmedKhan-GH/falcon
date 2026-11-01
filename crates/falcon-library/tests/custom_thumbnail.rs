//! Custom thumbnails, Refresh Thumbnail and Refresh Palette on a temp library.

use std::fs;
use std::io::Cursor;
use std::path::Path;

use falcon_library::import::ImportOptions;
use falcon_library::{Library, thumbnail};
use image::{ImageFormat, Rgb, RgbImage};

fn picture(w: u32, h: u32, tint: u8) -> RgbImage {
    RgbImage::from_fn(w, h, |x, y| Rgb([tint, ((x * 255) / w) as u8, ((y * 255) / h) as u8]))
}

fn write(img: &RgbImage, path: &Path, fmt: ImageFormat) {
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), fmt).unwrap();
    fs::write(path, out).unwrap();
}

fn keys(lib: &Library, id: &str) -> Vec<String> {
    let item = lib.item(id).unwrap();
    let text = fs::read_to_string(item.info_dir(lib.root()).join("metadata.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    v.as_object().unwrap().keys().cloned().collect()
}

fn setup(file: &str, img: &RgbImage, fmt: ImageFormat) -> (tempfile::TempDir, Library, String) {
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = Library::create(tmp.path().join("T.library")).unwrap();
    let src = tmp.path().join(file);
    write(img, &src, fmt);
    let id = lib.import_file(&src, &ImportOptions::default()).unwrap();
    (tmp, lib, id)
}

#[test]
fn custom_thumbnail_is_copied_and_flagged() {
    let (tmp, mut lib, id) = setup("anim.gif", &picture(1200, 900, 10), ImageFormat::Gif);
    let before = lib.item(&id).unwrap().clone();
    let order = keys(&lib, &id);
    let custom = tmp.path().join("cover.jpg");
    write(&picture(400, 300, 250), &custom, ImageFormat::Jpeg);

    lib.set_custom_thumbnail(&id, &custom).unwrap();
    let item = lib.item(&id).unwrap().clone();
    let thumb = item.thumbnail_path(lib.root()).unwrap();
    assert_eq!(fs::read(&thumb).unwrap(), fs::read(&custom).unwrap(), "copied byte for byte");
    assert_eq!((item.width, item.height), (400, 300));
    assert!(thumbnail::has_custom_thumbnail(&item));
    assert_ne!(item.palettes, before.palettes);
    let mut expected = order.clone();
    expected.push("customThumbnail".into());
    assert_eq!(keys(&lib, &id), expected, "the flag is appended; nothing else moves");

    // Refresh leaves a custom thumbnail alone.
    assert!(!lib.refresh_thumbnail(&id).unwrap());
    assert_eq!(fs::read(&thumb).unwrap(), fs::read(&custom).unwrap());

    // Reset draws the item's own again.
    assert!(lib.reset_custom_thumbnail(&id).unwrap());
    let item = lib.item(&id).unwrap().clone();
    assert!(!thumbnail::has_custom_thumbnail(&item));
    assert_eq!((item.width, item.height), (1200, 900));
    let t = thumbnail::decode(&item.thumbnail_path(lib.root()).unwrap()).unwrap();
    assert_eq!((t.width(), t.height()), (426, 320));
    assert_eq!(keys(&lib, &id), order);
    assert_eq!(item.palettes, before.palettes);
}

#[test]
fn frame_thumbnail_is_a_png_like_eagles() {
    let (_tmp, mut lib, id) = setup("anim.gif", &picture(1200, 900, 10), ImageFormat::Gif);
    let before = lib.item(&id).unwrap().clone();
    let order = keys(&lib, &id);
    let frame = image::DynamicImage::ImageRgb8(picture(1200, 900, 250)).to_rgba8();
    lib.set_frame_thumbnail(&id, &frame).unwrap();
    let item = lib.item(&id).unwrap().clone();
    let thumb = item.thumbnail_path(lib.root()).unwrap();
    let bytes = fs::read(&thumb).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "written as PNG, as Eagle's canvas does");
    let t = thumbnail::decode(&thumb).unwrap();
    assert_eq!((t.width(), t.height()), (640, 480));
    assert_eq!((item.width, item.height), (1200, 900), "the item keeps its size");
    assert!(!thumbnail::has_custom_thumbnail(&item));
    assert_ne!(item.palettes, before.palettes);
    assert_eq!(keys(&lib, &id), order, "no keys added or moved");
}

#[test]
fn custom_thumbnail_refusals() {
    let (tmp, mut lib, id) = setup("a.png", &picture(50, 50, 0), ImageFormat::Png);
    let custom = tmp.path().join("c.png");
    write(&picture(20, 20, 0), &custom, ImageFormat::Png);
    assert!(lib.set_custom_thumbnail(&id, &custom).is_err(), "a png is its own thumbnail");
    let (tmp, mut lib, id) = setup("a.gif", &picture(50, 50, 0), ImageFormat::Gif);
    let junk = tmp.path().join("junk.png");
    fs::write(&junk, b"not an image").unwrap();
    assert!(lib.set_custom_thumbnail(&id, &junk).is_err());
    assert!(!thumbnail::has_custom_thumbnail(lib.item(&id).unwrap()));
}

#[test]
fn refresh_thumbnail_picks_up_a_changed_original() {
    let (_tmp, mut lib, id) = setup("p.png", &picture(1000, 1000, 0), ImageFormat::Png);
    let order = keys(&lib, &id);
    let item = lib.item(&id).unwrap().clone();
    let raw = item.original_path(lib.root());
    // Replace the original behind the library's back.
    write(&picture(2000, 1000, 255), &raw, ImageFormat::Png);
    assert!(lib.refresh_thumbnail(&id).unwrap());
    let after = lib.item(&id).unwrap().clone();
    assert_eq!((after.width, after.height), (2000, 1000));
    assert_eq!(after.size, fs::metadata(&raw).unwrap().len());
    let t = thumbnail::decode(&after.thumbnail_path(lib.root()).unwrap()).unwrap();
    assert_eq!((t.width(), t.height()), (640, 320));
    assert_ne!(after.palettes, item.palettes);
    assert_eq!(keys(&lib, &id), order);
}

#[test]
fn refresh_palette_recomputes_only_the_palette() {
    let (_tmp, mut lib, id) = setup("p.png", &picture(300, 200, 90), ImageFormat::Png);
    let mut item = lib.item(&id).unwrap().clone();
    let good = item.palettes.clone();
    assert!(!good.is_empty());
    item.palettes.clear();
    lib.update_item(item).unwrap();
    let order = keys(&lib, &id);
    assert!(lib.refresh_palette(&id).unwrap());
    let after = lib.item(&id).unwrap();
    assert_eq!(after.palettes, good, "same median cut as import");
    assert_eq!(keys(&lib, &id), order);
}
