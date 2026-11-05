//! Import Links: the rules for a pasted list of addresses and the fetching
//! that turns each one into an item. Pure rules (`parse`, `prefill`,
//! `title_of`, `shortcut`) are tested; `resolve` talks to the address the
//! user typed and to no other host.

use std::io::Read;
use std::path::Path;
use std::time::Duration;

/// How long one address may take to answer, as in Eagle.
pub const TIMEOUT: Duration = Duration::from_secs(10);
/// A page is read this far to find its title.
const PAGE_BYTES: u64 = 256 * 1024;
/// Images over this size are kept as bookmarks rather than downloaded.
const IMAGE_BYTES: u64 = 256 * 1024 * 1024;
/// The longest name a bookmark takes from a page title, in characters.
const NAME_CHARS: usize = 128;

/// The lines of `text` with surrounding space removed and blanks dropped.
fn lines(text: &str) -> Vec<&str> {
    text.lines().map(str::trim).filter(|l| !l.is_empty()).collect()
}

/// Whether a line is an address Import Links takes: `http://` or
/// `https://` (any case), a first character that is not space, `/`, `$`,
/// `.`, `?` or `#`, one more character, and no whitespace after.
pub fn is_link(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    let Some(rest) = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://")) else { return false };
    let mut chars = rest.chars();
    let (Some(first), Some(_)) = (chars.next(), chars.next()) else { return false };
    !line.chars().any(char::is_whitespace) && !"/$.?#".contains(first)
}

/// The addresses in `text`, one per line, repeats dropped; or, when any
/// line is not an address (or there is none), every such line.
pub fn parse(text: &str) -> Result<Vec<String>, Vec<String>> {
    let all = lines(text);
    let bad: Vec<String> = all.iter().filter(|l| !is_link(l)).map(|l| l.to_string()).collect();
    if all.is_empty() || !bad.is_empty() {
        return Err(bad);
    }
    let mut seen = std::collections::HashSet::new();
    Ok(all.into_iter().filter(|l| seen.insert(*l)).map(String::from).collect())
}

/// What the dialog starts with: the clipboard's valid lines when the
/// clipboard text begins with `http`, otherwise nothing.
pub fn prefill(clipboard: &str) -> String {
    if !clipboard.starts_with("http") {
        return String::new();
    }
    lines(clipboard).into_iter().filter(|l| is_link(l)).collect::<Vec<_>>().join("\n")
}

/// The text of a `.url` file for `address`.
pub fn shortcut(address: &str) -> String {
    format!("[InternetShortcut]\r\nURL={address}\r\n")
}

/// The page title in the start of an HTML document: the first `<title>`
/// element, with its text decoded and whitespace collapsed.
pub fn title_of(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let open = lower.find("<title")?;
    let start = open + lower[open..].find('>')? + 1;
    let end = start + lower[start..].find("</title")?;
    let text = decode_entities(html.get(start..end)?);
    let one = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!one.is_empty()).then_some(one)
}

fn decode_entities(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let decoded = rest.find(';').filter(|e| *e <= 10).and_then(|e| {
            let body = &rest[1..e];
            let c = match body {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                _ => body
                    .strip_prefix("#x")
                    .or_else(|| body.strip_prefix("#X"))
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .or_else(|| body.strip_prefix('#').and_then(|d| d.parse().ok()))
                    .and_then(char::from_u32),
            }?;
            Some((c, e + 1))
        });
        match decoded {
            Some((c, n)) => {
                out.push(c);
                rest = &rest[n..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The host of an address, for a name when the page has no title.
pub fn host_of(address: &str) -> String {
    let rest = address.split_once("://").map_or(address, |(_, r)| r);
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = rest[..end].rsplit('@').next().unwrap_or("");
    host.split(':').next().unwrap_or(host).to_string()
}

/// A name safe as a file name: no separators or control characters, at
/// most 128 characters; the host when nothing is left.
pub fn file_name(title: Option<&str>, address: &str) -> String {
    let clean = |s: &str| -> String {
        let s: String = s.chars().filter(|c| !c.is_control() && !"/\\:".contains(*c)).take(NAME_CHARS).collect();
        s.trim().trim_start_matches('.').trim().to_string()
    };
    let name = title.map(clean).filter(|n| !n.is_empty()).unwrap_or_else(|| clean(&host_of(address)));
    if name.is_empty() { "link".into() } else { falcon_library::disk_name::on_disk(&name).into_owned() }
}

/// The extension for an image content type (parameters ignored); `None`
/// for anything that is not an image falcon imports.
pub fn image_ext(content_type: &str) -> Option<&'static str> {
    let kind = content_type.split(';').next()?.trim().to_ascii_lowercase();
    Some(match kind.as_str() {
        "image/jpeg" | "image/jpg" | "image/pjpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "image/bmp" => "bmp",
        "image/avif" => "avif",
        "image/heic" => "heic",
        "image/heif" => "heif",
        "image/tiff" => "tiff",
        "image/x-icon" | "image/vnd.microsoft.icon" => "ico",
        _ => return None,
    })
}

/// A name for an image from the last part of its address's path.
fn image_stem(address: &str) -> String {
    let rest = address.split_once("://").map_or(address, |(_, r)| r);
    let path = rest.split(['?', '#']).next().unwrap_or("");
    let last = path.split_once('/').map_or("", |(_, p)| p).rsplit('/').next().unwrap_or("");
    let stem = last.rsplit_once('.').map_or(last, |(s, _)| s);
    file_name(Some(&crate::urls::percent_decode(stem)), address)
}

/// A file ready to import: the image an address pointed at, or a `.url`
/// bookmark for it.
#[derive(Debug)]
pub struct Resolved {
    pub address: String,
    pub path: std::path::PathBuf,
}

/// Serialises choosing a free name and writing the file, since several
/// addresses are fetched at once.
static NAMING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Writes `bytes` to a path in `dir` for `stem.ext` that did not exist.
fn write_free(dir: &Path, stem: &str, ext: &str, bytes: &[u8]) -> std::io::Result<std::path::PathBuf> {
    let _guard = NAMING.lock().unwrap_or_else(|p| p.into_inner());
    let path = crate::capture::free_path(dir, stem, ext);
    std::fs::write(&path, bytes)?;
    Ok(path)
}

fn agent() -> ureq::Agent {
    // No user agent, cookies or other identifying header is sent.
    ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(false)
        .user_agent(ureq::config::AutoHeaderValue::None)
        .accept(ureq::config::AutoHeaderValue::None)
        .build()
        .into()
}

/// The image format `head` starts with, as the extension to save it
/// under, judged from magic bytes alone. `None` for anything else,
/// including HTML. SVG counts (Eagle makes thumbnails for it) but only
/// when the text opens, after an optional byte order mark, XML
/// declaration, comments and `<!DOCTYPE svg`, with an `<svg` element.
pub fn sniff(head: &[u8]) -> Option<&'static str> {
    let at = |i: usize, s: &[u8]| head.get(i..i + s.len()) == Some(s);
    if at(0, b"\x89PNG\r\n\x1a\n") {
        return Some("png");
    }
    if at(0, b"\xff\xd8\xff") {
        return Some("jpg");
    }
    if at(0, b"GIF87a") || at(0, b"GIF89a") {
        return Some("gif");
    }
    if at(0, b"RIFF") && at(8, b"WEBP") {
        return Some("webp");
    }
    if at(0, b"II*\0") || at(0, b"MM\0*") {
        return Some("tiff");
    }
    // "BM" alone is too weak: the DIB header size that follows must be one of the known ones.
    if at(0, b"BM") && head.get(14..18).is_some_and(|b| matches!(u32::from_le_bytes([b[0], b[1], b[2], b[3]]), 12 | 40 | 52 | 56 | 64 | 108 | 124)) {
        return Some("bmp");
    }
    if at(0, &[0, 0, 1, 0]) && head.get(4..6).is_some_and(|c| u16::from_le_bytes([c[0], c[1]]) > 0) && head.len() >= 22 {
        return Some("ico");
    }
    if at(4, b"ftyp") {
        let brand = head.get(8..12)?;
        return match brand {
            b"avif" | b"avis" => Some("avif"),
            b"heic" | b"heix" | b"heim" | b"heis" | b"hevc" | b"hevx" | b"hevm" | b"hevs" => Some("heic"),
            b"mif1" | b"msf1" => Some("heif"),
            _ => None,
        };
    }
    sniff_svg(head).then_some("svg")
}

/// Whether `head` is the start of an SVG document (see `sniff`).
fn sniff_svg(head: &[u8]) -> bool {
    let mut rest = head.strip_prefix(b"\xef\xbb\xbf").unwrap_or(head);
    loop {
        rest = rest.trim_ascii_start();
        let skip = |rest: &[u8], close: &[u8]| rest.windows(close.len()).position(|w| w == close).map(|i| i + close.len());
        let n = if rest.starts_with(b"<?") {
            skip(rest, b"?>")
        } else if rest.starts_with(b"<!--") {
            skip(rest, b"-->")
        } else if rest.len() >= 13 && rest[..13].eq_ignore_ascii_case(b"<!doctype svg") {
            skip(rest, b">")
        } else {
            break;
        };
        match n {
            Some(n) => rest = &rest[n..],
            None => return false,
        }
    }
    rest.starts_with(b"<svg") && rest.get(4).is_some_and(|c| c.is_ascii_whitespace() || matches!(c, b'>' | b'/'))
}

/// Fetches `address` (once, with a 10 s limit) and writes the file to
/// import into `dir`: the image itself when the server says it is one or
/// the bytes are one, otherwise a `.url` bookmark named after the page
/// title (or the host when the page is unreachable or has none), as Eagle
/// falls back to a bookmark whatever goes wrong. A missing, wrong or
/// generic content type (including `text/html`) is not believed over
/// the bytes: the start of the body is checked with `sniff` and the file
/// gets the extension of what it is. Only the address given is contacted.
pub fn resolve(address: &str, dir: &Path) -> std::io::Result<Resolved> {
    let mut title = None;
    if let Ok(mut resp) = agent().get(address).call() {
        let ctype = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
        let typed = image_ext(&ctype);
        let mut reader = resp.body_mut().as_reader();
        // The head decides what the body is; a read error leaves what arrived.
        let mut bytes = Vec::new();
        let head_ok = reader.by_ref().take(PAGE_BYTES).read_to_end(&mut bytes).is_ok();
        let ext = sniff(&bytes).or(typed);
        if let Some(ext) = ext {
            if head_ok && reader.take(IMAGE_BYTES + 1 - bytes.len() as u64).read_to_end(&mut bytes).is_ok() && !bytes.is_empty() && bytes.len() as u64 <= IMAGE_BYTES {
                let path = write_free(dir, &image_stem(address), ext, &bytes)?;
                return Ok(Resolved { address: address.into(), path });
            }
        } else if typed.is_none() && (ctype.is_empty() || ctype.to_ascii_lowercase().contains("html")) {
            title = title_of(&String::from_utf8_lossy(&bytes));
        }
    }
    let path = write_free(dir, &file_name(title.as_deref(), address), "url", shortcut(address).as_bytes())?;
    Ok(Resolved { address: address.into(), path })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_need_a_scheme_and_a_host() {
        for ok in ["http://a.b", "https://example.com/x?y=1#z", "HTTPS://Example.com", "http://localhost:8080"] {
            assert!(is_link(ok), "{ok}");
        }
        for bad in ["", "example.com", "ftp://a.b", "https://", "http://x", "https:///a", "http://.a", "https://a b", "http://?q", "javascript:alert(1)", "https://#a"] {
            assert!(!is_link(bad), "{bad}");
        }
    }

    #[test]
    fn parse_keeps_lines_in_order_and_drops_repeats() {
        let text = "  https://a.example/1 \n\n https://b.example\r\nhttps://a.example/1\n";
        assert_eq!(parse(text), Ok(vec!["https://a.example/1".to_string(), "https://b.example".to_string()]));
    }

    #[test]
    fn parse_names_every_bad_line() {
        assert_eq!(parse("https://ok.example\nnope\nhttp://\n"), Err(vec!["nope".to_string(), "http://".to_string()]));
        assert_eq!(parse(" \n\t\n"), Err(Vec::new()), "nothing to import");
        assert_eq!(parse(""), Err(Vec::new()));
    }

    #[test]
    fn the_clipboard_fills_the_box_only_with_links() {
        assert_eq!(prefill("https://a.example\n\nnot a link\nhttp://b.example "), "https://a.example\nhttp://b.example");
        assert_eq!(prefill("see https://a.example"), "");
        assert_eq!(prefill(" https://a.example"), "", "Eagle tests the raw text");
        assert_eq!(prefill("httpx nothing"), "");
    }

    #[test]
    fn shortcut_text_reads_back_as_the_address() {
        let text = shortcut("https://a.example/p?q=1");
        assert_eq!(falcon_library::weblink::parse("url", text.as_bytes()).as_deref(), Some("https://a.example/p?q=1"));
    }

    #[test]
    fn titles_are_found_and_decoded() {
        assert_eq!(title_of("<html><head><TITLE lang=x>\n  Tom &amp; Jerry\n &#169; &#x41;</title>").as_deref(), Some("Tom & Jerry © A"));
        assert_eq!(title_of("<title></title>"), None);
        assert_eq!(title_of("<p>none</p>"), None);
        assert_eq!(title_of("<title>cut off"), None);
        assert_eq!(title_of("<title>a &bogus; b</title>").as_deref(), Some("a &bogus; b"));
    }

    #[test]
    fn names_are_file_safe() {
        assert_eq!(file_name(Some("a/b\\c: d"), "https://h.example"), "abc d");
        assert_eq!(file_name(Some("  "), "https://user@h.example:81/x"), "h.example");
        assert_eq!(file_name(None, "https://h.example/x"), "h.example");
        assert_eq!(file_name(Some(&"x".repeat(300)), "https://h.example").chars().count(), 128);
        assert_eq!(file_name(Some("..hidden"), "https://h.example"), "hidden");
    }

    #[test]
    fn image_names_come_from_the_path() {
        assert_eq!(image_stem("https://h.example/a/b%20c.jpg?x=1"), "b c");
        assert_eq!(image_stem("https://h.example/"), "h.example");
        assert_eq!(image_ext("Image/JPEG; charset=x"), Some("jpg"));
        assert_eq!(image_ext("text/html"), None);
    }

    /// A one-shot local server answering with `head` and `body`.
    fn serve(head: &'static str, body: &'static [u8]) -> String {
        serve_declaring(head, body, body.len())
    }

    /// Like `serve`, but announces `declared` bytes, so a shorter body is cut off.
    fn serve_declaring(head: &'static str, body: &'static [u8], declared: usize) -> String {
        use std::io::Write;
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut seen = [0u8; 2048];
                let n = s.read(&mut seen).unwrap_or(0);
                let req = String::from_utf8_lossy(&seen[..n]).to_ascii_lowercase();
                // Nothing that names the user or the program is sent.
                assert!(!req.contains("user-agent") && !req.contains("cookie") && !req.contains("referer"), "{req}");
                let _ = write!(s, "HTTP/1.1 200 OK\r\n{head}\r\nContent-Length: {declared}\r\nConnection: close\r\n\r\n");
                let _ = s.write_all(body);
            }
        });
        format!("http://{addr}/pic.png")
    }

    #[test]
    fn a_page_becomes_a_bookmark_named_after_its_title() {
        let dir = tempfile::tempdir().unwrap();
        let url = serve("Content-Type: text/html; charset=utf-8", b"<html><title>Hello page</title>");
        let r = resolve(&url, dir.path()).unwrap();
        assert_eq!(r.path.file_name().unwrap(), "Hello page.url");
        assert_eq!(falcon_library::weblink::read(&r.path).as_deref(), Some(url.as_str()));
    }

    #[test]
    fn an_image_is_downloaded() {
        let dir = tempfile::tempdir().unwrap();
        let url = serve("Content-Type: image/png", b"\x89PNG-bytes");
        let r = resolve(&url, dir.path()).unwrap();
        assert_eq!(r.path.file_name().unwrap(), "pic.png");
        assert_eq!(std::fs::read(&r.path).unwrap(), b"\x89PNG-bytes");
    }

    #[test]
    fn a_bookmark_imports_as_a_url_item_in_the_folder() {
        use falcon_library::import::ImportOptions;
        let dir = tempfile::tempdir().unwrap();
        let url = serve("Content-Type: text/html", b"<title>Saved / page</title>");
        let r = resolve(&url, dir.path()).unwrap();
        let mut lib = falcon_library::Library::create(dir.path().join("t.library")).unwrap();
        let opts = ImportOptions { folders: Vec::new(), url: r.address.clone(), ..Default::default() };
        let id = lib.import_file(&r.path, &opts).unwrap();
        let item = lib.item(&id).unwrap();
        assert_eq!((item.ext.as_str(), item.url.as_str(), item.name.as_str()), ("url", url.as_str(), "Saved  page"));
        assert!(item.tags.is_empty());
    }

    #[test]
    fn an_unreachable_address_is_still_a_bookmark() {
        let dir = tempfile::tempdir().unwrap();
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", l.local_addr().unwrap());
        drop(l);
        let r = resolve(&url, dir.path()).unwrap();
        assert_eq!(r.path.extension().unwrap(), "url");
    }

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nrest-of-image";

    #[test]
    fn sniff_knows_the_image_formats() {
        let cases: [(&[u8], &str); 14] = [
            (PNG, "png"),
            (b"\xff\xd8\xff\xdb\0\x43", "jpg"),
            (b"GIF89a\x01\0", "gif"),
            (b"GIF87a\x01\0", "gif"),
            (b"RIFF\x10\0\0\0WEBPVP8 ", "webp"),
            (b"II*\0\x08\0\0\0", "tiff"),
            (b"MM\0*\0\0\0\x08", "tiff"),
            (b"BM\x46\0\0\0\0\0\0\0\x36\0\0\0\x28\0\0\0", "bmp"),
            (b"\0\0\x01\0\x01\0\x10\x10\0\0\x01\0\x20\0\x68\x04\0\0\x16\0\0\0", "ico"),
            (b"\0\0\0\x1cftypavif\0\0\0\0", "avif"),
            (b"\0\0\0\x18ftypheic\0\0\0\0", "heic"),
            (b"\0\0\0\x18ftypmif1\0\0\0\0", "heif"),
            (b"<svg xmlns='http://www.w3.org/2000/svg'/>", "svg"),
            (b"\xef\xbb\xbf <?xml version=\"1.0\"?>\n<!-- c -->\n<!DOCTYPE svg PUBLIC \"x\" \"y\">\n<svg\nwidth='1'>", "svg"),
        ];
        for (bytes, ext) in cases {
            assert_eq!(sniff(bytes), Some(ext), "{}", String::from_utf8_lossy(bytes));
        }
    }

    #[test]
    fn sniff_refuses_everything_else() {
        let none: [&[u8]; 12] = [
            b"",
            b"<!doctype html><html><body><svg></svg>",
            b"<html><svg>",
            b"<svgfoo>",
            b"<?xml version='1.0'?><feed/>",
            b"<?xml unterminated <svg>",
            b"BM but not a bitmap at all",
            b"\0\0\x01\0\0\0",
            b"\0\0\0\x18ftypmp42\0\0\0\0",
            b"RIFF\x10\0\0\0WAVEfmt ",
            b"%PDF-1.7",
            b"\x89PN",
        ];
        for bytes in none {
            assert_eq!(sniff(bytes), None, "{}", String::from_utf8_lossy(bytes));
        }
    }

    fn resolved(url: &str) -> (tempfile::TempDir, Resolved) {
        let dir = tempfile::tempdir().unwrap();
        let r = resolve(url, dir.path()).unwrap();
        (dir, r)
    }

    #[test]
    fn an_image_with_no_content_type_is_downloaded() {
        let (_d, r) = resolved(&serve("X-Other: 1", PNG));
        assert_eq!(r.path.file_name().unwrap(), "pic.png");
        assert_eq!(std::fs::read(&r.path).unwrap(), PNG);
    }

    #[test]
    fn an_octet_stream_image_gets_its_extension_from_the_bytes() {
        let url = serve("Content-Type: application/octet-stream", b"GIF89a\x01\0\x01\0rest").replace("pic.png", "download.bin");
        let (_d, r) = resolved(&url);
        assert_eq!(r.path.file_name().unwrap(), "download.gif");
    }

    #[test]
    fn an_image_labelled_html_is_still_an_image() {
        let (_d, r) = resolved(&serve("Content-Type: text/html", b"\xff\xd8\xff\xe0JFIFdata"));
        assert_eq!(r.path.file_name().unwrap(), "pic.jpg");
    }

    #[test]
    fn a_wrong_image_type_does_not_set_the_extension() {
        let (_d, r) = resolved(&serve("Content-Type: image/jpeg", PNG));
        assert_eq!(r.path.file_name().unwrap(), "pic.png");
    }

    #[test]
    fn real_html_without_a_type_is_a_bookmark_with_its_title() {
        let (_d, r) = resolved(&serve("X-Other: 1", b"<!doctype html><title>Plain page</title><svg></svg>"));
        assert_eq!(r.path.file_name().unwrap(), "Plain page.url");
    }

    #[test]
    fn an_empty_body_is_a_bookmark_named_after_the_host() {
        for head in ["Content-Type: image/png", "Content-Type: text/html", "X-Other: 1"] {
            let (_d, r) = resolved(&serve(head, b""));
            assert_eq!(r.path.file_name().unwrap(), "127.0.0.1.url", "{head}");
        }
    }

    #[test]
    fn a_truncated_image_is_a_bookmark_and_a_truncated_page_keeps_its_title() {
        let (_d, r) = resolved(&serve_declaring("X-Other: 1", PNG, PNG.len() + 100));
        assert_eq!(r.path.extension().unwrap(), "url");
        let (_d, r) = resolved(&serve_declaring("Content-Type: text/html", b"<title>Cut</title><p>", 5000));
        assert_eq!(r.path.file_name().unwrap(), "Cut.url");
    }
}
