//! The system font each Chinese, Japanese or Korean interface needs: San
//! Francisco has no CJK glyphs, so the language's font is added after it
//! (and after the icon font) the first time that language is shown.

use std::sync::atomic::{AtomicU8, Ordering};

use eframe::egui::{self, FontData, FontFamily, FontInsert, InsertFontFamily};

use super::Lang;

/// The Windows font for a language's script, if the interface font lacks it.
#[cfg(windows)]
fn font_path(lang: Lang) -> Option<std::path::PathBuf> {
    let file = match lang {
        Lang::Ja => "YuGothR.ttc",
        Lang::Ko => "malgun.ttf",
        Lang::ZhCn => "msyh.ttc",
        Lang::ZhTw => "msjh.ttc",
        Lang::En | Lang::De | Lang::Es | Lang::Ru => return None,
    };
    let windows = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    Some(std::path::Path::new(&windows).join("Fonts").join(file))
}

/// The macOS font for a language's script, if San Francisco lacks it.
#[cfg(not(windows))]
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
