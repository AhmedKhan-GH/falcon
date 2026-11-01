//! Reading a file's facts and single pictures: what the viewer's probe,
//! the seek bar and hover previews, and the import thumbnail need.

use std::path::Path;

use crate::decode::{Decoder, Input, Packet, Scaler};
use crate::ffi;
use crate::sync;
use crate::{Error, Frame, Info, Result};

/// The facts of a file with a video stream FFmpeg can decode.
pub fn probe(path: &Path) -> Result<Info> {
    let input = Input::open(path)?;
    let (index, codec) = input.best(ffi::AVMEDIA_TYPE_VIDEO).ok_or_else(|| Error("no video stream FFmpeg can decode".into()))?;
    let video = Decoder::open(&input, index, codec)?;
    Ok(info(&input, &video))
}

pub(crate) fn info(input: &Input, video: &Decoder) -> Info {
    let s = &video.info;
    let (width, height) = if s.turns % 2 == 1 { (s.height, s.width) } else { (s.width, s.height) };
    Info {
        duration: input.duration(Some(s.index)),
        width: width.max(0) as u32,
        height: height.max(0) as u32,
        fps: s.fps as f32,
        has_audio: input.best(ffi::AVMEDIA_TYPE_AUDIO).is_some(),
        turns: s.turns,
    }
}

/// Decodes single pictures of one file, scaled to fit a size: for the seek
/// bar's hover thumbnail, the grid's hover previews and thumbnails.
/// Usable from any thread.
pub struct Grabber {
    input: Input,
    video: Decoder,
    scaler: Scaler,
    packet: Packet,
    max_side: u32,
    info: Info,
    /// The time of the last picture decoded, to decode on from it rather
    /// than seek when the next request is just ahead.
    last: Option<f64>,
}

/// How far ahead of the last picture a request is decoded to without
/// seeking, in seconds.
const DECODE_ON: f64 = 1.0;
/// Packets read for one picture before giving up.
const MAX_PACKETS: usize = 5000;

impl Grabber {
    /// Opens `path`; pictures are scaled so their long side is at most
    /// `max_side` pixels.
    pub fn open(path: &Path, max_side: u32) -> Result<Grabber> {
        let input = Input::open(path)?;
        let (index, codec) = input.best(ffi::AVMEDIA_TYPE_VIDEO).ok_or_else(|| Error("no video stream FFmpeg can decode".into()))?;
        let video = Decoder::open(&input, index, codec)?;
        let info = info(&input, &video);
        let (scaler, packet) = (Scaler::new(input.api()), Packet::new(input.api())?);
        Ok(Grabber { input, video, scaler, packet, max_side, info, last: None })
    }

    pub fn info(&self) -> &Info {
        &self.info
    }

    /// The picture at `secs`: the first frame at or after it, decoded on
    /// from the keyframe before it; the last frame when `secs` is past the
    /// end.
    pub fn grab(&mut self, secs: f64) -> Result<Frame> {
        let secs = secs.max(0.0);
        let on = self.last.is_some_and(|l| secs >= l && secs - l < DECODE_ON);
        if !on {
            self.input.seek(secs)?;
            self.video.flush();
        }
        let fps = self.video.info.fps;
        let index = self.video.info.index;
        let mut ended = false;
        let mut held = false;
        for _ in 0..MAX_PACKETS {
            if !ended {
                self.packet.unref();
                if self.input.read(&mut self.packet)? {
                    if self.packet.stream_index() != index {
                        continue;
                    }
                    self.video.send(Some(&self.packet))?;
                } else {
                    ended = true;
                    self.video.send(None)?;
                }
            }
            let mut got = false;
            while self.video.receive() {
                got = true;
                let f = self.video.frame();
                let pts = self.video.pts(f);
                if pts.is_some_and(|t| sync::before_target(t, secs, fps)) {
                    self.video.hold();
                    held = true;
                    continue;
                }
                let frame = self.scaler.convert(f, &self.video.info, self.max_side).ok_or_else(|| Error("could not convert the picture".into()))?;
                self.last = pts;
                return Ok(frame);
            }
            if ended && !got {
                break;
            }
        }
        // Past the end: the last frame decoded. The decoder is drained, so
        // the next request seeks.
        self.last = None;
        if held && let Some(f) = self.video.held() {
            return self.scaler.convert(f, &self.video.info, self.max_side).ok_or_else(|| Error("could not convert the picture".into()));
        }
        Err(Error(format!("no picture at {secs:.2} s")))
    }
}

/// The picture at `secs` at full size.
pub fn frame_at(path: &Path, secs: f64) -> Result<Frame> {
    Grabber::open(path, u32::MAX)?.grab(secs)
}

/// How many later pictures are tried when the thumbnail's picture is black.
const BLACK_RETRIES: usize = 5;

/// An import thumbnail: the picture where Eagle takes one
/// ([`sync::thumbnail_time`]), or the first of a few later ones that is not
/// black, at full size, with the file's facts and the time used.
pub fn thumbnail(path: &Path) -> Result<(Info, Frame, f64)> {
    let mut g = Grabber::open(path, u32::MAX)?;
    let info = g.info().clone();
    let first_t = sync::thumbnail_time(info.duration);
    let first = g.grab(first_t)?;
    if !first.is_black() || info.duration <= 0.0 {
        return Ok((info, first, first_t));
    }
    let step = (info.duration - first_t) / (BLACK_RETRIES + 1) as f64;
    for k in 1..=BLACK_RETRIES {
        let t = first_t + step * k as f64;
        if let Ok(f) = g.grab(t)
            && !f.is_black()
        {
            return Ok((info, f, t));
        }
    }
    Ok((info, first, first_t))
}
