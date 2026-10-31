//! Reading and writing Eagle `.library` folders.

pub mod backup;
pub mod duplicates;
pub mod edit;
pub mod error;
pub mod exif;
pub mod fontmeta;
pub mod fontpreview;
pub mod library;
pub mod ids;
pub mod keywords;
pub mod media;
pub mod model;
pub mod pack;
pub mod palette;
pub mod search;
pub mod smart;
pub mod undo;

pub use error::{Error, Result};
pub use library::Library;
pub mod import;
pub mod thumbnail;
pub mod waveform;
