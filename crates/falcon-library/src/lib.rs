//! Reading and writing Eagle `.library` folders.

pub mod error;
pub mod library;
pub mod model;

pub use error::{Error, Result};
pub use library::Library;
