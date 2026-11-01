//! Opus for symphonia: a [`Decoder`] over the pure-Rust `opus-decoder`
//! crate, registered next to symphonia's own codecs. symphonia's Ogg, CAF
//! and MP4 readers already find Opus tracks; 0.5 just has no decoder.

use opus_decoder::{OpusDecoder, OpusMultistreamDecoder};
use symphonia::core::audio::{AsAudioBufferRef, AudioBuffer, AudioBufferRef, Channels, Signal, SignalSpec};
use symphonia::core::codecs::{CODEC_TYPE_OPUS, CodecDescriptor, CodecParameters, Decoder, DecoderOptions, FinalizeResult};
use symphonia::core::errors::{Error, Result};
use symphonia::core::formats::Packet;

/// Opus always decodes at 48 kHz.
const RATE: u32 = 48_000;
/// The longest Opus packet, 120 ms, in frames.
const MAX_FRAMES: usize = 5760;

enum Inner {
    Single(Box<OpusDecoder>),
    Multi(OpusMultistreamDecoder),
}

pub struct OpusAdapter {
    params: CodecParameters,
    inner: Inner,
    channels: usize,
    buf: AudioBuffer<f32>,
    scratch: Vec<f32>,
}

/// The stream layout from an Ogg `OpusHead` (RFC 7845 §5.1): channels,
/// and for mapping family 1 and up the stream counts and channel map.
struct Head {
    channels: usize,
    streams: Option<(usize, usize, Vec<u8>)>,
}

fn parse_head(extra: &[u8]) -> Option<Head> {
    if extra.len() < 19 || &extra[..8] != b"OpusHead" {
        return None;
    }
    let channels = usize::from(extra[9]);
    let streams = if extra[18] != 0 && extra.len() >= 21 + channels {
        Some((usize::from(extra[19]), usize::from(extra[20]), extra[21..21 + channels].to_vec()))
    } else {
        None
    };
    Some(Head { channels, streams })
}

/// `n` channels, front ones first.
pub(super) fn channels_spec(n: usize) -> Channels {
    let mut bits = 0u32;
    for i in 0..n.min(32) {
        bits |= 1 << i;
    }
    Channels::from_bits_truncate(bits)
}

fn spec(n: usize) -> SignalSpec {
    SignalSpec::new(RATE, channels_spec(n))
}

impl Decoder for OpusAdapter {
    fn try_new(params: &CodecParameters, _: &DecoderOptions) -> Result<Self> {
        if params.codec != CODEC_TYPE_OPUS {
            return Err(Error::Unsupported("opus: not an opus track"));
        }
        let head = params.extra_data.as_deref().and_then(parse_head);
        let channels = head.as_ref().map(|h| h.channels).or(params.channels.map(|c| c.count())).unwrap_or(2).max(1);
        let inner = match head.and_then(|h| h.streams) {
            Some((streams, coupled, map)) => Inner::Multi(
                OpusMultistreamDecoder::new(RATE, channels, streams, coupled, &map).map_err(|_| Error::Unsupported("opus: bad channel mapping"))?,
            ),
            None if channels <= 2 => Inner::Single(Box::new(OpusDecoder::new(RATE, channels).map_err(|_| Error::Unsupported("opus: bad channel count"))?)),
            None => return Err(Error::Unsupported("opus: more than two channels without a mapping")),
        };
        Ok(OpusAdapter {
            params: params.clone(),
            inner,
            channels,
            buf: AudioBuffer::new(MAX_FRAMES as u64, spec(channels)),
            scratch: vec![0.0; MAX_FRAMES * channels],
        })
    }

    fn supported_codecs() -> &'static [CodecDescriptor] {
        &[CodecDescriptor {
            codec: CODEC_TYPE_OPUS,
            short_name: "opus",
            long_name: "Opus (opus-decoder)",
            inst_func: |params, opts| Ok(Box::new(OpusAdapter::try_new(params, opts)?)),
        }]
    }

    fn reset(&mut self) {
        match &mut self.inner {
            Inner::Single(d) => d.reset(),
            Inner::Multi(d) => d.reset(),
        }
    }

    fn codec_params(&self) -> &CodecParameters {
        &self.params
    }

    fn decode(&mut self, packet: &Packet) -> Result<AudioBufferRef<'_>> {
        let data = packet.buf();
        let got = match &mut self.inner {
            Inner::Single(d) => d.decode_float(data, &mut self.scratch, false),
            Inner::Multi(d) => d.decode_float(data, &mut self.scratch, false),
        };
        let frames = got.map_err(|_| Error::DecodeError("opus: invalid packet"))?.min(MAX_FRAMES);
        self.buf.clear();
        self.buf.render_reserved(Some(frames));
        let ch = self.channels;
        for c in 0..ch {
            let plane = self.buf.chan_mut(c);
            for (i, s) in plane.iter_mut().enumerate().take(frames) {
                *s = self.scratch[i * ch + c];
            }
        }
        self.buf.trim(packet.trim_start() as usize, packet.trim_end() as usize);
        Ok(self.buf.as_audio_buffer_ref())
    }

    fn finalize(&mut self) -> FinalizeResult {
        FinalizeResult::default()
    }

    fn last_decoded(&self) -> AudioBufferRef<'_> {
        self.buf.as_audio_buffer_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_opus_heads() {
        let mut h = b"OpusHead".to_vec();
        h.extend_from_slice(&[1, 2, 0x38, 1, 0x80, 0xbb, 0, 0, 0, 0, 0]);
        let head = parse_head(&h).unwrap();
        assert_eq!(head.channels, 2);
        assert!(head.streams.is_none());
        let mut surround = b"OpusHead".to_vec();
        surround.extend_from_slice(&[1, 6, 0, 0, 0x80, 0xbb, 0, 0, 0, 0, 1, 4, 2, 0, 4, 1, 2, 3, 5]);
        let head = parse_head(&surround).unwrap();
        assert_eq!(head.streams, Some((4, 2, vec![0, 4, 1, 2, 3, 5])));
        assert!(parse_head(b"OpusTags").is_none());
        assert_eq!(spec(6).channels.count(), 6);
    }
}
