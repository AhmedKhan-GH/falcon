//! Audio waveforms: decoding a file to mono peaks and drawing the waveform
//! picture Eagle uses as an audio item's thumbnail (see the Thumbnails
//! chapter of the docs). Eagle caches no peak data on disk, so neither does
//! falcon.

use std::path::Path;

use image::{Rgba, RgbaImage};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;

use crate::media;

/// Size Eagle records for an audio item and draws its waveform at.
pub const WIDTH: u32 = 280;
pub const HEIGHT: u32 = 140;
/// Eagle's waveform colour, #0072EF.
pub const COLOR: [u8; 3] = [0x00, 0x72, 0xEF];

/// Frames folded into one block on the first pass, before the file's
/// length is known.
const BLOCK: usize = 256;

/// Decodes an audio file and returns `buckets` (min, max) pairs of the mono
/// mix, in -1..=1. `None` if it can't be decoded.
pub fn peaks(path: &Path, ext: &str, buckets: usize) -> Option<Vec<(f32, f32)>> {
    let mut format = media::open_audio(path, ext)?;
    let track = format.default_track()?;
    let id = track.id;
    let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default()).ok()?;
    let mut blocks: Vec<(f32, f32)> = Vec::new();
    let (mut lo, mut hi, mut n) = (f32::MAX, f32::MIN, 0usize);
    let mut buf: Option<SampleBuffer<f32>> = None;
    while let Ok(packet) = format.next_packet() {
        if packet.track_id() != id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(_) => break,
        };
        let spec = *decoded.spec();
        let channels = spec.channels.count().max(1);
        let sb = match &mut buf {
            Some(b) if b.capacity() >= decoded.capacity() * channels => b,
            _ => buf.insert(SampleBuffer::new(decoded.capacity() as u64, spec)),
        };
        sb.copy_interleaved_ref(decoded);
        for frame in sb.samples().chunks(channels) {
            let s = frame.iter().sum::<f32>() / channels as f32;
            lo = lo.min(s);
            hi = hi.max(s);
            n += 1;
            if n == BLOCK {
                blocks.push((lo, hi));
                (lo, hi, n) = (f32::MAX, f32::MIN, 0);
            }
        }
    }
    if n > 0 {
        blocks.push((lo, hi));
    }
    (!blocks.is_empty()).then(|| rebucket(&blocks, buckets))
}

/// Merges (min, max) pairs into `buckets` evenly spread groups.
pub fn rebucket(blocks: &[(f32, f32)], buckets: usize) -> Vec<(f32, f32)> {
    let buckets = buckets.max(1);
    (0..buckets)
        .map(|b| {
            let start = b * blocks.len() / buckets;
            let end = ((b + 1) * blocks.len() / buckets).max(start + 1).min(blocks.len());
            blocks.get(start..end).unwrap_or(&[]).iter().fold((0.0f32, 0.0f32), |(l, h), &(a, z)| (l.min(a), h.max(z)))
        })
        .collect()
}

/// Draws peaks as a mirrored waveform, one column per peak, scaled so the
/// loudest peak fills the height (Eagle's waveform is normalised), on a
/// transparent background.
pub fn render(peaks: &[(f32, f32)], height: u32, color: [u8; 3]) -> RgbaImage {
    let width = peaks.len().max(1) as u32;
    let mut img = RgbaImage::new(width, height);
    let loudest = peaks.iter().map(|&(l, h)| l.abs().max(h.abs())).fold(0.0f32, f32::max);
    let scale = if loudest > 0.0 { 1.0 / loudest } else { 0.0 };
    let mid = height as f32 / 2.0;
    for (x, &(l, h)) in peaks.iter().enumerate() {
        let top = (mid - h * scale * mid).floor().clamp(0.0, height as f32 - 1.0) as u32;
        let bottom = (mid - l * scale * mid).ceil().clamp(top as f32 + 1.0, height as f32) as u32;
        for y in top..bottom {
            img.put_pixel(x as u32, y, Rgba([color[0], color[1], color[2], 255]));
        }
    }
    img
}

/// The audio thumbnail: a 280×140 waveform.
pub fn thumbnail(path: &Path, ext: &str) -> Option<RgbaImage> {
    let p = peaks(path, ext, WIDTH as usize)?;
    Some(render(&p, HEIGHT, COLOR))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::tests::wav;

    #[test]
    fn peaks_follow_the_envelope() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("ramp.wav");
        // A tone that gets louder over two seconds.
        std::fs::write(&p, wav(8000, 2.0, |t| (t / 2.0) * (t * 220.0 * std::f64::consts::TAU).sin())).unwrap();
        let peaks = peaks(&p, "wav", 400).unwrap();
        assert_eq!(peaks.len(), 400);
        let amp = |i: usize| peaks[i].1 - peaks[i].0;
        assert!(amp(10) < amp(200) && amp(200) < amp(390), "{} {} {}", amp(10), amp(200), amp(390));
        assert!(peaks[399].1 > 0.9 && peaks[399].0 < -0.9);
    }

    #[test]
    fn renders_a_waveform() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("tone.wav");
        std::fs::write(&p, wav(8000, 1.0, |t| 0.5 * (t * 440.0 * std::f64::consts::TAU).sin())).unwrap();
        let img = thumbnail(&p, "wav").unwrap();
        assert_eq!(img.dimensions(), (WIDTH, HEIGHT));
        // Normalised: a steady tone fills the height in every column.
        assert_eq!(img.get_pixel(140, 1).0, [0x00, 0x72, 0xEF, 255]);
        assert_eq!(img.get_pixel(140, 70).0[3], 255);
        let silent = render(&[(0.0, 0.0); 4], 10, COLOR);
        assert!(silent.pixels().filter(|p| p.0[3] > 0).count() <= 4, "a silent file is a thin line");
    }

    #[test]
    fn rebuckets() {
        let blocks = [(-0.1, 0.1), (-0.5, 0.2), (-0.2, 0.9), (0.0, 0.0)];
        assert_eq!(rebucket(&blocks, 2), [(-0.5, 0.2), (-0.2, 0.9)]);
        assert_eq!(rebucket(&blocks, 8).len(), 8);
    }
}
