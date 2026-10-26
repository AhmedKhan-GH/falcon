//! Reading and writing Eagle `.library` folders.

pub mod edit;
pub mod error;
pub mod library;
pub mod ids;
pub mod model;
pub mod palette;
pub mod search;

pub use error::{Error, Result};
pub use library::Library;
pub mod import;
pub mod thumbnail;
