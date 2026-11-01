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
//! apart), so a copy still in progress is left alone.
//!
//! Eagle deletes each file once it is imported. falcon does the same while
//! "Delete files after importing them" is on (the default), with three
//! differences, all on the side of keeping a file: it removes a file only
//! once its import committed (the item is in the library and its original
//! is in place there), only when the file is still directly inside the
//! watched folder and unchanged since it was handed over, and it moves the
//! file to the Trash (`NSFileManager trashItemAtURL:`) instead of
//! unlinking it, so a mistake can be undone from the Trash ([`after_import`]).
//! With the setting off, falcon leaves the files where they are. Either
//! way it remembers what it handed over (path, size and mtime, kept with
//! the app state), so a file that stays is not imported again.

use std::collections::{HashMap, HashSet};
use std::io;
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

    /// The version of `path` that was handed over, if it was.
    pub fn sig(&self, path: &Path) -> Option<Sig> {
        self.0.iter().rev().find(|(p, _)| p == path).map(|(_, s)| *s)
    }

    /// Forgets `path`, once it has left the folder, so a file dropped
    /// there again later under the same name imports again.
    pub fn forget(&mut self, path: &Path) {
        self.0.retain(|(p, _)| p != path);
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

/// What became of a file from the watched folder after its import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum After {
    /// Left where it is: deleting is off, the import didn't commit, or the
    /// file isn't the one that was imported any more.
    Kept,
    /// Moved to the Trash.
    Trashed,
    /// Removing it was allowed but failed; it stays in the folder.
    TrashFailed,
}

/// Whether `file`, handed over from `watched` as version `expected`, may be
/// removed now that its import gave `committed` (the item's original in
/// the library, `None` when the import failed or the file was held for the
/// duplicate dialog). Every check errs towards keeping the file:
/// - the import committed and its original exists, and is not the file
///   itself;
/// - `file` is a regular file (not a link or a folder) directly inside
///   `watched`, compared after resolving links, so nothing outside the
///   watched folder is ever touched;
/// - it is still the version that was handed over (same size and
///   modification time), so a file replaced since is kept and imported
///   in turn.
pub fn may_remove(file: &Path, watched: &Path, expected: Option<Sig>, committed: Option<&Path>) -> bool {
    let Some(original) = committed else { return false };
    let Ok(meta) = std::fs::symlink_metadata(file) else { return false };
    if !meta.file_type().is_file() || expected != Some(Sig::of(&meta)) {
        return false;
    }
    let (Some(parent), Some(_)) = (file.parent(), file.file_name()) else { return false };
    let (Ok(parent), Ok(watched)) = (parent.canonicalize(), watched.canonicalize()) else { return false };
    if parent != watched {
        return false;
    }
    match (original.canonicalize(), file.canonicalize()) {
        (Ok(o), Ok(f)) => o != f && !o.starts_with(&watched) && o.is_file(),
        _ => false,
    }
}

/// After a watched file's import: when `delete` is on and [`may_remove`]
/// allows it, hands the file to `trash` (the app passes [`trash`]; tests
/// pass their own). A trash failure keeps the file.
pub fn after_import(file: &Path, watched: &Path, delete: bool, expected: Option<Sig>, committed: Option<&Path>, trash: impl FnOnce(&Path) -> io::Result<()>) -> After {
    if !delete || !may_remove(file, watched, expected, committed) {
        return After::Kept;
    }
    match trash(file) {
        Ok(()) => After::Trashed,
        Err(_) => After::TrashFailed,
    }
}

/// What the import thread needs to remove auto-imported files: the
/// watched folder and the list of files handed over. Only made while
/// "Delete files after importing them" is on.
#[derive(Clone)]
pub struct Removal {
    pub watched: PathBuf,
    pub imported: Arc<Mutex<Imported>>,
}

impl Removal {
    /// [`after_import`] for `file`, with the version handed over taken from
    /// the list. A trashed file is forgotten, so the same file dropped in
    /// again later imports again (the duplicate check then applies).
    pub fn finish(&self, file: &Path, committed: Option<&Path>, trash: impl FnOnce(&Path) -> io::Result<()>) -> After {
        let expected = self.imported.lock().unwrap_or_else(|p| p.into_inner()).sig(file);
        let after = after_import(file, &self.watched, true, expected, committed, trash);
        if after == After::Trashed {
            self.imported.lock().unwrap_or_else(|p| p.into_inner()).forget(file);
        }
        after
    }
}

/// Moves `path` to the Trash. falcon trashes rather than deletes so that a
/// file removed by mistake can be put back from the Trash.
pub fn trash(path: &Path) -> io::Result<()> {
    platform::trash(path)
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

    use objc2_foundation::{NSFileManager, NSNumber, NSString, NSURL, NSURLVolumeIsLocalKey};

    pub fn trash(path: &Path) -> std::io::Result<()> {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        NSFileManager::defaultManager().trashItemAtURL_resultingItemURL_error(&url, None).map_err(|e| std::io::Error::other(e.localizedDescription().to_string()))
    }

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

    pub fn trash(_: &std::path::Path) -> std::io::Result<()> {
        Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "no Trash on this system"))
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

    /// A watched folder holding `photo.png`, handed over, and a library
    /// folder beside it holding the imported original.
    struct Setup {
        _tmp: tempfile::TempDir,
        watched: PathBuf,
        file: PathBuf,
        original: PathBuf,
        removal: Removal,
    }

    fn setup() -> Setup {
        let tmp = tempfile::tempdir().unwrap();
        let watched = tmp.path().join("Drop/Books");
        let lib = tmp.path().join("Books.library/images/X.info");
        std::fs::create_dir_all(&watched).unwrap();
        std::fs::create_dir_all(&lib).unwrap();
        let file = watched.join("photo.png");
        let original = lib.join("photo.png");
        std::fs::write(&file, vec![1u8; 4000]).unwrap();
        std::fs::write(&original, vec![1u8; 4000]).unwrap();
        let imported: Arc<Mutex<Imported>> = Arc::default();
        imported.lock().unwrap().add(file.clone(), Sig::of(&std::fs::metadata(&file).unwrap()));
        let removal = Removal { watched: watched.clone(), imported };
        Setup { _tmp: tmp, watched, file, original, removal }
    }

    /// A stand-in for the Trash that deletes and records what it was given.
    fn bin(log: &Mutex<Vec<PathBuf>>) -> impl FnOnce(&Path) -> io::Result<()> + '_ {
        move |p: &Path| {
            log.lock().unwrap().push(p.to_path_buf());
            std::fs::remove_file(p)
        }
    }

    #[test]
    fn a_committed_import_removes_the_file() {
        let s = setup();
        let log = Mutex::new(Vec::new());
        assert_eq!(s.removal.finish(&s.file, Some(&s.original), bin(&log)), After::Trashed);
        assert_eq!(*log.lock().unwrap(), std::slice::from_ref(&s.file));
        assert!(!s.file.exists() && s.original.exists(), "only the watched copy goes");
        assert_eq!(s.removal.imported.lock().unwrap().sig(&s.file), None, "forgotten, so the same file dropped again imports again");
    }

    #[test]
    fn a_failed_or_held_import_keeps_the_file() {
        let s = setup();
        let log = Mutex::new(Vec::new());
        assert_eq!(s.removal.finish(&s.file, None, bin(&log)), After::Kept, "failed, or held for the duplicate dialog");
        let gone = s.original.with_file_name("missing.png");
        assert_eq!(s.removal.finish(&s.file, Some(&gone), bin(&log)), After::Kept, "the original isn't in the library");
        assert_eq!(s.removal.finish(&s.file, Some(&s.file), bin(&log)), After::Kept, "the original can't be the file itself");
        assert!(log.lock().unwrap().is_empty() && s.file.exists());
        assert!(s.removal.imported.lock().unwrap().sig(&s.file).is_some(), "still remembered, so it isn't imported again");
    }

    #[test]
    fn deleting_off_keeps_the_file() {
        let s = setup();
        let sig = s.removal.imported.lock().unwrap().sig(&s.file);
        let log = Mutex::new(Vec::new());
        assert_eq!(after_import(&s.file, &s.watched, false, sig, Some(&s.original), bin(&log)), After::Kept);
        assert!(log.lock().unwrap().is_empty() && s.file.exists());
        assert_eq!(after_import(&s.file, &s.watched, true, sig, Some(&s.original), bin(&log)), After::Trashed);
    }

    #[test]
    fn nothing_outside_the_watched_folder_is_removed() {
        let s = setup();
        let log = Mutex::new(Vec::new());
        // A file in a subfolder, or elsewhere, even if listed as handed over.
        let sub = s.watched.join("sub");
        std::fs::create_dir(&sub).unwrap();
        let nested = sub.join("deep.png");
        std::fs::write(&nested, vec![2u8; 4000]).unwrap();
        let sig = Some(Sig::of(&std::fs::metadata(&nested).unwrap()));
        assert!(!may_remove(&nested, &s.watched, sig, Some(&s.original)));
        let outside = s.watched.parent().unwrap().join("other.png");
        std::fs::write(&outside, vec![2u8; 4000]).unwrap();
        let sig = Some(Sig::of(&std::fs::metadata(&outside).unwrap()));
        assert!(!may_remove(&outside, &s.watched, sig, Some(&s.original)));
        // A path that climbs out through `..`.
        let climbing = s.watched.join("../other.png");
        assert!(!may_remove(&climbing, &s.watched, sig, Some(&s.original)));
        // A link inside the folder is not followed to its target.
        #[cfg(unix)]
        {
            let link = s.watched.join("link.png");
            std::os::unix::fs::symlink(&outside, &link).unwrap();
            assert!(!may_remove(&link, &s.watched, sig, Some(&s.original)));
        }
        assert_eq!(after_import(&outside, &s.watched, true, sig, Some(&s.original), bin(&log)), After::Kept);
        assert!(log.lock().unwrap().is_empty() && outside.exists() && nested.exists());
    }

    #[test]
    fn a_file_changed_since_it_was_handed_over_is_kept() {
        let s = setup();
        std::fs::write(&s.file, vec![3u8; 9000]).unwrap();
        let log = Mutex::new(Vec::new());
        assert_eq!(s.removal.finish(&s.file, Some(&s.original), bin(&log)), After::Kept);
        let other = s.watched.join("never-handed-over.png");
        std::fs::write(&other, vec![1u8; 4000]).unwrap();
        assert_eq!(s.removal.finish(&other, Some(&s.original), bin(&log)), After::Kept, "not in the list");
        assert!(log.lock().unwrap().is_empty() && s.file.exists());
    }

    #[test]
    fn a_trash_failure_keeps_the_file() {
        let s = setup();
        let after = s.removal.finish(&s.file, Some(&s.original), |_: &Path| Err(io::Error::other("no")));
        assert_eq!(after, After::TrashFailed);
        assert!(s.file.exists());
        assert!(s.removal.imported.lock().unwrap().sig(&s.file).is_some(), "still remembered, so it isn't imported in a loop");
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
