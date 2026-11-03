//! The grid's hover previews for video and audio cards, after Eagle's: a
//! hovered video card plays the file, muted unless the sound was switched
//! on, and the pointer scrubs it; a hovered audio card plays the track with
//! its progress drawn over the card. The drawing is `ui/grid/hover_media.rs`;
//! this file holds the state, the rules and the worker.
//!
//! Eagle plays the file itself in a `<video>` element, and so does falcon,
//! through `falcon_video::Video` with its output scaled down to the card
//! (at most [`FRAME_MAX_SIDE`] pixels). That player decodes on its own
//! threads (AVFoundation's, or FFmpeg's workers for what macOS can't play),
//! has the file's own sound, seeks exactly and keeps its own clock, where a
//! worker stepping `falcon_video::Grabber` would pay a seek for every
//! picture and need a second decoder for the sound. It has to live on the
//! main thread, so a worker reads the file first ([`falcon_video::Prepared`])
//! and the main thread only makes the player, takes each new small picture
//! and copies it into one texture. Only the hovered card has a player,
//! made after [`VIDEO_DELAY`]; it is dropped when the pointer leaves, which
//! ends its decoding, and a card that is left while its worker still reads
//! throws the result away. Memory stays at one picture and the player's
//! short queue.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

use eframe::egui::{self, TextureHandle};
use falcon_video::{Prepared, Video};

/// How long a video card is hovered before its preview starts, and an
/// audio card before it plays (Eagle's 200 ms). Eagle's video comment says
/// 500 ms but the timer it sets is 250 ms, which is what runs.
pub const VIDEO_DELAY: f64 = 0.25;
pub const AUDIO_DELAY: f64 = 0.2;
/// After the pointer stops moving, the preview plays on from where it was
/// scrubbed to (Eagle's 200 ms).
pub const SCRUB_REST: f64 = 0.2;
/// How long the time stays up after the pointer moves (Eagle's 600 ms).
pub const LABEL_SECS: f64 = 0.6;
/// The largest side of a video card's pictures, and the smallest it asks for.
pub const FRAME_MAX_SIDE: u32 = 640;
pub const FRAME_MIN_SIDE: u32 = 64;
/// How long a scrub waits for the picture of its last seek before seeking
/// again: on a clip with far-apart keyframes, a seek for every pointer move
/// would throw each away before it showed anything.
pub const SEEK_GAP: f64 = 0.12;

/// The side of a card's pictures, from the side of the card in pixels.
pub fn frame_side(card_pixels: f32) -> u32 {
    (card_pixels.ceil() as u32).clamp(FRAME_MIN_SIDE, FRAME_MAX_SIDE)
}

/// How long to wait for a preview's next picture, in seconds: a little
/// under one frame of a clip of `fps`, so frames aren't missed, at the
/// screen's rate at most (and at least a few times a second, or for a clip
/// that states no rate).
pub fn repaint_after(fps: f32) -> f64 {
    let frame = if fps > 0.0 { 1.0 / f64::from(fps) } else { 1.0 / 30.0 };
    (frame * 0.9).clamp(1.0 / 60.0, 1.0 / 15.0)
}

/// The next seek of a scrub. The latest position wins; it goes out once the
/// last seek has shown its picture or after [`SEEK_GAP`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SeekGate {
    wanted: Option<f64>,
    sent_at: Option<f64>,
    arrived: bool,
}

impl SeekGate {
    /// The pointer asks for time `t`.
    pub fn want(&mut self, t: f64) {
        self.wanted = Some(t);
    }

    /// A picture has come from the player.
    pub fn arrived(&mut self) {
        self.arrived = true;
    }

    /// The seek to make now, if any.
    pub fn take(&mut self, now: f64) -> Option<f64> {
        let open = self.arrived || self.sent_at.is_none_or(|at| now - at >= SEEK_GAP);
        if !open {
            return None;
        }
        let t = self.wanted.take()?;
        self.sent_at = Some(now);
        self.arrived = false;
        Some(t)
    }
}

/// The hover player's sound, reachable from the notification that the
/// window was hidden (which comes while the app isn't drawing frames), and
/// that hiding happened, for the next frame to end the previews.
static SOUND: std::sync::Mutex<Option<Arc<rodio::Player>>> = std::sync::Mutex::new(None);
static HIDDEN: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// The video preview playing, for the same notification, which comes on
    /// the main thread as the player has to be used on.
    static PLAYING: RefCell<Weak<RefCell<Video>>> = const { RefCell::new(Weak::new()) };
}

/// The window was hidden or minimized: silences the hover sound at once
/// and has the next frame end the previews.
pub fn window_hidden() {
    if let Some(p) = SOUND.lock().ok().and_then(|g| g.clone()) {
        p.pause();
    }
    if let Some(v) = PLAYING.with(|p| p.borrow().upgrade())
        && let Ok(mut v) = v.try_borrow_mut()
    {
        v.pause();
    }
    HIDDEN.store(true, Ordering::Relaxed);
}

/// A video card's player: read on a worker thread, then played here.
/// Dropping it stops the playing; a worker still reading finds nobody to
/// hand the file to and ends.
pub struct Preview {
    rx: mpsc::Receiver<falcon_video::Result<Prepared>>,
    video: Option<Rc<RefCell<Video>>>,
    /// The one texture the pictures go into.
    frame: Option<TextureHandle>,
    pub failed: bool,
    seeks: SeekGate,
    /// What the player was last told: playing, muted and at which volume.
    running: bool,
    sound: Option<(bool, f32)>,
    /// An output was asked for already (with none, it isn't asked for again).
    mixer_tried: bool,
}

impl Preview {
    /// Starts reading `path` for pictures at most `side` pixels across.
    pub fn start(ctx: &egui::Context, path: PathBuf, side: u32) -> Preview {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let _ = std::thread::Builder::new().name("falcon-hover-video".into()).spawn(move || {
            let _ = tx.send(Prepared::new(&path, side));
            ctx.request_repaint();
        });
        Preview::waiting(rx)
    }

    fn waiting(rx: mpsc::Receiver<falcon_video::Result<Prepared>>) -> Preview {
        Preview { rx, video: None, frame: None, failed: false, seeks: SeekGate::default(), running: false, sound: None, mixer_tried: false }
    }

    /// Makes the player once the worker is done, runs it (looping at the
    /// end, or a change of decoder), and takes a new picture into the
    /// texture, grayed when `gray`.
    pub fn poll(&mut self, ctx: &egui::Context, gray: bool) {
        if self.failed {
            return;
        }
        if self.video.is_none() {
            match self.rx.try_recv() {
                Ok(prepared) => self.begin(prepared),
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => self.failed = true,
            }
        }
        let Some(video) = &self.video else { return };
        let mut v = video.borrow_mut();
        v.tick();
        if v.error().is_some() {
            self.failed = true;
            return;
        }
        let Some(f) = v.frame() else { return };
        drop(v);
        self.seeks.arrived();
        let img = crate::ui::viewer::video::color_image(&f, gray);
        match &mut self.frame {
            Some(tex) if tex.size() == [f.width, f.height] => tex.set_partial([0, 0], img, egui::TextureOptions::LINEAR),
            _ => self.frame = Some(ctx.load_texture("hover-video-frame", img, egui::TextureOptions::LINEAR)),
        }
    }

    fn begin(&mut self, prepared: falcon_video::Result<Prepared>) {
        let Ok(mut video) = prepared.and_then(Prepared::start) else {
            self.failed = true;
            return;
        };
        if video.info().duration <= 0.0 {
            self.failed = true;
            return;
        }
        video.set_looping(true);
        // Quiet until `drive` applies the remembered choice.
        video.set_muted(true);
        let video = Rc::new(RefCell::new(video));
        PLAYING.with(|p| *p.borrow_mut() = Rc::downgrade(&video));
        self.video = Some(video);
    }

    /// The clip's length, its stated frame rate and the player's time, once
    /// the player exists.
    pub fn clock(&self) -> Option<(f64, f32, f64)> {
        let v = self.video.as_ref()?.borrow();
        Some((v.info().duration, v.info().fps, v.current_time()))
    }

    /// Whether unmuting this preview needs the output yet (FFmpeg plays
    /// through falcon's output, the system player through its own), and
    /// notes that it was asked for.
    pub fn take_mixer_wish(&mut self) -> bool {
        let wanted = !self.mixer_tried && falcon_video::ffmpeg_available() && self.video.as_ref().is_some_and(|v| v.borrow().needs_mixer());
        self.mixer_tried |= wanted;
        wanted
    }

    pub fn set_mixer(&mut self, mixer: rodio::mixer::Mixer) {
        if let Some(v) = &self.video {
            v.borrow_mut().set_mixer(mixer);
        }
    }

    /// Tells the player what this frame wants: where the scrub is (`seek`),
    /// whether it plays, and its sound. Only changes reach it.
    pub fn drive(&mut self, now: f64, seek: Option<f64>, playing: bool, muted: bool, volume: f32) {
        let Some(video) = &self.video else { return };
        let mut v = video.borrow_mut();
        if self.sound != Some((muted, volume)) {
            self.sound = Some((muted, volume));
            v.set_volume(volume);
            v.set_muted(muted);
        }
        if let Some(t) = seek {
            self.seeks.want(t);
        }
        if !playing && self.running {
            self.running = false;
            v.pause();
        }
        if let Some(t) = self.seeks.take(if playing { f64::INFINITY } else { now }) {
            v.seek(t);
        }
        if playing && !self.running {
            self.running = true;
            v.play();
        }
    }

    /// The latest picture, once one has come.
    pub fn frame(&self) -> Option<&TextureHandle> {
        self.frame.as_ref()
    }
}

/// The hovered video card's preview.
pub struct VideoHover {
    pub id: String,
    /// When the hover began (egui time).
    pub since: f64,
    pub preview: Option<Preview>,
    /// The time shown, in seconds.
    pub t: f64,
    /// The pointer's x last frame, and when it last moved.
    pub last_x: Option<f32>,
    pub moved_at: f64,
    /// Until when the time label shows.
    pub label_until: f64,
    /// The frame it was last drawn in.
    pub seen: u64,
}

impl VideoHover {
    /// Whether the hover has lasted long enough for the preview to start.
    pub fn warm(&self, now: f64) -> bool {
        now - self.since >= VIDEO_DELAY
    }

    /// One frame of the preview's clock: the pointer's x (`None` off the
    /// picture) across a picture `width` wide from `left`, over a clip of
    /// `duration` whose player is at `clock`. The first position seen only
    /// arms scrubbing; a move after it sets the time from the pointer
    /// (and shows the label); once the pointer has rested for
    /// [`SCRUB_REST`] the player plays and its time is shown. Returns
    /// whether the preview is playing (false while scrubbing), and the time
    /// to seek to when the pointer moved.
    pub fn step(&mut self, now: f64, x: Option<f32>, left: f32, width: f32, duration: f64, clock: f64) -> (bool, Option<f64>) {
        let mut seek = None;
        match (x, self.last_x) {
            (Some(p), Some(last)) if p != last => {
                self.t = crate::logic::seek_time(p, left, width, duration);
                self.moved_at = now;
                self.label_until = now + LABEL_SECS;
                seek = Some(self.t);
            }
            _ if now - self.moved_at >= SCRUB_REST => self.t = clock,
            _ => {}
        }
        self.last_x = x.or(self.last_x);
        (now - self.moved_at >= SCRUB_REST, seek)
    }
}
/// The hovered audio card's playback.
pub struct AudioHover {
    pub id: String,
    pub since: f64,
    pub started: bool,
    pub seen: u64,
}

/// Both previews and the audio player they use (a player of its own, so
/// the viewer's is never disturbed).
#[derive(Default)]
pub struct HoverMedia {
    pub video: Option<VideoHover>,
    pub audio: Option<AudioHover>,
    pub player: crate::audio::Audio,
    /// The card whose sound or play button the pointer was on in the last
    /// frame, and that frame: the button covers the card, which then isn't
    /// hovered, but its preview must go on.
    toggle_over: Option<(String, u64)>,
}

impl HoverMedia {
    /// The hovered video card's state, started afresh for a new card.
    pub fn video_for(&mut self, id: &str, now: f64, frame: u64) -> &mut VideoHover {
        if self.video.as_ref().is_none_or(|v| v.id != id) {
            self.video = Some(VideoHover { id: id.to_string(), since: now, preview: None, t: 0.0, last_x: None, moved_at: now, label_until: 0.0, seen: frame });
        }
        let v = self.video.as_mut().expect("just set");
        v.seen = frame;
        v
    }

    /// The hovered audio card's state, started afresh for a new card (the
    /// old one's track stops).
    pub fn audio_for(&mut self, id: &str, now: f64, frame: u64) -> &mut AudioHover {
        if self.audio.as_ref().is_none_or(|a| a.id != id) {
            self.player.stop();
            self.audio = Some(AudioHover { id: id.to_string(), since: now, started: false, seen: frame });
        }
        let a = self.audio.as_mut().expect("just set");
        a.seen = frame;
        a
    }

    /// At the start of `frame`: ends the previews of cards not hovered in
    /// the frame before (the pointer left, or the grid is gone). Their
    /// player is dropped and their track stops.
    pub fn begin(&mut self, frame: u64) {
        if HIDDEN.swap(false, Ordering::Relaxed) {
            self.stop_all();
        }
        if let Ok(mut g) = SOUND.lock() {
            *g = self.player.handle();
        }
        let stale = |seen: u64| seen + 1 < frame;
        if self.video.as_ref().is_some_and(|v| stale(v.seen)) {
            self.video = None;
        }
        if self.audio.as_ref().is_some_and(|a| stale(a.seen)) {
            self.audio = None;
            self.player.stop();
        }
    }
}

impl HoverMedia {
    /// After the grid is drawn in `frame`: ends the previews of cards not
    /// hovered in it, at once (the pointer left, the card scrolled away or
    /// a drag began), so a sound never plays on into the next frame.
    pub fn end_frame(&mut self, frame: u64) {
        if self.video.as_ref().is_some_and(|v| v.seen != frame) {
            self.video = None;
        }
        if self.audio.as_ref().is_some_and(|a| a.seen != frame) {
            self.audio = None;
            self.player.stop();
        }
    }

    /// Ends every preview and its sound (the window was hidden).
    pub fn stop_all(&mut self) {
        self.video = None;
        self.audio = None;
        self.player.stop();
        self.toggle_over = None;
    }

    /// Notes that the pointer is on card `id`'s button in `frame`.
    pub fn set_toggle_over(&mut self, id: &str, over: bool, frame: u64) {
        if over {
            self.toggle_over = Some((id.to_string(), frame));
        } else if self.toggle_over.as_ref().is_some_and(|(i, _)| i == id) {
            self.toggle_over = None;
        }
    }

    /// Whether the pointer was on card `id`'s button in the frame before `frame`.
    pub fn over_toggle(&self, id: &str, frame: u64) -> bool {
        self.toggle_over.as_ref().is_some_and(|(i, f)| i == id && f + 1 >= frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_are_sized_to_the_card() {
        assert_eq!(frame_side(200.0), 200);
        assert_eq!(frame_side(200.2), 201);
        assert_eq!(frame_side(30.0), FRAME_MIN_SIDE, "a List row's thumbnail");
        assert_eq!(frame_side(4000.0), FRAME_MAX_SIDE);
    }

    #[test]
    fn repaints_follow_the_clip() {
        assert!((repaint_after(24.0) - 0.9 / 24.0).abs() < 1e-9, "a little under a frame");
        assert_eq!(repaint_after(120.0), 1.0 / 60.0, "the screen's rate at most");
        assert_eq!(repaint_after(1.0), 1.0 / 15.0, "the bar and label still move");
        assert!((repaint_after(0.0) - 0.9 / 30.0).abs() < 1e-9, "no stated rate");
    }

    #[test]
    fn a_scrub_waits_for_its_picture_and_the_latest_position_wins() {
        let mut g = SeekGate::default();
        assert_eq!(g.take(0.0), None, "nothing asked for");
        g.want(1.0);
        assert_eq!(g.take(0.0), Some(1.0), "the first goes at once");
        assert_eq!(g.take(0.01), None, "only once");
        g.want(2.0);
        g.want(3.0);
        assert_eq!(g.take(0.05), None, "the picture of 1.0 hasn't come");
        g.arrived();
        assert_eq!(g.take(0.06), Some(3.0), "the picture came: the latest goes");
        g.want(4.0);
        assert_eq!(g.take(0.06 + SEEK_GAP - 0.01), None);
        assert_eq!(g.take(0.06 + SEEK_GAP), Some(4.0), "no picture in time: it goes anyway");
    }

    fn hover(now: f64) -> VideoHover {
        VideoHover { id: "a".into(), since: now, preview: None, t: 0.0, last_x: None, moved_at: now, label_until: 0.0, seen: 0 }
    }

    #[test]
    fn a_video_card_starts_after_the_delay() {
        let v = hover(10.0);
        assert!(!v.warm(10.0) && !v.warm(10.24) && v.warm(10.25) && v.warm(12.0));
        assert_eq!(AUDIO_DELAY, 0.2);
    }

    #[test]
    fn the_pointer_scrubs_and_the_preview_plays_on_when_it_rests() {
        let mut v = hover(0.0);
        // Resting from the start: plays with the player, the first position only arms.
        assert_eq!(v.step(0.3, Some(50.0), 0.0, 200.0, 20.0, 0.1), (true, None));
        assert_eq!((v.last_x, v.label_until, v.t), (Some(50.0), 0.0, 0.1));
        // A move maps the pointer across the picture onto the clip and seeks.
        assert_eq!(v.step(0.4, Some(100.0), 0.0, 200.0, 20.0, 0.2), (false, Some(10.0)), "scrubbing is paused");
        assert_eq!(v.t, 10.0);
        assert!((v.label_until - 1.0).abs() < 1e-9);
        assert_eq!(v.step(0.5, Some(100.0), 0.0, 200.0, 20.0, 9.0), (false, None), "still inside the rest time");
        assert_eq!(v.t, 10.0, "the player's old time isn't shown");
        // Rested for 0.2 s: playing on, the time is the player's.
        assert_eq!(v.step(0.61, Some(100.0), 0.0, 200.0, 20.0, 10.5), (true, None));
        assert_eq!(v.t, 10.5);
        // Off the picture the last position is kept; the left edge is time 0, the right the end.
        assert_eq!(v.step(0.7, Some(0.0), 0.0, 200.0, 20.0, 10.6), (false, Some(0.0)));
        assert_eq!(v.step(0.8, Some(500.0), 0.0, 200.0, 20.0, 0.0), (false, Some(20.0)));
        assert_eq!(v.step(1.5, None, 0.0, 200.0, 20.0, 0.5), (true, None));
        assert_eq!(v.t, 0.5, "past the end the player loops");
        assert_eq!(v.last_x, Some(500.0));
    }

    /// A preview whose worker is still reading, and the worker's end of it.
    fn reading() -> (Preview, mpsc::Sender<falcon_video::Result<Prepared>>) {
        let (tx, rx) = mpsc::channel();
        (Preview::waiting(rx), tx)
    }

    #[test]
    fn each_card_has_its_own_player_and_the_last_one_goes_at_once() {
        let mut h = HoverMedia::default();
        let (p, a) = reading();
        h.video_for("a", 0.0, 1).preview = Some(p);
        let same = h.video_for("a", 0.1, 2);
        assert!(same.preview.is_some(), "the same card keeps its player");
        assert_eq!(h.video_for("b", 0.2, 3).id, "b");
        assert!(h.video.as_ref().is_some_and(|v| v.preview.is_none()), "the new card waits for its delay");
        assert!(a.send(Err(falcon_video::Error("late".into()))).is_err(), "the old card's worker has nobody to hand to");
    }

    #[test]
    fn leaving_a_card_drops_its_player() {
        let mut h = HoverMedia::default();
        let (p, a) = reading();
        h.video_for("a", 0.0, 1).preview = Some(p);
        h.end_frame(1);
        assert!(a.send(Err(falcon_video::Error("still".into()))).is_ok(), "drawn in this frame");
        h.end_frame(2);
        assert!(h.video.is_none());
        assert!(a.send(Err(falcon_video::Error("gone".into()))).is_err(), "no waiting for another frame");
        let (p, b) = reading();
        h.video_for("b", 0.0, 3).preview = Some(p);
        h.stop_all();
        assert!(b.send(Err(falcon_video::Error("hidden".into()))).is_err(), "hiding the window ends it too");
    }

    #[test]
    fn a_worker_that_fails_or_vanishes_leaves_the_thumbnail() {
        let ctx = egui::Context::default();
        let (mut p, tx) = reading();
        p.poll(&ctx, false);
        assert!(!p.failed && p.frame().is_none() && p.clock().is_none(), "still reading");
        tx.send(Err(falcon_video::Error("not a video".into()))).unwrap();
        p.poll(&ctx, false);
        assert!(p.failed && p.frame().is_none());
        let (mut p, tx) = reading();
        drop(tx);
        p.poll(&ctx, false);
        assert!(p.failed);
        // Nothing is told to a player that isn't there.
        p.drive(1.0, Some(2.0), true, false, 1.0);
        assert!(!p.take_mixer_wish());
    }

    #[test]
    fn a_card_left_in_a_frame_ends_in_that_frame() {
        let mut h = HoverMedia::default();
        h.video_for("a", 0.0, 5);
        h.audio_for("x", 0.0, 5);
        h.end_frame(5);
        assert!(h.video.is_some() && h.audio.is_some(), "both were drawn in frame 5");
        h.video_for("a", 0.0, 6);
        h.end_frame(6);
        assert!(h.video.is_some() && h.audio.is_none(), "the audio card was not drawn in frame 6");
        h.end_frame(7);
        assert!(h.video.is_none(), "the pointer left: no waiting for another frame");
        assert!(h.player.loaded().is_none());
    }

    #[test]
    fn stopping_all_clears_both_previews_and_the_button_memory() {
        let mut h = HoverMedia::default();
        h.video_for("a", 0.0, 1);
        h.audio_for("x", 0.0, 1);
        h.set_toggle_over("a", true, 1);
        assert!(h.over_toggle("a", 2) && !h.over_toggle("a", 3) && !h.over_toggle("b", 2));
        h.stop_all();
        assert!(h.video.is_none() && h.audio.is_none() && h.player.loaded().is_none() && !h.over_toggle("a", 2));
    }

    #[test]
    fn previews_end_when_their_card_is_not_hovered() {
        let mut h = HoverMedia::default();
        h.video_for("a", 0.0, 1);
        h.begin(2);
        assert!(h.video.is_some());
        let since = h.video_for("a", 5.0, 2).since;
        assert_eq!(since, 0.0, "the same card keeps its hover start");
        h.begin(4);
        assert!(h.video.is_none(), "not hovered in frame 3");
        assert_eq!(h.video_for("b", 7.0, 4).since, 7.0);
        h.audio_for("x", 0.0, 4);
        h.begin(6);
        assert!(h.audio.is_none());
    }
}
