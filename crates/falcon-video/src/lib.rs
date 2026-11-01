//! Video for falcon's viewer, through the operating system's own media
//! framework: AVFoundation on macOS. Nothing here links a third-party
//! codec library, so falcon keeps its MIT/Apache-2.0 licence and plays
//! whatever the system plays (H.264, HEVC, ProRes, … in mp4, mov and m4v;
//! not WebM or Matroska).
//!
//! - [`probe`] reads a file's duration, display size and audio track.
//! - [`frame_at`] decodes the frame at a time, on any thread.
//! - [`Grabber`] decodes small pictures of one file again and again, for
//!   previews such as the seek bar's hover thumbnail.
//! - [`Video`] plays a file with sound and hands out each new frame as RGBA
//!   pixels for the caller to upload as a texture. It lives on the main
//!   thread, as AVFoundation's player requires.
//!
//! All Objective-C interop sits in the private `mac` module; this file and
//! `pixels` are plain Rust.

mod pixels;

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
pub use mac::{Grabber, Video, frame_at, probe};

#[cfg(not(target_os = "macos"))]
mod other;
#[cfg(not(target_os = "macos"))]
pub use other::{Grabber, Video, frame_at, probe};

pub use pixels::{Frame, quarter_turns};

/// What [`probe`] learns about a file.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    /// Seconds.
    pub duration: f64,
    /// The size the picture is shown at, after the track's rotation.
    pub width: u32,
    pub height: u32,
    /// Frames per second as the video track states it (0 if unknown).
    pub fps: f32,
    pub has_audio: bool,
    /// Clockwise quarter turns the track asks for when shown (0 to 3).
    pub turns: u8,
}

/// Why a file could not be opened, probed or decoded.
#[derive(Clone, Debug, PartialEq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Playback speeds, slowest first: the steps the speed keys move through.
pub const RATES: [f32; 11] = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 3.0, 4.0, 8.0];

/// The next speed up (or down) from `rate`; the ends stay put. A rate not
/// in [`RATES`] moves to the nearest step in that direction.
pub fn step_rate(rate: f32, up: bool) -> f32 {
    if up {
        RATES.iter().copied().find(|&r| r > rate + 1e-4).unwrap_or(rate)
    } else {
        RATES.iter().rev().copied().find(|&r| r < rate - 1e-4).unwrap_or(rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_step_and_stop_at_the_ends() {
        assert_eq!(step_rate(1.0, true), 1.25);
        assert_eq!(step_rate(1.0, false), 0.75);
        assert_eq!(step_rate(8.0, true), 8.0);
        assert_eq!(step_rate(0.25, false), 0.25);
        assert_eq!(step_rate(2.0, true), 3.0);
        assert_eq!(step_rate(1.1, true), 1.25);
        assert_eq!(step_rate(1.1, false), 1.0);
    }
}
