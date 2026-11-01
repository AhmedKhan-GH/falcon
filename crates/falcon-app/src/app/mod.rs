//! The main window, laid out after Eagle's: sidebar, toolbar, card grid and
//! inspector, with Eagle's dark theme.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui::{
    self, Align2, Color32, CornerRadius, Key, Modifiers, Pos2, Rect, Response, RichText,
    Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2, text::LayoutJob, vec2,
};
use falcon_library::model::{Folder, Item};
use falcon_library::smart_draft::{self, ConditionDraft};
use falcon_library::{Library, thumbnail};

use crate::colors;
use crate::icons as ic;
use crate::toast::{Kind as ToastKind, Life, Toast};
use crate::loader::{Tier, job_for};
use crate::logic::{self, Layout, NavCounts, Scope};
use falcon_library::search::{ColorQuery, Logic, OrderBy, TextFilter};
use crate::theme::{self as t, font};
use crate::ui::widgets::{
    ChipClick, badge, chip, divider, ext_placeholder, field, folder_chip, galley, icon, icon_button, paint_thumb_bg, prop,
    rating, tag_chip_removable, viewer_ui, wide_button, window_drag,
};

// The app core.
pub(crate) mod actions;
pub(crate) mod commands;
pub(crate) mod state;
pub(crate) mod import;
pub(crate) mod library;
pub(crate) mod navigation;
#[path = "../image_edit.rs"]
pub(crate) mod image_edit;

// Sidebar.
pub(crate) use crate::ui::organize as organize_ui;
pub(crate) use crate::ui::sidebar::folders as folders_ui;
pub(crate) use crate::ui::sidebar::{smart_has, smart_name};

// Toolbar and filter bar.
pub(crate) use crate::ui::filter_bar::media as filters_ui;
pub(crate) use crate::ui::filter_bar::{filter_chip, parse_hex};

// Grid.
pub(crate) use crate::ui::grid::{GRID_GAP, GRID_PAD_TOP, GRID_PAD_X, list_span};
pub(crate) use crate::ui::grid::duplicates as duplicates_view;

// Inspector.
pub(crate) use crate::ui::inspector::tag_picker;

// Viewer.
pub(crate) use crate::ui::viewer::audio as media_view;
pub(crate) use crate::ui::viewer::font as font_view;
pub(crate) use crate::ui::viewer::image as image_view;
pub(crate) use crate::ui::viewer::video as video_view;
pub(crate) use crate::ui::viewer::gif as gif_view;
pub(crate) use crate::ui::viewer::open as open_ui;
pub(crate) use crate::ui::viewer::pdf as pdf_view;
pub(crate) use crate::ui::viewer::modes;
pub(crate) use crate::ui::viewer::preview_window;
pub(crate) use crate::ui::viewer::text as text_view;
pub(crate) use crate::ui::viewer::comments;
pub(crate) use crate::ui::viewer::crop;

// Dialogs and menus.
pub(crate) use crate::ui::dialogs::export as export_ui;
pub(crate) use crate::ui::dialogs::ext as ext_ui;
pub(crate) use crate::ui::dialogs::quick_switch as quick_switch_ui;
pub(crate) use crate::ui::dialogs::library_notices as library_ui;
pub(crate) use crate::ui::dialogs::library as library_dialogs;

pub(crate) use actions::{Action, BULK_CONFIRM};
use commands::editable_now;
use navigation::parent_scope;
pub use state::Launch;
pub(crate) use state::{
    Confirm, DragFolder, DragItems, DragQuick, EditBuf, Fold, Folds, Marquee, MultiEditBuf, Progress, Saved,
    SmartDraft, Switch, TILE_MAX, TILE_MIN, ViewState, Workspace,
};

pub struct App {
    /// The library being drawn, with all its per-library state.
    pub(crate) ws: Workspace,
    /// The other open libraries, kept whole while hidden.
    pub(crate) others: crate::workspaces::Parked<Workspace>,
    /// The window title last sent, so it is only sent on change.
    pub(crate) title: String,
    /// Open the library switcher next frame (⌘L).
    pub(crate) open_switcher: bool,
    /// The local HTTP API for the shown library, as Eagle runs it.
    pub(crate) server: Option<falcon_server::Handle>,
    pub(crate) ctx: egui::Context,
    pub(crate) error: Option<String>,
    pub(crate) tile: f32,
    pub(crate) show_sidebar: bool,
    pub(crate) show_inspector: bool,
    /// The Quick Access list is folded to its header.
    pub(crate) quick_collapsed: bool,
    /// The last answer to the Move to Trash question in a folder (take the
    /// items out of the folder only), kept while falcon runs, as Eagle does.
    pub(crate) trash_folder_only: bool,
    /// The sidebar's and inspector's widths, dragged by their inner edge.
    pub(crate) sidebar_width: f32,
    pub(crate) inspector_width: f32,
    pub(crate) focus_search: bool,
    /// Focus the sidebar's folder filter next frame (Find ▸ Filter Sidebar).
    pub(crate) focus_folder_filter: bool,
    /// Go to Folder (`quick_switch_ui.rs`).
    pub(crate) quick: quick_switch_ui::QuickSwitch,
    /// Eagle's in-app tag clipboard (Copy Tags), shared by the open
    /// libraries; the text also goes to the system clipboard.
    pub(crate) copied_tags: Option<Vec<String>>,
    /// Set while a drag out of the window is in progress.
    pub(crate) native_drag: Arc<std::sync::atomic::AtomicBool>,
    /// Background downloads of iCloud originals (`icloud.rs`).
    pub(crate) fetches: crate::icloud::Fetches,
    /// Open With lists by extension (`open_with.rs`).
    pub(crate) open_apps: crate::open_with::AppCache,
    /// Recently opened libraries, newest first.
    pub(crate) recent: Vec<PathBuf>,
    /// Folders items were last added to, newest first (`item_cmds.rs`).
    pub(crate) recent_folders: Vec<String>,
    /// The Rename dialog for several items, and the template it used last.
    pub(crate) item_rename: Option<commands::items::ItemRename>,
    pub(crate) rename_template: String,
    /// Items opened in each library, for Recently Used (`recent.rs`).
    pub(crate) recent_items: crate::recent::RecentItems,
    /// How long the viewer has shown its item, so it counts as used.
    pub(crate) view_timer: crate::recent::ViewTimer,
    /// The Recently Used list as it was when the page was opened, so
    /// items viewed from it don't jump to the top under the user (opening
    /// the page again refreshes it).
    pub(crate) recent_shown: Option<(PathBuf, Vec<String>)>,
    /// The Clear Library History dialog is showing.
    pub(crate) clear_history_open: bool,
    /// The libraries' `icon.png` textures.
    pub(crate) lib_icons: crate::library_icons::LibraryIcons,
    /// What to open once the frame is done (it replaces the library being
    /// drawn).
    pub(crate) switch_to: Option<Switch>,
    /// The create-library dialog and its follow-up steps.
    pub(crate) dialogs: library_dialogs::State,
    /// Whether the welcome flow has been finished (or skipped by having
    /// used falcon before).
    pub(crate) welcomed: bool,
    pub(crate) prefs: crate::prefs::Prefs,
    /// Search scope, pinned filters and Go to Folder history.
    pub(crate) search: crate::search_state::SearchPrefs,
    pub(crate) prefs_window: crate::prefs::Window,
    /// The app lock (Preferences ▸ Password).
    pub(crate) lock: crate::applock::Lock,
    pub(crate) shortcuts: crate::shortcuts::Shortcuts,
    /// The small always-on-top drop window (see `collect.rs`).
    pub(crate) collect: crate::collect::Collect,
    /// Name, description and tags the collect window adds to the import
    /// it is starting; set only for that call.
    pub(crate) collect_extra: Option<crate::collect::Meta>,
    /// Audio playback for the viewer.
    pub(crate) audio: crate::audio::Audio,
    /// Video playback for the viewer (`video_view.rs`).
    pub(crate) video: video_view::VideoState,
    /// The font viewer's tab and theme, kept across fonts and launches.
    pub(crate) font_look: font_view::FontLook,
    /// The PDF reader for the viewer (`pdf_view.rs`).
    pub(crate) pdf: pdf_view::PdfState,
    /// The image viewer's zoom mode and remembered views (`image_view.rs`).
    pub(crate) image: image_view::ImageState,
    /// Drop targets and other import state (`import.rs`).
    pub(crate) imports: import::Imports,
    /// The macOS menu bar (`menu.rs`).
    pub(crate) menu_bar: crate::menu::Bar,
    /// Layouts remembered per view, the fallback from before, and the
    /// display options (see `layout_panel.rs`).
    pub(crate) layouts: HashMap<String, Layout>,
    pub(crate) legacy_layout: Option<Layout>,
    pub(crate) display: logic::Display,
    /// Reload the shown library from disk next frame (the layout panel's
    /// Refresh).
    pub(crate) refresh: bool,
    /// The text viewer's zoom level, kept across items and launches.
    pub(crate) text_zoom: u8,
    /// Inspector sections folded away (`Folds`).
    pub(crate) folds: Folds,
    /// The thread watching the shown library's auto-import folder.
    pub(crate) auto_import: Option<crate::auto_import::Watcher>,
    /// Files it has handed over, shared with the thread and saved.
    pub(crate) auto_imported: Arc<Mutex<crate::auto_import::Imported>>,
    /// A folder that couldn't be watched and was reported, so it isn't
    /// checked again until the preferences change.
    pub(crate) auto_import_refused: Option<PathBuf>,
    /// Presentation mode and the other View modes (`modes.rs`).
    pub(crate) modes: modes::Modes,
    /// The item open in its own window (`preview_window.rs`).
    pub(crate) preview: Option<preview_window::PreviewWin>,

    /// The About window is open, and its icon once loaded (`about.rs`).
    pub(crate) about: bool,
    pub(crate) about_icon: Option<egui::TextureHandle>,
    /// What the arrow keys act on (Eagle's focus zones).
    pub(crate) zone: logic::Zone,
    /// The views of the sidebar's visible rows this frame, top to bottom,
    /// for ↑ and ↓ while the sidebar has the keys.
    pub(crate) side_rows: Vec<Scope>,
    /// The row the keys last opened, and whether to scroll it into view.
    pub(crate) side_cursor: Option<usize>,
    pub(crate) side_reveal: bool,
    /// A notice to acknowledge (`library_ui.rs`).
    pub(crate) alert: Option<library_ui::Alert>,
    /// Reload the shown library from disk next frame: a `metadata.json`
    /// write was refused because the file changed elsewhere.
    pub(crate) conflict_reload: bool,
    /// A library that can't be found, for the missing-library screen.
    pub(crate) missing: Option<PathBuf>,
    /// A merge of another library in progress (`library_ui.rs`).
    pub(crate) merge: Option<library_ui::Merge>,
    /// An `.eaglepack` import waiting for an answer.
    pub(crate) pack_ask: Option<library_ui::PackAsk>,
    /// Empty Cache and Reload is asking for confirmation.
    pub(crate) confirm_reload: bool,
    /// A library being read on a background thread (`library_ui.rs`).
    pub(crate) loading: Option<library_ui::Loading>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>, launch: Launch) -> Self {
        t::install(&cc.egui_ctx);
        let mut app = App {
            ws: Workspace::new(&cc.egui_ctx, None),
            others: Default::default(),
            title: String::new(),
            open_switcher: false,
            server: None,
            ctx: cc.egui_ctx.clone(),
            error: None,
            tile: 139.0,
            show_sidebar: true,
            sidebar_width: t::SIDEBAR_WIDTH,
            inspector_width: t::INSPECTOR_WIDTH,
            show_inspector: true,
            quick_collapsed: false,
            trash_folder_only: true,
            focus_search: false,
            focus_folder_filter: false,
            quick: Default::default(),
            copied_tags: None,
            native_drag: Arc::default(),
            imports: Default::default(),
            fetches: Default::default(),
            open_apps: Default::default(),
            recent: Vec::new(),
            recent_folders: Vec::new(),
            item_rename: None,
            rename_template: "Untitled - %N".into(),
            recent_items: Default::default(),
            view_timer: Default::default(),
            recent_shown: None,
            clear_history_open: false,
            lib_icons: Default::default(),
            switch_to: None,
            dialogs: Default::default(),
            welcomed: true,
            prefs: Default::default(),
            search: Default::default(),
            prefs_window: Default::default(),
            lock: Default::default(),
            shortcuts: Default::default(),
            collect: Default::default(),
            collect_extra: None,
            audio: Default::default(),
            video: Default::default(),
            font_look: Default::default(),
            pdf: Default::default(),
            image: Default::default(),
            menu_bar: Default::default(),
            layouts: HashMap::new(),
            legacy_layout: None,
            display: Default::default(),
            refresh: false,
            text_zoom: 0,
            folds: Folds::default(),
            auto_import: None,
            auto_imported: Arc::default(),
            auto_import_refused: None,
            modes: Default::default(),
            preview: None,

            about: false,
            about_icon: None,
            zone: logic::Zone::Content,
            side_rows: Vec::new(),
            side_cursor: None,
            side_reveal: false,
            alert: None,
            conflict_reload: false,
            missing: None,
            merge: None,
            pack_ask: None,
            confirm_reload: false,
            loading: None,
        };
        let saved: Saved = cc.storage.and_then(|s| eframe::get_value(s, eframe::APP_KEY)).unwrap_or_default();
        app.welcomed = library_dialogs::already_welcomed(saved.welcomed, saved.recent.len(), saved.libraries.len(), saved.library.is_some());
        app.tile = saved.tile.unwrap_or(app.tile).clamp(TILE_MIN, TILE_MAX);
        app.ws.sort = saved.sort.as_deref().and_then(|l| OrderBy::ALL.into_iter().find(|o| o.label() == l)).unwrap_or(app.ws.sort);
        app.ws.increase = saved.increase.unwrap_or(app.ws.increase);
        app.show_sidebar = saved.show_sidebar.unwrap_or(true);
        let clamp = |w: Option<f32>, default: f32| w.unwrap_or(default).clamp(*t::PANEL_WIDTHS.start(), *t::PANEL_WIDTHS.end());
        app.sidebar_width = clamp(saved.sidebar_width, t::SIDEBAR_WIDTH);
        app.inspector_width = clamp(saved.inspector_width, t::INSPECTOR_WIDTH);
        app.show_inspector = saved.show_inspector.unwrap_or(true);
        app.quick_collapsed = saved.quick_collapsed;
        app.ws.collapsed = saved.collapsed.into_iter().collect();
        app.recent = saved.recent;
        app.recent_folders = saved.recent_folders;
        app.legacy_layout = saved.layout;
        app.layouts = saved.layouts;
        app.display = saved.display;
        app.prefs = saved.prefs.sanitized();
        app.search = saved.search;
        // The interface size is a preference, as in Eagle, so egui's own
        // ⌘+ and ⌘− (which falcon uses to zoom the grid) must not change it.
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        cc.egui_ctx.set_zoom_factor(crate::prefs::zoom_factor(app.prefs.ui_zoom));
        app.prefs_window.set_pane(saved.prefs_pane);
        app.lock = crate::applock::Lock::at_launch(app.prefs.app_lock.as_ref());
        app.shortcuts = saved.shortcuts;
        app.collect.pos = saved.collect_pos.map(Pos2::from);
        app.video.volume = saved.video_volume.unwrap_or(1.0).clamp(0.0, 1.0);
        app.video.looping = saved.video_loop.unwrap_or(false);
        app.video.muted = saved.video_muted.unwrap_or(false);
        app.video.resume = saved.video_positions;
        app.pdf.set_marks(saved.pdf_marks);
        app.image.edge = saved.zoom_edge.unwrap_or(false);
        app.folds = saved.folds;
        app.text_zoom = saved.text_zoom.unwrap_or(0).min(crate::textdoc::ZOOM_LEVELS - 1);
        app.auto_imported = Arc::new(Mutex::new(saved.auto_imported));
        app.font_look = saved.font_look.unwrap_or_default();
        app.modes.hide_navigator = saved.hide_navigator.unwrap_or(false);
        app.recent_items = saved.recent_items;
        // Reopen the libraries that were open, then the one given on the
        // command line (focused if it was already open).
        app.missing = crate::workspaces::missing_shown(&saved.libraries, saved.active, saved.library.as_deref(), |p| p.is_dir());
        let (libraries, active) = crate::workspaces::restore(&saved.libraries, saved.active, saved.library.as_deref(), |p| p.is_dir());
        for p in libraries {
            app.open(p);
        }
        app.focus(active);
        if let Some(p) = initial {
            app.open(p);
        }
        if !launch.import.is_empty() {
            app.import(launch.import);
        }
        // One item opens in the viewer (unless --select); several are selected.
        if let [id] = launch.items.as_slice()
            && !launch.select
        {
            app.ws.viewer = Some(id.clone());
        }
        if let Some(first) = launch.items.first() {
            app.ws.scroll_to = Some(first.clone());
        }
        app.ws.selected = launch.items;
        if let Some(q) = launch.search {
            app.ws.query = q;
        }
        if let Some(rgb) = launch.color.as_deref().and_then(parse_hex) {
            app.ws.filter.color = Some(ColorQuery::new(rgb));
            app.ws.color_text = logic::hex(rgb);
            app.ws.show_filter = true;
        }
        if launch.tags {
            app.ws.scope = Scope::AllTags;
        }
        for link in launch.links {
            app.follow_link(link);
        }
        crate::links::set_repaint(&app.ctx);
        app
    }

}


pub(crate) type Shared = falcon_server::Shared;

pub(crate) fn lock(s: &Shared) -> std::sync::MutexGuard<'_, Library> {
    s.lock().unwrap_or_else(|p| p.into_inner())
}

/// The picture shown under the pointer while dragging out: a 128 px PNG of
/// `path`, or a plain tile when it can't be decoded. Always valid PNG, since
/// the drag crate panics on an image AppKit can't load.
pub(crate) fn drag_image(path: &std::path::Path) -> Vec<u8> {
    // A picture still in iCloud would hold the drag until it downloads.
    let decoded = if crate::icloud::is_dataless(path) { None } else { thumbnail::decode(path).ok() };
    let img = decoded
        .map(|i| i.thumbnail(128, 128))
        .unwrap_or_else(|| image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(96, 96, image::Rgba([60, 61, 64, 255]))));
    let mut out = std::io::Cursor::new(Vec::new());
    if img.to_rgba8().write_to(&mut out, image::ImageFormat::Png).is_err() {
        return Vec::new();
    }
    out.into_inner()
}

// ---------------------------------------------------------------------------
// Sidebar


// ---------------------------------------------------------------------------
// Toolbar


// ---------------------------------------------------------------------------
// Filter bar

// ---------------------------------------------------------------------------
// Grid


// ---------------------------------------------------------------------------
// Inspector



// ---------------------------------------------------------------------------
// Viewer and frame


impl eframe::App for App {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.save_state(storage);
    }

    fn persist_egui_memory(&self) -> bool {
        false
    }

    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // The red button or ⌘W on the window: like Eagle, falcon stays
        // in the Dock and comes back when its icon is clicked.
        if ctx.input(|i| i.viewport().close_requested()) && crate::dock::keep_running_hidden() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            for ws in std::iter::once(&mut self.ws).chain(self.others.parked_mut()) {
                let _ = ws.flush_and_backup();
            }
        }
        if self.lock.is_locked() {
            // Nothing but the lock screen acts while locked.
            let _ = crate::menu::take_clicks();
            self.lock_screen(ui);
        } else {
            self.menu_clicks(&ctx);
            self.draw(ui, frame);
        }
        let state = self.menu_state(&ctx);
        self.menu_bar.sync(&ctx, &self.shortcuts, &state);
    }
}

impl App {
    /// One frame of the main window.
    pub(crate) fn draw(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if ctx.cumulative_frame_nr() == 0 {
            // Come to the front on launch, like any Mac app.
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        self.follow_api_switch();
        self.follow_api_preferences();
        for link in crate::links::take() {
            self.follow_link(link);
        }
        self.poll_loading();
        if std::mem::take(&mut self.refresh) {
            self.refresh_library();
        }
        if std::mem::take(&mut self.conflict_reload) {
            self.reload_after_conflict();
        }
        self.sync_layout();
        self.follow_modes(&ctx);
        t::follow_system(&ctx);
        self.ws.loader.poll(&ctx);
        let title = match &self.ws.root {
            Some(r) => r.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            None => "Falcon".to_string(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        self.run_backups();
        self.apply_outside_changes();
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        // Our own items dragged out and back in must not import twice.
        let dragging_out = self.native_drag.load(std::sync::atomic::Ordering::Relaxed);
        if !dropped.is_empty() && !dragging_out {
            self.drop_files(dropped, crate::drop_pos::at_drop(&ctx));
        }
        self.pdf_keys(&ctx);
        self.video_keys(&ctx);
        self.gif_keys(&ctx);
        self.edit_keys(&ctx);
        self.keyboard(&ctx);
        if let Some(d) = self.prefs_window.show(&ctx) {
            self.apply_preferences(d);
        }
        self.import_dialogs(&ctx);
        self.about_ui(&ctx);
        self.library_dialogs(&ctx);
        if self.collect.open {
            self.collect_window(&ctx);
        }
        self.merge_dialogs(&ctx);
        self.pack_dialogs(&ctx);
        self.reload_confirm(&ctx);
        self.clear_history_dialog(&ctx);
        self.alert_modal(&ctx);
        if self.ws.lib.is_some() {
            self.missing_modal(&ctx);
            self.loading_modal(&ctx);
        }

        let Some(shared) = self.ws.lib.clone() else {
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
                if self.loading.is_some() {
                    self.loading_page(ui);
                } else if self.missing.is_some() {
                    self.missing_page(ui);
                } else {
                    self.empty_or_welcome(ui);
                }
            });
            return;
        };
        let mut guard = lock(&shared);
        let lib: &mut Library = &mut guard;
        self.begin_frame_edits(lib);
        // An item the viewer has shown for a second counts as used.
        let now = ctx.input(|i| i.time);
        if let Some(id) = self.view_timer.tick(self.ws.viewer.as_deref(), now) {
            self.recent_items.add(lib.root(), &id);
        }
        if let Some(wait) = self.view_timer.wait(now) {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
        }
        // Typing in a text item the viewer has just left (an arrow key,
        // Esc) is saved before the viewer opens the next one.
        self.text_saves(lib, ctx.input(|i| i.time));
        self.ws.up = parent_scope(&self.ws.scope, lib);
        self.ws.editable = editable_now(&self.acted_on(), lib);
        // Items can vanish under the selection (deleted here or over the API).
        if self.ws.selected.iter().any(|id| lib.item(id).is_none()) {
            self.ws.selected.retain(|id| lib.item(id).is_some());
        }
        // As in Eagle's detail mode, the inspector shows the item in the
        // viewer: the selection follows it.
        if let Some(id) = self.ws.viewer.clone()
            && self.ws.selected != [id.clone()]
            && lib.item(&id).is_some()
        {
            self.ws.selected = vec![id.clone()];
            self.ws.anchor = Some(id.clone());
            self.ws.scroll_to = Some(id);
        }

        let (show_sidebar, show_inspector) = self.panels_shown();
        if show_sidebar {
            // Dragging the inner edge resizes it, as in Eagle.
            let shown = egui::Panel::left("sidebar")
                .default_size(self.sidebar_width)
                .size_range(t::PANEL_WIDTHS)
                .resizable(true)
                .show_separator_line(false)
                .frame(egui::Frame::NONE)
                .show(ui, |ui| self.sidebar(ui, lib));
            self.sidebar_width = shown.response.rect.width();
        }
        if show_inspector {
            let shown = egui::Panel::right("inspector")
                .default_size(self.inspector_width)
                .size_range(t::PANEL_WIDTHS)
                .resizable(true)
                .show_separator_line(false)
                .frame(egui::Frame::NONE)
                .show(ui, |ui| self.inspector(ui, lib));
            self.inspector_width = shown.response.rect.width();
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t::p().main)).show(ui, |ui| {
            let full = ui.max_rect();
            if self.ws.viewer.is_some() {
                // Eagle's detail mode: the viewer, with its own bar, takes
                // the place of the toolbar, filter bar and grid, between
                // the sidebar and the inspector.
                self.refresh_order(lib);
                let window = ctx.content_rect();
                let (l, r) = logic::viewer_span(
                    window.left(),
                    window.right(),
                    show_sidebar.then_some(self.sidebar_width),
                    show_inspector.then_some(self.inspector_width),
                );
                let mut screen = Rect::from_x_y_ranges(l..=r, full.y_range()).intersect(full);
                // Presenting, the viewer's bar sits above the window, out
                // of sight, and the picture has the whole screen.
                if self.modes.presenting {
                    screen.min.y -= t::BAR_HEIGHT;
                }
                self.viewer(ui, lib, screen);
            }
            if self.ws.viewer.is_none() {
                let bar = Rect::from_min_size(full.min, vec2(full.width(), t::BAR_HEIGHT));
                let mut top = ui.new_child(egui::UiBuilder::new().max_rect(bar));
                self.toolbar(&mut top, lib);
                if !self.show_inspector {
                    let c = Pos2::new(bar.right() - 24.0, bar.center().y);
                    if icon_button(&mut top, c, ic::SIDEBAR_SIMPLE, 18.0, true, "Show inspector").clicked() {
                        self.show_inspector = true;
                    }
                }
                let mut grid_top = bar.bottom();
                let pb = Rect::from_min_size(Pos2::new(full.left(), grid_top), vec2(full.width(), 26.0));
                let mut pui = ui.new_child(egui::UiBuilder::new().max_rect(pb));
                if self.progress_bar(&mut pui, pb) || self.export_bar(&mut pui, pb) {
                    grid_top = pb.bottom();
                }
                if self.ws.show_filter {
                    let fb = Rect::from_min_size(Pos2::new(full.left(), bar.bottom()), vec2(full.width(), 40.0));
                    let mut fui = ui.new_child(egui::UiBuilder::new().max_rect(fb));
                    self.filter_bar(&mut fui, lib);
                    grid_top = fb.bottom();
                }
                let grid = Rect::from_min_max(Pos2::new(full.left(), grid_top), full.max);
                let mut g = ui.new_child(egui::UiBuilder::new().max_rect(grid));
                g.set_clip_rect(grid);
                self.grid(&mut g, lib);
                self.error_toast(ui, Pos2::new(full.center().x, grid_top + 10.0));
            }

            self.status_toast(ui, &ctx, full);
        });
        // Playback stops when the viewer closes or moves to another item.
        if self.audio.loaded().is_some() && self.audio.loaded() != self.ws.viewer.as_deref() {
            self.audio.stop();
        }
        self.drop_hint(&ctx);
        self.video.keep_only(self.ws.viewer.as_deref());
        self.fetches.prune((self.ws.view.id.as_str() == self.ws.viewer.as_deref().unwrap_or_default()).then_some(self.ws.view.file.as_path()));
        self.pdf.keep_only(self.ws.viewer.as_deref());
        // Items dragged past the window edge become a macOS file drag, so
        // they can be dropped into Finder or other apps, as from Eagle.
        if let Some(d) = egui::DragAndDrop::payload::<DragItems>(&ctx)
            && ctx.pointer_latest_pos().is_some_and(|p| !ctx.content_rect().contains(p))
            && !self.native_drag.load(std::sync::atomic::Ordering::Relaxed)
        {
            let files: Vec<PathBuf> = d.0.iter().filter_map(|id| lib.item(id)).map(|i| i.original_path(lib.root())).collect();
            let preview = d.0.first().and_then(|id| lib.item(id)).map(|i| {
                i.thumbnail_path(lib.root()).unwrap_or_else(|| i.original_path(lib.root()))
            });
            egui::DragAndDrop::clear_payload(&ctx);
            if let Some(image) = preview.filter(|_| !files.is_empty()).map(|p| drag_image(&p)) {
                let flag = self.native_drag.clone();
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
                let done = flag.clone();
                let ctx2 = ctx.clone();
                let started = drag::start_drag(
                    &*frame,
                    drag::DragItem::Files(files),
                    drag::Image::Raw(image),
                    move |_, _| {
                        done.store(false, std::sync::atomic::Ordering::Relaxed);
                        ctx2.request_repaint();
                    },
                    drag::Options::default(),
                );
                if let Err(e) = started {
                    flag.store(false, std::sync::atomic::Ordering::Relaxed);
                    self.flash_error(format!("Could not drag out: {e}"));
                }
            }
        }
        if let Some(d) = egui::DragAndDrop::payload::<DragItems>(&ctx)
            && let Some(pos) = ctx.pointer_latest_pos()
        {
            let text = if d.0.len() == 1 { "1 item".to_string() } else { format!("{} items", d.0.len()) };
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("drag-ghost")));
            let g = painter.layout_no_wrap(text, font(12.0), Color32::WHITE);
            let r = Rect::from_min_size(pos + vec2(12.0, 12.0), g.size() + vec2(16.0, 8.0));
            painter.rect_filled(r, CornerRadius::same(10), t::p().primary);
            painter.galley(r.min + vec2(8.0, 4.0), g, Color32::WHITE);
        }
        if self.ws.smart_dialog.is_some() {
            self.smart_dialog_ui(&ctx, lib);
        }
        self.organize_dialogs(&ctx, lib);
        self.tag_picker_ui(&ctx, lib);
        self.quick_switch_ui(&ctx, lib);
        self.folder_dialogs(&ctx, lib);
        self.ext_dialog(&ctx);
        self.item_rename_dialog(&ctx, lib);
        self.collect_duplicates();
        self.duplicate_dialog(&ctx, lib);
        self.export_dialogs(&ctx, lib);
        if let Some(c) = self.ws.confirm.clone() {
            let (title, detail, label) = match &c {
                Confirm::DeleteFolders(ids) => {
                    let title = match ids.as_slice() {
                        [one] => format!("Delete “{}”?", logic::find_folder(&lib.metadata.folders, one).map_or("", |f| f.name.as_str())),
                        _ => format!("Delete {} folders?", ids.len()),
                    };
                    (title, "Subfolders are deleted too.".to_string(), "Delete")
                }
                Confirm::DeleteSmarts(ids) => (
                    format!("Delete {} smart folders?", ids.len()),
                    "Only the smart folders go; the items they show stay.".to_string(),
                    "Delete",
                ),
                Confirm::DeleteItems(ids) => (
                    format!("Delete {} item{} permanently?", ids.len(), if ids.len() == 1 { "" } else { "s" }),
                    "The files are removed from the library and cannot be restored.".to_string(),
                    "Delete",
                ),
                Confirm::DeleteTags(tags) => (
                    if let [one] = tags.as_slice() { format!("Delete the tag “{one}”?") } else { format!("Delete {} tags?", tags.len()) },
                    "They are removed from every item. The items themselves stay.".to_string(),
                    if tags.len() == 1 { "Delete Tag" } else { "Delete Tags" },
                ),
                Confirm::ClearTagHistory => (
                    "Clear the tag history?".to_string(),
                    "The tag picker's Recent tags start over. Tags stay on their items.".to_string(),
                    "Clear",
                ),
                Confirm::EmptyTrash => {
                    let n = lib.items().iter().filter(|i| i.is_deleted).count();
                    (format!("Empty the Trash ({n})?"), "The files are removed from the library and cannot be restored.".to_string(), "Empty Trash")
                }
                Confirm::TrashInFolder(ids, _) => (
                    format!("Move {} to Trash?", export_ui::plural(ids.len(), "item")),
                    "Some of them are in other folders as well.".to_string(),
                    if self.trash_folder_only { "Remove" } else { "Move to Trash" },
                ),
                Confirm::RestoreAll => {
                    let n = lib.items().iter().filter(|i| i.is_deleted).count();
                    (format!("Restore all {n} items in the Trash?"), "They go back to their folders. ⌘Z puts them back in the Trash.".to_string(), "Restore All")
                }
            };
            let modal = egui::Modal::new(egui::Id::unique("confirm")).show(&ctx, |ui| {
                ui.set_width(320.0);
                ui.label(RichText::new(title).size(15.0));
                ui.add_space(4.0);
                ui.label(RichText::new(detail).color(t::p().text_muted));
                if matches!(c, Confirm::TrashInFolder(..)) {
                    ui.add_space(6.0);
                    ui.radio_value(&mut self.trash_folder_only, true, "Only take them out of this folder")
                        .on_hover_text("Items in no other folder still go to the Trash");
                    ui.radio_value(&mut self.trash_folder_only, false, "Move them all to the Trash");
                }
                if matches!(c, Confirm::DeleteFolders(_)) {
                    ui.add_space(6.0);
                    ui.checkbox(&mut self.ws.folders.trash_items, "Move the items to the Trash as well")
                        .on_hover_text("Items that are also in another folder stay. Untick to keep every item.");
                }
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(RichText::new(label).color(t::p().icon_red)).clicked() {
                        self.ws.actions.push(match c.clone() {
                            Confirm::DeleteFolders(ids) => Action::Folders(folders_ui::FolderAction::Delete(ids, self.ws.folders.trash_items)),
                            Confirm::DeleteSmarts(ids) => Action::Folders(folders_ui::FolderAction::DeleteSmart(ids)),
                            Confirm::DeleteItems(ids) => Action::DeletePermanently(ids),
                            Confirm::EmptyTrash => Action::EmptyTrash,
                            Confirm::TrashInFolder(ids, fid) => Action::TrashInFolder(ids, fid, self.trash_folder_only),
                            Confirm::RestoreAll => Action::Trash(lib.items().iter().filter(|i| i.is_deleted).map(|i| i.id.clone()).collect(), false),
                            Confirm::DeleteTags(t) => Action::DeleteTags(t),
                            Confirm::ClearTagHistory => Action::ClearTagHistory,
                        });
                        self.ws.confirm = None;
                    }
                    if ui.button("Cancel").clicked() {
                        self.ws.confirm = None;
                    }
                });
            });
            if modal.should_close() {
                self.ws.confirm = None;
            }
        }
        if let Some((n, _)) = &self.ws.bulk {
            let n = *n;
            let mut answer = None;
            let modal = egui::Modal::new(egui::Id::unique("bulk")).show(&ctx, |ui| {
                ui.set_width(320.0);
                ui.label(RichText::new(format!("Change {n} items?")).size(15.0));
                ui.add_space(4.0);
                ui.label(RichText::new("Everything selected is changed at once.").color(t::p().text_muted));
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("OK").clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        answer = Some(false);
                    }
                });
            });
            if modal.should_close() {
                answer = Some(false);
            }
            match answer {
                Some(true) => {
                    if let Some((_, actions)) = self.ws.bulk.take() {
                        self.ws.actions.extend(actions);
                    }
                }
                Some(false) => self.ws.bulk = None,
                None => {}
            }
        }
        self.text_saves(lib, ctx.input(|i| i.time));
        self.preview_window(&ctx, lib);
        self.apply_actions(lib);
        self.end_frame_edits(lib);
        self.check_conflict(lib);
        self.ws.loader.keep_original(self.ws.viewer.as_deref());
        self.ws.loader.want(std::mem::take(&mut self.ws.frame_jobs));
        drop(guard);
        match self.switch_to.take() {
            Some(Switch::Open(p)) => self.open_in_background(p),
            Some(Switch::Pick) => self.pick_library(),
            Some(Switch::Create) => self.new_library(),
            Some(Switch::Focus(i)) => self.focus(i),
            Some(Switch::Close(i)) => self.close_workspace(i),
            Some(Switch::Merge) => self.merge_library(),
            None => {}
        }
        self.run_import_request();
        self.auto_import_tick();
    }
}

#[cfg(test)]
mod tests {
    use super::drag_image;

    fn is_png(bytes: &[u8]) -> bool {
        image::load_from_memory_with_format(bytes, image::ImageFormat::Png).is_ok()
    }

    #[test]
    fn drag_images_are_always_valid_png() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/Assets.library");
        if let Ok(lib) = falcon_library::Library::open(&fixture)
            && let Some(thumb) = lib.items().iter().find_map(|i| i.thumbnail_path(lib.root()))
        {
            assert!(is_png(&drag_image(&thumb)), "WebP thumbnail");
        }
        let dir = tempfile::tempdir().unwrap();
        let txt = dir.path().join("notes.txt");
        std::fs::write(&txt, "hello").unwrap();
        assert!(is_png(&drag_image(&txt)), "undecodable file");
        assert!(is_png(&drag_image(&dir.path().join("missing.png"))), "missing file");
    }
}
