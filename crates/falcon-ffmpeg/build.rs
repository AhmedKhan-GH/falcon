//! Sets `ffmpeg_built` when `scripts/build-ffmpeg.sh` has installed FFmpeg
//! into the workspace's `target/ffmpeg`, so the decoding tests run only
//! then. Nothing is linked: the libraries are opened at run time.

use std::path::Path;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(ffmpeg_built)");
    let lib = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ffmpeg/lib");
    println!("cargo::rerun-if-changed={}", lib.display());
    println!("cargo::rerun-if-changed={}", lib.join("libavformat.61.dylib").display());
    if lib.join("libavformat.61.dylib").exists() || lib.join("libavformat.so.61").exists() {
        println!("cargo::rustc-cfg=ffmpeg_built");
    }
}
