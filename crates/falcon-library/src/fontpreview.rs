//! Font specimens: the 600×600 picture Eagle renders as a font item's
//! thumbnail (see the Thumbnails chapter of the docs), drawn with resvg
//! using only the font itself.

use std::sync::Arc;

use image::{DynamicImage, RgbaImage};
use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg;
use serde_json::{Map, Value};

use crate::fontmeta;

/// Size of the specimen, which Eagle also records as the item's size.
pub const SIZE: u32 = 600;
/// Text colour, #333.
const INK: &str = "#333333";
/// Line height as a multiple of each line's font size.
const LINE_HEIGHT: f32 = 1.35;

/// Which specimen to draw, chosen from the font's script support the way
/// Eagle chooses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Specimen {
    Latin,
    SimplifiedChinese,
    TraditionalChinese,
    Japanese,
    Korean,
}

impl Specimen {
    /// Picks the specimen from a font's `fontMetas`.
    pub fn for_metas(metas: &Map<String, Value>) -> Specimen {
        let support = |k: &str| metas.get("support").and_then(|s| s.get(k)).and_then(Value::as_bool) == Some(true);
        let ps_lang = |k: &str| metas.get("postScriptName").and_then(|p| p.get(k)).is_some();
        let (cn, tw, jp, kr) = (support("zh_CN"), support("zh_TW"), support("jp"), support("kr"));
        if let Some(lang) = metas.get("preferLng").and_then(Value::as_str) {
            return match lang {
                "zh_CN" => Specimen::SimplifiedChinese,
                "zh_TW" => Specimen::TraditionalChinese,
                "jp" => Specimen::Japanese,
                "kr" => Specimen::Korean,
                _ => Specimen::Latin,
            };
        }
        if cn && tw {
            Specimen::SimplifiedChinese
        } else if ps_lang("ja") {
            Specimen::Japanese
        } else if cn && ps_lang("zh") {
            Specimen::SimplifiedChinese
        } else if tw && ps_lang("zh_TW") {
            Specimen::TraditionalChinese
        } else if cn {
            Specimen::SimplifiedChinese
        } else if kr {
            Specimen::Korean
        } else if tw {
            Specimen::TraditionalChinese
        } else if jp {
            Specimen::Japanese
        } else {
            Specimen::Latin
        }
    }

    /// The lines with their font sizes, and the width of the text block.
    /// The sample words are falcon's own; the sizes and layout follow
    /// Eagle's.
    fn lines(self) -> (Vec<(&'static str, f32)>, f32) {
        let cjk = |a: &'static str, b: &'static str| (vec![(a, 100.0), (b, 100.0), ("AaBbCcDd", 82.0), ("01234567", 82.0)], 380.0);
        match self {
            Specimen::Latin => (vec![("ABCDEFG", 90.0), ("abcdefg", 90.0), ("0123456", 90.0)], 400.0),
            Specimen::SimplifiedChinese => cjk("字体样张", "汉字之美"),
            Specimen::TraditionalChinese => cjk("字體樣張", "漢字之美"),
            Specimen::Japanese => cjk("あいうえ", "日本語文"),
            Specimen::Korean => (vec![("가나다라마", 90.0), ("AaBbCcDd", 82.0), ("01234567", 82.0)], 380.0),
        }
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;")
}

/// Renders the specimen of a font file (ttf, otf, ttc/otc face 0, woff):
/// dark grey lines on white, left-aligned in a block centred on the canvas.
pub fn render(data: &[u8], metas: &Map<String, Value>) -> Option<DynamicImage> {
    let sfnt = fontmeta::sfnt(data)?.into_owned();
    let face = fontmeta::parse(&sfnt)?;
    let upm = f32::from(face.units_per_em());
    let (asc, desc) = (f32::from(face.ascender()) / upm, -f32::from(face.descender()) / upm);
    let mut db = usvg::fontdb::Database::new();
    db.load_font_data(sfnt.clone());
    let family = db.faces().next()?.families.first()?.0.clone();

    let (lines, block_width) = Specimen::for_metas(metas).lines();
    let block_height: f32 = lines.iter().map(|(_, s)| s * LINE_HEIGHT).sum();
    let left = (SIZE as f32 - block_width) / 2.0;
    let mut y = (SIZE as f32 - block_height) / 2.0;
    let mut spans = String::new();
    for (text, size) in &lines {
        let line = size * LINE_HEIGHT;
        // CSS puts half the leading above the ascent.
        let baseline = y + (line - (asc + desc) * size) / 2.0 + asc * size;
        spans.push_str(&format!(r#"<text x="{left}" y="{baseline}" font-size="{size}">{}</text>"#, escape(text)));
        y += line;
    }
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{SIZE}" height="{SIZE}"><rect width="100%" height="100%" fill="#ffffff"/><g font-family="&quot;{}&quot;" fill="{INK}">{spans}</g></svg>"##,
        escape(&family)
    );
    let opts = usvg::Options { fontdb: Arc::new(db), ..Default::default() };
    let tree = usvg::Tree::from_str(&svg, &opts).ok()?;
    let mut pixmap = Pixmap::new(SIZE, SIZE)?;
    resvg::render(&tree, Transform::identity(), &mut pixmap.as_mut());
    let raw: Vec<u8> = pixmap.pixels().iter().flat_map(|p| [p.red(), p.green(), p.blue(), 255]).collect();
    RgbaImage::from_raw(SIZE, SIZE, raw).map(|i| DynamicImage::ImageRgb8(DynamicImage::ImageRgba8(i).to_rgb8()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn picks_specimens() {
        let m = |v: Value| v.as_object().unwrap().clone();
        assert_eq!(Specimen::for_metas(&m(json!({}))), Specimen::Latin);
        assert_eq!(Specimen::for_metas(&m(json!({"support": {"zh_CN": true, "zh_TW": true}}))), Specimen::SimplifiedChinese);
        assert_eq!(Specimen::for_metas(&m(json!({"support": {"zh_TW": true}}))), Specimen::TraditionalChinese);
        assert_eq!(Specimen::for_metas(&m(json!({"postScriptName": {"ja": "x"}}))), Specimen::Japanese);
        assert_eq!(Specimen::for_metas(&m(json!({"support": {"kr": true, "jp": true}}))), Specimen::Korean);
    }

    #[test]
    fn renders_a_system_font() {
        let Ok(data) = std::fs::read("/System/Library/Fonts/Supplemental/Arial.ttf") else { return };
        let metas = fontmeta::font_metas(&data, "X").unwrap();
        let img = render(&data, &metas).unwrap().to_rgb8();
        assert_eq!(img.dimensions(), (SIZE, SIZE));
        let dark = img.pixels().filter(|p| p.0[0] < 128).count();
        assert!(dark > 5_000, "text was drawn: {dark} dark pixels");
        // The corners stay white; the text sits in the middle.
        assert_eq!(img.get_pixel(5, 5).0, [255, 255, 255]);
        assert_eq!(img.get_pixel(594, 594).0, [255, 255, 255]);
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("specimen_thumbnail.png");
        crate::thumbnail::write_thumbnail(DynamicImage::ImageRgb8(img), crate::thumbnail::DEFAULT_MAX_SIDE, &out).unwrap();
        assert_eq!(crate::thumbnail::decode(&out).unwrap().width(), 320);
    }
}
