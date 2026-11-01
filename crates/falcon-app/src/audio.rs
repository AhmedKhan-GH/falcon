//! Audio playback for the viewer, and the waveforms drawn under it.
//!
//! Files are decoded by `falcon_library::audio` (the same decoder the
//! waveform, the stored duration and the tempo use) and played through
//! rodio on the default output device. Only an output stream is ever
//! opened, which needs no macOS permission; nothing here touches an input
//! device.

use std::collections::{HashMap, HashSet};
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use crate::locale::trf;
use eframe::egui;
use falcon_library::audio::AudioSource;
use falcon_library::waveform;

/// Peaks computed per item for the viewer's waveform; the drawing merges
/// them down to its bar count.
pub const PEAKS: usize = 1200;

/// What is loaded into the player.
struct Track {
    id: String,
    path: PathBuf,
    ext: String,
    player: Arc<rodio::Player>,
    /// The length the decoder found, for items whose record has none.
    duration: Option<f64>,
}

/// The viewer's audio player: one track at a time.
pub struct Audio {
    /// The output stream, opened on first use and kept for the session.
    device: Option<rodio::MixerDeviceSink>,
    /// The output stream being opened on a worker thread for a sound
    /// effect, so the first one doesn't hold up the frame.
    opening: Option<mpsc::Receiver<Result<rodio::MixerDeviceSink, String>>>,
    track: Option<Track>,
    /// Start again from the top at the end (Eagle's default).
    pub looping: bool,
    /// 0..=1.
    pub volume: f32,
    /// Why the last load failed, shown in the viewer.
    pub error: Option<String>,
    /// Playback speed of the loaded track, 1 at each load.
    speed: f32,
    /// The loaded item's length in seconds, as its record gives it (the
    /// viewer sets it), for the skip keys.
    pub duration: f64,
    /// The volume last drawn and until when the volume tip shows, as in
    /// the video viewer.
    pub tip_seen: Option<f32>,
    pub tip_until: f64,
}

/// The speeds in the audio player's menu (Eagle's video.js list for
/// audio); the speed keys step through all of [`falcon_video::RATES`].
pub const MENU_RATES: [f32; 7] = [0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0];

impl Default for Audio {
    fn default() -> Self {
        Audio { device: None, opening: None, track: None, looping: true, volume: 1.0, error: None, speed: 1.0, duration: 0.0, tip_seen: None, tip_until: 0.0 }
    }
}

/// Opens the default output device's stream (output only).
fn open_sink() -> Result<rodio::MixerDeviceSink, String> {
    let mut d = rodio::DeviceSinkBuilder::open_default_sink().map_err(|e| e.to_string())?;
    d.log_on_drop(false);
    Ok(d)
}

/// A decoded file as a rodio source. Each block of the decoder is one
/// rodio span, so a change of rate or channels between blocks is followed.
struct Stream {
    source: Box<dyn AudioSource>,
    block: Vec<f32>,
    at: usize,
    channels: NonZero<u16>,
    rate: NonZero<u32>,
}

impl Stream {
    fn open(path: &Path, ext: &str) -> Result<Stream, String> {
        let source = falcon_library::audio::open(path, ext).map_err(|e| e.to_string())?;
        let mut s = Stream { source, block: Vec::new(), at: 0, channels: NonZero::<u16>::MIN, rate: NonZero::<u32>::MIN };
        s.refill();
        Ok(s)
    }

    /// Takes the next block from the decoder; leaves `block` empty at the end.
    fn refill(&mut self) {
        self.block.clear();
        self.at = 0;
        if let Some(b) = self.source.next_block() {
            self.block.extend_from_slice(b.samples);
            self.channels = NonZero::new(b.channels).unwrap_or(NonZero::<u16>::MIN);
            self.rate = NonZero::new(b.rate).unwrap_or(NonZero::<u32>::MIN);
        }
    }
}

impl Iterator for Stream {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let s = *self.block.get(self.at)?;
        self.at += 1;
        if self.at == self.block.len() {
            self.refill();
        }
        Some(s)
    }
}

impl rodio::Source for Stream {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.block.len() - self.at)
    }

    fn channels(&self) -> rodio::ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        self.rate
    }

    fn total_duration(&self) -> Option<Duration> {
        self.source.duration().map(Duration::from_secs_f64)
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        self.source.seek(pos.as_secs_f64()).map_err(|e| rodio::source::SeekError::Other(Arc::new(e)))?;
        self.refill();
        Ok(())
    }
}

impl Audio {
    /// The item loaded, if any.
    pub fn loaded(&self) -> Option<&str> {
        self.track.as_ref().map(|t| t.id.as_str())
    }

    /// Loads an item's file and starts playing it if `autoplay`. Loading the
    /// item already loaded does nothing.
    pub fn load(&mut self, id: &str, path: &Path, ext: &str, autoplay: bool) {
        if self.loaded() == Some(id) {
            return;
        }
        self.stop();
        self.error = None;
        self.speed = 1.0;
        self.tip_seen = None;
        if let Err(e) = self.open() {
            self.error = Some(trf("No audio output: {error}", &[("error", &e)]));
            return;
        }
        let Some(device) = &self.device else { return };
        let source = match Stream::open(path, ext) {
            Ok(s) => s,
            Err(e) => {
                self.error = Some(trf("Can't play this file: {error}", &[("error", &e)]));
                return;
            }
        };
        let player = Arc::new(rodio::Player::connect_new(device.mixer()));
        player.set_volume(self.volume);
        if !autoplay {
            player.pause();
        }
        let duration = source.source.duration();
        player.append(source);
        self.track = Some(Track { id: id.to_string(), path: path.to_path_buf(), ext: ext.to_string(), player, duration });
    }

    /// Opens the default output stream unless it is open (waiting for one
    /// a sound effect is opening); it stays open for the session.
    fn open(&mut self) -> Result<(), String> {
        self.adopt(true);
        if self.device.is_none() {
            self.device = Some(open_sink()?);
        }
        Ok(())
    }

    /// Takes the stream a worker opened, if it is ready (or once it is,
    /// with `wait`).
    fn adopt(&mut self, wait: bool) {
        let Some(rx) = &self.opening else { return };
        let got = if wait {
            rx.recv().ok()
        } else {
            match rx.try_recv() {
                Ok(r) => Some(r),
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => None,
            }
        };
        self.opening = None;
        match got {
            Some(Ok(d)) => self.device = Some(d),
            Some(Err(e)) => crate::errlog::error(&format!("audio output: {e}")),
            None => {}
        }
    }

    /// Mixes a short mono clip (a sound effect, `sounds.rs`) into the
    /// output stream, beside whatever track is playing, at full volume
    /// whatever the player's. Never waits: the first clip opens the stream
    /// on a worker thread and plays from there; one asked for while that
    /// is under way is dropped.
    pub fn play_clip(&mut self, rate: u32, samples: &[f32]) -> Result<(), String> {
        let rate = std::num::NonZero::new(rate).ok_or("no sample rate")?;
        let clip = rodio::buffer::SamplesBuffer::new(std::num::NonZero::<u16>::MIN, rate, samples.to_vec());
        self.adopt(false);
        if let Some(device) = &self.device {
            device.mixer().add(clip);
        } else if self.opening.is_none() {
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let r = open_sink();
                if let Ok(d) = &r {
                    d.mixer().add(clip);
                }
                let _ = tx.send(r);
            });
            self.opening = Some(rx);
        }
        Ok(())
    }

    /// The loaded track's length as decoded, for items whose record holds
    /// no duration (imported before falcon could read the format).
    pub fn track_duration(&self) -> Option<f64> {
        self.track.as_ref().and_then(|t| t.duration)
    }

    /// A handle on the loaded track's player, for pausing it from another
    /// thread (the window being hidden).
    pub fn handle(&self) -> Option<Arc<rodio::Player>> {
        self.track.as_ref().map(|t| t.player.clone())
    }

    /// Stops and unloads the track.
    pub fn stop(&mut self) {
        // Dropping the player stops its sound.
        self.track = None;
    }

    pub fn is_playing(&self) -> bool {
        self.track.as_ref().is_some_and(|t| !t.player.is_paused() && !t.player.empty())
    }

    /// Play or pause; at the end, plays again from the top.
    pub fn toggle(&mut self) {
        let Some(t) = &self.track else { return };
        if t.player.empty() {
            self.restart(true);
        } else if t.player.is_paused() {
            t.player.play();
        } else {
            t.player.pause();
        }
    }

    /// The playback position in seconds of the file. rodio counts in
    /// played time, which runs `speed` times slower than the file's.
    pub fn position(&self) -> f64 {
        self.track.as_ref().map_or(0.0, |t| t.player.get_pos().as_secs_f64() * f64::from(self.speed))
    }

    /// Jumps to `secs` of the file. A track that already ended is reloaded
    /// first.
    pub fn seek(&mut self, secs: f64) {
        if self.track.as_ref().is_some_and(|t| t.player.empty()) {
            self.restart(false);
        }
        if let Some(t) = &self.track {
            let _ = t.player.try_seek(Duration::from_secs_f64(secs.max(0.0) / f64::from(self.speed)));
        }
    }

    pub fn speed(&self) -> f32 {
        self.speed
    }

    /// Sets the playback speed (clamped to 0.25–8×). The track is re-seeked
    /// to where it is, so rodio's played-time clock restarts at the new
    /// speed and [`Audio::position`] stays right. Unlike a browser, rodio
    /// changes the pitch with the speed.
    pub fn set_speed(&mut self, speed: f32) {
        let pos = self.position();
        self.speed = speed.clamp(falcon_video::RATES[0], falcon_video::RATES[falcon_video::RATES.len() - 1]);
        if let Some(t) = &self.track {
            t.player.set_speed(self.speed);
        }
        if self.track.as_ref().is_some_and(|t| !t.player.empty()) {
            self.seek(pos);
        }
    }

    pub fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.0);
        if let Some(t) = &self.track {
            t.player.set_volume(self.volume);
        }
    }

    /// Called every frame while the track is shown: loops it at the end.
    pub fn tick(&mut self) {
        if self.looping && self.track.as_ref().is_some_and(|t| t.player.empty()) {
            self.restart(true);
        }
    }

    /// Appends the file again (sources can't rewind once finished).
    fn restart(&mut self, play: bool) {
        let Some(t) = &self.track else { return };
        match Stream::open(&t.path, &t.ext) {
            Ok(source) => {
                if play {
                    t.player.play();
                } else {
                    t.player.pause();
                }
                t.player.append(source);
            }
            Err(e) => {
                self.error = Some(trf("Can't play this file: {error}", &[("error", &e)]));
                self.track = None;
            }
        }
    }
}

/// An item's (min, max) peaks, shared with the drawing.
pub type Peaks = Arc<Vec<(f32, f32)>>;

/// Waveform peaks per item id, computed once on a background thread and
/// kept for the session (Eagle stores no peak data either).
#[derive(Default)]
pub struct Waveforms {
    done: Arc<Mutex<HashMap<String, Option<Peaks>>>>,
    started: HashSet<String>,
}

impl Waveforms {
    /// The peaks of an item: `None` while they are being computed,
    /// `Some(None)` if the file couldn't be decoded.
    pub fn get(&mut self, ctx: &egui::Context, id: &str, path: &Path, ext: &str) -> Option<Option<Peaks>> {
        if let Some(p) = self.done.lock().ok()?.get(id) {
            return Some(p.clone());
        }
        if self.started.insert(id.to_string()) {
            let (done, ctx, id, path, ext) = (self.done.clone(), ctx.clone(), id.to_string(), path.to_path_buf(), ext.to_string());
            std::thread::spawn(move || {
                let peaks = waveform::peaks(&path, &ext, PEAKS).map(Arc::new);
                if let Ok(mut m) = done.lock() {
                    m.insert(id, peaks);
                }
                ctx.request_repaint();
            });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::Source;

    /// A mono 16-bit WAV of `secs` seconds of a 440 Hz tone.
    fn wav(path: &Path, secs: u32) {
        let rate = 8000u32;
        let samples: Vec<i16> = (0..rate * secs).map(|n| ((n as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 12000.0) as i16).collect();
        let data_len = samples.len() as u32 * 2;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data_len).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * 2).to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        for s in samples {
            b.extend_from_slice(&s.to_le_bytes());
        }
        std::fs::write(path, b).unwrap();
    }

    /// Polls `get` until the background thread is done.
    fn wait(w: &mut Waveforms, ctx: &egui::Context, id: &str, p: &Path) -> Option<Peaks> {
        for _ in 0..500 {
            if let Some(got) = w.get(ctx, id, p, "wav") {
                return got;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("waveform never finished");
    }

    #[test]
    fn decodes_for_playback_without_a_device() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("tone.wav");
        wav(&p, 2);
        let mut d = Stream::open(&p, "wav").unwrap();
        let total = d.total_duration().unwrap().as_secs_f64();
        assert!((total - 2.0).abs() < 0.01, "{total}");
        assert_eq!((d.channels().get(), d.sample_rate().get()), (1, 8000));
        // Seeking through rodio's interface lands where asked: the samples
        // after 1.5 s are the ones the file has there.
        d.try_seek(Duration::from_secs_f64(1.5)).unwrap();
        let rest: Vec<f32> = d.by_ref().collect();
        assert_eq!(rest.len(), 4000);
        let want = (12000.0f32 * (12000.0 * 440.0 * std::f32::consts::TAU / 8000.0).sin()).trunc() / 32768.0;
        assert!((rest[0] - want).abs() < 1e-3, "{} vs {want}", rest[0]);
        assert!(Stream::open(&dir.path().join("missing.wav"), "wav").is_err());
        // A format with no decoder is refused with a reason, not a panic.
        let wma = dir.path().join("x.wma");
        std::fs::write(&wma, [0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0, 0, 0, 0]).unwrap();
        assert_eq!(Stream::open(&wma, "wma").err().as_deref(), Some("unsupported audio format"));
        // Nothing is loaded until the viewer asks, so no device is opened here.
        let mut a = Audio::default();
        assert_eq!(a.loaded(), None);
        assert!(!a.is_playing());
        assert_eq!(a.position(), 0.0);
        a.set_speed(20.0);
        assert_eq!(a.speed(), 8.0, "speeds are clamped to the player's range");
        a.set_speed(0.5);
        assert_eq!(a.speed(), 0.5);
    }

    #[test]
    fn waveforms_are_computed_once_in_the_background() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("tone.wav");
        wav(&p, 1);
        let ctx = egui::Context::default();
        let mut w = Waveforms::default();
        assert!(w.get(&ctx, "a", &p, "wav").is_none());
        let peaks = wait(&mut w, &ctx, "a", &p).expect("peaks computed");
        assert_eq!(peaks.len(), PEAKS);
        assert!(peaks.iter().any(|&(_, h)| h > 0.3));
        // A file that isn't audio settles as "no waveform" rather than pending.
        let bad = dir.path().join("bad.wav");
        std::fs::write(&bad, b"nope").unwrap();
        assert_eq!(wait(&mut w, &ctx, "b", &bad), None);
    }
}
