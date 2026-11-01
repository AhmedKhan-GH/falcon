//! Decoding through a real FFmpeg. These run only when
//! `scripts/build-ffmpeg.sh` has installed it into `target/ffmpeg`
//! (`build.rs` sets `ffmpeg_built`); otherwise they are ignored.
//!
//! The clips are AVI files written here, byte by byte: uncompressed BGR
//! pictures and 16-bit PCM sound, so no encoder is needed (the build has
//! none) and every pixel's value is known. Frame `i` is coloured
//! [`color`]`(i)` over its top three quarters and white below.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use falcon_ffmpeg::{Frame, Player, ffi, frame_at, probe, thumbnail};

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

/// Writes an AVI of [`FRAMES`] frames at [`FPS`], `W`×`H`, with a 440 Hz
/// stereo tone when `audio`. `pixel(i, y)` colours row `y` of frame `i`.
fn write_avi(path: &Path, audio: bool, pixel: impl Fn(u32, u32) -> [u8; 3]) {
    let frame_bytes = W * H * 3;
    let per_frame = RATE / FPS;
    let streams = if audio { 2 } else { 1 };

    let mut avih = Vec::new();
    for x in [1_000_000 / FPS, 0, 0, 0x10, FRAMES, 0, streams, frame_bytes, W, H, 0, 0, 0, 0] {
        le32(&mut avih, x);
    }
    let mut strh = b"vids".to_vec();
    strh.extend_from_slice(&[0; 4]);
    for x in [0u32, 0, 0, 1, FPS, 0, FRAMES, frame_bytes, u32::MAX, 0] {
        le32(&mut strh, x);
    }
    for x in [0u16, 0, W as u16, H as u16] {
        le16(&mut strh, x);
    }
    let mut strf = Vec::new();
    for x in [40, W, H] {
        le32(&mut strf, x);
    }
    le16(&mut strf, 1);
    le16(&mut strf, 24);
    for x in [0, frame_bytes, 0, 0, 0, 0] {
        le32(&mut strf, x);
    }
    let mut hdrl = chunk(b"avih", &avih);
    hdrl.extend(list(b"strl", &[chunk(b"strh", &strh), chunk(b"strf", &strf)].concat()));
    if audio {
        let mut ah = b"auds".to_vec();
        ah.extend_from_slice(&[0; 4]);
        for x in [0u32, 0, 0, 4, RATE * 4, 0, per_frame * FRAMES, per_frame * 4, u32::MAX, 4] {
            le32(&mut ah, x);
        }
        ah.extend_from_slice(&[0; 8]);
        let mut af = Vec::new();
        le16(&mut af, 1);
        le16(&mut af, 2);
        le32(&mut af, RATE);
        le32(&mut af, RATE * 4);
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
    for i in 0..FRAMES {
        // Bottom-up rows of BGR.
        let mut px = Vec::with_capacity(frame_bytes as usize);
        for row in (0..H).rev() {
            let [r, g, b] = pixel(i, row);
            for _ in 0..W {
                px.extend_from_slice(&[b, g, r]);
            }
        }
        add(&mut movi, b"00db", &px);
        if audio {
            let mut pcm = Vec::new();
            for n in 0..per_frame {
                let t = f64::from(i * per_frame + n) / f64::from(RATE);
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
    std::fs::write(path, chunk(b"RIFF", &body)).unwrap();
}

/// The standard clip: coloured frames with a white bottom quarter.
fn clip(dir: &Path, audio: bool) -> PathBuf {
    let path = dir.join(if audio { "tone.avi" } else { "silent.avi" });
    write_avi(&path, audio, |i, y| if y >= H * 3 / 4 { [255, 255, 255] } else { color(i) });
    path
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
    ];
    let mut c = String::from(
        "#include <stdio.h>\n#include <stddef.h>\n#include <errno.h>\n#include <libavformat/avformat.h>\n#include <libavcodec/avcodec.h>\n\
         #include <libavutil/frame.h>\n#include <libswscale/swscale.h>\n#include <libavutil/log.h>\nint main(void) {\n",
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
        p.seek(info.duration / 2.0);
        let s0 = Instant::now();
        let mid = next_frame(&mut p, 5.0).map(|f| (f.width, f.height));
        let seek_ms = s0.elapsed().as_millis();
        drop(p);
        let peak = puller.map(|h| h.join().unwrap());
        println!(
            "{name}: {}x{} {:.2}s {:.2}fps audio={} turns={} | thumb {}x{} at {at:.2}s in {t_thumb:?} | played {frames} frames, clock {t:.2}s | seek {seek_ms} ms -> {mid:?} | peak {peak:?}",
            info.width, info.height, info.duration, info.fps, info.has_audio, info.turns, thumb.width, thumb.height
        );
    }
}
