# Video playback and the FFmpeg fallback

Off macOS there is no system player, so FFmpeg plays everything, and the Linux packages bundle it. On macOS, Falcon plays video through AVFoundation (`crates/falcon-video`). That covers mp4, mov and m4v with H.264, HEVC, ProRes and similar codecs. Everything AVFoundation refuses, such as Matroska, WebM, AVI, WMV, FLV, Ogg, MPEG-TS and RealMedia, falls back to FFmpeg (`crates/falcon-ffmpeg`). This mirrors Eagle, which falls back from Chromium's `<video>` element to a bundled mpv player.

FFmpeg is optional. Falcon never links it. The shared libraries are opened at run time with `dlopen` (through the `libloading` crate). Without them Falcon builds and runs, and only the fallback is missing: the viewer's can't-play page then says the FFmpeg libraries weren't found.

## What Eagle does

From `index.html`, `preview-window.html`, the `mediaElement` and `mpvMediaElement` directives in `app.bundle.js`, `js/thumbs/video.js` and `js/hover-preview/video-hover-preview.js`:

| Question | Eagle | Seen or inferred | Falcon |
|---|---|---|---|
| Video formats | `ts 3gp 360 afx vap eva mp4 mov m4v webm mkv avi wmv mpg mts flv m2ts f4v` open in the video viewer | Seen | Same list (`media::VIDEO_EXTENSIONS`). Other video files (ogv, rm, vob…) are plain items with a QuickLook thumbnail, as in Eagle |
| When to fall back | The native element errors, reports no size or no duration, or renders no frame within 1.5 s of playing; then `useMpvPlayer` swaps in `<mpv-video>` with the same controls | Seen | AVFoundation is tried first. A file it refuses on opening, that fails while loading, or of which it has drawn no picture 1.5 s after opening plays through FFmpeg in the same viewer, keeping volume, mute, speed, loop, place and play state. The time counts from opening, not from playing, since AVFoundation draws a poster picture while paused, and that picture counts. The switch happens once |
| Thumbnail at import | Native `<video>` first; on failure an `<mpv-video>` seeks to `min(10, duration / 3)` seconds and takes a 480-wide JPEG | Seen | QuickLook first for formats macOS reads; FFmpeg first for the others. The FFmpeg picture is taken at the same time and written as Eagle's 480-wide JPEG. When that picture is black, up to five later ones are tried (falcon's addition) |
| Hover preview | `<mpv-video>` for files the native element can't play, muted, looping, scrubbed by the pointer | Seen | The card plays the file through `falcon_video::Video` with small pictures, read off the main thread (`falcon_video::Prepared`), so FFmpeg plays these files there, muted or with their sound, as in the viewer |

## Building FFmpeg

`scripts/build-ffmpeg.sh` makes the libraries. On Linux it needs a C compiler, make, `nasm`, zlib's headers and `python3-venv`, and keeps x86 assembly on, since decoding is too slow without it. On macOS it needs clang, make, the Xcode command-line tools and a `python3` (3.15 here) to make a venv; no nasm, yasm, cmake or system pkg-config (arm64 needs no assembler beyond clang's). There it builds dav1d and FFmpeg for macOS 12, the oldest Falcon supports (`MACOSX_DEPLOYMENT_TARGET` sets another), and builds them again when that changes. Nothing goes outside `target/`. It:

1. makes a venv in `target/ffmpeg-tools/venv` and installs pinned wheels into it, each checked against its PyPI SHA-256 (`pip --require-hashes --only-binary :all:`): meson 1.12.1 and ninja 1.13.2 to build dav1d, and pkgconf 3.0.7.post0, through which FFmpeg's configure finds dav1d;
2. downloads `https://downloads.videolan.org/pub/videolan/dav1d/1.5.4/dav1d-1.5.4.tar.xz`, checks its SHA-256 (`686616b7c69eb88d44459391ab25cac13b6647a3b288835c5784e71c1514a5c5`, the hash VideoLAN publishes next to the tarball and the one MacPorts' dav1d Portfile records), and builds it with meson into `target/ffmpeg-tools/dav1d` (see below);
3. downloads `https://ffmpeg.org/releases/ffmpeg-7.1.5.tar.xz` into an empty build folder (`target/ffmpeg-build`, or `$FFMPEG_BUILD_DIR`);
4. checks its SHA-256, `de668509caf9e35e3cd162473441fdb29538c6d96ed080292b3cf9e6fc5d558f`. ffmpeg.org publishes only a GPG signature; this hash is the one MacPorts and Gentoo record for the signed tarball;
5. configures, builds and installs into `target/ffmpeg` (or `$FFMPEG_PREFIX`), never system-wide, and copies `libdav1d.7.dylib` and dav1d's licence (`COPYING.dav1d`) next to the FFmpeg libraries;
6. refuses to build if `config.h` shows `CONFIG_GPL 1` or `CONFIG_NONFREE 1`, if `CONFIG_GPL` isn't 0, if `libavfilter` or `atempo` isn't in or any other selectable filter is, if any encoder or muxer is on, or if the `libdav1d` decoder didn't make it into `config_components.h`, and writes `BUILD-INFO.txt` with the versions, sources, hashes and configure lines of FFmpeg and dav1d.

Running it again is cheap. The venv is remade only when a pinned tool version changes, dav1d is rebuilt only when it is missing or its pinned version changes, and FFmpeg only when it is missing, when dav1d was rebuilt, or when the script changes (a stamp in `target/ffmpeg/.stamp`). `FFMPEG_FORCE=1` rebuilds both.

The configure line, also recorded by FFmpeg itself as `FFMPEG_CONFIGURATION`:

```sh
./configure --prefix=<repo>/target/ffmpeg --install-name-dir=@rpath \
  --extra-ldflags=-Wl,-rpath,@loader_path --cc=clang --arch=arm64 \
  --enable-shared --disable-static --disable-programs --disable-doc --disable-debug \
  --disable-avdevice --disable-postproc --disable-filters --enable-filter=atempo \
  --disable-encoders --disable-muxers --disable-network --disable-autodetect \
  --enable-zlib --enable-libdav1d --pkg-config=pkgconf \
  --disable-x86asm --disable-protocols --enable-protocol=file \
  --disable-decoders --enable-decoder=h264,hevc,mpeg1video,mpeg2video,mpeg4,msmpeg4v1,msmpeg4v2,msmpeg4v3,wmv1,wmv2,wmv3,vc1,h263,h263i,h263p,flv,vp6,vp6a,vp6f,vp8,vp9,theora,rv10,rv20,rv30,rv40,mjpeg,mjpegb,dvvideo,prores,dnxhd,cinepak,huffyuv,ffvhuff,ffv1,rawvideo,indeo3,indeo4,indeo5,svq1,svq3,msvideo1,msrle,qtrle,utvideo,magicyuv,png,libdav1d,aac,aac_latm,mp1,mp1float,mp2,mp2float,mp3,mp3float,ac3,eac3,dca,truehd,mlp,vorbis,opus,flac,alac,wmav1,wmav2,wmapro,wmalossless,cook,ra_144,ra_288,sipr,atrac3,nellymoser,amrnb,amrwb,ape,adpcm_ms,adpcm_ima_wav,adpcm_ima_qt,adpcm_swf,gsm_ms,pcm_s8,pcm_u8,pcm_s16le,pcm_s16be,pcm_s24le,pcm_s24be,pcm_s32le,pcm_s32be,pcm_f32le,pcm_f32be,pcm_f64le,pcm_alaw,pcm_mulaw,pcm_bluray,pcm_dvd \
  --disable-demuxers --enable-demuxer=matroska,av1,obu,avi,asf,flv,live_flv,ogg,mpegts,mpegps,mpegvideo,mov,rm,h264,hevc,m4v,vc1,ivf,nut,dv,mxf,yuv4mpegpipe,aac,mp3,flac,wav,w64,aiff,ac3,eac3,dts,truehd,amr,ape
```

Parsers and bitstream filters keep FFmpeg's defaults, since the demuxers and decoders above need them (the AV1 parser and the `av1_frame_merge` and `av1_frame_split` filters among them). libswscale and libswresample are built, and libavfilter with one filter, `atempo` (its `abuffer` and `abuffersink` graph ends are always built, and no conversion filter is needed since the graph runs on one fixed format); libavdevice and libpostproc are not. libavfilter links only libavutil. Besides dav1d, the build links only macOS's own libz, CoreFoundation, CoreVideo and CoreMedia.

The libraries carry `@rpath` install names and an `@loader_path` rpath, so each finds its siblings whether it is opened from `target/ffmpeg/lib` or from the app bundle.

## AV1 and dav1d

AV1 is decoded by dav1d 1.5.4 (VideoLAN, BSD 2-Clause), FFmpeg's `libdav1d` decoder. FFmpeg's own `av1` decoder is left out: it only drives hardware decoders. The `matroska` (also WebM), `mov` (MP4) and `ivf` demuxers read AV1, and the `av1` (Annex B) and `obu` demuxers read raw streams.

dav1d is configured with

```sh
meson setup … --prefix=<repo>/target/ffmpeg-tools/dav1d --libdir=lib --buildtype=release \
  -Ddefault_library=shared -Denable_asm=true -Denable_tools=false -Denable_tests=false \
  -Denable_examples=false -Denable_docs=false
```

On arm64 its assembly is `.S` files that clang assembles, so nasm isn't needed. Meson gives the library the install name `@rpath/libdav1d.7.dylib`, like the FFmpeg libraries, and libavcodec links it by that name, so it is found through libavcodec's `@loader_path` rpath next to it, in `target/ffmpeg/lib` or in the bundle. dav1d is installed into its own folder first and copied into `target/ffmpeg/lib`, so rebuilding FFmpeg (which empties `target/ffmpeg`) leaves the dav1d build alone.

Which files reach dav1d: WebM and Matroska go to FFmpeg first. MP4 goes to AVFoundation first, which decodes AV1 on Macs with an AV1 decoder; when the player fails while loading, falcon switches to FFmpeg, and when `falcon_video::Grabber` opened a file through AVFoundation but can't decode its pictures, it switches to FFmpeg for that and every later picture (hover strips and the seek bar's thumbnail). At import, an MP4 still goes to QuickLook first, which reads AV1 on Macs with an AV1 decoder, and to FFmpeg when QuickLook fails.

## Licence

FFmpeg built this way is LGPL-2.1-or-later; dav1d is BSD 2-Clause, which the LGPL build may link (configure's licence summary still reads `LGPL version 2.1 or later`, and `CONFIG_GPL` and `CONFIG_NONFREE` stay 0). Falcon stays MIT/Apache-2.0: it contains no FFmpeg code, links nothing at build time, and loads unmodified shared libraries that a user can replace. The loader in `falcon-ffmpeg/src/ffi.rs` reads each library's `*_license()` string at run time and refuses any library that is not LGPL (a `--enable-gpl` or nonfree build), and any major version other than FFmpeg 7.1's (libavutil 59, libavcodec 61, libavformat 61, libswscale 8, libswresample 5, libavfilter 10). A build made before `atempo` was added has no libavfilter and is refused too; run the script again.

`scripts/bundle.sh` copies the six FFmpeg libraries and `libdav1d.7.dylib` into `Falcon.app/Contents/Frameworks`, sets their install names to `@rpath/…`, adds `@executable_path/../Frameworks` to the executable's rpaths and signs the bundle ad hoc again (`codesign -v --strict --deep` passes). The LGPL text, dav1d's licence (`COPYING.dav1d.txt`) and `BUILD-INFO.txt` go in `Contents/Resources/FFmpeg`, since code signing allows only code in `Frameworks`. Without `target/ffmpeg` the bundle builds as before. `scripts/package-macos.sh`, which makes the macOS download, refuses to go on without it, signs the bundle ad hoc again as a whole, and checks that each binary in it runs on macOS 12 and links only the system's libraries and the bundle's own. `NOTICE` and `ATTRIBUTION.md` record the versions, licences and sources of FFmpeg and dav1d.

## Where the libraries are looked for

In order: `$FALCON_FFMPEG_DIR`, the bundle's `Contents/Frameworks` next to the executable, and the workspace's `target/ffmpeg/lib`. The first folder holding `libavformat.61.dylib` is used. `falcon_video::ffmpeg_status()` reports the version and folder, or why FFmpeg can't be used.

## The bindings

`falcon-ffmpeg` has its own small FFI rather than `ffmpeg-sys`, so nothing about FFmpeg is needed at build time. dav1d is never opened directly; libavcodec loads it. `ffi::has_decoder` asks libavcodec for a decoder by name (`libdav1d`). It resolves about fifty functions (opening and reading files, decoding, seeking, `sws_scale`, `swr_convert`, the `atempo` graph and their helpers). Structs are declared only as far as the last field read, laid out as FFmpeg 7.1 declares them. The test `struct_layouts_match_the_installed_headers` compiles a C program against the installed headers and checks every offset and constant the bindings use.

## The player

`falcon_ffmpeg::Player` sits behind the same `falcon_video::Video` the viewer uses for AVFoundation, so play and pause, seeking, speed, looping, volume and mute, frame stepping, the current time and the duration drive one viewer.

- **Threads.** A demux thread reads packets, decodes and resamples the sound, stretches it at any speed but normal, and queues video packets. A video thread decodes pictures and converts them to RGBA with libswscale (after the pixel aspect ratio and any display-matrix rotation) into a queue of at most eight frames, fewer for large pictures (96 MB at most). The viewer takes the frame due each time it draws and uploads it as a texture.
- **Clock.** The sound goes to the app's rodio output as stereo 32-bit float at 48 kHz. The samples the output takes move the audio clock, which is the master clock while sound plays. With no sound track, no output, the sound muted, or the sound ended first, a wall clock runs the picture and takes over from where the audio clock stopped. Frames due are shown; late ones are skipped.
- **Mute.** Muting stops decoding the sound and switches to the wall clock; unmuting decodes it again from the playhead.
- **Speed.** The sound goes through FFmpeg's `atempo` filter (`falcon_ffmpeg::Tempo`), so it keeps its pitch at any speed, as `preservesPitch` does in Eagle's `<video>`. At normal speed the filter is skipped. `atempo` takes 0.5 and up, so slower speeds chain it (0.25 is 0.5 × 0.5). Each sample out covers `rate` samples of the clip, so the audio clock's arithmetic is the same as for resampled sound, and the time of what comes out is counted from the first sample in (`sync::TempoTimeline`), as the filter holds some back. The filter's last window at the very end of the clip may be cut short, about 20 ms of output. Should a replacement FFmpeg lack `atempo`, the sound is resampled to speed and its pitch moves with it.
- **Seeking.** A seek finds the keyframe before the time (aiming a quarter second early, since FFmpeg rounds to the nearest frame) and decodes forward to the frame on screen at that time. Every seek starts a new generation, and work for an older one is dropped. Seeking past the last frame shows the last frame.
- **Frame steps.** Forward steps take the next decoded frames; backward steps seek to the earlier frame.
- **Frame tools.** Set as thumbnail, copy and save frame decode the picture at the playhead again at full size, as with AVFoundation.

## Other uses

- `falcon_video::probe`, `frame_at` and `Grabber` fall back to FFmpeg, so the seek bar's hover thumbnail works for these formats, and `falcon_video::Prepared` does, so the grid's hover preview does too.
- `falcon_ffmpeg::AudioFile` decodes a file's sound alone. `falcon_library::audio::open`, the decoder behind audio playback, waveforms, durations and the tempo, falls back to it (`audio/ffmpeg.rs`) when symphonia can't read a file: the soundtrack of an mkv, webm, avi or wmv in the hover preview.
- At import, `media::duration` asks FFmpeg for the length of video it can't otherwise read, and FFmpeg draws the thumbnail as described above. Refresh Thumbnail goes the same way.

## Tests

- Unit tests cover the clocks, the frame queue, timestamp and seek arithmetic, rotation, black-frame detection and the licence check.
- `crates/falcon-ffmpeg/tests/ffmpeg.rs` runs only when `target/ffmpeg` exists (`build.rs` sets `ffmpeg_built`; otherwise the tests are ignored), and its AV1 tests only when that build includes `libdav1d.7.dylib` (`ffmpeg_dav1d`). It writes its own AVI clips byte by byte (uncompressed BGR pictures and PCM sound, since the build has no encoders) and checks probing, picture grabbing, thumbnails, playback, seeking, stepping, looping, the audio clock, muting, the struct layouts, and `atempo`: output lengths and the pitch of a tone from 0.1x to 16x, where its sound lands in time, and a 2x run through the player (half the samples, the clock at twice the wall clock, the pitch unchanged, a seek).
- AV1: `crates/falcon-ffmpeg/testdata/av1.ivf` (1.2 KB) is falcon's own clip, encoded with rav1e 0.8.1 from 25 flat 64 × 48 pictures whose colours count the frame, keyframes at 0 and 13. `falcon_ffmpeg::testing::av1` rewraps it as WebM, Matroska and MP4 (every element and box written by hand, `av1C` included), and the tests check that `libdav1d` is there, then probing, the first frame's colour, frame-accurate seeks across the keyframe, the import-time thumbnail, the grabber, and the player's seeks and steps in each container. falcon-library's import test and the AVFoundation test run the same clips through the import thumbnail and through `falcon_video` (player, grabber, `frame_at`).
- `crates/falcon-video/tests/avfoundation.rs` also decodes its AVFoundation-written H.264/AAC clips, and a rotated one, through FFmpeg and checks they match.
- `FALCON_VIDEO_SAMPLES=<folder> cargo test -p falcon-ffmpeg --test ffmpeg samples -- --ignored --nocapture` probes, thumbnails, plays and seeks every clip in a folder, for checks on real files.
