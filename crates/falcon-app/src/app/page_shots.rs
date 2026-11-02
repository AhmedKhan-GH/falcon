//! Bookmark thumbnails drawn from the page: the app's side of `page_shot`.
//! Import Links queues the bookmarks it made and Refresh Thumbnail queues
//! the ones selected, and nothing else ever does. A picture is scaled and
//! saved on a worker through the library's normal thumbnail path, then the
//! card is drawn again. A page that gives no picture keeps what it had,
//! which for a new bookmark is the plain icon.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, Weak};
use std::time::Instant;

use eframe::egui;
use falcon_library::Library;
use falcon_library::model::Item;
use falcon_library::thumbnail::{PageThumbnail, has_custom_thumbnail};

use super::App;
use crate::locale::tr_n;
use crate::page_shot::{Finished, PAGE_SIZE, Shots, Timing, decode_tiff};
use crate::webview::{self, Page};

#[cfg(target_os = "macos")]
type Engine = crate::page_shot::webkit::WebKit;
#[cfg(not(target_os = "macos"))]
type Engine = crate::page_shot::NoEngine;

fn engine(ctx: &egui::Context) -> Engine {
    #[cfg(target_os = "macos")]
    {
        let ctx = ctx.clone();
        Engine::new(move || ctx.request_repaint())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = ctx;
        crate::page_shot::NoEngine
    }
}

/// An item waiting for its picture. The library is held weakly, so one that
/// closes drops its pages.
#[derive(Clone)]
struct Target {
    lib: Weak<Mutex<Library>>,
    id: String,
    /// Asked for with Refresh Thumbnail, so the outcome is reported. An
    /// import's bookmark just keeps its icon.
    asked: bool,
}

impl PartialEq for Target {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.lib.ptr_eq(&other.lib)
    }
}

/// What the worker did with a picture.
enum Saved {
    Yes,
    /// The item has gone, has another address or has a custom thumbnail now.
    Skipped,
    Failed(String),
}

struct Running {
    shots: Shots<Target, Engine>,
    tx: Sender<(Target, Saved)>,
    rx: Receiver<(Target, Saved)>,
    /// Pictures with a worker.
    saving: usize,
}

/// The queue, made when the first page is asked for.
#[derive(Default)]
pub(crate) struct PageShots {
    running: Option<Running>,
    /// Refresh Thumbnail's results so far, reported when the queue empties.
    drawn: usize,
    failed: usize,
}

impl PageShots {
    fn enqueue(&mut self, ctx: &egui::Context, target: Target, url: &str) -> bool {
        let run = self.running.get_or_insert_with(|| {
            let (tx, rx) = channel();
            Running { shots: Shots::new(engine(ctx), Timing::default()), tx, rx, saving: 0 }
        });
        run.shots.enqueue(target, url)
    }
}

/// The address a page screenshot of `item` would load: a `.url` bookmark
/// with an http or https address and no custom thumbnail.
fn page_address(root: &std::path::Path, item: &Item) -> Option<String> {
    if has_custom_thumbnail(item) {
        return None;
    }
    match webview::page_for(&item.ext, &item.url, &item.original_path(root)) {
        Some(Page::Remote(url)) => Some(url),
        _ => None,
    }
}

/// Scales and saves one picture. The slow part runs without the lock; the
/// library is checked again once it is held.
fn save(target: &Target, url: &str, tiff: &[u8]) -> Saved {
    let Some(shared) = target.lib.upgrade() else { return Saved::Skipped };
    let Some(img) = decode_tiff(tiff) else { return Saved::Failed("unreadable picture".into()) };
    let page = match PageThumbnail::draw(image::DynamicImage::ImageRgba8(img)) {
        Ok(page) => page,
        Err(e) => return Saved::Failed(e.to_string()),
    };
    let mut lib = shared.lock().unwrap_or_else(|p| p.into_inner());
    let root = lib.root().to_path_buf();
    if lib.item(&target.id).and_then(|item| page_address(&root, item)).as_deref() != Some(url) {
        return Saved::Skipped;
    }
    match lib.set_page_thumbnail(&target.id, &page, PAGE_SIZE.0, PAGE_SIZE.1) {
        Ok(true) => Saved::Yes,
        Ok(false) => Saved::Skipped,
        Err(e) => Saved::Failed(e.to_string()),
    }
}

impl App {
    /// Queues the pages of the bookmarks Import Links made, among the items
    /// just `added` to the open library.
    pub(crate) fn page_shots_for_links(&mut self, added: &[String]) {
        if self.link_import.bookmarks.is_empty() {
            return;
        }
        let Some(shared) = self.ws.lib.clone() else { return };
        let lib = shared.lock().unwrap_or_else(|p| p.into_inner());
        self.page_shots_for_links_in(&lib, added);
    }

    /// The same, with the library already locked.
    pub(crate) fn page_shots_for_links_in(&mut self, lib: &Library, added: &[String]) {
        let Some(shared) = &self.ws.lib else { return };
        let weak = Arc::downgrade(shared);
        for id in added {
            let Some(url) = lib.item(id).and_then(|item| page_address(lib.root(), item)) else { continue };
            if self.link_import.bookmarks.remove(&url) {
                self.page_shots.enqueue(&self.ctx, Target { lib: weak.clone(), id: id.clone(), asked: false }, &url);
            }
        }
        self.ctx.request_repaint();
    }

    /// Refresh Thumbnail: queues the pages of the bookmarks among `ids` and
    /// returns the other items, which refresh as usual.
    pub(crate) fn page_shots_refresh(&mut self, lib: &Library, ids: Vec<String>) -> Vec<String> {
        let (Some(shared), true) = (&self.ws.lib, webview::WebView::available()) else { return ids };
        let weak = Arc::downgrade(shared);
        let mut rest = Vec::new();
        let mut queued = 0;
        for id in ids {
            match lib.item(&id).and_then(|item| page_address(lib.root(), item)) {
                Some(url) => queued += usize::from(self.page_shots.enqueue(&self.ctx, Target { lib: weak.clone(), id, asked: true }, &url)),
                None => rest.push(id),
            }
        }
        if queued > 0 {
            self.flash(tr_n(queued, "Drawing {n} bookmark page", "Drawing {n} bookmark pages"));
            self.ctx.request_repaint();
        }
        rest
    }

    /// Moves the queue along, once a frame.
    pub(crate) fn page_shots_tick(&mut self, ctx: &egui::Context) {
        let Some(run) = &mut self.page_shots.running else { return };
        run.shots.retain(|t| t.lib.strong_count() > 0);
        let now = Instant::now();
        let mut failed = 0;
        for Finished { key, url, result } in run.shots.tick(now) {
            match result {
                Ok(tiff) => {
                    run.saving += 1;
                    let (tx, ctx) = (run.tx.clone(), ctx.clone());
                    std::thread::spawn(move || {
                        let saved = save(&key, &url, &tiff);
                        let _ = tx.send((key, saved));
                        ctx.request_repaint();
                    });
                }
                Err(_) => failed += usize::from(key.asked),
            }
        }
        let saved: Vec<_> = run.rx.try_iter().collect();
        run.saving -= saved.len();
        if let Some(wait) = run.shots.next_wake(now) {
            ctx.request_repaint_after(wait);
        }
        let idle = run.shots.is_idle() && run.saving == 0;
        self.page_shots.failed += failed;
        for (target, saved) in saved {
            match saved {
                Saved::Yes => {
                    self.page_shots.drawn += usize::from(target.asked);
                    self.page_shot_saved(&target);
                }
                Saved::Skipped => {}
                Saved::Failed(e) => {
                    crate::errlog::error(&format!("bookmark page: could not save the thumbnail: {e}"));
                    self.page_shots.failed += usize::from(target.asked);
                }
            }
        }
        if idle {
            let (drawn, failed) = (std::mem::take(&mut self.page_shots.drawn), std::mem::take(&mut self.page_shots.failed));
            if failed > 0 {
                self.flash_error(tr_n(failed, "{n} bookmark page could not be drawn", "{n} bookmark pages could not be drawn"));
            } else if drawn > 0 {
                self.flash(tr_n(drawn, "Refreshed {n} thumbnail", "Refreshed {n} thumbnails"));
            }
        }
    }

    /// Draws the card again in whichever open library holds the item.
    fn page_shot_saved(&mut self, target: &Target) {
        let holds = |ws: &super::Workspace| ws.lib.as_ref().is_some_and(|l| std::ptr::eq(Arc::as_ptr(l), target.lib.as_ptr()));
        let now = Instant::now();
        if holds(&self.ws) {
            self.ws.loader.forget(&target.id);
            self.ws.backup.changed(now);
        } else if let Some(ws) = self.others.parked_mut().find(|ws| holds(ws)) {
            ws.loader.forget(&target.id);
            ws.backup.changed(now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use falcon_library::import::ImportOptions;

    fn bookmark(lib: &mut Library, dir: &std::path::Path, url: &str) -> String {
        let src = dir.join("page.url");
        std::fs::write(&src, crate::link_import::shortcut(url)).unwrap();
        lib.import_file(&src, &ImportOptions { url: url.into(), ..Default::default() }).unwrap()
    }

    #[test]
    fn only_web_bookmarks_without_a_custom_thumbnail_have_a_page() {
        let dir = tempfile::tempdir().unwrap();
        let mut lib = Library::create(dir.path().join("T.library")).unwrap();
        let web = bookmark(&mut lib, dir.path(), "https://example.com/");
        let root = lib.root().to_path_buf();
        assert_eq!(page_address(&root, lib.item(&web).unwrap()).as_deref(), Some("https://example.com/"));

        let mut other = lib.item(&web).unwrap().clone();
        other.url = "file:///etc/hosts".into();
        assert_eq!(page_address(&root, &other), None, "not a web address");
        let mut custom = lib.item(&web).unwrap().clone();
        custom.extra.insert(falcon_library::thumbnail::CUSTOM_THUMBNAIL_KEY.into(), true.into());
        assert_eq!(page_address(&root, &custom), None, "the user's own thumbnail stays");
        let mut html = lib.item(&web).unwrap().clone();
        html.ext = "html".into();
        assert_eq!(page_address(&root, &html), None, "not a bookmark");
    }

    #[test]
    fn a_picture_is_saved_only_for_the_address_it_shows() {
        let dir = tempfile::tempdir().unwrap();
        let mut lib = Library::create(dir.path().join("T.library")).unwrap();
        let id = bookmark(&mut lib, dir.path(), "https://example.com/");
        let root = lib.root().to_path_buf();
        let shared = Arc::new(Mutex::new(lib));
        let target = Target { lib: Arc::downgrade(&shared), id: id.clone(), asked: false };

        use tiff::encoder::{TiffEncoder, colortype};
        let mut tiff = std::io::Cursor::new(Vec::new());
        let pixels = vec![200u8; 16 * 10 * 4];
        TiffEncoder::new(&mut tiff).unwrap().write_image::<colortype::RGBA8>(16, 10, &pixels).unwrap();
        let tiff = tiff.into_inner();

        assert!(matches!(save(&target, "https://elsewhere.example/", &tiff), Saved::Skipped));
        assert!(matches!(save(&target, "https://example.com/", b"junk"), Saved::Failed(_)));
        assert!(matches!(save(&target, "https://example.com/", &tiff), Saved::Yes));
        let lib = shared.lock().unwrap();
        let item = lib.item(&id).unwrap();
        assert_eq!((item.width, item.height), (1440, 900));
        assert!(item.thumbnail_path(&root).is_some());
        drop(lib);
        let gone = Target { lib: Weak::new(), id, asked: false };
        assert!(matches!(save(&gone, "https://example.com/", &tiff), Saved::Skipped), "the library closed");
    }
}
