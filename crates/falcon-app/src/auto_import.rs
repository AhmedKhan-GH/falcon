//! The watched auto-import folder (Preferences ▸ Auto-Import), after
//! Eagle's `auto-import.js`: files added to a folder are imported into the
//! open library by themselves.
//!
//! As in Eagle, each library watches a folder named after it inside the
//! chosen one (`<chosen>/<library name without .library>`), so one chosen
//! folder serves every library. Network volumes are refused.
//!
//! Eagle watches with `fs.watch`; falcon has no file-event crate, so a
//! background thread lists the folder every two seconds instead. A file is
//! imported once its size and modification time have stayed the same for
//! three seconds (Eagle waits for six unchanged checks half a second
//! apart), so a copy still in progress is left alone. Eagle deletes each
//! file once it is imported; falcon only reads the folder, so it remembers
//! what it imported (path, size and mtime, kept with the app state) and
//! leaves the files where they are.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};

/// How often the folder is listed.
pub const POLL: Duration = Duration::from_secs(2);
/// How long a file must stay unchanged before it is imported.
pub const SETTLE: Duration = Duration::from_secs(3);
/// Eagle skips files of 1,000 bytes or less (SVGs only when empty).
pub const MIN_SIZE: u64 = 1000;
/// How many imported files are remembered.
pub const REMEMBERED: usize = 5000;

/// Why a file in the watched folder is left alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// The name contains `.tmp` anywhere, as Eagle checks.
    Temporary,
    /// A hidden file (`.DS_Store`, AppleDouble `._` files), which falcon's
    /// own drops skip too.
    Hidden,
    NoExtension,
    /// An unfinished browser download, or an internet shortcut (`.url`),
    /// which Eagle excludes by name.
    Unsupported,
    TooSmall,
}

/// Whether a file named `name` of `size` bytes is skipped, by Eagle's
/// rules. Any other extension imports, as it does when dropped (Eagle
/// takes only its supported formats here).
pub fn skip(name: &str, size: u64) -> Option<Skip> {
    if name.contains(".tmp") {
        return Some(Skip::Temporary);
    }
    if name.starts_with('.') {
        return Some(Skip::Hidden);
    }
    let ext = Path::new(name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if ext.is_empty() {
        return Some(Skip::NoExtension);
    }
    if ext == "url" || !falcon_library::import::is_recognized(&ext) {
        return Some(Skip::Unsupported);
    }
    let least = if ext == "svg" { 1 } else { MIN_SIZE };
    (size <= least).then_some(Skip::TooSmall)
}

/// What identifies a version of a file: its size and modification time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Sig {
    pub size: u64,
    pub mtime: u64,
}

impl Sig {
    pub fn of(meta: &std::fs::Metadata) -> Sig {
        let mtime = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64);
        Sig { size: meta.len(), mtime }
    }
}

/// Files seen in the folder and since when each has been unchanged.
/// The app's side of auto-import, kept in `App`.
#[derive(Default)]
pub struct AutoImport {
    /// The thread watching the shown library's auto-import folder.
    pub watcher: Option<Watcher>,
    /// Files it has handed over, shared with the thread and saved.
    pub imported: Arc<Mutex<Imported>>,
    /// A folder that couldn't be watched and was reported, so it isn't
    /// checked again until the preferences change.
    pub refused: Option<PathBuf>,
}

#[derive(Debug, Default)]
pub struct Settle {
    files: HashMap<PathBuf, (Sig, Instant)>,
}

impl Settle {
    /// Notes `path` as `sig` at `now`; true once it has been unchanged for
    /// `SETTLE`.
    pub fn observe(&mut self, path: &Path, sig: Sig, now: Instant) -> bool {
        match self.files.get_mut(path) {
            Some((s, since)) if *s == sig => now.duration_since(*since) >= SETTLE,
            Some(entry) => {
                *entry = (sig, now);
                false
            }
            None => {
                self.files.insert(path.to_path_buf(), (sig, now));
                false
            }
        }
    }

    /// Forgets files no longer in the folder, or handed to the import.
    pub fn keep_only(&mut self, present: &HashSet<PathBuf>) {
        self.files.retain(|p, _| present.contains(p));
    }
}

/// Files already imported, newest last, so they aren't imported again
/// while they stay in the folder unchanged.
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Imported(Vec<(PathBuf, Sig)>);

impl Imported {
    pub fn contains(&self, path: &Path, sig: Sig) -> bool {
        self.0.iter().any(|(p, s)| p == path && *s == sig)
    }

    pub fn add(&mut self, path: PathBuf, sig: Sig) {
        self.0.retain(|(p, _)| *p != path);
        self.0.push((path, sig));
        if self.0.len() > REMEMBERED {
            let extra = self.0.len() - REMEMBERED;
            self.0.drain(..extra);
        }
    }
}

/// One look at the folder: the files ready to import now, which are noted
/// as imported. A missing or unreadable folder yields nothing.
pub fn scan(dir: &Path, settle: &mut Settle, imported: &Mutex<Imported>, now: Instant) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        settle.keep_only(&HashSet::new());
        return Vec::new();
    };
    let mut present = HashSet::new();
    let mut ready = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        if !meta.is_file() || skip(&name, meta.len()).is_some() {
            continue;
        }
        let sig = Sig::of(&meta);
        if imported.lock().unwrap_or_else(|p| p.into_inner()).contains(&path, sig) {
            continue;
        }
        if settle.observe(&path, sig, now) {
            ready.push((path, sig));
        } else {
            present.insert(path);
        }
    }
    settle.keep_only(&present);
    ready.sort_by(|a, b| a.0.cmp(&b.0));
    let mut list = imported.lock().unwrap_or_else(|p| p.into_inner());
    for (path, sig) in &ready {
        list.add(path.clone(), *sig);
    }
    ready.into_iter().map(|(p, _)| p).collect()
}

/// The background thread watching one folder. Dropping it stops the
/// thread within a fraction of a second.
pub struct Watcher {
    pub dir: PathBuf,
    stop: Arc<AtomicBool>,
    batches: Receiver<Vec<PathBuf>>,
}

impl Watcher {
    /// Starts watching `dir`, calling `wake` when files are ready. Never
    /// starts under the test harness.
    pub fn start(dir: PathBuf, imported: Arc<Mutex<Imported>>, wake: impl Fn() + Send + 'static) -> Option<Watcher> {
        if cfg!(test) {
            return None;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, batches): (Sender<Vec<PathBuf>>, _) = channel();
        let (flag, at) = (stop.clone(), dir.clone());
        std::thread::Builder::new()
            .name("auto-import".into())
            .spawn(move || {
                let mut settle = Settle::default();
                while !flag.load(Ordering::Relaxed) {
                    let ready = scan(&at, &mut settle, &imported, Instant::now());
                    if !ready.is_empty() {
                        if tx.send(ready).is_err() {
                            return;
                        }
                        wake();
                    }
                    let until = Instant::now() + POLL;
                    while Instant::now() < until && !flag.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            })
            .ok()?;
        Some(Watcher { dir, stop, batches })
    }

    /// The files that became ready since the last call, batch by batch.
    pub fn take(&self) -> Vec<Vec<PathBuf>> {
        self.batches.try_iter().collect()
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The folder a library watches inside the chosen folder.
pub fn watch_dir(chosen: &Path, library_root: &Path) -> PathBuf {
    let name = library_root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    chosen.join(name.strip_suffix(".library").unwrap_or(&name))
}

/// Whether `path` is on a network volume. Eagle asks for the file system
/// type and refuses `smbfs`, `afpfs` and `nfs` (and `smb://` paths); falcon
/// asks macOS whether the volume is local, which also catches WebDAV and
/// other remote file systems.
pub fn is_network_volume(path: &Path) -> bool {
    let s = path.to_string_lossy().to_ascii_lowercase();
    if s.starts_with("smb://") || s.starts_with("afp://") || s.starts_with("nfs://") {
        return true;
    }
    platform::is_remote(path)
}

#[cfg(target_os = "macos")]
mod platform {
    use std::path::Path;

    use objc2_foundation::{NSNumber, NSString, NSURL, NSURLVolumeIsLocalKey};

    pub fn is_remote(path: &Path) -> bool {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        let mut value = None;
        // SAFETY: the key's value is an NSNumber holding a boolean (or nil).
        let got = unsafe { url.getResourceValue_forKey_error(&mut value, NSURLVolumeIsLocalKey) };
        match (got, value) {
            (Ok(()), Some(v)) => v.downcast::<NSNumber>().is_ok_and(|n| !n.as_bool()),
            _ => false,
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    pub fn is_remote(_: &std::path::Path) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eagles_skip_rules() {
        assert_eq!(skip("photo.jpg", 5000), None);
        assert_eq!(skip("photo.jpg.tmp", 5000), Some(Skip::Temporary));
        assert_eq!(skip("a.tmp.png", 5000), Some(Skip::Temporary), "Eagle looks for .tmp anywhere in the name");
        assert_eq!(skip(".DS_Store", 9000), Some(Skip::Hidden));
        assert_eq!(skip("README", 5000), Some(Skip::NoExtension));
        assert_eq!(skip("link.url", 5000), Some(Skip::Unsupported));
        assert_eq!(skip("movie.mp4.crdownload", 5000), Some(Skip::Unsupported));
        assert_eq!(skip("tiny.png", 1000), Some(Skip::TooSmall));
        assert_eq!(skip("small.png", 1001), None);
        assert_eq!(skip("icon.svg", 1), Some(Skip::TooSmall));
        assert_eq!(skip("icon.svg", 2), None);
        assert_eq!(skip("model.XYZ", 5000), None, "any other format imports, as dropped files do");
    }

    #[test]
    fn a_file_must_stay_unchanged_to_settle() {
        let t0 = Instant::now();
        let mut s = Settle::default();
        let p = Path::new("/w/a.png");
        let a = Sig { size: 10, mtime: 1 };
        assert!(!s.observe(p, a, t0));
        assert!(!s.observe(p, a, t0 + Duration::from_secs(2)));
        // Still growing: the wait starts again.
        let b = Sig { size: 20, mtime: 2 };
        assert!(!s.observe(p, b, t0 + Duration::from_secs(4)));
        assert!(!s.observe(p, b, t0 + Duration::from_secs(6)));
        assert!(s.observe(p, b, t0 + Duration::from_secs(7)));
    }

    #[test]
    fn imported_files_are_remembered_and_capped() {
        let mut list = Imported::default();
        let sig = Sig { size: 1, mtime: 1 };
        list.add("/w/a".into(), sig);
        assert!(list.contains(Path::new("/w/a"), sig));
        assert!(!list.contains(Path::new("/w/a"), Sig { size: 2, mtime: 1 }), "a changed file imports again");
        for n in 0..REMEMBERED + 10 {
            list.add(format!("/w/{n}").into(), sig);
        }
        assert_eq!(list.0.len(), REMEMBERED);
        assert!(!list.contains(Path::new("/w/a"), sig), "the oldest go first");
    }

    #[test]
    fn scanning_imports_settled_files_once() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("big.png"), vec![1u8; 4000]).unwrap();
        std::fs::write(dir.join("small.png"), vec![1u8; 10]).unwrap();
        std::fs::write(dir.join("part.png.tmp"), vec![1u8; 4000]).unwrap();
        std::fs::create_dir(dir.join("sub")).unwrap();
        let imported = Mutex::new(Imported::default());
        let mut settle = Settle::default();
        let t0 = Instant::now();
        assert!(scan(dir, &mut settle, &imported, t0).is_empty());
        assert!(scan(dir, &mut settle, &imported, t0 + Duration::from_secs(2)).is_empty());
        let ready = scan(dir, &mut settle, &imported, t0 + Duration::from_secs(4));
        assert_eq!(ready, [dir.join("big.png")]);
        // Not again while it stays as it is, even after a restart.
        let mut fresh = Settle::default();
        for s in [0, 4, 8] {
            assert!(scan(dir, &mut fresh, &imported, t0 + Duration::from_secs(s)).is_empty());
        }
        assert!(scan(&dir.join("missing"), &mut settle, &imported, t0).is_empty());
    }

    #[test]
    fn watchers_never_run_under_test() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(Watcher::start(tmp.path().to_path_buf(), Arc::default(), || {}).is_none());
    }

    #[test]
    fn each_library_watches_a_folder_named_after_it() {
        let chosen = Path::new("/Users/me/Drop");
        assert_eq!(watch_dir(chosen, Path::new("/x/Books.library")), Path::new("/Users/me/Drop/Books"));
        assert_eq!(watch_dir(chosen, Path::new("/x/Plain")), Path::new("/Users/me/Drop/Plain"));
    }

    #[test]
    fn local_folders_are_not_network_volumes() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!is_network_volume(tmp.path()));
        assert!(is_network_volume(Path::new("smb://192.168.0.1/share")));
    }
}
