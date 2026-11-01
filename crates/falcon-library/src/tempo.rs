//! Tempo detection for audio items, stored at import as `bpm`.
//!
//! Eagle runs a beat tracker over the decoded audio (the first minutes of
//! long files) and stores the tempo it settles on, between 60 and 200 BPM,
//! with up to three decimals. Falcon estimates the same quantity its own
//! way: a loudness-rise ("onset") envelope at about 100 frames a second,
//! whose autocorrelation is scored by a comb of beat periods over 60–200
//! BPM, with a mild preference for tempos near 120 to settle octave ties.

use std::path::Path;


use crate::media;

/// The tempo range searched, in beats per minute.
pub const MIN_BPM: f64 = 60.0;
pub const MAX_BPM: f64 = 200.0;

/// Peaks below this (about -60 dBFS) count as silence.
const SILENCE: f32 = 0.001;
/// Envelope frames per second (before rounding the hop to whole samples).
const FRAME_RATE: f64 = 100.0;
/// Beat-period multiples the comb adds up.
const COMB: usize = 4;
/// Search step, in BPM.
const STEP: f64 = 0.01;
/// The largest loudness rise (log RMS per frame) must reach this.
const MIN_RISE: f64 = 0.05;
/// The comb score must reach this share of the envelope's energy.
const MIN_PERIODICITY: f64 = 0.15;

/// How much of a file Eagle analyses, by its length in seconds: the first
/// minute of files over an hour, two minutes past ten minutes, three past
/// three, otherwise all of it.
pub fn analysed_seconds(duration: f64) -> Option<f64> {
    if duration > 3600.0 {
        Some(60.0)
    } else if duration > 600.0 {
        Some(120.0)
    } else if duration > 180.0 {
        Some(180.0)
    } else {
        None
    }
}

/// The tempo of an audio file in BPM, rounded to three decimals, or `None`
/// if it can't be decoded or holds no beat (silence, too short).
pub fn bpm(path: &Path, ext: &str) -> Option<f64> {
    let limit = media::audio_duration(path, ext).and_then(analysed_seconds);
    let (samples, rate) = mono(path, ext, limit)?;
    estimate(&samples, rate)
}

/// Decodes the mono mix of an audio file, stopping after `seconds` when
/// given.
fn mono(path: &Path, ext: &str, seconds: Option<f64>) -> Option<(Vec<f32>, u32)> {
    let mut source = crate::audio::open(path, ext).ok()?;
    let rate = source.sample_rate();
    let max = seconds.map_or(usize::MAX, |s| (s * f64::from(rate)) as usize);
    let mut out = Vec::new();
    while out.len() < max {
        let Some(block) = source.next_block() else { break };
        out.extend(block.frames().map(|f| f.iter().sum::<f32>() / f.len() as f32));
    }
    out.truncate(max);
    Some((out, rate))
}

/// Estimates the tempo of mono `samples` at `rate` Hz, in BPM rounded to
/// three decimals.
pub fn estimate(samples: &[f32], rate: u32) -> Option<f64> {
    if rate == 0 || !samples.iter().any(|s| s.abs() >= SILENCE) {
        return None;
    }
    let hop = (f64::from(rate) / FRAME_RATE).round().max(1.0) as usize;
    let fps = f64::from(rate) / hop as f64;
    let onset = onset_envelope(samples, hop);
    let max_lag = (60.0 * fps / MIN_BPM * COMB as f64).ceil() as usize + 2;
    // The longest period has to fit at least twice.
    if onset.len() < 2 * (60.0 * fps / MIN_BPM) as usize + 2 {
        return None;
    }
    let max_lag = max_lag.min(onset.len() - 1);
    let ac: Vec<f64> = (0..=max_lag).map(|lag| onset.iter().zip(&onset[lag..]).map(|(a, b)| a * b).sum()).collect();
    if ac[0] <= 0.0 {
        return None;
    }
    // Catmull-Rom between lags, so a peak can sit between two of them.
    let at = |lag: f64| -> Option<f64> {
        let i = lag.floor() as usize;
        let t = lag - lag.floor();
        let (p0, p1, p2, p3) = (*ac.get(i.checked_sub(1)?)?, *ac.get(i)?, *ac.get(i + 1)?, *ac.get(i + 2)?);
        Some(p1 + 0.5 * t * (p2 - p0 + t * (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3 + t * (3.0 * (p1 - p2) + p3 - p0))))
    };
    let mut best: Option<(f64, f64)> = None;
    let steps = ((MAX_BPM - MIN_BPM) / STEP).round() as usize;
    for s in 0..=steps {
        let bpm = MIN_BPM + s as f64 * STEP;
        let period = 60.0 * fps / bpm;
        let comb: Vec<f64> = (1..=COMB).map_while(|k| at(period * k as f64)).collect();
        if comb.is_empty() {
            continue;
        }
        let score = comb.iter().sum::<f64>() / comb.len() as f64 * prior(bpm);
        if best.is_none_or(|(_, b)| score > b) {
            best = Some((bpm, score));
        }
    }
    let (bpm, score) = best?;
    // No clear rises (a steady tone, a hiss), or no beat standing out
    // against the envelope's own energy: no tempo.
    let rises = onset.iter().copied().fold(f64::MIN, f64::max);
    (rises >= MIN_RISE && score >= MIN_PERIODICITY * ac[0]).then(|| (bpm * 1000.0).round() / 1000.0)
}

/// Rises in loudness, frame by frame: the log RMS over two hops, its
/// positive differences, lightly smoothed and with the mean taken out.
fn onset_envelope(samples: &[f32], hop: usize) -> Vec<f64> {
    let frames = samples.len() / hop;
    let level: Vec<f64> = (0..frames)
        .map(|i| {
            let w = &samples[i * hop..((i + 2) * hop).min(samples.len())];
            let rms = (w.iter().map(|s| f64::from(*s) * f64::from(*s)).sum::<f64>() / w.len() as f64).sqrt();
            (1.0 + 1000.0 * rms).ln()
        })
        .collect();
    let rise: Vec<f64> = level.windows(2).map(|w| (w[1] - w[0]).max(0.0)).collect();
    let smooth: Vec<f64> = (0..rise.len())
        .map(|i| {
            let prev = if i > 0 { rise[i - 1] } else { rise[i] };
            let next = rise.get(i + 1).copied().unwrap_or(rise[i]);
            0.25 * prev + 0.5 * rise[i] + 0.25 * next
        })
        .collect();
    let mean = smooth.iter().sum::<f64>() / smooth.len().max(1) as f64;
    smooth.into_iter().map(|v| v - mean).collect()
}

/// A gentle preference for tempos near 120 BPM (a log-normal weight one
/// octave wide), so that a beat heard at both 60 and 120 reads as 120.
fn prior(bpm: f64) -> f64 {
    let octaves = (bpm / 120.0).log2();
    (-0.5 * octaves * octaves).exp()
}

/// `bpm` as Eagle's JSON holds it: a whole tempo without a fraction.
pub fn json_value(bpm: f64) -> serde_json::Value {
    if bpm.fract() == 0.0 { serde_json::json!(bpm as i64) } else { serde_json::json!(bpm) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Clicks (a 4 ms 1.5 kHz burst) at `bpm` over `secs` seconds.
    fn clicks(bpm: f64, secs: f64) -> impl Fn(f64) -> f64 {
        let beat = 60.0 / bpm;
        move |t: f64| {
            if t > secs {
                return 0.0;
            }
            let since = t % beat;
            if since < 0.004 { 0.8 * (since * 1500.0 * std::f64::consts::TAU).sin() * (1.0 - since / 0.004) } else { 0.0 }
        }
    }

    fn samples(rate: u32, secs: f64, f: impl Fn(f64) -> f64) -> Vec<f32> {
        (0..(f64::from(rate) * secs) as usize).map(|i| f(i as f64 / f64::from(rate)) as f32).collect()
    }

    #[test]
    fn finds_click_tempos() {
        for want in [60.0, 75.0, 90.0, 120.0, 140.0, 174.0] {
            let got = estimate(&samples(22050, 20.0, clicks(want, 20.0)), 22050).unwrap();
            assert!((got - want).abs() <= 1.0, "{want} -> {got}");
        }
    }

    #[test]
    fn silence_and_short_clips_have_no_tempo() {
        assert_eq!(estimate(&vec![0.0; 22050 * 10], 22050), None);
        assert_eq!(estimate(&samples(22050, 1.0, clicks(120.0, 1.0)), 22050), None);
        assert_eq!(estimate(&[], 44100), None);
        assert_eq!(estimate(&[0.5; 100], 0), None);
        // A steady tone and a hiss have no beat either.
        let tone = samples(22050, 10.0, |t| 0.5 * (t * 440.0 * std::f64::consts::TAU).sin());
        assert_eq!(estimate(&tone, 22050), None);
        let mut rng = fastrand::Rng::with_seed(1);
        let hiss: Vec<f32> = (0..22050 * 10).map(|_| rng.f32() - 0.5).collect();
        assert_eq!(estimate(&hiss, 22050), None);
    }

    #[test]
    fn a_beat_over_a_bed_of_sound() {
        // Kicks on the beat, hats on the off-beats, a drone and some hiss.
        let mut rng = fastrand::Rng::with_seed(2);
        let mix: Vec<f32> = (0..22050 * 20)
            .map(|i| {
                let t = i as f64 / 22050.0;
                let (b, e) = (t % 0.5, t % 0.25);
                let kick = if b < 0.08 { (b * 60.0 * std::f64::consts::TAU).sin() * (1.0 - b / 0.08) } else { 0.0 };
                let hat = if e < 0.02 { (rng.f64() - 0.5) * 0.4 * (1.0 - e / 0.02) } else { 0.0 };
                (0.8 * kick + hat + (rng.f64() - 0.5) * 0.1 + 0.2 * (t * 220.0 * std::f64::consts::TAU).sin()) as f32
            })
            .collect();
        let got = estimate(&mix, 22050).unwrap();
        assert!((got - 120.0).abs() <= 1.0, "{got}");
        // At the top of the range the beat reads an octave down, at 100.
        let fast = estimate(&samples(22050, 20.0, clicks(200.0, 20.0)), 22050).unwrap();
        assert!((fast - 100.0).abs() <= 1.0, "{fast}");
    }

    #[test]
    fn long_files_are_sampled_like_eagle() {
        assert_eq!(analysed_seconds(100.0), None);
        assert_eq!(analysed_seconds(200.0), Some(180.0));
        assert_eq!(analysed_seconds(700.0), Some(120.0));
        assert_eq!(analysed_seconds(4000.0), Some(60.0));
        assert_eq!(json_value(120.0).to_string(), "120");
        assert_eq!(json_value(119.985).to_string(), "119.985");
    }

    #[test]
    fn reads_a_wav_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("beat.wav");
        std::fs::write(&p, media::tests::wav(22050, 15.0, clicks(120.0, 15.0))).unwrap();
        let got = bpm(&p, "wav").unwrap();
        assert!((got - 120.0).abs() <= 2.0, "{got}");
        let quiet = dir.path().join("quiet.wav");
        std::fs::write(&quiet, media::tests::wav(22050, 5.0, |_| 0.0)).unwrap();
        assert_eq!(bpm(&quiet, "wav"), None);
    }
}
