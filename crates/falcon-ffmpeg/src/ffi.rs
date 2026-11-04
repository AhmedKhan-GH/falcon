//! The hand-written FFmpeg surface falcon uses, and the loader that opens
//! the shared libraries at run time.
//!
//! Only the functions in [`Api`] are called. Structs are declared as far as
//! the last field falcon reads, laid out as FFmpeg 7.1 declares them
//! (libavutil 59, libavcodec 61, libavformat 61, libswscale 8,
//! libswresample 5, libavfilter 10). The loader refuses libraries of any other major
//! version, since a struct prefix is only right for the version it was
//! copied from, and refuses any build whose licence is not the LGPL. The
//! `layout` test checks every offset against the installed headers.

use std::ffi::{CStr, c_char, c_double, c_int, c_uint, c_void};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Major versions of the libraries the structs below describe.
pub const AVUTIL_MAJOR: u32 = 59;
pub const AVCODEC_MAJOR: u32 = 61;
pub const AVFORMAT_MAJOR: u32 = 61;
pub const SWSCALE_MAJOR: u32 = 8;
pub const SWRESAMPLE_MAJOR: u32 = 5;
pub const AVFILTER_MAJOR: u32 = 10;

pub const AVMEDIA_TYPE_VIDEO: c_int = 0;
pub const AVMEDIA_TYPE_AUDIO: c_int = 1;
pub const AV_PIX_FMT_RGBA: c_int = 26;
pub const AV_SAMPLE_FMT_FLT: c_int = 3;
pub const AV_PKT_DATA_DISPLAYMATRIX: c_int = 5;
pub const AV_NOPTS_VALUE: i64 = i64::MIN;
pub const AV_TIME_BASE: i64 = 1_000_000;
pub const AVSEEK_FLAG_BACKWARD: c_int = 1;
pub const SWS_BILINEAR: c_int = 2;
pub const SWS_AREA: c_int = 0x20;
pub const AV_LOG_QUIET: c_int = -8;
/// `AVERROR(EAGAIN)`: EAGAIN is 35 on macOS and 11 on Linux.
#[cfg(target_os = "macos")]
pub const AVERROR_EAGAIN: c_int = -35;
#[cfg(not(target_os = "macos"))]
pub const AVERROR_EAGAIN: c_int = -11;
/// `AVERROR_EOF`, `-MKTAG('E','O','F',' ')`.
pub const AVERROR_EOF: c_int = -0x2046_4F45;
/// `AVERROR_INVALIDDATA`, `-MKTAG('I','N','D','A')`.
pub const AVERROR_INVALIDDATA: c_int = -0x4144_4E49;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AVRational {
    pub num: c_int,
    pub den: c_int,
}

impl AVRational {
    /// The value as a float, or `None` for 0/x, x/0 and negatives.
    pub fn value(self) -> Option<f64> {
        (self.num > 0 && self.den > 0).then(|| f64::from(self.num) / f64::from(self.den))
    }
}

#[repr(C)]
pub struct AVChannelLayout {
    pub order: c_int,
    pub nb_channels: c_int,
    pub u: u64,
    pub opaque: *mut c_void,
}

impl Default for AVChannelLayout {
    fn default() -> Self {
        AVChannelLayout { order: 0, nb_channels: 0, u: 0, opaque: std::ptr::null_mut() }
    }
}

/// The start of `AVFormatContext`, up to `duration`.
#[repr(C)]
pub struct AVFormatContext {
    pub av_class: *const c_void,
    pub iformat: *const c_void,
    pub oformat: *const c_void,
    pub priv_data: *mut c_void,
    pub pb: *mut c_void,
    pub ctx_flags: c_int,
    pub nb_streams: c_uint,
    pub streams: *mut *mut AVStream,
    pub nb_stream_groups: c_uint,
    pub stream_groups: *mut c_void,
    pub nb_chapters: c_uint,
    pub chapters: *mut c_void,
    pub url: *mut c_char,
    pub start_time: i64,
    pub duration: i64,
}

/// The start of `AVStream`, up to `avg_frame_rate`.
#[repr(C)]
pub struct AVStream {
    pub av_class: *const c_void,
    pub index: c_int,
    pub id: c_int,
    pub codecpar: *mut AVCodecParameters,
    pub priv_data: *mut c_void,
    pub time_base: AVRational,
    pub start_time: i64,
    pub duration: i64,
    pub nb_frames: i64,
    pub disposition: c_int,
    pub discard: c_int,
    pub sample_aspect_ratio: AVRational,
    pub metadata: *mut c_void,
    pub avg_frame_rate: AVRational,
}

/// The start of `AVCodecParameters`, up to `framerate`.
#[repr(C)]
pub struct AVCodecParameters {
    pub codec_type: c_int,
    pub codec_id: c_int,
    pub codec_tag: u32,
    pub extradata: *mut u8,
    pub extradata_size: c_int,
    pub coded_side_data: *mut AVPacketSideData,
    pub nb_coded_side_data: c_int,
    pub format: c_int,
    pub bit_rate: i64,
    pub bits_per_coded_sample: c_int,
    pub bits_per_raw_sample: c_int,
    pub profile: c_int,
    pub level: c_int,
    pub width: c_int,
    pub height: c_int,
    pub sample_aspect_ratio: AVRational,
    pub framerate: AVRational,
}

#[repr(C)]
pub struct AVPacketSideData {
    pub data: *mut u8,
    pub size: usize,
    pub kind: c_int,
}

/// The start of `AVPacket`, up to `stream_index`.
#[repr(C)]
pub struct AVPacket {
    pub buf: *mut c_void,
    pub pts: i64,
    pub dts: i64,
    pub data: *mut u8,
    pub size: c_int,
    pub stream_index: c_int,
}

/// `AVFrame` up to `ch_layout` (FFmpeg 7.1 still has the deprecated
/// fields `key_frame`, `interlaced_frame`, `top_field_first`,
/// `palette_has_changed`, `pkt_pos` and `pkt_size`).
#[repr(C)]
pub struct AVFrame {
    pub data: [*mut u8; 8],
    pub linesize: [c_int; 8],
    pub extended_data: *mut *mut u8,
    pub width: c_int,
    pub height: c_int,
    pub nb_samples: c_int,
    pub format: c_int,
    pub key_frame: c_int,
    pub pict_type: c_int,
    pub sample_aspect_ratio: AVRational,
    pub pts: i64,
    pub pkt_dts: i64,
    pub time_base: AVRational,
    pub quality: c_int,
    pub opaque: *mut c_void,
    pub repeat_pict: c_int,
    pub interlaced_frame: c_int,
    pub top_field_first: c_int,
    pub palette_has_changed: c_int,
    pub sample_rate: c_int,
    pub buf: [*mut c_void; 8],
    pub extended_buf: *mut *mut c_void,
    pub nb_extended_buf: c_int,
    pub side_data: *mut *mut c_void,
    pub nb_side_data: c_int,
    pub flags: c_int,
    pub color_range: c_int,
    pub color_primaries: c_int,
    pub color_trc: c_int,
    pub colorspace: c_int,
    pub chroma_location: c_int,
    pub best_effort_timestamp: i64,
    pub pkt_pos: i64,
    pub metadata: *mut c_void,
    pub decode_error_flags: c_int,
    pub pkt_size: c_int,
    pub hw_frames_ctx: *mut c_void,
    pub opaque_ref: *mut c_void,
    pub crop_top: usize,
    pub crop_bottom: usize,
    pub crop_left: usize,
    pub crop_right: usize,
    pub private_ref: *mut c_void,
    pub ch_layout: AVChannelLayout,
}

/// Opaque types, only ever behind pointers.
#[repr(C)]
pub struct AVCodec {
    _private: [u8; 0],
}
#[repr(C)]
pub struct AVCodecContext {
    _private: [u8; 0],
}
#[repr(C)]
pub struct SwsContext {
    _private: [u8; 0],
}
#[repr(C)]
pub struct SwrContext {
    _private: [u8; 0],
}
#[repr(C)]
pub struct AVFilter {
    _private: [u8; 0],
}
#[repr(C)]
pub struct AVFilterContext {
    _private: [u8; 0],
}
#[repr(C)]
pub struct AVFilterGraph {
    _private: [u8; 0],
}

/// Every FFmpeg function falcon calls, resolved from the loaded libraries.
/// The libraries stay loaded for the life of the process.
pub struct Api {
    pub version: String,
    pub dir: PathBuf,
    // libavutil
    pub av_frame_alloc: unsafe extern "C" fn() -> *mut AVFrame,
    pub av_frame_free: unsafe extern "C" fn(*mut *mut AVFrame),
    pub av_frame_unref: unsafe extern "C" fn(*mut AVFrame),
    pub av_frame_ref: unsafe extern "C" fn(*mut AVFrame, *const AVFrame) -> c_int,
    pub av_frame_get_buffer: unsafe extern "C" fn(*mut AVFrame, c_int) -> c_int,
    pub av_log_set_level: unsafe extern "C" fn(c_int),
    pub av_channel_layout_default: unsafe extern "C" fn(*mut AVChannelLayout, c_int),
    pub av_channel_layout_uninit: unsafe extern "C" fn(*mut AVChannelLayout),
    pub av_opt_set: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, c_int) -> c_int,
    pub av_display_rotation_get: unsafe extern "C" fn(*const i32) -> c_double,
    pub av_strerror: unsafe extern "C" fn(c_int, *mut c_char, usize) -> c_int,
    // libavcodec
    pub avcodec_alloc_context3: unsafe extern "C" fn(*const AVCodec) -> *mut AVCodecContext,
    pub avcodec_free_context: unsafe extern "C" fn(*mut *mut AVCodecContext),
    pub avcodec_parameters_to_context: unsafe extern "C" fn(*mut AVCodecContext, *const AVCodecParameters) -> c_int,
    pub avcodec_open2: unsafe extern "C" fn(*mut AVCodecContext, *const AVCodec, *mut *mut c_void) -> c_int,
    pub avcodec_send_packet: unsafe extern "C" fn(*mut AVCodecContext, *const AVPacket) -> c_int,
    pub avcodec_receive_frame: unsafe extern "C" fn(*mut AVCodecContext, *mut AVFrame) -> c_int,
    pub avcodec_flush_buffers: unsafe extern "C" fn(*mut AVCodecContext),
    pub avcodec_find_decoder_by_name: unsafe extern "C" fn(*const c_char) -> *const AVCodec,
    pub av_packet_alloc: unsafe extern "C" fn() -> *mut AVPacket,
    pub av_packet_free: unsafe extern "C" fn(*mut *mut AVPacket),
    pub av_packet_unref: unsafe extern "C" fn(*mut AVPacket),
    pub av_packet_side_data_get: unsafe extern "C" fn(*const AVPacketSideData, c_int, c_int) -> *const AVPacketSideData,
    // libavformat
    pub avformat_open_input: unsafe extern "C" fn(*mut *mut AVFormatContext, *const c_char, *const c_void, *mut *mut c_void) -> c_int,
    pub avformat_find_stream_info: unsafe extern "C" fn(*mut AVFormatContext, *mut *mut c_void) -> c_int,
    pub av_find_best_stream: unsafe extern "C" fn(*mut AVFormatContext, c_int, c_int, c_int, *mut *const AVCodec, c_int) -> c_int,
    pub av_read_frame: unsafe extern "C" fn(*mut AVFormatContext, *mut AVPacket) -> c_int,
    pub av_seek_frame: unsafe extern "C" fn(*mut AVFormatContext, c_int, i64, c_int) -> c_int,
    pub avformat_close_input: unsafe extern "C" fn(*mut *mut AVFormatContext),
    // libswscale
    pub sws_get_cached_context: unsafe extern "C" fn(
        *mut SwsContext,
        c_int,
        c_int,
        c_int,
        c_int,
        c_int,
        c_int,
        c_int,
        *mut c_void,
        *mut c_void,
        *const c_double,
    ) -> *mut SwsContext,
    pub sws_scale: unsafe extern "C" fn(*mut SwsContext, *const *const u8, *const c_int, c_int, c_int, *const *mut u8, *const c_int) -> c_int,
    pub sws_free_context: unsafe extern "C" fn(*mut SwsContext),
    // libswresample
    pub swr_alloc_set_opts2: unsafe extern "C" fn(
        *mut *mut SwrContext,
        *const AVChannelLayout,
        c_int,
        c_int,
        *const AVChannelLayout,
        c_int,
        c_int,
        c_int,
        *mut c_void,
    ) -> c_int,
    pub swr_init: unsafe extern "C" fn(*mut SwrContext) -> c_int,
    pub swr_convert: unsafe extern "C" fn(*mut SwrContext, *const *mut u8, c_int, *const *const u8, c_int) -> c_int,
    pub swr_get_out_samples: unsafe extern "C" fn(*mut SwrContext, c_int) -> c_int,
    pub swr_free: unsafe extern "C" fn(*mut *mut SwrContext),
    // libavfilter
    pub avfilter_graph_alloc: unsafe extern "C" fn() -> *mut AVFilterGraph,
    pub avfilter_graph_free: unsafe extern "C" fn(*mut *mut AVFilterGraph),
    pub avfilter_get_by_name: unsafe extern "C" fn(*const c_char) -> *const AVFilter,
    pub avfilter_graph_create_filter:
        unsafe extern "C" fn(*mut *mut AVFilterContext, *const AVFilter, *const c_char, *const c_char, *mut c_void, *mut AVFilterGraph) -> c_int,
    pub avfilter_link: unsafe extern "C" fn(*mut AVFilterContext, c_uint, *mut AVFilterContext, c_uint) -> c_int,
    pub avfilter_graph_config: unsafe extern "C" fn(*mut AVFilterGraph, *mut c_void) -> c_int,
    pub av_buffersrc_add_frame: unsafe extern "C" fn(*mut AVFilterContext, *mut AVFrame) -> c_int,
    pub av_buffersink_get_frame: unsafe extern "C" fn(*mut AVFilterContext, *mut AVFrame) -> c_int,
    /// Keeps the libraries loaded while the pointers above are in use.
    _libs: Vec<libloading::Library>,
}

/// Why FFmpeg could not be used.
#[derive(Clone, Debug, PartialEq)]
pub enum LoadError {
    /// No FFmpeg libraries were found in any of the places searched.
    Missing(Vec<PathBuf>),
    /// Libraries were found but can't be used (wrong version or licence,
    /// a missing symbol, a library that would not load).
    Unusable(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Missing(dirs) => {
                let dirs: Vec<String> = dirs.iter().map(|d| d.display().to_string()).collect();
                write!(f, "FFmpeg libraries not found (looked in {})", dirs.join(", "))
            }
            LoadError::Unusable(why) => write!(f, "FFmpeg can't be used: {why}"),
        }
    }
}

/// The file name of library `name` with major version `major`.
fn file_name(name: &str, major: u32) -> String {
    if cfg!(target_os = "macos") { format!("lib{name}.{major}.dylib") } else { format!("lib{name}.so.{major}") }
}

/// Where the libraries are looked for, in order: `$FALCON_FFMPEG_DIR`,
/// the app bundle's `Contents/Frameworks` next to the executable (Linux
/// packages: `lib/falcon` beside `bin`), and the workspace's `target/ffmpeg/lib` that `scripts/build-ffmpeg.sh` fills.
pub fn search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(d) = std::env::var_os("FALCON_FFMPEG_DIR") {
        dirs.push(PathBuf::from(d));
    }
    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        dirs.push(exe_dir.join(if cfg!(target_os = "macos") { "../Frameworks" } else { "../lib/falcon" }));
    }
    dirs.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ffmpeg/lib"));
    dirs
}

static API: OnceLock<Result<Api, LoadError>> = OnceLock::new();

/// The loaded FFmpeg, opened on first use.
pub fn api() -> Result<&'static Api, LoadError> {
    API.get_or_init(|| {
        let dirs = search_dirs();
        let Some(dir) = dirs.iter().find(|d| d.join(file_name("avformat", AVFORMAT_MAJOR)).is_file()) else {
            return Err(LoadError::Missing(dirs));
        };
        load(dir)
    })
    .as_ref()
    .map_err(Clone::clone)
}

fn unusable(msg: impl Into<String>) -> LoadError {
    LoadError::Unusable(msg.into())
}

/// Opens the six libraries in `dir` and resolves every function.
fn load(dir: &Path) -> Result<Api, LoadError> {
    let open = |name: &str, major: u32| -> Result<libloading::Library, LoadError> {
        let path = dir.join(file_name(name, major));
        // SAFETY: loading runs the library's initialisers; FFmpeg's do
        // nothing beyond setting up its own tables.
        unsafe { libloading::Library::new(&path) }.map_err(|e| unusable(format!("{}: {e}", path.display())))
    };
    // Dependencies first, so each library finds the ones it links.
    let avutil = open("avutil", AVUTIL_MAJOR)?;
    let swresample = open("swresample", SWRESAMPLE_MAJOR)?;
    let avfilter = open("avfilter", AVFILTER_MAJOR)?;
    let avcodec = open("avcodec", AVCODEC_MAJOR)?;
    let avformat = open("avformat", AVFORMAT_MAJOR)?;
    let swscale = open("swscale", SWSCALE_MAJOR)?;

    /// Resolves `name` in `lib` as the function pointer type expected.
    fn sym<T: Copy>(lib: &libloading::Library, name: &str) -> Result<T, LoadError> {
        let cname = format!("{name}\0");
        // SAFETY: each call site names the type FFmpeg 7.1 declares for the
        // symbol, and the major-version checks below reject other ABIs.
        unsafe { lib.get::<T>(cname.as_bytes()).map(|s| *s) }.map_err(|e| unusable(format!("{name}: {e}")))
    }

    type Version = unsafe extern "C" fn() -> c_uint;
    type License = unsafe extern "C" fn() -> *const c_char;
    let checks = [
        (&avutil, "avutil", AVUTIL_MAJOR),
        (&avcodec, "avcodec", AVCODEC_MAJOR),
        (&avformat, "avformat", AVFORMAT_MAJOR),
        (&swscale, "swscale", SWSCALE_MAJOR),
        (&swresample, "swresample", SWRESAMPLE_MAJOR),
        (&avfilter, "avfilter", AVFILTER_MAJOR),
    ];
    for (lib, name, major) in checks {
        let version: Version = sym(lib, &format!("{name}_version"))?;
        let license: License = sym(lib, &format!("{name}_license"))?;
        // SAFETY: both take nothing and return a constant.
        let (v, l) = unsafe { (version(), license()) };
        if v >> 16 != major {
            return Err(unusable(format!("lib{name} is version {}, falcon needs {major}", v >> 16)));
        }
        // SAFETY: FFmpeg returns a static NUL-terminated string.
        let l = if l.is_null() { String::new() } else { unsafe { CStr::from_ptr(l) }.to_string_lossy().into_owned() };
        if !license_ok(&l) {
            return Err(unusable(format!("lib{name} is licensed \"{l}\"; falcon only loads an LGPL build")));
        }
    }
    let info: License = sym(&avutil, "av_version_info")?;
    // SAFETY: as for the licence string.
    let version = unsafe { CStr::from_ptr(info()) }.to_string_lossy().into_owned();

    let api = Api {
        version,
        dir: dir.to_path_buf(),
        av_frame_alloc: sym(&avutil, "av_frame_alloc")?,
        av_frame_free: sym(&avutil, "av_frame_free")?,
        av_frame_unref: sym(&avutil, "av_frame_unref")?,
        av_frame_ref: sym(&avutil, "av_frame_ref")?,
        av_frame_get_buffer: sym(&avutil, "av_frame_get_buffer")?,
        av_log_set_level: sym(&avutil, "av_log_set_level")?,
        av_channel_layout_default: sym(&avutil, "av_channel_layout_default")?,
        av_channel_layout_uninit: sym(&avutil, "av_channel_layout_uninit")?,
        av_opt_set: sym(&avutil, "av_opt_set")?,
        av_display_rotation_get: sym(&avutil, "av_display_rotation_get")?,
        av_strerror: sym(&avutil, "av_strerror")?,
        avcodec_alloc_context3: sym(&avcodec, "avcodec_alloc_context3")?,
        avcodec_free_context: sym(&avcodec, "avcodec_free_context")?,
        avcodec_parameters_to_context: sym(&avcodec, "avcodec_parameters_to_context")?,
        avcodec_open2: sym(&avcodec, "avcodec_open2")?,
        avcodec_send_packet: sym(&avcodec, "avcodec_send_packet")?,
        avcodec_receive_frame: sym(&avcodec, "avcodec_receive_frame")?,
        avcodec_flush_buffers: sym(&avcodec, "avcodec_flush_buffers")?,
        avcodec_find_decoder_by_name: sym(&avcodec, "avcodec_find_decoder_by_name")?,
        av_packet_alloc: sym(&avcodec, "av_packet_alloc")?,
        av_packet_free: sym(&avcodec, "av_packet_free")?,
        av_packet_unref: sym(&avcodec, "av_packet_unref")?,
        av_packet_side_data_get: sym(&avcodec, "av_packet_side_data_get")?,
        avformat_open_input: sym(&avformat, "avformat_open_input")?,
        avformat_find_stream_info: sym(&avformat, "avformat_find_stream_info")?,
        av_find_best_stream: sym(&avformat, "av_find_best_stream")?,
        av_read_frame: sym(&avformat, "av_read_frame")?,
        av_seek_frame: sym(&avformat, "av_seek_frame")?,
        avformat_close_input: sym(&avformat, "avformat_close_input")?,
        sws_get_cached_context: sym(&swscale, "sws_getCachedContext")?,
        sws_scale: sym(&swscale, "sws_scale")?,
        sws_free_context: sym(&swscale, "sws_freeContext")?,
        swr_alloc_set_opts2: sym(&swresample, "swr_alloc_set_opts2")?,
        swr_init: sym(&swresample, "swr_init")?,
        swr_convert: sym(&swresample, "swr_convert")?,
        swr_get_out_samples: sym(&swresample, "swr_get_out_samples")?,
        swr_free: sym(&swresample, "swr_free")?,
        avfilter_graph_alloc: sym(&avfilter, "avfilter_graph_alloc")?,
        avfilter_graph_free: sym(&avfilter, "avfilter_graph_free")?,
        avfilter_get_by_name: sym(&avfilter, "avfilter_get_by_name")?,
        avfilter_graph_create_filter: sym(&avfilter, "avfilter_graph_create_filter")?,
        avfilter_link: sym(&avfilter, "avfilter_link")?,
        avfilter_graph_config: sym(&avfilter, "avfilter_graph_config")?,
        av_buffersrc_add_frame: sym(&avfilter, "av_buffersrc_add_frame")?,
        av_buffersink_get_frame: sym(&avfilter, "av_buffersink_get_frame")?,
        _libs: vec![swscale, avformat, avcodec, avfilter, swresample, avutil],
    };
    // SAFETY: plain value; FFmpeg's log goes to stderr otherwise.
    unsafe { (api.av_log_set_level)(AV_LOG_QUIET) };
    Ok(api)
}

/// Whether the loaded FFmpeg has the decoder called `name` (FFmpeg's
/// short name, such as `libdav1d` for AV1).
pub fn has_decoder(api: &Api, name: &str) -> bool {
    let Ok(name) = std::ffi::CString::new(name) else { return false };
    // SAFETY: a NUL-terminated name; FFmpeg returns a static codec or null.
    !unsafe { (api.avcodec_find_decoder_by_name)(name.as_ptr()) }.is_null()
}

/// Whether a library's licence string is one falcon may load: the LGPL,
/// never the GPL (`--enable-gpl`) or a nonfree build.
pub fn license_ok(license: &str) -> bool {
    let l = license.to_ascii_lowercase();
    l.starts_with("lgpl") && !l.contains("nonfree")
}

/// FFmpeg's message for an error code.
pub fn error_text(api: &Api, code: c_int) -> String {
    let mut buf = [0 as c_char; 128];
    // SAFETY: the buffer is writable for its whole length and FFmpeg
    // NUL-terminates what it writes.
    unsafe {
        (api.av_strerror)(code, buf.as_mut_ptr(), buf.len());
        CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_lgpl_builds_are_accepted() {
        assert!(license_ok("LGPL version 2.1 or later"));
        assert!(license_ok("LGPL version 3 or later"));
        assert!(!license_ok("GPL version 2 or later"));
        assert!(!license_ok("GPL version 3 or later"));
        assert!(!license_ok("nonfree and unredistributable"));
        assert!(!license_ok(""));
    }

    #[test]
    fn library_names_carry_the_major_version() {
        let n = file_name("avformat", 61);
        assert!(n == "libavformat.61.dylib" || n == "libavformat.so.61");
    }

    #[test]
    fn error_codes_match_ffmpeg() {
        // MKTAG('E','O','F',' ') = 'E' | 'O' << 8 | 'F' << 16 | ' ' << 24.
        let tag = i32::from(b'E') | i32::from(b'O') << 8 | i32::from(b'F') << 16 | i32::from(b' ') << 24;
        assert_eq!(AVERROR_EOF, -tag);
    }

    #[test]
    fn rationals() {
        assert_eq!(AVRational { num: 30000, den: 1001 }.value().map(|v| (v * 1000.0).round()), Some(29970.0));
        assert_eq!(AVRational { num: 0, den: 1 }.value(), None);
        assert_eq!(AVRational { num: 1, den: 0 }.value(), None);
    }
}
