use std::cell::RefCell;
use std::rc::Rc;

use super::*;

/// What the fake engine was asked to do, and what its pages will report.
#[derive(Default)]
struct Log {
    opened: Vec<String>,
    open_now: usize,
    most_open: usize,
    snapshots: usize,
    dropped: usize,
    events: VecDeque<PageEvent>,
}

struct Fake {
    log: Rc<RefCell<Log>>,
    readiness: Readiness,
}

struct FakePage(Rc<RefCell<Log>>);

impl Page for FakePage {
    fn next_event(&mut self) -> Option<PageEvent> {
        self.0.borrow_mut().events.pop_front()
    }
    fn snapshot(&mut self) {
        self.0.borrow_mut().snapshots += 1;
    }
}

impl Drop for FakePage {
    fn drop(&mut self) {
        let mut l = self.0.borrow_mut();
        l.open_now -= 1;
        l.dropped += 1;
    }
}

impl Engine for Fake {
    type Page = FakePage;
    fn prepare(&mut self) -> Readiness {
        self.readiness
    }
    fn open(&mut self, url: &str, size: (u32, u32)) -> Option<FakePage> {
        assert_eq!(size, (1440, 900));
        let mut l = self.log.borrow_mut();
        l.opened.push(url.to_string());
        l.open_now += 1;
        l.most_open = l.most_open.max(l.open_now);
        Some(FakePage(self.log.clone()))
    }
}

const TIMING: Timing = Timing { load: Duration::from_secs(20), settle: Duration::from_millis(2500), shot: Duration::from_secs(10) };

fn shots(readiness: Readiness) -> (Shots<u32, Fake>, Rc<RefCell<Log>>) {
    let log = Rc::new(RefCell::new(Log::default()));
    (Shots::new(Fake { log: log.clone(), readiness }, TIMING), log)
}

fn push(log: &Rc<RefCell<Log>>, event: PageEvent) {
    log.borrow_mut().events.push_back(event);
}

fn secs(s: u64) -> Duration {
    Duration::from_secs(s)
}

#[test]
fn only_http_and_https_are_queued() {
    let (mut s, log) = shots(Readiness::Ready);
    for (i, url) in ["file:///etc/passwd", "javascript:alert(1)", "someapp://open", "ftp://example.com/", "about:blank", "https://", "https://exa mple.com"].iter().enumerate() {
        assert!(!s.enqueue(i as u32, url), "{url}");
    }
    assert!(s.is_idle());
    assert!(s.tick(Instant::now()).is_empty());
    assert!(log.borrow().opened.is_empty());
    assert!(s.enqueue(10, "HTTP://example.com/"));
    assert!(s.enqueue(11, " https://example.com/a?b=1 "));
    s.tick(Instant::now());
    assert_eq!(log.borrow().opened, ["HTTP://example.com/"]);
}

#[test]
fn a_page_goes_from_loading_to_a_picture_after_settling() {
    let (mut s, log) = shots(Readiness::Ready);
    let t = Instant::now();
    assert!(s.enqueue(1, "https://example.com/"));
    assert!(s.tick(t).is_empty());
    assert_eq!(log.borrow().opened.len(), 1);
    // Still loading: no picture is asked for however often it ticks.
    assert!(s.tick(t + secs(5)).is_empty());
    assert_eq!(log.borrow().snapshots, 0);
    // Finished: the page gets its 2.5 seconds.
    push(&log, PageEvent::Finished);
    let done = t + secs(6);
    assert!(s.tick(done).is_empty());
    assert!(s.tick(done + Duration::from_millis(2499)).is_empty());
    assert_eq!(log.borrow().snapshots, 0);
    assert!(s.tick(done + Duration::from_millis(2500)).is_empty());
    assert_eq!(log.borrow().snapshots, 1, "asked for once");
    assert!(s.tick(done + secs(3)).is_empty());
    assert_eq!(log.borrow().snapshots, 1);
    push(&log, PageEvent::Shot(vec![1, 2, 3]));
    let out = s.tick(done + secs(4));
    assert_eq!(out, vec![Finished { key: 1, url: "https://example.com/".into(), result: Ok(vec![1, 2, 3]) }]);
    assert!(s.is_idle());
    assert_eq!(log.borrow().dropped, 1, "the web view goes with the page");
}

#[test]
fn a_page_that_never_finishes_times_out_once_and_is_not_retried() {
    let (mut s, log) = shots(Readiness::Ready);
    let t = Instant::now();
    s.enqueue(1, "https://slow.example/");
    s.tick(t);
    assert!(s.tick(t + secs(19)).is_empty());
    let out = s.tick(t + secs(20));
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].result, Err(Failure::TimedOut));
    assert_eq!(log.borrow().open_now, 0, "the page is stopped");
    for n in 21..200 {
        assert!(s.tick(t + secs(n)).is_empty());
    }
    assert_eq!(log.borrow().opened.len(), 1, "never loaded again");
    // Only asking again loads it again.
    assert!(s.enqueue(1, "https://slow.example/"));
    s.tick(t + secs(201));
    assert_eq!(log.borrow().opened.len(), 2);
}

#[test]
fn a_failed_load_and_a_missing_picture_are_reported() {
    let (mut s, log) = shots(Readiness::Ready);
    let t = Instant::now();
    s.enqueue(1, "https://a.example/");
    s.tick(t);
    push(&log, PageEvent::Failed);
    assert_eq!(s.tick(t + secs(1))[0].result, Err(Failure::Load));

    s.enqueue(2, "https://b.example/");
    s.tick(t + secs(2));
    push(&log, PageEvent::Finished);
    s.tick(t + secs(3));
    s.tick(t + secs(6));
    assert_eq!(log.borrow().snapshots, 1);
    push(&log, PageEvent::ShotFailed);
    assert_eq!(s.tick(t + secs(7))[0].result, Err(Failure::Shot));
}

#[test]
fn a_picture_that_never_comes_times_out() {
    let (mut s, log) = shots(Readiness::Ready);
    let t = Instant::now();
    s.enqueue(1, "https://a.example/");
    s.tick(t);
    push(&log, PageEvent::Finished);
    s.tick(t);
    s.tick(t + secs(3));
    assert!(s.tick(t + secs(12)).is_empty());
    assert_eq!(s.tick(t + secs(13))[0].result, Err(Failure::Shot));
    assert!(s.is_idle());
}

#[test]
fn events_after_finishing_do_not_fail_the_page() {
    let (mut s, log) = shots(Readiness::Ready);
    let t = Instant::now();
    s.enqueue(1, "https://a.example/");
    s.tick(t);
    push(&log, PageEvent::Finished);
    push(&log, PageEvent::Failed);
    push(&log, PageEvent::Finished);
    assert!(s.tick(t).is_empty());
    s.tick(t + secs(3));
    assert_eq!(log.borrow().snapshots, 1);
    // A second Finished did not restart the wait or ask twice.
    s.tick(t + secs(4));
    assert_eq!(log.borrow().snapshots, 1);
}

#[test]
fn one_page_at_a_time_in_order() {
    let (mut s, log) = shots(Readiness::Ready);
    let t = Instant::now();
    for (key, url) in [(1, "https://a.example/"), (2, "https://b.example/"), (3, "https://c.example/")] {
        assert!(s.enqueue(key, url));
    }
    let mut ended = Vec::new();
    for n in 0..200 {
        let now = t + Duration::from_millis(500 * n);
        let opened = log.borrow().opened.len();
        ended.extend(s.tick(now).into_iter().map(|f| f.key));
        if log.borrow().opened.len() > opened {
            push(&log, PageEvent::Finished);
        }
        if log.borrow().snapshots > ended.len() {
            push(&log, PageEvent::Shot(vec![]));
        }
    }
    assert_eq!(ended, [1, 2, 3]);
    assert_eq!(log.borrow().opened, ["https://a.example/", "https://b.example/", "https://c.example/"]);
    assert_eq!(log.borrow().most_open, 1);
    assert_eq!(log.borrow().open_now, 0);
}

#[test]
fn a_key_is_queued_once() {
    let (mut s, log) = shots(Readiness::Ready);
    let t = Instant::now();
    assert!(s.enqueue(1, "https://a.example/"));
    assert!(!s.enqueue(1, "https://a.example/"), "queued");
    s.tick(t);
    assert!(!s.enqueue(1, "https://a.example/"), "open");
    assert_eq!(log.borrow().opened.len(), 1);
}

#[test]
fn waits_for_the_engine_and_gives_up_on_it() {
    let (mut s, log) = shots(Readiness::Waiting);
    let t = Instant::now();
    s.enqueue(1, "https://a.example/");
    s.enqueue(2, "https://b.example/");
    assert!(s.tick(t).is_empty());
    assert_eq!(s.next_wake(t), Some(TIMING.shot));
    assert!(s.tick(t + secs(9)).is_empty());
    assert!(log.borrow().opened.is_empty());
    let out = s.tick(t + secs(10));
    assert_eq!(out.iter().map(|f| (f.key, f.result.clone())).collect::<Vec<_>>(), [(1, Err(Failure::Unavailable)), (2, Err(Failure::Unavailable))]);
    assert!(s.is_idle());
}

#[test]
fn an_engine_that_becomes_ready_opens_the_page() {
    let (mut s, log) = shots(Readiness::Waiting);
    let t = Instant::now();
    s.enqueue(1, "https://a.example/");
    s.tick(t);
    s.engine.readiness = Readiness::Ready;
    s.tick(t + secs(1));
    assert_eq!(log.borrow().opened.len(), 1);
}

#[test]
fn an_unavailable_engine_fails_everything_at_once() {
    let (mut s, log) = shots(Readiness::Unavailable);
    s.enqueue(1, "https://a.example/");
    let out = s.tick(Instant::now());
    assert_eq!(out[0].result, Err(Failure::Unavailable));
    assert!(log.borrow().opened.is_empty());
}

#[test]
fn retain_drops_queued_jobs_and_stops_the_open_page() {
    let (mut s, log) = shots(Readiness::Ready);
    let t = Instant::now();
    s.enqueue(1, "https://a.example/");
    s.enqueue(2, "https://b.example/");
    s.enqueue(3, "https://c.example/");
    s.tick(t);
    s.retain(|k| *k == 3);
    assert_eq!(log.borrow().open_now, 0, "1's page stopped");
    s.tick(t);
    assert_eq!(log.borrow().opened, ["https://a.example/", "https://c.example/"]);
}

#[test]
fn next_wake_follows_the_stage() {
    let (mut s, log) = shots(Readiness::Ready);
    let t = Instant::now();
    assert_eq!(s.next_wake(t), None);
    s.enqueue(1, "https://a.example/");
    s.tick(t);
    assert_eq!(s.next_wake(t + secs(5)), Some(secs(15)));
    assert_eq!(s.next_wake(t + secs(50)), Some(Duration::ZERO));
    push(&log, PageEvent::Finished);
    s.tick(t);
    assert_eq!(s.next_wake(t), Some(TIMING.settle));
    s.tick(t + secs(3));
    assert_eq!(s.next_wake(t + secs(3)), Some(TIMING.shot));
}

#[test]
fn tiff_pictures_are_read() {
    use tiff::encoder::{TiffEncoder, colortype};
    let mut data = std::io::Cursor::new(Vec::new());
    TiffEncoder::new(&mut data).unwrap().write_image::<colortype::RGBA8>(2, 1, &[10, 20, 30, 255, 40, 50, 60, 255]).unwrap();
    let img = decode_tiff(data.get_ref()).unwrap();
    assert_eq!((img.width(), img.height()), (2, 1));
    assert_eq!(img.get_pixel(1, 0).0, [40, 50, 60, 255]);
    let mut data = std::io::Cursor::new(Vec::new());
    TiffEncoder::new(&mut data).unwrap().write_image::<colortype::RGB8>(1, 1, &[1, 2, 3]).unwrap();
    assert_eq!(decode_tiff(data.get_ref()).unwrap().get_pixel(0, 0).0, [1, 2, 3, 255]);
    assert!(decode_tiff(b"not a tiff").is_none());
}

#[test]
fn navigation_schemes() {
    for main in [true, false] {
        assert!(scheme_allowed("http", main));
        assert!(scheme_allowed("HTTPS", main));
        for s in ["file", "javascript", "someapp", "ftp", "mailto"] {
            assert!(!scheme_allowed(s, main), "{s}");
        }
    }
    for s in ["about", "data", "blob"] {
        assert!(!scheme_allowed(s, true), "{s}: a page never navigates there");
        assert!(scheme_allowed(s, false), "{s}: a frame may");
    }
}
