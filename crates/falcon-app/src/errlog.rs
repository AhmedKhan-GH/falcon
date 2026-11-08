//! The error log, `~/Library/Logs/Falcon/falcon.log` on macOS: one line per error,
//! kept small by moving the file aside to `falcon.1.log` once it passes
//! 1 MiB. Writing never fails loudly; a log that can't be written is
//! simply not written.

use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::NaiveDateTime;

/// The log is moved aside when it is larger than this before a write.
const LIMIT: u64 = 1024 * 1024;

/// The folder holding the log.
fn dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        return Some(falcon_library::paths::home_dir()?.join("Library/Logs/Falcon"));
    }
    #[cfg(windows)]
    return Some(falcon_library::paths::local_data_dir()?.join("Falcon").join("Logs"));
    #[cfg(not(windows))]
    let state = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join(".local/state")))?;
    #[cfg(not(windows))]
    Some(state.join("falcon"))
}

/// The log file.
pub fn path() -> Option<PathBuf> {
    dir().map(|d| d.join("falcon.log"))
}

/// Appends an error line to the log.
pub fn error(msg: &str) {
    if let Some(dir) = dir() {
        append_in(&dir, chrono::Local::now().naive_local(), msg);
    }
}

/// Makes sure the log exists, starting it with a first line if it is new.
pub fn ensure() -> Option<PathBuf> {
    let file = path()?;
    ensure_in(file.parent()?)
}

fn ensure_in(dir: &Path) -> Option<PathBuf> {
    std::fs::create_dir_all(dir).ok()?;
    let file = dir.join("falcon.log");
    if !file.exists() {
        std::fs::write(&file, "Falcon error log\n").ok()?;
    }
    Some(file)
}

/// The line written for `msg`.
fn line(now: NaiveDateTime, msg: &str) -> String {
    format!("{} ERROR {msg}\n", now.format("%Y-%m-%d %H:%M:%S"))
}

fn append_in(dir: &Path, now: NaiveDateTime, msg: &str) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let file = dir.join("falcon.log");
    if std::fs::metadata(&file).is_ok_and(|m| m.len() > LIMIT) {
        let _ = std::fs::rename(&file, dir.join("falcon.1.log"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&file) {
        let _ = f.write_all(line(now, msg).as_bytes());
    }
}

/// Logs panics too, after the hook that was already there.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        let place = info.location().map_or_else(|| "unknown".to_string(), |l| format!("{}:{}", l.file(), l.line()));
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown".to_string());
        error(&format!("panic at {place}: {message}"));
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    #[test]
    fn lines_have_a_time_and_level() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("Logs/Falcon");
        append_in(&logs, at("2026-11-01 09:05:03"), "first");
        append_in(&logs, at("2026-11-01 09:05:04"), "second");
        let text = std::fs::read_to_string(logs.join("falcon.log")).unwrap();
        assert_eq!(text, "2026-11-01 09:05:03 ERROR first\n2026-11-01 09:05:04 ERROR second\n");
    }

    #[test]
    fn a_large_log_is_moved_aside() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("falcon.log"), vec![b'x'; LIMIT as usize + 1]).unwrap();
        std::fs::write(dir.path().join("falcon.1.log"), "old").unwrap();
        append_in(dir.path(), at("2026-11-01 10:00:00"), "fresh");
        let old = std::fs::read(dir.path().join("falcon.1.log")).unwrap();
        assert_eq!(old.len(), LIMIT as usize + 1);
        assert_eq!(std::fs::read_to_string(dir.path().join("falcon.log")).unwrap(), "2026-11-01 10:00:00 ERROR fresh\n");
    }

    #[test]
    fn a_log_at_the_limit_stays() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("falcon.log"), vec![b'x'; LIMIT as usize]).unwrap();
        append_in(dir.path(), at("2026-11-01 10:00:00"), "more");
        assert!(!dir.path().join("falcon.1.log").exists());
    }

    #[test]
    fn ensure_starts_a_log_once() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("a/b");
        let file = ensure_in(&logs).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "Falcon error log\n");
        append_in(&logs, at("2026-11-01 10:00:00"), "x");
        ensure_in(&logs).unwrap();
        assert!(std::fs::read_to_string(&file).unwrap().ends_with("ERROR x\n"));
    }
}
