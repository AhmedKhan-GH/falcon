//! Sound effects, after Eagle's (Preferences ▸ Notification ▸ Sound
//! effects): a short sound when items or folders are deleted, when a
//! screenshot goes into the library, and for the import events Eagle
//! sounds while the master switch is on (held duplicates, failed imports,
//! the auto-import notice).
//!
//! The sounds are falcon's own, synthesised here on first use from short
//! envelopes (no audio files ship). They play through the viewer's output
//! stream (`audio.rs`), mixed in without waiting; nothing here opens an
//! input device. As in Eagle, one debounce covers every sound: a call
//! within [`DEBOUNCE`] of the previous one stays silent and restarts the
//! wait, so a 200-item delete or a burst of deletes plays once.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Samples per second of the synthesised sounds.
pub const RATE: u32 = 44_100;

/// Eagle's `$.playSound` debounce: leading edge, 500 ms.
pub const DEBOUNCE: Duration = Duration::from_millis(500);

/// What a sound is played for.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Sound {
    /// Items moved to the Trash or taken out of a folder (Eagle's
    /// `deleteImage`).
    DeleteItem,
    /// Folders, smart folders or tags deleted, items deleted for good, the
    /// Trash emptied (Eagle's `deleteFolder`).
    DeleteFolder,
    /// A screenshot added to the library (Eagle's `screencapture`).
    Capture,
    /// Imported files held as duplicates (master switch only, as in Eagle).
    Duplicate,
    /// An import that failed (master switch only).
    ImportFailed,
    /// The auto-import notice (master switch only).
    Notice,
}

impl Sound {
    #[cfg(test)]
    pub const ALL: [Sound; 6] = [Sound::DeleteItem, Sound::DeleteFolder, Sound::Capture, Sound::Duplicate, Sound::ImportFailed, Sound::Notice];

    /// How long the sound lasts.
    pub fn duration(self) -> Duration {
        Duration::from_millis(match self {
            Sound::DeleteItem => 180,
            Sound::DeleteFolder => 240,
            Sound::Capture => 150,
            Sound::Duplicate => 220,
            Sound::ImportFailed => 260,
            Sound::Notice => 300,
        })
    }

    /// The number of samples the sound has at [`RATE`].
    pub fn sample_count(self) -> usize {
        (self.duration().as_secs_f64() * f64::from(RATE)).round() as usize
    }
}

/// Eagle's `notification.soundEffect`: the master switch and one switch
/// per event, all on by default. Saved with the other preferences.
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(default)]
pub struct SoundPrefs {
    /// `soundEffect.enable`.
    pub enabled: bool,
    /// `when.deleteImage`.
    pub delete_item: bool,
    /// `when.deleteFolder`.
    pub delete_folder: bool,
    /// `when.screencapture`.
    pub capture: bool,
    /// `when.extension`. falcon has no browser extension, so nothing plays
    /// for it; it is kept for the API's preferences report.
    pub extension: bool,
}

impl Default for SoundPrefs {
    fn default() -> Self {
        SoundPrefs { enabled: true, delete_item: true, delete_folder: true, capture: true, extension: true }
    }
}

impl SoundPrefs {
    /// Whether `sound` may play: the master switch, and the event's own
    /// switch where Eagle has one.
    pub fn allows(&self, sound: Sound) -> bool {
        self.enabled
            && match sound {
                Sound::DeleteItem => self.delete_item,
                Sound::DeleteFolder => self.delete_folder,
                Sound::Capture => self.capture,
                Sound::Duplicate | Sound::ImportFailed | Sound::Notice => true,
            }
    }
}

/// Where sounds go: the audio output in the app, a recorder in tests.
pub trait Output {
    /// Starts mono `samples` at `rate`; returns at once.
    fn play(&mut self, rate: u32, samples: &Arc<[f32]>);
}

impl Output for crate::audio::Audio {
    fn play(&mut self, rate: u32, samples: &Arc<[f32]>) {
        if let Err(e) = self.play_clip(rate, samples) {
            crate::errlog::error(&format!("sound effect: {e}"));
        }
    }
}

/// The sound effects' state: the synthesised sounds and the debounce.
#[derive(Default)]
pub struct SoundState {
    clips: HashMap<Sound, Arc<[f32]>>,
    /// The last call that passed the switches.
    last: Option<Instant>,
}

impl SoundState {
    /// Plays `sound` on `out` if `prefs` allow it and no other sound was
    /// asked for within [`DEBOUNCE`]. Returns whether it played.
    pub fn play(&mut self, sound: Sound, prefs: &SoundPrefs, now: Instant, out: &mut dyn Output) -> bool {
        if !prefs.allows(sound) {
            return false;
        }
        let quiet = self.last.is_some_and(|t| now.saturating_duration_since(t) < DEBOUNCE);
        self.last = Some(now);
        if quiet {
            return false;
        }
        let clip = self.clips.entry(sound).or_insert_with(|| synth(sound).into());
        out.play(RATE, clip);
        true
    }
}

/// A small deterministic noise source (xorshift32), so each sound is the
/// same every time.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// An envelope rising over `attack` seconds and then decaying
/// exponentially with time constant `decay`, at time `t` from its start.
fn env(t: f32, attack: f32, decay: f32) -> f32 {
    if t < 0.0 {
        0.0
    } else if t < attack {
        t / attack
    } else {
        (-(t - attack) / decay).exp()
    }
}

/// A sine tone with a little second harmonic, softer than a pure sine.
fn tone(t: f32, hz: f32) -> f32 {
    let w = std::f32::consts::TAU * hz * t;
    w.sin() + 0.25 * (2.0 * w).sin()
}

/// Synthesises `sound`: [`Sound::sample_count`] mono samples at [`RATE`], peaking
/// at a moderate level, faded to silence at the end.
pub fn synth(sound: Sound) -> Vec<f32> {
    let n = sound.sample_count();
    let dt = 1.0 / RATE as f32;
    let mut noise = Noise(0x2F6E_2B1D);
    let mut out = Vec::with_capacity(n);
    // One-pole filter states for the noise sounds.
    let (mut lp, mut lp2) = (0.0f32, 0.0f32);
    for i in 0..n {
        let t = i as f32 * dt;
        let s = match sound {
            // A soft whoosh: noise through a low-pass that closes as it fades.
            Sound::DeleteItem => {
                let cutoff = 3200.0 * (1.0 - t / 0.18).max(0.05);
                let a = 1.0 - (-std::f32::consts::TAU * cutoff * dt).exp();
                lp += a * (noise.next() - lp);
                lp2 += a * (lp - lp2);
                lp2 * env(t, 0.03, 0.05) * 3.0
            }
            // Two lower tones, the second a fifth below the first.
            Sound::DeleteFolder => tone(t, 392.0) * env(t, 0.006, 0.035) + tone(t, 261.6) * env(t - 0.1, 0.006, 0.05),
            // A shutter: two short bright noise clicks.
            Sound::Capture => {
                let x = noise.next();
                lp += 0.35 * (x - lp);
                let bright = x - lp;
                bright * (env(t, 0.002, 0.008) + 0.8 * env(t - 0.07, 0.002, 0.012))
            }
            // Two equal short blips.
            Sound::Duplicate => tone(t, 660.0) * (env(t, 0.005, 0.025) + env(t - 0.11, 0.005, 0.03)),
            // A low, slightly rough tone.
            Sound::ImportFailed => {
                let w = std::f32::consts::TAU * 196.0 * t;
                (w.sin() + 0.4 * (3.0 * w).sin() + 0.2 * (5.0 * w).sin()) * env(t, 0.01, 0.08)
            }
            // A rising pair of notes.
            Sound::Notice => tone(t, 523.3) * env(t, 0.006, 0.05) + tone(t, 784.0) * env(t - 0.09, 0.006, 0.07),
        };
        out.push(s);
    }
    // Peak at 0.35 and fade the last 10 ms out, so nothing clicks.
    let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if peak > 0.0 {
        let fade = (RATE as usize / 100).min(n);
        for (i, s) in out.iter_mut().enumerate() {
            *s *= 0.35 / peak;
            let left = n - i;
            if left <= fade {
                *s *= (left - 1) as f32 / fade as f32;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Recorder(Vec<(u32, usize)>);

    impl Output for Recorder {
        fn play(&mut self, rate: u32, samples: &Arc<[f32]>) {
            self.0.push((rate, samples.len()));
        }
    }

    #[test]
    fn defaults_match_eagle() {
        let p = SoundPrefs::default();
        assert!(p.enabled && p.delete_item && p.delete_folder && p.capture && p.extension);
        assert!(Sound::ALL.iter().all(|s| p.allows(*s)));
    }

    #[test]
    fn prefs_round_trip() {
        let p = SoundPrefs { enabled: true, delete_item: false, delete_folder: true, capture: false, extension: false };
        let back: SoundPrefs = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p);
        // Missing fields fall back to on.
        let partial: SoundPrefs = serde_json::from_str(r#"{"capture":false}"#).unwrap();
        assert_eq!(partial, SoundPrefs { capture: false, ..Default::default() });
    }

    #[test]
    fn master_off_blocks_every_sound() {
        let p = SoundPrefs { enabled: false, ..Default::default() };
        let (mut st, mut rec) = (SoundState::default(), Recorder::default());
        let t0 = Instant::now();
        for (i, s) in Sound::ALL.iter().enumerate() {
            assert!(!p.allows(*s));
            assert!(!st.play(*s, &p, t0 + Duration::from_secs(i as u64 + 1), &mut rec));
        }
        assert!(rec.0.is_empty());
    }

    #[test]
    fn event_switches() {
        let p = SoundPrefs { delete_item: false, delete_folder: false, capture: false, ..Default::default() };
        assert!(!p.allows(Sound::DeleteItem) && !p.allows(Sound::DeleteFolder) && !p.allows(Sound::Capture));
        // The import sounds follow only the master switch, as in Eagle.
        assert!(p.allows(Sound::Duplicate) && p.allows(Sound::ImportFailed) && p.allows(Sound::Notice));
    }

    #[test]
    fn debounce() {
        let p = SoundPrefs::default();
        let (mut st, mut rec) = (SoundState::default(), Recorder::default());
        let t0 = Instant::now();
        assert!(st.play(Sound::DeleteItem, &p, t0, &mut rec));
        // A burst of calls 100 ms apart keeps restarting the wait.
        for i in 1..=10 {
            assert!(!st.play(Sound::DeleteFolder, &p, t0 + Duration::from_millis(100 * i), &mut rec));
        }
        // 500 ms after the last call it plays again.
        assert!(st.play(Sound::Capture, &p, t0 + Duration::from_millis(1500), &mut rec));
        assert_eq!(rec.0, vec![(RATE, Sound::DeleteItem.sample_count()), (RATE, Sound::Capture.sample_count())]);
        // A call refused by the switches doesn't count towards the wait.
        let off = SoundPrefs { capture: false, ..Default::default() };
        assert!(!st.play(Sound::Capture, &off, t0 + Duration::from_millis(2100), &mut rec));
        assert!(st.play(Sound::DeleteItem, &p, t0 + Duration::from_millis(2200), &mut rec));
    }

    #[test]
    fn synthesis_is_finite_audible_and_sized() {
        for s in Sound::ALL {
            let pcm = synth(s);
            assert_eq!(pcm.len(), s.sample_count(), "{s:?}");
            assert!(pcm.iter().all(|x| x.is_finite() && x.abs() <= 0.35 + 1e-6), "{s:?}");
            let rms = (pcm.iter().map(|x| x * x).sum::<f32>() / pcm.len() as f32).sqrt();
            assert!(rms > 0.01, "{s:?} is silent ({rms})");
            assert_eq!(*pcm.last().unwrap(), 0.0, "{s:?} ends at silence");
            assert_eq!(pcm, synth(s), "{s:?} is the same every time");
        }
        // Each sound differs from the others.
        let all: Vec<Vec<f32>> = Sound::ALL.iter().map(|s| synth(*s)).collect();
        for i in 0..all.len() {
            for j in i + 1..all.len() {
                assert_ne!(all[i], all[j]);
            }
        }
    }
}
