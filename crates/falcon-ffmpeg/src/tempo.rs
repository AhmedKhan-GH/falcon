//! Changing the speed of sound without changing its pitch, through FFmpeg's
//! `atempo` filter: Eagle's main player is a `<video>` element, whose
//! `preservesPitch` does the same.
//!
//! A [`Tempo`] is a small filter graph, `abuffer -> atempo -> abuffersink`,
//! taking and giving the player's stereo floats at 48 kHz, so no conversion
//! filter is needed. The filter holds some sound back and its own timestamps
//! count output time, so [`crate::sync::TempoTimeline`] says which part of
//! the clip what comes out belongs to. A graph can't be rewound: a seek
//! makes a new one.

use std::ffi::{CString, c_int, c_uint};
use std::ptr;

use crate::decode::{OUT_CHANNELS, OUT_RATE, check};
use crate::ffi::{self, AVERROR_EAGAIN, AVERROR_EOF, AVFilterContext, AVFilterGraph, AVFrame, Api};
use crate::sync::{self, TempoTimeline};
use crate::{Error, Result};

/// Sound going through `atempo` at one speed.
pub struct Tempo {
    api: &'static Api,
    graph: *mut AVFilterGraph,
    src: *mut AVFilterContext,
    sink: *mut AVFilterContext,
    frame: *mut AVFrame,
    rate: f64,
    timeline: TempoTimeline,
    /// Sample frames sent in, the timestamps the source wants.
    frames_in: i64,
}

impl Tempo {
    /// A graph for `rate` times speed.
    pub fn new(rate: f64) -> Result<Tempo> {
        let api = ffi::api().map_err(|e| Error(e.to_string()))?;
        Tempo::with_api(api, rate)
    }

    fn with_api(api: &'static Api, rate: f64) -> Result<Tempo> {
        // SAFETY: takes nothing, returns an owned frame or null.
        let frame = unsafe { (api.av_frame_alloc)() };
        // SAFETY: takes nothing, returns an owned graph or null.
        let graph = unsafe { (api.avfilter_graph_alloc)() };
        // Built first, so that its Drop frees both on every way out below.
        let mut tempo = Tempo {
            api,
            graph,
            src: ptr::null_mut(),
            sink: ptr::null_mut(),
            frame,
            rate,
            timeline: TempoTimeline::new(rate, OUT_RATE),
            frames_in: 0,
        };
        if frame.is_null() || graph.is_null() {
            return Err(Error("could not allocate the tempo filter".into()));
        }
        let source = format!("sample_rate={OUT_RATE}:sample_fmt=flt:channel_layout=stereo:time_base=1/{OUT_RATE}");
        tempo.src = tempo.filter("abuffer", "in", Some(&source))?;
        tempo.sink = tempo.filter("abuffersink", "out", None)?;
        let mut last = tempo.src;
        for (i, speed) in sync::tempo_chain(rate).into_iter().enumerate() {
            let next = tempo.filter("atempo", &format!("tempo{i}"), Some(&format!("tempo={speed}")))?;
            tempo.link(last, next)?;
            last = next;
        }
        tempo.link(last, tempo.sink)?;
        // SAFETY: owned; every filter in it is linked; a null log context is allowed.
        check(api, unsafe { (api.avfilter_graph_config)(tempo.graph, ptr::null_mut()) }, "set up the tempo filter")?;
        Ok(tempo)
    }

    /// Adds a filter to the graph.
    fn filter(&self, kind: &str, name: &str, args: Option<&str>) -> Result<*mut AVFilterContext> {
        let (kind, name) = (cstring(kind)?, cstring(name)?);
        let args = args.map(cstring).transpose()?;
        // SAFETY: a NUL-terminated name; FFmpeg returns a static filter or null.
        let filter = unsafe { (self.api.avfilter_get_by_name)(kind.as_ptr()) };
        if filter.is_null() {
            return Err(Error(format!("this FFmpeg has no {} filter", kind.to_string_lossy())));
        }
        let mut ctx = ptr::null_mut();
        // SAFETY: `ctx` is an out-pointer, the strings live through the call
        // and FFmpeg copies them; the graph is owned and takes the filter.
        let r = unsafe { (self.api.avfilter_graph_create_filter)(&mut ctx, filter, name.as_ptr(), args.as_ref().map_or(ptr::null(), |a| a.as_ptr()), ptr::null_mut(), self.graph) };
        check(self.api, r, "add a tempo filter")?;
        Ok(ctx)
    }

    fn link(&self, from: *mut AVFilterContext, to: *mut AVFilterContext) -> Result<()> {
        const FIRST_PAD: c_uint = 0;
        // SAFETY: both filters belong to the owned graph, each with one pad on that side.
        check(self.api, unsafe { (self.api.avfilter_link)(from, FIRST_PAD, to, FIRST_PAD) }, "link the tempo filter")?;
        Ok(())
    }

    /// The speed this graph was made for.
    pub fn rate(&self) -> f64 {
        self.rate
    }

    /// Sends in interleaved stereo floats at [`OUT_RATE`], the first of them
    /// at media time `pts` if it is known.
    pub fn push(&mut self, samples: &[f32], pts: Option<f64>) -> Result<()> {
        let frames = samples.len() / OUT_CHANNELS;
        if frames == 0 {
            return Ok(());
        }
        self.timeline.input(pts);
        let api = self.api;
        // SAFETY: the frame is owned and empty (the source moved the last
        // one out); `av_frame_get_buffer` allocates one interleaved plane of
        // `frames` stereo floats for the fields set here, which `samples`
        // fills; the source takes the reference and leaves the frame empty.
        let r = unsafe {
            let f = &mut *self.frame;
            f.format = ffi::AV_SAMPLE_FMT_FLT;
            f.sample_rate = OUT_RATE as c_int;
            f.nb_samples = frames as c_int;
            f.pts = self.frames_in;
            (api.av_channel_layout_default)(&mut f.ch_layout, OUT_CHANNELS as c_int);
            let r = (api.av_frame_get_buffer)(self.frame, 0);
            if r < 0 {
                (api.av_frame_unref)(self.frame);
                r
            } else {
                ptr::copy_nonoverlapping(samples.as_ptr(), f.data[0].cast::<f32>(), frames * OUT_CHANNELS);
                let r = (api.av_buffersrc_add_frame)(self.src, self.frame);
                (api.av_frame_unref)(self.frame);
                r
            }
        };
        check(api, r, "send sound to the tempo filter")?;
        self.frames_in += frames as i64;
        Ok(())
    }

    /// Says no more sound is coming, so [`Tempo::pull`] gives what is left.
    pub fn finish(&mut self) -> Result<()> {
        // SAFETY: owned; a null frame is how the source is told of the end.
        check(self.api, unsafe { (self.api.av_buffersrc_add_frame)(self.src, ptr::null_mut()) }, "finish the tempo filter")?;
        Ok(())
    }

    /// Appends all the sound ready to `out`, and returns the media time of
    /// the first sample appended (if any was and the time is known).
    pub fn pull(&mut self, out: &mut Vec<f32>) -> Option<f64> {
        let mut first = None;
        loop {
            // SAFETY: both owned; the sink fills the empty frame or leaves it empty.
            let r = unsafe { (self.api.av_buffersink_get_frame)(self.sink, self.frame) };
            if r < 0 {
                debug_assert!(r == AVERROR_EAGAIN || r == AVERROR_EOF, "{}", ffi::error_text(self.api, r));
                return first;
            }
            // SAFETY: owned, and just filled by the sink.
            let f = unsafe { &*self.frame };
            let frames = f.nb_samples.max(0) as usize;
            if f.format == ffi::AV_SAMPLE_FMT_FLT && frames > 0 {
                let time = self.timeline.next();
                // SAFETY: one interleaved plane of `frames` stereo floats,
                // the format the graph was set up with and just checked.
                let got = unsafe { std::slice::from_raw_parts(f.data[0].cast::<f32>(), frames * OUT_CHANNELS) };
                out.extend_from_slice(got);
                self.timeline.output(frames);
                first = first.or(time);
            }
            // SAFETY: owned.
            unsafe { (self.api.av_frame_unref)(self.frame) };
        }
    }
}

impl Drop for Tempo {
    fn drop(&mut self) {
        // SAFETY: owned; both accept a pointer to null and null it.
        unsafe {
            (self.api.av_frame_free)(&mut self.frame);
            (self.api.avfilter_graph_free)(&mut self.graph);
        }
    }
}

fn cstring(s: &str) -> Result<CString> {
    CString::new(s).map_err(|e| Error(e.to_string()))
}

/// The player's stretcher: the graph for the current speed, made when first
/// needed and forgotten at a seek. No graph at normal speed, or at a speed
/// the filter refused (a replacement FFmpeg without `atempo`, say); the
/// player resamples to speed then, as a pitch-moving last resort.
pub struct Stretch {
    api: &'static Api,
    tempo: Option<Tempo>,
    refused: Option<f64>,
}

impl Stretch {
    pub fn new(api: &'static Api) -> Stretch {
        Stretch { api, tempo: None, refused: None }
    }

    /// The graph to send sound at `rate` through, if it needs one.
    pub fn at(&mut self, rate: f64) -> Option<&mut Tempo> {
        if !sync::needs_tempo(rate) {
            return None;
        }
        if self.tempo.as_ref().is_some_and(|t| t.rate() != rate) {
            self.tempo = None;
        }
        if self.tempo.is_none() && self.refused != Some(rate) {
            match Tempo::with_api(self.api, rate) {
                Ok(t) => self.tempo = Some(t),
                Err(_) => self.refused = Some(rate),
            }
        }
        self.tempo.as_mut()
    }

    /// Forgets the graph and anything it held back, after a seek.
    pub fn reset(&mut self) {
        self.tempo = None;
    }
}
