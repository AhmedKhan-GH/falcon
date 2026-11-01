//! The grid's hover previews for video and audio cards, after Eagle's: a
//! hovered video card plays a silent preview that the pointer scrubs, and
//! a hovered audio card plays the track with its progress drawn over the
//! card. The drawing is `ui/grid/hover_media.rs`; this file holds the
//! state, the rules and the worker.
//!
//! Eagle plays the file itself in a `<video>` element. Falcon decodes a
//! strip of small pictures of the clip instead, on a worker thread with a
//! `falcon_video::Grabber` (AVFoundation's player lives on the main thread
//! and hands out full-size frames, too heavy for a card; formats macOS
//! can't play are grabbed through the FFmpeg fallback), and steps
//! through the strip in real time. Only the hovered card has a strip, and
//! it is dropped when the pointer leaves, so memory stays at one strip of
//! at most [`STRIP_FRAMES`] pictures of [`STRIP_MAX_SIDE`] pixels.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

use eframe::egui::{self, TextureHandle};

/// How long a video card is hovered before its preview starts (Eagle's
/// 500 ms), and an audio card before it plays (Eagle's 200 ms).
pub const VIDEO_DELAY: f64 = 0.5;
pub const AUDIO_DELAY: f64 = 0.2;
/// After the pointer stops moving, the preview plays on from where it was
/// scrubbed to (Eagle's 200 ms).
pub const SCRUB_REST: f64 = 0.2;
/// How long the time stays up after the pointer moves (Eagle's 600 ms).
pub const LABEL_SECS: f64 = 0.6;
/// Pictures in a video card's strip, and their largest side in pixels.
pub const STRIP_FRAMES: usize = 48;
pub const STRIP_MAX_SIDE: u32 = 640;
/// How far (seconds) the sound may drift from the picture before it is
/// moved back to it.
pub const AUDIO_DRIFT: f64 = 0.3;

/// The order the strip's pictures are decoded in: by bit-reversed index,
/// so the first few spread over the whole clip and the gaps fill in after
/// (0, 24, 12, 36, 6, …), and scrubbing has something near every point
/// early on.
pub fn strip_order(n: usize) -> Vec<usize> {
    if n == 0 {
        return Vec::new();
    }
    let bits = n.next_power_of_two().trailing_zeros();
    let rev = |i: usize| if bits == 0 { 0 } else { i.reverse_bits() >> (usize::BITS - bits) };
    (0..n.next_power_of_two()).map(rev).filter(|&i| i < n).collect()
}

/// The time picture `i` of `n` shows: the middle of its share of the clip.
pub fn strip_time(i: usize, n: usize, duration: f64) -> f64 {
    (i as f64 + 0.5) / n.max(1) as f64 * duration.max(0.0)
}

/// The picture of `n` that shows time `t`.
pub fn strip_index(t: f64, n: usize, duration: f64) -> usize {
    if duration <= 0.0 || n == 0 {
        return 0;
    }
    ((t / duration * n as f64) as usize).min(n - 1)
}

/// The decoded picture nearest to `i` (earlier ones first on a tie).
pub fn nearest_loaded(loaded: &[bool], i: usize) -> Option<usize> {
    (0..loaded.len()).find_map(|d| [i.checked_sub(d), Some(i + d)].into_iter().flatten().find(|&j| loaded.get(j) == Some(&true)))
}

/// Playing on: `t` moved on by `dt`, from the top again past the end
/// (Eagle's preview loops).
pub fn advance(t: f64, dt: f64, duration: f64) -> f64 {
    if duration <= 0.0 {
        return 0.0;
    }
    (t + dt.max(0.0)).rem_euclid(duration)
}

/// How far apart the sound at `audio` and the picture at `t` are, both
/// looping over `duration` (just after a wrap they are close, not a whole
/// clip apart).
pub fn audio_drift(audio: f64, t: f64, duration: f64) -> f64 {
    let d = (audio - t).abs();
    if duration > 0.0 { d.min(duration - d.min(duration)) } else { d }
}

/// Whether the sound has to be moved to the picture's time.
pub fn needs_reseek(audio: f64, t: f64, duration: f64) -> bool {
    audio_drift(audio, t, duration) > AUDIO_DRIFT
}

/// The hover player's sound, reachable from the notification that the
/// window was hidden (which comes while the app isn't drawing frames), and
/// that hiding happened, for the next frame to end the previews.
static SOUND: std::sync::Mutex<Option<Arc<rodio::Player>>> = std::sync::Mutex::new(None);
static HIDDEN: AtomicBool = AtomicBool::new(false);

/// The window was hidden or minimized: silences the hover sound at once
/// and has the next frame end the previews.
pub fn window_hidden() {
    if let Some(p) = SOUND.lock().ok().and_then(|g| g.clone()) {
        p.pause();
    }
    HIDDEN.store(true, Ordering::Relaxed);
}

enum Msg {
    Duration(f64),
    Frame(usize, falcon_video::Frame),
    Failed,
}

/// A video card's strip of pictures, decoded on a worker thread. Dropping
/// it stops the worker after the picture it is on.
pub struct Strip {
    rx: mpsc::Receiver<Msg>,
    stop: Arc<AtomicBool>,
    pub duration: Option<f64>,
    pub failed: bool,
    frames: Vec<Option<TextureHandle>>,
    /// Whether the pictures were made for the grayscale preview.
    gray: bool,
}

impl Strip {
    /// Starts decoding `path` into pictures at most `side` pixels across.
    pub fn start(ctx: &egui::Context, path: PathBuf, side: u32, gray: bool) -> Strip {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let (flag, ctx) = (stop.clone(), ctx.clone());
        let _ = std::thread::Builder::new().name("falcon-hover-video".into()).spawn(move || {
            let send = |m: Msg| {
                let ok = tx.send(m).is_ok();
                ctx.request_repaint();
                ok
            };
            let (Ok(info), Ok(grabber)) = (falcon_video::probe(&path), falcon_video::Grabber::open(&path, side)) else {
                send(Msg::Failed);
                return;
            };
            if info.duration <= 0.0 || !send(Msg::Duration(info.duration)) {
                send(Msg::Failed);
                return;
            }
            for i in strip_order(STRIP_FRAMES) {
                if flag.load(Ordering::Relaxed) {
                    return;
                }
                if let Ok(f) = grabber.grab(strip_time(i, STRIP_FRAMES, info.duration))
                    && !send(Msg::Frame(i, f))
                {
                    return;
                }
            }
        });
        Strip { rx, stop, duration: None, failed: false, frames: vec![None; STRIP_FRAMES], gray }
    }

    /// Takes in what the worker sent, uploading a few pictures a frame.
    pub fn poll(&mut self, ctx: &egui::Context) {
        for _ in 0..6 {
            match self.rx.try_recv() {
                Ok(Msg::Duration(d)) => self.duration = Some(d),
                Ok(Msg::Failed) => self.failed = true,
                Ok(Msg::Frame(i, f)) => {
                    let img = crate::ui::viewer::video::color_image(&f, self.gray);
                    self.frames[i] = Some(ctx.load_texture(format!("hover-video-{i}"), img, egui::TextureOptions::LINEAR));
                }
                Err(_) => return,
            }
        }
    }

    /// The decoded picture nearest the one showing `t`.
    pub fn frame_at(&self, t: f64) -> Option<&TextureHandle> {
        let d = self.duration?;
        let loaded: Vec<bool> = self.frames.iter().map(Option::is_some).collect();
        nearest_loaded(&loaded, strip_index(t, self.frames.len(), d)).and_then(|j| self.frames[j].as_ref())
    }

    pub fn gray(&self) -> bool {
        self.gray
    }
}

impl Drop for Strip {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The hovered video card's preview.
pub struct VideoHover {
    pub id: String,
    /// When the hover began (egui time).
    pub since: f64,
    pub strip: Option<Strip>,
    /// The time shown, in seconds.
    pub t: f64,
    /// The pointer's x last frame, and when it last moved.
    pub last_x: Option<f32>,
    pub moved_at: f64,
    /// Until when the time label shows.
    pub label_until: f64,
    /// The frame it was last drawn in.
    pub seen: u64,
    /// Whether the sound was tried (a file that won't load stays silent
    /// instead of being retried every frame).
    pub sound_tried: bool,
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
            self.stop_video_sound();
            self.video = Some(VideoHover { id: id.to_string(), since: now, strip: None, t: 0.0, last_x: None, moved_at: now, label_until: 0.0, seen: frame, sound_tried: false });
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
    /// strip is dropped and their track stops.
    pub fn begin(&mut self, frame: u64) {
        if HIDDEN.swap(false, Ordering::Relaxed) {
            self.stop_all();
        }
        if let Ok(mut g) = SOUND.lock() {
            *g = self.player.handle();
        }
        let stale = |seen: u64| seen + 1 < frame;
        if self.video.as_ref().is_some_and(|v| stale(v.seen)) {
            self.stop_video_sound();
            self.video = None;
        }
        if self.audio.as_ref().is_some_and(|a| stale(a.seen)) {
            self.audio = None;
            self.player.stop();
        }
    }
}

impl HoverMedia {
    /// Stops the sound of the video preview, if it is what the player holds.
    fn stop_video_sound(&mut self) {
        if let Some(v) = &self.video
            && self.player.loaded() == Some(v.id.as_str())
        {
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

    /// Keeps the sound of the video preview of `id` in step with the
    /// picture at `t`: silent while `muted`, paused while the pointer
    /// scrubs (`playing` false), moved back to the picture when it drifts.
    /// A file the player can't decode just stays silent.
    #[allow(clippy::too_many_arguments)]
    pub fn video_sound(&mut self, id: &str, path: &std::path::Path, ext: &str, muted: bool, playing: bool, t: f64, duration: f64, volume: f32) {
        if muted {
            if self.player.loaded() == Some(id) {
                self.player.stop();
            }
            return;
        }
        if self.player.loaded() != Some(id) {
            let Some(v) = self.video.as_mut().filter(|v| v.id == id && !v.sound_tried) else { return };
            v.sound_tried = true;
            self.player.looping = true;
            self.player.volume = volume;
            self.player.load(id, path, ext, false);
            if self.player.loaded() != Some(id) {
                return;
            }
            self.player.seek(t);
        }
        self.player.set_volume(volume);
        self.player.tick();
        if playing {
            if needs_reseek(self.player.position(), t, duration) {
                self.player.seek(t);
            }
            if !self.player.is_playing() {
                self.player.toggle();
            }
        } else if self.player.is_playing() {
            self.player.toggle();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_order_covers_every_picture_once_spread_out() {
        let order = strip_order(48);
        assert_eq!(order.len(), 48);
        let mut sorted = order.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..48).collect::<Vec<_>>());
        assert_eq!(&order[..4], &[0, 32, 16, 8], "the first pictures spread over the clip");
        assert_eq!(strip_order(1), vec![0]);
        assert!(strip_order(0).is_empty());
        assert_eq!(strip_order(5).len(), 5);
    }

    #[test]
    fn times_and_indices_agree() {
        for i in 0..10 {
            assert_eq!(strip_index(strip_time(i, 10, 20.0), 10, 20.0), i);
        }
        assert_eq!(strip_index(25.0, 10, 20.0), 9, "past the end stays on the last picture");
        assert_eq!(strip_index(3.0, 10, 0.0), 0);
    }

    #[test]
    fn the_nearest_decoded_picture_stands_in() {
        let loaded = [true, false, false, false, true];
        assert_eq!(nearest_loaded(&loaded, 1), Some(0));
        assert_eq!(nearest_loaded(&loaded, 3), Some(4));
        assert_eq!(nearest_loaded(&loaded, 2), Some(0), "earlier first on a tie");
        assert_eq!(nearest_loaded(&[false, false], 1), None);
    }

    #[test]
    fn playing_on_loops() {
        assert_eq!(advance(1.0, 0.5, 10.0), 1.5);
        assert!((advance(9.8, 0.5, 10.0) - 0.3).abs() < 1e-9);
        assert_eq!(advance(3.0, 1.0, 0.0), 0.0);
    }

    #[test]
    fn sound_follows_the_picture_within_a_third_of_a_second() {
        assert!(!needs_reseek(5.1, 5.0, 10.0));
        assert!(!needs_reseek(4.8, 5.0, 10.0));
        assert!(needs_reseek(6.0, 5.0, 10.0), "after a scrub");
        assert!(!needs_reseek(9.9, 0.1, 10.0), "across the loop point they are close");
        assert!(needs_reseek(9.0, 0.1, 10.0));
        assert!((audio_drift(9.9, 0.1, 10.0) - 0.2).abs() < 1e-9);
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
