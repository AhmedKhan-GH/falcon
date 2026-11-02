//! MHTML web archives (`.mhtml`): reading the MIME parts (RFC 2045, 2046,
//! 2047 and 2557) and unpacking them into a folder a browser engine can
//! load from disk.
//!
//! WebKit on macOS doesn't open MHTML, so the web viewer and the import
//! thumbnail use [`unpack`]: every part becomes a file in a fresh folder
//! (the root page as `index.html`), and the references in the HTML and CSS
//! parts that point at other parts, by `Content-Location` or `cid:`, are
//! rewritten to the files' relative names. Whatever isn't in the archive is
//! left as written, and a Content Security Policy put at the top of every
//! page lets it load only `file:`, `data:` and `blob:` resources, so
//! nothing is fetched from the network.
//!
//! The reader tolerates CRLF or LF line ends, a missing closing boundary,
//! folded headers, nested multiparts, quoted-printable with `=0D` line
//! ends, encoded words in headers, and the shapes Chrome, Edge and Word
//! write ("Save as MHTML": full URLs as `Content-Location`, `cid:`
//! references, `Snapshot-Content-Location`). Text parts are decoded from
//! their charset with `encoding_rs` and written as UTF-8 with a byte order
//! mark, which takes precedence over any charset the page declares.
//!
//! QuickLook's HTML renderer loads a page's files through a scheme of its
//! own, which no policy source matches, so the import thumbnail unpacks
//! without the policy ([`Policy::None`]); QuickLook fetches nothing from
//! the network itself.
//!
//! Unpacking is bounded by [`Limits`] (total decoded size and part count),
//! and file names are made by falcon, never taken as paths from the
//! archive, so nothing is written outside the target folder.

use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use base64::Engine as _;

/// How much an archive may unpack to, by default: 256 MB of decoded parts.
pub const MAX_TOTAL: u64 = 256 * 1024 * 1024;
/// How many parts an archive may have, by default.
pub const MAX_PARTS: usize = 20_000;
/// How deeply multiparts may nest.
const MAX_DEPTH: usize = 8;
/// The root page's file name in the unpacked folder.
pub const ROOT_NAME: &str = "index.html";

/// The policy put at the top of every unpacked page: the archive's own
/// files and inline data load, the network doesn't.
pub const CSP: &str = "default-src 'none'; img-src file: data: blob:; media-src file: data: blob:; font-src file: data:; \
style-src file: data: 'unsafe-inline'; script-src file: 'unsafe-inline' 'unsafe-eval'; frame-src file: data:; \
child-src file: data:; object-src 'none'; connect-src 'none'; form-action 'none'";

/// Whether unpacked pages carry [`CSP`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    /// The policy goes at the top of every page (the live viewer).
    Offline,
    /// No policy (QuickLook, which doesn't fetch on its own).
    None,
}

/// Bounds on what an archive may unpack to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The decoded size of all parts together, in bytes.
    pub max_total: u64,
    /// The number of parts, nested ones included.
    pub max_parts: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_total: MAX_TOTAL, max_parts: MAX_PARTS }
    }
}

/// Why an archive couldn't be read or unpacked.
#[derive(Debug, thiserror::Error)]
pub enum MhtmlError {
    #[error("not a MIME web archive")]
    NotArchive,
    #[error("the archive has no parts")]
    Empty,
    #[error("the archive unpacks to more than {0} bytes")]
    TooLarge(u64),
    #[error("the archive has more than {0} parts")]
    TooManyParts(usize),
    #[error("the archive's parts are nested too deeply")]
    TooDeep,
    #[error("part {0} can't be decoded: {1}")]
    BadPart(usize, String),
    #[error("refused to write {0:?} outside the folder")]
    UnsafeName(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// One part of an archive, decoded from its transfer encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    /// The MIME type, lower-cased (`text/html`); `text/plain` when the part
    /// gives none, as RFC 2045 says.
    pub content_type: String,
    /// The `charset` parameter, as written.
    pub charset: Option<String>,
    /// `Content-Location`, with encoded words decoded.
    pub location: Option<String>,
    /// `Content-ID`, without its angle brackets.
    pub content_id: Option<String>,
    /// The bytes, transfer-decoded (still in `charset` for text).
    pub body: Vec<u8>,
}

impl Part {
    fn is_html(&self) -> bool {
        matches!(self.content_type.as_str(), "text/html" | "application/xhtml+xml")
    }

    fn is_css(&self) -> bool {
        self.content_type == "text/css"
    }
}

/// A parsed archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Archive {
    /// Every leaf part in order, nested multiparts flattened.
    pub parts: Vec<Part>,
    /// The index of the root page: the part `start=` names, else the first
    /// HTML part, else the first part.
    pub root: usize,
    /// The `Subject` header (the page title Chrome saves), decoded.
    pub subject: Option<String>,
    /// The page's address: `Snapshot-Content-Location`, else the root
    /// part's `Content-Location` when it is a web address.
    pub address: Option<String>,
}

/// Header names lower-cased with their unfolded values, in order.
#[derive(Debug, Default)]
struct Headers(Vec<(String, String)>);

impl Headers {
    fn get(&self, name: &str) -> Option<&str> {
        self.0.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
}

/// Splits `data` into its header block and body at the first empty line.
/// Leading empty lines are skipped. Without an empty line it is all head.
fn split_head(data: &[u8]) -> (&[u8], &[u8]) {
    let mut start = 0;
    while start < data.len() {
        let rest = &data[start..];
        if rest.starts_with(b"\r\n") {
            start += 2;
        } else if rest.starts_with(b"\n") {
            start += 1;
        } else {
            break;
        }
    }
    let data = &data[start..];
    let mut i = 0;
    while let Some(nl) = data[i..].iter().position(|&b| b == b'\n') {
        let at = i + nl + 1;
        let rest = &data[at..];
        if rest.starts_with(b"\r\n") {
            return (&data[..at], &data[at + 2..]);
        }
        if rest.starts_with(b"\n") {
            return (&data[..at], &data[at + 1..]);
        }
        i = at;
    }
    (data, &[])
}

/// Reads a header block: one header per line, continuation lines (starting
/// with a space or tab) appended to the one before.
fn parse_headers(head: &[u8]) -> Headers {
    let text = String::from_utf8_lossy(head);
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.starts_with([' ', '\t']) {
            if let Some((_, v)) = out.last_mut() {
                v.push(' ');
                v.push_str(line.trim());
            }
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim();
            if !name.is_empty() && !name.contains(' ') {
                out.push((name.to_ascii_lowercase(), value.trim().to_string()));
            }
        }
    }
    Headers(out)
}

/// Splits a structured header value (`type/sub; a=b; c="d"`) into its
/// first item, lower-cased, and its parameters (names lower-cased,
/// values unquoted).
fn parse_params(value: &str) -> (String, Vec<(String, String)>) {
    let mut items = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                cur.push(c);
            }
            '\\' if quoted => {
                cur.push(c);
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            ';' if !quoted => items.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    items.push(cur);
    let mut it = items.into_iter();
    let first = it.next().unwrap_or_default().trim().to_ascii_lowercase();
    let params = it
        .filter_map(|p| {
            let (k, v) = p.split_once('=')?;
            Some((k.trim().to_ascii_lowercase(), unquote(v.trim())))
        })
        .collect();
    (first, params)
}

fn unquote(v: &str) -> String {
    let Some(inner) = v.strip_prefix('"') else { return v.to_string() };
    let inner = inner.strip_suffix('"').unwrap_or(inner);
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(n) = chars.next() {
                out.push(n);
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn param<'a>(params: &'a [(String, String)], name: &str) -> Option<&'a str> {
    params.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

/// Splits a multipart body into its parts at `--boundary` lines. A part
/// runs to the line break before the next delimiter; without a closing
/// delimiter the last part runs to the end. `None` when no delimiter is
/// found at all.
fn split_multipart<'a>(body: &'a [u8], boundary: &str) -> Option<Vec<&'a [u8]>> {
    let needle = format!("--{boundary}");
    let needle = needle.as_bytes();
    let mut parts = Vec::new();
    // Where the current part's content starts, once the first delimiter
    // has been seen.
    let mut open: Option<usize> = None;
    let mut line = 0;
    while line <= body.len() {
        let end = body[line..].iter().position(|&b| b == b'\n').map_or(body.len(), |p| line + p);
        let text = &body[line..end];
        if let Some(rest) = text.strip_prefix(needle) {
            let rest = rest.strip_suffix(b"\r").unwrap_or(rest);
            let close = rest.starts_with(b"--");
            if close || rest.iter().all(|b| b.is_ascii_whitespace()) {
                if let Some(start) = open {
                    // The line break before the delimiter belongs to it.
                    let mut stop = line.max(start);
                    if stop > start && body[stop - 1] == b'\n' {
                        stop -= 1;
                        if stop > start && body[stop - 1] == b'\r' {
                            stop -= 1;
                        }
                    }
                    parts.push(&body[start..stop]);
                }
                if close {
                    return Some(parts);
                }
                open = Some((end + 1).min(body.len()));
            }
        }
        line = end + 1;
    }
    let start = open?;
    if start < body.len() {
        parts.push(&body[start..]);
    }
    Some(parts)
}

/// Decodes base64 leniently: whitespace and stray characters are skipped,
/// padding is optional and a dangling last character is dropped.
fn decode_base64(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut clean: Vec<u8> = data
        .iter()
        .filter_map(|&c| match c {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/' => Some(c),
            b'-' => Some(b'+'),
            b'_' => Some(b'/'),
            _ => None,
        })
        .collect();
    if clean.len() % 4 == 1 {
        clean.pop();
    }
    let engine = base64::engine::GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        base64::engine::GeneralPurposeConfig::new()
            .with_decode_padding_mode(base64::engine::DecodePaddingMode::RequireNone)
            .with_decode_allow_trailing_bits(true),
    );
    engine.decode(&clean).map_err(|e| e.to_string())
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Decodes quoted-printable: `=XX` escapes (either case), soft line breaks
/// (`=` at a line end, also with trailing blanks), and an `=` that starts
/// neither kept as it is.
fn decode_qp(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        let c = data[i];
        if c != b'=' {
            out.push(c);
            i += 1;
            continue;
        }
        if let (Some(&a), Some(&b)) = (data.get(i + 1), data.get(i + 2))
            && let (Some(h), Some(l)) = (hex(a), hex(b))
        {
            out.push(h << 4 | l);
            i += 3;
            continue;
        }
        // A soft line break, possibly with blanks before it.
        let mut j = i + 1;
        while matches!(data.get(j), Some(b' ' | b'\t')) {
            j += 1;
        }
        match data.get(j) {
            Some(b'\r') if data.get(j + 1) == Some(&b'\n') => i = j + 2,
            Some(b'\n') => i = j + 1,
            None => i = j,
            _ => {
                out.push(b'=');
                i += 1;
            }
        }
    }
    out
}

/// Turns the `\r\r\n` that some writers leave (a quoted-printable `=0D`
/// before a real line break) into a single `\r\n`.
fn fix_cr(data: Vec<u8>) -> Vec<u8> {
    if !data.windows(3).any(|w| w == b"\r\r\n") {
        return data;
    }
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if data[i..].starts_with(b"\r\r\n") {
            i += 1;
            continue;
        }
        out.push(data[i]);
        i += 1;
    }
    out
}

/// Decodes RFC 2047 encoded words (`=?utf-8?Q?Caf=C3=A9?=`,
/// `=?iso-8859-1?B?...?=`) in a header value; blanks between two encoded
/// words are dropped, unknown charsets and malformed words are kept as
/// written.
pub fn decode_words(value: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    let mut after_word = false;
    while let Some(at) = rest.find("=?") {
        let (before, from) = rest.split_at(at);
        let decoded = encoded_word(from);
        match decoded {
            Some((text, used)) => {
                if !(after_word && before.chars().all(char::is_whitespace)) {
                    out.push_str(before);
                }
                out.push_str(&text);
                rest = &from[used..];
                after_word = true;
            }
            None => {
                out.push_str(before);
                out.push_str("=?");
                rest = &from[2..];
                after_word = false;
            }
        }
    }
    out.push_str(rest);
    out
}

/// One encoded word at the start of `s`: its text and its length.
fn encoded_word(s: &str) -> Option<(String, usize)> {
    let inner = s.strip_prefix("=?")?;
    let q1 = inner.find('?')?;
    let charset = &inner[..q1];
    let rest = &inner[q1 + 1..];
    let q2 = rest.find('?')?;
    let enc = &rest[..q2];
    let rest = &rest[q2 + 1..];
    let end = rest.find("?=")?;
    let text = &rest[..end];
    if text.contains(char::is_whitespace) {
        return None;
    }
    let bytes = match enc {
        "Q" | "q" => decode_qp(text.replace('_', " ").as_bytes()),
        "B" | "b" => decode_base64(text.as_bytes()).ok()?,
        _ => return None,
    };
    // RFC 2231 allows `charset*language`.
    let charset = charset.split('*').next().unwrap_or(charset);
    let encoding = encoding_rs::Encoding::for_label(charset.as_bytes())?;
    let (decoded, _, _) = encoding.decode(&bytes);
    Some((decoded.into_owned(), 2 + q1 + 1 + q2 + 1 + end + 2))
}

/// The charset a page names in its first kilobyte (`<meta charset=...>` or
/// `content="...; charset=..."`), or a style sheet in its `@charset`.
fn sniff_charset(data: &[u8], html: bool) -> Option<String> {
    let head = &data[..data.len().min(1024)];
    let text = String::from_utf8_lossy(head);
    if !html {
        let rest = text.strip_prefix("@charset \"")?;
        return Some(rest[..rest.find('"')?].to_string());
    }
    let lower = text.to_ascii_lowercase();
    let at = lower.find("charset=")? + "charset=".len();
    let value = lower[at..].trim_start_matches(['"', '\'', ' ']);
    let end = value.find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))).unwrap_or(value.len());
    (end > 0).then(|| value[..end].to_string())
}

/// A text part's characters: by its byte order mark, else its `charset`,
/// else the charset it declares itself, else UTF-8 when it is valid UTF-8,
/// else windows-1252 (as browsers read undeclared Latin text). Labels are
/// WHATWG's, so `iso-8859-1` reads as windows-1252, as in browsers.
pub fn decode_text(data: &[u8], charset: Option<&str>, html: bool) -> String {
    if let Some((encoding, _)) = encoding_rs::Encoding::for_bom(data) {
        return encoding.decode(data).0.into_owned();
    }
    let named = charset.map(str::to_string).or_else(|| sniff_charset(data, html));
    let encoding = named.and_then(|l| encoding_rs::Encoding::for_label(l.trim().as_bytes())).unwrap_or_else(|| {
        if std::str::from_utf8(data).is_ok() { encoding_rs::UTF_8 } else { encoding_rs::WINDOWS_1252 }
    });
    encoding.decode_without_bom_handling(data).0.into_owned()
}

struct Parser {
    limits: Limits,
    total: u64,
    parts: Vec<Part>,
}

impl Parser {
    /// Reads one entity (`data` with its own headers) into `self.parts`,
    /// recursing into multiparts.
    fn entity(&mut self, data: &[u8], depth: usize) -> Result<(), MhtmlError> {
        let (head, body) = split_head(data);
        let headers = parse_headers(head);
        self.entity_with(&headers, body, depth)
    }

    fn entity_with(&mut self, headers: &Headers, body: &[u8], depth: usize) -> Result<(), MhtmlError> {
        let (ctype, params) = parse_params(headers.get("content-type").unwrap_or("text/plain"));
        if ctype.starts_with("multipart/") {
            if depth >= MAX_DEPTH {
                return Err(MhtmlError::TooDeep);
            }
            let boundary = param(&params, "boundary").filter(|b| !b.is_empty()).ok_or(MhtmlError::NotArchive)?;
            let pieces = split_multipart(body, boundary).ok_or(MhtmlError::NotArchive)?;
            for piece in pieces {
                self.entity(piece, depth + 1)?;
            }
            return Ok(());
        }
        if self.parts.len() >= self.limits.max_parts {
            return Err(MhtmlError::TooManyParts(self.limits.max_parts));
        }
        let index = self.parts.len();
        let cte = headers.get("content-transfer-encoding").unwrap_or("").trim().to_ascii_lowercase();
        let ctype = if ctype.contains('/') { ctype } else { "text/plain".to_string() };
        let decoded = match cte.as_str() {
            "base64" => decode_base64(body).map_err(|e| MhtmlError::BadPart(index, e))?,
            "quoted-printable" => decode_qp(body),
            _ => body.to_vec(),
        };
        let decoded = if ctype.starts_with("text/") { fix_cr(decoded) } else { decoded };
        self.total += decoded.len() as u64;
        if self.total > self.limits.max_total {
            return Err(MhtmlError::TooLarge(self.limits.max_total));
        }
        let location = headers.get("content-location").map(|v| decode_words(v).trim().trim_matches(['<', '>']).trim().to_string()).filter(|v| !v.is_empty());
        let content_id = headers.get("content-id").map(|v| v.trim().trim_matches(['<', '>']).trim().to_string()).filter(|v| !v.is_empty());
        self.parts.push(Part { content_type: ctype, charset: param(&params, "charset").map(str::to_string), location, content_id, body: decoded });
        Ok(())
    }
}

/// Reads an archive. A file whose top header isn't a multipart but is a
/// single HTML entity (some writers do this) is an archive of one page.
pub fn parse(data: &[u8], limits: Limits) -> Result<Archive, MhtmlError> {
    let (head, body) = split_head(data);
    let headers = parse_headers(head);
    let Some(ct) = headers.get("content-type") else { return Err(MhtmlError::NotArchive) };
    let (ctype, params) = parse_params(ct);
    if !ctype.starts_with("multipart/") && !matches!(ctype.as_str(), "text/html" | "application/xhtml+xml") {
        return Err(MhtmlError::NotArchive);
    }
    let mut parser = Parser { limits, total: 0, parts: Vec::new() };
    parser.entity_with(&headers, body, 0)?;
    let parts = parser.parts;
    if parts.is_empty() {
        return Err(MhtmlError::Empty);
    }
    let start = param(&params, "start").map(|s| s.trim().trim_matches(['<', '>']).to_string());
    let root = start
        .and_then(|s| parts.iter().position(|p| p.content_id.as_deref() == Some(s.as_str()) || p.location.as_deref() == Some(s.as_str())))
        .or_else(|| parts.iter().position(Part::is_html))
        .unwrap_or(0);
    let subject = headers.get("subject").map(|s| decode_words(s).trim().to_string()).filter(|s| !s.is_empty());
    let address = headers
        .get("snapshot-content-location")
        .map(|s| decode_words(s).trim().to_string())
        .or_else(|| parts[root].location.clone())
        .filter(|a| {
            let l = a.to_ascii_lowercase();
            l.starts_with("http://") || l.starts_with("https://")
        });
    Ok(Archive { parts, root, subject, address })
}

/// What [`unpack`] made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unpacked {
    /// The root page, `index.html` in the folder.
    pub root: PathBuf,
    /// Every file written, the root first.
    pub files: Vec<PathBuf>,
    /// The page title (`Subject`) and address, from the archive.
    pub subject: Option<String>,
    pub address: Option<String>,
}

/// The file extension for a MIME type, when it is one browsers decide by
/// extension when loading from disk.
fn ext_for(ctype: &str) -> Option<&'static str> {
    Some(match ctype {
        "text/html" | "application/xhtml+xml" => "html",
        "text/css" => "css",
        "text/javascript" | "application/javascript" | "application/x-javascript" => "js",
        "image/png" => "png",
        "image/jpeg" | "image/jpg" | "image/pjpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/avif" => "avif",
        "image/bmp" => "bmp",
        "image/svg+xml" => "svg",
        "image/x-icon" | "image/vnd.microsoft.icon" => "ico",
        "image/tiff" => "tif",
        "font/woff" | "application/font-woff" => "woff",
        "font/woff2" => "woff2",
        "font/ttf" | "application/x-font-ttf" => "ttf",
        "font/otf" => "otf",
        "text/plain" => "txt",
        "application/json" => "json",
        "audio/mpeg" => "mp3",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        _ => return None,
    })
}

/// Percent-decodes `s` (invalid escapes kept), reading the bytes as UTF-8.
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let (Some(h), Some(l)) = (b.get(i + 1).copied().and_then(hex), b.get(i + 2).copied().and_then(hex))
        {
            out.push(h << 4 | l);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Percent-encodes a file name for use in a relative URL: everything but
/// ASCII letters, digits and `-._~`.
pub fn percent_encode(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A safe file name from what an archive suggests: letters, digits, `-`,
/// `_` and `.` kept, everything else `_`, no leading dot, at most 60
/// characters.
fn sanitize(name: &str) -> String {
    let mut s: String = name.chars().map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' }).collect();
    while s.starts_with('.') {
        s.remove(0);
    }
    s.chars().take(60).collect()
}

/// The file name of part `index`: a number to keep names unique, then the
/// last path segment of its location (or its content id), with the
/// extension its type needs.
fn file_name(index: usize, part: &Part) -> String {
    let from_location = part.location.as_deref().and_then(|l| {
        let l = l.split(['?', '#']).next().unwrap_or(l);
        let l = l.strip_prefix("cid:").map_or(l, |c| c.split('@').next().unwrap_or(c));
        let last = l.rsplit(['/', '\\']).find(|s| !s.is_empty())?;
        let s = sanitize(&percent_decode(last));
        (!s.is_empty()).then_some(s)
    });
    let base = from_location
        .or_else(|| part.content_id.as_deref().map(|c| sanitize(c.split('@').next().unwrap_or(c))).filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "part".to_string());
    let (stem, ext) = match base.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() && !e.is_empty() && e.len() <= 5 => (s.to_string(), Some(e.to_ascii_lowercase())),
        _ => (base.clone(), None),
    };
    let ext = match (ext_for(&part.content_type), ext) {
        (Some("jpg"), Some(e)) if e == "jpeg" || e == "jpe" => e,
        (Some("html"), Some(e)) if e == "htm" || e == "xhtml" => "html".to_string(),
        (Some(want), _) => want.to_string(),
        (None, Some(e)) => e,
        (None, None) => "bin".to_string(),
    };
    format!("{index}-{stem}.{ext}")
}

/// `dir/name`, refused unless `name` is a single plain file name.
pub fn safe_join(dir: &Path, name: &str) -> Result<PathBuf, MhtmlError> {
    let p = Path::new(name);
    let mut comps = p.components();
    let ok = matches!(comps.next(), Some(Component::Normal(c)) if c == p.as_os_str())
        && comps.next().is_none()
        && !name.contains(['/', '\\', '\0'])
        && !name.is_empty();
    if ok { Ok(dir.join(name)) } else { Err(MhtmlError::UnsafeName(name.to_string())) }
}

/// The scheme of `url`, lower-cased, when it has one.
fn scheme(url: &str) -> Option<String> {
    let colon = url.find(':')?;
    let s = &url[..colon];
    let mut chars = s.chars();
    let first = chars.next()?;
    (first.is_ascii_alphabetic() && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) && s.len() > 1).then(|| s.to_ascii_lowercase())
}

/// Removes `.` and `..` segments from a URL path (RFC 3986, 5.2.4).
fn remove_dots(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let segments: Vec<&str> = path.split('/').collect();
    let n = segments.len();
    for (i, seg) in segments.iter().enumerate() {
        match *seg {
            "." => {
                if i == n - 1 {
                    out.push("");
                }
            }
            ".." => {
                if out.len() > 1 {
                    out.pop();
                }
                if i == n - 1 {
                    out.push("");
                }
            }
            s => out.push(s),
        }
    }
    let joined = out.join("/");
    if path.starts_with('/') && !joined.starts_with('/') { format!("/{joined}") } else { joined }
}

/// Resolves `reference` against `base` (RFC 3986, 5.2, simplified to the
/// shapes archives use). `None` when the base has no scheme.
pub fn resolve(base: &str, reference: &str) -> Option<String> {
    if scheme(reference).is_some() {
        return Some(reference.to_string());
    }
    let base_scheme = scheme(base)?;
    let after = &base[base_scheme.len() + 1..];
    let (authority, rest) = match after.strip_prefix("//") {
        Some(a) => {
            let end = a.find(['/', '?', '#']).unwrap_or(a.len());
            (Some(&a[..end]), &a[end..])
        }
        None => (None, after),
    };
    let rest = rest.split('#').next().unwrap_or(rest);
    let (path, query) = rest.split_once('?').map_or((rest, None), |(p, q)| (p, Some(q)));
    let prefix = match authority {
        Some(a) => format!("{base_scheme}://{a}"),
        None => format!("{base_scheme}:"),
    };
    if let Some(net) = reference.strip_prefix("//") {
        return Some(format!("{base_scheme}://{net}"));
    }
    if reference.starts_with('/') {
        let (p, q) = reference.split_once('?').map_or((reference, None), |(p, q)| (p, Some(q)));
        return Some(format!("{prefix}{}{}", remove_dots(p), q.map(|q| format!("?{q}")).unwrap_or_default()));
    }
    if reference.starts_with('?') {
        return Some(format!("{prefix}{path}{reference}"));
    }
    if reference.is_empty() {
        return Some(format!("{prefix}{path}{}", query.map(|q| format!("?{q}")).unwrap_or_default()));
    }
    let dir = match path.rfind('/') {
        Some(i) => &path[..=i],
        None if authority.is_some() => "/",
        None => "",
    };
    let (p, q) = reference.split_once('?').map_or((reference, None), |(p, q)| (p, Some(q)));
    Some(format!("{prefix}{}{}", remove_dots(&format!("{dir}{p}")), q.map(|q| format!("?{q}")).unwrap_or_default()))
}

/// Finds archive parts by the addresses pages use for them.
struct Lookup {
    by_location: HashMap<String, usize>,
    by_cid: HashMap<String, usize>,
    names: Vec<String>,
}

impl Lookup {
    fn new(archive: &Archive, names: Vec<String>) -> Lookup {
        let root_base = archive.parts[archive.root].location.clone();
        let mut by_location = HashMap::new();
        let mut by_cid = HashMap::new();
        for (i, p) in archive.parts.iter().enumerate() {
            if let Some(l) = &p.location {
                by_location.entry(l.clone()).or_insert(i);
                by_location.entry(percent_decode(l)).or_insert(i);
                if let Some(abs) = root_base.as_deref().and_then(|b| resolve(b, l)) {
                    by_location.entry(abs).or_insert(i);
                }
            }
            if let Some(c) = &p.content_id {
                by_cid.entry(c.clone()).or_insert(i);
            }
        }
        Lookup { by_location, by_cid, names }
    }

    /// The part `reference` (no fragment, entities already decoded) names,
    /// read against `base`.
    fn find(&self, reference: &str, base: Option<&str>) -> Option<usize> {
        let r = reference.trim();
        if r.is_empty() {
            return None;
        }
        if r.get(..4).is_some_and(|p| p.eq_ignore_ascii_case("cid:")) && r.len() > 4 {
            let id = percent_decode(&r[4..]);
            return self.by_cid.get(&id).or_else(|| self.by_location.get(r)).copied();
        }
        if let Some(&i) = self.by_location.get(r) {
            return Some(i);
        }
        let abs = base.and_then(|b| resolve(b, r))?;
        self.by_location.get(&abs).or_else(|| self.by_location.get(&percent_decode(&abs))).copied()
    }

    /// The new reference for `reference` (entities decoded), keeping its
    /// fragment; `None` to leave it alone.
    fn rewrite(&self, reference: &str, base: Option<&str>) -> Option<String> {
        let t = reference.trim();
        if t.is_empty() || t.starts_with('#') {
            return None;
        }
        let lower = t.get(..11).unwrap_or(t).to_ascii_lowercase();
        if lower.starts_with("data:") || lower.starts_with("javascript:") || lower.starts_with("about:") || lower.starts_with("blob:") || lower.starts_with("mailto:") {
            return None;
        }
        let without = t.split('#').next().unwrap_or(t);
        let i = self.find(without, base)?;
        let fragment = t.find('#').map_or("", |at| &t[at..]);
        Some(format!("{}{fragment}", percent_encode(&self.names[i])))
    }
}

/// Decodes the character references that matter in URLs and inline styles.
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&#38;", "&")
        .replace("&#x26;", "&")
        .replace("&amp;", "&")
}

/// Rewrites the `url(...)` and `@import "..."` references in a style
/// sheet, read against `base`.
fn rewrite_css(css: &str, base: Option<&str>, lookup: &Lookup) -> String {
    let mut out = String::with_capacity(css.len());
    let bytes = css.as_bytes();
    let mut i = 0;
    let mut copied = 0;
    while i < bytes.len() {
        // Comments are copied as they are.
        if bytes[i..].starts_with(b"/*") {
            i = css[i + 2..].find("*/").map_or(bytes.len(), |e| i + 2 + e + 2);
            continue;
        }
        let url = bytes.len() - i >= 4 && bytes[i..i + 4].eq_ignore_ascii_case(b"url(");
        let import = bytes.len() - i >= 7 && bytes[i..i + 7].eq_ignore_ascii_case(b"@import");
        if !url && !import {
            i += 1;
            continue;
        }
        let mut j = i + if url { 4 } else { 7 };
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        let quote = bytes.get(j).copied().filter(|&q| q == b'"' || q == b'\'');
        let (start, end) = match quote {
            Some(q) => {
                let s = j + 1;
                let Some(e) = css[s..].find(q as char).map(|e| s + e) else { break };
                (s, e)
            }
            None if url => {
                let s = j;
                let Some(e) = css[s..].find(')').map(|e| s + e) else { break };
                let mut e2 = e;
                while e2 > s && bytes[e2 - 1].is_ascii_whitespace() {
                    e2 -= 1;
                }
                (s, e2)
            }
            // `@import url(...)` is handled when `url(` is reached.
            None => {
                i = j;
                continue;
            }
        };
        let raw = &css[start..end];
        if let Some(new) = lookup.rewrite(raw, base) {
            out.push_str(&css[copied..start]);
            out.push_str(&new);
            copied = end;
        }
        i = end + 1;
    }
    out.push_str(&css[copied..]);
    out
}

/// Attributes that hold one URL.
const URL_ATTRS: [&str; 8] = ["src", "href", "poster", "background", "data", "xlink:href", "action", "longdesc"];

/// The meta elements put at the top of every unpacked page.
fn injected_head(policy: Policy) -> String {
    match policy {
        Policy::Offline => format!("<meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"{CSP}\">"),
        Policy::None => "<meta charset=\"utf-8\">".to_string(),
    }
}

/// A tag's attribute: its lower-cased name and its value's byte span in
/// the page (`None` for an attribute without a value), with its quote.
struct Attr {
    name: String,
    value: Option<(usize, usize)>,
    quote: Option<char>,
}

/// Reads the tag starting at `start` (just after its `<`): its lower-cased
/// name, attributes and the index just past its `>`.
fn read_tag(html: &str, start: usize) -> (String, Vec<Attr>, usize) {
    let b = html.as_bytes();
    let mut i = start;
    while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' && b[i] != b'/' {
        i += 1;
    }
    let name = html[start..i].to_ascii_lowercase();
    let mut attrs = Vec::new();
    loop {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        if i >= b.len() {
            return (name, attrs, b.len());
        }
        if b[i] == b'>' {
            return (name, attrs, i + 1);
        }
        let ns = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && !matches!(b[i], b'=' | b'>') && !(b[i] == b'/' && i > ns) {
            i += 1;
        }
        if i == ns {
            i += 1;
            continue;
        }
        let aname = html[ns..i].to_ascii_lowercase();
        let mut j = i;
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        if j < b.len() && b[j] == b'=' {
            j += 1;
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < b.len() && (b[j] == b'"' || b[j] == b'\'') {
                let q = b[j];
                let vs = j + 1;
                let ve = html[vs..].find(q as char).map_or(b.len(), |e| vs + e);
                attrs.push(Attr { name: aname, value: Some((vs, ve)), quote: Some(q as char) });
                i = (ve + 1).min(b.len());
            } else {
                let vs = j;
                while j < b.len() && !b[j].is_ascii_whitespace() && b[j] != b'>' {
                    j += 1;
                }
                attrs.push(Attr { name: aname, value: Some((vs, j)), quote: None });
                i = j;
            }
        } else {
            attrs.push(Attr { name: aname, value: None, quote: None });
        }
    }
}

/// The first index from `from` where `needle` (ASCII, lower-case) occurs
/// in `hay`, ignoring ASCII case.
fn find_ci(hay: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()].eq_ignore_ascii_case(needle))
}

/// Escapes a new attribute value for its quote.
fn escape_attr(v: &str, quote: Option<char>) -> String {
    let v = v.replace('&', "&amp;");
    match quote {
        Some('\'') => v.replace('\'', "&#39;"),
        _ => v.replace('"', "&quot;"),
    }
}

/// Rewrites a `srcset` value (`url 1x, url 2x`).
fn rewrite_srcset(value: &str, base: Option<&str>, lookup: &Lookup) -> Option<String> {
    let mut changed = false;
    let items: Vec<String> = value
        .split(',')
        .map(|cand| {
            let c = cand.trim();
            let (url, desc) = c.split_once(char::is_whitespace).map_or((c, ""), |(u, d)| (u, d.trim()));
            match lookup.rewrite(url, base) {
                Some(n) => {
                    changed = true;
                    if desc.is_empty() { n } else { format!("{n} {desc}") }
                }
                None => c.to_string(),
            }
        })
        .collect();
    changed.then(|| items.join(", "))
}

/// Rewrites a page's references to archive parts and puts the charset and
/// security policy at the top of its head. `base` is the part's own
/// location; a `<base href>` replaces it for what follows and is pointed
/// at the folder.
fn rewrite_html(html: &str, base: Option<&str>, lookup: &Lookup, policy: Policy) -> String {
    let mut base = base.map(str::to_string);
    let b = html.as_bytes();
    let mut out = String::with_capacity(html.len() + 512);
    let mut copied = 0;
    let mut injected = false;
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        if html[i..].starts_with("<!--") {
            i = html[i + 4..].find("-->").map_or(b.len(), |e| i + 4 + e + 3);
            continue;
        }
        if !b.get(i + 1).is_some_and(u8::is_ascii_alphabetic) {
            i += 1;
            continue;
        }
        let (name, attrs, end) = read_tag(html, i + 1);
        // Replacements inside this tag, as (start, end, text).
        let mut edits: Vec<(usize, usize, String)> = Vec::new();
        for a in &attrs {
            let Some((vs, ve)) = a.value else { continue };
            let raw = &html[vs..ve];
            let decoded = decode_entities(raw);
            let new = if name == "base" && a.name == "href" {
                if let Some(abs) = match base.as_deref() {
                    Some(bs) => resolve(bs, decoded.trim()),
                    None => scheme(decoded.trim()).map(|_| decoded.trim().to_string()),
                } {
                    base = Some(abs);
                }
                Some("./".to_string())
            } else if URL_ATTRS.contains(&a.name.as_str()) {
                lookup.rewrite(&decoded, base.as_deref())
            } else if a.name == "srcset" || a.name == "imagesrcset" {
                rewrite_srcset(&decoded, base.as_deref(), lookup)
            } else if a.name == "style" {
                let css = rewrite_css(&decoded, base.as_deref(), lookup);
                (css != decoded).then_some(css)
            } else {
                None
            };
            if let Some(new) = new {
                let text = match a.quote {
                    Some(_) => escape_attr(&new, a.quote),
                    None => format!("\"{}\"", escape_attr(&new, Some('"'))),
                };
                edits.push((vs, ve, text));
            }
        }
        if !edits.is_empty() {
            for (s, e, text) in edits {
                out.push_str(&html[copied..s]);
                out.push_str(&text);
                copied = e;
            }
        }
        let mut next = end;
        if !injected && name == "head" {
            out.push_str(&html[copied..end]);
            out.push_str(&injected_head(policy));
            copied = end;
            injected = true;
        }
        if matches!(name.as_str(), "style" | "script" | "textarea" | "title") {
            let close = format!("</{name}");
            let stop = find_ci(b, end, close.as_bytes()).unwrap_or(b.len());
            if name == "style" {
                out.push_str(&html[copied..end]);
                out.push_str(&rewrite_css(&html[end..stop], base.as_deref(), lookup));
                copied = stop;
            }
            next = stop;
        }
        i = next.max(i + 1);
    }
    out.push_str(&html[copied..]);
    if injected {
        return out;
    }
    // No <head>: after the doctype (before it would switch to quirks mode).
    let mut n = out.len().min(512);
    while !out.is_char_boundary(n) {
        n -= 1;
    }
    let lower = out[..n].to_ascii_lowercase();
    let at = lower.find("<!doctype").and_then(|d| out[d..].find('>').map(|e| d + e + 1)).unwrap_or(0);
    out.insert_str(at, &injected_head(policy));
    out
}

/// Unpacks the archive in `data` into `dir`, which must exist and should
/// be empty: the root page as `index.html`, every other part under a name
/// falcon makes, HTML and CSS rewritten to point at each other's files.
pub fn unpack(data: &[u8], dir: &Path, limits: Limits, policy: Policy) -> Result<Unpacked, MhtmlError> {
    let archive = parse(data, limits)?;
    let names: Vec<String> = archive.parts.iter().enumerate().map(|(i, p)| if i == archive.root { ROOT_NAME.to_string() } else { file_name(i, p) }).collect();
    let lookup = Lookup::new(&archive, names);
    let root_base = archive.parts[archive.root].location.clone();
    let mut files = Vec::with_capacity(archive.parts.len());
    let mut written: u64 = 0;
    let mut order: Vec<usize> = (0..archive.parts.len()).collect();
    order.swap(0, archive.root);
    for i in order {
        let part = &archive.parts[i];
        let path = safe_join(dir, &lookup.names[i])?;
        let base = part.location.as_deref().filter(|l| scheme(l).is_some_and(|s| s != "cid")).or(root_base.as_deref());
        let bytes = if part.is_html() || part.is_css() {
            let text = decode_text(&part.body, part.charset.as_deref(), part.is_html());
            let text = if part.is_html() { rewrite_html(&text, base, &lookup, policy) } else { rewrite_css(&text, base, &lookup) };
            let mut v = Vec::with_capacity(text.len() + 3);
            v.extend_from_slice(b"\xEF\xBB\xBF");
            v.extend_from_slice(text.as_bytes());
            v
        } else {
            part.body.clone()
        };
        written += bytes.len() as u64;
        // Rewriting adds a little to each page; allow for it.
        if written > limits.max_total.saturating_add(limits.max_total / 8) {
            return Err(MhtmlError::TooLarge(limits.max_total));
        }
        fs::write(&path, bytes)?;
        files.push(path);
    }
    Ok(Unpacked { root: dir.join(ROOT_NAME), files, subject: archive.subject, address: archive.address })
}

/// Unpacks the archive file at `path` into `dir`, refusing a file larger
/// than twice the size limit (base64 makes parts a third larger).
pub fn unpack_file(path: &Path, dir: &Path, limits: Limits, policy: Policy) -> Result<Unpacked, MhtmlError> {
    let len = fs::metadata(path)?.len();
    if len > limits.max_total.saturating_mul(2) {
        return Err(MhtmlError::TooLarge(limits.max_total));
    }
    unpack(&fs::read(path)?, dir, limits, policy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chrome_sample_unpacks() {
        let data = include_bytes!("../testdata/mhtml/chrome.mhtml");
        let dir = tempfile::tempdir().unwrap();
        let u = unpack(data, dir.path(), Limits::default(), Policy::Offline).unwrap();
        let html = fs::read_to_string(&u.root).unwrap();
        assert!(html.starts_with('\u{FEFF}'), "a byte order mark leads");
        assert!(html.contains("<head><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\""));
        assert!(html.contains("<h1>Café archive</h1>"), "quoted-printable UTF-8 decoded");
        assert!(html.contains("href=\"1-css-1.css\""), "cid: stylesheet rewritten");
        assert!(html.contains("src=\"2-red.png\""), "full-URL image with a query rewritten");
        assert_eq!(u.subject.as_deref(), Some("Café page"));
        assert_eq!(u.address.as_deref(), Some("https://example.com/page"));
        let names: Vec<String> = u.files.iter().map(|f| f.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["index.html", "1-css-1.css", "2-red.png"]);
        let png = fs::read(dir.path().join("2-red.png")).unwrap();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(image::load_from_memory(&png).is_ok(), "base64 decoded intact");
    }

    // ---- helpers ----

    const CHROME: &[u8] = include_bytes!("../testdata/mhtml/chrome.mhtml");

    /// One entity: header lines (no trailing break), a blank line, the body.
    fn part(headers: &str, body: &[u8]) -> Vec<u8> {
        let mut v = headers.replace('\n', "\r\n").into_bytes();
        v.extend_from_slice(b"\r\n\r\n");
        v.extend_from_slice(body);
        v
    }

    fn multipart_body(boundary: &str, parts: &[Vec<u8>]) -> Vec<u8> {
        let mut v = Vec::new();
        for p in parts {
            v.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
            v.extend_from_slice(p);
            v.extend_from_slice(b"\r\n");
        }
        v.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        v
    }

    /// A whole file: `top` headers, then the parts between `boundary` lines.
    fn wrap(top: &str, boundary: &str, parts: &[Vec<u8>]) -> Vec<u8> {
        part(top, &multipart_body(boundary, parts))
    }

    fn archive(parts: &[Vec<u8>]) -> Vec<u8> {
        wrap("MIME-Version: 1.0\nContent-Type: multipart/related; boundary=\"BND\"", "BND", parts)
    }

    fn html_part(location: &str, body: &str) -> Vec<u8> {
        part(&format!("Content-Type: text/html; charset=utf-8\nContent-Location: {location}"), body.as_bytes())
    }

    fn png_part(location: &str) -> Vec<u8> {
        part(&format!("Content-Type: image/png\nContent-Location: {location}"), b"PNGDATA")
    }

    fn unpack_to(data: &[u8], policy: Policy) -> (tempfile::TempDir, Unpacked) {
        let dir = tempfile::tempdir().unwrap();
        let u = unpack(data, dir.path(), Limits::default(), policy).unwrap();
        (dir, u)
    }

    /// A file's text without its byte order mark.
    fn read_text(dir: &Path, name: &str) -> String {
        let bytes = fs::read(dir.join(name)).unwrap();
        assert!(bytes.starts_with(b"\xEF\xBB\xBF"), "{name} leads with a byte order mark");
        String::from_utf8(bytes[3..].to_vec()).unwrap()
    }

    fn lcg(state: &mut u64) -> u8 {
        *state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (*state >> 33) as u8
    }

    // ---- transfer encodings ----

    #[test]
    fn base64_with_breaks_missing_padding_and_stray_characters() {
        let original: Vec<u8> = (0..=255u8).collect();
        let encoded = base64::engine::general_purpose::STANDARD.encode(&original);
        let unpadded = encoded.trim_end_matches('=');
        let mut messy = String::new();
        for (i, c) in unpadded.chars().enumerate() {
            if i % 76 == 0 && i > 0 {
                messy.push_str("\r\n");
            }
            if i == 10 {
                messy.push_str(" *!\t");
            }
            messy.push(c);
        }
        messy.push_str("\r\n");
        let data = archive(&[part("Content-Type: application/octet-stream\nContent-Transfer-Encoding: base64", messy.as_bytes())]);
        let a = parse(&data, Limits::default()).unwrap();
        assert_eq!(a.parts[0].body, original);
    }

    #[test]
    fn quoted_printable_escapes_and_soft_breaks() {
        assert_eq!(decode_qp(b"a=3Db=3db=C3=a9"), "a=b=bé".as_bytes());
        // Soft breaks: CRLF, LF, and trailing blanks before the break.
        assert_eq!(decode_qp(b"ab=\r\ncd=\nef=  \r\ngh=\t\nij"), b"abcdefghij");
        // A lone `=` that starts neither an escape nor a break stays.
        assert_eq!(decode_qp(b"a=zz b= c =g"), b"a=zz b= c =g");
        // An `=` at the very end is a soft break with nothing after it.
        assert_eq!(decode_qp(b"x=="), b"x=");
    }

    #[test]
    fn quoted_printable_cr_before_crlf_gives_one_crlf() {
        let data = archive(&[part("Content-Type: text/plain\nContent-Transfer-Encoding: quoted-printable", b"line1=0D\r\nline2=0D\r\n")]);
        let a = parse(&data, Limits::default()).unwrap();
        assert_eq!(a.parts[0].body, b"line1\r\nline2\r\n");
    }

    // ---- structure ----

    #[test]
    fn nested_multipart_flattens_and_root_is_the_html() {
        let inner = multipart_body("inner", &[part("Content-Type: text/plain", b"plain"), part("Content-Type: text/html", b"<p>html</p>")]);
        let nested = part("Content-Type: multipart/alternative; boundary=\"inner\"", &inner);
        let data = archive(&[nested, part("Content-Type: image/png", b"png")]);
        let a = parse(&data, Limits::default()).unwrap();
        let types: Vec<&str> = a.parts.iter().map(|p| p.content_type.as_str()).collect();
        assert_eq!(types, ["text/plain", "text/html", "image/png"]);
        assert_eq!(a.root, 1);
    }

    #[test]
    fn start_parameter_picks_the_root_by_content_id() {
        let parts = [
            part("Content-Type: text/html\nContent-ID: <first@x>", b"first"),
            part("Content-Type: text/html\nContent-ID: <second@x>", b"second"),
        ];
        let plain = parse(&archive(&parts), Limits::default()).unwrap();
        assert_eq!(plain.root, 0, "without start the first html part");
        let data = wrap("Content-Type: multipart/related; start=\"<second@x>\"; boundary=\"BND\"", "BND", &parts);
        let a = parse(&data, Limits::default()).unwrap();
        assert_eq!(a.root, 1);
        assert_eq!(a.parts[a.root].body, b"second");
    }

    // ---- text ----

    /// Unpacks one html part with `ctype` header text and returns index.html.
    fn unpack_page(ctype: &str, body: &[u8]) -> String {
        let data = archive(&[part(&format!("Content-Type: {ctype}"), body)]);
        let (dir, _) = unpack_to(&data, Policy::None);
        read_text(dir.path(), "index.html")
    }

    #[test]
    fn charset_utf8_page() {
        let html = unpack_page("text/html; charset=utf-8", "<p>é€</p>".as_bytes());
        assert!(html.contains("<p>é€</p>"));
    }

    #[test]
    fn charset_windows_1252_page() {
        let html = unpack_page("text/html; charset=windows-1252", b"<p>\x80 \xE9 \x93q\x94</p>");
        assert!(html.contains("<p>€ é “q”</p>"), "{html}");
    }

    #[test]
    fn charset_iso_8859_1_label_reads_as_windows_1252() {
        let html = unpack_page("text/html; charset=iso-8859-1", b"<p>\x80 \xE9 \x93q\x94</p>");
        assert!(html.contains("<p>€ é “q”</p>"), "{html}");
    }

    #[test]
    fn charset_undeclared_invalid_utf8_falls_back_to_windows_1252() {
        let html = unpack_page("text/html", b"<p>caf\xE9 \x80</p>");
        assert!(html.contains("<p>café €</p>"), "{html}");
    }

    #[test]
    fn charset_from_meta_when_mime_gives_none() {
        let html = unpack_page("text/html", b"<html><head><meta charset=\"windows-1252\"></head><body>\x93q\x94 \xE9</body></html>");
        assert!(html.contains("“q” é"), "{html}");
    }

    #[test]
    fn encoded_words_in_headers() {
        assert_eq!(decode_words("=?utf-8?Q?Caf=C3=A9_au_lait?="), "Café au lait");
        assert_eq!(decode_words("=?utf-8?B?Q2Fmw6k=?="), "Café");
        assert_eq!(decode_words("=?iso-8859-1?Q?Caf=E9?="), "Café");
        // Blanks between two adjacent encoded words are dropped.
        assert_eq!(decode_words("=?utf-8?Q?Caf?= =?utf-8?Q?=C3=A9?="), "Café");
        // Text around a word is kept.
        assert_eq!(decode_words("pre =?utf-8?Q?x?= post"), "pre x post");
        // An unknown charset is left as written.
        assert_eq!(decode_words("=?x-nonsense?Q?abc?="), "=?x-nonsense?Q?abc?=");
    }

    #[test]
    fn encoded_words_in_location_and_subject() {
        let data = wrap(
            "Subject: =?iso-8859-1?Q?Caf=E9_page?=\nContent-Type: multipart/related; boundary=\"BND\"",
            "BND",
            &[part("Content-Type: text/html\nContent-Location: =?utf-8?B?aHR0cHM6Ly9leGFtcGxlLmNvbS9hLmh0bWw=?=", b"x")],
        );
        let a = parse(&data, Limits::default()).unwrap();
        assert_eq!(a.subject.as_deref(), Some("Café page"));
        assert_eq!(a.parts[0].location.as_deref(), Some("https://example.com/a.html"));
        assert_eq!(a.address.as_deref(), Some("https://example.com/a.html"));
    }

    // ---- reference rewriting ----

    const SITE: &str = "https://example.com/dir/sub/page.html";

    /// A page at `SITE` with the references the tests below look at, and
    /// the parts they point to (indexes in the comments).
    fn site() -> (tempfile::TempDir, Unpacked) {
        let root = r##"<!DOCTYPE html><html><head>
<link rel="stylesheet" href="../css/main.css">
<style>@import "c.css"; p { background: url(a.png) } i { background: url( 'a.png' ) }</style>
<script>var u = "a.png"; document.write('<img src="a.png">'); var c = "url(a.png)";</script>
</head><body>
<img id=rel src="a.png">
<img id=up src="../up.png">
<img id=abs src="/abs.png">
<img id=proto src="//cdn.example.com/x.png">
<img id=query src="q.png?v=2">
<a id=frag href="frag.html#sec">f</a>
<a id=amp href="a&amp;b.png">amp</a>
<img id=srcset srcset="s1.png 1x, s2.png 2x" src="s1.png">
<div id=inline style="background:url(&quot;a.png&quot;)"></div>
<img id=cid src="cid:logo@x">
<img id=missing src="missing.png">
<img id=remote src="https://example.com/nope.png">
<img id=data src="data:image/png;base64,AAAA">
<base href="https://example.com/other/">
<img id=based src="b.png">
</body></html>"##;
        let data = archive(&[
            html_part(SITE, root),                                                          // 0
            png_part("https://example.com/dir/sub/a.png"),                                  // 1
            png_part("https://example.com/dir/up.png"),                                     // 2
            png_part("https://example.com/abs.png"),                                        // 3
            png_part("https://cdn.example.com/x.png"),                                      // 4
            png_part("https://example.com/dir/sub/q.png?v=2"),                              // 5
            html_part("https://example.com/dir/sub/frag.html", "<p>f</p>"),                 // 6
            png_part("https://example.com/dir/sub/a&b.png"),                                // 7
            png_part("https://example.com/dir/sub/s1.png"),                                 // 8
            png_part("https://example.com/dir/sub/s2.png"),                                 // 9
            part("Content-Type: text/css\nContent-Location: https://example.com/dir/sub/c.css", b"p{}"), // 10
            part("Content-Type: text/css\nContent-Location: https://example.com/dir/css/main.css", b"div { background: url(../img/in.png) }"), // 11
            png_part("https://example.com/dir/img/in.png"),                                 // 12
            png_part("https://example.com/other/b.png"),                                    // 13
            part("Content-Type: image/png\nContent-ID: <logo@x>", b"PNGDATA"),              // 14
        ]);
        unpack_to(&data, Policy::None)
    }

    #[test]
    fn rewrites_relative_absolute_and_protocol_relative_references() {
        let (dir, _) = site();
        let html = read_text(dir.path(), "index.html");
        assert!(html.contains("id=rel src=\"1-a.png\""), "{html}");
        assert!(html.contains("id=up src=\"2-up.png\""), "dot-dot path");
        assert!(html.contains("id=abs src=\"3-abs.png\""), "absolute path");
        assert!(html.contains("id=proto src=\"4-x.png\""), "protocol-relative");
        assert!(html.contains("<link rel=\"stylesheet\" href=\"11-main.css\">"));
        assert!(html.contains("id=cid src=\"14-logo.png\""), "cid: reference");
    }

    #[test]
    fn rewrites_query_fragment_and_entities() {
        let (dir, _) = site();
        let html = read_text(dir.path(), "index.html");
        assert!(html.contains("id=query src=\"5-q.png\""), "query matched the part with the same query");
        assert!(html.contains("id=frag href=\"6-frag.html#sec\""), "fragment kept");
        assert!(html.contains("id=amp href=\"7-a_b.png\""), "&amp; in the href matched a & location");
    }

    #[test]
    fn rewrites_srcset_with_descriptors_and_inline_style() {
        let (dir, _) = site();
        let html = read_text(dir.path(), "index.html");
        assert!(html.contains("srcset=\"8-s1.png 1x, 9-s2.png 2x\""), "{html}");
        assert!(html.contains("style=\"background:url(&quot;1-a.png&quot;)\""), "{html}");
    }

    #[test]
    fn rewrites_style_blocks_with_url_and_import() {
        let (dir, _) = site();
        let html = read_text(dir.path(), "index.html");
        assert!(html.contains("@import \"10-c.css\";"), "{html}");
        assert!(html.contains("url(1-a.png)"), "unquoted url()");
        assert!(html.contains("url( '1-a.png' )"), "quoted url() with blanks");
    }

    #[test]
    fn css_part_resolves_against_its_own_location() {
        let (dir, _) = site();
        let css = read_text(dir.path(), "11-main.css");
        assert!(css.contains("url(12-in.png)"), "{css}");
    }

    #[test]
    fn base_href_changes_the_base_and_points_at_the_folder() {
        let (dir, _) = site();
        let html = read_text(dir.path(), "index.html");
        assert!(html.contains("<base href=\"./\">"), "{html}");
        assert!(html.contains("id=based src=\"13-b.png\""), "resolved against the new base");
    }

    #[test]
    fn leaves_unknown_and_data_references_alone() {
        let (dir, _) = site();
        let html = read_text(dir.path(), "index.html");
        assert!(html.contains("id=missing src=\"missing.png\""));
        assert!(html.contains("id=remote src=\"https://example.com/nope.png\""));
        assert!(html.contains("id=data src=\"data:image/png;base64,AAAA\""));
    }

    #[test]
    fn script_content_is_not_rewritten() {
        let (dir, _) = site();
        let html = read_text(dir.path(), "index.html");
        assert!(html.contains(r#"<script>var u = "a.png"; document.write('<img src="a.png">'); var c = "url(a.png)";</script>"#), "{html}");
    }

    #[test]
    fn word_archive_rewrites_image_and_file_list() {
        let root = part(
            "Content-Type: text/html; charset=\"windows-1252\"\nContent-Transfer-Encoding: quoted-printable\nContent-Location: file:///C:/Users/x/doc.htm",
            b"<html><head><link rel=3DFile-List href=3D\"doc_files/filelist.xml\"></head><body><img src=3D\"doc_files/image001.png\"></body></html>",
        );
        let data = wrap(
            "MIME-Version: 1.0\nContent-Type: multipart/related; type=\"text/html\"; boundary=\"----=_NextPart_01\"",
            "----=_NextPart_01",
            &[
                root,
                png_part("file:///C:/Users/x/doc_files/image001.png"),
                part("Content-Type: text/xml\nContent-Location: file:///C:/Users/x/doc_files/filelist.xml", b"<xml/>"),
            ],
        );
        let (dir, u) = unpack_to(&data, Policy::None);
        assert_eq!(u.files.len(), 3);
        let html = read_text(dir.path(), "index.html");
        assert!(html.contains("href=\"2-filelist.xml\""), "{html}");
        assert!(html.contains("src=\"1-image001.png\""), "{html}");
    }

    // ---- file names ----

    #[test]
    fn hostile_locations_land_as_plain_files_in_the_folder() {
        let outer = tempfile::tempdir().unwrap();
        let out = outer.path().join("out");
        fs::create_dir(&out).unwrap();
        let locations: [&[u8]; 6] = [
            b"../../../etc/passwd",
            b"/etc/passwd",
            b"file:///etc/passwd",
            b"..\\..\\x.png",
            b"a\0b/c.png",
            b"x/y/z/../../w.png",
        ];
        let mut parts = vec![html_part("https://example.com/", "<p>hi</p>")];
        for loc in locations {
            let mut headers = b"Content-Type: image/png\r\nContent-Location: ".to_vec();
            headers.extend_from_slice(loc);
            headers.extend_from_slice(b"\r\n\r\nPNGDATA");
            parts.push(headers);
        }
        let data = archive(&parts);
        let u = unpack(&data, &out, Limits::default(), Policy::None).unwrap();
        assert_eq!(u.files.len(), parts.len());
        let mut count = 0;
        for entry in fs::read_dir(&out).unwrap() {
            let entry = entry.unwrap();
            assert!(entry.file_type().unwrap().is_file(), "{:?} is a plain file", entry.path());
            assert_eq!(entry.path().parent().unwrap(), out);
            count += 1;
        }
        assert_eq!(count, parts.len());
        for f in &u.files {
            assert_eq!(f.parent().unwrap(), out);
        }
        let siblings: Vec<_> = fs::read_dir(outer.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(siblings, ["out"], "nothing written beside the folder");
    }

    #[test]
    fn safe_join_accepts_only_plain_names() {
        let dir = Path::new("/some/dir");
        assert_eq!(safe_join(dir, "index.html").unwrap(), dir.join("index.html"));
        for bad in ["..", ".", "a/b", "/abs", "", "a\\b", "a\0b"] {
            assert!(matches!(safe_join(dir, bad), Err(MhtmlError::UnsafeName(_))), "{bad:?}");
        }
    }

    #[test]
    fn percent_encoding_round_trips() {
        assert_eq!(percent_encode("a b&é.png"), "a%20b%26%C3%A9.png");
        assert_eq!(percent_decode("a%20b%26%C3%A9.png"), "a b&é.png");
        assert_eq!(percent_decode("100%zz%2"), "100%zz%2", "invalid escapes kept");
    }

    #[test]
    fn resolve_follows_rfc_3986_for_common_shapes() {
        let base = "https://example.com/a/b/c.html?x=1";
        assert_eq!(resolve(base, "d.png").as_deref(), Some("https://example.com/a/b/d.png"));
        assert_eq!(resolve(base, "../d.png").as_deref(), Some("https://example.com/a/d.png"));
        assert_eq!(resolve(base, "/d.png").as_deref(), Some("https://example.com/d.png"));
        assert_eq!(resolve(base, "//other.org/d.png").as_deref(), Some("https://other.org/d.png"));
        assert_eq!(resolve(base, "?y=2").as_deref(), Some("https://example.com/a/b/c.html?y=2"));
        assert_eq!(resolve(base, "http://z/").as_deref(), Some("http://z/"));
        assert_eq!(resolve("not a url", "d.png"), None);
    }

    // ---- limits ----

    fn text_parts(n: usize, size: usize) -> Vec<u8> {
        let parts: Vec<Vec<u8>> = (0..n).map(|_| part("Content-Type: text/plain", &vec![b'x'; size])).collect();
        archive(&parts)
    }

    #[test]
    fn total_size_cap() {
        let data = text_parts(2, 20);
        let limits = Limits { max_total: 30, ..Limits::default() };
        assert!(matches!(parse(&data, limits), Err(MhtmlError::TooLarge(30))));
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(unpack(&data, dir.path(), limits, Policy::None), Err(MhtmlError::TooLarge(_))));
        assert!(parse(&data, Limits { max_total: 40, ..Limits::default() }).is_ok());
    }

    #[test]
    fn part_count_cap() {
        let data = text_parts(3, 1);
        let limits = Limits { max_parts: 2, ..Limits::default() };
        assert!(matches!(parse(&data, limits), Err(MhtmlError::TooManyParts(2))));
        assert_eq!(parse(&data, Limits { max_parts: 3, ..Limits::default() }).unwrap().parts.len(), 3);
    }

    /// A file of `levels` multiparts inside each other around one text part.
    fn nested(levels: usize) -> Vec<u8> {
        let mut inner = part("Content-Type: text/plain", b"x");
        for k in (0..levels).rev() {
            let boundary = format!("lvl{k}x");
            inner = part(&format!("Content-Type: multipart/mixed; boundary=\"{boundary}\""), &multipart_body(&boundary, &[inner]));
        }
        inner
    }

    #[test]
    fn nesting_depth_cap() {
        assert_eq!(parse(&nested(8), Limits::default()).unwrap().parts.len(), 1);
        assert!(matches!(parse(&nested(9), Limits::default()), Err(MhtmlError::TooDeep)));
        assert!(matches!(parse(&nested(12), Limits::default()), Err(MhtmlError::TooDeep)));
    }

    // ---- malformed input ----

    #[test]
    fn empty_and_headerless_input_is_an_error() {
        assert!(parse(b"", Limits::default()).is_err());
        assert!(parse(b"\r\n\r\n", Limits::default()).is_err());
        assert!(parse(b"just some text", Limits::default()).is_err());
    }

    #[test]
    fn headers_only_is_an_error() {
        assert!(parse(b"Content-Type: multipart/related; boundary=\"B\"\r\n", Limits::default()).is_err());
        assert!(parse(b"Content-Type: multipart/related; boundary=\"B\"\r\n\r\n", Limits::default()).is_err());
    }

    #[test]
    fn missing_boundary_parameter_is_an_error() {
        let data = b"Content-Type: multipart/related\r\n\r\n--B\r\nContent-Type: text/html\r\n\r\nx\r\n--B--\r\n";
        assert!(matches!(parse(data, Limits::default()), Err(MhtmlError::NotArchive)));
    }

    #[test]
    fn boundary_that_never_appears_is_an_error() {
        let data = b"Content-Type: multipart/related; boundary=\"B\"\r\n\r\n--OTHER\r\nContent-Type: text/html\r\n\r\nx\r\n--OTHER--\r\n";
        assert!(parse(data, Limits::default()).is_err());
    }

    #[test]
    fn missing_final_boundary_still_reads_the_last_part() {
        let data = b"Content-Type: multipart/related; boundary=\"B\"\r\n\r\n--B\r\nContent-Type: text/html\r\n\r\nfirst\r\n--B\r\nContent-Type: image/png\r\n\r\nlast part";
        let a = parse(data, Limits::default()).unwrap();
        assert_eq!(a.parts.len(), 2);
        assert_eq!(a.parts[0].body, b"first");
        assert_eq!(a.parts[1].body, b"last part");
    }

    #[test]
    fn file_cut_inside_a_base64_part_gives_a_prefix() {
        let original: Vec<u8> = (0..100u8).collect();
        let encoded = base64::engine::general_purpose::STANDARD.encode(&original);
        for cut in 1..encoded.len() {
            let head = "Content-Type: application/octet-stream\r\nContent-Transfer-Encoding: base64\r\n\r\n";
            let data = format!("Content-Type: multipart/related; boundary=\"B\"\r\n\r\n--B\r\n{head}{}", &encoded[..cut]);
            let a = parse(data.as_bytes(), Limits::default()).unwrap();
            let body = &a.parts[0].body;
            assert!(original.starts_with(body), "cut at {cut}");
        }
    }

    #[test]
    fn random_bytes_never_panic() {
        let mut state = 0x1234_5678_9abc_def0u64;
        let dir = tempfile::tempdir().unwrap();
        for n in 0..400 {
            let len = (lcg(&mut state) as usize) * 2 % 300 + (n % 7);
            let mut buf: Vec<u8> = (0..len).map(|_| lcg(&mut state)).collect();
            if n % 2 == 1 {
                // Behind a valid multipart header, so the parts get read.
                let mut v = b"Content-Type: multipart/related; boundary=\"B\"\r\n\r\n--B\r\n".to_vec();
                v.append(&mut buf);
                buf = v;
            }
            let _ = parse(&buf, Limits::default());
            let _ = unpack(&buf, dir.path(), Limits::default(), Policy::Offline);
        }
    }

    #[test]
    fn every_prefix_of_the_chrome_sample_never_panics() {
        let dir = tempfile::tempdir().unwrap();
        for n in 0..=CHROME.len() {
            let _ = parse(&CHROME[..n], Limits::default());
            if n % 7 == 0 {
                let _ = unpack(&CHROME[..n], dir.path(), Limits::default(), Policy::Offline);
            }
        }
    }

    // ---- line ends and single parts ----

    #[test]
    fn lf_line_ends_parse_like_crlf() {
        let mut lf = Vec::new();
        let mut i = 0;
        while i < CHROME.len() {
            if CHROME[i..].starts_with(b"\r\n") {
                lf.push(b'\n');
                i += 2;
            } else {
                lf.push(CHROME[i]);
                i += 1;
            }
        }
        assert!(!lf.contains(&b'\r'));
        let a = parse(CHROME, Limits::default()).unwrap();
        let b = parse(&lf, Limits::default()).unwrap();
        assert_eq!(a.parts.len(), b.parts.len());
        assert_eq!(a.root, b.root);
        assert_eq!((&a.subject, &a.address), (&b.subject, &b.address));
        for (x, y) in a.parts.iter().zip(&b.parts) {
            assert_eq!((&x.content_type, &x.location, &x.content_id), (&y.content_type, &y.location, &y.content_id));
            if x.content_type.starts_with("text/") {
                // Hard line breaks in a text part keep the file's line ends.
                let tx = decode_text(&x.body, x.charset.as_deref(), x.content_type == "text/html");
                let ty = decode_text(&y.body, y.charset.as_deref(), y.content_type == "text/html");
                assert_eq!(tx.replace("\r\n", "\n"), ty);
            } else {
                assert_eq!(x.body, y.body);
            }
        }
    }

    #[test]
    fn single_part_html_unpacks_to_index() {
        let data = b"Content-Type: text/html; charset=utf-8\r\nContent-Location: https://example.com/\r\n\r\n<html><body>hi</body></html>";
        let (dir, u) = unpack_to(data, Policy::Offline);
        assert_eq!(u.files, [dir.path().join("index.html")]);
        assert!(read_text(dir.path(), "index.html").contains("<body>hi</body>"));
        assert_eq!(u.address.as_deref(), Some("https://example.com/"));
    }
}
