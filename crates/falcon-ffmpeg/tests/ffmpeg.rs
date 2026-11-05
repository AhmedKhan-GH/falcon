//! Decoding through a real FFmpeg. These run only when
//! `scripts/build-ffmpeg.sh` has installed it into `target/ffmpeg`
//! (`build.rs` sets `ffmpeg_built`); otherwise they are ignored.
//!
//! The clips are AVI files written by `falcon_ffmpeg::testing`, byte by
//! byte: uncompressed BGR pictures and 16-bit PCM sound, so no encoder is
//! needed (the build has none) and every pixel's value is known. Frame `i` is coloured
//! [`color`]`(i)` over its top three quarters and white below.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use falcon_ffmpeg::testing::Clip;
use falcon_ffmpeg::{AudioOut, Frame, Player, Tempo, ffi, frame_at, probe, thumbnail};

const W: u32 = 64;
const H: u32 = 48;
const FPS: u32 = 25;
const FRAMES: u32 = 25;
const RATE: u32 = 16_000;

/// Frame `i`'s colour (RGB): red counts the frame.
fn color(i: u32) -> [u8; 3] {
    [(10 + i * 7) as u8, 100, 200]
}

/// The frame a picture shows, from its top left pixel's red.
fn index_of(f: &Frame) -> u32 {
    (u32::from(f.pixel(0, 0)[0]).saturating_sub(10) + 3) / 7
}

fn write_avi(path: &Path, audio: bool, pixel: impl Fn(u32, u32) -> [u8; 3]) {
    let clip = Clip { width: W, height: H, frames: FRAMES, fps: FPS, audio_rate: audio.then_some(RATE) };
    std::fs::write(path, clip.avi(pixel)).unwrap();
}

/// The standard clip: coloured frames with a white bottom quarter.
fn clip(dir: &Path, audio: bool) -> PathBuf {
    let path = dir.join(if audio { "tone.avi" } else { "silent.avi" });
    write_avi(&path, audio, |i, y| if y >= H * 3 / 4 { [255, 255, 255] } else { color(i) });
    path
}

/// A preview opened on one thread and played on another: pictures fit the
/// side asked for, the time follows a seek, and dropping it doesn't wait.
#[test]
#[cfg_attr(not(ffmpeg_built), ignore = "needs scripts/build-ffmpeg.sh")]
fn a_preview_plays_small_pictures() {
    let dir = tempfile::tempdir().unwrap();
    let path = clip(dir.path(), false);
    let (mut p, out) = std::thread::spawn(move || Player::open_preview(&path, false, 32)).join().unwrap().unwrap();
    assert!(out.is_none());
    let deadline = Instant::now() + Duration::from_secs(5);
    let wait = |p: &mut Player| {
        while Instant::now() < deadline {
            if let Some(f) = p.frame() {
                return f;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("no picture");
    };
    let f = wait(&mut p);
    assert_eq!((f.width, f.height), (32, 24));
    p.seek(0.6);
    let f = wait(&mut p);
    assert_eq!((f.width, f.height), (32, 24));
    assert_eq!(index_of(&f), 15, "a paused seek shows that time");
    let began = Instant::now();
    drop(p);
    assert!(began.elapsed() < Duration::from_millis(50));
}

#[test]
#[cfg_attr(not(ffmpeg_built), ignore = "needs scripts/build-ffmpeg.sh")]
fn loads_an_lgpl_build() {
    let (version, dir) = falcon_ffmpeg::status().expect("FFmpeg loads");
    assert!(version.starts_with("7.1"), "{version}");
    assert!(dir.exists());
}

#[test]
#[cfg_attr(not(ffmpeg_built), ignore = "needs scripts/build-ffmpeg.sh")]
fn probes_and_grabs_frames() {
    let dir = tempfile::tempdir().unwrap();
    let path = clip(dir.path(), true);
    let info = probe(&path).unwrap();
    assert_eq!((info.width, info.height, info.turns), (W, H, 0));
    assert!((info.duration - 1.0).abs() < 0.05, "{}", info.duration);
    assert_eq!(info.fps, 25.0);
    assert!(info.has_audio);

    let f = frame_at(&path, 0.5).unwrap();
    assert_eq!((f.width, f.height), (W as usize, H as usize));
    assert_eq!(index_of(&f), 12);
    assert_eq!(f.pixel(5, 0), [color(12)[0], 100, 200, 255], "rows are upright");
    assert_eq!(f.pixel(5, H as usize - 1), [255, 255, 255, 255]);

    let mut g = falcon_ffmpeg::Grabber::open(&path, 32).unwrap();
    let small = g.grab(0.2).unwrap();
    assert_eq!((small.width, small.height), (32, 24), "scaled to fit");
    assert_eq!(index_of(&g.grab(0.8).unwrap()), 20, "seeks forwards");
    assert_eq!(index_of(&g.grab(0.1).unwrap()), 2, "and backwards");
    assert_eq!(index_of(&g.grab(5.0).unwrap()), FRAMES - 1, "past the end is the last frame");
}

#[test]
#[cfg_attr(not(ffmpeg_built), ignore = "needs scripts/build-ffmpeg.sh")]
fn thumbnails_skip_black_frames() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fade.avi");
    write_avi(&path, false, |i, _| if i < 16 { [0, 0, 0] } else { color(i) });
    let (info, f, at) = thumbnail(&path).unwrap();
    assert_eq!(info.width, W);
    assert!(!f.is_black());
    assert!(index_of(&f) >= 16 && at > 0.6, "frame {} at {at}", index_of(&f));
}

#[test]
#[cfg_attr(not(ffmpeg_built), ignore = "needs scripts/build-ffmpeg.sh")]
fn rejects_files_that_are_not_video() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("not.mkv");
    std::fs::write(&path, b"plain text, not a video").unwrap();
    assert!(probe(&path).is_err());
    assert!(Player::open(&path, false).is_err());
}

/// Calls `frame` until it hands out a picture or `secs` pass.
fn next_frame(p: &mut Player, secs: f64) -> Option<Frame> {
    let end = Instant::now() + Duration::from_secs_f64(secs);
    while Instant::now() < end {
        p.tick();
        if let Some(f) = p.frame() {
            return Some(f);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    None
}

#[test]
#[cfg_attr(not(ffmpeg_built), ignore = "needs scripts/build-ffmpeg.sh")]
fn plays_seeks_steps_and_stops_on_the_wall_clock() {
    let dir = tempfile::tempdir().unwrap();
    let path = clip(dir.path(), true);
    let (mut p, out) = Player::open(&path, false).unwrap();
    assert!(out.is_none(), "no sound asked for");

    // Paused at the start, the first frame still shows.
    assert_eq!(index_of(&next_frame(&mut p, 2.0).unwrap()), 0);
    assert!(p.is_ready());
    assert_eq!(p.current_time(), 0.0);

    p.play();
    let started = Instant::now();
    let mut seen = Vec::new();
    while started.elapsed() < Duration::from_millis(400) {
        if let Some(f) = next_frame(&mut p, 0.5) {
            seen.push(index_of(&f));
        }
    }
    assert!(seen.windows(2).all(|w| w[0] < w[1]), "frames in order: {seen:?}");
    let last = *seen.last().unwrap();
    assert!((5..=13).contains(&last), "about 10 frames in 0.4 s at 25 fps: {seen:?}");

    p.pause();
    p.seek(0.6);
    assert_eq!(p.current_time(), 0.6);
    assert_eq!(index_of(&next_frame(&mut p, 2.0).unwrap()), 15);
    p.step(1);
    assert_eq!(index_of(&next_frame(&mut p, 2.0).unwrap()), 16);
    p.step(-2);
    assert_eq!(index_of(&next_frame(&mut p, 2.0).unwrap()), 14);
    assert!(!p.is_playing());

    // Played to the end it stops there, and play starts it again.
    p.seek(0.9);
    p.play();
    let end = Instant::now() + Duration::from_secs(3);
    while p.is_playing() && Instant::now() < end {
        let _ = next_frame(&mut p, 0.05);
    }
    assert!(!p.is_playing(), "stopped at the end");
    assert!(p.current_time() > 0.95);
    p.play();
    assert_eq!(index_of(&next_frame(&mut p, 2.0).unwrap()), 0, "played again from the top");

    // Looping starts again by itself, at double speed.
    p.set_looping(true);
    p.set_rate(2.0);
    p.seek(0.8);
    let mut wrapped = false;
    let end = Instant::now() + Duration::from_secs(3);
    while !wrapped && Instant::now() < end {
        if let Some(f) = next_frame(&mut p, 0.1) {
            wrapped = index_of(&f) < 5;
        }
    }
    assert!(wrapped && p.is_playing());
    assert_eq!(index_of(&p.current_frame().unwrap()), index_of(&frame_at(&path, p.current_time()).unwrap()));
}

#[test]
#[cfg_attr(not(ffmpeg_built), ignore = "needs scripts/build-ffmpeg.sh")]
fn sound_drives_the_clock() {
    let dir = tempfile::tempdir().unwrap();
    let path = clip(dir.path(), true);
    let (mut p, out) = Player::open(&path, true).unwrap();
    let mut out = out.expect("a sound track");
    assert_eq!((falcon_ffmpeg::AudioOut::CHANNELS, falcon_ffmpeg::AudioOut::RATE), (2, 48_000));
    p.set_volume(0.5);
    p.play();
    // Pull 0.3 s of sound in real time, as an audio device would.
    let pull = std::thread::spawn(move || {
        let mut peak = 0f32;
        let start = Instant::now();
        let mut n = 0usize;
        while n < 48_000 * 2 * 3 / 10 {
            let due = start.elapsed().as_secs_f64() * 96_000.0;
            while (n as f64) < due {
                peak = peak.max(out.next_sample().unwrap().abs());
                n += 1;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        (peak, out)
    });
    let mut frames = 0;
    let end = Instant::now() + Duration::from_millis(350);
    while Instant::now() < end {
        if next_frame(&mut p, 0.05).is_some() {
            frames += 1;
        }
    }
    let (peak, mut out) = pull.join().unwrap();
    // The tone peaks at 8000 / 32768 ≈ 0.24, halved by the volume.
    assert!(peak > 0.08 && peak < 0.15, "peak {peak}");
    let t = p.current_time();
    assert!((0.2..0.45).contains(&t), "the clock follows the sound taken: {t}");
    assert!(frames >= 5, "{frames} frames");

    // Muted, the picture runs on the wall clock with no sound pulled.
    p.set_muted(true);
    let before = p.current_time();
    std::thread::sleep(Duration::from_millis(150));
    assert!(p.current_time() > before + 0.1);
    // What the output took before muting plays out, then silence.
    let after: Vec<f32> = (0..4096).map(|_| out.next_sample().unwrap()).collect();
    assert!(after[2048..].iter().all(|&s| s == 0.0), "silence while muted");

    drop(p);
    let mut ended = false;
    for _ in 0..4096 {
        if out.next_sample().is_none() {
            ended = true;
            break;
        }
    }
    assert!(ended, "the sound ends with the player");
}

/// `secs` of a stereo tone at `hz` as the player's samples.
fn tone(hz: f64, secs: f64) -> Vec<f32> {
    let frames = (secs * f64::from(AudioOut::RATE)) as usize;
    (0..frames).flat_map(|n| [((n as f64 / f64::from(AudioOut::RATE) * hz * std::f64::consts::TAU).sin() * 0.25) as f32; 2]).collect()
}

/// The pitch of stereo samples in Hz, from one channel's upward zero crossings.
fn pitch(samples: &[f32]) -> f64 {
    let left: Vec<f32> = samples.iter().step_by(2).copied().collect();
    let ups = left.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
    ups as f64 / (left.len() as f64 / f64::from(AudioOut::RATE))
}

/// A real clip's sound played through at double speed, as fast as it
/// comes: sample frames heard against what its length predicts, and how
/// often the sound crosses zero against the same part at normal speed
/// (both ratios about 1 when the pitch holds; resampling would double the
/// second).
fn sound_at_double_speed(path: &Path, duration: f64) -> (f64, f64) {
    let secs = duration.min(20.0);
    let take = |rate: f32| {
        let (mut p, out) = Player::open(path, true).unwrap();
        let mut out = out.unwrap();
        p.set_rate(rate);
        p.play();
        let wanted = (secs / f64::from(rate) * 96_000.0) as usize;
        let (mut heard, mut quiet) = (Vec::with_capacity(wanted), 0);
        while heard.len() < wanted && quiet < 400 {
            let block: Vec<f32> = (0..2048).map(|_| out.next_sample().unwrap()).collect();
            if block.iter().all(|&s| s == 0.0) {
                quiet += 1;
                std::thread::sleep(Duration::from_millis(1));
            } else {
                quiet = 0;
                heard.extend(block);
            }
            let _ = p.frame();
        }
        heard
    };
    let (normal, fast) = (take(1.0), take(2.0));
    let frames = fast.len() as f64 / (secs * 48_000.0);
    (frames, pitch(&fast) / pitch(&normal).max(1.0))
}

/// What a [`Tempo`] at `rate` makes of `input`, all of it.
fn stretched(rate: f64, input: &[f32]) -> Vec<f32> {
    let mut t = Tempo::new(rate).unwrap();
    let mut out = Vec::new();
    for block in input.chunks(2048) {
        t.push(block, Some(0.0)).unwrap();
        t.pull(&mut out);
    }
    t.finish().unwrap();
    t.pull(&mut out);
    out
}

#[test]
#[cfg_attr(not(ffmpeg_built), ignore = "needs scripts/build-ffmpeg.sh")]
fn tempo_changes_the_length_and_keeps_the_pitch() {
    let input = tone(440.0, 2.0);
    for rate in [0.1, 0.25, 0.5, 0.75, 1.25, 2.0, 4.0, 8.0, 16.0] {
        let out = stretched(rate, &input);
        // The filter's last window, about 1024 sample frames out, may be cut short.
        let covered = out.len() as f64 * rate;
        let slack = 2.0 * 1024.0 * (rate + 1.0);
        assert!((covered - input.len() as f64).abs() <= slack, "{rate}x: {} samples from {}, covering {covered}", out.len(), input.len());
        assert_eq!(out.len() % 2, 0, "whole stereo frames");
        let heard = pitch(&out);
        assert!((400.0..480.0).contains(&heard), "{rate}x sounds at {heard} Hz");
    }
}

#[test]
#[cfg_attr(not(ffmpeg_built), ignore = "needs scripts/build-ffmpeg.sh")]
fn tempo_output_lines_up_with_the_clip() {
    // Half a second of silence, then the tone: at 2x it starts a quarter
    // second in, and its media time, by the timeline, is the half second.
    let mut input = vec![0f32; 48_000];
    input.extend(tone(440.0, 1.0));
    let mut t = Tempo::new(2.0).unwrap();
    let mut out = Vec::new();
    let mut first = None;
    for block in input.chunks(2048) {
        t.push(block, Some(3.0)).unwrap();
        first = first.or(t.pull(&mut out));
    }
    t.finish().unwrap();
    first = first.or(t.pull(&mut out));
    assert_eq!(first, Some(3.0), "the first sample out is the first sample in");
    let onset = out.iter().position(|s| s.abs() > 0.1).unwrap() / 2;
    let at = onset as f64 / f64::from(AudioOut::RATE);
    assert!((at - 0.25).abs() < 0.03, "the tone starts at {at} s");
}

#[test]
#[cfg_attr(not(ffmpeg_built), ignore = "needs scripts/build-ffmpeg.sh")]
fn double_speed_halves_the_sound_and_keeps_clock_and_pitch() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("long.avi");
    // Four seconds of 48 kHz sound, so the player's samples are the clip's.
    let clip = Clip { width: W, height: H, frames: 100, fps: FPS, audio_rate: Some(48_000) };
    std::fs::write(&path, clip.avi(|i, _| color(i % 25))).unwrap();
    let (mut p, out) = Player::open(&path, true).unwrap();
    let mut out = out.expect("a sound track");
    p.set_rate(2.0);
    assert_eq!(p.rate(), 2.0);
    p.play();
    // Half a second of output in real time, as an audio device takes it.
    let start = Instant::now();
    let mut heard = Vec::new();
    let mut taken = 0usize;
    while start.elapsed() < Duration::from_millis(500) {
        while (taken as f64) < start.elapsed().as_secs_f64() * 96_000.0 {
            heard.push(out.next_sample().unwrap());
            taken += 1;
        }
        let _ = p.frame();
        std::thread::sleep(Duration::from_millis(5));
    }
    let t = p.current_time();
    assert!((0.7..1.15).contains(&t), "half a second at 2x moves the clock a second: {t}");
    // The rest as fast as it comes, leaving out the silence of a starved output.
    let mut quiet = 0;
    while quiet < 400 {
        let block: Vec<f32> = (0..2048).map(|_| out.next_sample().unwrap()).collect();
        if block.iter().all(|&s| s == 0.0) {
            quiet += 1;
            std::thread::sleep(Duration::from_millis(1));
        } else {
            quiet = 0;
        }
        heard.extend(block);
        let _ = p.frame();
    }
    heard.retain(|&s| s != 0.0);
    let frames = heard.len() / 2;
    assert!((91_000..101_000).contains(&frames), "4 s at 2x is about 96000 sample frames at 48 kHz: {frames}");
    let tone = pitch(&heard);
    assert!((400.0..480.0).contains(&tone), "the 440 Hz tone sounds at {tone} Hz");
    let clock = p.current_time();
    assert!((3.7..=4.0).contains(&clock), "the clock reaches the end with the sound: {clock}");

    // After a seek the sound starts at the target, still on the clip's time.
    p.seek(1.0);
    let mut waited = 0;
    while waited < 2000 && out.next_sample().unwrap() == 0.0 {
        waited += 1;
        if waited % 1024 == 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    for _ in 0..12_000 {
        out.next_sample().unwrap();
    }
    let _ = next_frame(&mut p, 1.0);
    let t = p.current_time();
    assert!((1.0..1.6).contains(&t), "6000 sample frames past 1 s at 2x is about 1.25 s: {t}");
}

#[test]
#[cfg_attr(not(ffmpeg_built), ignore = "needs scripts/build-ffmpeg.sh")]
fn sound_alone_decodes_and_seeks() {
    let dir = tempfile::tempdir().unwrap();
    let path = clip(dir.path(), true);
    let mut a = falcon_ffmpeg::AudioFile::open(&path).unwrap();
    assert!((a.duration() - 1.0).abs() < 0.05);
    let all: Vec<f32> = std::iter::from_fn(|| a.next_sample()).collect();
    // One second of stereo at 48 kHz, within a resampler's few samples.
    assert!((all.len() as i64 - 96_000).abs() < 400, "{}", all.len());
    let peak = all.iter().fold(0f32, |m, s| m.max(s.abs()));
    assert!((0.2..0.3).contains(&peak), "the tone peaks at 8000 / 32768: {peak}");
    a.seek(0.75).unwrap();
    assert_eq!(a.position(), 0.75);
    let mut rest = 0;
    while let Some(b) = a.next_block() {
        rest += b.len();
    }
    assert!((rest as i64 - 24_000).abs() < 1000, "a quarter second is left: {rest}");
    assert!((a.position() - 1.0).abs() < 0.02, "{}", a.position());
    let silent = dir.path().join("silent.avi");
    write_avi(&silent, false, |i, _| color(i));
    assert!(falcon_ffmpeg::AudioFile::open(&silent).is_err());
}

#[test]
#[cfg_attr(not(ffmpeg_built), ignore = "needs scripts/build-ffmpeg.sh")]
fn struct_layouts_match_the_installed_headers() {
    use std::mem::offset_of;
    use ffi::*;
    let include = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ffmpeg/include");
    let ours: Vec<(&str, i64)> = vec![
        ("AVFormatContext.nb_streams", offset_of!(AVFormatContext, nb_streams) as i64),
        ("AVFormatContext.streams", offset_of!(AVFormatContext, streams) as i64),
        ("AVFormatContext.start_time", offset_of!(AVFormatContext, start_time) as i64),
        ("AVFormatContext.duration", offset_of!(AVFormatContext, duration) as i64),
        ("AVStream.index", offset_of!(AVStream, index) as i64),
        ("AVStream.codecpar", offset_of!(AVStream, codecpar) as i64),
        ("AVStream.time_base", offset_of!(AVStream, time_base) as i64),
        ("AVStream.start_time", offset_of!(AVStream, start_time) as i64),
        ("AVStream.duration", offset_of!(AVStream, duration) as i64),
        ("AVStream.sample_aspect_ratio", offset_of!(AVStream, sample_aspect_ratio) as i64),
        ("AVStream.avg_frame_rate", offset_of!(AVStream, avg_frame_rate) as i64),
        ("AVCodecParameters.coded_side_data", offset_of!(AVCodecParameters, coded_side_data) as i64),
        ("AVCodecParameters.nb_coded_side_data", offset_of!(AVCodecParameters, nb_coded_side_data) as i64),
        ("AVCodecParameters.width", offset_of!(AVCodecParameters, width) as i64),
        ("AVCodecParameters.height", offset_of!(AVCodecParameters, height) as i64),
        ("AVCodecParameters.sample_aspect_ratio", offset_of!(AVCodecParameters, sample_aspect_ratio) as i64),
        ("AVCodecParameters.framerate", offset_of!(AVCodecParameters, framerate) as i64),
        ("AVPacketSideData.data", offset_of!(AVPacketSideData, data) as i64),
        ("AVPacketSideData.size", offset_of!(AVPacketSideData, size) as i64),
        ("AVPacketSideData.type", offset_of!(AVPacketSideData, kind) as i64),
        ("AVPacket.size", offset_of!(AVPacket, size) as i64),
        ("AVPacket.stream_index", offset_of!(AVPacket, stream_index) as i64),
        ("AVFrame.data", offset_of!(AVFrame, data) as i64),
        ("AVFrame.linesize", offset_of!(AVFrame, linesize) as i64),
        ("AVFrame.extended_data", offset_of!(AVFrame, extended_data) as i64),
        ("AVFrame.width", offset_of!(AVFrame, width) as i64),
        ("AVFrame.height", offset_of!(AVFrame, height) as i64),
        ("AVFrame.nb_samples", offset_of!(AVFrame, nb_samples) as i64),
        ("AVFrame.format", offset_of!(AVFrame, format) as i64),
        ("AVFrame.sample_aspect_ratio", offset_of!(AVFrame, sample_aspect_ratio) as i64),
        ("AVFrame.pts", offset_of!(AVFrame, pts) as i64),
        ("AVFrame.sample_rate", offset_of!(AVFrame, sample_rate) as i64),
        ("AVFrame.best_effort_timestamp", offset_of!(AVFrame, best_effort_timestamp) as i64),
        ("AVFrame.ch_layout", offset_of!(AVFrame, ch_layout) as i64),
        ("AVChannelLayout.nb_channels", offset_of!(AVChannelLayout, nb_channels) as i64),
        ("AVChannelLayout.u", offset_of!(AVChannelLayout, u) as i64),
        ("sizeof(AVChannelLayout)", std::mem::size_of::<AVChannelLayout>() as i64),
        ("AVMEDIA_TYPE_VIDEO", i64::from(AVMEDIA_TYPE_VIDEO)),
        ("AVMEDIA_TYPE_AUDIO", i64::from(AVMEDIA_TYPE_AUDIO)),
        ("AV_PIX_FMT_RGBA", i64::from(AV_PIX_FMT_RGBA)),
        ("AV_SAMPLE_FMT_FLT", i64::from(AV_SAMPLE_FMT_FLT)),
        ("AV_PKT_DATA_DISPLAYMATRIX", i64::from(AV_PKT_DATA_DISPLAYMATRIX)),
        ("AVSEEK_FLAG_BACKWARD", i64::from(AVSEEK_FLAG_BACKWARD)),
        ("SWS_BILINEAR", i64::from(SWS_BILINEAR)),
        ("SWS_AREA", i64::from(SWS_AREA)),
        ("AV_LOG_QUIET", i64::from(AV_LOG_QUIET)),
        ("AVERROR(EAGAIN)", i64::from(AVERROR_EAGAIN)),
        ("AVERROR_EOF", i64::from(AVERROR_EOF)),
        ("AVERROR_INVALIDDATA", i64::from(AVERROR_INVALIDDATA)),
        ("AV_TIME_BASE", AV_TIME_BASE),
        ("AV_NOPTS_VALUE", AV_NOPTS_VALUE),
        ("LIBAVFILTER_VERSION_MAJOR", i64::from(AVFILTER_MAJOR)),
    ];
    let mut c = String::from(
        "#include <stdio.h>\n#include <stddef.h>\n#include <errno.h>\n#include <libavformat/avformat.h>\n#include <libavcodec/avcodec.h>\n\
         #include <libavutil/frame.h>\n#include <libswscale/swscale.h>\n#include <libavutil/log.h>\n#include <libavfilter/avfilter.h>\nint main(void) {\n",
    );
    for (name, _) in &ours {
        let expr = if let Some(t) = name.strip_prefix("sizeof(").and_then(|n| n.strip_suffix(')')) {
            format!("sizeof({t})")
        } else if let Some((t, f)) = name.split_once('.') {
            format!("offsetof({t}, {f})")
        } else {
            name.to_string()
        };
        c.push_str(&format!("  printf(\"%s %lld\\n\", \"{name}\", (long long)({expr}));\n"));
    }
    c.push_str("  return 0;\n}\n");
    let dir = tempfile::tempdir().unwrap();
    let (src, bin) = (dir.path().join("layout.c"), dir.path().join("layout"));
    std::fs::write(&src, c).unwrap();
    let cc = std::process::Command::new("cc").arg("-I").arg(&include).arg(&src).arg("-o").arg(&bin).output().expect("a C compiler");
    assert!(cc.status.success(), "{}", String::from_utf8_lossy(&cc.stderr));
    let out = std::process::Command::new(&bin).output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    for (line, (name, value)) in text.lines().zip(&ours) {
        let (n, v) = line.rsplit_once(' ').unwrap();
        assert_eq!(n, *name);
        assert_eq!(v.parse::<i64>().unwrap(), *value, "{name}");
    }
    assert_eq!(text.lines().count(), ours.len());
}

/// A name FFmpeg must be handed intact: on Windows it reads the path as UTF-8
/// whatever the system's code page is.
#[test]
#[cfg_attr(not(ffmpeg_built), ignore = "needs scripts/build-ffmpeg.sh")]
fn decodes_a_file_with_a_non_ascii_path() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("Ünï çödé 映像");
    std::fs::create_dir(&folder).unwrap();
    let path = folder.join("クリップ – été.avi");
    write_avi(&path, false, |i, _| color(i));
    let info = probe(&path).unwrap();
    assert_eq!((info.width, info.height), (W, H));
    let frame = frame_at(&path, 0.0).unwrap();
    assert_eq!((frame.width as u32, frame.height as u32), (W, H));
}

/// Every FFmpeg DLL in the process, avcodec's dav1d too, came from the folder
/// the loader was pointed at, not from `PATH` or the current directory.
#[cfg(windows)]
#[test]
#[cfg_attr(not(ffmpeg_dav1d), ignore = "needs scripts/build-ffmpeg.sh with dav1d")]
fn dlls_all_come_from_the_ffmpeg_folder() {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut std::ffi::c_void;
        fn GetModuleFileNameW(module: *mut std::ffi::c_void, path: *mut u16, size: u32) -> u32;
    }
    let api = ffi::api().unwrap();
    let want = std::path::absolute(&api.dir).unwrap();
    for name in ["avutil-59.dll", "swresample-5.dll", "avfilter-10.dll", "avcodec-61.dll", "avformat-61.dll", "swscale-8.dll", "libdav1d.dll"] {
        let wide: Vec<u16> = std::ffi::OsStr::new(name).encode_wide().chain([0]).collect();
        // SAFETY: a NUL-terminated name, and a buffer as long as the size passed.
        let path = unsafe {
            let module = GetModuleHandleW(wide.as_ptr());
            assert!(!module.is_null(), "{name} isn't loaded");
            let mut buf = [0u16; 1024];
            let len = GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32);
            PathBuf::from(std::ffi::OsString::from_wide(&buf[..len as usize]))
        };
        assert_eq!(path.parent().map(Path::to_path_buf).as_ref(), Some(&want), "{name}");
    }
}

/// The AV1 clip, `testdata/av1.ivf`: falcon's own, encoded with rav1e 0.8.1
/// (`--speed 10 --quantizer 20 --keyint 13 --min-keyint 13 --low-latency
/// --no-scene-detection`) from 25 flat 64 × 48 pictures, picture `i`
/// coloured [`color`]`(i)`, keyframes at 0 and 13 (the frame count in its
/// header, which rav1e leaves at 0, was set to 25). The build has no AV1
/// encoder; `testing::av1` rewraps the stream as WebM, Matroska and MP4.
fn av1_clips(dir: &Path) -> Vec<PathBuf> {
    let ivf = include_bytes!("../testdata/av1.ivf");
    let clip = falcon_ffmpeg::testing::av1::Av1Clip::from_ivf(ivf).unwrap();
    let files: [(&str, Vec<u8>); 3] = [("av1.webm", clip.matroska(true)), ("av1.mkv", clip.matroska(false)), ("av1.mp4", clip.mp4())];
    files
        .into_iter()
        .map(|(name, bytes)| {
            let path = dir.join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        })
        .collect()
}

/// Whether a decoded AV1 picture shows frame `i`'s colour everywhere, to
/// within the encoder's loss and the YUV round trip.
fn shows(f: &Frame, i: u32) -> bool {
    let want = color(i);
    [(0, 0), (f.width / 2, f.height / 2), (f.width - 1, f.height - 1)]
        .iter()
        .all(|&(x, y)| f.pixel(x, y)[..3].iter().zip(want).all(|(&got, want)| got.abs_diff(want) <= 3))
}

#[test]
#[cfg_attr(not(ffmpeg_dav1d), ignore = "needs scripts/build-ffmpeg.sh with dav1d")]
fn av1_decodes_through_dav1d() {
    let api = ffi::api().unwrap();
    assert!(ffi::has_decoder(api, "libdav1d"), "libavcodec was built with --enable-libdav1d");
    assert!(!ffi::has_decoder(api, "no-such-decoder"));
    // libavcodec found dav1d next to itself.
    assert!(api.dir.join("libdav1d.7.dylib").is_file() || api.dir.join("libdav1d.so.7").is_file() || api.dir.join("libdav1d.dll").is_file());

    // The raw stream as encoded (IVF states no frame rate, so only the
    // size, length and first picture are checked here).
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("av1.ivf");
    std::fs::write(&path, include_bytes!("../testdata/av1.ivf")).unwrap();
    let info = probe(&path).unwrap();
    assert_eq!((info.width, info.height), (W, H));
    assert!((info.duration - 1.0).abs() < 0.05, "{}", info.duration);
    assert!(shows(&frame_at(&path, 0.0).unwrap(), 0));
}

#[test]
#[cfg_attr(not(ffmpeg_dav1d), ignore = "needs scripts/build-ffmpeg.sh with dav1d")]
fn av1_probes_grabs_thumbnails_and_seeks_in_every_container() {
    let dir = tempfile::tempdir().unwrap();
    for path in av1_clips(dir.path()) {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let info = probe(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!((info.width, info.height, info.turns, info.has_audio), (W, H, 0, false), "{name}");
        assert!((info.duration - 1.0).abs() < 0.05, "{name}: {}", info.duration);
        assert_eq!(info.fps, 25.0, "{name}");

        let first = frame_at(&path, 0.0).unwrap();
        assert_eq!((first.width, first.height), (W as usize, H as usize), "{name}");
        assert!(shows(&first, 0), "{name}: first frame {:?}", first.pixel(0, 0));
        // Past the second keyframe, then back into the first group.
        assert!(shows(&frame_at(&path, 0.6).unwrap(), 15), "{name}");
        assert!(shows(&frame_at(&path, 0.2).unwrap(), 5), "{name}");

        let (_, thumb, at) = thumbnail(&path).unwrap();
        assert!((at - 1.0 / 3.0).abs() < 0.05, "{name}: Eagle's duration / 3, at {at}");
        assert_eq!(index_of(&thumb), 8, "{name}");

        let mut g = falcon_ffmpeg::Grabber::open(&path, 32).unwrap();
        assert_eq!(index_of(&g.grab(0.8).unwrap()), 20, "{name}");
        assert_eq!(index_of(&g.grab(0.1).unwrap()), 2, "{name}");

        let (mut p, _) = Player::open(&path, false).unwrap();
        assert!(shows(&next_frame(&mut p, 2.0).unwrap(), 0), "{name}");
        p.seek(0.6);
        assert_eq!(index_of(&next_frame(&mut p, 2.0).unwrap()), 15, "{name}");
        p.step(1);
        assert_eq!(index_of(&next_frame(&mut p, 2.0).unwrap()), 16, "{name}");
        p.step(-4);
        assert_eq!(index_of(&next_frame(&mut p, 2.0).unwrap()), 12, "{name}: back over the keyframe");
    }
}


/// A manual check on real clips: `FALCON_VIDEO_SAMPLES=<folder> cargo test
/// -p falcon-ffmpeg --test ffmpeg samples -- --ignored --nocapture`
/// probes, thumbnails, seeks and plays a second of every file in the folder.
#[test]
#[ignore = "set FALCON_VIDEO_SAMPLES to a folder of clips"]
fn samples() {
    let dir = std::env::var_os("FALCON_VIDEO_SAMPLES").expect("FALCON_VIDEO_SAMPLES");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    paths.sort();
    for path in paths {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let t0 = Instant::now();
        let info = match probe(&path) {
            Ok(i) => i,
            Err(e) => {
                println!("{name}: {e}");
                continue;
            }
        };
        let (_, thumb, at) = thumbnail(&path).unwrap();
        let t_thumb = t0.elapsed();
        let (mut p, out) = Player::open(&path, true).unwrap();
        let puller = out.map(|mut out| {
            std::thread::spawn(move || {
                let start = Instant::now();
                let (mut n, mut peak) = (0usize, 0f32);
                while start.elapsed() < Duration::from_millis(1200) {
                    while (n as f64) < start.elapsed().as_secs_f64() * 96_000.0 {
                        match out.next_sample() {
                            Some(s) => peak = peak.max(s.abs()),
                            None => return peak,
                        }
                        n += 1;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                peak
            })
        });
        p.play();
        let start = Instant::now();
        let mut frames = 0;
        while start.elapsed() < Duration::from_secs(1) {
            if next_frame(&mut p, 0.05).is_some() {
                frames += 1;
            }
        }
        let t = p.current_time();
        let tempo = info.has_audio.then(|| sound_at_double_speed(&path, info.duration));
        p.seek(info.duration / 2.0);
        let s0 = Instant::now();
        let mid = next_frame(&mut p, 5.0).map(|f| (f.width, f.height));
        let seek_ms = s0.elapsed().as_millis();
        drop(p);
        let peak = puller.map(|h| h.join().unwrap());
        println!(
            "{name}: {}x{} {:.2}s {:.2}fps audio={} turns={} | thumb {}x{} at {at:.2}s in {t_thumb:?} | played {frames} frames, clock {t:.2}s | seek {seek_ms} ms -> {mid:?} | peak {peak:?} | 2x sound {tempo:?}",
            info.width, info.height, info.duration, info.fps, info.has_audio, info.turns, thumb.width, thumb.height
        );
    }
}
