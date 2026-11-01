//! The system font each Chinese, Japanese or Korean interface needs: San
//! Francisco has no CJK glyphs, so the language's font is added after it
//! (and after the icon font) the first time that language is shown.

use std::sync::atomic::{AtomicU8, Ordering};

use eframe::egui::{self, FontData, FontFamily, FontInsert, InsertFontFamily};

use super::Lang;

/// The macOS font for a language's script, if San Francisco lacks it.
fn font_path(lang: Lang) -> Option<&'static str> {
    match lang {
        Lang::Ja => Some("/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc"),
        Lang::Ko => Some("/System/Library/Fonts/AppleSDGothicNeo.ttc"),
        Lang::ZhCn => Some("/System/Library/Fonts/Hiragino Sans GB.ttc"),
        Lang::ZhTw => Some("/System/Library/Fonts/STHeiti Light.ttc"),
        Lang::En | Lang::De | Lang::Es | Lang::Ru => None,
    }
}

/// Adds the font `lang` needs, once. Without the file the text shows as
/// boxes, as it did before.
pub fn load_fonts(ctx: &egui::Context, lang: Lang) {
    static ADDED: AtomicU8 = AtomicU8::new(0);
    let Some(path) = font_path(lang) else { return };
    let bit = 1u8 << lang.index();
    if ADDED.fetch_or(bit, Ordering::Relaxed) & bit != 0 {
        return;
    }
    let Ok(bytes) = std::fs::read(path) else { return };
    let families = [FontFamily::Proportional, FontFamily::Monospace].map(|family| InsertFontFamily { family, priority: egui::FontPriority::Lowest });
    ctx.add_font(FontInsert { name: format!("falcon-ui-{}", lang.code()), data: FontData::from_owned(bytes), families: families.to_vec() });
}
