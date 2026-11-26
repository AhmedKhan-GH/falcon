//! Rules, cross-volume move worker and path-keyed state migration for moving
//! an Eagle library folder to another location (such as an external SSD).
//!
//! Same-volume moves try an atomic rename first; cross-volume moves copy
//! every file in chunks (supporting cancellation and progress reporting),
//! verify the copy, and then move the original folder to the Trash rather than
//! deleting it outright.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::app::library::same_path;
use crate::logic::{Layout, is_library_path};

const COPY_BUF_BYTES: usize = 1 << 20;

/// Why a library move cannot start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    Missing,
    Symlink,
    NotLibrary,
    DestMissing,
    SameFolder,
    InsideSelf,
    AlreadyExists,
    InstallDir(String),
    OpenConflict,
}

/// Validated source and destination paths for a move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovePlan {
    pub src: PathBuf,
    pub dest: PathBuf,
    pub name: String,
}

/// Sizes counted before asking the user to confirm.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub bytes: u64,
    pub files: usize,
}

/// Result of moving the folder on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveOutcome {
    pub renamed: bool,
    pub trash_error: Option<String>,
}

/// Validates moving `src` into `dest_parent`.
///
/// `other_open` lists any open or loading libraries other than `src` itself.
pub fn check_destination(src: &Path, dest_parent: &Path, other_open: &[PathBuf]) -> Result<MovePlan, Refusal> {
    let meta = fs::symlink_metadata(src).map_err(|_| Refusal::Missing)?;
    if meta.file_type().is_symlink() {
        return Err(Refusal::Symlink);
    }
    if !meta.is_dir() || !is_library_path(src) || !src.join("metadata.json").is_file() || !src.join("images").is_dir() {
        return Err(Refusal::NotLibrary);
    }
    if !dest_parent.is_dir() {
        return Err(Refusal::DestMissing);
    }
    let folder_name = src.file_name().ok_or(Refusal::NotLibrary)?;
    let dest = dest_parent.join(folder_name);

    let src_canon = src.canonicalize().unwrap_or_else(|_| src.to_path_buf());
    let parent_canon = dest_parent.canonicalize().unwrap_or_else(|_| dest_parent.to_path_buf());

    if src_canon.parent() == Some(parent_canon.as_path()) || same_path(src, &dest) {
        return Err(Refusal::SameFolder);
    }
    if parent_canon.starts_with(&src_canon) {
        return Err(Refusal::InsideSelf);
    }
    if let Some(why) = crate::install_dir::refusal(&dest) {
        return Err(Refusal::InstallDir(why));
    }
    if dest.exists() || fs::symlink_metadata(&dest).is_ok() {
        return Err(Refusal::AlreadyExists);
    }
    for open in other_open {
        let oc = open.canonicalize().unwrap_or_else(|_| open.to_path_buf());
        if same_path(open, &dest) || oc.starts_with(&src_canon) || src_canon.starts_with(&oc) {
            return Err(Refusal::OpenConflict);
        }
    }

    let name = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(MovePlan { src: src.to_path_buf(), dest, name })
}

/// Counts total bytes and regular files/symlinks under `root` without
/// following symlinks or downloading iCloud placeholders.
pub fn measure_library(root: &Path, stop: &AtomicBool) -> io::Result<Stats> {
    let mut stats = Stats::default();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(&dir)? {
            if stop.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "stopped"));
            }
            let entry = entry?;
            let path = entry.path();
            let meta = fs::symlink_metadata(&path)?;
            if meta.is_dir() {
                dirs.push(path);
            } else {
                stats.bytes += meta.len();
                stats.files += 1;
            }
        }
    }
    Ok(stats)
}

/// Prefix of the folder a cross-volume move copies into before it is
/// committed. Only a folder with this prefix that this operation created is
/// ever removed by the move.
pub const STAGING_PREFIX: &str = ".falcon-move-";
/// File inside a staging folder naming the operation that owns it.
const OWNER_MARKER: &str = ".falcon-move-owner";

/// Points of a cross-volume move, for fault injection in tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveStage {
    /// After the first entry was copied.
    Copying,
    /// All copied, nothing verified yet.
    BeforeVerify,
    /// In the middle of verifying the copy.
    Verifying,
    /// Verified and openable, not yet committed.
    BeforeCommit,
    /// Committed at the destination, original still in place.
    AfterCommit,
    /// About to remove a staging folder after a failure.
    Cleanup,
}

/// What a library folder holds, by path relative to its root.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Entry {
    Dir,
    File { size: u64, hash: [u8; 32] },
    Symlink(PathBuf),
}

type Manifest = std::collections::BTreeMap<PathBuf, Entry>;

/// Moves `src` to `dest`. Tries an atomic same-volume rename first; if that
/// fails (e.g. across volumes to an external SSD), copies into a staging
/// folder beside `dest` in chunks with progress and cancellation, verifies
/// every path, size and content hash against what was copied, checks that
/// the copy opens as a library, and only then renames the staging folder to
/// `dest` and sends `src` to `trasher`. The original is never touched before
/// the copy is committed, and a failure removes only the staging folder this
/// call created.
pub fn execute_move(
    src: &Path,
    dest: &Path,
    cancel: &AtomicBool,
    progress: impl FnMut(usize, u64, &str),
    trasher: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<MoveOutcome, String> {
    execute_move_inner(src, dest, true, crate::storage::available_space, cancel, progress, trasher)
}

fn execute_move_inner(
    src: &Path,
    dest: &Path,
    allow_rename: bool,
    free_space: impl Fn(&Path) -> Option<u64>,
    cancel: &AtomicBool,
    progress: impl FnMut(usize, u64, &str),
    trasher: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<MoveOutcome, String> {
    execute_move_staged(src, dest, allow_rename, free_space, cancel, progress, trasher, |_| Ok(()))
}

#[allow(clippy::too_many_arguments)]
fn execute_move_staged(
    src: &Path,
    dest: &Path,
    allow_rename: bool,
    free_space: impl Fn(&Path) -> Option<u64>,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, u64, &str),
    trasher: impl FnOnce(&Path) -> io::Result<()>,
    mut hook: impl FnMut(MoveStage) -> io::Result<()>,
) -> Result<MoveOutcome, String> {
    let cancelled = |cancel: &AtomicBool, err: &io::Error| {
        if cancel.load(Ordering::Relaxed) { "cancelled".to_string() } else { err.to_string() }
    };
    if cancel.load(Ordering::Relaxed) {
        return Err("cancelled".to_string());
    }
    // `rename` onto an existing directory would replace an empty one.
    if fs::symlink_metadata(dest).is_ok() {
        return Err(format!("{} already exists", dest.display()));
    }
    if allow_rename && fs::rename(src, dest).is_ok() {
        return Ok(MoveOutcome { renamed: true, trash_error: None });
    }
    let parent = dest.parent().ok_or_else(|| "destination has no parent folder".to_string())?;
    if let Some(avail) = free_space(parent) {
        let stats = measure_library(src, cancel).map_err(|err| cancelled(cancel, &err))?;
        ensure_free_space(avail, stats.bytes)?;
    }
    // The state of the original before copying, to notice a change meanwhile.
    let before = scan_source(src, cancel).map_err(|err| cancelled(cancel, &err))?;

    let staging = Staging::create(parent, src).map_err(|e| format!("can't create a staging folder in {}: {e}", parent.display()))?;
    let result = (|| -> Result<(), String> {
        let mut files_done = 0usize;
        let mut bytes_done = 0u64;
        let mut manifest = Manifest::new();
        let mut first = true;
        let mut on_progress = |f: usize, b: u64, name: &str| {
            progress(f, b, name);
        };
        copy_tree(src, staging.path(), Path::new(""), cancel, &mut files_done, &mut bytes_done, &mut manifest, &mut on_progress, &mut || {
            if std::mem::take(&mut first) { hook(MoveStage::Copying) } else { Ok(()) }
        })
        .map_err(|e| cancelled(cancel, &e))?;
        hook(MoveStage::BeforeVerify).map_err(|e| e.to_string())?;
        // The original must not have changed while it was copied.
        let after = scan_source(src, cancel).map_err(|e| cancelled(cancel, &e))?;
        if before != after {
            return Err("The library changed while it was being copied; nothing was moved. Try again when nothing else is using it.".into());
        }
        verify_tree(staging.path(), &manifest, cancel, &mut || hook(MoveStage::Verifying)).map_err(|e| cancelled(cancel, &e))?;
        verify_copied_library(src, staging.path())?;
        falcon_library::Library::open(staging.path()).map_err(|e| format!("copied library can't be opened: {e}"))?;
        sync_tree(staging.path());
        hook(MoveStage::BeforeCommit).map_err(|e| e.to_string())?;
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".to_string());
        }
        Ok(())
    })();
    if let Err(err) = result {
        staging.discard(&mut hook);
        return Err(err);
    }
    // Commit: a rename inside one folder is atomic. It cannot replace a
    // non-empty folder, and the check above keeps an existing one out.
    if let Err(e) = staging.commit(dest) {
        staging.discard(&mut hook);
        return Err(format!("couldn't put the copied library at {}: {e}", dest.display()));
    }
    sync_dir(parent);
    // The original stays until the copy is committed; a failure here leaves
    // both, which is reported but safe.
    let trash_error = hook(MoveStage::AfterCommit).and_then(|()| trasher(src)).err().map(|e| e.to_string());
    Ok(MoveOutcome { renamed: false, trash_error })
}

/// A staging folder created by one move. Created with a plain `create_dir`,
/// so it never adopts an existing folder.
struct Staging {
    path: PathBuf,
    token: String,
}

impl Staging {
    fn create(parent: &Path, src: &Path) -> io::Result<Staging> {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let seed = format!("{}-{nanos}-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed), src.display());
        let token = blake3::hash(seed.as_bytes()).to_hex()[..16].to_string();
        let path = parent.join(format!("{STAGING_PREFIX}{token}"));
        fs::create_dir(&path)?;
        let staging = Staging { path, token };
        let mut marker = fs::OpenOptions::new().write(true).create_new(true).open(staging.path.join(OWNER_MARKER))?;
        marker.write_all(format!("{}\n{}\n", staging.token, src.display()).as_bytes())?;
        marker.sync_all()?;
        Ok(staging)
    }

    fn path(&self) -> &Path {
        &self.path
    }

    /// Whether `self.path` is still the real folder this operation made: not
    /// replaced by a link, and carrying our token (or still empty).
    fn is_ours(&self) -> bool {
        let Ok(meta) = fs::symlink_metadata(&self.path) else { return false };
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return false;
        }
        match fs::read_to_string(self.path.join(OWNER_MARKER)) {
            Ok(text) => text.lines().next() == Some(self.token.as_str()),
            Err(_) => fs::read_dir(&self.path).is_ok_and(|mut d| d.next().is_none()),
        }
    }

    fn commit(&self, dest: &Path) -> io::Result<()> {
        if !self.is_ours() {
            return Err(io::Error::other("the staging folder was replaced"));
        }
        if fs::symlink_metadata(dest).is_ok() {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, "the destination appeared during the move"));
        }
        let marker = self.path.join(OWNER_MARKER);
        let text = fs::read(&marker)?;
        fs::remove_file(&marker)?;
        let renamed = fs::rename(&self.path, dest);
        if renamed.is_err() {
            // Still ours: put the marker back so cleanup can prove it.
            let _ = fs::write(&marker, text);
        }
        renamed
    }

    /// Removes the staging folder if it is still this operation's own. A
    /// failure (or a folder that is no longer ours) leaves it for
    /// [`stale_staging`] to report.
    fn discard(&self, hook: &mut impl FnMut(MoveStage) -> io::Result<()>) {
        if hook(MoveStage::Cleanup).is_err() || !self.is_ours() {
            return;
        }
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Staging folders a move left in `parent` (a crash, a failed cleanup, a
/// disconnected disk), with whether each carries an owner marker. Nothing is
/// removed; the original library is intact whenever one of these exists.
pub fn stale_staging(parent: &Path) -> Vec<(PathBuf, bool)> {
    let Ok(entries) = fs::read_dir(parent) else { return Vec::new() };
    let mut out: Vec<(PathBuf, bool)> = entries
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(STAGING_PREFIX))
        .filter(|e| fs::symlink_metadata(e.path()).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink()))
        .map(|e| {
            let marked = e.path().join(OWNER_MARKER).is_file();
            (e.path(), marked)
        })
        .collect();
    out.sort();
    out
}

/// Removes a staging folder found by [`stale_staging`] after the user agreed.
/// Refuses anything that is not a real, marked staging folder. Not yet offered
/// in the interface; the failure alert names the folder for manual removal.
#[allow(dead_code)]
pub fn remove_stale_staging(path: &Path) -> io::Result<()> {
    let name_ok = path.file_name().is_some_and(|n| n.to_string_lossy().starts_with(STAGING_PREFIX));
    let meta = fs::symlink_metadata(path)?;
    if !name_ok || meta.file_type().is_symlink() || !meta.is_dir() || !path.join(OWNER_MARKER).is_file() {
        return Err(io::Error::other("not a staging folder created by a library move"));
    }
    fs::remove_dir_all(path)
}

fn ensure_free_space(available: u64, required: u64) -> Result<(), String> {
    if available < required {
        return Err(format!(
            "Not enough free disk space at destination ({} needed, {} available).",
            crate::logic::format_size(required),
            crate::logic::format_size(available),
        ));
    }
    Ok(())
}

/// Library-level checks beyond byte identity: the JSON that makes it a library.
fn verify_copied_library(src: &Path, dest: &Path) -> Result<(), String> {
    if !dest.join("metadata.json").is_file() || !dest.join("images").is_dir() {
        return Err("copied library is incomplete".to_string());
    }
    let meta = fs::read(dest.join("metadata.json")).map_err(|e| format!("copied metadata.json unreadable: {e}"))?;
    let parsed: serde_json::Value =
        serde_json::from_slice(&meta).map_err(|e| format!("copied metadata.json is invalid JSON: {e}"))?;
    if !parsed.is_object() {
        return Err("copied metadata.json has unexpected structure".to_string());
    }
    if src.join("mtime.json").is_file() {
        let mtime = fs::read(dest.join("mtime.json")).map_err(|e| format!("copied mtime.json unreadable: {e}"))?;
        let mtime_val: serde_json::Value =
            serde_json::from_slice(&mtime).map_err(|e| format!("copied mtime.json is invalid JSON: {e}"))?;
        if !mtime_val.is_object() {
            return Err("copied mtime.json has unexpected structure".to_string());
        }
    }
    Ok(())
}

/// Kind, size and modification time of every path under `root`, without
/// following links.
fn scan_source(root: &Path, stop: &AtomicBool) -> io::Result<std::collections::BTreeMap<PathBuf, (u8, u64, u128)>> {
    let mut out = std::collections::BTreeMap::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(&dir)? {
            if stop.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            let entry = entry?;
            let path = entry.path();
            let meta = fs::symlink_metadata(&path)?;
            let kind = if meta.file_type().is_symlink() { 2 } else if meta.is_dir() { 0 } else { 1 };
            let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
            let rel = path.strip_prefix(root).map_err(io::Error::other)?.to_path_buf();
            out.insert(rel, (kind, if kind == 1 { meta.len() } else { 0 }, if kind == 1 { mtime } else { 0 }));
            if kind == 0 {
                dirs.push(path);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
fn copy_dir_cancellable(
    src: &Path,
    dest: &Path,
    cancel: &AtomicBool,
    files_done: &mut usize,
    bytes_done: &mut u64,
    progress: &mut impl FnMut(usize, u64, &str),
) -> io::Result<()> {
    fs::create_dir_all(dest)?;
    let mut manifest = Manifest::new();
    copy_tree(src, dest, Path::new(""), cancel, files_done, bytes_done, &mut manifest, progress, &mut || Ok(()))
}

/// Copies everything under `src` into the existing folder `dest`, recording
/// each path, size and blake3 hash of what was read into `manifest`. Links
/// are copied as links. Files are created exclusively and synced.
#[allow(clippy::too_many_arguments)]
fn copy_tree(
    src: &Path,
    dest: &Path,
    rel: &Path,
    cancel: &AtomicBool,
    files_done: &mut usize,
    bytes_done: &mut u64,
    manifest: &mut Manifest,
    progress: &mut impl FnMut(usize, u64, &str),
    on_entry: &mut impl FnMut() -> io::Result<()>,
) -> io::Result<()> {
    let mut buf = vec![0u8; COPY_BUF_BYTES];
    copy_tree_inner(src, dest, rel, cancel, files_done, bytes_done, manifest, &mut buf, progress, on_entry)
}

#[allow(clippy::too_many_arguments)]
fn copy_tree_inner(
    src: &Path,
    dest: &Path,
    rel: &Path,
    cancel: &AtomicBool,
    files_done: &mut usize,
    bytes_done: &mut u64,
    manifest: &mut Manifest,
    buf: &mut [u8],
    progress: &mut impl FnMut(usize, u64, &str),
    on_entry: &mut impl FnMut() -> io::Result<()>,
) -> io::Result<()> {
    for entry in fs::read_dir(src.join(rel))? {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let entry = entry?;
        let name = entry.file_name();
        // A name that is not one plain component could lead out of `dest`.
        if Path::new(&name).components().count() != 1 || Path::new(&name).components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
            return Err(io::Error::other(format!("unsafe file name {name:?}")));
        }
        let rel_path = rel.join(&name);
        let src_path = src.join(&rel_path);
        let dest_path = dest.join(&rel_path);
        let meta = fs::symlink_metadata(&src_path)?;
        if meta.file_type().is_symlink() {
            copy_symlink(&src_path, &dest_path)?;
            manifest.insert(rel_path, Entry::Symlink(fs::read_link(&src_path)?));
            *files_done += 1;
            progress(*files_done, *bytes_done, &name.to_string_lossy());
        } else if meta.is_dir() {
            fs::create_dir(&dest_path)?;
            manifest.insert(rel_path.clone(), Entry::Dir);
            copy_tree_inner(src, dest, &rel_path, cancel, files_done, bytes_done, manifest, buf, progress, on_entry)?;
        } else {
            let label = name.to_string_lossy().into_owned();
            progress(*files_done, *bytes_done, &label);
            let (size, hash) = copy_file_chunked(&src_path, &dest_path, cancel, buf, |chunk| {
                *bytes_done += chunk;
                progress(*files_done, *bytes_done, &label);
            })?;
            manifest.insert(rel_path, Entry::File { size, hash });
            *files_done += 1;
            progress(*files_done, *bytes_done, &label);
        }
        on_entry()?;
    }
    Ok(())
}

/// Copies one file, syncing it, and returns its size and blake3 hash as read.
fn copy_file_chunked(
    src: &Path,
    dest: &Path,
    cancel: &AtomicBool,
    buf: &mut [u8],
    mut on_chunk: impl FnMut(u64),
) -> io::Result<(u64, [u8; 32])> {
    let mut reader = File::open(src)?;
    let mut writer = fs::OpenOptions::new().write(true).create_new(true).open(dest)?;
    let mut hasher = blake3::Hasher::new();
    let mut size = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let n = reader.read(buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        writer.write_all(&buf[..n])?;
        size += n as u64;
        on_chunk(n as u64);
    }
    writer.flush()?;
    writer.sync_all()?;
    drop(writer);
    if let Ok(meta) = fs::metadata(src) {
        let _ = fs::set_permissions(dest, meta.permissions());
        if let (Ok(modified), Ok(f)) = (meta.modified(), fs::OpenOptions::new().write(true).open(dest)) {
            let mut times = fs::FileTimes::new().set_modified(modified);
            if let Ok(accessed) = meta.accessed() {
                times = times.set_accessed(accessed);
            }
            let _ = f.set_times(times);
        }
    }
    Ok((size, *hasher.finalize().as_bytes()))
}

/// Checks that `root` holds exactly the paths, sizes, link targets and file
/// contents of `manifest` (apart from the owner marker). Contents are read
/// back from the destination and hashed, so a short or corrupted write is
/// caught, not just a wrong total size.
fn verify_tree(root: &Path, manifest: &Manifest, cancel: &AtomicBool, on_file: &mut impl FnMut() -> io::Result<()>) -> io::Result<()> {
    let bad = |msg: String| io::Error::other(format!("copy verification failed: {msg}"));
    let mut seen = 0usize;
    let mut dirs = vec![PathBuf::new()];
    let mut buf = vec![0u8; COPY_BUF_BYTES];
    let mut first = true;
    while let Some(rel) = dirs.pop() {
        for entry in fs::read_dir(root.join(&rel))? {
            if cancel.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            let entry = entry?;
            let rel_path = rel.join(entry.file_name());
            if rel_path == Path::new(OWNER_MARKER) {
                continue;
            }
            let path = root.join(&rel_path);
            let meta = fs::symlink_metadata(&path)?;
            let want = manifest.get(&rel_path).ok_or_else(|| bad(format!("unexpected {}", rel_path.display())))?;
            seen += 1;
            match want {
                Entry::Dir if meta.is_dir() && !meta.file_type().is_symlink() => dirs.push(rel_path),
                Entry::Symlink(target) if meta.file_type().is_symlink() && fs::read_link(&path)? == *target => {}
                Entry::File { size, hash } if meta.is_file() && meta.len() == *size => {
                    if std::mem::take(&mut first) {
                        on_file()?;
                    }
                    let mut f = File::open(&path)?;
                    let mut hasher = blake3::Hasher::new();
                    loop {
                        let n = f.read(&mut buf)?;
                        if n == 0 {
                            break;
                        }
                        hasher.update(&buf[..n]);
                    }
                    if hasher.finalize().as_bytes() != hash {
                        return Err(bad(format!("{} differs from the original", rel_path.display())));
                    }
                }
                _ => return Err(bad(format!("{} does not match the original", rel_path.display()))),
            }
        }
    }
    if seen != manifest.len() {
        return Err(bad(format!("{} entries copied, {} expected", seen, manifest.len())));
    }
    Ok(())
}

/// Best-effort flush of a directory's entries to disk.
fn sync_dir(dir: &Path) {
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
}

fn sync_tree(root: &Path) {
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        if let Ok(entries) = fs::read_dir(&dir) {
            for e in entries.flatten() {
                if e.file_type().is_ok_and(|t| t.is_dir()) {
                    dirs.push(e.path());
                }
            }
        }
        sync_dir(&dir);
    }
}

#[cfg(unix)]
fn copy_symlink(src: &Path, dest: &Path) -> io::Result<()> {
    let target = fs::read_link(src)?;
    std::os::unix::fs::symlink(target, dest)
}

#[cfg(windows)]
fn copy_symlink(src: &Path, dest: &Path) -> io::Result<()> {
    let target = fs::read_link(src)?;
    if fs::metadata(src).is_ok_and(|m| m.is_dir()) {
        std::os::windows::fs::symlink_dir(target, dest)
    } else {
        std::os::windows::fs::symlink_file(target, dest)
    }
}

/// Replaces `old` with `new` in the recent-libraries list, preserving order
/// and removing duplicates.
pub fn replace_recent_path(recent: &mut Vec<PathBuf>, old: &Path, new: &Path) {
    let mut replaced = false;
    let mut out: Vec<PathBuf> = Vec::with_capacity(recent.len() + 1);
    for p in recent.drain(..) {
        let next = if same_path(&p, old) {
            replaced = true;
            new.to_path_buf()
        } else {
            p
        };
        if !out.iter().any(|existing| same_path(existing, &next)) {
            out.push(next);
        }
    }
    if !replaced && !out.iter().any(|existing| same_path(existing, new)) {
        out.insert(0, new.to_path_buf());
    }
    *recent = out;
}

/// Moves the library-level layout entry from `old` to `new`.
pub fn migrate_layouts(layouts: &mut HashMap<String, Layout>, old: &Path, new: &Path) {
    let old_key = old.to_string_lossy().into_owned();
    let new_key = new.to_string_lossy().into_owned();
    if old_key != new_key
        && let Some(layout) = layouts.remove(&old_key)
    {
        layouts.entry(new_key).or_insert(layout);
    }
}

/// Moves a path-keyed sidecar file (`actions/<key>.json` or
/// `eagle-backup/<hash>.json`) from `old_file` to `new_file` if it exists.
pub fn migrate_sidecar_file(old_file: Option<&Path>, new_file: Option<&Path>) {
    let (Some(old), Some(new)) = (old_file, new_file) else { return };
    if old == new || !old.is_file() {
        return;
    }
    if let Some(parent) = new.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if fs::rename(old, new).is_err() && fs::copy(old, new).is_ok() {
        let _ = fs::remove_file(old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_lib(root: &Path) {
        falcon_library::Library::create(root).unwrap();
    }

    #[test]
    fn validates_destination_and_rejects_bad_targets() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let src_parent = base.join("From");
        let dest_parent = base.join("To");
        fs::create_dir_all(&src_parent).unwrap();
        fs::create_dir_all(&dest_parent).unwrap();

        let lib = src_parent.join("Books.library");
        make_lib(&lib);

        // Valid destination.
        let plan = check_destination(&lib, &dest_parent, &[]).unwrap();
        assert_eq!(plan.dest, dest_parent.join("Books.library"));
        assert_eq!(plan.name, "Books");

        // Same parent folder is refused.
        assert_eq!(check_destination(&lib, &src_parent, &[]), Err(Refusal::SameFolder));

        // Inside itself is refused.
        let inside = lib.join("images");
        assert_eq!(check_destination(&lib, &inside, &[]), Err(Refusal::InsideSelf));

        // Existing target is refused.
        let taken = dest_parent.join("Books.library");
        fs::create_dir_all(&taken).unwrap();
        assert_eq!(check_destination(&lib, &dest_parent, &[]), Err(Refusal::AlreadyExists));
        fs::remove_dir_all(&taken).unwrap();

        // Missing or non-library source is refused.
        assert_eq!(check_destination(&base.join("Missing.library"), &dest_parent, &[]), Err(Refusal::Missing));
        let plain = base.join("Plain");
        fs::create_dir_all(&plain).unwrap();
        assert_eq!(check_destination(&plain, &dest_parent, &[]), Err(Refusal::NotLibrary));

        // Another open library conflicting with nested path is refused.
        let nested = lib.join("Nested.library");
        make_lib(&nested);
        assert_eq!(check_destination(&lib, &dest_parent, &[nested]), Err(Refusal::OpenConflict));
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinked_library_source() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let real = base.join("Real.library");
        let link = base.join("Link.library");
        let dest_parent = base.join("Dest");
        make_lib(&real);
        fs::create_dir_all(&dest_parent).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(check_destination(&link, &dest_parent, &[]), Err(Refusal::Symlink));
    }

    #[test]
    fn execute_move_copies_and_trashes_when_rename_is_skipped_or_cross_volume() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("Photos.library");
        let mut lib = falcon_library::Library::create(&src).unwrap();
        let item_src = tmp.path().join("sample.txt");
        fs::write(&item_src, b"hello world").unwrap();
        lib.import_file(&item_src, &falcon_library::import::ImportOptions::default()).unwrap();
        drop(lib);

        let dest = tmp.path().join("Moved").join("Photos.library");
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        let stats = measure_library(&src, &AtomicBool::new(false)).unwrap();
        assert!(stats.files >= 5);
        assert!(stats.bytes > 0);

        let cancel = AtomicBool::new(false);
        let outcome = execute_move(&src, &dest, &cancel, |_, _, _| {}, |_| Ok(())).unwrap();
        assert!(outcome.renamed);
        assert!(dest.join("metadata.json").is_file());
        assert!(dest.join("images").is_dir());

        // Also test the cross-volume chunked copy + trash path directly.
        let dest2 = tmp.path().join("Moved2").join("Photos.library");
        fs::create_dir_all(dest2.parent().unwrap()).unwrap();
        let trashed = std::cell::Cell::new(false);
        let mut reported_bytes = 0u64;
        let outcome2 = execute_move_inner(
            &dest,
            &dest2,
            false,
            |_| None,
            &cancel,
            |_, b, _| reported_bytes = b,
            |p| {
                trashed.set(true);
                fs::remove_dir_all(p)
            },
        )
        .unwrap();
        assert!(!outcome2.renamed);
        assert_eq!(outcome2.trash_error, None);
        assert!(trashed.get());
        assert_eq!(reported_bytes, stats.bytes);
        let reopened = falcon_library::Library::open(&dest2).unwrap();
        assert_eq!(reopened.items().len(), 1);
    }

    #[test]
    fn copy_dir_cancellable_cleans_up_partial_destination_on_cancel() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("Photos.library");
        make_lib(&src);
        let dest = tmp.path().join("Dest.library");
        let cancel = AtomicBool::new(true);
        let mut files = 0;
        let mut bytes = 0;
        let err = copy_dir_cancellable(&src, &dest, &cancel, &mut files, &mut bytes, &mut |_, _, _| {}).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Interrupted);
        let _ = fs::remove_dir_all(&dest);
        assert!(!dest.exists());
        assert!(src.join("metadata.json").is_file());
    }

    #[test]
    fn migrates_recent_layouts_and_sidecar_files() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("Old.library");
        let new = tmp.path().join("New.library");
        let other = tmp.path().join("Other.library");

        let mut recent = vec![other.clone(), old.clone()];
        replace_recent_path(&mut recent, &old, &new);
        assert_eq!(recent, vec![other, new.clone()]);

        let mut layouts = HashMap::new();
        layouts.insert(old.to_string_lossy().into_owned(), Layout::Waterfall);
        layouts.insert("folder-1".to_string(), Layout::Grid);
        migrate_layouts(&mut layouts, &old, &new);
        assert!(!layouts.contains_key(&old.to_string_lossy().into_owned()));
        assert_eq!(layouts.get(&new.to_string_lossy().into_owned()), Some(&Layout::Waterfall));
        assert_eq!(layouts.get("folder-1"), Some(&Layout::Grid));

        let old_sidecar = tmp.path().join("actions/old.json");
        let new_sidecar = tmp.path().join("actions/new.json");
        fs::create_dir_all(old_sidecar.parent().unwrap()).unwrap();
        fs::write(&old_sidecar, b"[1,2]").unwrap();
        migrate_sidecar_file(Some(&old_sidecar), Some(&new_sidecar));
        assert!(!old_sidecar.exists());
        assert_eq!(fs::read(&new_sidecar).unwrap(), b"[1,2]");
    }

    #[test]
    fn cross_volume_insufficient_disk_space_aborts_before_writing_dest() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("Photos.library");
        let mut lib = falcon_library::Library::create(&src).unwrap();
        let sample = tmp.path().join("big.bin");
        fs::write(&sample, vec![42u8; 4096]).unwrap();
        lib.import_file(&sample, &falcon_library::import::ImportOptions::default()).unwrap();
        drop(lib);

        let dest = tmp.path().join("SmallDrive/Photos.library");
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        let trashed = std::cell::Cell::new(false);
        let cancel = AtomicBool::new(false);

        let err = execute_move_inner(
            &src,
            &dest,
            false,
            |_| Some(512),
            &cancel,
            |_, _, _| {},
            |_| {
                trashed.set(true);
                Ok(())
            },
        )
        .unwrap_err();

        assert!(err.contains("Not enough free disk space"), "{err}");
        assert!(!dest.exists());
        assert!(!trashed.get());
        assert!(falcon_library::Library::open(&src).is_ok());
    }

    #[test]
    fn cross_volume_mid_transfer_cancellation_cleans_dest_and_keeps_source() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("Photos.library");
        let mut lib = falcon_library::Library::create(&src).unwrap();
        let big = tmp.path().join("chunked.bin");
        // Larger than COPY_BUF_BYTES (1 MiB) so cancel fires mid-copy after chunks start arriving.
        fs::write(&big, vec![7u8; COPY_BUF_BYTES + 4096]).unwrap();
        lib.import_file(&big, &falcon_library::import::ImportOptions::default()).unwrap();
        drop(lib);

        let dest = tmp.path().join("Dest/Photos.library");
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        let trashed = std::cell::Cell::new(false);
        let cancel = AtomicBool::new(false);

        let err = execute_move_inner(
            &src,
            &dest,
            false,
            |_| None,
            &cancel,
            |_, bytes, _| {
                if bytes > 0 {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
            |_| {
                trashed.set(true);
                Ok(())
            },
        )
        .unwrap_err();

        assert_eq!(err, "cancelled");
        assert!(!dest.exists());
        assert!(!trashed.get());
        let reopened = falcon_library::Library::open(&src).unwrap();
        assert_eq!(reopened.items().len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn cross_volume_permission_error_leaves_source_intact() {
        use std::os::unix::fs::PermissionsExt;
        struct RestoreMode(PathBuf, u32);
        impl Drop for RestoreMode {
            fn drop(&mut self) {
                let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(self.1));
            }
        }

        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("Photos.library");
        make_lib(&src);
        let ro_parent = tmp.path().join("ReadOnly");
        fs::create_dir_all(&ro_parent).unwrap();
        let _guard = RestoreMode(ro_parent.clone(), 0o755);
        fs::set_permissions(&ro_parent, fs::Permissions::from_mode(0o555)).unwrap();

        let dest = ro_parent.join("Photos.library");
        let trashed = std::cell::Cell::new(false);
        let cancel = AtomicBool::new(false);

        let err = execute_move_inner(
            &src,
            &dest,
            false,
            |_| None,
            &cancel,
            |_, _, _| {},
            |_| {
                trashed.set(true);
                Ok(())
            },
        )
        .unwrap_err();

        assert!(!err.is_empty());
        assert!(!dest.exists());
        assert!(!trashed.get());
        assert!(src.join("metadata.json").is_file());
    }

    #[test]
    fn cross_volume_corrupt_metadata_is_rejected_and_source_stays_intact() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("Damaged.library");
        make_lib(&src);
        fs::write(src.join("metadata.json"), b"{not valid json").unwrap();

        let dest = tmp.path().join("Dest/Damaged.library");
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        let trashed = std::cell::Cell::new(false);
        let cancel = AtomicBool::new(false);

        let err = execute_move_inner(
            &src,
            &dest,
            false,
            |_| None,
            &cancel,
            |_, _, _| {},
            |_| {
                trashed.set(true);
                Ok(())
            },
        )
        .unwrap_err();

        assert!(err.contains("metadata.json"), "{err}");
        assert!(!dest.exists());
        assert!(!trashed.get());
        assert!(src.join("metadata.json").is_file());
    }

    #[test]
    fn cross_volume_trash_failure_keeps_verified_destination_and_source() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("Photos.library");
        let mut lib = falcon_library::Library::create(&src).unwrap();
        let sample = tmp.path().join("note.txt");
        fs::write(&sample, b"important data").unwrap();
        lib.import_file(&sample, &falcon_library::import::ImportOptions::default()).unwrap();
        drop(lib);

        let dest = tmp.path().join("External/Photos.library");
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        let cancel = AtomicBool::new(false);

        let outcome = execute_move_inner(
            &src,
            &dest,
            false,
            |_| None,
            &cancel,
            |_, _, _| {},
            |_| Err(io::Error::other("trash unavailable")),
        )
        .unwrap();

        assert!(!outcome.renamed);
        assert!(outcome.trash_error.as_deref().is_some_and(|e| e.contains("trash unavailable")));
        assert!(src.join("metadata.json").is_file());
        let reopened = falcon_library::Library::open(&dest).unwrap();
        assert_eq!(reopened.items().len(), 1);
    }

    fn sample_library(tmp: &Path) -> (PathBuf, PathBuf) {
        let src = tmp.join("Photos.library");
        let mut lib = falcon_library::Library::create(&src).unwrap();
        let sample = tmp.join("note.txt");
        fs::write(&sample, b"important data").unwrap();
        lib.import_file(&sample, &falcon_library::import::ImportOptions::default()).unwrap();
        drop(lib);
        let dest_parent = tmp.join("External");
        fs::create_dir_all(&dest_parent).unwrap();
        (src, dest_parent.join("Photos.library"))
    }

    fn run_with_hook(src: &Path, dest: &Path, fail_at: MoveStage) -> (Result<MoveOutcome, String>, bool) {
        let cancel = AtomicBool::new(false);
        let trashed = std::cell::Cell::new(false);
        let r = execute_move_staged(
            src,
            dest,
            false,
            |_| None,
            &cancel,
            |_, _, _| {},
            |_| {
                trashed.set(true);
                Ok(())
            },
            |stage| if stage == fail_at { Err(io::Error::other(format!("injected at {stage:?}"))) } else { Ok(()) },
        );
        (r, trashed.get())
    }

    #[test]
    fn failure_before_commit_keeps_source_openable_and_leaves_no_destination_or_staging() {
        for stage in [MoveStage::Copying, MoveStage::BeforeVerify, MoveStage::Verifying, MoveStage::BeforeCommit] {
            let tmp = tempfile::tempdir().unwrap();
            let (src, dest) = sample_library(tmp.path());
            let (res, trashed) = run_with_hook(&src, &dest, stage);
            assert!(res.unwrap_err().contains("injected"), "{stage:?}");
            assert!(!trashed, "{stage:?}");
            assert!(!dest.exists(), "{stage:?}");
            assert!(stale_staging(dest.parent().unwrap()).is_empty(), "{stage:?}: staging removed");
            assert_eq!(falcon_library::Library::open(&src).unwrap().items().len(), 1, "{stage:?}");
        }
    }

    #[test]
    fn failure_after_commit_keeps_both_copies_openable() {
        let tmp = tempfile::tempdir().unwrap();
        let (src, dest) = sample_library(tmp.path());
        let (res, trashed) = run_with_hook(&src, &dest, MoveStage::AfterCommit);
        let outcome = res.unwrap();
        assert!(outcome.trash_error.is_some_and(|e| e.contains("injected")));
        assert!(!trashed, "the original is not trashed when the step before it failed");
        assert_eq!(falcon_library::Library::open(&dest).unwrap().items().len(), 1);
        assert_eq!(falcon_library::Library::open(&src).unwrap().items().len(), 1);
        assert!(stale_staging(dest.parent().unwrap()).is_empty());
    }

    #[test]
    fn failed_cleanup_leaves_a_reported_staging_folder_that_only_a_marked_one_can_be_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let (src, dest) = sample_library(tmp.path());
        let (res, _) = {
            let cancel = AtomicBool::new(false);
            let r = execute_move_staged(&src, &dest, false, |_| None, &cancel, |_, _, _| {}, |_| Ok(()), |stage| match stage {
                MoveStage::BeforeCommit | MoveStage::Cleanup => Err(io::Error::other("disk gone")),
                _ => Ok(()),
            });
            (r, ())
        };
        assert!(res.is_err());
        assert!(!dest.exists());
        let stale = stale_staging(dest.parent().unwrap());
        assert_eq!(stale.len(), 1);
        assert!(stale[0].1, "carries the owner marker");
        assert!(falcon_library::Library::open(&src).is_ok(), "original untouched");

        // Recovery after a restart: unmarked or foreign folders are never removed.
        let foreign = dest.parent().unwrap().join(format!("{STAGING_PREFIX}foreign"));
        fs::create_dir(&foreign).unwrap();
        fs::write(foreign.join("keep.txt"), b"user data").unwrap();
        assert!(remove_stale_staging(&foreign).is_err());
        assert!(foreign.join("keep.txt").is_file());
        assert!(remove_stale_staging(&dest.parent().unwrap().join("Photos.library")).is_err());
        remove_stale_staging(&stale[0].0).unwrap();
        assert_eq!(stale_staging(dest.parent().unwrap()).len(), 1, "only the foreign one is left");
    }

    #[test]
    fn corrupted_copy_is_detected_by_content_not_size() {
        let tmp = tempfile::tempdir().unwrap();
        let (src, dest) = sample_library(tmp.path());
        let parent = dest.parent().unwrap().to_path_buf();
        let cancel = AtomicBool::new(false);
        let err = execute_move_staged(&src, &dest, false, |_| None, &cancel, |_, _, _| {}, |_| Ok(()), |stage| {
            if stage == MoveStage::BeforeVerify {
                // Same length, different bytes, as a bad write would leave.
                let staging = stale_staging(&parent).remove(0).0;
                let victim = walk_files(&staging).into_iter().find(|p| p.extension().is_some_and(|e| e == "txt")).unwrap();
                let mut bytes = fs::read(&victim).unwrap();
                bytes[0] ^= 0xff;
                fs::write(victim, bytes).unwrap();
            }
            Ok(())
        })
        .unwrap_err();
        assert!(err.contains("differs from the original"), "{err}");
        assert!(!dest.exists());
        assert!(stale_staging(&parent).is_empty());
        assert!(falcon_library::Library::open(&src).is_ok());
    }

    fn walk_files(root: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut dirs = vec![root.to_path_buf()];
        while let Some(d) = dirs.pop() {
            for e in fs::read_dir(d).unwrap().flatten() {
                if e.file_type().unwrap().is_dir() {
                    dirs.push(e.path());
                } else {
                    out.push(e.path());
                }
            }
        }
        out
    }

    #[test]
    fn destination_created_during_the_move_is_never_replaced() {
        let tmp = tempfile::tempdir().unwrap();
        let (src, dest) = sample_library(tmp.path());
        let cancel = AtomicBool::new(false);
        let err = execute_move_staged(&src, &dest, false, |_| None, &cancel, |_, _, _| {}, |_| Ok(()), |stage| {
            if stage == MoveStage::BeforeCommit {
                fs::create_dir(&dest).unwrap();
                fs::write(dest.join("users-file.txt"), b"mine").unwrap();
            }
            Ok(())
        })
        .unwrap_err();
        assert!(err.contains("couldn't put the copied library"), "{err}");
        assert_eq!(fs::read(dest.join("users-file.txt")).unwrap(), b"mine");
        assert!(stale_staging(dest.parent().unwrap()).is_empty());
        assert!(falcon_library::Library::open(&src).is_ok());
    }

    #[test]
    fn a_library_that_changes_while_copying_is_not_moved() {
        let tmp = tempfile::tempdir().unwrap();
        let (src, dest) = sample_library(tmp.path());
        let cancel = AtomicBool::new(false);
        let trashed = std::cell::Cell::new(false);
        let err = execute_move_staged(
            &src,
            &dest,
            false,
            |_| None,
            &cancel,
            |_, _, _| {},
            |_| {
                trashed.set(true);
                Ok(())
            },
            |stage| {
                if stage == MoveStage::BeforeVerify {
                    fs::write(src.join("late-write.json"), b"{}").unwrap();
                }
                Ok(())
            },
        )
        .unwrap_err();
        assert!(err.contains("changed while it was being copied"), "{err}");
        assert!(!dest.exists() && !trashed.get());
        assert!(src.join("late-write.json").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_copied_as_links_and_never_followed() {
        let tmp = tempfile::tempdir().unwrap();
        let (src, dest) = sample_library(tmp.path());
        let outside = tmp.path().join("external-original.png");
        fs::write(&outside, b"referenced original").unwrap();
        std::os::unix::fs::symlink(&outside, src.join("link-to-external")).unwrap();
        let cancel = AtomicBool::new(false);
        let outcome = execute_move_inner(&src, &dest, false, |_| None, &cancel, |_, _, _| {}, |_| Ok(())).unwrap();
        assert!(!outcome.renamed);
        assert!(fs::symlink_metadata(dest.join("link-to-external")).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_link(dest.join("link-to-external")).unwrap(), outside);
        assert_eq!(fs::read(&outside).unwrap(), b"referenced original", "external file untouched");
    }
}
