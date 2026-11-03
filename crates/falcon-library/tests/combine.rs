//! Combine Images on a temp library: real files in, a real PNG item out, and
//! the sources left as they were.

use std::fs;
use std::path::Path;

use falcon_library::Library;
use falcon_library::combine::{self, Layout, Options};
use falcon_library::import::ImportOptions;
use image::{ImageFormat, Rgb, RgbImage, Rgba, RgbaImage};

fn solid(path: &Path, w: u32, h: u32, colour: [u8; 3]) {
    RgbImage::from_pixel(w, h, Rgb(colour)).save(path).unwrap();
}

/// A JPEG whose EXIF says to turn it a quarter clockwise (orientation 6).
fn turned_jpeg(path: &Path, w: u32, h: u32) {
    let mut jpeg = Vec::new();
    RgbImage::from_pixel(w, h, Rgb([0, 0, 255])).write_to(&mut std::io::Cursor::new(&mut jpeg), ImageFormat::Jpeg).unwrap();
    let mut tiff = b"II*\0\x08\0\0\0\x01\0".to_vec();
    tiff.extend([0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let mut app1 = b"Exif\0\0".to_vec();
    app1.extend(tiff);
    let mut out = vec![0xFF, 0xD8, 0xFF, 0xE1];
    out.extend(((app1.len() + 2) as u16).to_be_bytes());
    out.extend(app1);
    out.extend(&jpeg[2..]);
    fs::write(path, out).unwrap();
}

fn keep(_: usize, _: &Path) -> bool {
    true
}

const OPAQUE_WHITE: Option<[u8; 3]> = Some([255, 255, 255]);

#[test]
fn a_combine_makes_one_new_png_and_leaves_the_sources() {
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = Library::create(tmp.path().join("T.library")).unwrap();
    let folder = lib.create_folder(None, "Sheets").unwrap();
    let (red, green) = (tmp.path().join("red.png"), tmp.path().join("green.png"));
    solid(&red, 10, 20, [255, 0, 0]);
    solid(&green, 30, 20, [0, 255, 0]);
    let ids = [lib.import_file(&red, &ImportOptions::default()).unwrap(), lib.import_file(&green, &ImportOptions::default()).unwrap()];
    let originals: Vec<_> = ids.iter().map(|id| lib.item(id).unwrap().original_path(lib.root())).collect();
    let snapshot = |lib: &Library| -> Vec<(Vec<u8>, String)> {
        ids.iter()
            .zip(&originals)
            .map(|(id, file)| (fs::read(file).unwrap(), fs::read_to_string(lib.item(id).unwrap().info_dir(lib.root()).join("metadata.json")).unwrap()))
            .collect()
    };
    let before = snapshot(&lib);
    let count = lib.items().len();

    let options = Options { layout: Layout::Horizontal, gap: 4, background: OPAQUE_WHITE };
    let import = ImportOptions { folders: vec![folder.clone()], name: Some("Combined Image".into()), ..Default::default() };
    let done = combine::stage(&originals, options, &import, keep).unwrap().unwrap();
    assert_eq!((done.width, done.height, done.scaled_down), (44, 20, false));

    lib.begin_transaction("Combine");
    let new = lib.commit_import(done.staged).unwrap();
    let tx = lib.end_transaction().unwrap();

    let item = lib.item(&new).unwrap().clone();
    assert_eq!((item.ext.as_str(), item.name.as_str()), ("png", "Combined Image"));
    assert_eq!((item.width, item.height), (44, 20));
    assert_eq!(item.folders, [folder]);
    let out = image::open(item.original_path(lib.root())).unwrap().into_rgba8();
    assert_eq!(*out.get_pixel(0, 0), Rgba([255, 0, 0, 255]));
    assert_eq!(*out.get_pixel(9, 19), Rgba([255, 0, 0, 255]));
    assert_eq!(*out.get_pixel(12, 10), Rgba([255, 255, 255, 255]), "the gap shows the background");
    assert_eq!(*out.get_pixel(14, 0), Rgba([0, 255, 0, 255]));
    assert_eq!(*out.get_pixel(43, 19), Rgba([0, 255, 0, 255]));

    assert_eq!(snapshot(&lib), before, "the sources are untouched");
    assert_eq!(lib.items().len(), count + 1);

    // One undo step takes it away again, to the trash as for other new items.
    lib.revert(tx).unwrap();
    assert!(lib.item(&new).unwrap().is_deleted);
    assert_eq!(snapshot(&lib), before);
}

#[test]
fn a_transparent_grid_scales_to_its_cells() {
    let tmp = tempfile::tempdir().unwrap();
    let (a, b, c) = (tmp.path().join("a.png"), tmp.path().join("b.png"), tmp.path().join("c.bmp"));
    solid(&a, 40, 40, [10, 20, 30]);
    solid(&b, 20, 20, [200, 100, 50]);
    solid(&c, 40, 20, [90, 90, 90]);
    let options = Options { layout: Layout::Grid(2), gap: 0, background: None };
    let (out, _) = combine::compose(&[&a, &b, &c], options, &mut keep).unwrap().unwrap();
    assert_eq!(out.dimensions(), (80, 80));
    assert_eq!(*out.get_pixel(20, 20), Rgba([10, 20, 30, 255]));
    // The small one is scaled to fill its cell.
    assert_eq!(*out.get_pixel(41, 1), Rgba([200, 100, 50, 255]));
    // The wide one is centred, with transparent space above, and the empty
    // last cell stays clear.
    assert_eq!(out.get_pixel(20, 41)[3], 0);
    assert_eq!(*out.get_pixel(20, 60), Rgba([90, 90, 90, 255]));
    assert_eq!(out.get_pixel(60, 60)[3], 0);
}

#[test]
fn transparent_pictures_show_the_background_through() {
    let tmp = tempfile::tempdir().unwrap();
    let (a, b) = (tmp.path().join("a.png"), tmp.path().join("b.png"));
    RgbaImage::from_pixel(4, 4, Rgba([0, 0, 0, 0])).save(&a).unwrap();
    solid(&b, 4, 4, [1, 2, 3]);
    let options = Options { layout: Layout::Vertical, gap: 0, background: Some([9, 8, 7]) };
    let (out, _) = combine::compose(&[&a, &b], options, &mut keep).unwrap().unwrap();
    assert_eq!(*out.get_pixel(1, 1), Rgba([9, 8, 7, 255]));
    assert_eq!(*out.get_pixel(1, 6), Rgba([1, 2, 3, 255]));
}

#[test]
fn exif_orientation_is_honoured() {
    let tmp = tempfile::tempdir().unwrap();
    let (turned, plain) = (tmp.path().join("turned.jpg"), tmp.path().join("plain.png"));
    turned_jpeg(&turned, 40, 20);
    solid(&plain, 10, 40, [0, 255, 0]);
    assert_eq!(combine::display_size(&turned).unwrap(), (20, 40));
    let options = Options { layout: Layout::Horizontal, gap: 0, background: None };
    let (out, _) = combine::compose(&[&turned, &plain], options, &mut keep).unwrap().unwrap();
    assert_eq!(out.dimensions(), (30, 40));
    assert_eq!(*out.get_pixel(25, 20), Rgba([0, 255, 0, 255]));
    let p = out.get_pixel(10, 20);
    assert!(p[2] > 200 && p[1] < 60, "{p:?}");
}

#[test]
fn stopping_or_a_bad_file_makes_nothing_and_is_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let (good, bad) = (tmp.path().join("good.png"), tmp.path().join("bad.png"));
    solid(&good, 4, 4, [1, 2, 3]);
    fs::write(&bad, b"not a picture").unwrap();
    let options = Options { layout: Layout::Horizontal, gap: 0, background: None };
    let import = ImportOptions::default();
    let err = combine::stage(&[&good, &bad], options, &import, keep).unwrap_err();
    assert!(err.contains("bad.png"), "{err}");
    assert!(combine::stage(&[&good, &good], options, &import, |n, _| n == 0).unwrap().is_none());
    assert!(combine::stage(&[&good, &good], options, &import, |n, _| n < 2).unwrap().is_none(), "stopped before the save");
}
