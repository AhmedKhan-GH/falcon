use std::fs;
use std::path::{Path, PathBuf};

use falcon_library::Library;

fn fixture() -> Option<PathBuf> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/Assets.library");
    if p.is_dir() {
        Some(p)
    } else {
        eprintln!("fixture absent, skipping");
        None
    }
}

#[test]
fn opens_fixture() {
    let Some(p) = fixture() else { return };
    let lib = Library::open(&p).unwrap();
    assert_eq!(lib.items().len(), 91);
    assert_eq!(lib.mtime.all, Some(91));
    let ids = lib.folder_ids();
    assert_eq!(lib.all_folders().len(), 15);
    for item in lib.items() {
        for f in &item.folders {
            assert!(ids.contains(f.as_str()), "{} -> missing folder {f}", item.id);
        }
        assert!(item.original_path(lib.root()).is_file(), "{}", item.id);
    }
    assert!(lib.items().iter().any(|i| i.thumbnail_path(lib.root()).is_some()));
}

#[test]
fn round_trips_byte_identically() {
    let Some(p) = fixture() else { return };
    let lib = Library::open(&p).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("Out.library");
    lib.save_to(&out).unwrap();

    let mut files = vec!["metadata.json".to_string(), "mtime.json".into(), "tags.json".into(), "saved-filters.json".into()];
    for item in lib.items() {
        files.push(format!("images/{}.info/metadata.json", item.id));
        // Payload files are copied unchanged too.
        let orig = item.original_path(lib.root());
        let rel = orig.strip_prefix(lib.root()).unwrap().to_str().unwrap().to_string();
        files.push(rel);
    }
    for f in &files {
        assert_eq!(fs::read(p.join(f)).unwrap(), fs::read(out.join(f)).unwrap(), "{f}");
    }
    // Backups are carried over.
    let backup = fs::read_dir(p.join("backup")).unwrap().count();
    assert_eq!(fs::read_dir(out.join("backup")).unwrap().count(), backup);

    // The saved copy reopens to an equal model.
    let again = Library::open(&out).unwrap();
    assert_eq!(again.items(), lib.items());
    assert_eq!(again.metadata, lib.metadata);
}

#[test]
fn rejects_non_library() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(Library::open(tmp.path()).is_err());
}

#[test]
fn mtime_index_ids_are_items() {
    let Some(p) = fixture() else { return };
    let lib = Library::open(&p).unwrap();
    // mtime.json lists only 32 of the 91 items in this fixture; "all" is the item total.
    assert_eq!(lib.mtime.entries.len(), 32);
    for (id, _) in &lib.mtime.entries {
        assert!(lib.item(id).is_some(), "{id}");
    }
}

#[test]
fn refuses_to_save_onto_itself() {
    let Some(p) = fixture() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let copy = tmp.path().join("Copy.library");
    Library::open(&p).unwrap().save_to(&copy).unwrap();
    let before = fs::read(copy.join("metadata.json")).unwrap();

    let lib = Library::open(&copy).unwrap();
    assert!(matches!(lib.save_to(&copy), Err(falcon_library::Error::SaveOntoSelf(_))));
    assert_eq!(fs::read(copy.join("metadata.json")).unwrap(), before);
}

#[test]
fn creates_an_empty_library() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("New.library");
    let mut lib = Library::create(&path).unwrap();
    assert!(lib.items().is_empty());
    assert_eq!(fs::read_to_string(path.join("tags.json")).unwrap(), r#"{"historyTags":[],"starredTags":[]}"#);
    assert_eq!(fs::read_to_string(path.join("mtime.json")).unwrap(), r#"{"all":0}"#);
    lib.create_folder(None, "First").unwrap();
    assert_eq!(Library::open(&path).unwrap().all_folders().len(), 1);
    assert!(Library::create(&path).is_err(), "refuses an existing folder");
}
