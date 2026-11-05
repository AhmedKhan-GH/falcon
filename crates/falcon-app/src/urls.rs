//! The inspector's URL field: which typed values are kept, and how a stored
//! link is opened (a file path or a web address).

use std::path::PathBuf;

/// Where a stored link points.
#[derive(Debug, PartialEq, Eq)]
pub enum LinkTarget {
    File(PathBuf),
    Web(String),
}

fn starts_with_ci(s: &str, prefix: &str) -> bool {
    s.get(..prefix.len()).is_some_and(|p| p.eq_ignore_ascii_case(prefix))
}

/// The text after an optional `http://`, `https://` or `ftp://`.
fn strip_scheme(s: &str) -> &str {
    for p in ["https://", "http://", "ftp://"] {
        if starts_with_ci(s, p) {
            return &s[p.len()..];
        }
    }
    s
}

fn has_scheme(s: &str) -> bool {
    strip_scheme(s).len() != s.len()
}

/// A dotted IPv4 address that is public (not private, loopback or link-local).
fn public_ipv4(host: &str) -> Option<bool> {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() != 4 || parts.iter().any(|p| p.is_empty() || p.len() > 3 || !p.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    let o: Vec<u16> = parts.iter().map(|p| p.parse().unwrap_or(999)).collect();
    if o.iter().any(|&n| n > 255) {
        return Some(false);
    }
    let private = o[0] == 10 || o[0] == 127 || (o[0] == 169 && o[1] == 254) || (o[0] == 192 && o[1] == 168) || (o[0] == 172 && (16..=31).contains(&o[1]));
    Some(!private && (1..=223).contains(&o[0]))
}

fn hostname(host: &str) -> bool {
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    let ok = labels.iter().all(|l| !l.is_empty() && !l.starts_with('-') && !l.ends_with('-') && l.chars().all(|c| c.is_alphanumeric() || c == '-'));
    let tld = labels[labels.len() - 1];
    ok && tld.chars().count() >= 2 && tld.chars().all(char::is_alphabetic)
}

/// Whether `s` looks like a web address: a host (name or public IPv4) with an
/// optional scheme, port and path, and no whitespace.
pub fn is_web_url(s: &str) -> bool {
    if s.is_empty() || s.chars().any(char::is_whitespace) {
        return false;
    }
    let rest = strip_scheme(s);
    let end = rest.find([':', '/']).unwrap_or(rest.len());
    let (host, tail) = rest.split_at(end);
    let host_ok = match public_ipv4(host) {
        Some(public) => public,
        None => hostname(host),
    };
    if !host_ok {
        return false;
    }
    let tail = match tail.strip_prefix(':') {
        Some(t) => {
            let digits = t.bytes().take_while(u8::is_ascii_digit).count();
            if !(2..=5).contains(&digits) {
                return false;
            }
            &t[digits..]
        }
        None => tail,
    };
    tail.is_empty() || tail.starts_with('/')
}

/// Whether the inspector saves a typed URL; otherwise the field reverts.
pub fn url_accepted(text: &str, exists: impl Fn(&str) -> bool) -> bool {
    text.is_empty() || starts_with_ci(text, "file://") || is_web_url(text) || exists(text) || text.contains("://")
}

/// Decodes `%XX` escapes as UTF-8; a malformed escape stays as typed and
/// invalid UTF-8 is replaced.
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = ((b[i + 1] as char).to_digit(16), (b[i + 2] as char).to_digit(16))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// What a stored link opens, if anything.
pub fn link_target(url: &str, exists: impl Fn(&str) -> bool) -> Option<LinkTarget> {
    if starts_with_ci(url, "file://") {
        return Some(LinkTarget::File(file_url_path(&percent_decode(&url["file://".len()..]))));
    }
    if is_web_url(url) {
        return Some(LinkTarget::Web(if has_scheme(url) { url.to_string() } else { format!("http://{url}") }));
    }
    if exists(url) {
        return Some(LinkTarget::File(PathBuf::from(url)));
    }
    url.contains("://").then(|| LinkTarget::Web(url.to_string()))
}

/// The path a `file://` address names, after what its `//` is followed by.
#[cfg(not(windows))]
fn file_url_path(rest: &str) -> PathBuf {
    PathBuf::from(rest)
}

/// The path a `file://` address names: `/C:/a` is `C:\a`, and `host/share/a`
/// (with no leading slash) is `\\host\share\a`.
#[cfg(windows)]
fn file_url_path(rest: &str) -> PathBuf {
    let rest = rest.strip_prefix("localhost/").map_or_else(|| rest.to_string(), |r| format!("/{r}"));
    let slashes = rest.replace('/', "\\");
    match slashes.strip_prefix('\\') {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(format!(r"\\{slashes}")),
    }
}

/// Opens a link with the system. Never call this from tests.
pub fn open_link(t: &LinkTarget) {
    match t {
        LinkTarget::File(p) => crate::open_with::open_default(std::slice::from_ref(p)),
        LinkTarget::Web(url) => open_web(url),
    }
}

#[cfg(not(windows))]
fn open_web(url: &str) {
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let _ = std::process::Command::new(opener).arg(url).spawn();
}

#[cfg(windows)]
fn open_web(url: &str) {
    crate::open_with::shell_open(url.into());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn file_addresses_name_windows_paths() {
        assert_eq!(file_url_path("/C:/a b/c.png"), PathBuf::from(r"C:\a b\c.png"));
        assert_eq!(file_url_path("localhost/C:/a"), PathBuf::from(r"C:\a"));
        assert_eq!(file_url_path("host/share/a"), PathBuf::from(r"\\host\share\a"));
    }

    fn none(_: &str) -> bool {
        false
    }

    #[test]
    fn web_urls() {
        for ok in ["example.com", "https://www.example.co.uk/a?b=1", "http://8.8.8.8:8080/x", "ftp://files.example.org", "HTTP://Example.COM", "bücher.de", "a-b.example.com:80/"] {
            assert!(is_web_url(ok), "{ok}");
        }
        for bad in ["hello", "http://localhost", "192.168.1.1", "exa mple.com", "example.c", "", "-a.com", "a-.com", "10.0.0.1", "127.0.0.1", "169.254.1.1", "172.16.0.1", "172.31.9.9", "0.1.2.3", "224.1.1.1", "example.com:1", "example.com:123456", "example.com?x=1", "example.com/a b", "1.2.3.4.5", "999.1.1.1"] {
            assert!(!is_web_url(bad), "{bad}");
        }
        assert!(is_web_url("172.32.0.1"));
        assert!(is_web_url("223.1.1.1"));
    }

    #[test]
    fn accepted() {
        assert!(url_accepted("", none));
        assert!(url_accepted("file:///a/b", none));
        assert!(url_accepted("example.com", none));
        assert!(url_accepted("http://localhost", none), "has ://");
        assert!(url_accepted("slack://open", none));
        assert!(url_accepted("/some/path", |p| p == "/some/path"));
        assert!(!url_accepted("/some/path", none));
        assert!(!url_accepted("hello", none));
        assert!(!url_accepted("192.168.1.1", none));
    }

    #[test]
    fn decoding() {
        assert_eq!(percent_decode("/Users/a/b%20c.png"), "/Users/a/b c.png");
        assert_eq!(percent_decode("%E2%9C%93%20ok"), "\u{2713} ok");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz%4"), "%zz%4");
        assert_eq!(percent_decode("%C3%A9"), "é");
    }

    #[test]
    fn targets() {
        assert_eq!(link_target("file:///Users/a/b%20c.png", none), Some(LinkTarget::File("/Users/a/b c.png".into())));
        assert_eq!(link_target("example.com/x", none), Some(LinkTarget::Web("http://example.com/x".into())));
        assert_eq!(link_target("https://example.com", none), Some(LinkTarget::Web("https://example.com".into())));
        assert_eq!(link_target("/tmp/x", |p| p == "/tmp/x"), Some(LinkTarget::File("/tmp/x".into())));
        assert_eq!(link_target("http://localhost", none), Some(LinkTarget::Web("http://localhost".into())));
        assert_eq!(link_target("slack://open", none), Some(LinkTarget::Web("slack://open".into())));
        assert_eq!(link_target("hello", none), None);
    }
}
