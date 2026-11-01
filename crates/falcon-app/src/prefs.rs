//! Preferences: the settings falcon honours, and a window for them laid
//! out after Eagle's (a search box and the pane list on the left, the pane
//! on the right, Save, Apply and Cancel at the bottom). Each pane is one
//! function that adds blocks of settings to a `Page`; the search shows the
//! matching blocks of every pane.

use std::ops::RangeInclusive;

use eframe::egui::{self, Align2, Color32, CornerRadius, Key, Modifiers, RichText, Sense, Stroke, Ui, vec2};

use crate::applock::{self, Secret};
use crate::logic::Scope;
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
    /// Eagle's `notification.notification.when.repeatImage`: ask what to
    /// do with imported files already in the library. Off imports them
    /// as new items without asking, as Eagle does.
    pub notify_duplicates: bool,
    /// Eagle's `general.autoSelect`: select the new items when an import
    /// finishes.
    pub auto_select: bool,
    /// Eagle's `font.autoTag`: add a font's style (Regular, Bold, Italic...)
    /// as a tag on import.
    pub font_style_tags: bool,
    /// Run the local HTTP API while a library is open.
    pub api_enabled: bool,
    /// The first port the API tries.
    pub api_port: u16,
    /// The folder the collect window adds items to (None: no folder).
    /// Chosen in the collect window itself.
    pub collect_folder: Option<String>,
    /// Eagle's `theme.css`: the colour theme.
    pub theme: Theme,
    /// Eagle's `general.zoom`: the size of the whole interface, in percent
    /// (`ZOOM_STEPS`).
    pub ui_zoom: u16,
    /// Eagle's `general.showSidebarBadge`: item counts beside sidebar rows.
    pub sidebar_counts: bool,
    /// Eagle's `habits.dblclickSidebarItem`: what double-clicking a folder
    /// or smart folder in the sidebar does.
    pub sidebar_double_click: SidebarDoubleClick,
    /// Eagle's `sidebar.*`: which sidebar sections are shown.
    pub sidebar: SidebarSections,
    /// Eagle's `habits.doubleclick`: whether double-clicking an item opens
    /// it in falcon's viewer or in its default app.
    pub double_click: DoubleClick,
    /// Eagle's `habits.keyspace`: what the viewer key (Space unless
    /// rebound) does outside the viewer.
    pub space: SpaceKey,
    /// Eagle's `habits.videoScrollBehavior`: whether the wheel over a video
    /// seeks or sets the volume.
    pub video_scroll: VideoScroll,
    /// Eagle's `video.zoomFill`: enlarge videos to fill the viewer.
    pub video_fill: bool,
    /// Eagle's `video.autoPlay`: start playing when a video opens.
    pub video_autoplay: bool,
    /// Eagle's `video.rememberPosition`: resume where each video was left
    /// (saved per item in the app state, as Eagle keeps it per item).
    pub video_resume: bool,
    /// Eagle's `video.loopShortVideo`: always loop clips of 30 s or less.
    pub video_loop_short: bool,
    /// Eagle's `notification.notification.enable`: the switch over every
    /// pop-up notice below (and `notify_duplicates`).
    pub notifications: bool,
    /// The "Added N" note when an import finishes (falcon's own; failures
    /// are always reported).
    pub notify_import_done: bool,
    /// Eagle's `privacy.*`: the app password, if one is set (`applock`).
    pub app_lock: Option<Secret>,
}

/// What the wheel does over a playing video.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum VideoScroll {
    /// Seeks (Eagle's default).
    #[default]
    Progress,
    Volume,
}

/// What double-clicking an item in the grid does.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum DoubleClick {
    /// Opens it in falcon's viewer (Eagle's default).
    #[default]
    Viewer,
    /// Opens it in its default app.
    External,
}

/// What Space does in the grid.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum SpaceKey {
    /// Opens the selected item in falcon's viewer (Eagle's default).
    #[default]
    Preview,
    /// Shows the selected items in macOS Quick Look.
    QuickLook,
    /// Scrolls the grid down a page.
    Scroll,
}

/// What double-clicking a sidebar folder does.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum SidebarDoubleClick {
    Rename,
    /// Expands or collapses its subfolders (Eagle's default).
    #[default]
    Collapse,
}

/// The sidebar sections that can be hidden, all shown by default as in
/// Eagle. (Eagle's Community entry has no counterpart; All, All Tags and
/// Trash are always shown.)
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(default)]
pub struct SidebarSections {
    pub uncategorized: bool,
    pub untagged: bool,
    pub recent: bool,
    pub random: bool,
    pub quick_access: bool,
    pub smart_folders: bool,
    pub folders: bool,
}

impl Default for SidebarSections {
    fn default() -> Self {
        SidebarSections { uncategorized: true, untagged: true, recent: true, random: true, quick_access: true, smart_folders: true, folders: true }
    }
}

impl SidebarSections {
    /// Whether the entry for `scope` is shown among the sidebar's top rows.
    pub fn shows(&self, scope: &Scope) -> bool {
        match scope {
            Scope::Uncategorized => self.uncategorized,
            Scope::Untagged => self.untagged,
            Scope::Recent => self.recent,
            Scope::Random(_) => self.random,
            _ => true,
        }
    }
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            hover_zoom: true,
            keywords_as_tags: false,
            notify_duplicates: true,
            auto_select: true,
            font_style_tags: true,
            api_enabled: true,
            api_port: falcon_server::DEFAULT_PORT,
            collect_folder: None,
            theme: Theme::Dark,
            ui_zoom: 100,
            sidebar_counts: true,
            sidebar_double_click: SidebarDoubleClick::default(),
            sidebar: SidebarSections::default(),
            double_click: DoubleClick::default(),
            space: SpaceKey::default(),
            video_scroll: VideoScroll::default(),
            video_fill: false,
            video_autoplay: true,
            video_resume: true,
            video_loop_short: true,
            notifications: true,
            notify_import_done: true,
            app_lock: None,
        }
    }
}

/// The interface sizes offered, in percent: Eagle's list up to 150%.
pub const ZOOM_STEPS: [u16; 8] = [50, 75, 80, 90, 100, 110, 125, 150];

/// egui's zoom factor for an interface size in percent, kept within
/// 50–150%.
pub fn zoom_factor(percent: u16) -> f32 {
    f32::from(percent.clamp(ZOOM_STEPS[0], ZOOM_STEPS[ZOOM_STEPS.len() - 1])) / 100.0
}

impl Prefs {
    /// Fixes values that could have been edited by hand in the saved state.
    pub fn sanitized(mut self) -> Self {
        if port_error(self.api_port).is_some() {
            self.api_port = Prefs::default().api_port;
        }
        self.ui_zoom = self.ui_zoom.clamp(ZOOM_STEPS[0], ZOOM_STEPS[ZOOM_STEPS.len() - 1]);
        self
    }

    /// How the app imports into `folders`: everything as Eagle defaults
    /// it, plus the keyword and font style preferences. Items take the
    /// folders' auto-tags, as they do when imported or pasted in Eagle's
    /// window.
    pub fn import_options(&self, folders: Vec<String>) -> falcon_library::import::ImportOptions {
        falcon_library::import::ImportOptions {
            folders,
            keywords_as_tags: self.keywords_as_tags,
            font_weight_tag: self.font_style_tags,
            folder_auto_tags: true,
            ..Default::default()
        }
    }

    /// Whether imported files already in the library are held for the
    /// duplicate dialog: only with notices on, as in Eagle.
    pub fn asks_duplicates(&self) -> bool {
        self.notifications && self.notify_duplicates
    }

    /// Whether a finished import says how many items it added.
    pub fn tells_import_done(&self) -> bool {
        self.notifications && self.notify_import_done
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

/// The panes, in Eagle's order. Eagle's Screenshot, AI, MCP and Proxy
/// panes have no counterpart (falcon has no capture, plugins or network
/// features), and a pane appears only once it has a setting that works.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Section {
    #[default]
    General,
    Sidebar,
    Controls,
    Preview,
    Shortcuts,
    Notification,
    Password,
    Developer,
}

impl Section {
    pub const ALL: [Section; 8] = [Section::General, Section::Sidebar, Section::Controls, Section::Preview, Section::Shortcuts, Section::Notification, Section::Password, Section::Developer];

    pub fn label(self) -> &'static str {
        match self {
            Section::General => "General",
            Section::Sidebar => "Sidebar",
            Section::Controls => "Controls",
            Section::Preview => "Preview",
            Section::Shortcuts => "Shortcuts",
            Section::Notification => "Notification",
            Section::Password => "Password",
            Section::Developer => "Developer",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Section::General => crate::icons::GEAR,
            Section::Sidebar => crate::icons::SIDEBAR_SIMPLE,
            Section::Controls => crate::icons::MOUSE,
            Section::Preview => crate::icons::EYE,
            Section::Shortcuts => crate::icons::KEYBOARD,
            Section::Notification => crate::icons::BELL,
            Section::Password => crate::icons::LOCK_SIMPLE,
            Section::Developer => crate::icons::CODE,
        }
    }

    /// Eagle separates its pane list into groups; a line goes above the
    /// first pane of each group.
    fn group(self) -> u8 {
        match self {
            Section::General | Section::Sidebar => 0,
            Section::Controls | Section::Preview | Section::Shortcuts => 1,
            Section::Notification | Section::Password => 2,
            Section::Developer => 3,
        }
    }
}

/// Whether a block of settings matches the search: every word typed must
/// appear in its pane's name, its title or one of its keywords (the labels
/// of its settings and other words for them), ignoring case. An empty
/// search matches everything.
pub fn matches(query: &str, fields: &[&str]) -> bool {
    let hay = fields.join(" ").to_lowercase();
    query.split_whitespace().all(|w| hay.contains(&w.to_lowercase()))
}

/// The Preferences window. Edits go to a draft; Save and Apply hand it to
/// the app, Cancel (or Esc) drops it, as in Eagle.
#[derive(Default)]
pub struct Window {
    pub open: bool,
    section: Section,
    /// The search across every pane; while it has text the panes give way
    /// to the blocks that match.
    query: String,
    draft: Option<Draft>,
    port_text: String,
    recorder: Recorder,
    password: PasswordForm,
    /// The theme and interface size last saved or applied; previewed
    /// ones revert to them.
    applied_theme: Theme,
    applied_zoom: u16,
}

/// Size of the window, close to Eagle's.
const SIZE: egui::Vec2 = vec2(720.0, 500.0);
const NAV_WIDTH: f32 = 180.0;

/// What a pane draws into: blocks of settings, filtered by the search.
/// Each pane is one function below that adds its blocks in order; a new
/// setting goes into its block (adding its words to the block's keywords
/// so the search finds it), or a new block where Eagle has it.
pub struct Page<'u> {
    ui: &'u mut Ui,
    pane: Section,
    query: String,
    /// How many blocks matched the search, and whether this pane's name
    /// has been drawn above its first one.
    hits: usize,
    headed: bool,
    /// A pane to switch to, asked for from a search result.
    goto: Option<Section>,
}

impl Page<'_> {
    fn searching(&self) -> bool {
        !self.query.trim().is_empty()
    }

    /// Draws a titled block of settings if it matches the search.
    pub fn block(&mut self, title: &str, keywords: &[&str], add: impl FnOnce(&mut Ui)) {
        let mut fields = vec![self.pane.label(), title];
        fields.extend_from_slice(keywords);
        if !matches(&self.query, &fields) {
            return;
        }
        if self.searching() && !self.headed {
            self.headed = true;
            self.ui.add_space(if self.hits == 0 { 4.0 } else { 8.0 });
            self.ui.label(RichText::new(self.pane.label().to_uppercase()).size(11.0).color(t::p().text_muted));
        }
        self.hits += 1;
        let ui = &mut *self.ui;
        ui.add_space(4.0);
        ui.label(RichText::new(title).strong().size(13.0));
        ui.add_space(2.0);
        ui.indent(title, add);
        ui.add_space(10.0);
    }
}

impl Window {
    pub fn show_with(&mut self, draft: Draft) {
        self.port_text = draft.prefs.api_port.to_string();
        self.applied_theme = draft.prefs.theme;
        self.applied_zoom = draft.prefs.ui_zoom;
        self.draft = Some(draft);
        self.open = true;
        self.query.clear();
        self.recorder.stop();
        self.password = PasswordForm::default();
    }

    pub fn close(&mut self) {
        self.open = false;
        self.draft = None;
        self.recorder.stop();
    }

    /// The pane shown last, remembered across launches as Eagle does.
    pub fn pane(&self) -> Section {
        self.section
    }

    pub fn set_pane(&mut self, s: Section) {
        self.section = s;
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

            let searching = !self.query.trim().is_empty();
            let inner = body.shrink2(vec2(0.0, 8.0)).with_max_x(body.right() - 8.0);
            ui.painter().rect(inner, CornerRadius::same(8), t::p().main, Stroke::new(1.0, t::p().divider), egui::StrokeKind::Inside);
            let header = egui::Rect::from_min_size(inner.min, vec2(inner.width(), t::BAR_HEIGHT));
            let heading = if searching { "Search" } else { self.section.label() };
            ui.painter().text(header.left_center() + vec2(16.0, 0.0), Align2::LEFT_CENTER, heading, font(14.0), t::p().text);
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
            let port_error = port.as_ref().err().cloned();
            let goto = egui::ScrollArea::vertical().id_salt(("prefs", searching, self.section)).auto_shrink(false).show(&mut c, |ui| {
                let panes: Vec<Section> = if searching { Section::ALL.to_vec() } else { vec![self.section] };
                let mut hits = 0;
                let mut goto = None;
                for s in panes {
                    let mut page = Page { ui, pane: s, query: self.query.clone(), hits, headed: false, goto: None };
                    self.pane_ui(&mut page, &mut draft, port_error.as_ref());
                    hits = page.hits;
                    goto = goto.or(page.goto);
                }
                if searching && hits == 0 {
                    ui.add_space(40.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("No settings match").size(14.0).color(t::p().text));
                        note(ui, "Try another word, or clear the search to browse the panes.");
                    });
                }
                goto
            });
            if let Some(s) = goto.inner {
                self.section = s;
                self.query.clear();
            }

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
                self.applied_zoom = draft.prefs.ui_zoom;
                result = Some(draft.clone());
                close |= save || save_key;
            }
        });
        if close {
            if let Some(theme) = theme_to_restore(self.applied_theme, t::current()) {
                t::set_theme(ctx, theme);
            }
            ctx.set_zoom_factor(zoom_factor(self.applied_zoom));
            self.close();
        } else {
            self.draft = Some(draft);
        }
        result
    }

    /// Draws one pane's blocks.
    fn pane_ui(&mut self, p: &mut Page, d: &mut Draft, port_error: Option<&String>) {
        match p.pane {
            Section::General => general(p, d),
            Section::Sidebar => sidebar(p, d),
            Section::Controls => controls(p, d),
            Section::Preview => preview(p, d),
            Section::Shortcuts => self.shortcuts(p, d),
            Section::Notification => notification(p, d),
            Section::Password => password(p, d, &mut self.password),
            Section::Developer => developer(p, d, &mut self.port_text, port_error),
        }
    }

    fn shortcuts(&mut self, p: &mut Page, d: &mut Draft) {
        if p.searching() {
            // The shortcut list has its own search, in its pane.
            let mut go = false;
            p.block("Keyboard shortcuts", &["keys", "hotkey", "chord", "key bindings"], |ui| {
                go = ui.button("Show the Shortcuts pane").clicked();
            });
            if go {
                p.goto = Some(Section::Shortcuts);
            }
        } else {
            self.recorder.ui(p.ui, &mut d.keys);
        }
    }

    fn nav(&mut self, ui: &mut Ui) {
        let (title, _) = ui.allocate_exact_size(vec2(ui.available_width(), t::BAR_HEIGHT), Sense::hover());
        ui.painter().text(title.left_center(), Align2::LEFT_CENTER, "Preferences", font(14.0), t::p().text);
        let search = ui.add(
            egui::TextEdit::singleline(&mut self.query)
                .hint_text(RichText::new(format!("{}  Search", crate::icons::MAGNIFYING_GLASS)).color(t::p().text_muted))
                .desired_width(f32::INFINITY),
        );
        if search.changed() {
            self.recorder.stop();
        }
        ui.add_space(6.0);
        let searching = !self.query.trim().is_empty();
        let mut group = Section::ALL[0].group();
        for s in Section::ALL {
            if s.group() != group {
                group = s.group();
                let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 9.0), Sense::hover());
                ui.painter().hline(r.x_range().shrink(6.0), r.center().y, Stroke::new(1.0, t::p().divider));
            }
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
            let fill = if s == self.section && !searching {
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
                self.query.clear();
                self.recorder.stop();
            }
        }
    }
}

fn note(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(11.0).color(t::p().text_muted));
}

/// General: appearance, the window, and how imports behave.
fn general(p: &mut Page, d: &mut Draft) {
    let keywords = ["theme", "colour", "color", "dark", "light", "gray", "blue", "purple", "auto", "zoom", "size", "scale", "interface", "counts", "badge", "sidebar", "numbers"];
    p.block("Appearance", &keywords, |ui| {
        ui.horizontal(|ui| {
            for theme in Theme::all() {
                if ui.selectable_value(&mut d.prefs.theme, theme, theme.to_string()).changed() {
                    // Previewed at once; Cancel switches back.
                    t::set_theme(ui.ctx(), theme);
                }
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Interface zoom");
            let before = d.prefs.ui_zoom;
            egui::ComboBox::from_id_salt("ui-zoom").selected_text(format!("{}%", d.prefs.ui_zoom)).show_ui(ui, |ui| {
                for z in ZOOM_STEPS {
                    ui.selectable_value(&mut d.prefs.ui_zoom, z, format!("{z}%"));
                }
            });
            if d.prefs.ui_zoom != before {
                // Previewed at once, like the theme; Cancel switches back.
                ui.ctx().set_zoom_factor(zoom_factor(d.prefs.ui_zoom));
            }
        });
        ui.checkbox(&mut d.prefs.sidebar_counts, "Show item counts in the sidebar");
    });
    p.block("Window", &["sidebar", "inspector", "panel", "show"], |ui| {
        ui.checkbox(&mut d.panels.sidebar, "Show the sidebar");
        ui.checkbox(&mut d.panels.inspector, "Show the inspector");
    });
    p.block("Importing", &["import", "collect", "keywords", "iptc", "xmp", "tags", "duplicate", "already", "select", "new items"], |ui| {
        ui.checkbox(&mut d.prefs.auto_select, "Select the new items after importing");
        ui.checkbox(&mut d.prefs.notify_duplicates, "Ask what to do when an imported file is already in the library");
        note(ui, "When off, such files are imported as new items.");
        ui.checkbox(&mut d.prefs.keywords_as_tags, "Add IPTC and XMP keywords as tags when importing images");
    });
}

/// Sidebar: what double-clicking does, and which sections are shown.
fn sidebar(p: &mut Page, d: &mut Draft) {
    p.block("Double-clicking a folder", &["double click", "rename", "expand", "collapse", "folder"], |ui| {
        ui.radio_value(&mut d.prefs.sidebar_double_click, SidebarDoubleClick::Rename, "Renames it");
        ui.radio_value(&mut d.prefs.sidebar_double_click, SidebarDoubleClick::Collapse, "Expands or collapses it");
    });
    let keywords = ["sections", "show", "hide", "uncategorized", "untagged", "recently used", "random", "quick access", "smart folders", "folders"];
    p.block("Sections", &keywords, |ui| {
        let s = &mut d.prefs.sidebar;
        ui.checkbox(&mut s.uncategorized, "Uncategorized");
        ui.checkbox(&mut s.untagged, "Untagged");
        ui.checkbox(&mut s.recent, "Recently Used");
        ui.checkbox(&mut s.random, "Random");
        ui.checkbox(&mut s.quick_access, "Quick Access");
        ui.checkbox(&mut s.smart_folders, "Smart Folders");
        ui.checkbox(&mut s.folders, "Folders");
    });
}

/// Controls: what the mouse and the keyboard do.
fn controls(p: &mut Page, d: &mut Draft) {
    p.block("Mouse", &["hover", "zoom button", "cards", "double click", "open", "default app", "wheel", "scroll"], |ui| {
        ui.label("Zoom button on hovered cards");
        ui.radio_value(&mut d.prefs.hover_zoom, true, "Show");
        ui.radio_value(&mut d.prefs.hover_zoom, false, "Hide");
        note(ui, "Holding Z shows the preview either way.");
        ui.add_space(6.0);
        ui.label("Double-clicking an item");
        ui.radio_value(&mut d.prefs.double_click, DoubleClick::Viewer, "Opens it in falcon");
        ui.radio_value(&mut d.prefs.double_click, DoubleClick::External, "Opens it in its default app");
    });
    p.block("Keyboard", &["space", "spacebar", "preview", "quick look", "scroll"], |ui| {
        ui.label("Space in the grid");
        ui.radio_value(&mut d.prefs.space, SpaceKey::Preview, "Opens the viewer");
        if cfg!(target_os = "macos") {
            ui.radio_value(&mut d.prefs.space, SpaceKey::QuickLook, "Shows Quick Look");
        }
        ui.radio_value(&mut d.prefs.space, SpaceKey::Scroll, "Scrolls down a page");
        note(ui, "In the viewer Space still closes it, or plays and pauses media.");
    });
}

/// Preview: how the viewer shows each kind of file. Eagle's order is
/// image, video, GIF, then fonts.
fn preview(p: &mut Page, d: &mut Draft) {
    let keywords = ["video", "movie", "wheel", "scroll", "progress", "volume", "stretch", "fill", "autoplay", "play", "position", "resume", "loop", "short"];
    p.block("Video", &keywords, |ui| {
        let v = &mut d.prefs;
        ui.label("Scrolling over a video");
        ui.radio_value(&mut v.video_scroll, VideoScroll::Progress, "Moves through it");
        ui.radio_value(&mut v.video_scroll, VideoScroll::Volume, "Changes the volume");
        ui.add_space(4.0);
        ui.checkbox(&mut v.video_fill, "Stretch videos to fill the viewer");
        ui.checkbox(&mut v.video_autoplay, "Play videos when they open");
        ui.checkbox(&mut v.video_resume, "Resume where each video was left");
        ui.checkbox(&mut v.video_loop_short, "Always loop clips of 30 seconds or less");
    });
    p.block("Fonts", &["font", "style", "regular", "bold", "italic", "tags", "import"], |ui| {
        ui.checkbox(&mut d.prefs.font_style_tags, "Add style tags (Regular, Bold, Italic) when importing fonts");
    });
}

/// Notification: which notices falcon shows.
fn notification(p: &mut Page, d: &mut Draft) {
    p.block("Pop-ups", &["notice", "notification", "pop-up", "duplicate", "already", "import", "added", "finished"], |ui| {
        let n = &mut d.prefs;
        ui.checkbox(&mut n.notifications, "Show notices");
        ui.add_enabled_ui(n.notifications, |ui| {
            ui.indent("notices", |ui| {
                ui.checkbox(&mut n.notify_duplicates, "Ask what to do when an imported file is already in the library");
                ui.checkbox(&mut n.notify_import_done, "Say how many items an import added");
            });
        });
        note(ui, "Import failures are always reported. falcon plays no sound effects.");
    });
}

/// What the Password pane is asking for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PasswordMode {
    Set,
    Change,
    Remove,
}

/// The Password pane's fields.
#[derive(Default)]
struct PasswordForm {
    mode: Option<PasswordMode>,
    old: String,
    new: String,
    again: String,
    hint: String,
    error: Option<&'static str>,
}

/// Checks the Password pane's form against the current password and
/// returns what it changes the stored password to.
fn submit_password(form: &PasswordForm, current: Option<&Secret>) -> Result<Option<Secret>, &'static str> {
    let Some(mode) = form.mode else { return Ok(current.cloned()) };
    if mode != PasswordMode::Set && !current.is_some_and(|s| s.matches(&form.old)) {
        return Err(if mode == PasswordMode::Remove { "The password is wrong." } else { "The old password is wrong." });
    }
    if mode == PasswordMode::Remove {
        return Ok(None);
    }
    applock::check_new(&form.new, &form.again)?;
    Ok(Some(Secret::new(&form.new, &form.hint)))
}

/// Password: lock falcon behind a password.
fn password(p: &mut Page, d: &mut Draft, form: &mut PasswordForm) {
    let keywords = ["password", "lock", "privacy", "security", "launch", "hint"];
    p.block("App password", &keywords, |ui| {
        let set = d.prefs.app_lock.is_some();
        if !set {
            let mut on = form.mode == Some(PasswordMode::Set);
            if ui.checkbox(&mut on, "Lock falcon with a password").changed() {
                *form = PasswordForm { mode: on.then_some(PasswordMode::Set), ..Default::default() };
            }
        } else if form.mode.is_none() {
            ui.label("falcon asks for its password when it opens, and after Lock Now (⌥⌘L).");
            ui.horizontal(|ui| {
                if ui.button("Change Password…").clicked() {
                    *form = PasswordForm { mode: Some(PasswordMode::Change), hint: d.prefs.app_lock.as_ref().map(|s| s.hint.clone()).unwrap_or_default(), ..Default::default() };
                }
                if ui.button("Remove Password…").clicked() {
                    *form = PasswordForm { mode: Some(PasswordMode::Remove), ..Default::default() };
                }
            });
        }
        let Some(mode) = form.mode else {
            note(ui, "The password guards this window only; it is kept in falcon's settings, not in the library.");
            return;
        };
        ui.add_space(4.0);
        let field = |ui: &mut Ui, label: &str, text: &mut String, secret: bool| {
            ui.horizontal(|ui| {
                ui.add_sized([130.0, 20.0], egui::Label::new(label));
                ui.add(egui::TextEdit::singleline(text).password(secret).desired_width(200.0));
            });
        };
        if mode != PasswordMode::Set {
            field(ui, if mode == PasswordMode::Remove { "Password" } else { "Old password" }, &mut form.old, true);
        }
        if mode != PasswordMode::Remove {
            field(ui, "New password", &mut form.new, true);
            field(ui, "Again", &mut form.again, true);
            field(ui, "Hint (optional)", &mut form.hint, false);
        }
        if let Some(e) = form.error {
            ui.label(RichText::new(e).size(11.0).color(t::p().icon_red));
        }
        ui.horizontal(|ui| {
            let label = match mode {
                PasswordMode::Set => "Set Password",
                PasswordMode::Change => "Change Password",
                PasswordMode::Remove => "Remove Password",
            };
            if ui.button(label).clicked() {
                match submit_password(form, d.prefs.app_lock.as_ref()) {
                    Ok(secret) => {
                        d.prefs.app_lock = secret;
                        *form = PasswordForm::default();
                    }
                    Err(e) => form.error = Some(e),
                }
            }
            if mode != PasswordMode::Set && ui.button("Cancel").clicked() {
                *form = PasswordForm::default();
            }
        });
        note(ui, "Takes effect when you Save or Apply.");
    });
}

/// Developer: the local HTTP API.
fn developer(p: &mut Page, d: &mut Draft, port_text: &mut String, error: Option<&String>) {
    let pr = &mut d.prefs;
    p.block("HTTP API", &["api", "server", "port", "local", "plugin"], |ui| {
        ui.checkbox(&mut pr.api_enabled, "Run the local API while a library is open");
        ui.add_enabled_ui(pr.api_enabled, |ui| {
            ui.horizontal(|ui| {
                ui.label("First port");
                ui.add(egui::TextEdit::singleline(port_text).desired_width(70.0));
            });
            match error {
                Some(e) => {
                    ui.label(RichText::new(e).size(11.0).color(t::p().icon_red));
                }
                None => {
                    let first = parse_port(port_text).unwrap_or(pr.api_port);
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
        assert!(p.notify_duplicates, "Eagle's repeatImage notification defaults to on");
        assert!(p.auto_select, "Eagle's autoSelect defaults to on");
        assert!(p.font_style_tags, "Eagle's font autoTag defaults to on");
        assert_eq!(p.api_ports(), Some(41596..=41600));
    }

    #[test]
    fn imports_follow_the_keyword_setting() {
        let mut p = Prefs::default();
        let o = p.import_options(vec!["F1".into()]);
        assert_eq!((o.folders.as_slice(), o.keywords_as_tags, o.palette), (["F1".to_string()].as_slice(), false, true));
        assert!(o.folder_auto_tags, "items imported in the window take folder auto-tags");
        p.keywords_as_tags = true;
        assert!(p.import_options(Vec::new()).keywords_as_tags);
    }

    #[test]
    fn imports_follow_the_font_setting() {
        let mut p = Prefs::default();
        assert!(p.import_options(Vec::new()).font_weight_tag);
        p.font_style_tags = false;
        assert!(!p.import_options(Vec::new()).font_weight_tag);
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
    fn zoom_stays_in_range() {
        assert_eq!(Prefs::default().ui_zoom, 100, "Eagle's zoom defaults to 100%");
        assert!(Prefs::default().sidebar_counts, "Eagle's showSidebarBadge defaults to on");
        assert_eq!(zoom_factor(100), 1.0);
        assert_eq!(zoom_factor(125), 1.25);
        assert_eq!(zoom_factor(10), 0.5);
        assert_eq!(zoom_factor(400), 1.5);
        assert_eq!(Prefs { ui_zoom: 300, ..Default::default() }.sanitized().ui_zoom, 150);
        assert_eq!(Prefs { ui_zoom: 0, ..Default::default() }.sanitized().ui_zoom, 50);
        assert!(ZOOM_STEPS.windows(2).all(|w| w[0] < w[1]) && ZOOM_STEPS.contains(&100));
    }

    #[test]
    fn sidebar_sections() {
        let p = Prefs::default();
        assert_eq!(p.sidebar_double_click, SidebarDoubleClick::Collapse, "Eagle's dblclickSidebarItem defaults to collapse");
        let all = [Scope::All, Scope::Uncategorized, Scope::Untagged, Scope::Recent, Scope::Random(3), Scope::AllTags, Scope::Duplicates, Scope::Trash];
        assert!(all.iter().all(|s| p.sidebar.shows(s)), "every section is shown by default");
        let s = SidebarSections { uncategorized: false, random: false, ..Default::default() };
        assert!(!s.shows(&Scope::Uncategorized) && !s.shows(&Scope::Random(9)));
        assert!(s.shows(&Scope::Untagged) && s.shows(&Scope::All) && s.shows(&Scope::Trash));
        let p: Prefs = serde_json::from_str(r#"{"sidebar":{"folders":false},"sidebar_double_click":"rename"}"#).unwrap();
        assert!(!p.sidebar.folders && p.sidebar.quick_access);
        assert_eq!(p.sidebar_double_click, SidebarDoubleClick::Rename);
    }

    #[test]
    fn control_defaults() {
        let p = Prefs::default();
        assert_eq!(p.double_click, DoubleClick::Viewer, "Eagle's doubleclick defaults to internal");
        assert_eq!(p.space, SpaceKey::Preview, "Eagle's keyspace defaults to preview");
        let p: Prefs = serde_json::from_str(r#"{"double_click":"external","space":"quicklook"}"#).unwrap();
        assert_eq!((p.double_click, p.space), (DoubleClick::External, SpaceKey::QuickLook));
    }

    #[test]
    fn video_defaults() {
        let p = Prefs::default();
        assert_eq!(p.video_scroll, VideoScroll::Progress, "Eagle's videoScrollBehavior defaults to progress");
        assert!(!p.video_fill, "Eagle's zoomFill defaults to off");
        assert!(p.video_autoplay && p.video_resume && p.video_loop_short, "Eagle's autoPlay, rememberPosition and loopShortVideo default to on");
    }

    #[test]
    fn notices_follow_the_master_switch() {
        let p = Prefs::default();
        assert!(p.notifications && p.asks_duplicates() && p.tells_import_done(), "Eagle's notifications default to on");
        let off = Prefs { notifications: false, ..Default::default() };
        assert!(!off.asks_duplicates() && !off.tells_import_done());
        let some = Prefs { notify_import_done: false, ..Default::default() };
        assert!(some.asks_duplicates() && !some.tells_import_done());
    }

    #[test]
    fn password_form() {
        let form = |mode, old: &str, new: &str, again: &str| PasswordForm { mode: Some(mode), old: old.into(), new: new.into(), again: again.into(), hint: "tip".into(), error: None };
        assert_eq!(Prefs::default().app_lock, None, "Eagle's privacy lock defaults to off");
        assert_eq!(submit_password(&PasswordForm::default(), None), Ok(None));
        let set = submit_password(&form(PasswordMode::Set, "", "pw", "pw"), None).unwrap().unwrap();
        assert!(set.matches("pw") && set.hint == "tip");
        assert!(submit_password(&form(PasswordMode::Set, "", "pw", "px"), None).is_err());
        assert!(submit_password(&form(PasswordMode::Change, "bad", "new", "new"), Some(&set)).is_err());
        let changed = submit_password(&form(PasswordMode::Change, "pw", "new", "new"), Some(&set)).unwrap().unwrap();
        assert!(changed.matches("new") && !changed.matches("pw"));
        assert!(submit_password(&form(PasswordMode::Remove, "bad", "", ""), Some(&set)).is_err());
        assert_eq!(submit_password(&form(PasswordMode::Remove, "pw", "", ""), Some(&set)), Ok(None));
    }

    #[test]
    fn search_matches_every_word() {
        assert!(matches("", &["General", "Appearance"]));
        assert!(matches("  ", &["General"]));
        assert!(matches("theme", &["General", "Appearance", "theme", "dark"]));
        assert!(matches("THE dark", &["General", "Appearance", "theme", "dark"]));
        assert!(matches("gener", &["General", "Window"]), "a pane's name finds its blocks");
        assert!(!matches("theme port", &["General", "Appearance", "theme"]));
        assert!(!matches("zzz", &["General"]));
    }

    #[test]
    fn panes_follow_eagles_order() {
        let labels: Vec<_> = Section::ALL.iter().map(|s| s.label()).collect();
        assert_eq!(labels, ["General", "Sidebar", "Controls", "Preview", "Shortcuts", "Notification", "Password", "Developer"]);
        let groups: Vec<_> = Section::ALL.iter().map(|s| s.group()).collect();
        assert!(groups.windows(2).all(|w| w[0] <= w[1]), "groups are contiguous");
        let s: Section = serde_json::from_str(r#""preview""#).unwrap();
        assert_eq!(s, Section::Preview);
        assert_eq!(serde_json::to_string(&Section::Developer).unwrap(), r#""developer""#);
    }

    /// Prefs saved before the panes were reorganised load unchanged.
    #[test]
    fn old_prefs_still_load() {
        let old = r#"{"hover_zoom":false,"keywords_as_tags":true,"notify_duplicates":false,"auto_select":false,
            "font_style_tags":false,"api_enabled":false,"api_port":42000,"collect_folder":"F1","theme":"blue"}"#;
        let p: Prefs = serde_json::from_str(old).unwrap();
        assert!(!p.hover_zoom && p.keywords_as_tags && !p.notify_duplicates && !p.auto_select && !p.font_style_tags && !p.api_enabled);
        assert_eq!((p.api_port, p.collect_folder.as_deref(), p.theme), (42000, Some("F1"), Theme::Blue));
        // And the same through eframe's RON storage, as the app saves it.
        #[derive(serde::Deserialize)]
        struct Saved {
            prefs: Prefs,
        }
        let mem = Mem(Some(r#"(tile: Some(150.0), prefs: (hover_zoom: false, api_port: 42000, theme: gray, backup_on_change: true))"#.into()));
        let p = eframe::get_value::<Saved>(&mem, eframe::APP_KEY).unwrap().prefs;
        assert_eq!((p.hover_zoom, p.api_port, p.theme), (false, 42000, Theme::Gray));
    }

    #[test]
    fn missing_fields_take_defaults() {
        let p: Prefs = serde_json::from_str(r#"{"hover_zoom":false}"#).unwrap();
        assert!(!p.hover_zoom);
        assert_eq!(p.api_port, 41596);
        assert!(p.api_enabled);
    }
}
