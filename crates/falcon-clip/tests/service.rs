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
        item.thumbnail_path(&root).unwrap_or_else(|| item.original_path(&root))
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
