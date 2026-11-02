//! The page screenshot queue against real WebKit and a page served here on
//! a free local port. WebKit needs the process's main thread, so this runs
//! without the test harness; `main` pumps the run loop as the app's event
//! loop does. Nothing is shown on any screen: the window is off every
//! display and the process has no Dock icon.
//!
//! It skips, saying so, when the process has no window server session or
//! WebKit can't load even a local page.

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("skipped: WebKit is macOS only");
}

#[cfg(target_os = "macos")]
fn main() {
    mac::run();
}

#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/page_shot/mod.rs"]
mod page_shot;

#[cfg(target_os = "macos")]
mod mac {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use objc2::MainThreadMarker;
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSDate, NSRunLoop};

    use crate::page_shot::webkit::WebKit;
    use crate::page_shot::{Failure, Finished, Shots, Timing, decode_tiff};

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGSessionCopyCurrentDictionary() -> *const std::ffi::c_void;
    }

    /// A request the server saw: its path and its header lines.
    #[derive(Clone, Debug)]
    struct Request {
        path: String,
        headers: Vec<String>,
    }

    type Log = Arc<Mutex<Vec<Request>>>;

    fn page(port: u16) -> String {
        // Green from a first-party stylesheet, a third-party image (another
        // host name for the same server), a cookie, a dialog and a new window.
        format!(
            r#"<!doctype html><html><head><title>t</title><link rel="stylesheet" href="/first.css"></head><body>
            <img src="http://localhost:{port}/tracker.png">
            <script>document.cookie = "script=1"; localStorage.setItem("a", "b"); alert("hi"); confirm("sure?");
            window.open("http://127.0.0.1:{port}/popup");</script></body></html>"#
        )
    }

    fn respond(mut s: TcpStream, port: u16, log: &Log) {
        let mut buf = [0; 8192];
        let mut text = String::new();
        while !text.contains("\r\n\r\n") {
            match s.read(&mut buf) {
                Ok(0) | Err(_) => return,
                Ok(n) => text.push_str(&String::from_utf8_lossy(&buf[..n])),
            }
        }
        let mut lines = text.lines();
        let path = lines.next().and_then(|l| l.split(' ').nth(1)).unwrap_or("").to_string();
        log.lock().unwrap().push(Request { path: path.clone(), headers: lines.take_while(|l| !l.is_empty()).map(String::from).collect() });
        let (head, body) = match path.as_str() {
            "/slow" => {
                // Never answers; the connection stays open until the test ends.
                std::thread::sleep(Duration::from_secs(600));
                return;
            }
            "/to-file" => ("302 Found\r\nLocation: file:///etc/hosts".to_string(), String::new()),
            "/first.css" => ("200 OK\r\nContent-Type: text/css".to_string(), "html,body{background:rgb(0,160,0)}".to_string()),
            "/one" | "/two" => ("200 OK\r\nContent-Type: text/html\r\nSet-Cookie: seen=1".to_string(), page(port)),
            _ => ("404 Not Found\r\nContent-Type: text/plain".to_string(), String::new()),
        };
        let _ = write!(s, "HTTP/1.1 {head}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    }

    fn serve() -> (u16, Log) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let log = Log::default();
        let seen = log.clone();
        std::thread::spawn(move || {
            for s in listener.incoming().flatten() {
                let seen = seen.clone();
                std::thread::spawn(move || respond(s, port, &seen));
            }
        });
        (port, log)
    }

    /// Ticks and pumps until the queue is empty, or `limit` passes.
    fn drain(shots: &mut Shots<&'static str, WebKit>, limit: Duration) -> Vec<Finished<&'static str>> {
        let start = Instant::now();
        let mut out = Vec::new();
        while !shots.is_idle() && start.elapsed() < limit {
            out.extend(shots.tick(Instant::now()));
            let until = NSDate::dateWithTimeIntervalSinceNow(0.05);
            NSRunLoop::currentRunLoop().runUntilDate(&until);
        }
        out
    }

    pub fn run() {
        // SAFETY: a CoreGraphics query with no arguments; a non-null result
        // is an owned dictionary, leaked for the test's short life.
        if unsafe { CGSessionCopyCurrentDictionary() }.is_null() {
            println!("skipped: no window server session");
            return;
        }
        let mtm = MainThreadMarker::new().expect("main thread");
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        // SAFETY: NSApplicationActivationPolicyProhibited: no Dock icon.
        let _: bool = unsafe { msg_send![&*app, setActivationPolicy: 2isize] };

        let (port, log) = serve();
        let timing = Timing { load: Duration::from_secs(20), settle: Duration::from_millis(300), shot: Duration::from_secs(10) };
        let mut shots = Shots::new(WebKit::new(|| {}), timing);
        let base = format!("http://127.0.0.1:{port}");

        assert!(shots.enqueue("one", &format!("{base}/one")));
        assert!(shots.enqueue("two", &format!("{base}/two")));
        assert!(!shots.enqueue("file", "file:///etc/hosts"));
        let out = drain(&mut shots, Duration::from_secs(60));
        if out.iter().any(|f| matches!(f.result, Err(Failure::Unavailable | Failure::TimedOut))) {
            println!("skipped: WebKit can't load a local page here: {out:?}");
            return;
        }
        assert_eq!(out.len(), 2);
        for f in &out {
            let data = f.result.as_ref().unwrap_or_else(|e| panic!("{}: {e:?}", f.key));
            let img = decode_tiff(data).unwrap_or_else(|| panic!("{}: unreadable picture of {} bytes", f.key, data.len()));
            println!("{}: picture {}x{}", f.key, img.width(), img.height());
            assert_eq!(u64::from(img.width()) * 900, u64::from(img.height()) * 1440, "the page's shape");
            let [r, g, b, a] = img.get_pixel(img.width() / 2, img.height() - 2).0;
            assert!(g > 120 && r < 90 && b < 90 && a == 255, "the page's green, got {r},{g},{b},{a}");
        }

        let requests = log.lock().unwrap().clone();
        let paths: Vec<&str> = requests.iter().map(|r| r.path.as_str()).collect();
        println!("requests: {paths:?}");
        assert!(paths.contains(&"/first.css"), "first-party loads go ahead");
        assert!(!paths.contains(&"/tracker.png"), "third-party loads are blocked");
        assert!(!paths.contains(&"/popup"), "no new windows");
        for r in &requests {
            let has = |name: &str| r.headers.iter().any(|h| h.to_ascii_lowercase().starts_with(name));
            assert!(!has("cookie:"), "no cookie reaches {}: {:?}", r.path, r.headers);
            assert!(!r.headers.iter().any(|h| h.to_ascii_lowercase().contains("falcon")), "{:?}", r.headers);
        }
        let two = requests.iter().find(|r| r.path == "/two").unwrap();
        assert!(!two.headers.iter().any(|h| h.to_ascii_lowercase().starts_with("referer:")), "{:?}", two.headers);

        // A redirect to a file is refused, and a page that never answers
        // times out, once each.
        let quick = Timing { load: Duration::from_secs(2), ..timing };
        let mut shots = Shots::new(WebKit::new(|| {}), quick);
        assert!(shots.enqueue("slow", &format!("{base}/slow")));
        assert!(shots.enqueue("to-file", &format!("{base}/to-file")));
        let out = drain(&mut shots, Duration::from_secs(30));
        assert_eq!(out.iter().map(|f| (f.key, f.result.clone().map(|_| ()))).collect::<Vec<_>>(), [("slow", Err(Failure::TimedOut)), ("to-file", Err(Failure::Load))]);
        let slows = log.lock().unwrap().iter().filter(|r| r.path == "/slow").count();
        assert_eq!(slows, 1, "not retried");

        // No window is left behind.
        let left: Vec<Retained<AnyObject>> = app.windows().iter().filter(|w| w.isVisible()).map(|w| Retained::into_super(Retained::into_super(Retained::into_super(w)))).collect();
        assert!(left.is_empty(), "{} windows left", left.len());
        println!("ok");
    }
}
