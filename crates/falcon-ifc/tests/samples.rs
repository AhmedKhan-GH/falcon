//! Reads every `.ifc` under `$FALCON_IFC_SAMPLES` (for example the
//! buildingSMART sample files, which are not committed) and prints, per
//! file, the products read, those with geometry, triangles and the product
//! types left without geometry. Without the variable, or without web-ifc,
//! it does nothing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
    if !falcon_ifc::available() {
        return;
    }
    let mut files = Vec::new();
    walk(Path::new(&dir), &mut files);
    files.sort();
    for f in files {
        let bytes = std::fs::read(&f).unwrap();
        let t = std::time::Instant::now();
        let r = falcon_ifc::read(&bytes, falcon_ifc::Limits::default());
        let ms = t.elapsed().as_millis();
        let name = f.strip_prefix(&dir).unwrap_or(&f).display().to_string();
        match r {
            Ok(s) => {
                let with = s.products.iter().filter(|p| p.geometries > 0).count();
                let mut without: BTreeMap<&str, usize> = BTreeMap::new();
                for p in s.products.iter().filter(|p| p.geometries == 0) {
                    *without.entry(p.type_name.as_str()).or_default() += 1;
                }
                if std::env::var_os("FALCON_IFC_IDS").is_some() {
                    let ids: Vec<String> = s.products.iter().filter(|p| p.geometries == 0).take(40).map(|p| format!("{}#{}", p.type_name, p.id)).collect();
                    println!("{ids:?}");
                }
                println!("{name}\t{}\t{with}\t{}\t{ms}ms\t{without:?}", s.products.len(), s.triangle_count());
            }
            Err(e) => println!("{name}\tERROR {e}\t{ms}ms"),
        }
    }
}
