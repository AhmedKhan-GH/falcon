//! Importing the files of a synthetic Pixave package into a temporary
//! library, as the app's File ▸ Import ▸ Pixave Library… does.

use std::fs;
use std::path::{Path, PathBuf};

use falcon_library::import::ImportOptions;
use falcon_library::{Library, pixave};
use image::{ImageFormat, Rgb, RgbImage};

fn hashes(root: &Path) -> Vec<(PathBuf, blake3::Hash)> {
    fn go(dir: &Path, out: &mut Vec<(PathBuf, blake3::Hash)>) {
        for e in fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                go(&p, out);
            } else {
                out.push((p.clone(), blake3::hash(&fs::read(&p).unwrap())));
            }
        }
    }
    let mut out = Vec::new();
    go(root, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn imports_the_files_into_a_folder_named_after_the_library() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("Source").join("Moodboard.pxvlibrary");
    fs::create_dir_all(src.join("Items/one.pxvitem")).unwrap();
    fs::create_dir_all(src.join("Items/two.pxvitem")).unwrap();
    RgbImage::from_fn(64, 32, |x, _| Rgb([x as u8 * 4, 40, 90])).save_with_format(src.join("Items/one.pxvitem/sky.png"), ImageFormat::Png).unwrap();
    RgbImage::from_fn(16, 16, |_, y| Rgb([10, y as u8 * 8, 10])).save_with_format(src.join("Items/two.pxvitem/leaf.png"), ImageFormat::Png).unwrap();
    let conn = rusqlite::Connection::open(src.join("Pixave.db")).unwrap();
    conn.execute_batch("CREATE TABLE images (id INTEGER PRIMARY KEY, name TEXT);").unwrap();
    drop(conn);
    let before = hashes(&src);

    let probe = pixave::probe(&src).unwrap();
    let files = probe.files();
    assert_eq!(files.len(), 2);

    let root = tmp.path().join("Sandbox").join("Dest.library");
    fs::create_dir_all(root.parent().unwrap()).unwrap();
    let mut lib = Library::create(&root).unwrap();
    let folder = lib.create_folder(None, "Moodboard").unwrap();
    let opts = ImportOptions { folders: vec![folder.clone()], palette: false, ..Default::default() };
    let ids: Vec<String> = files.iter().map(|f| lib.import_file(f, &opts).unwrap()).collect();
    drop(lib);

    assert_eq!(hashes(&src), before, "the Pixave package is left untouched");
    for f in &files {
        assert!(f.exists(), "files are copied, never moved");
    }

    let lib = Library::open(&root).unwrap();
    let mut names: Vec<(String, String)> = ids.iter().map(|id| lib.item(id).unwrap()).map(|i| (i.name.clone(), i.ext.clone())).collect();
    names.sort();
    assert_eq!(names, vec![("leaf".to_string(), "png".to_string()), ("sky".to_string(), "png".to_string())]);
    for id in &ids {
        let item = lib.item(id).unwrap();
        assert_eq!(item.folders, vec![folder.clone()]);
        assert!(item.original_path(lib.root()).is_file());
    }
    assert!(lib.all_folders().iter().any(|f| f.id == folder && f.name == "Moodboard"));

    // The Eagle JSON written for the new items and folder round-trips.
    let copy = tmp.path().join("Sandbox").join("Copy.library");
    lib.save_to(&copy).unwrap();
    let mut files = vec!["metadata.json".to_string()];
    files.extend(ids.iter().map(|id| format!("images/{id}.info/metadata.json")));
    for f in &files {
        assert_eq!(fs::read(root.join(f)).unwrap(), fs::read(copy.join(f)).unwrap(), "{f}");
    }
    let again = Library::open(&copy).unwrap();
    assert_eq!(again.items(), lib.items());
    assert_eq!(again.metadata, lib.metadata);
}
