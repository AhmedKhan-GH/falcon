//! Keeps falcon away from real libraries while it is in development.
//!
//! The app and the CLI call [`enforce`] at startup. After that, [`check`]
//! only accepts libraries inside the sandbox folder (`~/Falcon Sandbox`,
//! or `$FALCON_SANDBOX`), so a real Eagle library is never opened, written
//! or switched to by mistake. Copy a library into the sandbox to work on it.
//! Code that never calls `enforce`, such as tests, is unaffected.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Environment variable that moves the sandbox folder.
pub const ENV: &str = "FALCON_SANDBOX";

static ROOT: OnceLock<PathBuf> = OnceLock::new();

/// The sandbox folder: `$FALCON_SANDBOX`, or `Falcon Sandbox` in the home folder.
pub fn default_root() -> PathBuf {
    if let Some(p) = std::env::var_os(ENV).filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
    home.join("Falcon Sandbox")
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
            root.display()
        )),
        _ => Ok(()),
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

#[cfg(test)]
mod tests {
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
}
