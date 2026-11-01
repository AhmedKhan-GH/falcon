//! Activating fonts the way Eagle does on macOS: the original is copied to
//! `~/Library/Fonts/EagleApp/<postScriptName>.<ext>`, where macOS picks it
//! up for every app, and deleting that copy deactivates it. Eagle decides
//! a font is active from that folder's file names (`<base>_<.ext>` against
//! `<postScriptName>_.<ext>`), which `search::installed_font_keys` and the
//! Font filter read too.
//!
//! Every function takes the folder, so tests use a temp dir; the app passes
//! `search::eagle_font_dir()` and only on the user's confirmed request.

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

/// Copies the original (`src`) into `dir` under [`file_name`], creating
/// the folder; a copy already there is kept, as in Eagle. Returns the path.
pub fn activate(dir: &Path, item: &Item, src: &Path) -> io::Result<PathBuf> {
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

/// Removes the font's copy from `dir`, if there is one.
pub fn deactivate(dir: &Path, item: &Item) -> io::Result<()> {
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
        let dir = tmp.path().join("Fonts/EagleApp");
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
}
