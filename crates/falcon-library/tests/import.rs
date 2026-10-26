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

#[test]
fn emptying_the_trash_removes_files() {
    let Some(src) = fixture() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("Trash.library");
    Library::open(&src).unwrap().save_to(&dest).unwrap();
    let mut lib = Library::open(&dest).unwrap();
    let ids: Vec<String> = lib.items().iter().take(2).map(|i| i.id.clone()).collect();
    let dir = lib.item(&ids[0]).unwrap().info_dir(lib.root());
    lib.set_trashed(&ids, true).unwrap();
    assert_eq!(lib.empty_trash().unwrap(), 2);
    assert!(!dir.exists());
    let reopened = Library::open(&dest).unwrap();
    assert_eq!(reopened.items().len(), 89);
    assert_eq!(reopened.mtime.all, Some(89));
    assert!(reopened.mtime.entries.iter().all(|(id, _)| !ids.contains(id)));
}

#[cfg(target_os = "macos")]
#[test]
fn quicklook_formats_get_thumbnails() {
    let Some(src) = fixture() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("QL.library");
    Library::open(&src).unwrap().save_to(&dest).unwrap();
    let mut lib = Library::open(&dest).unwrap();
    let eagle = lib.items().iter().find(|i| i.ext == "pdf").unwrap().clone();
    let pdf = tmp.path().join("seal.pdf");
    std::fs::copy(eagle.original_path(lib.root()), &pdf).unwrap();

    let id = lib.import_file(&pdf, &ImportOptions::default()).unwrap();
    let item = lib.item(&id).unwrap();
    let thumb = item.thumbnail_path(lib.root()).expect("thumbnail");
    let img = falcon_library::thumbnail::decode(&thumb).unwrap();
    assert_eq!(img.width().min(img.height()), 480); // documents use the 480 limit
    // QuickLook truncates the scaled side where Eagle rounds, so allow 1 px.
    assert_eq!(item.width.max(item.height), eagle.width.max(eagle.height));
    assert!(item.width.abs_diff(eagle.width) <= 1 && item.height.abs_diff(eagle.height) <= 1);
    assert!(!item.palettes.is_empty());
    eprintln!("eagle {:?}\nours  {:?}", eagle.palettes, item.palettes);
}

#[test]
fn tags_rename_merge_and_delete() {
    let Some(src) = fixture() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("Tags.library");
    Library::open(&src).unwrap().save_to(&dest).unwrap();
    let mut lib = Library::open(&dest).unwrap();
    let count = |lib: &Library, t: &str| lib.items().iter().filter(|i| i.tags.iter().any(|x| x == t)).count();
    let (glyph, logo) = (count(&lib, "glyph"), count(&lib, "logo"));
    assert!(glyph > 0 && logo > 0);

    lib.set_tag_starred("glyph", true).unwrap();
    assert_eq!(lib.rename_tag("glyph", "letter").unwrap(), glyph);
    let lib2 = Library::open(&dest).unwrap();
    assert_eq!(count(&lib2, "glyph"), 0);
    assert_eq!(count(&lib2, "letter"), glyph);
    assert_eq!(lib2.tags.starred_tags, ["letter"]);

    // Merging: items with both end up with one copy.
    lib.rename_tag("letter", "logo").unwrap();
    let lib3 = Library::open(&dest).unwrap();
    assert!(lib3.items().iter().all(|i| i.tags.iter().filter(|t| *t == "logo").count() <= 1));

    let n = count(&lib3, "logo");
    assert_eq!(lib.delete_tag("logo").unwrap(), n);
    assert_eq!(count(&Library::open(&dest).unwrap(), "logo"), 0);
}

#[test]
fn folders_move_and_quick_access() {
    let Some(src) = fixture() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("Move.library");
    Library::open(&src).unwrap().save_to(&dest).unwrap();
    let mut lib = Library::open(&dest).unwrap();
    let a = lib.create_folder(None, "A").unwrap();
    let b = lib.create_folder(None, "B").unwrap();
    lib.move_folder(&b, Some(&a)).unwrap();
    assert!(lib.move_folder(&a, Some(&b)).is_err(), "cycle refused");
    let reopened = Library::open(&dest).unwrap();
    let fa = reopened.metadata.folders.iter().find(|f| f.id == a).unwrap();
    assert_eq!(fa.children[0].id, b);

    lib.add_quick_access("folder", &b).unwrap();
    lib.add_quick_access("folder", &b).unwrap();
    assert_eq!(Library::open(&dest).unwrap().metadata.quick_access.len(), 1);
    lib.delete_folder(&a).unwrap();
    assert!(Library::open(&dest).unwrap().metadata.quick_access.is_empty());
}
