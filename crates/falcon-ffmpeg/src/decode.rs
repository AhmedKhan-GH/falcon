//! Safe owners for the FFmpeg objects falcon uses: an opened file, its
//! packets, a decoder, a picture scaler and a sound resampler. Each frees
//! what it owns when dropped.
//!
//! The `unsafe` blocks here rest on the same few facts, named where used:
//!
//! - **owned**: the pointer was returned non-null by FFmpeg's allocator
//!   for this owner and is freed only by its `Drop`.
//! - **layout**: the struct prefix read matches FFmpeg 7.1, which the
//!   loader enforces by major version (see `ffi`).
//! - **frame**: an `AVFrame` FFmpeg just returned from
//!   `avcodec_receive_frame`, valid until the next call on that decoder.

use std::ffi::{CString, c_int};
use std::path::Path;
use std::ptr;

use crate::ffi::{self, AVCodec, AVCodecContext, AVFormatContext, AVFrame, AVPacket, AVRational, AVStream, Api, SwrContext, SwsContext};
use crate::sync;
use crate::{Error, Frame, Result};

fn check(api: &Api, code: c_int, what: &str) -> Result<c_int> {
    if code < 0 { Err(Error(format!("{what}: {}", ffi::error_text(api, code)))) } else { Ok(code) }
}

/// How far before a seek's time the keyframe search starts, in seconds:
/// more than half a frame at 2 fps and up.
const SEEK_MARGIN: f64 = 0.25;

/// An opened media file.
pub struct Input {
    api: &'static Api,
    ctx: *mut AVFormatContext,
}

// SAFETY: an AVFormatContext may move between threads; `Input` is not
// `Sync`, so it is only ever used from one thread at a time.
unsafe impl Send for Input {}

impl Input {
    pub fn open(path: &Path) -> Result<Input> {
        let api = ffi::api().map_err(|e| Error(e.to_string()))?;
        if !path.is_file() {
            return Err(Error(format!("no file at {}", path.display())));
        }
        // The `file:` prefix keeps a colon in the name from reading as a
        // protocol.
        let url = {
            use std::os::unix::ffi::OsStrExt;
            let mut bytes = b"file:".to_vec();
            bytes.extend_from_slice(path.as_os_str().as_bytes());
            CString::new(bytes).map_err(|_| Error("the path contains a NUL byte".into()))?
        };
        let mut ctx = ptr::null_mut();
        // SAFETY: `ctx` is a null out-pointer FFmpeg fills on success (and
        // frees itself on failure); the URL is NUL-terminated.
        check(api, unsafe { (api.avformat_open_input)(&mut ctx, url.as_ptr(), ptr::null(), ptr::null_mut()) }, "open")?;
        let input = Input { api, ctx };
        // SAFETY: owned.
        check(api, unsafe { (api.avformat_find_stream_info)(input.ctx, ptr::null_mut()) }, "read streams")?;
        Ok(input)
    }

    pub fn api(&self) -> &'static Api {
        self.api
    }

    pub fn stream(&self, index: i32) -> &AVStream {
        // SAFETY: owned, layout; callers pass indices FFmpeg returned for
        // this file, which are below `nb_streams`.
        unsafe { &**(*self.ctx).streams.add(index as usize) }
    }

    /// The file's start time and length in `AV_TIME_BASE` units.
    fn times(&self) -> (i64, i64) {
        // SAFETY: owned, layout.
        unsafe { ((*self.ctx).start_time, (*self.ctx).duration) }
    }

    /// The best stream of a kind and its decoder.
    pub fn best(&self, kind: c_int) -> Option<(i32, *const AVCodec)> {
        let mut codec = ptr::null();
        // SAFETY: owned; `codec` is an out-pointer.
        let i = unsafe { (self.api.av_find_best_stream)(self.ctx, kind, -1, -1, &mut codec, 0) };
        (i >= 0 && !codec.is_null()).then_some((i, codec))
    }

    /// Length in seconds: the container's, else the stream's.
    pub fn duration(&self, stream: Option<i32>) -> f64 {
        let (_, d) = self.times();
        if d != ffi::AV_NOPTS_VALUE && d > 0 {
            return d as f64 / ffi::AV_TIME_BASE as f64;
        }
        stream
            .and_then(|i| {
                let s = self.stream(i);
                (s.duration > 0).then(|| sync::secs(s.duration, s.time_base, 0)).flatten()
            })
            .unwrap_or(0.0)
    }

    /// Reads the next packet into `pkt`; false at the end of the file.
    pub fn read(&mut self, pkt: &mut Packet) -> Result<bool> {
        // SAFETY: owned (both).
        let r = unsafe { (self.api.av_read_frame)(self.ctx, pkt.0) };
        if r == ffi::AVERROR_EOF {
            return Ok(false);
        }
        check(self.api, r, "read").map(|_| true)
    }

    /// Moves to the keyframe at or before `t` seconds from the start.
    pub fn seek(&mut self, t: f64) -> Result<()> {
        let (start, _) = self.times();
        // FFmpeg rounds the time to the nearest frame of the stream, which
        // can land one frame after the frame showing at `t`; aiming a
        // little earlier makes sure the keyframe found is not after it.
        let ts = sync::seek_ts(t - SEEK_MARGIN, start);
        // SAFETY: owned; stream -1 takes AV_TIME_BASE units.
        let r = unsafe { (self.api.av_seek_frame)(self.ctx, -1, ts, ffi::AVSEEK_FLAG_BACKWARD) };
        check(self.api, r, "seek").map(|_| ())
    }
}

impl Drop for Input {
    fn drop(&mut self) {
        // SAFETY: owned; FFmpeg nulls the pointer.
        unsafe { (self.api.avformat_close_input)(&mut self.ctx) };
    }
}

/// A packet read from a file, owned until decoded or dropped.
pub struct Packet(*mut AVPacket, &'static Api);

// SAFETY: a packet's buffers are reference counted and not tied to a thread.
unsafe impl Send for Packet {}

impl Packet {
    pub fn new(api: &'static Api) -> Result<Packet> {
        // SAFETY: plain allocation.
        let p = unsafe { (api.av_packet_alloc)() };
        if p.is_null() { Err(Error("out of memory".into())) } else { Ok(Packet(p, api)) }
    }

    pub fn stream_index(&self) -> i32 {
        // SAFETY: owned, layout.
        unsafe { (*self.0).stream_index }
    }

    pub fn size(&self) -> usize {
        // SAFETY: owned, layout.
        unsafe { (*self.0).size.max(0) as usize }
    }

    pub fn unref(&mut self) {
        // SAFETY: owned.
        unsafe { (self.1.av_packet_unref)(self.0) };
    }
}

impl Drop for Packet {
    fn drop(&mut self) {
        // SAFETY: owned; frees the packet and its data reference.
        unsafe { (self.1.av_packet_free)(&mut self.0) };
    }
}

/// What a decoder learned about its stream.
#[derive(Clone, Copy, Debug)]
pub struct StreamInfo {
    pub index: i32,
    pub time_base: AVRational,
    pub start: i64,
    /// Frames per second (0 when unknown, for video).
    pub fps: f64,
    /// Shown size after the pixel aspect ratio, before turning (video).
    pub width: i32,
    pub height: i32,
    /// Clockwise quarter turns the stream asks for (video).
    pub turns: u8,
    /// Pixel aspect ratio (video; 0/1 when unknown).
    pub sar: AVRational,
}

/// A decoder for one stream, with a frame to receive into.
pub struct Decoder {
    api: &'static Api,
    ctx: *mut AVCodecContext,
    frame: *mut AVFrame,
    /// A second reference to a frame worth keeping (see [`Decoder::hold`]).
    held: *mut AVFrame,
    has_held: bool,
    pub info: StreamInfo,
}

// SAFETY: as for `Input`: moved between threads, never shared.
unsafe impl Send for Decoder {}

impl Decoder {
    /// Opens a decoder for `input`'s stream `index` with `codec`, letting
    /// FFmpeg pick the thread count.
    pub fn open(input: &Input, index: i32, codec: *const AVCodec) -> Result<Decoder> {
        let api = input.api;
        let stream = input.stream(index);
        // SAFETY: plain allocation; frees on failure are below.
        let (ctx, frame, held) = unsafe { ((api.avcodec_alloc_context3)(codec), (api.av_frame_alloc)(), (api.av_frame_alloc)()) };
        let mut dec = Decoder { api, ctx, frame, held, has_held: false, info: stream_info(api, stream) };
        if ctx.is_null() || frame.is_null() || held.is_null() {
            return Err(Error("out of memory".into()));
        }
        let tb = format!("{}/{}\0", stream.time_base.num, stream.time_base.den.max(1));
        // SAFETY: owned (`ctx`), layout (`codecpar` belongs to the stream
        // and outlives this call); option names and values are
        // NUL-terminated and the context starts with its AVClass.
        unsafe {
            check(api, (api.avcodec_parameters_to_context)(ctx, stream.codecpar), "decoder parameters")?;
            (api.av_opt_set)(ctx.cast(), c"threads".as_ptr(), c"auto".as_ptr(), 0);
            (api.av_opt_set)(ctx.cast(), c"pkt_timebase".as_ptr(), tb.as_ptr().cast(), 0);
            check(api, (api.avcodec_open2)(ctx, codec, ptr::null_mut()), "open decoder")?;
        }
        dec.info.index = index;
        Ok(dec)
    }

    /// Hands the decoder a packet, or `None` at the end of the stream to
    /// drain it. Call [`Decoder::receive`] until it returns `None` after each.
    pub fn send(&mut self, pkt: Option<&Packet>) -> Result<()> {
        let p = pkt.map_or(ptr::null(), |p| p.0.cast_const());
        // SAFETY: owned (both).
        let r = unsafe { (self.api.avcodec_send_packet)(self.ctx, p) };
        // EAGAIN can't happen when every send is drained; EOF means the
        // decoder was drained already. Bad packets are skipped.
        if r == ffi::AVERROR_EAGAIN || r == ffi::AVERROR_EOF || r == ffi::AVERROR_INVALIDDATA {
            return Ok(());
        }
        check(self.api, r, "decode").map(|_| ())
    }

    /// Decodes the next frame into [`Decoder::frame`]; false when the
    /// decoder wants more input (or has none left).
    pub fn receive(&mut self) -> bool {
        // SAFETY: owned (both); FFmpeg unreferences the frame before it
        // writes the next one.
        unsafe { (self.api.avcodec_receive_frame)(self.ctx, self.frame) >= 0 }
    }

    /// The frame last received.
    pub fn frame(&self) -> &AVFrame {
        // SAFETY: owned; after a successful `receive` this is a decoded
        // frame (frame), before one an empty frame of zeros.
        unsafe { &*self.frame }
    }

    /// Seconds of the frame last received, from the stream start.
    pub fn pts(&self, f: &AVFrame) -> Option<f64> {
        let ts = if f.best_effort_timestamp != ffi::AV_NOPTS_VALUE { f.best_effort_timestamp } else { f.pts };
        sync::secs(ts, self.info.time_base, self.info.start)
    }

    /// Keeps the frame last received (a new reference to its buffers, not
    /// a copy), replacing any frame kept before.
    pub fn hold(&mut self) {
        // SAFETY: owned (both frames); the source is a decoded frame.
        unsafe {
            (self.api.av_frame_unref)(self.held);
            self.has_held = (self.api.av_frame_ref)(self.held, self.frame) >= 0;
        }
    }

    /// The frame kept by [`Decoder::hold`], if any.
    pub fn held(&self) -> Option<&AVFrame> {
        // SAFETY: owned; set by a successful `av_frame_ref`.
        self.has_held.then(|| unsafe { &*self.held })
    }

    /// Forgets buffered input and any kept frame, after a seek.
    pub fn flush(&mut self) {
        // SAFETY: owned.
        unsafe {
            (self.api.avcodec_flush_buffers)(self.ctx);
            (self.api.av_frame_unref)(self.held);
        }
        self.has_held = false;
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        // SAFETY: owned; both calls accept null and null the pointer.
        unsafe {
            (self.api.av_frame_free)(&mut self.frame);
            (self.api.av_frame_free)(&mut self.held);
            (self.api.avcodec_free_context)(&mut self.ctx);
        }
    }
}

fn stream_info(api: &Api, s: &AVStream) -> StreamInfo {
    // SAFETY: layout; `codecpar` is always set on a demuxed stream.
    let par = unsafe { &*s.codecpar };
    let fps = s.avg_frame_rate.value().or(par.framerate.value()).unwrap_or(0.0);
    let sar = if s.sample_aspect_ratio.num > 0 { s.sample_aspect_ratio } else { par.sample_aspect_ratio };
    // SAFETY: layout; FFmpeg returns null or a side data entry whose data
    // is the nine int32 of a display matrix.
    let degrees = unsafe {
        let sd = (api.av_packet_side_data_get)(par.coded_side_data, par.nb_coded_side_data, ffi::AV_PKT_DATA_DISPLAYMATRIX);
        if sd.is_null() || (*sd).size < 36 { 0.0 } else { (api.av_display_rotation_get)((*sd).data.cast()) }
    };
    StreamInfo {
        index: s.index,
        time_base: s.time_base,
        start: s.start_time,
        fps,
        width: sync::display_width(par.width, sar),
        height: par.height,
        turns: sync::turns_from_degrees(degrees),
        sar,
    }
}

/// Converts decoded pictures to RGBA at a chosen size.
pub struct Scaler {
    api: &'static Api,
    ctx: *mut SwsContext,
}

// SAFETY: as for `Input`.
unsafe impl Send for Scaler {}

impl Scaler {
    pub fn new(api: &'static Api) -> Scaler {
        Scaler { api, ctx: ptr::null_mut() }
    }

    /// `f` as RGBA, scaled to fit `max_side` (after the pixel aspect ratio)
    /// and turned upright by `turns`.
    pub fn convert(&mut self, f: &AVFrame, info: &StreamInfo, max_side: u32) -> Option<Frame> {
        if f.width <= 0 || f.height <= 0 {
            return None;
        }
        let sar = if f.sample_aspect_ratio.num > 0 { f.sample_aspect_ratio } else { info.sar };
        let shown_w = sync::display_width(f.width, sar);
        let (w, h) = sync::fit(shown_w, f.height, max_side);
        let flags = if w < f.width / 2 { ffi::SWS_AREA } else { ffi::SWS_BILINEAR };
        // SAFETY: owned (the cached context, null at first); plain values.
        self.ctx = unsafe {
            (self.api.sws_get_cached_context)(self.ctx, f.width, f.height, f.format, w, h, ffi::AV_PIX_FMT_RGBA, flags, ptr::null_mut(), ptr::null_mut(), ptr::null())
        };
        if self.ctx.is_null() {
            return None;
        }
        let (ow, oh) = (w as usize, h as usize);
        let mut rgba = vec![0u8; ow * oh * 4];
        let dst = [rgba.as_mut_ptr(), ptr::null_mut(), ptr::null_mut(), ptr::null_mut()];
        let stride = [w * 4, 0, 0, 0];
        // SAFETY: frame (the source planes and strides); the destination is
        // one packed plane of exactly `h` rows of `w * 4` bytes.
        let rows = unsafe { (self.api.sws_scale)(self.ctx, f.data.as_ptr().cast(), f.linesize.as_ptr(), 0, f.height, dst.as_ptr(), stride.as_ptr()) };
        if rows <= 0 {
            return None;
        }
        // Video is opaque whatever the source's alpha.
        for px in rgba.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
        Some(Frame { width: ow, height: oh, rgba }.turned(info.turns))
    }
}

impl Drop for Scaler {
    fn drop(&mut self) {
        // SAFETY: owned; accepts null.
        unsafe { (self.api.sws_free_context)(self.ctx) };
    }
}

/// Channels and rate of the sound the player hands out: stereo, 32-bit
/// float, interleaved.
pub const OUT_CHANNELS: usize = 2;
pub const OUT_RATE: u32 = 48_000;

/// Converts decoded sound to [`OUT_CHANNELS`] float samples, resampled so
/// that it plays at `rate` times speed when output at [`OUT_RATE`].
pub struct Resampler {
    api: &'static Api,
    ctx: *mut SwrContext,
    /// What the context was made for: input format, rate, channels, channel
    /// mask, and the output rate.
    key: (c_int, c_int, c_int, u64, c_int),
}

// SAFETY: as for `Input`.
unsafe impl Send for Resampler {}

impl Resampler {
    pub fn new(api: &'static Api) -> Resampler {
        Resampler { api, ctx: ptr::null_mut(), key: (0, 0, 0, 0, 0) }
    }

    /// The output rate for playback at `rate` times speed.
    pub fn out_rate(rate: f64) -> c_int {
        (f64::from(OUT_RATE) / rate.clamp(0.1, 16.0)).round() as c_int
    }

    /// `f`'s samples as interleaved stereo floats for playback at `rate`.
    pub fn convert(&mut self, f: &AVFrame, rate: f64) -> Result<Vec<f32>> {
        let out_rate = Self::out_rate(rate);
        let key = (f.format, f.sample_rate, f.ch_layout.nb_channels, f.ch_layout.u, out_rate);
        if self.ctx.is_null() || key != self.key {
            self.reset();
            let mut out = ffi::AVChannelLayout::default();
            // SAFETY: `out` is a layout FFmpeg fills and we uninit; the
            // input layout is the frame's (frame); `ctx` is a null
            // out-pointer FFmpeg allocates.
            let r = unsafe {
                (self.api.av_channel_layout_default)(&mut out, OUT_CHANNELS as c_int);
                let r = (self.api.swr_alloc_set_opts2)(&mut self.ctx, &out, ffi::AV_SAMPLE_FMT_FLT, out_rate, &f.ch_layout, f.format, f.sample_rate, 0, ptr::null_mut());
                (self.api.av_channel_layout_uninit)(&mut out);
                if r < 0 { r } else { (self.api.swr_init)(self.ctx) }
            };
            check(self.api, r, "set up resampling")?;
            self.key = key;
        }
        // SAFETY: owned; plain count.
        let cap = unsafe { (self.api.swr_get_out_samples)(self.ctx, f.nb_samples) }.max(0) as usize + 32;
        let mut out = vec![0f32; cap * OUT_CHANNELS];
        let dst = [out.as_mut_ptr().cast::<u8>()];
        // SAFETY: frame (`extended_data` holds the input planes); the output
        // is one interleaved plane of `cap` sample frames.
        let n = unsafe { (self.api.swr_convert)(self.ctx, dst.as_ptr(), cap as c_int, f.extended_data.cast(), f.nb_samples) };
        let n = check(self.api, n, "resample")? as usize;
        out.truncate(n * OUT_CHANNELS);
        Ok(out)
    }

    /// Forgets the context (and anything it buffered), after a seek or a
    /// change of speed.
    pub fn reset(&mut self) {
        // SAFETY: owned; accepts null and nulls the pointer.
        unsafe { (self.api.swr_free)(&mut self.ctx) };
    }
}

impl Drop for Resampler {
    fn drop(&mut self) {
        self.reset();
    }
}
