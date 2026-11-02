//! Compiles web-ifc and falcon's C interface to it (`cpp/shim.cpp`) when
//! `scripts/fetch-web-ifc.sh` has put the sources in the workspace's
//! `target/web-ifc` (or `$FALCON_WEB_IFC_DIR`), and sets `web_ifc` so the
//! crate uses them. Without the sources the crate builds empty and
//! `falcon_ifc::available()` is false.

use std::path::{Path, PathBuf};

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|e| e == "cpp") {
            out.push(p);
        }
    }
}

fn main() {
    println!("cargo::rustc-check-cfg=cfg(web_ifc)");
    println!("cargo::rerun-if-env-changed=FALCON_WEB_IFC_DIR");
    println!("cargo::rerun-if-changed=cpp/shim.cpp");
    let root = std::env::var_os("FALCON_WEB_IFC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/web-ifc"));
    let stamp = root.join("SOURCES.txt");
    println!("cargo::rerun-if-changed={}", stamp.display());
    let cpp = root.join("web-ifc/src/cpp");
    if !stamp.exists() || !cpp.join("web-ifc/modelmanager/ModelManager.cpp").exists() {
        return;
    }
    let mut files = Vec::new();
    sources(&cpp.join("web-ifc"), &mut files);
    let mut b = cc::Build::new();
    b.cpp(true)
        .std("c++20")
        .opt_level(2)
        .debug(false)
        .warnings(false)
        .define("NDEBUG", None)
        .include(&cpp)
        .include(cpp.join("web-ifc"))
        .include(root.join("glm"))
        .include(root.join("tinynurbs/include"))
        .include(root.join("fast_float/include"))
        .include(root.join("cdt/CDT/include"))
        .include(root.join("earcut/include"))
        .include(root.join("spdlog/include"))
        .include(root.join("stduuid/include"))
        .include(root.join("unordered_dense/include"))
        .files(&files)
        .file("cpp/shim.cpp");
    b.compile("falcon_web_ifc");
    println!("cargo::rustc-cfg=web_ifc");
}
