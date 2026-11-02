//! Durations of audio and video files, stored at import as `duration`
//! (seconds, with a fraction, as a browser's media element reports it).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;


/// Eagle's audio formats.
pub const AUDIO_EXTENSIONS: [&str; 6] = ["mp3", "wav", "flac", "ogg", "aac", "m4a"];
/// Eagle's video formats.
pub const VIDEO_EXTENSIONS: [&str; 18] =
    ["ts", "3gp", "360", "afx", "vap", "eva", "mp4", "mov", "m4v", "webm", "mkv", "avi", "wmv", "mpg", "mts", "flv", "m2ts", "f4v"];

pub fn is_audio(ext: &str) -> bool {
    AUDIO_EXTENSIONS.contains(&ext)
}

pub fn is_video(ext: &str) -> bool {
    VIDEO_EXTENSIONS.contains(&ext)
}

/// The duration in seconds of an audio or video file falcon can read:
/// the audio formats through [`crate::audio`], ISO-media video (mp4, mov,
/// m4v, 3gp, f4v) from its movie header, and other video through FFmpeg
/// when it is installed.
pub fn duration(path: &Path, ext: &str) -> Option<f64> {
    if is_audio(ext) {
        return audio_duration(path, ext);
    }
    if matches!(ext, "mp4" | "mov" | "m4v" | "3gp" | "f4v")
        && let Some(d) = File::open(path).ok().and_then(|mut f| mp4_duration(&mut f))
    {
        return Some(d);
    }
    if is_video(ext) && falcon_ffmpeg::available() {
        return falcon_ffmpeg::probe(path).ok().map(|i| i.duration).filter(|d| *d > 0.0);
    }
    None
}

/// An audio file's length: from its header, or by adding up packet
/// durations when the header doesn't say (an MP3 without a Xing header).
/// `None` when the file doesn't decode (see [`crate::audio`]).
pub fn audio_duration(path: &Path, ext: &str) -> Option<f64> {
    let source = crate::audio::open(path, ext).ok()?;
    if let Some(d) = source.duration() {
        return Some(d);
    }
    drop(source);
    let mut format = crate::audio::open_format(path, ext).ok()?;
    let track = format.default_track()?;
    let id = track.id;
    let params = track.codec_params.clone();
    let mut total = 0u64;
    while let Ok(p) = format.next_packet() {
        if p.track_id() == id {
            total += p.dur;
        }
    }
    let secs = if let Some(tb) = params.time_base {
        let t = tb.calc_time(total);
        t.seconds as f64 + t.frac
    } else {
        total as f64 / f64::from(params.sample_rate?)
    };
    (secs > 0.0).then_some(secs)
}

/// Reads `moov/mvhd` and returns duration ÷ timescale.
pub fn mp4_duration<R: Read + Seek>(r: &mut R) -> Option<f64> {
    let end = r.seek(SeekFrom::End(0)).ok()?;
    let (start, size) = find_box(r, 0, end, b"moov")?;
    let (mvhd, _) = find_box(r, start, start + size, b"mvhd")?;
    r.seek(SeekFrom::Start(mvhd)).ok()?;
    let mut head = [0u8; 4];
    r.read_exact(&mut head).ok()?;
    let (timescale, duration) = if head[0] == 1 {
        let mut b = [0u8; 28];
        r.read_exact(&mut b).ok()?;
        (u32::from_be_bytes(b[16..20].try_into().ok()?), u64::from_be_bytes(b[20..28].try_into().ok()?))
    } else {
        let mut b = [0u8; 16];
        r.read_exact(&mut b).ok()?;
        (u32::from_be_bytes(b[8..12].try_into().ok()?), u64::from(u32::from_be_bytes(b[12..16].try_into().ok()?)))
    };
    (timescale > 0 && duration > 0 && duration != u64::from(u32::MAX)).then(|| duration as f64 / f64::from(timescale))
}

/// Finds a box by type among the boxes in `[from, to)`; returns the offset
/// of its payload and the payload's size.
fn find_box<R: Read + Seek>(r: &mut R, mut at: u64, to: u64, kind: &[u8; 4]) -> Option<(u64, u64)> {
    while at + 8 <= to {
        r.seek(SeekFrom::Start(at)).ok()?;
        let mut h = [0u8; 8];
        r.read_exact(&mut h).ok()?;
        let mut size = u64::from(u32::from_be_bytes(h[..4].try_into().ok()?));
        let mut header = 8;
        if size == 1 {
            let mut l = [0u8; 8];
            r.read_exact(&mut l).ok()?;
            size = u64::from_be_bytes(l);
            header = 16;
        } else if size == 0 {
            size = to - at;
        }
        if size < header {
            return None;
        }
        if &h[4..8] == kind {
            return Some((at + header, size - header));
        }
        at += size;
    }
    None
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Cursor;

    /// A mono 16-bit WAV of `secs` seconds at `rate` Hz, from `f(t)`.
    pub(crate) fn wav(rate: u32, secs: f64, f: impl Fn(f64) -> f64) -> Vec<u8> {
        let n = (f64::from(rate) * secs) as u32;
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&(36 + n * 2).to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&rate.to_le_bytes());
        v.extend_from_slice(&(rate * 2).to_le_bytes());
        v.extend_from_slice(&2u16.to_le_bytes());
        v.extend_from_slice(&16u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&(n * 2).to_le_bytes());
        for i in 0..n {
            let s = (f(f64::from(i) / f64::from(rate)).clamp(-1.0, 1.0) * 32767.0) as i16;
            v.extend_from_slice(&s.to_le_bytes());
        }
        v
    }

    #[test]
    fn wav_duration() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("tone.wav");
        std::fs::write(&p, wav(8000, 2.5, |t| (t * 440.0 * std::f64::consts::TAU).sin())).unwrap();
        let d = duration(&p, "wav").unwrap();
        assert!((d - 2.5).abs() < 1e-6, "{d}");
    }

    #[test]
    fn mp4_header() {
        let mvhd = |version: u8| {
            let mut b = vec![version, 0, 0, 0];
            if version == 1 {
                b.extend_from_slice(&[0; 16]);
                b.extend_from_slice(&600u32.to_be_bytes());
                b.extend_from_slice(&9000u64.to_be_bytes());
            } else {
                b.extend_from_slice(&[0; 8]);
                b.extend_from_slice(&600u32.to_be_bytes());
                b.extend_from_slice(&9000u32.to_be_bytes());
            }
            b.extend_from_slice(&[0; 80]);
            boxed(b"mvhd", &b)
        };
        for v in [0, 1] {
            let mut file = boxed(b"ftyp", b"isom\0\0\0\0");
            file.extend(boxed(b"free", &[0; 3]));
            file.extend(boxed(b"moov", &mvhd(v)));
            assert_eq!(mp4_duration(&mut Cursor::new(file)), Some(15.0));
        }
        assert_eq!(mp4_duration(&mut Cursor::new(boxed(b"ftyp", b"isom"))), None);
    }

    fn boxed(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend_from_slice(kind);
        v.extend_from_slice(payload);
        v
    }
}
