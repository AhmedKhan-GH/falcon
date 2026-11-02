//! Reads every `.ifc` under `$FALCON_IFC_SAMPLES` (for example the
//! buildingSMART sample files, which are not committed) the way the viewer
//! does and through falcon's own reader, and prints one line per file:
//! schema, time, products shown and skipped, triangles, the geometry types
//! not read and whether web-ifc failed on it. Without the variable it does
//! nothing.

use std::path::{Path, PathBuf};

use falcon_library::model3d::ifc;

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("ifc")) {
            out.push(p);
        }
    }
}

#[test]
fn sample_files() {
    let Some(dir) = std::env::var_os("FALCON_IFC_SAMPLES") else { return };
    let mut files = Vec::new();
    walk(Path::new(&dir), &mut files);
    files.sort();
    for f in files {
        let bytes = std::fs::read(&f).unwrap();
        let name = f.strip_prefix(&dir).unwrap_or(&f).display().to_string();
        let start = std::time::Instant::now();
        let full = ifc::read(&bytes);
        let ms = start.elapsed().as_millis();
        let start = std::time::Instant::now();
        let own = ifc::read_tessellated(&bytes);
        let own_ms = start.elapsed().as_millis();
        let own = own.map_or("error".to_string(), |(_, r)| format!("{} shown {} skipped {} tris", r.shown, r.skipped, r.triangles));
        match full {
            Ok((model, r)) => println!(
                "{name}\t{}\t{}MB\t{ms}ms\tshown {}\tskipped {}\ttris {}\tmeshes {}\tunread {:?}\tfallback {}\town {own_ms}ms {own}",
                r.schema,
                bytes.len() >> 20,
                r.shown,
                r.skipped,
                r.triangles,
                model.map_or(0, |m| m.meshes.len()),
                r.unsupported,
                r.fallback,
            ),
            Err(e) => println!("{name}\t{}MB\t{ms}ms\tERROR {e}", bytes.len() >> 20),
        }
    }
}
