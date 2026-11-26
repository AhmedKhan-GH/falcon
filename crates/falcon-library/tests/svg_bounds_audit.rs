//! Reproducible SVG resource-bounds and external-ref safety audit tests.

use std::fs;
use std::io::Cursor;
use std::time::Instant;

use base64::Engine as _;
use falcon_library::thumbnail;
use image::{ImageFormat, Rgba, RgbaImage};

#[test]
fn svg_resource_bounds_stress_audit() {
    let tmp = tempfile::tempdir().unwrap();

    // 1. Excessive node count (25,000 rect elements, under 32 MiB byte cap).
    let mut nodes_svg =
        String::from(r##"<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512">"##);
    for i in 0..25_000 {
        let x = (i * 7) % 500;
        let y = (i * 13) % 500;
        nodes_svg.push_str(&format!(
            r##"<rect x="{x}" y="{y}" width="8" height="8" fill="#123"/>"##
        ));
    }
    nodes_svg.push_str("</svg>");
    let nodes_path = tmp.path().join("many_nodes.svg");
    fs::write(&nodes_path, &nodes_svg).unwrap();
    let t0 = Instant::now();
    let res_nodes = thumbnail::render_svg_bounded(&nodes_path, 320, 32 * 1024 * 1024, 4096);
    println!(
        "nodes_25k_bytes={} elapsed_ms={} ok={}",
        nodes_svg.len(),
        t0.elapsed().as_millis(),
        res_nodes.is_ok()
    );
    assert!(res_nodes.is_ok());

    // 2. Complex single path with 10,000 cubic curves.
    let mut path_svg = String::from(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512"><path fill="none" stroke="#f00" d="M 0 0"##,
    );
    for i in 0..10_000 {
        let x = (i * 3) % 512;
        let y = (i * 5) % 512;
        path_svg.push_str(&format!(" C {x} 10 20 {y} {x} {y}"));
    }
    path_svg.push_str(r##""/></svg>"##);
    let complex_path = tmp.path().join("complex_path.svg");
    fs::write(&complex_path, &path_svg).unwrap();
    let t1 = Instant::now();
    let res_complex = thumbnail::render_svg_bounded(&complex_path, 320, 32 * 1024 * 1024, 4096);
    println!(
        "complex_10k_bytes={} elapsed_ms={} ok={}",
        path_svg.len(),
        t1.elapsed().as_millis(),
        res_complex.is_ok()
    );
    assert!(res_complex.is_ok());

    // 3. Nested <use> fanout (depth 12 => 4,096 nodes; depth 18 rejected by usvg node limit).
    let mut use12 = String::from(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256"><defs><g id="u0"><rect width="10" height="10" fill="#0f0"/></g>"##,
    );
    for d in 1..=12 {
        use12.push_str(&format!(
            r##"<g id="u{d}"><use href="#u{}"/><use href="#u{}" x="1"/></g>"##,
            d - 1,
            d - 1
        ));
    }
    use12.push_str(r##"</defs><use href="#u12"/></svg>"##);
    let use12_path = tmp.path().join("nested_use12.svg");
    fs::write(&use12_path, &use12).unwrap();
    assert!(thumbnail::render_svg_bounded(&use12_path, 256, 32 * 1024 * 1024, 4096).is_ok());

    let mut use18 = String::from(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256"><defs><g id="u0"><rect width="4" height="4" fill="#0f0"/></g>"##,
    );
    for d in 1..=18 {
        use18.push_str(&format!(
            r##"<g id="u{d}"><use href="#u{}"/><use href="#u{}" x="1"/></g>"##,
            d - 1,
            d - 1
        ));
    }
    use18.push_str(r##"</defs><use href="#u18"/></svg>"##);
    let use18_path = tmp.path().join("nested_use18.svg");
    fs::write(&use18_path, &use18).unwrap();
    assert!(thumbnail::render_svg_bounded(&use18_path, 256, 32 * 1024 * 1024, 4096).is_err());

    // 4. Embedded base64 raster PNG (valid) vs malformed embedded raster.
    let mut png_bytes = Vec::new();
    RgbaImage::from_pixel(16, 16, Rgba([255, 128, 0, 255]))
        .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
        .unwrap();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
    let emb_ok = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><image href="data:image/png;base64,{b64}" width="64" height="64"/></svg>"##
    );
    let emb_ok_path = tmp.path().join("emb_ok.svg");
    fs::write(&emb_ok_path, emb_ok).unwrap();
    let emb_img = thumbnail::render_svg_bounded(&emb_ok_path, 64, 32 * 1024 * 1024, 4096)
        .unwrap()
        .to_rgba8();
    assert_eq!(emb_img.get_pixel(32, 32).0, [255, 128, 0, 255]);

    let emb_bad = r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><rect width="64" height="64" fill="#00f"/><image href="data:image/png;base64,NOTVALIDPNG" width="64" height="64"/></svg>"##;
    let emb_bad_path = tmp.path().join("emb_bad.svg");
    fs::write(&emb_bad_path, emb_bad).unwrap();
    let res_emb_bad = thumbnail::render_svg_bounded(&emb_bad_path, 64, 32 * 1024 * 1024, 4096)
        .unwrap()
        .to_rgba8();
    assert_eq!(res_emb_bad.get_pixel(32, 32).0, [0, 0, 255, 255]);

    // 5. External resource href blocking (file:// and https://).
    let secret = tmp.path().join("secret.png");
    fs::write(&secret, &png_bytes).unwrap();
    let ext_svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><rect width="64" height="64" fill="#0a0"/><image href="file://{}" width="64" height="64"/><image href="https://example.invalid/x.png" width="64" height="64"/></svg>"##,
        secret.display()
    );
    let ext_path = tmp.path().join("ext_ref.svg");
    fs::write(&ext_path, ext_svg).unwrap();
    let ext_img = thumbnail::render_svg_bounded(&ext_path, 64, 32 * 1024 * 1024, 4096)
        .unwrap()
        .to_rgba8();
    assert_eq!(ext_img.get_pixel(32, 32).0, [0, 170, 0, 255]);
}
