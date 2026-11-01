# Video playback and the FFmpeg fallback

Falcon plays video through macOS's AVFoundation (`crates/falcon-video`). That covers mp4, mov and m4v with H.264, HEVC, ProRes and similar codecs. Everything AVFoundation refuses, such as Matroska, WebM, AVI, WMV, FLV, Ogg, MPEG-TS and RealMedia, falls back to FFmpeg (`crates/falcon-ffmpeg`). This mirrors Eagle, which falls back from Chromium's `<video>` element to a bundled mpv player.

FFmpeg is optional. Falcon never links it. The shared libraries are opened at run time with `dlopen` (through the `libloading` crate). Without them Falcon builds and runs, and only the fallback is missing: the viewer's can't-play page then says the FFmpeg libraries weren't found.

## What Eagle does

From `index.html`, `preview-window.html`, the `mediaElement` and `mpvMediaElement` directives in `app.bundle.js`, `js/thumbs/video.js` and `js/hover-preview/video-hover-preview.js`:

| Question | Eagle | Seen or inferred | Falcon |
|---|---|---|---|
| Video formats | `ts 3gp 360 afx vap eva mp4 mov m4v webm mkv avi wmv mpg mts flv m2ts f4v` open in the video viewer | Seen | Same list (`media::VIDEO_EXTENSIONS`). FFmpeg also takes ogv, mpeg, vob, asf, rm, rmvb, divx, mxf and nut when they are opened |
| When to fall back | The native element errors, reports no size or no duration, or renders no frame within 1.5 s of playing; then `useMpvPlayer` swaps in `<mpv-video>` with the same controls | Seen | AVFoundation is tried first. A file it refuses on opening, or that fails while loading, plays through FFmpeg in the same viewer, keeping volume, mute, speed, loop, place and play state. The 1.5 s no-frame check is not copied |
| Thumbnail at import | Native `<video>` first; on failure an `<mpv-video>` seeks to `min(10, duration / 3)` seconds and takes a 480-wide JPEG | Seen | QuickLook first for formats macOS reads; FFmpeg first for the others. The FFmpeg picture is taken at the same time and written as Eagle's 480-wide JPEG. When that picture is black, up to five later ones are tried (falcon's addition) |
| Hover preview | `<mpv-video>` for files the native element can't play, muted, looping, scrubbed by the pointer | Seen | The strip of pictures comes from `falcon_video::Grabber`, which falls back to FFmpeg; unmuted sound comes from the hover player through falcon's shared audio decoder, which falls back to FFmpeg when symphonia can't read the file |

## Building FFmpeg

`scripts/build-ffmpeg.sh` makes the libraries. It needs only clang, make and the Xcode command-line tools (no nasm, yasm, pkg-config or cmake; arm64 needs no assembler beyond clang's). It:

1. downloads `https://ffmpeg.org/releases/ffmpeg-7.1.5.tar.xz` into an empty build folder (`target/ffmpeg-build`, or `$FFMPEG_BUILD_DIR`);
2. checks its SHA-256, `de668509caf9e35e3cd162473441fdb29538c6d96ed080292b3cf9e6fc5d558f`. ffmpeg.org publishes only a GPG signature; this hash is the one MacPorts and Gentoo record for the signed tarball;
3. configures, builds and installs into `target/ffmpeg` (or `$FFMPEG_PREFIX`), never system-wide;
4. refuses to build if `config.h` shows `CONFIG_GPL 1` or `CONFIG_NONFREE 1`, and writes `BUILD-INFO.txt` with the version, source, hash and configure line.

The configure line, also recorded by FFmpeg itself as `FFMPEG_CONFIGURATION`:

```sh
./configure --prefix=<repo>/target/ffmpeg --install-name-dir=@rpath \
  --extra-ldflags=-Wl,-rpath,@loader_path --cc=clang --arch=arm64 \
  --enable-shared --disable-static --disable-programs --disable-doc --disable-debug \
  --disable-avdevice --disable-postproc --disable-avfilter \
  --disable-encoders --disable-muxers --disable-network --disable-autodetect \
  --enable-zlib --disable-x86asm --disable-protocols --enable-protocol=file \
  --disable-decoders --enable-decoder=h264,hevc,mpeg1video,mpeg2video,mpeg4,msmpeg4v1,msmpeg4v2,msmpeg4v3,wmv1,wmv2,wmv3,vc1,h263,h263i,h263p,flv,vp6,vp6a,vp6f,vp8,vp9,theora,rv10,rv20,rv30,rv40,mjpeg,mjpegb,dvvideo,prores,dnxhd,cinepak,huffyuv,ffvhuff,ffv1,rawvideo,indeo3,indeo4,indeo5,svq1,svq3,msvideo1,msrle,qtrle,utvideo,magicyuv,png,aac,aac_latm,mp1,mp1float,mp2,mp2float,mp3,mp3float,ac3,eac3,dca,truehd,mlp,vorbis,opus,flac,alac,wmav1,wmav2,wmapro,wmalossless,cook,ra_144,ra_288,sipr,atrac3,nellymoser,amrnb,amrwb,ape,adpcm_ms,adpcm_ima_wav,adpcm_ima_qt,adpcm_swf,gsm_ms,pcm_s8,pcm_u8,pcm_s16le,pcm_s16be,pcm_s24le,pcm_s24be,pcm_s32le,pcm_s32be,pcm_f32le,pcm_f32be,pcm_f64le,pcm_alaw,pcm_mulaw,pcm_bluray,pcm_dvd \
  --disable-demuxers --enable-demuxer=matroska,avi,asf,flv,live_flv,ogg,mpegts,mpegps,mpegvideo,mov,rm,h264,hevc,m4v,vc1,ivf,nut,dv,mxf,yuv4mpegpipe,aac,mp3,flac,wav,w64,aiff,ac3,eac3,dts,truehd,amr,ape
```

Parsers and bitstream filters keep FFmpeg's defaults, since the demuxers and decoders above need them. libswscale and libswresample are built; libavdevice, libavfilter and libpostproc are not. The build links only macOS's own libz, CoreFoundation, CoreVideo and CoreMedia.

The libraries carry `@rpath` install names and an `@loader_path` rpath, so each finds its siblings whether it is opened from `target/ffmpeg/lib` or from the app bundle.

AV1 is not decoded: FFmpeg's own AV1 decoder needs hardware acceleration, and the software decoders (dav1d, libaom) need meson or cmake to build. A WebM with AV1 video shows the can't-play page.

## Licence

FFmpeg built this way is LGPL-2.1-or-later. Falcon stays MIT/Apache-2.0: it contains no FFmpeg code, links nothing at build time, and loads unmodified shared libraries that a user can replace. The loader in `falcon-ffmpeg/src/ffi.rs` reads each library's `*_license()` string at run time and refuses any library that is not LGPL (a `--enable-gpl` or nonfree build), and any major version other than FFmpeg 7.1's (libavutil 59, libavcodec 61, libavformat 61, libswscale 8, libswresample 5).

`scripts/bundle.sh` copies the five libraries into `Falcon.app/Contents/Frameworks`, sets their install names to `@rpath/…`, adds `@executable_path/../Frameworks` to the executable's rpaths and signs the bundle ad hoc again. The LGPL text and `BUILD-INFO.txt` go in `Contents/Resources/FFmpeg`, since code signing allows only code in `Frameworks`. Without `target/ffmpeg` the bundle builds as before. `NOTICE` and `ATTRIBUTION.md` record FFmpeg's version, licence and source.

## Where the libraries are looked for

In order: `$FALCON_FFMPEG_DIR`, the bundle's `Contents/Frameworks` next to the executable, and the workspace's `target/ffmpeg/lib`. The first folder holding `libavformat.61.dylib` is used. `falcon_video::ffmpeg_status()` reports the version and folder, or why FFmpeg can't be used.

## The bindings

`falcon-ffmpeg` has its own small FFI rather than `ffmpeg-sys`, so nothing about FFmpeg is needed at build time. It resolves about forty functions (opening and reading files, decoding, seeking, `sws_scale`, `swr_convert` and their helpers). Structs are declared only as far as the last field read, laid out as FFmpeg 7.1 declares them. The test `struct_layouts_match_the_installed_headers` compiles a C program against the installed headers and checks every offset and constant the bindings use.

## The player

`falcon_ffmpeg::Player` sits behind the same `falcon_video::Video` the viewer uses for AVFoundation, so play and pause, seeking, speed, looping, volume and mute, frame stepping, the current time and the duration drive one viewer.

- **Threads.** A demux thread reads packets, decodes and resamples the sound, and queues video packets. A video thread decodes pictures and converts them to RGBA with libswscale (after the pixel aspect ratio and any display-matrix rotation) into a queue of at most eight frames, fewer for large pictures (96 MB at most). The viewer takes the frame due each time it draws and uploads it as a texture.
- **Clock.** The sound goes to the app's rodio output as stereo 32-bit float at 48 kHz. The samples the output takes move the audio clock, which is the master clock while sound plays. With no sound track, no output, the sound muted, or the sound ended first, a wall clock runs the picture and takes over from where the audio clock stopped. Frames due are shown; late ones are skipped.
- **Mute.** Muting stops decoding the sound and switches to the wall clock; unmuting decodes it again from the playhead.
- **Speed.** The sound is resampled to the new speed, so its pitch moves with the speed (FFmpeg's pitch-keeping filter is in libavfilter, which is not built).
- **Seeking.** A seek finds the keyframe before the time (aiming a quarter second early, since FFmpeg rounds to the nearest frame) and decodes forward to the frame on screen at that time. Every seek starts a new generation, and work for an older one is dropped. Seeking past the last frame shows the last frame.
- **Frame steps.** Forward steps take the next decoded frames; backward steps seek to the earlier frame.
- **Frame tools.** Set as thumbnail, copy and save frame decode the picture at the playhead again at full size, as with AVFoundation.

## Other uses

- `falcon_video::probe`, `frame_at` and `Grabber` fall back to FFmpeg, so the seek bar's hover thumbnail and the grid's hover strip work for these formats.
- `falcon_ffmpeg::AudioFile` decodes a file's sound alone. `falcon_library::audio::open`, the decoder behind audio playback, the hover sound, waveforms, durations and the tempo, falls back to it (`audio/ffmpeg.rs`) when symphonia can't read a file: the soundtrack of an mkv, webm or avi in the hover preview, and WMA, APE or AMR files.
- At import, `media::duration` asks FFmpeg for the length of video it can't otherwise read, and FFmpeg draws the thumbnail as described above. Refresh Thumbnail goes the same way.

## Tests

- Unit tests cover the clocks, the frame queue, timestamp and seek arithmetic, rotation, black-frame detection and the licence check.
- `crates/falcon-ffmpeg/tests/ffmpeg.rs` runs only when `target/ffmpeg` exists (`build.rs` sets `ffmpeg_built`; otherwise the tests are ignored). It writes its own AVI clips byte by byte (uncompressed BGR pictures and PCM sound, since the build has no encoders) and checks probing, picture grabbing, thumbnails, playback, seeking, stepping, looping, the audio clock, muting and the struct layouts.
- `crates/falcon-video/tests/avfoundation.rs` also decodes its AVFoundation-written H.264/AAC clips, and a rotated one, through FFmpeg and checks they match.
- `FALCON_VIDEO_SAMPLES=<folder> cargo test -p falcon-ffmpeg --test ffmpeg samples -- --ignored --nocapture` probes, thumbnails, plays and seeks every clip in a folder, for checks on real files.
