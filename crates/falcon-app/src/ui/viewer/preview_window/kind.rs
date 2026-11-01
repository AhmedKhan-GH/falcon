//! The preview window's pure rules: which viewer a file gets, the opacity
//! steps, page stepping and the PDF render scale.

use falcon_library::media;

/// What the window shows an item with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Video,
    Audio,
    Text,
    Pdf,
    /// A font: the main viewer's specimen page.
    Font,
    /// A 3D model falcon reads: the main viewer's model viewer.
    Model,
    /// A bookmark or web page: the main viewer's web view.
    Web,
    /// A picture (or a type with only its thumbnail): the zoomable canvas.
    Picture,
}

/// The viewer for a file extension.
pub fn kind_of(ext: &str) -> Kind {
    let ext = ext.to_lowercase();
    if media::is_video(&ext) {
        Kind::Video
    } else if media::is_audio(&ext) {
        Kind::Audio
    } else if ext == "txt" {
        Kind::Text
    } else if ext == "pdf" {
        Kind::Pdf
    } else if falcon_library::search::type_group(&ext) == Some("font") {
        Kind::Font
    } else if falcon_library::model3d::is_supported(&ext) {
        Kind::Model
    } else if crate::webview::is_web(&ext) {
        Kind::Web
    } else {
        Kind::Picture
    }
}

/// The window opacities of the Opacity menu, in percent.
pub const OPACITIES: [u8; 10] = [100, 90, 80, 70, 60, 50, 40, 30, 20, 10];

/// The page after stepping `delta` pages from `page` in a document of
/// `count` pages, kept inside the document.
pub fn step_page(page: usize, delta: isize, count: usize) -> usize {
    page.saturating_add_signed(delta).min(count.saturating_sub(1))
}

/// The most text read from a file, in bytes.
pub const TEXT_LIMIT: u64 = 2 * 1024 * 1024;

/// The most pixels a PDF page is rendered at.
pub const MAX_PAGE_PIXELS: f64 = 16_777_216.0;

/// Render scales are kept in eighths of a pixel per point, so resizing the
/// window does not render again at every pixel.
const SCALE_STEPS: f64 = 8.0;

/// The render key for a page drawn at `fit` points per point on a screen
/// of `ppp` pixels per point: rounded up to a step, never 0.
pub fn render_key(fit: f32, ppp: f32) -> i32 {
    ((f64::from(fit) * f64::from(ppp) * SCALE_STEPS).ceil() as i32).max(1)
}

/// The pixels per point a render key stands for, for a page of `size`
/// points, held under [`MAX_PAGE_PIXELS`].
pub fn key_scale(key: i32, size: (f64, f64)) -> f64 {
    let cap = (MAX_PAGE_PIXELS / (size.0 * size.1).max(1.0)).sqrt();
    (f64::from(key) / SCALE_STEPS).min(cap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewers_follow_the_extension() {
        assert_eq!(kind_of("mp4"), Kind::Video);
        assert_eq!(kind_of("MOV"), Kind::Video);
        assert_eq!(kind_of("mp3"), Kind::Audio);
        assert_eq!(kind_of("txt"), Kind::Text);
        assert_eq!(kind_of("PDF"), Kind::Pdf);
        assert_eq!(kind_of("png"), Kind::Picture);
        assert_eq!(kind_of("ttf"), Kind::Font);
        assert_eq!(kind_of("OTF"), Kind::Font);
        assert_eq!(kind_of("glb"), Kind::Model);
        assert_eq!(kind_of("STL"), Kind::Model);
        assert_eq!(kind_of("fbx"), Kind::Picture, "3D formats falcon can't read show their still");
        assert_eq!(kind_of("url"), Kind::Web);
        assert_eq!(kind_of("html"), Kind::Web);
        assert_eq!(kind_of(""), Kind::Picture);
    }

    #[test]
    fn opacities_run_from_full_to_a_tenth() {
        assert_eq!(OPACITIES.first(), Some(&100));
        assert_eq!(OPACITIES.last(), Some(&10));
        assert!(OPACITIES.windows(2).all(|w| w[0] - w[1] == 10));
    }

    #[test]
    fn pages_stay_inside_the_document() {
        assert_eq!(step_page(0, -1, 5), 0);
        assert_eq!(step_page(0, 1, 5), 1);
        assert_eq!(step_page(4, 1, 5), 4);
        assert_eq!(step_page(2, -10, 5), 0);
        assert_eq!(step_page(2, 10, 5), 4);
        assert_eq!(step_page(0, 1, 0), 0);
    }

    #[test]
    fn render_scales_round_up_and_are_capped() {
        assert_eq!(render_key(1.0, 2.0), 16);
        assert_eq!(render_key(1.01, 2.0), 17);
        assert_eq!(render_key(0.0, 2.0), 1);
        assert_eq!(key_scale(16, (612.0, 792.0)), 2.0);
        let huge = key_scale(800, (10_000.0, 10_000.0));
        assert!(huge * huge * 1e8 <= MAX_PAGE_PIXELS + 1.0);
    }
}
