//! Platforms without AVFoundation: every call fails cleanly.

use std::path::Path;

use crate::{Error, Frame, Info, Result};

pub fn on_main_thread() -> bool {
    true
}

fn unsupported<T>() -> Result<T> {
    Err(Error("video playback needs macOS".into()))
}

pub fn probe(_path: &Path) -> Result<Info> {
    unsupported()
}

pub fn frame_at(_path: &Path, _secs: f64) -> Result<Frame> {
    unsupported()
}

/// Never constructed off macOS.
pub struct Grabber;

impl Grabber {
    pub fn open(_path: &Path, _max_side: u32) -> Result<Grabber> {
        unsupported()
    }
    pub fn grab(&self, _secs: f64) -> Result<Frame> {
        unsupported()
    }
}

/// Never constructed off macOS.
pub struct Prepared;

impl Prepared {
    pub fn new(_path: &Path) -> Result<Prepared> {
        unsupported()
    }
}

/// Never constructed off macOS.
pub struct Video {
    info: Info,
}

impl Video {
    pub fn open(_path: &Path) -> Result<Video> {
        unsupported()
    }
    pub fn start(_prepared: Prepared, _max_side: Option<u32>) -> Result<Video> {
        unsupported()
    }
    pub fn info(&self) -> &Info {
        &self.info
    }
    pub fn is_ready(&self) -> bool {
        false
    }
    pub fn error(&self) -> Option<String> {
        None
    }
    pub fn play(&mut self) {}
    pub fn pause(&mut self) {}
    pub fn toggle(&mut self) {}
    pub fn is_playing(&self) -> bool {
        false
    }
    pub fn seek(&mut self, _secs: f64) {}
    pub fn step(&mut self, _frames: i32) {}
    pub fn rate(&self) -> f32 {
        1.0
    }
    pub fn set_rate(&mut self, _rate: f32) {}
    pub fn set_volume(&mut self, _volume: f32) {}
    pub fn set_muted(&mut self, _muted: bool) {}
    pub fn set_looping(&mut self, _looping: bool) {}
    pub fn looping(&self) -> bool {
        false
    }
    pub fn current_time(&self) -> f64 {
        0.0
    }
    pub fn tick(&mut self) {}
    pub fn frame(&mut self) -> Option<Frame> {
        None
    }
    pub fn current_frame(&self) -> Result<Frame> {
        unsupported()
    }
}
