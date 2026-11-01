//! Snapshots of the library `metadata.json` in `backup/`, kept the way Eagle
//! keeps them, and recovery from them when `metadata.json` is damaged.
//!
//! Eagle copies `metadata.json` to `backup/backup-YYYY-MM-DD HH.mm.ss.SSS.json`
//! (local time) when the library changed and every 30 minutes. It skips the
//! copy when the newest backup has the same size, refuses to back up a file
//! that doesn't parse or lacks `applicationVersion`, and keeps at most 100
//! backups. On open, a `metadata.json` that fails to parse is replaced by the
//! newest backup that parses and has a `modificationTime`; the damaged file
//! is kept as `backup/metadata.json.damaged-<ms>`.
//!
//! Eagle also keeps one more copy outside the library, in its app data:
//! `eagle-backup/<hash>.json`, where `<hash>` is the 32-bit FNV-1a hash of
//! the library path (no trailing slash) over its UTF-16 code units, as 8
//! lowercase hex digits ([`app_data_backup_name`]). It replaces that copy
//! whenever it writes a new backup into an existing `backup/` list (not
//! for the very first one); falcon replaces it with every new backup.
//! Nothing is written for a library whose folder is gone, so a backup
//! never recreates a moved or deleted library.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{Local, NaiveDateTime};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::library::Library;
use crate::model::LibraryMetadata;

/// Most backups Eagle keeps; older ones are deleted.
pub const MAX_BACKUP_COUNT: usize = 100;
const BACKUP_DIR: &str = "backup";
const METADATA: &str = "metadata.json";
/// Eagle only counts files whose name contains this.
const MARKER: &str = "backup-";

/// The file name Eagle gives a backup taken at `time` (local time), e.g.
/// `backup-2026-10-26 02.15.25.310.json`.
pub fn backup_name(time: NaiveDateTime) -> String {
    format!("{MARKER}{}.json", time.format("%Y-%m-%d %H.%M.%S%.3f"))
}

impl Library {
    /// Copies `metadata.json` into `backup/` as Eagle does and returns the
    /// new backup's path. Returns `None` without writing when the file on
    /// disk is damaged (doesn't parse or has no `applicationVersion`) or the
    /// newest backup has the same size. Backups beyond the newest
    /// [`MAX_BACKUP_COUNT`] are deleted.
    pub fn backup_metadata(&self) -> Result<Option<PathBuf>> {
        backup_library(self.root())
    }

    /// Backup files in `backup/`, newest first (by name, as Eagle sorts).
    pub fn backups(&self) -> Result<Vec<PathBuf>> {
        list(&self.root().join(BACKUP_DIR))
    }

    /// Replaces `metadata.json` with the backup at `path` and reloads the
    /// folder tree from it. The current file is first kept as
    /// `backup/metadata.json.damaged-<ms>`. Fails, changing nothing, if the
    /// backup doesn't parse as a library `metadata.json`.
    pub fn restore_metadata_from_backup(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let metadata = parse_backup(path).ok_or_else(|| Error::Import { path: path.to_path_buf(), reason: "not a usable metadata.json backup".into() })?;
        replace_metadata(self.root(), path)?;
        self.metadata = metadata;
        // The file on disk is now the backup: nothing pending, and undo
        // starts from here.
        self.committed.metadata = self.metadata.clone();
        self.dirty.metadata = false;
        self.disk_stamp = self.metadata.modification_time;
        Ok(())
    }

    /// Opens the library like [`Library::open`], but if `metadata.json`
    /// can't be read or parsed, restores it from the newest usable backup
    /// first, as Eagle does on launch. Returns the backup used, if any.
    pub fn open_or_restore(path: impl AsRef<Path>) -> Result<(Library, Option<PathBuf>)> {
        let root = path.as_ref();
        let metadata = root.join(METADATA);
        match Library::open(root) {
            Ok(lib) => Ok((lib, None)),
            Err(Error::Json { path, .. } | Error::Io { path, .. }) if path == metadata => match restore_newest(root)? {
                Some(used) => Ok((Library::open(root)?, Some(used))),
                None => Library::open(root).map(|lib| (lib, None)),
            },
            Err(e) => Err(e),
        }
    }
}

/// The name of the app-data copy of the library at `root`:
/// `<fnv1a-32 of the path>.json`, as Eagle names it.
pub fn app_data_backup_name(root: &Path) -> String {
    let path = root.to_string_lossy();
    let path = path.trim_end_matches(['/', '\\']);
    format!("{}.json", fnv1a_hex(path))
}

/// 32-bit FNV-1a over the UTF-16 code units of `s` (as JavaScript strings
/// index them), as 8 lowercase hex digits.
fn fnv1a_hex(s: &str) -> String {
    let mut h: u32 = 0x811c_9dc5;
    for unit in s.encode_utf16() {
        h ^= u32::from(unit);
        h = h.wrapping_mul(0x0100_0193);
    }
    format!("{h:08x}")
}

/// [`backup_library`], also replacing the library's copy in `app_backups`
/// (the app's `eagle-backup` folder, created if needed) whenever a new
/// backup is written. A failure there is ignored, as Eagle ignores it.
pub fn backup_library_with(root: &Path, app_backups: Option<&Path>) -> Result<Option<PathBuf>> {
    let made = backup_library(root)?;
    if let (Some(made), Some(dir)) = (&made, app_backups) {
        let _ = fs::create_dir_all(dir).and_then(|()| write_atomic(&dir.join(app_data_backup_name(root)), &fs::read(made)?));
    }
    Ok(made)
}

/// Writes `bytes` to `path` through a temporary file beside it and a
/// rename, so a reader never sees half a file.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    std::io::Write::write_all(&mut tmp, bytes)?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

/// [`Library::backup_metadata`] for the library at `root`, without opening
/// it. It only reads `metadata.json` and writes into `backup/`, so the app
/// runs it on a background thread while the library stays in use: a
/// `metadata.json` caught mid-write doesn't parse and is skipped.
pub fn backup_library(root: &Path) -> Result<Option<PathBuf>> {
    backup_at(root, Local::now().naive_local())
}

/// [`Library::backup_metadata`] at a given local time.
pub(crate) fn backup_at(root: &Path, now: NaiveDateTime) -> Result<Option<PathBuf>> {
    let current = root.join(METADATA);
    let dir = root.join(BACKUP_DIR);
    if !current.is_file() {
        // The library moved or was deleted: never recreate its folder.
        return Ok(None);
    }
    fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;

    let text = fs::read(&current).map_err(|e| Error::io(&current, e))?;
    let healthy = serde_json::from_slice::<Value>(&text).ok().is_some_and(|v| truthy(v.get("applicationVersion")));
    if !healthy {
        return Ok(None);
    }

    let existing = list(&dir)?;
    let target = dir.join(backup_name(now));
    let changed = match existing.first() {
        None => true,
        Some(newest) => fs::metadata(newest).map(|m| m.len()).ok() != Some(text.len() as u64),
    };
    if changed {
        write_atomic(&target, &text).map_err(|e| Error::io(&target, e))?;
    }
    // Eagle trims the list it read before copying, so with the new copy at
    // most MAX_BACKUP_COUNT remain.
    for old in existing.iter().skip(MAX_BACKUP_COUNT - 1) {
        let _ = fs::remove_file(old);
    }
    Ok(changed.then_some(target))
}

fn list(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| Error::io(dir, e))? {
        let entry = entry.map_err(|e| Error::io(dir, e))?;
        if entry.file_name().to_string_lossy().contains(MARKER) && entry.path().is_file() {
            out.push(entry.path());
        }
    }
    out.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    Ok(out)
}

/// Restores `metadata.json` from the newest usable backup; returns it.
fn restore_newest(root: &Path) -> Result<Option<PathBuf>> {
    for candidate in list(&root.join(BACKUP_DIR))? {
        if fs::metadata(&candidate).is_ok_and(|m| m.len() > 0) && parse_backup(&candidate).is_some() {
            replace_metadata(root, &candidate)?;
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

/// A backup Eagle would accept (parses, has a `modificationTime`) that
/// falcon can also load.
fn parse_backup(path: &Path) -> Option<LibraryMetadata> {
    let bytes = fs::read(path).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    if !truthy(value.get("modificationTime")) {
        return None;
    }
    serde_json::from_value(value).ok()
}

fn replace_metadata(root: &Path, backup: &Path) -> Result<()> {
    let current = root.join(METADATA);
    if current.exists() {
        let dir = root.join(BACKUP_DIR);
        fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        let ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
        let damaged = dir.join(format!("{METADATA}.damaged-{ms}"));
        let _ = fs::copy(&current, damaged);
    }
    fs::copy(backup, &current).map_err(|e| Error::io(&current, e))?;
    Ok(())
}

/// JavaScript truthiness for the values Eagle checks.
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(h: u32, m: u32, s: u32, ms: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, 26).unwrap().and_hms_milli_opt(h, m, s, ms).unwrap()
    }

    fn temp_library() -> (tempfile::TempDir, Library) {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::create(dir.path().join("Test.library")).unwrap();
        (dir, lib)
    }

    #[test]
    fn app_data_name_is_eagles_fnv_hash() {
        // FNV-1a 32 reference values.
        assert_eq!(fnv1a_hex(""), "811c9dc5");
        assert_eq!(fnv1a_hex("a"), "e40c292c");
        assert_eq!(fnv1a_hex("foobar"), "bf9cf968");
        // Code units, not bytes: one unit per BMP character, two for others.
        assert_ne!(fnv1a_hex("é"), fnv1a_hex("\u{c3}\u{a9}"));
        let p = Path::new("/Users/me/Falcon Sandbox/Books.library");
        assert_eq!(app_data_backup_name(p), format!("{}.json", fnv1a_hex("/Users/me/Falcon Sandbox/Books.library")));
        assert_eq!(app_data_backup_name(Path::new("/Users/me/Falcon Sandbox/Books.library/")), app_data_backup_name(p));
    }

    #[test]
    fn app_data_copy_follows_each_new_backup() {
        let (d, mut lib) = temp_library();
        let app = d.path().join("app/eagle-backup");
        let copy = app.join(app_data_backup_name(lib.root()));
        backup_library_with(lib.root(), Some(&app)).unwrap().unwrap();
        assert_eq!(fs::read(&copy).unwrap(), fs::read(lib.root().join(METADATA)).unwrap());
        lib.create_folder(None, "Changed").unwrap();
        backup_library_with(lib.root(), Some(&app)).unwrap().unwrap();
        assert_eq!(fs::read(&copy).unwrap(), fs::read(lib.root().join(METADATA)).unwrap());
        assert_eq!(fs::read_dir(&app).unwrap().count(), 1, "one copy per library, no temp files left");
    }

    #[test]
    fn never_recreates_a_missing_library() {
        let (d, lib) = temp_library();
        let root = lib.root().to_path_buf();
        drop(lib);
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(backup_library_with(&root, Some(&d.path().join("app"))).unwrap(), None);
        assert!(!root.exists());
    }

    #[test]
    fn name_matches_eagle() {
        assert_eq!(backup_name(at(2, 15, 25, 310)), "backup-2026-10-26 02.15.25.310.json");
        assert_eq!(backup_name(at(14, 5, 7, 9)), "backup-2026-10-26 14.05.07.009.json");
    }

    #[test]
    fn skips_when_newest_has_same_size() {
        let (_d, mut lib) = temp_library();
        let first = backup_at(lib.root(), at(1, 0, 0, 0)).unwrap().unwrap();
        assert_eq!(fs::read(&first).unwrap(), fs::read(lib.root().join(METADATA)).unwrap());
        assert_eq!(backup_at(lib.root(), at(1, 0, 1, 0)).unwrap(), None);
        lib.create_folder(None, "Longer metadata now").unwrap();
        let second = backup_at(lib.root(), at(1, 0, 2, 0)).unwrap().unwrap();
        assert_eq!(lib.backups().unwrap(), vec![second, first]);
    }

    #[test]
    fn refuses_damaged_metadata() {
        let (_d, lib) = temp_library();
        fs::write(lib.root().join(METADATA), b"{\"folders\":[]}").unwrap();
        assert_eq!(backup_at(lib.root(), at(1, 0, 0, 0)).unwrap(), None);
        fs::write(lib.root().join(METADATA), b"{not json").unwrap();
        assert_eq!(backup_at(lib.root(), at(1, 0, 0, 0)).unwrap(), None);
        assert!(lib.backups().unwrap().is_empty());
    }

    #[test]
    fn keeps_at_most_max_count() {
        let (_d, lib) = temp_library();
        let dir = lib.root().join(BACKUP_DIR);
        fs::create_dir_all(&dir).unwrap();
        for i in 0..120 {
            fs::write(dir.join(backup_name(at(0, i / 60, i % 60, 0))), b"x").unwrap();
        }
        fs::write(dir.join("unrelated.txt"), b"keep").unwrap();
        let new = backup_at(lib.root(), at(5, 0, 0, 0)).unwrap().unwrap();
        let left = lib.backups().unwrap();
        assert_eq!(left.len(), MAX_BACKUP_COUNT);
        assert_eq!(left[0], new);
        // The newest 99 old ones survive.
        assert_eq!(left[1], dir.join(backup_name(at(0, 1, 59, 0))));
        assert_eq!(left[99], dir.join(backup_name(at(0, 0, 21, 0))));
        assert!(dir.join("unrelated.txt").exists());
    }

    #[test]
    fn restores_from_chosen_backup() {
        let (_d, mut lib) = temp_library();
        let before = backup_at(lib.root(), at(1, 0, 0, 0)).unwrap().unwrap();
        lib.create_folder(None, "Later").unwrap();
        assert_eq!(lib.metadata.folders.len(), 1);
        lib.restore_metadata_from_backup(&before).unwrap();
        assert!(lib.metadata.folders.is_empty());
        assert_eq!(fs::read(lib.root().join(METADATA)).unwrap(), fs::read(&before).unwrap());
        let damaged = fs::read_dir(lib.root().join(BACKUP_DIR))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("metadata.json.damaged-"))
            .count();
        assert_eq!(damaged, 1);
        let bad = lib.root().join(BACKUP_DIR).join("backup-bad.json");
        fs::write(&bad, b"[]").unwrap();
        assert!(lib.restore_metadata_from_backup(&bad).is_err());
    }

    #[test]
    fn open_or_restore_recovers_corrupt_metadata() {
        let (_d, mut lib) = temp_library();
        lib.create_folder(None, "Kept").unwrap();
        let good = backup_at(lib.root(), at(1, 0, 0, 0)).unwrap().unwrap();
        // A newer but unusable backup is passed over.
        let dir = lib.root().join(BACKUP_DIR);
        fs::write(dir.join(backup_name(at(2, 0, 0, 0))), b"{\"folders\":").unwrap();
        fs::write(dir.join(backup_name(at(3, 0, 0, 0))), b"").unwrap();
        fs::write(lib.root().join(METADATA), b"{\"folders\":[").unwrap();
        assert!(Library::open(lib.root()).is_err());

        let (reopened, used) = Library::open_or_restore(lib.root()).unwrap();
        assert_eq!(used, Some(good));
        assert_eq!(reopened.metadata.folders[0].name, "Kept");

        // A healthy library opens with no restore.
        let (_, used) = Library::open_or_restore(lib.root()).unwrap();
        assert_eq!(used, None);
    }
}
