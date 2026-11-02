//! Falcon's actions live in its own app data and never touch the
//! library's `actions.config.json`, which belongs to Eagle's AI Action
//! plugin. These tests fail if anything in the Actions path reads, writes,
//! replaces or leaves a temp file beside that file again.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use falcon_library::Library;
use falcon_library::actions::{ActionDef, Step, store_path};
use falcon_library::import::ImportOptions;

use super::ActionsState;

/// What Eagle's plugin might have written; falcon must leave it alone.
const EAGLE: &[u8] = br#"[{"id":"LX1","name":"Describe","prompt":"Write a caption","model":"m","shortcut":"Command + Shift + 1"}]"#;

fn stamp() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000)
}

/// A library at `<tmp>/<name>` with one imported image and Eagle's file.
fn library(tmp: &Path, name: &str) -> (Library, String) {
    let mut lib = Library::create(tmp.join(name)).unwrap();
    let src = tmp.join(format!("{name}.png"));
    image::RgbImage::from_pixel(8, 8, image::Rgb([1, 2, 3])).save(&src).unwrap();
    let id = lib.import_file(&src, &ImportOptions { palette: false, ..Default::default() }).unwrap();
    let eagle = lib.root().join("actions.config.json");
    fs::write(&eagle, EAGLE).unwrap();
    fs::File::options().write(true).open(&eagle).unwrap().set_modified(stamp()).unwrap();
    (lib, id)
}

fn names(dir: &Path) -> BTreeSet<String> {
    fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect()
}

/// The quit path's library half: flush everything, then back up.
fn quit_flush(lib: &mut Library, app: &Path) {
    lib.flush().unwrap();
    falcon_library::backup::backup_library_with(lib.root(), Some(&app.join("eagle-backup"))).unwrap();
}

fn assert_untouched(root: &Path, before: &BTreeSet<String>) {
    let eagle = root.join("actions.config.json");
    assert_eq!(fs::read(&eagle).unwrap(), EAGLE, "the bytes of {}", eagle.display());
    assert_eq!(fs::metadata(&eagle).unwrap().modified().unwrap(), stamp(), "the mtime of {}", eagle.display());
    let after = names(root);
    assert_eq!(&after, before, "nothing new beside the library's files");
    assert!(!after.iter().any(|n| n.starts_with(".tmp") || (n.contains("action") && n != "actions.config.json")), "{after:?}");
}

fn def(name: &str, steps: Vec<Step>) -> ActionDef {
    ActionDef { id: falcon_library::ids::new_id(), name: name.into(), shortcut: None, steps, runnable: true }
}

#[test]
fn actions_never_touch_the_librarys_actions_config() {
    let tmp = tempfile::tempdir().unwrap();
    let app = tmp.path().join("app");
    let dir = app.join("actions");
    let (mut a, item) = library(tmp.path(), "A.library");
    let (mut b, _) = library(tmp.path(), "B.library");
    let folder = a.create_folder(None, "Filed").unwrap();
    // Let the library write whatever it writes on its own first.
    quit_flush(&mut a, &app);
    quit_flush(&mut b, &app);
    let (before_a, before_b) = (names(a.root()), names(b.root()));

    let mut s = ActionsState::new(Some(dir.clone()));
    s.follow(Some(a.root()));
    assert!(s.defs().is_empty(), "Eagle's entries are not imported");
    assert!(s.broken.is_none());

    // Create, edit, give a key, duplicate, reorder.
    let mut file_it = def("File it", vec![Step::AddTags { tags: vec!["ref".into()] }, Step::AddFolders { folders: vec![folder.clone()] }, Step::Rating { rating: 4 }]);
    s.commit(|f| f.put(&file_it)).unwrap();
    file_it.name = "File as reference".into();
    s.commit(|f| f.put(&file_it)).unwrap();
    s.commit(|f| f.set_shortcut(&file_it.id, Some("Command + Shift + 2".into()))).unwrap();
    let mut copy = None;
    s.commit(|f| copy = f.duplicate(&file_it.id, "Copy")).unwrap();
    let copy = copy.unwrap();
    s.commit(|f| f.reorder(&copy, 0)).unwrap();

    // Run it, as the reducer does.
    let run = s.run_of(&file_it.id, vec![item.clone()]).unwrap();
    a.begin_transaction("Run");
    let report = a.run_steps(&run.ids, &run.steps).unwrap();
    a.end_transaction();
    assert_eq!(report.changed, 1);
    assert_eq!(a.item(&item).unwrap().folders, vec![folder]);

    // Delete one.
    s.commit(|f| f.remove(&copy)).unwrap();

    // Switch libraries and keep an action there too.
    s.follow(Some(b.root()));
    assert!(s.defs().is_empty(), "B doesn't see A's actions");
    let rate = def("Rate one", vec![Step::Rating { rating: 1 }]);
    s.commit(|f| f.put(&rate)).unwrap();
    s.follow(Some(a.root()));
    assert_eq!(s.defs().iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), ["File as reference"], "A doesn't see B's");
    s.follow(None);

    quit_flush(&mut a, &app);
    quit_flush(&mut b, &app);
    assert_untouched(a.root(), &before_a);
    assert_untouched(b.root(), &before_b);

    // Everything landed in the app data folder, one file per library.
    let (store_a, store_b) = (store_path(&dir, a.root()), store_path(&dir, b.root()));
    assert_ne!(store_a, store_b);
    let file_names = |p: &PathBuf| p.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(names(&dir), BTreeSet::from([file_names(&store_a), file_names(&store_b)]), "no temp file left either");
    let text = fs::read_to_string(&store_a).unwrap();
    assert!(text.starts_with(&format!(r#"[{{"id":"{}","name":"File as reference","shortcut":"Command + Shift + 2","steps":[{{"type":"addTags""#, file_it.id)), "{text}");

    // Reopened (a new launch), the same library sees its actions.
    let mut again = ActionsState::new(Some(dir.clone()));
    again.follow(Some(a.root()));
    assert_eq!(again.defs().len(), 1);
    assert_eq!(again.defs()[0].shortcut.as_deref(), Some("Command + Shift + 2"));
    assert_untouched(a.root(), &before_a);
}

#[test]
fn a_library_without_eagles_file_gets_none() {
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = Library::create(tmp.path().join("C.library")).unwrap();
    lib.flush().unwrap();
    let before = names(lib.root());
    let mut s = ActionsState::new(Some(tmp.path().join("app/actions")));
    s.follow(Some(lib.root()));
    s.commit(|f| f.put(&def("Rate", vec![Step::Rating { rating: 2 }]))).unwrap();
    s.follow(None);
    lib.flush().unwrap();
    assert_eq!(names(lib.root()), before);
    assert!(!lib.root().join("actions.config.json").exists(), "never created");
}

#[test]
fn a_failed_write_keeps_the_list_and_leaves_no_partial_file() {
    let (d, mut s) = super::state_with("[]");
    let store = s.store().unwrap().to_path_buf();
    // A folder where the file goes makes the rename fail.
    fs::remove_file(&store).unwrap();
    fs::create_dir(&store).unwrap();
    assert!(s.commit(|f| f.put(&def("Rate", vec![Step::Rating { rating: 2 }]))).is_err());
    assert!(s.defs().is_empty(), "the edit isn't kept when it couldn't be saved");
    assert_eq!(names(store.parent().unwrap()).len(), 1, "only the blocking folder");
    assert!(!d.path().join("T.library/actions.config.json").exists());
}
