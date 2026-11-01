//! Raw AAC (`.aac`, ADTS) for symphonia. symphonia 0.5's own reader only
//! syncs on MPEG-4 headers (`FF F1`), so the MPEG-2 ones Apple's encoders
//! write (`FF F9`) and CRC-protected frames don't open, and it guesses the
//! length from the bitrate. This reader takes every ADTS header, indexes
//! the frames once at open, and so knows the exact length and seeks to
//! the frame.

use std::io::{Seek, SeekFrom};

use symphonia::core::codecs::{CODEC_TYPE_AAC, CodecParameters};
use symphonia::core::errors::{Error, Result, SeekErrorKind};
use symphonia::core::formats::{Cue, FormatOptions, FormatReader, Packet, SeekMode, SeekTo, SeekedTo, Track};
use symphonia::core::io::{MediaSourceStream, ReadBytes};
use symphonia::core::meta::{Metadata, MetadataLog};
use symphonia::core::probe::{Descriptor, Instantiate, QueryDescriptor};
use symphonia::core::units::TimeBase;

use super::opus::channels_spec;

/// Frames of audio in one AAC packet.
const FRAMES: u64 = 1024;
const RATES: [u32; 13] = [96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Header {
    /// Audio object type (1 = Main, 2 = LC, …).
    object: u8,
    rate_index: u8,
    channels: u8,
    /// Header bytes (7, or 9 with a CRC).
    head: u8,
    /// Whole frame, header included.
    len: u16,
}

fn parse(b: &[u8; 7]) -> Option<Header> {
    if b[0] != 0xFF || b[1] & 0xF6 != 0xF0 {
        return None;
    }
    let crc = b[1] & 1 == 0;
    let object = (b[2] >> 6) + 1;
    let rate_index = (b[2] >> 2) & 0xF;
    let channels = ((b[2] & 1) << 2) | (b[3] >> 6);
    let len = (u16::from(b[3] & 3) << 11) | (u16::from(b[4]) << 3) | u16::from(b[5] >> 5);
    let head = if crc { 9 } else { 7 };
    let blocks = (b[6] & 3) + 1;
    (usize::from(rate_index) < RATES.len() && len > head && blocks == 1).then_some(Header { object, rate_index, channels, head: head as u8, len })
}

/// The AudioSpecificConfig the AAC decoder reads.
fn asc(h: &Header) -> [u8; 2] {
    [(h.object << 3) | (h.rate_index >> 1), ((h.rate_index & 1) << 7) | (h.channels << 3)]
}

pub struct AdtsReader {
    reader: MediaSourceStream,
    tracks: Vec<Track>,
    /// Byte offset and header of every frame.
    frames: Vec<(u64, Header)>,
    next: usize,
    metadata: MetadataLog,
}

impl QueryDescriptor for AdtsReader {
    fn query() -> &'static [Descriptor] {
        &[Descriptor {
            short_name: "adts",
            long_name: "Audio Data Transport Stream (raw AAC)",
            extensions: &["aac"],
            mime_types: &["audio/aac"],
            markers: &[&[0xFF, 0xF1], &[0xFF, 0xF9], &[0xFF, 0xF0], &[0xFF, 0xF8]],
            score: Self::score,
            inst: Instantiate::Format(|source, opts| Ok(Box::new(AdtsReader::try_new(source, opts)?))),
        }]
    }

    fn score(_: &[u8]) -> u8 {
        255
    }
}

impl FormatReader for AdtsReader {
    fn try_new(mut reader: MediaSourceStream, _: &FormatOptions) -> Result<Self> {
        let mut frames = Vec::new();
        let mut pos = reader.pos();
        let mut b = [0u8; 7];
        // Index frames until the stream ends or stops being ADTS (a
        // trailing tag, say).
        while reader.read_buf_exact(&mut b).is_ok() {
            let Some(h) = parse(&b) else { break };
            if frames.first().is_some_and(|(_, f): &(u64, Header)| f.rate_index != h.rate_index) {
                break;
            }
            frames.push((pos, h));
            pos += u64::from(h.len);
            if reader.ignore_bytes(u64::from(h.len) - 7).is_err() {
                frames.pop();
                break;
            }
        }
        let Some(&(start, first)) = frames.first() else {
            return Err(Error::DecodeError("adts: no frames"));
        };
        let rate = RATES[usize::from(first.rate_index)];
        let mut params = CodecParameters::new();
        params
            .for_codec(CODEC_TYPE_AAC)
            .with_sample_rate(rate)
            .with_time_base(TimeBase::new(1, rate))
            .with_n_frames(frames.len() as u64 * FRAMES)
            .with_extra_data(Box::new(asc(&first)));
        if first.channels > 0 {
            params.with_channels(channels_spec(usize::from(first.channels)));
        }
        reader.seek(SeekFrom::Start(start))?;
        Ok(AdtsReader { reader, tracks: vec![Track::new(0, params)], frames, next: 0, metadata: MetadataLog::default() })
    }

    fn cues(&self) -> &[Cue] {
        &[]
    }

    fn metadata(&mut self) -> Metadata<'_> {
        self.metadata.metadata()
    }

    fn seek(&mut self, _: SeekMode, to: SeekTo) -> Result<SeekedTo> {
        let rate = self.tracks[0].codec_params.sample_rate.unwrap_or(1);
        let required_ts = match to {
            SeekTo::TimeStamp { ts, .. } => ts,
            SeekTo::Time { time, .. } => TimeBase::new(1, rate).calc_timestamp(time),
        };
        let index = ((required_ts / FRAMES) as usize).min(self.frames.len());
        if index == self.frames.len() && index > 0 && required_ts > self.frames.len() as u64 * FRAMES {
            return Err(Error::SeekError(SeekErrorKind::OutOfRange));
        }
        self.next = index;
        if let Some(&(pos, _)) = self.frames.get(index) {
            self.reader.seek(SeekFrom::Start(pos))?;
        }
        Ok(SeekedTo { track_id: 0, required_ts, actual_ts: index as u64 * FRAMES })
    }

    fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    fn next_packet(&mut self) -> Result<Packet> {
        let Some(&(pos, h)) = self.frames.get(self.next) else {
            return Err(Error::IoError(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "end of stream")));
        };
        if self.reader.pos() != pos + u64::from(h.head) {
            self.reader.seek(SeekFrom::Start(pos + u64::from(h.head)))?;
        }
        let data = self.reader.read_boxed_slice_exact(usize::from(h.len - u16::from(h.head)))?;
        let ts = self.next as u64 * FRAMES;
        self.next += 1;
        Ok(Packet::new_from_boxed_slice(0, ts, FRAMES, data))
    }

    fn into_inner(self: Box<Self>) -> MediaSourceStream {
        self.reader
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_both_header_kinds() {
        // MPEG-2, no CRC, LC, 16 kHz, mono, 352-byte frame.
        let h = parse(&[0xFF, 0xF9, 0x60, 0x40, 0x2C, 0x1F, 0xFC]).unwrap();
        assert_eq!((h.object, h.rate_index, h.channels, h.head, h.len), (2, 8, 1, 7, 352));
        assert_eq!(asc(&h), [0x14, 0x08]);
        // MPEG-4 with a CRC.
        let h = parse(&[0xFF, 0xF0, 0x50, 0x80, 0x2C, 0x1F, 0xFC]).unwrap();
        assert_eq!((h.object, h.rate_index, h.channels, h.head), (2, 4, 2, 9));
        assert!(parse(&[0xFF, 0xFB, 0x90, 0x64, 0, 0, 0]).is_none(), "an MP3 frame");
        assert!(parse(&[0xFF, 0xF1, 0x7C, 0x40, 0x2C, 0x1F, 0xFC]).is_none(), "reserved rate");
    }
}
