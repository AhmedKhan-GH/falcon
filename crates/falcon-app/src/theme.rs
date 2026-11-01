//! Eagle's themes: colours for dark (sampled from Eagle's window and
//! stylesheets) and for light, lightgray, gray, blue and purple (worked out
//! from the stylesheets, see `docs/src/app.md`), Auto (gray or light after
//! the macOS appearance), SF Pro as the UI font, Phosphor Light for icons.
//!
//! Drawing code reads the current colours with `p()`. The `pub const`
//! colours below are the dark theme's and are kept until every use has
//! moved to `p()`.

use std::sync::Arc;
#[cfg(not(test))]
use std::sync::atomic::{AtomicU8, Ordering};

use crate::locale::tr;
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

/// Eagle's eight folder, smart folder and tag group colours in its dark
/// themes (`--color-<name>` in `style/base/_color.scss`), in menu order.
pub const ICON_RED: Color32 = rgb(0xff6667);
pub const ICON_ORANGE: Color32 = rgb(0xff9f0a);
pub const ICON_YELLOW: Color32 = rgb(0xffd60a);
pub const ICON_GREEN: Color32 = rgb(0x30d159);
pub const ICON_AQUA: Color32 = rgb(0x40cbe0);
pub const ICON_BLUE: Color32 = rgb(0x00aaff);
pub const ICON_PURPLE: Color32 = rgb(0xc499ff);
pub const ICON_PINK: Color32 = rgb(0xff99cc);
/// The uncoloured swatch.
pub const ICON_NONE: Color32 = rgb(0xbdbec0);

/// Eagle's themes, saved under Eagle's names (`body[theme=…]`, and `auto`).
/// The order of the variants indexes `PALETTES`; Auto, which has no
/// palette of its own, comes last.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Dark,
    Light,
    Gray,
    Blue,
    Purple,
    Lightgray,
    /// Gray while macOS is in dark mode, light otherwise, as in Eagle.
    Auto,
}

impl Theme {
    /// In the order Eagle's Preferences lists them.
    pub fn all() -> [Theme; 7] {
        [Theme::Auto, Theme::Light, Theme::Lightgray, Theme::Gray, Theme::Dark, Theme::Blue, Theme::Purple]
    }

    /// The fixed theme shown for this choice when the system appearance is
    /// `system`. Auto picks gray or light like Eagle; with the appearance
    /// unknown it picks gray, matching egui's dark fallback.
    pub fn resolve(self, system: Option<egui::Theme>) -> Theme {
        match (self, system) {
            (Theme::Auto, Some(egui::Theme::Light)) => Theme::Light,
            (Theme::Auto, _) => Theme::Gray,
            (fixed, _) => fixed,
        }
    }

    /// The theme's colours; Auto's are those it shows in dark mode.
    pub const fn palette(self) -> Palette {
        match self {
            Theme::Dark => DARK,
            Theme::Light => LIGHT,
            Theme::Gray | Theme::Auto => GRAY,
            Theme::Blue => BLUE,
            Theme::Purple => PURPLE,
            Theme::Lightgray => LIGHTGRAY,
        }
    }

    /// Whether egui's light defaults suit this theme.
    pub fn is_light(self) -> bool {
        matches!(self, Theme::Light | Theme::Lightgray)
    }
}

impl std::fmt::Display for Theme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Theme::Dark => tr("Dark"),
            Theme::Light => tr("Light"),
            Theme::Gray => tr("Gray"),
            Theme::Blue => tr("Blue"),
            Theme::Purple => tr("Purple"),
            Theme::Lightgray => tr("Lightgray"),
            Theme::Auto => tr("Auto"),
        })
    }
}

/// One theme's colours. The fields are the lower-case names of the
/// colour constants above, which they replace.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Palette {
    pub main: Color32,
    pub panel: Color32,
    pub divider: Color32,
    pub row_selected: Color32,
    pub row_hover: Color32,
    pub field: Color32,
    pub field_border: Color32,
    pub search: Color32,
    pub button: Color32,
    pub button_hover: Color32,
    pub card_border: Color32,
    pub card_hover: Color32,
    pub guide: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub text_value: Color32,
    pub text_disabled: Color32,
    pub primary: Color32,
    pub brand: Color32,
    pub icon_red: Color32,
    pub icon_orange: Color32,
    pub icon_yellow: Color32,
    pub icon_green: Color32,
    pub icon_aqua: Color32,
    pub icon_blue: Color32,
    pub icon_purple: Color32,
    pub icon_pink: Color32,
    pub icon_none: Color32,
}

/// Dark: the values sampled from Eagle's window.
pub const DARK: Palette = Palette {
    main: MAIN,
    panel: PANEL,
    divider: DIVIDER,
    row_selected: ROW_SELECTED,
    row_hover: ROW_HOVER,
    field: FIELD,
    field_border: FIELD_BORDER,
    search: SEARCH,
    button: BUTTON,
    button_hover: BUTTON_HOVER,
    card_border: CARD_BORDER,
    card_hover: CARD_HOVER,
    guide: GUIDE,
    text: TEXT,
    text_muted: TEXT_MUTED,
    text_value: TEXT_VALUE,
    text_disabled: TEXT_DISABLED,
    primary: PRIMARY,
    brand: BRAND,
    icon_red: ICON_RED,
    icon_orange: ICON_ORANGE,
    icon_yellow: ICON_YELLOW,
    icon_green: ICON_GREEN,
    icon_aqua: ICON_AQUA,
    icon_blue: ICON_BLUE,
    icon_purple: ICON_PURPLE,
    icon_pink: ICON_PINK,
    icon_none: ICON_NONE,
};

/// The folder colours of Eagle's dark themes (dark, gray, blue, purple).
const DARK_ICONS: [Color32; 9] = [ICON_RED, ICON_ORANGE, ICON_YELLOW, ICON_GREEN, ICON_AQUA, ICON_BLUE, ICON_PURPLE, ICON_PINK, ICON_NONE];

/// A dark-family palette from its flattened surface and text colours
/// (in field order up to `text_disabled`); primary, brand and icons are
/// shared by Eagle's dark themes.
const fn dark_family(c: [u32; 17]) -> Palette {
    palette(c, PRIMARY, DARK_ICONS)
}

const fn palette(c: [u32; 17], primary: Color32, icons: [Color32; 9]) -> Palette {
    Palette {
        main: rgb(c[0]),
        panel: rgb(c[1]),
        divider: rgb(c[2]),
        row_selected: rgb(c[3]),
        row_hover: rgb(c[4]),
        field: rgb(c[5]),
        field_border: rgb(c[6]),
        search: rgb(c[7]),
        button: rgb(c[8]),
        button_hover: rgb(c[9]),
        card_border: rgb(c[10]),
        card_hover: rgb(c[11]),
        guide: rgb(c[12]),
        text: rgb(c[13]),
        text_muted: rgb(c[14]),
        text_value: rgb(c[15]),
        text_disabled: rgb(c[16]),
        primary,
        brand: BRAND,
        icon_red: icons[0],
        icon_orange: icons[1],
        icon_yellow: icons[2],
        icon_green: icons[3],
        icon_aqua: icons[4],
        icon_blue: icons[5],
        icon_purple: icons[6],
        icon_pink: icons[7],
        icon_none: icons[8],
    }
}

/// Light: a white base (`body[theme=light]` uses `--color-white`) with
/// `--color-dark` (`#2c2f32`) text and overlays, Eagle's `#0072ef` primary
/// and its darker light-theme folder colours.
pub const LIGHT: Palette = palette(
    [
        0xffffff, 0xf9f9f9, 0xe0e0e0, 0xe5e5e5, 0xefefef, 0xfcfcfc, 0xe3e3e3, 0xffffff, 0xefefef, 0xe5e5e5, 0xe6e6e6, 0xdfe0e0,
        0xe0e0e1, 0x2c2f32, 0x7e8082, 0x55575a, 0xc0c1c2,
    ],
    rgb(0x0072ef),
    LIGHT_ICONS,
);

/// The folder colours of Eagle's light themes (light, lightgray).
const LIGHT_ICONS: [Color32; 9] =
    [rgb(0xe55245), rgb(0xcc782f), rgb(0xc29343), rgb(0x548164), rgb(0x0097a7), rgb(0x3987bf), rgb(0x8a67ab), rgb(0xcc6699), rgb(0x50555b)];

/// Lightgray: the light family (`body[theme=lightgray]` shares light's
/// block and folder colours) on a `#e3e4e6` base.
pub const LIGHTGRAY: Palette = palette(
    [
        0xe3e4e6, 0xdedfe1, 0xc8c9cb, 0xcccdd0, 0xd5d6d8, 0xefeff0, 0xd7d7d8, 0xf1f2f3, 0xd5d6d8, 0xcccdd0, 0xcccdcf, 0xc8c9cb,
        0xc9cacd, 0x2c2f32, 0x737578, 0x505255, 0xacaeb0,
    ],
    rgb(0x0072ef),
    LIGHT_ICONS,
);

/// The dark-family palettes put `--color-lightgray` overlays on their
/// `--color-theme` base, layered as worked out in `docs/src/app.md`
/// (Themes); they were not checked against Eagle's window.
///
/// Gray: base `#37383c`.
pub const GRAY: Palette = dark_family([
    0x37383c, 0x3d3e42, 0x4d4d51, 0x505155, 0x46474b, 0x313235, 0x414245, 0x2c2d30, 0x46474b, 0x505155, 0x414246, 0x545559,
    0x505155, 0xf8f9fb, 0xadaeb1, 0xd3d4d6, 0x717275,
]);

/// Blue: base `#0d1630`.
pub const BLUE: Palette = dark_family([
    0x0d1630, 0x141d36, 0x272f46, 0x2b334a, 0x1f2840, 0x10172b, 0x232a3c, 0x0a1226, 0x1f2840, 0x2b334a, 0x19223a, 0x30384e,
    0x2d354b, 0xf8f9fb, 0x9da1ac, 0xcacdd4, 0x535a6d,
]);

/// Purple: base `#1c1424`.
pub const PURPLE: Palette = dark_family([
    0x1c1424, 0x231b2a, 0x352d3b, 0x38313f, 0x2e2634, 0x1c1622, 0x2e2934, 0x16101d, 0x2e2634, 0x38313f, 0x27202f, 0x3d3644,
    0x3a3341, 0xf8f9fb, 0xa3a0a7, 0xcdcdd1, 0x5e5965,
]);

/// Every fixed theme's palette, indexed by `Theme as usize`.
static PALETTES: [Palette; 6] = [DARK, LIGHT, GRAY, BLUE, PURPLE, LIGHTGRAY];

impl Theme {
    fn from_index(i: u8) -> Theme {
        match i {
            1 => Theme::Light,
            2 => Theme::Gray,
            3 => Theme::Blue,
            4 => Theme::Purple,
            5 => Theme::Lightgray,
            6 => Theme::Auto,
            _ => Theme::Dark,
        }
    }
}

/// The chosen theme and the fixed theme it shows (they differ only for
/// Auto). Atomics rather than a lock: `p()` is read many times per frame.
#[cfg(not(test))]
static CHOSEN: AtomicU8 = AtomicU8::new(Theme::Dark as u8);
#[cfg(not(test))]
static SHOWN: AtomicU8 = AtomicU8::new(Theme::Dark as u8);

#[cfg(not(test))]
fn load() -> (Theme, Theme) {
    (Theme::from_index(CHOSEN.load(Ordering::Relaxed)), Theme::from_index(SHOWN.load(Ordering::Relaxed)))
}

#[cfg(not(test))]
fn store(chosen: Theme, shown: Theme) {
    CHOSEN.store(chosen as u8, Ordering::Relaxed);
    SHOWN.store(shown as u8, Ordering::Relaxed);
}

// Tests run in parallel threads; a theme per thread keeps one test's
// `set_theme` from changing the colours another test reads.
#[cfg(test)]
thread_local! {
    static CURRENT: std::cell::Cell<(Theme, Theme)> = const { std::cell::Cell::new((Theme::Dark, Theme::Dark)) };
}

#[cfg(test)]
fn load() -> (Theme, Theme) {
    CURRENT.with(|c| c.get())
}

#[cfg(test)]
fn store(chosen: Theme, shown: Theme) {
    CURRENT.with(|c| c.set((chosen, shown)));
}

/// The current theme's colours.
pub fn p() -> &'static Palette {
    &PALETTES[load().1 as usize]
}

/// The chosen theme (possibly Auto).
pub fn current() -> Theme {
    load().0
}

/// Switches to `theme`: later `p()` calls return its colours, and egui's
/// own widgets are restyled to match.
pub fn set_theme(ctx: &egui::Context, theme: Theme) {
    store(theme, theme.resolve(ctx.system_theme()));
    apply_visuals(ctx, theme);
    ctx.request_repaint();
}

/// Called once a frame: under Auto, picks up a change of the macOS
/// appearance. egui swaps its own styles by itself (see `apply_visuals`);
/// this keeps `p()` in step. Other themes cost one atomic load.
pub fn follow_system(ctx: &egui::Context) {
    let (chosen, shown) = load();
    if chosen != Theme::Auto {
        return;
    }
    let now = chosen.resolve(ctx.system_theme());
    if now != shown {
        store(chosen, now);
        ctx.request_repaint();
    }
}

/// Points egui's visuals at a theme's palette. A fixed theme gives both of
/// egui's styles the same colours and pins the window's appearance. Auto
/// leaves the window following macOS (so its appearance changes reach
/// egui as `RawInput::system_theme`) and gives egui's dark style gray's
/// colours and its light style light's, so egui switches between them.
fn apply_visuals(ctx: &egui::Context, theme: Theme) {
    if theme == Theme::Auto {
        ctx.set_theme(egui::ThemePreference::System);
        ctx.style_mut_of(egui::Theme::Dark, |s| style_for(s, Theme::Auto.resolve(Some(egui::Theme::Dark))));
        ctx.style_mut_of(egui::Theme::Light, |s| style_for(s, Theme::Auto.resolve(Some(egui::Theme::Light))));
    } else {
        ctx.set_theme(if theme.is_light() { egui::Theme::Light } else { egui::Theme::Dark });
        ctx.all_styles_mut(|s| style_for(s, theme));
    }
}

/// Restyles one of egui's styles for a fixed theme. egui's light defaults
/// are the starting point for the light themes so widgets falcon doesn't
/// colour itself match.
fn style_for(s: &mut egui::Style, theme: Theme) {
    let pal = theme.palette();
    s.visuals = if theme.is_light() { egui::Visuals::light() } else { egui::Visuals::dark() };
    let v = &mut s.visuals;
    v.panel_fill = pal.main;
    v.window_fill = pal.panel;
    v.extreme_bg_color = pal.field;
    v.faint_bg_color = pal.panel;
    v.override_text_color = Some(pal.text);
    v.selection.bg_fill = pal.primary.gamma_multiply(0.45);
    v.selection.stroke = Stroke::new(1.0, pal.primary);
    v.hyperlink_color = pal.text_value;
    v.window_corner_radius = CornerRadius::same(10);
    v.window_stroke = Stroke::new(1.0, pal.divider);
    v.menu_corner_radius = CornerRadius::same(8);
    v.text_cursor.stroke = Stroke::new(1.5, pal.primary);
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = RADIUS;
        w.bg_stroke = Stroke::NONE;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, pal.divider);
    v.widgets.inactive.weak_bg_fill = pal.button;
    v.widgets.inactive.bg_fill = pal.button;
    v.widgets.hovered.weak_bg_fill = pal.button_hover;
    v.widgets.hovered.bg_fill = pal.button_hover;
    v.widgets.active.weak_bg_fill = pal.row_selected;
}

pub const RADIUS: CornerRadius = CornerRadius::same(6);
pub const FIELD_RADIUS: CornerRadius = CornerRadius::same(8);

/// Height of the toolbar and the panel header rows.
pub const BAR_HEIGHT: f32 = 48.0;
/// Sidebar row height.
pub const ROW_HEIGHT: f32 = 27.0;
pub const SIDEBAR_WIDTH: f32 = 240.0;
/// How far the sidebar and the inspector can be dragged: Eagle stops
/// both at 200 px; the upper bound keeps the grid usable.
pub const PANEL_WIDTHS: std::ops::RangeInclusive<f32> = 200.0..=480.0;
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
        e if falcon_library::media::is_audio(e) => 0x333333,
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

/// Installs fonts, spacing and the current theme's visuals.
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

    apply_visuals(ctx, current());
    ctx.all_styles_mut(|s| {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG relative luminance.
    fn luminance(c: Color32) -> f32 {
        let lin = |v: u8| {
            let v = v as f32 / 255.0;
            if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
    }

    /// The fixed theme on screen: the chosen one, or what Auto resolved to.
    fn shown() -> Theme {
        load().1
    }

    fn contrast(a: Color32, b: Color32) -> f32 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn text_stands_out_from_every_background() {
        for theme in Theme::all() {
            let p = theme.palette();
            for (name, bg) in [("main", p.main), ("panel", p.panel), ("field", p.field), ("search", p.search), ("row_selected", p.row_selected), ("button", p.button)] {
                assert!(contrast(p.text, bg) > 7.0, "{theme} text on {name}: {}", contrast(p.text, bg));
                assert!(contrast(p.text_value, bg) > 4.5, "{theme} value on {name}");
                // Lightgray's muted text on its 10% overlays (selected
                // rows, buttons) comes out of the stylesheet at 2.9:1.
                let muted = if matches!(name, "row_selected" | "button") { 2.8 } else { 3.0 };
                assert!(contrast(p.text_muted, bg) > muted, "{theme} muted on {name}: {}", contrast(p.text_muted, bg));
            }
            assert!((luminance(p.text) - luminance(p.main)).abs() > 0.5, "{theme}");
            // Disabled and muted text sit between the background and the text.
            assert!(contrast(p.text_disabled, p.main) < contrast(p.text_muted, p.main), "{theme}");
            assert!(contrast(p.text_muted, p.main) < contrast(p.text, p.main), "{theme}");
        }
    }

    #[test]
    fn surfaces_differ_from_the_content_area() {
        for theme in Theme::all() {
            let p = theme.palette();
            for (name, c) in [("panel", p.panel), ("divider", p.divider), ("row_selected", p.row_selected), ("row_hover", p.row_hover), ("card_hover", p.card_hover)] {
                assert_ne!(c, p.main, "{theme} {name}");
            }
            assert_ne!(p.row_selected, p.row_hover, "{theme}");
        }
    }

    #[test]
    fn theme_round_trips_under_eagles_names() {
        for theme in Theme::all() {
            let json = serde_json::to_string(&theme).unwrap();
            assert_eq!(json, format!("\"{}\"", theme.to_string().to_lowercase()));
            assert_eq!(serde_json::from_str::<Theme>(&json).unwrap(), theme);
        }
        assert_eq!(Theme::default(), Theme::Dark);
    }

    #[test]
    fn palettes_line_up_with_themes() {
        for theme in Theme::all() {
            assert_eq!(Theme::from_index(theme as u8), theme);
            if theme != Theme::Auto {
                assert_eq!(PALETTES[theme as usize], theme.palette());
            }
        }
        assert_eq!(PALETTES.len(), Theme::all().len() - 1);
    }

    #[test]
    fn auto_follows_the_system_appearance_like_eagle() {
        assert_eq!(Theme::Auto.resolve(Some(egui::Theme::Dark)), Theme::Gray);
        assert_eq!(Theme::Auto.resolve(Some(egui::Theme::Light)), Theme::Light);
        assert_eq!(Theme::Auto.resolve(None), Theme::Gray);
        for theme in Theme::all().into_iter().filter(|&t| t != Theme::Auto) {
            for system in [None, Some(egui::Theme::Dark), Some(egui::Theme::Light)] {
                assert_eq!(theme.resolve(system), theme);
            }
        }
    }

    #[test]
    fn lightgray_is_a_light_theme_on_its_own_base() {
        assert!(Theme::Lightgray.is_light());
        assert_eq!(LIGHTGRAY.main, rgb(0xe3e4e6));
        assert_eq!(LIGHTGRAY.text, LIGHT.text);
        assert_eq!(LIGHTGRAY.primary, LIGHT.primary);
        assert_eq!(LIGHTGRAY.icon_red, LIGHT.icon_red);
        // Darker than light everywhere but the text and the accents.
        assert!(luminance(LIGHTGRAY.main) < luminance(LIGHT.main));
        assert!(luminance(LIGHTGRAY.panel) < luminance(LIGHT.panel));
        assert!(contrast(LIGHTGRAY.text, LIGHTGRAY.main) > 10.0);
        assert!(contrast(LIGHTGRAY.primary, LIGHTGRAY.main) > 3.0);
    }

    #[test]
    fn eagles_names_for_the_new_themes() {
        assert_eq!(serde_json::to_string(&Theme::Auto).unwrap(), "\"auto\"");
        assert_eq!(serde_json::to_string(&Theme::Lightgray).unwrap(), "\"lightgray\"");
        assert_eq!(serde_json::from_str::<Theme>("\"lightgray\"").unwrap(), Theme::Lightgray);
        assert_eq!(Theme::all()[0], Theme::Auto);
    }

    #[test]
    fn auto_switches_with_the_system_each_frame() {
        let ctx = egui::Context::default();
        set_theme(&ctx, Theme::Auto);
        assert_eq!(current(), Theme::Auto);
        assert_eq!(shown(), Theme::Gray);
        assert_eq!(*p(), GRAY);
        assert_eq!(ctx.options(|o| o.theme_preference), egui::ThemePreference::System);
        // egui holds gray for dark mode and light for light mode.
        assert_eq!(ctx.style_of(egui::Theme::Dark).visuals.panel_fill, GRAY.main);
        assert_eq!(ctx.style_of(egui::Theme::Light).visuals.panel_fill, LIGHT.main);
        for (system, want) in [(egui::Theme::Light, Theme::Light), (egui::Theme::Dark, Theme::Gray)] {
            let input = egui::RawInput { system_theme: Some(system), ..Default::default() };
            ctx.run_ui(input, |_| {}).textures_delta.clear();
            follow_system(&ctx);
            assert_eq!(shown(), want);
            assert_eq!(*p(), want.palette());
            assert_eq!(current(), Theme::Auto);
        }
        // A fixed theme ignores the system.
        set_theme(&ctx, Theme::Blue);
        ctx.run_ui(egui::RawInput { system_theme: Some(egui::Theme::Light), ..Default::default() }, |_| {}).textures_delta.clear();
        follow_system(&ctx);
        assert_eq!(shown(), Theme::Blue);
    }

    #[test]
    fn dark_palette_keeps_the_constants() {
        assert_eq!(DARK.main, MAIN);
        assert_eq!(DARK.icon_none, ICON_NONE);
        assert_eq!(LIGHT.primary, BRAND);
        assert_eq!(GRAY.primary, PRIMARY);
    }

    #[test]
    fn set_theme_switches_the_palette() {
        let ctx = egui::Context::default();
        for theme in [Theme::Light, Theme::Lightgray, Theme::Purple, Theme::Dark] {
            set_theme(&ctx, theme);
            assert_eq!(current(), theme);
            assert_eq!(shown(), theme);
            assert_eq!(*p(), theme.palette());
            assert_eq!(ctx.global_style().visuals.panel_fill, theme.palette().main);
            assert_eq!(ctx.global_style().visuals.dark_mode, !theme.is_light());
            assert_eq!(ctx.theme() == egui::Theme::Light, theme.is_light());
            assert_eq!(ctx.global_style().visuals.override_text_color, Some(theme.palette().text));
        }
    }
}
