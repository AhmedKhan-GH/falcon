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

/// Moves `src` to `dest`. Tries an atomic same-volume rename first; if that
/// fails (e.g. across volumes to an external SSD), copies recursively in chunks
/// with progress and cancellation, verifies the copy, and sends `src` to
/// `trasher`.
pub fn execute_move(
    src: &Path,
    dest: &Path,
    cancel: &AtomicBool,
    progress: impl FnMut(usize, u64, &str),
    trasher: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<MoveOutcome, String> {
    execute_move_inner(
        src,
        dest,
        true,
        crate::storage::available_space,
        cancel,
        progress,
        trasher,
    )
}

fn execute_move_inner(
    src: &Path,
    dest: &Path,
    allow_rename: bool,
    free_space: impl Fn(&Path) -> Option<u64>,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, u64, &str),
    trasher: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<MoveOutcome, String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("cancelled".to_string());
    }
    if allow_rename && fs::rename(src, dest).is_ok() {
        return Ok(MoveOutcome { renamed: true, trash_error: None });
    }
    if let Some(parent) = dest.parent()
        && let Some(avail) = free_space(parent)
    {
        let stats = measure_library(src, cancel).map_err(|err| {
            if cancel.load(Ordering::Relaxed) {
                "cancelled".to_string()
            } else {
                err.to_string()
            }
        })?;
        ensure_free_space(avail, stats.bytes)?;
    }
    let mut files_done = 0usize;
    let mut bytes_done = 0u64;
    if let Err(err) = copy_dir_cancellable(src, dest, cancel, &mut files_done, &mut bytes_done, &mut progress) {
        let _ = fs::remove_dir_all(dest);
        return Err(if cancel.load(Ordering::Relaxed) { "cancelled".to_string() } else { err.to_string() });
    }
    if let Err(err) = verify_copied_library(src, dest) {
        let _ = fs::remove_dir_all(dest);
        return Err(err);
    }
    let trash_error = trasher(src).err().map(|e| e.to_string());
    Ok(MoveOutcome { renamed: false, trash_error })
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

fn copy_dir_cancellable(
    src: &Path,
    dest: &Path,
    cancel: &AtomicBool,
    files_done: &mut usize,
    bytes_done: &mut u64,
    progress: &mut impl FnMut(usize, u64, &str),
) -> io::Result<()> {
    fs::create_dir_all(dest)?;
    let mut buf = vec![0u8; COPY_BUF_BYTES];
    copy_dir_inner(src, dest, cancel, files_done, bytes_done, &mut buf, progress)
}

fn copy_dir_inner(
    src: &Path,
    dest: &Path,
    cancel: &AtomicBool,
    files_done: &mut usize,
    bytes_done: &mut u64,
    buf: &mut [u8],
    progress: &mut impl FnMut(usize, u64, &str),
) -> io::Result<()> {
    for entry in fs::read_dir(src)? {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let entry = entry?;
        let name = entry.file_name();
        let src_path = entry.path();
        let dest_path = dest.join(&name);
        let meta = fs::symlink_metadata(&src_path)?;
        if meta.file_type().is_symlink() {
            copy_symlink(&src_path, &dest_path)?;
            *files_done += 1;
            progress(*files_done, *bytes_done, &name.to_string_lossy());
        } else if meta.is_dir() {
            fs::create_dir_all(&dest_path)?;
            copy_dir_inner(&src_path, &dest_path, cancel, files_done, bytes_done, buf, progress)?;
        } else {
            let label = name.to_string_lossy().into_owned();
            progress(*files_done, *bytes_done, &label);
            copy_file_chunked(&src_path, &dest_path, cancel, buf, |chunk| {
                *bytes_done += chunk;
                progress(*files_done, *bytes_done, &label);
            })?;
            *files_done += 1;
            progress(*files_done, *bytes_done, &label);
        }
    }
    Ok(())
}

fn copy_file_chunked(
    src: &Path,
    dest: &Path,
    cancel: &AtomicBool,
    buf: &mut [u8],
    mut on_chunk: impl FnMut(u64),
) -> io::Result<()> {
    let mut reader = File::open(src)?;
    let mut writer = File::create(dest)?;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let n = reader.read(buf)?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n])?;
        on_chunk(n as u64);
    }
    writer.flush()?;
    if let Ok(meta) = fs::metadata(src) {
        let _ = fs::set_permissions(dest, meta.permissions());
    }
    Ok(())
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
}
