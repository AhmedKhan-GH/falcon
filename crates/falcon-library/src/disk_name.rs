//! What an item's name is called on disk.
//!
//! An item's name stays in `metadata.json` exactly as it came, so a library
//! made on a Mac round-trips byte for byte, but not every such name can be a
//! Windows file: `:` would write to a hidden stream of another file, `\` and
//! a drive letter would lead out of the item's folder, and `CON` is a device.
//! [`on_disk`] turns a file name into the one Windows keeps it under, which is
//! the name the usual tools give such a file when they copy a library over.
//! Elsewhere every name is its own.

use std::borrow::Cow;

/// The name `file` is kept under on this system.
#[cfg(not(windows))]
pub fn on_disk(file: &str) -> Cow<'_, str> {
    Cow::Borrowed(file)
}

/// The name `file` is kept under on this system: `_` for what Windows forbids,
/// no trailing dots or spaces, and `_` after a device name.
#[cfg(windows)]
pub fn on_disk(file: &str) -> Cow<'_, str> {
    if !needs_change(file) {
        return Cow::Borrowed(file);
    }
    let cleaned: String = file.chars().map(|c| if forbidden(c) { '_' } else { c }).collect();
    let cleaned = cleaned.trim_end_matches(['.', ' ']);
    let base = cleaned.split('.').next().unwrap_or(cleaned);
    Cow::Owned(match (cleaned.is_empty(), is_device(base)) {
        (true, _) => "_".to_string(),
        (_, true) => format!("{base}_{}", &cleaned[base.len()..]),
        _ => cleaned.to_string(),
    })
}

#[cfg(windows)]
fn forbidden(c: char) -> bool {
    c.is_control() || "<>:\"/\\|?*".contains(c)
}

#[cfg(windows)]
fn needs_change(file: &str) -> bool {
    file.is_empty() || file.ends_with(['.', ' ']) || file.chars().any(forbidden) || is_device(file.split('.').next().unwrap_or(file))
}

/// Whether Windows reads `base`, with any extension, as a device.
#[cfg(windows)]
pub(crate) fn is_device(base: &str) -> bool {
    let upper = base.trim_end_matches(' ').to_ascii_uppercase();
    match upper.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" => true,
        _ => (upper.starts_with("COM") || upper.starts_with("LPT")) && {
            let mut rest = base.trim_end_matches(' ')[3..].chars();
            matches!((rest.next(), rest.next()), (Some('1'..='9' | '¹' | '²' | '³'), None))
        },
    }
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::on_disk;

    #[test]
    fn every_name_is_its_own() {
        for name in ["a:b.png", "CON.png", "trailing.", r"back\slash.png"] {
            assert_eq!(on_disk(name), name);
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::on_disk;

    #[test]
    fn what_windows_can_keep_is_left_alone() {
        for name in ["a b.png", "été.jpeg", ".hidden", "CONSOLE.png", "COM10.txt", "COM.txt", "a.CON", "..png"] {
            assert_eq!(on_disk(name), name);
            assert!(matches!(on_disk(name), std::borrow::Cow::Borrowed(_)));
        }
    }

    #[test]
    fn what_windows_cannot_keep_changes() {
        assert_eq!(on_disk("a:b.png"), "a_b.png");
        assert_eq!(on_disk(r"..\..\evil.png"), ".._.._evil.png");
        assert_eq!(on_disk("../evil.png"), ".._evil.png");
        assert_eq!(on_disk(r"C:\Windows\x.png"), "C__Windows_x.png");
        assert_eq!(on_disk("what?*.png"), "what__.png");
        assert_eq!(on_disk("trailing. . "), "trailing");
        assert_eq!(on_disk("..."), "_");
        assert_eq!(on_disk(""), "_");
        assert_eq!(on_disk("con.png"), "con_.png");
        assert_eq!(on_disk("NUL"), "NUL_");
        assert_eq!(on_disk("Lpt1.tar.gz"), "Lpt1_.tar.gz");
        assert_eq!(on_disk("COM¹.png"), "COM¹_.png");
    }

    /// The whole point: a Mac name never writes a stream or leaves the folder.
    #[test]
    fn names_land_as_plain_files_in_their_folder() {
        let tmp = tempfile::tempdir().unwrap();
        for name in ["a:b.png", r"..\up.png", "NUL.png", "what?.png", "dot. "] {
            std::fs::write(tmp.path().join(&*on_disk(name)), name).unwrap();
        }
        let mut names: Vec<_> = std::fs::read_dir(tmp.path()).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        names.sort();
        assert_eq!(names, [".._up.png", "NUL_.png", "a_b.png", "dot", "what_.png"]);
        assert_eq!(std::fs::read_dir(tmp.path().parent().unwrap()).unwrap().filter(|e| e.as_ref().unwrap().file_name() == "up.png").count(), 0);
    }
}
