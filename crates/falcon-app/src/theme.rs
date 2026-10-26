//! Eagle's dark theme: colours sampled from Eagle's window and stylesheets,
//! SF Pro as the UI font, Phosphor Light for icons.

use std::sync::Arc;

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke};

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Content area and toolbar.
pub const MAIN: Color32 = rgb(0x17191d);
/// Sidebar and inspector.
pub const PANEL: Color32 = rgb(0x1f2023);
/// Panel edges and section dividers.
pub const DIVIDER: Color32 = rgb(0x303134);
/// Selected sidebar row.
pub const ROW_SELECTED: Color32 = rgb(0x353639);
/// Hovered sidebar row.
pub const ROW_HOVER: Color32 = rgb(0x2a2b2e);
/// Text fields in panels.
pub const FIELD: Color32 = rgb(0x19191d);
pub const FIELD_BORDER: Color32 = rgb(0x2c2d30);
/// The toolbar search field.
pub const SEARCH: Color32 = rgb(0x131416);
/// Buttons such as Export.
pub const BUTTON: Color32 = rgb(0x2b2c2f);
pub const BUTTON_HOVER: Color32 = rgb(0x343538);
/// Grid card outline.
pub const CARD_BORDER: Color32 = rgb(0x242528);
pub const CARD_HOVER: Color32 = rgb(0x3a3b3e);
/// Folder tree guide lines.
pub const GUIDE: Color32 = rgb(0x37383b);

pub const TEXT: Color32 = rgb(0xf8f9fb);
/// Counts, section headers, dimensions, placeholders.
pub const TEXT_MUTED: Color32 = rgb(0xa2a2a4);
/// Property values.
pub const TEXT_VALUE: Color32 = rgb(0xcdced0);
pub const TEXT_DISABLED: Color32 = rgb(0x5a5b5e);
/// `--color-primary` in dark themes.
pub const PRIMARY: Color32 = rgb(0x3297ff);
/// The library switcher's icon.
pub const BRAND: Color32 = rgb(0x0072ef);

pub const RADIUS: CornerRadius = CornerRadius::same(6);
pub const FIELD_RADIUS: CornerRadius = CornerRadius::same(8);

/// Height of the toolbar and the panel header rows.
pub const BAR_HEIGHT: f32 = 48.0;
/// Sidebar row height.
pub const ROW_HEIGHT: f32 = 27.0;
pub const SIDEBAR_WIDTH: f32 = 240.0;
pub const INSPECTOR_WIDTH: f32 = 240.0;

/// Font family of the filled icon set.
pub fn fill_family() -> FontFamily {
    FontFamily::Name("phosphor-fill".into())
}

pub fn font(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

/// Badge colour Eagle uses for a file type, if it shows one.
pub fn ext_color(ext: &str) -> Option<Color32> {
    let c = match ext {
        "mp3" | "wav" | "flac" | "ogg" | "aac" | "m4a" | "mp4" | "mov" | "m4v" | "webm" | "mkv" | "avi"
        | "wmv" | "mpg" | "ts" => 0x333333,
        "gif" => 0xbb1404,
        "eps" | "svg" => 0xe97d34,
        "doc" | "docx" => 0x265cb9,
        "xls" | "xlsx" => 0x2a7b35,
        "ppt" | "pptx" | "potx" => 0xc03e29,
        "pdf" => 0xc3001a,
        "pages" => 0xc27106,
        "numbers" => 0x0eb819,
        "key" => 0x007eb4,
        "psd" | "psdt" | "psb" => 0x055ea5,
        "ai" => 0xba4c08,
        "indd" => 0xbf0b5c,
        _ => return None,
    };
    Some(rgb(c))
}

/// Installs fonts and visuals.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let mut add = |name: &str, data: FontData| {
        fonts.font_data.insert(name.into(), Arc::new(data));
    };
    add("phosphor-light", FontData::from_static(include_bytes!("../assets/Phosphor-Light.ttf")));
    add("phosphor-fill", FontData::from_static(include_bytes!("../assets/Phosphor-Fill.ttf")));
    let system = |path: &str| std::fs::read(path).ok().map(FontData::from_owned);
    let sf = system("/System/Library/Fonts/SFNS.ttf");
    let sf_mono = system("/System/Library/Fonts/SFNSMono.ttf");
    let has_sf = sf.is_some();
    let has_mono = sf_mono.is_some();
    if let Some(d) = sf {
        add("sf", d);
    }
    if let Some(d) = sf_mono {
        add("sf-mono", d);
    }
    let prop = fonts.families.entry(FontFamily::Proportional).or_default();
    if has_sf {
        prop.insert(0, "sf".into());
    }
    prop.push("phosphor-light".into());
    let monospace = fonts.families.entry(FontFamily::Monospace).or_default();
    if has_mono {
        monospace.insert(0, "sf-mono".into());
    }
    monospace.push("phosphor-light".into());
    fonts.families.insert(fill_family(), vec!["phosphor-fill".into()]);
    ctx.set_fonts(fonts);

    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(|s| {
        let v = &mut s.visuals;
        v.panel_fill = MAIN;
        v.window_fill = PANEL;
        v.extreme_bg_color = FIELD;
        v.faint_bg_color = PANEL;
        v.override_text_color = Some(TEXT);
        v.selection.bg_fill = PRIMARY.gamma_multiply(0.45);
        v.selection.stroke = Stroke::new(1.0, PRIMARY);
        v.hyperlink_color = TEXT_VALUE;
        v.window_corner_radius = CornerRadius::same(10);
        v.window_stroke = Stroke::new(1.0, DIVIDER);
        v.menu_corner_radius = CornerRadius::same(8);
        v.text_cursor.stroke = Stroke::new(1.5, PRIMARY);
        for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.corner_radius = RADIUS;
            w.bg_stroke = Stroke::NONE;
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, DIVIDER);
        v.widgets.inactive.weak_bg_fill = BUTTON;
        v.widgets.inactive.bg_fill = BUTTON;
        v.widgets.hovered.weak_bg_fill = BUTTON_HOVER;
        v.widgets.hovered.bg_fill = BUTTON_HOVER;
        v.widgets.active.weak_bg_fill = ROW_SELECTED;
        s.spacing.scroll = egui::style::ScrollStyle::floating();
        s.spacing.scroll.bar_width = 6.0;
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 5.0);
        s.interaction.selectable_labels = false;
        s.text_styles.insert(egui::TextStyle::Body, font(13.0));
        s.text_styles.insert(egui::TextStyle::Button, font(13.0));
        s.text_styles.insert(egui::TextStyle::Small, font(11.0));
        s.text_styles.insert(egui::TextStyle::Monospace, mono(12.0));
    });
}
