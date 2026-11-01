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
use falcon_pdf::{Document, Hit, Link, LinkTarget, OutlineEntry, SearchOptions};

use crate::app::App;
use crate::ui::widgets::icon_button;
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

/// pdf.js's scroll modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ScrollMode {
    /// One page (or spread) at a time.
    Page,
    #[default]
    Vertical,
    Horizontal,
    /// Pages left to right, wrapping into rows.
    Wrapped,
}

/// pdf.js's spread modes: pages side by side in pairs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Spread {
    #[default]
    None,
    /// Pairs starting with page 1: 1–2, 3–4, …
    Odd,
    /// Page 1 alone, then 2–3, 4–5, …
    Even,
}

/// The spreads of `n` pages, as index ranges, in order.
pub fn spreads(n: usize, spread: Spread) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut i = 0;
    if spread == Spread::Even && n > 0 {
        out.push(0..1);
        i = 1;
    }
    while i < n {
        let end = if spread == Spread::None { i + 1 } else { (i + 2).min(n) };
        out.push(i..end);
        i = end;
    }
    out
}

/// Space between the two pages of a spread, in screen points.
const SPREAD_GAP: f32 = 2.0;

/// How the pages are arranged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arrange {
    pub scroll: ScrollMode,
    pub spread: Spread,
    /// The view's width, for wrapping.
    pub view_w: f32,
    /// In page mode, the page whose spread shows.
    pub page: usize,
}

impl Default for Arrange {
    fn default() -> Self {
        Arrange { scroll: ScrollMode::Vertical, spread: Spread::None, view_w: 0.0, page: 0 }
    }
}

/// Where each page sits in the scrolling content, in screen points.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub tops: Vec<f32>,
    /// Left edges in content exactly `width + 2 PAD` wide; wider content
    /// centres everything (see [`Layout::left`]).
    pub lefts: Vec<f32>,
    pub sizes: Vec<Vec2>,
    /// The widest row (or column) of pages.
    pub width: f32,
    /// The whole content, padding included.
    pub height: f32,
    pub scroll: ScrollMode,
    /// One page per row, top to bottom: lookups can search by height.
    pub column: bool,
}

/// Lays out pages of `pages` points at `zoom` in one column, `gap` points
/// apart (the reader's default, and the thumbnail list).
pub fn layout(pages: &[(f32, f32)], zoom: f32, gap: f32) -> Layout {
    arrange(pages, zoom, gap, Arrange::default())
}

/// Lays out pages of `pages` points at `zoom` by pdf.js's scroll and
/// spread modes: spreads sit side by side, centred on each other; rows are
/// centred. In page mode only `how.page`'s spread is placed, the other
/// pages get no size.
pub fn arrange(pages: &[(f32, f32)], zoom: f32, gap: f32, how: Arrange) -> Layout {
    let n = pages.len();
    let size = |i: usize| vec2(pages[i].0.max(0.0) * UNIT * zoom, pages[i].1.max(0.0) * UNIT * zoom);
    let column = how.scroll == ScrollMode::Vertical && how.spread == Spread::None;
    let sizes: Vec<Vec2> = (0..n).map(size).collect();
    let mut l = Layout { tops: vec![0.0; n], lefts: vec![0.0; n], sizes: sizes.clone(), scroll: how.scroll, column, ..Default::default() };
    let groups = spreads(n, how.spread);
    // Each spread's size, with its pages side by side.
    let span = |g: &std::ops::Range<usize>| {
        let w: f32 = g.clone().map(|i| sizes[i].x).sum::<f32>() + SPREAD_GAP * (g.len().saturating_sub(1)) as f32;
        let h = g.clone().map(|i| sizes[i].y).fold(0.0, f32::max);
        vec2(w, h)
    };
    // Places spread `g` with its top-left at (x, y) in a line `line_h` tall.
    let place = |l: &mut Layout, g: &std::ops::Range<usize>, x: f32, y: f32, line_h: f32| {
        let mut x = x;
        for i in g.clone() {
            l.lefts[i] = x;
            l.tops[i] = y + (line_h - sizes[i].y) / 2.0;
            x += sizes[i].x + SPREAD_GAP;
        }
    };
    // Lines of spreads: one per spread, all in one, or wrapped to the view.
    let lines: Vec<Vec<usize>> = match how.scroll {
        ScrollMode::Vertical => (0..groups.len()).map(|g| vec![g]).collect(),
        ScrollMode::Horizontal => vec![(0..groups.len()).collect()],
        ScrollMode::Page => groups.iter().position(|g| g.contains(&how.page.min(n.saturating_sub(1)))).map(|g| vec![vec![g]]).unwrap_or_default(),
        ScrollMode::Wrapped => {
            let room = (how.view_w - 2.0 * PAD - SCROLLBAR).max(1.0);
            let mut lines: Vec<Vec<usize>> = Vec::new();
            let mut used = 0.0;
            for (k, g) in groups.iter().enumerate() {
                let w = span(g).x;
                match lines.last_mut() {
                    Some(line) if used + gap + w <= room => {
                        line.push(k);
                        used += gap + w;
                    }
                    _ => {
                        lines.push(vec![k]);
                        used = w;
                    }
                }
            }
            lines
        }
    };
    let line_size = |line: &[usize]| {
        let sizes: Vec<Vec2> = line.iter().map(|&k| span(&groups[k])).collect();
        let w = sizes.iter().map(|s| s.x).sum::<f32>() + gap * line.len().saturating_sub(1) as f32;
        vec2(w, sizes.iter().map(|s| s.y).fold(0.0, f32::max))
    };
    l.width = lines.iter().map(|line| line_size(line).x).fold(0.0, f32::max);
    let mut y = PAD;
    for line in &lines {
        let ls = line_size(line);
        let mut x = PAD + (l.width - ls.x) / 2.0;
        for &k in line {
            let g = &groups[k];
            place(&mut l, g, x, y, ls.y);
            x += span(g).x + gap;
        }
        y += ls.y + gap;
    }
    l.height = if lines.is_empty() { 2.0 * PAD } else { y - gap + PAD };
    // In page mode the pages not shown sit out of reach.
    if how.scroll == ScrollMode::Page {
        let shown: Vec<usize> = lines.iter().flatten().flat_map(|&k| groups[k].clone()).collect();
        for i in (0..n).filter(|i| !shown.contains(i)) {
            l.sizes[i] = Vec2::ZERO;
            l.tops[i] = -1e9;
            l.lefts[i] = -1e9;
        }
    }
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
        self.lefts[i] + ((content_w - self.width - 2.0 * PAD) / 2.0).max(0.0)
    }

    /// The scroll offset that puts page `i`'s top edge at the top of the view.
    pub fn top_of(&self, i: usize) -> f32 {
        self.tops.get(i).map_or(0.0, |t| (t - PAD).max(0.0))
    }

    /// Page `i`'s rectangle in content `content_w` wide.
    pub fn rect(&self, i: usize, content_w: f32) -> Rect {
        Rect::from_min_size(Pos2::new(self.left(i, content_w), self.tops[i]), self.sizes[i])
    }

    /// The first and last page with any part in `view` (content
    /// coordinates), in any scroll mode.
    pub fn visible_in(&self, view: Rect, content_w: f32) -> Option<(usize, usize)> {
        if self.column {
            return self.visible(view.top(), view.height());
        }
        let mut hit = (0..self.tops.len()).filter(|&i| self.sizes[i].x > 0.0 && self.rect(i, content_w).intersects(view));
        let first = hit.next()?;
        Some((first, hit.next_back().unwrap_or(first)))
    }

    /// The page showing the most in `view`: the current page.
    pub fn current_in(&self, view: Rect, content_w: f32) -> usize {
        if self.column {
            return self.current(view.top(), view.height());
        }
        let Some((a, b)) = self.visible_in(view, content_w) else { return self.tops.len().saturating_sub(1) };
        let shown = |i: usize| {
            let r = self.rect(i, content_w).intersect(view);
            if r.is_positive() { r.area() } else { 0.0 }
        };
        (a..=b).fold(a, |best, i| if shown(i) > shown(best) + 0.5 { i } else { best })
    }

    /// The page under a content position, if not in a gap.
    pub fn page_at_pos(&self, pos: Pos2, content_w: f32) -> Option<usize> {
        if self.column {
            return self.page_at(pos.y);
        }
        (0..self.tops.len()).find(|&i| self.rect(i, content_w).contains(pos))
    }

    /// The scroll offset that brings page `i` into view from `offset`: its
    /// top edge to the top in a column or rows, its left edge to the left
    /// in a row, the start in page mode.
    pub fn offset_for(&self, i: usize, offset: Vec2, content_w: f32) -> Vec2 {
        if i >= self.tops.len() {
            return offset;
        }
        match self.scroll {
            ScrollMode::Page => Vec2::ZERO,
            ScrollMode::Horizontal => vec2((self.left(i, content_w) - PAD).max(0.0), offset.y),
            ScrollMode::Vertical | ScrollMode::Wrapped => vec2(offset.x, self.top_of(i)),
        }
    }
}

/// The horizontal offset in `new` that shows the same spot of the same
/// page as `offset` did in `old`, for a row of pages.
pub fn anchor_x(old: &Layout, new: &Layout, offset: f32) -> f32 {
    let n = old.lefts.len().min(new.lefts.len());
    if n == 0 {
        return 0.0;
    }
    let p = partition(n, |i| old.lefts[i] + old.sizes[i].x + GAP <= offset).min(n - 1);
    let frac = (offset - old.lefts[p]) / old.sizes[p].x.max(1.0);
    (new.lefts[p] + frac * new.sizes[p].x).max(0.0)
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

/// The outline entry for page `page`: the last entry pointing at it, else
/// the last one pointing at an earlier page (the section it is in).
pub fn current_outline(outline: &[OutlineEntry], page: usize) -> Option<usize> {
    let mut best: Option<(usize, usize)> = None;
    for (n, e) in outline.iter().enumerate() {
        if let Some(p) = e.page.filter(|&p| p <= page)
            && best.is_none_or(|(_, bp)| p >= bp)
        {
            best = Some((n, p));
        }
    }
    best.map(|b| b.0)
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
    /// The view's quarter turns and scroll and spread modes, as pdf.js's
    /// view history keeps them.
    #[serde(default)]
    pub turns: u8,
    #[serde(default)]
    pub scroll: ScrollMode,
    #[serde(default)]
    pub spread: Spread,
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

/// A point on a page `size` points big (as the file shows it) once the
/// page is turned `turns` quarter turns clockwise for viewing (pdf.js's
/// Rotate Clockwise, which only changes the view).
pub fn turn_point(p: (f64, f64), size: (f64, f64), turns: u8) -> (f64, f64) {
    let ((x, y), (w, h)) = (p, size);
    match turns % 4 {
        1 => (h - y, x),
        2 => (w - x, h - y),
        3 => (y, w - x),
        _ => (x, y),
    }
}

/// The inverse of [`turn_point`]: a point on the turned page back on the
/// page as the file shows it.
pub fn unturn_point(p: (f64, f64), size: (f64, f64), turns: u8) -> (f64, f64) {
    let ((x, y), (w, h)) = (p, size);
    match turns % 4 {
        1 => (y, h - x),
        2 => (w - x, h - y),
        3 => (w - y, x),
        _ => (x, y),
    }
}

/// A page's size once turned.
pub fn turn_size(size: (f32, f32), turns: u8) -> (f32, f32) {
    if turns % 2 == 1 { (size.1, size.0) } else { size }
}

/// A rectangle on a page `size` big, on the turned page.
pub fn turn_rect(r: &falcon_pdf::Rect, size: (f64, f64), turns: u8) -> falcon_pdf::Rect {
    let (ax, ay) = turn_point((r.x, r.y), size, turns);
    let (bx, by) = turn_point((r.x + r.w, r.y + r.h), size, turns);
    falcon_pdf::Rect { x: ax.min(bx), y: ay.min(by), w: (ax - bx).abs(), h: (ay - by).abs() }
}

/// A page picture turned `turns` quarter turns clockwise.
pub fn turn_bitmap(b: falcon_pdf::Bitmap, turns: u8) -> falcon_pdf::Bitmap {
    let turns = turns % 4;
    if turns == 0 {
        return b;
    }
    let (w, h) = (b.width, b.height);
    let (nw, nh) = if turns % 2 == 1 { (h, w) } else { (w, h) };
    let mut out = vec![0u8; b.rgba.len()];
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = match turns {
                1 => (h - 1 - y, x),
                2 => (w - 1 - x, h - 1 - y),
                _ => (y, w - 1 - x),
            };
            let (s, d) = ((y * w + x) * 4, (ny * nw + nx) * 4);
            out[d..d + 4].copy_from_slice(&b.rgba[s..s + 4]);
        }
    }
    falcon_pdf::Bitmap { width: nw, height: nh, rgba: out }
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
    /// Turn the pages a quarter clockwise (1) or anticlockwise (-1).
    Rotate(i8),
    /// Print the document (⌘P).
    Print,
    /// The hand tool (true) or the text selection tool.
    Hand(bool),
}

/// The reader's keys, after pdf.js: n/j and p/k turn pages, Home/End and
/// ⌘↑/⌘↓ go to the ends, ↑/↓ and Page Up/Down scroll, ⌘+/⌘−/⌘0 zoom,
/// ⌘F finds and ⌘G/⇧⌘G (or ↩/⇧↩ in the find field) step through hits,
/// r and ⇧R turn the pages, s and h pick the text and hand tools.
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
            Key::P if !m.shift => Some(PdfKey::Print),
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
        Key::R => Some(PdfKey::Rotate(if m.shift { -1 } else { 1 })),
        Key::S if !m.shift => Some(PdfKey::Hand(false)),
        Key::H if !m.shift => Some(PdfKey::Hand(true)),
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
    /// Quarter turns the picture is drawn turned by.
    turns: u8,
    /// Desaturated for the grayscale preview.
    gray: bool,
}

enum Req {
    /// The pages wanted now, most wanted first; replaces the previous list.
    Want(Vec<Job>),
    Search(u64, String, SearchOptions),
    Select(u64, usize, (f64, f64), (f64, f64)),
    /// A password to try on a locked document.
    Password(String),
    /// Write attachment `usize` to the file chosen.
    Save(usize, PathBuf),
}

enum Reply {
    Opened { sizes: Vec<(f64, f64)>, outline: Vec<OutlineEntry>, info: Box<DocInfo> },
    Failed(String),
    /// The document needs a password (`true`: the last one tried failed).
    Locked(bool),
    Page(Job, Option<falcon_pdf::Bitmap>),
    Found(u64, Vec<Hit>),
    Selected(u64, falcon_pdf::Selection),
    /// A page's links, sent once, with its first picture.
    Links(usize, Vec<Link>),
    /// An attachment was written (to the path) or couldn't be.
    Saved(Result<PathBuf, String>),
}

/// A selection request: serial, page, drag start and end.
type SelectReq = (u64, usize, (f64, f64), (f64, f64));

/// The latest request of each kind, waiting for the worker.
#[derive(Default)]
struct Pending {
    queue: VecDeque<Job>,
    search: Option<(u64, String, SearchOptions)>,
    select: Option<SelectReq>,
    saves: Vec<(usize, PathBuf)>,
}

impl Pending {
    fn take(&mut self, r: Req) {
        match r {
            Req::Want(jobs) => self.queue = jobs.into(),
            Req::Search(g, s, o) => self.search = Some((g, s, o)),
            Req::Select(g, p, a, b) => self.select = Some((g, p, a, b)),
            Req::Password(_) => {}
            Req::Save(i, path) => self.saves.push((i, path)),
        }
    }

    fn idle(&self) -> bool {
        self.queue.is_empty() && self.search.is_none() && self.select.is_none() && self.saves.is_empty()
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
    // A locked document waits for passwords until one opens it.
    let mut password: Option<String> = None;
    let doc = loop {
        match Document::open_with_password(path, password.as_deref()) {
            Ok(d) => break d,
            Err(e) if e.needs_password() => {
                if !send(Reply::Locked(password.is_some())) {
                    return;
                }
                password = loop {
                    match rx.recv() {
                        Ok(Req::Password(p)) => break Some(p),
                        Ok(_) => {}
                        Err(_) => return,
                    }
                };
            }
            Err(e) => {
                send(Reply::Failed(e.0));
                return;
            }
        }
    };
    let info = DocInfo {
        pdf: doc.info(),
        linearized: std::fs::File::open(path).ok().is_some_and(is_linearized),
        password,
        attachments: doc.attachments(),
        layers: doc.layers(),
    };
    if !send(Reply::Opened { sizes: doc.page_sizes(), outline: doc.outline(), info: Box::new(info) }) {
        return;
    }
    let mut p = Pending::default();
    // Jobs just rendered: a resent list may still name them before their
    // pictures reach the viewer.
    let mut recent: VecDeque<Job> = VecDeque::new();
    let mut linked: HashSet<usize> = HashSet::new();
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
        let reply = if let Some((i, dest)) = p.saves.pop() {
            Reply::Saved(doc.attachment_data(i).map_err(|e| e.0).and_then(|bytes| std::fs::write(&dest, bytes).map(|()| dest).map_err(|e| e.to_string())))
        } else if let Some((g, page, a, b)) = p.select.take() {
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
            if job.key != THUMB_KEY && linked.insert(job.page) && !send(Reply::Links(job.page, doc.links(job.page))) {
                return;
            }
            let bitmap = if job.key == THUMB_KEY {
                doc.thumbnail(job.page, THUMB_PX)
            } else {
                let (w, h) = doc.page_size(job.page).unwrap_or((612.0, 792.0));
                let cap = (MAX_PAGE_PIXELS / (w * h).max(1.0)).sqrt();
                doc.render(job.page, key_scale(job.key).min(cap), max_side)
            };
            let gray = |mut b: falcon_pdf::Bitmap| {
                if job.gray {
                    crate::logic::grayscale_rgba(&mut b.rgba);
                }
                b
            };
            Reply::Page(job, bitmap.ok().map(|b| gray(turn_bitmap(b, job.turns))))
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
    /// Waiting for a password (`true`: the last one was wrong).
    Locked(bool),
    Ready,
    Failed(String),
}

/// What the Document Properties dialog shows beyond the item itself.
#[derive(Clone, Debug, Default)]
struct DocInfo {
    pdf: falcon_pdf::Info,
    /// "Fast web view": the file is linearized.
    linearized: bool,
    /// The password that opened it, for printing.
    password: Option<String>,
    /// The embedded files and the optional content groups, for the
    /// sidebar's Attachments and Layers lists.
    attachments: Vec<falcon_pdf::Attachment>,
    layers: Vec<falcon_pdf::Layer>,
}

/// Whether a PDF file is linearized ("fast web view"): its first object,
/// near the start of the file, is the linearization dictionary.
pub fn is_linearized(mut file: impl std::io::Read) -> bool {
    let mut head = [0u8; 1024];
    let n = file.read(&mut head).unwrap_or(0);
    head[..n].windows(11).any(|w| w == b"/Linearized")
}

#[derive(Default)]
struct Find {
    open: bool,
    text: String,
    match_case: bool,
    whole_words: bool,
    match_diacritics: bool,
    /// The query last sent, to search again only when it changes.
    sent: Option<(String, SearchOptions)>,
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

/// The sidebar's lists, as in pdf.js's sidebar.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Tab {
    Thumbnails,
    Outline,
    Attachments,
    Layers,
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
    /// Quarter turns clockwise the pages are shown at (view only).
    turns: u8,
    /// The hand tool: dragging pans instead of selecting text.
    hand: bool,
    /// Tools ▸ Presentation Mode was chosen this frame.
    present: bool,
    scroll: ScrollMode,
    spread: Spread,
    /// What the layout was last made from, to make it again on a change.
    laid: Option<(f32, u8, ScrollMode, Spread, usize, i32)>,
    /// Wheel movement past the end of the page in page mode.
    page_wheel: f32,
    /// Each page's links, once its first picture has been drawn.
    links: HashMap<usize, Vec<Link>>,
    info: DocInfo,
    /// The password being typed for a locked document.
    password: String,
    /// The Document Properties dialog is open.
    properties: bool,
    /// The outline entry last highlighted, to scroll to a new one.
    outline_shown: Option<usize>,
    /// Print was asked for this frame.
    print: bool,
    /// The pages are drawn for the grayscale preview.
    gray: bool,
    /// A note for the app's toast (an attachment saved or not).
    notice: Option<String>,
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
            turns: mark.map_or(0, |m| m.turns % 4),
            hand: false,
            present: false,
            scroll: mark.map_or_else(ScrollMode::default, |m| m.scroll),
            spread: mark.map_or_else(Spread::default, |m| m.spread),
            laid: None,
            page_wheel: 0.0,
            links: HashMap::new(),
            info: DocInfo::default(),
            password: String::new(),
            properties: false,
            outline_shown: None,
            print: false,
            gray: false,
            notice: None,
        }
    }

    /// The first page of the spread after (`step` 1) or before (-1) the
    /// current page's.
    fn spread_step(&self, step: isize) -> usize {
        let groups = spreads(self.count(), self.spread);
        let at = groups.iter().position(|g| g.contains(&self.current)).unwrap_or(0) as isize;
        let k = (at + step).clamp(0, groups.len() as isize - 1).max(0) as usize;
        groups.get(k).map_or(0, |g| g.start)
    }

    /// Switches the scroll or spread mode, keeping the current page.
    fn set_modes(&mut self, scroll: ScrollMode, spread: Spread) {
        self.scroll = scroll;
        self.spread = spread;
        self.restore = Some(self.current);
    }

    /// The pages' sizes as shown, turned.
    fn shown_sizes(&self) -> Vec<(f32, f32)> {
        self.sizes.iter().map(|&s| turn_size(s, self.turns)).collect()
    }

    /// Turns every page a quarter clockwise (`delta` 1) or anticlockwise
    /// (-1), as pdf.js's Rotate: pictures are drawn again turned, the
    /// selection is dropped and the current page stays.
    fn rotate(&mut self, delta: i8) {
        self.turns = (i16::from(self.turns) + i16::from(delta)).rem_euclid(4) as u8;
        self.pages = PageCache::new(PAGE_BUDGET);
        self.thumbs = PageCache::new(THUMB_BUDGET);
        self.broken.clear();
        self.sent.clear();
        self.sel = Selecting { serial: self.sel.serial, ..Default::default() };
        self.restore = Some(self.current);
    }

    /// Switches the page pictures to or from the grayscale preview: they
    /// are rendered again (Eagle grays its pdf.js frame too).
    fn set_gray(&mut self, gray: bool) {
        if self.gray == gray {
            return;
        }
        self.gray = gray;
        self.pages = PageCache::new(PAGE_BUDGET);
        self.thumbs = PageCache::new(THUMB_BUDGET);
        self.broken.clear();
        self.sent.clear();
    }

    fn mark(&self) -> Mark {
        Mark { id: self.id.clone(), page: self.current, zoom: self.zoom.key(), turns: self.turns, scroll: self.scroll, spread: self.spread }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        loop {
            match self.rx.try_recv() {
                Ok(Reply::Locked(wrong)) => {
                    self.status = Status::Locked(wrong);
                    self.password.clear();
                }
                Ok(Reply::Opened { sizes, outline, info }) => {
                    self.info = *info;
                    self.sizes = sizes.into_iter().map(|(w, h)| (w as f32, h as f32)).collect();
                    self.outline = outline;
                    self.status = Status::Ready;
                    if let Some(p) = self.restore {
                        self.restore = Some(p.min(self.sizes.len().saturating_sub(1)));
                        self.current = self.restore.unwrap_or(0);
                    }
                }
                Ok(Reply::Failed(e)) => self.status = Status::Failed(e),
                Ok(Reply::Page(job, _)) if job.turns != self.turns || job.gray != self.gray => {}
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
                Ok(Reply::Links(page, links)) => {
                    self.links.insert(page, links);
                }
                Ok(Reply::Saved(r)) => {
                    self.notice = Some(match r {
                        Ok(path) => format!("Saved {}", path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())),
                        Err(e) => format!("Can't save the attachment: {e}"),
                    });
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
        self.current = page;
        if self.scroll == ScrollMode::Page {
            // Laid out again around the new page, from the top.
            self.restore = Some(page);
        } else {
            self.jump = Some(self.layout.offset_for(page, self.offset, self.layout.content_width(self.view.x)));
        }
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
            PdfKey::NextPage => self.go_to(self.spread_step(1)),
            PdfKey::PrevPage => self.go_to(self.spread_step(-1)),
            PdfKey::First => self.go_to(0),
            PdfKey::Last => self.go_to(self.count().saturating_sub(1)),
            PdfKey::Lines(n) => self.scroll_by(n * 40.0),
            PdfKey::Screens(n) => self.scroll_by(n * (self.view.y - 40.0).max(40.0)),
            PdfKey::ZoomIn => self.zoom = Zoom::Percent(zoom_step(self.zoom_factor(), true)),
            PdfKey::ZoomOut => self.zoom = Zoom::Percent(zoom_step(self.zoom_factor(), false)),
            PdfKey::ZoomReset => self.zoom = Zoom::Auto,
            PdfKey::Rotate(d) => self.rotate(d),
            PdfKey::Print => self.print = true,
            PdfKey::Hand(on) => {
                self.hand = on;
                if on {
                    self.sel = Selecting { serial: self.sel.serial, ..Default::default() };
                }
            }
        }
    }

    /// Scrolls so the current find hit shows, a third of the way down.
    fn reveal_hit(&mut self) {
        let Some(hit) = self.find.current.and_then(|c| self.find.hits.get(c)).cloned() else { return };
        if hit.page >= self.count() {
            return;
        }
        if self.scroll == ScrollMode::Page && self.layout.sizes.get(hit.page).is_none_or(|s| s.x <= 0.0) {
            self.go_to(hit.page);
            return;
        }
        let cw = self.layout.content_width(self.view.x);
        let page = self.layout.rect(hit.page, cw);
        let k = self.zoom_factor() * UNIT;
        let raw = (f64::from(self.sizes[hit.page].0), f64::from(self.sizes[hit.page].1));
        let r = turn_rect(&hit.rects.first().copied().unwrap_or_default(), raw, self.turns);
        let spot = Rect::from_min_size(page.min + vec2(r.x as f32 * k, r.y as f32 * k), vec2(r.w as f32 * k, r.h as f32 * k));
        let view = Rect::from_min_size(self.offset.to_pos2(), self.view).shrink(20.0);
        if !view.contains_rect(spot) {
            let max = vec2((cw - self.view.x).max(0.0), (self.layout.height - self.view.y).max(0.0));
            let x = if spot.left() < view.left() || spot.right() > view.right() { spot.left() - self.view.x / 3.0 } else { self.offset.x };
            let y = if spot.top() < view.top() || spot.bottom() > view.bottom() { spot.top() - self.view.y / 3.0 } else { self.offset.y };
            self.jump = Some(vec2(x.clamp(0.0, max.x), y.clamp(0.0, max.y)));
        }
        self.current = hit.page;
    }

    /// Sends the latest search when the query changed.
    fn search_if_changed(&mut self) {
        let options = SearchOptions { match_case: self.find.match_case, whole_words: self.find.whole_words, match_diacritics: self.find.match_diacritics };
        let q = (self.find.text.clone(), options);
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
        let _ = self.tx.send(Req::Search(self.find.serial, q.0, q.1));
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
        let sizes = self.shown_sizes();
        // The zoom fits the current page's spread (pdf.js measures the
        // spread for Page Fit and Page Width).
        let groups = spreads(sizes.len(), self.spread);
        let group = groups.iter().position(|g| g.contains(&self.current)).unwrap_or(0);
        let reference = groups.get(group).map_or((612.0, 792.0), |g| {
            let w = g.clone().map(|i| sizes[i].0).sum::<f32>() + SPREAD_GAP / UNIT * (g.len() - 1) as f32;
            (w, g.clone().map(|i| sizes[i].1).fold(0.0, f32::max))
        });
        let scale = self.zoom.resolve(reference, view);
        let page_group = if self.scroll == ScrollMode::Page { group } else { 0 };
        let wrap_w = if self.scroll == ScrollMode::Wrapped { view.x.round() as i32 } else { 0 };
        let sig = (scale, self.turns, self.scroll, self.spread, page_group, wrap_w);
        if self.laid.is_some_and(|l| (l.0 - scale).abs() < 1e-4 && (l.1, l.2, l.3, l.4, l.5) == (sig.1, sig.2, sig.3, sig.4, sig.5)) && self.layout.tops.len() == self.sizes.len() {
            return;
        }
        let same_arrangement = self.laid.is_some_and(|l| (l.1, l.2, l.3, l.4) == (sig.1, sig.2, sig.3, sig.4));
        self.laid = Some(sig);
        let new = arrange(&sizes, scale, GAP, Arrange { scroll: self.scroll, spread: self.spread, view_w: view.x, page: self.current });
        if same_arrangement && self.scale > 0.0 && self.jump.is_none() && !self.layout.tops.is_empty() {
            let ratio = scale / self.scale;
            self.jump = Some(match self.zoom_at.take() {
                Some(at) => (self.offset + at) * ratio - at,
                None if self.scroll == ScrollMode::Horizontal => vec2(anchor_x(&self.layout, &new, self.offset.x), self.offset.y * ratio),
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
pub(crate) struct PdfState {
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

pub(crate) fn is_pdf(item: &Item) -> bool {
    item.ext.eq_ignore_ascii_case("pdf")
}

// ---------------------------------------------------------------------------
// Drawing

impl App {
    /// The reader's keys, read before the app's own so ⌘F, ⌘C, zoom and
    /// the scrolling keys act on the document.
    pub(crate) fn pdf_keys(&mut self, ctx: &egui::Context) {
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
    pub(crate) fn pdf_viewer(&mut self, parent: &mut Ui, lib: &Library, item: &Item, screen: Rect) -> bool {
        let ctx = &parent.ctx().clone();
        if !cfg!(target_os = "macos") || !is_pdf(item) {
            return false;
        }
        self.pdf.ensure(ctx, lib, item);
        {
            let ui = &mut crate::ui::widgets::viewer_ui(parent, screen, "pdf-viewer");
            ui.painter().rect_filled(screen, 0.0, t::p().main);
            let bar = Rect::from_min_size(screen.min, vec2(screen.width(), t::BAR_HEIGHT));
            let mut area = Rect::from_min_max(Pos2::new(screen.left(), bar.bottom()), screen.max);
            ui.interact(area, ui.scope_id().with("pdf-bg"), Sense::click());
            let sidebar = self.pdf.sidebar;
            if let Some(o) = self.pdf.open.as_mut() {
                o.set_gray(self.modes.grayscale);
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
                    Status::Locked(wrong) => password_prompt(o, ui, area, wrong),
                }
            }
            let (name_x, mut right) = crate::ui::viewer::audio::bar_shell(self, ui, item, bar);
            let mut present = false;
            if let Some(o) = self.pdf.open.as_mut().filter(|o| o.status == Status::Ready) {
                let (tab, left) = toolbar(o, ui, bar, right - 8.0, self.pdf.sidebar);
                self.pdf.sidebar = tab;
                right = left;
                present = std::mem::take(&mut o.present);
            }
            if present {
                self.toggle_presentation(ctx);
            }
            if let Some(note) = self.pdf.open.as_mut().and_then(|o| o.notice.take()) {
                self.flash(note);
            }
            if let Some(o) = self.pdf.open.as_mut().filter(|o| o.properties) {
                properties_dialog(o, ctx, item);
            }
            // PDFKit's print panel, on this (the main) thread; it is modal.
            if let Some(o) = self.pdf.open.as_mut().filter(|o| o.print) {
                o.print = false;
                let title = o.info.pdf.title.clone().unwrap_or_else(|| item.name.clone());
                if let Err(e) = falcon_pdf::print(&item.original_path(lib.root()), o.info.password.as_deref(), &title) {
                    self.flash(format!("Can't print: {e}"));
                }
            }
            crate::ui::viewer::audio::bar_name(ui, item, bar, name_x, right);
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
    // Read before the scroll area takes it.
    let wheel = ui.input(|i| if i.pointer.hover_pos().is_some_and(|p| area.contains(p)) { i.smooth_scroll_delta.y } else { 0.0 });
    o.relayout(area.size());
    if let Some(p) = o.restore.take() {
        o.jump = Some(o.layout.offset_for(p, Vec2::ZERO, o.layout.content_width(area.width())));
    }
    let ppp = ui.ctx().pixels_per_point();
    let key = scale_key(o.scale * UNIT * ppp);
    let settled = o.changed.elapsed() >= SETTLE;
    let k = o.scale * UNIT;
    let turns = o.turns;

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
        let Some((a, b)) = o.layout.visible_in(viewport, cw) else { return };
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
            let raw = (f64::from(o.sizes[i].0), f64::from(o.sizes[i].1));
            let to_screen = |rc: &falcon_pdf::Rect| {
                let rc = turn_rect(rc, raw, turns);
                Rect::from_min_size(r.min + vec2(rc.x as f32 * k, rc.y as f32 * k), vec2(rc.w as f32 * k, rc.h as f32 * k))
            };
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
                jobs.push(Job { page: i, key, turns, gray: o.gray });
            }
        }

        // Dragging over a page selects its text (or, with the hand tool,
        // pans); a click clears the selection.
        let screen_view = Rect::from_min_size(origin + viewport.min.to_vec2(), viewport.size());
        let resp = ui.interact(screen_view, ui.scope_id().with("pdf-select"), Sense::click_and_drag());
        // Links: the pointing hand and the address over them; a click
        // follows them, as pdf.js's link layer does.
        let link_at = |o: &Open, pos: Pos2| -> Option<LinkTarget> {
            let c = (pos - origin).to_pos2();
            let i = o.layout.page_at_pos(c, cw)?;
            let page = o.layout.rect(i, cw);
            let raw = (f64::from(o.sizes[i].0), f64::from(o.sizes[i].1));
            o.links.get(&i)?.iter().find_map(|l| {
                let r = turn_rect(&l.rect, raw, turns);
                Rect::from_min_size(page.min + vec2(r.x as f32 * k, r.y as f32 * k), vec2(r.w as f32 * k, r.h as f32 * k)).contains(c).then(|| l.target.clone())
            })
        };
        if !resp.dragged()
            && let Some(target) = resp.hover_pos().and_then(|p| link_at(o, p))
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            let tip = match &target {
                LinkTarget::Url(u) => u.clone(),
                LinkTarget::Page { page, .. } => format!("Page {}", page + 1),
            };
            let clicked = resp.clicked();
            resp.clone().on_hover_text_at_pointer(tip);
            if clicked {
                follow_link(o, ui.ctx(), &target);
                return;
            }
            if !o.hand {
                return;
            }
        }
        if o.hand {
            if resp.dragged() {
                o.jump = Some(o.offset - resp.drag_delta());
            }
            if resp.hovered() || resp.dragged() {
                ui.ctx().set_cursor_icon(if resp.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
            }
            return;
        }
        // A point on page `i` as the file shows it, from a screen position.
        let at = |o: &Open, pos: Pos2, page: Option<usize>| -> Option<(usize, (f64, f64))> {
            let c = pos - origin;
            let i = page.or_else(|| o.layout.page_at_pos(c.to_pos2(), cw))?;
            let (w, h) = turn_size(o.sizes[i], turns);
            let x = ((c.x - o.layout.left(i, cw)) / k).clamp(0.0, w);
            let y = ((c.y - o.layout.tops[i]) / k).clamp(0.0, h);
            let raw = (f64::from(o.sizes[i].0), f64::from(o.sizes[i].1));
            Some((i, unturn_point((f64::from(x), f64::from(y)), raw, turns)))
        };
        if resp.hovered()
            && let Some(pos) = resp.hover_pos()
            && o.layout.page_at_pos((pos - origin).to_pos2(), cw).is_some()
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
    if o.scroll == ScrollMode::Page {
        page_wheel(o, wheel, area);
    } else {
        let cw = o.layout.content_width(area.width());
        o.current = o.layout.current_in(Rect::from_min_size(o.offset.to_pos2(), area.size()), cw);
    }
}

/// Follows a link: to its page (and the spot on it it names), or to its
/// web or mail address in the default app. Other kinds of address (files,
/// scripts) are not opened.
fn follow_link(o: &mut Open, ctx: &egui::Context, target: &LinkTarget) {
    match target {
        LinkTarget::Page { page, point } => {
            o.go_to(*page);
            if let (Some(pt), Some(jump), true) = (point, o.jump, matches!(o.scroll, ScrollMode::Vertical | ScrollMode::Wrapped))
                && let Some(&(w, h)) = o.sizes.get(*page)
            {
                let (_, y) = turn_point(*pt, (f64::from(w), f64::from(h)), o.turns);
                let top = o.layout.tops[*page] + y as f32 * o.zoom_factor() * UNIT - PAD;
                o.jump = Some(vec2(jump.x, top.max(0.0)));
            }
        }
        LinkTarget::Url(url) => {
            if link_opens(url) {
                ctx.open_url(egui::OpenUrl::new_tab(url));
            }
        }
    }
}

/// Whether a link's address may be opened: web and mail addresses only.
pub fn link_opens(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    ["http://", "https://", "mailto:"].iter().any(|s| lower.starts_with(s))
}

/// In page mode, the wheel turns the page once the page has scrolled to
/// its end (or at once when it fits), as pdf.js's page scrolling does.
fn page_wheel(o: &mut Open, dy: f32, area: Rect) {
    const TURN: f32 = 60.0;
    if dy == 0.0 {
        return;
    }
    let max = (o.layout.height - area.height()).max(0.0);
    let at_top = o.offset.y <= 0.5;
    let at_end = o.offset.y >= max - 0.5;
    // egui's wheel is positive upwards.
    if (dy > 0.0 && !at_top) || (dy < 0.0 && !at_end) || o.page_wheel * dy < 0.0 {
        o.page_wheel = 0.0;
        if !((dy > 0.0 && at_top) || (dy < 0.0 && at_end)) {
            return;
        }
    }
    o.page_wheel += dy;
    if o.page_wheel.abs() >= TURN {
        let step = if o.page_wheel > 0.0 { -1 } else { 1 };
        o.page_wheel = 0.0;
        let to = o.spread_step(step);
        if to != o.current {
            o.go_to(to);
        }
    }
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
            let sizes = o.shown_sizes();
            let widest = sizes.iter().map(|s| s.0).fold(1.0f32, f32::max);
            let tl = layout(&sizes, THUMB_W / (widest * UNIT), 26.0);
            let (turns, gray) = (o.turns, o.gray);
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
                        None => jobs.push(Job { page: i, key: THUMB_KEY, turns, gray }),
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
            // The entry for the current page is highlighted, and scrolled to
            // when it changes.
            let here = current_outline(&o.outline, o.current);
            let reveal = here != o.outline_shown;
            o.outline_shown = here;
            egui::ScrollArea::vertical().id_salt("pdf-outline").auto_shrink([false, false]).show(&mut child, |ui| {
                if o.outline.is_empty() {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("No outline").font(font(12.0)).color(t::p().text_muted));
                }
                for (n, e) in o.outline.iter().enumerate() {
                    if here == Some(n) {
                        let row = Rect::from_min_size(Pos2::new(list.left(), ui.cursor().top()), vec2(list.width(), ui.spacing().interact_size.y));
                        ui.painter().rect_filled(row, 0.0, t::p().row_selected);
                        if reveal {
                            ui.scroll_to_rect(row, Some(egui::Align::Center));
                        }
                    }
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
        Tab::Attachments => {
            // pdf.js's attachments list: a click saves the file.
            let mut save = None;
            egui::ScrollArea::vertical().id_salt("pdf-attachments").auto_shrink([false, false]).show(&mut child, |ui| {
                if o.info.attachments.is_empty() {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("No attachments").font(font(12.0)).color(t::p().text_muted));
                }
                for (n, a) in o.info.attachments.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        let size = a.size.map(|b| format!("  {}", crate::logic::format_size(b))).unwrap_or_default();
                        let text = egui::RichText::new(format!("{}{size}", a.name)).font(font(12.0)).color(t::p().text);
                        let resp = ui.push_id(("attachment", n), |ui| ui.add(egui::Label::new(text).truncate().sense(Sense::click()))).inner;
                        let tip = a.description.as_deref().map_or_else(|| "Save the attachment…".to_string(), |d| format!("{d}\nClick to save it"));
                        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tip).clicked() {
                            save = Some((n, a.name.clone()));
                        }
                    });
                }
            });
            if let Some((n, name)) = save
                && let Some(dest) = rfd::FileDialog::new().set_title("Save Attachment").set_file_name(name).save_file()
            {
                let _ = o.tx.send(Req::Save(n, dest));
            }
        }
        Tab::Layers => {
            // pdf.js's layers list. PDFKit draws the document's default
            // configuration only, so the list shows which layers that has
            // on without switching them.
            egui::ScrollArea::vertical().id_salt("pdf-layers").auto_shrink([false, false]).show(&mut child, |ui| {
                if o.info.layers.is_empty() {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("No layers").font(font(12.0)).color(t::p().text_muted));
                    return;
                }
                for l in &o.info.layers {
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        let (mark, color) = if l.visible { ("●", t::p().text) } else { ("○", t::p().text_muted) };
                        ui.label(egui::RichText::new(mark).font(font(11.0)).color(color));
                        ui.add(egui::Label::new(egui::RichText::new(&l.name).font(font(12.0)).color(color)).truncate());
                    });
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.add(egui::Label::new(egui::RichText::new("Shown as the document sets them; macOS's PDF engine can't switch layers.").font(font(11.0)).color(t::p().text_muted)).wrap());
                });
            });
        }
    }
}

/// The reader's controls in the viewer bar, leftwards from `right`:
/// sidebar and find toggles, page navigation and zoom. Returns the
/// sidebar choice and the x left of the controls.
fn toolbar(o: &mut Open, ui: &mut Ui, bar: Rect, right: f32, mut sidebar: Option<Tab>) -> (Option<Tab>, f32) {
    let cy = bar.center().y;
    let p = ui.painter().clone();
    // pdf.js's Tools menu, at the right end as in its toolbar.
    let menu = Rect::from_center_size(Pos2::new(right - 24.0, cy), vec2(48.0, 24.0));
    let mut mui = ui.new_child(egui::UiBuilder::new().max_rect(menu));
    egui::containers::menu::MenuButton::new(egui::RichText::new("Tools").font(font(12.0))).ui(&mut mui, |ui| tools_menu(o, ui));
    let mut x = menu.left() - 16.0;
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
            // As pdf.js, the lists a document lacks can't be picked.
            let (has_files, has_layers) = (!o.info.attachments.is_empty(), !o.info.layers.is_empty());
            if ui.add_enabled(has_files, egui::Button::selectable(sidebar == Some(Tab::Attachments), "Attachments")).clicked() {
                sidebar = Some(Tab::Attachments);
                ui.close();
            }
            if ui.add_enabled(has_layers, egui::Button::selectable(sidebar == Some(Tab::Layers), "Layers")).clicked() {
                sidebar = Some(Tab::Layers);
                ui.close();
            }
        });
    }
    (sidebar, x - 16.0)
}

/// The prompt for a locked document's password, in the middle of the
/// reader.
fn password_prompt(o: &mut Open, ui: &mut Ui, area: Rect, wrong: bool) {
    let r = Rect::from_center_size(area.center(), vec2(340.0, 132.0));
    let p = ui.painter().clone();
    p.rect_filled(r, CornerRadius::same(10), t::p().panel);
    p.rect_stroke(r, CornerRadius::same(10), Stroke::new(1.0, t::p().divider), egui::StrokeKind::Inside);
    p.text(r.min + vec2(18.0, 22.0), Align2::LEFT_CENTER, "This PDF is locked", font(14.0), t::p().text);
    let line = if wrong { "That password doesn't open it. Try again." } else { "Type its password to read it." };
    p.text(r.min + vec2(18.0, 46.0), Align2::LEFT_CENTER, line, font(12.5), if wrong { t::p().icon_red } else { t::p().text_muted });
    let field = Rect::from_min_size(r.min + vec2(18.0, 64.0), vec2(r.width() - 36.0, 24.0));
    let id = Id::unique("pdf-password");
    let mut fui = ui.new_child(egui::UiBuilder::new().max_rect(field));
    let resp = fui.add(TextEdit::singleline(&mut o.password).id(id).password(true).font(font(12.5)).desired_width(field.width()));
    if !ui.ctx().memory(|m| m.focused().is_some()) {
        resp.request_focus();
    }
    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
    let button = Rect::from_min_size(Pos2::new(r.right() - 18.0 - 80.0, r.bottom() - 34.0), vec2(80.0, 24.0));
    let open = ui.put(button, egui::Button::new("Open")).clicked();
    if (enter || open) && !o.password.is_empty() {
        let _ = o.tx.send(Req::Password(std::mem::take(&mut o.password)));
        o.status = Status::Opening;
    }
}

/// Document Properties: the file, its metadata and the current page's size.
fn properties_dialog(o: &mut Open, ctx: &egui::Context, item: &Item) {
    let i = &o.info.pdf;
    let none = || "-".to_string();
    let date = |t: Option<f64>| t.map_or_else(none, |s| crate::logic::date_time_text((s * 1000.0) as i64));
    let size = o.sizes.get(o.current).map(|&s| turn_size(s, o.turns)).map_or_else(none, |(w, h)| page_size_text(w, h));
    let rows: Vec<(&str, String)> = vec![
        ("File", format!("{}.{}", item.name, item.ext)),
        ("Size", format!("{} ({} bytes)", crate::logic::format_size(item.size), item.size)),
        ("Title", i.title.clone().unwrap_or_else(none)),
        ("Author", i.author.clone().unwrap_or_else(none)),
        ("Subject", i.subject.clone().unwrap_or_else(none)),
        ("Keywords", i.keywords.clone().unwrap_or_else(none)),
        ("Created", date(i.created)),
        ("Modified", date(i.modified)),
        ("Application", i.creator.clone().unwrap_or_else(none)),
        ("PDF producer", i.producer.clone().unwrap_or_else(none)),
        ("PDF version", format!("{}.{}", i.version.0, i.version.1)),
        ("Pages", i.pages.to_string()),
        ("Page size", size),
        ("Fast web view", if o.info.linearized { "Yes" } else { "No" }.to_string()),
        ("Password", if i.encrypted { "Yes" } else { "No" }.to_string()),
    ];
    let modal = egui::Modal::new(Id::unique("pdf-properties")).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.label(egui::RichText::new("Document Properties").size(15.0));
        ui.add_space(8.0);
        egui::Grid::new("pdf-properties-grid").num_columns(2).spacing(vec2(14.0, 5.0)).show(ui, |ui| {
            for (k, v) in rows {
                ui.label(egui::RichText::new(k).color(t::p().text_muted));
                ui.add(egui::Label::new(egui::RichText::new(v).color(t::p().text)).wrap());
                ui.end_row();
            }
        });
        ui.add_space(10.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.button("Close").clicked()).inner
    });
    if modal.inner || modal.should_close() {
        o.properties = false;
    }
}

/// A page size as pdf.js's Document Properties gives it: inches, and the
/// paper's name when it is a common one, then the orientation.
pub fn page_size_text(w_pt: f32, h_pt: f32) -> String {
    let (wi, hi) = (w_pt / 72.0, h_pt / 72.0);
    let round = |v: f32| ((v * 100.0).round() / 100.0).to_string();
    let orientation = if w_pt > h_pt { "Landscape" } else { "Portrait" };
    // Paper sizes in points, short side first, matched within a point.
    const PAPERS: [(&str, f32, f32); 5] = [("Letter", 612.0, 792.0), ("Legal", 612.0, 1008.0), ("Tabloid", 792.0, 1224.0), ("A4", 595.28, 841.89), ("A3", 841.89, 1190.55)];
    let (short, long) = (w_pt.min(h_pt), w_pt.max(h_pt));
    let name = PAPERS.iter().find(|p| (p.1 - short).abs() < 1.0 && (p.2 - long).abs() < 1.0).map(|p| p.0);
    match name {
        Some(n) => format!("{} × {} in ({n}, {orientation})", round(wi), round(hi)),
        None => format!("{} × {} in ({orientation})", round(wi), round(hi)),
    }
}

/// The entries of pdf.js's Tools menu that Eagle keeps and PDFKit allows.
fn tools_menu(o: &mut Open, ui: &mut Ui) {
    ui.set_min_width(220.0);
    let row = |ui: &mut Ui, label: &str, on: bool, key: &str| ui.add(egui::Button::new(label).selected(on).shortcut_text(key)).clicked();
    if row(ui, "Print…", false, "⌘P") {
        o.act(PdfKey::Print);
        ui.close();
    }
    if row(ui, "Presentation Mode", false, "F5") {
        o.present = true;
        ui.close();
    }
    ui.separator();
    if row(ui, "Go to First Page", false, "Home") {
        o.act(PdfKey::First);
        ui.close();
    }
    if row(ui, "Go to Last Page", false, "End") {
        o.act(PdfKey::Last);
        ui.close();
    }
    ui.separator();
    if row(ui, "Rotate Clockwise", false, "R") {
        o.act(PdfKey::Rotate(1));
        ui.close();
    }
    if row(ui, "Rotate Counterclockwise", false, "⇧R") {
        o.act(PdfKey::Rotate(-1));
        ui.close();
    }
    ui.separator();
    if row(ui, "Text Selection Tool", !o.hand, "S") {
        o.act(PdfKey::Hand(false));
        ui.close();
    }
    if row(ui, "Hand Tool", o.hand, "H") {
        o.act(PdfKey::Hand(true));
        ui.close();
    }
    ui.separator();
    for (mode, label) in [(ScrollMode::Page, "Page Scrolling"), (ScrollMode::Vertical, "Vertical Scrolling"), (ScrollMode::Horizontal, "Horizontal Scrolling"), (ScrollMode::Wrapped, "Wrapped Scrolling")] {
        if row(ui, label, o.scroll == mode, "") {
            o.set_modes(mode, o.spread);
            ui.close();
        }
    }
    ui.separator();
    for (spread, label) in [(Spread::None, "No Spreads"), (Spread::Odd, "Odd Spreads"), (Spread::Even, "Even Spreads")] {
        if row(ui, label, o.spread == spread, "") {
            o.set_modes(o.scroll, spread);
            ui.close();
        }
    }
    ui.separator();
    if row(ui, "Document Properties…", false, "") {
        o.properties = true;
        ui.close();
    }
}

/// The find bar under the top bar's right end: the query, previous/next,
/// match case, whole words, match diacritics and the hit count.
fn find_bar(o: &mut Open, ui: &mut Ui, top_right: Pos2) {
    let size = vec2(540.0, 36.0);
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
    for (on, label, tip, w) in [
        (&mut o.find.match_case, "Aa", "Match Case", 50.0),
        (&mut o.find.whole_words, "Word", "Whole Words", 62.0),
        (&mut o.find.match_diacritics, "é", "Match Diacritics (e doesn't find é)", 44.0),
    ] {
        let rect = Rect::from_min_size(Pos2::new(x, cy - 10.0), vec2(w, 20.0));
        let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        cui.checkbox(on, egui::RichText::new(label).font(font(12.0))).on_hover_text(tip);
        x = rect.right() + 2.0;
    }
    x += 2.0;
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
        // The widest page sits at the padding; narrower ones centre on it.
        assert_eq!(l.left(1, l.content_width(100.0)), PAD);
        assert_eq!(l.left(0, l.content_width(100.0)), PAD + 90.0);
        assert_eq!(l.content_width(500.0), 792.0 + 2.0 * PAD);
        assert_eq!(l.top_of(1), 792.0 + GAP);
        assert_eq!(l.top_of(9), 0.0);
    }

    #[test]
    fn spreads_pair_pages() {
        let r = |v: Vec<std::ops::Range<usize>>| v.into_iter().map(|g| (g.start, g.end)).collect::<Vec<_>>();
        assert_eq!(r(spreads(3, Spread::None)), [(0, 1), (1, 2), (2, 3)]);
        assert_eq!(r(spreads(5, Spread::Odd)), [(0, 2), (2, 4), (4, 5)]);
        assert_eq!(r(spreads(5, Spread::Even)), [(0, 1), (1, 3), (3, 5)]);
        assert_eq!(r(spreads(0, Spread::Even)), []);
    }

    #[test]
    fn scroll_and_spread_modes_arrange_pages() {
        // Pages of 75 points are 100 screen points at 100%.
        let pages = [(75.0, 75.0); 5];
        let how = |scroll, spread, view_w, page| Arrange { scroll, spread, view_w, page };
        // Odd spreads in a column: rows of two side by side.
        let l = arrange(&pages, 1.0, GAP, how(ScrollMode::Vertical, Spread::Odd, 0.0, 0));
        assert_eq!(l.tops[0], l.tops[1]);
        assert_eq!(l.lefts[1], l.lefts[0] + 100.0 + SPREAD_GAP);
        assert_eq!(l.tops[2], PAD + 100.0 + GAP);
        assert_eq!(l.width, 200.0 + SPREAD_GAP);
        // The odd last page is centred under the pairs.
        assert_eq!(l.lefts[4], PAD + (l.width - 100.0) / 2.0);
        assert!(!l.column);
        // Horizontal: one row.
        let h = arrange(&pages, 1.0, GAP, how(ScrollMode::Horizontal, Spread::None, 0.0, 0));
        assert!(h.tops.iter().all(|&t| t == PAD));
        assert_eq!(h.lefts[3], PAD + 3.0 * (100.0 + GAP));
        assert_eq!(h.height, 100.0 + 2.0 * PAD);
        assert_eq!(h.offset_for(3, vec2(0.0, 7.0), h.content_width(300.0)), vec2(3.0 * (100.0 + GAP), 7.0));
        // Wrapped: as many as fit in the view's width per row.
        let room = 2.0 * PAD + SCROLLBAR + 100.0 * 2.0 + GAP;
        let w = arrange(&pages, 1.0, GAP, how(ScrollMode::Wrapped, Spread::None, room, 0));
        assert_eq!(w.tops[0], w.tops[1]);
        assert_eq!(w.tops[2], PAD + 100.0 + GAP);
        assert_eq!(w.tops[4], PAD + 2.0 * (100.0 + GAP));
        // Page mode: only the spread of the page asked for.
        let p = arrange(&pages, 1.0, GAP, how(ScrollMode::Page, Spread::Even, 0.0, 2));
        assert_eq!((p.sizes[1].x, p.sizes[2].x), (100.0, 100.0));
        assert_eq!((p.sizes[0].x, p.sizes[3].x), (0.0, 0.0));
        assert_eq!(p.tops[1], PAD);
        let cw = p.content_width(400.0);
        assert_eq!(p.visible_in(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0)), cw), Some((1, 2)));
        assert_eq!(p.offset_for(2, vec2(5.0, 5.0), cw), Vec2::ZERO);
        // Finding pages by position, and the page showing most.
        let cw = l.content_width(300.0);
        let right = l.rect(1, cw).center();
        assert_eq!(l.page_at_pos(right, cw), Some(1));
        assert_eq!(l.current_in(Rect::from_min_size(Pos2::new(0.0, 150.0), vec2(300.0, 100.0)), cw), 2);
        let wide = h.content_width(250.0);
        assert_eq!(h.current_in(Rect::from_min_size(Pos2::new(230.0, 0.0), vec2(250.0, 130.0)), wide), 2);
    }

    #[test]
    fn horizontal_zoom_keeps_the_reading_position() {
        let pages = vec![LETTER; 4];
        let how = Arrange { scroll: ScrollMode::Horizontal, ..Default::default() };
        let a = arrange(&pages, 1.0, GAP, how);
        let b = arrange(&pages, 2.0, GAP, how);
        let x = a.lefts[2] + a.sizes[2].x / 4.0;
        assert!((anchor_x(&a, &b, x) - (b.lefts[2] + b.sizes[2].x / 4.0)).abs() < 1e-3);
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
    fn outline_entry_for_the_page() {
        let e = |title: &str, depth, page| OutlineEntry { title: title.into(), depth, page };
        let outline = vec![e("One", 0, Some(0)), e("One.a", 1, Some(2)), e("Two", 0, Some(5)), e("Link out", 0, None), e("Two.a", 1, Some(5))];
        assert_eq!(current_outline(&outline, 0), Some(0));
        assert_eq!(current_outline(&outline, 1), Some(0));
        assert_eq!(current_outline(&outline, 3), Some(1));
        // Two entries for page 6: the later, deeper one.
        assert_eq!(current_outline(&outline, 5), Some(4));
        assert_eq!(current_outline(&outline, 99), Some(4));
        assert_eq!(current_outline(&outline[1..], 0), None);
        assert_eq!(current_outline(&[], 3), None);
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
            remember(&mut m, Mark { id: format!("I{n}"), page: n, zoom: "auto".into(), turns: 0, scroll: ScrollMode::Vertical, spread: Spread::None });
        }
        assert_eq!(m.len(), REMEMBER);
        assert_eq!(m[0].id, "I5");
        remember(&mut m, Mark { id: "I10".into(), page: 99, zoom: "1.5".into(), turns: 1, scroll: ScrollMode::Wrapped, spread: Spread::Odd });
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
        // pdf.js's r / ⇧R rotate and s / h pick the tool.
        assert_eq!(key_action(Key::R, none, false, false), Some(PdfKey::Rotate(1)));
        assert_eq!(key_action(Key::R, Modifiers::SHIFT, false, false), Some(PdfKey::Rotate(-1)));
        assert_eq!(key_action(Key::H, none, false, false), Some(PdfKey::Hand(true)));
        assert_eq!(key_action(Key::S, none, false, false), Some(PdfKey::Hand(false)));
        assert_eq!(key_action(Key::R, none, true, true), None, "typing in the find field");
        assert_eq!(key_action(Key::P, cmd, false, false), Some(PdfKey::Print));
    }

    #[test]
    fn view_rotation_turns_points_and_rects() {
        let size = (100.0, 200.0);
        for t in 0..4 {
            for p in [(0.0, 0.0), (10.0, 20.0), (100.0, 200.0)] {
                let q = unturn_point(turn_point(p, size, t), size, t);
                assert!((q.0 - p.0).abs() < 1e-9 && (q.1 - p.1).abs() < 1e-9, "turns {t}");
            }
        }
        // Turned clockwise, the top-left corner goes to the top-right.
        assert_eq!(turn_point((0.0, 0.0), size, 1), (200.0, 0.0));
        assert_eq!(turn_point((0.0, 0.0), size, 3), (0.0, 100.0));
        assert_eq!(turn_size((100.0, 200.0), 1), (200.0, 100.0));
        assert_eq!(turn_size((100.0, 200.0), 2), (100.0, 200.0));
        let r = falcon_pdf::Rect { x: 10.0, y: 20.0, w: 30.0, h: 5.0 };
        assert_eq!(turn_rect(&r, size, 1), falcon_pdf::Rect { x: 175.0, y: 10.0, w: 5.0, h: 30.0 });
        assert_eq!(turn_rect(&r, size, 0), r);
    }

    #[test]
    fn document_properties_text() {
        assert_eq!(page_size_text(612.0, 792.0), "8.5 × 11 in (Letter, Portrait)");
        assert_eq!(page_size_text(842.0, 595.0), "11.69 × 8.26 in (A4, Landscape)");
        assert_eq!(page_size_text(300.0, 300.0), "4.17 × 4.17 in (Portrait)");
        assert!(is_linearized(&b"%PDF-1.5\n%\xe2\xe3\n1 0 obj\n<< /Linearized 1 /L 1234 >>"[..]));
        assert!(!is_linearized(&b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>"[..]));
    }

    #[test]
    fn only_web_and_mail_links_open() {
        assert!(link_opens("https://example.com"));
        assert!(link_opens("HTTP://example.com"));
        assert!(link_opens("mailto:a@b.c"));
        assert!(!link_opens("file:///etc/passwd"));
        assert!(!link_opens("javascript:alert(1)"));
        assert!(!link_opens("/Applications/Calculator.app"));
    }

    #[test]
    fn page_pictures_turn() {
        // 2 x 1: red, green.
        let b = falcon_pdf::Bitmap { width: 2, height: 1, rgba: vec![255, 0, 0, 255, 0, 255, 0, 255] };
        let cw = turn_bitmap(b.clone(), 1);
        assert_eq!((cw.width, cw.height), (1, 2));
        assert_eq!(cw.pixel(0, 0), [255, 0, 0, 255]);
        assert_eq!(cw.pixel(0, 1), [0, 255, 0, 255]);
        let ccw = turn_bitmap(b.clone(), 3);
        assert_eq!(ccw.pixel(0, 0), [0, 255, 0, 255]);
        let half = turn_bitmap(b.clone(), 2);
        assert_eq!(half.pixel(0, 0), [0, 255, 0, 255]);
        assert_eq!(turn_bitmap(b.clone(), 4), b);
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
