//! The FFmpeg backend: files symphonia and the Opus decoder can't read
//! (WMA, APE, AMR, the soundtrack of a Matroska, WebM or AVI file) decode
//! through `falcon-ffmpeg` when an LGPL FFmpeg is installed.

use std::path::Path;

use super::{AudioError, AudioSource, Block};

/// A file's sound decoded by FFmpeg, as stereo `f32` at 48 kHz.
pub struct FfmpegSource(falcon_ffmpeg::AudioFile);

impl FfmpegSource {
    /// Opens `path` when FFmpeg is installed and can read its sound.
    pub fn open(path: &Path) -> Result<FfmpegSource, AudioError> {
        if !falcon_ffmpeg::available() {
            return Err(AudioError::UnsupportedCodec);
        }
        falcon_ffmpeg::AudioFile::open(path).map(FfmpegSource).map_err(|_| AudioError::UnsupportedCodec)
    }
}

impl AudioSource for FfmpegSource {
    fn channels(&self) -> u16 {
        falcon_ffmpeg::AudioFile::CHANNELS
    }

    fn sample_rate(&self) -> u32 {
        falcon_ffmpeg::AudioFile::RATE
    }

    fn duration(&self) -> Option<f64> {
        Some(self.0.duration()).filter(|d| *d > 0.0)
    }

    fn next_block(&mut self) -> Option<Block<'_>> {
        let samples = self.0.next_block()?;
        Some(Block { samples, channels: falcon_ffmpeg::AudioFile::CHANNELS, rate: falcon_ffmpeg::AudioFile::RATE })
    }

    fn seek(&mut self, secs: f64) -> Result<f64, AudioError> {
        let t = match self.duration() {
            Some(d) => secs.clamp(0.0, d),
            None => secs.max(0.0),
        };
        self.0.seek(t).map_err(|e| AudioError::Seek(e.0))?;
        Ok(t)
    }

    fn position(&self) -> f64 {
        self.0.position()
    }

    fn backend(&self) -> &'static str {
        "ffmpeg"
    }
}
