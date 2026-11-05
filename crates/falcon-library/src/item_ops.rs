//! Item commands that work on an item's files or on per-item keys Eagle
//! writes for them: Duplicate, Replace with File, changing the extension,
//! Pin to Top inside a folder and marking a palette colour as the item's
//! primary colour.
//!
//! On disk these follow Eagle: a duplicate is a copy of the whole
//! `<ID>.info` folder under a new id whose `metadata.json` differs only in
//! `id`; a pin is `pinned: {<folder id>: <ms>}` on the item; a primary
//! colour is `marked: true` on one palette entry (Eagle writes `false`
//! when it is unmarked again). Originals are never left half written: new
//! bytes go to a temporary file in the `.info` folder first and the old
//! original is kept as `<file>.bk` until the new one is in place.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::ids;
use crate::library::{Library, write_json};
use crate::model::Item;

/// The item key holding the folders it is pinned in.
pub const PINNED_KEY: &str = "pinned";
/// The palette key marking an item's primary colour.
pub const MARKED_KEY: &str = "marked";
/// Longest extension the Change Extension dialog accepts.
pub const MAX_EXTENSION: usize = 10;

/// When the item was pinned to the top of `folder` (ms), if it is. As in
/// Eagle, a zero stamp counts as not pinned.
pub fn pinned_at(item: &Item, folder: &str) -> Option<f64> {
    let t = item.extra.get(PINNED_KEY)?.get(folder)?.as_f64()?;
    (t != 0.0).then_some(t)
}

/// Whether every item is pinned in `folder` (Eagle then offers Unpin
/// instead of Pin; it looks at the first 100 of the selection).
pub fn all_pinned<'a>(items: impl IntoIterator<Item = &'a Item>, folder: &str) -> bool {
    items.into_iter().take(100).all(|i| pinned_at(i, folder).is_some())
}

/// Moves the items pinned in `folder` to the front, the most recently
/// pinned first; the rest keep their order. Eagle does this inside a folder
/// unless the folder is sorted at random.
pub fn pinned_first(order: &mut [usize], items: &[Item], folder: &str) {
    order.sort_by(|&a, &b| match (pinned_at(&items[a], folder), pinned_at(&items[b], folder)) {
        (Some(x), Some(y)) => y.total_cmp(&x),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
}

/// Why an extension typed into the Change Extension dialog is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionError {
    Empty,
    /// Only ASCII letters and digits are allowed.
    BadCharacter,
    TooLong,
}

impl std::fmt::Display for ExtensionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ExtensionError::Empty => "Type an extension",
            ExtensionError::BadCharacter => "Use only letters and digits",
            ExtensionError::TooLong => "At most 10 characters",
        })
    }
}

/// Checks an extension as Eagle's dialog does (letters and digits, at
/// most [`MAX_EXTENSION`]) and returns it in lower case, as stored.
pub fn check_extension(text: &str) -> std::result::Result<String, ExtensionError> {
    let t = text.trim();
    if t.is_empty() {
        Err(ExtensionError::Empty)
    } else if !t.chars().all(|c| c.is_ascii_alphanumeric()) {
        Err(ExtensionError::BadCharacter)
    } else if t.len() > MAX_EXTENSION {
        Err(ExtensionError::TooLong)
    } else {
        Ok(t.to_ascii_lowercase())
    }
}

/// The original's file name with another extension.
fn file_name(name: &str, ext: &str) -> String {
    let file = if ext.is_empty() { name.to_string() } else { format!("{name}.{ext}") };
    crate::disk_name::on_disk(&file).into_owned()
}

/// `path` with `suffix` added to its file name.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// Copies `src` to `dest` and flushes it to disk.
fn copy_synced(src: &Path, dest: &Path) -> Result<()> {
    fs::copy(src, dest).map_err(|e| Error::io(dest, e))?;
    crate::disk_name::make_writable(dest).map_err(|e| Error::io(dest, e))?;
    // Windows only flushes a file opened for writing.
    fs::OpenOptions::new().read(true).write(cfg!(windows)).open(dest).and_then(|f| f.sync_all()).map_err(|e| Error::io(dest, e))
}

/// Puts `staged` (a finished file in the same folder) where `old` was,
/// under the name `new`: `old` becomes `old.bk`, `staged` is renamed to
/// `new`, then the backup goes. If the rename fails the original comes
/// back and `staged` is removed.
pub(crate) fn swap_in(staged: &Path, old: &Path, new: &Path) -> Result<()> {
    let bk = with_suffix(old, ".bk");
    if let Err(e) = fs::rename(old, &bk) {
        let _ = fs::remove_file(staged);
        return Err(Error::io(old, e));
    }
    if let Err(e) = fs::rename(staged, new) {
        let _ = fs::rename(&bk, old);
        let _ = fs::remove_file(staged);
        return Err(Error::io(new, e));
    }
    let _ = fs::remove_file(&bk);
    Ok(())
}

impl Library {
    /// Eagle's Duplicate (⌘D): copies the item's whole `.info` folder to a
    /// new id and writes its `metadata.json` with only the `id` changed, so
    /// the copy keeps the name, tags, folders, dates and everything else.
    /// Returns the new id.
    pub fn duplicate_item(&mut self, id: &str) -> Result<String> {
        let item = self.item(id).cloned().ok_or_else(|| self.invalid(format!("no item {id}")))?;
        let from = item.info_dir(self.root());
        let mut new_id = ids::new_id();
        while self.root().join("images").join(format!("{new_id}.info")).exists() || self.item(&new_id).is_some() {
            new_id = ids::new_id();
        }
        let mut copy = item;
        copy.id = new_id.clone();
        let to = copy.info_dir(self.root());
        let made = (|| -> Result<()> {
            copy_dir(&from, &to)?;
            // Written at once even in batched mode, so the folder never
            // holds the original's metadata under the new name.
            write_json(&to.join("metadata.json"), &copy)?;
            self.register_item(copy)
        })();
        match made {
            Ok(()) => Ok(new_id),
            Err(e) => {
                let _ = fs::remove_dir_all(&to);
                Err(e)
            }
        }
    }

    /// Pins the items to the top of `folders` (Eagle's Pin to Top), or
    /// unpins them. Only the folders each item is in are touched. A pin is
    /// stamped `now - n` for the `n`th item, so they keep their order at
    /// the top; unpinning the last folder removes the `pinned` key.
    /// Returns how many items changed.
    pub fn set_pinned(&mut self, ids: &[String], folders: &[String], pin: bool, now: u64) -> Result<usize> {
        let mut changed = 0;
        for (n, id) in ids.iter().enumerate() {
            let Some(mut item) = self.item(id).cloned() else { continue };
            let targets: Vec<&String> = folders.iter().filter(|f| item.folders.contains(f)).collect();
            if targets.is_empty() {
                continue;
            }
            let mut map = match item.extra.get(PINNED_KEY) {
                Some(serde_json::Value::Object(m)) => m.clone(),
                _ => serde_json::Map::new(),
            };
            let before = map.clone();
            for f in targets {
                if pin {
                    map.insert(f.clone(), now.saturating_sub(n as u64).into());
                } else {
                    map.remove(f.as_str());
                }
            }
            if map == before {
                continue;
            }
            if map.is_empty() {
                item.extra.shift_remove(PINNED_KEY);
            } else {
                item.extra.insert(PINNED_KEY.into(), serde_json::Value::Object(map));
            }
            self.update_item(item)?;
            changed += 1;
        }
        Ok(changed)
    }

    /// Marks or unmarks palette entry `index` as the item's primary colour
    /// (Eagle's "Set as Primary Color"). Like Eagle, unmarking writes
    /// `marked: false` rather than dropping the key.
    pub fn set_palette_marked(&mut self, id: &str, index: usize, marked: bool) -> Result<()> {
        let mut item = self.item(id).cloned().ok_or_else(|| self.invalid(format!("no item {id}")))?;
        let Some(p) = item.palettes.get_mut(index) else { return Err(self.invalid("no such palette colour")) };
        p.extra.insert(MARKED_KEY.into(), marked.into());
        self.update_item(item)
    }

    /// Eagle's Change Extension: renames the original to the new extension
    /// (checked with [`check_extension`]) and stores it. The file's bytes,
    /// the thumbnail and everything else stay. Refused when a file of that
    /// name is already in the `.info` folder.
    pub fn set_extension(&mut self, id: &str, ext: &str) -> Result<()> {
        let ext = check_extension(ext).map_err(|e| self.invalid(e.to_string()))?;
        let mut item = self.item(id).cloned().ok_or_else(|| self.invalid(format!("no item {id}")))?;
        if ext == item.ext {
            return Ok(());
        }
        let old = item.original_path(self.root());
        let new = item.info_dir(self.root()).join(file_name(&item.name, &ext));
        if new.exists() && !same_file(&old, &new) {
            return Err(self.invalid(format!("{} already exists", new.display())));
        }
        fs::rename(&old, &new).map_err(|e| Error::io(&old, e))?;
        item.ext = ext;
        if let Err(e) = self.update_item(item) {
            let _ = fs::rename(&new, &old);
            return Err(e);
        }
        if let Some(j) = &mut self.journal {
            j.irreversible = true;
        }
        Ok(())
    }

    /// Eagle's Replace with File: the item keeps its id, name and
    /// everything it was given, but its original becomes a copy of `src`
    /// with `src`'s extension (lower case, `unknown` when it has none).
    /// Size, dates and dimensions are read again and the thumbnail and
    /// palette redrawn, except that a custom thumbnail is kept.
    pub fn replace_original(&mut self, id: &str, src: &Path) -> Result<()> {
        let mut item = self.item(id).cloned().ok_or_else(|| self.invalid(format!("no item {id}")))?;
        if !src.is_file() {
            return Err(Error::Import { path: src.to_path_buf(), reason: "not a file".into() });
        }
        let ext = src.extension().map(|e| e.to_string_lossy().to_lowercase()).filter(|e| !e.is_empty()).unwrap_or_else(|| "unknown".into());
        let dir = item.info_dir(self.root());
        let old = item.original_path(self.root());
        let new = dir.join(file_name(&item.name, &ext));
        if new != old && new.exists() {
            return Err(self.invalid(format!("{} already exists", new.display())));
        }
        let staged = dir.join(format!(".{}.falcon-replace", file_name(&item.name, &ext)));
        if let Err(e) = copy_synced(src, &staged) {
            let _ = fs::remove_file(&staged);
            return Err(e);
        }
        swap_in(&staged, &old, &new)?;
        if let Some(j) = &mut self.journal {
            j.irreversible = true;
        }
        item.ext = ext;
        if crate::thumbnail::has_custom_thumbnail(&item) {
            crate::import::restat(&mut item, &new)?;
            return self.update_item(item);
        }
        match self.redraw_item(item.clone()) {
            Ok(()) => Ok(()),
            // A format falcon can't draw still gets its new extension and
            // file facts; the old thumbnail stays, as in Eagle.
            Err(_) => {
                crate::import::restat(&mut item, &new)?;
                self.update_item(item)
            }
        }
    }
}

/// Whether two paths name the same file (a case-only rename on a
/// case-insensitive disk).
fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// Copies a folder's files and subfolders, except `metadata.json` at its
/// top, keeping file times.
fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    fn go(from: &Path, to: &Path, top: bool) -> Result<()> {
        fs::create_dir_all(to).map_err(|e| Error::io(to, e))?;
        for entry in fs::read_dir(from).map_err(|e| Error::io(from, e))? {
            let entry = entry.map_err(|e| Error::io(from, e))?;
            let name = entry.file_name();
            if top && name == "metadata.json" {
                continue;
            }
            let (src, dest) = (entry.path(), to.join(&name));
            if entry.file_type().map_err(|e| Error::io(&src, e))?.is_dir() {
                go(&src, &dest, false)?;
            } else {
                fs::copy(&src, &dest).map_err(|e| Error::io(&dest, e))?;
                if let Ok(m) = fs::metadata(&src) {
                    let _ = filetime::set_file_times(&dest, filetime::FileTime::from_last_access_time(&m), filetime::FileTime::from_last_modification_time(&m));
                }
            }
        }
        Ok(())
    }
    if to.exists() {
        return Err(Error::Import { path: to.to_path_buf(), reason: "already exists".into() });
    }
    go(from, to, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, pinned: Option<serde_json::Value>) -> Item {
        let mut v = serde_json::json!({"id": id, "name": id, "size": 1, "btime": 1, "mtime": 1, "ext": "png", "tags": [], "folders": ["F"], "isDeleted": false, "url": "", "annotation": "", "modificationTime": 1, "height": 1, "width": 1, "lastModified": 1, "palettes": []});
        if let Some(p) = pinned {
            v["pinned"] = p;
        }
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn pinned_items_come_first_newest_pin_first() {
        let items = vec![
            item("a", None),
            item("b", Some(serde_json::json!({"F": 100}))),
            item("c", None),
            item("d", Some(serde_json::json!({"F": 300, "G": 1}))),
            item("e", Some(serde_json::json!({"G": 500}))),
            item("f", Some(serde_json::json!({"F": 0}))),
        ];
        let mut order: Vec<usize> = (0..items.len()).collect();
        pinned_first(&mut order, &items, "F");
        let ids: Vec<&str> = order.iter().map(|&i| items[i].id.as_str()).collect();
        assert_eq!(ids, ["d", "b", "a", "c", "e", "f"]);
        assert!(all_pinned([&items[1], &items[3]], "F"));
        assert!(!all_pinned([&items[1], &items[4]], "F"));
        assert_eq!(pinned_at(&items[5], "F"), None, "a zero stamp is not a pin");
    }

    #[test]
    fn extensions_are_checked_like_eagle() {
        assert_eq!(check_extension("JPG"), Ok("jpg".into()));
        assert_eq!(check_extension(" mp4 "), Ok("mp4".into()));
        assert_eq!(check_extension(""), Err(ExtensionError::Empty));
        assert_eq!(check_extension("tar.gz"), Err(ExtensionError::BadCharacter));
        assert_eq!(check_extension("é"), Err(ExtensionError::BadCharacter));
        assert_eq!(check_extension("abcdefghijk"), Err(ExtensionError::TooLong));
        assert_eq!(check_extension("abcdefghij"), Ok("abcdefghij".into()));
    }
}
