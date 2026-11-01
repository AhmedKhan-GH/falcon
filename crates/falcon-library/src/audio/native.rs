//! The symphonia backend of [`super::AudioSource`].

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, CODEC_TYPE_OPUS, CodecRegistry, Decoder, DecoderOptions};
use symphonia::core::errors::Error;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::{Hint, Probe};
use symphonia::core::units::{Time, TimeBase};
use symphonia::default::formats;

use super::{AudioError, AudioSource, Block, adts, edit};

/// symphonia's decoders with the Opus adapter added.
static CODECS: LazyLock<CodecRegistry> = LazyLock::new(|| {
    let mut r = CodecRegistry::new();
    symphonia::default::register_enabled_codecs(&mut r);
    r.register_all::<super::opus::OpusAdapter>();
    r
});

/// symphonia's readers, with falcon's ADTS reader in place of its own.
static PROBE: LazyLock<Probe> = LazyLock::new(|| {
    let mut p = Probe::default();
    p.register_all::<adts::AdtsReader>();
    p.register_all::<formats::CafReader>();
    p.register_all::<formats::FlacReader>();
    p.register_all::<formats::IsoMp4Reader>();
    p.register_all::<formats::MpaReader>();
    p.register_all::<formats::AiffReader>();
    p.register_all::<formats::WavReader>();
    p.register_all::<formats::OggReader>();
    p.register_all::<symphonia_metadata::id3v2::Id3v2Reader>();
    p
});

/// Opens a file's container.
pub(crate) fn open_format(path: &Path, ext: &str) -> Result<Box<dyn FormatReader>, AudioError> {
    let file = File::open(path).map_err(|e| AudioError::Io(e.to_string()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    hint.with_extension(ext);
    PROBE
        .format(&hint, mss, &FormatOptions { enable_gapless: true, ..Default::default() }, &MetadataOptions::default())
        .map(|p| p.format)
        .map_err(|e| match e {
            Error::IoError(e) if e.kind() != std::io::ErrorKind::UnexpectedEof => AudioError::Io(e.to_string()),
            _ => AudioError::UnknownFormat,
        })
}

fn secs(tb: Option<TimeBase>, rate: Option<u32>, ts: u64) -> Option<f64> {
    match tb {
        Some(tb) => {
            let t = tb.calc_time(ts);
            Some(t.seconds as f64 + t.frac)
        }
        None => rate.filter(|r| *r > 0).map(|r| ts as f64 / f64::from(r)),
    }
}

/// A file decoded by symphonia.
pub struct SymphoniaSource {
    path: PathBuf,
    ext: String,
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track: u32,
    time_base: Option<TimeBase>,
    rate: u32,
    channels: u16,
    /// Seconds of priming at the stream's start that aren't audio.
    lead: f64,
    /// Seconds decoded before a seek's time so the decoder settles (80 ms
    /// for Opus, RFC 7845 §4.6).
    preroll: f64,
    duration: Option<f64>,
    buf: Vec<f32>,
    /// The part of `buf` still to hand out.
    start: usize,
    /// Stream frames before `buf[start]`, priming included.
    frame: u64,
    /// Frames to drop from the next decoded blocks, after a seek lands
    /// before the asked time.
    skip: u64,
    ended: bool,
}

impl SymphoniaSource {
    pub fn open(path: &Path, ext: &str) -> Result<Self, AudioError> {
        let format = open_format(path, ext)?;
        let track = format.tracks().iter().find(|t| t.codec_params.codec != CODEC_TYPE_NULL).ok_or(AudioError::NoAudioTrack)?;
        let params = track.codec_params.clone();
        let id = track.id;
        let decoder = CODECS.make(&params, &DecoderOptions::default()).map_err(|_| AudioError::UnsupportedCodec)?;
        let header = params.n_frames.and_then(|n| secs(params.time_base, params.sample_rate, n));
        let (lead, length) = match edit::read(path) {
            Some(e) => (e.lead, e.length),
            // An Ogg Opus stream's pre-skip, which symphonia 0.5 reports
            // but doesn't trim.
            None if params.codec == CODEC_TYPE_OPUS => {
                let lead = f64::from(params.delay.unwrap_or(0)) / 48_000.0;
                (lead, header.map(|h| h - lead))
            }
            None => (0.0, None),
        };
        let mut s = SymphoniaSource {
            path: path.to_path_buf(),
            ext: ext.to_string(),
            format,
            decoder,
            track: id,
            time_base: params.time_base,
            rate: params.sample_rate.unwrap_or(0),
            channels: params.channels.map_or(0, |c| c.count() as u16),
            lead,
            preroll: if params.codec == CODEC_TYPE_OPUS { 0.08 } else { 0.0 },
            duration: length.or(header).filter(|d| *d > 0.0),
            buf: Vec::new(),
            start: 0,
            frame: 0,
            skip: 0,
            ended: false,
        };
        // Decode the first block now, so a file whose codec fails at once
        // is refused here and the real rate is known (an MP4's sample
        // entry can disagree with the codec's own header).
        if !s.fill() {
            return Err(AudioError::UnsupportedCodec);
        }
        if s.lead > 0.0 {
            s.skip_frames(s.lead_frames());
            if s.start >= s.buf.len() && !s.fill() {
                return Err(AudioError::UnsupportedCodec);
            }
        }
        Ok(s)
    }

    fn lead_frames(&self) -> u64 {
        (self.lead * f64::from(self.sample_rate())).round() as u64
    }

    fn end_frame(&self) -> Option<u64> {
        self.duration.map(|d| self.lead_frames() + (d * f64::from(self.sample_rate())).round() as u64)
    }

    /// Drops `n` frames from the buffered block, and the rest from the
    /// next ones.
    fn skip_frames(&mut self, n: u64) {
        let ch = usize::from(self.channels());
        let here = ((self.buf.len() - self.start) / ch) as u64;
        let now = n.min(here);
        self.start += now as usize * ch;
        self.frame += now;
        self.skip += n - now;
    }

    /// Decodes packets until a block with samples is in `buf`; false at
    /// the end of the stream.
    fn fill(&mut self) -> bool {
        self.buf.clear();
        self.start = 0;
        while !self.ended {
            let packet = match self.format.next_packet() {
                Ok(p) => p,
                Err(Error::ResetRequired) => {
                    self.decoder.reset();
                    continue;
                }
                Err(_) => {
                    self.ended = true;
                    break;
                }
            };
            if packet.track_id() != self.track {
                continue;
            }
            let decoded = match self.decoder.decode(&packet) {
                Ok(d) => d,
                Err(Error::DecodeError(_)) => continue,
                Err(_) => {
                    self.ended = true;
                    break;
                }
            };
            let spec = *decoded.spec();
            let frames = decoded.frames();
            if frames == 0 {
                continue;
            }
            let channels = spec.channels.count().max(1);
            let mut sb = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
            sb.copy_interleaved_ref(decoded);
            self.rate = spec.rate;
            self.channels = channels as u16;
            let drop = self.skip.min(frames as u64) as usize;
            self.skip -= drop as u64;
            self.frame += drop as u64;
            if drop == frames {
                continue;
            }
            let mut keep = frames - drop;
            if let Some(end) = self.end_frame() {
                keep = keep.min(end.saturating_sub(self.frame) as usize);
                if keep == 0 {
                    self.ended = true;
                    break;
                }
            }
            self.buf.extend_from_slice(&sb.samples()[drop * channels..(drop + keep) * channels]);
            return true;
        }
        false
    }

    /// Starts over on a fresh reader, for containers that can't seek from
    /// where they are (symphonia's CAF reader at the end of the stream).
    fn reopen(&mut self) -> Result<(), AudioError> {
        let format = open_format(&self.path, &self.ext)?;
        let track = format.tracks().iter().find(|t| t.id == self.track).ok_or(AudioError::NoAudioTrack)?;
        self.decoder = CODECS.make(&track.codec_params, &DecoderOptions::default()).map_err(|_| AudioError::UnsupportedCodec)?;
        self.format = format;
        self.frame = 0;
        self.skip = 0;
        self.ended = false;
        self.buf.clear();
        self.start = 0;
        Ok(())
    }
}

impl AudioSource for SymphoniaSource {
    fn channels(&self) -> u16 {
        self.channels.max(1)
    }

    fn sample_rate(&self) -> u32 {
        self.rate.max(1)
    }

    fn duration(&self) -> Option<f64> {
        self.duration
    }

    fn next_block(&mut self) -> Option<Block<'_>> {
        if self.start >= self.buf.len() && !self.fill() {
            return None;
        }
        let from = self.start;
        self.start = self.buf.len();
        self.frame += ((self.buf.len() - from) / usize::from(self.channels())) as u64;
        Some(Block { samples: &self.buf[from..], channels: self.channels(), rate: self.sample_rate() })
    }

    fn seek(&mut self, to_secs: f64) -> Result<f64, AudioError> {
        let at = match self.duration {
            Some(d) => to_secs.clamp(0.0, d),
            None => to_secs.max(0.0),
        };
        let target = at + self.lead;
        let rate = f64::from(self.sample_rate());
        let from = (target - self.preroll).max(0.0);
        let to = || SeekTo::Time { time: Time::new(from.trunc() as u64, from.fract()), track_id: Some(self.track) };
        let first = self.format.seek(SeekMode::Accurate, to());
        let seeked = match first {
            Ok(s) => Some(s),
            Err(_) => {
                let to = to();
                self.reopen()?;
                self.format.seek(SeekMode::Accurate, to).ok()
            }
        };
        self.decoder.reset();
        self.ended = false;
        self.buf.clear();
        self.start = 0;
        match seeked {
            Some(to) => {
                // The reader lands on a packet at or before the asked
                // time; the frames in between are decoded and dropped.
                let actual = secs(self.time_base, Some(self.rate), to.actual_ts).unwrap_or(0.0).min(target);
                self.frame = (actual * rate).round() as u64;
                self.skip = ((target - actual) * rate).round() as u64;
            }
            // Still no seek: decode from the top and drop up to the time.
            None => {
                self.frame = 0;
                self.skip = (target * rate).round() as u64;
            }
        }
        Ok((self.frame + self.skip) as f64 / rate - self.lead)
    }

    fn position(&self) -> f64 {
        ((self.frame + self.skip) as f64 / f64::from(self.sample_rate()) - self.lead).max(0.0)
    }

    fn backend(&self) -> &'static str {
        "symphonia"
    }
}
