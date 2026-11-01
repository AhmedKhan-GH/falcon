//! The AVFoundation side: every Objective-C and Core Media call in falcon's
//! video support is in this file.
//!
//! Most framework methods are `unsafe` in objc2 only because their
//! bindings cannot prove more than the types say. The invariants the calls
//! here rely on are the same throughout, so each block names which apply:
//!
//! - **owned**: every receiver and argument is a live `Retained`/`CFRetained`
//!   (or a reference borrowed from one) of the class the binding expects.
//! - **plain**: arguments are plain values (`CMTime`, numbers, `bool`) or
//!   null where the framework documents null as allowed.
//! - **main**: the call is on an object of a main-thread-only class
//!   (`AVPlayer`, `AVPlayerItem`) and we hold a `MainThreadMarker` or are
//!   inside a `Video`, which can only be built with one and is not `Send`.

use std::path::Path;
use std::ptr;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, MainThreadMarker};
use objc2_av_foundation::{
    AVAsset, AVAssetImageGenerator, AVMediaTypeAudio, AVMediaTypeVideo, AVPlayer, AVPlayerActionAtItemEnd, AVPlayerItem, AVPlayerItemStatus,
    AVPlayerItemVideoOutput, AVURLAsset,
};
use objc2_core_foundation::{CFString, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo, CGImageByteOrderInfo};
use objc2_core_media::{CMClock, CMTime};
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow, CVPixelBufferGetHeight, CVPixelBufferGetPixelFormatType,
    CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress, kCVPixelBufferPixelFormatTypeKey,
    kCVPixelFormatType_32BGRA, kCVReturnSuccess,
};
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};

use crate::pixels::{self, Frame, quarter_turns};
use crate::{Error, Info, Result};

/// Whether this is the main thread, where AVFoundation's player lives.
pub fn on_main_thread() -> bool {
    MainThreadMarker::new().is_some()
}

/// The timescale used for times we build: fine enough for any frame rate.
const TIMESCALE: i32 = 600_000;

fn err(msg: impl Into<String>) -> Error {
    Error(msg.into())
}

fn time(secs: f64) -> CMTime {
    // SAFETY: plain.
    unsafe { CMTime::with_seconds(secs.max(0.0), TIMESCALE) }
}

/// Seconds of a `CMTime`; invalid and indefinite times read as 0.
fn seconds(t: CMTime) -> f64 {
    // SAFETY: plain.
    let s = unsafe { t.seconds() };
    if s.is_finite() { s } else { 0.0 }
}

fn zero() -> CMTime {
    // SAFETY: plain.
    unsafe { CMTime::new(0, 1) }
}

/// An asset for a local file. AVFoundation reads lazily, so a file that is
/// not media only shows up later as an asset with no tracks.
fn asset(path: &Path) -> Result<Retained<AVURLAsset>> {
    if !path.is_file() {
        return Err(err(format!("no file at {}", path.display())));
    }
    let path = path.to_str().ok_or_else(|| err("the path is not valid UTF-8"))?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    // SAFETY: owned; `None` is the documented "no options".
    Ok(unsafe { AVURLAsset::URLAssetWithURL_options(&url, None) })
}

/// Reads what the viewer needs before playing. Fails for files without a
/// video track AVFoundation can play (WebM, Matroska, non-media files).
pub fn probe(path: &Path) -> Result<Info> {
    let asset = asset(path)?;
    info(&asset)
}

#[allow(deprecated)] // The synchronous track getters are fine for local files.
fn info(asset: &AVAsset) -> Result<Info> {
    let (video, audio) = (media_type(true)?, media_type(false)?);
    // SAFETY: owned. The getters load the asset's header synchronously,
    // which is safe off and on the main thread for a local file.
    unsafe {
        let track = asset.tracksWithMediaType(video).firstObject().ok_or_else(|| err("no video track this system can read"))?;
        if !asset.isPlayable() || !track.isPlayable() {
            return Err(err("this system can't play the video"));
        }
        let size = track.naturalSize();
        let transform = track.preferredTransform();
        let turns = quarter_turns(transform.a, transform.b);
        let (w, h) = (size.width.abs().round() as u32, size.height.abs().round() as u32);
        let (width, height) = if turns % 2 == 1 { (h, w) } else { (w, h) };
        let duration = seconds(asset.duration());
        let has_audio = asset.tracksWithMediaType(audio).count() > 0;
        Ok(Info { duration, width, height, fps: track.nominalFrameRate(), has_audio, turns })
    }
}

fn media_type(video: bool) -> Result<&'static NSString> {
    // SAFETY: reading framework constants that are set at load time.
    let t = unsafe { if video { AVMediaTypeVideo } else { AVMediaTypeAudio } };
    t.ok_or_else(|| err("AVFoundation is not available"))
}

/// The picture at `secs` (exactly, not the nearest keyframe), turned the
/// way the track asks. Safe to call from any thread.
pub fn frame_at(path: &Path, secs: f64) -> Result<Frame> {
    let asset = asset(path)?;
    info(&asset)?;
    frame_of(&asset, secs)
}

/// The picture of an already probed asset at `secs`, as [`frame_at`].
#[allow(deprecated)] // The synchronous image call blocks, which is what callers want.
fn frame_of(asset: &AVAsset, secs: f64) -> Result<Frame> {
    // SAFETY: owned, plain. AVAssetImageGenerator is usable from any thread.
    let image = unsafe {
        let generator = AVAssetImageGenerator::assetImageGeneratorWithAsset(asset);
        generator.setAppliesPreferredTrackTransform(true);
        generator.setRequestedTimeToleranceBefore(zero());
        generator.setRequestedTimeToleranceAfter(zero());
        generator.copyCGImageAtTime_actualTime_error(time(secs), ptr::null_mut()).map_err(|e| err(e.localizedDescription().to_string()))?
    };
    cg_to_frame(&image)
}

/// Decodes small pictures of one file quickly, for previews such as the
/// seek bar's hover thumbnail: one image generator kept open, pictures no
/// larger than `max_side` on their long side.
///
/// Usable from any thread, but not `Send`: open it on the thread that uses it.
pub struct Grabber {
    generator: Retained<AVAssetImageGenerator>,
}

impl Grabber {
    /// Opens `path` for grabbing; fails for anything [`probe`] refuses.
    pub fn open(path: &Path, max_side: u32) -> Result<Grabber> {
        let asset = asset(path)?;
        info(&asset)?;
        let side = f64::from(max_side.max(1));
        // SAFETY: owned, plain. AVAssetImageGenerator is usable from any thread.
        let generator = unsafe {
            let g = AVAssetImageGenerator::assetImageGeneratorWithAsset(&asset);
            g.setAppliesPreferredTrackTransform(true);
            g.setMaximumSize(CGSize::new(side, side));
            g.setRequestedTimeToleranceBefore(zero());
            g.setRequestedTimeToleranceAfter(zero());
            g
        };
        Ok(Grabber { generator })
    }

    /// The picture at `secs`, scaled down to fit the size given to `open`.
    #[allow(deprecated)] // The synchronous image call blocks, which is what callers want.
    pub fn grab(&self, secs: f64) -> Result<Frame> {
        // SAFETY: owned, plain.
        let image = unsafe { self.generator.copyCGImageAtTime_actualTime_error(time(secs), ptr::null_mut()) }.map_err(|e| err(e.localizedDescription().to_string()))?;
        cg_to_frame(&image)
    }
}

/// Draws a CGImage into an RGBA buffer of its own size.
fn cg_to_frame(image: &CGImage) -> Result<Frame> {
    let (w, h) = (CGImage::width(Some(image)), CGImage::height(Some(image)));
    if w == 0 || h == 0 {
        return Err(err("the decoder returned an empty picture"));
    }
    let mut rgba = vec![0u8; w * h * 4];
    let space = CGColorSpace::new_device_rgb().ok_or_else(|| err("no RGB colour space"))?;
    let info = CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0;
    // SAFETY: `rgba` is `w * h * 4` bytes and outlives the context, which
    // is dropped at the end of this function; 8 bits per component with
    // `w * 4` bytes per row matches that buffer exactly.
    let ctx = unsafe { CGBitmapContextCreate(rgba.as_mut_ptr().cast(), w, h, 8, w * 4, Some(&space), info) }.ok_or_else(|| err("no bitmap context"))?;
    let rect = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w as f64, h as f64));
    CGContext::draw_image(Some(&ctx), rect, Some(image));
    drop(ctx);
    // Video is opaque, so premultiplied and straight alpha are the same.
    for px in rgba.as_chunks_mut::<4>().0 {
        px[3] = 255;
    }
    Ok(Frame { width: w, height: h, rgba })
}

/// Copies a BGRA pixel buffer out as RGBA, turned `turns` quarter turns.
fn buffer_to_frame(buffer: &CVPixelBuffer, turns: u8) -> Option<Frame> {
    if CVPixelBufferGetPixelFormatType(buffer) != kCVPixelFormatType_32BGRA {
        return None;
    }
    // SAFETY: owned; the buffer is locked read-only for the copy, and the
    // base address is read only between lock and unlock, over `stride *
    // height` bytes, which is the buffer's size for a packed format.
    unsafe {
        if CVPixelBufferLockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly) != kCVReturnSuccess {
            return None;
        }
        let (w, h, stride) = (CVPixelBufferGetWidth(buffer), CVPixelBufferGetHeight(buffer), CVPixelBufferGetBytesPerRow(buffer));
        let base = CVPixelBufferGetBaseAddress(buffer) as *const u8;
        let frame = (!base.is_null()).then(|| pixels::from_bgra(std::slice::from_raw_parts(base, stride * h), w, h, stride, turns));
        CVPixelBufferUnlockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly);
        frame
    }
}

/// A playing video: AVPlayer handles decoding, timing and sound; each new
/// frame is copied out through an `AVPlayerItemVideoOutput`.
///
/// Main thread only (it is not `Send`). Dropping it stops playback.
pub struct Video {
    asset: Retained<AVURLAsset>,
    player: Retained<AVPlayer>,
    item: Retained<AVPlayerItem>,
    output: Retained<AVPlayerItemVideoOutput>,
    info: Info,
    /// Whether the user wants it playing (AVPlayer's rate drops to 0 at the
    /// end, which `tick` turns into a loop or a stop).
    playing: bool,
    looping: bool,
    rate: f32,
}

impl Video {
    /// Opens a file for playback, paused at the start. Fails off the main
    /// thread, and for anything [`probe`] refuses.
    pub fn open(path: &Path) -> Result<Video> {
        let mtm = MainThreadMarker::new().ok_or_else(|| err("video playback runs on the main thread"))?;
        let asset = asset(path)?;
        let info = info(&asset)?;
        // kCVPixelBufferPixelFormatTypeKey is a CFString, toll-free bridged
        // to NSString.
        // SAFETY: CFString and NSString share one object layout (toll-free
        // bridging), and the key is a constant that lives for the program.
        let key: &NSString = unsafe { &*(kCVPixelBufferPixelFormatTypeKey as *const CFString as *const NSString) };
        let format = NSNumber::new_u32(kCVPixelFormatType_32BGRA);
        let settings = NSDictionary::<NSString, AnyObject>::from_slices(&[key], &[format.as_ref()]);
        // SAFETY: owned, main (`mtm`); the dictionary holds the
        // NSString -> NSNumber pair the attributes initialiser documents.
        let (player, item, output) = unsafe {
            let output = AVPlayerItemVideoOutput::initWithPixelBufferAttributes(AVPlayerItemVideoOutput::alloc(), Some(&settings));
            let item = AVPlayerItem::playerItemWithAsset(&asset, mtm);
            item.addOutput(&output);
            let player = AVPlayer::playerWithPlayerItem(Some(&item), mtm);
            player.setActionAtItemEnd(AVPlayerActionAtItemEnd::Pause);
            (player, item, output)
        };
        Ok(Video { asset, player, item, output, info, playing: false, looping: false, rate: 1.0 })
    }

    pub fn info(&self) -> &Info {
        &self.info
    }

    /// The item has loaded and can show frames.
    pub fn is_ready(&self) -> bool {
        // SAFETY: owned, main.
        unsafe { self.item.status() == AVPlayerItemStatus::ReadyToPlay }
    }

    /// Why the item can't play, once AVFoundation has given up on it.
    pub fn error(&self) -> Option<String> {
        // SAFETY: owned, main.
        unsafe {
            if self.item.status() != AVPlayerItemStatus::Failed {
                return None;
            }
            Some(self.item.error().map_or_else(|| "playback failed".to_string(), |e| e.localizedDescription().to_string()))
        }
    }

    /// Plays at the current speed; from the end, plays from the start.
    pub fn play(&mut self) {
        if self.at_end() {
            self.seek(0.0);
        }
        self.playing = true;
        // SAFETY: owned, main, plain.
        unsafe { self.player.setRate(self.rate) };
    }

    pub fn pause(&mut self) {
        self.playing = false;
        // SAFETY: owned, main.
        unsafe { self.player.pause() };
    }

    pub fn toggle(&mut self) {
        if self.playing { self.pause() } else { self.play() }
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Jumps to `secs` exactly (clamped to the clip).
    pub fn seek(&mut self, secs: f64) {
        let secs = secs.clamp(0.0, self.info.duration.max(0.0));
        // SAFETY: owned, main, plain.
        unsafe { self.player.seekToTime_toleranceBefore_toleranceAfter(time(secs), zero(), zero()) };
    }

    /// Moves `frames` frames forwards (or backwards when negative) and
    /// pauses, as frame stepping does in any player.
    pub fn step(&mut self, frames: i32) {
        self.pause();
        // SAFETY: owned, main, plain. Stepping past either end is clamped
        // by AVFoundation.
        unsafe { self.item.stepByCount(frames as isize) };
    }

    pub fn rate(&self) -> f32 {
        self.rate
    }

    pub fn set_rate(&mut self, rate: f32) {
        self.rate = rate.clamp(crate::RATES[0], crate::RATES[crate::RATES.len() - 1]);
        if self.playing {
            // SAFETY: owned, main, plain.
            unsafe { self.player.setRate(self.rate) };
        }
    }

    /// 0..=1.
    pub fn set_volume(&mut self, volume: f32) {
        // SAFETY: owned, main, plain.
        unsafe { self.player.setVolume(volume.clamp(0.0, 1.0)) };
    }

    pub fn set_muted(&mut self, muted: bool) {
        // SAFETY: owned, main, plain.
        unsafe { self.player.setMuted(muted) };
    }

    pub fn set_looping(&mut self, looping: bool) {
        self.looping = looping;
    }

    pub fn looping(&self) -> bool {
        self.looping
    }

    /// The playhead in seconds.
    pub fn current_time(&self) -> f64 {
        // SAFETY: owned, main.
        seconds(unsafe { self.player.currentTime() }).clamp(0.0, self.info.duration.max(0.0))
    }

    fn at_end(&self) -> bool {
        let frame = if self.info.fps > 0.0 { 1.0 / self.info.fps as f64 } else { 0.05 };
        self.info.duration > 0.0 && self.current_time() >= self.info.duration - frame
    }

    /// Call every frame: at the end of the clip, starts again from the top
    /// when looping, or stops.
    pub fn tick(&mut self) {
        // SAFETY: owned, main.
        let stopped = unsafe { self.player.rate() } == 0.0;
        if self.playing && stopped && self.at_end() {
            if self.looping {
                self.seek(0.0);
                // SAFETY: owned, main, plain.
                unsafe { self.player.setRate(self.rate) };
            } else {
                self.playing = false;
            }
        }
    }

    /// The picture at the playhead at the clip's full size, turned upright:
    /// what Eagle's frame tools (set as thumbnail, copy, save) capture.
    /// Decoded afresh by an image generator on the player's asset, so it
    /// does not disturb playback or the frames [`Video::frame`] hands out.
    pub fn current_frame(&self) -> Result<Frame> {
        frame_of(&self.asset, self.current_time())
    }

    /// The frame due now, if it differs from the last one handed out.
    pub fn frame(&mut self) -> Option<Frame> {
        // SAFETY: owned, main, plain. The host clock is a process-wide
        // singleton; `itemTimeForHostTime` maps its seconds onto the item.
        unsafe {
            let now = seconds(CMClock::host_time_clock().time());
            let t = self.output.itemTimeForHostTime(now);
            if !self.output.hasNewPixelBufferForItemTime(t) {
                return None;
            }
            let buffer = self.output.copyPixelBufferForItemTime_itemTimeForDisplay(t, ptr::null_mut())?;
            buffer_to_frame(&buffer, self.info.turns)
        }
    }
}

impl Drop for Video {
    fn drop(&mut self) {
        // SAFETY: owned, main (a `Video` never leaves the main thread).
        unsafe {
            self.player.pause();
            self.player.replaceCurrentItemWithPlayerItem(None);
        }
    }
}
