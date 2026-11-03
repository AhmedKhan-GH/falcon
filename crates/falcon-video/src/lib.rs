//! Video for falcon's viewer. Files play through the operating system's
//! own media framework, AVFoundation on macOS (H.264, HEVC, ProRes, … in
//! mp4, mov and m4v). Whatever it refuses (Matroska, WebM, AVI, WMV, FLV,
//! Ogg, MPEG-TS, …) falls back to FFmpeg through `falcon-ffmpeg`, which
//! loads an LGPL FFmpeg at run time when one is present, as Eagle falls
//! back from Chromium's player to its bundled one. One [`Video`] drives
//! the same viewer either way.
//!
//! A [`Video`] on the system player also moves to FFmpeg, once, when the
//! system player fails after opening, or when it has drawn no picture within
//! 1.5 s of opening: Eagle gives its `<video>` element the same time. The
//! system player draws a poster picture while paused, so a paused clip
//! counts as drawn. A slow disk gets one switch: FFmpeg is not left again,
//! and if it can't open the file the system player stays and keeps its time.
//!
//! - [`probe`] reads a file's duration, display size and audio track.
//! - [`frame_at`] decodes the frame at a time, on any thread.
//! - [`Grabber`] decodes small pictures of one file again and again, for
//!   previews such as the seek bar's hover thumbnail.
//! - [`Video`] plays a file with sound and hands out each new frame as RGBA
//!   pixels for the caller to upload as a texture. It lives on the main
//!   thread, as AVFoundation's player requires.
//! - [`Prepared`] readies a small [`Video`] off the main thread, for the
//!   grid's hover previews: its frames fit a given side, and nothing about
//!   opening or dropping it waits on the file.
//!
//! All Objective-C interop sits in the private `mac` module; this file and
//! `pixels` are plain Rust.

use std::cell::RefCell;
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

mod pixels;

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
use mac as sys;

#[cfg(not(target_os = "macos"))]
mod other;
#[cfg(not(target_os = "macos"))]
use other as sys;

pub use falcon_ffmpeg::Info;
pub use pixels::{Frame, quarter_turns};

/// Why a file could not be opened, probed or decoded.
#[derive(Clone, Debug, PartialEq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<falcon_ffmpeg::Error> for Error {
    fn from(e: falcon_ffmpeg::Error) -> Self {
        Error(e.0)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Playback speeds, slowest first: the steps the speed keys move through.
pub const RATES: [f32; 11] = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 3.0, 4.0, 8.0];

/// The next speed up (or down) from `rate`; the ends stay put. A rate not
/// in [`RATES`] moves to the nearest step in that direction.
pub fn step_rate(rate: f32, up: bool) -> f32 {
    if up {
        RATES.iter().copied().find(|&r| r > rate + 1e-4).unwrap_or(rate)
    } else {
        RATES.iter().rev().copied().find(|&r| r < rate - 1e-4).unwrap_or(rate)
    }
}

/// Which decoder plays a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// The operating system's (AVFoundation).
    System,
    /// FFmpeg, loaded at run time.
    Ffmpeg,
}

/// Whether the FFmpeg fallback can be used.
pub fn ffmpeg_available() -> bool {
    falcon_ffmpeg::available()
}

/// The FFmpeg fallback's version and where it was loaded from, or why it
/// can't be used.
pub fn ffmpeg_status() -> std::result::Result<(String, PathBuf), String> {
    falcon_ffmpeg::status().map_err(|e| e.to_string())
}

/// Whether files with extension `ext` need the FFmpeg fallback: formats
/// macOS can't play, such as Matroska, WebM, AVI, WMV and FLV.
pub fn needs_ffmpeg(ext: &str) -> bool {
    falcon_ffmpeg::handles(ext)
}

/// How long the system player has to draw its first picture.
const NO_FRAME_LIMIT: Duration = Duration::from_millis(1500);

/// Whether a system player opened `elapsed` ago should give way to FFmpeg
/// for having drawn nothing. Once it has drawn a picture (a paused player's
/// poster counts) it never does, however long it then takes.
fn switch_for_no_frame(elapsed: Duration, had_frame: bool, ffmpeg: bool) -> bool {
    ffmpeg && !had_frame && elapsed >= NO_FRAME_LIMIT
}

/// Runs `system`, and when it fails and FFmpeg is present, `ffmpeg`. The
/// system's error is kept unless FFmpeg was tried.
fn either<T>(system: impl FnOnce() -> Result<T>, ffmpeg: impl FnOnce() -> falcon_ffmpeg::Result<T>) -> Result<T> {
    match system() {
        Ok(v) => Ok(v),
        Err(e) if !ffmpeg_available() => Err(e),
        Err(_) => ffmpeg().map_err(Error::from),
    }
}

/// Reads what the viewer needs before playing. Fails for files with no
/// video track the system or FFmpeg can play.
pub fn probe(path: &Path) -> Result<Info> {
    either(|| sys::probe(path), || falcon_ffmpeg::probe(path))
}

/// The picture at `secs`, turned the way the track asks. Safe to call
/// from any thread.
pub fn frame_at(path: &Path, secs: f64) -> Result<Frame> {
    either(|| sys::frame_at(path, secs), || falcon_ffmpeg::frame_at(path, secs))
}

/// Decodes small pictures of one file quickly, for previews such as the
/// seek bar's hover thumbnail, with pictures no larger than `max_side` on
/// their long side.
///
/// Usable from any thread, but not `Send`: open it on the thread that uses it.
pub struct Grabber {
    grab: RefCell<Grab>,
    path: std::path::PathBuf,
    max_side: u32,
}

enum Grab {
    System(sys::Grabber),
    Ffmpeg(falcon_ffmpeg::Grabber),
}

impl Grabber {
    /// Opens `path` for grabbing; fails for anything [`probe`] refuses.
    pub fn open(path: &Path, max_side: u32) -> Result<Grabber> {
        let grab = either(
            || sys::Grabber::open(path, max_side).map(Grab::System),
            || falcon_ffmpeg::Grabber::open(path, max_side).map(Grab::Ffmpeg),
        )?;
        Ok(Grabber { grab: RefCell::new(grab), path: path.to_path_buf(), max_side })
    }

    /// The picture at `secs`, scaled down to fit the size given to `open`.
    /// When the system opened the file but can't decode its pictures (AV1
    /// in an MP4 on a Mac without an AV1 decoder, say), FFmpeg takes over
    /// for this and every later picture.
    pub fn grab(&self, secs: f64) -> Result<Frame> {
        let mut grab = self.grab.borrow_mut();
        match &mut *grab {
            Grab::Ffmpeg(g) => g.grab(secs).map_err(Error::from),
            Grab::System(g) => match g.grab(secs) {
                Ok(f) => Ok(f),
                Err(e) if !ffmpeg_available() => Err(e),
                Err(e) => {
                    let Ok(mut g) = falcon_ffmpeg::Grabber::open(&self.path, self.max_side) else { return Err(e) };
                    let f = g.grab(secs).map_err(Error::from);
                    *grab = Grab::Ffmpeg(g);
                    f
                }
            },
        }
    }
}

/// The FFmpeg player's sound as a rodio source: stereo `f32` at 48 kHz,
/// silent while paused, ending when the player is dropped.
struct Sound(falcon_ffmpeg::AudioOut);

impl Iterator for Sound {
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<rodio::Sample> {
        self.0.next_sample().map(|s| s as rodio::Sample)
    }
}

impl rodio::Source for Sound {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> rodio::ChannelCount {
        NonZero::new(falcon_ffmpeg::AudioOut::CHANNELS).unwrap_or(NonZero::<u16>::MIN)
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        NonZero::new(falcon_ffmpeg::AudioOut::RATE).unwrap_or(NonZero::<u32>::MIN)
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

/// What opening FFmpeg's player gives: the player and its sound.
type Opened = falcon_ffmpeg::Result<(falcon_ffmpeg::Player, Option<falcon_ffmpeg::AudioOut>)>;

enum Readied {
    System(sys::Prepared),
    Ffmpeg(falcon_ffmpeg::Player, Option<falcon_ffmpeg::AudioOut>),
}

/// A preview read and ready to play, as [`Video::start`] of what any thread
/// prepared: everything that waits on the file (AVFoundation reading its
/// header, FFmpeg probing its streams) has been done by then.
pub struct Prepared {
    readied: Readied,
    path: PathBuf,
    max_side: u32,
}

impl Prepared {
    /// Reads `path` for a preview whose pictures fit `max_side` on their
    /// long side, on any thread. Fails for anything [`probe`] refuses.
    pub fn new(path: &Path, max_side: u32) -> Result<Prepared> {
        let readied = match sys::Prepared::new(path) {
            Ok(p) => Readied::System(p),
            Err(e) if !ffmpeg_available() => return Err(e),
            Err(_) => {
                let (player, sound) = falcon_ffmpeg::Player::open_preview(path, true, max_side)?;
                Readied::Ffmpeg(player, sound)
            }
        };
        Ok(Prepared { readied, path: path.to_path_buf(), max_side })
    }

    /// The player, paused at the start. Main thread only. FFmpeg's sound
    /// stays silent until [`Video::set_mixer`] gives it an output, so a
    /// muted preview needs none.
    pub fn start(self) -> Result<Video> {
        if !sys::on_main_thread() {
            return Err(Error("video playback runs on the main thread".into()));
        }
        let (inner, held_sound) = match self.readied {
            Readied::System(p) => (Inner::System(sys::Video::start(p, Some(self.max_side))?), None),
            Readied::Ffmpeg(p, sound) => (Inner::Ffmpeg(p), sound),
        };
        let mut video = Video {
            inner,
            path: self.path,
            mixer: None,
            volume: 1.0,
            muted: false,
            fallback_error: None,
            opened: Instant::now(),
            had_frame: false,
            held_frame: None,
            preview_side: Some(self.max_side),
            held_sound,
            opening: None,
        };
        video.apply_mute();
        Ok(video)
    }
}

enum Inner {
    System(sys::Video),
    Ffmpeg(falcon_ffmpeg::Player),
}

/// A playing video, through whichever back end can play it. AVFoundation
/// is tried first; a file it refuses on opening, fails on later, or draws no
/// picture of within 1.5 s is played by FFmpeg when present, keeping the
/// player's settings and place.
///
/// Main thread only (it is not `Send`). Dropping it stops playback.
pub struct Video {
    inner: Inner,
    path: PathBuf,
    /// The output the FFmpeg player's sound goes to.
    mixer: Option<rodio::mixer::Mixer>,
    volume: f32,
    muted: bool,
    /// Why FFmpeg could not take over from the system player, once tried.
    fallback_error: Option<String>,
    /// When the system player was opened, for [`NO_FRAME_LIMIT`].
    opened: Instant,
    /// The system player has handed out a picture.
    had_frame: bool,
    /// A picture taken while checking for one, for the next [`Video::frame`].
    held_frame: Option<Frame>,
    /// The side a preview's pictures fit: such a player's move to FFmpeg
    /// keeps that size and opens off the main thread.
    preview_side: Option<u32>,
    /// A preview's FFmpeg sound while it has no output (its player is muted).
    held_sound: Option<falcon_ffmpeg::AudioOut>,
    /// FFmpeg's player being opened for a preview.
    opening: Option<mpsc::Receiver<Opened>>,
}

impl Video {
    /// Opens a file for playback, paused at the start, with no sound when
    /// FFmpeg plays it. Fails off the main thread, and for anything
    /// [`probe`] refuses.
    pub fn open(path: &Path) -> Result<Video> {
        Video::open_with(path, None)
    }

    /// As [`Video::open`]; when FFmpeg plays the file its sound goes to
    /// `mixer` (AVFoundation plays its own).
    pub fn open_with(path: &Path, mixer: Option<&rodio::mixer::Mixer>) -> Result<Video> {
        if !sys::on_main_thread() {
            return Err(Error("video playback runs on the main thread".into()));
        }
        let mixer = mixer.cloned();
        let inner = match sys::Video::open(path) {
            Ok(v) => Inner::System(v),
            Err(e) if !ffmpeg_available() => return Err(e),
            Err(_) => Inner::Ffmpeg(open_ffmpeg(path, mixer.as_ref())?),
        };
        Ok(Video { inner, path: path.to_path_buf(), mixer, volume: 1.0, muted: false, fallback_error: None, opened: Instant::now(), had_frame: false, held_frame: None, preview_side: None, held_sound: None, opening: None })
    }

    /// Which back end is playing.
    pub fn backend(&self) -> Backend {
        match self.inner {
            Inner::System(_) => Backend::System,
            Inner::Ffmpeg(_) => Backend::Ffmpeg,
        }
    }

    pub fn info(&self) -> &Info {
        match &self.inner {
            Inner::System(v) => v.info(),
            Inner::Ffmpeg(p) => p.info(),
        }
    }

    /// The video has loaded and can show frames.
    pub fn is_ready(&self) -> bool {
        match &self.inner {
            Inner::System(v) => v.is_ready(),
            Inner::Ffmpeg(p) => p.is_ready(),
        }
    }

    /// Why the video can't play, once every back end has given up. A system
    /// failure FFmpeg may still recover from (on the next [`Video::tick`])
    /// is not reported.
    pub fn error(&self) -> Option<String> {
        match &self.inner {
            Inner::System(v) => v.error().filter(|_| self.fallback_error.is_some() || !ffmpeg_available()),
            Inner::Ffmpeg(_) => None,
        }
    }

    /// Plays at the current speed; from the end, plays from the start.
    pub fn play(&mut self) {
        match &mut self.inner {
            Inner::System(v) => v.play(),
            Inner::Ffmpeg(p) => p.play(),
        }
    }

    pub fn pause(&mut self) {
        match &mut self.inner {
            Inner::System(v) => v.pause(),
            Inner::Ffmpeg(p) => p.pause(),
        }
    }

    pub fn toggle(&mut self) {
        match &mut self.inner {
            Inner::System(v) => v.toggle(),
            Inner::Ffmpeg(p) => p.toggle(),
        }
    }

    pub fn is_playing(&self) -> bool {
        match &self.inner {
            Inner::System(v) => v.is_playing(),
            Inner::Ffmpeg(p) => p.is_playing(),
        }
    }

    /// Jumps to `secs` exactly (clamped to the clip).
    pub fn seek(&mut self, secs: f64) {
        match &mut self.inner {
            Inner::System(v) => v.seek(secs),
            Inner::Ffmpeg(p) => p.seek(secs),
        }
    }

    /// Moves `frames` frames forwards (or backwards when negative) and
    /// pauses, as frame stepping does in any player.
    pub fn step(&mut self, frames: i32) {
        match &mut self.inner {
            Inner::System(v) => v.step(frames),
            Inner::Ffmpeg(p) => p.step(frames),
        }
    }

    pub fn rate(&self) -> f32 {
        match &self.inner {
            Inner::System(v) => v.rate(),
            Inner::Ffmpeg(p) => p.rate(),
        }
    }

    pub fn set_rate(&mut self, rate: f32) {
        let rate = rate.clamp(RATES[0], RATES[RATES.len() - 1]);
        match &mut self.inner {
            Inner::System(v) => v.set_rate(rate),
            Inner::Ffmpeg(p) => p.set_rate(rate),
        }
    }

    /// 0..=1.
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        match &mut self.inner {
            Inner::System(v) => v.set_volume(self.volume),
            Inner::Ffmpeg(p) => p.set_volume(self.volume),
        }
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        self.apply_mute();
    }

    /// Mutes the player as asked, and FFmpeg's while its sound has no output
    /// to go to (its clock would wait for one).
    fn apply_mute(&mut self) {
        match &mut self.inner {
            Inner::System(v) => v.set_muted(self.muted),
            Inner::Ffmpeg(p) => p.set_muted(self.muted || self.held_sound.is_some()),
        }
    }

    /// Whether this preview has yet to be given an output by [`Video::set_mixer`].
    pub fn needs_mixer(&self) -> bool {
        self.preview_side.is_some() && self.mixer.is_none()
    }

    /// Gives a preview the output its FFmpeg sound goes to, whichever
    /// player has it now or takes over later.
    pub fn set_mixer(&mut self, mixer: rodio::mixer::Mixer) {
        if let Some(s) = self.held_sound.take() {
            mixer.add(Sound(s));
        }
        self.mixer = Some(mixer);
        self.apply_mute();
    }

    pub fn set_looping(&mut self, looping: bool) {
        match &mut self.inner {
            Inner::System(v) => v.set_looping(looping),
            Inner::Ffmpeg(p) => p.set_looping(looping),
        }
    }

    pub fn looping(&self) -> bool {
        match &self.inner {
            Inner::System(v) => v.looping(),
            Inner::Ffmpeg(p) => p.looping(),
        }
    }

    /// The playhead in seconds.
    pub fn current_time(&self) -> f64 {
        match &self.inner {
            Inner::System(v) => v.current_time(),
            Inner::Ffmpeg(p) => p.current_time(),
        }
    }

    /// Call every frame: at the end of the clip, starts again from the top
    /// when looping, or stops. When the system player has failed or has
    /// drawn nothing in time, FFmpeg takes over here.
    pub fn tick(&mut self) {
        self.adopt_opened();
        if let Inner::System(v) = &self.inner
            && v.error().is_some()
            && self.fallback_error.is_none()
            && self.opening.is_none()
            && ffmpeg_available()
        {
            self.fall_back();
        }
        self.check_first_frame();
        match &mut self.inner {
            Inner::System(v) => v.tick(),
            Inner::Ffmpeg(p) => p.tick(),
        }
    }

    /// Applies [`switch_for_no_frame`]. A picture that arrived since the last
    /// [`Video::frame`] (a busy main thread can be that late) is kept, not
    /// taken for a player that drew nothing.
    fn check_first_frame(&mut self) {
        let Inner::System(v) = &mut self.inner else { return };
        if self.fallback_error.is_some() || self.opening.is_some() || !switch_for_no_frame(self.opened.elapsed(), self.had_frame, ffmpeg_available()) {
            return;
        }
        if let Some(f) = v.frame() {
            self.had_frame = true;
            self.held_frame = Some(f);
            return;
        }
        self.fall_back();
    }

    /// Makes the system player's time for its first picture run out now,
    /// so tests needn't find a file it never draws.
    #[doc(hidden)]
    pub fn expire_first_frame_time(&mut self) {
        self.opened = Instant::now().checked_sub(NO_FRAME_LIMIT).unwrap_or(self.opened);
    }

    /// Replaces the failed system player with FFmpeg's, carrying over the
    /// speed, volume, mute, looping, place and whether it was playing. A
    /// preview opens it on a worker and goes on until [`Video::tick`] finds
    /// it ready.
    fn fall_back(&mut self) {
        if let Some(side) = self.preview_side {
            let (tx, rx) = mpsc::channel();
            let path = self.path.clone();
            std::thread::spawn(move || {
                let _ = tx.send(falcon_ffmpeg::Player::open_preview(&path, true, side));
            });
            self.opening = Some(rx);
            return;
        }
        match open_ffmpeg(&self.path, self.mixer.as_ref()) {
            Ok(p) => self.take_over(p),
            Err(e) => self.fallback_error = Some(e.0),
        }
    }

    /// Takes the player a worker opened for a preview, if it is ready.
    fn adopt_opened(&mut self) {
        let Some(rx) = &self.opening else { return };
        let opened = match rx.try_recv() {
            Ok(r) => r,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err(falcon_ffmpeg::Error("the FFmpeg worker stopped".into())),
        };
        self.opening = None;
        match opened {
            Ok((p, sound)) => {
                match (&self.mixer, sound) {
                    (Some(m), Some(s)) => m.add(Sound(s)),
                    (None, s) => self.held_sound = s,
                    (Some(_), None) => {}
                }
                self.take_over(p);
            }
            Err(e) => self.fallback_error = Some(e.0),
        }
    }

    /// Puts FFmpeg's player in the system player's place, as it was.
    fn take_over(&mut self, mut p: falcon_ffmpeg::Player) {
        let Inner::System(old) = &self.inner else { return };
        let (rate, looping, playing, at) = (old.rate(), old.looping(), old.is_playing(), old.current_time());
        p.set_volume(self.volume);
        p.set_muted(self.muted || self.held_sound.is_some());
        p.set_rate(rate);
        p.set_looping(looping);
        if at > 0.0 {
            p.seek(at);
        }
        if playing {
            p.play();
        }
        self.inner = Inner::Ffmpeg(p);
    }

    /// The picture at the playhead at the clip's full size, turned upright:
    /// what Eagle's frame tools (set as thumbnail, copy, save) capture.
    /// Decoded afresh, so it does not disturb playback or the frames
    /// [`Video::frame`] hands out.
    pub fn current_frame(&self) -> Result<Frame> {
        match &self.inner {
            Inner::System(v) => v.current_frame(),
            Inner::Ffmpeg(p) => p.current_frame().map_err(Error::from),
        }
    }

    /// The frame due now, if it differs from the last one handed out.
    pub fn frame(&mut self) -> Option<Frame> {
        if let Some(f) = self.held_frame.take() {
            return Some(f);
        }
        match &mut self.inner {
            Inner::System(v) => {
                let f = v.frame();
                self.had_frame |= f.is_some();
                f
            }
            Inner::Ffmpeg(p) => p.frame(),
        }
    }
}

/// Opens FFmpeg's player, its sound added to `mixer` when there is one.
fn open_ffmpeg(path: &Path, mixer: Option<&rodio::mixer::Mixer>) -> Result<falcon_ffmpeg::Player> {
    let (player, sound) = falcon_ffmpeg::Player::open(path, mixer.is_some())?;
    if let (Some(m), Some(s)) = (mixer, sound) {
        m.add(Sound(s));
    }
    Ok(player)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_step_and_stop_at_the_ends() {
        assert_eq!(step_rate(1.0, true), 1.25);
        assert_eq!(step_rate(1.0, false), 0.75);
        assert_eq!(step_rate(8.0, true), 8.0);
        assert_eq!(step_rate(0.25, false), 0.25);
        assert_eq!(step_rate(2.0, true), 3.0);
        assert_eq!(step_rate(1.1, true), 1.25);
        assert_eq!(step_rate(1.1, false), 1.0);
    }

    #[test]
    fn a_player_with_no_picture_after_the_limit_switches() {
        let (before, at, after) = (Duration::from_millis(1499), NO_FRAME_LIMIT, Duration::from_secs(60));
        assert!(!switch_for_no_frame(Duration::ZERO, false, true));
        assert!(!switch_for_no_frame(before, false, true), "still within its time");
        assert!(switch_for_no_frame(at, false, true));
        assert!(switch_for_no_frame(after, false, true));
    }

    #[test]
    fn a_player_that_drew_a_picture_stays() {
        // A paused player's poster picture counts, however long it then sits.
        assert!(!switch_for_no_frame(NO_FRAME_LIMIT, true, true));
        assert!(!switch_for_no_frame(Duration::from_secs(3600), true, true));
    }

    #[test]
    fn without_ffmpeg_there_is_nothing_to_switch_to() {
        assert!(!switch_for_no_frame(NO_FRAME_LIMIT, false, false));
        assert!(!switch_for_no_frame(Duration::from_secs(60), false, false));
    }

    #[test]
    fn missing_files_are_refused_by_both() {
        let p = Path::new("/nonexistent/falcon-video-test.mkv");
        assert!(probe(p).is_err());
        assert!(frame_at(p, 0.0).is_err());
        assert!(Grabber::open(p, 64).is_err());
    }
}
