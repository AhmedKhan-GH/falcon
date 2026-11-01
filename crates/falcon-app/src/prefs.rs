//! Preferences: the settings falcon honours, and a window for them laid
//! out after Eagle's (a search box and the pane list on the left, the pane
//! on the right, Save, Apply and Cancel at the bottom). Each pane is one
//! function that adds blocks of settings to a `Page`; the search shows the
//! matching blocks of every pane.

use std::ops::RangeInclusive;

use eframe::egui::{self, Align2, Color32, CornerRadius, Key, Modifiers, RichText, Sense, Stroke, Ui, vec2};

use crate::applock::{self, Secret};
use crate::locale::{tr, tr_key, trf};
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
    /// Eagle's `developer.apiToken`: requests carrying it as `token` are
    /// served by the API whatever their `Host`. Empty by default, as in
    /// Eagle; only Generate/Regenerate in the Developer pane makes one.
    /// Empty means token auth is off and the API serves only requests whose
    /// Host is its own address.
    pub api_token: String,
    /// Eagle's `general.showCollectModal`, which falcon only keeps for the
    /// API: `preferences/collect/on` and `off` set it, and
    /// `application/info` reports it.
    pub show_collect_modal: bool,
    /// The folder the collect window adds items to (None: no folder).
    /// Chosen in the collect window itself.
    pub collect_folder: Option<String>,
    /// The folders the collect window adds items to. Older saved state
    /// has only `collect_folder`, which `collect::migrate` moves here.
    pub collect_folders: Vec<String>,
    /// Folder ids the collect window used lately, newest first.
    pub collect_recent: Vec<String>,
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
    /// Eagle's `privacy.enableTouchID`: the lock screen offers Touch ID
    /// (`touch_id`), on a Mac that has it.
    pub touch_id: bool,
    /// Eagle's `habits.gifViewer`: always show the frame-by-frame player
    /// for GIFs in the viewer.
    pub gif_player: bool,
    /// Eagle's `habits.alwaysPlayGIF`: GIFs and animated WebPs play in the
    /// grid without being hovered.
    pub always_play_animated: bool,
    /// Eagle's `habits.scrollBehavior`: what the wheel does in the image viewer.
    pub viewer_wheel: crate::logic::WheelMode,
    /// Eagle's `habits.defaultRatio` "100%": images open at actual size
    /// instead of Smart Zoom.
    pub open_actual_size: bool,
    /// Eagle's `habits.renderBehavior` "pixelated": sharp pixels when an
    /// image is enlarged.
    pub pixelated: bool,
    /// Eagle's `habits.rememberLastZoom`: reopen an image at the zoom and
    /// position it was left at (for the session).
    pub remember_zoom: bool,
    /// Eagle's `imageRotateMode`: whether rotating and flipping the image
    /// shown in the viewer only changes the preview or rewrites the file.
    pub rotate_mode: RotateMode,
    /// Eagle's `habits.transparency` "show": a checkerboard behind
    /// transparent images.
    pub transparency_grid: bool,
    /// Eagle's `autoImport.enable`: import files added to the watched
    /// folder (`auto_import.rs`).
    pub auto_import: bool,
    /// Eagle's `autoImport.path`: the chosen folder. Each library watches
    /// the folder named after it inside this one.
    pub auto_import_path: Option<std::path::PathBuf>,
    /// Eagle's `notification.notification.when.autoImport`: a notice when
    /// files from the watched folder start importing.
    pub notify_auto_import: bool,
    /// Delete each file from the watched folder once it is in the library,
    /// as Eagle always does (it has no setting for it). On by default. Off
    /// keeps the files and remembers what was imported instead
    /// (`auto_import::Imported`).
    pub auto_import_delete: bool,
    /// The tag picker's own settings (its gear menu), as Eagle keeps them
    /// for its tag panel.
    pub tag_picker: crate::tags::PickerPrefs,
    /// Eagle's `habits.middleBtn`: what a middle click on a card does.
    pub middle_button: MiddleButton,
    /// Eagle's `video.hoverPlay`: hovered video cards play a preview that
    /// the pointer scrubs.
    pub video_hover: bool,
    /// Eagle's `general.language`: the interface language, the system's
    /// unless one is chosen (`locale.rs`).
    pub language: crate::locale::Language,
    /// Eagle's `screencapture.format`: what screenshots are saved as.
    pub screenshot_format: crate::capture::Format,
    /// Eagle's `screencapture.quality`, for JPG and WebP screenshots
    /// (`capture::QUALITIES`).
    pub screenshot_quality: u8,
    /// Eagle's `screencapture.shortcutsEnable`: the global capture keys
    /// are registered (`hotkeys.rs`).
    pub screenshot_shortcuts: bool,
    /// Eagle's `general.showCaptureCollectModal`: an area or window
    /// screenshot opens the collect window to choose where it goes.
    pub screenshot_ask: bool,
    /// Eagle's `notification.notification.when.screencapture`: a system
    /// notification when an area or window screenshot is taken.
    pub notify_capture: bool,
    /// Eagle's `screencapture.autoTagging.enable`: screenshots get the
    /// "Screenshot" tag.
    pub screenshot_tag: bool,
    /// Eagle's `screencapture.autoWriteClipboard`: screenshots also go to
    /// the clipboard.
    pub screenshot_clipboard: bool,
    /// Eagle's `screencapture.useRetina`: keep a Retina screen's full
    /// pixel size; off halves screenshots taken there.
    pub screenshot_retina: bool,
    /// Eagle's `general.showMenuItem`: the menu bar icon (`tray.rs`).
    pub menu_icon: bool,
    /// A system notification when an export finishes (falcon's own,
    /// after Eagle's "Export Successful" notice).
    pub notify_export_done: bool,
}

/// What a middle click on a card in the grid does.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum MiddleButton {
    /// Nothing (Eagle's "none").
    Nothing,
    /// Opens the item in the preview window (Eagle's default), except
    /// audio, as in Eagle.
    #[default]
    NewWindow,
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

/// What the viewer's rotate and flip do (Eagle's `imageRotateMode`).
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum RotateMode {
    /// Only the viewer's drawing changes; the file is left alone.
    Preview,
    /// The file is rewritten (Eagle's default).
    #[default]
    Write,
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
            api_token: String::new(),
            show_collect_modal: false,
            collect_folder: None,
            collect_folders: Vec::new(),
            collect_recent: Vec::new(),
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
            touch_id: false,
            gif_player: false,
            always_play_animated: false,
            viewer_wheel: crate::logic::WheelMode::Scroll,
            open_actual_size: false,
            pixelated: false,
            remember_zoom: true,
            rotate_mode: RotateMode::default(),
            transparency_grid: false,
            auto_import: false,
            auto_import_path: None,
            notify_auto_import: true,
            auto_import_delete: true,
            tag_picker: Default::default(),
            middle_button: MiddleButton::default(),
            video_hover: true,
            language: crate::locale::Language::Auto,
            screenshot_format: crate::capture::Format::Png,
            screenshot_quality: crate::capture::DEFAULT_QUALITY,
            screenshot_shortcuts: true,
            screenshot_ask: false,
            notify_capture: true,
            screenshot_tag: true,
            screenshot_clipboard: false,
            screenshot_retina: true,
            menu_icon: true,
            notify_export_done: true,
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
        // A token is never made here. One that is only whitespace is empty;
        // any other is kept, so tokens made by earlier versions survive.
        if self.api_token.trim().is_empty() {
            self.api_token.clear();
        }
        self.ui_zoom = self.ui_zoom.clamp(ZOOM_STEPS[0], ZOOM_STEPS[ZOOM_STEPS.len() - 1]);
        // Only Eagle's three qualities are offered.
        if !crate::capture::QUALITIES.iter().any(|q| q.0 == self.screenshot_quality) {
            self.screenshot_quality = crate::capture::DEFAULT_QUALITY;
        }
        // As in Eagle, a chosen folder that is gone is forgotten and
        // auto-import turned off.
        if self.auto_import_path.as_ref().is_some_and(|p| !p.is_dir()) {
            self.auto_import_path = None;
            self.auto_import = false;
        }
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

    /// Whether files found in the watched folder get a notice.
    pub fn tells_auto_import(&self) -> bool {
        self.notifications && self.notify_auto_import
    }

    /// Whether a screenshot taken in `mode` posts a notification: only an
    /// area or window one, as in Eagle (a full-screen one only sounds).
    pub fn tells_capture(&self, mode: crate::capture::Mode) -> bool {
        self.notifications && self.notify_capture && mode != crate::capture::Mode::Full
    }

    /// Whether a screenshot taken in `mode` opens the collect window to
    /// choose where it goes: never a full-screen one, as in Eagle.
    pub fn asks_capture(&self, mode: crate::capture::Mode) -> bool {
        self.screenshot_ask && mode != crate::capture::Mode::Full
    }

    /// Whether a finished export posts a notification.
    pub fn tells_export_done(&self) -> bool {
        self.notifications && self.notify_export_done
    }

    /// How screenshots are saved.
    pub fn capture_options(&self) -> crate::capture::Options {
        crate::capture::Options { format: self.screenshot_format, quality: self.screenshot_quality, retina: self.screenshot_retina, clipboard: self.screenshot_clipboard }
    }

    /// The folder the library at `root` watches, while auto-import is on
    /// and a folder is chosen.
    pub fn auto_import_dir(&self, root: &std::path::Path) -> Option<std::path::PathBuf> {
        let chosen = self.auto_import_path.as_ref().filter(|_| self.auto_import)?;
        Some(crate::auto_import::watch_dir(chosen, root))
    }

    /// The ports to try for the API, or None when it is off.
    pub fn api_ports(&self) -> Option<RangeInclusive<u16>> {
        let first = if port_error(self.api_port).is_none() { self.api_port } else { falcon_server::DEFAULT_PORT };
        self.api_enabled.then_some(first..=first + (PORT_SPAN - 1))
    }

    /// The preferences the API reports in `application/info`, in Eagle's
    /// shape and spelling (`"true"`/`"false"`, `"on"`/`"off"` and so on):
    /// the settings falcon has, each under Eagle's key.
    pub fn eagle_preferences(&self) -> serde_json::Value {
        let b = |v: bool| if v { "true" } else { "false" };
        let on = |v: bool| if v { "on" } else { "off" };
        serde_json::json!({
            "general": {
                "zoom": self.ui_zoom.to_string(),
                "showSidebarBadge": b(self.sidebar_counts),
                "autoSelect": b(self.auto_select),
                "showCollectModal": b(self.show_collect_modal),
                "IPTC": b(self.keywords_as_tags),
            },
            "habits": {
                "dblclickSidebarItem": match self.sidebar_double_click {
                    SidebarDoubleClick::Rename => "rename",
                    SidebarDoubleClick::Collapse => "collapse",
                },
                "scrollBehavior": match self.viewer_wheel {
                    crate::logic::WheelMode::Scroll => "scroll",
                    crate::logic::WheelMode::Zoom => "zoom",
                    crate::logic::WheelMode::Paging => "paging",
                },
                "videoScrollBehavior": match self.video_scroll {
                    VideoScroll::Progress => "progress",
                    VideoScroll::Volume => "volume",
                },
                "hoverZoom": on(self.hover_zoom),
                "renderBehavior": if self.pixelated { "pixelated" } else { "non-pixelated" },
                "rememberLastZoom": on(self.remember_zoom),
                "defaultRatio": if self.open_actual_size { "100%" } else { "auto" },
                "transparency": if self.transparency_grid { "show" } else { "hide" },
                "keyspace": match self.space {
                    SpaceKey::Preview => "preview",
                    SpaceKey::QuickLook => "preview-native",
                    SpaceKey::Scroll => "scroll",
                },
                "doubleclick": match self.double_click {
                    DoubleClick::Viewer => "internal",
                    DoubleClick::External => "external",
                },
                "middleBtn": match self.middle_button {
                    MiddleButton::Nothing => "none",
                    MiddleButton::NewWindow => "openNewWindow",
                },
                "gifViewer": on(self.gif_player),
                "alwaysPlayGIF": on(self.always_play_animated),
            },
            "notification": {
                "notification": {
                    "enable": b(self.notifications),
                    "when": {"repeatImage": b(self.notify_duplicates), "autoImport": b(self.notify_auto_import)},
                },
            },
            "video": {
                "zoomFill": b(self.video_fill),
                "autoPlay": b(self.video_autoplay),
                "rememberPosition": b(self.video_resume),
                "loopShortVideo": b(self.video_loop_short),
                "hoverPlay": b(self.video_hover),
            },
            "font": {"autoTag": b(self.font_style_tags)},
            "autoImport": {
                "enable": b(self.auto_import),
                "path": self.auto_import_path.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
            },
            "developer": {"apiToken": self.api_token},
        })
    }

    /// Takes back what the API changed in [`Prefs::eagle_preferences`]:
    /// only `general.showCollectModal` can change there.
    pub fn take_eagle_preferences(&mut self, v: &serde_json::Value) {
        if let Some(s) = v["general"]["showCollectModal"].as_str() {
            self.show_collect_modal = s == "true";
        }
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
        return Some(tr("Use a port of 1024 or higher").to_string());
    }
    let Some(last) = first.checked_add(PORT_SPAN - 1) else {
        return Some(trf("Use a port of {port} or lower", &[("port", &(u16::MAX - (PORT_SPAN - 1)))]));
    };
    if (first..=last).contains(&EAGLE_PORT) {
        return Some(trf("{first}–{last} would include Eagle's port {port}", &[("first", &first), ("last", &last), ("port", &EAGLE_PORT)]));
    }
    None
}

/// Parses the port field.
pub fn parse_port(text: &str) -> Result<u16, String> {
    let p: u16 = text.trim().parse().map_err(|_| tr("Enter a number from 1024 to 65531").to_string())?;
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

/// The panes, in Eagle's order. Eagle's AI, MCP and Proxy panes have no
/// counterpart (falcon has no plugins or network features), and a pane
/// appears only once it has a setting that works.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Section {
    #[default]
    General,
    Sidebar,
    Controls,
    Preview,
    Screenshot,
    Shortcuts,
    Notification,
    Password,
    AutoImport,
    Developer,
}

impl Section {
    pub const ALL: [Section; 10] = [
        Section::General,
        Section::Sidebar,
        Section::Controls,
        Section::Preview,
        Section::Screenshot,
        Section::Shortcuts,
        Section::Notification,
        Section::Password,
        Section::AutoImport,
        Section::Developer,
    ];

    /// The English name, a key for `tr` (also what the search matches).
    pub fn label(self) -> &'static str {
        match self {
            Section::General => tr_key("General"),
            Section::Sidebar => tr_key("Sidebar"),
            Section::Controls => tr_key("Controls"),
            Section::Preview => tr_key("Preview"),
            Section::Screenshot => tr_key("Screenshot"),
            Section::Shortcuts => tr_key("Shortcuts"),
            Section::Notification => tr_key("Notification"),
            Section::Password => tr_key("Password"),
            Section::AutoImport => tr_key("Auto-Import"),
            Section::Developer => tr_key("Developer"),
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Section::General => crate::icons::GEAR,
            Section::Sidebar => crate::icons::SIDEBAR_SIMPLE,
            Section::Controls => crate::icons::MOUSE,
            Section::Preview => crate::icons::EYE,
            Section::Screenshot => crate::icons::CAMERA,
            Section::Shortcuts => crate::icons::KEYBOARD,
            Section::Notification => crate::icons::BELL,
            Section::Password => crate::icons::LOCK_SIMPLE,
            Section::AutoImport => crate::icons::TRAY_ARROW_DOWN,
            Section::Developer => crate::icons::CODE,
        }
    }

    /// Eagle separates its pane list into groups; a line goes above the
    /// first pane of each group.
    fn group(self) -> u8 {
        match self {
            Section::General | Section::Sidebar => 0,
            Section::Controls | Section::Preview | Section::Screenshot | Section::Shortcuts => 1,
            Section::Notification | Section::Password | Section::AutoImport => 2,
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
    applied_language: crate::locale::Language,
    /// The shown library's folder, for the Auto-Import pane.
    pub library: Option<std::path::PathBuf>,
    /// Why the folder last picked for auto-import was refused.
    auto_import_note: Option<String>,
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
        let shown = tr(title);
        let mut fields = vec![self.pane.label(), tr(self.pane.label()), title, shown];
        fields.extend_from_slice(keywords);
        if !matches(&self.query, &fields) {
            return;
        }
        if self.searching() && !self.headed {
            self.headed = true;
            self.ui.add_space(if self.hits == 0 { 4.0 } else { 8.0 });
            self.ui.label(RichText::new(tr(self.pane.label()).to_uppercase()).size(11.0).color(t::p().text_muted));
        }
        self.hits += 1;
        let ui = &mut *self.ui;
        ui.add_space(4.0);
        ui.label(RichText::new(shown).strong().size(13.0));
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
        self.applied_language = draft.prefs.language;
        self.draft = Some(draft);
        self.open = true;
        self.query.clear();
        self.recorder.stop();
        self.password = PasswordForm::default();
        self.auto_import_note = None;
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
            let heading = if searching { tr("Search") } else { tr(self.section.label()) };
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
                        ui.label(RichText::new(tr("No settings match")).size(14.0).color(t::p().text));
                        note(ui, tr("Try another word, or clear the search to browse the panes."));
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
            let save = f.add_enabled(valid, egui::Button::new(RichText::new(tr("Save")).color(Color32::WHITE)).fill(t::p().primary)).clicked();
            let apply = f.add_enabled(valid, egui::Button::new(tr("Apply"))).clicked();
            close |= f.button(tr("Cancel")).clicked();
            if (save || apply || (save_key && valid))
                && let Ok(p) = port
            {
                draft.prefs.api_port = p;
                self.applied_theme = draft.prefs.theme;
                self.applied_zoom = draft.prefs.ui_zoom;
                self.applied_language = draft.prefs.language;
                result = Some(draft.clone());
                close |= save || save_key;
            }
        });
        if close {
            if let Some(theme) = theme_to_restore(self.applied_theme, t::current()) {
                t::set_theme(ctx, theme);
            }
            ctx.set_zoom_factor(zoom_factor(self.applied_zoom));
            crate::locale::set(self.applied_language.resolve());
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
            Section::Screenshot => self.screenshot(p, d),
            Section::Shortcuts => self.shortcuts(p, d),
            Section::Notification => notification(p, d),
            Section::Password => password(p, d, &mut self.password),
            Section::AutoImport => auto_import(p, d, self.library.as_deref(), &mut self.auto_import_note),
            Section::Developer => developer(p, d, &mut self.port_text, port_error),
        }
    }

    /// Screenshot: format, the global keys and what happens to each picture.
    fn screenshot(&mut self, p: &mut Page, d: &mut Draft) {
        use crate::capture::{self, Format};
        use crate::shortcuts::Command;
        let keywords = ["format", "png", "jpg", "jpeg", "webp", "quality", "high", "medium", "low", "size"];
        p.block(tr_key("Format and quality"), &keywords, |ui| {
            ui.horizontal(|ui| {
                ui.label(tr("Format"));
                for f in Format::ALL {
                    ui.selectable_value(&mut d.prefs.screenshot_format, f, f.label());
                }
            });
            if d.prefs.screenshot_format != Format::Png {
                ui.horizontal(|ui| {
                    ui.label(tr("Quality"));
                    for (q, name) in capture::QUALITIES {
                        ui.selectable_value(&mut d.prefs.screenshot_quality, q, tr(name));
                    }
                });
            }
            note(ui, tr("PNG keeps every pixel; JPG and WebP files are smaller."));
        });
        let recorder = &mut self.recorder;
        p.block(tr_key("Keys"), &["keyboard", "shortcut", "keys", "global", "hotkey", "area", "window", "screen", "capture"], |ui| {
            ui.checkbox(&mut d.prefs.screenshot_shortcuts, tr("Use the screenshot keys"));
            if d.prefs.screenshot_shortcuts {
                recorder.rows(ui, &mut d.keys, |s| matches!(s.cmd, Command::Capture(_)));
            }
            note(ui, tr("These keys work from any app, also while falcon is hidden."));
        });
        let keywords = ["ask", "where", "save", "collect", "folder", "notify", "notification", "tag", "screenshot", "clipboard", "copy", "retina", "2x", "@2x", "resolution"];
        p.block(tr_key("Options"), &keywords, |ui| {
            ui.checkbox(&mut d.prefs.screenshot_ask, tr("Choose where each screenshot is saved"));
            note(ui, tr("Area and window screenshots wait in the collect window; full-screen ones go straight into the library."));
            ui.add_enabled_ui(d.prefs.notifications, |ui| {
                ui.checkbox(&mut d.prefs.notify_capture, tr("Show a notification for each screenshot"));
            });
            ui.checkbox(&mut d.prefs.screenshot_tag, tr("Tag screenshots “Screenshot”"));
            ui.checkbox(&mut d.prefs.screenshot_clipboard, tr("Copy screenshots to the clipboard too"));
            ui.checkbox(&mut d.prefs.screenshot_retina, tr("Keep full Retina resolution (@2x)"));
            note(ui, tr("When off, screenshots taken on a Retina screen are saved at half size."));
        });
    }

    fn shortcuts(&mut self, p: &mut Page, d: &mut Draft) {
        if p.searching() {
            // The shortcut list has its own search, in its pane.
            let mut go = false;
            p.block(tr_key("Keyboard shortcuts"), &["keys", "hotkey", "chord", "key bindings"], |ui| {
                go = ui.button(tr("Show the Shortcuts pane")).clicked();
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
        ui.painter().text(title.left_center(), Align2::LEFT_CENTER, tr("Preferences"), font(14.0), t::p().text);
        let search = ui.add(
            egui::TextEdit::singleline(&mut self.query)
                .hint_text(RichText::new(format!("{}  {}", crate::icons::MAGNIFYING_GLASS, tr("Search"))).color(t::p().text_muted))
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
            ui.painter().text(rect.left_center() + vec2(30.0, 0.0), Align2::LEFT_CENTER, tr(s.label()), font(13.0), t::p().text);
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
    p.block(tr_key("Appearance"), &keywords, |ui| {
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
            ui.label(tr("Interface zoom"));
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
        ui.checkbox(&mut d.prefs.sidebar_counts, tr("Show item counts in the sidebar"));
    });
    p.block(tr_key("Language"), &["language", "locale", "translation", "english", "chinese", "japanese", "korean", "german", "spanish", "russian"], |ui| {
        let before = d.prefs.language;
        egui::ComboBox::from_id_salt("language").selected_text(d.prefs.language.label()).show_ui(ui, |ui| {
            for l in crate::locale::Language::all() {
                ui.selectable_value(&mut d.prefs.language, l, l.label());
            }
        });
        if d.prefs.language != before {
            // Previewed at once, like the theme; Cancel switches back.
            let lang = d.prefs.language.resolve();
            crate::locale::load_fonts(ui.ctx(), lang);
            crate::locale::set(lang);
        }
    });
    p.block(tr_key("Window"), &["sidebar", "inspector", "panel", "show", "menu bar", "icon", "tray", "status"], |ui| {
        ui.checkbox(&mut d.panels.sidebar, tr("Show the sidebar"));
        ui.checkbox(&mut d.panels.inspector, tr("Show the inspector"));
        if cfg!(target_os = "macos") {
            ui.checkbox(&mut d.prefs.menu_icon, tr("Show the falcon icon in the menu bar"));
        }
    });
    p.block(tr_key("Importing"), &["import", "collect", "keywords", "iptc", "xmp", "tags", "duplicate", "already", "select", "new items", "retina", "2x", "@2x", "screenshot"], |ui| {
        ui.checkbox(&mut d.prefs.auto_select, tr("Select the new items after importing"));
        ui.checkbox(&mut d.prefs.notify_duplicates, tr("Ask what to do when an imported file is already in the library"));
        note(ui, tr("When off, such files are imported as new items."));
        ui.checkbox(&mut d.prefs.keywords_as_tags, tr("Add IPTC and XMP keywords as tags when importing images"));
        ui.checkbox(&mut d.prefs.screenshot_retina, tr("Keep full Retina resolution in screenshots (@2x)"));
    });
}

/// Sidebar: what double-clicking does, and which sections are shown.
fn sidebar(p: &mut Page, d: &mut Draft) {
    p.block(tr_key("Double-clicking a folder"), &["double click", "rename", "expand", "collapse", "folder"], |ui| {
        ui.radio_value(&mut d.prefs.sidebar_double_click, SidebarDoubleClick::Rename, tr("Renames it"));
        ui.radio_value(&mut d.prefs.sidebar_double_click, SidebarDoubleClick::Collapse, tr("Expands or collapses it"));
    });
    let keywords = ["sections", "show", "hide", "uncategorized", "untagged", "recently used", "random", "quick access", "smart folders", "folders"];
    p.block(tr_key("Sections"), &keywords, |ui| {
        let s = &mut d.prefs.sidebar;
        ui.checkbox(&mut s.uncategorized, tr("Uncategorized"));
        ui.checkbox(&mut s.untagged, tr("Untagged"));
        ui.checkbox(&mut s.recent, tr("Recently Used"));
        ui.checkbox(&mut s.random, tr("Random"));
        ui.checkbox(&mut s.quick_access, tr("Quick Access"));
        ui.checkbox(&mut s.smart_folders, tr("Smart Folders"));
        ui.checkbox(&mut s.folders, tr("Folders"));
    });
}

/// Controls: what the mouse and the keyboard do.
fn controls(p: &mut Page, d: &mut Draft) {
    p.block(tr_key("Mouse"), &["hover", "zoom button", "cards", "double click", "open", "default app", "wheel", "scroll", "middle", "new window"], |ui| {
        ui.label(tr("Zoom button on hovered cards"));
        ui.radio_value(&mut d.prefs.hover_zoom, true, tr("Show"));
        ui.radio_value(&mut d.prefs.hover_zoom, false, tr("Hide"));
        note(ui, tr("Holding Z shows the preview either way."));
        ui.add_space(6.0);
        ui.label(tr("Double-clicking an item"));
        ui.radio_value(&mut d.prefs.double_click, DoubleClick::Viewer, tr("Opens it in falcon"));
        ui.radio_value(&mut d.prefs.double_click, DoubleClick::External, tr("Opens it in its default app"));
        ui.add_space(6.0);
        use crate::logic::WheelMode;
        ui.label(tr("The wheel over an image in the viewer"));
        ui.radio_value(&mut d.prefs.viewer_wheel, WheelMode::Scroll, tr("Scrolls it"));
        ui.radio_value(&mut d.prefs.viewer_wheel, WheelMode::Zoom, tr("Zooms it"));
        ui.radio_value(&mut d.prefs.viewer_wheel, WheelMode::Paging, tr("Goes to the previous or next item"));
        note(ui, tr("With ⌘, ⌃ or ⌥, or a pinch, it zooms either way."));
        ui.add_space(6.0);
        ui.label(tr("The wheel button on an item"));
        ui.radio_value(&mut d.prefs.middle_button, MiddleButton::NewWindow, tr("Opens it in a new window"));
        ui.radio_value(&mut d.prefs.middle_button, MiddleButton::Nothing, tr("Does nothing"));
        note(ui, tr("Audio files never open in a new window. In the viewer it enters or leaves presentation mode either way."));
    });
    p.block(tr_key("Keyboard"), &["space", "spacebar", "preview", "quick look", "scroll"], |ui| {
        ui.label(tr("Space in the grid"));
        ui.radio_value(&mut d.prefs.space, SpaceKey::Preview, tr("Opens the viewer"));
        if cfg!(target_os = "macos") {
            ui.radio_value(&mut d.prefs.space, SpaceKey::QuickLook, tr("Shows Quick Look"));
        }
        ui.radio_value(&mut d.prefs.space, SpaceKey::Scroll, tr("Scrolls down a page"));
        note(ui, tr("In the viewer Space still closes it, or plays and pauses media."));
    });
}

/// Preview: how the viewer shows each kind of file. Eagle's order is
/// image, video, GIF, then fonts.
fn preview(p: &mut Page, d: &mut Draft) {
    p.block(tr_key("Image"), &["image", "zoom", "actual size", "fit", "pixel", "pixelated", "smooth", "remember", "last", "transparency", "checkerboard", "grid", "rotate", "flip", "mirror"], |ui| {
        ui.checkbox(&mut d.prefs.open_actual_size, tr("Open images at actual size instead of fitting them to the window"));
        ui.checkbox(&mut d.prefs.pixelated, tr("Show sharp pixels when an image is enlarged"));
        ui.checkbox(&mut d.prefs.remember_zoom, tr("Reopen an image at the zoom and position it was left at"));
        ui.checkbox(&mut d.prefs.transparency_grid, tr("Show a checkerboard behind transparent areas"));
        ui.add_space(4.0);
        ui.label(tr("Rotating and flipping the image in the viewer"));
        ui.radio_value(&mut d.prefs.rotate_mode, RotateMode::Write, tr("Writes to the file"));
        ui.radio_value(&mut d.prefs.rotate_mode, RotateMode::Preview, tr("Changes the preview only"));
        note(ui, tr("The card menu in the grid always writes to the file."));
    });
    let keywords = ["video", "movie", "wheel", "scroll", "progress", "volume", "stretch", "fill", "autoplay", "play", "position", "resume", "loop", "short", "hover", "preview", "scrub"];
    p.block(tr_key("Video"), &keywords, |ui| {
        let v = &mut d.prefs;
        ui.label(tr("Scrolling over a video"));
        ui.radio_value(&mut v.video_scroll, VideoScroll::Progress, tr("Moves through it"));
        ui.radio_value(&mut v.video_scroll, VideoScroll::Volume, tr("Changes the volume"));
        ui.add_space(4.0);
        ui.checkbox(&mut v.video_fill, tr("Stretch videos to fill the viewer"));
        ui.checkbox(&mut v.video_autoplay, tr("Play videos when they open"));
        ui.checkbox(&mut v.video_resume, tr("Resume where each video was left"));
        ui.checkbox(&mut v.video_loop_short, tr("Always loop clips of 30 seconds or less"));
        ui.checkbox(&mut v.video_hover, tr("Play a preview on hovered video cards"));
        note(ui, tr("Moving the pointer across the card moves through the clip."));
    });
    p.block(tr_key("GIF"), &["gif", "player", "frame", "animation", "autoplay", "hover", "webp", "avif"], |ui| {
        ui.checkbox(&mut d.prefs.always_play_animated, tr("Always play GIFs and animated WebPs and AVIFs in the grid"));
        ui.checkbox(&mut d.prefs.gif_player, tr("Always use the frame-by-frame player for GIFs"));
        note(ui, tr("Cards play while hovered either way; hold ⌥ to keep one still."));
    });
    p.block(tr_key("Fonts"), &["font", "style", "regular", "bold", "italic", "tags", "import"], |ui| {
        ui.checkbox(&mut d.prefs.font_style_tags, tr("Add style tags (Regular, Bold, Italic) when importing fonts"));
    });
}

/// Notification: which notices falcon shows.
fn notification(p: &mut Page, d: &mut Draft) {
    p.block(tr_key("Pop-ups"), &["notice", "notification", "pop-up", "duplicate", "already", "import", "added", "finished", "auto-import", "watched", "screenshot", "capture", "export", "system"], |ui| {
        let n = &mut d.prefs;
        ui.checkbox(&mut n.notifications, tr("Show notices"));
        ui.add_enabled_ui(n.notifications, |ui| {
            ui.indent("notices", |ui| {
                ui.checkbox(&mut n.notify_duplicates, tr("Ask what to do when an imported file is already in the library"));
                ui.checkbox(&mut n.notify_import_done, tr("Say how many items an import added"));
                ui.checkbox(&mut n.notify_auto_import, tr("Say when files from the auto-import folder start importing"));
                ui.checkbox(&mut n.notify_capture, tr("Show a notification for each screenshot"));
                ui.checkbox(&mut n.notify_export_done, tr("Show a notification when an export finishes"));
            });
        });
        note(ui, tr("Import failures are always reported. Notifications for screenshots, imports and exports appear while falcon is in the background, in the app built by scripts/bundle.sh. falcon plays no sound effects."));
    });
}

/// Whether a folder can be watched for auto-import, and why not.
pub fn auto_import_refusal(path: &std::path::Path) -> Option<String> {
    if crate::auto_import::is_network_volume(path) {
        Some("Auto-import doesn't work with network drives. Choose a folder on this Mac or a drive attached to it.".into())
    } else if !path.is_dir() {
        Some(format!("{} doesn't exist.", path.display()))
    } else {
        None
    }
}

/// Auto-Import: a folder whose new files are imported by themselves.
fn auto_import(p: &mut Page, d: &mut Draft, library: Option<&std::path::Path>, refused: &mut Option<String>) {
    let keywords = ["auto", "import", "watch", "monitor", "folder", "drop", "automatic", "inbox", "delete", "trash", "remove"];
    p.block("Auto-import", &keywords, |ui| {
        let was = d.prefs.auto_import;
        ui.checkbox(&mut d.prefs.auto_import, "Import files added to a folder automatically");
        ui.checkbox(&mut d.prefs.auto_import_delete, "Delete files after importing them");
        if d.prefs.auto_import_delete {
            ui.label(RichText::new("Files in the watched folder are moved to the Trash once they are in the library.").size(11.0).color(t::p().icon_red));
        } else {
            note(ui, "Files stay in the folder; falcon remembers which ones it imported and skips them.");
        }
        let mut choose = d.prefs.auto_import && !was && d.prefs.auto_import_path.is_none();
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let shown = d.prefs.auto_import_path.as_ref().map_or_else(|| "No folder chosen".to_string(), |p| p.display().to_string());
            ui.add(egui::Label::new(RichText::new(shown).color(t::p().text_value)).truncate());
        });
        ui.horizontal(|ui| {
            choose |= ui.button("Choose Folder…").clicked();
            let chosen = d.prefs.auto_import_path.clone().filter(|p| p.is_dir());
            let open = ui.add_enabled(chosen.is_some(), egui::Button::new("Open Folder")).on_hover_text("Shows the chosen folder in Finder");
            if open.clicked()
                && let Some(dir) = chosen
            {
                crate::open_with::open_default(&[dir]);
            }
        });
        if choose {
            if let Some(dir) = rfd::FileDialog::new().set_title("Choose a Folder to Watch").set_can_create_directories(true).pick_folder() {
                *refused = auto_import_refusal(&dir);
                if refused.is_none() {
                    d.prefs.auto_import_path = Some(dir);
                }
            }
            // As in Eagle, turning it on without picking a folder leaves it off.
            if d.prefs.auto_import_path.is_none() {
                d.prefs.auto_import = false;
            }
        }
        if let Some(why) = refused.as_deref() {
            ui.label(RichText::new(why).size(11.0).color(t::p().icon_red));
        }
        ui.add_space(4.0);
        match (library, &d.prefs.auto_import_path) {
            (Some(root), Some(chosen)) => {
                let dir = crate::auto_import::watch_dir(chosen, root);
                note(ui, &format!("Files dropped into {} are imported into this library.", dir.display()));
                // Eagle makes this folder itself; falcon only reads the
                // chosen folder, so it is made here when asked.
                if chosen.is_dir() && !dir.exists() {
                    ui.horizontal(|ui| {
                        note(ui, "That folder doesn't exist yet.");
                        if ui.small_button("Make It").clicked()
                            && let Err(e) = std::fs::create_dir(&dir)
                        {
                            *refused = Some(format!("Couldn't make {}: {e}", dir.display()));
                        }
                    });
                }
            }
            _ => note(ui, "Each library watches the folder named after it inside the chosen folder (\"Books\" for Books.library)."),
        }
        note(ui, "New files are imported once they stop growing; the originals stay where they are. Files under 1 KB, temporary (.tmp) files and files without an extension are skipped.");
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
    /// The fingerprint asked for to turn Touch ID on.
    touch_ticket: Option<u64>,
}

/// Checks the Password pane's form against the current password and
/// returns what it changes the stored password to.
fn submit_password(form: &PasswordForm, current: Option<&Secret>) -> Result<Option<Secret>, &'static str> {
    let Some(mode) = form.mode else { return Ok(current.cloned()) };
    if mode != PasswordMode::Set && !current.is_some_and(|s| s.matches(&form.old)) {
        return Err(if mode == PasswordMode::Remove { tr_key("The password is wrong.") } else { tr_key("The old password is wrong.") });
    }
    if mode == PasswordMode::Remove {
        return Ok(None);
    }
    applock::check_new(&form.new, &form.again)?;
    Ok(Some(Secret::new(&form.new, &form.hint)))
}

/// The Touch ID switch, shown only on a Mac that can use it, as Eagle
/// shows it. Turning it on asks for a fingerprint first and stays off if
/// that fails, as Eagle's does.
fn touch_id_switch(ui: &mut Ui, d: &mut Draft, form: &mut PasswordForm) {
    if let Some(ticket) = form.touch_ticket
        && let Some(ok) = crate::touch_id::poll(ticket)
    {
        form.touch_ticket = None;
        d.prefs.touch_id = ok;
    }
    if !crate::touch_id::available() {
        return;
    }
    let mut on = d.prefs.touch_id;
    let resp = ui.add_enabled(form.touch_ticket.is_none(), egui::Checkbox::new(&mut on, tr("Unlock with Touch ID")));
    if resp.changed() {
        if on {
            form.touch_ticket = crate::touch_id::start(tr("turn on Touch ID for unlocking Falcon"), ui.ctx());
        } else {
            d.prefs.touch_id = false;
        }
    }
}

/// Password: lock falcon behind a password.
fn password(p: &mut Page, d: &mut Draft, form: &mut PasswordForm) {
    let keywords = ["password", "lock", "privacy", "security", "launch", "hint"];
    p.block(tr_key("App password"), &keywords, |ui| {
        let set = d.prefs.app_lock.is_some();
        if !set {
            let mut on = form.mode == Some(PasswordMode::Set);
            if ui.checkbox(&mut on, tr("Lock falcon with a password")).changed() {
                *form = PasswordForm { mode: on.then_some(PasswordMode::Set), ..Default::default() };
            }
        } else if form.mode.is_none() {
            ui.label(tr("falcon asks for its password when it opens, and after Lock Now (⌥⌘L)."));
            ui.horizontal(|ui| {
                if ui.button(tr("Change Password…")).clicked() {
                    *form = PasswordForm { mode: Some(PasswordMode::Change), hint: d.prefs.app_lock.as_ref().map(|s| s.hint.clone()).unwrap_or_default(), ..Default::default() };
                }
                if ui.button(tr("Remove Password…")).clicked() {
                    *form = PasswordForm { mode: Some(PasswordMode::Remove), ..Default::default() };
                }
            });
            touch_id_switch(ui, d, form);
        }
        let Some(mode) = form.mode else {
            note(ui, tr("The password guards this window only; it is kept in falcon's settings, not in the library."));
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
            field(ui, if mode == PasswordMode::Remove { tr("Password") } else { tr("Old password") }, &mut form.old, true);
        }
        if mode != PasswordMode::Remove {
            field(ui, tr("New password"), &mut form.new, true);
            field(ui, tr("Again"), &mut form.again, true);
            field(ui, tr("Hint (optional)"), &mut form.hint, false);
        }
        if let Some(e) = form.error {
            ui.label(RichText::new(tr(e)).size(11.0).color(t::p().icon_red));
        }
        ui.horizontal(|ui| {
            let label = match mode {
                PasswordMode::Set => tr("Set Password"),
                PasswordMode::Change => tr("Change Password"),
                PasswordMode::Remove => tr("Remove Password"),
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
            if mode != PasswordMode::Set && ui.button(tr("Cancel")).clicked() {
                *form = PasswordForm::default();
            }
        });
        note(ui, tr("Takes effect when you Save or Apply."));
    });
}

/// Developer: the local HTTP API.
/// A new API token in Eagle's format (a random UUID, version 4).
pub fn new_token() -> String {
    let mut b = [0u8; 16];
    let read = std::fs::File::open("/dev/urandom").and_then(|mut f| std::io::Read::read_exact(&mut f, &mut b));
    if read.is_err() {
        // No /dev/urandom: fall back on the standard library's random hash
        // keys, mixed with the time.
        use std::hash::{BuildHasher, Hasher};
        for (i, chunk) in b.chunks_mut(8).enumerate() {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
            h.write_usize(i);
            chunk.copy_from_slice(&h.finish().to_le_bytes());
        }
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
}

fn developer(p: &mut Page, d: &mut Draft, port_text: &mut String, error: Option<&String>) {
    let pr = &mut d.prefs;
    let first = parse_port(port_text).unwrap_or(pr.api_port);
    p.block("API token", &["api", "token", "key", "auth", "developer", "restful"], |ui| {
        if pr.api_token.is_empty() {
            note(ui, "No token. Only requests addressed to localhost are served.");
            if ui.button("Generate").clicked() {
                pr.api_token = new_token();
            }
            return;
        }
        ui.horizontal(|ui| {
            ui.add(egui::Label::new(RichText::new(&pr.api_token).monospace()).selectable(true));
            if ui.button("Copy").clicked() {
                ui.ctx().copy_text(pr.api_token.clone());
            }
            if ui.button("Regenerate").clicked() {
                pr.api_token = new_token();
            }
        });
        let url = format!("http://localhost:{first}/?token={}", pr.api_token);
        ui.hyperlink_to(RichText::new(&url).size(11.0), &url);
        note(ui, "Programs that send this token are let in from any address the API listens on.");
    });
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
    fn rotate_mode_loads_old_prefs() {
        let p: Prefs = serde_json::from_str(r#"{"remember_zoom":false}"#).unwrap();
        assert_eq!(p.rotate_mode, RotateMode::Write);
        let p: Prefs = serde_json::from_str(r#"{"rotate_mode":"preview"}"#).unwrap();
        assert_eq!(p.rotate_mode, RotateMode::Preview);
    }

    #[test]
    fn defaults() {
        let p = Prefs::default();
        assert!(p.hover_zoom, "Eagle's hoverZoom defaults to on");
        assert!(!p.gif_player && !p.always_play_animated, "Eagle's gifViewer and alwaysPlayGIF default to off");
        assert_eq!(p.viewer_wheel, crate::logic::WheelMode::Scroll, "Eagle's scrollBehavior defaults to scroll");
        assert!(!p.open_actual_size, "Eagle's defaultRatio defaults to auto");
        assert!(!p.pixelated, "Eagle's renderBehavior defaults to non-pixelated");
        assert!(p.remember_zoom, "Eagle's rememberLastZoom defaults to on");
        assert_eq!(p.rotate_mode, RotateMode::Write, "Eagle's imageRotateMode defaults to write");
        assert!(!p.transparency_grid, "Eagle's transparency defaults to hide");
        assert!(!p.keywords_as_tags, "Eagle's IPTC defaults to off");
        assert!(p.notify_duplicates, "Eagle's repeatImage notification defaults to on");
        assert!(p.auto_select, "Eagle's autoSelect defaults to on");
        assert!(p.font_style_tags, "Eagle's font autoTag defaults to on");
        assert_eq!(p.middle_button, MiddleButton::NewWindow, "Eagle's middleBtn defaults to openNewWindow");
        assert!(p.video_hover, "Eagle's video hoverPlay defaults to true");
        let v = p.eagle_preferences();
        assert_eq!(v["habits"]["middleBtn"], "openNewWindow");
        assert_eq!(v["video"]["hoverPlay"], "true");
        assert_eq!(p.api_ports(), Some(41596..=41600));
        assert!(!p.auto_import && p.auto_import_path.is_none(), "Eagle's autoImport defaults to off with no folder");
        assert!(p.auto_import_delete, "Eagle deletes imported files from the watched folder");
        assert!(p.tells_auto_import(), "Eagle's autoImport notification defaults to on");
        let chosen = Prefs { auto_import_path: Some("/d".into()), ..Default::default() };
        assert_eq!(chosen.auto_import_dir(std::path::Path::new("/x/Books.library")), None, "off watches nothing");
        let on = Prefs { auto_import: true, ..chosen };
        assert_eq!(on.auto_import_dir(std::path::Path::new("/x/Books.library")), Some("/d/Books".into()));
    }

    #[test]
    fn auto_import_delete_is_on_for_saved_states_without_it() {
        let p: Prefs = serde_json::from_str("{}").unwrap();
        assert!(p.auto_import_delete);
        let off: Prefs = serde_json::from_str(r#"{"auto_import_delete":false}"#).unwrap();
        assert!(!off.auto_import_delete);
    }

    #[test]
    fn api_token_and_eagle_preferences() {
        let t = new_token();
        let parts: Vec<usize> = t.split('-').map(str::len).collect();
        assert_eq!(parts, [8, 4, 4, 4, 12]);
        assert_eq!(&t[14..15], "4", "a version 4 UUID");
        assert_ne!(t, new_token());
        assert!(Prefs::default().api_token.is_empty(), "Eagle's default is empty");
        let mut p = Prefs::default().sanitized();
        assert!(p.api_token.is_empty(), "Eagle's default is empty; nothing is made on launch");
        let kept = Prefs { api_token: "abc".into(), ..Default::default() }.sanitized();
        assert_eq!(kept.api_token, "abc");
        let old = Prefs { api_token: t.clone(), ..Default::default() }.sanitized();
        assert_eq!(old.api_token, t, "a token made by an earlier version survives");
        let blank = Prefs { api_token: "   ".into(), ..Default::default() }.sanitized();
        assert_eq!(blank.api_token, "");
        let v = p.eagle_preferences();
        assert_eq!(v["developer"]["apiToken"], "");
        p.api_token = t.clone();
        let v = p.eagle_preferences();
        assert_eq!(v["general"]["showCollectModal"], "false");
        assert_eq!(v["habits"]["hoverZoom"], "on");
        assert_eq!(v["developer"]["apiToken"], p.api_token.as_str());
        p.take_eagle_preferences(&serde_json::json!({"general": {"showCollectModal": "true"}}));
        assert!(p.show_collect_modal);
    }

    #[test]
    fn a_vanished_auto_import_folder_is_forgotten() {
        let tmp = tempfile::tempdir().unwrap();
        let kept = Prefs { auto_import: true, auto_import_path: Some(tmp.path().to_path_buf()), ..Default::default() }.sanitized();
        assert!(kept.auto_import && kept.auto_import_path.is_some());
        let gone = Prefs { auto_import: true, auto_import_path: Some(tmp.path().join("gone")), ..Default::default() }.sanitized();
        assert!(!gone.auto_import && gone.auto_import_path.is_none());
        assert!(auto_import_refusal(&tmp.path().join("gone")).is_some());
        assert!(auto_import_refusal(tmp.path()).is_none());
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
        let form = |mode, old: &str, new: &str, again: &str| PasswordForm { mode: Some(mode), old: old.into(), new: new.into(), again: again.into(), hint: "tip".into(), error: None, touch_ticket: None };
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
        assert_eq!(labels, ["General", "Sidebar", "Controls", "Preview", "Screenshot", "Shortcuts", "Notification", "Password", "Auto-Import", "Developer"]);
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
        assert!(p.collect_folders.is_empty() && p.collect_recent.is_empty());
        let mut p = p;
        crate::collect::migrate(&mut p.collect_folders, &mut p.collect_folder);
        assert_eq!((p.collect_folders.as_slice(), p.collect_folder.as_deref()), (&["F1".to_string()][..], None));
        let new: Prefs = serde_json::from_str(r#"{"collect_folders":["A","B"],"collect_recent":["B"]}"#).unwrap();
        assert_eq!((new.collect_folders.len(), new.collect_recent.len()), (2, 1));
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
