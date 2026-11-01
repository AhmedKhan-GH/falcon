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
pub(crate) mod import_ui;
#[path = "../image_edit.rs"]
pub(crate) mod image_edit;

// Sidebar.
pub(crate) use crate::ui::organize as organize_ui;
pub(crate) use crate::ui::sidebar::folders as folders_ui;
pub(crate) use crate::ui::sidebar::{smart_has, smart_name};

// Toolbar and filter bar.
pub(crate) mod filter_bar;
pub(crate) mod filters_ui;
pub(crate) mod layout_panel;

// Grid.
pub(crate) mod duplicates_view;
pub(crate) mod empty_view;
pub(crate) mod drop_hint;
#[path = "../covers.rs"]
pub(crate) mod covers;

// Inspector.
pub(crate) mod tag_picker;

// Viewer.
pub(crate) mod media_view;
pub(crate) mod font_view;
pub(crate) mod image_view;
pub(crate) mod video_view;
pub(crate) mod gif_view;
pub(crate) mod open_ui;
pub(crate) mod pdf_view;
pub(crate) mod modes;
pub(crate) mod preview_window;
pub(crate) mod text_view;
#[path = "../comments.rs"]
pub(crate) mod comments;
#[path = "../crop.rs"]
pub(crate) mod crop;

// Dialogs and menus.
pub(crate) mod about;
pub(crate) mod debug_ui;
pub(crate) mod dupes_ui;
pub(crate) mod export_ui;
pub(crate) mod ext_ui;
pub(crate) mod quick_switch_ui;
pub(crate) mod library_ui;
pub(crate) mod library_dialogs;

pub(crate) use actions::{Action, BULK_CONFIRM};
use commands::editable_now;
pub use state::Launch;
pub(crate) use state::{
    Confirm, DragFolder, DragItems, DragQuick, EditBuf, Fold, Folds, Marquee, MultiEditBuf, Progress, Saved,
    SmartDraft, Switch, TILE_MAX, TILE_MIN, ViewState, Workspace,
};

/// Grid geometry, measured from Eagle at 1280 x 800.
pub(crate) const GRID_PAD_X: f32 = 17.0;
pub(crate) const GRID_PAD_TOP: f32 = 4.0;
pub(crate) const GRID_GAP: f32 = 8.0;
pub(crate) const ROW_GAP: f32 = 10.0;

/// The parent of the open folder or smart folder, as a scope.
pub(crate) fn parent_scope(scope: &Scope, lib: &Library) -> Option<Scope> {
    use crate::organize::locate;
    match scope {
        Scope::Folder(id) => locate(&lib.metadata.folders, id).and_then(|(p, _)| p).map(|p| Scope::Folder(p.to_string())),
        Scope::Smart(id) => locate(&lib.smart_folders(), id).and_then(|(p, _)| p.map(|p| Scope::Smart(p.to_string()))),
        _ => None,
    }
}

/// What the toast says when opening a library restored its damaged
/// `metadata.json` from `backup` (see `Library::open_or_restore`).
pub(crate) fn restored_note(backup: &std::path::Path) -> String {
    let lib = backup.parent().and_then(std::path::Path::parent).and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let name = backup.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    format!("{lib}'s metadata.json was damaged and was restored from backup/{name}; the damaged file was kept in backup/")
}

pub(crate) fn is_hidden(p: &std::path::Path) -> bool {
    p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))
}

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
    /// Drop targets and other import state (`import_ui.rs`).
    pub(crate) imports: import_ui::Imports,
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

    /// Opens an `eagle://` link in the shown library, as Eagle does: an
    /// item opens in the viewer inside its first folder (or All), a folder
    /// or smart folder is shown. Ids the library doesn't have are ignored.
    pub(crate) fn follow_link(&mut self, link: crate::links::Link) {
        use crate::links::Link;
        let Some(shared) = self.ws.lib.clone() else { return };
        let target = {
            let lib = lock(&shared);
            match &link {
                Link::Item(id) => lib.item(id).map(|i| match i.folders.first().filter(|f| logic::find_folder(&lib.metadata.folders, f).is_some()) {
                    Some(f) => Scope::Folder(f.clone()),
                    None => Scope::All,
                }),
                Link::Folder(id) => logic::find_folder(&lib.metadata.folders, id).map(|_| Scope::Folder(id.clone())),
                Link::Smart(id) => lib.smart_folders().iter().any(|f| smart_has(f, id)).then(|| Scope::Smart(id.clone())),
            }
        };
        let Some(scope) = target else { return };
        self.go(scope);
        if let Link::Item(id) = link {
            self.ws.selected = vec![id.clone()];
            self.ws.anchor = Some(id.clone());
            self.ws.scroll_to = Some(id.clone());
            self.ws.viewer = Some(id);
        }
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    /// Opens the library at `path` in a workspace of its own and shows it,
    /// or shows it if it is already open.
    pub(crate) fn open(&mut self, path: PathBuf) {
        if let Some(i) = self.position_of(&path) {
            self.focus(i);
            crate::workspaces::remember(&mut self.recent, &path, RECENT_LIMIT);
            return;
        }
        let result = falcon_library::sandbox::guard(&path).and_then(|()| Library::open_or_restore(&path));
        self.opened(path, result);
    }

    /// Shows a library [`App::open`] or a background load
    /// (`library_ui::Loading`) read, or reports why it couldn't be read.
    pub(crate) fn opened(&mut self, path: PathBuf, result: falcon_library::Result<(Library, Option<PathBuf>)>) {
        match result {
            Ok((lib, restored)) => {
                let skipped = lib.skipped.len();
                self.add_workspace(Arc::new(Mutex::new(lib)));
                crate::workspaces::remember(&mut self.recent, &path, RECENT_LIMIT);
                self.error = None;
                self.welcomed = true;
                self.missing = None;
                let mut notes = Vec::new();
                if let Some(backup) = restored {
                    notes.push(restored_note(&backup));
                }
                if skipped > 0 {
                    let s = if skipped == 1 { "" } else { "s" };
                    notes.push(format!("{skipped} item{s} couldn't be read and are hidden; their files are untouched"));
                }
                if !notes.is_empty() {
                    self.flash(notes.join(". "));
                }
            }
            Err(_) if !path.exists() => self.library_gone(path),
            Err(e) => {
                if let Some(alert) = library_ui::alert_for(&e) {
                    crate::errlog::error(&format!("Could not open the library: {e}"));
                    self.show_alert(alert);
                } else if self.ws.lib.is_some() {
                    self.flash_error(format!("Could not open the library: {e}"));
                } else {
                    crate::errlog::error(&format!("Could not open the library: {e}"));
                }
                self.error = Some(e.to_string());
            }
        }
    }

    /// Shows `lib` in a new workspace, after the open ones (or in place of
    /// the empty window). It starts with the shown library's sort and
    /// layout.
    pub(crate) fn add_workspace(&mut self, lib: Shared) {
        let mut ws = Workspace::new(&self.ctx, Some(lib));
        ws.sort = self.ws.sort;
        ws.increase = self.ws.increase;
        ws.layout = self.ws.layout;
        if self.ws.lib.is_none() {
            // The empty window holds the folder state restored on launch.
            ws.collapsed = std::mem::take(&mut self.ws.collapsed);
            self.ws = ws;
        } else {
            self.others.push(&mut self.ws, ws);
        }
        self.serve_active();
    }

    /// Where the library at `path` sits in the switcher, if it is open.
    pub(crate) fn position_of(&self, path: &std::path::Path) -> Option<usize> {
        self.others.iter(&self.ws).position(|w| w.root.as_deref().is_some_and(|r| same_path(r, path)))
    }

    /// Shows the open library at switcher position `i`.
    pub(crate) fn focus(&mut self, i: usize) {
        if i == self.others.slot() {
            return;
        }
        self.others.focus(&mut self.ws, i);
        self.focus_search = false;
        self.serve_active();
    }

    /// Closes the open library at switcher position `i`, writing anything
    /// pending first. Closing the last one leaves the empty window.
    pub(crate) fn close_workspace(&mut self, i: usize) {
        let mut closed = match self.others.close(&mut self.ws, i) {
            Some(w) => w,
            None if i == self.others.slot() => {
                let mut empty = Workspace::new(&self.ctx, None);
                (empty.sort, empty.increase, empty.layout) = (self.ws.sort, self.ws.increase, self.ws.layout);
                std::mem::replace(&mut self.ws, empty)
            }
            None => return,
        };
        if let Err(e) = closed.flush_and_backup() {
            self.flash_error(format!("Could not save: {e}"));
        }
        drop(closed);
        self.serve_active();
    }

    /// Points the HTTP API at the shown library: swaps it into the running
    /// server, starts one if none runs, or stops it when nothing is open.
    pub(crate) fn serve_active(&mut self) {
        match (&self.server, self.ws.lib.clone()) {
            (Some(h), Some(lib)) => {
                h.set_library(lib);
                self.share_with_server();
            }
            (None, Some(_)) => self.start_server(),
            (_, None) => self.server = None,
        }
    }

    /// `library/switch` over the API replaces the library inside the shown
    /// workspace's slot. Give that slot its own library back and show the
    /// new one in a workspace of its own (an existing one if it is open),
    /// with fresh state and no undo history.
    pub(crate) fn follow_api_switch(&mut self) {
        let (Some(shared), Some(was)) = (self.ws.lib.clone(), self.ws.root.clone()) else { return };
        let now = lock(&shared).root().to_path_buf();
        if now == was {
            return;
        }
        let existing = self.position_of(&now);
        let mut note = None;
        match Library::open_or_restore(&was) {
            Ok((back, restored)) => {
                note = restored.as_deref().map(restored_note);
                let new = std::mem::replace(&mut *lock(&shared), back);
                match existing {
                    // The open workspace's copy is the one with its edits.
                    Some(i) => {
                        drop(new);
                        self.focus(i);
                    }
                    None => self.add_workspace(Arc::new(Mutex::new(new))),
                }
            }
            Err(_) => {
                // The old library can't be reopened: this workspace becomes
                // the new one, starting over.
                let mut ws = Workspace::new(&self.ctx, Some(shared));
                (ws.sort, ws.increase, ws.layout) = (self.ws.sort, self.ws.increase, self.ws.layout);
                self.ws = ws;
                if let Some(i) = existing {
                    self.close_workspace(i);
                }
            }
        }
        crate::workspaces::remember(&mut self.recent, &now, RECENT_LIMIT);
        let name = now.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        match note {
            Some(note) => self.flash(format!("Switched to {name} over the API. {note}")),
            None => self.flash(format!("Switched to {name} over the API")),
        }
        self.serve_active();
    }

    /// (Re)starts the HTTP API for the shown library on the ports set in
    /// Preferences. Never Eagle's own port: clients of a running (or later
    /// started) Eagle must keep reaching Eagle.
    pub(crate) fn start_server(&mut self) {
        self.server = None;
        let (Some(shared), Some(ports)) = (self.ws.lib.clone(), self.prefs.api_ports()) else { return };
        let ctx = self.ctx.clone();
        match falcon_server::start(shared, ports, move || ctx.request_repaint()) {
            Ok(h) => {
                self.flash(format!("API on http://localhost:{}", h.port));
                self.server = Some(h);
                self.share_with_server();
            }
            Err(e) => self.flash_error(format!("API not started: {e}")),
        }
    }

    /// Hands the API what Eagle's reads from the app: the Developer token,
    /// the preferences `application/info` reports, and the recent
    /// libraries `library/history` lists.
    pub(crate) fn share_with_server(&self) {
        let Some(h) = &self.server else { return };
        h.set_token(&self.prefs.api_token);
        h.set_preferences(self.prefs.eagle_preferences());
        h.set_history(self.recent.iter().map(|p| p.display().to_string()).collect());
    }

    /// Keeps what the API changed in the preferences (`preferences/collect/*`).
    pub(crate) fn follow_api_preferences(&mut self) {
        if let Some(v) = self.server.as_ref().and_then(|h| h.take_preferences()) {
            self.prefs.take_eagle_preferences(&v);
        }
    }

    pub(crate) fn open_preferences(&mut self) {
        let panels = crate::prefs::Panels { sidebar: self.show_sidebar, inspector: self.show_inspector };
        self.prefs_window.show_with(crate::prefs::Draft { prefs: self.prefs.clone(), panels, keys: self.shortcuts.clone() });
        self.prefs_window.library = self.ws.root.clone();
    }

    /// Lock Now: hides everything behind the lock screen, stopping playback
    /// and closing Preferences and the collect window.
    pub(crate) fn lock_now(&mut self) {
        if !self.lock.lock(self.prefs.app_lock.as_ref()) {
            return;
        }
        self.video.keep_only(None);
        self.audio.stop();
        self.prefs_window.close();
        if self.collect.open {
            self.collect.toggle();
        }
    }

    /// The lock screen: the password field, its hint and Unlock.
    pub(crate) fn lock_screen(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        let ctx = ui.ctx().clone();
        if ctx.cumulative_frame_nr() == 0 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        t::follow_system(&ctx);
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t::p().main)).show(ui, |ui| {
            window_drag(ui, Rect::from_min_size(rect.min, vec2(rect.width(), t::BAR_HEIGHT)), "lock");
            let card = Rect::from_center_size(rect.center(), vec2(300.0, 220.0));
            let mut c = ui.new_child(egui::UiBuilder::new().max_rect(card).layout(egui::Layout::top_down(egui::Align::Center)));
            c.label(RichText::new(ic::LOCK_SIMPLE).font(font(40.0)).color(t::p().text_muted));
            c.add_space(8.0);
            c.label(RichText::new("Falcon is locked").size(16.0).color(t::p().text));
            c.add_space(12.0);
            let field = c.add(
                TextEdit::singleline(&mut self.lock.typed)
                    .password(true)
                    .hint_text(RichText::new("Password").color(t::p().text_muted))
                    .desired_width(220.0)
                    .id_salt("lock-password"),
            );
            if !field.has_focus() && !field.lost_focus() {
                field.request_focus();
            }
            let enter = field.lost_focus() && ctx.input(|i| i.key_pressed(Key::Enter));
            c.add_space(8.0);
            let unlock = c.add(egui::Button::new(RichText::new("Unlock").color(Color32::WHITE)).fill(t::p().primary).min_size(vec2(220.0, 0.0))).clicked();
            if (enter || unlock) && self.lock.try_unlock(self.prefs.app_lock.as_ref()) {
                return;
            }
            if self.lock.failed {
                c.label(RichText::new("Wrong password").size(12.0).color(t::p().icon_red));
            }
            let hint = self.prefs.app_lock.as_ref().map(|s| s.hint.clone()).unwrap_or_default();
            if !hint.is_empty() {
                if self.lock.show_hint {
                    c.label(RichText::new(hint).size(12.0).color(t::p().text_muted));
                } else if c.link(RichText::new("Show hint").size(12.0)).clicked() {
                    self.lock.show_hint = true;
                }
            }
        });
    }

    /// Takes the Preferences window's Save or Apply.
    pub(crate) fn apply_preferences(&mut self, d: crate::prefs::Draft) {
        let restart = self.prefs.api_changed(&d.prefs);
        // The collect folder is chosen in the collect window, which may
        // have changed it while Preferences was open.
        let folder = self.prefs.collect_folder.take();
        let chosen = std::mem::take(&mut self.prefs.collect_folders);
        let recent = std::mem::take(&mut self.prefs.collect_recent);
        self.prefs = d.prefs;
        self.prefs.collect_folder = folder;
        self.prefs.collect_folders = chosen;
        self.prefs.collect_recent = recent;
        self.ctx.set_zoom_factor(crate::prefs::zoom_factor(self.prefs.ui_zoom));
        self.show_sidebar = d.panels.sidebar;
        self.show_inspector = d.panels.inspector;
        self.shortcuts = d.keys;
        // A folder refused before is checked again.
        self.auto_import_refused = None;
        self.share_with_server();
        if restart {
            self.start_server();
            if self.ws.lib.is_some() && !self.prefs.api_enabled {
                self.flash("API stopped");
            }
        }
    }

    /// Draws the collect window and imports what it receives into its
    /// chosen folder.
    pub(crate) fn collect_window(&mut self, ctx: &egui::Context) {
        use crate::collect;
        collect::migrate(&mut self.prefs.collect_folders, &mut self.prefs.collect_folder);
        let (folders, pool) = match self.ws.lib.as_ref() {
            Some(l) => {
                let lib = lock(l);
                let pool = collect::TagPool {
                    starred: lib.tags.starred_tags.clone(),
                    history: lib.tags.history_tags.clone(),
                    all: logic::tag_counts(lib.items()).into_iter().map(|(t, _)| t).collect(),
                };
                (Some(collect::flatten(&lib.metadata.folders)), pool)
            }
            None => (None, collect::TagPool::default()),
        };
        let mut state = collect::State { selected: &mut self.prefs.collect_folders, recent: &mut self.prefs.collect_recent };
        let event = self.collect.show(ctx, folders.as_deref(), &mut state, &pool, &self.shortcuts);
        let target = folders.as_deref().map(|f| collect::target(f, &self.prefs.collect_folders)).unwrap_or_default();
        match event {
            Some(collect::Event::CreateFolder(name)) => {
                if let Some(l) = self.ws.lib.clone() {
                    let made = lock(&l).create_folder(None, &name);
                    match made {
                        Ok(id) => self.prefs.collect_folders.push(id),
                        Err(e) => self.flash(format!("Could not create the folder: {e}")),
                    }
                }
            }
            Some(e @ (collect::Event::Files(_) | collect::Event::Paste)) => {
                self.collect_extra = Some(self.collect.meta.clone());
                match e {
                    collect::Event::Files(paths) => self.import_into(paths, target.clone()),
                    _ => self.paste_into(target.clone()),
                }
                self.collect_extra = None;
                self.collect.clear_meta();
                collect::push_recent(&mut self.prefs.collect_recent, &target);
            }
            None => {}
        }
    }

    /// Library ▸ Clear Library History: asks first. Shown with or without
    /// a library open.
    pub(crate) fn clear_history_dialog(&mut self, ctx: &egui::Context) {
        if self.clear_history_open {
            let modal = egui::Modal::new(egui::Id::unique("clear-library-history")).show(ctx, |ui| {
                ui.set_width(320.0);
                ui.label(RichText::new("Clear Library History").size(15.0));
                ui.add_space(4.0);
                ui.label(RichText::new("The list of recent libraries is emptied, except for the libraries that are open now. This can't be undone. No library is deleted.").color(t::p().text_muted));
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(RichText::new("Clear All").color(t::p().icon_red)).clicked() {
                        let open: Vec<PathBuf> = self.others.iter(&self.ws).filter_map(|w| w.root.clone()).collect();
                        self.recent = crate::workspaces::clear_history(&self.recent, &open);
                        self.clear_history_open = false;
                    }
                    if ui.button("Cancel").clicked() {
                        self.clear_history_open = false;
                    }
                });
            });
            if modal.should_close() {
                self.clear_history_open = false;
            }
        }
    }

    /// Opens the create-library dialog (`library_dialogs.rs`).
    pub(crate) fn new_library(&mut self) {
        self.dialogs.start_create();
    }

    /// Asks for a library and opens it in the background, with a loading
    /// screen.
    pub(crate) fn pick_library(&mut self) {
        if let Some(p) = rfd::FileDialog::new().set_title("Open Eagle Library").set_directory(falcon_library::sandbox::default_root()).pick_folder() {
            self.open_in_background(p);
        }
    }

    pub(crate) fn go(&mut self, scope: Scope) {
        let scope = match scope {
            Scope::Random(_) => Scope::Random(fastrand_seed()),
            s => s,
        };
        // Opening any view, even the one open, drops picked folders and
        // takes a fresh look at Recently Used.
        self.ws.folders.clear();
        self.recent_shown = None;
        if scope == self.ws.scope {
            return;
        }
        // The Duplicates page can scan the view it was opened from.
        if scope == Scope::Duplicates {
            self.ws.duplicates.opened_from(self.ws.scope.clone(), self.ws.order.clone(), self.ws.selected.clone());
        }
        self.ws.back.push(std::mem::replace(&mut self.ws.scope, scope));
        self.ws.forward.clear();
        self.ws.selected.clear();
        self.ws.anchor = None;
        self.view_changed();
    }

    /// The shown library's path, the key its pages share a layout under.
    pub(crate) fn root_key(&self) -> String {
        self.ws.root.as_ref().map(|r| r.to_string_lossy().into_owned()).unwrap_or_default()
    }

    /// Puts the layout the current view opens in into `ws.layout`.
    pub(crate) fn sync_layout(&mut self) {
        self.ws.layout = logic::layout_for(&self.layouts, &self.ws.scope, &self.root_key(), self.legacy_layout);
    }

    /// Switches the current view to `layout` and remembers it for this
    /// folder or smart folder, or for the library's other pages, as Eagle
    /// does.
    pub(crate) fn set_layout(&mut self, layout: Layout) {
        self.layouts.insert(logic::layout_key(&self.ws.scope, &self.root_key()), layout);
        self.ws.layout = layout;
    }

    /// The layout panel's Refresh: writes anything pending, reads the shown
    /// library again from disk, decodes its thumbnails again and reshuffles
    /// random orders, keeping the view and selection.
    pub(crate) fn refresh_library(&mut self) {
        let (Some(shared), Some(root)) = (self.ws.lib.clone(), self.ws.root.clone()) else { return };
        if let Err(e) = self.ws.flush_and_backup() {
            self.flash_error(format!("Could not save: {e}"));
            return;
        }
        match falcon_library::sandbox::guard(&root).and_then(|()| Library::open_or_restore(&root)) {
            Ok((fresh, _)) => {
                let ids: Vec<String> = {
                    let mut lib = lock(&shared);
                    let ids = lib.items().iter().map(|i| i.id.clone()).collect();
                    *lib = fresh;
                    ids
                };
                for id in &ids {
                    self.ws.loader.forget(id);
                }
                self.ws.org.reshuffle();
                if let Scope::Random(_) = self.ws.scope {
                    self.ws.scope = Scope::Random(fastrand_seed());
                }
                self.flash_brief("Refreshed");
            }
            Err(e) => self.flash_error(format!("Could not refresh: {e}")),
        }
    }

    pub(crate) fn go_back(&mut self) {
        if let Some(s) = self.ws.back.pop() {
            self.ws.forward.push(std::mem::replace(&mut self.ws.scope, s));
            self.ws.selected.clear();
            self.ws.folders.clear();
            self.view_changed();
        }
    }

    pub(crate) fn go_forward(&mut self) {
        if let Some(s) = self.ws.forward.pop() {
            self.ws.back.push(std::mem::replace(&mut self.ws.scope, s));
            self.ws.selected.clear();
            self.ws.folders.clear();
            self.view_changed();
        }
    }

    /// Applies a click on the card at `id` with the given modifiers.
    pub(crate) fn click(&mut self, id: &str, m: Modifiers) {
        if m.command {
            if let Some(p) = self.ws.selected.iter().position(|s| s == id) {
                self.ws.selected.remove(p);
            } else {
                self.ws.selected.push(id.to_string());
            }
            self.ws.anchor = Some(id.to_string());
        } else if m.shift
            && let Some(a) = self.ws.anchor.as_ref().and_then(|a| self.ws.order.iter().position(|x| x == a))
            && let Some(b) = self.ws.order.iter().position(|x| x == id)
        {
            let (lo, hi) = (a.min(b), a.max(b));
            self.ws.selected = self.ws.order[lo..=hi].to_vec();
        } else {
            self.ws.selected = vec![id.to_string()];
            self.ws.anchor = Some(id.to_string());
        }
    }

    /// Imports files and folders on a background thread: each file is
    /// staged without the library lock, then committed under it, so the
    /// window stays responsive. Progress shows under the toolbar.
    pub(crate) fn import(&mut self, paths: Vec<PathBuf>) {
        let folders = self.scope_folders();
        self.import_into(paths, folders);
    }

    /// The folder new items join: the one being viewed, if any.
    pub(crate) fn scope_folders(&self) -> Vec<String> {
        match &self.ws.scope {
            Scope::Folder(id) => vec![id.clone()],
            _ => Vec::new(),
        }
    }

    /// Imports `paths` into the shown library. A `*.library` folder among
    /// them is opened instead, never imported (only the first, if several).
    pub(crate) fn import_into(&mut self, paths: Vec<PathBuf>, folders: Vec<String>) {
        let (libraries, paths) = logic::split_libraries(paths);
        // A dropped or opened .eaglepack is imported as a package, not as
        // a file (the first one; Eagle takes one at a time).
        let (packs, paths): (Vec<PathBuf>, Vec<PathBuf>) = paths.into_iter().partition(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("eaglepack")));
        if let Some(first) = packs.into_iter().next() {
            self.start_pack_import(first, folders.first().cloned());
        }
        self.import_files(paths, folders);
        if let Some(first) = libraries.first() {
            self.open(first.clone());
            let opened = self.position_of(first).is_some();
            let others = libraries.len() - 1;
            if opened && others > 0 {
                let name = first.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let s = if others == 1 { "y was" } else { "ies were" };
                self.flash(format!("Opened {name}; {others} other librar{s} left closed (one opens at a time)"));
            }
        }
    }

    pub(crate) fn import_files(&mut self, paths: Vec<PathBuf>, folders: Vec<String>) {
        let extra = self.collect_extra.clone();
        if paths.is_empty() {
            return;
        }
        if self.ws.lib.is_none() {
            return;
        }
        let mut files = Vec::new();
        let mut stack = paths;
        while let Some(p) = stack.pop() {
            if p.is_dir() {
                // Flattened, as Eagle's grid drop does. The walk skips
                // junk and won't follow a folder link into a loop.
                if !falcon_library::folder_import::is_pixave_library(&p) {
                    files.extend(falcon_library::folder_import::walk(&p).all_files());
                }
            } else if !is_hidden(&p) {
                files.push(p);
            }
        }
        if files.is_empty() {
            return;
        }
        files.sort();
        let mut opts = self.prefs.import_options(folders);
        if let Some(meta) = &extra {
            meta.apply(&mut opts, files.len());
        }
        self.spawn_import(files.into_iter().map(|path| import_ui::Job { path, opts: opts.clone() }).collect());
    }

    /// The import progress bar; clears itself a moment after finishing.
    pub(crate) fn progress_bar(&mut self, ui: &mut Ui, rect: Rect) -> bool {
        let snapshot = {
            let pr = self.ws.progress.lock().unwrap_or_else(|p| p.into_inner());
            (pr.total, pr.done, pr.failed, pr.current.clone())
        };
        let (total, done, failed, current) = snapshot;
        if total == 0 {
            return false;
        }
        if done >= total {
            let mut pr = self.ws.progress.lock().unwrap_or_else(|p| p.into_inner());
            let errors = std::mem::take(&mut pr.errors);
            let added = std::mem::take(&mut pr.added);
            *pr = Progress { errors, ..Progress::default() };
            drop(pr);
            self.imports.select_after = Some(added);
            if failed > 0 {
                self.flash_error(format!("Added {}, {failed} failed", done - failed));
            } else if self.prefs.tells_import_done() {
                self.flash(format!("Added {done}"));
            }
            return false;
        }
        let p = ui.painter();
        p.rect_filled(rect, 0.0, t::p().main);
        let track = Rect::from_min_max(Pos2::new(rect.left() + GRID_PAD_X, rect.bottom() - 6.0), Pos2::new(rect.right() - GRID_PAD_X, rect.bottom() - 3.0));
        p.rect_filled(track, CornerRadius::same(2), t::p().button);
        let f = done as f32 / total as f32;
        let fill = Rect::from_min_max(track.min, Pos2::new(track.left() + track.width() * f, track.bottom()));
        p.rect_filled(fill, CornerRadius::same(2), t::p().primary);
        let text = format!("Importing {done} of {total}  ·  {current}");
        let g = galley(ui, &text, font(12.0), t::p().text_muted, rect.width() - 40.0);
        p.galley(Pos2::new(rect.left() + GRID_PAD_X, rect.top() + 3.0), g, t::p().text_muted);
        true
    }

    /// ⌘C: puts the selected originals on the clipboard as files.
    pub(crate) fn copy_selection(&mut self) {
        let Some(shared) = self.ws.lib.clone() else { return };
        let paths: Vec<PathBuf> = {
            let lib = lock(&shared);
            self.ws.selected.iter().filter_map(|id| lib.item(id)).map(|i| i.original_path(lib.root())).collect()
        };
        if let Ok(mut clip) = arboard::Clipboard::new()
            && clip.set().file_list(&paths).is_ok()
        {
            self.used(&paths);
            self.flash_brief(format!("Copied {} file{}", paths.len(), if paths.len() == 1 { "" } else { "s" }));
        }
    }

    /// Records library originals that were opened or copied as Recently
    /// Used, as Eagle does for its open, Open With, new window and copy
    /// commands. Other files are ignored.
    pub(crate) fn used(&mut self, files: &[PathBuf]) {
        if let Some(root) = self.ws.root.clone() {
            self.recent_items.add_many(&root, &crate::recent::item_ids(files));
        }
    }

    /// Records one item opened in the preview window as Recently Used.
    pub(crate) fn used_item(&mut self, id: &str) {
        if let Some(root) = self.ws.root.clone() {
            self.recent_items.add(&root, id);
        }
    }

    /// ⌘V in the grid: imports copied files, or a copied image as a PNG.
    pub(crate) fn paste(&mut self) {
        let folders = self.scope_folders();
        self.paste_into(folders);
    }

    pub(crate) fn paste_into(&mut self, folders: Vec<String>) {
        let Ok(mut clip) = arboard::Clipboard::new() else { return };
        if let Ok(files) = clip.get().file_list()
            && !files.is_empty()
        {
            self.import_into(files, folders);
            return;
        }
        if let Ok(img) = clip.get_image() {
            let Some(buf) = image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.into_owned()) else { return };
            let Ok(dir) = tempfile_dir() else { return };
            let stamp = chrono::Local::now().format("%Y-%m-%d at %H.%M.%S");
            let path = dir.join(format!("Pasted image {stamp}.png"));
            // The import thread deletes the file once it has copied it in.
            if buf.save(&path).is_ok() {
                self.import_into(vec![path], folders);
            }
        }
    }

}


pub(crate) type Shared = falcon_server::Shared;

/// Most libraries kept in the recent list.
pub(crate) const RECENT_LIMIT: usize = 10;

/// Whether two paths name the same library folder (exact, or after
/// resolving symlinks and `..`).
pub(crate) fn same_path(a: &std::path::Path, b: &std::path::Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
}

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

/// A private folder for clipboard images before they are imported.
pub(crate) fn tempfile_dir() -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join("falcon-paste");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub(crate) fn fastrand_seed() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64)
}

// ---------------------------------------------------------------------------
// Sidebar


// ---------------------------------------------------------------------------
// Toolbar

impl App {
    pub(crate) fn toolbar(&mut self, ui: &mut Ui, lib: &Library) {
        let bar = ui.max_rect();
        window_drag(ui, bar, "toolbar");
        let cy = bar.center().y;
        let mut x = bar.left() + logic::bar_inset(!self.show_sidebar);
        self.panels_button(ui, Pos2::new(x, cy));
        x += 30.0;
        if icon_button(ui, Pos2::new(x, cy), ic::CARET_LEFT, 17.0, !self.ws.back.is_empty(), "").clicked() {
            self.go_back();
        }
        if icon_button(ui, Pos2::new(x + 27.0, cy), ic::CARET_RIGHT, 17.0, !self.ws.forward.is_empty(), "").clicked() {
            self.go_forward();
        }
        let combined = self.combined_title().filter(|_| self.ws.query.trim().is_empty());
        let title = if let Some(t) = combined {
            t
        } else if let Scope::Smart(id) = &self.ws.scope {
            smart_name(lib, id)
        } else if self.ws.query.trim().is_empty() {
            self.ws.scope.title(&lib.metadata.folders)
        } else {
            "Search results".into()
        };
        let g = galley(ui, &title, font(15.0), t::p().text, 220.0);
        let title_end = x + 45.0 + g.size().x;
        ui.painter().galley(Pos2::new(x + 45.0, cy - g.size().y / 2.0), g, t::p().text);

        // Right side: search field, then the icon row.
        let search = Rect::from_min_size(Pos2::new(bar.right() - 18.0 - 178.0, cy - 15.0), vec2(178.0, 30.0));
        self.search_field(ui, search);
        let mut ix = search.left() - 20.0;
        let filter = icon_button(ui, Pos2::new(ix, cy), ic::FUNNEL, 18.0, true, "Filter");
        ix -= 26.0;
        let layout = icon_button(ui, Pos2::new(ix, cy), ic::LAYOUT, 18.0, true, "Layout and sort");
        ix -= 26.0;
        icon_button(ui, Pos2::new(ix, cy), ic::LIGHTNING, 18.0, false, "Actions");
        ix -= 26.0;
        icon_button(ui, Pos2::new(ix, cy), ic::PUZZLE_PIECE, 18.0, false, "Plugins");
        let icons_left = ix - 20.0;

        let own = self.own_sort(lib);
        egui::Popup::menu(&layout).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| self.layout_panel(ui, own));
        if filter.clicked() {
            self.ws.show_filter = !self.ws.show_filter;
        }
        if self.ws.show_filter || self.ws.filter.is_active() {
            let r = Rect::from_center_size(filter.rect.center(), Vec2::splat(26.0));
            ui.painter().rect_stroke(r, t::RADIUS, Stroke::new(1.0, t::p().primary), StrokeKind::Inside);
        }

        // Zoom slider between the title and the icons.
        let zx = (bar.left() + 237.0).max(title_end + 30.0);
        if zx + 130.0 < icons_left {
            self.zoom_slider(ui, Pos2::new(zx, cy));
        }
        ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, t::p().main));
    }

    /// Shows or hides the sidebar and inspector together (Eagle's
    /// `toggleAll`).
    pub(crate) fn toggle_panels(&mut self) {
        (self.show_sidebar, self.show_inspector) = logic::toggle_all(self.show_sidebar, self.show_inspector);
    }

    /// The panels button at the left of the toolbar and the viewer bar:
    /// a click shows or hides both panels; its menu toggles each one.
    pub(crate) fn panels_button(&mut self, ui: &mut Ui, center: Pos2) {
        use crate::shortcuts::{Command as C, display};
        let key = |cmd: C| self.shortcuts.binding(cmd).map(|c| display(ui.ctx(), &c)).unwrap_or_default();
        let (all, side, insp) = (key(C::ToggleAll), key(C::ToggleSidebar), key(C::ToggleInspector));
        let tip = if all.is_empty() { "Sidebar and Inspector (Tab)".to_string() } else { format!("Sidebar and Inspector (Tab, {all})") };
        let resp = icon_button(ui, center, ic::SIDEBAR_SIMPLE, 18.0, true, &tip);
        if resp.clicked() {
            self.toggle_panels();
        }
        resp.context_menu(|ui| {
            ui.set_min_width(220.0);
            if ui.add(egui::Button::new("Sidebar and Inspector").shortcut_text("Tab")).clicked() {
                self.toggle_panels();
            }
            if ui.add(egui::Button::selectable(self.show_sidebar, "Sidebar").shortcut_text(side)).clicked() {
                self.show_sidebar = !self.show_sidebar;
            }
            if ui.add(egui::Button::selectable(self.show_inspector, "Inspector").shortcut_text(insp)).clicked() {
                self.show_inspector = !self.show_inspector;
            }
        });
    }

    pub(crate) fn search_field(&mut self, ui: &mut Ui, rect: Rect) {
        let p = ui.painter();
        p.rect_filled(rect, t::FIELD_RADIUS, t::p().search);
        p.rect_stroke(rect, t::FIELD_RADIUS, Stroke::new(1.0, t::p().field_border), StrokeKind::Inside);
        icon(ui, Pos2::new(rect.left() + 14.0, rect.center().y), ic::MAGNIFYING_GLASS, 16.0, t::p().text);
        icon(ui, Pos2::new(rect.left() + 28.0, rect.center().y + 1.0), ic::CARET_DOWN, 9.0, t::p().text_muted);
        let caret = Rect::from_min_max(Pos2::new(rect.left() + 2.0, rect.top()), Pos2::new(rect.left() + 38.0, rect.bottom()));
        let scope_resp = ui.interact(caret, ui.scope_id().with("search-scope"), Sense::click()).on_hover_text("Search Scope");
        egui::Popup::menu(&scope_resp).show(|ui| self.search_scope_menu(ui));
        icon(ui, Pos2::new(rect.right() - 17.0, rect.center().y), ic::CAMERA, 17.0, t::p().text_disabled);
        let inner = Rect::from_min_max(Pos2::new(rect.left() + 40.0, rect.top()), Pos2::new(rect.right() - 32.0, rect.bottom()));
        let resp = ui.put(
            inner,
            TextEdit::singleline(&mut self.ws.query)
                .id_salt("search")
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .hint_text(RichText::new("Search").color(t::p().text_muted))
                .font(font(14.0))
                .align(Align2::LEFT_CENTER)
                .desired_width(inner.width()),
        );
        if std::mem::take(&mut self.focus_search) {
            resp.request_focus();
        }
    }

    pub(crate) fn zoom_slider(&mut self, ui: &mut Ui, left_center: Pos2) {
        let cy = left_center.y;
        let x0 = left_center.x;
        if icon_button(ui, Pos2::new(x0, cy), ic::MINUS, 13.0, true, "").clicked() {
            self.tile = (self.tile - 20.0).max(TILE_MIN);
        }
        let track = Rect::from_min_max(Pos2::new(x0 + 16.0, cy - 8.0), Pos2::new(x0 + 104.0, cy + 8.0));
        if icon_button(ui, Pos2::new(x0 + 120.0, cy), ic::PLUS, 13.0, true, "").clicked() {
            self.tile = (self.tile + 20.0).min(TILE_MAX);
        }
        let resp = ui.interact(track, ui.scope_id().with("zoom"), Sense::click_and_drag());
        if let Some(pos) = resp.interact_pointer_pos()
            && (resp.dragged() || resp.clicked())
        {
            let f = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0);
            self.tile = TILE_MIN + f * (TILE_MAX - TILE_MIN);
        }
        let f = (self.tile - TILE_MIN) / (TILE_MAX - TILE_MIN);
        let p = ui.painter();
        let y = track.center().y;
        p.line_segment([Pos2::new(track.left(), y), Pos2::new(track.right(), y)], Stroke::new(2.0, t::p().divider));
        let kx = track.left() + f * track.width();
        p.line_segment([Pos2::new(track.left(), y), Pos2::new(kx, y)], Stroke::new(2.0, t::p().text_muted));
        p.circle_filled(Pos2::new(kx, y), 7.0, Color32::WHITE);
    }
}

// ---------------------------------------------------------------------------
// Filter bar

/// Colours offered in the colour filter, like Eagle's swatch row.
pub(crate) const PRESET_COLORS: [[u8; 3]; 14] = [
    [0xff, 0x3b, 0x30],
    [0xff, 0x95, 0x00],
    [0xff, 0xd6, 0x0a],
    [0x34, 0xc7, 0x59],
    [0x30, 0xb0, 0xc7],
    [0x00, 0x7a, 0xff],
    [0x58, 0x56, 0xd6],
    [0xaf, 0x52, 0xde],
    [0xff, 0x2d, 0x55],
    [0xa2, 0x84, 0x5e],
    [0x00, 0x00, 0x00],
    [0x8e, 0x8e, 0x93],
    [0xff, 0xff, 0xff],
    [0x67, 0x95, 0x3f],
];

pub(crate) fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// Where the list layout's columns start in a row (after the thumbnail)
/// and how wide they are together.
pub(crate) fn list_span(row: Rect) -> (f32, f32) {
    let thumb = row.height() - 10.0;
    (row.left() + 6.0 + thumb + 12.0, row.width() - thumb - 18.0)
}

/// A filter chip: muted when off, primary-tinted with its value when on.
pub(crate) fn filter_chip(ui: &mut Ui, label: &str, value: Option<String>) -> Response {
    let text = match &value {
        Some(v) => format!("{label}: {v}"),
        None => label.to_string(),
    };
    let on = value.is_some();
    let g = galley(ui, &text, font(13.0), t::p().text, 220.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 30.0, 26.0), Sense::click());
    let fill = if on {
        t::p().primary.gamma_multiply(0.22)
    } else if resp.hovered() {
        t::p().button_hover
    } else {
        t::p().button
    };
    ui.painter().rect_filled(rect, t::RADIUS, fill);
    if on {
        ui.painter().rect_stroke(rect, t::RADIUS, Stroke::new(1.0, t::p().primary.gamma_multiply(0.7)), StrokeKind::Inside);
    }
    let color = if on { t::p().text } else { t::p().text_value };
    ui.painter().galley(Pos2::new(rect.left() + 10.0, rect.center().y - g.size().y / 2.0), g, color);
    icon(ui, Pos2::new(rect.right() - 11.0, rect.center().y), ic::CARET_DOWN, 10.0, t::p().text_muted);
    resp
}

pub(crate) fn logic_picker(ui: &mut Ui, logic: &mut Logic) {
    ui.horizontal(|ui| {
        for (l, name) in [(Logic::Or, "Any"), (Logic::And, "All"), (Logic::Equal, "Exact")] {
            if ui.selectable_label(*logic == l, name).clicked() {
                *logic = l;
            }
        }
    });
}

pub(crate) fn text_filter_menu(ui: &mut Ui, f: &mut TextFilter, keywords: &mut String) {
    ui.set_min_width(200.0);
    for (opt, name) in [(TextFilter::Off, "Any"), (TextFilter::Has, "Has"), (TextFilter::No, "None")] {
        let on = std::mem::discriminant(f) == std::mem::discriminant(&opt);
        if ui.selectable_label(on, name).clicked() {
            *f = opt;
        }
    }
    ui.separator();
    ui.label(RichText::new("Contains").color(t::p().text_muted).size(12.0));
    if ui.add(TextEdit::singleline(keywords).hint_text("keyword, keyword")).changed() {
        let words: Vec<String> = keywords.split(',').map(|w| w.trim().to_string()).filter(|w| !w.is_empty()).collect();
        *f = if words.is_empty() { TextFilter::Off } else { TextFilter::Keywords(words) };
    }
}

// ---------------------------------------------------------------------------
// Grid

impl App {
    /// The scope's items in grid order, as indices into the library.
    pub(crate) fn scope_order(&mut self, lib: &Library) -> Vec<usize> {
        let only: Option<HashSet<String>> = match &self.ws.scope {
            _ if self.ws.folders.combined() => self.combined_items(lib),
            Scope::Smart(id) => Some(lib.smart_folder_items(id).into_iter().map(|i| i.id.clone()).collect()),
            Scope::Recent => {
                if self.recent_shown.as_ref().is_none_or(|(root, _)| root != lib.root()) {
                    self.recent_shown = Some((lib.root().to_path_buf(), self.recent_items.list(lib.root()).to_vec()));
                }
                self.recent_shown.as_ref().map(|(_, ids)| ids.iter().cloned().collect())
            }
            _ => None,
        };
        self.grid_order(lib, only.as_ref())
    }

    /// Keeps `ws.order` current while the viewer hides the grid, so the
    /// viewer's stepping and counter follow edits and deletions.
    pub(crate) fn refresh_order(&mut self, lib: &Library) {
        if self.scope_gate(lib).is_some() {
            self.ws.order.clear();
            return;
        }
        let order = self.scope_order(lib);
        self.ws.order = order.iter().map(|&i| lib.items()[i].id.clone()).collect();
    }

    pub(crate) fn grid(&mut self, ui: &mut Ui, lib: &Library) {
        let area = ui.max_rect();
        self.ws.cards.begin(ui.ctx().cumulative_frame_nr());
        let scroll_by = self.ws.scroll.take();
        let order = self.scope_order(lib);
        if let Some(gate) = self.scope_gate(lib) {
            self.ws.order.clear();
            self.unlock_screen(ui, gate, lib);
            return;
        }
        self.sync_layout();
        let items: Vec<&Item> = order.iter().map(|&i| &lib.items()[i]).collect();
        let new_order: Vec<String> = items.iter().map(|i| i.id.clone()).collect();
        let order_changed = self.ws.hover.shown().is_some() && self.ws.order != new_order;
        self.ws.order = new_order;

        if self.ws.scope == Scope::Duplicates {
            self.duplicates_page(ui, lib);
            return;
        }
        if self.ws.scope == Scope::AllTags && self.ws.query.trim().is_empty() {
            self.tag_page(ui, lib);
            return;
        }
        self.subfolder_strip(ui, lib);
        if items.is_empty() {
            self.empty_view(ui, lib, area);
            return;
        }
        if self.ws.layout == Layout::List {
            self.list_header(ui, lib);
        }

        let layout = self.ws.layout;
        let width = area.width() - 2.0 * GRID_PAD_X;
        let aspects: Vec<f32> = items
            .iter()
            .map(|i| if i.width > 0 && i.height > 0 { i.width as f32 / i.height as f32 } else { 1.0 })
            .collect();
        let caption = logic::caption_height(layout, &self.display);
        let (places, content_h, cols) = logic::place(layout, &aspects, width, self.tile, GRID_GAP, caption, ROW_GAP);
        self.ws.columns = cols;
        let total_h = GRID_PAD_TOP + content_h + 20.0;
        let card_of = |n: usize| {
            let [x, y, w, h] = places[n];
            Rect::from_min_size(Pos2::new(GRID_PAD_X + x, GRID_PAD_TOP + y), vec2(w, h + caption))
        };

        let mut scroll = egui::ScrollArea::vertical().id_salt(("grid", format!("{:?}", self.ws.scope))).auto_shrink([false, false]);
        if let Some(id) = self.ws.scroll_to.take()
            && let Some(pos) = self.ws.order.iter().position(|x| *x == id)
        {
            let r = card_of(pos);
            let cur = ui.ctx().data(|d| d.get_temp::<f32>(ui.scope_id().with("grid-offset"))).unwrap_or(0.0);
            let view_h = area.height();
            if r.top() < cur {
                scroll = scroll.vertical_scroll_offset(r.top() - GRID_PAD_TOP);
            } else if r.bottom() > cur + view_h {
                scroll = scroll.vertical_scroll_offset(r.bottom() + ROW_GAP - view_h);
            }
        }
        if let Some(how) = scroll_by {
            let cur = ui.ctx().data(|d| d.get_temp::<f32>(ui.scope_id().with("grid-offset"))).unwrap_or(0.0);
            scroll = scroll.vertical_scroll_offset(how.offset(cur, area.height(), total_h));
        }
        // The rubber band ends with the button; near an edge it scrolls.
        let pointer = ui.ctx().pointer_latest_pos();
        if self.ws.marquee.is_some() && !ui.input(|i| i.pointer.primary_down()) {
            self.ws.marquee = None;
        }
        if self.ws.marquee.is_some()
            && let Some(p) = pointer
        {
            let step = logic::edge_scroll(p.y, area.top(), area.bottom());
            if step != 0.0 {
                let cur = ui.ctx().data(|d| d.get_temp::<f32>(ui.scope_id().with("grid-offset"))).unwrap_or(0.0);
                scroll = scroll.vertical_scroll_offset((cur + step).clamp(0.0, (total_h - area.height()).max(0.0)));
                ui.ctx().request_repaint();
            }
        }

        let mut jobs = Vec::new();
        let mut clicked: Option<(String, Modifiers)> = None;
        let mut opened: Option<String> = None;
        let mut double_clicked: Option<String> = None;
        let mut bg_clicked = false;
        let mut hovered: Option<(&Item, Rect, bool)> = None;
        let manual = self.manual_folder(lib);
        let mut band_rect: Option<Rect> = None;
        let out = scroll.show_viewport(ui, |ui, viewport| {
            let origin = ui.max_rect().min;
            let bg = ui.allocate_rect(Rect::from_min_size(origin, vec2(area.width(), total_h.max(area.height()))), Sense::click_and_drag());
            if bg.clicked() {
                bg_clicked = true;
            }
            if bg.drag_started_by(egui::PointerButton::Primary)
                && let Some(at) = ui.input(|i| i.pointer.press_origin())
            {
                let m = ui.input(|i| i.modifiers);
                let mode = logic::marquee_mode(m.command, m.shift);
                let base = if mode == logic::MarqueeMode::Replace { Vec::new() } else { self.ws.selected.clone() };
                self.ws.marquee = Some(Marquee { start: at - origin.to_vec2(), base, mode });
            }
            if let Some(mq) = &self.ws.marquee
                && let Some(p) = pointer
            {
                let now = p - origin.to_vec2();
                let band = logic::rect_between([mq.start.x, mq.start.y], [now.x, now.y]);
                let cards: Vec<logic::Place> = (0..items.len()).map(card_of).map(|r| [r.left(), r.top(), r.width(), r.height()]).collect();
                let hits = logic::rect_hits(&cards, band);
                let sel = logic::marquee_selection(&self.ws.order, &mq.base, &hits, mq.mode);
                if let Some(last) = hits.last().and_then(|&n| self.ws.order.get(n)) {
                    self.ws.anchor = Some(last.clone());
                }
                self.ws.selected = sel;
                let [x, y, w, h] = band;
                band_rect = Some(Rect::from_min_size(origin + vec2(x, y), vec2(w, h)));
            }
            let view = viewport.expand(200.0);
            for (n, item) in items.iter().enumerate() {
                let local = card_of(n);
                if local.bottom() < view.top() || local.top() > view.bottom() {
                    continue;
                }
                let card = local.translate(origin.to_vec2());
                let resp = ui.interact(card, ui.scope_id().with(("card", &item.id)), Sense::click_and_drag());
                if resp.drag_started() {
                    let ids = if self.ws.selected.contains(&item.id) { self.ws.selected.clone() } else { vec![item.id.clone()] };
                    resp.dnd_set_drag_payload(DragItems(ids));
                }
                if resp.secondary_clicked() && !self.ws.selected.contains(&item.id) {
                    self.ws.selected = vec![item.id.clone()];
                }
                let targets: Vec<String> = if self.ws.selected.contains(&item.id) { self.ws.selected.clone() } else { vec![item.id.clone()] };
                resp.context_menu(|ui| {
                    if self.item_menu(ui, lib, item, &targets) {
                        opened = Some(item.id.clone());
                    }
                });
                if resp.clicked() {
                    clicked = Some((item.id.clone(), ui.input(|i| i.modifiers)));
                }
                if resp.double_clicked() {
                    double_clicked = Some(item.id.clone());
                }
                let selected = self.ws.selected.contains(&item.id);
                self.card_anim(ui, lib, item, card, resp.hovered());
                match layout {
                    Layout::List => self.list_row(ui, lib, item, card, selected, resp.hovered(), n),
                    _ => self.card(ui, lib, item, card, caption, selected, resp.hovered()),
                }
                if resp.hovered() {
                    hovered = Some((item, card, self.zoom_button(ui, item, card, caption)));
                }
                if let Some(j) = job_for(item, lib.root(), Tier::Grid) {
                    jobs.push(j);
                }
            }
            if let Some(folder) = &manual {
                self.manual_drag(ui, folder, &places, caption, origin);
            }
            if let Some(r) = band_rect {
                ui.painter().rect_filled(r, CornerRadius::same(2), t::p().primary.gamma_multiply(0.15));
                ui.painter().rect_stroke(r, CornerRadius::same(2), Stroke::new(1.0, t::p().primary), StrokeKind::Inside);
            }
        });
        let offset = out.state.clamped_offset().y;
        let scrolled = ui.ctx().data(|d| d.get_temp::<f32>(ui.scope_id().with("grid-offset"))).is_some_and(|o| (o - offset).abs() > 0.5);
        ui.ctx().data_mut(|d| d.insert_temp(ui.scope_id().with("grid-offset"), offset));
        self.hover_preview(ui, lib, hovered, scrolled, order_changed || clicked.is_some() || opened.is_some());
        self.ws.frame_jobs.extend(jobs);
        if let Some((id, m)) = clicked {
            self.click(&id, m);
        } else if bg_clicked {
            self.ws.selected.clear();
        }
        if let Some(id) = &double_clicked
            && self.prefs.double_click == crate::prefs::DoubleClick::External
        {
            let files = open_ui::originals(lib, std::slice::from_ref(id));
            crate::open_with::open_default(&files);
            self.used(&files);
        } else if let Some(id) = opened.or(double_clicked) {
            self.ws.viewer = Some(id);
        }
    }

    /// The item context menu, for a grid card or the item in the viewer.
    /// `targets` are the items it acts on. Returns true when Open was
    /// chosen (the grid's menu only; the viewer's has no Open).
    /// The item context menu, with Eagle's search box on top.
    pub(crate) fn item_menu(&mut self, ui: &mut Ui, lib: &Library, item: &Item, targets: &[String]) -> bool {
        crate::menu_search::show(ui, |ui| self.item_menu_body(ui, lib, item, targets))
    }

    pub(crate) fn item_menu_body(&mut self, ui: &mut Ui, lib: &Library, item: &Item, targets: &[String]) -> bool {
        let in_viewer = self.ws.viewer.as_deref() == Some(item.id.as_str());
        let mut opened = false;
        ui.set_min_width(200.0);
        if !in_viewer && ui.button("Open").clicked() {
            opened = true;
        }
        self.open_menu(ui, &open_ui::originals(lib, targets));
        if targets.len() == 1 && self.ws.scope != Scope::Trash {
            self.location_menu(ui, lib, item);
        }
        self.item_copy_menu(ui, targets);
        ui.menu_button("Add to Folder", |ui| {
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for f in lib.all_folders() {
                    if ui.button(&f.name).clicked() {
                        self.push_bulk(targets.len(), Action::AddToFolder(targets.to_vec(), f.id.clone()));
                    }
                }
            });
        });
        let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::AddToLastFolder);
        let last = commands::items::last_used(&self.recent_folders, |id| logic::find_folder(&lib.metadata.folders, id).is_some());
        let tip = last.as_deref().and_then(|id| logic::find_folder(&lib.metadata.folders, id)).map(|f| f.name.clone());
        let b = ui.add_enabled(last.is_some(), egui::Button::new("Add to Last Used Folder").shortcut_text(keys));
        if b.clicked() {
            self.add_to_last_used(targets.to_vec());
        }
        if let Some(name) = tip {
            b.on_hover_text(name);
        }
        // Eagle's Copy Tags and Paste Tags.
        let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::CopyTags);
        if ui.add(egui::Button::new("Copy Tags").shortcut_text(keys)).clicked() {
            let items: Vec<&Item> = targets.iter().filter_map(|id| lib.item(id)).collect();
            let tags = crate::tags::common_tags(&items);
            let ctx = ui.ctx().clone();
            self.copy_tags(&ctx, tags);
        }
        let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::PasteTags);
        if ui.add_enabled(self.copied_tags.is_some(), egui::Button::new("Paste Tags").shortcut_text(keys)).clicked() {
            self.paste_tags(targets.to_vec());
        }
        if self.ws.scope != Scope::Trash {
            let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::Rename);
            let label = if targets.len() > 1 { format!("Rename {} Items…", targets.len()) } else { "Rename".into() };
            if ui.add(egui::Button::new(label).shortcut_text(keys)).clicked() {
                if targets.len() == 1 {
                    self.ws.selected = targets.to_vec();
                }
                self.rename_items(targets.to_vec());
            }
        }
        if targets.len() == 1 {
            let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::Duplicate);
            if ui.add(egui::Button::new("Duplicate").shortcut_text(keys)).clicked() {
                self.ws.actions.push(Action::Edit(image_edit::Op::Duplicate(item.id.clone())));
            }
        }
        // Eagle offers it for several items.
        if targets.len() > 1 && self.ws.scope != Scope::Trash && ui.button(format!("New Folder with {} Items…", targets.len())).clicked() {
            self.ws.folders.new_with = Some((targets.to_vec(), String::new()));
        }
        self.edit_menu(ui, item, targets, lib);
        if !in_viewer {
            let cur = logic::ThumbBg::of(item);
            ui.menu_button("Thumbnail Background", |ui| {
                if ui.add(egui::Button::selectable(cur.is_none(), "None")).clicked() {
                    self.ws.actions.push(Action::Background(targets.to_vec(), None));
                }
                ui.separator();
                for bg in logic::ThumbBg::ALL {
                    if ui.add(egui::Button::selectable(cur == Some(bg), bg.label())).clicked() {
                        self.ws.actions.push(Action::Background(targets.to_vec(), Some(bg)));
                    }
                }
            });
        }
        self.item_export_menu(ui, targets);
        // Eagle's Open Item in New Window.
        let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::OpenNewWindow);
        if ui.add(egui::Button::new("Open in New Window").shortcut_text(keys)).clicked() {
            self.preview = Some(preview_window::PreviewWin::new(item.id.clone()));
            self.used_item(&item.id);
        }
        // Eagle's Navigator toggle, in the viewer.
        if in_viewer {
            let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::Navigator);
            if ui.add(egui::Button::new("Navigator").selected(!self.modes.hide_navigator).shortcut_text(keys)).clicked() {
                self.modes.hide_navigator = !self.modes.hide_navigator;
            }
        }
        // Eagle's grayscale preview toggle; the menu stays open, as Eagle's
        // entry does.
        let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::Grayscale);
        if ui.add(egui::Button::new("Grayscale").selected(self.modes.grayscale).shortcut_text(keys)).clicked() {
            self.modes.grayscale = !self.modes.grayscale;
        }
        // Eagle's Enter / Leave Presentation Mode.
        let label = if self.modes.presenting { "Leave Presentation Mode" } else { "Enter Presentation Mode" };
        let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::Presentation);
        if ui.add(egui::Button::new(label).shortcut_text(keys)).clicked() {
            if !in_viewer && !self.modes.presenting {
                self.ws.viewer = Some(item.id.clone());
            }
            self.toggle_presentation(ui.ctx());
        }
        if crate::share::AVAILABLE && self.ws.scope != Scope::Trash && ui.button("Share…").clicked() {
            let files = open_ui::originals(lib, targets);
            let at = ui.ctx().input(|i| i.pointer.interact_pos()).unwrap_or_default();
            crate::share::share(&files, (at.x, at.y));
            ui.close();
        }
        ui.separator();
        if self.ws.scope == Scope::Trash {
            if ui.button("Restore").clicked() {
                self.leave_viewed(targets);
                self.ws.actions.push(Action::Trash(targets.to_vec(), false));
            }
            if ui.button(RichText::new("Delete Permanently").color(t::p().icon_red)).clicked() {
                self.ws.confirm = Some(Confirm::DeleteItems(targets.to_vec()));
            }
        } else {
            if let Scope::Folder(fid) = self.ws.scope.clone() {
                let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::RemoveFromFolder);
                if ui.add(egui::Button::new("Remove from Folder").shortcut_text(keys)).clicked() {
                    self.leave_viewed(targets);
                    self.select_after_removal(targets);
                    self.ws.actions.push(Action::RemoveFromFolder(targets.to_vec(), fid));
                }
            }
            if ui.button("Move to Trash").clicked() {
                self.leave_viewed(targets);
                self.push_bulk(targets.len(), Action::TrashAsk(targets.to_vec()));
            }
        }
        opened
    }

    /// After `ids` leave the grid (trashed, restored, deleted or moved out
    /// of the open folder): when they were the whole selection, selects the
    /// item after them (or before, at the end), as Eagle does; otherwise
    /// just drops them from the selection.
    pub(crate) fn select_after_removal(&mut self, ids: &[String]) {
        if self.ws.selected.is_empty() || self.ws.selected.iter().any(|s| !ids.contains(s)) {
            self.ws.selected.retain(|s| !ids.contains(s));
            return;
        }
        let next = logic::next_after_removal(&self.ws.order, ids);
        self.ws.selected = next.iter().cloned().collect();
        self.ws.anchor.clone_from(&next);
        self.ws.scroll_to = next;
    }

    /// Before `ids` leave the grid (trashed or restored), moves the viewer
    /// off them to the next item, or the previous one at the end, or
    /// closes it when none is left.
    pub(crate) fn leave_viewed(&mut self, ids: &[String]) {
        let Some(cur) = self.ws.viewer.clone() else { return };
        if !ids.contains(&cur) {
            return;
        }
        let at = self.ws.order.iter().position(|x| *x == cur).unwrap_or(0);
        let after = self.ws.order.iter().skip(at + 1).find(|x| !ids.contains(x));
        let before = self.ws.order.iter().take(at).rev().find(|x| !ids.contains(x));
        self.ws.viewer = after.or(before).cloned();
    }

    /// Draws an item's image into `rect`: fitted without enlarging (grid),
    /// or filling it exactly (waterfall and justified boxes already have the
    /// image's shape).
    pub(crate) fn draw_image(&mut self, ui: &Ui, lib: &Library, item: &Item, rect: Rect, fill: bool) {
        let failed = self.ws.loader.has_failed(&item.id, Tier::Grid);
        let playing = self.ws.cards.texture(&item.id).cloned();
        match playing.or_else(|| self.ws.loader.get(&item.id, Tier::Grid).cloned()) {
            Some(tex) => {
                let natural = if item.width > 0 && item.height > 0 { vec2(item.width as f32, item.height as f32) } else { tex.size_vec2() };
                let scale = (rect.width() / natural.x).min(rect.height() / natural.y);
                // Eagle's object-fit: scale-down never enlarges a small image.
                let scale = if fill { scale } else { scale.min(1.0) };
                let r = Rect::from_center_size(rect.center(), natural * scale);
                egui::Image::new((tex.id(), r.size())).corner_radius(4).paint_at(ui, r);
            }
            None if failed || job_for(item, lib.root(), Tier::Grid).is_none() => ext_placeholder(ui, rect, &item.ext),
            None => {}
        }
    }

    /// Draws an item's image so it covers `rect`, cropping what overflows
    /// (the grid layout's Fill mode).
    pub(crate) fn draw_image_cover(&mut self, ui: &Ui, lib: &Library, item: &Item, rect: Rect) {
        let failed = self.ws.loader.has_failed(&item.id, Tier::Grid);
        let playing = self.ws.cards.texture(&item.id).cloned();
        match playing.or_else(|| self.ws.loader.get(&item.id, Tier::Grid).cloned()) {
            Some(tex) => {
                let size = tex.size_vec2();
                let s = (rect.width() / size.x).max(rect.height() / size.y);
                let uv = Rect::from_center_size(Pos2::new(0.5, 0.5), vec2(rect.width() / (size.x * s), rect.height() / (size.y * s)));
                egui::Image::new((tex.id(), rect.size())).uv(uv).corner_radius(4).paint_at(ui, rect);
            }
            None if failed || job_for(item, lib.root(), Tier::Grid).is_none() => ext_placeholder(ui, rect, &item.ext),
            None => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn card(&mut self, ui: &Ui, lib: &Library, item: &Item, card: Rect, caption: f32, selected: bool, hovered: bool) {
        let tile = Rect::from_min_max(card.min, Pos2::new(card.right(), card.bottom() - caption));
        let grid = self.ws.layout == Layout::Grid;
        let d = self.display.clone();
        let p = ui.painter().clone();
        if grid {
            p.rect_filled(tile, t::RADIUS, t::p().main);
            let border = if hovered { t::p().card_hover } else { t::p().card_border };
            p.rect_stroke(tile, t::RADIUS, Stroke::new(1.0, border), StrokeKind::Inside);
            if let Some(bg) = logic::ThumbBg::of(item) {
                paint_thumb_bg(&p, tile.shrink(1.0), bg);
            }
            if d.fill {
                self.draw_image_cover(ui, lib, item, tile.shrink(1.0));
            } else {
                self.draw_image(ui, lib, item, tile.shrink(6.0), false);
            }
        } else {
            p.rect_filled(tile, CornerRadius::same(4), t::p().field);
            if let Some(bg) = logic::ThumbBg::of(item) {
                paint_thumb_bg(&p, tile, bg);
            }
            self.draw_image(ui, lib, item, tile, true);
            if hovered && !selected {
                p.rect_stroke(tile, CornerRadius::same(4), Stroke::new(1.0, t::p().card_hover), StrokeKind::Inside);
            }
        }
        if d.ext_badge {
            badge(ui, tile.min + vec2(5.0, 5.0), &item.ext);
        }
        if d.comment_badge {
            comments::count_badge(ui, tile, item);
        }
        if selected {
            p.rect_stroke(tile.expand(1.0), CornerRadius::same(7), Stroke::new(2.5, t::p().primary), StrokeKind::Inside);
        }
        if caption == 0.0 {
            return;
        }
        let mut y = tile.bottom() + 14.0;
        if d.show_name {
            let name = logic::caption_name(item, d.show_ext);
            let g = galley(ui, &name, font(13.0), t::p().text, tile.width());
            if selected {
                let pill = Rect::from_center_size(Pos2::new(tile.center().x, y), vec2(g.size().x + 10.0, 19.0));
                p.rect_filled(pill, CornerRadius::same(4), t::p().primary);
            }
            p.galley(Pos2::new(tile.center().x - g.size().x / 2.0, y - g.size().y / 2.0), g, t::p().text);
            y += 18.0;
        }
        if d.show_info {
            match logic::caption_info(item, d.info) {
                logic::Info::Text(text) => {
                    let g = galley(ui, &text, font(12.0), t::p().text_muted, tile.width());
                    p.galley(Pos2::new(tile.center().x - g.size().x / 2.0, y - g.size().y / 2.0), g, t::p().text_muted);
                }
                logic::Info::Stars(n) => {
                    for i in 0..5u8 {
                        let color = if i < n { t::p().icon_yellow } else { t::p().text_disabled };
                        let x = tile.center().x + (f32::from(i) - 2.0) * 12.0;
                        p.text(Pos2::new(x, y), Align2::CENTER_CENTER, "★", font(11.0), color);
                    }
                }
            }
        }
    }

    /// One row of the list layout: thumbnail, then the shown columns
    /// (`logic::ListColumns`), lined up with the header.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn list_row(&mut self, ui: &Ui, lib: &Library, item: &Item, row: Rect, selected: bool, hovered: bool, n: usize) {
        let p = ui.painter().clone();
        let bg = row.shrink2(vec2(0.0, 2.0));
        if selected {
            p.rect_filled(bg, t::RADIUS, t::p().primary.gamma_multiply(0.35));
        } else if hovered {
            p.rect_filled(bg, t::RADIUS, t::p().row_hover);
        } else if n % 2 == 1 {
            p.rect_filled(bg, t::RADIUS, t::p().row_hover.gamma_multiply(0.3));
        }
        let thumb = Rect::from_min_size(Pos2::new(row.left() + 6.0, row.top() + 5.0), Vec2::splat(row.height() - 10.0));
        p.rect_filled(thumb, CornerRadius::same(4), t::p().field);
        if let Some(bg) = logic::ThumbBg::of(item) {
            paint_thumb_bg(&p, thumb, bg);
        }
        self.draw_image(ui, lib, item, thumb.shrink(2.0), false);
        let cy = row.center().y;
        let (x0, span) = list_span(row);
        for (col, at, w) in self.display.columns.spans() {
            let (x, cw) = (x0 + span * at, span * w - 8.0);
            let (size, color) = if col == logic::ListColumn::Name { (13.0, t::p().text) } else { (12.0, t::p().text_muted) };
            match logic::list_cell(item, col, self.display.show_ext) {
                logic::Info::Text(text) => {
                    let g = galley(ui, &text, font(size), color, cw);
                    p.galley(Pos2::new(x, cy - g.size().y / 2.0), g, color);
                }
                logic::Info::Stars(lit) => {
                    for i in 0..5u8 {
                        let c = if i < lit { t::p().icon_yellow } else { t::p().text_disabled };
                        p.text(Pos2::new(x + f32::from(i) * 12.0, cy), Align2::LEFT_CENTER, "★", font(11.0), c);
                    }
                }
            }
        }
    }

    /// Draws a hovered card's zoom button, as Eagle does, and says whether
    /// the pointer is on it.
    pub(crate) fn zoom_button(&self, ui: &Ui, item: &Item, card: Rect, caption: f32) -> bool {
        if !self.prefs.hover_zoom || !crate::hover::has_zoom_button(&item.ext) {
            return false;
        }
        let list = self.ws.layout == Layout::List;
        let thumb = if list {
            Rect::from_min_size(Pos2::new(card.left() + 6.0, card.top() + 5.0), Vec2::splat(card.height() - 10.0))
        } else {
            Rect::from_min_max(card.min, Pos2::new(card.right(), card.bottom() - caption))
        };
        let rect = crate::hover::zoom_button_rect(thumb, self.tile, list);
        let hot = ui.rect_contains_pointer(rect);
        crate::hover::paint_button(ui.painter(), rect, list, hot, font(rect.width().min(24.0) * 0.6), ic::MAGNIFYING_GLASS);
        hot
    }

    /// Eagle's hover preview: an enlarged image over the window after the
    /// pointer rests on a card's zoom button, or at once while Z is held.
    pub(crate) fn hover_preview(&mut self, ui: &Ui, lib: &Library, hovered: Option<(&Item, Rect, bool)>, scrolled: bool, cancel: bool) {
        use crate::hover;
        let ctx = ui.ctx().clone();
        let (now, key, down) = ctx.input(|i| {
            let m = i.modifiers;
            (i.time, i.key_down(Key::Z) && !m.command && !m.ctrl && !m.shift, i.pointer.any_down())
        });
        let cancel = cancel
            || down
            || self.ws.viewer.is_some()
            || egui::DragAndDrop::has_any_payload(&ctx)
            || self.native_drag.load(std::sync::atomic::Ordering::Relaxed);
        let input = hover::Input {
            card: hovered.map(|h| h.0.id.as_str()),
            on_button: hovered.is_some_and(|h| h.2),
            key: key && ctx.memory(|m| m.focused().is_none()),
            scrolled,
            cancel,
            now,
        };
        if let Some(wait) = self.ws.hover.update(input) {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
        }
        let fade = ctx.animate_bool_with_time(egui::Id::unique("hover-preview-fade"), self.ws.hover.shown().is_some(), 0.15);
        let Some((item, card, _)) = hovered.filter(|h| self.ws.hover.shown() == Some(h.0.id.as_str())) else {
            self.ws.cards.close_popup();
            return;
        };
        let grid = self.ws.loader.get(&item.id, Tier::Grid).cloned();
        let natural = match (item.width, item.height, &grid) {
            (w, h, _) if w > 0 && h > 0 => vec2(w as f32, h as f32),
            (_, _, Some(t)) => t.size_vec2(),
            _ => return,
        };
        let window = ctx.content_rect();
        let rect = hover::preview_rect(card, natural, window, f32::INFINITY);
        // GIFs and animated WebPs play in the preview.
        if let Some(tex) = self.hover_anim(&ctx, lib, item, rect) {
            hover::paint(&ctx, window, rect, tex.id(), t::p().panel, fade);
            return;
        }
        let original = hover::wants_original(&item.ext, item.width, item.height, rect.size() * ctx.pixels_per_point(), hover::GRID_SIDE);
        let full = if original {
            if let Some(j) = job_for(item, lib.root(), Tier::Full) {
                self.ws.frame_jobs.insert(0, j);
            }
            self.ws.loader.get(&item.id, Tier::Full).cloned()
        } else {
            None
        };
        if let Some(tex) = full.or(grid) {
            hover::paint(&ctx, window, rect, tex.id(), t::p().panel, fade);
        }
    }

    pub(crate) fn tag_page(&mut self, ui: &mut Ui, lib: &Library) {
        let filter = self.ws.tag_filter.trim().to_lowercase();
        let mut tags: Vec<(String, usize)> =
            logic::tag_counts(lib.items()).into_iter().filter(|(t, _)| filter.is_empty() || t.to_lowercase().contains(&filter)).collect();
        let sort = self.ws.org.tag_sort;
        sort.apply(&mut tags);
        // The order the chips were drawn in last frame is what ⇧-click spans.
        self.ws.org.tag_order_shown = std::mem::take(&mut self.ws.org.tag_order);
        self.ws.org.tag_sel.retain(|t| tags.iter().any(|(x, _)| x == t) || lib.tags.starred_tags.contains(t));
        let starred: Vec<(String, usize)> =
            lib.tags.starred_tags.iter().filter_map(|s| tags.iter().find(|(t, _)| t == s).cloned()).collect();
        egui::ScrollArea::vertical().id_salt("tags-page").auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.add_space(GRID_PAD_X);
                let (rect, _) = ui.allocate_exact_size(vec2(240.0, 30.0), Sense::hover());
                ui.painter().rect_filled(rect, t::FIELD_RADIUS, t::p().search);
                ui.painter().rect_stroke(rect, t::FIELD_RADIUS, Stroke::new(1.0, t::p().field_border), StrokeKind::Inside);
                icon(ui, Pos2::new(rect.left() + 14.0, rect.center().y), ic::MAGNIFYING_GLASS, 15.0, t::p().text_muted);
                let inner = Rect::from_min_max(Pos2::new(rect.left() + 30.0, rect.top()), rect.max - vec2(8.0, 0.0));
                ui.put(
                    inner,
                    TextEdit::singleline(&mut self.ws.tag_filter)
                        .id_salt("tag-filter")
                        .frame(egui::Frame::NONE)
                        .margin(egui::Margin::ZERO)
                        .hint_text(RichText::new("Filter tags").color(t::p().text_muted))
                        .font(font(13.0))
                        .align(Align2::LEFT_CENTER),
                );
                ui.add_space(12.0);
                ui.label(RichText::new(format!("{} tags", tags.len())).color(t::p().text_muted));
                self.tag_page_controls(ui);
            });
            ui.add_space(14.0);
            if tags.is_empty() && starred.is_empty() {
                let top = ui.cursor().top();
                let rest = Rect::from_min_max(Pos2::new(ui.max_rect().left(), top), ui.max_rect().right_bottom());
                self.empty_tags(ui, rest, !filter.is_empty());
                return;
            }
            if self.tags_by_group() {
                self.tag_groups_page(ui, lib, &filter);
                return;
            }

            let mut groups: Vec<(String, Vec<(String, usize)>)> = Vec::new();
            if !starred.is_empty() {
                groups.push((format!("{}  Starred", ic::STAR), starred.clone()));
            }
            let letter = |t: &str| {
                let first = t.chars().next().map_or('#', |c| c.to_uppercase().next().unwrap_or(c));
                if first.is_alphabetic() { first } else { '#' }
            };
            // A-Z sections (Z-A for Z > A, # last either way), the sort inside each.
            let down = sort == crate::tags::TagSort::NameDown;
            tags.sort_by(|a, b| {
                let (x, y) = (letter(&a.0), letter(&b.0));
                (x == '#').cmp(&(y == '#')).then_with(|| if down { y.cmp(&x) } else { x.cmp(&y) })
            });
            let mut current: Option<char> = None;
            for (tag, n) in tags {
                let first = letter(&tag);
                if current != Some(first) {
                    groups.push((first.to_string(), Vec::new()));
                    current = Some(first);
                }
                groups.last_mut().unwrap().1.push((tag, n));
            }
            for (heading, group) in groups {
                ui.horizontal(|ui| {
                    ui.add_space(GRID_PAD_X + 2.0);
                    ui.label(RichText::new(heading).size(14.0).color(t::p().text_muted));
                });
                ui.add_space(2.0);
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(GRID_PAD_X);
                    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                    for (tag, n) in group {
                        self.tag_chip(ui, lib, &tag, n);
                    }
                });
                ui.add_space(16.0);
            }
        });
    }

    pub(crate) fn tag_chip(&mut self, ui: &mut Ui, lib: &Library, tag: &str, n: usize) {
        let renaming = self.ws.tag_rename.as_ref().is_some_and(|(t, _)| t == tag);
        if renaming {
            let (_, buf) = self.ws.tag_rename.as_mut().unwrap();
            let te = ui.add(TextEdit::singleline(buf).id_salt(("tag-rename", tag)).desired_width(140.0).font(font(13.0)));
            if !te.has_focus() && !te.lost_focus() {
                te.request_focus();
            }
            let cancel = ui.input(|i| i.key_pressed(Key::Escape));
            if te.lost_focus() || cancel {
                let (from, to) = self.ws.tag_rename.take().unwrap();
                if !cancel && !to.trim().is_empty() && to.trim() != from {
                    self.ws.actions.push(Action::RenameTag(from, to.trim().to_string()));
                }
            }
            return;
        }
        let name_g = galley(ui, tag, font(13.0), t::p().text, 300.0);
        let count_g = galley(ui, &n.to_string(), font(12.0), t::p().text_muted, 80.0);
        // Tags filed in a group carry the group's colour dot.
        let dot = App::tag_dot(lib, tag);
        let dot_w = if dot.is_some() { 14.0 } else { 0.0 };
        let size = vec2(name_g.size().x + count_g.size().x + 30.0 + dot_w, 28.0);
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
        App::tag_chip_drag(&resp, tag);
        self.ws.org.tag_order.push(tag.to_string());
        let picked = self.ws.org.tag_sel.iter().any(|t| t == tag);
        let fill = if picked {
            t::p().primary.gamma_multiply(0.45)
        } else if resp.hovered() {
            t::p().button_hover
        } else {
            t::p().button
        };
        ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
        if picked {
            ui.painter().rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, t::p().primary), StrokeKind::Inside);
        }
        let cy = rect.center().y;
        if let Some(c) = dot {
            ui.painter().circle_filled(Pos2::new(rect.left() + 14.0, cy), 4.0, c);
        }
        ui.painter().galley(Pos2::new(rect.left() + 11.0 + dot_w, cy - name_g.size().y / 2.0), name_g.clone(), t::p().text);
        ui.painter().galley(Pos2::new(rect.right() - 11.0 - count_g.size().x, cy - count_g.size().y / 2.0), count_g, t::p().text_muted);
        // Eagle's tag manager: a click picks (⌘ and ⇧ pick more), a
        // double-click shows the tag's items.
        if resp.double_clicked() {
            self.go(Scope::Tag(tag.to_string()));
        } else if resp.clicked() {
            let m = ui.input(|i| i.modifiers);
            let o = &mut self.ws.org;
            let (sel, anchor) = crate::tags::select_click(&o.tag_sel, o.tag_anchor.as_deref(), &o.tag_order_shown, tag, m.shift, m.command);
            o.tag_sel = sel;
            o.tag_anchor = anchor;
        }
        if resp.secondary_clicked() && !picked {
            self.ws.org.tag_sel = vec![tag.to_string()];
            self.ws.org.tag_anchor = Some(tag.to_string());
        }
        resp.context_menu(|ui| crate::menu_search::show(ui, |ui| {
            let tags = if self.ws.org.tag_sel.is_empty() { vec![tag.to_string()] } else { self.ws.org.tag_sel.clone() };
            self.tags_menu(ui, lib, &tags);
        }));
        resp.on_hover_text("Click to pick, ⌘ or ⇧ to pick more · double-click to show items · right-click for more. Renaming to an existing tag merges them.");
    }

    /// The menu of picked tags on the All Tags page, after Eagle's.
    pub(crate) fn tags_menu(&mut self, ui: &mut Ui, lib: &Library, tags: &[String]) {
        ui.set_min_width(200.0);
        let n = tags.len();
        if ui.button(if n == 1 { "Show Items".to_string() } else { format!("Show Items with Any of {n} Tags") }).clicked() {
            match tags {
                [one] => self.go(Scope::Tag(one.clone())),
                _ => self.filter_any_tags(tags.to_vec()),
            }
        }
        ui.separator();
        let all_starred = tags.iter().all(|t| lib.tags.starred_tags.contains(t));
        if ui.button(if all_starred { "Unstar" } else { "Star" }).clicked() {
            self.ws.actions.push(Action::StarTags(tags.to_vec(), !all_starred));
        }
        if let [one] = tags
            && ui.button("Rename…").clicked()
        {
            self.ws.tag_rename = Some((one.clone(), one.clone()));
        }
        if n > 1 {
            let merge = ui.add_enabled(crate::tags::can_merge(n), egui::Button::new(format!("Merge {n} Tags…")));
            if merge.on_disabled_hover_text("Eagle merges two to five tags at a time").clicked() {
                self.ws.org.merge = Some((tags.to_vec(), tags[0].clone()));
            }
        }
        ui.separator();
        self.tag_group_menu(ui, lib, tags);
        ui.separator();
        let label = if n == 1 { "Delete Tag".to_string() } else { format!("Delete {n} Tags") };
        if ui.button(RichText::new(label).color(t::p().icon_red)).clicked() {
            self.ws.confirm = Some(Confirm::DeleteTags(tags.to_vec()));
        }
    }

    /// Eagle's Filter with Tags: All, with the tag filter set to any of
    /// `tags`.
    pub(crate) fn filter_any_tags(&mut self, tags: Vec<String>) {
        self.go(Scope::All);
        self.ws.filter.tags.includes = tags;
        self.ws.filter.tags.excludes.clear();
        self.ws.filter.tags.empty = false;
        self.ws.filter.tags.logic = Logic::Or;
        self.ws.show_filter = true;
    }
}

// ---------------------------------------------------------------------------
// Inspector

impl App {
    pub(crate) fn inspector(&mut self, ui: &mut Ui, lib: &mut Library) {
        let full = ui.max_rect();
        let p = ui.painter().clone();
        p.rect_filled(full, 0.0, t::p().panel);
        p.vline(full.left() + 0.5, full.y_range(), Stroke::new(1.0, t::p().divider));
        let header = Rect::from_min_size(full.min, vec2(full.width(), t::BAR_HEIGHT));
        window_drag(ui, header, "inspector");
        if icon_button(ui, Pos2::new(full.right() - 24.0, header.center().y), ic::PUSH_PIN, 17.0, true, "Hide inspector")
            .clicked()
        {
            self.show_inspector = false;
        }
        let body = Rect::from_min_max(Pos2::new(full.left() + 16.0, header.bottom() + 8.0), Pos2::new(full.right() - 16.0, full.bottom()));
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(body));
        egui::ScrollArea::vertical().id_salt("inspector").auto_shrink([false, false]).show(&mut ui, |ui| {
            ui.spacing_mut().item_spacing.y = 9.0;
            if !self.ws.selected.is_empty() {
                self.flush_folder_edit(lib, None);
            }
            match self.ws.selected.len() {
                0 => self.scope_inspector(ui, lib),
                1 => {
                    let id = self.ws.selected[0].clone();
                    if let Some(item) = lib.item(&id).cloned() {
                        self.item_inspector(ui, lib, item);
                    }
                }
                n => self.multi_inspector(ui, lib, n),
            }
        });
    }

    /// A section title that folds the section away when clicked, with an
    /// arrow that points right when it is folded. True while it is shown.
    /// The state is shared by the three inspectors and every library.
    pub(crate) fn fold_heading(&mut self, ui: &mut Ui, which: Fold, text: &str) -> bool {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::click());
        let closed = self.folds.closed(which);
        if resp.clicked() {
            *closed = !*closed;
        }
        let color = if resp.hovered() { t::p().text } else { t::p().text_muted };
        icon(ui, Pos2::new(rect.left() + 6.0, rect.center().y), if *closed { ic::CARET_RIGHT } else { ic::CARET_DOWN }, 10.0, color);
        ui.painter().text(rect.left_center() + vec2(16.0, 0.0), Align2::LEFT_CENTER, text, font(13.0), color);
        !*closed
    }

    /// Saves text typed into the folder inspector when its fields went away
    /// with focus still in them (the scope or the selection changed), unless
    /// they are still shown for folder `shown`.
    pub(crate) fn flush_folder_edit(&mut self, lib: &Library, shown: Option<&str>) {
        let Some((id, name, desc, smart)) = self.ws.folder_edit.take_if(|(id, ..)| shown != Some(id.as_str())) else { return };
        let Some((old_name, old_desc)) = folder_texts(lib, &id, smart) else { return };
        if !name.trim().is_empty() && name.trim() != old_name {
            self.ws.actions.push(rename_action(&id, name.trim(), smart));
        }
        if desc != old_desc {
            self.ws.actions.push(if smart { Action::SetSmartDescription(id, desc) } else { Action::SetFolderDescription(id, desc) });
        }
    }

    pub(crate) fn scope_inspector(&mut self, ui: &mut Ui, lib: &Library) {
        let folder = match &self.ws.scope {
            Scope::Folder(id) => logic::find_folder(&lib.metadata.folders, id).cloned(),
            _ => None,
        };
        let smart_id = match &self.ws.scope {
            Scope::Smart(id) => Some(id.clone()),
            _ => None,
        };
        // The smart folder open, as (id, name, description), if it exists.
        let smart = smart_id.and_then(|id| folder_texts(lib, &id, true).map(|(n, d)| (id, n, d)));
        let shown = folder.as_ref().map(|f| (f.id.clone(), f.name.clone(), f.description.clone(), false)).or_else(|| smart.map(|(i, n, d)| (i, n, d, true)));
        self.flush_folder_edit(lib, shown.as_ref().map(|s| s.0.as_str()));
        if let Some((id, saved_name, saved_desc, is_smart)) = &shown {
            // A folder's name and description save when the field loses
            // focus (Return does that for the name), as one undo step each.
            // An emptied name goes back to the saved one, as in Eagle. A
            // smart folder's are edited the same way.
            if self.ws.folder_edit.is_none() {
                self.ws.folder_edit = Some((id.clone(), saved_name.clone(), saved_desc.clone(), *is_smart));
            }
            let (_, name, desc, _) = self.ws.folder_edit.as_mut().expect("set above");
            let r = field(ui, name, "Name", 33.0, false, "scope-name");
            if r.lost_focus() {
                let typed = name.trim();
                if !typed.is_empty() && typed != saved_name {
                    self.ws.actions.push(rename_action(id, typed, *is_smart));
                }
            } else if !r.has_focus() {
                // Follow renames made elsewhere, and undo.
                name.clone_from(saved_name);
            }
            let r = field(ui, desc, "Description", 54.0, true, "scope-desc");
            if r.lost_focus() {
                if desc != saved_desc {
                    let (id, text) = (id.clone(), desc.clone());
                    self.ws.actions.push(if *is_smart { Action::SetSmartDescription(id, text) } else { Action::SetFolderDescription(id, text) });
                }
            } else if !r.has_focus() {
                desc.clone_from(saved_desc);
            }
        } else {
            // Other scopes have fixed names.
            let mut name = self.ws.scope.title(&lib.metadata.folders);
            ui.add_enabled_ui(false, |ui| field(ui, &mut name, "Name", 33.0, false, "scope-name"));
        }
        divider(ui);
        let items: Vec<&Item> = self.ws.order.iter().filter_map(|id| lib.item(id)).collect();
        let size: u64 = items.iter().map(|i| i.size).sum();
        if self.fold_heading(ui, Fold::Properties, "Properties") {
            prop(ui, "Items", &items.len().to_string());
            prop(ui, "Size", &logic::format_size(size));
            if let Some(f) = &folder {
                prop(ui, "Date Imported", &logic::format_ms(f.modification_time));
                prop(ui, "Password", if f.password.is_empty() { "None" } else { "Set" });
            }
        }
        ui.add_space(4.0);
        self.scope_export_button(ui, lib);
    }

    pub(crate) fn multi_inspector(&mut self, ui: &mut Ui, lib: &Library, n: usize) {
        let items: Vec<&Item> = self.ws.selected.iter().filter_map(|id| lib.item(id)).collect();
        let ids: Vec<String> = items.iter().map(|i| i.id.clone()).collect();
        let size: u64 = items.iter().map(|i| i.size).sum();

        // A fanned stack of the first three previews, like Eagle's.
        let w = ui.available_width();
        let (stack, _) = ui.allocate_exact_size(vec2(w, 150.0), Sense::hover());
        for (k, item) in items.iter().take(3).enumerate().rev() {
            let off = (k as f32 - 1.0) * 14.0;
            let r = Rect::from_center_size(stack.center() + vec2(off, -off * 0.4), Vec2::splat(112.0));
            ui.painter().rect_filled(r, CornerRadius::same(6), t::p().field);
            ui.painter().rect_stroke(r, CornerRadius::same(6), Stroke::new(1.0, t::p().field_border), StrokeKind::Inside);
            if let Some(tex) = self.ws.loader.get(&item.id, Tier::Grid).cloned() {
                let ts = tex.size_vec2();
                let sc = ((r.width() - 10.0) / ts.x).min((r.height() - 10.0) / ts.y);
                let ir = Rect::from_center_size(r.center(), ts * sc);
                egui::Image::new((tex.id(), ir.size())).corner_radius(4).paint_at(ui, ir);
            }
            if let Some(j) = job_for(item, lib.root(), Tier::Grid) {
                self.ws.frame_jobs.push(j);
            }
        }
        ui.vertical_centered(|ui| ui.label(RichText::new(format!("{n} items selected")).size(15.0)));

        // Eagle's multi-selection inspector has no name field, but the note
        // and URL apply to every item: each shows the value they share, or
        // nothing when they differ.
        let shown = (
            logic::shared_text(items.iter().map(|i| i.annotation.as_str())).to_string(),
            logic::shared_text(items.iter().map(|i| i.url.as_str())).to_string(),
        );
        if self.ws.multi_edit.as_ref().is_none_or(|m| m.ids != ids || (m.shown != shown && !ui.ctx().egui_wants_keyboard_input())) {
            self.ws.multi_edit = Some(MultiEditBuf { ids: ids.clone(), notes: shown.0.clone(), url: shown.1.clone(), shown: shown.clone() });
        }
        let m = self.ws.multi_edit.as_mut().unwrap();
        let mut edits = Vec::new();
        let notes_hint = if shown.0.is_empty() && items.iter().any(|i| !i.annotation.is_empty()) { "Multiple notes" } else { "Notes..." };
        let r = field(ui, &mut m.notes, notes_hint, 54.0, true, "multi-notes");
        if r.lost_focus() && m.notes != m.shown.0 {
            let action = Action::SetNotes(ids.clone(), m.notes.clone());
            m.shown.0 = m.notes.clone();
            edits.push(action);
        }
        let url_hint = if shown.1.is_empty() && items.iter().any(|i| !i.url.is_empty()) { "Multiple URLs" } else { "http://" };
        let r = field(ui, &mut m.url, url_hint, 33.0, false, "multi-url");
        if r.lost_focus() && m.url.trim() != m.shown.1 {
            m.url = m.url.trim().to_string();
            let action = Action::SetUrls(ids.clone(), m.url.clone());
            m.shown.1 = m.url.clone();
            edits.push(action);
        }
        for action in edits {
            self.push_bulk(ids.len(), action);
        }

        // Tags every selected item shares; the picker adds to all.
        if self.fold_heading(ui, Fold::Tags, "Tags") {
            let common = crate::tags::common_tags(&items);
            self.tag_field(ui, lib, &common, &ids);
        }

        if self.fold_heading(ui, Fold::Folders, "Folders") {
            // A chip for each folder all of them are in; the × takes all of
            // them out, as for one item.
            let mut remove_folder = None;
            let mut open_folder = None;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for fid in logic::shared_folders(&items) {
                    let Some(f) = logic::find_folder(&lib.metadata.folders, &fid) else { continue };
                    let path = logic::folder_path(&lib.metadata.folders, &fid).unwrap_or_else(|| f.name.clone());
                    match folder_chip(ui, &f.name, &path) {
                        Some(ChipClick::Remove) => remove_folder = Some(fid),
                        Some(ChipClick::Open) => open_folder = Some(fid),
                        None => {}
                    }
                }
                let add = ui.add(egui::Button::new(RichText::new(format!("{}  Add All to Folder", ic::PLUS)).size(12.0).color(t::p().text_muted)).frame(false));
                egui::Popup::menu(&add).show(|ui| {
                    ui.set_min_width(200.0);
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for f in lib.all_folders() {
                            if ui.button(&f.name).clicked() {
                                self.push_bulk(ids.len(), Action::AddToFolder(ids.clone(), f.id.clone()));
                            }
                        }
                    });
                });
            });
            if let Some(fid) = remove_folder {
                self.ws.actions.push(Action::RemoveFromFolder(ids.clone(), fid));
            }
            if let Some(fid) = open_folder {
                self.go(Scope::Folder(fid));
            }
        }

        divider(ui);
        if self.fold_heading(ui, Fold::Properties, "Properties") {
            let stars: HashSet<u64> = items.iter().map(|i| i.extra.get("star").and_then(|v| v.as_u64()).unwrap_or(0)).collect();
            let shown = if stars.len() == 1 { stars.into_iter().next().unwrap_or(0) } else { 0 };
            if let Some(star) = rating(ui, shown) {
                self.push_bulk(ids.len(), Action::BatchRating(ids.clone(), star));
            }
            prop(ui, "Items", &n.to_string());
            prop(ui, "Size", &logic::format_size(size));
        }
        ui.add_space(4.0);
        let trash = self.ws.scope == Scope::Trash;
        let label = if trash { "Restore All" } else { "Move All to Trash" };
        if wide_button(ui, ic::TRASH, label, true).clicked() {
            let n = ids.len();
            if trash {
                self.ws.actions.push(Action::Trash(ids, false));
            } else {
                self.push_bulk(n, Action::TrashAsk(ids));
            }
        }
    }

    /// Copy Tags: keeps `tags` as the tag clipboard and puts them on the
    /// system clipboard one per line, as Eagle does. Nothing to copy keeps
    /// the old clipboard.
    pub(crate) fn copy_tags(&mut self, ctx: &egui::Context, tags: Vec<String>) {
        if tags.is_empty() {
            self.flash_brief("No tags to copy");
            return;
        }
        ctx.copy_text(tags.join("\n"));
        self.flash_brief(format!("Copied {}", export_ui::plural(tags.len(), "tag")));
        self.copied_tags = Some(tags);
    }

    /// Paste Tags: the copied tags go onto `ids`, after their own.
    pub(crate) fn paste_tags(&mut self, ids: Vec<String>) {
        if let Some(tags) = self.copied_tags.clone()
            && !ids.is_empty()
        {
            self.ws.actions.push(Action::PasteTags(ids, tags));
        }
    }

    /// The menu of the inspector's tag area (Eagle's tag input menu):
    /// copy, paste and clear the tags of `ids`, or for one chip `tag`,
    /// show its items, copy just it, or paste.
    pub(crate) fn tag_area_menu(&mut self, ui: &mut Ui, tags: &[String], ids: &[String], tag: Option<&str>) {
        use crate::shortcuts::Command as C;
        ui.set_min_width(190.0);
        if let Some(tag) = tag {
            if ui.button("Show Items with This Tag").clicked() {
                self.go(Scope::Tag(tag.to_string()));
            }
            if ui.button("Copy Tag").clicked() {
                let ctx = ui.ctx().clone();
                self.copy_tags(&ctx, vec![tag.to_string()]);
            }
        } else {
            let keys = self.shortcut_text(ui.ctx(), C::CopyTags);
            if ui.add_enabled(!tags.is_empty(), egui::Button::new("Copy Tags").shortcut_text(keys)).clicked() {
                let ctx = ui.ctx().clone();
                self.copy_tags(&ctx, tags.to_vec());
            }
        }
        let keys = self.shortcut_text(ui.ctx(), C::PasteTags);
        if ui.add_enabled(self.copied_tags.is_some(), egui::Button::new("Paste Tags").shortcut_text(keys)).clicked() {
            self.paste_tags(ids.to_vec());
        }
        if tag.is_none() {
            ui.separator();
            let keys = self.shortcut_text(ui.ctx(), C::ClearTags);
            if ui.add_enabled(!tags.is_empty(), egui::Button::new("Clear Tags").shortcut_text(keys)).clicked() {
                self.ws.actions.push(Action::ClearTags(ids.to_vec()));
            }
        }
    }

    /// The inspector's tags: a chip per tag (its × takes the tag off `ids`)
    /// and the button that opens the tag picker, as in Eagle.
    pub(crate) fn tag_field(&mut self, ui: &mut Ui, lib: &Library, tags: &[String], ids: &[String]) {
        let mut remove = None;
        let mut open = None;
        // The area senses clicks behind its chips: right-clicking between
        // them offers copy, paste and clear.
        let area = ui.scope_builder(egui::UiBuilder::new().sense(Sense::click()), |ui| ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for tag in tags {
                let chip = tag_chip_removable(ui, tag, App::tag_dot(lib, tag));
                if chip.clicked() {
                    remove = Some(tag.clone());
                }
                chip.context_menu(|ui| self.tag_area_menu(ui, tags, ids, Some(tag)));
            }
            let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::TagPanel);
            let label = if tags.is_empty() { format!("{}  Add Tags", ic::PLUS) } else { ic::PLUS.to_string() };
            let b = ui.add(egui::Button::new(RichText::new(label).size(12.0).color(t::p().text_muted)).frame(false));
            let b = b.on_hover_text(if keys.is_empty() { "Add tags".to_string() } else { format!("Add tags  ({keys})") });
            if b.clicked() {
                open = Some(b.rect.left_center());
            }
        }));
        area.response.context_menu(|ui| self.tag_area_menu(ui, tags, ids, None));
        if let Some(tag) = remove {
            self.ws.actions.push(Action::RemoveTags(ids.to_vec(), vec![tag]));
        }
        if let Some(at) = open {
            let ctx = ui.ctx().clone();
            self.open_tag_picker(&ctx, Some(at));
        }
    }

    pub(crate) fn item_inspector(&mut self, ui: &mut Ui, lib: &mut Library, item: Item) {
        if self.ws.edit.as_ref().is_none_or(|e| e.id != item.id) {
            self.ws.edit = Some(EditBuf {
                id: item.id.clone(),
                name: item.name.clone(),
                notes: item.annotation.clone(),
                url: item.url.clone(),
            });
        }
        let w = ui.available_width();

        // Preview.
        let aspect = if item.width > 0 && item.height > 0 { item.height as f32 / item.width as f32 } else { 1.0 };
        let ph = (w * aspect).clamp(80.0, 260.0);
        let (prect, presp) = ui.allocate_exact_size(vec2(w, ph), Sense::click());
        if let Some(tex) = self.ws.loader.get(&item.id, Tier::Preview) {
            let ts = if item.width > 0 { vec2(item.width as f32, item.height as f32) } else { tex.size_vec2() };
            let scale = (prect.width() / ts.x).min(prect.height() / ts.y).min(1.0);
            let r = Rect::from_center_size(prect.center(), ts * scale);
            egui::Image::new((tex.id(), r.size())).corner_radius(4).paint_at(ui, r);
        } else if !thumbnail::is_supported(&item.ext) && item.thumbnail_path(lib.root()).is_none() {
            ext_placeholder(ui, prect, &item.ext);
        }
        if presp.double_clicked() {
            self.ws.viewer = Some(item.id.clone());
        }
        if let Some(j) = job_for(&item, lib.root(), Tier::Preview) {
            self.ws.frame_jobs.push(j);
        }

        // Palette.
        if !item.palettes.is_empty() {
            let d = 16.0;
            let (row, _) = ui.allocate_exact_size(vec2(w, d), Sense::hover());
            let mut toggle_primary: Option<(usize, bool)> = None;
            for (n, c) in item.palettes.iter().enumerate() {
                let center = Pos2::new(row.left() + d / 2.0 + n as f32 * (d + 6.0), row.center().y);
                if center.x + d / 2.0 > row.right() {
                    break;
                }
                let color = Color32::from_rgb(c.color[0], c.color[1], c.color[2]);
                ui.painter().circle_filled(center, d / 2.0, color);
                let marked = c.extra.get(falcon_library::item_ops::MARKED_KEY).and_then(|v| v.as_bool()).unwrap_or(false);
                if marked {
                    ui.painter().circle_stroke(center, d / 2.0 + 1.0, Stroke::new(2.0, t::p().primary));
                } else {
                    ui.painter().circle_stroke(center, d / 2.0, Stroke::new(1.0, t::p().divider));
                }
                let hit = ui.interact(Rect::from_center_size(center, Vec2::splat(d)), ui.scope_id().with(("swatch", n)), Sense::click());
                let hex = logic::hex(c.color);
                if hit.clicked() {
                    self.ws.filter.color = Some(ColorQuery::new(c.color));
                    self.ws.color_text = hex.clone();
                    self.ws.show_filter = true;
                }
                let mut similar = false;
                hit.context_menu(|ui| {
                    if ui.button("Find Similar Colours").clicked() {
                        similar = true;
                        ui.close();
                    }
                    ui.separator();
                    let values = [
                        colors::hex(c.color),
                        colors::rgb(c.color),
                        colors::rgba(c.color),
                        colors::hsl(c.color),
                        colors::hsv(c.color),
                        colors::hwb(c.color),
                        colors::cmyk(c.color),
                    ];
                    for (i, v) in values.iter().enumerate() {
                        if i == 4 {
                            ui.separator();
                        }
                        if ui.button(format!("Copy {v}")).clicked() {
                            ui.ctx().copy_text(v.clone());
                            self.flash_brief(format!("Copied {v}"));
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.checkbox(&mut { marked }, "Primary Colour").clicked() {
                        toggle_primary = Some((n, !marked));
                        ui.close();
                    }
                });
                if similar {
                    self.ws.filter.color = Some(ColorQuery::new(c.color));
                    self.ws.color_text = hex.clone();
                    self.ws.show_filter = true;
                }
                hit.on_hover_text(format!("{hex}  ·  {}%\nClick to find similar colours, right-click for more", c.ratio));
            }
            if let Some((n, on)) = toggle_primary
                && let Err(e) = lib.set_palette_marked(&item.id, n, on)
            {
                self.flash_error(format!("Could not save: {e}"));
            }
        }

        let mut changed: Option<Item> = None;
        let link_keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::OpenLink);
        let edit = self.ws.edit.as_mut().unwrap();
        let r = field(ui, &mut edit.name, "Title", 33.0, false, "item-name");
        if std::mem::take(&mut self.ws.focus_title) {
            r.request_focus();
        }
        if r.lost_focus() && edit.name.trim() != item.name && !edit.name.trim().is_empty() {
            let mut it = item.clone();
            it.name = edit.name.trim().to_string();
            changed = Some(it);
        }
        let r = field(ui, &mut edit.notes, "Notes...", 54.0, true, "item-notes");
        if r.lost_focus() && edit.notes != item.annotation {
            let mut it = changed.clone().unwrap_or_else(|| item.clone());
            it.annotation = edit.notes.clone();
            changed = Some(it);
        }
        let r = field(ui, &mut edit.url, "http://", 33.0, false, "item-url");
        let mut open_link = false;
        if !item.url.is_empty() || !edit.url.is_empty() {
            let at = Pos2::new(r.rect.right() - 18.0, r.rect.center().y);
            let tip = format!("Open Link ({link_keys})");
            open_link = icon_button(ui, at, ic::ARROW_SQUARE_OUT, 16.0, true, &tip).clicked();
        }
        if r.lost_focus() && edit.url != item.url {
            if crate::urls::url_accepted(edit.url.trim(), |p| std::path::Path::new(p).exists()) {
                let mut it = changed.clone().unwrap_or_else(|| item.clone());
                it.url = edit.url.trim().to_string();
                changed = Some(it);
            } else {
                edit.url = item.url.clone();
                self.ws.status = Some(Toast::new("Not a valid link", ToastKind::Info, Life::Normal));
            }
        }
        if open_link && let Some(t) = crate::urls::link_target(&item.url, |p| std::path::Path::new(p).exists()) {
            crate::urls::open_link(&t);
        }

        // Tags.
        if self.fold_heading(ui, Fold::Tags, "Tags") {
            self.tag_field(ui, lib, &item.tags, std::slice::from_ref(&item.id));
        }

        // Folders.
        let folders_open = self.fold_heading(ui, Fold::Folders, "Folders");
        let mut remove_folder = None;
        let mut open_folder = None;
        let mut add_folder = None;
        if folders_open {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for fid in &item.folders {
                    let name = logic::find_folder(&lib.metadata.folders, fid).map_or_else(|| fid.clone(), |f| f.name.clone());
                    let path = logic::folder_path(&lib.metadata.folders, fid).unwrap_or_else(|| name.clone());
                    match folder_chip(ui, &name, &path) {
                        Some(ChipClick::Remove) => remove_folder = Some(fid.clone()),
                        Some(ChipClick::Open) => open_folder = Some(fid.clone()),
                        None => {}
                    }
                }
                let add = ui.add(egui::Button::new(RichText::new(format!("{}  Add Category", ic::PLUS)).size(12.0).color(t::p().text_muted)).frame(false));
                egui::Popup::menu(&add).show(|ui| {
                    ui.set_min_width(200.0);
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for f in lib.all_folders() {
                            if !item.folders.contains(&f.id) && ui.button(&f.name).clicked() {
                                add_folder = Some(f.id.clone());
                            }
                        }
                    });
                });
            });
        }
        if let Some(fid) = remove_folder {
            let mut it = changed.clone().unwrap_or_else(|| item.clone());
            it.folders.retain(|x| *x != fid);
            changed = Some(it);
        }
        // Eagle's quick open: show the folder with this item still selected.
        if let Some(fid) = open_folder
            && logic::find_folder(&lib.metadata.folders, &fid).is_some()
        {
            self.go(Scope::Folder(fid));
            self.ws.selected = vec![item.id.clone()];
            self.ws.anchor = Some(item.id.clone());
            self.ws.scroll_to = Some(item.id.clone());
        }
        if let Some(fid) = add_folder {
            self.note_folder_use(std::slice::from_ref(&fid));
            let mut it = changed.clone().unwrap_or_else(|| item.clone());
            // Eagle's folder picker in the inspector adds auto-tags too.
            if falcon_library::autotag::join_folder(&mut it, &lib.metadata.folders, &fid) {
                changed = Some(it);
            }
        }

        self.comment_list(ui, &item);
        divider(ui);
        if self.fold_heading(ui, Fold::Properties, "Properties") {
            let star = item.extra.get("star").and_then(|v| v.as_u64()).unwrap_or(0);
            if let Some(new_star) = rating(ui, star) {
                let mut it = changed.clone().unwrap_or_else(|| item.clone());
                if new_star == 0 {
                    it.extra.remove("star");
                } else {
                    it.extra.insert("star".into(), new_star.into());
                }
                changed = Some(it);
            }
            if item.width > 0 {
                prop(ui, "Dimensions", &format!("{} × {}", item.width, item.height));
            }
            prop(ui, "Size", &logic::format_size(item.size));
            let type_row = prop(ui, "Type", &item.ext.to_uppercase());
            let type_hit = ui.interact(type_row.rect, ui.scope_id().with("type-row"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Click to change the extension");
            if type_hit.clicked() {
                self.ws.ext_dialog = Some(ext_ui::ExtDialog { id: item.id.clone(), current: item.ext.to_lowercase(), text: item.ext.clone(), uid: egui::Id::unique("change-extension"), focused: false });
            }
            prop(ui, "Date Imported", &logic::format_ms(item.modification_time));
            prop(ui, "Date Created", &logic::format_ms(item.btime));
            prop(ui, "Date Modified", &logic::format_ms(item.mtime));
            media_view::media_props(ui, &item);
        }
        ui.add_space(4.0);
        if wide_button(ui, ic::ARROW_SQUARE_OUT, "Open in Default App", true).clicked() {
            let files = [item.original_path(lib.root())];
            crate::open_with::open_default(&files);
            self.used(&files);
        }
        ui.add_space(4.0);
        if wide_button(ui, ic::EXPORT, "Show in Finder", true).clicked() {
            let _ = std::process::Command::new("open").arg("-R").arg(item.original_path(lib.root())).spawn();
        }

        if let Some(it) = changed {
            match lib.update_item(it) {
                Ok(()) => {
                    if let Some(e) = self.ws.edit.as_mut() {
                        e.name = lib.item(&item.id).map_or(e.name.clone(), |i| i.name.clone());
                    }
                }
                Err(e) => self.flash_error(format!("Could not save: {e}")),
            }
        }
    }
}

impl App {
    pub(crate) fn smart_dialog_ui(&mut self, ctx: &egui::Context, lib: &Library) {
        let Some(mut d) = self.ws.smart_dialog.take() else { return };
        let mut keep = true;
        let folders = organize_ui::folder_choices(&lib.metadata.folders);
        let max_rules_height = (ctx.content_rect().height() - 320.0).max(160.0);
        let modal = egui::Modal::new(egui::Id::unique("smart-dialog")).show(ctx, |ui| {
            ui.set_width(660.0);
            ui.label(RichText::new(if d.editing.is_some() { "Edit Smart Folder" } else { "New Smart Folder" }).size(16.0));
            ui.add_space(6.0);
            ui.add(TextEdit::singleline(&mut d.name).hint_text("Name").desired_width(f32::INFINITY));
            ui.add_space(6.0);
            App::smart_dialog_extras(ui, &mut d, lib);
            ui.add_space(6.0);
            egui::ScrollArea::vertical().max_height(max_rules_height).auto_shrink([false, true]).show(ui, |ui| {
                organize_ui::smart_conditions_ui(ui, &mut d.conditions, &folders);
            });
            let result = smart_draft::conditions(&d.conditions);
            if let Err(e) = &result {
                ui.add_space(4.0);
                ui.label(RichText::new(e).color(t::p().icon_red));
            }
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let ok = !d.name.trim().is_empty() && !d.conditions.is_empty() && result.is_ok();
                let label = if d.editing.is_some() { "Save" } else { "Create" };
                if ui.add_enabled(ok, egui::Button::new(label)).clicked()
                    && let Ok(conditions) = result
                {
                    let name = d.name.trim().to_string();
                    self.save_smart_draft(&d, name, conditions);
                    keep = false;
                }
                if ui.button("Cancel").clicked() {
                    keep = false;
                }
            });
        });
        if modal.should_close() {
            keep = false;
        }
        if keep {
            self.ws.smart_dialog = Some(d);
        }
    }
}

/// The saved name and description of a folder, or of a smart folder.
pub(crate) fn folder_texts(lib: &Library, id: &str, smart: bool) -> Option<(String, String)> {
    if !smart {
        return logic::find_folder(&lib.metadata.folders, id).map(|f| (f.name.clone(), f.description.clone()));
    }
    fn find(list: &[falcon_library::smart::SmartFolder], id: &str) -> Option<(String, String)> {
        list.iter().find_map(|f| if f.id == id { Some((f.name.clone(), f.description.clone())) } else { find(&f.children, id) })
    }
    find(&lib.smart_folders(), id)
}

/// The action that renames a folder or a smart folder from the inspector.
pub(crate) fn rename_action(id: &str, name: &str, smart: bool) -> Action {
    if smart { Action::Organize(organize_ui::OrgAction::RenameSmart(id.to_string(), name.to_string())) } else { Action::RenameFolder(id.to_string(), name.to_string()) }
}

// ---------------------------------------------------------------------------
// Viewer and frame

impl App {
    /// The open item's viewer, drawn by `parent` (the central panel) over
    /// `screen`: as in Eagle's detail mode it takes the place of the grid,
    /// between the sidebar and the inspector.
    pub(crate) fn viewer(&mut self, parent: &mut Ui, lib: &Library, screen: Rect) {
        let ctx = &parent.ctx().clone();
        let Some(id) = self.ws.viewer.clone() else { return };
        let Some(item) = lib.item(&id) else {
            self.ws.viewer = None;
            return;
        };
        if self.ws.view.id != id {
            self.ws.view = ViewState { id: id.clone(), file: item.original_path(lib.root()), ..Default::default() };
        }
        // An original still in iCloud downloads in the background first.
        if self.icloud_viewer(parent, lib, item, screen) {
            return;
        }
        // An animation playing in colour starts again in grayscale, and back.
        if self.ws.view.anim.as_ref().is_some_and(|a| a.gray != self.modes.grayscale) {
            self.ws.view.anim = None;
        }
        if self.ws.view.anim.is_none()
            && let Some(kind) = crate::anim::kind(&item.ext)
        {
            let mut player = crate::anim::Player::open(ctx, item.original_path(lib.root()), kind, crate::anim::Opts::viewer());
            player.gray = self.modes.grayscale;
            self.ws.view.anim = Some(player);
        }
        if self.text_viewer(parent, lib, item, screen)
            || self.video_viewer(parent, lib, item, screen)
            || self.pdf_viewer(parent, lib, item, screen)
            || self.media_viewer(parent, lib, item, screen)
        {
            return;
        }
        let ui = &mut viewer_ui(parent, screen, "viewer");
        let p = ui.painter().clone();
        p.rect_filled(screen, 0.0, t::p().main);
        let bar = Rect::from_min_size(screen.min, vec2(screen.width(), t::BAR_HEIGHT));
        let area = Rect::from_min_max(Pos2::new(screen.left(), bar.bottom()), screen.max);
        let (area, gif_bar) = self.gif_split(item, area);

        // The image: a frame of an animation, the full render, or the preview.
        let now = ctx.input(|i| i.time);
        let gif = self.ws.view.anim.as_mut().and_then(|a| {
            a.update(now);
            a.texture().cloned()
        });
        let tex = gif
            .or_else(|| self.ws.loader.get(&id, Tier::Full).cloned())
            .or_else(|| self.ws.loader.get(&id, Tier::Preview).cloned());
        self.image_canvas(ui, lib, item, tex, bar, area, gif_bar);
    }

    /// Eagle's "cannot preview" page: the file's type, a line saying so
    /// and a button listing the apps that can open it.
    pub(crate) fn cannot_preview(&mut self, ui: &mut Ui, item: &Item, area: Rect) {
        let c = area.center();
        ext_placeholder(ui, Rect::from_center_size(c - vec2(0.0, 70.0), Vec2::splat(150.0)), &item.ext);
        let name = format!("{}.{}", item.name, item.ext);
        self.notice_lines(ui, area, &name, "Falcon can't show a preview of this file. Try opening it in another app.");
    }

    /// The title, line and Open With button under the picture of a page
    /// shown instead of a preview.
    pub(crate) fn notice_lines(&mut self, ui: &mut Ui, area: Rect, title: &str, line: &str) {
        let c = area.center();
        let g = galley(ui, title, font(15.0), t::p().text, (area.width() - 40.0).max(1.0));
        ui.painter().galley(Pos2::new(c.x - g.size().x / 2.0, c.y + 14.0), g, t::p().text);
        ui.painter().text(c + vec2(0.0, 48.0), Align2::CENTER_CENTER, line, font(12.5), t::p().text_muted);
        let b = ui.put(Rect::from_center_size(c + vec2(0.0, 84.0), vec2(210.0, 28.0)), egui::Button::new("Open With Another App…"));
        let file = self.ws.view.file.clone();
        egui::Popup::menu(&b).show(|ui| {
            ui.set_min_width(200.0);
            self.open_menu(ui, std::slice::from_ref(&file));
        });
    }

    pub(crate) fn empty_state(&mut self, ui: &mut Ui) {
        let r = ui.max_rect();
        window_drag(ui, Rect::from_min_size(r.min, vec2(r.width(), t::BAR_HEIGHT)), "empty");
        ui.painter().rect_filled(r, 0.0, t::p().main);
        let c = r.center();
        ui.painter().text(c - vec2(0.0, 40.0), Align2::CENTER_CENTER, "Open an Eagle library", font(18.0), t::p().text);
        if let Some(e) = &self.error {
            ui.painter().text(c - vec2(0.0, 14.0), Align2::CENTER_CENTER, e, font(12.0), t::p().text_muted);
        }
        let b = Rect::from_center_size(c + vec2(0.0, 24.0), vec2(180.0, 32.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(b));
        if wide_button(&mut child, ic::FOLDER_OPEN, "Choose Library…", true).clicked() {
            self.pick_library();
        }
    }
}

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
    use super::{drag_image, restored_note};
    use std::path::Path;

    #[test]
    fn restore_note_names_the_library_and_backup() {
        let note = restored_note(Path::new("/s/Photos.library/backup/backup-2026-10-26 02.15.25.310.json"));
        assert_eq!(note, "Photos's metadata.json was damaged and was restored from backup/backup-2026-10-26 02.15.25.310.json; the damaged file was kept in backup/");
    }

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
