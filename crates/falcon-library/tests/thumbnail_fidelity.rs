//! Regenerates the thumbnails Eagle made for the fixture's raster images and
//! compares them pixel by pixel. Run with `--ignored --nocapture` to print the
//! per-filter error table.

use std::path::Path;

use falcon_library::{Library, thumbnail};
use image::imageops::FilterType;

fn mean_abs_diff(a: &image::RgbaImage, b: &image::RgbaImage) -> f64 {
    let sum: u64 = a.as_raw().iter().zip(b.as_raw()).map(|(x, y)| u64::from(x.abs_diff(*y))).sum();
    sum as f64 / a.as_raw().len() as f64
}

#[test]
#[ignore = "measurement, not a pass/fail check"]
fn compare_filters() {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/Assets.library");
    let Ok(lib) = Library::open(&p) else { return };
    let filters = [
        ("nearest", FilterType::Nearest),
        ("triangle", FilterType::Triangle),
        ("catmull", FilterType::CatmullRom),
        ("gaussian", FilterType::Gaussian),
        ("lanczos3", FilterType::Lanczos3),
    ];
    let mut totals = vec![0.0; filters.len()];
    let mut n = 0;
    let (mut single, mut encoded) = (0.0, 0.0);
    for item in lib.items().iter().filter(|i| i.ext == "png") {
        let Some(t) = item.thumbnail_path(lib.root()) else { continue };
        let eagle = thumbnail::decode(&t).unwrap().to_rgba8();
        let orig = thumbnail::decode(&item.original_path(lib.root())).unwrap();
        n += 1;
        let (tw, th) = eagle.dimensions();
        single += mean_abs_diff(&orig.resize_exact(tw, th, FilterType::Lanczos3).to_rgba8(), &eagle);
        let bytes = thumbnail::encode(&thumbnail::resize(orig.clone(), thumbnail::max_side(&item.ext))).unwrap();
        encoded += mean_abs_diff(&image::load_from_memory(&bytes).unwrap().to_rgba8(), &eagle);
        for (k, (_, f)) in filters.iter().enumerate() {
            let ours = thumbnail::resize_with(orig.clone(), thumbnail::max_side(&item.ext), *f).to_rgba8();
            assert_eq!(ours.dimensions(), eagle.dimensions(), "{}", item.id);
            totals[k] += mean_abs_diff(&ours, &eagle);
        }
    }
    for (k, (name, _)) in filters.iter().enumerate() {
        if k == 0 {
            println!("single-pass lanczos3 (old) {:.3}; catmull + webp q75 {:.3}", single / n as f64, encoded / n as f64);
        }
        println!("{name:10} mean abs diff {:.3} over {n} thumbnails", totals[k] / n as f64);
    }
}
