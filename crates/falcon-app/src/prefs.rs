//! Preferences: the settings falcon honours, and a window for them laid
//! out after Eagle's (a section list on the left, the section on the
//! right, Save, Apply and Cancel at the bottom).

use std::ops::RangeInclusive;

use eframe::egui::{self, Align2, Color32, CornerRadius, Key, Modifiers, RichText, Sense, Stroke, Ui, vec2};

use crate::shortcuts::{Recorder, Shortcuts};
use crate::theme::{self as t, Theme, font};

/// Eagle's own API port, which falcon must never take.
pub const EAGLE_PORT: u16 = 41595;
/// How many ports falcon tries, from the first one up.
pub const PORT_SPAN: u16 = 5;

/// What falcon remembers as preferences. Saved with the rest of the app
/// state; unknown or missing fields fall back to the defaults.
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Prefs {
    /// Eagle's `habits.hoverZoom`: the zoom button on hovered cards.
    pub hover_zoom: bool,
    /// Eagle's `general.IPTC`: add IPTC/XMP keywords as tags on import.
    pub keywords_as_tags: bool,
    /// Run the local HTTP API while a library is open.
    pub api_enabled: bool,
    /// The first port the API tries.
    pub api_port: u16,
    /// The folder the collect window adds items to (None: no folder).
    /// Chosen in the collect window itself.
    pub collect_folder: Option<String>,
    /// Eagle's `theme.css`: the colour theme.
    pub theme: Theme,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            hover_zoom: true,
            keywords_as_tags: false,
            api_enabled: true,
            api_port: falcon_server::DEFAULT_PORT,
            collect_folder: None,
            theme: Theme::Dark,
        }
    }
}

impl Prefs {
    /// Fixes values that could have been edited by hand in the saved state.
    pub fn sanitized(mut self) -> Self {
        if port_error(self.api_port).is_some() {
            self.api_port = Prefs::default().api_port;
        }
        self
    }

    /// How the app imports into `folders`: everything as Eagle defaults
    /// it, plus the keyword preference.
    pub fn import_options(&self, folders: Vec<String>) -> falcon_library::import::ImportOptions {
        falcon_library::import::ImportOptions { folders, keywords_as_tags: self.keywords_as_tags, ..Default::default() }
    }

    /// The ports to try for the API, or None when it is off.
    pub fn api_ports(&self) -> Option<RangeInclusive<u16>> {
        let first = if port_error(self.api_port).is_none() { self.api_port } else { falcon_server::DEFAULT_PORT };
        self.api_enabled.then_some(first..=first + (PORT_SPAN - 1))
    }

    /// Whether going from `self` to `new` needs the API restarted.
    pub fn api_changed(&self, new: &Prefs) -> bool {
        self.api_ports() != new.api_ports()
    }
}

/// The theme in the saved app state, read on its own so it can be applied
/// at startup without the rest of the state.
pub fn saved_theme(storage: Option<&dyn eframe::Storage>) -> Theme {
    #[derive(serde::Deserialize, Default)]
    #[serde(default)]
    struct Saved {
        prefs: ThemeOnly,
    }
    #[derive(serde::Deserialize, Default)]
    #[serde(default)]
    struct ThemeOnly {
        theme: Theme,
    }
    storage.and_then(|s| eframe::get_value::<Saved>(s, eframe::APP_KEY)).unwrap_or_default().prefs.theme
}

/// The theme to switch back to when the window closes without saving:
/// the last saved or applied one, if a different one is being previewed.
pub fn theme_to_restore(applied: Theme, shown: Theme) -> Option<Theme> {
    (applied != shown).then_some(applied)
}

/// Why a first API port is unusable, if it is.
pub fn port_error(first: u16) -> Option<String> {
    if first < 1024 {
        return Some("Use a port of 1024 or higher".into());
    }
    let Some(last) = first.checked_add(PORT_SPAN - 1) else {
        return Some(format!("Use a port of {} or lower", u16::MAX - (PORT_SPAN - 1)));
    };
    if (first..=last).contains(&EAGLE_PORT) {
        return Some(format!("{first}–{last} would include Eagle's port {EAGLE_PORT}"));
    }
    None
}

/// Parses the port field.
pub fn parse_port(text: &str) -> Result<u16, String> {
    let p: u16 = text.trim().parse().map_err(|_| "Enter a number from 1024 to 65531".to_string())?;
    match port_error(p) {
        Some(e) => Err(e),
        None => Ok(p),
    }
}

/// Panel visibility, edited here but owned by the main window.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Panels {
    pub sidebar: bool,
    pub inspector: bool,
}

/// Everything the window edits, handed back on Save or Apply.
#[derive(Clone, PartialEq, Debug)]
pub struct Draft {
    pub prefs: Prefs,
    pub panels: Panels,
    pub keys: Shortcuts,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Section {
    #[default]
    General,
    Preview,
    Shortcuts,
    Developer,
}

impl Section {
    pub const ALL: [Section; 4] = [Section::General, Section::Preview, Section::Shortcuts, Section::Developer];

    pub fn label(self) -> &'static str {
        match self {
            Section::General => "General",
            Section::Preview => "Preview",
            Section::Shortcuts => "Shortcuts",
            Section::Developer => "Developer",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Section::General => crate::icons::GEAR,
            Section::Preview => crate::icons::EYE,
            Section::Shortcuts => crate::icons::KEYBOARD,
            Section::Developer => crate::icons::CODE,
        }
    }
}

/// The Preferences window. Edits go to a draft; Save and Apply hand it to
/// the app, Cancel (or Esc) drops it, as in Eagle.
#[derive(Default)]
pub struct Window {
    pub open: bool,
    section: Section,
    draft: Option<Draft>,
    port_text: String,
    recorder: Recorder,
    /// The theme last saved or applied; a previewed one reverts to it.
    applied_theme: Theme,
}

/// Size of the window, close to Eagle's.
const SIZE: egui::Vec2 = vec2(720.0, 500.0);
const NAV_WIDTH: f32 = 180.0;

impl Window {
    pub fn show_with(&mut self, draft: Draft) {
        self.port_text = draft.prefs.api_port.to_string();
        self.applied_theme = draft.prefs.theme;
        self.draft = Some(draft);
        self.open = true;
        self.recorder.stop();
    }

    pub fn close(&mut self) {
        self.open = false;
        self.draft = None;
        self.recorder.stop();
    }

    /// Draws the window; returns a draft to apply on Save or Apply.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Draft> {
        if !self.open {
            return None;
        }
        let Some(mut draft) = self.draft.take() else {
            self.open = false;
            return None;
        };
        // A shortcut being recorded takes the key presses first, so Esc
        // and ⌘↩ can be recorded instead of closing the window.
        self.recorder.capture(ctx, &mut draft.keys);
        let port = parse_port(&self.port_text);
        let (save_key, esc) = ctx.input_mut(|i| {
            (i.consume_key(Modifiers::COMMAND, Key::Enter), i.consume_key(Modifiers::NONE, Key::Escape))
        });
        let mut result = None;
        let mut close = esc;
        let frame = egui::Frame::NONE.fill(t::p().panel).stroke(Stroke::new(1.0, t::p().divider)).corner_radius(CornerRadius::same(10));
        egui::Modal::new(egui::Id::unique("preferences")).frame(frame).show(ctx, |ui| {
            ui.set_width(SIZE.x);
            ui.set_height(SIZE.y);
            let full = ui.max_rect();
            let nav = egui::Rect::from_min_size(full.min, vec2(NAV_WIDTH, SIZE.y));
            let body = egui::Rect::from_min_max(egui::pos2(nav.right(), full.top()), egui::pos2(full.left() + SIZE.x, full.top() + SIZE.y));
            let mut nav_ui = ui.new_child(egui::UiBuilder::new().max_rect(nav.shrink2(vec2(12.0, 0.0))));
            self.nav(&mut nav_ui);

            let inner = body.shrink2(vec2(0.0, 8.0)).with_max_x(body.right() - 8.0);
            ui.painter().rect(inner, CornerRadius::same(8), t::p().main, Stroke::new(1.0, t::p().divider), egui::StrokeKind::Inside);
            let header = egui::Rect::from_min_size(inner.min, vec2(inner.width(), t::BAR_HEIGHT));
            ui.painter().text(header.left_center() + vec2(16.0, 0.0), Align2::LEFT_CENTER, self.section.label(), font(14.0), t::p().text);
            let x = egui::Rect::from_center_size(header.right_center() - vec2(24.0, 0.0), vec2(24.0, 24.0));
            let xr = ui.interact(x, ui.scope_id().with("close"), Sense::click());
            if xr.hovered() {
                ui.painter().rect_filled(x, CornerRadius::same(6), t::p().button_hover);
            }
            ui.painter().text(x.center(), Align2::CENTER_CENTER, crate::icons::X, font(14.0), t::p().text_muted);
            close |= xr.clicked();

            let footer = egui::Rect::from_min_max(egui::pos2(inner.left(), inner.bottom() - 52.0), inner.max);
            let content = egui::Rect::from_min_max(egui::pos2(inner.left(), header.bottom()), egui::pos2(inner.right(), footer.top()));
            let mut c = ui.new_child(egui::UiBuilder::new().max_rect(content.shrink2(vec2(16.0, 0.0))));
            egui::ScrollArea::vertical().auto_shrink(false).show(&mut c, |ui| match self.section {
                Section::General => general(ui, &mut draft),
                Section::Preview => preview(ui, &mut draft.prefs),
                Section::Shortcuts => self.recorder.ui(ui, &mut draft.keys),
                Section::Developer => developer(ui, &mut draft.prefs, &mut self.port_text, port.as_ref().err()),
            });

            ui.painter().hline(footer.x_range(), footer.top(), Stroke::new(1.0, t::p().divider));
            let mut f = ui.new_child(egui::UiBuilder::new().max_rect(footer.shrink2(vec2(16.0, 10.0))).layout(egui::Layout::right_to_left(egui::Align::Center)));
            let valid = port.is_ok();
            let save = f.add_enabled(valid, egui::Button::new(RichText::new("Save").color(Color32::WHITE)).fill(t::p().primary)).clicked();
            let apply = f.add_enabled(valid, egui::Button::new("Apply")).clicked();
            close |= f.button("Cancel").clicked();
            if (save || apply || (save_key && valid))
                && let Ok(p) = port
            {
                draft.prefs.api_port = p;
                self.applied_theme = draft.prefs.theme;
                result = Some(draft.clone());
                close |= save || save_key;
            }
        });
        if close {
            if let Some(theme) = theme_to_restore(self.applied_theme, t::current()) {
                t::set_theme(ctx, theme);
            }
            self.close();
        } else {
            self.draft = Some(draft);
        }
        result
    }

    fn nav(&mut self, ui: &mut Ui) {
        let (title, _) = ui.allocate_exact_size(vec2(ui.available_width(), t::BAR_HEIGHT), Sense::hover());
        ui.painter().text(title.left_center(), Align2::LEFT_CENTER, "Preferences", font(14.0), t::p().text);
        for s in Section::ALL {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
            let fill = if s == self.section {
                t::p().row_selected
            } else if resp.hovered() {
                t::p().row_hover
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
            ui.painter().text(rect.left_center() + vec2(14.0, 0.0), Align2::CENTER_CENTER, s.icon(), font(15.0), t::p().text);
            ui.painter().text(rect.left_center() + vec2(30.0, 0.0), Align2::LEFT_CENTER, s.label(), font(13.0), t::p().text);
            if resp.clicked() {
                self.section = s;
                self.recorder.stop();
            }
        }
    }
}

fn block(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    ui.add_space(4.0);
    ui.label(RichText::new(title).strong().size(13.0));
    ui.add_space(2.0);
    ui.indent(title, add);
    ui.add_space(10.0);
}

fn note(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(11.0).color(t::p().text_muted));
}

fn general(ui: &mut Ui, d: &mut Draft) {
    block(ui, "Theme", |ui| {
        ui.horizontal(|ui| {
            for theme in Theme::all() {
                if ui.selectable_value(&mut d.prefs.theme, theme, theme.to_string()).changed() {
                    // Previewed at once; Cancel switches back.
                    t::set_theme(ui.ctx(), theme);
                }
            }
        });
    });
    block(ui, "Window", |ui| {
        ui.checkbox(&mut d.panels.sidebar, "Show the sidebar");
        ui.checkbox(&mut d.panels.inspector, "Show the inspector");
    });
    block(ui, "Import", |ui| {
        ui.checkbox(&mut d.prefs.keywords_as_tags, "Add IPTC and XMP keywords as tags when importing images");
    });
}

fn preview(ui: &mut Ui, p: &mut Prefs) {
    block(ui, "Hover zoom", |ui| {
        ui.radio_value(&mut p.hover_zoom, true, "Show the zoom button on hovered cards");
        ui.radio_value(&mut p.hover_zoom, false, "Hide it");
        note(ui, "Holding Z shows the preview either way.");
    });
}

fn developer(ui: &mut Ui, p: &mut Prefs, port_text: &mut String, error: Option<&String>) {
    block(ui, "HTTP API", |ui| {
        ui.checkbox(&mut p.api_enabled, "Run the local API while a library is open");
        ui.add_enabled_ui(p.api_enabled, |ui| {
            ui.horizontal(|ui| {
                ui.label("First port");
                ui.add(egui::TextEdit::singleline(port_text).desired_width(70.0));
            });
            match error {
                Some(e) => {
                    ui.label(RichText::new(e).size(11.0).color(t::p().icon_red));
                }
                None => {
                    let first = parse_port(port_text).unwrap_or(p.api_port);
                    note(ui, &format!("falcon tries ports {first} to {} and never uses Eagle's {EAGLE_PORT}.", first + PORT_SPAN - 1));
                }
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let p = Prefs::default();
        assert!(p.hover_zoom, "Eagle's hoverZoom defaults to on");
        assert!(!p.keywords_as_tags, "Eagle's IPTC defaults to off");
        assert_eq!(p.api_ports(), Some(41596..=41600));
    }

    #[test]
    fn imports_follow_the_keyword_setting() {
        let mut p = Prefs::default();
        let o = p.import_options(vec!["F1".into()]);
        assert_eq!((o.folders.as_slice(), o.keywords_as_tags, o.palette), (["F1".to_string()].as_slice(), false, true));
        p.keywords_as_tags = true;
        assert!(p.import_options(Vec::new()).keywords_as_tags);
    }

    #[test]
    fn ports_never_reach_eagles() {
        assert!(port_error(41596).is_none());
        assert!(port_error(41590).is_none(), "41590–41594 stops short of 41595");
        for p in 41591..=41595 {
            assert!(port_error(p).is_some(), "{p}");
        }
        assert!(port_error(80).is_some());
        assert!(port_error(65531).is_none());
        assert!(port_error(65532).is_some());
        assert_eq!(parse_port(" 42000 "), Ok(42000));
        assert!(parse_port("abc").is_err());
        assert!(parse_port("41595").is_err());
        for p in [1024, 41590, 41596, 50000, 65531] {
            let prefs = Prefs { api_port: p, ..Default::default() };
            assert!(!prefs.api_ports().unwrap().contains(&EAGLE_PORT));
        }
    }

    #[test]
    fn sanitize_and_api_changes() {
        let bad = Prefs { api_port: 41594, ..Default::default() }.sanitized();
        assert_eq!(bad.api_port, 41596);
        let p = Prefs::default();
        assert!(!p.api_changed(&Prefs { hover_zoom: false, ..p.clone() }));
        assert!(p.api_changed(&Prefs { api_enabled: false, ..p.clone() }));
        assert!(p.api_changed(&Prefs { api_port: 42000, ..p.clone() }));
        assert_eq!(Prefs { api_enabled: false, ..p }.api_ports(), None);
    }

    #[test]
    fn theme_defaults_and_restores() {
        assert_eq!(Prefs::default().theme, Theme::Dark);
        let p: Prefs = serde_json::from_str(r#"{"theme":"purple"}"#).unwrap();
        assert_eq!(p.theme, Theme::Purple);
        assert_eq!(theme_to_restore(Theme::Dark, Theme::Light), Some(Theme::Dark));
        assert_eq!(theme_to_restore(Theme::Blue, Theme::Blue), None);
    }

    /// In-memory stand-in for eframe's storage.
    struct Mem(Option<String>);

    impl eframe::Storage for Mem {
        fn get_string(&self, _key: &str) -> Option<String> {
            self.0.clone()
        }
        fn set_string(&mut self, _key: &str, value: String) {
            self.0 = Some(value);
        }
        fn remove_string(&mut self, _key: &str) {
            self.0 = None;
        }
        fn flush(&mut self) {}
    }

    #[test]
    fn saved_theme_reads_only_the_theme() {
        // The app state has many more fields than the theme reader knows.
        #[derive(serde::Serialize)]
        struct State {
            library: Option<String>,
            tile: f32,
            prefs: Prefs,
            recent: Vec<String>,
        }
        let mut mem = Mem(None);
        let state = State { library: Some("/x.library".into()), tile: 200.0, prefs: Prefs { theme: Theme::Gray, ..Default::default() }, recent: vec!["/y".into()] };
        eframe::set_value(&mut mem, eframe::APP_KEY, &state);
        assert_eq!(saved_theme(Some(&mem)), Theme::Gray);
        assert_eq!(saved_theme(Some(&Mem(None))), Theme::Dark);
        assert_eq!(saved_theme(Some(&Mem(Some("garbage".into())))), Theme::Dark);
        assert_eq!(saved_theme(None), Theme::Dark);
    }

    #[test]
    fn missing_fields_take_defaults() {
        let p: Prefs = serde_json::from_str(r#"{"hover_zoom":false}"#).unwrap();
        assert!(!p.hover_zoom);
        assert_eq!(p.api_port, 41596);
        assert!(p.api_enabled);
    }
}
