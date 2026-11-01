//! falcon's fallback video decoder: FFmpeg, loaded at run time.
//!
//! macOS plays mp4, mov and m4v through AVFoundation; Matroska, WebM, AVI,
//! WMV, FLV, Ogg, MPEG-TS and the rest go through FFmpeg instead, as Eagle
//! falls back to its bundled player. falcon never links FFmpeg: the shared
//! libraries are opened with `dlopen` when first needed (see [`ffi`]), so
//! falcon builds and runs without them and only the fallback is missing.
//! The libraries must be an LGPL, decode-only build such as the one
//! `scripts/build-ffmpeg.sh` makes; any GPL or nonfree build is refused.
//!
//! - [`probe`] reads a file's duration, size and tracks.
//! - [`Grabber`] decodes single pictures, scaled, for previews; [`frame_at`]
//!   one picture at full size; [`thumbnail`] the import thumbnail.
//! - [`Player`] plays a file: worker threads decode ahead, the owner takes
//!   each frame when it is due, and [`AudioOut`] carries the sound to the
//!   caller's audio output, which drives the clock.

mod decode;
pub mod ffi;
mod frame;
mod media;
mod player;
pub mod sync;

pub use frame::Frame;
pub use media::{Grabber, frame_at, probe, thumbnail};
pub use player::{AudioOut, Player};

/// What [`probe`] learns about a file.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    /// Seconds.
    pub duration: f64,
    /// The size the picture is shown at, after the pixel aspect ratio and
    /// the stream's rotation.
    pub width: u32,
    pub height: u32,
    /// Frames per second as the stream states it (0 if unknown).
    pub fps: f32,
    pub has_audio: bool,
    /// Clockwise quarter turns the stream asks for (0 to 3).
    pub turns: u8,
}

/// Why a file could not be opened or decoded.
#[derive(Clone, Debug, PartialEq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Whether FFmpeg can be used, and which: `Ok` with its version string and
/// the folder it was loaded from.
pub fn status() -> std::result::Result<(String, std::path::PathBuf), ffi::LoadError> {
    ffi::api().map(|a| (a.version.clone(), a.dir.clone()))
}

/// Whether FFmpeg was found and can be used.
pub fn available() -> bool {
    ffi::api().is_ok()
}

/// File extensions macOS can't play that falcon hands to FFmpeg: Eagle's
/// video formats outside mp4/mov/m4v, and other common containers.
pub const EXTENSIONS: [&str; 22] = [
    "mkv", "webm", "avi", "wmv", "asf", "flv", "f4v", "ogv", "ogg", "ts", "mts", "m2ts", "mpg", "mpeg", "vob", "3gp", "3g2", "rm",
    "rmvb", "divx", "mxf", "nut",
];

/// Whether files with extension `ext` (any case) go to FFmpeg first.
pub fn handles(ext: &str) -> bool {
    let ext = ext.to_ascii_lowercase();
    EXTENSIONS.contains(&ext.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_formats() {
        for ext in ["mkv", "WEBM", "avi", "wmv", "flv", "ts", "m2ts", "3gp", "rm", "ogv"] {
            assert!(handles(ext), "{ext}");
        }
        for ext in ["mp4", "mov", "m4v", "png", ""] {
            assert!(!handles(ext), "{ext}");
        }
    }
}
