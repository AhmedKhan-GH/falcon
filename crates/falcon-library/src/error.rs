//! Error type for library I/O.

use std::path::PathBuf;

/// Errors raised while reading or writing a library.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A filesystem operation failed.
    #[error("I/O error at {path}: {source}")]
    Io {
        /// Path the operation was acting on.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// A JSON file could not be parsed or produced.
    #[error("JSON error in {path}: {source}")]
    Json {
        /// File that failed.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: serde_json::Error,
    },
    /// The folder does not look like an Eagle library.
    #[error("{0} is not an Eagle library (missing metadata.json or images/)")]
    NotALibrary(PathBuf),
    /// `save_to` was pointed at the library's own folder, which would copy
    /// every file onto itself.
    #[error("cannot save {0} onto itself")]
    SaveOntoSelf(PathBuf),
    /// The file cannot be imported (empty, missing, unknown folder, ...).
    #[error("cannot import {path}: {reason}")]
    Import {
        /// The file being imported.
        path: PathBuf,
        /// Why it was refused.
        reason: String,
    },
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::Io { path: path.into(), source }
    }
    pub(crate) fn json(path: impl Into<PathBuf>, source: serde_json::Error) -> Self {
        Error::Json { path: path.into(), source }
    }
}
