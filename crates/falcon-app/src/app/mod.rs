//! The main window, laid out after Eagle's: sidebar, toolbar, card grid and
//! inspector, with Eagle's dark theme.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Pos2, Rect, Ui, vec2};
use falcon_library::Library;
use falcon_library::search::OrderBy;

use crate::icons as ic;
use crate::logic::{self, Layout};
use crate::locale::tr;
use crate::theme as t;
use crate::ui::widgets::{galley, icon_button};

pub(crate) mod actions;
pub(crate) mod commands;
pub(crate) mod image_edit;
pub(crate) mod import;
pub(crate) mod library;
pub(crate) mod long_ops;
pub(crate) mod mcp;
pub(crate) mod modal;
pub(crate) mod navigation;
pub(crate) mod open_queue;
pub(crate) mod page_shots;
pub(crate) mod state;
pub(crate) mod sounds;
pub(crate) mod system;

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
    pub(crate) mcp: mcp::McpState,
    pub(crate) ctx: egui::Context,
    pub(crate) error: Option<String>,
    pub(crate) tile: f32,
    pub(crate) show_sidebar: bool,
    pub(crate) show_inspector: bool,
    /// Which sidebar sections are folded to their headers.
    pub(crate) sections: crate::ui::sidebar::SectionFold,
    /// The last answer to the Move to Trash question in a folder (take the
    /// items out of the folder only), kept while falcon runs, as Eagle does.
    pub(crate) trash_folder_only: bool,
    /// The sidebar's and inspector's widths, dragged by their inner edge.
    pub(crate) sidebar_width: f32,
    pub(crate) inspector_width: f32,
    pub(crate) focus_search: bool,
    /// Focus the sidebar's folder filter next frame (Find ▸ Filter Sidebar).
    pub(crate) focus_folder_filter: bool,
    /// Go to Folder (`ui/dialogs/quick_switch.rs`).
    pub(crate) quick: crate::ui::dialogs::quick_switch::QuickSwitch,
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
    /// Folders items were last added to, newest first (`app/commands/items.rs`).
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
    /// The Notification Center: what toasts and notices said (`notice_history.rs`).
    pub(crate) notices: crate::notice_history::History,
    pub(crate) notice_window_open: bool,
    /// Remove from Disk is asking for confirmation (`ui/dialogs/remove_library.rs`).
    pub(crate) remove_library: Option<crate::ui::dialogs::remove_library::RemoveLibrary>,
    /// The library tools' dialogs (`ui/dialogs/tools.rs`).
    pub(crate) tools: crate::ui::dialogs::tools::Tools,
    /// The localhost check of the running API (`api_check.rs`).
    pub(crate) api_check: crate::api_check::Check,
    /// A library whose close waits for its tasks (`ui/dialogs/force_close.rs`).
    pub(crate) force_close: Option<crate::ui::dialogs::force_close::Pending>,
    /// A switch waiting for an answer (`ui/dialogs/switch_warning.rs`).
    pub(crate) switch_warning: Option<crate::ui::dialogs::switch_warning::Target>,
    /// Quitting while tasks run (`ui/dialogs/quit_confirm.rs`).
    pub(crate) quit: crate::ui::dialogs::quit_confirm::QuitConfirm,
    /// The libraries' `icon.png` textures.
    pub(crate) lib_icons: crate::library_icons::LibraryIcons,
    /// What to open once the frame is done (it replaces the library being
    /// drawn).
    pub(crate) switch_to: Option<Switch>,
    /// The create-library dialog and its follow-up steps.
    pub(crate) dialogs: crate::ui::dialogs::library::State,
    /// Whether the welcome flow has been finished (or skipped by having
    /// used falcon before).
    pub(crate) welcomed: bool,
    /// The first-run tips were dismissed once (`tips.rs`).
    pub(crate) tips_seen: bool,
    /// The tips overlay (`ui/dialogs/tips.rs`).
    pub(crate) tips: crate::tips::TipsState,
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
    /// The sound effects (`sounds.rs`), played through `audio`'s stream.
    pub(crate) sounds: crate::sounds::SoundState,
    /// Video playback for the viewer (`ui/viewer/video.rs`).
    pub(crate) video: crate::ui::viewer::video::VideoState,
    /// The font viewer's tab and theme, kept across fonts and launches.
    pub(crate) font_look: crate::ui::viewer::font::FontLook,
    /// The PDF reader for the viewer (`ui/viewer/pdf.rs`).
    pub(crate) pdf: crate::ui::viewer::pdf::PdfState,
    /// The web viewer's native page (macOS).
    pub(crate) web: crate::webview::WebView,
    /// The image viewer's zoom mode and remembered views (`ui/viewer/image.rs`).
    pub(crate) image: crate::ui::viewer::image::ImageState,
    /// The 3D model viewer (`model3d/`, `ui/viewer/model.rs`).
    pub(crate) model3d: crate::model3d::Model3dState,
    /// The page control of a multi-page TIFF in the viewer (`ui/viewer/tiff.rs`).
    pub(crate) tiff: crate::tiff_pages::TiffPages,
    /// The grid's video and audio hover previews (`hover_media.rs`).
    pub(crate) hover_media: crate::hover_media::HoverMedia,
    /// Drop targets and other import state (`import.rs`).
    pub(crate) imports: import::Imports,
    /// File ▸ Import Links… (`ui/dialogs/import_links.rs`).
    pub(crate) link_import: crate::ui::dialogs::import_links::LinkImport,
    /// Screenshots of bookmarked pages (`page_shots.rs`).
    pub(crate) page_shots: page_shots::PageShots,
    /// The macOS menu bar (`menu.rs`).
    pub(crate) menu_bar: crate::menu::Bar,
    /// The open library's actions, their panel and editor (`actions/`).
    pub(crate) user_actions: crate::actions::ActionsState,
    /// Layouts remembered per view, the fallback from before, and the
    /// display options (see `ui/toolbar/layout_panel.rs`).
    pub(crate) layouts: HashMap<String, Layout>,
    pub(crate) legacy_layout: Option<Layout>,
    pub(crate) display: logic::Display,
    /// Reload the shown library from disk next frame (the layout panel's
    /// Refresh).
    pub(crate) refresh: bool,
    /// Reload the library next frame for this smart folder's Refresh.
    pub(crate) refresh_smart: Option<String>,
    /// The text viewer's zoom level, kept across items and launches.
    pub(crate) text_zoom: u8,
    /// Inspector sections folded away (`Folds`).
    pub(crate) folds: Folds,
    /// The All Tags page's sort and grouping (`TagLook`).
    pub(crate) tag_look: crate::tags::TagLook,
    /// Emoji folder icons and their textures (`folder_emoji.rs`).
    pub(crate) folder_emoji: crate::folder_emoji_tex::FolderEmojis,
    /// The auto-import folder's watcher and what it has handed over
    /// (`auto_import.rs`).
    pub(crate) auto_import: crate::auto_import::AutoImport,
    /// Presentation mode and the other View modes (`ui/viewer/modes.rs`).
    pub(crate) modes: crate::ui::viewer::modes::Modes,
    /// The preview windows open, each with its items (`ui/viewer/preview_window/`).
    pub(crate) preview: Vec<crate::ui::viewer::preview_window::PreviewWin>,
    /// Numbers preview windows so each has its own viewport.
    pub(crate) preview_serial: u64,
    /// The serial of the preview window being drawn, while one is: the
    /// shared viewers (3D, web) then leave out the main viewer's controls.
    pub(crate) preview_drawing: Option<u64>,

    /// The About window (`about.rs`).
    pub(crate) about: crate::ui::dialogs::about::AboutWindow,
    /// What the arrow keys act on (Eagle's focus zones).
    pub(crate) zone: logic::Zone,
    /// The sidebar rows the arrow keys walk (`ui/sidebar`).
    pub(crate) side: crate::ui::sidebar::SidebarKeys,
    /// A notice to acknowledge (`ui/dialogs/library_notices.rs`).
    pub(crate) alert: Option<crate::ui::dialogs::library_notices::Alert>,
    /// Reload the shown library from disk next frame: a `metadata.json`
    /// write was refused because the file changed elsewhere.
    pub(crate) conflict_reload: bool,
    /// A library that can't be found, for the missing-library screen.
    pub(crate) missing: Option<PathBuf>,
    /// A merge of another library in progress (`ui/dialogs/library_notices.rs`).
    pub(crate) merge: Option<crate::ui::dialogs::library_notices::Merge>,
    /// An `.eaglepack` import waiting for an answer.
    pub(crate) pack_ask: Option<crate::ui::dialogs::library_notices::PackAsk>,
    /// Empty Cache and Reload is asking for confirmation.
    pub(crate) confirm_reload: bool,
    /// A library being read on a background thread (`ui/dialogs/library_notices.rs`).
    pub(crate) loading: Option<crate::ui::dialogs::library_notices::Loading>,
    /// Libraries waiting for their turn behind the loading screen, and the
    /// launch's starting view (`app/open_queue.rs`).
    pub(crate) opens: open_queue::OpenQueue,
    /// The global keys, the menu bar icon and screen capture (`system.rs`).
    pub(crate) system: system::System,
    /// Smaller main-window state (`shell.rs`): text cards' cache.
    pub(crate) shell: crate::shell::Shell,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>, launch: Launch) -> Self {
        t::install(&cc.egui_ctx);
        cc.egui_ctx.add_plugin(crate::access_order::AccessOrder);
        let mut app = App {
            ws: Workspace::new(&cc.egui_ctx, None),
            others: Default::default(),
            title: String::new(),
            open_switcher: false,
            server: None,
            mcp: Default::default(),
            ctx: cc.egui_ctx.clone(),
            error: None,
            tile: 139.0,
            show_sidebar: true,
            sidebar_width: t::SIDEBAR_WIDTH,
            inspector_width: t::INSPECTOR_WIDTH,
            show_inspector: true,
            sections: Default::default(),
            trash_folder_only: true,
            focus_search: false,
            focus_folder_filter: false,
            quick: Default::default(),
            copied_tags: None,
            native_drag: Arc::default(),
            imports: Default::default(),
            link_import: Default::default(),
            page_shots: Default::default(),
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
            notices: Default::default(),
            notice_window_open: false,
            remove_library: None,
            tools: Default::default(),
            api_check: Default::default(),
            force_close: None,
            switch_warning: None,
            quit: Default::default(),
            lib_icons: Default::default(),
            switch_to: None,
            dialogs: Default::default(),
            welcomed: true,
            tips_seen: true,
            tips: Default::default(),
            prefs: Default::default(),
            search: Default::default(),
            prefs_window: Default::default(),
            lock: Default::default(),
            shortcuts: Default::default(),
            collect: Default::default(),
            collect_extra: None,
            audio: Default::default(),
            sounds: Default::default(),
            video: Default::default(),
            font_look: Default::default(),
            pdf: Default::default(),
            web: Default::default(),
            image: Default::default(),
            model3d: crate::model3d::Model3dState::new(cc),
            tiff: Default::default(),
            hover_media: Default::default(),
            shell: Default::default(),
            menu_bar: Default::default(),
            user_actions: Default::default(),
            layouts: HashMap::new(),
            legacy_layout: None,
            display: Default::default(),
            refresh: false,
            refresh_smart: None,
            text_zoom: 0,
            folds: Folds::default(),
            tag_look: Default::default(),
            folder_emoji: Default::default(),
            auto_import: Default::default(),
            modes: Default::default(),
            preview: Vec::new(),
            preview_serial: 0,
            preview_drawing: None,

            about: Default::default(),
            zone: logic::Zone::Content,
            side: Default::default(),
            alert: None,
            conflict_reload: false,
            missing: None,
            merge: None,
            pack_ask: None,
            confirm_reload: false,
            loading: None,
            opens: Default::default(),
            system: Default::default(),
        };
        let saved: Saved = cc.storage.and_then(|s| eframe::get_value(s, eframe::APP_KEY)).unwrap_or_default();
        app.welcomed = crate::ui::dialogs::library::already_welcomed(saved.welcomed, saved.recent.len(), saved.libraries.len(), saved.library.is_some());
        app.tips_seen = saved.tips_seen;
        app.tile = saved.tile.unwrap_or(app.tile).clamp(TILE_MIN, TILE_MAX);
        app.ws.sort = saved.sort.as_deref().and_then(|l| OrderBy::ALL.into_iter().find(|o| o.label() == l)).unwrap_or(app.ws.sort);
        app.ws.increase = saved.increase.unwrap_or(app.ws.increase);
        app.show_sidebar = saved.show_sidebar.unwrap_or(true);
        crate::dock::on_hidden(crate::hover_media::window_hidden);
        let clamp = |w: Option<f32>, default: f32| w.unwrap_or(default).clamp(*t::PANEL_WIDTHS.start(), *t::PANEL_WIDTHS.end());
        app.sidebar_width = clamp(saved.sidebar_width, t::SIDEBAR_WIDTH);
        app.inspector_width = clamp(saved.inspector_width, t::INSPECTOR_WIDTH);
        app.show_inspector = saved.show_inspector.unwrap_or(true);
        app.sections = crate::ui::sidebar::SectionFold { quick: saved.quick_collapsed, smart: saved.smart_collapsed, folders: saved.folders_collapsed };
        app.ws.collapsed = saved.collapsed.into_iter().collect();
        app.recent = saved.recent;
        app.recent_folders = saved.recent_folders;
        app.notices = saved.notices;
        app.notices.trim();
        app.legacy_layout = saved.layout;
        app.layouts = saved.layouts;
        app.display = saved.display;
        app.prefs = saved.prefs.sanitized();
        let lang = app.prefs.language.resolve();
        crate::locale::load_fonts(&cc.egui_ctx, lang);
        crate::locale::set(lang);
        app.search = saved.search;
        // The interface size is a preference, as in Eagle, so egui's own
        // ⌘+ and ⌘− (which falcon uses to zoom the grid) must not change it.
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        cc.egui_ctx.set_zoom_factor(crate::prefs::zoom_factor(app.prefs.ui_zoom));
        app.prefs_window.set_pane(saved.prefs_pane);
        app.lock = crate::applock::Lock::at_launch(app.prefs.app_lock.as_ref());
        app.shortcuts = saved.shortcuts;
        app.shortcuts.migrate();
        app.collect.pos = saved.collect_pos.map(Pos2::from);
        app.video.volume = saved.video_volume.unwrap_or(1.0).clamp(0.0, 1.0);
        app.video.looping = saved.video_loop.unwrap_or(false);
        app.video.muted = saved.video_muted.unwrap_or(false);
        app.video.resume = saved.video_positions;
        app.pdf.set_marks(saved.pdf_marks);
        app.image.edge = saved.zoom_edge.unwrap_or(false);
        app.folds = saved.folds;
        app.tag_look = saved.tag_look;
        app.folder_emoji.store = saved.folder_emoji;
        app.text_zoom = saved.text_zoom.unwrap_or(0).min(crate::textdoc::ZOOM_LEVELS - 1);
        app.auto_import.imported = Arc::new(Mutex::new(saved.auto_imported));
        app.font_look = saved.font_look.unwrap_or_default();
        app.modes.hide_navigator = saved.hide_navigator.unwrap_or(false);
        app.recent_items = saved.recent_items;
        // Reopen the libraries that were open, then the one given on the
        // command line (focused if it was already open).
        app.missing = crate::workspaces::missing_shown(&saved.libraries, saved.active, saved.library.as_deref(), |p| p.is_dir());
        let (libraries, active) = crate::workspaces::restore(&saved.libraries, saved.active, saved.library.as_deref(), |p| p.is_dir());
        // Each is read behind the loading screen; the command line's
        // starting view waits until they are open (`app/open_queue.rs`).
        let show = initial.or_else(|| libraries.get(active).cloned());
        app.queue_launch(libraries, show, launch);
        crate::links::set_repaint(&app.ctx);
        crate::open_files::set_repaint(&app.ctx);
        app
    }

}


pub(crate) type Shared = falcon_server::Shared;

pub(crate) fn lock(s: &Shared) -> std::sync::MutexGuard<'_, Library> {
    s.lock().unwrap_or_else(|p| p.into_inner())
}

impl eframe::App for App {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.save_state(storage);
    }

    fn on_exit(&mut self) {
        // Unpacked web archives don't outlive falcon.
        crate::web_archive::remove_own();
    }

    fn persist_egui_memory(&self) -> bool {
        false
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        crate::vibrancy::clear_color(t::p().main, self.vibrancy_on())
    }

    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // The red button or ⌘W on the window: like Eagle, falcon stays
        // in the Dock and comes back when its icon is clicked. With the
        // Dock icon option off it leaves the Dock too while the menu bar
        // icon is shown, else the close goes through and quits (`dock.rs`).
        let mut left_dock = false;
        if ctx.input(|i| i.viewport().close_requested()) && self.quit_on_close_request(&ctx) {
            let action = crate::dock::close_action(self.prefs.keep_dock_icon, self.prefs.menu_icon, crate::dock::CAN_HIDE);
            if action != crate::dock::CloseAction::Quit && crate::dock::keep_running_hidden() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                if action == crate::dock::CloseAction::HideFromDock {
                    crate::dock::set_in_dock(false);
                    left_dock = true;
                }
                self.hover_media.stop_all();
                for ws in std::iter::once(&mut self.ws).chain(self.others.parked_mut()) {
                    let _ = ws.flush_and_backup();
                }
            }
        }
        // Opened again some other way (Finder, a link): back in the Dock.
        if !left_dock && !crate::dock::in_dock() && ctx.input(|i| i.viewport().focused == Some(true)) {
            crate::dock::set_in_dock(true);
        }
        self.system_tick(&ctx);
        // Eagle stops hover playback when its window is hidden. Hiding
        // the app and minimizing are caught by `dock::on_hidden`; this
        // covers a viewport that reports itself minimized.
        if ctx.input(|i| i.viewport().minimized) == Some(true) {
            self.hover_media.stop_all();
        }
        crate::vibrancy::sync_boxes(&ctx, t::p().panel, self.vibrancy_on());
        if self.lock.is_locked() {
            // Nothing but the lock screen acts while locked.
            let _ = crate::menu::take_clicks();
            self.lock_screen(ui);
        } else {
            self.menu_clicks(&ctx);
            self.draw(ui, frame);
        }
        self.quit_dialog(&ctx);
        // The web page sits over the viewer only in frames that asked for it.
        self.web.sync(&ctx, frame);
        self.page_shots_tick(&ctx);
        crate::colorspace::sync(frame, self.prefs.color_space);
        crate::vibrancy::sync_main_window(frame, self.vibrancy_on());
        let state = self.menu_state(&ctx);
        self.menu_bar.sync(&ctx, &self.shortcuts, &state, &self.user_actions.menu_entries());
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
        self.take_mcp_undo();
        self.poll_loading();
        self.pump_opens();
        // Links and files opened with falcon wait while the launch's
        // libraries are still being read.
        if !self.opens_pending() {
            for link in crate::links::take() {
                self.follow_link(link);
            }
            // Files opened with falcon: packages and files are imported,
            // libraries opened (`App::import_into`), as when dropped.
            let opened = crate::open_files::take();
            if !opened.is_empty() {
                self.import(opened);
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
        }
        if std::mem::take(&mut self.refresh) {
            self.refresh_library();
        }
        if let Some(id) = self.refresh_smart.take() {
            self.refresh_smart_folder(&id);
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
        self.notice_history_keys(&ctx);
        self.folder_emoji.store.set_library(self.ws.root.as_deref());
        self.video_keys(&ctx);
        self.gif_keys(&ctx);
        self.hover_media.begin(ctx.cumulative_frame_nr());
        self.tiff_keys(&ctx);
        self.edit_keys(&ctx);
        self.actions_tick(&ctx);
        self.keyboard(&ctx);
        if let Some(d) = self.prefs_window.show(&ctx) {
            self.apply_preferences(d);
        }
        self.import_dialogs(&ctx);
        self.import_links_ui(&ctx);
        self.about_ui(&ctx);
        self.notice_history_ui(&ctx);
        self.tips_ui(&ctx);
        self.library_dialogs(&ctx);
        if self.collect.open {
            self.collect_window(&ctx);
            for status in self.collect.take_statuses() {
                self.notices.record(&status, crate::notice_history::Kind::Info, crate::notice_history::now());
            }
        }
        self.merge_dialogs(&ctx);
        self.tools_dialogs(&ctx);
        self.pack_dialogs(&ctx);
        self.progress_modal(&ctx);
        self.dock_import_progress();
        self.reload_confirm(&ctx);
        self.clear_history_dialog(&ctx);
        self.remove_library_dialog(&ctx);
        self.force_close_dialog(&ctx);
        self.switch_warning_dialog(&ctx);
        self.poll_api_check();
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
                    if icon_button(&mut top, c, ic::SIDEBAR_SIMPLE, 18.0, true, tr("Show inspector")).clicked() {
                        self.show_inspector = true;
                    }
                }
                let mut grid_top = bar.bottom();
                let pb = Rect::from_min_size(Pos2::new(full.left(), grid_top), vec2(full.width(), 26.0));
                let mut pui = ui.new_child(egui::UiBuilder::new().max_rect(pb));
                if self.progress_bar(&mut pui, pb) {
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
        self.capture_notice_ui(&ctx);
        self.video.keep_only(self.ws.viewer.as_deref());
        self.fetches.prune((self.ws.view.id.as_str() == self.ws.viewer.as_deref().unwrap_or_default()).then_some(self.ws.view.file.as_path()));
        self.pdf.keep_only(self.ws.viewer.as_deref());
        self.drag_out(&ctx, frame, lib);
        if self.ws.smart_dialog.is_some() {
            self.smart_dialog_ui(&ctx, lib);
        }
        self.organize_dialogs(&ctx, lib);
        self.tag_picker_ui(&ctx, lib);
        self.quick_switch_ui(&ctx, lib);
        self.actions_panel_ui(&ctx, lib);
        self.action_editor_ui(&ctx, lib);
        self.folder_dialogs(&ctx, lib);
        self.ext_dialog(&ctx);
        self.item_rename_dialog(&ctx, lib);
        self.collect_duplicates();
        self.duplicate_dialog(&ctx, lib);
        self.export_dialogs(&ctx, lib);
        self.confirm_dialog(&ctx, lib);
        self.bulk_dialog(&ctx);
        self.text_saves(lib, ctx.input(|i| i.time));
        self.preview_window(&ctx, lib);
        self.apply_actions(lib);
        self.end_frame_edits(lib);
        self.check_conflict(lib);
        self.ws.loader.keep_original(self.ws.viewer.as_deref());
        self.ws.loader.want(std::mem::take(&mut self.ws.frame_jobs));
        drop(guard);
        match self.switch_to.take() {
            Some(Switch::Open(p)) => self.request_switch(crate::ui::dialogs::switch_warning::Target::Open(p)),
            Some(Switch::Pick) => self.pick_library(),
            Some(Switch::Create) => self.new_library(),
            Some(Switch::Focus(i)) => self.request_focus(i),
            Some(Switch::Close(i)) => self.request_close(i),
            Some(Switch::Merge) => self.merge_library(),
            None => {}
        }
        self.run_import_request();
        self.auto_import_tick();
    }
}
