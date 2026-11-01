//! Audio playback for the viewer, and the waveforms drawn under it.
//!
//! Playback goes through rodio on the default output device. Only an
//! output stream is ever opened, which needs no macOS permission; nothing
//! here touches an input device.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::locale::trf;
use eframe::egui;
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
}

/// The viewer's audio player: one track at a time.
pub struct Audio {
    /// The output stream, opened on first use and kept for the session.
    device: Option<rodio::MixerDeviceSink>,
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
        Audio { device: None, track: None, looping: true, volume: 1.0, error: None, speed: 1.0, duration: 0.0, tip_seen: None, tip_until: 0.0 }
    }
}

fn decoder(path: &Path, ext: &str) -> Result<rodio::Decoder<BufReader<File>>, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    rodio::Decoder::builder()
        .with_data(BufReader::new(file))
        .with_byte_len(len)
        .with_hint(ext)
        .with_seekable(true)
        .build()
        .map_err(|e| e.to_string())
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
        if self.device.is_none() {
            match rodio::DeviceSinkBuilder::open_default_sink() {
                Ok(mut d) => {
                    d.log_on_drop(false);
                    self.device = Some(d);
                }
                Err(e) => {
                    self.error = Some(trf("No audio output: {error}", &[("error", &e)]));
                    return;
                }
            }
        }
        let Some(device) = &self.device else { return };
        let source = match decoder(path, ext) {
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
        player.append(source);
        self.track = Some(Track { id: id.to_string(), path: path.to_path_buf(), ext: ext.to_string(), player });
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
        match decoder(&t.path, &t.ext) {
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
        let d = decoder(&p, "wav").unwrap();
        let total = d.total_duration().unwrap().as_secs_f64();
        assert!((total - 2.0).abs() < 0.01, "{total}");
        assert!(decoder(&dir.path().join("missing.wav"), "wav").is_err());
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
