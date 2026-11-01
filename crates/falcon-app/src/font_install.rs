//! Activating fonts. Eagle copies the original to
//! `~/Library/Fonts/EagleApp/<postScriptName>.<ext>`, where macOS picks it
//! up for every app, and deleting that copy deactivates it. Eagle decides
//! a font is active from that folder's file names (`<base>_<.ext>` against
//! `<postScriptName>_.<ext>`), which `search::installed_font_keys` reads.
//!
//! falcon does the same in a folder of its own, `~/Library/Fonts/Falcon/`
//! (`search::falcon_font_dir`), with the same naming, and never adds to or
//! removes from `EagleApp/`, which the user's Eagle manages. A font counts
//! as active when it is in either folder ([`activation`]); only a copy in
//! falcon's folder can be deactivated from falcon.
//!
//! Every function takes the folders, so tests use temp dirs; the app passes
//! `search::falcon_font_dir()` and `search::eagle_font_dir()`, and writes
//! only on the user's confirmed request.

use std::io;
use std::path::{Path, PathBuf};

use falcon_library::model::Item;
use falcon_library::search;

/// The file name Eagle gives a font's activated copy: its PostScript name
/// (the first language of `fontMetas.postScriptName`, as Eagle reads it)
/// and the item's extension. `None` without a PostScript name, or when the
/// name could reach outside the folder.
pub fn file_name(item: &Item) -> Option<String> {
    let ps = search::font_name(item, "postScriptName")?;
    if ps.is_empty() || ps.starts_with('.') || ps.contains(['/', '\\', '\0', ':']) {
        return None;
    }
    Some(format!("{ps}.{}", item.ext))
}

/// Whether the font is active: its key is among the folder's files.
pub fn is_active(dir: &Path, item: &Item) -> bool {
    search::font_key(item).is_some_and(|k| search::installed_font_keys(dir).contains(&k))
}

/// Where a font is activated, as the font viewer shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activation {
    /// In neither folder: Activate copies it into falcon's.
    Off,
    /// In falcon's folder (perhaps in Eagle's too): Deactivate removes
    /// falcon's copy.
    Falcon,
    /// Only in Eagle's folder: activated by Eagle, and left to Eagle.
    Eagle,
}

/// Where `item` is activated, given falcon's folder and Eagle's (if any).
pub fn activation(falcon_dir: &Path, eagle_dir: Option<&Path>, item: &Item) -> Activation {
    if is_active(falcon_dir, item) {
        Activation::Falcon
    } else if eagle_dir.is_some_and(|d| is_active(d, item)) {
        Activation::Eagle
    } else {
        Activation::Off
    }
}

/// Refuses Eagle's folder, which falcon must never change, even if it
/// were passed by mistake.
fn guard(dir: &Path) -> io::Result<()> {
    if dir.file_name().is_some_and(|n| n == "EagleApp") {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "falcon doesn't change Eagle's font folder"));
    }
    Ok(())
}

/// Copies the original (`src`) into `dir` (falcon's folder) under
/// [`file_name`], creating the folder; a copy already there is kept, as in
/// Eagle. Returns the path.
pub fn activate(dir: &Path, item: &Item, src: &Path) -> io::Result<PathBuf> {
    guard(dir)?;
    let name = file_name(item).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the font has no usable PostScript name"))?;
    std::fs::create_dir_all(dir)?;
    let dest = dir.join(name);
    if !dest.exists() {
        // Copy beside the target, then move it in, so macOS never sees half a font.
        let tmp = dir.join(format!(".{}.falcon-tmp", dest.file_name().unwrap_or_default().to_string_lossy()));
        std::fs::copy(src, &tmp)?;
        std::fs::rename(&tmp, &dest)?;
    }
    Ok(dest)
}

/// Removes the font's copy from `dir` (falcon's folder), if there is one.
pub fn deactivate(dir: &Path, item: &Item) -> io::Result<()> {
    guard(dir)?;
    let Some(name) = file_name(item) else { return Ok(()) };
    match std::fs::remove_file(dir.join(name)) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font(ps: &str, ext: &str) -> Item {
        let mut item: Item = serde_json::from_value(serde_json::json!({
            "id": "F1", "name": "My Font", "size": 4, "btime": 0, "mtime": 0, "ext": ext, "tags": [], "folders": [],
            "isDeleted": false, "url": "", "annotation": "", "modificationTime": 0, "height": 0, "width": 0, "lastModified": 0,
            "palettes": [],
        }))
        .unwrap();
        item.extra.insert("fontMetas".into(), serde_json::json!({ "postScriptName": { "en": ps } }));
        item
    }

    #[test]
    fn names_follow_eagle() {
        assert_eq!(file_name(&font("Inter-Bold", "otf")).as_deref(), Some("Inter-Bold.otf"));
        assert_eq!(file_name(&font("../evil", "ttf")), None);
        assert_eq!(file_name(&font(".hidden", "ttf")), None);
        assert_eq!(file_name(&font("", "ttf")), None);
    }

    #[test]
    fn activate_and_deactivate_in_a_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("Fonts/Falcon");
        let src = tmp.path().join("My Font.otf");
        std::fs::write(&src, b"font bytes").unwrap();
        let item = font("Inter-Bold", "otf");
        assert!(!is_active(&dir, &item), "a missing folder means nothing is active");
        let dest = activate(&dir, &item, &src).unwrap();
        assert_eq!(dest, dir.join("Inter-Bold.otf"));
        assert_eq!(std::fs::read(&dest).unwrap(), b"font bytes");
        assert!(is_active(&dir, &item));
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names.len(), 1, "no temp file left behind");
        // Activating again keeps the copy that is there.
        std::fs::write(&src, b"changed").unwrap();
        activate(&dir, &item, &src).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"font bytes");
        deactivate(&dir, &item).unwrap();
        assert!(!dest.exists() && !is_active(&dir, &item));
        deactivate(&dir, &item).unwrap();
    }

    #[test]
    fn eagles_folder_counts_but_is_never_changed() {
        let tmp = tempfile::tempdir().unwrap();
        let (falcon, eagle) = (tmp.path().join("Fonts/Falcon"), tmp.path().join("Fonts/EagleApp"));
        std::fs::create_dir_all(&eagle).unwrap();
        let src = tmp.path().join("My Font.otf");
        std::fs::write(&src, b"font bytes").unwrap();
        let item = font("Inter-Bold", "otf");
        assert_eq!(activation(&falcon, Some(&eagle), &item), Activation::Off);
        // Activated by Eagle only: counts, but falcon leaves it alone.
        std::fs::write(eagle.join("Inter-Bold.otf"), b"eagle's copy").unwrap();
        assert_eq!(activation(&falcon, Some(&eagle), &item), Activation::Eagle);
        assert_eq!(activation(&falcon, None, &item), Activation::Off, "no Eagle folder on this system");
        deactivate(&falcon, &item).unwrap();
        assert_eq!(std::fs::read(eagle.join("Inter-Bold.otf")).unwrap(), b"eagle's copy");
        assert_eq!(deactivate(&eagle, &item).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(activate(&eagle, &item, &src).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert!(eagle.join("Inter-Bold.otf").exists());
        // Activating from falcon writes only falcon's folder.
        activate(&falcon, &item, &src).unwrap();
        assert_eq!(activation(&falcon, Some(&eagle), &item), Activation::Falcon);
        assert_eq!(std::fs::read_dir(&eagle).unwrap().count(), 1, "nothing added to Eagle's folder");
        assert_eq!(std::fs::read(eagle.join("Inter-Bold.otf")).unwrap(), b"eagle's copy");
        // Deactivating removes falcon's copy; Eagle's still counts.
        deactivate(&falcon, &item).unwrap();
        assert!(!falcon.join("Inter-Bold.otf").exists());
        assert_eq!(activation(&falcon, Some(&eagle), &item), Activation::Eagle);
    }
}
