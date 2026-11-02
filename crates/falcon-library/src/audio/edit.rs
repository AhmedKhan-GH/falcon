//! Where a file's audio starts and how long it runs, when the container
//! says so but symphonia 0.5 doesn't act on it: the edit list or iTunes
//! gapless tag (`iTunSMPB`) of an MP4 or M4A (encoder priming, 2112 frames
//! for Apple's AAC).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Seconds of priming before the audio and the audio's length.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Edit {
    pub lead: f64,
    pub length: Option<f64>,
}

pub(super) fn read(path: &Path) -> Option<Edit> {
    let mut f = File::open(path).ok()?;
    let mut head = [0u8; 8];
    f.read_exact(&mut head).ok()?;
    if &head[4..] == b"ftyp" {
        mp4(&mut f)
    } else {
        None
    }
}

/// The boxes in `[from, to)`: type, payload offset and payload size.
fn boxes<R: Read + Seek>(r: &mut R, from: u64, to: u64) -> Vec<([u8; 4], u64, u64)> {
    let mut out = Vec::new();
    let mut at = from;
    while at + 8 <= to {
        if r.seek(SeekFrom::Start(at)).is_err() {
            break;
        }
        let mut h = [0u8; 8];
        if r.read_exact(&mut h).is_err() {
            break;
        }
        let mut size = u64::from(u32::from_be_bytes([h[0], h[1], h[2], h[3]]));
        let mut header = 8;
        if size == 1 {
            let mut l = [0u8; 8];
            if r.read_exact(&mut l).is_err() {
                break;
            }
            size = u64::from_be_bytes(l);
            header = 16;
        } else if size == 0 {
            size = to - at;
        }
        if size < header || at + size > to {
            break;
        }
        out.push(([h[4], h[5], h[6], h[7]], at + header, size - header));
        at += size;
    }
    out
}

fn payload<R: Read + Seek>(r: &mut R, at: u64, len: u64) -> Option<Vec<u8>> {
    if len > 1 << 20 {
        return None;
    }
    r.seek(SeekFrom::Start(at)).ok()?;
    let mut b = vec![0u8; len as usize];
    r.read_exact(&mut b).ok()?;
    Some(b)
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn be64(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// Timescale of an `mvhd` or `mdhd` payload.
fn timescale(b: &[u8]) -> Option<u32> {
    be32(b, if b.first() == Some(&1) { 20 } else { 12 }).filter(|t| *t > 0)
}

/// The first edit that plays media; an empty edit (-1) first would delay
/// the start, which falcon doesn't do.
fn elst(e: &[u8], movie_scale: u32, media_scale: u32) -> Option<Edit> {
    let v1 = e.first() == Some(&1);
    let count = be32(e, 4)? as usize;
    let size = if v1 { 20 } else { 12 };
    for i in 0..count {
        let o = 8 + i * size;
        let (duration, time) = if v1 { (be64(e, o)?, be64(e, o + 8)? as i64) } else { (u64::from(be32(e, o)?), i64::from(be32(e, o + 4)? as i32)) };
        if time < 0 {
            continue;
        }
        let length = (duration > 0).then(|| duration as f64 / f64::from(movie_scale));
        return Some(Edit { lead: time as f64 / f64::from(media_scale), length });
    }
    None
}

/// iTunes' gapless tag, somewhere in `moov/udta`: hex fields of which the
/// second is the priming, the third the padding and the fourth the frames
/// of audio.
fn smpb(tags: &[u8], media_scale: u32) -> Option<Edit> {
    let at = tags.windows(8).position(|w| w == b"iTunSMPB")?;
    let rest = &tags[at..];
    let data = rest.windows(4).position(|w| w == b"data")?;
    let size = be32(rest, data.checked_sub(4)?)? as usize;
    let text = std::str::from_utf8(rest.get(data + 12..data - 4 + size)?).ok()?;
    let fields: Vec<u64> = text.split_whitespace().filter_map(|f| u64::from_str_radix(f, 16).ok()).collect();
    let (priming, frames) = (*fields.get(1)?, *fields.get(3)?);
    let scale = f64::from(media_scale);
    (priming > 0 || frames > 0).then(|| Edit { lead: priming as f64 / scale, length: (frames > 0).then(|| frames as f64 / scale) })
}

fn mp4<R: Read + Seek>(r: &mut R) -> Option<Edit> {
    let end = r.seek(SeekFrom::End(0)).ok()?;
    let (_, moov, moov_len) = boxes(r, 0, end).into_iter().find(|b| &b.0 == b"moov")?;
    let top = boxes(r, moov, moov + moov_len);
    let (_, at, len) = *top.iter().find(|b| &b.0 == b"mvhd")?;
    let movie_scale = timescale(&payload(r, at, len)?)?;
    let tags = top.iter().find(|b| &b.0 == b"udta").and_then(|&(_, at, len)| payload(r, at, len));
    for &(kind, at, len) in &top {
        if &kind != b"trak" {
            continue;
        }
        let trak = boxes(r, at, at + len);
        let Some(&(_, mdia, mdia_len)) = trak.iter().find(|b| &b.0 == b"mdia") else { continue };
        let media = boxes(r, mdia, mdia + mdia_len);
        let Some(&(_, at, len)) = media.iter().find(|b| &b.0 == b"hdlr") else { continue };
        if payload(r, at, len).and_then(|h| h.get(8..12).map(|t| t == b"soun")) != Some(true) {
            continue;
        }
        let &(_, at, len) = media.iter().find(|b| &b.0 == b"mdhd")?;
        let media_scale = timescale(&payload(r, at, len)?)?;
        if let Some(&(_, edts, edts_len)) = trak.iter().find(|b| &b.0 == b"edts")
            && let Some((_, at, len)) = boxes(r, edts, edts + edts_len).into_iter().find(|b| &b.0 == b"elst")
            && let Some(e) = payload(r, at, len).and_then(|e| elst(&e, movie_scale, media_scale))
        {
            return Some(e);
        }
        return tags.and_then(|t| smpb(&t, media_scale));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn bx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut v = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend_from_slice(kind);
        v.extend_from_slice(body);
        v
    }

    /// A v0 `mvhd`/`mdhd` payload with `scale`.
    fn header(scale: u32) -> Vec<u8> {
        let mut b = vec![0u8; 12];
        b.extend_from_slice(&scale.to_be_bytes());
        b.extend_from_slice(&[0; 8]);
        b
    }

    #[test]
    fn reads_an_mp4_edit_list() {
        let mut hdlr = vec![0u8; 8];
        hdlr.extend_from_slice(b"soun");
        hdlr.extend_from_slice(&[0; 12]);
        let mut elst = vec![0, 0, 0, 0, 0, 0, 0, 2];
        for (d, t) in [(0u32, -1i32), (600, 2112)] {
            elst.extend_from_slice(&d.to_be_bytes());
            elst.extend_from_slice(&t.to_be_bytes());
            elst.extend_from_slice(&[0, 1, 0, 0]);
        }
        let mdia = bx(b"mdia", &[bx(b"mdhd", &header(16000)), bx(b"hdlr", &hdlr)].concat());
        let trak = bx(b"trak", &[bx(b"edts", &bx(b"elst", &elst)), mdia].concat());
        let file = [bx(b"ftyp", b"M4A \0\0\0\0"), bx(b"moov", &[bx(b"mvhd", &header(600)), trak].concat())].concat();
        let e = mp4(&mut Cursor::new(file)).unwrap();
        assert_eq!(e, Edit { lead: 2112.0 / 16000.0, length: Some(1.0) });
    }

    #[test]
    fn reads_an_itunes_gapless_tag() {
        let text = b" 00000000 00000840 00000140 0000000000003E80 00000000";
        let data = bx(b"data", &[&[0, 0, 0, 1, 0, 0, 0, 0], text.as_slice()].concat());
        let moov = [bx(b"name", b"\0\0\0\0iTunSMPB"), data].concat();
        assert_eq!(smpb(&moov, 16000), Some(Edit { lead: 0.132, length: Some(1.0) }));
        assert_eq!(smpb(b"nothing", 16000), None);
    }
}
