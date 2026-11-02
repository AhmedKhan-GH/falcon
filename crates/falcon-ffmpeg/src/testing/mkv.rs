//! A Matroska or WebM file holding only 16-bit PCM sound, written by hand
//! like the other test clips (no encoder is needed), for the tests of how
//! the soundtrack of such a container is decoded.

use super::av1::{el, uint};

/// The file's bytes: `samples` (mono, `rate` per second) in blocks of 20 ms,
/// one cluster a second, with cues pointing at the clusters. `webm` only
/// changes the document type.
pub fn pcm_matroska(webm: bool, rate: u32, samples: &[i16]) -> Vec<u8> {
    let doc: &[u8] = if webm { b"webm" } else { b"matroska" };
    let header = el(
        0x1A45DFA3,
        &[uint(0x4286, 1), uint(0x42F7, 1), uint(0x42F2, 4), uint(0x42F3, 8), el(0x4282, doc), uint(0x4287, 4), uint(0x4285, 2)].concat(),
    );
    let duration_ms = samples.len() as f64 * 1000.0 / f64::from(rate);
    let info = el(0x1549A966, &[uint(0x2AD7B1, 1_000_000), el(0x4489, &duration_ms.to_be_bytes()), el(0x4D80, b"falcon tests"), el(0x5741, b"falcon tests")].concat());
    let audio = el(0xE1, &[el(0xB5, &f64::from(rate).to_be_bytes()), uint(0x9F, 1), uint(0x6264, 16)].concat());
    let track = el(0xAE, &[uint(0xD7, 1), uint(0x73C5, 1), uint(0x83, 2), el(0x86, b"A_PCM/INT/LIT"), audio].concat());
    let tracks = el(0x1654AE6B, &track);

    let block = (rate / 50) as usize;
    let per_cluster = 50;
    let (mut clusters, mut starts) = (Vec::new(), Vec::new());
    for (c, chunk) in samples.chunks(block * per_cluster).enumerate() {
        let start_ms = c as u64 * 1000;
        starts.push((start_ms, clusters.len()));
        let mut body = uint(0xE7, start_ms);
        for (i, part) in chunk.chunks(block).enumerate() {
            let mut b = vec![0x81];
            b.extend_from_slice(&((i * 20) as i16).to_be_bytes());
            b.push(0x80);
            b.extend(part.iter().flat_map(|s| s.to_le_bytes()));
            body.extend(el(0xA3, &b));
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
