//! AV1 test clips in the containers falcon meets them in. The build has no
//! AV1 encoder, so the pictures come from an IVF file encoded elsewhere
//! (`testdata/av1.ivf`); these functions only rewrap its temporal units
//! as WebM, Matroska or MP4, writing every box and element by hand.

/// An AV1 stream: one temporal unit (the OBUs of one picture) per frame.
#[derive(Clone, Debug)]
pub struct Av1Clip {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub frames: Vec<Vec<u8>>,
}

/// OBU types used below.
const OBU_SEQUENCE_HEADER: u8 = 1;

/// Reads a LEB128 number from the front of `b`: the value and its length.
fn leb128(b: &[u8]) -> Option<(usize, usize)> {
    let mut v = 0usize;
    for (i, byte) in b.iter().enumerate().take(8) {
        v |= usize::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            return Some((v, i + 1));
        }
    }
    None
}

/// The OBUs of a temporal unit in the low-overhead format (each with a
/// size field), as (type, whole OBU, payload).
fn obus(tu: &[u8]) -> Vec<(u8, &[u8], &[u8])> {
    let mut out = Vec::new();
    let mut p = 0;
    while p < tu.len() {
        let header = tu[p];
        let kind = (header >> 3) & 0x0f;
        let ext = usize::from(header >> 2 & 1);
        let Some((size, n)) = leb128(&tu[p + 1 + ext..]) else { break };
        let start = p + 1 + ext + n;
        let end = (start + size).min(tu.len());
        out.push((kind, &tu[p..end], &tu[start..end]));
        p = end;
    }
    out
}

impl Av1Clip {
    /// Parses an IVF file (32-byte header, then a 12-byte header before
    /// each frame) holding AV1 at a whole number of frames per second.
    pub fn from_ivf(b: &[u8]) -> Option<Self> {
        let u16_at = |p: usize| u16::from_le_bytes([b[p], b[p + 1]]);
        let u32_at = |p: usize| u32::from_le_bytes(b[p..p + 4].try_into().unwrap());
        if b.len() < 32 || &b[0..4] != b"DKIF" || &b[8..12] != b"AV01" {
            return None;
        }
        let (width, height) = (u32::from(u16_at(12)), u32::from(u16_at(14)));
        let (rate, scale) = (u32_at(16), u32_at(20).max(1));
        let mut frames = Vec::new();
        let mut p = usize::from(u16_at(6));
        while p + 12 <= b.len() {
            let size = u32_at(p) as usize;
            frames.push(b.get(p + 12..p + 12 + size)?.to_vec());
            p += 12 + size;
        }
        Some(Av1Clip { width, height, fps: rate / scale, frames })
    }

    /// Whether frame `i` starts a new sequence: it carries a sequence
    /// header, which encoders (rav1e among them) repeat at each keyframe.
    pub fn is_key(&self, i: usize) -> bool {
        obus(&self.frames[i]).iter().any(|(kind, _, _)| *kind == OBU_SEQUENCE_HEADER)
    }

    /// The `av1C` record (AV1 codec configuration) that MP4 and Matroska
    /// carry: profile, level and tier read from the sequence header, the
    /// colour fields for 8-bit 4:2:0, and the sequence header OBU itself.
    pub fn av1c(&self) -> Vec<u8> {
        let (obu, payload) = obus(&self.frames[0])
            .into_iter()
            .find(|(kind, _, _)| *kind == OBU_SEQUENCE_HEADER)
            .map(|(_, obu, payload)| (obu, payload))
            .expect("the first frame carries the sequence header");
        let bit = |n: usize| payload[n / 8] >> (7 - n % 8) & 1;
        let bits = |from: usize, count: usize| (from..from + count).fold(0u8, |v, n| v << 1 | bit(n));
        let profile = bits(0, 3);
        // Without timing info: one bit for the display delay flag, five for
        // the operating point count, twelve for the first one's idc.
        let reduced = bit(4) == 1;
        let (level, tier) = if reduced {
            (bits(5, 5), 0)
        } else {
            assert_eq!(bit(5), 0, "timing info is not parsed");
            let level = bits(25, 5);
            (level, if level > 7 { bit(30) } else { 0 })
        };
        let mut v = vec![0x81, profile << 5 | level, tier << 7 | 0b0000_1100, 0];
        v.extend_from_slice(obu);
        v
    }

    fn duration_ms(&self) -> f64 {
        self.frames.len() as f64 * 1000.0 / f64::from(self.fps)
    }

    fn frame_ms(&self, i: usize) -> u64 {
        i as u64 * 1000 / u64::from(self.fps)
    }

    /// The clip as a WebM (`webm`) or Matroska file: one cluster per
    /// keyframe, with cues pointing at them.
    pub fn matroska(&self, webm: bool) -> Vec<u8> {
        let doc: &[u8] = if webm { b"webm" } else { b"matroska" };
        let header = el(
            0x1A45DFA3,
            &[uint(0x4286, 1), uint(0x42F7, 1), uint(0x42F2, 4), uint(0x42F3, 8), el(0x4282, doc), uint(0x4287, 4), uint(0x4285, 2)]
                .concat(),
        );
        let info = el(
            0x1549A966,
            &[uint(0x2AD7B1, 1_000_000), el(0x4489, &self.duration_ms().to_be_bytes()), el(0x4D80, b"falcon tests"), el(0x5741, b"falcon tests")]
                .concat(),
        );
        let video = el(0xE0, &[uint(0xB0, u64::from(self.width)), uint(0xBA, u64::from(self.height))].concat());
        let track = el(
            0xAE,
            &[
                uint(0xD7, 1),
                uint(0x73C5, 1),
                uint(0x83, 1),
                el(0x86, b"V_AV1"),
                el(0x63A2, &self.av1c()),
                uint(0x23E383, 1_000_000_000 / u64::from(self.fps)),
                video,
            ]
            .concat(),
        );
        let tracks = el(0x1654AE6B, &track);

        let mut clusters = Vec::new();
        let mut starts = Vec::new();
        let mut i = 0;
        while i < self.frames.len() {
            let start_ms = self.frame_ms(i);
            starts.push((start_ms, clusters.len()));
            let mut body = uint(0xE7, start_ms);
            loop {
                let mut block = vec![0x81];
                block.extend_from_slice(&((self.frame_ms(i) - start_ms) as i16).to_be_bytes());
                block.push(if self.is_key(i) { 0x80 } else { 0 });
                block.extend_from_slice(&self.frames[i]);
                body.extend(el(0xA3, &block));
                i += 1;
                if i == self.frames.len() || self.is_key(i) {
                    break;
                }
            }
            clusters.extend(el(0x1F43B675, &body));
        }
        // Cluster positions count from the start of the segment's data.
        let before = info.len() + tracks.len();
        let cues: Vec<u8> = starts
            .iter()
            .map(|&(ms, at)| el(0xBB, &[uint(0xB3, ms), el(0xB7, &[uint(0xF7, 1), uint(0xF1, (before + at) as u64)].concat())].concat()))
            .collect::<Vec<_>>()
            .concat();
        let segment = el(0x18538067, &[info, tracks, clusters, el(0x1C53BB6B, &cues)].concat());
        [header, segment].concat()
    }

    /// The clip as an MP4 file: `moov` first, then one chunk of samples
    /// in `mdat`, with keyframes listed in `stss`.
    pub fn mp4(&self) -> Vec<u8> {
        const TIMESCALE: u32 = 12_800;
        let delta = TIMESCALE / self.fps;
        let n = self.frames.len() as u32;
        let ms = self.duration_ms() as u32;
        let ftyp = mp4_box(b"ftyp", &[&b"isom"[..], &be32(0x200), b"isom", b"iso6", b"av01", b"mp41"].concat());
        let matrix: Vec<u8> = [0x0001_0000u32, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000].iter().flat_map(|v| v.to_be_bytes()).collect();

        let moov = |chunk_offset: u32| {
            let mvhd = full_box(
                b"mvhd",
                0,
                &[&be32(0)[..], &be32(0), &be32(1000), &be32(ms), &be32(0x0001_0000), &[1, 0], &[0; 10], &matrix, &[0; 24], &be32(2)].concat(),
            );
            let tkhd = full_box(
                b"tkhd",
                3,
                &[
                    &be32(0)[..],
                    &be32(0),
                    &be32(1),
                    &be32(0),
                    &be32(ms),
                    &[0; 8],
                    &[0; 8],
                    &matrix,
                    &be32(self.width << 16),
                    &be32(self.height << 16),
                ]
                .concat(),
            );
            let mdhd = full_box(b"mdhd", 0, &[&be32(0)[..], &be32(0), &be32(TIMESCALE), &be32(n * delta), &[0x55, 0xC4, 0, 0]].concat());
            let hdlr = full_box(b"hdlr", 0, &[&be32(0)[..], b"vide", &[0; 12], b"VideoHandler\0"].concat());
            let vmhd = full_box(b"vmhd", 1, &[0; 8]);
            let dinf = mp4_box(b"dinf", &full_box(b"dref", 0, &[&be32(1)[..], &full_box(b"url ", 1, &[])].concat()));
            let entry = mp4_box(
                b"av01",
                &[
                    &[0u8; 6][..],
                    &[0, 1],
                    &[0; 16],
                    &(self.width as u16).to_be_bytes(),
                    &(self.height as u16).to_be_bytes(),
                    &be32(0x0048_0000),
                    &be32(0x0048_0000),
                    &be32(0),
                    &[0, 1],
                    &[0; 32],
                    &[0, 0x18, 0xff, 0xff],
                    &mp4_box(b"av1C", &self.av1c()),
                ]
                .concat(),
            );
            let stsd = full_box(b"stsd", 0, &[&be32(1)[..], &entry].concat());
            let stts = full_box(b"stts", 0, &[be32(1), be32(n), be32(delta)].concat());
            let keys: Vec<u32> = (0..self.frames.len()).filter(|&i| self.is_key(i)).map(|i| i as u32 + 1).collect();
            let stss = full_box(b"stss", 0, &[be32(keys.len() as u32), keys.iter().flat_map(|k| k.to_be_bytes()).collect()].concat());
            let stsc = full_box(b"stsc", 0, &[be32(1), be32(1), be32(n), be32(1)].concat());
            let sizes: Vec<u8> = self.frames.iter().flat_map(|f| (f.len() as u32).to_be_bytes()).collect();
            let stsz = full_box(b"stsz", 0, &[be32(0), be32(n), sizes].concat());
            let stco = full_box(b"stco", 0, &[be32(1), be32(chunk_offset)].concat());
            let stbl = mp4_box(b"stbl", &[stsd, stts, stss, stsc, stsz, stco].concat());
            let minf = mp4_box(b"minf", &[vmhd, dinf, stbl].concat());
            let mdia = mp4_box(b"mdia", &[mdhd, hdlr, minf].concat());
            let trak = mp4_box(b"trak", &[tkhd, mdia].concat());
            mp4_box(b"moov", &[mvhd, trak].concat())
        };
        // The offset doesn't change moov's size, so measure with a dummy.
        let offset = (ftyp.len() + moov(0).len() + 8) as u32;
        let mdat = mp4_box(b"mdat", &self.frames.concat());
        [ftyp, moov(offset), mdat].concat()
    }
}

/// A Matroska element: ID, size (as an eight-byte vint) and data.
pub(super) fn el(id: u32, data: &[u8]) -> Vec<u8> {
    let id = id.to_be_bytes();
    let skip = id.iter().take_while(|b| **b == 0).count();
    let mut v = id[skip..].to_vec();
    v.push(0x01);
    v.extend_from_slice(&(data.len() as u64).to_be_bytes()[1..]);
    v.extend_from_slice(data);
    v
}

/// A Matroska unsigned integer element, in eight bytes.
pub(super) fn uint(id: u32, value: u64) -> Vec<u8> {
    el(id, &value.to_be_bytes())
}

fn be32(v: u32) -> Vec<u8> {
    v.to_be_bytes().to_vec()
}

fn mp4_box(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut v = be32(8 + data.len() as u32);
    v.extend_from_slice(kind);
    v.extend_from_slice(data);
    v
}

/// A box with a version (0 here) and 24 bits of flags.
fn full_box(kind: &[u8; 4], flags: u32, data: &[u8]) -> Vec<u8> {
    mp4_box(kind, &[&be32(flags & 0x00ff_ffff)[..], data].concat())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip() -> Av1Clip {
        Av1Clip::from_ivf(include_bytes!("../../testdata/av1.ivf")).unwrap()
    }

    #[test]
    fn reads_the_ivf_fixture() {
        let c = clip();
        assert_eq!((c.width, c.height, c.fps, c.frames.len()), (64, 48, 25, 25));
        let keys: Vec<usize> = (0..c.frames.len()).filter(|&i| c.is_key(i)).collect();
        assert_eq!(keys, [0, 13]);
    }

    #[test]
    fn av1c_carries_the_sequence_header() {
        let v = clip().av1c();
        assert_eq!(v[0], 0x81, "marker and version 1");
        assert_eq!(v[1] >> 5, 0, "main profile");
        assert_eq!(v[4] >> 3 & 0x0f, OBU_SEQUENCE_HEADER);
    }

    #[test]
    fn containers_have_their_signatures() {
        let c = clip();
        let mkv = c.matroska(true);
        assert_eq!(&mkv[0..4], [0x1A, 0x45, 0xDF, 0xA3]);
        assert!(mkv.windows(5).any(|w| w == b"V_AV1"));
        let mp4 = c.mp4();
        assert_eq!(&mp4[4..8], b"ftyp");
        let total: usize = c.frames.iter().map(Vec::len).sum();
        assert_eq!(&mp4[mp4.len() - total - 4..mp4.len() - total], b"mdat");
    }
}
