//! The one way the API turns a URL or base64 data into a file to import
//! (BEC-1C). The file then goes through the normal import pipeline
//! (`add_path`), so there is no second importer.
//!
//! Safeguards:
//! * only `http`/`https` (and `data:` URLs, handled here too); no credentials in the URL;
//! * **SSRF**: every address a name resolves to must be a public one
//!   ([`is_public_ip`]: no loopback, private, link-local, CGNAT, multicast,
//!   documentation or reserved ranges, IPv4 or IPv6, including IPv4 hidden in
//!   IPv6). The check lives in the *resolver the connection itself uses*, so
//!   a name can't pass the check and then resolve elsewhere (no DNS-rebinding
//!   race), and every redirect hop is resolved, hence checked, the same way;
//! * redirects bounded, connect and total timeouts, no proxy, no cookies;
//! * a size limit enforced while streaming (and from `Content-Length` up
//!   front), the file written to a private temp directory that is removed on
//!   every failure;
//! * the content type must be one Falcon imports as media or a document (no
//!   HTML, scripts or JSON), and for common formats the file's first bytes
//!   must match the type;
//! * the temp file name is generated (`download.<ext>`); a supplied name is
//!   only ever display text, passed through [`sanitize_name`].
//!
//! Limits: [`FetchPolicy::default`]: 100 MiB, 5 redirects, 10 s to connect,
//! 120 s in total. Supported: [`ALLOWED_EXT`].

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;
use std::time::Duration;

use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{DefaultConnector, NextTimeout};

/// The extensions a URL import may produce.
pub const ALLOWED_EXT: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "avif", "bmp", "tif", "tiff", "ico", "svg", "heic", "heif", "jxl", "apng", "mp4", "m4v", "mov", "webm", "mkv", "avi", "mp3", "wav",
    "m4a", "aac", "ogg", "flac", "pdf", "otf", "ttf", "woff", "woff2", "psd", "eps",
];

#[derive(Clone, Debug)]
pub struct FetchPolicy {
    pub max_bytes: u64,
    pub max_redirects: u32,
    pub connect_timeout: Duration,
    pub total_timeout: Duration,
    /// Tests only: one loopback port that may be fetched (a fixture server).
    pub allow_loopback_port: Option<u16>,
    /// Where the private temp directory is made; the system default if None.
    pub temp_base: Option<PathBuf>,
}

impl Default for FetchPolicy {
    fn default() -> Self {
        FetchPolicy {
            max_bytes: 100 * 1024 * 1024,
            max_redirects: 5,
            connect_timeout: Duration::from_secs(10),
            total_timeout: Duration::from_secs(120),
            allow_loopback_port: None,
            temp_base: None,
        }
    }
}

/// A downloaded file in its own temp directory; dropping it deletes both.
#[derive(Debug)]
pub struct Fetched {
    _dir: tempfile::TempDir,
    pub file: PathBuf,
    pub ext: String,
}

/// Whether an address is one a server-side request may go to.
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => public_v4(v),
        IpAddr::V6(v) => public_v6(v),
    }
}

fn public_v4(a: Ipv4Addr) -> bool {
    let o = a.octets();
    !(a.is_unspecified()
        || a.is_loopback()
        || a.is_private()
        || a.is_link_local()
        || a.is_broadcast()
        || a.is_multicast()
        || o[0] == 0
        || (o[0] == 100 && (64..=127).contains(&o[1])) // CGNAT
        || (o[0] == 192 && o[1] == 0 && o[2] == 0) // IETF protocol assignments
        || (o[0] == 192 && o[1] == 0 && o[2] == 2) // documentation
        || (o[0] == 198 && (o[1] == 18 || o[1] == 19)) // benchmarking
        || (o[0] == 198 && o[1] == 51 && o[2] == 100)
        || (o[0] == 203 && o[1] == 0 && o[2] == 113)
        || o[0] >= 240) // reserved
}

fn public_v6(a: Ipv6Addr) -> bool {
    if let Some(v4) = a.to_ipv4_mapped() {
        return public_v4(v4);
    }
    let s = a.segments();
    // IPv4-compatible (::a.b.c.d), NAT64 (64:ff9b::/96) and 6to4 (2002::/16) carry an IPv4 address.
    if s[..6] == [0; 6] && !a.is_unspecified() && !a.is_loopback() {
        return public_v4(Ipv4Addr::new((s[6] >> 8) as u8, s[6] as u8, (s[7] >> 8) as u8, s[7] as u8));
    }
    if s[0] == 0x64 && s[1] == 0xff9b && s[2..6] == [0; 4] {
        return public_v4(Ipv4Addr::new((s[6] >> 8) as u8, s[6] as u8, (s[7] >> 8) as u8, s[7] as u8));
    }
    if s[0] == 0x2002 {
        return public_v4(Ipv4Addr::new((s[1] >> 8) as u8, s[1] as u8, (s[2] >> 8) as u8, s[2] as u8));
    }
    !(a.is_unspecified()
        || a.is_loopback()
        || a.is_multicast()
        || (s[0] & 0xfe00) == 0xfc00 // unique local
        || (s[0] & 0xffc0) == 0xfe80 // link-local
        || (s[0] & 0xffc0) == 0xfec0 // deprecated site-local
        || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
        || (s[0] == 0x2001 && s[1] == 0) // Teredo
        || (s[0] == 0x0100 && s[1..4] == [0; 3])) // discard-only
}

/// Resolves like the default resolver, then refuses any name with an
/// address that isn't public. ureq connects only to what this returns.
#[derive(Debug)]
struct GuardedResolver {
    allow_loopback_port: Option<u16>,
}

impl Resolver for GuardedResolver {
    fn resolve(&self, uri: &ureq::http::Uri, config: &ureq::config::Config, timeout: NextTimeout) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let found = DefaultResolver::default().resolve(uri, config, timeout)?;
        let ok = found.iter().all(|a| is_public_ip(a.ip()) || (a.ip().is_loopback() && Some(a.port()) == self.allow_loopback_port));
        if ok {
            Ok(found)
        } else {
            Err(ureq::Error::Other("destination address is not allowed".into()))
        }
    }
}

fn media_type_ext(mime: &str) -> Option<&'static str> {
    Some(match mime {
        "image/png" | "image/apng" => "png",
        "image/jpeg" | "image/jpg" | "image/pjpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/avif" => "avif",
        "image/bmp" => "bmp",
        "image/tiff" => "tiff",
        "image/svg+xml" => "svg",
        "image/heic" => "heic",
        "image/heif" => "heif",
        "image/jxl" => "jxl",
        "image/x-icon" | "image/vnd.microsoft.icon" => "ico",
        "image/vnd.adobe.photoshop" => "psd",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/quicktime" => "mov",
        "audio/mpeg" => "mp3",
        "audio/wav" | "audio/x-wav" => "wav",
        "audio/mp4" => "m4a",
        "audio/ogg" => "ogg",
        "application/pdf" => "pdf",
        "font/ttf" => "ttf",
        "font/otf" => "otf",
        "font/woff" => "woff",
        "font/woff2" => "woff2",
        _ => return None,
    })
}

/// The extension for a response's content type, falling back on the URL's
/// own extension only for generic or unlisted media types.
fn ext_for(content_type: &str, url_path: &str) -> Result<String, String> {
    let mime = content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    if let Some(e) = media_type_ext(&mime) {
        return Ok(e.to_string());
    }
    let generic = matches!(mime.as_str(), "" | "application/octet-stream" | "binary/octet-stream");
    let media = ["image/", "video/", "audio/", "font/"].iter().any(|p| mime.starts_with(p));
    if generic || media {
        let ext = url_ext(url_path);
        if let Some(e) = ext.filter(|e| ALLOWED_EXT.contains(&e.as_str())) {
            return Ok(e);
        }
    }
    Err(format!("Unsupported content type: {}", if mime.is_empty() { "unknown" } else { &mime }))
}

fn url_ext(path: &str) -> Option<String> {
    let name = path.split(['?', '#']).next()?.rsplit('/').next()?;
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    (!ext.is_empty() && ext.len() <= 5 && ext.chars().all(|c| c.is_ascii_alphanumeric())).then_some(ext)
}

/// Checks a file's first bytes against its extension for the common formats.
pub fn check_signature(ext: &str, head: &[u8]) -> Result<(), String> {
    let starts = |p: &[u8]| head.starts_with(p);
    let text = String::from_utf8_lossy(&head[..head.len().min(512)]).to_ascii_lowercase();
    let looks_html = text.trim_start().starts_with("<!doctype html") || text.trim_start().starts_with("<html");
    let ok = match ext {
        "png" | "apng" => starts(b"\x89PNG\r\n\x1a\n"),
        "jpg" | "jpeg" => starts(&[0xff, 0xd8, 0xff]),
        "gif" => starts(b"GIF8"),
        "webp" => head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP",
        "bmp" => starts(b"BM"),
        "pdf" => starts(b"%PDF"),
        "tif" | "tiff" => starts(b"II*\0") || starts(b"MM\0*"),
        "mp4" | "m4v" | "mov" | "m4a" | "heic" | "heif" | "avif" => head.len() >= 12 && &head[4..8] == b"ftyp",
        "webm" | "mkv" => starts(&[0x1a, 0x45, 0xdf, 0xa3]),
        "svg" => text.contains("<svg") && !looks_html,
        "psd" => starts(b"8BPS"),
        _ => true,
    };
    if ok && ext != "svg" && looks_html {
        return Err("The downloaded file is a web page, not the expected file.".into());
    }
    if ok { Ok(()) } else { Err(format!("The downloaded data is not a valid .{ext} file.")) }
}

/// A name that is safe as display text and as a file name component: the
/// last path component, no control characters or reserved symbols, no dot
/// prefix, at most 100 characters (not bytes, so no split character).
pub fn sanitize_name(raw: &str, fallback: &str) -> String {
    let last = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = last.chars().filter(|c| !c.is_control() && !matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0')).collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim_end_matches('.').trim();
    let cut: String = cleaned.chars().take(100).collect();
    let cut = cut.trim();
    if cut.is_empty() { fallback.to_string() } else { cut.to_string() }
}

fn new_dir(policy: &FetchPolicy) -> Result<tempfile::TempDir, String> {
    match &policy.temp_base {
        Some(b) => tempfile::Builder::new().prefix("falcon-import-").tempdir_in(b),
        None => tempfile::Builder::new().prefix("falcon-import-").tempdir(),
    }
    .map_err(|e| e.to_string())
}

/// Writes already-decoded bytes (from a `data:` URL) the same way a download is written.
fn from_bytes(bytes: &[u8], ext: &str, policy: &FetchPolicy) -> Result<Fetched, String> {
    if bytes.len() as u64 > policy.max_bytes {
        return Err("The data is larger than the import limit.".into());
    }
    check_signature(ext, bytes)?;
    let dir = new_dir(policy)?;
    let file = dir.path().join(format!("download.{ext}"));
    std::fs::write(&file, bytes).map_err(|e| e.to_string())?;
    Ok(Fetched { _dir: dir, file, ext: ext.to_string() })
}

/// Decodes a `data:` URL (base64 only) into a temp file.
pub fn from_data_url(url: &str, policy: &FetchPolicy) -> Result<Fetched, String> {
    use base64::Engine;
    let rest = url.strip_prefix("data:").ok_or("Invalid data URL.")?;
    let (meta, data) = rest.split_once(',').ok_or("Invalid data URL.")?;
    let mut parts = meta.split(';');
    let mime = parts.next().unwrap_or("").to_ascii_lowercase();
    if !parts.any(|p| p.eq_ignore_ascii_case("base64")) {
        return Err("Only base64 data URLs are supported.".into());
    }
    let ext = media_type_ext(&mime).ok_or_else(|| format!("Unsupported content type: {mime}"))?;
    if data.len() as u64 > policy.max_bytes / 3 * 4 + 8 {
        return Err("The data is larger than the import limit.".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(data.trim()).map_err(|_| "The data is not valid base64.".to_string())?;
    from_bytes(&bytes, ext, policy)
}

/// Downloads `url` under `policy` into a temp file.
pub fn fetch_url(url: &str, policy: &FetchPolicy) -> Result<Fetched, String> {
    if url.len() > 4096 {
        return Err("The URL is too long.".into());
    }
    let uri: ureq::http::Uri = url.parse().map_err(|_| "Unsupported URL.".to_string())?;
    if !matches!(uri.scheme_str(), Some("http" | "https")) {
        return Err("Unsupported URL.".into());
    }
    match uri.authority() {
        Some(a) if !a.as_str().contains('@') && !a.host().is_empty() => {}
        _ => return Err("Unsupported URL.".into()),
    }
    let config = ureq::Agent::config_builder()
        .proxy(None)
        .max_redirects(policy.max_redirects)
        .max_redirects_will_error(true)
        .timeout_connect(Some(policy.connect_timeout))
        .timeout_global(Some(policy.total_timeout))
        .http_status_as_error(true)
        .build();
    let agent = ureq::Agent::with_parts(config, DefaultConnector::default(), GuardedResolver { allow_loopback_port: policy.allow_loopback_port });
    let mut resp = agent.get(url).call().map_err(|e| format!("Download failed: {e}"))?;

    if let Some(len) = resp.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok())
        && len > policy.max_bytes
    {
        return Err("The file is larger than the import limit.".into());
    }
    let content_type = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let ext = ext_for(&content_type, uri.path())?;

    let dir = new_dir(policy)?;
    let file = dir.path().join(format!("download.{ext}"));
    let mut out = std::fs::File::create(&file).map_err(|e| e.to_string())?;
    let mut reader = resp.body_mut().with_config().limit(policy.max_bytes + 1).reader();
    let (mut total, mut head, mut buf) = (0u64, Vec::new(), [0u8; 64 * 1024]);
    loop {
        let n = reader.read(&mut buf).map_err(|e| {
            let m = e.to_string();
            if m.contains("limit") { "The file is larger than the import limit.".to_string() } else { format!("Download failed: {m}") }
        })?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > policy.max_bytes {
            return Err("The file is larger than the import limit.".into());
        }
        if head.len() < 512 {
            head.extend_from_slice(&buf[..n.min(512 - head.len())]);
        }
        out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
    }
    out.flush().map_err(|e| e.to_string())?;
    if total == 0 {
        return Err("The download was empty.".into());
    }
    check_signature(&ext, &head)?;
    Ok(Fetched { _dir: dir, file, ext })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDRxxxxxxxxxxxxxxxxxxxxxxxx";

    fn ip(s: &str) -> bool {
        is_public_ip(s.parse().unwrap())
    }

    #[test]
    fn private_and_special_addresses_are_refused() {
        for a in [
            "127.0.0.1", "10.0.0.5", "172.16.3.4", "172.31.255.255", "192.168.1.1", "169.254.169.254", "0.0.0.0", "100.64.0.1", "224.0.0.1", "255.255.255.255", "240.0.0.1",
            "192.0.2.1", "198.51.100.7", "203.0.113.9", "198.18.0.1", "::1", "::", "fe80::1", "fc00::1", "fd12:3456::1", "ff02::1", "2001:db8::1", "::ffff:127.0.0.1",
            "::ffff:10.0.0.1", "::127.0.0.1", "64:ff9b::7f00:1", "2002:7f00:1::1", "2002:c0a8:101::1", "fec0::1",
        ] {
            assert!(!ip(a), "{a} must be refused");
        }
    }

    #[test]
    fn public_addresses_are_allowed() {
        for a in ["93.184.216.34", "8.8.8.8", "1.1.1.1", "2606:4700:4700::1111", "2001:4860:4860::8888", "::ffff:8.8.8.8", "64:ff9b::808:808"] {
            assert!(ip(a), "{a} must be allowed");
        }
    }

    #[test]
    fn content_types_and_extensions() {
        assert_eq!(ext_for("image/png", "/a"), Ok("png".into()));
        assert_eq!(ext_for("image/jpeg; charset=x", "/a.bin"), Ok("jpg".into()));
        assert_eq!(ext_for("application/octet-stream", "/x/y.webp?token=1"), Ok("webp".into()));
        assert_eq!(ext_for("", "/x/y.png"), Ok("png".into()));
        for bad in ["text/html", "application/json", "application/javascript", "text/plain", "application/x-msdownload"] {
            assert!(ext_for(bad, "/a.png").is_err(), "{bad}");
        }
        assert!(ext_for("application/octet-stream", "/run.exe").is_err());
        assert!(ext_for("application/octet-stream", "/noext").is_err());
    }

    #[test]
    fn signatures_are_checked() {
        assert!(check_signature("png", PNG).is_ok());
        assert!(check_signature("png", b"<html>nope").is_err());
        assert!(check_signature("jpg", &[0xff, 0xd8, 0xff, 0xe0]).is_ok());
        assert!(check_signature("gif", b"GIF89a").is_ok());
        assert!(check_signature("svg", b"<?xml?><svg xmlns='x'/>").is_ok());
        assert!(check_signature("svg", b"<!DOCTYPE html><html><svg>").is_err());
        assert!(check_signature("ttf", b"\0\x01\0\0anything").is_ok(), "formats without a check pass");
        assert!(check_signature("ttf", b"<!doctype html><html>").is_err(), "but never a web page");
    }

    #[test]
    fn names_are_sanitized() {
        assert_eq!(sanitize_name("../../etc/passwd", "image"), "passwd");
        assert_eq!(sanitize_name("..\\..\\win.ini", "image"), "win.ini");
        assert_eq!(sanitize_name("..", "image"), "image");
        assert_eq!(sanitize_name("a/b/", "image"), "image");
        assert_eq!(sanitize_name("  .hidden.png ", "image"), "hidden.png");
        assert_eq!(sanitize_name("bad\0na:me*?.png", "image"), "badname.png");
        assert_eq!(sanitize_name("line\nbreak.png", "image"), "linebreak.png");
        let long = "é".repeat(500);
        let n = sanitize_name(&long, "image");
        assert_eq!(n.chars().count(), 100, "cut on characters, never inside one");
        assert_eq!(sanitize_name("猫の写真.png", "image"), "猫の写真.png");
    }

    #[test]
    fn data_urls() {
        use base64::Engine;
        let policy = FetchPolicy::default();
        let b64 = base64::engine::general_purpose::STANDARD.encode(PNG);
        let f = from_data_url(&format!("data:image/png;base64,{b64}"), &policy).unwrap();
        assert!(f.file.ends_with("download.png") && f.file.is_file());
        let dir = f.file.parent().unwrap().to_path_buf();
        drop(f);
        assert!(!dir.exists(), "the temp directory goes with the value");
        assert!(from_data_url("data:text/html;base64,PGh0bWw+", &policy).is_err());
        assert!(from_data_url("data:image/png,rawtext", &policy).is_err());
        assert!(from_data_url(&format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(b"<html>")), &policy).is_err());
        let small = FetchPolicy { max_bytes: 8, ..FetchPolicy::default() };
        assert!(from_data_url(&format!("data:image/png;base64,{b64}"), &small).is_err());
    }

    #[test]
    fn urls_must_be_plain_http() {
        let p = FetchPolicy::default();
        for u in ["file:///etc/passwd", "ftp://x/y.png", "gopher://x", "javascript:alert(1)", "http://user:pw@example.com/a.png", "http:///a.png", "not a url", ""] {
            assert!(fetch_url(u, &p).is_err(), "{u}");
        }
    }

    /// A loopback fixture server; counts the requests it served.
    struct Fixture {
        port: u16,
        hits: Arc<AtomicUsize>,
        server: Arc<tiny_http::Server>,
    }

    impl Fixture {
        fn start(redirect_to: Option<String>) -> Fixture {
            let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
            let port = server.server_addr().to_ip().unwrap().port();
            let hits = Arc::new(AtomicUsize::new(0));
            let (s, h) = (server.clone(), hits.clone());
            std::thread::spawn(move || {
                for req in s.incoming_requests() {
                    h.fetch_add(1, Ordering::SeqCst);
                    let path = req.url().to_string();
                    let ct = |v: &str| tiny_http::Header::from_bytes("Content-Type", v).unwrap();
                    let resp = match path.as_str() {
                        "/ok.png" => tiny_http::Response::from_data(PNG.to_vec()).with_header(ct("image/png")).boxed(),
                        "/page.png" => tiny_http::Response::from_data(b"<html>hi</html>".to_vec()).with_header(ct("image/png")).boxed(),
                        "/page" => tiny_http::Response::from_data(b"<html>hi</html>".to_vec()).with_header(ct("text/html")).boxed(),
                        "/big.png" => {
                            let mut v = PNG.to_vec();
                            v.resize(3 * 1024 * 1024, 0);
                            tiny_http::Response::from_data(v).with_header(ct("image/png")).boxed()
                        }
                        "/redir" => tiny_http::Response::empty(302)
                            .with_header(tiny_http::Header::from_bytes("Location", redirect_to.clone().unwrap_or_else(|| "/redir".into())).unwrap())
                            .boxed(),
                        _ => tiny_http::Response::from_data(b"x".to_vec()).with_status_code(404).boxed(),
                    };
                    let _ = req.respond(resp);
                }
            });
            Fixture { port, hits, server }
        }
        fn url(&self, p: &str) -> String {
            format!("http://127.0.0.1:{}{p}", self.port)
        }
        fn hits(&self) -> usize {
            self.hits.load(Ordering::SeqCst)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.server.unblock();
        }
    }

    fn test_policy(fx: &Fixture, base: &std::path::Path) -> FetchPolicy {
        FetchPolicy { allow_loopback_port: Some(fx.port), temp_base: Some(base.to_path_buf()), max_bytes: 1024 * 1024, max_redirects: 3, ..FetchPolicy::default() }
    }

    #[test]
    fn strict_policy_never_connects_to_loopback() {
        let fx = Fixture::start(None);
        let base = tempfile::tempdir().unwrap();
        let strict = FetchPolicy { temp_base: Some(base.path().to_path_buf()), ..FetchPolicy::default() };
        assert!(fetch_url(&fx.url("/ok.png"), &strict).is_err());
        assert!(fetch_url(&format!("http://localhost:{}/ok.png", fx.port), &strict).is_err());
        assert!(fetch_url(&format!("http://[::1]:{}/ok.png", fx.port), &strict).is_err());
        assert!(fetch_url(&format!("http://[::ffff:127.0.0.1]:{}/ok.png", fx.port), &strict).is_err());
        assert!(fetch_url(&format!("http://2130706433:{}/ok.png", fx.port), &strict).is_err(), "decimal IP form");
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(fx.hits(), 0, "the fixture must never have been contacted");
        assert_eq!(std::fs::read_dir(base.path()).unwrap().count(), 0);
    }

    #[test]
    fn downloads_validates_and_cleans_up() {
        let fx = Fixture::start(None);
        let base = tempfile::tempdir().unwrap();
        let p = test_policy(&fx, base.path());
        let f = fetch_url(&fx.url("/ok.png"), &p).unwrap();
        assert_eq!(f.ext, "png");
        assert_eq!(std::fs::read(&f.file).unwrap(), PNG);
        assert!(f.file.starts_with(base.path()), "inside the designated temp directory");
        drop(f);
        assert_eq!(std::fs::read_dir(base.path()).unwrap().count(), 0, "success cleans up too");

        for (path, why) in [("/page", "html content type"), ("/page.png", "html body under an image type"), ("/big.png", "oversized"), ("/missing.png", "404")] {
            assert!(fetch_url(&fx.url(path), &p).is_err(), "{why}");
            assert_eq!(std::fs::read_dir(base.path()).unwrap().count(), 0, "{why}: nothing left behind");
        }
    }

    #[test]
    fn redirects_are_revalidated_and_bounded() {
        let target = Fixture::start(None);
        let base = tempfile::tempdir().unwrap();
        // The first fixture is allowed; the one it redirects to is not.
        let hop = Fixture::start(Some(target.url("/ok.png")));
        let p = test_policy(&hop, base.path());
        assert!(fetch_url(&hop.url("/redir"), &p).is_err(), "a redirect to another loopback port is refused");
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(target.hits(), 0, "the redirect target was never contacted");
        let looping = Fixture::start(None);
        let p = test_policy(&looping, base.path());
        let err = fetch_url(&looping.url("/redir"), &p).unwrap_err();
        assert!(looping.hits() <= 5, "bounded: {} hits ({err})", looping.hits());
        assert_eq!(std::fs::read_dir(base.path()).unwrap().count(), 0);
    }
}
