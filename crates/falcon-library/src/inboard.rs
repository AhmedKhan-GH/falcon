//! Reading an Inboard backup folder for import, as Eagle's File ▸ Import ▸
//! Inboard does.
//!
//! The folder holds `InboardBackup.json` with `folders` (each with a
//! `title`) and `items`, plus the files in `images/` and `screenshots/`.
//! Eagle creates one top-level folder per Inboard folder (all of them, in
//! order; when two share a title, items go to the later one), then imports
//! every item of `type` `"image"` whose file exists: the file is the last
//! path component of the item's `path`, looked up in `screenshots/` when
//! that path runs through a `/screenshots/` folder and in `images/`
//! otherwise. The item keeps Inboard's `title` as its name, its `tags`,
//! its `url`, its trashed state (`isTrashed` other than 0) and its `date`
//! as the date added.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::error::{Error, Result};

/// The backup's index file.
pub const INDEX: &str = "InboardBackup.json";

/// What an Inboard backup holds.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct InboardBackup {
    /// Folder titles, in order (possibly repeated).
    pub folders: Vec<String>,
    /// The items that can be imported.
    pub items: Vec<InboardItem>,
    /// Image items whose file isn't in the backup.
    pub missing: usize,
}

/// One image to import.
#[derive(Debug, Clone, PartialEq)]
pub struct InboardItem {
    pub file: PathBuf,
    pub name: String,
    pub tags: Vec<String>,
    pub url: String,
    pub trashed: bool,
    /// The date added, ms since the epoch, when the backup's date reads.
    pub added_at: Option<u64>,
    /// Titles of the Inboard folders it was in.
    pub folders: Vec<String>,
}

/// Whether `dir` is an Inboard backup folder.
pub fn is_backup(dir: &Path) -> bool {
    dir.join(INDEX).is_file()
}

/// Reads the backup in `dir`.
pub fn read(dir: &Path) -> Result<InboardBackup> {
    let index = dir.join(INDEX);
    let bytes = fs::read(&index).map_err(|e| Error::io(&index, e))?;
    let v: Value = serde_json::from_slice(&bytes).map_err(|e| Error::json(&index, e))?;
    let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let list = |v: &Value, k: &str| v.get(k).and_then(Value::as_array).cloned().unwrap_or_default();
    let mut out = InboardBackup { folders: list(&v, "folders").iter().map(|f| text(f, "title")).collect(), ..Default::default() };
    for item in list(&v, "items") {
        if item.get("type").and_then(Value::as_str) != Some("image") {
            continue;
        }
        let path = text(&item, "path");
        let base = path.rsplit('/').next().unwrap_or_default();
        let sub = if path.contains("/screenshots/") { "screenshots" } else { "images" };
        let file = dir.join(sub).join(base);
        if base.is_empty() || !file.is_file() {
            out.missing += 1;
            continue;
        }
        out.items.push(InboardItem {
            file,
            name: text(&item, "title"),
            tags: list(&item, "tags").iter().filter_map(|t| t.as_str().map(String::from)).collect(),
            url: text(&item, "url"),
            trashed: match item.get("isTrashed") {
                Some(Value::Number(n)) => n.as_f64() != Some(0.0),
                Some(Value::Bool(b)) => *b,
                _ => false,
            },
            added_at: item.get("date").and_then(parse_date),
            folders: list(&item, "folders").iter().filter_map(|t| t.as_str().map(String::from)).collect(),
        });
    }
    Ok(out)
}

/// Inboard's `date`: a number is taken as ms (as JavaScript's `new
/// Date(n)` takes it), a string as RFC 3339 / ISO 8601 or
/// `YYYY-MM-DD HH:MM:SS ±ZZZZ`.
fn parse_date(v: &Value) -> Option<u64> {
    match v {
        Value::Number(n) => n.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64),
        Value::String(s) => {
            let s = s.trim();
            chrono::DateTime::parse_from_rfc3339(s)
                .or_else(|_| chrono::DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S %z"))
                .ok()
                .and_then(|d| u64::try_from(d.timestamp_millis()).ok())
        }
        _ => None,
    }
}

/// Which folder id each title maps to after creating one folder per
/// entry of `titles` (ids in the same order): a repeated title maps to the
/// last folder made with it, as in Eagle.
pub fn folder_map(titles: &[String], ids: &[String]) -> HashMap<String, String> {
    titles.iter().cloned().zip(ids.iter().cloned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_backup() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir_all(d.path().join("images")).unwrap();
        fs::create_dir_all(d.path().join("screenshots")).unwrap();
        fs::write(d.path().join("images/a.png"), b"a").unwrap();
        fs::write(d.path().join("screenshots/s.png"), b"s").unwrap();
        let json = serde_json::json!({
            "folders": [{"title": "Logos"}, {"title": "UI"}, {"title": "Logos"}],
            "items": [
                {"type": "image", "title": "A", "tags": ["x", "y"], "url": "https://example.com/a",
                 "isTrashed": 0, "date": "2018-03-04T05:06:07Z", "folders": ["Logos", "Nope"],
                 "path": "/Users/someone/Library/Containers/x/Data/images/a.png"},
                {"type": "image", "title": "Shot", "tags": [], "url": "", "isTrashed": 1,
                 "date": 1520139967000_u64, "folders": [], "path": "/x/screenshots/s.png"},
                {"type": "image", "title": "Gone", "path": "/x/images/gone.png", "folders": []},
                {"type": "link", "title": "Skip", "path": "/x/images/a.png"}
            ]
        });
        fs::write(d.path().join(INDEX), json.to_string()).unwrap();
        assert!(is_backup(d.path()));
        let b = read(d.path()).unwrap();
        assert_eq!(b.folders, vec!["Logos", "UI", "Logos"]);
        assert_eq!(b.missing, 1);
        assert_eq!(b.items.len(), 2);
        let a = &b.items[0];
        assert_eq!((a.name.as_str(), a.file.clone(), a.trashed), ("A", d.path().join("images/a.png"), false));
        assert_eq!(a.added_at, Some(1_520_139_967_000));
        assert_eq!(a.tags, vec!["x", "y"]);
        let s = &b.items[1];
        assert_eq!((s.file.clone(), s.trashed, s.added_at), (d.path().join("screenshots/s.png"), true, Some(1_520_139_967_000)));

        let map = folder_map(&b.folders, &["F1".into(), "F2".into(), "F3".into()]);
        assert_eq!(map["Logos"], "F3", "a repeated title goes to the last folder");
        assert_eq!(map["UI"], "F2");
    }

    #[test]
    fn dates() {
        assert_eq!(parse_date(&serde_json::json!("2018-03-04 05:06:07 +0000")), Some(1_520_139_967_000));
        assert_eq!(parse_date(&serde_json::json!("not a date")), None);
        assert_eq!(parse_date(&serde_json::json!(null)), None);
    }
}
