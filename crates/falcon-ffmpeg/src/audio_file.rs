//! A file's sound alone, decoded on demand: falcon's shared audio decoder
//! (`falcon_library::audio`) falls back to it for files its pure-Rust
//! decoders can't read, such as a video's soundtrack or WMA.

use std::path::Path;

use crate::decode::{Decoder, Input, OUT_CHANNELS, OUT_RATE, Packet, Resampler};
use crate::{Error, Result, ffi};

/// The best sound track of a file as interleaved stereo `f32` at 48 kHz.
pub struct AudioFile {
    input: Input,
    decoder: Decoder,
    resampler: Resampler,
    packet: Packet,
    buf: Vec<f32>,
    pos: usize,
    /// After a seek: drop sound before this time.
    skip_to: Option<f64>,
    drained: bool,
    duration: f64,
    /// Media time of the next sample handed out.
    position: f64,
}

impl AudioFile {
    pub const CHANNELS: u16 = OUT_CHANNELS as u16;
    pub const RATE: u32 = OUT_RATE;

    pub fn open(path: &Path) -> Result<AudioFile> {
        let input = Input::open(path)?;
        let (index, codec) = input.best(ffi::AVMEDIA_TYPE_AUDIO).ok_or_else(|| Error("no sound FFmpeg can decode".into()))?;
        let decoder = Decoder::open(&input, index, codec)?;
        let duration = input.duration(Some(index));
        let (resampler, packet) = (Resampler::new(input.api()), Packet::new(input.api())?);
        Ok(AudioFile { input, decoder, resampler, packet, buf: Vec::new(), pos: 0, skip_to: None, drained: false, duration, position: 0.0 })
    }

    /// Length in seconds (0 when unknown).
    pub fn duration(&self) -> f64 {
        self.duration
    }

    /// Where the next sample is, in seconds.
    pub fn position(&self) -> f64 {
        self.position
    }

    /// The next sample, or `None` at the end.
    pub fn next_sample(&mut self) -> Option<f32> {
        while self.pos >= self.buf.len() {
            if !self.fill() {
                return None;
            }
        }
        let s = self.buf[self.pos];
        self.pos += 1;
        if self.pos.is_multiple_of(OUT_CHANNELS) {
            self.position += 1.0 / f64::from(OUT_RATE);
        }
        Some(s)
    }

    /// The rest of the decoded block, or the next one: interleaved stereo,
    /// never empty; `None` at the end.
    pub fn next_block(&mut self) -> Option<&[f32]> {
        if self.pos >= self.buf.len() && !self.fill() {
            return None;
        }
        let block = &self.buf[self.pos..];
        self.position += (block.len() / OUT_CHANNELS) as f64 / f64::from(OUT_RATE);
        self.pos = self.buf.len();
        Some(block)
    }

    /// Moves to `secs`: the packet before it, decoded forward to it.
    pub fn seek(&mut self, secs: f64) -> Result<()> {
        self.input.seek(secs)?;
        self.decoder.flush();
        self.resampler.reset();
        self.buf.clear();
        self.pos = 0;
        self.skip_to = Some(secs.max(0.0));
        self.drained = false;
        self.position = secs.max(0.0);
        Ok(())
    }

    /// Decodes until there are samples in `buf`; false at the end.
    fn fill(&mut self) -> bool {
        self.buf.clear();
        self.pos = 0;
        let index = self.decoder.info.index;
        while self.buf.is_empty() {
            if self.drained {
                return false;
            }
            self.packet.unref();
            match self.input.read(&mut self.packet) {
                Ok(true) if self.packet.stream_index() != index => continue,
                Ok(true) => {
                    let _ = self.decoder.send(Some(&self.packet));
                }
                _ => {
                    let _ = self.decoder.send(None);
                    self.drained = true;
                }
            }
            while self.decoder.receive() {
                let f = self.decoder.frame();
                let pts = self.decoder.pts(f);
                let Ok(mut samples) = self.resampler.convert(f, 1.0) else { continue };
                if let (Some(target), Some(t)) = (self.skip_to, pts) {
                    if t < target {
                        let skip = (((target - t) * f64::from(OUT_RATE)) as usize * OUT_CHANNELS).min(samples.len());
                        samples.drain(..skip);
                    }
                    if !samples.is_empty() {
                        self.skip_to = None;
                    }
                }
                self.buf.extend_from_slice(&samples);
            }
        }
        true
    }
}
