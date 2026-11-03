//! Checks falcon-video against real AVFoundation on clips written here by
//! AVFoundation itself (24 frames at 12 fps, red for the first second and
//! blue for the second; one with a silent audio track, one without).
//!
//! AVPlayer is main-thread-only, so this test runs without the libtest
//! harness: `main` is the process's main thread and pumps its run loop
//! while the player loads, the way the app's event loop does.

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("skipped: AVFoundation is macOS only");
}

#[cfg(target_os = "macos")]
fn main() {
    mac::run();
}

#[cfg(target_os = "macos")]
mod mac {
    use std::path::Path;
    use std::ptr::{self, NonNull};
    use std::time::{Duration, Instant};

    use falcon_video::{Backend, Frame, Grabber, Prepared, Video, frame_at, probe};
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_av_foundation::{
        AVAssetWriter, AVAssetWriterInput, AVAssetWriterInputPixelBufferAdaptor, AVAssetWriterStatus, AVFileTypeMPEG4, AVFileTypeQuickTimeMovie,
        AVMediaTypeAudio, AVMediaTypeVideo, AVVideoCodecKey, AVVideoCodecTypeH264, AVVideoHeightKey, AVVideoWidthKey,
    };
    use objc2_core_audio_types::{AudioStreamBasicDescription, kAudioFormatFlagIsPacked, kAudioFormatFlagIsSignedInteger, kAudioFormatLinearPCM, kAudioFormatMPEG4AAC};
    use objc2_core_foundation::{CFRetained, CGAffineTransform};
    use objc2_core_media::{
        CMAudioFormatDescriptionCreate, CMAudioSampleBufferCreateReadyWithPacketDescriptions, CMBlockBuffer, CMFormatDescription, CMSampleBuffer, CMTime,
        kCMBlockBufferAssureMemoryNowFlag,
    };
    use objc2_core_video::{
        CVPixelBuffer, CVPixelBufferCreate, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
        CVPixelBufferUnlockBaseAddress, kCVPixelFormatType_32BGRA,
    };
    use objc2_foundation::{NSCopying, NSDate, NSDictionary, NSNumber, NSRunLoop, NSString, NSURL, ns_string};

    const W: usize = 160;
    const H: usize = 120;
    const FPS: i32 = 12;
    const FRAMES: i64 = 24;
    const RATE: f64 = 44_100.0;

    fn dict(pairs: &[(&NSString, Retained<AnyObject>)]) -> Retained<NSDictionary<NSString, AnyObject>> {
        let keys: Vec<&NSString> = pairs.iter().map(|p| p.0).collect();
        let values: Vec<&AnyObject> = pairs.iter().map(|p| &*p.1).collect();
        NSDictionary::from_slices(&keys, &values)
    }

    fn num(n: f64) -> Retained<AnyObject> {
        NSNumber::new_f64(n).into()
    }

    /// A BGRA picture filled with `top` over its upper half and `bottom`
    /// over the rest.
    fn picture(top: [u8; 4], bottom: [u8; 4]) -> CFRetained<CVPixelBuffer> {
        let mut out: *mut CVPixelBuffer = ptr::null_mut();
        unsafe {
            assert_eq!(CVPixelBufferCreate(None, W, H, kCVPixelFormatType_32BGRA, None, NonNull::from(&mut out)), 0);
            let pb = CFRetained::from_raw(NonNull::new(out).unwrap());
            CVPixelBufferLockBaseAddress(&pb, CVPixelBufferLockFlags(0));
            let stride = CVPixelBufferGetBytesPerRow(&pb);
            let base = CVPixelBufferGetBaseAddress(&pb) as *mut u8;
            let bytes = std::slice::from_raw_parts_mut(base, stride * H);
            for (y, row) in bytes.chunks_exact_mut(stride).enumerate() {
                let bgra = if y < H / 2 { top } else { bottom };
                for px in row[..W * 4].as_chunks_mut::<4>().0 {
                    px.copy_from_slice(&bgra);
                }
            }
            CVPixelBufferUnlockBaseAddress(&pb, CVPixelBufferLockFlags(0));
            pb
        }
    }

    /// `frames` samples of mono 16-bit silence starting at `start` samples.
    fn silence(format: &CMFormatDescription, start: i64, frames: usize) -> CFRetained<CMSampleBuffer> {
        unsafe {
            let len = frames * 2;
            let mut block: *mut CMBlockBuffer = ptr::null_mut();
            let s = CMBlockBuffer::create_with_memory_block(None, ptr::null_mut(), len, None, ptr::null(), 0, len, kCMBlockBufferAssureMemoryNowFlag, NonNull::from(&mut block));
            assert_eq!(s, 0);
            let block = CFRetained::from_raw(NonNull::new(block).unwrap());
            assert_eq!(CMBlockBuffer::fill_data_bytes(0, &block, 0, len), 0);
            let mut sample: *mut CMSampleBuffer = ptr::null_mut();
            let s = CMAudioSampleBufferCreateReadyWithPacketDescriptions(None, &block, format, frames as isize, CMTime::new(start, RATE as i32), ptr::null(), NonNull::from(&mut sample));
            assert_eq!(s, 0);
            CFRetained::from_raw(NonNull::new(sample).unwrap())
        }
    }

    const RED: [u8; 4] = [0, 0, 255, 255];
    const BLUE: [u8; 4] = [255, 0, 0, 255];

    /// Writes the test clip; `audio` adds a silent AAC track. A `portrait`
    /// clip is stored red over blue and tagged to be shown turned a quarter
    /// turn clockwise, as phones tag upright recordings.
    fn write_clip(path: &Path, audio: bool, portrait: bool) {
        let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str().unwrap()));
        unsafe {
            let kind = if path.extension().is_some_and(|e| e == "mp4") { AVFileTypeMPEG4 } else { AVFileTypeQuickTimeMovie };
            let writer = AVAssetWriter::assetWriterWithURL_fileType_error(&url, kind.unwrap()).expect("writer");
            let settings = dict(&[
                (AVVideoCodecKey.unwrap(), AVVideoCodecTypeH264.unwrap().copy().into()),
                (AVVideoWidthKey.unwrap(), num(W as f64)),
                (AVVideoHeightKey.unwrap(), num(H as f64)),
            ]);
            let video = AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(AVMediaTypeVideo.unwrap(), Some(&settings));
            let adaptor = AVAssetWriterInputPixelBufferAdaptor::assetWriterInputPixelBufferAdaptorWithAssetWriterInput_sourcePixelBufferAttributes(&video, None);
            // Real-time mode keeps the encoder from holding a lookahead of
            // frames, which otherwise stalls a clip this short.
            video.setExpectsMediaDataInRealTime(true);
            if portrait {
                video.setTransform(CGAffineTransform { a: 0.0, b: 1.0, c: -1.0, d: 0.0, tx: H as f64, ty: 0.0 });
            }
            assert!(writer.canAddInput(&video));
            writer.addInput(&video);

            let pcm = AudioStreamBasicDescription {
                mSampleRate: RATE,
                mFormatID: kAudioFormatLinearPCM,
                mFormatFlags: kAudioFormatFlagIsSignedInteger | kAudioFormatFlagIsPacked,
                mBytesPerPacket: 2,
                mFramesPerPacket: 1,
                mBytesPerFrame: 2,
                mChannelsPerFrame: 1,
                mBitsPerChannel: 16,
                mReserved: 0,
            };
            let mut format: *const CMFormatDescription = ptr::null();
            assert_eq!(CMAudioFormatDescriptionCreate(None, NonNull::from(&pcm), 0, ptr::null(), 0, ptr::null(), None, NonNull::from(&mut format)), 0);
            let format = CFRetained::from_raw(NonNull::new(format as *mut CMFormatDescription).unwrap());
            let sound = audio.then(|| {
                let settings = dict(&[
                    (ns_string!("AVFormatIDKey"), num(kAudioFormatMPEG4AAC as f64)),
                    (ns_string!("AVSampleRateKey"), num(RATE)),
                    (ns_string!("AVNumberOfChannelsKey"), num(1.0)),
                ]);
                let input = AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(AVMediaTypeAudio.unwrap(), Some(&settings));
                assert!(writer.canAddInput(&input));
                writer.addInput(&input);
                input
            });

            assert!(writer.startWriting(), "{:?}", writer.error());
            writer.startSessionAtSourceTime(CMTime::new(0, FPS));
            let chunk = (RATE / 10.0) as usize;
            let total = (RATE * FRAMES as f64 / FPS as f64) as i64;
            let (mut frame, mut sample) = (0i64, if audio { 0 } else { total });
            let deadline = Instant::now() + Duration::from_secs(20);
            while frame < FRAMES || sample < total {
                assert!(Instant::now() < deadline, "writer stalled at frame {frame}, sample {sample}: {:?} {:?}", writer.status(), writer.error());
                let mut progressed = false;
                // Feed whichever track is behind, so the writer can interleave.
                let video_turn = sample >= total || (frame < FRAMES && (frame as f64 / FPS as f64) <= sample as f64 / RATE);
                if video_turn && video.isReadyForMoreMediaData() {
                    let (top, bottom) = if portrait { (RED, BLUE) } else if frame < FRAMES / 2 { (RED, RED) } else { (BLUE, BLUE) };
                    assert!(adaptor.appendPixelBuffer_withPresentationTime(&picture(top, bottom), CMTime::new(frame, FPS)), "{:?}", writer.error());
                    frame += 1;
                    progressed = true;
                } else if !video_turn && let Some(input) = &sound && input.isReadyForMoreMediaData() {
                    let n = chunk.min((total - sample) as usize);
                    assert!(input.appendSampleBuffer(&silence(&format, sample, n)), "{:?}", writer.error());
                    sample += n as i64;
                    progressed = true;
                }
                if !progressed {
                    pump(0.005);
                }
            }
            video.markAsFinished();
            if let Some(input) = &sound {
                input.markAsFinished();
            }
            let block = block2::RcBlock::new(|| {});
            writer.finishWritingWithCompletionHandler(&block);
            let deadline = Instant::now() + Duration::from_secs(20);
            while writer.status() == AVAssetWriterStatus::Writing {
                assert!(Instant::now() < deadline, "finishing stalled");
                pump(0.01);
            }
            assert_eq!(writer.status(), AVAssetWriterStatus::Completed, "{:?}", writer.error());
        }
    }

    /// Runs the main run loop for `secs`, as the app's event loop would.
    fn pump(secs: f64) {
        NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(secs));
    }

    /// Pumps until `done` holds or `secs` pass.
    fn wait_for(secs: f64, mut done: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs_f64(secs);
        while Instant::now() < deadline {
            if done() {
                return true;
            }
            pump(0.01);
        }
        done()
    }

    fn is_red(f: &Frame) -> bool {
        let [r, g, b] = f.mean();
        r > 180.0 && g < 70.0 && b < 70.0
    }

    fn is_blue(f: &Frame) -> bool {
        let [r, g, b] = f.mean();
        b > 180.0 && r < 70.0 && g < 70.0
    }

    fn check(name: &str, ok: bool, detail: impl std::fmt::Debug) {
        if ok {
            println!("ok   {name}");
        } else {
            panic!("FAIL {name}: {detail:?}");
        }
    }

    /// The next frame the player hands out, within a few seconds.
    fn next_frame(v: &mut Video) -> Option<Frame> {
        let mut got = None;
        wait_for(5.0, || {
            got = v.frame();
            got.is_some()
        });
        got
    }

    /// A preview read on another thread, as the grid's hover does.
    fn prepared(path: &Path, side: u32) -> Prepared {
        let path = path.to_path_buf();
        std::thread::spawn(move || Prepared::new(&path, side)).join().unwrap().expect("prepare")
    }

    /// The next picture of a playing preview, running it as the app does.
    fn small_frame(v: &mut Video) -> Option<Frame> {
        let mut got = None;
        wait_for(5.0, || {
            v.tick();
            got = v.frame();
            got.is_some()
        });
        got
    }

    /// The hover preview's player: small pictures, a scrub, then playing on
    /// and looping.
    fn preview(path: &Path) {
        let mut v = prepared(path, 32).start().expect("start preview");
        check("preview: the system plays it", v.backend() == Backend::System, v.backend());
        v.set_muted(true);
        v.set_looping(true);
        v.play();
        let f = small_frame(&mut v);
        let long = W.max(H);
        let want = (W * 32 / long, H * 32 / long);
        check("preview: the picture is small and red", f.as_ref().is_some_and(|f| (f.width, f.height) == want && is_red(f)), f.map(|f| (f.width, f.height, f.mean())));
        v.pause();
        v.seek(1.5);
        let mut f = None;
        wait_for(5.0, || {
            v.tick();
            if let Some(g) = v.frame() {
                f = Some(g);
            }
            f.as_ref().is_some_and(is_blue)
        });
        check("preview: a scrub shows that time while paused", f.as_ref().is_some_and(|f| is_blue(f) && (f.width, f.height) == want), f.map(|f| f.mean()));
        v.play();
        let mut looped = false;
        wait_for(5.0, || {
            v.tick();
            let _ = v.frame();
            looped = v.current_time() < 1.0;
            looped
        });
        check("preview: playing on loops at the end", looped, v.current_time());
    }

    pub fn run() {
        let dir = tempfile::tempdir().unwrap();
        let (silent, sound) = (dir.path().join("silent.mov"), dir.path().join("sound.mp4"));
        write_clip(&silent, false, false);
        write_clip(&sound, true, false);
        preview(&sound);

        for (path, audio) in [(&silent, false), (&sound, true)] {
            let name = path.file_name().unwrap().to_string_lossy();
            let info = probe(path).expect("probe");
            check(&format!("{name}: duration is 2 s"), (info.duration - 2.0).abs() < 0.1, info.duration);
            check(&format!("{name}: size is {W}x{H}"), (info.width, info.height) == (W as u32, H as u32), (info.width, info.height));
            check(&format!("{name}: has_audio is {audio}"), info.has_audio == audio, info.has_audio);
            check(&format!("{name}: 12 fps"), (info.fps - 12.0).abs() < 0.5, info.fps);
            check(&format!("{name}: no rotation"), info.turns == 0, info.turns);
            let first = frame_at(path, 0.0).expect("first frame");
            check(&format!("{name}: first frame is red"), is_red(&first) && (first.width, first.height) == (W, H), first.mean());
            let later = frame_at(path, 1.5).expect("frame at 1.5 s");
            check(&format!("{name}: frame at 1.5 s is blue"), is_blue(&later), later.mean());
            // A grabber off the main thread gives small pictures, many times.
            let p = path.to_path_buf();
            let (small, again) = std::thread::spawn(move || {
                let g = Grabber::open(&p, 40).expect("grabber");
                (g.grab(0.2).expect("grab"), g.grab(1.7).expect("grab again"))
            })
            .join()
            .unwrap();
            check(&format!("{name}: grabbed pictures fit 40 px"), (small.width, small.height) == (40, 30) && is_red(&small) && is_blue(&again), (small.width, small.height));
        }

        // Files that aren't video fail cleanly everywhere.
        let text = dir.path().join("notes.mp4");
        std::fs::write(&text, b"not a video at all").unwrap();
        let missing = dir.path().join("missing.mov");
        for p in [&text, &missing, &dir.path().to_path_buf()] {
            check(&format!("{} is refused", p.display()), probe(p).is_err() && frame_at(p, 0.0).is_err() && Video::open(p).is_err() && Grabber::open(p, 40).is_err(), probe(p));
        }

        // The player is main-thread-only and says so elsewhere.
        let p = silent.clone();
        let off_main = std::thread::spawn(move || Video::open(&p).err().map(|e| e.0)).join().unwrap();
        check("Video::open off the main thread fails", off_main.is_some(), off_main);

        let mut v = Video::open(&silent).expect("open");
        check("player info matches probe", v.info().duration > 1.9 && v.info().width == W as u32 && !v.info().has_audio, v.info());
        check("player becomes ready", wait_for(5.0, || v.is_ready()), v.error());
        let f = next_frame(&mut v);
        check("player's first frame is red", f.as_ref().is_some_and(is_red), f.map(|f| f.mean()));

        v.seek(1.5);
        check("seek moves the playhead", wait_for(3.0, || (v.current_time() - 1.5).abs() < 0.01), v.current_time());
        let f = next_frame(&mut v);
        check("frame after seeking to 1.5 s is blue", f.as_ref().is_some_and(is_blue), f.map(|f| f.mean()));
        let cur = v.current_frame();
        check("current_frame at 1.5 s is blue and full size", cur.as_ref().is_ok_and(|f| is_blue(f) && (f.width, f.height) == (W, H)), cur.map(|f| f.mean()));

        v.seek(0.5);
        wait_for(3.0, || (v.current_time() - 0.5).abs() < 0.01);
        let t0 = v.current_time();
        v.step(1);
        check("step(1) moves one frame forward", wait_for(3.0, || (v.current_time() - (t0 + 1.0 / 12.0)).abs() < 0.01), (t0, v.current_time()));
        v.step(-1);
        check("step(-1) moves back", wait_for(3.0, || (v.current_time() - t0).abs() < 0.01), (t0, v.current_time()));
        check("stepping pauses", !v.is_playing(), v.is_playing());

        // Playback advances the clock and stops at the end unless looping.
        v.seek(0.0);
        wait_for(2.0, || v.current_time() < 0.01);
        v.play();
        check("play advances", wait_for(3.0, || v.current_time() > 0.3), v.current_time());
        v.pause();
        let t = v.current_time();
        pump(0.3);
        check("pause holds the playhead", (v.current_time() - t).abs() < 0.05, (t, v.current_time()));

        v.set_rate(2.0);
        v.seek(1.5);
        wait_for(2.0, || (v.current_time() - 1.5).abs() < 0.01);
        v.play();
        let stopped = wait_for(4.0, || {
            v.tick();
            !v.is_playing()
        });
        check("without loop, playback stops at the end", stopped && v.current_time() > 1.8, v.current_time());

        v.set_looping(true);
        v.set_rate(1.0);
        v.seek(1.7);
        wait_for(2.0, || (v.current_time() - 1.7).abs() < 0.01);
        v.play();
        let looped = wait_for(4.0, || {
            v.tick();
            v.current_time() < 1.0 && v.current_time() > 0.0
        });
        check("with loop, playback starts again from the top", looped && v.is_playing(), v.current_time());
        drop(v);

        // A clip with sound opens and plays too.
        let mut v = Video::open(&sound).expect("open sound clip");
        check("sound clip becomes ready", wait_for(5.0, || v.is_ready()), v.error());
        v.set_volume(0.0);
        v.play();
        check("sound clip plays", wait_for(3.0, || v.current_time() > 0.2), v.current_time());
        drop(v);

        // A clip tagged with a rotation is shown upright: stored red over
        // blue, turned clockwise it reads blue on the left, red on the right.
        let portrait = dir.path().join("portrait.mov");
        write_clip(&portrait, false, true);
        let info = probe(&portrait).expect("probe portrait");
        check("portrait: size is turned", (info.width, info.height, info.turns) == (H as u32, W as u32, 1), &info);
        let upright = |f: &Frame| {
            let (l, r) = (f.pixel(f.width / 4, f.height / 2), f.pixel(f.width * 3 / 4, f.height / 2));
            (f.width, f.height) == (H, W) && l[2] > 180 && l[0] < 70 && r[0] > 180 && r[2] < 70
        };
        let mut v = prepared(&portrait, 32).start().expect("start portrait preview");
        v.play();
        let f = small_frame(&mut v);
        let small_upright = |f: &Frame| {
            let (l, r) = (f.pixel(f.width / 4, f.height / 2), f.pixel(f.width * 3 / 4, f.height / 2));
            (f.width, f.height) == (24, 32) && l[2] > 180 && l[0] < 70 && r[0] > 180 && r[2] < 70
        };
        check("portrait preview: small and upright", f.as_ref().is_some_and(small_upright), f.map(|f| (f.width, f.height)));
        drop(v);
        let still = frame_at(&portrait, 0.5).expect("portrait frame");
        check("portrait: decoded frame is upright", upright(&still), (still.width, still.height));
        let mut v = Video::open(&portrait).expect("open portrait");
        wait_for(5.0, || v.is_ready());
        let f = next_frame(&mut v);
        check("portrait: player frame is upright", f.as_ref().is_some_and(upright), f.map(|f| (f.width, f.height)));
        drop(v);

        // The same H.264/AAC clips through the FFmpeg fallback, when it is
        // installed (scripts/build-ffmpeg.sh): same facts, same pictures.
        if falcon_ffmpeg::available() {
            for path in [&silent, &sound] {
                let name = path.file_name().unwrap().to_string_lossy();
                let ours = falcon_ffmpeg::probe(path).expect("FFmpeg probe");
                let system = probe(path).unwrap();
                check(&format!("FFmpeg {name}: same facts"), (ours.width, ours.height, ours.has_audio, ours.turns) == (system.width, system.height, system.has_audio, 0) && (ours.duration - system.duration).abs() < 0.1, &ours);
                let (first, later) = (falcon_ffmpeg::frame_at(path, 0.0).unwrap(), falcon_ffmpeg::frame_at(path, 1.5).unwrap());
                check(&format!("FFmpeg {name}: red then blue"), is_red(&first) && is_blue(&later), (first.mean(), later.mean()));
            }
            let info = falcon_ffmpeg::probe(&portrait).unwrap();
            check("FFmpeg portrait: size is turned", (info.width, info.height, info.turns) == (H as u32, W as u32, 1), &info);
            let still = falcon_ffmpeg::frame_at(&portrait, 0.5).unwrap();
            check("FFmpeg portrait: frame is upright", upright(&still), (still.width, still.height));
            let (mut p, out) = falcon_ffmpeg::Player::open(&sound, true).unwrap();
            check("FFmpeg sound.mp4: has a sound output", out.is_some(), ());
            p.set_muted(true);
            p.play();
            let start = Instant::now();
            let mut frames = 0;
            while start.elapsed() < Duration::from_millis(600) {
                p.tick();
                frames += usize::from(p.frame().is_some());
                std::thread::sleep(Duration::from_millis(5));
            }
            check("FFmpeg sound.mp4: plays on the wall clock when muted", frames >= 5 && p.current_time() > 0.4, (frames, p.current_time()));
            drop(p);

            // Eagle's no-frame switch: a clip paused on its poster stays with the
            // system player, even when nothing asked for a frame in time.
            let mut v = Video::open(&sound).expect("open for the no-frame check");
            wait_for(1.7, || false);
            v.tick();
            check("no-frame switch: a paused clip with a picture stays", v.backend() == Backend::System, v.backend());
            check("no-frame switch: the picture found is handed out", v.frame().is_some_and(|f| is_red(&f)), ());
            v.expire_first_frame_time();
            v.tick();
            check("no-frame switch: never after a picture", v.backend() == Backend::System, v.backend());
            // No picture in time: FFmpeg takes over once, with the settings.
            let mut v = Video::open(&sound).expect("open for the no-frame check");
            v.set_rate(2.0);
            v.set_looping(true);
            v.set_muted(true);
            v.set_volume(0.5);
            v.seek(0.5);
            v.tick();
            check("no-frame switch: not before the limit", v.backend() == Backend::System, v.backend());
            v.expire_first_frame_time();
            v.tick();
            check("no-frame switch: FFmpeg takes over", v.backend() == Backend::Ffmpeg, v.backend());
            check("no-frame switch: speed and loop carry over", v.rate() == 2.0 && v.looping(), (v.rate(), v.looping()));
            let mut f = None;
            wait_for(5.0, || {
                v.tick();
                f = v.frame();
                f.is_some()
            });
            check("no-frame switch: FFmpeg shows the picture", f.is_some() && v.backend() == Backend::Ffmpeg, v.backend());

            // AV1 (dav1d inside FFmpeg) in WebM, Matroska and MP4, through
            // the calls the viewer, the preview window and hover previews
            // make. WebM and Matroska go to FFmpeg; MP4 to AVFoundation
            // first, which decodes AV1 on Macs with an AV1 decoder (M3 and
            // later), else to FFmpeg. Frame `i` is flat (10 + 7i, 100, 200).
            let ivf = include_bytes!("../../falcon-ffmpeg/testdata/av1.ivf");
            let clip = falcon_ffmpeg::testing::av1::Av1Clip::from_ivf(ivf).unwrap();
            let shows = |f: &Frame, i: f32| {
                let [r, g, b] = f.mean();
                (r - (10.0 + 7.0 * i)).abs() < 12.0 && (g - 100.0).abs() < 12.0 && (b - 200.0).abs() < 12.0
            };
            let dav1d = falcon_ffmpeg::ffi::api().is_ok_and(|api| falcon_ffmpeg::ffi::has_decoder(api, "libdav1d"));
            if !dav1d {
                println!("skip AV1 checks: this FFmpeg was built without dav1d (run scripts/build-ffmpeg.sh)");
            }
            let av1 = if dav1d { vec![("av1.webm", clip.matroska(true)), ("av1.mkv", clip.matroska(false)), ("av1.mp4", clip.mp4())] } else { Vec::new() };
            for (name, bytes) in av1.clone() {
                // A preview of a file the system can't draw: FFmpeg takes over
                // in `tick`, at the same small size, on a worker.
                let path = dir.path().join(format!("preview-{name}"));
                std::fs::write(&path, bytes).unwrap();
                let mut v = prepared(&path, 32).start().expect("start AV1 preview");
                v.play();
                let f = small_frame(&mut v);
                check(&format!("AV1 preview {name}: small picture ({:?})", v.backend()), f.as_ref().is_some_and(|f| f.width == 32 && f.height == 24), f.map(|f| (f.width, f.height)));
            }
            for (name, bytes) in av1 {
                let path = dir.path().join(name);
                std::fs::write(&path, bytes).unwrap();
                let info = probe(&path).expect("probe AV1");
                check(&format!("AV1 {name}: facts"), (info.width, info.height) == (64, 48) && (info.duration - 1.0).abs() < 0.05, &info);
                let f = frame_at(&path, 0.6).expect("AV1 frame");
                check(&format!("AV1 {name}: picture at 0.6 s"), shows(&f, 15.0), f.mean());
                let g = Grabber::open(&path, 32).expect("AV1 grabber");
                let small = g.grab(0.2).expect("AV1 hover picture");
                check(&format!("AV1 {name}: hover picture"), small.width == 32 && shows(&small, 5.0), (small.width, small.mean()));
                // `tick` is where a failed system player hands over to FFmpeg.
                let mut v = Video::open(&path).expect("open AV1");
                let mut f = None;
                wait_for(5.0, || {
                    v.tick();
                    f = v.frame();
                    f.is_some()
                });
                check(&format!("AV1 {name}: player shows the first frame ({:?})", v.backend()), f.as_ref().is_some_and(|f| shows(f, 0.0)), f.map(|f| f.mean()));
            }
            // What macOS refuses is opened by the worker, with small pictures.
            let clip = falcon_ffmpeg::testing::Clip { width: 64, height: 48, frames: 25, fps: 25, audio_rate: Some(16_000) };
            let avi = dir.path().join("preview.avi");
            std::fs::write(&avi, clip.avi(|_, _| [10, 100, 200])).unwrap();
            let mut v = prepared(&avi, 32).start().expect("start AVI preview");
            check("AVI preview: FFmpeg plays it", v.backend() == Backend::Ffmpeg, v.backend());
            check("AVI preview: wants an output", v.needs_mixer(), ());
            v.set_looping(true);
            v.play();
            let f = small_frame(&mut v);
            check("AVI preview: small picture", f.as_ref().is_some_and(|f| (f.width, f.height) == (32, 24)), f.map(|f| (f.width, f.height)));
            let began = Instant::now();
            drop(v);
            check("AVI preview: dropping doesn't wait", began.elapsed() < Duration::from_millis(50), began.elapsed());
        } else {
            println!("skip FFmpeg checks: {:?}", falcon_ffmpeg::status().err());
        }
        println!("all AVFoundation checks passed");
    }
}
