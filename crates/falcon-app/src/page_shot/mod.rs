//! Screenshots of bookmarked pages: a queue that loads one address at a
//! time in a hidden web view, waits for the page to settle and takes a
//! picture of it. It is driven from the UI thread with [`Shots::tick`].
//!
//! The rules live here and are tested against a fake [`Engine`]; WebKit is
//! in `webkit`. Nothing in this module knows about the app or the library,
//! so `tests/page_shot.rs` can run it against real WebKit on the main thread.
//!
//! A page that fails or never finishes is reported once and forgotten. No
//! address comes back unless the user imports or refreshes it again.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
pub mod webkit;

/// The size pages are laid out at, in points, as in Eagle.
pub const PAGE_SIZE: (u32, u32) = (1440, 900);

/// Whether `url` is an http or https address, the only kind a bookmark
/// loads (a link file pointing at `file:`, `javascript:` or an app's own
/// scheme is shown as a still instead).
pub fn is_web_address(url: &str) -> bool {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    let rest = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://"));
    rest.is_some_and(|r| !r.is_empty()) && !url.chars().any(char::is_whitespace)
}

/// Whether `scheme` may load: a page only http and https; a frame also its
/// own blank, inline and blob documents.
fn scheme_allowed(scheme: &str, main_frame: bool) -> bool {
    let scheme = scheme.to_ascii_lowercase();
    matches!(scheme.as_str(), "http" | "https") || (!main_frame && matches!(scheme.as_str(), "about" | "data" | "blob"))
}

/// How long each stage may take.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// From asking for the page until it has finished loading.
    pub load: Duration,
    /// From finishing until the picture is taken, for late layout. Eagle
    /// waits 2.5 s.
    pub settle: Duration,
    /// For WebKit to hand over the picture, and to get ready at the start.
    pub shot: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Timing { load: Duration::from_secs(20), settle: Duration::from_millis(2500), shot: Duration::from_secs(10) }
    }
}

/// Whether the engine can open pages yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    Waiting,
    Ready,
    /// It never will; every page fails.
    Unavailable,
}

/// What happened to an open page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageEvent {
    Finished,
    Failed,
    /// The picture, as TIFF data.
    Shot(Vec<u8>),
    ShotFailed,
}

/// One loading page. Dropping it stops the page and frees its web view.
pub trait Page {
    /// The oldest event not yet taken.
    fn next_event(&mut self) -> Option<PageEvent>;
    /// Asks for a picture; the answer is a later event.
    fn snapshot(&mut self);
}

/// Opens pages. At most one is open at a time.
pub trait Engine {
    type Page: Page;
    /// Starts getting ready if it hasn't, and says how that is going.
    fn prepare(&mut self) -> Readiness;
    /// Starts loading `url`; `None` when the page can't be opened.
    fn open(&mut self, url: &str, size: (u32, u32)) -> Option<Self::Page>;
}

/// Why a page gave no picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// There is no web view to use.
    Unavailable,
    /// The page failed to load.
    Load,
    /// The page did not finish in time.
    TimedOut,
    /// WebKit gave no picture.
    Shot,
}

/// How one queued page ended.
#[derive(Debug, PartialEq, Eq)]
pub struct Finished<K> {
    pub key: K,
    pub url: String,
    pub result: Result<Vec<u8>, Failure>,
}

struct Job<K> {
    key: K,
    url: String,
}

enum Stage {
    Loading,
    Settling { until: Instant },
    Shooting { since: Instant },
}

struct Current<K, P> {
    job: Job<K>,
    page: P,
    stage: Stage,
    since: Instant,
}

/// The queue and the page being worked on.
pub struct Shots<K, E: Engine> {
    engine: E,
    timing: Timing,
    queue: VecDeque<Job<K>>,
    current: Option<Current<K, E::Page>>,
    /// When the engine started getting ready, while it is.
    preparing: Option<Instant>,
}

impl<K: PartialEq, E: Engine> Shots<K, E> {
    pub fn new(engine: E, timing: Timing) -> Self {
        Shots { engine, timing, queue: VecDeque::new(), current: None, preparing: None }
    }

    /// Queues the page `url` for `key`. False, doing nothing, when it isn't
    /// an http or https address or `key` is already queued or being drawn.
    pub fn enqueue(&mut self, key: K, url: &str) -> bool {
        let busy = self.current.as_ref().is_some_and(|c| c.job.key == key) || self.queue.iter().any(|j| j.key == key);
        if busy || !is_web_address(url) {
            return false;
        }
        self.queue.push_back(Job { key, url: url.trim().to_string() });
        true
    }

    /// Whether nothing is queued or being drawn.
    pub fn is_idle(&self) -> bool {
        self.current.is_none() && self.queue.is_empty()
    }

    /// Drops what `keep` refuses, stopping its page if it is the open one.
    pub fn retain(&mut self, mut keep: impl FnMut(&K) -> bool) {
        self.queue.retain(|j| keep(&j.key));
        if self.current.as_ref().is_some_and(|c| !keep(&c.job.key)) {
            self.current = None;
        }
    }

    /// Moves things along and returns the pages that ended.
    pub fn tick(&mut self, now: Instant) -> Vec<Finished<K>> {
        let mut out = Vec::new();
        self.step_current(now, &mut out);
        if self.current.is_none() && !self.queue.is_empty() {
            self.start_next(now, &mut out);
        }
        out
    }

    /// How long until [`Shots::tick`] has a timer to look at, when something
    /// is under way. Events from the engine wake the app sooner.
    pub fn next_wake(&self, now: Instant) -> Option<Duration> {
        let at = match &self.current {
            Some(c) => match c.stage {
                Stage::Loading => c.since + self.timing.load,
                Stage::Settling { until } => until,
                Stage::Shooting { since } => since + self.timing.shot,
            },
            None => self.preparing? + self.timing.shot,
        };
        Some(at.saturating_duration_since(now))
    }

    fn step_current(&mut self, now: Instant, out: &mut Vec<Finished<K>>) {
        let Some(mut c) = self.current.take() else { return };
        let mut result = None;
        while result.is_none()
            && let Some(event) = c.page.next_event()
        {
            match (&c.stage, event) {
                (Stage::Loading, PageEvent::Finished) => c.stage = Stage::Settling { until: now + self.timing.settle },
                (Stage::Loading, PageEvent::Failed) => result = Some(Err(Failure::Load)),
                (Stage::Shooting { .. }, PageEvent::Shot(data)) => result = Some(Ok(data)),
                (Stage::Shooting { .. }, PageEvent::ShotFailed) => result = Some(Err(Failure::Shot)),
                // A frame failing or a later navigation after the page has
                // finished doesn't change the picture's chances.
                _ => {}
            }
        }
        if result.is_none() {
            match c.stage {
                Stage::Loading if now >= c.since + self.timing.load => result = Some(Err(Failure::TimedOut)),
                Stage::Settling { until } if now >= until => {
                    c.page.snapshot();
                    c.stage = Stage::Shooting { since: now };
                }
                Stage::Shooting { since } if now >= since + self.timing.shot => result = Some(Err(Failure::Shot)),
                _ => {}
            }
        }
        match result {
            Some(result) => out.push(Finished { key: c.job.key, url: c.job.url, result }),
            None => self.current = Some(c),
        }
    }

    fn start_next(&mut self, now: Instant, out: &mut Vec<Finished<K>>) {
        let mut ready = self.engine.prepare();
        if ready == Readiness::Waiting {
            let since = *self.preparing.get_or_insert(now);
            if now < since + self.timing.shot {
                return;
            }
            ready = Readiness::Unavailable;
        }
        self.preparing = None;
        while let Some(job) = self.queue.pop_front() {
            let page = if ready == Readiness::Ready { self.engine.open(&job.url, PAGE_SIZE) } else { None };
            match page {
                Some(page) => {
                    self.current = Some(Current { job, page, stage: Stage::Loading, since: now });
                    return;
                }
                None => out.push(Finished { key: job.key, url: job.url, result: Err(Failure::Unavailable) }),
            }
        }
    }
}

/// The picture WebKit handed over, as RGBA. `None` when it can't be read.
pub fn decode_tiff(data: &[u8]) -> Option<image::RgbaImage> {
    use tiff::ColorType;
    use tiff::decoder::{Decoder, DecodingResult};
    let mut d = Decoder::new(std::io::Cursor::new(data)).ok()?;
    let (w, h) = d.dimensions().ok()?;
    let color = d.colortype().ok()?;
    let data: Vec<u8> = match d.read_image().ok()? {
        DecodingResult::U8(v) => v,
        DecodingResult::U16(v) => v.into_iter().map(|x| (x >> 8) as u8).collect(),
        _ => return None,
    };
    let rgba: Vec<u8> = match color {
        ColorType::RGBA(_) => data,
        ColorType::RGB(_) => data.as_chunks::<3>().0.iter().flat_map(|[r, g, b]| [*r, *g, *b, 255]).collect(),
        _ => return None,
    };
    image::RgbaImage::from_raw(w, h, rgba)
}

/// The engine on platforms without WebKit: every page fails.
#[cfg(not(target_os = "macos"))]
pub struct NoEngine;

#[cfg(not(target_os = "macos"))]
pub enum NoPage {}

#[cfg(not(target_os = "macos"))]
impl Page for NoPage {
    fn next_event(&mut self) -> Option<PageEvent> {
        match *self {}
    }
    fn snapshot(&mut self) {
        match *self {}
    }
}

#[cfg(not(target_os = "macos"))]
impl Engine for NoEngine {
    type Page = NoPage;
    fn prepare(&mut self) -> Readiness {
        Readiness::Unavailable
    }
    fn open(&mut self, _url: &str, _size: (u32, u32)) -> Option<NoPage> {
        None
    }
}

#[cfg(test)]
mod tests;
