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
