//! Checks the palette port against the palettes Eagle stored in the fixture.
//! Eagle computed them from the same thumbnail files, so any difference comes
//! from decoding or resizing, not from the quantizer.

use std::path::Path;

use falcon_library::{Library, palette};

#[test]
fn matches_eagle_on_fixture_thumbnails() {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/Assets.library");
    if !p.is_dir() {
        eprintln!("fixture absent, skipping");
        return;
    }
    let lib = Library::open(&p).unwrap();
    let (mut exact, mut checked) = (0, 0);
    for item in lib.items() {
        let Some(thumb) = item.thumbnail_path(lib.root()) else { continue };
        // Thumbnails are WebP despite the .png name.
        let img = image::ImageReader::open(&thumb).unwrap().with_guessed_format().unwrap().decode().unwrap();
        let ours = palette::extract(&img);
        checked += 1;
        if img.width() <= palette::ANALYSIS_WIDTH {
            // No resize: the quantizer sees exactly Eagle's pixels.
            assert_eq!(ours, item.palettes, "{}", item.id);
            exact += 1;
        } else {
            // Chromium's canvas resampling isn't reproduced bit for bit, so
            // only the dominant colour has to agree.
            assert_eq!(ours[0], item.palettes[0], "{}", item.id);
        }
    }
    assert_eq!((exact, checked), (12, 19));
}
