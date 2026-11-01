//! The app's state: one `Workspace` per open library (where the user is in
//! it, what is selected, its dialogs and caches), the image viewer's
//! `ViewState`, the small buffers and markers the drawing code shares, and
//! `Saved`, what falcon remembers between launches.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use eframe::egui::{self, Pos2, Vec2};
use falcon_library::search::{Filter, OrderBy};
use falcon_library::smart_draft::{self, ConditionDraft};
use falcon_library::undo::Transaction;

use super::{Action, App, Shared, image_edit, import, lock};
use crate::loader::Loader;
use crate::logic::{self, Layout, Scope};

pub(crate) const TILE_MIN: f32 = 80.0;

pub(crate) const TILE_MAX: f32 = 400.0;

/// What falcon remembers between launches, as Eagle does: the open
/// libraries, zoom, sort and panel layout.
#[derive(serde::Serialize, serde::Deserialize, Default)]
#[serde(default)]
pub(crate) struct Saved {
    /// The library being shown. Also all that state saved before several
    /// libraries could be open has (see `workspaces::restore`).
    pub(crate) library: Option<PathBuf>,
    /// Every open library, in switcher order, and which one is shown.
    pub(crate) libraries: Vec<PathBuf>,
    pub(crate) active: Option<usize>,
    pub(crate) tile: Option<f32>,
    pub(crate) sort: Option<String>,
    pub(crate) increase: Option<bool>,
    pub(crate) show_sidebar: Option<bool>,
    pub(crate) show_inspector: Option<bool>,
    /// Whether the Quick Access list is folded away, as Eagle remembers.
    pub(crate) quick_collapsed: bool,
    /// The same for the Smart Folders and Folders sections.
    pub(crate) smart_collapsed: bool,
    pub(crate) folders_collapsed: bool,
    /// Panel widths as last dragged.
    pub(crate) sidebar_width: Option<f32>,
    pub(crate) inspector_width: Option<f32>,
    pub(crate) collapsed: Vec<String>,
    pub(crate) recent: Vec<PathBuf>,
    /// Folder ids items were last added to, newest first (Eagle's
    /// `recentMoveFolders`), for Add to Last Used Folder.
    pub(crate) recent_folders: Vec<String>,
    /// The one layout falcon kept before layouts were remembered per view;
    /// now only the fallback for a library with none of its own.
    pub(crate) layout: Option<Layout>,
    /// Layouts by folder or smart folder id, or by library path for the
    /// other pages (`logic::layout_for`).
    pub(crate) layouts: HashMap<String, Layout>,
    /// The grid display options of the layout panel.
    pub(crate) display: logic::Display,
    pub(crate) prefs: crate::prefs::Prefs,
    /// The Preferences pane shown last.
    pub(crate) prefs_pane: crate::prefs::Section,
    /// Where the collect window was last.
    pub(crate) collect_pos: Option<[f32; 2]>,
    /// Changed keyboard shortcuts (see `shortcuts.rs`).
    pub(crate) shortcuts: crate::shortcuts::Shortcuts,
    /// The video player's volume, loop setting and mute.
    pub(crate) video_volume: Option<f32>,
    pub(crate) video_loop: Option<bool>,
    pub(crate) video_muted: Option<bool>,
    /// Where each video was left (Preferences ▸ Preview ▸ resume).
    pub(crate) video_positions: crate::ui::viewer::video::Positions,
    /// The PDF reader's last page and zoom of recent documents.
    pub(crate) pdf_marks: Vec<crate::ui::viewer::pdf::Mark>,
    /// The image viewer's "fit to window edge" mode, kept like Eagle's
    /// `eagle.viewer.lastZoomMode`.
    pub(crate) zoom_edge: Option<bool>,
    /// The text viewer's text size (`textdoc::zoom_scale`).
    pub(crate) text_zoom: Option<u8>,
    /// Files the auto-import folder has handed over (`auto_import.rs`).
    pub(crate) auto_imported: crate::auto_import::Imported,
    /// The font viewer's tab and theme.
    pub(crate) font_look: Option<crate::ui::viewer::font::FontLook>,
    /// The image viewer's navigator is hidden (Eagle's `isHideNavigator`).
    pub(crate) hide_navigator: Option<bool>,
    /// Search scope, pinned filters and Go to Folder history.
    pub(crate) search: crate::search_state::SearchPrefs,
    /// Recently Used, per library (`recent.rs`).
    pub(crate) recent_items: crate::recent::RecentItems,
    /// Which inspector sections are folded away, for every library.
    pub(crate) folds: Folds,
    /// The welcome flow was finished.
    pub(crate) welcomed: bool,
}

/// An inspector section whose title folds it away.
#[derive(Clone, Copy)]
pub(crate) enum Fold {
    Properties,
    Tags,
    Folders,
    Comments,
}

/// The inspector sections the user folded away. Saved as "closed" flags so
/// state from before they could fold, and a new section, start expanded.
#[derive(serde::Serialize, serde::Deserialize, Default, Clone, Copy, PartialEq, Debug)]
#[serde(default)]
pub(crate) struct Folds {
    pub(crate) properties: bool,
    pub(crate) tags: bool,
    pub(crate) folders: bool,
    pub(crate) comments: bool,
}

impl Folds {
    pub(crate) fn closed(&mut self, which: Fold) -> &mut bool {
        match which {
            Fold::Properties => &mut self.properties,
            Fold::Tags => &mut self.tags,
            Fold::Folders => &mut self.folders,
            Fold::Comments => &mut self.comments,
        }
    }
}

/// A rubber-band selection in progress: where it started, in the grid's
/// content space, and the selection it started from.
pub(crate) struct Marquee {
    pub(crate) start: Pos2,
    pub(crate) base: Vec<String>,
    pub(crate) mode: logic::MarqueeMode,
}

/// Ids being dragged from the grid.
#[derive(Clone)]
pub(crate) struct DragItems(pub(crate) Vec<String>);

/// A folder being dragged in the sidebar.
#[derive(Clone)]
pub(crate) struct DragFolder(pub(crate) String);

/// A Quick Access row being dragged, by the id it pins.
#[derive(Clone)]
pub(crate) struct DragQuick(pub(crate) String);

/// The starting view requested on the command line.
#[derive(Default)]
pub struct Launch {
    /// Items to open (one) or select (several, or with `select`).
    pub items: Vec<String>,
    pub select: bool,
    pub import: Vec<PathBuf>,
    pub search: Option<String>,
    pub color: Option<String>,
    pub tags: bool,
    /// `eagle://folder/…` and `eagle://smart-folder/…` links to open.
    pub links: Vec<crate::links::Link>,
}

/// A library change requested from the library menu or a shortcut,
/// applied once the frame is done.
pub(crate) enum Switch {
    Open(PathBuf),
    Pick,
    Create,
    /// Show the open library at this switcher position.
    Focus(usize),
    /// Close the open library at this switcher position.
    Close(usize),
    /// Merge another library into the shown one.
    Merge,
}

/// Background import counters.
#[derive(Default)]
pub(crate) struct Progress {
    pub(crate) total: usize,
    pub(crate) done: usize,
    pub(crate) failed: usize,
    pub(crate) current: String,
    /// Every failed import since the list was last cleared, kept across
    /// batches like Eagle's error list (`import.rs` shows it).
    pub(crate) errors: Vec<import::ImportError>,
    /// Ids of the items added in this batch, for auto-select.
    pub(crate) added: Vec<String>,
}

/// Zoom, pan and animation of the image viewer.
#[derive(Default)]
pub(crate) struct ViewState {
    /// Item the state belongs to; reset when another item opens.
    pub(crate) id: String,
    /// Scale relative to the image's pixels; `None` fits the window.
    pub(crate) zoom: Option<f32>,
    pub(crate) pan: Vec2,
    /// The animation playing, for GIFs, animated WebPs and APNGs (see
    /// `anim.rs`).
    pub(crate) anim: Option<crate::anim::Player>,
    /// The GIF player bar's drag state.
    pub(crate) gif: crate::ui::viewer::gif::GifUi,
    /// The item's original, for the bar's open button.
    pub(crate) file: PathBuf,
    /// A zoom key pressed this frame, for the image viewer.
    pub(crate) zoom_key: Option<logic::ViewZoom>,
}

/// The smart-folder dialog's state.
#[derive(Clone)]
pub(crate) struct SmartDraft {
    /// The smart folder being edited; `None` creates a new one.
    pub(crate) editing: Option<String>,
    pub(crate) name: String,
    /// Every condition, in the editable form that writes back unchanged
    /// parts verbatim (see `falcon_library::smart_draft`).
    pub(crate) conditions: Vec<ConditionDraft>,
    /// The parent smart folder (`None` is the top level), and where it was.
    pub(crate) parent: Option<String>,
    pub(crate) orig_parent: Option<String>,
    pub(crate) icon: Option<String>,
    pub(crate) color: Option<String>,
}

impl Default for SmartDraft {
    fn default() -> Self {
        SmartDraft {
            editing: None,
            name: String::new(),
            conditions: vec![ConditionDraft::default()],
            parent: None,
            orig_parent: None,
            icon: None,
            color: None,
        }
    }
}

impl SmartDraft {
    /// Fills the dialog from a stored smart folder, every condition included.
    pub(crate) fn from_folder(sf: &falcon_library::smart::SmartFolder) -> SmartDraft {
        SmartDraft {
            editing: Some(sf.id.clone()),
            name: sf.name.clone(),
            conditions: smart_draft::drafts(&sf.conditions),
            icon: sf.icon.clone(),
            color: sf.icon_color.clone(),
            ..Default::default()
        }
    }
}

/// A destructive action waiting for the user to confirm it.
#[derive(Clone)]
pub(crate) enum Confirm {
    /// Folders to delete; the dialog asks whether their items go to the
    /// Trash too.
    DeleteFolders(Vec<String>),
    DeleteSmarts(Vec<String>),
    DeleteItems(Vec<String>),
    EmptyTrash,
    /// Trash ▸ Restore All Items.
    RestoreAll,
    /// Move to Trash in a folder where some items are also in other
    /// folders: `(items, folder)`.
    TrashInFolder(Vec<String>, String),
    DeleteTags(Vec<String>),
    ClearTagHistory,
}

/// Uncommitted inspector text for one item.
pub(crate) struct EditBuf {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) notes: String,
    pub(crate) url: String,
}

/// Uncommitted multi-selection inspector text: the items, then the note
/// and URL as shown (their shared value, or empty) and as typed.
pub(crate) struct MultiEditBuf {
    pub(crate) ids: Vec<String>,
    pub(crate) shown: (String, String),
    pub(crate) notes: String,
    pub(crate) url: String,
}

/// Uncommitted folder inspector text: `(id, name, description, smart)`, for
/// a folder or (`smart`) a smart folder.
pub(crate) type FolderEditBuf = (String, String, String, bool);

/// Everything that belongs to one open library: the library itself, where
/// the user is in it, what is selected, its undo history, its dialogs and
/// its image caches. The app draws one workspace at a time.
pub(crate) struct Workspace {
    pub(crate) lib: Option<Shared>,
    /// `lib`'s root when it was opened here. The HTTP API can put another
    /// library into `lib` (`library/switch`); the app notices the change.
    pub(crate) root: Option<PathBuf>,
    pub(crate) loader: Loader,
    pub(crate) scope: Scope,
    pub(crate) back: Vec<Scope>,
    pub(crate) forward: Vec<Scope>,
    pub(crate) query: String,
    pub(crate) sort: OrderBy,
    pub(crate) layout: Layout,
    /// Eagle's `sortIncrease`: time keys newest first, others ascending.
    pub(crate) increase: bool,
    pub(crate) filter: Filter,
    pub(crate) show_filter: bool,
    /// Hex text typed into the colour filter.
    pub(crate) color_text: String,
    /// Keyword text typed into the Notes and URL filters.
    pub(crate) note_text: String,
    pub(crate) url_text: String,
    pub(crate) selected: Vec<String>,
    pub(crate) anchor: Option<String>,
    pub(crate) viewer: Option<String>,
    pub(crate) collapsed: HashSet<String>,
    pub(crate) folder_filter: String,
    pub(crate) edit: Option<EditBuf>,
    pub(crate) multi_edit: Option<MultiEditBuf>,
    pub(crate) folder_edit: Option<FolderEditBuf>,
    pub(crate) status: Option<crate::toast::Toast>,
    /// Ids in grid order, refreshed every frame.
    pub(crate) order: Vec<String>,
    pub(crate) columns: usize,
    pub(crate) scroll_to: Option<String>,
    /// The rubber band being dragged over the grid, if any.
    pub(crate) marquee: Option<Marquee>,
    /// Where ⌥↑ goes: the open folder's (or smart folder's) parent.
    /// Refreshed each frame while the library is locked.
    pub(crate) up: Option<Scope>,
    /// Whether some of the items acted on can be flipped, some rotated, and
    /// the one item cropped (for the menu bar). Refreshed with `up`.
    pub(crate) editable: [bool; 3],
    /// A Home, End, Page Up or Page Down for the grid, applied next frame.
    pub(crate) scroll: Option<crate::shortcuts::Scroll>,
    /// Images needed this frame, handed to the loader once at the end.
    pub(crate) frame_jobs: Vec<crate::loader::Job>,
    pub(crate) actions: Vec<Action>,
    /// Folder being renamed in the sidebar, with the text typed so far.
    pub(crate) renaming: Option<(String, String)>,
    /// Subfolder card being renamed above the grid, with the text so far.
    pub(crate) strip_rename: Option<(String, String)>,
    pub(crate) confirm: Option<Confirm>,
    /// Actions held back until the bulk dialog is answered, with the number
    /// of items they change (`App::push_bulk`).
    pub(crate) bulk: Option<(usize, Vec<Action>)>,
    pub(crate) smart_dialog: Option<SmartDraft>,
    pub(crate) view: ViewState,
    /// The GIF player turned on with the viewer's toolbar button, until
    /// the viewer closes (Eagle's `usingGifPlayer`).
    pub(crate) gif_player: bool,
    /// Animated grid cards and hover previews playing (see `anim.rs`).
    pub(crate) cards: crate::anim::Cards,
    /// Focus the inspector's title field next frame (F2).
    pub(crate) focus_title: bool,
    /// The Change Extension dialog (`ui/dialogs/ext.rs`).
    pub(crate) ext_dialog: Option<crate::ui::dialogs::ext::ExtDialog>,
    /// The tag picker (`ui/inspector/tag_picker.rs`).
    pub(crate) picker: crate::ui::inspector::tag_picker::TagPicker,
    pub(crate) progress: Arc<Mutex<Progress>>,
    /// Imported files found already in the library, waiting to be asked
    /// about (`import_dupes`), and the dialog asking.
    pub(crate) dupes: crate::import_dupes::Queue,
    pub(crate) dup_dialog: Option<crate::import_dupes::Dialog>,
    /// Tag being renamed on the All Tags page, with the new name so far.
    pub(crate) tag_rename: Option<(String, String)>,
    pub(crate) tag_filter: String,
    /// The enlarged preview of a hovered card (see `hover.rs`).
    pub(crate) hover: crate::hover::Hover,
    /// Edits that ⌘Z reverts, newest last, and what ⇧⌘Z redoes.
    pub(crate) undo: Vec<Transaction>,
    pub(crate) redo: Vec<Transaction>,
    /// ⌘Z (true) or ⇧⌘Z (false) pressed, applied once the library is locked.
    pub(crate) undo_request: Option<bool>,
    /// The library the undo history belongs to.
    pub(crate) undo_root: Option<PathBuf>,
    /// Labels of the actions applied this frame, naming its transaction.
    pub(crate) frame_labels: Vec<String>,
    /// Unlocked folders and the organising dialogs (see `ui/organize.rs`).
    pub(crate) org: crate::ui::organize::OrgState,
    /// Text typed into the Date, Duration, Camera and Font chips.
    pub(crate) filter_ui: crate::ui::filter_bar::media::MediaFilterUi,
    /// Waveforms for the viewer, and its font specimen.
    pub(crate) waveforms: crate::audio::Waveforms,
    pub(crate) font_view: crate::ui::viewer::font::FontView,
    /// The text item open in the viewer and its unsaved typing.
    pub(crate) text: Option<crate::ui::viewer::text::TextView>,
    /// The Duplicates page's hash cache and results.
    pub(crate) duplicates: crate::ui::grid::duplicates::Duplicates,
    /// Comment mode, crop tool and comment editing (`app/image_edit.rs`).
    pub(crate) tools: image_edit::Tools,
    /// When `metadata.json` is next backed up (see `backups.rs`).
    pub(crate) backup: crate::backups::Schedule,
    /// Export requests, prompts and the background copy (`ui/dialogs/export.rs`).
    pub(crate) export: crate::ui::dialogs::export::ExportState,
    /// Folders and smart folders picked with ⌘ and ⇧ (`ui/sidebar/folders.rs`).
    pub(crate) folders: crate::ui::sidebar::folders::FolderUi,
    /// Polls the library for changes made by other apps (`watch.rs`).
    pub(crate) watch: Option<crate::watch::Watcher>,
    /// The storage checks run when the library opened (`storage.rs`),
    /// until their result is taken.
    pub(crate) health: Option<std::sync::mpsc::Receiver<Vec<crate::storage::Warning>>>,
}

impl Workspace {
    /// A fresh workspace for `lib` (or the empty window when `None`).
    pub(crate) fn new(ctx: &egui::Context, lib: Option<Shared>) -> Self {
        let root = lib.as_ref().map(|l| lock(l).root().to_path_buf());
        let watch = root.clone().map(|r| {
            let ctx = ctx.clone();
            crate::watch::Watcher::start(r, move || ctx.request_repaint())
        });
        let health = root.clone().map(|r| {
            let ctx = ctx.clone();
            crate::storage::spawn_check(r, move || ctx.request_repaint())
        });
        Workspace {
            root,
            watch,
            health,
            lib,
            loader: Loader::new(ctx),
            scope: Scope::All,
            back: Vec::new(),
            forward: Vec::new(),
            query: String::new(),
            sort: OrderBy::Import,
            layout: Layout::default(),
            increase: true,
            filter: Filter::default(),
            show_filter: false,
            color_text: String::new(),
            note_text: String::new(),
            url_text: String::new(),
            selected: Vec::new(),
            anchor: None,
            viewer: None,
            collapsed: HashSet::new(),
            folder_filter: String::new(),
            edit: None,
            multi_edit: None,
            folder_edit: None,
            status: None,
            order: Vec::new(),
            columns: 1,
            scroll_to: None,
            marquee: None,
            scroll: None,
            up: None,
            editable: [false; 3],
            frame_jobs: Vec::new(),
            actions: Vec::new(),
            renaming: None,
            strip_rename: None,
            confirm: None,
            bulk: None,
            smart_dialog: None,
            view: ViewState::default(),
            gif_player: false,
            cards: Default::default(),
            focus_title: false,
            ext_dialog: None,
            picker: Default::default(),
            progress: Arc::new(Mutex::new(Progress::default())),
            dupes: Default::default(),
            dup_dialog: None,
            tag_rename: None,
            tag_filter: String::new(),
            hover: Default::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            undo_request: None,
            undo_root: None,
            frame_labels: Vec::new(),
            org: Default::default(),
            filter_ui: Default::default(),
            waveforms: Default::default(),
            font_view: Default::default(),
            text: None,
            duplicates: Default::default(),
            tools: Default::default(),
            backup: crate::backups::Schedule::new(Instant::now()),
            export: Default::default(),
            folders: Default::default(),
        }
    }

    /// Flushes pending writes and takes a backup now if a write is waiting
    /// for one, on this thread: the library is closing or the app saving
    /// its state (also on quit), and a background copy might not finish.
    pub(crate) fn flush_and_backup(&mut self) -> falcon_library::Result<()> {
        let Some(l) = self.lib.clone() else { return Ok(()) };
        if let Some(text) = &mut self.text {
            text.save(&mut lock(&l))?;
        }
        let report = lock(&l).flush()?;
        if report.wants_backup() {
            self.backup.changed(Instant::now());
        }
        if self.backup.take_pending()
            && let Some(root) = &self.root
        {
            crate::backups::backup_now(root);
        }
        Ok(())
    }
}

impl App {
    /// Writes `Saved` (eframe calls this periodically and on quit).
    pub(crate) fn save_state(&mut self, storage: &mut dyn eframe::Storage) {
        // Also runs on quit: nothing batched may be left unwritten, and a
        // write still waiting for its backup gets it now.
        for ws in std::iter::once(&mut self.ws).chain(self.others.parked_mut()) {
            let _ = ws.flush_and_backup();
        }
        let libraries: Vec<PathBuf> = self.others.iter(&self.ws).filter_map(|w| w.root.clone()).collect();
        let saved = Saved {
            library: self.ws.root.clone(),
            active: (!libraries.is_empty()).then_some(self.others.slot()),
            libraries,
            tile: Some(self.tile),
            sort: Some(self.ws.sort.label().to_string()),
            increase: Some(self.ws.increase),
            show_sidebar: Some(self.show_sidebar),
            sidebar_width: Some(self.sidebar_width),
            inspector_width: Some(self.inspector_width),
            show_inspector: Some(self.show_inspector),
            quick_collapsed: self.sections.quick,
            smart_collapsed: self.sections.smart,
            folders_collapsed: self.sections.folders,
            collapsed: self.ws.collapsed.iter().cloned().collect(),
            recent: self.recent.clone(),
            recent_folders: self.recent_folders.clone(),
            layout: self.legacy_layout,
            layouts: self.layouts.clone(),
            display: self.display.clone(),
            prefs: self.prefs.clone(),
            search: self.search.clone(),
            prefs_pane: self.prefs_window.pane(),
            shortcuts: self.shortcuts.clone(),
            collect_pos: self.collect.pos.map(<[f32; 2]>::from),
            video_volume: Some(self.video.volume),
            video_loop: Some(self.video.looping),
            video_muted: Some(self.video.muted),
            video_positions: self.video.positions(),
            pdf_marks: self.pdf.marks(),
            zoom_edge: Some(self.image.edge),
            text_zoom: Some(self.text_zoom),
            folds: self.folds,
            auto_imported: self.auto_import.imported.lock().unwrap_or_else(|p| p.into_inner()).clone(),
            font_look: Some(self.font_look),
            hide_navigator: Some(self.modes.hide_navigator),
            recent_items: self.recent_items.clone(),
            welcomed: self.welcomed,
        };
        eframe::set_value(storage, eframe::APP_KEY, &saved);
    }
}

#[cfg(test)]
mod tests {
    use super::Saved;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    /// eframe's storage, in memory.
    #[derive(Default)]
    struct Memory(HashMap<String, String>);

    impl eframe::Storage for Memory {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.to_string(), value);
        }
        fn remove_string(&mut self, key: &str) {
            self.0.remove(key);
        }
        fn flush(&mut self) {}
    }

    #[test]
    fn saved_state_from_one_library_still_loads() {
        // As saved before several libraries could be open (RON, as eframe stores it).
        let mut m = Memory::default();
        // `backup_on_change` was a preference that did nothing (Eagle has
        // none; backups always run) and was removed. State saved with it
        // must still load.
        m.set_string_raw(r#"(library: Some("/x/A.library"), tile: Some(150.0), recent: ["/x/A.library", "/x/B.library"], prefs: (backup_on_change: true))"#);
        let saved: Saved = eframe::get_value(&m, eframe::APP_KEY).expect("old state parses");
        assert_eq!(saved.tile, Some(150.0));
        let (libs, active) = crate::workspaces::restore(&saved.libraries, saved.active, saved.library.as_deref(), |_| true);
        assert_eq!((libs, active), (vec![PathBuf::from("/x/A.library")], 0));
    }

    #[test]
    fn sidebar_section_folds_default_open_and_are_kept() {
        let mut m = Memory::default();
        m.set_string_raw(r#"(quick_collapsed: true)"#);
        let saved: Saved = eframe::get_value(&m, eframe::APP_KEY).unwrap();
        assert!(saved.quick_collapsed && !saved.smart_collapsed && !saved.folders_collapsed);
        let saved = Saved { smart_collapsed: true, folders_collapsed: true, ..Default::default() };
        eframe::set_value(&mut m, eframe::APP_KEY, &saved);
        let back: Saved = eframe::get_value(&m, eframe::APP_KEY).unwrap();
        assert!(!back.quick_collapsed && back.smart_collapsed && back.folders_collapsed);
    }

    #[test]
    fn folded_inspector_sections_default_to_open_and_are_kept() {
        let mut m = Memory::default();
        m.set_string_raw(r#"(tile: Some(150.0))"#);
        let saved: Saved = eframe::get_value(&m, eframe::APP_KEY).unwrap();
        assert_eq!(saved.folds, super::Folds::default());
        m.set_string_raw(r#"(folds: (tags: true))"#);
        let mut saved: Saved = eframe::get_value(&m, eframe::APP_KEY).unwrap();
        assert!(*saved.folds.closed(super::Fold::Tags));
        assert!(!*saved.folds.closed(super::Fold::Properties));
        eframe::set_value(&mut m, eframe::APP_KEY, &saved);
        let back: Saved = eframe::get_value(&m, eframe::APP_KEY).unwrap();
        assert_eq!(back.folds, saved.folds);
    }

    #[test]
    fn saved_state_keeps_every_open_library() {
        let mut m = Memory::default();
        let libs = vec![PathBuf::from("/x/A.library"), PathBuf::from("/x/B.library")];
        let saved = Saved { library: Some(libs[1].clone()), libraries: libs.clone(), active: Some(1), ..Default::default() };
        eframe::set_value(&mut m, eframe::APP_KEY, &saved);
        let back: Saved = eframe::get_value(&m, eframe::APP_KEY).unwrap();
        let exists = |p: &Path| p != Path::new("/nowhere");
        assert_eq!(crate::workspaces::restore(&back.libraries, back.active, back.library.as_deref(), exists), (libs, 1));
    }

    impl Memory {
        fn set_string_raw(&mut self, ron: &str) {
            eframe::Storage::set_string(self, eframe::APP_KEY, ron.to_string());
        }
    }
}
