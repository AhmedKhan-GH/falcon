//! Keeps falcon away from real libraries while it is in development.
//!
//! The app and the CLI call [`enforce`] at startup. After that, [`check`]
//! only accepts libraries inside the sandbox folder (`~/Falcon Sandbox`,
//! or `$FALCON_SANDBOX`), so a real Eagle library is never opened, written
//! or switched to by mistake. Copy a library into the sandbox to work on it.
//! Code that never calls `enforce`, such as tests, is unaffected.

use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

/// Environment variable that moves the sandbox folder.
pub const ENV: &str = "FALCON_SANDBOX";

static ROOT: OnceLock<PathBuf> = OnceLock::new();

/// The sandbox folder: `$FALCON_SANDBOX`, or `Falcon Sandbox` in the home folder.
pub fn default_root() -> PathBuf {
    if let Some(p) = std::env::var_os(ENV).filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    home_dir().unwrap_or_else(|| PathBuf::from(".")).join("Falcon Sandbox")
}

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

/// Restricts every later [`check`] to libraries inside `root`, creating it
/// if needed. Only the first call has any effect.
pub fn enforce(root: PathBuf) {
    let _ = std::fs::create_dir_all(&root);
    let root = root.canonicalize().unwrap_or(root);
    let _ = ROOT.set(root);
}

/// The enforced sandbox folder, if any.
pub fn root() -> Option<&'static Path> {
    ROOT.get().map(PathBuf::as_path)
}

/// Whether falcon may open or create a library at `path`.
pub fn check(path: &Path) -> Result<(), String> {
    match root() {
        Some(root) if !inside(root, path) => Err(format!(
            "Falcon only opens libraries in {} while it is in development. Copy the library there to work on it.",
            shown(root).display()
        )),
        _ => Ok(()),
    }
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

/// [`check`] as a library [`Error`](crate::Error).
pub fn guard(path: &Path) -> crate::Result<()> {
    check(path).map_err(crate::Error::Sandbox)
}

/// Whether `path` (which may not exist yet) lies inside `root`, after
/// resolving symlinks and `..` so neither can lead outside it.
fn inside(root: &Path, path: &Path) -> bool {
    // A relative path is relative to the working directory.
    let Ok(mut existing) = std::path::absolute(path) else { return false };
    if existing.components().any(|c| matches!(c, Component::Normal(name) if ambiguous_name(name))) {
        return false;
    }
    let mut rest = Vec::new();
    while !existing.exists() {
        match (existing.file_name().map(ToOwned::to_owned), existing.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                existing = parent.to_path_buf();
            }
            _ => return false,
        }
    }
    let Ok(mut resolved) = existing.canonicalize() else { return false };
    for name in rest.into_iter().rev() {
        if name == ".." {
            return false;
        }
        resolved.push(name);
    }
    resolved != root && resolved.starts_with(root)
}

/// Whether a path's folder or file name means something other than itself: on
/// Windows a colon opens a stream, and a name of only dots and spaces is
/// dropped or kept depending on where in the path it is.
fn ambiguous_name(name: &std::ffi::OsStr) -> bool {
    let name = name.to_string_lossy();
    cfg!(windows) && (name.contains(':') || name.trim_matches(['.', ' ']).is_empty())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::inside;

    #[test]
    fn only_paths_inside_the_root_pass() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Sandbox");
        std::fs::create_dir_all(root.join("A.library")).unwrap();
        std::fs::create_dir_all(tmp.path().join("Real.library")).unwrap();
        let root = root.canonicalize().unwrap();
        assert!(inside(&root, &root.join("A.library")));
        assert!(inside(&root, &root.join("New.library")));
        assert!(!inside(&root, &root));
        assert!(!inside(&root, &tmp.path().join("Real.library")));
        assert!(!inside(&root, &root.join("../Real.library")));
        assert!(!inside(&root, &root.join("Missing/../../Real.library")));
        assert!(!inside(&root, std::path::Path::new("")));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(tmp.path().join("Real.library"), root.join("Link.library")).unwrap();
            assert!(!inside(&root, &root.join("Link.library")));
        }
    }

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
    /// not be what decides where the sandbox is.
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

    /// Windows spellings of one path all resolve to one answer, however the
    /// path is written, and none of them gets out of the sandbox.
    #[cfg(windows)]
    #[test]
    fn windows_spellings_neither_escape_nor_falsely_refuse() {
        use std::path::{Path, PathBuf};
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Sandbox");
        std::fs::create_dir_all(root.join("A.library")).unwrap();
        std::fs::create_dir_all(tmp.path().join("Real.library")).unwrap();
        std::fs::write(tmp.path().join("Real.library").join("metadata.json"), "{}").unwrap();
        let root = root.canonicalize().unwrap();
        // The plain form of the root, as a user or a file dialog writes it.
        let plain = super::shown(&root);
        let s = plain.to_str().unwrap().to_string();
        let real = super::shown(&tmp.path().canonicalize().unwrap()).join("Real.library");

        let allowed: Vec<PathBuf> = vec![
            plain.join("A.library"),
            plain.join("New.library"),
            root.join("A.library"),
            root.join("New.library"),
            // Case doesn't matter on NTFS, for existing folders or new ones under them.
            PathBuf::from(s.to_uppercase()).join("a.LIBRARY"),
            PathBuf::from(s.to_lowercase()).join("New.library"),
            // Forward slashes are separators.
            PathBuf::from(s.replace('\\', "/") + "/A.library"),
            PathBuf::from(format!(r"{s}\.\A.library")),
            // The system drops trailing dots and spaces, so this is A.library.
            PathBuf::from(format!(r"{s}\A.library.")),
            PathBuf::from(format!(r"{s}\A.library ")),
            PathBuf::from(format!(r"{s}\Sub\..\A.library")),
        ];
        for p in &allowed {
            assert!(inside(&root, p), "falsely refused {p:?}");
        }

        let real_text = real.to_str().unwrap();
        let refused: Vec<PathBuf> = vec![
            root.clone(),
            plain.clone(),
            PathBuf::from(s.to_uppercase()),
            PathBuf::from(format!(r"{s}\")),
            PathBuf::from(format!(r"{s}\.")),
            PathBuf::from(format!(r"{s}.")),
            real.clone(),
            PathBuf::from(real_text.to_uppercase()),
            PathBuf::from(format!(r"\\?\{real_text}")),
            PathBuf::from(format!(r"\\.\{real_text}")),
            PathBuf::from(format!(r"\\?\{s}\..\Real.library")),
            PathBuf::from(format!(r"\\?\{s}\New\..\..\Real.library")),
            PathBuf::from(format!(r"\\?\{s}\New/../../Real.library")),
            plain.join(r"..\Real.library"),
            plain.join("../Real.library"),
            plain.join(r"New\..\..\Real.library"),
            PathBuf::from(format!(r"{s}\.. \Real.library")),
            PathBuf::from(format!(r"{s}\..  \Real.library")),
            PathBuf::from(format!(r"{s}\...\Real.library")),
            PathBuf::from(format!(r"{s}\.. .\Real.library")),
            // An absolute path pushed on a sandbox path replaces it.
            plain.join(&real),
            // A sibling that merely starts with the root's name.
            PathBuf::from(format!(r"{s} Other\A.library")),
            PathBuf::from(format!(r"{s}:stream\A.library")),
            Path::new(&s[..2]).join("Real.library"),
        ];
        for p in &refused {
            assert!(!inside(&root, p), "let through {p:?}");
        }

        // A junction needs no privilege, so it is the link anyone can make.
        let link = root.join("Link.library");
        junction(&real, &link);
        assert!(!inside(&root, &link));
        assert!(!inside(&root, &plain.join("Link.library")));
        assert!(!inside(&root, &plain.join("Link.library").join("metadata.json")));
        assert!(!inside(&root, &plain.join("LINK.LIBRARY").join("New")));
        // And a root reached through a junction is the same root.
        let alias = tmp.path().join("Alias");
        junction(&plain, &alias);
        assert!(inside(&root, &alias.join("A.library")));
        assert!(!inside(&root, &alias));
    }
}
