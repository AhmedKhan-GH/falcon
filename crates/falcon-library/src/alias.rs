//! Extensions Eagle accepts as another extension's name for the same bytes
//! (`.jfif` is a JPEG, `.psdt` a Photoshop template). macOS knows some of
//! them by no type, so QuickLook refuses the name even though it reads the
//! contents; falcon maps each to the extension that works and hands QuickLook
//! the file under that name.

use std::path::{Path, PathBuf};

/// The extensions that stand for another, with the one they stand for.
pub const ALIASES: [(&str, &str); 9] = [
    ("jfif", "jpg"),
    ("jpe", "jpg"),
    ("insp", "jpg"),
    ("psdt", "psd"),
    ("hif", "heic"),
    ("tif", "tiff"),
    ("ait", "ai"),
    ("indt", "indd"),
    ("m2ts", "mts"),
];

/// The extension `ext` (any case) stands for, or `ext` lower-cased when it
/// is not an alias.
pub fn canonical(ext: &str) -> String {
    let lower = ext.to_ascii_lowercase();
    ALIASES.iter().find(|(a, _)| *a == lower).map_or(lower, |(_, c)| (*c).to_string())
}

/// Whether `ext` is one of the alias names.
pub fn is_alias(ext: &str) -> bool {
    let lower = ext.to_ascii_lowercase();
    ALIASES.iter().any(|(a, _)| *a == lower)
}

/// A file under the name QuickLook needs, kept while it is alive.
pub struct Renamed {
    _dir: tempfile::TempDir,
    path: PathBuf,
}

impl Renamed {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// `path` under its canonical extension in a temporary folder (a hard link,
/// else a copy), or `None` when its extension is not an alias or the link
/// can't be made.
pub fn renamed(path: &Path) -> Option<Renamed> {
    let ext = path.extension()?.to_str()?;
    if !is_alias(ext) {
        return None;
    }
    let dir = tempfile::Builder::new().prefix("falcon-alias-").tempdir().ok()?;
    let target = dir.path().join(Path::new(path.file_stem()?).with_extension(canonical(ext)));
    std::fs::hard_link(path, &target).or_else(|_| std::fs::copy(path, &target).map(|_| ())).ok()?;
    Some(Renamed { _dir: dir, path: target })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_map_to_their_canonical_extension() {
        assert_eq!(canonical("JFIF"), "jpg");
        assert_eq!(canonical("psdt"), "psd");
        assert_eq!(canonical("m2ts"), "mts");
        assert_eq!(canonical("PNG"), "png");
        assert!(is_alias("Insp") && !is_alias("jpg"));
    }

    #[test]
    fn a_renamed_file_has_the_canonical_name_and_the_same_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("shot.psdt");
        std::fs::write(&src, b"8BPS").unwrap();
        let r = renamed(&src).unwrap();
        assert_eq!(r.path().file_name().unwrap(), "shot.psd");
        assert_eq!(std::fs::read(r.path()).unwrap(), b"8BPS");
        assert!(renamed(&tmp.path().join("a.png")).is_none());
    }
}
