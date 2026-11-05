//! Refusing libraries kept inside falcon's own install folder, as Eagle
//! refuses ones inside its own: removing or replacing the app would delete
//! them. Eagle checks when a library is created or opened; so does falcon,
//! and also for the library Add to Other Library writes into.
//!
//! The install folder is the `.app` bundle the running executable sits in,
//! or, for a bare executable (`cargo run`), the folder holding it. A library
//! anywhere inside the bundle is refused; next to a bare executable, a
//! library directly in its folder is (Eagle's rule for its install path).

use std::path::{Path, PathBuf};

use crate::locale::tr;

/// Where falcon is installed: the enclosing `.app` bundle, or the
/// executable's folder.
#[derive(Debug, Clone, PartialEq)]
pub enum Install {
    Bundle(PathBuf),
    Folder(PathBuf),
}

/// Works out the install location from the executable's path.
pub fn install_of(exe: &Path) -> Option<Install> {
    if let Some(bundle) = exe.ancestors().find(|a| a.extension().is_some_and(|e| e.eq_ignore_ascii_case("app"))) {
        return Some(Install::Bundle(bundle.to_path_buf()));
    }
    exe.parent().map(|p| Install::Folder(p.to_path_buf()))
}

/// Whether a library at `lib` lies in the install location.
pub fn inside(install: &Install, lib: &Path) -> bool {
    let lib = resolve(lib);
    match install {
        Install::Bundle(b) => lib.starts_with(resolve(b)),
        Install::Folder(f) => lib.parent().is_some_and(|p| p == resolve(f)),
    }
}

/// `p` with symlinks resolved as far as it exists, so a path through a link
/// can't slip past the check.
fn resolve(p: &Path) -> PathBuf {
    let Ok(abs) = std::path::absolute(p) else { return p.to_path_buf() };
    let mut existing = abs.as_path();
    let mut rest = Vec::new();
    while !existing.exists() {
        match (existing.file_name(), existing.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name.to_owned());
                existing = parent;
            }
            _ => return abs,
        }
    }
    let mut out = existing.canonicalize().unwrap_or_else(|_| existing.to_path_buf());
    for name in rest.into_iter().rev() {
        out.push(name);
    }
    out
}

/// Why a library at `lib` is refused, if it is inside falcon's install
/// location.
pub fn refusal(lib: &Path) -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let install = install_of(&resolve(&exe))?;
    inside(&install, lib).then(|| {
        tr("Libraries can't be kept in the folder Falcon is installed in, because removing or updating the app would delete them. Move the library somewhere else and open it again.").to_string()
    })
}

/// The sandbox guard followed by [`refusal`], for every place falcon
/// creates, opens or writes into a library.
pub fn guard(lib: &Path) -> falcon_library::Result<()> {
    falcon_library::sandbox::guard(lib)?;
    match refusal(lib) {
        Some(why) => Err(falcon_library::Error::Sandbox(why)),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_bundle_or_the_folder() {
        let exe = Path::new("/Applications/Falcon.app/Contents/MacOS/falcon-app");
        assert_eq!(install_of(exe), Some(Install::Bundle(PathBuf::from("/Applications/Falcon.app"))));
        let bare = Path::new("/work/target/debug/falcon-app");
        assert_eq!(install_of(bare), Some(Install::Folder(PathBuf::from("/work/target/debug"))));
    }

    #[test]
    fn refuses_libraries_inside_the_install_location() {
        let d = tempfile::tempdir().unwrap();
        let bundle = d.path().join("Falcon.app");
        std::fs::create_dir_all(bundle.join("Contents/Resources")).unwrap();
        let b = Install::Bundle(bundle.clone());
        assert!(inside(&b, &bundle.join("Contents/Resources/A.library")));
        assert!(inside(&b, &bundle.join("New.library")));
        assert!(!inside(&b, &d.path().join("A.library")));
        assert!(!inside(&b, &d.path().join("Falcon.app copy/A.library")));
        let f = Install::Folder(d.path().join("bin"));
        std::fs::create_dir_all(d.path().join("bin")).unwrap();
        assert!(inside(&f, &d.path().join("bin/A.library")));
        assert!(!inside(&f, &d.path().join("bin/sub/A.library")));
        assert!(!inside(&f, &d.path().join("A.library")));
        #[cfg(unix)]
        std::os::unix::fs::symlink(bundle.join("Contents"), d.path().join("Link")).unwrap();
        #[cfg(windows)]
        assert!(std::process::Command::new("cmd").arg("/C").arg("mklink").arg("/J").arg(d.path().join("Link")).arg(bundle.join("Contents")).output().unwrap().status.success());
        assert!(inside(&b, &d.path().join("Link/A.library")));
    }

    #[test]
    fn the_test_runner_location_is_fine_for_a_sandbox_library() {
        // The running test binary is a bare executable; a library elsewhere passes.
        let d = tempfile::tempdir().unwrap();
        assert!(refusal(&d.path().join("A.library")).is_none());
    }
}
