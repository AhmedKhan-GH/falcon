//! Test clips for falcon's own tests: AVI files written byte by byte, with
//! uncompressed BGR pictures and 16-bit PCM sound, so no encoder is needed
//! (falcon's FFmpeg build has none) and every pixel's value is known.

/// An AVI clip to write: `frames` pictures of `width` × `height` at `fps`,
/// with a 440 Hz stereo tone at `audio_rate` samples per second if given.
/// `width` should be a multiple of 4 (AVI pads rows to 4 bytes).
#[derive(Clone, Copy, Debug)]
pub struct Clip {
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub fps: u32,
    pub audio_rate: Option<u32>,
}

fn le16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn le32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn chunk(id: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut v = id.to_vec();
    le32(&mut v, data.len() as u32);
    v.extend_from_slice(data);
    if data.len() % 2 == 1 {
        v.push(0);
    }
    v
}

fn list(kind: &[u8; 4], content: &[u8]) -> Vec<u8> {
    let mut data = kind.to_vec();
    data.extend_from_slice(content);
    chunk(b"LIST", &data)
}

impl Clip {
    /// The file's bytes; `pixel(i, y)` is the RGB of row `y` (from the top)
    /// of frame `i`.
    pub fn avi(&self, pixel: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        let Clip { width: w, height: h, frames, fps, audio_rate } = *self;
        let frame_bytes = w * h * 3;
        let streams = if audio_rate.is_some() { 2 } else { 1 };

        let mut avih = Vec::new();
        for x in [1_000_000 / fps, 0, 0, 0x10, frames, 0, streams, frame_bytes, w, h, 0, 0, 0, 0] {
            le32(&mut avih, x);
        }
        let mut strh = b"vids".to_vec();
        strh.extend_from_slice(&[0; 4]);
        for x in [0u32, 0, 0, 1, fps, 0, frames, frame_bytes, u32::MAX, 0] {
            le32(&mut strh, x);
        }
        for x in [0u16, 0, w as u16, h as u16] {
            le16(&mut strh, x);
        }
        let mut strf = Vec::new();
        for x in [40, w, h] {
            le32(&mut strf, x);
        }
        le16(&mut strf, 1);
        le16(&mut strf, 24);
        for x in [0, frame_bytes, 0, 0, 0, 0] {
            le32(&mut strf, x);
        }
        let mut hdrl = chunk(b"avih", &avih);
        hdrl.extend(list(b"strl", &[chunk(b"strh", &strh), chunk(b"strf", &strf)].concat()));
        if let Some(rate) = audio_rate {
            let per_frame = rate / fps;
            let mut ah = b"auds".to_vec();
            ah.extend_from_slice(&[0; 4]);
            for x in [0u32, 0, 0, 4, rate * 4, 0, per_frame * frames, per_frame * 4, u32::MAX, 4] {
                le32(&mut ah, x);
            }
            ah.extend_from_slice(&[0; 8]);
            let mut af = Vec::new();
            le16(&mut af, 1);
            le16(&mut af, 2);
            le32(&mut af, rate);
            le32(&mut af, rate * 4);
            le16(&mut af, 4);
            le16(&mut af, 16);
            le16(&mut af, 0);
            hdrl.extend(list(b"strl", &[chunk(b"strh", &ah), chunk(b"strf", &af)].concat()));
        }

        let mut movi = Vec::new();
        let mut idx = Vec::new();
        let mut add = |movi: &mut Vec<u8>, id: &[u8; 4], data: &[u8]| {
            idx.extend_from_slice(id);
            le32(&mut idx, 0x10);
            le32(&mut idx, movi.len() as u32 + 4);
            le32(&mut idx, data.len() as u32);
            movi.extend(chunk(id, data));
        };
        for i in 0..frames {
            // Bottom-up rows of BGR.
            let mut px = Vec::with_capacity(frame_bytes as usize);
            for row in (0..h).rev() {
                let [r, g, b] = pixel(i, row);
                for _ in 0..w {
                    px.extend_from_slice(&[b, g, r]);
                }
            }
            add(&mut movi, b"00db", &px);
            if let Some(rate) = audio_rate {
                let per_frame = rate / fps;
                let mut pcm = Vec::new();
                for n in 0..per_frame {
                    let t = f64::from(i * per_frame + n) / f64::from(rate);
                    let s = ((t * 440.0 * std::f64::consts::TAU).sin() * 8000.0) as i16;
                    pcm.extend_from_slice(&s.to_le_bytes());
                    pcm.extend_from_slice(&s.to_le_bytes());
                }
                add(&mut movi, b"01wb", &pcm);
            }
        }
        let mut body = b"AVI ".to_vec();
        body.extend(list(b"hdrl", &hdrl));
        body.extend(list(b"movi", &movi));
        body.extend(chunk(b"idx1", &idx));
        chunk(b"RIFF", &body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avi_files_are_riff_with_an_index() {
        let clip = Clip { width: 4, height: 2, frames: 3, fps: 10, audio_rate: Some(1000) };
        let bytes = clip.avi(|_, _| [1, 2, 3]);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize, bytes.len() - 8);
        assert_eq!(&bytes[8..12], b"AVI ");
        assert!(bytes.windows(4).any(|w| w == b"idx1"));
        // 3 pictures of 24 bytes and 3 blocks of 100 samples × 4 bytes.
        assert_eq!(bytes.windows(4).filter(|w| *w == b"00db").count(), 3 + 3);
        assert!(bytes.windows(3).any(|w| w == [3, 2, 1]), "pixels are stored BGR");
    }
}
