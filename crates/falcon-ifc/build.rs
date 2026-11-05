//! Builds `falcon-ifc-worker`, web-ifc and falcon's `cpp/worker.cpp` linked
//! into one executable, when `scripts/fetch-web-ifc.sh` has put the sources in
//! the workspace's `target/web-ifc` (or `$FALCON_WEB_IFC_DIR`), and tells the
//! crate where it is. It stays an executable of its own, nothing of it linked
//! into falcon, so a crash in it can't reach the app. Without the sources
//! nothing is built and `falcon_ifc::available()` is false.

use std::path::{Path, PathBuf};
use std::process::Command;

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
    println!("cargo::rerun-if-env-changed=FALCON_WEB_IFC_DIR");
    // The cc crate builds for the macOS this names (13.3 at least, below).
    println!("cargo::rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");
    println!("cargo::rerun-if-changed=cpp/worker.cpp");
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
        .cargo_metadata(false)
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
        .file("cpp/worker.cpp");
    // web-ifc formats numbers with std::format, which macOS's C++ library has from 13.3, so the
    // worker is built for 13.3 at least. Older systems can't start it, and IFC files fall back to
    // falcon's own reader there.
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "macos") {
        let asked = std::env::var("MACOSX_DEPLOYMENT_TARGET").ok();
        let version = |v: &str| -> (u32, u32) {
            let mut parts = v.split('.').map(|p| p.parse().unwrap_or(0));
            (parts.next().unwrap_or(0), parts.next().unwrap_or(0))
        };
        if asked.as_deref().is_none_or(|v| version(v) < (13, 3)) {
            b.flag("-mmacosx-version-min=13.3");
        }
    }
    // MSVC needs telling about C++ exceptions, large objects and UTF-8 sources. The worker has its C
    // runtime linked in, since a clean Windows may lack Visual C++'s DLLs.
    let msvc = b.get_compiler().is_like_msvc();
    if msvc {
        b.flag("/EHsc").flag("/bigobj").flag("/utf-8").define("NOMINMAX", None).static_crt(true);
    }
    let objects = b.compile_intermediates();
    let exe = PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join(if msvc { "falcon-ifc-worker.exe" } else { "falcon-ifc-worker" });
    let mut link: Command = b.get_compiler().to_command();
    if msvc {
        // No console: a console program started from a windowed app would flash one up. The pipes
        // for the request and the reply work all the same.
        link.args(&objects).arg(format!("/Fe:{}", exe.display())).arg("/link").arg("/SUBSYSTEM:WINDOWS").arg("/ENTRY:mainCRTStartup").arg("/OPT:REF");
    } else {
        link.args(&objects).arg("-pthread").arg("-o").arg(&exe);
    }
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "macos") {
        link.arg("-Wl,-dead_strip");
    }
    let status = link.status().expect("the C++ compiler runs");
    assert!(status.success(), "linking falcon-ifc-worker failed");
    println!("cargo::rustc-env=FALCON_IFC_WORKER={}", exe.display());
}
