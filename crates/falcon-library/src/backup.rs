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
        backup_at(self.root(), Local::now().naive_local())
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

/// [`Library::backup_metadata`] at a given local time.
pub(crate) fn backup_at(root: &Path, now: NaiveDateTime) -> Result<Option<PathBuf>> {
    let current = root.join(METADATA);
    let dir = root.join(BACKUP_DIR);
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
        fs::write(&target, &text).map_err(|e| Error::io(&target, e))?;
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
