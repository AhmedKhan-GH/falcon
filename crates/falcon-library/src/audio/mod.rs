//! Audio decoding: the one path playback, seeking, durations, waveforms
//! and the tempo all read audio through.
//!
//! [`open`] returns an [`AudioSource`], a stream of interleaved `f32`
//! blocks that can seek. Today the only backend is symphonia (pure Rust,
//! MPL-2.0) with its MP3, AAC, ALAC, FLAC, Vorbis, PCM and ADPCM decoders
//! in MP4/M4A, CAF, Ogg, WAV, AIFF, FLAC, MP3 and ADTS files, plus the
//! pure-Rust `opus-decoder` crate for Opus (in Ogg or CAF). Formats with no
//! FOSS pure-Rust decoder (WMA, APE, AMR) don't open: [`open`] returns an
//! [`AudioError`] and callers fall back (the viewer shows the reason, the
//! waveform is flat, no duration is stored). A later backend (FFmpeg,
//! loaded at runtime) can implement [`AudioSource`] and be tried after
//! symphonia in [`open`] without the callers changing.

mod adts;
mod edit;
mod native;
mod opus;

use std::fmt;
use std::path::Path;

pub use native::SymphoniaSource;
pub(crate) use native::open_format;

/// A decoded audio stream.
pub trait AudioSource: Send {
    /// Channels in each block's frames.
    fn channels(&self) -> u16;
    /// Frames per second.
    fn sample_rate(&self) -> u32;
    /// The stream's length in seconds, when the file says.
    fn duration(&self) -> Option<f64>;
    /// The next block of samples, or `None` at the end.
    fn next_block(&mut self) -> Option<Block<'_>>;
    /// Moves so the next block starts at `secs` (clamped to the stream)
    /// and returns the time it starts at.
    fn seek(&mut self, secs: f64) -> Result<f64, AudioError>;
    /// Where the next block starts, in seconds.
    fn position(&self) -> f64;
    /// Which decoder backs the stream, for messages and tests.
    fn backend(&self) -> &'static str;
}

/// Decoded audio: interleaved samples in -1..=1, never empty.
#[derive(Debug, Clone, Copy)]
pub struct Block<'a> {
    pub samples: &'a [f32],
    pub channels: u16,
    pub rate: u32,
}

impl Block<'_> {
    /// The samples one frame at a time.
    pub fn frames(&self) -> std::slice::Chunks<'_, f32> {
        self.samples.chunks(usize::from(self.channels.max(1)))
    }
}

/// Why a file can't be decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioError {
    /// The file couldn't be read.
    Io(String),
    /// No backend recognises the container.
    UnknownFormat,
    /// The container holds no audio track.
    NoAudioTrack,
    /// The track's codec has no decoder here (WMA, APE, AMR, …).
    UnsupportedCodec,
    /// The stream couldn't be moved to the asked time.
    Seek(String),
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioError::Io(e) => f.write_str(e),
            AudioError::UnknownFormat => f.write_str("unsupported audio format"),
            AudioError::NoAudioTrack => f.write_str("no audio track"),
            AudioError::UnsupportedCodec => f.write_str("unsupported audio codec"),
            AudioError::Seek(e) => write!(f, "seek failed: {e}"),
        }
    }
}

impl std::error::Error for AudioError {}

/// Opens an audio file (`ext` is a hint; the contents decide). Backends
/// are tried in turn; the first error is kept when none can.
pub fn open(path: &Path, ext: &str) -> Result<Box<dyn AudioSource>, AudioError> {
    SymphoniaSource::open(path, ext).map(|s| Box::new(s) as Box<dyn AudioSource>)
}

/// Audio formats that decode, by extension, with the codecs each may hold.
pub const DECODED: [(&str, &str); 11] = [
    ("mp3", "MP3"),
    ("wav", "PCM, IEEE float, A-law, µ-law, IMA and MS ADPCM"),
    ("flac", "FLAC"),
    ("ogg", "Vorbis, Opus, FLAC"),
    ("opus", "Opus"),
    ("aac", "AAC-LC (ADTS)"),
    ("m4a", "AAC-LC, ALAC, FLAC, Opus"),
    ("aiff", "PCM"),
    ("aif", "PCM"),
    ("aifc", "PCM, IEEE float, A-law, µ-law"),
    ("caf", "PCM, ALAC, AAC-LC, FLAC, Opus, A-law, µ-law"),
];

/// Audio formats with no FOSS pure-Rust decoder, left for a runtime-loaded
/// FFmpeg backend.
pub const GAPS: [&str; 4] = ["wma", "ape", "amr", "awb"];

#[cfg(test)]
mod tests;
