//! The player's timing rules, free of FFmpeg so they can be tested: the
//! wall clock, the audio clock, the queue of decoded frames waiting to be
//! shown, the arithmetic of timestamps and seeks, and the bookkeeping of
//! sound stretched to another speed.

use std::collections::VecDeque;

use crate::ffi::{AV_NOPTS_VALUE, AVRational};

/// A media clock running off the wall clock: `pts` at wall time `since`,
/// advancing at `rate` while running and frozen while paused. Times are
/// seconds; the wall time is passed in, so the clock is plain arithmetic.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WallClock {
    pts: f64,
    since: Option<f64>,
    rate: f64,
}

impl WallClock {
    pub fn new(pts: f64) -> WallClock {
        WallClock { pts, since: None, rate: 1.0 }
    }

    pub fn now(&self, wall: f64) -> f64 {
        match self.since {
            Some(s) => self.pts + (wall - s).max(0.0) * self.rate,
            None => self.pts,
        }
    }

    pub fn running(&self) -> bool {
        self.since.is_some()
    }

    pub fn start(&mut self, wall: f64) {
        if self.since.is_none() {
            self.since = Some(wall);
        }
    }

    pub fn pause(&mut self, wall: f64) {
        self.pts = self.now(wall);
        self.since = None;
    }

    /// Jumps to `pts`, keeping running or paused.
    pub fn set(&mut self, pts: f64, wall: f64) {
        self.pts = pts;
        if self.since.is_some() {
            self.since = Some(wall);
        }
    }

    pub fn set_rate(&mut self, rate: f64, wall: f64) {
        self.pts = self.now(wall);
        if self.since.is_some() {
            self.since = Some(wall);
        }
        self.rate = rate;
    }
}

/// Where the sound is: the media time of the first sample of the chunk
/// the output last took, when it took it, and how much media time that
/// chunk covers (at speed `rate`, each second of samples covers `rate`
/// seconds of the clip, whose pitch stays put). Between takes the clock runs on from the chunk's start,
/// but never past its end, so a starved output stops the clock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AudioClock {
    pub pts: f64,
    pub taken_at: f64,
    pub span: f64,
    pub rate: f64,
}

impl AudioClock {
    pub fn now(&self, wall: f64) -> f64 {
        self.pts + ((wall - self.taken_at).max(0.0) * self.rate).min(self.span)
    }
}

/// Whether sound at `rate` needs stretching. At normal speed (to within
/// the player's own tolerance) it skips the filter altogether.
pub fn needs_tempo(rate: f64) -> bool {
    (rate - 1.0).abs() >= 1e-4
}

/// The slowest speed one `atempo` filter takes; it goes up to 100, past the
/// player's fastest.
const ATEMPO_MIN: f64 = 0.5;

/// The `atempo` speeds to chain for `rate` (clamped to the player's 0.1 to
/// 16): one filter, with halving filters ahead of it below 0.5, as FFmpeg's
/// documentation describes. Their product is `rate`.
pub fn tempo_chain(rate: f64) -> Vec<f64> {
    let mut left = rate.clamp(0.1, 16.0);
    let mut chain = Vec::new();
    while left < ATEMPO_MIN {
        chain.push(ATEMPO_MIN);
        left /= ATEMPO_MIN;
    }
    chain.push(left);
    chain
}

/// The media time of sound coming out of a tempo filter, which holds some
/// back: every sample out covers `rate` samples in, counted from the time
/// of the first sample in, so the timestamps on what goes in don't apply to
/// what comes out of the same call.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TempoTimeline {
    rate: f64,
    sample_rate: f64,
    start: Option<f64>,
    frames_out: u64,
}

impl TempoTimeline {
    pub fn new(rate: f64, sample_rate: u32) -> TempoTimeline {
        TempoTimeline { rate, sample_rate: f64::from(sample_rate), start: None, frames_out: 0 }
    }

    /// Notes the time of sound going in; only the first known one counts.
    pub fn input(&mut self, pts: Option<f64>) {
        if self.start.is_none() {
            self.start = pts;
        }
    }

    /// The media time of the next sample frame out, once one is known.
    pub fn next(&self) -> Option<f64> {
        self.start.map(|s| s + self.frames_out as f64 * self.rate / self.sample_rate)
    }

    /// Notes `frames` sample frames coming out.
    pub fn output(&mut self, frames: usize) {
        self.frames_out += frames as u64;
    }
}

/// Decoded frames waiting for their time, oldest first.
#[derive(Debug)]
pub struct FrameQueue<T> {
    items: VecDeque<(f64, T)>,
}

impl<T> Default for FrameQueue<T> {
    fn default() -> Self {
        FrameQueue { items: VecDeque::new() }
    }
}

impl<T> FrameQueue<T> {
    pub fn push(&mut self, pts: f64, item: T) {
        self.items.push_back((pts, item));
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// The time of the next frame to show.
    pub fn front_pts(&self) -> Option<f64> {
        self.items.front().map(|(t, _)| *t)
    }

    /// The newest frame due at `now`, dropping older due frames: when the
    /// display falls behind, frames are skipped rather than shown late.
    pub fn pop_due(&mut self, now: f64) -> Option<(f64, T)> {
        let mut due = None;
        while self.items.front().is_some_and(|(t, _)| *t <= now) {
            due = self.items.pop_front();
        }
        due
    }

    /// The next frame whatever its time (for stepping and the first frame
    /// after a seek).
    pub fn pop(&mut self) -> Option<(f64, T)> {
        self.items.pop_front()
    }
}

/// How many decoded frames of `bytes` each to keep ready: up to
/// [`MAX_QUEUED_FRAMES`], within [`QUEUE_BYTES`], and never fewer than two.
pub fn queue_cap(bytes: usize) -> usize {
    (QUEUE_BYTES / bytes.max(1)).clamp(2, MAX_QUEUED_FRAMES)
}

pub const MAX_QUEUED_FRAMES: usize = 8;
pub const QUEUE_BYTES: usize = 96 << 20;

/// Seconds of a timestamp in `tb`, measured from the stream's `start`
/// (also in `tb`; files that don't start at zero, such as MPEG-TS
/// captures, show from 0). `None` for a missing timestamp.
pub fn secs(ts: i64, tb: AVRational, start: i64) -> Option<f64> {
    if ts == AV_NOPTS_VALUE || tb.den == 0 {
        return None;
    }
    let start = if start == AV_NOPTS_VALUE { 0 } else { start };
    Some((ts - start) as f64 * f64::from(tb.num) / f64::from(tb.den))
}

/// The container timestamp (`AV_TIME_BASE` units, as `av_seek_frame` takes
/// with stream -1) of `t` seconds after the file's `start`.
pub fn seek_ts(t: f64, start: i64) -> i64 {
    let start = if start == AV_NOPTS_VALUE { 0 } else { start };
    start + (t.max(0.0) * crate::ffi::AV_TIME_BASE as f64).round() as i64
}

/// Whether a frame at `pts` is still before a seek's `target` and should be
/// decoded but not shown: the next frame (one frame time later) would
/// already be showing at `target`. The decoder starts at the keyframe
/// before the target and works forward to the frame on screen at it.
pub fn before_target(pts: f64, target: f64, fps: f64) -> bool {
    pts + 1.0 / fps.max(1.0) <= target + 1e-6
}

/// The time to seek to for a step of `frames` back from `shown`.
pub fn step_back(shown: f64, frames: i32, fps: f64) -> f64 {
    (shown - f64::from(frames.unsigned_abs()) / fps.max(1.0)).max(0.0)
}

/// Clockwise quarter turns (0 to 3) for a display matrix rotation, which
/// FFmpeg gives in degrees anticlockwise.
pub fn turns_from_degrees(ccw: f64) -> u8 {
    if !ccw.is_finite() {
        return 0;
    }
    ((-ccw / 90.0).round() as i64).rem_euclid(4) as u8
}

/// The shown width of a picture `w` pixels wide with sample aspect ratio
/// `sar` (anamorphic DVD video has non-square pixels).
pub fn display_width(w: i32, sar: AVRational) -> i32 {
    match sar.value() {
        Some(r) if (r - 1.0).abs() > 1e-3 => ((f64::from(w) * r).round() as i32).max(1),
        _ => w,
    }
}

/// The size to scale a `w` × `h` picture to so its long side is at most
/// `max_side`, never enlarging.
pub fn fit(w: i32, h: i32, max_side: u32) -> (i32, i32) {
    let long = w.max(h).max(1) as f64;
    let max = f64::from(max_side.min(i32::MAX as u32));
    if long <= max {
        return (w.max(1), h.max(1));
    }
    let s = max / long;
    (((f64::from(w) * s).round() as i32).max(1), ((f64::from(h) * s).round() as i32).max(1))
}

/// Where Eagle's thumbnailer takes a video's picture: ten seconds in, or a
/// third of the way through a clip shorter than thirty seconds.
pub fn thumbnail_time(duration: f64) -> f64 {
    if duration > 0.0 { (duration / 3.0).min(10.0) } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wall_clock_runs_pauses_and_jumps() {
        let mut c = WallClock::new(5.0);
        assert_eq!(c.now(100.0), 5.0, "paused at start");
        c.start(100.0);
        assert_eq!(c.now(101.5), 6.5);
        c.pause(102.0);
        assert_eq!(c.now(110.0), 7.0, "frozen while paused");
        c.start(110.0);
        c.set_rate(2.0, 111.0);
        assert_eq!(c.now(112.0), 10.0, "one second at 1x then one at 2x");
        c.set(1.0, 112.0);
        assert_eq!(c.now(112.5), 2.0);
        assert!(c.running());
    }

    #[test]
    fn audio_clock_stops_at_the_end_of_its_chunk() {
        let a = AudioClock { pts: 10.0, taken_at: 50.0, span: 0.01, rate: 1.0 };
        assert_eq!(a.now(50.0), 10.0);
        assert!((a.now(50.005) - 10.005).abs() < 1e-9);
        assert_eq!(a.now(51.0), 10.01, "a starved output holds the clock");
        let fast = AudioClock { rate: 2.0, span: 1.0, ..a };
        assert_eq!(fast.now(50.25), 10.5);
    }

    #[test]
    fn stretched_sound_keeps_the_clock_on_media_time() {
        // A second of output at double speed covers two seconds of the clip.
        let a = AudioClock { pts: 3.0, taken_at: 10.0, span: 1024.0 * 2.0 / 48_000.0, rate: 2.0 };
        assert!((a.now(10.01) - 3.02).abs() < 1e-9);
        assert_eq!(a.now(11.0), 3.0 + a.span, "a starved output holds the clock");
    }

    #[test]
    fn tempo_is_skipped_at_normal_speed() {
        assert!(!needs_tempo(1.0));
        assert!(!needs_tempo(1.00001));
        assert!(needs_tempo(0.75));
        assert!(needs_tempo(1.25));
    }

    #[test]
    fn tempo_chains_stay_in_the_filters_range() {
        assert_eq!(tempo_chain(2.0), [2.0]);
        assert_eq!(tempo_chain(0.5), [0.5]);
        assert_eq!(tempo_chain(16.0), [16.0]);
        assert_eq!(tempo_chain(0.25), [0.5, 0.5]);
        assert_eq!(tempo_chain(100.0), [16.0], "clamped to the player's fastest");
        for rate in [0.1, 0.13, 0.25, 0.3, 0.49, 0.5, 0.75, 1.25, 3.0, 8.0, 16.0] {
            let chain = tempo_chain(rate);
            assert!(chain.iter().all(|t| (0.5..=100.0).contains(t)), "{rate}: {chain:?}");
            assert!((chain.iter().product::<f64>() - rate).abs() < 1e-9, "{rate}: {chain:?}");
        }
    }

    #[test]
    fn tempo_timeline_counts_from_the_first_sample_in() {
        let mut t = TempoTimeline::new(2.0, 48_000);
        assert_eq!(t.next(), None, "nothing known yet");
        t.input(None);
        assert_eq!(t.next(), None);
        t.input(Some(5.0));
        t.input(Some(5.5));
        assert_eq!(t.next(), Some(5.0), "later times in say nothing of what comes out");
        t.output(24_000);
        assert_eq!(t.next(), Some(6.0), "half a second out at 2x is a second of the clip");
        let mut slow = TempoTimeline::new(0.25, 48_000);
        slow.input(Some(0.0));
        slow.output(48_000);
        assert_eq!(slow.next(), Some(0.25));
    }

    #[test]
    fn due_frames_skip_late_ones() {
        let mut q = FrameQueue::default();
        for (i, t) in [0.0, 0.04, 0.08, 0.12].into_iter().enumerate() {
            q.push(t, i);
        }
        assert_eq!(q.pop_due(-0.01), None, "nothing due before the first");
        assert_eq!(q.pop_due(0.0), Some((0.0, 0)));
        assert_eq!(q.pop_due(0.1), Some((0.08, 2)), "the frame at 0.04 is skipped");
        assert_eq!(q.len(), 1);
        assert_eq!(q.front_pts(), Some(0.12));
        assert_eq!(q.pop(), Some((0.12, 3)));
        assert!(q.is_empty());
    }

    #[test]
    fn queue_caps_follow_frame_size() {
        assert_eq!(queue_cap(1920 * 1080 * 4), MAX_QUEUED_FRAMES);
        assert_eq!(queue_cap(3840 * 2160 * 4), 3);
        assert_eq!(queue_cap(7680 * 4320 * 4), 2, "never fewer than two");
        assert_eq!(queue_cap(0), MAX_QUEUED_FRAMES);
    }

    #[test]
    fn timestamps_to_seconds() {
        let tb = AVRational { num: 1, den: 90_000 };
        assert_eq!(secs(90_000, tb, 0), Some(1.0));
        assert_eq!(secs(180_000, tb, 90_000), Some(1.0), "measured from the stream start");
        assert_eq!(secs(45_000, tb, AV_NOPTS_VALUE), Some(0.5));
        assert_eq!(secs(AV_NOPTS_VALUE, tb, 0), None);
        assert_eq!(secs(1, AVRational { num: 1, den: 0 }, 0), None);
    }

    #[test]
    fn seek_targets() {
        assert_eq!(seek_ts(1.5, 0), 1_500_000);
        assert_eq!(seek_ts(1.0, 1_400_000), 2_400_000, "files starting late");
        assert_eq!(seek_ts(-3.0, AV_NOPTS_VALUE), 0);
        // Decoding forward from the keyframe: frames that are over by the
        // target are dropped; the one on screen at it is kept.
        assert!(before_target(0.9, 1.0, 25.0));
        assert!(before_target(0.44, 0.48, 25.0), "frame 11 ends where frame 12 starts");
        assert!(!before_target(0.48, 0.5, 25.0), "frame 12 shows from 0.48 to 0.52");
        assert!(!before_target(1.0, 1.0, 25.0));
        assert!((step_back(1.0, 1, 25.0) - 0.96).abs() < 1e-9);
        assert!((step_back(1.0, -5, 25.0) - 0.8).abs() < 1e-9);
        assert_eq!(step_back(0.01, 3, 25.0), 0.0);
    }

    #[test]
    fn rotations_and_shapes() {
        assert_eq!(turns_from_degrees(0.0), 0);
        assert_eq!(turns_from_degrees(-90.0), 1, "a portrait phone clip");
        assert_eq!(turns_from_degrees(90.0), 3);
        assert_eq!(turns_from_degrees(180.0), 2);
        assert_eq!(turns_from_degrees(f64::NAN), 0);
        assert_eq!(display_width(720, AVRational { num: 32, den: 27 }), 853);
        assert_eq!(display_width(720, AVRational { num: 1, den: 1 }), 720);
        assert_eq!(display_width(720, AVRational { num: 0, den: 1 }), 720);
        assert_eq!(fit(1920, 1080, 320), (320, 180));
        assert_eq!(fit(100, 50, 320), (100, 50), "never enlarged");
    }

    #[test]
    fn thumbnails_are_taken_where_eagle_takes_them() {
        assert_eq!(thumbnail_time(120.0), 10.0);
        assert_eq!(thumbnail_time(9.0), 3.0);
        assert_eq!(thumbnail_time(0.0), 0.0);
    }
}
