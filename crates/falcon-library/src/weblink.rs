//! Reading the address out of link files: Windows `.url` shortcuts (an INI
//! file), the link format in Eagle's list. A macOS `.webloc` is a plain
//! item, as in Eagle.

use std::fs;
use std::io::Read;
use std::path::Path;

/// Link files bigger than this are not read.
const MAX_BYTES: u64 = 1 << 20;

/// Whether an extension names a link file (`url`, any case).
pub fn is_link_file(ext: &str) -> bool {
    ext.eq_ignore_ascii_case("url")
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
    if ext.eq_ignore_ascii_case("url") { parse_internet_shortcut(data) } else { None }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_file_extensions() {
        assert!(is_link_file("url") && is_link_file("URL") && is_link_file("Url"));
        assert!(!is_link_file("webloc"), "a .webloc is a plain item, as in Eagle");
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
    fn parse_dispatches_on_the_extension() {
        assert!(parse("URL", b"[InternetShortcut]\nURL=https://a.example/\n").is_some());
        assert_eq!(parse("webloc", b"[InternetShortcut]\nURL=https://a.example/\n"), None);
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
