//! Where the user's folders are, and paths written the way a reader expects.

use std::path::{Path, PathBuf};

/// The user's home folder: `HOME`, which a normal Windows launch doesn't set.
#[cfg(not(windows))]
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// The user's home folder: `USERPROFILE`, or the system's answer without it.
#[cfg(windows)]
pub fn home_dir() -> Option<PathBuf> {
    std::env::home_dir()
}

/// `%LOCALAPPDATA%`, for what belongs to this machine and user.
#[cfg(windows)]
pub fn local_data_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").filter(|d| !d.is_empty()).map(PathBuf::from).or_else(|| Some(home_dir()?.join("AppData").join("Local")))
}

/// `path` without the `\\?\` prefix `canonicalize` gives it on Windows, which
/// means nothing to a reader or to most programs. Other systems have none.
pub fn shown(path: &Path) -> PathBuf {
    let text = path.as_os_str().to_string_lossy();
    if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{unc}"))
    } else if let Some(disk) = text.strip_prefix(r"\\?\").filter(|p| p.as_bytes().get(1) == Some(&b':')) {
        PathBuf::from(disk)
    } else {
        path.to_path_buf()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    #[test]
    fn verbatim_prefixes_are_dropped_for_showing() {
        use super::shown;
        use std::path::Path;
        assert_eq!(shown(Path::new("/home/me/a b")), Path::new("/home/me/a b"));
        assert_eq!(shown(Path::new(r"C:\me")), Path::new(r"C:\me"));
        assert_eq!(shown(Path::new(r"\\?\C:\me\a")), Path::new(r"C:\me\a"));
        assert_eq!(shown(Path::new(r"\\?\UNC\host\share\a")), Path::new(r"\\host\share\a"));
        assert_eq!(shown(Path::new(r"\\?\Volume{1}\a")), Path::new(r"\\?\Volume{1}\a"));
    }

    /// `HOME` is what Git Bash sets and a normal launch doesn't, so it must
    /// not be what decides where the user's folders are.
    #[cfg(windows)]
    #[test]
    fn the_windows_home_folder_is_the_user_profile() {
        assert_eq!(super::home_dir().map(std::path::PathBuf::into_os_string), std::env::var_os("USERPROFILE"));
    }

    /// A junction, the link to a folder Windows lets anyone make.
    #[cfg(windows)]
    pub(crate) fn junction(target: &std::path::Path, link: &std::path::Path) {
        let made = std::process::Command::new("cmd").arg("/C").arg("mklink").arg("/J").arg(super::shown(link)).arg(super::shown(target)).output().unwrap();
        assert!(made.status.success(), "{made:?}");
    }
}
