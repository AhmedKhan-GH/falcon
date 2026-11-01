//! The watched auto-import folder (Preferences ▸ Auto-Import), after
//! Eagle's `auto-import.js`: files added to a folder are imported into the
//! open library by themselves.
//!
//! As in Eagle, each library watches a folder named after it inside the
//! chosen one (`<chosen>/<library name without .library>`), so one chosen
//! folder serves every library. Network volumes are refused.

use std::path::{Path, PathBuf};

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
