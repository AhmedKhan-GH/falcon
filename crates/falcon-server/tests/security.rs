//! BEC-1: the local API's security rules end to end, over a small library in
//! a temp folder, with raw requests so any `Host`, `Origin` and
//! `Sec-Fetch-Site` header can be sent. Only loopback fixtures are used; no
//! other service or network is contacted.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use falcon_library::Library;
use falcon_server::Handle;
use serde_json::{Value, json};

/// Serializes the tests that count temp directories.
static TEMP_LOCK: Mutex<()> = Mutex::new(());

const EXT: &str = "chrome-extension://abcdefghijklmnop";

struct Env {
    _tmp: tempfile::TempDir,
    lib: falcon_server::Shared,
    server: Handle,
    open: String,
    secret: String,
}

fn png_data_url() -> String {
    use base64::Engine;
    let mut buf = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(8, 8, image::Rgb([200, 30, 30])).write_to(&mut buf, image::ImageFormat::Png).unwrap();
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(buf.into_inner()))
}

fn env() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = Library::create(tmp.path().join("Sec.library")).unwrap();
    let open = lib.create_folder(None, "Open").unwrap();
    let secret = lib.create_folder(None, "Secret").unwrap();
    lib.set_folder_password(&secret, Some(("pw", "hint"))).unwrap();
    let lib: falcon_server::Shared = Arc::new(Mutex::new(lib));
    let server = falcon_server::start(lib.clone(), 41800..41850, || {}).unwrap();
    Env { _tmp: tmp, lib, server, open, secret }
}

struct Reply {
    code: u16,
    head: String,
    body: String,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or(Value::Null)
    }
    fn has(&self, header: &str) -> bool {
        self.head.contains(&format!("\r\n{}:", header.to_lowercase()))
    }
    fn header(&self, name: &str) -> String {
        self.head.lines().find_map(|l| l.strip_prefix(&format!("{}:", name.to_lowercase()))).map(|v| v.trim().to_string()).unwrap_or_default()
    }
}

impl Env {
    fn port(&self) -> u16 {
        self.server.port
    }

    /// A request with arbitrary extra headers; `host` defaults to the server's own.
    fn send(&self, method: &str, path: &str, headers: &[(&str, &str)], body: Option<&str>) -> Reply {
        raw(self.port(), method, path, headers, body)
    }

    fn items(&self) -> usize {
        self.lib.lock().unwrap().items().len()
    }
}

fn raw(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: Option<&str>) -> Reply {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let mut req = format!("{method} {path} HTTP/1.1\r\n");
    if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host")) {
        req.push_str(&format!("Host: 127.0.0.1:{port}\r\n"));
    }
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("Connection: close\r\n");
    match body {
        Some(b) => req.push_str(&format!("Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{b}", b.len())),
        None => req.push_str("\r\n"),
    }
    s.write_all(req.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let text = String::from_utf8_lossy(&out).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    Reply { code: head.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0), head: head.to_lowercase(), body: body.to_string() }
}

fn web(origin: &str) -> [(&str, &str); 2] {
    [("Origin", origin), ("Sec-Fetch-Site", "cross-site")]
}

#[test]
fn listens_on_loopback_only() {
    let e = env();
    assert!(TcpStream::connect(("127.0.0.1", e.port())).is_ok());
    // The machine's outward address, found without sending anything.
    let probe = std::net::UdpSocket::bind("0.0.0.0:0").and_then(|s| s.connect("192.0.2.1:9").map(|_| s));
    if let Ok(s) = probe
        && let Ok(addr) = s.local_addr()
        && !addr.ip().is_loopback()
        && !addr.ip().is_unspecified()
    {
        let r = TcpStream::connect_timeout(&(addr.ip(), e.port()).into(), std::time::Duration::from_millis(500));
        assert!(r.is_err(), "the API must not answer on {}", addr.ip());
    }
}

#[test]
fn unexpected_hosts_are_rejected_including_rebinding() {
    let e = env();
    let p = e.port();
    for host in ["evil.example", &format!("evil.example:{p}"), &format!("0.0.0.0:{p}"), &format!("127.0.0.1.evil.example:{p}"), "localhost"] {
        let r = e.send("GET", "/api/library/info", &[("Host", host)], None);
        assert_eq!(r.code, 401, "{host}");
        assert!(!r.body.contains("Sec.library"));
    }
    for host in [format!("localhost:{p}"), format!("127.0.0.1:{p}"), format!("[::1]:{p}")] {
        assert_eq!(e.send("GET", "/api/library/info", &[("Host", &host)], None).code, 200, "{host}");
    }
}

#[test]
fn web_pages_get_discovery_only() {
    let e = env();
    for origin in ["https://www.pinterest.com", "https://evil.example", "null", "http://localhost:3000", "http://127.0.0.1:1"] {
        let h = web(origin);
        // Sensitive reads.
        for path in ["/api/library/info", "/api/library/history", "/api/folder/list", "/api/item/list", "/api/tag/all", "/api/v2/library/info"] {
            let r = e.send("GET", path, &h, None);
            assert_eq!(r.code, 403, "{origin} {path}");
            assert!(!r.has("access-control-allow-origin"), "no CORS grant for refused requests");
            assert!(!r.body.contains("Sec.library") && !r.body.contains(&e.open), "nothing leaks");
        }
        // State changes (CSRF).
        let r = e.send("POST", "/api/folder/create", &h, Some(r#"{"folderName":"Pwned"}"#));
        assert_eq!(r.code, 403, "{origin}");
        let r = e.send("POST", "/api/item/addFromURL", &h, Some(&json!({"url": png_data_url()}).to_string()));
        assert_eq!(r.code, 403, "{origin}");
        let r = e.send("POST", "/api/item/addFromPath", &h, Some(r#"{"path":"/etc/hosts"}"#));
        assert_eq!(r.code, 403, "{origin}");
    }
    assert_eq!(e.items(), 0);
    assert!(e.lib.lock().unwrap().all_folders().iter().all(|f| f.name != "Pwned"));
}

#[test]
fn cross_site_requests_without_an_origin_are_not_trusted() {
    let e = env();
    // An <img src> or no-cors fetch carries Sec-Fetch-Site but no Origin.
    for site in ["cross-site", "same-site"] {
        let r = e.send("GET", "/api/preferences/collect/on", &[("Sec-Fetch-Site", site)], None);
        assert_eq!(r.code, 403, "{site}");
        assert_eq!(e.server.take_preferences(), None, "the state-changing GET did not run");
        assert_eq!(e.send("GET", "/api/library/info", &[("Sec-Fetch-Site", site)], None).code, 403);
    }
    // A user typing the URL, and plain programs without browser headers, still work.
    assert_eq!(e.send("GET", "/api/library/info", &[("Sec-Fetch-Site", "none")], None).code, 200);
    assert_eq!(e.send("GET", "/api/library/info", &[], None).code, 200);
}

#[test]
fn discovery_is_minimal_and_never_leaks_secrets() {
    let e = env();
    e.server.set_token("tok-123");
    e.server.set_preferences(json!({"general": {"showCollectModal": "false"}, "developer": {"apiToken": "tok-123"}, "autoImport": {"path": "/Users/me/Inbox", "enable": "true"}}));
    let page = e.send("GET", "/", &web("https://evil.example"), None);
    assert_eq!(page.code, 200);
    assert_eq!(page.header("access-control-allow-origin"), "*", "discovery carries nothing sensitive");
    let d = page.json()["data"].clone();
    assert_eq!(d["preferences"], json!({"general": {"showCollectModal": "false"}}), "a web page gets the minimal answer");
    assert_eq!(d["application"], "Falcon");
    for r in [page, e.send("GET", "/api/application/info", &[], None), e.send("GET", "/api/v2/app/info", &[], None)] {
        assert!(!r.body.contains("tok-123"), "the Developer token never appears in discovery");
        assert!(!r.body.contains("/Users/me"), "no filesystem paths in discovery");
        assert!(!r.body.contains("Sec.library"));
    }
}

#[test]
fn extensions_need_approval_and_get_no_paths() {
    let _g = TEMP_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let e = env();
    let h = [("Origin", EXT), ("Sec-Fetch-Site", "none")];
    let r = e.send("GET", "/api/library/info", &h, None);
    assert_eq!(r.code, 403);
    assert_eq!(e.server.blocked_origin().as_deref(), Some(EXT), "the refusal is recorded for the user to approve");

    e.server.set_approved_origins(&[EXT.to_string(), "https://evil.example".to_string()]);
    let r = e.send("GET", "/api/library/info", &h, None);
    assert_eq!(r.code, 200);
    assert_eq!(r.header("access-control-allow-origin"), EXT, "the exact origin, never a wildcard");
    assert_eq!(r.header("vary"), "origin");
    // A web origin can't be approved.
    assert_eq!(e.send("GET", "/api/library/info", &web("https://evil.example"), None).code, 403);
    // Another extension is still refused.
    assert_eq!(e.send("GET", "/api/library/info", &[("Origin", "chrome-extension://other")], None).code, 403);
    // An approved extension can import from a URL, but never from a path, nor switch to an arbitrary library.
    let r = e.send("POST", "/api/item/addFromURL", &h, Some(&json!({"url": png_data_url(), "folderId": e.open}).to_string()));
    assert_eq!(r.code, 200, "{}", r.body);
    assert_eq!(e.items(), 1);
    for (path, body) in [
        ("/api/item/addFromPath", r#"{"path":"/etc/hosts"}"#),
        ("/api/item/addFromPaths", r#"{"paths":["/etc/hosts"]}"#),
        ("/api/v2/item/add", r#"{"path":"/etc/hosts"}"#),
        ("/api/v2/item/add", r#"{"items":[{"path":"/etc/hosts"}]}"#),
        ("/api/library/switch", r#"{"libraryPath":"/tmp/other.library"}"#),
    ] {
        assert_eq!(e.send("POST", path, &h, Some(body)).code, 403, "{path}");
    }
    assert_eq!(e.items(), 1);

    // The Developer token is explicit authorization for anything.
    e.server.set_token("tok");
    let r = e.send("POST", "/api/item/addFromPath?token=tok", &web("https://evil.example"), Some(r#"{"path":"/no/such/file.png"}"#));
    assert_ne!(r.code, 403, "a valid token is authorization (this one fails later, on the missing file)");
}

#[test]
fn preflight_is_answered_only_for_callers_that_could_be_served() {
    let e = env();
    let preflight = |origin: &str, path: &str| e.send("OPTIONS", path, &[("Origin", origin), ("Sec-Fetch-Site", "cross-site"), ("Access-Control-Request-Method", "POST")], None);
    let r = preflight("https://evil.example", "/api/item/addFromURL");
    assert_eq!(r.code, 403);
    assert!(!r.has("access-control-allow-origin"));
    assert_eq!(preflight(EXT, "/api/item/addFromURL").code, 403, "an unapproved extension");
    e.server.set_approved_origins(&[EXT.to_string()]);
    let r = preflight(EXT, "/api/item/addFromURL");
    assert_eq!((r.code, r.header("access-control-allow-origin")), (204, EXT.to_string()));
    let r = preflight("https://anything.example", "/");
    assert_eq!((r.code, r.header("access-control-allow-origin")), (204, "*".to_string()), "discovery only");
    assert_eq!(e.send("OPTIONS", "/api/library/info", &[("Host", "evil.example:1")], None).code, 403);
}

#[test]
fn url_import_refuses_private_and_unsupported_destinations() {
    let e = env();
    // A loopback fixture that must never be contacted.
    let fixture = TcpListener::bind("127.0.0.1:0").unwrap();
    let fp = fixture.local_addr().unwrap().port();
    fixture.set_nonblocking(true).unwrap();
    for url in [
        format!("http://127.0.0.1:{fp}/a.png"),
        format!("http://localhost:{fp}/a.png"),
        format!("http://[::1]:{fp}/a.png"),
        format!("http://[::ffff:127.0.0.1]:{fp}/a.png"),
        "http://169.254.169.254/latest/meta-data/x.png".to_string(),
        "http://10.0.0.1/a.png".to_string(),
        "file:///etc/hosts".to_string(),
        "ftp://example.com/a.png".to_string(),
        "http://user:pw@example.com/a.png".to_string(),
    ] {
        let r = e.send("POST", "/api/item/addFromURL", &[], Some(&json!({"url": url}).to_string()));
        assert_eq!(r.code, 500, "{url}: {}", r.body);
        assert_eq!(r.json()["status"], "error");
    }
    assert!(fixture.accept().is_err(), "the loopback fixture was never connected to");
    assert_eq!(e.items(), 0);
}

#[test]
fn data_imports_validate_and_clean_up() {
    let _g = TEMP_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let e = env();
    let temp_dirs = || std::fs::read_dir(std::env::temp_dir()).unwrap().filter(|d| d.as_ref().unwrap().file_name().to_string_lossy().starts_with("falcon-import-")).count();
    let before = temp_dirs();

    // Invalid content: html under an image type, a text type, not base64.
    use base64::Engine;
    let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
    for url in [format!("data:image/png;base64,{}", b64(b"<html>not an image</html>")), "data:text/html;base64,PGh0bWw+".to_string(), "data:image/png;base64,@@@".to_string()] {
        let r = e.send("POST", "/api/item/addFromURL", &[], Some(&json!({"url": url}).to_string()));
        assert_eq!(r.code, 500, "{url}");
    }
    assert_eq!(e.items(), 0);

    // Unsafe names are only display text: they never reach the filesystem.
    let marker = std::env::temp_dir().join("falcon-evil-marker.png");
    let _ = std::fs::remove_file(&marker);
    for name in ["../../falcon-evil-marker", "..\\..\\x", "a/b/c", ".."] {
        let r = e.send("POST", "/api/item/addFromURL", &[], Some(&json!({"url": png_data_url(), "name": name}).to_string()));
        assert_eq!(r.code, 200, "{name}: {}", r.body);
    }
    let names: Vec<String> = e.lib.lock().unwrap().items().iter().map(|i| i.name.clone()).collect();
    assert_eq!(names.len(), 4);
    for n in &names {
        assert!(!n.contains('/') && !n.contains('\\') && n != "..", "{n}");
    }
    assert!(names.contains(&"falcon-evil-marker".to_string()) && names.contains(&"c".to_string()) && names.contains(&"image".to_string()));
    assert!(!marker.exists());
    assert_eq!(temp_dirs(), before, "every import cleaned up its temporary directory");
    // Library isolation: an unknown folder id is dropped, not created or guessed.
    let r = e.send("POST", "/api/item/addFromURL", &[], Some(&json!({"url": png_data_url(), "folderId": "NOT-A-FOLDER"}).to_string()));
    assert_eq!(r.code, 200);
    let lib = e.lib.lock().unwrap();
    assert!(lib.items().last().unwrap().folders.is_empty());
}

#[test]
fn locked_folders_refuse_imports_until_unlocked() {
    let _g = TEMP_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let e = env();
    let body = json!({"url": png_data_url(), "folderId": e.secret}).to_string();
    for (path, b) in [
        ("/api/item/addFromURL", body.clone()),
        ("/api/v2/item/add", json!({"url": png_data_url(), "folders": [e.secret]}).to_string()),
        ("/api/item/addFromURLs", json!({"items": [{"url": png_data_url(), "folderId": e.secret}]}).to_string()),
    ] {
        let _ = e.send("POST", path, &[], Some(&b));
        assert_eq!(e.items(), 0, "{path}: nothing imported into a locked folder");
    }
    let r = e.send("POST", "/api/item/addFromURL", &[], Some(&body));
    assert!(r.json()["data"].as_str().unwrap().contains("password-protected"), "{}", r.body);
    assert_eq!(e.send("POST", "/api/folder/unlock", &[], Some(&json!({"folderId": e.secret, "password": "pw"}).to_string())).code, 200);
    assert_eq!(e.send("POST", "/api/item/addFromURL", &[], Some(&body)).code, 200);
    assert_eq!(e.items(), 1);
}

#[test]
fn oversized_bodies_are_refused() {
    let e = env();
    let big = format!(r#"{{"url":"data:image/png;base64,{}"}}"#, "A".repeat(49 * 1024 * 1024));
    let r = e.send("POST", "/api/item/addFromURL", &[], Some(&big));
    assert_eq!(r.code, 413);
    assert_eq!(e.items(), 0);
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

#[test]
fn compatibility_listener_is_off_by_default_and_follows_its_handle() {
    let e = env();
    // `start` never opens Eagle's port by itself.
    assert!(e.port() != falcon_server::EAGLE_PORT);
    let port = free_port();
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err(), "nothing listens until it is started");

    let listener = e.server.start_compat(port, || {}).unwrap();
    assert_eq!(listener.port, port);
    let r = raw(port, "GET", "/", &[], None);
    assert_eq!(r.code, 200);
    assert_eq!(r.json()["data"]["application"], "Falcon", "it says what it is");
    assert!(r.json()["data"]["buildVersion"].as_str().unwrap().starts_with("falcon-"));
    // The same rules apply on it, with its own Host check.
    assert_eq!(raw(port, "GET", "/api/library/info", &[("Host", &format!("127.0.0.1:{}", e.port()))], None).code, 401);
    assert_eq!(raw(port, "GET", "/api/library/info", &web("https://evil.example"), None).code, 403);
    assert_eq!(raw(port, "GET", "/api/library/info", &[], None).code, 200);
    // The normal listener keeps working.
    assert_eq!(e.send("GET", "/api/library/info", &[], None).code, 200);

    // A second start on the same port is refused, and the first keeps serving.
    assert!(matches!(e.server.start_compat(port, || {}), Err(falcon_server::CompatError::InUse(_))));
    assert_eq!(raw(port, "GET", "/", &[], None).code, 200);

    drop(listener);
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err(), "disabling closes the listener");
    assert_eq!(e.send("GET", "/api/library/info", &[], None).code, 200, "the main API is unaffected");
}

#[test]
fn compatibility_listener_never_takes_an_occupied_port() {
    let e = env();
    // Occupied on IPv4 (Eagle's own case).
    let v4 = TcpListener::bind("127.0.0.1:0").unwrap();
    let p = v4.local_addr().unwrap().port();
    let err = e.server.start_compat(p, || {}).err().expect("refused");
    assert!(matches!(err, falcon_server::CompatError::InUse(_)), "{err}");
    // The occupant is untouched: it still accepts connections.
    assert!(TcpStream::connect(("127.0.0.1", p)).is_ok());
    // Occupied on IPv6 only.
    if let Ok(v6) = TcpListener::bind("[::1]:0") {
        let p6 = v6.local_addr().unwrap().port();
        assert!(matches!(e.server.start_compat(p6, || {}), Err(falcon_server::CompatError::InUse(_))));
    }
    // The API's own port is never reused.
    assert!(matches!(e.server.start_compat(e.port(), || {}), Err(falcon_server::CompatError::InUse(_))));
}

#[test]
fn request_trace_keeps_metadata_only() {
    let e = env();
    e.server.set_token("tok-secret-123");
    let r = e.send(
        "GET",
        "/api/library/info?token=tok-secret-123&path=/Users/me/private",
        &[("Origin", "https://www.pinterest.com/pin/1?x=y"), ("Sec-Fetch-Site", "cross-site"), ("Sec-Fetch-Mode", "cors"), ("Cookie", "session=abc"), ("Authorization", "Bearer zzz")],
        None,
    );
    assert_eq!(r.code, 200, "a valid token is authorization");
    let _ = e.send("GET", "/api/folder/list", &web("https://evil.example"), None);
    let rows = e.server.recent_requests();
    let dump = format!("{rows:?}");
    for secret in ["tok-secret-123", "/Users/me/private", "session=abc", "Bearer", "zzz", "x=y", "/pin/1"] {
        assert!(!dump.contains(secret), "{secret} must not be recorded: {dump}");
    }
    let last = rows.last().unwrap();
    assert_eq!((last.method.as_str(), last.path.as_str(), last.origin.as_str(), last.status, last.caller.as_str()), ("GET", "/api/folder/list", "https://evil.example", 403, "website"));
    assert!(last.why.contains("not allowed"));
    let first = &rows[rows.len() - 2];
    assert_eq!(first.origin, "https://www.pinterest.com");
    assert!(first.flags.contains("cookie") && first.flags.contains("auth"), "presence only: {}", first.flags);
    assert!(first.fetch.starts_with("cross-site/cors"));
}

fn opaque() -> [(&'static str, &'static str); 3] {
    [("Sec-Fetch-Site", "cross-site"), ("Sec-Fetch-Mode", "no-cors"), ("Sec-Fetch-Dest", "empty")]
}

#[test]
fn opaque_browser_reads_need_a_session_and_are_limited() {
    let e = env();
    // The shape observed from Safari's extension background page (BEC-2A).
    assert_eq!(e.send("GET", "/api/folder/listRecent", &opaque(), None).code, 403, "no session: refused");
    e.server.start_session(600);
    assert!(e.server.session_remaining().unwrap() > 590);

    let r = e.send("GET", "/api/folder/listRecent", &opaque(), None);
    assert_eq!(r.code, 200, "{}", r.body);
    assert!(!r.has("access-control-allow-origin"), "no CORS grant: a page can't read the answer");
    let names: Vec<String> = r.json()["data"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap().to_string()).collect();
    assert!(names.contains(&"Open".to_string()));
    assert!(!names.contains(&"Secret".to_string()), "password-protected folders are never listed");
    let all = e.send("GET", "/api/folder/list", &opaque(), None).json();
    assert!(!all.to_string().contains("Secret") && !all.to_string().contains("password"), "{all}");
    let info = e.send("GET", "/api/library/info", &opaque(), None).json();
    assert!(!info.to_string().contains('/'), "no filesystem path: {info}");
    assert_eq!(e.send("GET", "/api/tag/all", &opaque(), None).code, 200);
    let hist = e.send("GET", "/api/library/history", &opaque(), None).json();
    assert!(!hist.to_string().contains('/'), "{hist}");

    // Everything else stays closed, with or without the session.
    for (m, p, b) in [
        ("GET", "/api/item/list", None),
        ("GET", "/api/item/info?id=x", None),
        ("GET", "/api/v2/library/info", None),
        ("GET", "/api/preferences/collect/on", None),
        ("POST", "/api/folder/create", Some(r#"{"folderName":"X"}"#)),
        ("POST", "/api/item/addFromURL", Some(r#"{"url":"https://example.com/a.png"}"#)),
        ("POST", "/api/library/switch", Some(r#"{"libraryPath":"/tmp/x.library"}"#)),
    ] {
        assert_eq!(e.send(m, p, &opaque(), b).code, 403, "{p}");
    }
    // A page that sends an Origin, or loads the URL as a script or image, never gets it.
    assert_eq!(e.send("GET", "/api/folder/listRecent", &[("Origin", "https://www.pinterest.com"), ("Sec-Fetch-Site", "cross-site"), ("Sec-Fetch-Mode", "cors")], None).code, 403);
    assert_eq!(e.send("GET", "/api/folder/listRecent", &[("Sec-Fetch-Site", "cross-site"), ("Sec-Fetch-Mode", "no-cors"), ("Sec-Fetch-Dest", "script")], None).code, 403);
    assert_eq!(e.items(), 0);

    e.server.stop_session();
    assert_eq!(e.send("GET", "/api/folder/listRecent", &opaque(), None).code, 403, "stopped");
    assert_eq!(e.server.session_remaining(), None);
}

#[test]
fn sessions_expire_and_end_with_the_library() {
    let e = env();
    e.server.start_session(1);
    assert_eq!(e.send("GET", "/api/folder/list", &opaque(), None).code, 200);
    std::thread::sleep(std::time::Duration::from_millis(1300));
    assert_eq!(e.send("GET", "/api/folder/list", &opaque(), None).code, 403, "expired");

    e.server.start_session(600);
    assert_eq!(e.send("GET", "/api/folder/list", &opaque(), None).code, 200);
    // Serving another library ends it.
    let tmp = tempfile::tempdir().unwrap();
    let other: falcon_server::Shared = Arc::new(Mutex::new(Library::create(tmp.path().join("Other.library")).unwrap()));
    e.server.set_library(other);
    assert_eq!(e.send("GET", "/api/folder/list", &opaque(), None).code, 403, "library switched");
    assert_eq!(e.server.session_remaining(), None);
}

#[test]
fn session_imports_wait_for_confirmation() {
    let _g = TEMP_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let e = env();
    let ext = "safari-web-extension://59f6ae21-9544-490d-9862-6e582c28be11";
    let h = [("Origin", ext), ("Sec-Fetch-Site", "none")];
    // No session: the origin can't be approved for one.
    assert!(!e.server.approve_session_origin(ext));
    assert_eq!(e.send("GET", "/api/folder/list", &h, None).code, 403);
    e.server.start_session(600);
    assert!(!e.server.approve_session_origin("https://www.pinterest.com"), "a website origin can't be approved");
    assert_eq!(e.send("GET", "/api/folder/list", &h, None).code, 403, "not approved yet");
    assert!(e.server.approve_session_origin(ext));
    let r = e.send("GET", "/api/folder/list", &h, None);
    assert_eq!(r.code, 200);
    assert_eq!(r.header("access-control-allow-origin"), ext);
    // Mutations and paths are not part of a session.
    assert_eq!(e.send("POST", "/api/folder/create", &h, Some(r#"{"folderName":"X"}"#)).code, 403);
    assert_eq!(e.send("POST", "/api/item/addFromPath", &h, Some(r#"{"path":"/etc/hosts"}"#)).code, 403);

    // An import is held until the user decides in the app.
    let port = e.port();
    let body = json!({"url": png_data_url(), "folderId": e.open, "name": "pin"}).to_string();
    let mut results = Vec::new();
    for accept in [false, true] {
        let b = body.clone();
        let t = std::thread::spawn(move || raw(port, "POST", "/api/item/addFromURL", &[("Origin", ext), ("Sec-Fetch-Site", "none"), ("Referer", "https://id.pinterest.com/pin/1/")], Some(&b)).code);
        let pending = loop {
            let p = e.server.pending_imports();
            if let Some(p) = p.first() {
                break p.clone();
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        let _ = accept;
        assert_eq!(e.items(), 0, "nothing is imported while it waits");
        assert_eq!((pending.count, pending.folder.as_str(), pending.hosts.as_slice()), (1, "Open", ["(embedded image)".to_string()].as_slice()));
        assert!(pending.site.starts_with(ext) && pending.site.contains("https://id.pinterest.com"), "{}", pending.site);
        e.server.resolve_import(pending.id, accept);
        results.push(t.join().unwrap());
        if !accept {
            assert_eq!(e.items(), 0, "declined: nothing committed");
        }
    }
    assert_eq!(results, [403, 200]);
    assert_eq!(e.items(), 1, "confirmed: imported through the normal pipeline");
    assert!(e.server.pending_imports().is_empty());
}

#[test]
fn browser_requests_are_rate_limited_and_batches_bounded() {
    let _g = TEMP_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let e = env();
    let mut limited = 0;
    for _ in 0..300 {
        if e.send("GET", "/", &opaque(), None).code == 429 {
            limited += 1;
        }
    }
    assert!(limited > 0, "a flood is told to slow down");
    assert_eq!(e.send("GET", "/api/library/info", &[], None).code, 200, "local programs are not limited");
    // A batch bigger than the limit is not even offered for confirmation.
    let ext = "chrome-extension://abcdefghijklmnop";
    e.server.start_session(600);
    e.server.approve_session_origin(ext);
    std::thread::sleep(std::time::Duration::from_millis(10_500)); // the window clears
    let items: Vec<Value> = (0..60).map(|i| json!({"url": format!("https://example.com/{i}.png")})).collect();
    let r = e.send("POST", "/api/item/addFromURLs", &[("Origin", ext)], Some(&json!({"items": items}).to_string()));
    assert_eq!(r.code, 403);
    assert!(e.server.pending_imports().is_empty());
}

fn form_post(port: u16, headers: &[(&str, &str)], fields: &[(&str, &str)]) -> Reply {
    let enc = |s: &str| s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect::<String>();
    let body = fields.iter().map(|(k, v)| format!("{}={}", enc(k), enc(v))).collect::<Vec<_>>().join("&");
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let mut req = format!("POST / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n");
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str(&format!("Content-Type: application/x-www-form-urlencoded;charset=UTF-8\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}", body.len()));
    s.write_all(req.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let text = String::from_utf8_lossy(&out).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    Reply { code: head.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0), head: head.to_lowercase(), body: body.to_string() }
}

#[test]
fn legacy_extension_api_saves_only_through_an_approved_session_with_confirmation() {
    let _g = TEMP_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let e = env();
    let port = free_port();
    let _l = e.server.start_legacy(port, || {}).unwrap();
    // Discovery for the older client: minimal, no data.
    let r = raw(port, "GET", "/", &web("https://evil.example"), None);
    assert_eq!(r.code, 200);
    assert_eq!(r.json()["showCollectModal"], false);
    assert!(!r.body.contains("Sec.library"));
    assert_eq!(raw(port, "GET", "/api/library/info", &[], None).code, 404, "only the older API's two calls exist there");

    let ext = "safari-web-extension://59f6ae21-9544-490d-9862-6e582c28be11";
    let src = png_data_url();
    let fields = [("src", src.as_str()), ("title", "../../Pin title"), ("url", "https://id.pinterest.com/pin/1/"), ("folderID", e.open.as_str()), ("type", "image"), ("version", "3.1.19")];
    let h = [("Origin", ext), ("Sec-Fetch-Site", "cross-site"), ("Sec-Fetch-Mode", "no-cors"), ("Sec-Fetch-Dest", "empty")];

    // No session, an unknown extension, or a page without the extension's origin: refused; nothing imported.
    assert_eq!(form_post(port, &h, &fields).code, 403);
    assert_eq!(e.server.blocked_origin().as_deref(), Some(ext), "offered to the user for this session");
    assert_eq!(form_post(port, &opaque(), &fields).code, 403, "no Origin: not the extension");
    assert_eq!(form_post(port, &web("https://id.pinterest.com"), &fields).code, 403);
    e.server.start_session(600);
    assert_eq!(e.items(), 0);
    // Declined, then confirmed. The first request from this extension is held for confirmation too.
    let mut codes = Vec::new();
    for accept in [false, true] {
        let (src2, open, hh) = (src.clone(), e.open.clone(), h);
        let t = std::thread::spawn(move || {
            let fields = [("src", src2.as_str()), ("title", "../../Pin title"), ("url", "https://id.pinterest.com/pin/1/"), ("folderID", open.as_str()), ("type", "image")];
            form_post(port, &hh, &fields).code
        });
        let p = loop {
            if let Some(p) = e.server.pending_imports().first() {
                break p.clone();
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        assert_eq!((p.count, p.folder.as_str()), (1, "Open"));
        assert_eq!(e.items(), 0, "nothing is imported while it waits");
        e.server.resolve_import(p.id, accept);
        codes.push(t.join().unwrap());
    }
    assert_eq!(codes, [403, 200]);
    // Confirming once approved the extension for the rest of the session.
    assert_eq!(raw(e.port(), "GET", "/api/folder/list", &[("Origin", ext), ("Sec-Fetch-Site", "none")], None).code, 200);
    let lib = e.lib.lock().unwrap();
    assert_eq!(lib.items().len(), 1);
    let item = &lib.items()[0];
    assert_eq!(item.name, "Pin title", "the title is display text only, sanitized");
    assert_eq!(item.url, "https://id.pinterest.com/pin/1/", "the source page is kept");
    assert_eq!(item.folders, std::slice::from_ref(&e.open));
    drop(lib);

    // A source that points inside the machine is still refused by the safe download, after confirmation.
    let (open, hh) = (e.open.clone(), h);
    let t = std::thread::spawn(move || form_post(port, &hh, &[("src", "http://127.0.0.1:1/a.png"), ("folderID", open.as_str())]).code);
    let p = loop {
        if let Some(p) = e.server.pending_imports().first() {
            break p.clone();
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    e.server.resolve_import(p.id, true);
    assert_eq!(t.join().unwrap(), 500);
    assert_eq!(e.items(), 1);
    // Stopping the session ends it.
    e.server.stop_session();
    assert_eq!(form_post(port, &h, &fields).code, 403);
}

#[test]
fn legacy_listener_follows_the_same_port_rules() {
    let e = env();
    let port = free_port();
    let l = e.server.start_legacy(port, || {}).unwrap();
    assert!(matches!(e.server.start_legacy(port, || {}), Err(falcon_server::CompatError::InUse(_))));
    drop(l);
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
    assert_eq!(falcon_server::LEGACY_PORT, 41593);
}
