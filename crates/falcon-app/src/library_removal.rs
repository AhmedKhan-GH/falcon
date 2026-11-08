//! The rule for Remove from Disk, a falcon addition (Eagle has no such
//! command): when a library folder may be moved to the Trash.
//!
//! [`check`] is one pure function (stat calls only) that says OK or names
//! the reason for refusing. [`trash_library`] re-runs it right before it
//! hands the folder to a trasher, so a change since the dialog opened is
//! caught. Nothing here deletes: a trasher that fails leaves everything
//! as it was.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::logic::is_library_path;

/// Why a library may not be removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// It can't be found or read.
    Missing,
    /// It, or a folder above it, is a symbolic link, or its path has a `..`.
    Symlink,
    NotDirectory,
    /// Its name doesn't end in `.library`.
    NotLibrary,
    /// It has no `metadata.json`.
    NoMetadata,
    /// It is open or being opened.
    Open,
    /// It lies inside an open library.
    InsideOpen,
    /// It contains an open library.
    ContainsOpen,
}

/// How a trash attempt ended without moving anything.
#[derive(Debug)]
pub enum TrashError {
    Refused(Refusal),
    Failed(io::Error),
}

fn canon(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// Whether `a` and `b` are one folder by device and inode.
#[cfg(unix)]
fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

/// Whether `a` and `b` are one folder. Resolving a path on Windows sees
/// through junctions, links, short names and case, and a folder can't have
/// hard links, so one folder has one resolved path.
#[cfg(windows)]
fn same_file(a: &Path, b: &Path) -> bool {
    matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

/// Whether `path` is open, inside an open library, or holds one.
fn open_conflict(path: &Path, canonical: &Path, open: &[PathBuf]) -> Option<Refusal> {
    for o in open {
        let oc = canon(o);
        if oc == canonical || o == path || same_file(o, path) {
            return Some(Refusal::Open);
        }
        if canonical.starts_with(&oc) || path.starts_with(o) {
            return Some(Refusal::InsideOpen);
        }
        if oc.starts_with(canonical) || o.starts_with(path) {
            return Some(Refusal::ContainsOpen);
        }
    }
    None
}

/// Whether `path` may be moved to the Trash.
///
/// `open` lists every open or loading library. The checks run in this
/// order and the first failure is returned: the folder (a real directory
/// named `*.library` with a `metadata.json`, reached through no symbolic
/// link), then the open libraries.
pub fn check(path: &Path, open: &[PathBuf]) -> Result<(), Refusal> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| Refusal::Missing)?;
    let canonical = path.canonicalize().map_err(|_| Refusal::Missing)?;
    if meta.file_type().is_symlink() {
        return Err(Refusal::Symlink);
    }
    // Every folder above it must be a real one too, or the path trashed
    // would not be the folder shown.
    let abs = std::path::absolute(path).map_err(|_| Refusal::Missing)?;
    if abs.components().any(|c| c == std::path::Component::ParentDir) {
        return Err(Refusal::Symlink);
    }
    if abs.ancestors().any(|a| std::fs::symlink_metadata(a).is_ok_and(|m| m.file_type().is_symlink())) {
        return Err(Refusal::Symlink);
    }
    if !meta.is_dir() {
        return Err(Refusal::NotDirectory);
    }
    if !is_library_path(&abs) {
        return Err(Refusal::NotLibrary);
    }
    if !std::fs::symlink_metadata(path.join("metadata.json")).is_ok_and(|m| m.is_file()) {
        return Err(Refusal::NoMetadata);
    }
    match open_conflict(path, &canonical, open) {
        Some(r) => Err(r),
        None => Ok(()),
    }
}

/// Whether `path` is one of the open libraries, or inside or around one.
pub fn is_open(path: &Path, open: &[PathBuf]) -> bool {
    open_conflict(path, &canon(path), open).is_some()
}

/// Runs the whole rule again, then hands `path` to `trasher`. A refusal or
/// a failed trasher leaves the folder where it is; nothing is deleted.
pub fn trash_library(path: &Path, open: &[PathBuf], trasher: impl FnOnce(&Path) -> io::Result<()>) -> Result<(), TrashError> {
    check(path, open).map_err(TrashError::Refused)?;
    trasher(path).map_err(TrashError::Failed)
}

/// The total size of the files under `path` in bytes. Walks with
/// `symlink_metadata`, so a symbolic link counts as itself and is never
/// followed, and never reads a file's contents (an iCloud placeholder
/// stays a placeholder). Stops with an error when `stop` is set or any
/// entry can't be read.
pub fn folder_size(path: &Path, stop: &AtomicBool) -> io::Result<u64> {
    let mut total = 0u64;
    let mut dirs = vec![path.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir)? {
            if stop.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "stopped"));
            }
            let entry = entry?;
            let meta = std::fs::symlink_metadata(entry.path())?;
            if meta.is_dir() {
                dirs.push(entry.path());
            } else {
                total += meta.len();
            }
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    use super::*;

    /// A junction, the link to a folder Windows lets anyone make; `is_symlink`
    /// is true of it, as of a symbolic link.
    #[cfg(windows)]
    fn symlink(target: impl AsRef<Path>, link: impl AsRef<Path>) -> io::Result<()> {
        let plain = |p: &Path| falcon_library::paths::shown(p);
        let out = std::process::Command::new("cmd").arg("/C").arg("mklink").arg("/J").arg(plain(link.as_ref())).arg(plain(target.as_ref())).output()?;
        if out.status.success() { Ok(()) } else { Err(io::Error::other(String::from_utf8_lossy(&out.stderr).into_owned())) }
    }

    struct Env {
        _tmp: tempfile::TempDir,
        /// The folder the test libraries are in.
        dir: PathBuf,
        lib: PathBuf,
    }

    fn make_lib(p: &Path) {
        std::fs::create_dir_all(p.join("images")).unwrap();
        std::fs::write(p.join("metadata.json"), "{}").unwrap();
    }

    fn env() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let dir = base.join("Libraries");
        let lib = dir.join("A.library");
        make_lib(&lib);
        Env { _tmp: tmp, dir, lib }
    }

    #[test]
    fn a_plain_closed_library_is_allowed() {
        let e = env();
        assert_eq!(check(&e.lib, &[]), Ok(()));
        let other = e.dir.join("B.library");
        make_lib(&other);
        assert_eq!(check(&e.lib, &[other]), Ok(()));
    }

    #[test]
    fn the_folder_holding_libraries_refuses() {
        let e = env();
        assert_eq!(check(&e.dir, &[]), Err(Refusal::NotLibrary));
        assert_eq!(check(e.dir.parent().unwrap(), &[]), Err(Refusal::NotLibrary));
    }

    #[test]
    fn a_symlink_to_a_library_refuses() {
        let e = env();
        let link = e.dir.join("Link.library");
        symlink(&e.lib, &link).unwrap();
        assert_eq!(check(&link, &[]), Err(Refusal::Symlink));
    }

    #[test]
    fn a_library_reached_through_a_linked_folder_refuses() {
        let e = env();
        let group = e.dir.join("Group");
        std::fs::create_dir_all(&group).unwrap();
        symlink(&e.lib, group.join("C.library")).unwrap();
        assert_eq!(check(&group.join("C.library"), &[]), Err(Refusal::Symlink));
        let via = e.dir.join("ViaLink");
        symlink(&group, &via).unwrap();
        make_lib(&group.join("D.library"));
        assert_eq!(check(&via.join("D.library"), &[]), Err(Refusal::Symlink));
    }

    #[test]
    fn a_file_refuses() {
        let e = env();
        let f = e.dir.join("F.library");
        std::fs::write(&f, "x").unwrap();
        assert_eq!(check(&f, &[]), Err(Refusal::NotDirectory));
    }

    #[test]
    fn a_folder_without_metadata_refuses() {
        let e = env();
        let d = e.dir.join("N.library");
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(check(&d, &[]), Err(Refusal::NoMetadata));
        // A folder named metadata.json is not the file.
        std::fs::create_dir_all(d.join("metadata.json")).unwrap();
        assert_eq!(check(&d, &[]), Err(Refusal::NoMetadata));
    }

    #[test]
    fn a_wrong_extension_refuses() {
        let e = env();
        let d = e.dir.join("Photos");
        make_lib(&d);
        assert_eq!(check(&d, &[]), Err(Refusal::NotLibrary));
    }

    #[test]
    fn a_missing_path_refuses() {
        let e = env();
        assert_eq!(check(&e.dir.join("Gone.library"), &[]), Err(Refusal::Missing));
    }

    #[test]
    fn an_open_library_refuses_by_path_and_by_spelling() {
        let e = env();
        assert_eq!(check(&e.lib, std::slice::from_ref(&e.lib)), Err(Refusal::Open));
        let dotted = e.dir.join("Sub/../A.library");
        std::fs::create_dir_all(e.dir.join("Sub")).unwrap();
        assert_eq!(check(&e.lib, &[dotted]), Err(Refusal::Open));
        let mut trailing = e.lib.clone().into_os_string();
        trailing.push(std::path::MAIN_SEPARATOR_STR);
        assert_eq!(check(&e.lib, &[PathBuf::from(trailing)]), Err(Refusal::Open));
    }

    #[cfg(windows)]
    #[test]
    fn an_open_library_refuses_however_its_path_is_written() {
        let e = env();
        let plain = falcon_library::paths::shown(&e.lib);
        let upper = PathBuf::from(plain.to_str().unwrap().to_uppercase());
        let forward = PathBuf::from(plain.to_str().unwrap().replace('\\', "/"));
        for spelling in [plain.clone(), upper, forward] {
            assert_eq!(check(&e.lib, std::slice::from_ref(&spelling)), Err(Refusal::Open), "{spelling:?}");
            assert_eq!(check(&spelling, std::slice::from_ref(&e.lib)), Err(Refusal::Open), "{spelling:?}");
        }
    }

    #[test]
    fn an_open_library_refuses_through_a_symlink() {
        let e = env();
        let link = e.dir.join("Elsewhere.library");
        symlink(&e.lib, &link).unwrap();
        assert_eq!(check(&e.lib, &[link]), Err(Refusal::Open));
    }

    #[test]
    fn nesting_with_an_open_library_refuses() {
        let e = env();
        let inner = e.lib.join("Inner.library");
        make_lib(&inner);
        // The open one is inside the candidate.
        assert_eq!(check(&e.lib, std::slice::from_ref(&inner)), Err(Refusal::ContainsOpen));
        // The candidate is inside the open one.
        assert_eq!(check(&inner, std::slice::from_ref(&e.lib)), Err(Refusal::InsideOpen));
    }

    #[test]
    fn trashing_runs_the_trasher_once_when_allowed() {
        let e = env();
        let seen = Cell::new(0);
        let got = trash_library(&e.lib, &[], |p| {
            assert_eq!(p, e.lib);
            seen.set(seen.get() + 1);
            Ok(())
        });
        assert!(got.is_ok());
        assert_eq!(seen.get(), 1);
        assert!(e.lib.exists(), "the fake trasher moves nothing");
    }

    #[test]
    fn a_refusal_never_reaches_the_trasher() {
        let e = env();
        let seen = Cell::new(false);
        let got = trash_library(&e.lib, std::slice::from_ref(&e.lib), |_| {
            seen.set(true);
            Ok(())
        });
        assert!(matches!(got, Err(TrashError::Refused(Refusal::Open))));
        assert!(!seen.get());
    }

    #[test]
    fn the_rule_runs_again_when_the_state_changed() {
        let e = env();
        // Fine when the dialog opened.
        assert_eq!(check(&e.lib, &[]), Ok(()));
        // Then the library is opened, or its metadata goes, or it turns into a link.
        let seen = Cell::new(false);
        let trasher = |_: &Path| {
            seen.set(true);
            Ok(())
        };
        assert!(matches!(trash_library(&e.lib, std::slice::from_ref(&e.lib), trasher), Err(TrashError::Refused(Refusal::Open))));
        std::fs::remove_file(e.lib.join("metadata.json")).unwrap();
        assert!(matches!(trash_library(&e.lib, &[], trasher), Err(TrashError::Refused(Refusal::NoMetadata))));
        let real = e.dir.parent().unwrap().join("Moved");
        std::fs::rename(&e.lib, &real).unwrap();
        make_lib(&real);
        symlink(&real, &e.lib).unwrap();
        assert!(matches!(trash_library(&e.lib, &[], trasher), Err(TrashError::Refused(Refusal::Symlink))));
        assert!(!seen.get());
    }

    #[test]
    fn a_failed_trasher_is_reported_and_leaves_the_folder() {
        let e = env();
        let got = trash_library(&e.lib, &[], |_| Err(io::Error::other("no Trash")));
        assert!(matches!(got, Err(TrashError::Failed(_))));
        assert!(e.lib.join("metadata.json").exists());
    }

    #[test]
    fn is_open_sees_the_same_cases() {
        let e = env();
        assert!(!is_open(&e.lib, &[]));
        assert!(is_open(&e.lib, std::slice::from_ref(&e.lib)));
    }

    #[test]
    fn the_size_walk_adds_files_and_skips_links() {
        let e = env();
        std::fs::write(e.lib.join("images/a.bin"), vec![0u8; 1000]).unwrap();
        std::fs::write(e.lib.join("b.bin"), vec![0u8; 24]).unwrap();
        let own = folder_size(&e.lib, &AtomicBool::new(false)).unwrap();
        assert_eq!(own, 1024 + 2, "the two files and metadata.json");
        // A link to a big folder outside adds only itself.
        let big = e.dir.parent().unwrap().join("big");
        std::fs::create_dir_all(&big).unwrap();
        std::fs::write(big.join("huge.bin"), vec![0u8; 100_000]).unwrap();
        symlink(&big, e.lib.join("link")).unwrap();
        let with_link = folder_size(&e.lib, &AtomicBool::new(false)).unwrap();
        assert!(with_link < own + 1000, "the linked folder was followed: {with_link}");
    }

    #[test]
    fn the_size_walk_fails_when_stopped_or_unreadable() {
        let e = env();
        assert!(folder_size(&e.lib, &AtomicBool::new(true)).is_err());
        assert!(folder_size(&e.dir.join("Gone"), &AtomicBool::new(false)).is_err());
    }
}
