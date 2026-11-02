//! Each fixture in `testdata/audio` is one second of mono audio made for
//! these tests: half a second of silence, then a 440 Hz tone. Raw AAC
//! can't say how much encoder priming it starts with, so there the tone
//! comes 2112 frames late; the Vorbis encoder shifts it a little early. The WAV is
//! written here; the rest were encoded from such a WAV (FLAC, AAC,
//! ALAC and Opus with macOS `afconvert`, the Opus packets put in Ogg by
//! hand, MP3 and Vorbis with the pure-Rust `rusty_mp3` and `rusty_vorbis`
//! encoders).

use std::path::{Path, PathBuf};

use super::*;
use crate::{media, waveform};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/audio").join(name)
}

fn wav(dir: &Path) -> PathBuf {
    let p = dir.join("tone.wav");
    std::fs::write(&p, media::tests::wav(8000, 1.0, |t| if t < 0.5 { 0.0 } else { 12000.0 / 32767.0 * (t * 440.0 * std::f64::consts::TAU).sin() })).unwrap();
    p
}

/// The MP3 fixture behind an ID3v2 tag holding a title.
fn tagged_mp3(dir: &Path) -> PathBuf {
    let p = dir.join("tagged.mp3");
    let frame = [b"TIT2".as_slice(), &[0, 0, 0, 5, 0, 0, 3], b"Tone"].concat();
    let mut file = b"ID3\x04\0\0\0\0\0".to_vec();
    file.push(frame.len() as u8);
    file.extend_from_slice(&frame);
    file.extend_from_slice(&std::fs::read(fixture("tone.mp3")).unwrap());
    std::fs::write(&p, file).unwrap();
    p
}

/// Every format the tests cover, with the length its file holds and when
/// the tone starts.
fn all(dir: &Path) -> Vec<(PathBuf, &'static str, f64, f64)> {
    vec![
        (wav(dir), "wav", 1.0, 0.5),
        (fixture("tone.mp3"), "mp3", 1.0, 0.5),
        (tagged_mp3(dir), "mp3", 1.0, 0.5),
        (fixture("tone.flac"), "flac", 1.0, 0.5),
        (fixture("tone.ogg"), "ogg", 0.896, 0.434),
        (fixture("opus.ogg"), "ogg", 1.0, 0.5),
        (fixture("tone.m4a"), "m4a", 1.0, 0.5),
        (fixture("tone.aac"), "aac", 1.152, 0.632),
        (fixture("alac.m4a"), "m4a", 1.0, 0.5),
    ]
}

fn loudest(b: Block<'_>) -> f32 {
    b.samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

/// Seconds from `secs` until the tone starts, after seeking there.
fn onset_after_seek(s: &mut dyn AudioSource, secs: f64) -> f64 {
    let landed = s.seek(secs).unwrap();
    assert!((landed - secs).abs() < 0.01, "asked {secs}, landed {landed}");
    let mut frames = 0usize;
    while let Some(b) = s.next_block() {
        if let Some(i) = b.frames().position(|f| f.iter().any(|x| x.abs() > 0.1)) {
            return (frames + i) as f64 / f64::from(b.rate);
        }
        frames += b.frames().len();
    }
    panic!("no tone after {secs}");
}

#[test]
fn every_format_decodes_with_its_length() {
    let dir = tempfile::tempdir().unwrap();
    for (path, ext, secs, onset) in all(dir.path()) {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let mut s = open(&path, ext).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(s.backend(), "symphonia");
        let d = media::audio_duration(&path, ext).unwrap_or_else(|| panic!("{name}: no duration"));
        assert!((d - secs).abs() < 0.005, "{name}: duration {d}");
        // Count what decodes: the same length, quiet then loud.
        let (mut frames, mut first, mut second, mut heard) = (0usize, 0.0f32, 0.0f32, None);
        let rate = f64::from(s.sample_rate());
        while let Some(b) = s.next_block() {
            for f in b.frames() {
                let (t, v) = (frames as f64 / rate, f[0].abs());
                if t < onset - 0.1 {
                    first = first.max(v);
                } else if t > onset + 0.1 && t < secs - 0.05 {
                    second = second.max(v);
                }
                if v > 0.1 && heard.is_none() {
                    heard = Some(t);
                }
                frames += 1;
            }
        }
        let decoded = frames as f64 / rate;
        assert!((decoded - secs).abs() < 0.005, "{name}: decoded {decoded}s");
        assert!(first < 0.05, "{name}: silent before the tone ({first})");
        assert!(second > 0.25, "{name}: then the tone ({second})");
        let heard = heard.unwrap();
        assert!((heard - onset).abs() < 0.005, "{name}: tone at {heard}");
        assert!((s.position() - decoded).abs() < 1e-6, "{name}: position follows the blocks");
    }
}

#[test]
fn seeks_land_where_asked() {
    let dir = tempfile::tempdir().unwrap();
    for (path, ext, _, onset) in all(dir.path()) {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let mut s = open(&path, ext).unwrap();
        // Forward, back, and again: the tone comes when it would have
        // without the seek.
        for at in [0.3, 0.1, 0.38] {
            let wait = onset_after_seek(s.as_mut(), at);
            assert!((wait - (onset - at)).abs() < 0.005, "{name}: after seeking to {at} the tone came {wait}s later");
        }
        s.seek(0.7).unwrap();
        assert!((s.position() - 0.7).abs() < 0.01, "{name}: position after a seek {}", s.position());
        assert!(loudest(s.next_block().unwrap()) > 0.2, "{name}: inside the tone");
        while s.next_block().is_some() {}
        let wait = onset_after_seek(s.as_mut(), 0.2);
        assert!((wait - (onset - 0.2)).abs() < 0.005, "{name}: seeking back after the end");
    }
}

#[test]
fn waveforms_come_from_the_same_decoder() {
    let dir = tempfile::tempdir().unwrap();
    for (path, ext, secs, onset) in all(dir.path()) {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let peaks = waveform::peaks(&path, ext, 100).unwrap_or_else(|| panic!("{name}: no waveform"));
        assert_eq!(peaks.len(), 100);
        let amp = |r: std::ops::Range<usize>| peaks[r].iter().map(|&(l, h)| h - l).fold(0.0f32, f32::max);
        let at = |t: f64| (t / secs * 100.0) as usize;
        assert!(amp(0..at(onset - 0.05)) < 0.1, "{name}: quiet start");
        assert!(amp(at(onset + 0.05)..at(secs - 0.05)) > 0.5, "{name}: loud end");
        assert!(waveform::thumbnail(&path, ext).is_some(), "{name}: thumbnail");
    }
}

#[test]
fn unsupported_files_fall_back_without_panicking() {
    let dir = tempfile::tempdir().unwrap();
    // Noise has no decoder.
    let cases: [(&str, &[u8]); 2] = [("x.ogg", b"not audio at all"), ("x.mp3", &[0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0, 0, 0, 0])];
    for (name, bytes) in cases {
        let p = dir.path().join(name);
        std::fs::write(&p, bytes).unwrap();
        let ext = name.rsplit('.').next().unwrap();
        assert_eq!(open(&p, ext).err(), Some(AudioError::UnknownFormat), "{name}");
        assert!(waveform::peaks(&p, ext, 10).is_none());
        assert!(media::audio_duration(&p, ext).is_none());
    }
    let missing = open(&dir.path().join("gone.mp3"), "mp3").err().unwrap();
    assert!(matches!(missing, AudioError::Io(_)), "{missing:?}");
    for (ext, _) in DECODED {
        assert!(media::is_audio(ext), "{ext}");
    }
    for ext in ["opus", "aiff", "aif", "aifc", "caf", "wma", "ape", "amr"] {
        assert!(!media::is_audio(ext), "{ext} is a plain file, as in Eagle");
    }
}


#[test]
fn video_soundtracks_decode_through_ffmpeg_when_installed() {
    if !falcon_ffmpeg::available() {
        eprintln!("skipped: FFmpeg is not installed (scripts/build-ffmpeg.sh)");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("clip.avi");
    let clip = falcon_ffmpeg::testing::Clip { width: 16, height: 16, frames: 20, fps: 20, audio_rate: Some(8000) };
    std::fs::write(&p, clip.avi(|_, _| [0, 0, 0])).unwrap();
    let mut s = open(&p, "avi").expect("FFmpeg decodes the soundtrack");
    assert_eq!((s.backend(), s.channels(), s.sample_rate()), ("ffmpeg", 2, 48_000));
    assert!((s.duration().unwrap() - 1.0).abs() < 0.05);
    assert_eq!(s.seek(0.5).unwrap(), 0.5);
    let mut frames = 0;
    while let Some(b) = s.next_block() {
        frames += b.frames().count();
    }
    assert!((frames as i64 - 24_000).abs() < 500, "half a second at 48 kHz: {frames}");
    assert!((s.position() - 1.0).abs() < 0.02);
}
