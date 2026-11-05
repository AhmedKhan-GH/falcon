//! Checks of where a library is stored, run once on a background thread
//! when it opens, after Eagle's warnings: a `metadata.json` that can't be
//! written (read-only), a folder where nothing can be created (Eagle's
//! permission check makes and removes a `<ms>.tmp` folder in the library;
//! so does this), a slow disk, and an NTFS volume on macOS.
//!
//! Eagle's own slow-disk test isn't in its readable source; falcon calls a
//! disk slow when that create-and-remove round trip takes longer than
//! [`SLOW`].

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::locale::{tr, trf};

/// A round trip slower than this counts as a slow disk.
pub const SLOW: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, PartialEq)]
pub enum Warning {
    /// `metadata.json` can't be opened for writing.
    ReadOnly,
    /// Nothing can be created in the library folder (the reason).
    NoPermission(String),
    /// Creating and removing a folder took this long.
    SlowDisk(Duration),
    /// The library is on an NTFS volume (through a third-party driver).
    Ntfs,
}

impl Warning {
    pub fn message(&self) -> String {
        match self {
            Warning::ReadOnly => tr("Its metadata.json is read-only, so changes can't be saved. Check the file's permissions, or move the library somewhere you can write.").into(),
            Warning::NoPermission(why) => trf("Falcon can't create files in the library folder ({why}), so changes may not be saved. Check the folder's permissions.", &[("why", why)]),
            Warning::SlowDisk(d) => trf("The disk it is on is slow to respond ({seconds} s for a simple write), so thumbnails and saving may lag. Check the drive or its connection.", &[("seconds", &format!("{:.1}", d.as_secs_f32()))]),
            Warning::Ntfs => tr("It is on an NTFS drive. NTFS drivers on macOS are slow and less reliable; a drive formatted as APFS or exFAT works better.").into(),
        }
    }

    /// Whether saving is at risk (Eagle shows a dialog), rather than speed
    /// (Eagle shows a notice).
    pub fn is_serious(&self) -> bool {
        matches!(self, Warning::ReadOnly | Warning::NoPermission(_))
    }
}

/// Runs the checks for `root`.
pub fn check(root: &Path) -> Vec<Warning> {
    let mut out = Vec::new();
    if std::fs::OpenOptions::new().append(true).open(root.join("metadata.json")).is_err_and(|e| e.kind() == std::io::ErrorKind::PermissionDenied) {
        out.push(Warning::ReadOnly);
    }
    let probe = probe_dir(root);
    let start = Instant::now();
    match std::fs::create_dir(&probe).and_then(|()| std::fs::remove_dir(&probe)) {
        Ok(()) => {
            let took = start.elapsed();
            if took > SLOW {
                out.push(Warning::SlowDisk(took));
            }
        }
        Err(e) => out.push(Warning::NoPermission(e.to_string())),
    }
    if platform::volume_type(root).is_some_and(|t| is_ntfs(&t)) {
        out.push(Warning::Ntfs);
    }
    out
}

/// Checks `root` on a background thread; the result arrives once.
pub fn spawn_check(root: PathBuf, wake: impl Fn() + Send + 'static) -> Receiver<Vec<Warning>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(check(&root));
        wake();
    });
    rx
}

fn probe_dir(root: &Path) -> PathBuf {
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
    root.join(format!("{ms}.tmp"))
}

/// Volume type names macOS reports for NTFS (Apple's read-only driver,
/// and third-party ones such as Paragon's and Tuxera's).
fn is_ntfs(type_name: &str) -> bool {
    type_name.to_ascii_lowercase().contains("ntfs")
}

#[cfg(target_os = "macos")]
mod platform {
    use std::path::Path;

    use objc2_foundation::{NSString, NSURL, NSURLVolumeTypeNameKey};

    /// The file system type of the volume holding `path` ("apfs", "ntfs").
    pub fn volume_type(path: &Path) -> Option<String> {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        let mut value = None;
        // SAFETY: the key's value is an NSString (or nil).
        unsafe { url.getResourceValue_forKey_error(&mut value, NSURLVolumeTypeNameKey) }.ok()?;
        value?.downcast::<NSString>().ok().map(|s| s.to_string())
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    pub fn volume_type(_: &std::path::Path) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_healthy_folder_has_no_warnings() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = falcon_library::Library::create(tmp.path().join("H.library")).unwrap();
        assert_eq!(check(lib.root()), Vec::<Warning>::new());
        assert_eq!(std::fs::read_dir(lib.root()).unwrap().filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().ends_with(".tmp")).count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn read_only_files_and_folders_are_reported() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let lib = falcon_library::Library::create(tmp.path().join("R.library")).unwrap();
        let root = lib.root().to_path_buf();
        let meta = root.join("metadata.json");
        std::fs::set_permissions(&meta, std::fs::Permissions::from_mode(0o444)).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o555)).unwrap();
        let got = check(&root);
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&meta, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(got.contains(&Warning::ReadOnly), "{got:?}");
        assert!(got.iter().any(|w| matches!(w, Warning::NoPermission(_))), "{got:?}");
        assert!(got.iter().all(Warning::is_serious));
    }

    /// Windows ignores the attribute on a folder, so only the file is checked.
    #[cfg(windows)]
    #[test]
    fn a_read_only_metadata_file_is_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = falcon_library::Library::create(tmp.path().join("R.library")).unwrap();
        let meta = lib.root().join("metadata.json");
        let mut permissions = std::fs::metadata(&meta).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&meta, permissions.clone()).unwrap();
        let got = check(lib.root());
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        std::fs::set_permissions(&meta, permissions).unwrap();
        assert!(got.contains(&Warning::ReadOnly), "{got:?}");
    }

    #[test]
    fn ntfs_names() {
        assert!(is_ntfs("ntfs"));
        assert!(is_ntfs("ufsd_NTFS"));
        assert!(!is_ntfs("apfs"));
        assert!(!is_ntfs("exfat"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn reads_the_volume_type() {
        let t = platform::volume_type(&std::env::temp_dir());
        assert!(t.is_some_and(|t| !t.is_empty()));
    }
}
