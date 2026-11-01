//! The viewer for PDF items: pages from `falcon-pdf` in one continuous
//! vertical scroll, rendered on a worker thread, with zoom, page
//! navigation, find, text selection and a thumbnail/outline sidebar (see
//! the desktop app chapter of the docs).
//!
//! The document is opened, rendered, searched and selected only on its
//! worker thread, so an iCloud file that still has to download, or a huge
//! book, never blocks the window. The UI sends the worker the pages it
//! wants (the visible ones and one either side) and turns the pictures that
//! come back into textures, kept in a pixel-budgeted LRU cache.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, Color32, CornerRadius, Event, Id, Key, Modifiers, Pos2, Rect, Sense, Stroke, TextEdit, TextureHandle, Ui, Vec2, vec2};
use falcon_library::Library;
use falcon_library::model::Item;
use falcon_pdf::{Document, Hit, OutlineEntry, SearchOptions};

use super::{App, icon_button};
use crate::icons as ic;
use crate::theme::{self as t, font, mono};

/// Screen points per PDF point at 100%, as in pdf.js (96 CSS pixels per
/// 72-point inch), so "100%" matches Eagle's reader.
pub const UNIT: f32 = 96.0 / 72.0;
/// Space around the column of pages and between pages, in screen points.
const PAD: f32 = 12.0;
const GAP: f32 = 10.0;
const SCROLLBAR: f32 = 16.0;
pub const MIN_ZOOM: f32 = 0.1;
pub const MAX_ZOOM: f32 = 10.0;
/// "Automatic" zoom never goes past this (pdf.js's MAX_AUTO_SCALE).
const MAX_AUTO: f32 = 1.25;
/// Texture budget for rendered pages and for thumbnails, in pixels.
const PAGE_BUDGET: usize = 128 << 20;
const THUMB_BUDGET: usize = 16 << 20;
/// One page's picture never exceeds this many pixels (pdf.js caps its
/// canvases at the same 16 MP); past that, deep zoom shows it enlarged.
const MAX_PAGE_PIXELS: f64 = 16_777_216.0;
const THUMB_W: f32 = 112.0;
const THUMB_PX: usize = 240;
const SIDEBAR_W: f32 = 168.0;
/// How long the zoom must hold still before pages re-render sharp.
const SETTLE: Duration = Duration::from_millis(160);
/// Documents whose last page and zoom are remembered (pdf.js keeps 20).
const REMEMBER: usize = 20;
/// The cache key scale used for thumbnails.
const THUMB_KEY: i32 = i32::MIN;
const PAGE_UP: &str = "\u{E13C}";
const PAGE_DOWN: &str = "\u{E136}";
const FIND_ID: &str = "pdf-find";
const PAGE_ID: &str = "pdf-page";

// ---------------------------------------------------------------------------
// Zoom and layout (plain logic, unit-tested)

/// The zoom choices of pdf.js's zoom menu, which Eagle's reader keeps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Zoom {
    Auto,
    PageFit,
    PageWidth,
    Actual,
    /// 1.0 is 100%.
    Percent(f32),
}

impl Zoom {
    pub const PRESETS: [f32; 8] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0];

    /// The value saved between launches (pdf.js's own spelling).
    pub fn key(self) -> String {
        match self {
            Zoom::Auto => "auto".into(),
            Zoom::PageFit => "page-fit".into(),
            Zoom::PageWidth => "page-width".into(),
            Zoom::Actual => "page-actual".into(),
            Zoom::Percent(p) => format!("{p}"),
        }
    }

    pub fn from_key(s: &str) -> Zoom {
        match s {
            "auto" => Zoom::Auto,
            "page-fit" => Zoom::PageFit,
            "page-width" => Zoom::PageWidth,
            "page-actual" => Zoom::Actual,
            other => other.parse::<f32>().ok().filter(|p| p.is_finite() && *p > 0.0).map_or(Zoom::Auto, |p| Zoom::Percent(p.clamp(MIN_ZOOM, MAX_ZOOM))),
        }
    }

    pub fn label(self) -> String {
        match self {
            Zoom::Auto => "Automatic Zoom".into(),
            Zoom::PageFit => "Page Fit".into(),
            Zoom::PageWidth => "Page Width".into(),
            Zoom::Actual => "Actual Size".into(),
            Zoom::Percent(p) => format!("{:.0}%", p * 100.0),
        }
    }

    /// The zoom factor for a page of `page` points in a view of `view`
    /// screen points. Automatic is page width (page fit for landscape
    /// pages), never above 125%, as in pdf.js.
    pub fn resolve(self, page: (f32, f32), view: Vec2) -> f32 {
        let (pw, ph) = (page.0 * UNIT, page.1 * UNIT);
        if pw <= 0.0 || ph <= 0.0 {
            return match self {
                Zoom::Percent(p) => p.clamp(MIN_ZOOM, MAX_ZOOM),
                _ => 1.0,
            };
        }
        let width = (view.x - 2.0 * PAD - SCROLLBAR) / pw;
        let fit = width.min((view.y - 2.0 * PAD) / ph);
        let z = match self {
            Zoom::Auto => (if pw > ph { fit } else { width }).min(MAX_AUTO),
            Zoom::PageFit => fit,
            Zoom::PageWidth => width,
            Zoom::Actual => 1.0,
            Zoom::Percent(p) => p,
        };
        if z.is_finite() { z.clamp(MIN_ZOOM, MAX_ZOOM) } else { 1.0 }
    }
}

/// One zoom step in or out from `cur` (10%, rounded to whole percents and
/// always moving at least one), within the zoom limits.
pub fn zoom_step(cur: f32, up: bool) -> f32 {
    let next = if up { cur * 1.1 } else { cur / 1.1 };
    let mut r = (next * 100.0).round() / 100.0;
    if up && r <= cur {
        r = cur + 0.01;
    }
    if !up && r >= cur {
        r = cur - 0.01;
    }
    r.clamp(MIN_ZOOM, MAX_ZOOM)
}

/// Where each page sits in the scrolling column, in screen points.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub tops: Vec<f32>,
    pub sizes: Vec<Vec2>,
    /// The widest page.
    pub width: f32,
    /// The whole column, padding included.
    pub height: f32,
}

/// Lays out pages of `pages` points at `zoom`, `gap` points apart.
pub fn layout(pages: &[(f32, f32)], zoom: f32, gap: f32) -> Layout {
    let mut l = Layout::default();
    let mut y = PAD;
    for &(w, h) in pages {
        let s = vec2(w.max(0.0) * UNIT * zoom, h.max(0.0) * UNIT * zoom);
        l.tops.push(y);
        l.sizes.push(s);
        l.width = l.width.max(s.x);
        y += s.y + gap;
    }
    l.height = if pages.is_empty() { 2.0 * PAD } else { y - gap + PAD };
    l
}

/// The first index in `0..n` where `pred` turns false (`pred` must be true
/// then false).
fn partition(n: usize, pred: impl Fn(usize) -> bool) -> usize {
    let (mut lo, mut hi) = (0, n);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if pred(mid) { lo = mid + 1 } else { hi = mid }
    }
    lo
}

impl Layout {
    fn bottom(&self, i: usize) -> f32 {
        self.tops[i] + self.sizes[i].y
    }

    /// The first and last page that show in a view `height` tall scrolled
    /// to `top`; `None` past the end or for an empty document.
    pub fn visible(&self, top: f32, height: f32) -> Option<(usize, usize)> {
        let n = self.tops.len();
        let first = partition(n, |i| self.bottom(i) <= top);
        if first >= n {
            return None;
        }
        let end = partition(n, |i| self.tops[i] < top + height);
        Some((first, end.max(first + 1) - 1))
    }

    /// The page showing the most in the view: the "current page".
    pub fn current(&self, top: f32, height: f32) -> usize {
        let Some((a, b)) = self.visible(top, height) else { return self.tops.len().saturating_sub(1) };
        let shown = |i: usize| self.bottom(i).min(top + height) - self.tops[i].max(top);
        (a..=b).fold(a, |best, i| if shown(i) > shown(best) + 0.5 { i } else { best })
    }

    /// The page under content height `y`, if not in a gap.
    pub fn page_at(&self, y: f32) -> Option<usize> {
        let i = partition(self.tops.len(), |i| self.bottom(i) <= y);
        (i < self.tops.len() && self.tops[i] <= y).then_some(i)
    }

    /// The content width for a view `view_w` wide.
    pub fn content_width(&self, view_w: f32) -> f32 {
        (self.width + 2.0 * PAD).max(view_w)
    }

    /// Page `i`'s left edge in content `content_w` wide: centred.
    pub fn left(&self, i: usize, content_w: f32) -> f32 {
        ((content_w - self.sizes[i].x) / 2.0).max(PAD)
    }

    /// The scroll offset that puts page `i`'s top edge at the top of the view.
    pub fn top_of(&self, i: usize) -> f32 {
        self.tops.get(i).map_or(0.0, |t| (t - PAD).max(0.0))
    }
}

/// The vertical offset in `new` that shows the same spot of the same page
/// as `offset` did in `old` (keeps the reading position across a zoom).
pub fn anchor(old: &Layout, new: &Layout, offset: f32) -> f32 {
    let n = old.tops.len().min(new.tops.len());
    if n == 0 {
        return 0.0;
    }
    let p = partition(n, |i| old.bottom(i) + GAP <= offset).min(n - 1);
    let h = old.sizes[p].y.max(1.0);
    let frac = (offset - old.tops[p]) / h;
    (new.tops[p] + frac * new.sizes[p].y).max(0.0)
}

/// The cache key for rendering at `px_per_pt` pixels per PDF point: steps
/// of 2^(1/16) (about 4%), rounding up so a page is never drawn blurry.
pub fn scale_key(px_per_pt: f32) -> i32 {
    (px_per_pt.max(1e-3).log2() * 16.0 - 1e-3).ceil() as i32
}

pub fn key_scale(key: i32) -> f64 {
    2f64.powf(f64::from(key) / 16.0)
}

/// A 1-based page number typed by the user, as a page index within the
/// document; out-of-range numbers go to the first or last page.
pub fn parse_page(s: &str, count: usize) -> Option<usize> {
    let n = s.trim().parse::<i64>().ok()?;
    (count > 0).then(|| (n.clamp(1, count as i64) - 1) as usize)
}

/// The next (or previous) find hit, wrapping around the ends.
pub fn step_hit(cur: Option<usize>, len: usize, forward: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match cur {
        None if forward => 0,
        None => len - 1,
        Some(c) if forward => (c + 1) % len,
        Some(c) => (c + len - 1) % len,
    })
}

/// The first hit on or after `page`, wrapping to the first: where a new
/// search starts, as pdf.js starts from the current page.
pub fn first_hit_from(hit_pages: &[usize], page: usize) -> Option<usize> {
    if hit_pages.is_empty() {
        return None;
    }
    Some(hit_pages.iter().position(|&p| p >= page).unwrap_or(0))
}

/// Rendered pages kept as textures, keyed by (page, scale key), evicting
/// the least recently used once their pixels pass the budget.
pub struct PageCache<T> {
    entries: HashMap<(usize, i32), (T, usize, u64)>,
    used: usize,
    budget: usize,
    tick: u64,
}

impl<T> PageCache<T> {
    pub fn new(budget: usize) -> Self {
        PageCache { entries: HashMap::new(), used: 0, budget, tick: 0 }
    }

    pub fn insert(&mut self, key: (usize, i32), value: T, pixels: usize) {
        self.tick += 1;
        if let Some((_, px, _)) = self.entries.insert(key, (value, pixels, self.tick)) {
            self.used -= px;
        }
        self.used += pixels;
        while self.used > self.budget && self.entries.len() > 1 {
            let Some(oldest) = self.entries.iter().filter(|(k, _)| **k != key).min_by_key(|(_, v)| v.2).map(|(k, _)| *k) else { break };
            if let Some((_, px, _)) = self.entries.remove(&oldest) {
                self.used -= px;
            }
        }
    }

    pub fn contains(&self, key: (usize, i32)) -> bool {
        self.entries.contains_key(&key)
    }

    /// Whether any scale of `page` is cached.
    pub fn has_page(&self, page: usize) -> bool {
        self.entries.keys().any(|k| k.0 == page)
    }

    /// The picture of `page` at scale `key`, or else the cached scale
    /// nearest to it (preferring the sharper of two equally near), to show
    /// enlarged or shrunk until the right one arrives. Marks it used.
    pub fn best(&mut self, page: usize, key: i32) -> Option<&T> {
        let pick = if self.entries.contains_key(&(page, key)) {
            (page, key)
        } else {
            *self.entries.keys().filter(|k| k.0 == page).min_by_key(|k| ((i64::from(k.1) - i64::from(key)).abs(), -i64::from(k.1)))?
        };
        self.tick += 1;
        let e = self.entries.get_mut(&pick)?;
        e.2 = self.tick;
        Some(&e.0)
    }

    #[cfg(test)]
    pub fn used(&self) -> usize {
        self.used
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// What the viewer remembers of a document: its last page and zoom.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Mark {
    pub id: String,
    /// 0-based.
    pub page: usize,
    pub zoom: String,
}

/// Records `mark` as the most recent, keeping the newest [`REMEMBER`].
pub fn remember(marks: &mut Vec<Mark>, mark: Mark) {
    marks.retain(|m| m.id != mark.id);
    marks.push(mark);
    if marks.len() > REMEMBER {
        let extra = marks.len() - REMEMBER;
        marks.drain(..extra);
    }
}

/// What a key does in the reader.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PdfKey {
    Find,
    FindNext(bool),
    CloseFind,
    NextPage,
    PrevPage,
    First,
    Last,
    Lines(f32),
    Screens(f32),
    ZoomIn,
    ZoomOut,
    ZoomReset,
}

/// The reader's keys, after pdf.js: n/j and p/k turn pages, Home/End and
/// ⌘↑/⌘↓ go to the ends, ↑/↓ and Page Up/Down scroll, ⌘+/⌘−/⌘0 zoom,
/// ⌘F finds and ⌘G/⇧⌘G (or ↩/⇧↩ in the find field) step through hits.
/// ←/→, Space and Esc (with find closed) are left to the viewer, which
/// moves between items or closes, as Eagle forwards them from its reader.
pub fn key_action(key: Key, m: Modifiers, find_focused: bool, find_open: bool) -> Option<PdfKey> {
    let cmd = m.command || m.mac_cmd;
    if find_focused {
        return match key {
            Key::Enter => Some(PdfKey::FindNext(!m.shift)),
            Key::Escape => Some(PdfKey::CloseFind),
            Key::F if cmd => Some(PdfKey::Find),
            Key::G if cmd => Some(PdfKey::FindNext(!m.shift)),
            _ => None,
        };
    }
    if cmd {
        return match key {
            Key::F if !m.shift => Some(PdfKey::Find),
            Key::G => Some(PdfKey::FindNext(!m.shift)),
            Key::Equals | Key::Plus => Some(PdfKey::ZoomIn),
            Key::Minus => Some(PdfKey::ZoomOut),
            Key::Num0 => Some(PdfKey::ZoomReset),
            Key::ArrowUp => Some(PdfKey::First),
            Key::ArrowDown => Some(PdfKey::Last),
            _ => None,
        };
    }
    if m.alt || m.ctrl {
        return None;
    }
    match key {
        Key::N | Key::J if !m.shift => Some(PdfKey::NextPage),
        Key::P | Key::K if !m.shift => Some(PdfKey::PrevPage),
        Key::Home => Some(PdfKey::First),
        Key::End => Some(PdfKey::Last),
        Key::ArrowDown => Some(PdfKey::Lines(1.0)),
        Key::ArrowUp => Some(PdfKey::Lines(-1.0)),
        Key::PageDown => Some(PdfKey::Screens(1.0)),
        Key::PageUp => Some(PdfKey::Screens(-1.0)),
        Key::Escape if find_open => Some(PdfKey::CloseFind),
        _ => None,
    }
}

/// Whether a file's contents live only in iCloud, so opening it starts a
/// download (the `SF_DATALESS` flag). Reading the flag does not download.
/// The one place to switch to a shared iCloud helper later.
pub fn icloud_only(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt;
        std::fs::metadata(path).is_ok_and(|m| m.st_flags() & 0x4000_0000 != 0)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        false
    }
}

// ---------------------------------------------------------------------------
// The worker thread

/// A page picture wanted: `key` is a [`scale_key`] or [`THUMB_KEY`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Job {
    page: usize,
    key: i32,
}

enum Req {
    /// The pages wanted now, most wanted first; replaces the previous list.
    Want(Vec<Job>),
    Search(u64, String, SearchOptions),
    Select(u64, usize, (f64, f64), (f64, f64)),
}

enum Reply {
    Opened { sizes: Vec<(f64, f64)>, outline: Vec<OutlineEntry> },
    Failed(String),
    Page(Job, Option<falcon_pdf::Bitmap>),
    Found(u64, Vec<Hit>),
    Selected(u64, falcon_pdf::Selection),
}

/// A selection request: serial, page, drag start and end.
type SelectReq = (u64, usize, (f64, f64), (f64, f64));

/// The latest request of each kind, waiting for the worker.
#[derive(Default)]
struct Pending {
    queue: VecDeque<Job>,
    search: Option<(u64, String, SearchOptions)>,
    select: Option<SelectReq>,
}

impl Pending {
    fn take(&mut self, r: Req) {
        match r {
            Req::Want(jobs) => self.queue = jobs.into(),
            Req::Search(g, s, o) => self.search = Some((g, s, o)),
            Req::Select(g, p, a, b) => self.select = Some((g, p, a, b)),
        }
    }

    fn idle(&self) -> bool {
        self.queue.is_empty() && self.search.is_none() && self.select.is_none()
    }
}

fn spawn(path: PathBuf, ctx: egui::Context, max_side: usize) -> (Sender<Req>, Receiver<Reply>) {
    let (tx, wrx) = mpsc::channel();
    let (wtx, rx) = mpsc::channel();
    // If the thread can't start, `rx` disconnects and the viewer says so.
    let _ = std::thread::Builder::new().name("falcon-pdf".into()).spawn(move || work(&path, &wrx, &wtx, &ctx, max_side));
    (tx, rx)
}

fn work(path: &Path, rx: &Receiver<Req>, tx: &Sender<Reply>, ctx: &egui::Context, max_side: usize) {
    let send = |r: Reply| {
        let ok = tx.send(r).is_ok();
        ctx.request_repaint();
        ok
    };
    let doc = match Document::open(path) {
        Ok(d) => d,
        Err(e) => {
            send(Reply::Failed(e.0));
            return;
        }
    };
    if !send(Reply::Opened { sizes: doc.page_sizes(), outline: doc.outline() }) {
        return;
    }
    let mut p = Pending::default();
    // Jobs just rendered: a resent list may still name them before their
    // pictures reach the viewer.
    let mut recent: VecDeque<Job> = VecDeque::new();
    loop {
        if p.idle() {
            match rx.recv() {
                Ok(r) => p.take(r),
                Err(_) => return,
            }
        }
        loop {
            match rx.try_recv() {
                Ok(r) => p.take(r),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        let reply = if let Some((g, page, a, b)) = p.select.take() {
            Reply::Selected(g, doc.selection(page, a, b))
        } else if let Some((g, text, o)) = p.search.take() {
            Reply::Found(g, doc.search(&text, o))
        } else if let Some(job) = p.queue.pop_front() {
            if recent.contains(&job) {
                continue;
            }
            recent.push_back(job);
            if recent.len() > 16 {
                recent.pop_front();
            }
            let bitmap = if job.key == THUMB_KEY {
                doc.thumbnail(job.page, THUMB_PX)
            } else {
                let (w, h) = doc.page_size(job.page).unwrap_or((612.0, 792.0));
                let cap = (MAX_PAGE_PIXELS / (w * h).max(1.0)).sqrt();
                doc.render(job.page, key_scale(job.key).min(cap), max_side)
            };
            Reply::Page(job, bitmap.ok())
        } else {
            continue;
        };
        if !send(reply) {
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// Viewer state

#[derive(Clone, Debug, PartialEq)]
enum Status {
    Opening,
    Ready,
    Failed(String),
}

#[derive(Default)]
struct Find {
    open: bool,
    text: String,
    match_case: bool,
    /// The query last sent, to search again only when it changes.
    sent: Option<(String, bool)>,
    serial: u64,
    searching: bool,
    hits: Vec<Hit>,
    current: Option<usize>,
    focus: bool,
}

#[derive(Default)]
struct Selecting {
    /// A drag selection exists (or is being made) on `page`.
    active: bool,
    page: usize,
    from: (f64, f64),
    to: (f64, f64),
    sent: Option<(f64, f64)>,
    waiting: bool,
    serial: u64,
    text: String,
    rects: Vec<falcon_pdf::Rect>,
}

/// The sidebar's two lists, as in pdf.js's sidebar.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Tab {
    Thumbnails,
    Outline,
}

/// One open document.
struct Open {
    id: String,
    tx: Sender<Req>,
    rx: Receiver<Reply>,
    status: Status,
    icloud: bool,
    sizes: Vec<(f32, f32)>,
    outline: Vec<OutlineEntry>,
    pages: PageCache<TextureHandle>,
    thumbs: PageCache<TextureHandle>,
    broken: HashSet<usize>,
    sent: Vec<Job>,
    zoom: Zoom,
    /// The zoom factor last laid out, and when it last changed.
    scale: f32,
    changed: Instant,
    layout: Layout,
    offset: Vec2,
    view: Vec2,
    jump: Option<Vec2>,
    /// A pointer position (relative to the view) to zoom around.
    zoom_at: Option<Vec2>,
    restore: Option<usize>,
    current: usize,
    page_text: String,
    find: Find,
    sel: Selecting,
}

impl Open {
    fn new(ctx: &egui::Context, item: &Item, path: PathBuf, mark: Option<&Mark>) -> Open {
        let icloud = icloud_only(&path);
        let max_side = ctx.input(|i| i.max_texture_side).clamp(1024, 8192);
        let (tx, rx) = spawn(path, ctx.clone(), max_side);
        Open {
            id: item.id.clone(),
            tx,
            rx,
            status: Status::Opening,
            icloud,
            sizes: Vec::new(),
            outline: Vec::new(),
            pages: PageCache::new(PAGE_BUDGET),
            thumbs: PageCache::new(THUMB_BUDGET),
            broken: HashSet::new(),
            sent: Vec::new(),
            zoom: mark.map_or(Zoom::Auto, |m| Zoom::from_key(&m.zoom)),
            scale: 0.0,
            changed: Instant::now(),
            layout: Layout::default(),
            offset: Vec2::ZERO,
            view: vec2(800.0, 600.0),
            jump: None,
            zoom_at: None,
            restore: mark.map(|m| m.page),
            current: 0,
            page_text: "1".into(),
            find: Find::default(),
            sel: Selecting::default(),
        }
    }

    fn mark(&self) -> Mark {
        Mark { id: self.id.clone(), page: self.current, zoom: self.zoom.key() }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        loop {
            match self.rx.try_recv() {
                Ok(Reply::Opened { sizes, outline }) => {
                    self.sizes = sizes.into_iter().map(|(w, h)| (w as f32, h as f32)).collect();
                    self.outline = outline;
                    self.status = Status::Ready;
                    if let Some(p) = self.restore {
                        self.restore = Some(p.min(self.sizes.len().saturating_sub(1)));
                        self.current = self.restore.unwrap_or(0);
                    }
                }
                Ok(Reply::Failed(e)) => self.status = Status::Failed(e),
                Ok(Reply::Page(job, Some(b))) => {
                    let img = egui::ColorImage::from_rgba_unmultiplied([b.width, b.height], &b.rgba);
                    let tex = ctx.load_texture(format!("pdf-{}-{}", job.page, job.key), img, egui::TextureOptions::LINEAR);
                    let cache = if job.key == THUMB_KEY { &mut self.thumbs } else { &mut self.pages };
                    cache.insert((job.page, job.key), tex, b.width * b.height);
                }
                Ok(Reply::Page(job, None)) => {
                    self.broken.insert(job.page);
                }
                Ok(Reply::Found(g, hits)) => {
                    if g == self.find.serial {
                        self.find.searching = false;
                        let pages: Vec<usize> = hits.iter().map(|h| h.page).collect();
                        self.find.current = first_hit_from(&pages, self.current);
                        self.find.hits = hits;
                        self.reveal_hit();
                    }
                }
                Ok(Reply::Selected(g, s)) => {
                    if g == self.sel.serial {
                        self.sel.waiting = false;
                        if self.sel.active {
                            self.sel.text = s.text;
                            self.sel.rects = s.rects;
                        }
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.status == Status::Opening {
                        self.status = Status::Failed("the reader stopped".into());
                    }
                    break;
                }
            }
        }
    }

    fn count(&self) -> usize {
        self.sizes.len()
    }

    fn go_to(&mut self, page: usize) {
        if self.count() == 0 {
            return;
        }
        let page = page.min(self.count() - 1);
        self.jump = Some(vec2(self.offset.x, self.layout.top_of(page)));
        self.current = page;
    }

    fn scroll_by(&mut self, dy: f32) {
        let max = (self.layout.height - self.view.y).max(0.0);
        let y = self.jump.map_or(self.offset.y, |j| j.y);
        self.jump = Some(vec2(self.offset.x, (y + dy).clamp(0.0, max)));
    }

    fn zoom_factor(&self) -> f32 {
        if self.scale > 0.0 { self.scale } else { 1.0 }
    }

    fn act(&mut self, a: PdfKey) {
        match a {
            PdfKey::Find => {
                self.find.open = true;
                self.find.focus = true;
            }
            PdfKey::FindNext(fwd) => {
                if !self.find.open {
                    self.find.open = true;
                    self.find.focus = true;
                }
                self.find.current = step_hit(self.find.current, self.find.hits.len(), fwd);
                self.reveal_hit();
            }
            PdfKey::CloseFind => {
                self.find.open = false;
                self.find.hits.clear();
                self.find.current = None;
                self.find.sent = None;
            }
            PdfKey::NextPage => self.go_to(self.current + 1),
            PdfKey::PrevPage => self.go_to(self.current.saturating_sub(1)),
            PdfKey::First => self.go_to(0),
            PdfKey::Last => self.go_to(self.count().saturating_sub(1)),
            PdfKey::Lines(n) => self.scroll_by(n * 40.0),
            PdfKey::Screens(n) => self.scroll_by(n * (self.view.y - 40.0).max(40.0)),
            PdfKey::ZoomIn => self.zoom = Zoom::Percent(zoom_step(self.zoom_factor(), true)),
            PdfKey::ZoomOut => self.zoom = Zoom::Percent(zoom_step(self.zoom_factor(), false)),
            PdfKey::ZoomReset => self.zoom = Zoom::Auto,
        }
    }

    /// Scrolls so the current find hit shows, a third of the way down.
    fn reveal_hit(&mut self) {
        let Some(hit) = self.find.current.and_then(|c| self.find.hits.get(c)) else { return };
        let Some(&top) = self.layout.tops.get(hit.page) else { return };
        let k = self.zoom_factor() * UNIT;
        let r = hit.rects.first().copied().unwrap_or_default();
        let y = top + r.y as f32 * k;
        let (view_top, view_bottom) = (self.offset.y, self.offset.y + self.view.y);
        if y < view_top + 20.0 || y + r.h as f32 * k > view_bottom - 20.0 {
            let max = (self.layout.height - self.view.y).max(0.0);
            self.jump = Some(vec2(self.offset.x, (y - self.view.y / 3.0).clamp(0.0, max)));
        }
        self.current = hit.page;
    }

    /// Sends the latest search when the query changed.
    fn search_if_changed(&mut self) {
        let q = (self.find.text.clone(), self.find.match_case);
        if self.find.sent.as_ref() == Some(&q) {
            return;
        }
        self.find.sent = Some(q.clone());
        self.find.serial += 1;
        self.find.hits.clear();
        self.find.current = None;
        if q.0.is_empty() {
            self.find.searching = false;
            return;
        }
        self.find.searching = true;
        let _ = self.tx.send(Req::Search(self.find.serial, q.0, SearchOptions { match_case: q.1 }));
    }

    /// Asks for the latest drag selection when it moved and the last
    /// answer is in.
    fn select_if_moved(&mut self) {
        let s = &mut self.sel;
        if !s.active || s.waiting || s.sent == Some(s.to) {
            return;
        }
        s.sent = Some(s.to);
        s.serial += 1;
        s.waiting = true;
        let _ = self.tx.send(Req::Select(s.serial, s.page, s.from, s.to));
    }

    fn want(&mut self, jobs: Vec<Job>) {
        if jobs != self.sent {
            let _ = self.tx.send(Req::Want(jobs.clone()));
            self.sent = jobs;
        }
    }

    /// Lays the pages out again when the zoom factor changes, keeping the
    /// reading position (or the point under the pointer for a pinch).
    fn relayout(&mut self, view: Vec2) {
        self.view = view;
        let reference = self.sizes.get(self.current).copied().unwrap_or((612.0, 792.0));
        let scale = self.zoom.resolve(reference, view);
        if (scale - self.scale).abs() < 1e-4 && self.layout.tops.len() == self.sizes.len() {
            return;
        }
        let new = layout(&self.sizes, scale, GAP);
        if self.scale > 0.0 && self.jump.is_none() && !self.layout.tops.is_empty() {
            let ratio = scale / self.scale;
            self.jump = Some(match self.zoom_at.take() {
                Some(at) => (self.offset + at) * ratio - at,
                None => vec2(self.offset.x * ratio, anchor(&self.layout, &new, self.offset.y)),
            });
        }
        self.zoom_at = None;
        self.layout = new;
        self.scale = scale;
        self.changed = Instant::now();
    }
}

/// The reader's state: the open document, and what it remembers.
#[derive(Default)]
pub(super) struct PdfState {
    open: Option<Open>,
    /// Last page and zoom of recent documents, newest last, saved between
    /// launches as pdf.js saves its view history.
    marks: Vec<Mark>,
    sidebar: Option<Tab>,
}

impl PdfState {
    /// The remembered pages, the open document's included, to save.
    pub fn marks(&self) -> Vec<Mark> {
        let mut m = self.marks.clone();
        if let Some(o) = self.open.as_ref().filter(|o| o.status == Status::Ready) {
            remember(&mut m, o.mark());
        }
        m
    }

    pub fn set_marks(&mut self, marks: Vec<Mark>) {
        self.marks = marks;
        self.marks.truncate(REMEMBER);
    }

    /// Closes the document unless it belongs to `viewer`.
    pub fn keep_only(&mut self, viewer: Option<&str>) {
        if self.open.as_ref().is_some_and(|o| viewer != Some(o.id.as_str())) {
            self.close();
        }
    }

    fn close(&mut self) {
        if let Some(o) = self.open.take()
            && o.status == Status::Ready
        {
            remember(&mut self.marks, o.mark());
        }
    }

    fn ensure(&mut self, ctx: &egui::Context, lib: &Library, item: &Item) {
        if self.open.as_ref().is_some_and(|o| o.id == item.id) {
            return;
        }
        self.close();
        let mark = self.marks.iter().find(|m| m.id == item.id);
        self.open = Some(Open::new(ctx, item, item.original_path(lib.root()), mark));
    }
}

pub(super) fn is_pdf(item: &Item) -> bool {
    item.ext.eq_ignore_ascii_case("pdf")
}

// ---------------------------------------------------------------------------
// Drawing

impl App {
    /// The reader's keys, read before the app's own so ⌘F, ⌘C, zoom and
    /// the scrolling keys act on the document.
    pub(super) fn pdf_keys(&mut self, ctx: &egui::Context) {
        let Some(viewer) = self.ws.viewer.as_deref() else { return };
        let Some(o) = self.pdf.open.as_mut().filter(|o| o.id == viewer && o.status == Status::Ready) else { return };
        if self.prefs_window.open {
            return;
        }
        let focused = ctx.memory(|m| m.focused());
        let find_focused = focused == Some(Id::unique(FIND_ID));
        if focused.is_some() && !find_focused {
            return;
        }
        let mut acts = Vec::new();
        let mut copy = false;
        let find_open = o.find.open;
        let has_selection = !o.sel.text.is_empty();
        ctx.input_mut(|i| {
            i.events.retain(|e| match e {
                Event::Copy if !find_focused && has_selection => {
                    copy = true;
                    false
                }
                Event::Key { key, pressed: true, modifiers, .. } => match key_action(*key, *modifiers, find_focused, find_open) {
                    Some(a) => {
                        acts.push(a);
                        false
                    }
                    None => true,
                },
                _ => true,
            })
        });
        if copy {
            ctx.copy_text(o.sel.text.clone());
        }
        for a in acts {
            if a == PdfKey::CloseFind && find_focused {
                ctx.memory_mut(|m| m.surrender_focus(Id::unique(FIND_ID)));
            }
            o.act(a);
        }
    }

    /// Draws the reader for a PDF item and returns true; false for anything
    /// else (and off macOS, where the image viewer shows the preview).
    pub(super) fn pdf_viewer(&mut self, parent: &mut Ui, lib: &Library, item: &Item, screen: Rect) -> bool {
        let ctx = &parent.ctx().clone();
        if !cfg!(target_os = "macos") || !is_pdf(item) {
            return false;
        }
        self.pdf.ensure(ctx, lib, item);
        {
            let ui = &mut super::viewer_ui(parent, screen, "pdf-viewer");
            ui.painter().rect_filled(screen, 0.0, t::p().main);
            let bar = Rect::from_min_size(screen.min, vec2(screen.width(), t::BAR_HEIGHT));
            let mut area = Rect::from_min_max(Pos2::new(screen.left(), bar.bottom()), screen.max);
            ui.interact(area, ui.scope_id().with("pdf-bg"), Sense::click());
            let sidebar = self.pdf.sidebar;
            if let Some(o) = self.pdf.open.as_mut() {
                o.poll(ctx);
                match o.status.clone() {
                    Status::Ready => {
                        let mut jobs = Vec::new();
                        if let Some(tab) = sidebar {
                            let side = Rect::from_min_max(area.min, Pos2::new(area.left() + SIDEBAR_W, area.bottom()));
                            area.min.x = side.right();
                            pages(o, ui, area, &mut jobs);
                            side_panel(o, ui, side, tab, &mut jobs);
                        } else {
                            pages(o, ui, area, &mut jobs);
                        }
                        o.want(jobs);
                        o.select_if_moved();
                        if o.find.open {
                            find_bar(o, ui, Pos2::new(screen.right() - 16.0, bar.bottom() + 8.0));
                        }
                        if o.changed.elapsed() < SETTLE * 2 {
                            ctx.request_repaint_after(SETTLE);
                        }
                    }
                    Status::Opening => {
                        let text = if o.icloud { "Downloading from iCloud…" } else { "Opening…" };
                        ui.painter().text(area.center(), Align2::CENTER_CENTER, text, font(13.0), t::p().text_muted);
                    }
                    Status::Failed(e) => {
                        ui.painter().text(area.center(), Align2::CENTER_CENTER, format!("Can't open this PDF: {e}"), font(13.0), t::p().text_muted);
                    }
                }
            }
            let (name_x, mut right) = super::media_view::bar_shell(self, ui, item, bar);
            if let Some(o) = self.pdf.open.as_mut().filter(|o| o.status == Status::Ready) {
                let (tab, left) = toolbar(o, ui, bar, right - 8.0, self.pdf.sidebar);
                self.pdf.sidebar = tab;
                right = left;
            }
            super::media_view::bar_name(ui, item, bar, name_x, right);
        }
        true
    }
}

/// The scrolling column of pages, with find hits and the selection drawn
/// on them; adds the page pictures it needs to `jobs`.
fn pages(o: &mut Open, ui: &mut Ui, area: Rect, jobs: &mut Vec<Job>) {
    // Pinch or ⌘-scroll zooms around the pointer.
    if let Some(pos) = ui.input(|i| i.pointer.hover_pos()).filter(|p| area.contains(*p)) {
        let z = ui.input(|i| i.zoom_delta());
        if z != 1.0 {
            o.zoom = Zoom::Percent((o.zoom_factor() * z).clamp(MIN_ZOOM, MAX_ZOOM));
            o.zoom_at = Some(pos - area.min);
        }
    }
    o.relayout(area.size());
    if let Some(p) = o.restore.take() {
        o.jump = Some(vec2(0.0, o.layout.top_of(p)));
    }
    let ppp = ui.ctx().pixels_per_point();
    let key = scale_key(o.scale * UNIT * ppp);
    let settled = o.changed.elapsed() >= SETTLE;
    let k = o.scale * UNIT;

    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area));
    child.set_clip_rect(area);
    let mut sa = egui::ScrollArea::both()
        .id_salt("pdf-pages")
        .auto_shrink([false, false])
        .scroll_source(egui::scroll_area::ScrollSource::SCROLL_BAR | egui::scroll_area::ScrollSource::MOUSE_WHEEL);
    if let Some(j) = o.jump.take() {
        sa = sa.scroll_offset(j);
    }
    let out = sa.show_viewport(&mut child, |ui, viewport| {
        let origin = ui.max_rect().min;
        let cw = o.layout.content_width(viewport.width());
        ui.set_min_size(vec2(cw, o.layout.height));
        let Some((a, b)) = o.layout.visible(viewport.top(), viewport.height()) else { return };
        let p = ui.painter().clone();
        let page_rect = |o: &Open, i: usize| Rect::from_min_size(origin + vec2(o.layout.left(i, cw), o.layout.tops[i]), o.layout.sizes[i]);
        for i in a..=b {
            let r = page_rect(o, i);
            p.rect_filled(r.expand(1.0), 0.0, t::p().divider);
            p.rect_filled(r, 0.0, Color32::WHITE);
            if let Some(tex) = o.pages.best(i, key) {
                p.image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            } else if o.broken.contains(&i) {
                p.text(r.center(), Align2::CENTER_CENTER, "Can't draw this page", font(12.0), Color32::from_gray(120));
            }
            let to_screen = |rc: &falcon_pdf::Rect| Rect::from_min_size(r.min + vec2(rc.x as f32 * k, rc.y as f32 * k), vec2(rc.w as f32 * k, rc.h as f32 * k));
            for (n, hit) in o.find.hits.iter().enumerate().filter(|(_, h)| h.page == i) {
                let color = if o.find.current == Some(n) { Color32::from_rgba_unmultiplied(255, 140, 0, 130) } else { Color32::from_rgba_unmultiplied(255, 220, 0, 90) };
                for rc in &hit.rects {
                    p.rect_filled(to_screen(rc).expand(1.0), CornerRadius::same(2), color);
                }
            }
            if o.sel.active && o.sel.page == i {
                for rc in &o.sel.rects {
                    p.rect_filled(to_screen(rc), 0.0, t::p().primary.gamma_multiply(0.35));
                }
            }
        }
        // The visible pages first, then one either side; a zoom that is
        // still moving only fills pages that have no picture at all.
        let n = o.count();
        let order = (a..=b).chain(a.checked_sub(1)).chain((b + 1 < n).then_some(b + 1));
        for i in order {
            if !o.broken.contains(&i) && !o.pages.contains((i, key)) && (settled || !o.pages.has_page(i)) {
                jobs.push(Job { page: i, key });
            }
        }

        // Dragging over a page selects its text; a click clears it.
        let screen_view = Rect::from_min_size(origin + viewport.min.to_vec2(), viewport.size());
        let resp = ui.interact(screen_view, ui.scope_id().with("pdf-select"), Sense::click_and_drag());
        let at = |o: &Open, pos: Pos2, page: Option<usize>| -> Option<(usize, (f64, f64))> {
            let c = pos - origin;
            let i = page.or_else(|| o.layout.page_at(c.y))?;
            let (w, h) = o.sizes[i];
            let x = ((c.x - o.layout.left(i, cw)) / k).clamp(0.0, w);
            let y = ((c.y - o.layout.tops[i]) / k).clamp(0.0, h);
            Some((i, (f64::from(x), f64::from(y))))
        };
        if resp.hovered()
            && let Some(pos) = resp.hover_pos()
            && o.layout.page_at((pos - origin).y).is_some()
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
        }
        // The selection starts where the button went down, not where the
        // drag was recognised a few points later.
        if resp.drag_started()
            && let Some(pos) = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos())
        {
            o.sel = match at(o, pos, None) {
                Some((page, d)) => Selecting { active: true, page, from: d, to: d, serial: o.sel.serial, ..Default::default() },
                None => Selecting { serial: o.sel.serial, ..Default::default() },
            };
        }
        if resp.dragged()
            && o.sel.active
            && let Some(pos) = resp.interact_pointer_pos()
            && let Some((_, d)) = at(o, pos, Some(o.sel.page))
        {
            o.sel.to = d;
        }
        if resp.clicked() {
            o.sel = Selecting { serial: o.sel.serial, ..Default::default() };
        }
    });
    o.offset = out.state.clamped_offset();
    o.current = o.layout.current(o.offset.y, area.height());
}

/// Thumbnails or the outline, down the left side.
fn side_panel(o: &mut Open, ui: &mut Ui, side: Rect, tab: Tab, jobs: &mut Vec<Job>) {
    let p = ui.painter().clone();
    p.rect_filled(side, 0.0, t::p().panel);
    p.line_segment([side.right_top(), side.right_bottom()], Stroke::new(1.0, t::p().divider));
    let list = Rect::from_min_max(side.min + vec2(0.0, 6.0), side.max);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list));
    child.set_clip_rect(list);
    match tab {
        Tab::Thumbnails => {
            let widest = o.sizes.iter().map(|s| s.0).fold(1.0f32, f32::max);
            let tl = layout(&o.sizes, THUMB_W / (widest * UNIT), 26.0);
            let current = o.current;
            let mut go = None;
            egui::ScrollArea::vertical().id_salt("pdf-thumbs").auto_shrink([false, false]).show_viewport(&mut child, |ui, viewport| {
                let origin = ui.max_rect().min;
                ui.set_min_size(vec2(list.width(), tl.height));
                let Some((a, b)) = tl.visible(viewport.top(), viewport.height()) else { return };
                for i in a..=b {
                    let r = Rect::from_min_size(origin + vec2((list.width() - tl.sizes[i].x) / 2.0, tl.tops[i]), tl.sizes[i]);
                    let resp = ui.interact(r.expand(4.0), ui.scope_id().with(("thumb", i)), Sense::click());
                    let p = ui.painter();
                    p.rect_filled(r, 0.0, Color32::WHITE);
                    match o.thumbs.best(i, THUMB_KEY) {
                        Some(tex) => {
                            p.image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                        }
                        None => jobs.push(Job { page: i, key: THUMB_KEY }),
                    }
                    let stroke = if i == current { Stroke::new(2.0, t::p().primary) } else { Stroke::new(1.0, t::p().divider) };
                    p.rect_stroke(r.expand(2.0), 2.0, stroke, egui::StrokeKind::Outside);
                    p.text(Pos2::new(r.center().x, r.bottom() + 12.0), Align2::CENTER_CENTER, format!("{}", i + 1), font(11.0), t::p().text_muted);
                    if resp.clicked() {
                        go = Some(i);
                    }
                }
            });
            if let Some(i) = go {
                o.go_to(i);
            }
        }
        Tab::Outline => {
            let mut go = None;
            egui::ScrollArea::vertical().id_salt("pdf-outline").auto_shrink([false, false]).show(&mut child, |ui| {
                if o.outline.is_empty() {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("No outline").font(font(12.0)).color(t::p().text_muted));
                }
                for (n, e) in o.outline.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add_space(8.0 + 12.0 * e.depth.min(6) as f32);
                        let label = egui::Label::new(egui::RichText::new(&e.title).font(font(12.0)).color(t::p().text)).truncate().sense(Sense::click());
                        let resp = ui.push_id(n, |ui| ui.add(label)).inner;
                        if resp.clicked()
                            && let Some(p) = e.page
                        {
                            go = Some(p);
                        }
                        if e.page.is_some() {
                            resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                        }
                    });
                }
            });
            if let Some(p) = go {
                o.go_to(p);
            }
        }
    }
}

/// The reader's controls in the viewer bar, leftwards from `right`:
/// sidebar and find toggles, page navigation and zoom. Returns the
/// sidebar choice and the x left of the controls.
fn toolbar(o: &mut Open, ui: &mut Ui, bar: Rect, right: f32, mut sidebar: Option<Tab>) -> (Option<Tab>, f32) {
    let cy = bar.center().y;
    let mut x = right - 8.0;
    let p = ui.painter().clone();
    if icon_button(ui, Pos2::new(x, cy), ic::PLUS, 15.0, o.zoom_factor() < MAX_ZOOM, "Zoom In (⌘+)").clicked() {
        o.act(PdfKey::ZoomIn);
    }
    x -= 70.0;
    let menu = Rect::from_center_size(Pos2::new(x, cy), vec2(100.0, 24.0));
    let mut mui = ui.new_child(egui::UiBuilder::new().max_rect(menu));
    let shown = match o.zoom {
        Zoom::Percent(_) => Zoom::Percent(o.zoom_factor()).label(),
        z => z.label(),
    };
    egui::containers::menu::MenuButton::new(egui::RichText::new(shown).font(font(12.0))).ui(&mut mui, |ui| {
        for z in [Zoom::Auto, Zoom::Actual, Zoom::PageFit, Zoom::PageWidth].into_iter().chain(Zoom::PRESETS.iter().map(|&p| Zoom::Percent(p))) {
            if ui.selectable_label(o.zoom == z, z.label()).clicked() {
                o.zoom = z;
                ui.close();
            }
        }
    });
    x -= 70.0;
    if icon_button(ui, Pos2::new(x, cy), ic::MINUS, 15.0, o.zoom_factor() > MIN_ZOOM, "Zoom Out (⌘−)").clicked() {
        o.act(PdfKey::ZoomOut);
    }
    x -= 24.0;
    p.line_segment([Pos2::new(x, cy - 8.0), Pos2::new(x, cy + 8.0)], Stroke::new(1.0, t::p().divider));

    // Page number field and count.
    let count = format!("of {}", o.count());
    let g = ui.painter().layout_no_wrap(count.clone(), font(12.5), t::p().text_muted);
    x -= 10.0 + g.size().x;
    p.galley(Pos2::new(x, cy - g.size().y / 2.0), g, t::p().text_muted);
    let field = Rect::from_center_size(Pos2::new(x - 28.0, cy), vec2(44.0, 22.0));
    let id = Id::unique(PAGE_ID);
    let editing = ui.ctx().memory(|m| m.has_focus(id));
    if !editing {
        o.page_text = (o.current + 1).to_string();
    }
    let mut fui = ui.new_child(egui::UiBuilder::new().max_rect(field));
    let resp = fui.add(TextEdit::singleline(&mut o.page_text).id(id).font(mono(12.5)).align(Align2::CENTER_CENTER).desired_width(field.width()));
    if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))
        && let Some(p) = parse_page(&o.page_text, o.count())
    {
        o.go_to(p);
    }
    x = field.left() - 18.0;
    if icon_button(ui, Pos2::new(x, cy), PAGE_DOWN, 15.0, o.current + 1 < o.count(), "Next Page (N)").clicked() {
        o.act(PdfKey::NextPage);
    }
    x -= 26.0;
    if icon_button(ui, Pos2::new(x, cy), PAGE_UP, 15.0, o.current > 0, "Previous Page (P)").clicked() {
        o.act(PdfKey::PrevPage);
    }
    x -= 22.0;
    p.line_segment([Pos2::new(x, cy - 8.0), Pos2::new(x, cy + 8.0)], Stroke::new(1.0, t::p().divider));
    x -= 22.0;
    if o.find.open {
        p.rect_filled(Rect::from_center_size(Pos2::new(x, cy), Vec2::splat(26.0)), t::RADIUS, t::p().primary.gamma_multiply(0.35));
    }
    if icon_button(ui, Pos2::new(x, cy), ic::MAGNIFYING_GLASS, 16.0, true, "Find in Document (⌘F)").clicked() {
        if o.find.open { o.act(PdfKey::CloseFind) } else { o.act(PdfKey::Find) }
    }
    x -= 28.0;
    if sidebar.is_some() {
        p.rect_filled(Rect::from_center_size(Pos2::new(x, cy), Vec2::splat(26.0)), t::RADIUS, t::p().primary.gamma_multiply(0.35));
    }
    let sresp = icon_button(ui, Pos2::new(x, cy), ic::SIDEBAR_SIMPLE, 16.0, true, "Thumbnails and Outline");
    if sresp.clicked() {
        sidebar = if sidebar.is_some() { None } else { Some(Tab::Thumbnails) };
    }
    if sidebar.is_some() {
        // Which list the sidebar shows, from the button's menu.
        sresp.context_menu(|ui| {
            if ui.selectable_label(sidebar == Some(Tab::Thumbnails), "Thumbnails").clicked() {
                sidebar = Some(Tab::Thumbnails);
                ui.close();
            }
            if ui.selectable_label(sidebar == Some(Tab::Outline), "Outline").clicked() {
                sidebar = Some(Tab::Outline);
                ui.close();
            }
        });
    }
    (sidebar, x - 16.0)
}

/// The find bar under the top bar's right end: the query, previous/next,
/// match case and the hit count.
fn find_bar(o: &mut Open, ui: &mut Ui, top_right: Pos2) {
    let size = vec2(420.0, 36.0);
    let r = Rect::from_min_size(Pos2::new(top_right.x - size.x, top_right.y), size);
    ui.interact(r, ui.scope_id().with("pdf-find-bg"), Sense::click());
    let p = ui.painter().clone();
    p.rect_filled(r, CornerRadius::same(8), t::p().panel);
    p.rect_stroke(r, CornerRadius::same(8), Stroke::new(1.0, t::p().divider), egui::StrokeKind::Inside);
    let field = Rect::from_min_size(r.min + vec2(8.0, 7.0), vec2(170.0, 22.0));
    let mut fui = ui.new_child(egui::UiBuilder::new().max_rect(field));
    let resp = fui.add(TextEdit::singleline(&mut o.find.text).id(Id::unique(FIND_ID)).hint_text("Find in document").font(font(12.5)).desired_width(field.width()));
    if std::mem::take(&mut o.find.focus) {
        resp.request_focus();
    }
    o.search_if_changed();
    let cy = r.center().y;
    let mut x = field.right() + 18.0;
    let has = !o.find.hits.is_empty();
    if icon_button(ui, Pos2::new(x, cy), PAGE_UP, 14.0, has, "Previous Match (⇧⌘G)").clicked() {
        o.act(PdfKey::FindNext(false));
    }
    x += 26.0;
    if icon_button(ui, Pos2::new(x, cy), PAGE_DOWN, 14.0, has, "Next Match (⌘G)").clicked() {
        o.act(PdfKey::FindNext(true));
    }
    x += 18.0;
    let case = Rect::from_min_size(Pos2::new(x, cy - 10.0), vec2(56.0, 20.0));
    let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(case));
    cui.checkbox(&mut o.find.match_case, egui::RichText::new("Aa").font(font(12.0))).on_hover_text("Match Case");
    x = case.right() + 4.0;
    let status = if o.find.searching {
        "Searching…".to_string()
    } else if o.find.text.is_empty() {
        String::new()
    } else if !has {
        "No matches".to_string()
    } else {
        format!("{} of {}", o.find.current.map_or(0, |c| c + 1), o.find.hits.len())
    };
    p.text(Pos2::new(x, cy), Align2::LEFT_CENTER, status, font(12.0), t::p().text_muted);
    if icon_button(ui, Pos2::new(r.right() - 16.0, cy), ic::X, 13.0, true, "Close (Esc)").clicked() {
        o.act(PdfKey::CloseFind);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LETTER: (f32, f32) = (612.0, 792.0);

    #[test]
    fn zoom_modes_resolve_like_pdfjs() {
        let view = vec2(816.0 + 2.0 * PAD + SCROLLBAR, 600.0);
        // The view fits a letter page's width exactly at 100%.
        assert!((Zoom::PageWidth.resolve(LETTER, view) - 1.0).abs() < 1e-4);
        assert!((Zoom::Auto.resolve(LETTER, view) - 1.0).abs() < 1e-4);
        let fit = Zoom::PageFit.resolve(LETTER, view);
        assert!((fit - (600.0 - 2.0 * PAD) / (792.0 * UNIT)).abs() < 1e-4);
        assert_eq!(Zoom::Actual.resolve(LETTER, view), 1.0);
        assert_eq!(Zoom::Percent(2.5).resolve(LETTER, view), 2.5);
        // Automatic never goes past 125%, and fits landscape pages whole.
        let wide = vec2(3000.0, 2000.0);
        assert_eq!(Zoom::Auto.resolve(LETTER, wide), MAX_AUTO);
        let land = (792.0, 612.0);
        assert!((Zoom::Auto.resolve(land, view) - Zoom::PageFit.resolve(land, view).min(MAX_AUTO)).abs() < 1e-4);
        // Degenerate pages and views stay within limits.
        assert_eq!(Zoom::PageFit.resolve((0.0, 0.0), view), 1.0);
        assert_eq!(Zoom::PageFit.resolve(LETTER, vec2(0.0, 0.0)), MIN_ZOOM);
        assert_eq!(Zoom::Percent(50.0).resolve(LETTER, view), MAX_ZOOM);
    }

    #[test]
    fn zoom_keys_round_trip() {
        for z in [Zoom::Auto, Zoom::PageFit, Zoom::PageWidth, Zoom::Actual, Zoom::Percent(1.5)] {
            assert_eq!(Zoom::from_key(&z.key()), z);
        }
        assert_eq!(Zoom::from_key("nonsense"), Zoom::Auto);
        assert_eq!(Zoom::from_key("-1"), Zoom::Auto);
        assert_eq!(Zoom::from_key("99"), Zoom::Percent(MAX_ZOOM));
        assert_eq!(Zoom::Percent(1.5).label(), "150%");
    }

    #[test]
    fn zoom_steps_move_and_stop() {
        assert_eq!(zoom_step(1.0, true), 1.1);
        assert_eq!(zoom_step(1.0, false), 0.91);
        assert_eq!(zoom_step(0.1, true), 0.11);
        assert_eq!(zoom_step(0.1, false), MIN_ZOOM);
        assert_eq!(zoom_step(10.0, true), MAX_ZOOM);
        assert!(zoom_step(0.104, false) < 0.104 || zoom_step(0.104, false) == MIN_ZOOM);
    }

    #[test]
    fn pages_stack_with_gaps() {
        let l = layout(&[LETTER, (792.0, 612.0), LETTER], 0.75, GAP);
        // At 75% a letter page is 612 × 792 screen points.
        assert_eq!(l.sizes[0], vec2(612.0, 792.0));
        assert_eq!(l.tops, vec![PAD, PAD + 792.0 + GAP, PAD + 792.0 + GAP + 612.0 + GAP]);
        assert_eq!(l.width, 792.0);
        assert_eq!(l.height, PAD + 792.0 + GAP + 612.0 + GAP + 792.0 + PAD);
        assert_eq!(layout(&[], 1.0, GAP).height, 2.0 * PAD);
        // Pages centre in a wider view.
        assert_eq!(l.left(0, 1000.0), 194.0);
        assert_eq!(l.left(0, 100.0), PAD);
        assert_eq!(l.content_width(500.0), 792.0 + 2.0 * PAD);
        assert_eq!(l.top_of(1), 792.0 + GAP);
        assert_eq!(l.top_of(9), 0.0);
    }

    #[test]
    fn visible_range_and_current_page() {
        // Ten pages 100 tall: page i spans PAD + 110 i .. + 100.
        let l = layout(&[(75.0, 75.0); 10], 1.0, GAP);
        assert_eq!(l.sizes[0].y, 100.0);
        assert_eq!(l.visible(0.0, 150.0), Some((0, 1)));
        assert_eq!(l.visible(PAD + 105.0, 10.0), Some((1, 1)), "a view inside the gap counts the next page");
        assert_eq!(l.visible(PAD + 110.0 * 4.0 + 50.0, 300.0), Some((4, 7)));
        assert_eq!(l.visible(5000.0, 100.0), None);
        assert_eq!(layout(&[], 1.0, GAP).visible(0.0, 100.0), None);
        // The page showing most wins.
        assert_eq!(l.current(0.0, 150.0), 0);
        assert_eq!(l.current(PAD + 80.0, 150.0), 1);
        assert_eq!(l.current(5000.0, 100.0), 9);
        assert_eq!(l.page_at(PAD + 50.0), Some(0));
        assert_eq!(l.page_at(PAD + 105.0), None);
        assert_eq!(l.page_at(PAD + 115.0), Some(1));
        assert_eq!(l.page_at(-5.0), None);
    }

    #[test]
    fn zooming_keeps_the_reading_position() {
        let pages = vec![LETTER; 5];
        let a = layout(&pages, 1.0, GAP);
        let b = layout(&pages, 2.0, GAP);
        // Halfway down page 3 stays halfway down page 3.
        let y = a.tops[2] + a.sizes[2].y / 2.0;
        let y2 = anchor(&a, &b, y);
        assert!((y2 - (b.tops[2] + b.sizes[2].y / 2.0)).abs() < 1e-3);
        assert_eq!(anchor(&a, &a, 123.0), 123.0);
        assert_eq!(anchor(&Layout::default(), &b, 50.0), 0.0);
    }

    #[test]
    fn scale_keys_round_up() {
        assert_eq!(scale_key(1.0), 0);
        assert_eq!(scale_key(2.0), 16);
        assert_eq!(scale_key(0.5), -16);
        for s in [0.3f32, 1.0, 1.7, 2.66, 13.3] {
            let back = key_scale(scale_key(s));
            assert!(back >= f64::from(s) * 0.9999 && back < f64::from(s) * 1.05, "{s} -> {back}");
        }
    }

    #[test]
    fn page_numbers_clamp() {
        assert_eq!(parse_page("3", 10), Some(2));
        assert_eq!(parse_page(" 1 ", 10), Some(0));
        assert_eq!(parse_page("0", 10), Some(0));
        assert_eq!(parse_page("-4", 10), Some(0));
        assert_eq!(parse_page("99", 10), Some(9));
        assert_eq!(parse_page("x", 10), None);
        assert_eq!(parse_page("1", 0), None);
    }

    #[test]
    fn find_navigation_wraps() {
        assert_eq!(step_hit(None, 0, true), None);
        assert_eq!(step_hit(None, 3, true), Some(0));
        assert_eq!(step_hit(None, 3, false), Some(2));
        assert_eq!(step_hit(Some(2), 3, true), Some(0));
        assert_eq!(step_hit(Some(0), 3, false), Some(2));
        assert_eq!(step_hit(Some(1), 3, true), Some(2));
        assert_eq!(first_hit_from(&[0, 2, 2, 5], 2), Some(1));
        assert_eq!(first_hit_from(&[0, 2, 2, 5], 3), Some(3));
        assert_eq!(first_hit_from(&[0, 2, 2, 5], 6), Some(0));
        assert_eq!(first_hit_from(&[], 0), None);
    }

    #[test]
    fn cache_evicts_least_recently_used() {
        let mut c = PageCache::new(100);
        c.insert((0, 0), "a", 40);
        c.insert((1, 0), "b", 40);
        assert_eq!(c.used(), 80);
        // Using page 0 makes page 1 the oldest.
        assert_eq!(c.best(0, 0), Some(&"a"));
        c.insert((2, 0), "c", 40);
        assert!(c.contains((0, 0)) && !c.contains((1, 0)) && c.contains((2, 0)));
        assert_eq!(c.used(), 80);
        // Replacing an entry counts its pixels once.
        c.insert((2, 0), "c2", 50);
        assert_eq!(c.used(), 90);
        assert_eq!(c.len(), 2);
        // One picture over the whole budget is still kept, alone.
        c.insert((3, 0), "huge", 500);
        assert_eq!(c.len(), 1);
        assert!(c.contains((3, 0)));
        assert_eq!(c.used(), 500);
    }

    #[test]
    fn cache_falls_back_to_the_nearest_scale() {
        let mut c = PageCache::new(1000);
        c.insert((4, 0), "small", 10);
        c.insert((4, 16), "big", 10);
        c.insert((5, 8), "other", 10);
        assert_eq!(c.best(4, 16), Some(&"big"));
        assert_eq!(c.best(4, 3), Some(&"small"));
        assert_eq!(c.best(4, 12), Some(&"big"));
        // Equally near: the sharper one.
        assert_eq!(c.best(4, 8), Some(&"big"));
        assert_eq!(c.best(6, 0), None);
        assert!(c.has_page(5) && !c.has_page(6));
    }

    #[test]
    fn marks_keep_the_newest_twenty() {
        let mut m = Vec::new();
        for n in 0..25 {
            remember(&mut m, Mark { id: format!("I{n}"), page: n, zoom: "auto".into() });
        }
        assert_eq!(m.len(), REMEMBER);
        assert_eq!(m[0].id, "I5");
        remember(&mut m, Mark { id: "I10".into(), page: 99, zoom: "1.5".into() });
        assert_eq!(m.len(), REMEMBER);
        assert_eq!(m.last().unwrap().page, 99);
        assert_eq!(m.iter().filter(|x| x.id == "I10").count(), 1);
    }

    #[test]
    fn keys_follow_pdfjs() {
        let none = Modifiers::NONE;
        let cmd = Modifiers::COMMAND;
        let shift_cmd = Modifiers { shift: true, ..Modifiers::COMMAND };
        assert_eq!(key_action(Key::N, none, false, false), Some(PdfKey::NextPage));
        assert_eq!(key_action(Key::J, none, false, false), Some(PdfKey::NextPage));
        assert_eq!(key_action(Key::K, none, false, false), Some(PdfKey::PrevPage));
        assert_eq!(key_action(Key::Home, none, false, false), Some(PdfKey::First));
        assert_eq!(key_action(Key::ArrowDown, cmd, false, false), Some(PdfKey::Last));
        assert_eq!(key_action(Key::ArrowDown, none, false, false), Some(PdfKey::Lines(1.0)));
        assert_eq!(key_action(Key::Equals, cmd, false, false), Some(PdfKey::ZoomIn));
        assert_eq!(key_action(Key::Num0, cmd, false, false), Some(PdfKey::ZoomReset));
        assert_eq!(key_action(Key::F, cmd, false, false), Some(PdfKey::Find));
        assert_eq!(key_action(Key::G, shift_cmd, false, false), Some(PdfKey::FindNext(false)));
        // Left and right move between items, Space closes, Esc closes the
        // viewer unless the find bar is open.
        assert_eq!(key_action(Key::ArrowLeft, none, false, false), None);
        assert_eq!(key_action(Key::ArrowRight, none, false, false), None);
        assert_eq!(key_action(Key::Space, none, false, false), None);
        assert_eq!(key_action(Key::Escape, none, false, false), None);
        assert_eq!(key_action(Key::Escape, none, false, true), Some(PdfKey::CloseFind));
        // In the find field, letters type and Enter steps through hits.
        assert_eq!(key_action(Key::N, none, true, true), None);
        assert_eq!(key_action(Key::ArrowLeft, none, true, true), None);
        assert_eq!(key_action(Key::Enter, none, true, true), Some(PdfKey::FindNext(true)));
        assert_eq!(key_action(Key::Enter, Modifiers::SHIFT, true, true), Some(PdfKey::FindNext(false)));
        assert_eq!(key_action(Key::Escape, none, true, true), Some(PdfKey::CloseFind));
    }

    #[test]
    fn local_files_are_not_icloud_only() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.pdf");
        std::fs::write(&f, b"%PDF").unwrap();
        assert!(!icloud_only(&f));
        assert!(!icloud_only(&dir.path().join("missing.pdf")));
    }
}
