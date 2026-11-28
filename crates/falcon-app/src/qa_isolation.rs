//! Mandatory QA Isolation and Boundary Enforcement.
//!
//! Guarantees that automated testing and QA runners can NEVER read, write,
//! or restore production user libraries, preferences, or AI search indexes.
//!
//! When QA mode is active (`FALCON_QA_DIR` or `FALCON_QA_SANDBOX`):
//! 1. Dedicated app data name (`falcon-qa`) is used.
//! 2. Reopening production/saved libraries is strictly disabled.
//! 3. All library and import paths are strictly validated against the authorized
//!    sandbox root, rejecting symlink escapes and path traversal (`..`).
//! 4. Any attempt to access paths outside the sandbox FAILS CLOSED immediately.

use std::fmt;
use std::path::{Path, PathBuf};

pub const QA_DIR_VAR: &str = "FALCON_QA_DIR";
pub const QA_SANDBOX_VAR: &str = "FALCON_QA_SANDBOX";

/// Error returned when a QA operation attempts to violate sandbox boundaries.
#[derive(Debug, PartialEq, Eq)]
pub enum QaSecurityError {
    MissingSandbox(PathBuf),
    SandboxEscape {
        target: PathBuf,
        sandbox: PathBuf,
    },
}

impl fmt::Display for QaSecurityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSandbox(p) => write!(f, "QA mode active but sandbox directory is invalid or missing: {}", p.display()),
            Self::SandboxEscape { target, sandbox } => write!(f, "Path '{}' escapes authorized QA sandbox '{}' (symlink or path traversal)", target.display(), sandbox.display()),
        }
    }
}

impl std::error::Error for QaSecurityError {}

/// Checks if the process is running under automated QA or test harness mode.
pub fn is_qa_mode() -> bool {
    std::env::var_os(QA_DIR_VAR).is_some() || std::env::var_os(QA_SANDBOX_VAR).is_some()
}

/// Returns the designated canonical sandbox directory when QA mode is active.
pub fn authorized_sandbox_root() -> Result<Option<PathBuf>, QaSecurityError> {
    let raw = std::env::var_os(QA_DIR_VAR).or_else(|| std::env::var_os(QA_SANDBOX_VAR));
    let Some(raw_path) = raw else {
        return Ok(None);
    };

    let p = PathBuf::from(raw_path);
    if !p.exists() || !p.is_dir() {
        return Err(QaSecurityError::MissingSandbox(p));
    }

    let canonical = p.canonicalize().map_err(|_| QaSecurityError::MissingSandbox(p))?;
    Ok(Some(canonical))
}

/// Validates that `target` resolves strictly inside the authorized QA sandbox.
/// Resolves symlinks and `..` components to prevent directory traversal escapes.
pub fn validate_qa_sandbox_path(target: &Path) -> Result<PathBuf, QaSecurityError> {
    let Some(sandbox) = authorized_sandbox_root()? else {
        // Not in QA mode: normal operation
        return Ok(target.to_path_buf());
    };

    // If target exists, canonicalize fully
    let canonical = if target.exists() {
        target.canonicalize().map_err(|_| QaSecurityError::SandboxEscape {
            target: target.to_path_buf(),
            sandbox: sandbox.clone(),
        })?
    } else {
        // For paths that do not exist yet (e.g. newly created library),
        // canonicalize the parent and append the filename.
        let parent = target.parent().unwrap_or(Path::new("/"));
        let canonical_parent = parent.canonicalize().map_err(|_| QaSecurityError::SandboxEscape {
            target: target.to_path_buf(),
            sandbox: sandbox.clone(),
        })?;
        canonical_parent.join(target.file_name().unwrap_or_default())
    };

    if !canonical.starts_with(&sandbox) {
        return Err(QaSecurityError::SandboxEscape {
            target: canonical,
            sandbox,
        });
    }

    Ok(canonical)
}

/// Sanitizes library open requests at application startup.
/// If in QA mode:
/// - Reopening saved production libraries is DISALLOWED.
/// - Only the explicitly supplied `initial` path is allowed, provided it is
///   inside the authorized sandbox.
pub fn sanitize_startup_libraries(
    initial: Option<PathBuf>,
    saved_libraries: Vec<PathBuf>,
) -> Result<(Option<PathBuf>, Vec<PathBuf>), QaSecurityError> {
    if !is_qa_mode() {
        return Ok((initial, saved_libraries));
    }

    // In QA mode, saved production libraries are ALWAYS dropped.
    let target = match initial {
        Some(p) => {
            let valid = validate_qa_sandbox_path(&p)?;
            Some(valid)
        }
        None => None,
    };

    Ok((target.clone(), target.into_iter().collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qa_sandbox_enforcement() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = tmp.path().join("sandbox");
        std::fs::create_dir_all(&sandbox).unwrap();

        let outside = tmp.path().join("outside.library");
        std::fs::create_dir_all(&outside).unwrap();

        let inside = sandbox.join("test.library");
        std::fs::create_dir_all(&inside).unwrap();

        // Simulate QA mode
        unsafe {
            std::env::set_var(QA_DIR_VAR, &sandbox);
        }

        // 1. Inside library is valid
        assert!(validate_qa_sandbox_path(&inside).is_ok());

        // 2. Outside library FAILS CLOSED
        assert!(matches!(
            validate_qa_sandbox_path(&outside),
            Err(QaSecurityError::SandboxEscape { .. })
        ));

        // 3. Path traversal escape (sandbox/../outside.library) FAILS CLOSED
        let traversal = sandbox.join("../outside.library");
        assert!(matches!(
            validate_qa_sandbox_path(&traversal),
            Err(QaSecurityError::SandboxEscape { .. })
        ));

        // 4. Saved libraries are stripped in QA mode
        let (initial, open) = sanitize_startup_libraries(Some(inside.clone()), vec![outside.clone()]).unwrap();
        assert_eq!(initial, Some(inside.canonicalize().unwrap()));
        assert_eq!(open.len(), 1);
        assert!(!open.contains(&outside));

        // 5. Every open, create and add-to-library goes through `install_dir::guard`.
        assert!(crate::install_dir::guard(&inside).is_ok());
        let refused = crate::install_dir::guard(&outside).unwrap_err().to_string();
        assert!(refused.contains("escapes authorized QA sandbox"), "{refused}");
        assert!(crate::install_dir::guard(&traversal).is_err());

        // 6. A saved auto-import folder outside the sandbox is refused too.
        let note = crate::prefs::auto_import_refusal(&outside).unwrap();
        assert!(note.contains("sandbox"), "{note}");
        assert!(crate::prefs::auto_import_refusal(&inside).is_none());

        // Clean up env
        unsafe {
            std::env::remove_var(QA_DIR_VAR);
        }
    }
}
