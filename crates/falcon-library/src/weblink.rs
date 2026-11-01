//! Reading the address out of link files: Windows `.url` shortcuts (an INI
//! file) and macOS `.webloc` files (an XML or binary property list).

use std::fs;
use std::io::Read;
use std::path::Path;

/// Link files bigger than this are not read.
const MAX_BYTES: u64 = 1 << 20;

/// Whether an extension names a link file (`url` or `webloc`, any case).
pub fn is_link_file(ext: &str) -> bool {
    ext.eq_ignore_ascii_case("url") || ext.eq_ignore_ascii_case("webloc")
}

/// Reads the address from a link file by its extension. Files over 1 MiB,
/// unreadable files and files without an address give `None`.
pub fn read(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?;
    if !is_link_file(ext) {
        return None;
    }
    let mut data = Vec::new();
    fs::File::open(path).ok()?.take(MAX_BYTES + 1).read_to_end(&mut data).ok()?;
    if data.len() as u64 > MAX_BYTES {
        return None;
    }
    parse(ext, &data)
}

/// Parses link file contents, picking the format from the extension.
pub fn parse(ext: &str, data: &[u8]) -> Option<String> {
    if ext.eq_ignore_ascii_case("url") {
        parse_internet_shortcut(data)
    } else if ext.eq_ignore_ascii_case("webloc") {
        parse_webloc(data)
    } else {
        None
    }
}

fn non_empty(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// Decodes the text of a `.url` file: UTF-16 with a byte order mark, UTF-8
/// with or without one.
fn decode_text(data: &[u8]) -> String {
    let utf16 = |rest: &[u8], le: bool| {
        let units: Vec<u16> =
            rest.as_chunks::<2>().0.iter().map(|&c| if le { u16::from_le_bytes(c) } else { u16::from_be_bytes(c) }).collect();
        String::from_utf16_lossy(&units)
    };
    match data {
        [0xFF, 0xFE, rest @ ..] => utf16(rest, true),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, false),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(data).into_owned(),
    }
}

/// The address of a Windows `.url` file: the first `URL=` key in the
/// `[InternetShortcut]` section. Section names and keys ignore case, and
/// keys in other sections are skipped.
pub fn parse_internet_shortcut(data: &[u8]) -> Option<String> {
    let text = decode_text(data);
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            inside = name.trim().eq_ignore_ascii_case("InternetShortcut");
        } else if inside
            && let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("URL")
        {
            return non_empty(value);
        }
    }
    None
}

/// The address of a macOS `.webloc` file, from an XML or binary property
/// list.
pub fn parse_webloc(data: &[u8]) -> Option<String> {
    if data.starts_with(b"bplist00") { parse_binary_plist(data) } else { parse_xml_plist(data) }
}

/// Finds `<key>URL</key>` and the `<string>` after it.
fn parse_xml_plist(data: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(data);
    let mut rest: &str = &text;
    while let Some(at) = rest.find("<key>") {
        rest = &rest[at + 5..];
        let end = rest.find("</key>")?;
        let key = &rest[..end];
        rest = &rest[end + 6..];
        if key.trim() != "URL" {
            continue;
        }
        let after = rest.trim_start().strip_prefix("<string>")?;
        let close = after.find("</string>")?;
        return non_empty(&unescape(&after[..close]));
    }
    None
}

/// Replaces the XML entities a property list uses.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let decoded = rest.find(';').and_then(|end| {
            let name = &rest[1..end];
            let ch = match name {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                _ => {
                    let num = name.strip_prefix('#')?;
                    let code = match num.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                        None => num.parse().ok()?,
                    };
                    char::from_u32(code)?
                }
            };
            Some((ch, end + 1))
        });
        match decoded {
            Some((ch, used)) => {
                out.push(ch);
                rest = &rest[used..];
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

/// A big-endian unsigned integer of 1 to 8 bytes.
fn be_uint(bytes: &[u8]) -> Option<usize> {
    if bytes.is_empty() || bytes.len() > 8 {
        return None;
    }
    let n = bytes.iter().fold(0u64, |acc, &b| (acc << 8) | u64::from(b));
    usize::try_from(n).ok()
}

/// Where an object starts, from the offset table.
fn object_offset(data: &[u8], table: usize, offset_size: usize, num_objects: usize, index: usize) -> Option<usize> {
    if index >= num_objects {
        return None;
    }
    let start = table.checked_add(index.checked_mul(offset_size)?)?;
    let off = be_uint(data.get(start..start.checked_add(offset_size)?)?)?;
    (off < data.len()).then_some(off)
}

/// Reads an object's marker and length: the low four bits, or an integer
/// object after the marker when they are 0xF. Returns the marker's high
/// nibble, the length and where the payload starts.
fn marker(data: &[u8], at: usize) -> Option<(u8, usize, usize)> {
    let m = *data.get(at)?;
    let low = usize::from(m & 0x0F);
    if low != 0x0F {
        return Some((m >> 4, low, at + 1));
    }
    let int = *data.get(at + 1)?;
    if int >> 4 != 0x1 {
        return None;
    }
    let size = 1usize << (int & 0x0F);
    if size > 8 {
        return None;
    }
    let len = be_uint(data.get(at + 2..at.checked_add(2)?.checked_add(size)?)?)?;
    Some((m >> 4, len, at + 2 + size))
}

/// Reads the string object at `at`: ASCII (0x5_) or UTF-16BE (0x6_).
fn plist_string(data: &[u8], at: usize) -> Option<String> {
    let (kind, len, start) = marker(data, at)?;
    match kind {
        0x5 => {
            let bytes = data.get(start..start.checked_add(len)?)?;
            Some(bytes.iter().map(|&b| char::from(b)).collect())
        }
        0x6 => {
            let end = start.checked_add(len.checked_mul(2)?)?;
            let units: Vec<u16> = data.get(start..end)?.as_chunks::<2>().0.iter().map(|&c| u16::from_be_bytes(c)).collect();
            String::from_utf16(&units).ok()
        }
        _ => None,
    }
}

/// Reads a binary property list whose top object is a dictionary and
/// returns the string under the `URL` key.
fn parse_binary_plist(data: &[u8]) -> Option<String> {
    let trailer = data.get(data.len().checked_sub(32)?..)?;
    let offset_size = usize::from(trailer[6]);
    let ref_size = usize::from(trailer[7]);
    let num_objects = be_uint(&trailer[8..16])?;
    let top = be_uint(&trailer[16..24])?;
    let table = be_uint(&trailer[24..32])?;
    if !(1..=8).contains(&offset_size) || !(1..=8).contains(&ref_size) {
        return None;
    }
    let object = |index: usize| object_offset(data, table, offset_size, num_objects, index);

    let (kind, count, start) = marker(data, object(top)?)?;
    if kind != 0xD {
        return None;
    }
    let refs = |i: usize| {
        let at = start.checked_add(i.checked_mul(ref_size)?)?;
        be_uint(data.get(at..at.checked_add(ref_size)?)?)
    };
    for i in 0..count {
        let key = plist_string(data, object(refs(i)?)?);
        if key.as_deref() == Some("URL") {
            let value = refs(count.checked_add(i)?)?;
            return non_empty(&plist_string(data, object(value)?)?);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `plutil -convert binary1` of a dict with `URL` = `https://example.com/a?b=1&c=2`.
    const BINARY_ASCII: &[u8] = &[
        0x62, 0x70, 0x6c, 0x69, 0x73, 0x74, 0x30, 0x30, 0xd1, 0x01, 0x02, 0x53, 0x55, 0x52, 0x4c, 0x5f, 0x10, 0x1d, 0x68, 0x74, 0x74, 0x70,
        0x73, 0x3a, 0x2f, 0x2f, 0x65, 0x78, 0x61, 0x6d, 0x70, 0x6c, 0x65, 0x2e, 0x63, 0x6f, 0x6d, 0x2f, 0x61, 0x3f, 0x62, 0x3d, 0x31, 0x26,
        0x63, 0x3d, 0x32, 0x08, 0x0b, 0x0f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2f,
    ];

    /// The same with `URL` = `https://例え.jp/ü`, which is stored as UTF-16.
    const BINARY_UTF16: &[u8] = &[
        0x62, 0x70, 0x6c, 0x69, 0x73, 0x74, 0x30, 0x30, 0xd1, 0x01, 0x02, 0x53, 0x55, 0x52, 0x4c, 0x6f, 0x10, 0x0f, 0x00, 0x68, 0x00, 0x74,
        0x00, 0x74, 0x00, 0x70, 0x00, 0x73, 0x00, 0x3a, 0x00, 0x2f, 0x00, 0x2f, 0x4f, 0x8b, 0x30, 0x48, 0x00, 0x2e, 0x00, 0x6a, 0x00, 0x70,
        0x00, 0x2f, 0x00, 0xfc, 0x08, 0x0b, 0x0f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x30,
    ];

    #[test]
    fn link_file_extensions() {
        assert!(is_link_file("url") && is_link_file("WEBLOC") && is_link_file("Url"));
        assert!(!is_link_file("txt") && !is_link_file(""));
    }

    #[test]
    fn a_url_file_with_crlf_line_ends() {
        let data = b"[InternetShortcut]\r\nURL=https://example.com/x?a=b\r\nIconIndex=0\r\n";
        assert_eq!(parse_internet_shortcut(data).as_deref(), Some("https://example.com/x?a=b"));
    }

    #[test]
    fn a_url_file_with_lowercase_keys_and_spaces() {
        let data = b"  [ internetshortcut ]\nurl = https://example.com/  \n";
        assert_eq!(parse_internet_shortcut(data).as_deref(), Some("https://example.com/"));
    }

    #[test]
    fn keys_in_other_sections_are_ignored() {
        let data = b"[Before]\nURL=https://wrong.example/\n[InternetShortcut]\nIconFile=x\nURL=https://right.example/\nURL=https://second.example/\n[After]\nURL=https://wrong2.example/\n";
        assert_eq!(parse_internet_shortcut(data).as_deref(), Some("https://right.example/"));
        assert_eq!(parse_internet_shortcut(b"[Other]\nURL=https://a.example/\n"), None);
        assert_eq!(parse_internet_shortcut(b"URL=https://a.example/\n"), None);
    }

    #[test]
    fn a_url_file_with_a_utf8_bom() {
        let mut data = vec![0xEF, 0xBB, 0xBF];
        data.extend_from_slice("[InternetShortcut]\nURL=https://example.com/ü\n".as_bytes());
        assert_eq!(parse_internet_shortcut(&data).as_deref(), Some("https://example.com/ü"));
    }

    #[test]
    fn a_url_file_in_utf16() {
        let text = "[InternetShortcut]\r\nURL=https://example.com/ü\r\n";
        let mut le = vec![0xFF, 0xFE];
        le.extend(text.encode_utf16().flat_map(|u| u.to_le_bytes()));
        let mut be = vec![0xFE, 0xFF];
        be.extend(text.encode_utf16().flat_map(|u| u.to_be_bytes()));
        assert_eq!(parse_internet_shortcut(&le).as_deref(), Some("https://example.com/ü"));
        assert_eq!(parse_internet_shortcut(&be).as_deref(), Some("https://example.com/ü"));
    }

    #[test]
    fn an_empty_address_is_none() {
        assert_eq!(parse_internet_shortcut(b"[InternetShortcut]\nURL=   \n"), None);
        assert_eq!(parse_internet_shortcut(b""), None);
    }

    #[test]
    fn an_xml_webloc_with_entities() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
	<key>Other</key>
	<string>nope</string>
	<key>URL</key>

	<string> https://example.com/?a=1&amp;b=&lt;2&gt;&quot;&apos;&#65;&#x42;&bogus; </string>
</dict></plist>"#;
        assert_eq!(parse_webloc(xml).as_deref(), Some("https://example.com/?a=1&b=<2>\"'AB&bogus;"));
        assert_eq!(parse_webloc(b"<plist><dict><key>URL</key><string></string></dict></plist>"), None);
        assert_eq!(parse_webloc(b"<plist><dict><key>URL</key><integer>1</integer></dict></plist>"), None);
        assert_eq!(parse_webloc(b"<plist><dict><key>URL</key><string>unterminated"), None);
    }

    #[test]
    fn a_binary_webloc() {
        assert_eq!(parse_webloc(BINARY_ASCII).as_deref(), Some("https://example.com/a?b=1&c=2"));
    }

    #[test]
    fn a_binary_webloc_with_a_non_ascii_address() {
        assert_eq!(parse_webloc(BINARY_UTF16).as_deref(), Some("https://例え.jp/ü"));
    }

    #[test]
    fn malformed_binary_plists_are_none() {
        assert_eq!(parse_webloc(b"bplist00"), None);
        assert_eq!(parse_webloc(b"bplist00garbage"), None);
        assert_eq!(parse_webloc(&[0xFF; 80]), None);
        for full in [BINARY_ASCII, BINARY_UTF16] {
            for cut in 0..full.len() - 1 {
                let _ = parse_webloc(&full[..cut]);
                assert_eq!(parse_webloc(&full[..cut]), None, "cut at {cut}");
            }
            // Flipping any byte must not panic.
            for i in 0..full.len() {
                let mut bad = full.to_vec();
                bad[i] ^= 0xFF;
                let _ = parse_webloc(&bad);
            }
        }
        // An absurd length in an otherwise valid file.
        let mut huge = BINARY_ASCII.to_vec();
        huge[13] = 0xFF;
        assert_eq!(parse_webloc(&huge), None);
    }

    #[test]
    fn parse_dispatches_on_the_extension() {
        assert!(parse("URL", b"[InternetShortcut]\nURL=https://a.example/\n").is_some());
        assert!(parse("webloc", BINARY_ASCII).is_some());
        assert_eq!(parse("txt", b"[InternetShortcut]\nURL=https://a.example/\n"), None);
    }

    #[test]
    fn read_checks_the_size_and_the_extension() {
        let tmp = tempfile::tempdir().unwrap();
        let small = tmp.path().join("a.URL");
        fs::write(&small, "[InternetShortcut]\nURL=https://a.example/\n").unwrap();
        assert_eq!(read(&small).as_deref(), Some("https://a.example/"));

        let mut big = b"[InternetShortcut]\nURL=https://a.example/\n;".to_vec();
        big.resize(MAX_BYTES as usize + 1, b'x');
        let path = tmp.path().join("big.url");
        fs::write(&path, &big).unwrap();
        assert_eq!(read(&path), None);
        big.truncate(MAX_BYTES as usize);
        fs::write(&path, &big).unwrap();
        assert!(read(&path).is_some());

        let other = tmp.path().join("a.txt");
        fs::write(&other, "[InternetShortcut]\nURL=https://a.example/\n").unwrap();
        assert_eq!(read(&other), None);
        assert_eq!(read(&tmp.path().join("missing.url")), None);
    }
}
