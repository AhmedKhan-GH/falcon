//! Sets `ffmpeg_built` when `scripts/build-ffmpeg.sh` has installed FFmpeg
//! into the workspace's `target/ffmpeg`, so the decoding tests run only
//! then (builds made before the atempo filter lack libavfilter and can't be
//! loaded, so they don't count), and `ffmpeg_dav1d` when that build includes dav1d (builds made
//! before AV1 support lack it), so the AV1 tests run only then. Nothing is
//! linked: the libraries are opened at run time.

use std::path::Path;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(ffmpeg_built)");
    println!("cargo::rustc-check-cfg=cfg(ffmpeg_dav1d)");
    let lib = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ffmpeg/lib");
    println!("cargo::rerun-if-changed={}", lib.display());
    println!("cargo::rerun-if-changed={}", lib.join("libavformat.61.dylib").display());
    println!("cargo::rerun-if-changed={}", lib.join("libavfilter.10.dylib").display());
    println!("cargo::rerun-if-changed={}", lib.join("avformat-61.dll").display());
    println!("cargo::rerun-if-changed={}", lib.join("avfilter-10.dll").display());
    let has = |names: [&str; 3]| names.iter().any(|name| lib.join(name).exists());
    if has(["libavformat.61.dylib", "libavformat.so.61", "avformat-61.dll"]) && has(["libavfilter.10.dylib", "libavfilter.so.10", "avfilter-10.dll"]) {
        println!("cargo::rustc-cfg=ffmpeg_built");
    }
    println!("cargo::rerun-if-changed={}", lib.join("libdav1d.7.dylib").display());
    println!("cargo::rerun-if-changed={}", lib.join("libdav1d.dll").display());
    if has(["libdav1d.7.dylib", "libdav1d.so.7", "libdav1d.dll"]) {
        println!("cargo::rustc-cfg=ffmpeg_dav1d");
    }
}
