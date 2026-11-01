//! The FFmpeg player: two worker threads decode ahead, the owner on the
//! UI thread takes frames when they are due.
//!
//! - The **demux** thread reads packets, decodes and resamples the sound
//!   itself (it is cheap), and queues video packets for the video thread.
//!   It stops reading when both queues are full enough and performs seeks.
//! - The **video** thread decodes and converts pictures to RGBA into a
//!   short queue of frames.
//! - The sound is pulled by an [`AudioOut`] the caller plays (falcon hands
//!   it to rodio). What it takes moves the audio clock, which is the master
//!   clock while sound plays; with no sound (no track, no output, muted, or
//!   the track ended first) a wall clock is.
//!
//! Every seek bumps a generation number; work done for an older generation
//! is thrown away, so a seek never shows a stale frame.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Instant;

use crate::decode::{Decoder, Input, OUT_CHANNELS, OUT_RATE, Packet, Resampler, Scaler};
use crate::ffi;
use crate::media;
use crate::sync::{self, AudioClock, FrameQueue, WallClock};
use crate::{Error, Frame, Info, Result};

/// Sound decoded ahead of the output, in media seconds.
const AUDIO_AHEAD: f64 = 1.0;
/// Video packets queued ahead of the video thread.
const VIDEO_PACKETS: usize = 48;
/// Bytes of video packets beyond which the demuxer waits whatever the sound needs.
const MAX_PACKET_BYTES: usize = 64 << 20;
/// Samples (both channels) the output takes at once: about 10 ms.
const CHUNK: usize = 1024;

enum VideoMsg {
    Packet(Packet),
    /// Start of a new generation: flush the decoder, drop frames before `target`.
    Flush { generation: u64, target: f64 },
    End,
}

struct State {
    stop: bool,
    generation: u64,
    /// A seek waiting for the demux thread.
    seek: Option<f64>,
    rate: f64,
    /// Whether sound is decoded and played (a track, an output, not muted).
    audio_on: bool,
    paused: bool,
    packets: VecDeque<VideoMsg>,
    packet_bytes: usize,
    frames: FrameQueue<Frame>,
    frame_cap: usize,
    /// The video thread has drained the decoder for this generation.
    video_done: bool,
    /// The demuxer reached the end for this generation.
    demux_done: bool,
    /// Interleaved samples ready for the output, and the media time of the
    /// first one.
    samples: VecDeque<f32>,
    samples_pts: f64,
    /// The speed the queued samples were resampled for.
    samples_rate: f64,
    clock: Option<AudioClock>,
}

impl State {
    fn audio_ahead(&self) -> f64 {
        (self.samples.len() / OUT_CHANNELS) as f64 * self.samples_rate / f64::from(OUT_RATE)
    }
}

struct Shared {
    state: Mutex<State>,
    cv: Condvar,
    /// The output volume, 0..=1, as `f32` bits.
    volume: AtomicU32,
    epoch: Instant,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn wait<'a>(&self, g: MutexGuard<'a, State>) -> MutexGuard<'a, State> {
        self.cv.wait(g).unwrap_or_else(|p| p.into_inner())
    }

    fn wall(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64()
    }
}

/// The sound of a [`Player`]: interleaved stereo `f32` samples at
/// [`OUT_RATE`], silence while paused or starved. It ends (returns `None`)
/// once the player is dropped. Pull it from the audio output's thread.
pub struct AudioOut {
    shared: Arc<Shared>,
    buf: Vec<f32>,
    pos: usize,
}

impl AudioOut {
    pub const CHANNELS: u16 = OUT_CHANNELS as u16;
    pub const RATE: u32 = OUT_RATE;

    /// The next sample.
    pub fn next_sample(&mut self) -> Option<f32> {
        if self.pos >= self.buf.len() {
            self.refill()?;
        }
        let s = self.buf[self.pos];
        self.pos += 1;
        Some(s)
    }

    fn refill(&mut self) -> Option<()> {
        let mut st = self.shared.lock();
        if st.stop {
            return None;
        }
        self.buf.clear();
        self.pos = 0;
        if st.paused || !st.audio_on || st.samples.is_empty() {
            self.buf.resize(CHUNK, 0.0);
            return Some(());
        }
        let n = st.samples.len().min(CHUNK) / OUT_CHANNELS * OUT_CHANNELS;
        let volume = f32::from_bits(self.shared.volume.load(Ordering::Relaxed));
        self.buf.extend(st.samples.drain(..n).map(|s| s * volume));
        if self.buf.is_empty() {
            self.buf.resize(CHUNK, 0.0);
            return Some(());
        }
        let span = (n / OUT_CHANNELS) as f64 * st.samples_rate / f64::from(OUT_RATE);
        st.clock = Some(AudioClock { pts: st.samples_pts, taken_at: self.shared.wall(), span, rate: st.samples_rate });
        st.samples_pts += span;
        drop(st);
        self.shared.cv.notify_all();
        Some(())
    }
}

/// What the clocks need from the shared state.
#[derive(Clone, Copy)]
struct Snapshot {
    /// Sound is playing (on, and not run out at the end).
    audio: bool,
    clock: Option<AudioClock>,
}

/// A playing file. Lives on the thread that draws it; dropping it stops
/// the workers and ends its [`AudioOut`].
pub struct Player {
    shared: Arc<Shared>,
    threads: Vec<JoinHandle<()>>,
    path: PathBuf,
    info: Info,
    /// Whether the file has a sound track with a decoder and an output.
    audio_out: bool,
    wall: WallClock,
    /// Whether the audio clock was the master last time it was read.
    on_audio: bool,
    generation: u64,
    playing: bool,
    looping: bool,
    rate: f32,
    muted: bool,
    /// Where the playhead is while paused or waiting for a seek.
    pos: f64,
    /// Frames to show regardless of the clock: one after a seek, `n` for a
    /// step forward.
    show_next: u32,
    /// The seek being waited for.
    seeking: Option<f64>,
    ready: bool,
}

impl Player {
    /// Opens `path` paused at the start. With `audio`, the file's sound is
    /// decoded and the returned [`AudioOut`] must be played for the clock
    /// to run; without, it plays silent on the wall clock.
    pub fn open(path: &Path, audio: bool) -> Result<(Player, Option<AudioOut>)> {
        let input = Input::open(path)?;
        let api = input.api();
        let (vi, vcodec) = input.best(ffi::AVMEDIA_TYPE_VIDEO).ok_or_else(|| Error("no video stream FFmpeg can decode".into()))?;
        let video = Decoder::open(&input, vi, vcodec)?;
        let info = media::info(&input, &video);
        let sound = if audio { input.best(ffi::AVMEDIA_TYPE_AUDIO).and_then(|(ai, ac)| Decoder::open(&input, ai, ac).ok()) } else { None };
        let audio_out = sound.is_some();
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                stop: false,
                generation: 0,
                seek: None,
                rate: 1.0,
                audio_on: audio_out,
                paused: true,
                packets: VecDeque::new(),
                packet_bytes: 0,
                frames: FrameQueue::default(),
                frame_cap: 4,
                video_done: false,
                demux_done: false,
                samples: VecDeque::new(),
                samples_pts: 0.0,
                samples_rate: 1.0,
                clock: None,
            }),
            cv: Condvar::new(),
            volume: AtomicU32::new(1.0f32.to_bits()),
            epoch: Instant::now(),
        });
        let fps = if info.fps > 0.0 { f64::from(info.fps) } else { 30.0 };
        let packet = Packet::new(api)?;
        let demux = {
            let shared = shared.clone();
            let resampler = Resampler::new(api);
            std::thread::Builder::new()
                .name("falcon-ffmpeg-demux".into())
                .spawn(move || demux_loop(&shared, input, vi, sound, resampler, packet))
                .map_err(|e| Error(e.to_string()))?
        };
        let decode = {
            let shared = shared.clone();
            let scaler = Scaler::new(api);
            std::thread::Builder::new()
                .name("falcon-ffmpeg-video".into())
                .spawn(move || video_loop(&shared, video, scaler, fps))
                .map_err(|e| Error(e.to_string()))?
        };
        let out = audio_out.then(|| AudioOut { shared: shared.clone(), buf: Vec::new(), pos: 0 });
        let player = Player {
            shared,
            threads: vec![demux, decode],
            path: path.to_path_buf(),
            info,
            audio_out,
            wall: WallClock::new(0.0),
            on_audio: false,
            generation: 0,
            playing: false,
            looping: false,
            rate: 1.0,
            muted: false,
            pos: 0.0,
            show_next: 1,
            seeking: None,
            ready: false,
        };
        Ok((player, out))
    }

    pub fn info(&self) -> &Info {
        &self.info
    }

    /// The first picture is decoded (or there is none to wait for).
    pub fn is_ready(&self) -> bool {
        self.ready
    }

    fn fps(&self) -> f64 {
        if self.info.fps > 0.0 { f64::from(self.info.fps) } else { 30.0 }
    }

    /// Plays at the current speed; from the end, plays from the start.
    pub fn play(&mut self) {
        if self.at_end() {
            self.seek(0.0);
        }
        self.playing = true;
        let wall = self.shared.wall();
        self.wall.set(self.pos, wall);
        self.wall.start(wall);
        self.shared.lock().paused = false;
        self.shared.cv.notify_all();
    }

    pub fn pause(&mut self) {
        if self.playing {
            self.pos = self.current_time();
        }
        self.playing = false;
        self.wall.pause(self.shared.wall());
        self.shared.lock().paused = true;
    }

    pub fn toggle(&mut self) {
        if self.playing { self.pause() } else { self.play() }
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Jumps to `secs` (clamped to the clip): the keyframe before it is
    /// found and decoded forward to the frame at `secs`.
    pub fn seek(&mut self, secs: f64) {
        let t = secs.clamp(0.0, self.info.duration.max(0.0));
        self.generation += 1;
        {
            let mut st = self.shared.lock();
            st.generation = self.generation;
            st.seek = Some(t);
            st.frames.clear();
            st.samples.clear();
            st.clock = None;
            st.video_done = false;
            st.demux_done = false;
        }
        self.shared.cv.notify_all();
        self.wall.set(t, self.shared.wall());
        self.on_audio = false;
        self.pos = t;
        self.seeking = Some(t);
        self.show_next = 1;
    }

    /// Moves `frames` frames forwards (or backwards when negative) and
    /// pauses. Forward steps take the next decoded frames; backward steps
    /// seek to the earlier frame.
    pub fn step(&mut self, frames: i32) {
        self.pause();
        if frames > 0 {
            self.show_next = self.show_next.max(frames.unsigned_abs());
        } else if frames < 0 {
            let t = sync::step_back(self.pos, frames, self.fps());
            self.seek(t);
        }
    }

    pub fn rate(&self) -> f32 {
        self.rate
    }

    /// Changes the speed. The sound is resampled to match (its pitch moves
    /// with the speed), so the queued sound is decoded again from here.
    pub fn set_rate(&mut self, rate: f32) {
        let rate = rate.clamp(0.1, 16.0);
        if (rate - self.rate).abs() < 1e-4 {
            return;
        }
        let now = self.current_time();
        self.rate = rate;
        self.wall.set_rate(f64::from(rate), self.shared.wall());
        let audio = {
            let mut st = self.shared.lock();
            st.rate = f64::from(rate);
            st.audio_on
        };
        if audio {
            self.seek(now);
        }
    }

    /// 0..=1.
    pub fn set_volume(&mut self, volume: f32) {
        self.shared.volume.store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    /// Muting stops decoding the sound and runs the picture on the wall
    /// clock; unmuting decodes it again from the playhead.
    pub fn set_muted(&mut self, muted: bool) {
        if muted == self.muted {
            return;
        }
        let now = self.current_time();
        self.muted = muted;
        if !self.audio_out {
            return;
        }
        {
            let mut st = self.shared.lock();
            st.audio_on = !muted;
            st.samples.clear();
            st.clock = None;
        }
        let wall = self.shared.wall();
        self.wall.set(now, wall);
        self.on_audio = false;
        self.pos = now;
        if !muted {
            self.seek(now);
        }
    }

    pub fn set_looping(&mut self, looping: bool) {
        self.looping = looping;
    }

    pub fn looping(&self) -> bool {
        self.looping
    }

    /// The playhead in seconds.
    pub fn current_time(&self) -> f64 {
        if let Some(t) = self.seeking {
            return t;
        }
        if !self.playing {
            return self.pos;
        }
        self.read_clock(self.snapshot()).clamp(0.0, self.info.duration.max(0.0))
    }

    fn snapshot(&self) -> Snapshot {
        let st = self.shared.lock();
        Snapshot { audio: st.audio_on && !(st.demux_done && st.samples.is_empty()), clock: st.clock }
    }

    /// The master clock's time: the audio clock while sound plays, else
    /// the wall clock. Until [`Player::follow_clock`] hands over, a stopped
    /// audio clock reads where it stopped.
    fn read_clock(&self, s: Snapshot) -> f64 {
        let wall = self.shared.wall();
        if s.audio || self.on_audio {
            return s.clock.map_or(self.pos, |c| c.now(wall));
        }
        self.wall.now(wall)
    }

    /// Notes which clock is the master; when the sound stops (it ended, or
    /// was muted) the wall clock carries on from where the audio clock was.
    fn follow_clock(&mut self, s: Snapshot) {
        if self.on_audio && !s.audio {
            let wall = self.shared.wall();
            self.wall.set(s.clock.map_or(self.pos, |c| c.now(wall)), wall);
        }
        self.on_audio = s.audio;
    }

    fn at_end(&self) -> bool {
        self.info.duration > 0.0 && self.pos >= self.info.duration - 1.0 / self.fps()
    }

    /// Call every frame: at the end of the clip, starts again from the top
    /// when looping, or stops.
    pub fn tick(&mut self) {
        if !self.playing || self.seeking.is_some() || self.show_next > 0 {
            return;
        }
        let ended = {
            let st = self.shared.lock();
            st.video_done && st.frames.is_empty() && !(st.audio_on && !st.samples.is_empty())
        };
        if !ended {
            return;
        }
        if self.looping {
            self.seek(0.0);
        } else {
            self.pause();
            self.pos = self.info.duration;
        }
    }

    /// The frame due now, if it differs from the last one handed out.
    pub fn frame(&mut self) -> Option<Frame> {
        let shared = self.shared.clone();
        let mut st = shared.lock();
        if self.show_next > 0 {
            let mut shown = None;
            while self.show_next > 0 {
                let Some(f) = st.frames.pop() else { break };
                self.show_next -= 1;
                shown = Some(f);
            }
            let done = st.video_done && st.frames.is_empty();
            drop(st);
            self.shared.cv.notify_all();
            if let Some((pts, f)) = shown {
                self.ready = true;
                if self.seeking.take().is_some() || !self.playing {
                    self.pos = pts.max(0.0);
                }
                let wall = self.shared.wall();
                self.wall.set(self.pos, wall);
                return Some(f);
            }
            if done {
                // Nothing left to show (a step past the end).
                self.ready = true;
                self.show_next = 0;
                self.seeking = None;
            }
            return None;
        }
        if !self.playing {
            return None;
        }
        let snapshot = Snapshot { audio: st.audio_on && !(st.demux_done && st.samples.is_empty()), clock: st.clock };
        self.follow_clock(snapshot);
        let now = self.read_clock(snapshot);
        let due = st.frames.pop_due(now);
        drop(st);
        let (_, f) = due?;
        self.shared.cv.notify_all();
        Some(f)
    }

    /// The picture at the playhead at the clip's full size, decoded afresh
    /// so playback is not disturbed.
    pub fn current_frame(&self) -> Result<Frame> {
        let t = self.current_time();
        media::frame_at(&self.path, t)
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.shared.lock().stop = true;
        self.shared.cv.notify_all();
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

/// Reads packets, decodes sound, queues video packets and carries out seeks.
fn demux_loop(shared: &Shared, mut input: Input, vi: i32, mut sound: Option<Decoder>, mut resampler: Resampler, mut packet: Packet) {
    let api = input.api();
    let mut target = f64::NEG_INFINITY;
    loop {
        let mut st = shared.lock();
        let (generation, rate, audio_on) = loop {
            if st.stop {
                return;
            }
            if let Some(t) = st.seek.take() {
                let generation = st.generation;
                drop(st);
                let _ = input.seek(t);
                if let Some(s) = sound.as_mut() {
                    s.flush();
                }
                resampler.reset();
                target = t;
                st = shared.lock();
                if st.generation == generation {
                    st.packets.clear();
                    st.packet_bytes = 0;
                    st.packets.push_back(VideoMsg::Flush { generation, target: t });
                    st.samples.clear();
                    st.demux_done = false;
                    shared.cv.notify_all();
                }
                continue;
            }
            let need_audio = st.audio_on && sound.is_some() && st.audio_ahead() < AUDIO_AHEAD;
            let need_video = st.packets.len() < VIDEO_PACKETS;
            if !st.demux_done && (need_audio || need_video) && st.packet_bytes < MAX_PACKET_BYTES {
                break (st.generation, st.rate, st.audio_on);
            }
            st = shared.wait(st);
        };
        drop(st);
        packet.unref();
        let more = input.read(&mut packet).unwrap_or(false);
        if more && packet.stream_index() == vi {
            let Ok(next) = Packet::new(api) else { return };
            let p = std::mem::replace(&mut packet, next);
            let mut st = shared.lock();
            if st.generation == generation {
                st.packet_bytes += p.size();
                st.packets.push_back(VideoMsg::Packet(p));
                drop(st);
                shared.cv.notify_all();
            }
            continue;
        }
        let Some(s) = sound.as_mut() else {
            if !more {
                end(shared, generation);
            }
            continue;
        };
        if more && (packet.stream_index() != s.info.index || !audio_on) {
            continue;
        }
        let _ = s.send(more.then_some(&packet));
        let mut out = Vec::new();
        let mut first_pts = None;
        while s.receive() {
            let f = s.frame();
            let pts = s.pts(f);
            let Ok(mut samples) = resampler.convert(f, rate) else { continue };
            let t = pts.unwrap_or(f64::NAN);
            if t.is_finite() && t < target {
                // Drop the part before a seek's target.
                let skip = (((target - t) * f64::from(OUT_RATE) / rate) as usize * OUT_CHANNELS).min(samples.len());
                samples.drain(..skip);
                if samples.is_empty() {
                    continue;
                }
                first_pts.get_or_insert(target);
            } else if t.is_finite() {
                first_pts.get_or_insert(t);
            }
            out.extend_from_slice(&samples);
        }
        let mut st = shared.lock();
        if st.generation == generation && st.audio_on && !out.is_empty() {
            if st.samples.is_empty() {
                st.samples_pts = first_pts.unwrap_or(st.samples_pts).max(0.0);
                st.samples_rate = rate;
            }
            st.samples.extend(out);
        }
        drop(st);
        if !more {
            end(shared, generation);
        }
    }
}

/// Marks the end of the file for `generation`.
fn end(shared: &Shared, generation: u64) {
    let mut st = shared.lock();
    if st.generation == generation {
        st.packets.push_back(VideoMsg::End);
        st.demux_done = true;
    }
    drop(st);
    shared.cv.notify_all();
}

/// Decodes queued video packets into RGBA frames.
fn video_loop(shared: &Shared, mut video: Decoder, mut scaler: Scaler, fps: f64) {
    let mut generation = 0;
    let mut target = f64::NEG_INFINITY;
    let mut delivered = false;
    let mut last_pts = 0.0;
    loop {
        let msg = {
            let mut st = shared.lock();
            loop {
                if st.stop {
                    return;
                }
                let control = matches!(st.packets.front(), Some(VideoMsg::Flush { .. } | VideoMsg::End));
                if st.packets.front().is_some() && (control || st.frames.len() < st.frame_cap) {
                    break;
                }
                st = shared.wait(st);
            }
            let msg = st.packets.pop_front();
            if let Some(VideoMsg::Packet(p)) = &msg {
                st.packet_bytes = st.packet_bytes.saturating_sub(p.size());
            }
            drop(st);
            shared.cv.notify_all();
            msg
        };
        let Some(msg) = msg else { continue };
        let ended = match msg {
            VideoMsg::Flush { generation: g, target: t } => {
                video.flush();
                generation = g;
                target = t;
                delivered = false;
                continue;
            }
            VideoMsg::Packet(p) => {
                let _ = video.send(Some(&p));
                false
            }
            VideoMsg::End => {
                let _ = video.send(None);
                true
            }
        };
        while video.receive() {
            let f = video.frame();
            let pts = video.pts(f).unwrap_or(last_pts + 1.0 / fps);
            last_pts = pts;
            if sync::before_target(pts, target, fps) {
                video.hold();
                continue;
            }
            let Some(frame) = scaler.convert(f, &video.info, u32::MAX) else { continue };
            delivered = true;
            push(shared, generation, pts, frame);
        }
        if ended {
            // A seek past the last frame shows the last frame.
            if !delivered
                && let Some(f) = video.held()
                && let Some(frame) = scaler.convert(f, &video.info, u32::MAX)
            {
                let pts = video.pts(f).unwrap_or(target);
                push(shared, generation, pts, frame);
            }
            let mut st = shared.lock();
            if st.generation == generation {
                st.video_done = true;
            }
            drop(st);
            shared.cv.notify_all();
        }
    }
}

fn push(shared: &Shared, generation: u64, pts: f64, frame: Frame) {
    let mut st = shared.lock();
    if st.generation == generation {
        st.frame_cap = sync::queue_cap(frame.rgba.len());
        st.frames.push(pts, frame);
    }
    drop(st);
    shared.cv.notify_all();
}
