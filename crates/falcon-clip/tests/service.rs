//! The worker against temporary libraries, with a tiny stand-in embedder: a
//! picture's vector is its average colour, and the words "red", "green" and
//! "blue" point along their colour. No network and no weights.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use falcon_clip::service::{Options, Shared};
use falcon_clip::{Embedder, Phase, Query, SearchError, Service, Status};
use falcon_library::Library;
use falcon_library::import::ImportOptions;
use image::{DynamicImage, Rgb, RgbImage};

const WAIT: Duration = Duration::from_secs(20);

struct Colours {
    pictures: Arc<AtomicUsize>,
}

fn unit(v: [f32; 3]) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
    v.iter().map(|x| x / n).collect()
}

impl Embedder for Colours {
    fn embed_images(&mut self, images: &[DynamicImage]) -> Result<Vec<Vec<f32>>, String> {
        self.pictures.fetch_add(images.len(), Ordering::SeqCst);
        Ok(images
            .iter()
            .map(|image| {
                let rgb = image.to_rgb8();
                let mut sum = [0.0; 3];
                for p in rgb.pixels() {
                    for c in 0..3 {
                        sum[c] += f32::from(p[c]);
                    }
                }
                unit(sum)
            })
            .collect())
    }

    fn embed_text(&mut self, text: &str) -> Result<Vec<f32>, String> {
        Ok(match text {
            "red" => vec![1.0, 0.0, 0.0],
            "green" => vec![0.0, 1.0, 0.0],
            "blue" => vec![0.0, 0.0, 1.0],
            _ => return Err("unknown word".to_string()),
        })
    }

    fn dim(&self) -> usize {
        3
    }

    fn device(&self) -> String {
        "Test".to_string()
    }
}

struct Fixture {
    dir: tempfile::TempDir,
    library: Shared,
    pictures: Arc<AtomicUsize>,
    installed: Arc<Mutex<bool>>,
    fail_load: Arc<Mutex<bool>>,
}

impl Fixture {
    fn new() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::create(dir.path().join("T.library")).unwrap();
        Fixture {
            dir,
            library: Arc::new(Mutex::new(library)),
            pictures: Arc::default(),
            installed: Arc::new(Mutex::new(true)),
            fail_load: Arc::new(Mutex::new(false)),
        }
    }

    fn index_dir(&self) -> PathBuf {
        self.dir.path().join("indexes")
    }

    fn service(&self) -> Service {
        let (pictures, installed, fail_load) = (self.pictures.clone(), self.installed.clone(), self.fail_load.clone());
        Service::start(Options {
            index_dir: self.index_dir(),
            model_id: "test-model".to_string(),
            dim: 3,
            throttle: false,
            is_installed: Box::new(move || *installed.lock().unwrap()),
            loader: Box::new(move || {
                if *fail_load.lock().unwrap() {
                    return Err("no model".to_string());
                }
                Ok(Box::new(Colours { pictures: pictures.clone() }))
            }),
            notify: Box::new(|| {}),
        })
    }

    fn add(&self, name: &str, colour: [u8; 3]) -> String {
        let path = self.dir.path().join(format!("{name}.png"));
        RgbImage::from_pixel(32, 32, Rgb(colour)).save(&path).unwrap();
        self.library.lock().unwrap().import_file(&path, &ImportOptions::default()).unwrap()
    }

    fn root(&self) -> PathBuf {
        self.library.lock().unwrap().root().to_path_buf()
    }

    /// The file that stands for the item: small pictures have no thumbnail.
    fn picture(&self, id: &str) -> PathBuf {
        let root = self.root();
        let lib = self.library.lock().unwrap();
        let item = lib.item(id).unwrap();
        item.thumbnail_path(&root).unwrap_or_else(|| lib.resolved_source_path(id).unwrap())
    }

    fn set_item(&self, id: &str, change: impl FnOnce(&mut falcon_library::model::Item)) {
        let mut lib = self.library.lock().unwrap();
        let mut item = lib.item(id).unwrap().clone();
        change(&mut item);
        lib.update_item(item).unwrap();
    }
}

fn wait_for(service: &Service, what: &str, ok: impl Fn(&Status) -> bool) -> Status {
    let start = Instant::now();
    loop {
        let status = service.status();
        if ok(&status) {
            return status;
        }
        assert!(start.elapsed() < WAIT, "timed out waiting for {what}: {status:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn ready(service: &Service, total: usize) -> Status {
    wait_for(service, "the index", |s| s.phase == Phase::Ready && s.total == total && s.done == total)
}

fn ids(service: &Service, word: &str) -> Vec<String> {
    service.search_blocking(Query::Text(word.to_string()), 10, WAIT).unwrap().into_iter().map(|h| h.id).collect()
}

/// Every file under a folder with its contents, to show nothing changed.
fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            if entry.path().is_dir() {
                stack.push(entry.path());
            } else {
                out.push((entry.path(), std::fs::read(entry.path()).unwrap()));
            }
        }
    }
    out.sort();
    out
}

/// A saved filter's rules run through the worker, read from the JSON the
/// library holds.
#[test]
fn a_saved_filters_rules_search_like_the_box_does() {
    use falcon_library::model::SavedFilter;
    let f = Fixture::new();
    let red = f.add("red", [250, 10, 10]);
    let green = f.add("green", [10, 250, 10]);
    let service = f.service();
    service.set_library(Some(f.library.clone()));
    service.set_enabled(true);
    ready(&service, 2);

    let saved = |rule: serde_json::Value| SavedFilter { name: "n".into(), rule, extra: Default::default() }.to_filter().ai;
    let text = saved(serde_json::json!({"semantic": {"value": "green"}}));
    let hits = service.search_blocking(Query::Text(text.semantic_text().unwrap().to_string()), 10, WAIT).unwrap();
    assert_eq!(hits.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(), [green.as_str()]);

    let image = saved(serde_json::json!({"image": {"itemId": red}}));
    let hits = service.search_blocking(Query::Item(image.image_item().unwrap().to_string()), 10, WAIT).unwrap();
    assert_eq!(hits[0].id, red);

    let gone = saved(serde_json::json!({"image": {"itemId": "NOPE"}}));
    assert_eq!(service.search_blocking(Query::Item(gone.image_item().unwrap().to_string()), 10, WAIT).unwrap_err(), SearchError::NoItem);
}

/// The status doesn't call a library ready before it has been looked at, and
/// says which library it means.
#[test]
fn ready_means_this_library_has_been_read() {
    let f = Fixture::new();
    let root = f.root();
    assert!(!Status { phase: Phase::Ready, ..Default::default() }.index_ready(&root), "no library");
    f.add("red", [250, 10, 10]);
    let service = f.service();
    service.set_enabled(true);
    service.set_library(Some(f.library.clone()));
    // A ready status is only ever one that has read the picture.
    let status = wait_for(&service, "ready", |s| s.index_ready(&root));
    assert_eq!((status.done, status.total), (1, 1));
    assert!(!status.index_ready(&root.with_extension("other")), "another library");

    // Paused with pictures left isn't ready.
    service.set_paused(true);
    f.add("green", [10, 250, 10]);
    let status = wait_for(&service, "the new picture", |s| s.phase == Phase::Paused);
    assert!(!status.index_ready(&root));
    service.set_paused(false);
    ready(&service, 2);
}

#[test]
fn nothing_runs_or_answers_until_it_is_turned_on_and_installed() {
    let f = Fixture::new();
    f.add("red", [250, 10, 10]);
    *f.installed.lock().unwrap() = false;
    let service = f.service();
    service.set_library(Some(f.library.clone()));
    assert_eq!(service.search_blocking(Query::Text("red".into()), 5, WAIT), Err(SearchError::NotReady));
    assert_eq!(service.status().phase, Phase::Off);
    service.set_enabled(true);
    wait_for(&service, "no model", |s| s.phase == Phase::NoModel);
    assert_eq!(service.search_blocking(Query::Text("red".into()), 5, WAIT), Err(SearchError::NotReady));
    assert_eq!(f.pictures.load(Ordering::SeqCst), 0, "nothing is indexed without the model");
    assert!(!f.index_dir().exists());
    *f.installed.lock().unwrap() = true;
    service.model_changed();
    ready(&service, 1);
    assert_eq!(f.pictures.load(Ordering::SeqCst), 1);
}

#[test]
fn text_search_finds_by_meaning_and_similar_finds_by_picture() {
    let f = Fixture::new();
    let red = f.add("red", [250, 10, 10]);
    let green = f.add("green", [10, 250, 10]);
    let blue = f.add("blue", [10, 10, 250]);
    let service = f.service();
    service.set_library(Some(f.library.clone()));
    service.set_enabled(true);
    ready(&service, 3);
    assert_eq!(ids(&service, "red").first(), Some(&red));
    assert_eq!(ids(&service, "green").first(), Some(&green));
    assert_eq!(ids(&service, "blue").first(), Some(&blue));
    let hits = service.search_blocking(Query::Item(green.clone()), 10, WAIT).unwrap();
    assert_eq!(hits[0].id, green);
    assert!(hits[0].score > 0.99);
    let picture = RgbImage::from_pixel(8, 8, Rgb([0, 0, 255]));
    let hits = service.search_blocking(Query::Image(Box::new(DynamicImage::ImageRgb8(picture))), 10, WAIT).unwrap();
    assert_eq!(hits[0].id, blue);
    let file = f.dir.path().join("blue.png");
    assert_eq!(service.search_blocking(Query::ImageFile(file), 10, WAIT).unwrap()[0].id, blue);
    let not_a_picture = f.dir.path().join("note.txt");
    std::fs::write(&not_a_picture, b"hello").unwrap();
    assert!(matches!(service.search_blocking(Query::ImageFile(not_a_picture), 10, WAIT), Err(SearchError::BadImage(_))));
    assert_eq!(service.search_blocking(Query::Item("nope".into()), 10, WAIT), Err(SearchError::NoItem));
    assert_eq!(service.search_blocking(Query::Text("  ".into()), 10, WAIT), Ok(Vec::new()));
}

#[test]
fn indexing_resumes_after_a_restart_without_reading_pictures_again() {
    let f = Fixture::new();
    let red = f.add("red", [250, 10, 10]);
    f.add("green", [10, 250, 10]);
    let service = f.service();
    service.set_library(Some(f.library.clone()));
    service.set_enabled(true);
    ready(&service, 2);
    assert_eq!(f.pictures.load(Ordering::SeqCst), 2);
    drop(service);
    let blue = f.add("blue", [10, 10, 250]);
    let service = f.service();
    service.set_library(Some(f.library.clone()));
    service.set_enabled(true);
    ready(&service, 3);
    assert_eq!(f.pictures.load(Ordering::SeqCst), 3, "only the new item is read");
    assert_eq!(ids(&service, "red").first(), Some(&red));
    assert_eq!(ids(&service, "blue").first(), Some(&blue));
}

#[test]
fn the_index_follows_import_edit_trash_restore_and_delete() {
    let f = Fixture::new();
    let red = f.add("red", [250, 10, 10]);
    let service = f.service();
    service.set_library(Some(f.library.clone()));
    service.set_enabled(true);
    ready(&service, 1);

    // Import.
    let green = f.add("green", [10, 250, 10]);
    ready(&service, 2);
    assert_eq!(ids(&service, "green").first(), Some(&green));

    // An edit that changes the picture: red becomes blue.
    RgbImage::from_pixel(32, 32, Rgb([10, 10, 250])).save(f.picture(&red)).unwrap();
    f.set_item(&red, |i| i.last_modified += 1);
    let start = Instant::now();
    while ids(&service, "blue").first() != Some(&red) {
        assert!(start.elapsed() < WAIT, "the edit wasn't picked up");
        std::thread::sleep(Duration::from_millis(50));
    }

    // An edit that leaves the picture alone isn't read again.
    let before = f.pictures.load(Ordering::SeqCst);
    f.set_item(&red, |i| i.tags.push("note".to_string()));
    std::thread::sleep(Duration::from_millis(2500));
    assert_eq!(f.pictures.load(Ordering::SeqCst), before);

    // The Trash hides an item from results and from the count, and it comes
    // back without being read again.
    f.set_item(&green, |i| i.is_deleted = true);
    ready(&service, 1);
    assert!(!ids(&service, "green").contains(&green));
    f.set_item(&green, |i| i.is_deleted = false);
    ready(&service, 2);
    assert_eq!(ids(&service, "green").first(), Some(&green));
    assert_eq!(f.pictures.load(Ordering::SeqCst), before, "restored without reading again");

    // Deleting for good forgets it.
    f.library.lock().unwrap().delete_permanently(std::slice::from_ref(&green)).unwrap();
    ready(&service, 1);
    assert!(!ids(&service, "green").contains(&green));
    assert_eq!(service.search_blocking(Query::Item(green), 10, WAIT), Err(SearchError::NoItem));
}

#[test]
fn nothing_is_written_inside_the_library() {
    let f = Fixture::new();
    f.add("red", [250, 10, 10]);
    f.add("green", [10, 250, 10]);
    let before = snapshot(&f.root());
    let service = f.service();
    service.set_library(Some(f.library.clone()));
    service.set_enabled(true);
    ready(&service, 2);
    ids(&service, "red");
    drop(service);
    assert_eq!(snapshot(&f.root()), before, "the library folder is byte-identical");
    assert!(std::fs::read_dir(f.index_dir()).unwrap().count() > 0, "the index lives in app data");
}

#[test]
fn pausing_stops_indexing_and_resuming_finishes_it() {
    let f = Fixture::new();
    for i in 0..20 {
        f.add(&format!("p{i}"), [(i * 10) as u8, 100, 100]);
    }
    let service = f.service();
    service.set_paused(true);
    service.set_library(Some(f.library.clone()));
    service.set_enabled(true);
    let status = wait_for(&service, "the pause", |s| s.phase == Phase::Paused);
    assert_eq!((status.total, status.done), (20, 0));
    assert_eq!(f.pictures.load(Ordering::SeqCst), 0);
    service.set_paused(false);
    ready(&service, 20);
    assert_eq!(f.pictures.load(Ordering::SeqCst), 20);
}

#[test]
fn an_item_without_a_readable_picture_is_skipped_once() {
    let f = Fixture::new();
    let id = f.add("broken", [1, 2, 3]);
    std::fs::write(f.picture(&id), b"not a picture").unwrap();
    let service = f.service();
    service.set_library(Some(f.library.clone()));
    service.set_enabled(true);
    let status = ready(&service, 1);
    assert_eq!(status.skipped, 1);
    assert_eq!(f.pictures.load(Ordering::SeqCst), 0);
    drop(service);
    let service = f.service();
    service.set_library(Some(f.library.clone()));
    service.set_enabled(true);
    let status = ready(&service, 1);
    assert_eq!(status.skipped, 1, "still skipped, without trying again");
}

#[test]
fn a_model_that_will_not_load_says_so_and_recovers_when_told() {
    let f = Fixture::new();
    f.add("red", [250, 10, 10]);
    *f.fail_load.lock().unwrap() = true;
    let service = f.service();
    service.set_library(Some(f.library.clone()));
    service.set_enabled(true);
    let status = wait_for(&service, "the failure", |s| s.phase == Phase::Failed);
    assert_eq!(status.error.as_deref(), Some("no model"));
    *f.fail_load.lock().unwrap() = false;
    service.model_changed();
    ready(&service, 1);
}

#[test]
fn rebuilding_reads_everything_again_and_forgetting_deletes_the_files() {
    let f = Fixture::new();
    f.add("red", [250, 10, 10]);
    let service = f.service();
    service.set_library(Some(f.library.clone()));
    service.set_enabled(true);
    ready(&service, 1);
    service.rebuild();
    let start = Instant::now();
    while f.pictures.load(Ordering::SeqCst) < 2 {
        assert!(start.elapsed() < WAIT, "not read again");
        std::thread::sleep(Duration::from_millis(10));
    }
    service.set_enabled(false);
    service.forget_indexes();
    wait_for(&service, "off", |s| s.phase == Phase::Off);
    drop(service);
    assert_eq!(std::fs::read_dir(f.index_dir()).unwrap().count(), 0);
}

#[test]
fn switching_libraries_keeps_each_index_apart() {
    let f = Fixture::new();
    let red = f.add("red", [250, 10, 10]);
    let other = Fixture::new();
    let blue = other.add("blue", [10, 10, 250]);
    let service = f.service();
    service.set_library(Some(f.library.clone()));
    service.set_enabled(true);
    ready(&service, 1);
    service.set_library(Some(other.library.clone()));
    let start = Instant::now();
    while ids(&service, "blue") != [blue.clone()] {
        assert!(start.elapsed() < WAIT, "the other library wasn't indexed");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!ids(&service, "red").contains(&red));
    service.set_library(None);
    assert_eq!(service.search_blocking(Query::Text("red".into()), 5, WAIT), Err(SearchError::NoLibrary));
}

fn add_linked(f: &Fixture, name: &str, size: u32, colour: [u8; 3]) -> (String, PathBuf) {
    let path = f.dir.path().join("external").join(format!("{name}.png"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    RgbImage::from_pixel(size, size, Rgb(colour)).save(&path).unwrap();
    let mut lib = f.library.lock().unwrap();
    lib.set_storage_mode(falcon_library::storage_mode::StorageMode::Link).unwrap();
    let id = lib.import_file(&path, &ImportOptions { palette: false, ..Default::default() }).unwrap();
    (id, path)
}

fn reopen_index(f: &Fixture) -> falcon_clip::store::Index {
    let root = f.root();
    falcon_clip::store::Index::open(&f.index_dir(), &root, "test-model", 3).unwrap()
}

fn mutate_sidecar_offline(f: &Fixture, id: &str) -> falcon_library::linked::LinkRecord {
    let root = f.root();
    let lib = f.library.lock().unwrap();
    let sidecar = lib.item(id).unwrap().link_record_path(&root);
    let orig: falcon_library::linked::LinkRecord = serde_json::from_slice(&std::fs::read(&sidecar).unwrap()).unwrap();
    let mut offline = orig.clone();
    offline.volume.mount_point = Some(f.dir.path().join("DisconnectedDrive"));
    offline.volume.volume_uuid = Some("00000000-0000-0000-0000-DEAD00000099".into());
    std::fs::write(&sidecar, serde_json::to_vec(&offline).unwrap()).unwrap();
    orig
}

fn restore_sidecar(f: &Fixture, id: &str, orig: &falcon_library::linked::LinkRecord) {
    let root = f.root();
    let lib = f.library.lock().unwrap();
    let sidecar = lib.item(id).unwrap().link_record_path(&root);
    std::fs::write(&sidecar, serde_json::to_vec(orig).unwrap()).unwrap();
}

#[test]
fn ai_01_linked_available_asset_indexes_and_searches() {
    let f = Fixture::new();
    let (red_id, _src) = add_linked(&f, "linked_red", 32, [250, 10, 10]);
    let (blue_id, _src2) = add_linked(&f, "linked_blue", 1200, [10, 10, 250]);

    let service = f.service();
    service.set_library(Some(f.library.clone()));
    service.set_enabled(true);
    let status = ready(&service, 2);
    assert_eq!(status.skipped, 0);
    assert_eq!(ids(&service, "red"), vec![red_id.clone()]);
    assert_eq!(ids(&service, "blue"), vec![blue_id.clone()]);
}

#[test]
fn ai_02_linked_offline_asset_preserves_existing_embedding() {
    let f = Fixture::new();
    let (id, _src) = add_linked(&f, "offline_red", 32, [250, 10, 10]);
    let service = f.service();
    service.set_library(Some(f.library.clone()));
    service.set_enabled(true);
    ready(&service, 1);

    mutate_sidecar_offline(&f, &id);
    service.refresh();
    let status = wait_for(&service, "offline handled", |s| s.phase == Phase::Ready && s.unavailable == 1);
    assert_eq!(status.skipped, 0);
    assert!(ids(&service, "red").contains(&id), "existing vector remains searchable while Offline");
    drop(service);

    let idx = reopen_index(&f);
    assert!(idx.vector(&id).is_some());
    assert!(!idx.is_skipped(&id));
}

#[test]
fn ai_03_linked_missing_asset_preserves_existing_embedding() {
    let f = Fixture::new();
    let (id, src) = add_linked(&f, "missing_green", 32, [10, 250, 10]);
    let service = f.service();
    service.set_library(Some(f.library.clone()));
    service.set_enabled(true);
    ready(&service, 1);

    std::fs::remove_file(&src).unwrap();
    service.refresh();
    let status = wait_for(&service, "missing handled", |s| s.phase == Phase::Ready && s.unavailable == 1);
    assert_eq!(status.skipped, 0);
    assert!(ids(&service, "green").contains(&id), "existing vector preserved when Linked source is Missing");
    drop(service);

    let idx = reopen_index(&f);
    assert!(idx.vector(&id).is_some());
    assert!(!idx.is_skipped(&id));
}

#[test]
fn ai_04_linked_invalid_reference_preserves_existing_embedding() {
    let f = Fixture::new();
    let (id, _src) = add_linked(&f, "invalid_blue", 32, [10, 10, 250]);
    let service = f.service();
    service.set_library(Some(f.library.clone()));
    service.set_enabled(true);
    ready(&service, 1);

    {
        let root = f.root();
        let lib = f.library.lock().unwrap();
        let sidecar = lib.item(&id).unwrap().link_record_path(&root);
        std::fs::write(&sidecar, b"{broken json").unwrap();
    }
    service.refresh();
    let status = wait_for(&service, "invalid handled", |s| s.phase == Phase::Ready && s.unavailable == 1);
    assert_eq!(status.skipped, 0);
    assert!(ids(&service, "blue").contains(&id));
    drop(service);

    let idx = reopen_index(&f);
    assert!(idx.vector(&id).is_some());
    assert!(!idx.is_skipped(&id));
}

#[cfg(unix)]
#[test]
fn ai_05_linked_permission_denied_preserves_existing_embedding() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let (id, src) = add_linked(&f, "perm_red", 32, [250, 10, 10]);
    let service = f.service();
    service.set_library(Some(f.library.clone()));
    service.set_enabled(true);
    ready(&service, 1);

    std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::File::open(&src).is_ok() {
        // Running as root bypasses mode bits; restore cleanly and skip.
        std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o644)).unwrap();
        return;
    }

    service.refresh();
    let status = wait_for(&service, "permission denied handled", |s| s.phase == Phase::Ready && s.unavailable == 1);
    assert_eq!(status.skipped, 0);
    assert!(ids(&service, "red").contains(&id));
    drop(service);

    let idx = reopen_index(&f);
    assert!(idx.vector(&id).is_some());
    assert!(!idx.is_skipped(&id));
    std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o644)).unwrap();
}

#[test]
fn ai_06_failed_or_aborted_embedder_refresh_preserves_existing_embedding_as_stale() {
    struct ToggleEmbedder {
        fail: Arc<std::sync::atomic::AtomicBool>,
        pictures: Arc<AtomicUsize>,
    }
    impl Embedder for ToggleEmbedder {
        fn embed_images(&mut self, images: &[DynamicImage]) -> Result<Vec<Vec<f32>>, String> {
            if self.fail.load(Ordering::SeqCst) {
                return Err("transient model/io abort".into());
            }
            Colours { pictures: self.pictures.clone() }.embed_images(images)
        }
        fn embed_text(&mut self, text: &str) -> Result<Vec<f32>, String> {
            Colours { pictures: self.pictures.clone() }.embed_text(text)
        }
        fn dim(&self) -> usize {
            3
        }
        fn device(&self) -> String {
            "Test".into()
        }
    }

    let f = Fixture::new();
    let (id, src) = add_linked(&f, "abort_red", 32, [250, 10, 10]);
    let fail = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let fail_ref = fail.clone();
    let pictures = f.pictures.clone();
    let service = Service::start(Options {
        index_dir: f.index_dir(),
        model_id: "test-model".to_string(),
        dim: 3,
        throttle: false,
        is_installed: Box::new(|| true),
        loader: Box::new(move || Ok(Box::new(ToggleEmbedder { fail: fail_ref.clone(), pictures: pictures.clone() }))),
        notify: Box::new(|| {}),
    });
    service.set_library(Some(f.library.clone()));
    service.set_enabled(true);
    ready(&service, 1);

    // Trigger refresh with changed file while embedder fails/aborts.
    fail.store(true, Ordering::SeqCst);
    RgbImage::from_pixel(36, 36, Rgb([10, 10, 250])).save(&src).unwrap();
    service.refresh();
    let status = wait_for(&service, "failed refresh", |s| s.phase == Phase::Failed);
    assert!(status.stale >= 1, "last valid embedding retained as stale: {status:?}");
    assert_eq!(status.skipped, 0);
    // Re-enabling / recovering keeps old vector in index without having dropped it.
    fail.store(false, Ordering::SeqCst);
    drop(service);
    let idx = reopen_index(&f);
    assert!(idx.vector(&id).is_some());
    assert!(!idx.is_skipped(&id));
}

#[test]
fn ai_07_decode_failed_refresh_keeps_last_valid_vector_as_stale_until_fixed() {
    let f = Fixture::new();
    let (id, src) = add_linked(&f, "stale_swap", 32, [250, 10, 10]);
    let service = f.service();
    service.set_library(Some(f.library.clone()));
    service.set_enabled(true);
    ready(&service, 1);
    assert!(ids(&service, "red").contains(&id));

    // Overwrite source with corrupt bytes -> DecodeFailed on refresh, yet old valid embedding stays searchable & stale!
    std::fs::write(&src, b"not a valid png at all").unwrap();
    service.refresh();
    let status = wait_for(&service, "stale after decode failure", |s| s.phase == Phase::Ready && s.stale == 1);
    assert_eq!(status.skipped, 0, "must NOT mark existing valid embedding as SKIPPED");
    assert!(ids(&service, "red").contains(&id), "last valid vector still answers searches while stale");

    // Replace with valid green image -> successful refresh clears stale and updates vector.
    RgbImage::from_pixel(40, 40, Rgb([10, 250, 10])).save(&src).unwrap();
    service.refresh();
    let status = wait_for(&service, "recovered from stale", |s| s.phase == Phase::Ready && s.stale == 0);
    assert_eq!(status.skipped, 0);
    assert!(ids(&service, "green").contains(&id));
    assert!(!ids(&service, "red").contains(&id));
}

#[test]
fn ai_08_reconnect_without_changes_does_not_force_full_reindex() {
    let f = Fixture::new();
    let (id_a, _src_a) = add_linked(&f, "a", 32, [250, 10, 10]);
    let (_id_b, _src_b) = add_linked(&f, "b", 32, [10, 250, 10]);
    let service = f.service();
    service.set_library(Some(f.library.clone()));
    service.set_enabled(true);
    ready(&service, 2);
    assert_eq!(f.pictures.load(Ordering::SeqCst), 2);

    let orig = mutate_sidecar_offline(&f, &id_a);
    service.refresh();
    wait_for(&service, "a offline", |s| s.phase == Phase::Ready && s.unavailable == 1);

    restore_sidecar(&f, &id_a, &orig);
    service.refresh();
    wait_for(&service, "a back online", |s| s.phase == Phase::Ready && s.unavailable == 0);
    assert_eq!(f.pictures.load(Ordering::SeqCst), 2, "unchanged source reconnecting must NOT reindex");
}

#[test]
fn ai_09_reconnect_with_modified_content_reindexes_only_changed_item() {
    let f = Fixture::new();
    let (id_a, src_a) = add_linked(&f, "a", 32, [250, 10, 10]);
    let (id_b, _src_b) = add_linked(&f, "b", 32, [10, 250, 10]);
    let service = f.service();
    service.set_library(Some(f.library.clone()));
    service.set_enabled(true);
    ready(&service, 2);
    assert_eq!(f.pictures.load(Ordering::SeqCst), 2);

    let orig = mutate_sidecar_offline(&f, &id_a);
    service.refresh();
    wait_for(&service, "a offline", |s| s.phase == Phase::Ready && s.unavailable == 1);

    // Modify only A externally (different size & color) while offline, then reconnect.
    RgbImage::from_pixel(44, 44, Rgb([10, 10, 250])).save(&src_a).unwrap();
    restore_sidecar(&f, &id_a, &orig);
    service.refresh();
    wait_for(&service, "only a reindexed", |s| s.phase == Phase::Ready && s.unavailable == 0 && f.pictures.load(Ordering::SeqCst) == 3);
    assert_eq!(f.pictures.load(Ordering::SeqCst), 3, "ONLY the changed item reindexed (B untouched)");
    assert!(ids(&service, "blue").contains(&id_a));
    assert!(ids(&service, "green").contains(&id_b));
}

#[test]
fn ai_10_available_unsupported_format_is_marked_skipped() {
    let f = Fixture::new();
    let bin = f.dir.path().join("external").join("raw.bin");
    std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
    std::fs::write(&bin, b"binary payload with unsupported format").unwrap();
    let id = {
        let mut lib = f.library.lock().unwrap();
        lib.set_storage_mode(falcon_library::storage_mode::StorageMode::Link).unwrap();
        lib.import_file(&bin, &ImportOptions { palette: false, ..Default::default() }).unwrap()
    };

    let service = f.service();
    service.set_library(Some(f.library.clone()));
    service.set_enabled(true);
    let status = ready(&service, 1);
    assert_eq!(status.skipped, 1);
    drop(service);

    let idx = reopen_index(&f);
    assert!(idx.is_skipped(&id));
    assert!(idx.vector(&id).is_none());
}

#[test]
fn ai_11_available_corrupt_new_file_decode_failed_is_marked_skipped() {
    let f = Fixture::new();
    let bad_png = f.dir.path().join("external").join("corrupt.png");
    std::fs::create_dir_all(bad_png.parent().unwrap()).unwrap();
    std::fs::write(&bad_png, b"corrupted png bytes").unwrap();
    let bad_id = {
        let mut lib = f.library.lock().unwrap();
        lib.set_storage_mode(falcon_library::storage_mode::StorageMode::Link).unwrap();
        lib.import_file(&bad_png, &ImportOptions { palette: false, ..Default::default() }).unwrap()
    };
    let (ok_id, _) = add_linked(&f, "ok_green", 32, [10, 250, 10]);

    let service = f.service();
    service.set_library(Some(f.library.clone()));
    service.set_enabled(true);
    let status = ready(&service, 2);
    assert_eq!(status.skipped, 1);
    assert!(ids(&service, "green").contains(&ok_id));
    drop(service);

    let idx = reopen_index(&f);
    assert!(idx.is_skipped(&bad_id));
    assert!(idx.vector(&ok_id).is_some());
}

#[test]
fn ai_12_explicit_delete_removes_vector_while_interrupted_ops_create_no_orphans() {
    let f = Fixture::new();
    let (id_keep, src_keep) = add_linked(&f, "keep_red", 64, [240, 20, 20]);
    let (id_del, src_del) = add_linked(&f, "del_green", 64, [20, 240, 20]);

    let service = f.service();
    service.set_library(Some(f.library.clone()));
    service.set_enabled(true);
    ready(&service, 2);

    // Fault-injected interrupted import & duplicate must not create orphan or cross-asset vectors.
    {
        let _guard = falcon_library::fault::install(|fp| {
            if matches!(
                fp,
                falcon_library::fault::FaultPoint::ImportBeforePublishRename
                    | falcon_library::fault::FaultPoint::DuplicateBeforePublishRename
            ) {
                Err(std::io::Error::other("simulated crash"))
            } else {
                Ok(())
            }
        });
        let mut lib = f.library.lock().unwrap();
        assert!(lib.import_file(&src_keep, &ImportOptions { palette: false, ..Default::default() }).is_err());
        assert!(lib.duplicate_item(&id_keep).is_err());
    }

    // Explicit catalog deletion removes only the deleted item's embedding without touching external source.
    {
        let mut lib = f.library.lock().unwrap();
        lib.delete_permanently(std::slice::from_ref(&id_del)).unwrap();
    }
    service.refresh();
    let status = ready(&service, 1);
    assert_eq!(status.done, 1);
    assert!(src_keep.is_file() && src_del.is_file(), "external files untouched");
    drop(service);

    let idx = reopen_index(&f);
    assert!(idx.vector(&id_keep).is_some());
    assert!(idx.vector(&id_del).is_none());
    assert_eq!(idx.known_ids(), vec![id_keep]);
}

#[test]
fn small_and_large_svgs_index_transparently_and_malformed_svgs_do_not_stall_queue() {
    let f = Fixture::new();
    let import_svg = |name: &str, body: &[u8]| -> String {
        let p = f.dir.path().join(format!("{name}.svg"));
        std::fs::write(&p, body).unwrap();
        f.library.lock().unwrap().import_file(&p, &ImportOptions { palette: false, ..Default::default() }).unwrap()
    };

    // Small transparent SVG (< 100 KB): thumbnail is removed by plan_svg (`remove_thumbnail = true`).
    let small_green = br##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="120"><rect x="20" y="20" width="80" height="80" fill="#00ff00" fill-opacity="0.5"/></svg>"##;
    let small_id = import_svg("small_green", small_green);
    let root = f.root();
    assert!(f.library.lock().unwrap().item(&small_id).unwrap().thumbnail_path(&root).is_none());

    // Large SVG (>= 100 KB): keeps thumbnail file.
    let mut big_blue = String::from(r##"<svg xmlns="http://www.w3.org/2000/svg" width="240" height="240"><rect width="240" height="240" fill="#0000ff"/>"##);
    for i in 0..1900 {
        big_blue.push_str(&format!(r##"<rect x="{}" y="{}" width="1" height="1" fill="#0000ff"/>"##, i % 200, (i * 3) % 200));
    }
    big_blue.push_str("</svg>");
    assert!(big_blue.len() >= 100_000);
    let big_id = import_svg("big_blue", big_blue.as_bytes());
    assert!(f.library.lock().unwrap().item(&big_id).unwrap().thumbnail_path(&root).is_some());

    // Malformed SVG: must skip safely without breaking the queue for valid SVGs.
    let bad_id = import_svg("malformed", b"<svg><not-valid-xml");

    let service = f.service();
    service.set_library(Some(f.library.clone()));
    service.set_enabled(true);
    let status = ready(&service, 3);
    assert_eq!(status.skipped, 1);

    assert!(ids(&service, "green").contains(&small_id), "small SVG indexed without cached thumbnail");
    assert!(ids(&service, "blue").contains(&big_id), "large SVG keeps working");
    assert!(!ids(&service, "green").contains(&bad_id));

    // Visual match on small SVG succeeds using SVG rasterizer.
    let sim = service.search_blocking(Query::Item(small_id.clone()), 5, WAIT).unwrap();
    assert!(sim.iter().any(|h| h.id == small_id));
}

#[test]
fn previously_skipped_small_svg_retries_automatically_without_dropping_existing_vectors() {
    use std::time::UNIX_EPOCH;
    let f = Fixture::new();
    let red_id = f.add("red", [250, 10, 10]);

    let svg_path = f.dir.path().join("leaf.svg");
    std::fs::write(&svg_path, br##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="120"><rect width="120" height="120" fill="#00cc00"/></svg>"##).unwrap();
    let svg_id = f.library.lock().unwrap().import_file(&svg_path, &ImportOptions { palette: false, ..Default::default() }).unwrap();

    let (lib_root, svg_raw) = {
        let lib = f.library.lock().unwrap();
        let item = lib.item(&svg_id).unwrap();
        (lib.root().to_path_buf(), item.managed_storage_path(lib.root()))
    };
    let m = std::fs::metadata(&svg_raw).unwrap();
    let legacy_stamp = (m.modified().unwrap().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64) ^ m.len().rotate_left(48);

    {
        let mut idx = falcon_clip::store::Index::open(&f.index_dir(), &lib_root, "test-model", 3).unwrap();
        let red_item = f.library.lock().unwrap().item(&red_id).unwrap().clone();
        let red_pic = red_item.thumbnail_path(&lib_root).unwrap_or_else(|| red_item.managed_storage_path(&lib_root));
        let rm = std::fs::metadata(&red_pic).unwrap();
        let red_stamp = (rm.modified().unwrap().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64) ^ rm.len().rotate_left(48);
        idx.put(&red_id, red_stamp, &[1.0, 0.0, 0.0]).unwrap();
        idx.skip(&svg_id, legacy_stamp).unwrap();
        idx.flush().unwrap();
    }

    let service = f.service();
    service.set_library(Some(f.library.clone()));
    service.set_enabled(true);
    let status = ready(&service, 2);
    assert_eq!(status.skipped, 0);
    assert_eq!(f.pictures.load(Ordering::SeqCst), 1, "existing valid red vector kept without reindex");
    assert!(ids(&service, "green").contains(&svg_id));
    assert!(ids(&service, "red").contains(&red_id));
}

#[test]
fn combined_linked_svg_offline_missing_corrupt_reconnect_and_delete_matrix() {
    use std::time::UNIX_EPOCH;
    let f = Fixture::new();
    let red_managed = f.add("existing_red", [250, 10, 10]);

    let ext_dir = f.dir.path().join("ext_svg");
    std::fs::create_dir_all(&ext_dir).unwrap();
    let green_svg_path = ext_dir.join("icon.svg");
    let green_svg_bytes = br##"<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96"><rect width="96" height="96" fill="#00e000"/></svg>"##;
    std::fs::write(&green_svg_path, green_svg_bytes).unwrap();
    let green_hash = blake3::hash(green_svg_bytes).to_hex().to_string();

    let (linked_svg_id, legacy_svg_id) = {
        let mut lib = f.library.lock().unwrap();
        lib.set_storage_mode(falcon_library::storage_mode::StorageMode::Link).unwrap();
        let linked_svg_id = lib.import_file(&green_svg_path, &ImportOptions { palette: false, ..Default::default() }).unwrap();
        assert!(lib.item(&linked_svg_id).unwrap().thumbnail_path(lib.root()).is_none());

        let legacy_path = ext_dir.join("legacy.svg");
        std::fs::write(&legacy_path, green_svg_bytes).unwrap();
        let legacy_svg_id = lib.import_file(&legacy_path, &ImportOptions { palette: false, ..Default::default() }).unwrap();
        (linked_svg_id, legacy_svg_id)
    };

    let (lib_root, red_pic, legacy_src) = {
        let lib = f.library.lock().unwrap();
        let r = lib.item(&red_managed).unwrap();
        let rp = r.thumbnail_path(lib.root()).unwrap_or_else(|| r.managed_storage_path(lib.root()));
        let ls = lib.resolved_source_path(&legacy_svg_id).unwrap();
        (lib.root().to_path_buf(), rp, ls)
    };
    let stamp_of = |p: &Path| {
        let m = std::fs::metadata(p).unwrap();
        (m.modified().unwrap().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64) ^ m.len().rotate_left(48)
    };
    {
        let mut idx = falcon_clip::store::Index::open(&f.index_dir(), &lib_root, "test-model", 3).unwrap();
        idx.put(&red_managed, stamp_of(&red_pic), &[1.0, 0.0, 0.0]).unwrap();
        idx.skip(&legacy_svg_id, stamp_of(&legacy_src)).unwrap();
        idx.flush().unwrap();
    }

    let service = f.service();
    service.set_library(Some(f.library.clone()));
    service.set_enabled(true);
    let s1 = ready(&service, 3);
    assert_eq!(s1.skipped, 0);
    // 2 SVGs indexed cleanly; existing valid `red_managed` vector NOT needlessly reindexed!
    assert_eq!(f.pictures.load(Ordering::SeqCst), 2);
    assert!(ids(&service, "green").contains(&linked_svg_id));
    assert!(ids(&service, "green").contains(&legacy_svg_id));
    assert!(ids(&service, "red").contains(&red_managed));

    // Offline & Missing linked small SVG retain embedding.
    let orig_sidecar = mutate_sidecar_offline(&f, &linked_svg_id);
    service.refresh();
    let s_off = wait_for(&service, "offline svg", |s| s.phase == Phase::Ready && s.unavailable == 1);
    assert_eq!(s_off.unavailable, 1);
    assert!(ids(&service, "green").contains(&linked_svg_id));

    restore_sidecar(&f, &linked_svg_id, &orig_sidecar);
    std::fs::remove_file(&green_svg_path).unwrap();
    service.refresh();
    wait_for(&service, "missing svg", |s| s.phase == Phase::Ready && s.unavailable == 1);
    assert!(ids(&service, "green").contains(&linked_svg_id));

    // Reconnect original bytes -> returns cleanly without reindex if stamp untouched; restore file with new mtime.
    std::fs::write(&green_svg_path, green_svg_bytes).unwrap();
    service.refresh();
    ready(&service, 3);
    assert!(ids(&service, "green").contains(&linked_svg_id));

    // Corrupt linked SVG on refresh -> retains last-known-good green embedding as stale!
    let prev_runs = f.pictures.load(Ordering::SeqCst);
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&green_svg_path, b"<svg><broken-xml").unwrap();
    service.refresh();
    let s_stale = wait_for(&service, "corrupt svg stale", |s| s.phase == Phase::Ready && s.stale == 1);
    assert_eq!(s_stale.stale, 1);
    assert!(ids(&service, "green").contains(&linked_svg_id), "corrupt refresh keeps last-known-good embedding");

    // Modified valid SVG triggers incremental reindex (switches from green to blue).
    let blue_svg_bytes = br##"<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96"><rect width="96" height="96" fill="#0000ee"/></svg>"##;
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&green_svg_path, blue_svg_bytes).unwrap();
    service.refresh();
    let s_mod = wait_for(&service, "modified svg reindexed", |s| s.phase == Phase::Ready && s.stale == 0);
    assert_eq!(s_mod.stale, 0);
    assert!(f.pictures.load(Ordering::SeqCst) > prev_runs);
    assert!(ids(&service, "blue").contains(&linked_svg_id));

    // Reconnect unchanged without content change avoids extra embed calls.
    let runs_before_unchanged = f.pictures.load(Ordering::SeqCst);
    let side2 = mutate_sidecar_offline(&f, &linked_svg_id);
    service.refresh();
    wait_for(&service, "offline2", |s| s.phase == Phase::Ready && s.unavailable == 1);
    restore_sidecar(&f, &linked_svg_id, &side2);
    service.refresh();
    ready(&service, 3);
    assert_eq!(f.pictures.load(Ordering::SeqCst), runs_before_unchanged);

    // Explicit catalog deletion removes only intended vector (`legacy_svg_id`), keeping others intact.
    {
        let mut lib = f.library.lock().unwrap();
        lib.delete_permanently(std::slice::from_ref(&legacy_svg_id)).unwrap();
    }
    service.refresh();
    ready(&service, 2);
    drop(service);
    let idx = reopen_index(&f);
    assert!(idx.vector(&legacy_svg_id).is_none());
    assert!(idx.vector(&linked_svg_id).is_some());
    assert!(idx.vector(&red_managed).is_some());
    assert_eq!(blake3::hash(green_svg_bytes).to_hex().to_string(), green_hash);
}
