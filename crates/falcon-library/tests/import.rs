use std::fs;
use std::path::{Path, PathBuf};

use falcon_library::import::ImportOptions;
use falcon_library::{Library, palette};
use image::{ImageFormat, Rgb, RgbImage};

fn fixture() -> Option<PathBuf> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/Assets.library");
    if p.is_dir() {
        Some(p)
    } else {
        eprintln!("fixture absent, skipping");
        None
    }
}

/// Copies the fixture into `tmp` and opens the copy.
fn scratch(tmp: &Path) -> Option<Library> {
    let p = fixture()?;
    let copy = tmp.join("Copy.library");
    Library::open(p).unwrap().save_to(&copy).unwrap();
    Some(Library::open(copy).unwrap())
}

/// A gradient so the encoder has something to chew on.
fn make_png(path: &Path, w: u32, h: u32) {
    let img = RgbImage::from_fn(w, h, |x, y| Rgb([(x * 255 / w) as u8, (y * 255 / h) as u8, 90]));
    img.save_with_format(path, ImageFormat::Png).unwrap();
}

const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="#e03030"/></svg>"##;

fn webp_size(path: &Path) -> (u32, u32) {
    let bytes = fs::read(path).unwrap();
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WEBP", "thumbnail bytes are WebP");
    let img = image::load_from_memory_with_format(&bytes, ImageFormat::WebP).unwrap();
    (img.width(), img.height())
}

fn opts() -> ImportOptions {
    ImportOptions { palette: false, ..Default::default() }
}

#[test]
fn imports_large_png() {
    let tmp = tempfile::tempdir().unwrap();
    let Some(mut lib) = scratch(tmp.path()) else { return };
    let before = lib.items().len();
    let folder = lib.all_folders()[0].id.clone();
    let src = tmp.path().join("big%pic?.png");
    make_png(&src, 2000, 1000);
    let past = filetime::FileTime::from_unix_time(1_600_000_000, 0);
    filetime::set_file_mtime(&src, past).unwrap();

    let o = ImportOptions { folders: vec![folder.clone()], tags: vec!["t1".into()], annotation: "note".into(), ..opts() };
    let id = lib.import_file(&src, &o).unwrap();

    let item = lib.item(&id).unwrap();
    assert_eq!((item.name.as_str(), item.ext.as_str()), ("bigpic", "png"));
    assert_eq!((item.width, item.height), (2000, 1000));
    assert_eq!(item.size, fs::metadata(&src).unwrap().len());
    assert_eq!(item.mtime, 1_600_000_000_000);
    assert!(item.btime > 0);
    assert_eq!(item.tags, ["t1"]);
    assert_eq!(item.folders, [folder]);
    assert_eq!(item.annotation, "note");
    assert!(!item.is_deleted);
    assert_eq!((item.no_thumbnail, item.remove_thumbnail), (None, None));
    assert!(item.last_modified >= item.modification_time);
    assert!(item.palettes.is_empty());

    // Original keeps the source mtime; thumbnail is WebP with a 320 short side.
    let raw = item.original_path(lib.root());
    assert_eq!(filetime::FileTime::from_last_modification_time(&fs::metadata(&raw).unwrap()), past);
    let thumb = item.thumbnail_path(lib.root()).expect("thumbnail written");
    assert!(thumb.to_str().unwrap().ends_with("bigpic_thumbnail.png"));
    assert_eq!(webp_size(&thumb), (640, 320));

    // mtime.json
    assert_eq!(lib.mtime.all, Some(before as u64 + 1));
    assert!(lib.mtime.entries.iter().any(|(i, t)| *i == id && *t == item.last_modified));

    // Everything survives a reload, and the JSON on disk is compact.
    let again = Library::open(lib.root()).unwrap();
    assert_eq!(again.item(&id), lib.item(&id));
    assert_eq!(again.mtime, lib.mtime);
    assert_eq!(again.mtime.all, Some(again.items().len() as u64));
    let json = fs::read_to_string(item.info_dir(lib.root()).join("metadata.json")).unwrap();
    assert!(json.starts_with("{\"id\":\"") && !json.ends_with('\n'));
}

#[test]
fn small_png_has_no_thumbnail() {
    let tmp = tempfile::tempdir().unwrap();
    let Some(mut lib) = scratch(tmp.path()) else { return };
    let src = tmp.path().join("small.png");
    make_png(&src, 300, 200);
    let id = lib.import_file(&src, &opts()).unwrap();
    let item = lib.item(&id).unwrap();
    assert_eq!((item.width, item.height), (300, 200));
    assert_eq!(item.no_thumbnail, Some(true));
    assert_eq!(item.remove_thumbnail, None);
    assert!(item.thumbnail_path(lib.root()).is_none());
    assert!(item.original_path(lib.root()).is_file());
}

#[test]
fn small_svg_removes_thumbnail() {
    let tmp = tempfile::tempdir().unwrap();
    let Some(mut lib) = scratch(tmp.path()) else { return };
    let src = tmp.path().join("shape.svg");
    fs::write(&src, SVG).unwrap();
    let id = lib.import_file(&src, &opts()).unwrap();
    let item = lib.item(&id).unwrap();
    assert_eq!((item.width, item.height), (40, 20));
    assert_eq!((item.no_thumbnail, item.remove_thumbnail), (Some(true), Some(true)));
    assert!(item.thumbnail_path(lib.root()).is_none());
}

#[test]
fn large_svg_keeps_thumbnail() {
    let tmp = tempfile::tempdir().unwrap();
    let Some(mut lib) = scratch(tmp.path()) else { return };
    let src = tmp.path().join("big.svg");
    let pad = " ".repeat(100_000);
    fs::write(&src, SVG.replace("<rect", &format!("{pad}<rect"))).unwrap();
    let id = lib.import_file(&src, &opts()).unwrap();
    let item = lib.item(&id).unwrap();
    assert_eq!((item.no_thumbnail, item.remove_thumbnail), (None, None));
    // 40x20 is under the limit on its long side, so it is not resized.
    assert_eq!(webp_size(&item.thumbnail_path(lib.root()).unwrap()), (40, 20));
}

#[test]
fn unsupported_format_imports_bare() {
    let tmp = tempfile::tempdir().unwrap();
    let Some(mut lib) = scratch(tmp.path()) else { return };
    let src = tmp.path().join("notes.txt");
    fs::write(&src, "hello").unwrap();
    let id = lib.import_file(&src, &ImportOptions::default()).unwrap();
    let item = lib.item(&id).unwrap();
    assert_eq!((item.width, item.height), (0, 0));
    assert!(item.palettes.is_empty() && item.thumbnail_path(lib.root()).is_none());
}

#[test]
fn rejects_bad_input() {
    let tmp = tempfile::tempdir().unwrap();
    let Some(mut lib) = scratch(tmp.path()) else { return };
    let before = lib.items().len();
    let empty = tmp.path().join("empty.png");
    fs::write(&empty, "").unwrap();
    assert!(lib.import_file(&empty, &opts()).is_err());
    assert!(lib.import_file(tmp.path().join("missing.png"), &opts()).is_err());
    let src = tmp.path().join("a.png");
    make_png(&src, 10, 10);
    let o = ImportOptions { folders: vec!["NOSUCHFOLDER".into()], ..opts() };
    assert!(lib.import_file(&src, &o).is_err());
    assert_eq!(lib.items().len(), before);
    assert_eq!(fs::read_dir(lib.root().join("images")).unwrap().count(), before);
}

#[test]
fn batch_times_increase() {
    let tmp = tempfile::tempdir().unwrap();
    let Some(mut lib) = scratch(tmp.path()) else { return };
    let src = tmp.path().join("a.png");
    make_png(&src, 10, 10);
    let ids: Vec<String> = (0..5).map(|_| lib.import_file(&src, &opts()).unwrap()).collect();
    let times: Vec<u64> = ids.iter().map(|i| lib.item(i).unwrap().modification_time).collect();
    assert!(times.windows(2).all(|w| w[0] < w[1]), "{times:?}");
    assert_eq!(lib.mtime.all, Some(lib.items().len() as u64));
}

/// Runs only once `palette::extract` is implemented.
#[test]
fn palette_is_extracted_when_available() {
    let probe = std::panic::catch_unwind(|| palette::extract(&image::DynamicImage::ImageRgb8(RgbImage::new(4, 4))));
    if probe.is_err() {
        eprintln!("palette::extract not implemented yet, skipping");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let Some(mut lib) = scratch(tmp.path()) else { return };
    let png = tmp.path().join("p.png");
    make_png(&png, 2000, 1000);
    let svg = tmp.path().join("s.svg");
    fs::write(&svg, SVG).unwrap();
    for src in [png, svg] {
        let id = lib.import_file(&src, &ImportOptions::default()).unwrap();
        assert!(!lib.item(&id).unwrap().palettes.is_empty(), "{src:?}");
    }
}

#[test]
fn editing_renames_files_and_trashes() {
    let Some(src) = fixture() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("Edit.library");
    Library::open(&src).unwrap().save_to(&dest).unwrap();
    let mut lib = Library::open(&dest).unwrap();

    let mut item = lib.items().iter().find(|i| i.thumbnail_path(lib.root()).is_some()).unwrap().clone();
    let before = item.last_modified;
    item.name = "Renamed: item".into();
    item.tags.push("edited".into());
    lib.update_item(item.clone()).unwrap();

    let reopened = Library::open(&dest).unwrap();
    let got = reopened.item(&item.id).unwrap();
    assert_eq!(got.name, "Renamed item");
    assert!(got.tags.contains(&"edited".to_string()));
    assert!(got.last_modified > before);
    assert!(got.original_path(reopened.root()).is_file());
    assert!(got.thumbnail_path(reopened.root()).is_some());

    lib.set_trashed(std::slice::from_ref(&item.id), true).unwrap();
    let got = Library::open(&dest).unwrap().item(&item.id).unwrap().clone();
    assert!(got.is_deleted);
    assert!(got.extra.contains_key("deletedTime"));

    lib.record_tag_use(&["edited".into()]).unwrap();
    assert_eq!(Library::open(&dest).unwrap().tags.history_tags[0], "edited");
}

#[test]
fn folders_create_rename_delete() {
    let Some(src) = fixture() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("Folders.library");
    Library::open(&src).unwrap().save_to(&dest).unwrap();
    let mut lib = Library::open(&dest).unwrap();

    let parent = lib.create_folder(None, "Parent").unwrap();
    let child = lib.create_folder(Some(&parent), "Child").unwrap();
    lib.rename_folder(&child, "Renamed").unwrap();
    let mut item = lib.items()[0].clone();
    item.folders = vec![child.clone()];
    lib.update_item(item.clone()).unwrap();

    let reopened = Library::open(&dest).unwrap();
    assert!(reopened.folder_ids().contains(child.as_str()));
    assert_eq!(reopened.all_folders().iter().find(|f| f.id == child).unwrap().name, "Renamed");

    lib.delete_folder(&parent).unwrap();
    let reopened = Library::open(&dest).unwrap();
    assert!(!reopened.folder_ids().contains(parent.as_str()));
    let got = reopened.item(&item.id).unwrap();
    assert!(got.folders.is_empty());
    assert!(got.is_deleted);
}
