//! The main window, laid out after Eagle's: sidebar, toolbar, card grid and
//! inspector, with Eagle's dark theme.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, Galley, Key, Modifiers, Pos2, Rect, Response, RichText,
    Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2, text::LayoutJob, vec2,
};
use falcon_library::import::ImportOptions;
use falcon_library::model::{Folder, Item};
use falcon_library::{Library, thumbnail};

use crate::icons as ic;
use crate::loader::{Loader, Tier, job_for};
use crate::logic::{self, NavCounts, Scope};
use falcon_library::search::{ColorQuery, Filter, Logic, OrderBy, Shape, TextFilter};
use crate::theme::{self as t, font, mono};

/// Grid geometry, measured from Eagle at 1280 x 800.
const GRID_PAD_X: f32 = 17.0;
const GRID_PAD_TOP: f32 = 4.0;
const GRID_GAP: f32 = 8.0;
const CARD_TEXT: f32 = 46.0;
const ROW_GAP: f32 = 10.0;
const TILE_MIN: f32 = 80.0;
const TILE_MAX: f32 = 400.0;

/// What falcon remembers between launches, as Eagle does: the library,
/// zoom, sort and panel layout.
#[derive(serde::Serialize, serde::Deserialize, Default)]
#[serde(default)]
struct Saved {
    library: Option<PathBuf>,
    tile: Option<f32>,
    sort: Option<String>,
    increase: Option<bool>,
    show_sidebar: Option<bool>,
    show_inspector: Option<bool>,
    collapsed: Vec<String>,
    recent: Vec<PathBuf>,
}

/// Ids being dragged from the grid.
#[derive(Clone)]
struct DragItems(Vec<String>);

/// A folder being dragged in the sidebar.
#[derive(Clone)]
struct DragFolder(String);

/// Library changes requested while drawing, applied at the end of a frame.
enum Action {
    CreateFolder { parent: Option<String> },
    RenameFolder(String, String),
    DeleteFolder(String),
    AddToFolder(Vec<String>, String),
    Trash(Vec<String>, bool),
    DeletePermanently(Vec<String>),
    EmptyTrash,
    CreateSmart(String, Vec<serde_json::Value>),
    UpdateSmart(String, String, Vec<serde_json::Value>),
    RenameTag(String, String),
    MoveFolder(String, Option<String>),
    /// Adds and/or removes a tag on several items.
    BatchTag(Vec<String>, Option<String>, Option<String>),
    BatchRating(Vec<String>, u64),
    QuickAdd(&'static str, String),
    QuickRemove(String),
    DeleteTag(String),
    StarTag(String, bool),
    DeleteSmart(String),
}

/// A library change requested from the library menu.
enum Switch {
    Open(PathBuf),
    Pick,
    Create,
}

/// Background import counters.
#[derive(Default)]
struct Progress {
    total: usize,
    done: usize,
    failed: usize,
    current: String,
}

fn is_hidden(p: &std::path::Path) -> bool {
    p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))
}

/// Zoom, pan and animation of the full-window viewer.
#[derive(Default)]
struct ViewState {
    /// Item the state belongs to; reset when another item opens.
    id: String,
    /// Scale relative to the image's pixels; `None` fits the window.
    zoom: Option<f32>,
    pan: Vec2,
    /// Decoded GIF frames with their delays in seconds.
    frames: Vec<(egui::TextureHandle, f32)>,
    started: Option<Instant>,
}

/// Decodes up to 300 GIF frames into textures.
fn gif_frames(ctx: &egui::Context, path: &std::path::Path) -> Vec<(egui::TextureHandle, f32)> {
    use image::AnimationDecoder;
    let Ok(file) = std::fs::File::open(path) else { return Vec::new() };
    let Ok(decoder) = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(file)) else { return Vec::new() };
    decoder
        .into_frames()
        .take(300)
        .filter_map(Result::ok)
        .enumerate()
        .map(|(n, f)| {
            let (num, den) = f.delay().numer_denom_ms();
            let secs = (num as f32 / den.max(1) as f32 / 1000.0).max(0.02);
            let buf = f.into_buffer();
            let img = egui::ColorImage::from_rgba_unmultiplied([buf.width() as usize, buf.height() as usize], buf.as_raw());
            (ctx.load_texture(format!("gif-{n}"), img, egui::TextureOptions::LINEAR), secs)
        })
        .collect()
}

/// A rule row in the smart-folder dialog.
#[derive(Clone)]
struct RuleDraft {
    property: &'static str,
    method: &'static str,
    value: String,
}

impl Default for RuleDraft {
    fn default() -> Self {
        RuleDraft { property: "name", method: "contain", value: String::new() }
    }
}

/// The smart-folder dialog's state.
#[derive(Clone, Default)]
struct SmartDraft {
    /// The smart folder being edited; `None` creates a new one.
    editing: Option<String>,
    name: String,
    any: bool,
    rules: Vec<RuleDraft>,
}

impl SmartDraft {
    /// Fills the dialog from a stored smart folder's first condition.
    fn from_folder(sf: &falcon_library::smart::SmartFolder) -> SmartDraft {
        let cond = sf.conditions.first().cloned().unwrap_or_default();
        let any = cond.get("match").and_then(|m| m.as_str()) == Some("OR");
        let rules = cond
            .get("rules")
            .and_then(|r| r.as_array())
            .map(|rules| rules.iter().filter_map(RuleDraft::from_json).collect())
            .unwrap_or_default();
        SmartDraft { editing: Some(sf.id.clone()), name: sf.name.clone(), any, rules }
    }
}

/// Properties offered in the dialog, with their methods (Eagle's names).
const SMART_PROPS: [(&str, &str, &[&str]); 12] = [
    ("name", "Name", &["contain", "uncontain", "equal", "startWith", "endWith", "empty", "not-empty"]),
    ("annotation", "Notes", &["contain", "uncontain", "equal", "empty", "not-empty"]),
    ("url", "URL", &["contain", "uncontain", "equal", "empty", "not-empty"]),
    ("folderName", "Folder name", &["contain", "uncontain", "equal"]),
    ("tags", "Tags", &["union", "intersection", "equal", "identity", "empty", "not-empty"]),
    ("type", "Type", &["equal", "unequal"]),
    ("rating", "Rating", &["equal", "unequal"]),
    ("shape", "Shape", &["equal", "unequal"]),
    ("color", "Color", &["similar", "accuracy", "grayscale"]),
    ("width", "Width", &[">=", "<=", "=", ">", "<", "between"]),
    ("fileSize", "File size (MB)", &[">=", "<=", "between"]),
    ("createTime", "Date imported", &["within", "after", "before"]),
];

impl RuleDraft {
    /// Reads a stored rule back into the dialog's text form.
    fn from_json(r: &serde_json::Value) -> Option<RuleDraft> {
        let prop = r.get("property")?.as_str()?;
        let (property, _, methods) = SMART_PROPS.iter().find(|p| p.0 == prop)?;
        let m = r.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let method = methods.iter().find(|x| **x == m).copied().unwrap_or(methods[0]);
        let v = r.get("value").cloned().unwrap_or_default();
        let num = |x: &serde_json::Value| x.as_f64().map(|n| if n.fract() == 0.0 { format!("{n:.0}") } else { n.to_string() });
        let value = match (prop, &v) {
            ("createTime", serde_json::Value::Array(a)) => {
                let n = a.first().and_then(|x| x.as_f64()).unwrap_or(0.0);
                if method == "within" {
                    format!("{n:.0}")
                } else {
                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64);
                    format!("{:.0}", ((now - n) / 86_400_000.0).max(0.0))
                }
            }
            ("width" | "fileSize", serde_json::Value::Array(a)) => {
                let nums: Vec<String> = a.iter().filter_map(num).collect();
                if method == "between" { nums.join("-") } else { nums.first().cloned().unwrap_or_default() }
            }
            (_, serde_json::Value::Array(a)) => a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", "),
            ("rating", serde_json::Value::String(s)) if s == "none" => "0".into(),
            (_, serde_json::Value::String(s)) => s.clone(),
            _ => String::new(),
        };
        Some(RuleDraft { property, method, value })
    }

    /// The rule in Eagle's JSON form, or `None` when the value is unusable.
    fn to_json(&self) -> Option<serde_json::Value> {
        use serde_json::json;
        let v = self.value.trim();
        let nums = || -> Option<Vec<f64>> {
            let n: Vec<f64> = v.split(['-', ',', ' ']).filter(|x| !x.is_empty()).filter_map(|x| x.parse().ok()).collect();
            (!n.is_empty()).then_some(n)
        };
        let value = match self.property {
            "tags" => json!(v.split(',').map(str::trim).filter(|x| !x.is_empty()).collect::<Vec<_>>()),
            "width" | "fileSize" => {
                let n = nums()?;
                json!([n[0], n.get(1).copied().unwrap_or(0.0)])
            }
            "createTime" => {
                let n = nums()?;
                if self.method == "within" {
                    json!([n[0]])
                } else {
                    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as f64;
                    json!([ms - n[0] * 86_400_000.0])
                }
            }
            "rating" => json!(if v.is_empty() || v == "0" { "none".to_string() } else { v.to_string() }),
            _ if matches!(self.method, "empty" | "not-empty" | "grayscale") => json!(""),
            _ if v.is_empty() => return None,
            _ => json!(v),
        };
        let mut rule = json!({"property": self.property, "method": self.method, "value": value});
        if self.property == "fileSize" {
            rule["unit"] = json!("mb");
        }
        Some(rule)
    }
}

/// A destructive action waiting for the user to confirm it.
#[derive(Clone)]
enum Confirm {
    DeleteFolder(String),
    DeleteItems(Vec<String>),
    EmptyTrash,
    DeleteTag(String),
}

/// Uncommitted inspector text for one item.
struct EditBuf {
    id: String,
    name: String,
    notes: String,
    url: String,
    new_tag: String,
}

pub struct App {
    lib: Option<Shared>,
    /// The local HTTP API for the open library, as Eagle runs it.
    server: Option<falcon_server::Handle>,
    ctx: egui::Context,
    error: Option<String>,
    loader: Loader,
    scope: Scope,
    back: Vec<Scope>,
    forward: Vec<Scope>,
    query: String,
    sort: OrderBy,
    /// Eagle's `sortIncrease`: time keys newest first, others ascending.
    increase: bool,
    filter: Filter,
    show_filter: bool,
    /// Hex text typed into the colour filter.
    color_text: String,
    /// Keyword text typed into the Notes and URL filters.
    note_text: String,
    url_text: String,
    tile: f32,
    selected: Vec<String>,
    anchor: Option<String>,
    viewer: Option<String>,
    collapsed: HashSet<String>,
    folder_filter: String,
    show_sidebar: bool,
    show_inspector: bool,
    edit: Option<EditBuf>,
    status: Option<(String, Instant)>,
    /// Ids in grid order, refreshed every frame.
    order: Vec<String>,
    columns: usize,
    scroll_to: Option<String>,
    /// Images needed this frame, handed to the loader once at the end.
    frame_jobs: Vec<crate::loader::Job>,
    actions: Vec<Action>,
    /// Folder being renamed in the sidebar, with the text typed so far.
    renaming: Option<(String, String)>,
    confirm: Option<Confirm>,
    smart_dialog: Option<SmartDraft>,
    view: ViewState,
    /// Focus the inspector's title field next frame (F2).
    focus_title: bool,
    focus_search: bool,
    /// Tag typed into the multi-selection inspector.
    batch_tag: String,
    progress: Arc<Mutex<Progress>>,
    /// Recently opened libraries, newest first.
    recent: Vec<PathBuf>,
    /// What to open once the frame is done (it replaces the library being
    /// drawn).
    switch_to: Option<Switch>,
    /// Tag being renamed on the All Tags page, with the new name so far.
    tag_rename: Option<(String, String)>,
    tag_filter: String,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>, open_items: Vec<String>, import: Vec<PathBuf>) -> Self {
        t::install(&cc.egui_ctx);
        let mut app = App {
            lib: None,
            server: None,
            ctx: cc.egui_ctx.clone(),
            error: None,
            loader: Loader::new(&cc.egui_ctx),
            scope: Scope::All,
            back: Vec::new(),
            forward: Vec::new(),
            query: String::new(),
            sort: OrderBy::Import,
            increase: true,
            filter: Filter::default(),
            show_filter: false,
            color_text: String::new(),
            note_text: String::new(),
            url_text: String::new(),
            tile: 139.0,
            selected: Vec::new(),
            anchor: None,
            viewer: None,
            collapsed: HashSet::new(),
            folder_filter: String::new(),
            show_sidebar: true,
            show_inspector: true,
            edit: None,
            status: None,
            order: Vec::new(),
            columns: 1,
            scroll_to: None,
            frame_jobs: Vec::new(),
            actions: Vec::new(),
            renaming: None,
            confirm: None,
            smart_dialog: None,
            view: ViewState::default(),
            focus_title: false,
            focus_search: false,
            batch_tag: String::new(),
            progress: Arc::new(Mutex::new(Progress::default())),
            recent: Vec::new(),
            switch_to: None,
            tag_rename: None,
            tag_filter: String::new(),
        };
        let saved: Saved = cc.storage.and_then(|s| eframe::get_value(s, eframe::APP_KEY)).unwrap_or_default();
        app.tile = saved.tile.unwrap_or(app.tile).clamp(TILE_MIN, TILE_MAX);
        app.sort = saved.sort.as_deref().and_then(|l| OrderBy::ALL.into_iter().find(|o| o.label() == l)).unwrap_or(app.sort);
        app.increase = saved.increase.unwrap_or(app.increase);
        app.show_sidebar = saved.show_sidebar.unwrap_or(true);
        app.show_inspector = saved.show_inspector.unwrap_or(true);
        app.collapsed = saved.collapsed.into_iter().collect();
        app.recent = saved.recent.into_iter().filter(|p| p.is_dir()).collect();
        // Reopen the last library unless one was given on the command line.
        if let Some(p) = initial.or(saved.library.filter(|p| p.is_dir())) {
            app.open(p);
        }
        if !import.is_empty() {
            app.import(import);
        }
        // One item opens in the viewer; several are selected.
        if let [id] = open_items.as_slice() {
            app.viewer = Some(id.clone());
        }
        if let Some(first) = open_items.first() {
            app.scroll_to = Some(first.clone());
        }
        app.selected = open_items;
        app
    }

    fn open(&mut self, path: PathBuf) {
        match Library::open(&path) {
            Ok(lib) => {
                self.server = None; // stops the previous library's server
                let shared: Shared = Arc::new(Mutex::new(lib));
                let ctx = self.ctx.clone();
                // Never Eagle's own port: clients of a running (or later
                // started) Eagle must keep reaching Eagle.
                let ports = falcon_server::DEFAULT_PORT..falcon_server::DEFAULT_PORT + 5;
                match falcon_server::start(shared.clone(), ports, move || ctx.request_repaint()) {
                    Ok(h) => {
                        self.flash(format!("API on http://localhost:{}", h.port));
                        self.server = Some(h);
                    }
                    Err(e) => self.flash(format!("API not started: {e}")),
                }
                self.lib = Some(shared);
                self.recent.retain(|p| *p != path);
                self.recent.insert(0, path.clone());
                self.recent.truncate(10);
                self.error = None;
                self.scope = Scope::All;
                self.back.clear();
                self.forward.clear();
                self.selected.clear();
                self.edit = None;
                self.loader.clear_all();
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn new_library(&mut self) {
        let Some(dest) = rfd::FileDialog::new().set_title("Create Library").set_file_name("Untitled.library").save_file() else {
            return;
        };
        let dest = if dest.extension().is_some_and(|e| e == "library") { dest } else { dest.with_extension("library") };
        match Library::create(&dest) {
            Ok(_) => self.open(dest),
            Err(e) => self.flash(format!("Could not create the library: {e}")),
        }
    }

    fn pick_library(&mut self) {
        if let Some(p) = rfd::FileDialog::new().set_title("Open Eagle Library").pick_folder() {
            self.open(p);
        }
    }

    fn go(&mut self, scope: Scope) {
        let scope = match scope {
            Scope::Random(_) => Scope::Random(fastrand_seed()),
            s => s,
        };
        if scope == self.scope {
            return;
        }
        self.back.push(std::mem::replace(&mut self.scope, scope));
        self.forward.clear();
        self.selected.clear();
        self.anchor = None;
    }

    fn go_back(&mut self) {
        if let Some(s) = self.back.pop() {
            self.forward.push(std::mem::replace(&mut self.scope, s));
            self.selected.clear();
        }
    }

    fn go_forward(&mut self) {
        if let Some(s) = self.forward.pop() {
            self.back.push(std::mem::replace(&mut self.scope, s));
            self.selected.clear();
        }
    }

    fn flash(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), Instant::now()));
    }

    /// Applies a click on the card at `id` with the given modifiers.
    fn click(&mut self, id: &str, m: Modifiers) {
        if m.command {
            if let Some(p) = self.selected.iter().position(|s| s == id) {
                self.selected.remove(p);
            } else {
                self.selected.push(id.to_string());
            }
            self.anchor = Some(id.to_string());
        } else if m.shift
            && let Some(a) = self.anchor.as_ref().and_then(|a| self.order.iter().position(|x| x == a))
            && let Some(b) = self.order.iter().position(|x| x == id)
        {
            let (lo, hi) = (a.min(b), a.max(b));
            self.selected = self.order[lo..=hi].to_vec();
        } else {
            self.selected = vec![id.to_string()];
            self.anchor = Some(id.to_string());
        }
    }

    /// Imports files and folders on a background thread: each file is
    /// staged without the library lock, then committed under it, so the
    /// window stays responsive. Progress shows under the toolbar.
    fn import(&mut self, paths: Vec<PathBuf>) {
        let Some(shared) = self.lib.clone() else { return };
        let folders = match &self.scope {
            Scope::Folder(id) => vec![id.clone()],
            _ => Vec::new(),
        };
        let opts = ImportOptions { folders, ..Default::default() };
        let mut files = Vec::new();
        let mut stack = paths;
        while let Some(p) = stack.pop() {
            if p.is_dir() {
                if let Ok(rd) = std::fs::read_dir(&p) {
                    stack.extend(rd.flatten().map(|e| e.path()).filter(|p| !is_hidden(p)));
                }
            } else if !is_hidden(&p) {
                files.push(p);
            }
        }
        if files.is_empty() {
            return;
        }
        files.sort();
        let progress = self.progress.clone();
        {
            let mut pr = progress.lock().unwrap_or_else(|p| p.into_inner());
            pr.total += files.len();
        }
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            for f in files {
                let result = falcon_library::import::stage(&f, &opts).and_then(|staged| lock(&shared).commit_import(staged));
                let mut pr = progress.lock().unwrap_or_else(|p| p.into_inner());
                pr.done += 1;
                if result.is_err() {
                    pr.failed += 1;
                }
                pr.current = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                drop(pr);
                ctx.request_repaint();
            }
        });
    }

    /// The import progress bar; clears itself a moment after finishing.
    fn progress_bar(&mut self, ui: &mut Ui, rect: Rect) -> bool {
        let snapshot = {
            let pr = self.progress.lock().unwrap_or_else(|p| p.into_inner());
            (pr.total, pr.done, pr.failed, pr.current.clone())
        };
        let (total, done, failed, current) = snapshot;
        if total == 0 {
            return false;
        }
        if done >= total {
            let mut pr = self.progress.lock().unwrap_or_else(|p| p.into_inner());
            *pr = Progress::default();
            drop(pr);
            let msg = if failed > 0 { format!("Added {}, {failed} failed", done - failed) } else { format!("Added {done}") };
            self.flash(msg);
            return false;
        }
        let p = ui.painter();
        p.rect_filled(rect, 0.0, t::MAIN);
        let track = Rect::from_min_max(Pos2::new(rect.left() + GRID_PAD_X, rect.bottom() - 6.0), Pos2::new(rect.right() - GRID_PAD_X, rect.bottom() - 3.0));
        p.rect_filled(track, CornerRadius::same(2), t::BUTTON);
        let f = done as f32 / total as f32;
        let fill = Rect::from_min_max(track.min, Pos2::new(track.left() + track.width() * f, track.bottom()));
        p.rect_filled(fill, CornerRadius::same(2), t::PRIMARY);
        let text = format!("Importing {done} of {total}  ·  {current}");
        let g = galley(ui, &text, font(12.0), t::TEXT_MUTED, rect.width() - 40.0);
        p.galley(Pos2::new(rect.left() + GRID_PAD_X, rect.top() + 3.0), g, t::TEXT_MUTED);
        true
    }

    /// ⌘C: puts the selected originals on the clipboard as files.
    fn copy_selection(&mut self) {
        let Some(shared) = self.lib.clone() else { return };
        let paths: Vec<PathBuf> = {
            let lib = lock(&shared);
            self.selected.iter().filter_map(|id| lib.item(id)).map(|i| i.original_path(lib.root())).collect()
        };
        if let Ok(mut clip) = arboard::Clipboard::new()
            && clip.set().file_list(&paths).is_ok()
        {
            self.flash(format!("Copied {} file{}", paths.len(), if paths.len() == 1 { "" } else { "s" }));
        }
    }

    /// ⌘V in the grid: imports copied files, or a copied image as a PNG.
    fn paste(&mut self) {
        let Ok(mut clip) = arboard::Clipboard::new() else { return };
        if let Ok(files) = clip.get().file_list()
            && !files.is_empty()
        {
            self.import(files);
            return;
        }
        if let Ok(img) = clip.get_image() {
            let Some(buf) = image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.into_owned()) else { return };
            let Ok(dir) = tempfile_dir() else { return };
            let stamp = chrono::Local::now().format("%Y-%m-%d at %H.%M.%S");
            let path = dir.join(format!("Pasted image {stamp}.png"));
            if buf.save(&path).is_ok() {
                self.import(vec![path.clone()]);
            }
            let _ = std::fs::remove_file(path);
        }
    }

    fn keyboard(&mut self, ctx: &egui::Context) {
        if ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        let (f2, find, back, fwd, zin, zout, newf, copy) = ctx.input(|i| {
            let c = i.modifiers.command;
            (
                i.key_pressed(Key::F2),
                c && i.key_pressed(Key::F),
                c && i.key_pressed(Key::OpenBracket),
                c && i.key_pressed(Key::CloseBracket),
                c && (i.key_pressed(Key::Equals) || i.key_pressed(Key::Plus)),
                c && i.key_pressed(Key::Minus),
                c && i.modifiers.shift && i.key_pressed(Key::N),
                c && i.key_pressed(Key::C),
            )
        });
        if self.viewer.is_none() {
            if f2 && self.selected.len() == 1 {
                self.show_inspector = true;
                self.focus_title = true;
            }
            if find {
                self.focus_search = true;
            }
            if back {
                self.go_back();
            }
            if fwd {
                self.go_forward();
            }
            if zin {
                self.tile = (self.tile + 20.0).min(TILE_MAX);
            }
            if zout {
                self.tile = (self.tile - 20.0).max(TILE_MIN);
            }
            if newf {
                let parent = match &self.scope {
                    Scope::Folder(id) => Some(id.clone()),
                    _ => None,
                };
                self.actions.push(Action::CreateFolder { parent });
            }
            if copy && !self.selected.is_empty() {
                self.copy_selection();
            }
        }
        let paste = ctx.input(|i| {
            i.events.iter().any(|e| matches!(e, egui::Event::Paste(_)))
                || (i.modifiers.command && i.key_pressed(Key::V))
        });
        if paste && self.viewer.is_none() {
            self.paste();
            return;
        }
        let (left, right, up, down, space, esc, all, del) = ctx.input(|i| {
            (
                i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::ArrowRight),
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::Space) || i.key_pressed(Key::Enter),
                i.key_pressed(Key::Escape),
                i.modifiers.command && i.key_pressed(Key::A),
                i.modifiers.command && i.key_pressed(Key::Backspace),
            )
        });
        if esc {
            if self.viewer.is_some() {
                self.viewer = None;
            } else {
                self.selected.clear();
            }
        }
        if all {
            self.selected = self.order.clone();
        }
        if space && self.viewer.is_some() {
            self.viewer = None;
            return;
        }
        if space && self.viewer.is_none() && self.selected.len() == 1 {
            self.viewer = self.selected.first().cloned();
            return;
        }
        if del && !self.selected.is_empty() {
            let ids = self.selected.clone();
            if self.scope == Scope::Trash {
                self.confirm = Some(Confirm::DeleteItems(ids));
            } else {
                self.actions.push(Action::Trash(ids, true));
            }
        }
        let step: isize = if self.viewer.is_some() {
            match (left || up, right || down) {
                (true, _) => -1,
                (_, true) => 1,
                _ => 0,
            }
        } else {
            let c = self.columns as isize;
            match (left, right, up, down) {
                (true, ..) => -1,
                (_, true, ..) => 1,
                (_, _, true, _) => -c,
                (.., true) => c,
                _ => 0,
            }
        };
        if step != 0 && !self.order.is_empty() {
            let cur = self.viewer.as_ref().or(self.selected.last());
            let pos = cur.and_then(|c| self.order.iter().position(|x| x == c));
            let next = match pos {
                Some(p) => (p as isize + step).clamp(0, self.order.len() as isize - 1) as usize,
                None => 0,
            };
            let id = self.order[next].clone();
            if self.viewer.is_some() {
                self.viewer = Some(id.clone());
            }
            self.selected = vec![id.clone()];
            self.anchor = Some(id.clone());
            self.scroll_to = Some(id);
        }
    }
}

impl App {
    fn apply_actions(&mut self, lib: &mut Library) {
        for action in std::mem::take(&mut self.actions) {
            let result = match action {
                Action::CreateFolder { parent } => lib.create_folder(parent.as_deref(), "New Folder").map(|id| {
                    if let Some(p) = parent {
                        self.collapsed.remove(&p);
                    }
                    self.renaming = Some((id, "New Folder".into()));
                }),
                Action::RenameFolder(id, name) => lib.rename_folder(&id, &name),
                Action::DeleteFolder(id) => {
                    if self.scope == Scope::Folder(id.clone()) {
                        self.scope = Scope::All;
                    }
                    lib.delete_folder(&id)
                }
                Action::AddToFolder(ids, fid) => {
                    let n = ids.len();
                    let mut r = Ok(());
                    for id in ids {
                        if let Some(mut item) = lib.item(&id).cloned()
                            && !item.folders.contains(&fid)
                        {
                            item.folders.push(fid.clone());
                            item.is_deleted = false;
                            r = r.and(lib.update_item(item));
                        }
                    }
                    let name = logic::find_folder(&lib.metadata.folders, &fid).map_or_else(String::new, |f| f.name.clone());
                    self.flash(format!("Added {n} to {name}"));
                    r
                }
                Action::CreateSmart(name, conditions) => lib.create_smart_folder(&name, conditions).map(|id| {
                    self.scope = Scope::Smart(id);
                    self.selected.clear();
                }),
                Action::UpdateSmart(id, name, conditions) => lib.update_smart_folder(&id, &name, conditions),
                Action::DeleteSmart(id) => {
                    if self.scope == Scope::Smart(id.clone()) {
                        self.scope = Scope::All;
                    }
                    lib.delete_smart_folder(&id)
                }
                Action::MoveFolder(id, parent) => {
                    if let Some(p) = &parent {
                        self.collapsed.remove(p);
                    }
                    lib.move_folder(&id, parent.as_deref())
                }
                Action::BatchTag(ids, add, remove) => {
                    let mut r = Ok(());
                    for id in &ids {
                        let Some(mut item) = lib.item(id).cloned() else { continue };
                        let before = item.tags.clone();
                        if let Some(t) = &remove {
                            item.tags.retain(|x| x != t);
                        }
                        if let Some(t) = &add
                            && !item.tags.contains(t)
                        {
                            item.tags.push(t.clone());
                        }
                        if item.tags != before {
                            r = r.and(lib.update_item(item));
                        }
                    }
                    if let Some(t) = add {
                        r = r.and(lib.record_tag_use(&[t]));
                    }
                    r
                }
                Action::BatchRating(ids, star) => {
                    let mut r = Ok(());
                    for id in &ids {
                        let Some(mut item) = lib.item(id).cloned() else { continue };
                        if star == 0 {
                            item.extra.remove("star");
                        } else {
                            item.extra.insert("star".into(), star.into());
                        }
                        r = r.and(lib.update_item(item));
                    }
                    r
                }
                Action::QuickAdd(kind, id) => lib.add_quick_access(kind, &id),
                Action::QuickRemove(id) => lib.remove_quick_access(&id),
                Action::RenameTag(from, to) => lib.rename_tag(&from, &to).map(|n| {
                    if self.scope == Scope::Tag(from.clone()) {
                        self.scope = Scope::Tag(to.clone());
                    }
                    self.flash(format!("Renamed “{from}” to “{to}” on {n} items"));
                }),
                Action::DeleteTag(tag) => lib.delete_tag(&tag).map(|n| self.flash(format!("Removed “{tag}” from {n} items"))),
                Action::StarTag(tag, on) => lib.set_tag_starred(&tag, on),
                Action::DeletePermanently(ids) => {
                    self.selected.retain(|s| !ids.contains(s));
                    let r = lib.delete_permanently(&ids);
                    self.flash(format!("Deleted {}", ids.len()));
                    r
                }
                Action::EmptyTrash => lib.empty_trash().map(|n| {
                    self.selected.clear();
                    self.flash(format!("Deleted {n}"));
                }),
                Action::Trash(ids, trash) => {
                    self.selected.retain(|s| !ids.contains(s));
                    let r = lib.set_trashed(&ids, trash);
                    self.flash(if trash { format!("Moved {} to Trash", ids.len()) } else { format!("Restored {}", ids.len()) });
                    r
                }
            };
            if let Err(e) = result {
                self.flash(format!("Could not save: {e}"));
            }
        }
    }
}

type Shared = falcon_server::Shared;

fn lock(s: &Shared) -> std::sync::MutexGuard<'_, Library> {
    s.lock().unwrap_or_else(|p| p.into_inner())
}

/// A private folder for clipboard images before they are imported.
fn tempfile_dir() -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join("falcon-paste");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn fastrand_seed() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64)
}

// ---------------------------------------------------------------------------
// Small painting helpers

fn galley(ui: &Ui, text: &str, f: egui::FontId, color: Color32, max_w: f32) -> Arc<Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_string(), f, color);
    job.wrap.max_width = max_w.max(1.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    ui.painter().layout_job(job)
}

fn icon(ui: &Ui, center: Pos2, glyph: &str, size: f32, color: Color32) {
    ui.painter().text(center, Align2::CENTER_CENTER, glyph, font(size), color);
}

/// A bare icon button, as in Eagle's toolbars.
fn icon_button(ui: &mut Ui, center: Pos2, glyph: &str, size: f32, enabled: bool, tip: &str) -> Response {
    let rect = Rect::from_center_size(center, Vec2::splat(26.0));
    let resp = ui.interact(rect, ui.scope_id().with(("icon", glyph, center.x as i32)), Sense::click());
    let color = if !enabled {
        t::TEXT_DISABLED
    } else if resp.hovered() {
        t::TEXT
    } else {
        Color32::from_gray(222)
    };
    if enabled && resp.hovered() {
        ui.painter().rect_filled(rect, t::RADIUS, t::ROW_HOVER);
    }
    icon(ui, center, glyph, size, color);
    if tip.is_empty() { resp } else { resp.on_hover_text(tip) }
}

/// Makes `rect` drag the window, and double-click zoom it, like a title bar.
fn window_drag(ui: &Ui, rect: Rect, id: &str) {
    let resp = ui.interact(rect, ui.scope_id().with(("drag", id)), Sense::click_and_drag());
    if resp.drag_started() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    if resp.double_clicked() {
        let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
    }
}

/// A rounded input box with Eagle's field styling.
fn field(ui: &mut Ui, value: &mut String, hint: &str, height: f32, multiline: bool, id: &str) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, t::FIELD_RADIUS, t::FIELD);
    p.rect_stroke(rect, t::FIELD_RADIUS, Stroke::new(1.0, t::FIELD_BORDER), StrokeKind::Inside);
    let inner = rect.shrink2(vec2(11.0, if multiline { 8.0 } else { 0.0 }));
    let edit = if multiline { TextEdit::multiline(value) } else { TextEdit::singleline(value) };
    let edit = edit
        .id_salt(id)
        .frame(egui::Frame::NONE)
        .margin(egui::Margin::ZERO)
        .hint_text(RichText::new(hint).color(t::TEXT_MUTED))
        .font(font(14.0))
        .text_color(t::TEXT)
        .desired_width(inner.width())
        .align(if multiline { Align2::LEFT_TOP } else { Align2::LEFT_CENTER });
    ui.put(inner, edit)
}

/// A file-type label like Eagle's: translucent type colour, uppercase.
fn badge(ui: &Ui, top_left: Pos2, ext: &str) {
    let Some(c) = t::ext_color(ext) else { return };
    let g = galley(ui, &ext.to_uppercase(), font(10.0), Color32::from_white_alpha(204), 60.0);
    let rect = Rect::from_min_size(top_left, vec2(g.size().x + 6.0, 18.0));
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(4), c.gamma_multiply(0.6));
    p.rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.0, c.gamma_multiply(0.5)), StrokeKind::Inside);
    p.galley(Pos2::new(rect.left() + 3.0, rect.center().y - g.size().y / 2.0), g, Color32::WHITE);
}

// ---------------------------------------------------------------------------
// Sidebar

struct SidebarCtx<'a> {
    folder_counts: &'a HashMap<String, usize>,
    nav: &'a NavCounts,
    /// Ids pinned to Quick Access.
    quick: &'a HashSet<String>,
}

impl App {
    fn sidebar(&mut self, ui: &mut Ui, lib: &Library) {
        let full = ui.max_rect();
        let p = ui.painter().clone();
        p.rect_filled(full, 0.0, t::PANEL);
        p.vline(full.right() - 0.5, full.y_range(), Stroke::new(1.0, t::DIVIDER));

        // Header: traffic lights on the left, actions on the right.
        let header = Rect::from_min_size(full.min, vec2(full.width(), t::BAR_HEIGHT));
        window_drag(ui, header, "sidebar");
        let cy = header.center().y;
        let r = full.right();
        if icon_button(ui, Pos2::new(r - 24.0, cy), ic::SIDEBAR_SIMPLE, 18.0, true, "Hide sidebar").clicked() {
            self.show_sidebar = false;
        }
        if icon_button(ui, Pos2::new(r - 51.0, cy), ic::ARROWS_LEFT_RIGHT, 18.0, true, "Switch Library").clicked() {
            self.switch_to = Some(Switch::Pick);
        }
        if icon_button(ui, Pos2::new(r - 79.0, cy), ic::PLUS, 18.0, true, "Create Folder").clicked() {
            self.actions.push(Action::CreateFolder { parent: None });
        }

        // Library switcher.
        let lib_row = Rect::from_min_size(Pos2::new(full.left(), header.bottom()), vec2(full.width(), 30.0));
        let resp = ui.interact(lib_row, ui.scope_id().with("library"), Sense::click());
        let ic_rect = Rect::from_center_size(Pos2::new(full.left() + 29.0, lib_row.center().y), Vec2::splat(22.0));
        p.rect_filled(ic_rect, CornerRadius::same(5), t::BRAND);
        icon(ui, ic_rect.center(), ic::FOLDER, 14.0, Color32::WHITE);
        let name = lib.root().file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let g = galley(ui, &name, font(15.0), t::TEXT, full.width() - 80.0);
        let gw = g.size().x;
        p.galley(Pos2::new(full.left() + 44.0, lib_row.center().y - g.size().y / 2.0), g, t::TEXT);
        icon(ui, Pos2::new(full.left() + 44.0 + gw + 10.0, lib_row.center().y), ic::CARET_UP_DOWN, 12.0, t::TEXT);
        let current = lib.root().to_path_buf();
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(240.0);
            ui.label(RichText::new("Libraries").color(t::TEXT_MUTED).size(12.0));
            for p in self.recent.clone() {
                let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let mark = if p == current { ic::CHECK } else { " " };
                if ui.button(format!("{mark}  {name}")).on_hover_text(p.display().to_string()).clicked() && p != current {
                    self.switch_to = Some(Switch::Open(p.clone()));
                }
            }
            ui.separator();
            if ui.button("Open Library…").clicked() {
                self.switch_to = Some(Switch::Pick);
            }
            if ui.button("Create Library…").clicked() {
                self.switch_to = Some(Switch::Create);
            }
            if ui.button("Show in Finder").clicked() {
                let _ = std::process::Command::new("open").arg("-R").arg(&current).spawn();
            }
        });

        // Filter box at the bottom.
        let filter_rect = Rect::from_min_max(
            Pos2::new(full.left() + 11.0, full.bottom() - 37.0),
            Pos2::new(full.right() - 11.0, full.bottom() - 11.0),
        );

        let list_rect = Rect::from_min_max(Pos2::new(full.left(), lib_row.bottom() + 6.0), Pos2::new(full.right() - 1.0, filter_rect.top() - 8.0));
        let folder_counts = logic::folder_counts(lib.items());
        let nav = logic::nav_counts(lib.items());
        let quick: HashSet<String> =
            lib.metadata.quick_access.iter().filter_map(|q| q.get("id").and_then(|v| v.as_str()).map(String::from)).collect();
        let cx = SidebarCtx { folder_counts: &folder_counts, nav: &nav, quick: &quick };
        let mut list = ui.new_child(egui::UiBuilder::new().max_rect(list_rect));
        egui::ScrollArea::vertical().id_salt("sidebar-list").auto_shrink([false, false]).show(&mut list, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            self.nav_rows(ui, &cx);
            ui.add_space(12.0);
            if !lib.metadata.quick_access.is_empty() {
                let _ = section(ui, "Quick Access");
                let smart = lib.smart_folders();
                for q in &lib.metadata.quick_access {
                    let (Some(kind), Some(id)) = (q.get("type").and_then(|v| v.as_str()), q.get("id").and_then(|v| v.as_str())) else {
                        continue;
                    };
                    let (glyph, name, scope, count) = if kind == "smartFolder" {
                        let Some(sf) = smart.iter().find(|s| s.id == id) else { continue };
                        (ic::FOLDER_STAR, sf.name.clone(), Scope::Smart(id.to_string()), lib.smart_folder_items(id).len())
                    } else {
                        let Some(f) = logic::find_folder(&lib.metadata.folders, id) else { continue };
                        (ic::FOLDER, f.name.clone(), Scope::Folder(id.to_string()), cx.folder_counts.get(id).copied().unwrap_or(0))
                    };
                    let active = self.scope == scope;
                    let resp = sidebar_row(ui, 0, glyph, &name, (count > 0).then_some(count), active, None);
                    if resp.clicked() {
                        self.go(scope);
                    }
                    resp.context_menu(|ui| {
                        if ui.button("Remove from Quick Access").clicked() {
                            self.actions.push(Action::QuickRemove(id.to_string()));
                        }
                    });
                }
                ui.add_space(8.0);
            }
            if section_with_add(ui, "Smart Folders", "Create Smart Folder") {
                self.smart_dialog = Some(SmartDraft::default());
            }
            for sf in lib.smart_folders() {
                let count = lib.smart_folder_items(&sf.id).len();
                let active = self.scope == Scope::Smart(sf.id.clone());
                let resp = sidebar_row(ui, 0, ic::FOLDER_STAR, &sf.name, (count > 0).then_some(count), active, None);
                if resp.clicked() {
                    self.go(Scope::Smart(sf.id.clone()));
                }
                let pinned = quick.contains(&sf.id);
                resp.context_menu(|ui| {
                    if ui.button("Edit…").clicked() {
                        self.smart_dialog = Some(SmartDraft::from_folder(&sf));
                    }
                    if pinned {
                        if ui.button("Remove from Quick Access").clicked() {
                            self.actions.push(Action::QuickRemove(sf.id.clone()));
                        }
                    } else if ui.button("Add to Quick Access").clicked() {
                        self.actions.push(Action::QuickAdd("smartFolder", sf.id.clone()));
                    }
                    if ui.button(RichText::new("Delete Smart Folder").color(Color32::from_rgb(0xff, 0x66, 0x67))).clicked() {
                        self.actions.push(Action::DeleteSmart(sf.id.clone()));
                    }
                });
            }
            let total = lib.all_folders().len();
            let header = section(ui, &format!("Folders ({total})"));
            if header.dnd_hover_payload::<DragFolder>().is_some() {
                ui.painter().rect_stroke(header.rect.shrink2(vec2(11.0, 1.0)), t::RADIUS, Stroke::new(1.5, t::PRIMARY), StrokeKind::Inside);
            }
            if let Some(d) = header.dnd_release_payload::<DragFolder>() {
                self.actions.push(Action::MoveFolder(d.0.clone(), None));
            }
            let filter = self.folder_filter.trim().to_lowercase();
            for f in &lib.metadata.folders {
                self.folder_row(ui, f, 0, &cx, &filter);
            }
            ui.add_space(8.0);
        });

        p.rect_filled(filter_rect, t::RADIUS, t::FIELD);
        p.rect_stroke(filter_rect, t::RADIUS, Stroke::new(1.0, t::FIELD_BORDER), StrokeKind::Inside);
        icon(ui, Pos2::new(filter_rect.left() + 15.0, filter_rect.center().y), ic::FUNNEL, 15.0, t::TEXT_MUTED);
        let inner = Rect::from_min_max(Pos2::new(filter_rect.left() + 30.0, filter_rect.top()), filter_rect.max);
        ui.put(
            inner,
            TextEdit::singleline(&mut self.folder_filter)
                .id_salt("folder-filter")
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .hint_text(RichText::new("Filter").color(t::TEXT_MUTED))
                .font(font(13.0))
                .align(Align2::LEFT_CENTER)
                .desired_width(inner.width() - 8.0),
        );
    }

    fn nav_rows(&mut self, ui: &mut Ui, cx: &SidebarCtx) {
        let n = cx.nav;
        let entries: [(&str, &str, Scope, usize); 7] = [
            (ic::IMAGES, "All", Scope::All, n.all),
            (ic::FOLDER_DASHED, "Uncategorized", Scope::Uncategorized, n.uncategorized),
            (ic::TAG_SIMPLE, "Untagged", Scope::Untagged, n.untagged),
            (ic::CLOCK, "Recently Used", Scope::Recent, 0),
            (ic::SHUFFLE, "Random", Scope::Random(0), 0),
            (ic::BOOKMARK_SIMPLE, "All Tags", Scope::AllTags, n.tags),
            (ic::TRASH, "Trash", Scope::Trash, n.trash),
        ];
        for (glyph, label, scope, count) in entries {
            let active = match (&scope, &self.scope) {
                (Scope::Random(_), Scope::Random(_)) => true,
                (a, b) => a == b,
            };
            let resp = sidebar_row(ui, 0, glyph, label, (count > 0).then_some(count), active, None);
            if scope == Scope::Trash {
                resp.context_menu(|ui| {
                    if ui.add_enabled(n.trash > 0, egui::Button::new("Empty Trash")).clicked() {
                        self.confirm = Some(Confirm::EmptyTrash);
                    }
                });
                drop_highlight(ui, &resp, 0);
                if let Some(d) = resp.dnd_release_payload::<DragItems>() {
                    self.actions.push(Action::Trash(d.0.clone(), true));
                }
            }
            if resp.clicked() {
                self.go(scope);
            }
        }
    }

    /// One folder and, unless collapsed, its children. Returns whether
    /// anything was drawn (filtered folders are hidden with their subtree
    /// unless a descendant matches).
    fn folder_row(&mut self, ui: &mut Ui, f: &Folder, depth: usize, cx: &SidebarCtx, filter: &str) -> Option<Rect> {
        if !filter.is_empty() && !subtree_matches(f, filter) {
            return None;
        }
        let has_children = !f.children.is_empty();
        let open = has_children && (!filter.is_empty() || !self.collapsed.contains(&f.id));
        let active = self.scope == Scope::Folder(f.id.clone());
        let count = cx.folder_counts.get(&f.id).copied();
        let glyph = if open { ic::FOLDER_OPEN } else { ic::FOLDER };
        let renaming = self.renaming.as_ref().is_some_and(|(id, _)| *id == f.id);
        let label = if renaming { "" } else { f.name.as_str() };
        let resp = sidebar_row(ui, depth, glyph, label, count, active, has_children.then_some(open));
        let row_rect = resp.rect;
        if renaming {
            let r = Rect::from_min_max(
                Pos2::new(row_rect.left() + icon_x(depth) + 12.0, row_rect.top() + 3.0),
                Pos2::new(row_rect.right() - 14.0, row_rect.bottom() - 3.0),
            );
            ui.painter().rect_filled(r, CornerRadius::same(4), t::FIELD);
            ui.painter().rect_stroke(r, CornerRadius::same(4), Stroke::new(1.0, t::PRIMARY), StrokeKind::Inside);
            let (_, buf) = self.renaming.as_mut().unwrap();
            let te = ui.put(
                r.shrink2(vec2(4.0, 0.0)),
                TextEdit::singleline(buf)
                    .id_salt(("rename", &f.id))
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::ZERO)
                    .font(font(13.5))
                    .align(Align2::LEFT_CENTER),
            );
            if !te.has_focus() && !te.lost_focus() {
                te.request_focus();
            }
            let cancel = ui.input(|i| i.key_pressed(Key::Escape));
            if te.lost_focus() || cancel {
                let (id, name) = self.renaming.take().unwrap();
                let name = name.trim().to_string();
                if !cancel && !name.is_empty() && name != f.name {
                    self.actions.push(Action::RenameFolder(id, name));
                }
            }
        }
        if resp.drag_started() {
            resp.dnd_set_drag_payload(DragFolder(f.id.clone()));
        }
        let dragging_self = resp.dnd_hover_payload::<DragFolder>().is_some_and(|d| d.0 == f.id);
        if !dragging_self {
            drop_highlight(ui, &resp, depth);
        }
        if let Some(d) = resp.dnd_release_payload::<DragItems>() {
            self.actions.push(Action::AddToFolder(d.0.clone(), f.id.clone()));
        }
        if let Some(d) = resp.dnd_release_payload::<DragFolder>()
            && d.0 != f.id
        {
            self.actions.push(Action::MoveFolder(d.0.clone(), Some(f.id.clone())));
        }
        let pinned = cx.quick.contains(&f.id);
        resp.context_menu(|ui| {
            ui.set_min_width(180.0);
            if ui.button("New Subfolder").clicked() {
                self.actions.push(Action::CreateFolder { parent: Some(f.id.clone()) });
            }
            if ui.button("Rename").clicked() {
                self.renaming = Some((f.id.clone(), f.name.clone()));
            }
            if pinned {
                if ui.button("Remove from Quick Access").clicked() {
                    self.actions.push(Action::QuickRemove(f.id.clone()));
                }
            } else if ui.button("Add to Quick Access").clicked() {
                self.actions.push(Action::QuickAdd("folder", f.id.clone()));
            }
            if depth > 0 && ui.button("Move to Top Level").clicked() {
                self.actions.push(Action::MoveFolder(f.id.clone(), None));
            }
            ui.separator();
            if ui.button(RichText::new("Delete Folder").color(Color32::from_rgb(0xff, 0x66, 0x67))).clicked() {
                self.confirm = Some(Confirm::DeleteFolder(f.id.clone()));
            }
        });
        if resp.clicked() {
            let caret = caret_rect(row_rect, depth);
            if has_children && resp.interact_pointer_pos().is_some_and(|p| caret.contains(p)) {
                if !self.collapsed.remove(&f.id) {
                    self.collapsed.insert(f.id.clone());
                }
            } else {
                self.go(Scope::Folder(f.id.clone()));
            }
        }
        if resp.double_clicked() && has_children && !self.collapsed.remove(&f.id) {
            self.collapsed.insert(f.id.clone());
        }
        if open {
            let mut last_center = None;
            for c in &f.children {
                if let Some(r) = self.folder_row(ui, c, depth + 1, cx, filter) {
                    last_center = Some(r.center().y);
                }
            }
            if let Some(end) = last_center {
                // Guide line from this folder's icon down to its last child.
                let x = row_rect.left() + icon_x(depth);
                let p = ui.painter();
                let top = row_rect.bottom() - 3.0;
                p.line_segment([Pos2::new(x, top), Pos2::new(x, end - 5.0)], Stroke::new(1.0, t::GUIDE));
                p.line_segment([Pos2::new(x, end - 5.0), Pos2::new(x + 5.0, end)], Stroke::new(1.0, t::GUIDE));
            }
        }
        Some(row_rect)
    }
}

/// Outlines a sidebar row while grid items are dragged over it.
fn drop_highlight(ui: &Ui, resp: &Response, depth: usize) {
    if resp.dnd_hover_payload::<DragItems>().is_some() || resp.dnd_hover_payload::<DragFolder>().is_some() {
        let r = resp.rect;
        let bg = Rect::from_min_max(Pos2::new(r.left() + icon_x(depth) - 14.0, r.top() + 0.5), Pos2::new(r.right() - 11.0, r.bottom() - 0.5));
        ui.painter().rect_filled(bg, t::RADIUS, t::PRIMARY.gamma_multiply(0.25));
        ui.painter().rect_stroke(bg, t::RADIUS, Stroke::new(1.5, t::PRIMARY), StrokeKind::Inside);
    }
}

fn subtree_matches(f: &Folder, filter: &str) -> bool {
    f.name.to_lowercase().contains(filter) || f.children.iter().any(|c| subtree_matches(c, filter))
}

/// Horizontal centre of a row's icon at `depth`.
fn icon_x(depth: usize) -> f32 {
    29.0 + 20.0 * depth as f32
}

fn caret_rect(row: Rect, depth: usize) -> Rect {
    Rect::from_center_size(Pos2::new(row.left() + icon_x(depth) - 19.0, row.center().y), vec2(16.0, row.height()))
}

fn smart_name(lib: &Library, id: &str) -> String {
    let mut name = String::from("Smart Folder");
    for sf in lib.smart_folders() {
        sf.walk(&mut Vec::new(), &mut |f, _| {
            if f.id == id {
                name = f.name.clone();
            }
        });
    }
    name
}

/// A section header with a + button that shows on hover, like Eagle's.
fn section_with_add(ui: &mut Ui, title: &str, tip: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 25.0), Sense::hover());
    ui.painter().text(Pos2::new(rect.left() + 20.0, rect.center().y), Align2::LEFT_CENTER, title, font(13.0), t::TEXT_MUTED);
    let hovered = resp.hovered() || ui.rect_contains_pointer(rect);
    hovered && icon_button(ui, Pos2::new(rect.right() - 24.0, rect.center().y), ic::PLUS, 15.0, true, tip).clicked()
}

fn section(ui: &mut Ui, title: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 25.0), Sense::hover());
    ui.painter().text(
        Pos2::new(rect.left() + 20.0, rect.center().y),
        Align2::LEFT_CENTER,
        title,
        font(13.0),
        t::TEXT_MUTED,
    );
    resp
}

fn sidebar_row(
    ui: &mut Ui,
    depth: usize,
    glyph: &str,
    label: &str,
    count: Option<usize>,
    active: bool,
    caret: Option<bool>,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), t::ROW_HEIGHT), Sense::click_and_drag());
    let ix = rect.left() + icon_x(depth);
    let bg = Rect::from_min_max(Pos2::new(ix - 14.0, rect.top() + 0.5), Pos2::new(rect.right() - 11.0, rect.bottom() - 0.5));
    let p = ui.painter();
    if active {
        p.rect_filled(bg, t::RADIUS, t::ROW_SELECTED);
    } else if resp.hovered() {
        p.rect_filled(bg, t::RADIUS, t::ROW_HOVER);
    }
    if let Some(open) = caret {
        // A small solid triangle, pointing down when open.
        let c = caret_rect(rect, depth).center();
        let pts = if open {
            vec![c + vec2(-4.5, -2.5), c + vec2(4.5, -2.5), c + vec2(0.0, 3.0)]
        } else {
            vec![c + vec2(-2.5, -4.5), c + vec2(3.0, 0.0), c + vec2(-2.5, 4.5)]
        };
        p.add(egui::Shape::convex_polygon(pts, t::TEXT_MUTED, Stroke::NONE));
    }
    p.text(Pos2::new(ix, rect.center().y), Align2::CENTER_CENTER, glyph, font(18.0), t::TEXT);
    let count_w = if count.is_some() { 30.0 } else { 0.0 };
    let g = galley(ui, label, font(13.5), t::TEXT, rect.right() - 20.0 - count_w - (ix + 15.0));
    p.galley(Pos2::new(ix + 15.0, rect.center().y - g.size().y / 2.0), g, t::TEXT);
    if let Some(n) = count {
        p.text(Pos2::new(rect.right() - 19.0, rect.center().y), Align2::RIGHT_CENTER, n.to_string(), font(12.0), t::TEXT_MUTED);
    }
    resp
}

// ---------------------------------------------------------------------------
// Toolbar

impl App {
    fn toolbar(&mut self, ui: &mut Ui, lib: &Library) {
        let bar = ui.max_rect();
        window_drag(ui, bar, "toolbar");
        let cy = bar.center().y;
        let mut x = bar.left() + 22.0;
        if !self.show_sidebar {
            x += 72.0; // room for the traffic lights
            if icon_button(ui, Pos2::new(x, cy), ic::SIDEBAR_SIMPLE, 18.0, true, "Show sidebar").clicked() {
                self.show_sidebar = true;
            }
            x += 30.0;
        }
        if icon_button(ui, Pos2::new(x, cy), ic::CARET_LEFT, 17.0, !self.back.is_empty(), "").clicked() {
            self.go_back();
        }
        if icon_button(ui, Pos2::new(x + 27.0, cy), ic::CARET_RIGHT, 17.0, !self.forward.is_empty(), "").clicked() {
            self.go_forward();
        }
        let title = if let Scope::Smart(id) = &self.scope {
            smart_name(lib, id)
        } else if self.query.trim().is_empty() {
            self.scope.title(&lib.metadata.folders)
        } else {
            "Search results".into()
        };
        let g = galley(ui, &title, font(15.0), t::TEXT, 220.0);
        let title_end = x + 45.0 + g.size().x;
        ui.painter().galley(Pos2::new(x + 45.0, cy - g.size().y / 2.0), g, t::TEXT);

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

        egui::Popup::menu(&layout).show(|ui| self.layout_menu(ui));
        if filter.clicked() {
            self.show_filter = !self.show_filter;
        }
        if self.show_filter || self.filter.is_active() {
            let r = Rect::from_center_size(filter.rect.center(), Vec2::splat(26.0));
            ui.painter().rect_stroke(r, t::RADIUS, Stroke::new(1.0, t::PRIMARY), StrokeKind::Inside);
        }

        // Zoom slider between the title and the icons.
        let zx = (bar.left() + 237.0).max(title_end + 30.0);
        if zx + 130.0 < icons_left {
            self.zoom_slider(ui, Pos2::new(zx, cy));
        }
        ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, t::MAIN));
    }

    fn search_field(&mut self, ui: &mut Ui, rect: Rect) {
        let p = ui.painter();
        p.rect_filled(rect, t::FIELD_RADIUS, t::SEARCH);
        p.rect_stroke(rect, t::FIELD_RADIUS, Stroke::new(1.0, t::FIELD_BORDER), StrokeKind::Inside);
        icon(ui, Pos2::new(rect.left() + 14.0, rect.center().y), ic::MAGNIFYING_GLASS, 16.0, t::TEXT);
        icon(ui, Pos2::new(rect.left() + 28.0, rect.center().y + 1.0), ic::CARET_DOWN, 9.0, t::TEXT_MUTED);
        icon(ui, Pos2::new(rect.right() - 17.0, rect.center().y), ic::CAMERA, 17.0, t::TEXT_DISABLED);
        let inner = Rect::from_min_max(Pos2::new(rect.left() + 40.0, rect.top()), Pos2::new(rect.right() - 32.0, rect.bottom()));
        let resp = ui.put(
            inner,
            TextEdit::singleline(&mut self.query)
                .id_salt("search")
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .hint_text(RichText::new("Search").color(t::TEXT_MUTED))
                .font(font(14.0))
                .align(Align2::LEFT_CENTER)
                .desired_width(inner.width()),
        );
        if std::mem::take(&mut self.focus_search) {
            resp.request_focus();
        }
    }

    fn zoom_slider(&mut self, ui: &mut Ui, left_center: Pos2) {
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
        p.line_segment([Pos2::new(track.left(), y), Pos2::new(track.right(), y)], Stroke::new(2.0, Color32::from_gray(70)));
        let kx = track.left() + f * track.width();
        p.line_segment([Pos2::new(track.left(), y), Pos2::new(kx, y)], Stroke::new(2.0, Color32::from_gray(200)));
        p.circle_filled(Pos2::new(kx, y), 7.0, Color32::WHITE);
    }

    fn layout_menu(&mut self, ui: &mut Ui) {
        ui.set_min_width(190.0);
        ui.label(RichText::new("Sort by").color(t::TEXT_MUTED).size(12.0));
        for k in OrderBy::ALL {
            let mark = if k == self.sort { ic::CHECK } else { " " };
            if ui.button(format!("{mark}  {}", k.label())).clicked() {
                self.sort = k;
            }
        }
        ui.separator();
        let labels = if self.sort.is_time() { ["Newest First", "Oldest First"] } else { ["Ascending", "Descending"] };
        for (inc, label) in [(true, labels[0]), (false, labels[1])] {
            let mark = if inc == self.increase { ic::CHECK } else { " " };
            if ui.button(format!("{mark}  {label}")).clicked() {
                self.increase = inc;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Filter bar

/// Colours offered in the colour filter, like Eagle's swatch row.
const PRESET_COLORS: [[u8; 3]; 14] = [
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

fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// A filter chip: muted when off, primary-tinted with its value when on.
fn filter_chip(ui: &mut Ui, label: &str, value: Option<String>) -> Response {
    let text = match &value {
        Some(v) => format!("{label}: {v}"),
        None => label.to_string(),
    };
    let on = value.is_some();
    let g = galley(ui, &text, font(13.0), t::TEXT, 220.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 30.0, 26.0), Sense::click());
    let fill = if on {
        t::PRIMARY.gamma_multiply(0.22)
    } else if resp.hovered() {
        t::BUTTON_HOVER
    } else {
        t::BUTTON
    };
    ui.painter().rect_filled(rect, t::RADIUS, fill);
    if on {
        ui.painter().rect_stroke(rect, t::RADIUS, Stroke::new(1.0, t::PRIMARY.gamma_multiply(0.7)), StrokeKind::Inside);
    }
    let color = if on { t::TEXT } else { t::TEXT_VALUE };
    ui.painter().galley(Pos2::new(rect.left() + 10.0, rect.center().y - g.size().y / 2.0), g, color);
    icon(ui, Pos2::new(rect.right() - 11.0, rect.center().y), ic::CARET_DOWN, 10.0, t::TEXT_MUTED);
    resp
}

fn logic_picker(ui: &mut Ui, logic: &mut Logic) {
    ui.horizontal(|ui| {
        for (l, name) in [(Logic::Or, "Any"), (Logic::And, "All"), (Logic::Equal, "Exact")] {
            if ui.selectable_label(*logic == l, name).clicked() {
                *logic = l;
            }
        }
    });
}

fn text_filter_menu(ui: &mut Ui, f: &mut TextFilter, keywords: &mut String) {
    ui.set_min_width(200.0);
    for (opt, name) in [(TextFilter::Off, "Any"), (TextFilter::Has, "Has"), (TextFilter::No, "None")] {
        let on = std::mem::discriminant(f) == std::mem::discriminant(&opt);
        if ui.selectable_label(on, name).clicked() {
            *f = opt;
        }
    }
    ui.separator();
    ui.label(RichText::new("Contains").color(t::TEXT_MUTED).size(12.0));
    if ui.add(TextEdit::singleline(keywords).hint_text("keyword, keyword")).changed() {
        let words: Vec<String> = keywords.split(',').map(|w| w.trim().to_string()).filter(|w| !w.is_empty()).collect();
        *f = if words.is_empty() { TextFilter::Off } else { TextFilter::Keywords(words) };
    }
}

impl App {
    fn filter_bar(&mut self, ui: &mut Ui, lib: &Library) {
        let r = ui.max_rect();
        ui.painter().hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, t::DIVIDER));
        let inner = r.shrink2(vec2(GRID_PAD_X, 7.0));
        let mut row = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let ui = &mut row;
        ui.spacing_mut().item_spacing.x = 6.0;
        let f = &mut self.filter;

        let color_val = f.color.map(|c| logic::hex(c.rgb));
        let resp = filter_chip(ui, "Color", color_val);
        let color_text = &mut self.color_text;
        egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(230.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for c in PRESET_COLORS {
                    let (rect, hit) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
                    ui.painter().circle_filled(rect.center(), 10.0, Color32::from_rgb(c[0], c[1], c[2]));
                    ui.painter().circle_stroke(rect.center(), 10.0, Stroke::new(1.0, Color32::from_white_alpha(40)));
                    if f.color.is_some_and(|q| q.rgb == c) {
                        ui.painter().circle_stroke(rect.center(), 12.5, Stroke::new(1.5, t::PRIMARY));
                    }
                    if hit.clicked() {
                        let acc = f.color.map_or(20.0, |q| q.accuracy);
                        f.color = Some(ColorQuery { rgb: c, accuracy: acc });
                        *color_text = logic::hex(c);
                    }
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Hex");
                if ui.add(TextEdit::singleline(color_text).desired_width(90.0).hint_text("#FF0000")).changed()
                    && let Some(rgb) = parse_hex(color_text)
                {
                    let acc = f.color.map_or(20.0, |q| q.accuracy);
                    f.color = Some(ColorQuery { rgb, accuracy: acc });
                }
            });
            if let Some(q) = f.color.as_mut() {
                ui.horizontal(|ui| {
                    ui.label("Accuracy");
                    // Eagle draws the slider reversed: right is stricter.
                    let mut strict = 45.0 - q.accuracy;
                    ui.add(egui::Slider::new(&mut strict, 5.0..=40.0).show_value(false));
                    q.accuracy = 45.0 - strict;
                });
            }
            ui.checkbox(&mut f.grayscale, "Grayscale only");
            if ui.button("Clear").clicked() {
                f.color = None;
                f.grayscale = false;
                color_text.clear();
            }
        });

        let tags = logic::tag_counts(lib.items());
        let tv = (!f.tags.includes.is_empty() || f.tags.empty).then(|| {
            let n = f.tags.includes.len() + usize::from(f.tags.empty);
            if n == 1 { f.tags.includes.first().cloned().unwrap_or_else(|| "Untagged".into()) } else { format!("{n}") }
        });
        let resp = filter_chip(ui, "Tags", tv);
        egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(220.0);
            logic_picker(ui, &mut f.tags.logic);
            ui.separator();
            ui.checkbox(&mut f.tags.empty, "Untagged");
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for (tag, n) in &tags {
                    let mut on = f.tags.includes.contains(tag);
                    if ui.checkbox(&mut on, format!("{tag}  ({n})")).changed() {
                        if on {
                            f.tags.includes.push(tag.clone());
                        } else {
                            f.tags.includes.retain(|x| x != tag);
                        }
                    }
                }
            });
        });

        let fv = (!f.folders.includes.is_empty()).then(|| f.folders.includes.len().to_string());
        let resp = filter_chip(ui, "Folders", fv);
        egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(220.0);
            logic_picker(ui, &mut f.folders.logic);
            ui.separator();
            ui.checkbox(&mut f.folders.empty, "Uncategorized");
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for folder in lib.all_folders() {
                    let mut on = f.folders.includes.contains(&folder.id);
                    if ui.checkbox(&mut on, &folder.name).changed() {
                        if on {
                            f.folders.includes.push(folder.id.clone());
                        } else {
                            f.folders.includes.retain(|x| *x != folder.id);
                        }
                    }
                }
            });
        });

        let sv = (!f.shapes.is_empty()).then(|| f.shapes.len().to_string());
        let resp = filter_chip(ui, "Shape", sv);
        egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            for (shape, name) in [
                (Shape::Landscape, "Landscape"),
                (Shape::Portrait, "Portrait"),
                (Shape::Square, "Square"),
                (Shape::PanoramicLandscape, "Panoramic Landscape"),
                (Shape::PanoramicPortrait, "Panoramic Portrait"),
            ] {
                let mut on = f.shapes.contains(&shape);
                if ui.checkbox(&mut on, name).changed() {
                    if on {
                        f.shapes.insert(shape);
                    } else {
                        f.shapes.remove(&shape);
                    }
                }
            }
        });

        let rv = (!f.ratings.is_empty()).then(|| f.ratings.len().to_string());
        let resp = filter_chip(ui, "Rating", rv);
        egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            for n in (0..=5u8).rev() {
                let label = if n == 0 { "Unrated".to_string() } else { "★".repeat(n as usize) };
                let mut on = f.ratings.contains(&n);
                if ui.checkbox(&mut on, label).changed() {
                    if on {
                        f.ratings.insert(n);
                    } else {
                        f.ratings.remove(&n);
                    }
                }
            }
        });

        let mut exts: Vec<String> = lib.items().iter().map(|i| i.ext.clone()).collect::<HashSet<_>>().into_iter().collect();
        exts.sort();
        let yv = (!f.types.is_empty()).then(|| {
            if f.types.len() == 1 { f.types[0].to_uppercase() } else { f.types.len().to_string() }
        });
        let resp = filter_chip(ui, "Type", yv);
        egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            for ext in &exts {
                let mut on = f.types.contains(ext);
                if ui.checkbox(&mut on, ext.to_uppercase()).changed() {
                    if on {
                        f.types.push(ext.clone());
                    } else {
                        f.types.retain(|x| x != ext);
                    }
                }
            }
        });

        let label = |t: &TextFilter| match t {
            TextFilter::Off => None,
            TextFilter::Has => Some("Has".to_string()),
            TextFilter::No => Some("None".to_string()),
            TextFilter::Keywords(k) => Some(k.join(", ")),
        };
        let note_kw = &mut self.note_text;
        let resp = filter_chip(ui, "Notes", label(&f.note));
        egui::Popup::menu(&resp)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| text_filter_menu(ui, &mut f.note, note_kw));
        let url_kw = &mut self.url_text;
        let resp = filter_chip(ui, "URL", label(&f.url));
        egui::Popup::menu(&resp)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| text_filter_menu(ui, &mut f.url, url_kw));

        if f.is_active() {
            let clear = ui.add(egui::Button::new(RichText::new(format!("{}  Clear", ic::X)).color(t::TEXT_MUTED)).frame(false));
            if clear.clicked() {
                *f = Filter::default();
                self.color_text.clear();
                self.note_text.clear();
                self.url_text.clear();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Grid

impl App {
    fn grid(&mut self, ui: &mut Ui, lib: &Library) {
        let area = ui.max_rect();
        let only: Option<HashSet<String>> = match &self.scope {
            Scope::Smart(id) => Some(lib.smart_folder_items(id).into_iter().map(|i| i.id.clone()).collect()),
            _ => None,
        };
        let order = logic::visible(lib.items(), &lib.metadata.folders, &self.scope, &self.query, &self.filter, self.sort, self.increase, only.as_ref());
        let items: Vec<&Item> = order.iter().map(|&i| &lib.items()[i]).collect();
        self.order = items.iter().map(|i| i.id.clone()).collect();

        if self.scope == Scope::AllTags && self.query.trim().is_empty() {
            self.tag_page(ui, lib);
            return;
        }
        if items.is_empty() {
            ui.painter().text(area.center(), Align2::CENTER_CENTER, "No items", font(15.0), t::TEXT_MUTED);
            return;
        }

        let s = self.tile;
        let width = area.width() - 2.0 * GRID_PAD_X;
        let cols = logic::columns(width, s, GRID_GAP);
        self.columns = cols;
        let row_h = s + CARD_TEXT + ROW_GAP;
        let rows = items.len().div_ceil(cols);
        let total_h = GRID_PAD_TOP + rows as f32 * row_h + 20.0;

        let mut scroll = egui::ScrollArea::vertical().id_salt(("grid", format!("{:?}", self.scope))).auto_shrink([false, false]);
        if let Some(id) = self.scroll_to.take()
            && let Some(pos) = self.order.iter().position(|x| *x == id)
        {
            let row = (pos / cols) as f32;
            let top = GRID_PAD_TOP + row * row_h;
            let cur = ui.ctx().data(|d| d.get_temp::<f32>(ui.scope_id().with("grid-offset"))).unwrap_or(0.0);
            let view_h = area.height();
            if top < cur {
                scroll = scroll.vertical_scroll_offset(top);
            } else if top + row_h > cur + view_h {
                scroll = scroll.vertical_scroll_offset(top + row_h - view_h);
            }
        }

        let mut jobs = Vec::new();
        let mut clicked: Option<(String, Modifiers)> = None;
        let mut opened: Option<String> = None;
        let mut bg_clicked = false;
        let out = scroll.show_viewport(ui, |ui, viewport| {
            let origin = ui.max_rect().min;
            let bg = ui.allocate_rect(Rect::from_min_size(origin, vec2(area.width(), total_h.max(area.height()))), Sense::click());
            if bg.clicked() {
                bg_clicked = true;
            }
            let first = ((viewport.min.y - GRID_PAD_TOP) / row_h).floor().max(0.0) as usize;
            let last = (((viewport.max.y - GRID_PAD_TOP) / row_h).ceil() as usize + 1).min(rows);
            for r in first..last {
                for c in 0..cols {
                    let n = r * cols + c;
                    let Some(item) = items.get(n) else { break };
                    let tl = origin + vec2(GRID_PAD_X + c as f32 * (s + GRID_GAP), GRID_PAD_TOP + r as f32 * row_h);
                    let card = Rect::from_min_size(tl, vec2(s, s + CARD_TEXT));
                    let resp = ui.interact(card, ui.scope_id().with(("card", &item.id)), Sense::click_and_drag());
                    if resp.drag_started() {
                        let ids = if self.selected.contains(&item.id) { self.selected.clone() } else { vec![item.id.clone()] };
                        resp.dnd_set_drag_payload(DragItems(ids));
                    }
                    if resp.secondary_clicked() && !self.selected.contains(&item.id) {
                        self.selected = vec![item.id.clone()];
                    }
                    let scope_is_trash = self.scope == Scope::Trash;
                    let targets: Vec<String> = if self.selected.contains(&item.id) { self.selected.clone() } else { vec![item.id.clone()] };
                    resp.context_menu(|ui| {
                        ui.set_min_width(200.0);
                        if ui.button("Open").clicked() {
                            opened = Some(item.id.clone());
                        }
                        if ui.button("Show in Finder").clicked() {
                            let _ = std::process::Command::new("open").arg("-R").arg(item.original_path(lib.root())).spawn();
                        }
                        if ui.button("Copy File Path").clicked() {
                            ui.ctx().copy_text(item.original_path(lib.root()).display().to_string());
                        }
                        ui.menu_button("Add to Folder", |ui| {
                            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                                for f in lib.all_folders() {
                                    if ui.button(&f.name).clicked() {
                                        self.actions.push(Action::AddToFolder(targets.clone(), f.id.clone()));
                                    }
                                }
                            });
                        });
                        ui.separator();
                        if scope_is_trash {
                            if ui.button("Restore").clicked() {
                                self.actions.push(Action::Trash(targets.clone(), false));
                            }
                            if ui.button(RichText::new("Delete Permanently").color(Color32::from_rgb(0xff, 0x66, 0x67))).clicked() {
                                self.confirm = Some(Confirm::DeleteItems(targets.clone()));
                            }
                        } else if ui.button("Move to Trash").clicked() {
                            self.actions.push(Action::Trash(targets.clone(), true));
                        }
                    });
                    if resp.clicked() {
                        clicked = Some((item.id.clone(), ui.input(|i| i.modifiers)));
                    }
                    if resp.double_clicked() {
                        opened = Some(item.id.clone());
                    }
                    let selected = self.selected.contains(&item.id);
                    self.card(ui, lib, item, card, s, selected, resp.hovered());
                    if let Some(j) = job_for(item, lib.root(), Tier::Grid) {
                        jobs.push(j);
                    }
                }
            }
        });
        ui.ctx().data_mut(|d| d.insert_temp(ui.scope_id().with("grid-offset"), out.state.clamped_offset().y));
        self.frame_jobs.extend(jobs);
        if let Some((id, m)) = clicked {
            self.click(&id, m);
        } else if bg_clicked {
            self.selected.clear();
        }
        if let Some(id) = opened {
            self.viewer = Some(id);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn card(&mut self, ui: &Ui, lib: &Library, item: &Item, card: Rect, s: f32, selected: bool, hovered: bool) {
        let tile = Rect::from_min_size(card.min, Vec2::splat(s));
        let p = ui.painter();
        p.rect_filled(tile, t::RADIUS, t::MAIN);
        let border = if hovered { t::CARD_HOVER } else { t::CARD_BORDER };
        p.rect_stroke(tile, t::RADIUS, Stroke::new(1.0, border), StrokeKind::Inside);

        let inner = tile.shrink(6.0);
        let failed = self.loader.has_failed(&item.id, Tier::Grid);
        match self.loader.get(&item.id, Tier::Grid).cloned() {
            Some(tex) => {
                let tex_size = tex.size_vec2();
                // Eagle's object-fit: scale-down never enlarges a small image.
                let natural = if item.width > 0 && item.height > 0 {
                    vec2(item.width as f32, item.height as f32)
                } else {
                    tex_size
                };
                let scale = (inner.width() / natural.x).min(inner.height() / natural.y).min(1.0);
                let size = natural * scale;
                let r = Rect::from_center_size(inner.center(), size);
                egui::Image::new((tex.id(), size)).corner_radius(4).paint_at(ui, r);
            }
            None if failed || job_for(item, lib.root(), Tier::Grid).is_none() => {
                ext_placeholder(ui, inner, &item.ext);
            }
            None => {}
        }
        badge(ui, tile.min + vec2(5.0, 5.0), &item.ext);
        if selected {
            p.rect_stroke(tile.expand(1.0), CornerRadius::same(7), Stroke::new(2.5, t::PRIMARY), StrokeKind::Inside);
        }

        let name = format!("{}.{}", item.name, item.ext);
        let g = galley(ui, &name, font(13.0), t::TEXT, s);
        let y = tile.bottom() + 14.0;
        if selected {
            let pill = Rect::from_center_size(Pos2::new(tile.center().x, y), vec2(g.size().x + 10.0, 19.0));
            p.rect_filled(pill, CornerRadius::same(4), t::PRIMARY);
        }
        p.galley(Pos2::new(tile.center().x - g.size().x / 2.0, y - g.size().y / 2.0), g, t::TEXT);
        if item.width > 0 && item.height > 0 {
            p.text(
                Pos2::new(tile.center().x, tile.bottom() + 32.0),
                Align2::CENTER_CENTER,
                format!("{} × {}", item.width, item.height),
                font(12.0),
                t::TEXT_MUTED,
            );
        }
    }

    fn tag_page(&mut self, ui: &mut Ui, lib: &Library) {
        let filter = self.tag_filter.trim().to_lowercase();
        let tags: Vec<(String, usize)> =
            logic::tag_counts(lib.items()).into_iter().filter(|(t, _)| filter.is_empty() || t.to_lowercase().contains(&filter)).collect();
        let starred: Vec<(String, usize)> =
            lib.tags.starred_tags.iter().filter_map(|s| tags.iter().find(|(t, _)| t == s).cloned()).collect();
        egui::ScrollArea::vertical().id_salt("tags-page").auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.add_space(GRID_PAD_X);
                let (rect, _) = ui.allocate_exact_size(vec2(240.0, 30.0), Sense::hover());
                ui.painter().rect_filled(rect, t::FIELD_RADIUS, t::SEARCH);
                ui.painter().rect_stroke(rect, t::FIELD_RADIUS, Stroke::new(1.0, t::FIELD_BORDER), StrokeKind::Inside);
                icon(ui, Pos2::new(rect.left() + 14.0, rect.center().y), ic::MAGNIFYING_GLASS, 15.0, t::TEXT_MUTED);
                let inner = Rect::from_min_max(Pos2::new(rect.left() + 30.0, rect.top()), rect.max - vec2(8.0, 0.0));
                ui.put(
                    inner,
                    TextEdit::singleline(&mut self.tag_filter)
                        .id_salt("tag-filter")
                        .frame(egui::Frame::NONE)
                        .margin(egui::Margin::ZERO)
                        .hint_text(RichText::new("Filter tags").color(t::TEXT_MUTED))
                        .font(font(13.0))
                        .align(Align2::LEFT_CENTER),
                );
                ui.add_space(12.0);
                ui.label(RichText::new(format!("{} tags", tags.len())).color(t::TEXT_MUTED));
            });
            ui.add_space(14.0);

            let mut groups: Vec<(String, Vec<(String, usize)>)> = Vec::new();
            if !starred.is_empty() {
                groups.push((format!("{}  Starred", ic::STAR), starred.clone()));
            }
            let mut current: Option<char> = None;
            for (tag, n) in tags {
                let first = tag.chars().next().map_or('#', |c| c.to_uppercase().next().unwrap_or(c));
                let first = if first.is_alphabetic() { first } else { '#' };
                if current != Some(first) {
                    groups.push((first.to_string(), Vec::new()));
                    current = Some(first);
                }
                groups.last_mut().unwrap().1.push((tag, n));
            }
            for (heading, group) in groups {
                ui.horizontal(|ui| {
                    ui.add_space(GRID_PAD_X + 2.0);
                    ui.label(RichText::new(heading).size(14.0).color(t::TEXT_MUTED));
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

    fn tag_chip(&mut self, ui: &mut Ui, lib: &Library, tag: &str, n: usize) {
        let renaming = self.tag_rename.as_ref().is_some_and(|(t, _)| t == tag);
        if renaming {
            let (_, buf) = self.tag_rename.as_mut().unwrap();
            let te = ui.add(TextEdit::singleline(buf).id_salt(("tag-rename", tag)).desired_width(140.0).font(font(13.0)));
            if !te.has_focus() && !te.lost_focus() {
                te.request_focus();
            }
            let cancel = ui.input(|i| i.key_pressed(Key::Escape));
            if te.lost_focus() || cancel {
                let (from, to) = self.tag_rename.take().unwrap();
                if !cancel && !to.trim().is_empty() && to.trim() != from {
                    self.actions.push(Action::RenameTag(from, to.trim().to_string()));
                }
            }
            return;
        }
        let name_g = galley(ui, tag, font(13.0), t::TEXT, 300.0);
        let count_g = galley(ui, &n.to_string(), font(12.0), t::TEXT_MUTED, 80.0);
        let size = vec2(name_g.size().x + count_g.size().x + 30.0, 28.0);
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        let fill = if resp.hovered() { t::BUTTON_HOVER } else { t::BUTTON };
        ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
        let cy = rect.center().y;
        ui.painter().galley(Pos2::new(rect.left() + 11.0, cy - name_g.size().y / 2.0), name_g.clone(), t::TEXT);
        ui.painter().galley(Pos2::new(rect.right() - 11.0 - count_g.size().x, cy - count_g.size().y / 2.0), count_g, t::TEXT_MUTED);
        if resp.clicked() {
            self.go(Scope::Tag(tag.to_string()));
        }
        let starred = lib.tags.starred_tags.iter().any(|s| s == tag);
        resp.context_menu(|ui| {
            ui.set_min_width(180.0);
            if ui.button("Rename…").clicked() {
                self.tag_rename = Some((tag.to_string(), tag.to_string()));
            }
            if ui.button(if starred { "Unstar" } else { "Star" }).clicked() {
                self.actions.push(Action::StarTag(tag.to_string(), !starred));
            }
            ui.separator();
            if ui.button(RichText::new("Delete Tag").color(Color32::from_rgb(0xff, 0x66, 0x67))).clicked() {
                self.confirm = Some(Confirm::DeleteTag(tag.to_string()));
            }
        });
        resp.on_hover_text("Click to show items · right-click to rename, star or delete. Renaming to an existing tag merges them.");
    }
}

fn ext_placeholder(ui: &Ui, rect: Rect, ext: &str) {
    let s = rect.width().min(rect.height()) * 0.42;
    let r = Rect::from_center_size(rect.center(), vec2(s * 0.8, s));
    icon(ui, r.center(), ic::FILE, s, Color32::from_gray(90));
    ui.painter().text(
        r.center() + vec2(0.0, s * 0.12),
        Align2::CENTER_CENTER,
        ext.to_uppercase(),
        font((s * 0.18).max(9.0)),
        Color32::from_gray(150),
    );
}

// ---------------------------------------------------------------------------
// Inspector

impl App {
    fn inspector(&mut self, ui: &mut Ui, lib: &mut Library) {
        let full = ui.max_rect();
        let p = ui.painter().clone();
        p.rect_filled(full, 0.0, t::PANEL);
        p.vline(full.left() + 0.5, full.y_range(), Stroke::new(1.0, t::DIVIDER));
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
            match self.selected.len() {
                0 => self.scope_inspector(ui, lib),
                1 => {
                    let id = self.selected[0].clone();
                    if let Some(item) = lib.item(&id).cloned() {
                        self.item_inspector(ui, lib, item);
                    }
                }
                n => self.multi_inspector(ui, lib, n),
            }
        });
    }

    fn scope_inspector(&mut self, ui: &mut Ui, lib: &Library) {
        let folder = match &self.scope {
            Scope::Folder(id) => logic::find_folder(&lib.metadata.folders, id).cloned(),
            _ => None,
        };
        let mut name = self.scope.title(&lib.metadata.folders);
        let mut desc = folder.as_ref().map(|f| f.description.clone()).unwrap_or_default();
        field(ui, &mut name, "Name", 33.0, false, "scope-name");
        if folder.is_some() {
            field(ui, &mut desc, "Description", 33.0, false, "scope-desc");
        }
        divider(ui);
        let items: Vec<&Item> = self.order.iter().filter_map(|id| lib.item(id)).collect();
        let size: u64 = items.iter().map(|i| i.size).sum();
        heading(ui, "Properties");
        prop(ui, "Items", &items.len().to_string());
        prop(ui, "Size", &logic::format_size(size));
        if let Some(f) = &folder {
            prop(ui, "Date Imported", &logic::format_ms(f.modification_time));
            prop(ui, "Password", if f.password.is_empty() { "None" } else { "Set" });
        }
        ui.add_space(4.0);
        wide_button(ui, ic::EXPORT, "Export", false);
    }

    fn multi_inspector(&mut self, ui: &mut Ui, lib: &Library, n: usize) {
        let items: Vec<&Item> = self.selected.iter().filter_map(|id| lib.item(id)).collect();
        let ids: Vec<String> = items.iter().map(|i| i.id.clone()).collect();
        let size: u64 = items.iter().map(|i| i.size).sum();

        // A fanned stack of the first three previews, like Eagle's.
        let w = ui.available_width();
        let (stack, _) = ui.allocate_exact_size(vec2(w, 150.0), Sense::hover());
        for (k, item) in items.iter().take(3).enumerate().rev() {
            let off = (k as f32 - 1.0) * 14.0;
            let r = Rect::from_center_size(stack.center() + vec2(off, -off * 0.4), Vec2::splat(112.0));
            ui.painter().rect_filled(r, CornerRadius::same(6), t::FIELD);
            ui.painter().rect_stroke(r, CornerRadius::same(6), Stroke::new(1.0, t::FIELD_BORDER), StrokeKind::Inside);
            if let Some(tex) = self.loader.get(&item.id, Tier::Grid).cloned() {
                let ts = tex.size_vec2();
                let sc = ((r.width() - 10.0) / ts.x).min((r.height() - 10.0) / ts.y);
                let ir = Rect::from_center_size(r.center(), ts * sc);
                egui::Image::new((tex.id(), ir.size())).corner_radius(4).paint_at(ui, ir);
            }
            if let Some(j) = job_for(item, lib.root(), Tier::Grid) {
                self.frame_jobs.push(j);
            }
        }
        ui.vertical_centered(|ui| ui.label(RichText::new(format!("{n} items selected")).size(15.0)));

        // Tags every selected item shares; adding applies to all.
        heading(ui, "Tags");
        let mut common: Vec<String> = items.first().map(|i| i.tags.clone()).unwrap_or_default();
        common.retain(|t| items.iter().all(|i| i.tags.contains(t)));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for tag in &common {
                if chip(ui, tag, true).clicked() {
                    self.actions.push(Action::BatchTag(ids.clone(), None, Some(tag.clone())));
                }
            }
            let buf = &mut self.batch_tag;
            let r = ui.add(
                TextEdit::singleline(buf)
                    .id_salt("batch-tag")
                    .hint_text(RichText::new("Add tag to all").color(t::TEXT_MUTED))
                    .font(font(13.0))
                    .frame(egui::Frame::NONE)
                    .desired_width(120.0),
            );
            if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !buf.trim().is_empty() {
                self.actions.push(Action::BatchTag(ids.clone(), Some(buf.trim().to_string()), None));
                buf.clear();
                r.request_focus();
            }
        });

        heading(ui, "Folders");
        let add = ui.add(egui::Button::new(RichText::new(format!("{}  Add All to Folder", ic::PLUS)).size(12.0).color(t::TEXT_MUTED)).frame(false));
        egui::Popup::menu(&add).show(|ui| {
            ui.set_min_width(200.0);
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for f in lib.all_folders() {
                    if ui.button(&f.name).clicked() {
                        self.actions.push(Action::AddToFolder(ids.clone(), f.id.clone()));
                    }
                }
            });
        });

        divider(ui);
        heading(ui, "Properties");
        let stars: HashSet<u64> = items.iter().map(|i| i.extra.get("star").and_then(|v| v.as_u64()).unwrap_or(0)).collect();
        let shown = if stars.len() == 1 { stars.into_iter().next().unwrap_or(0) } else { 0 };
        if let Some(star) = rating(ui, shown) {
            self.actions.push(Action::BatchRating(ids.clone(), star));
        }
        prop(ui, "Items", &n.to_string());
        prop(ui, "Size", &logic::format_size(size));
        ui.add_space(4.0);
        let trash = self.scope == Scope::Trash;
        let label = if trash { "Restore All" } else { "Move All to Trash" };
        if wide_button(ui, ic::TRASH, label, true).clicked() {
            self.actions.push(Action::Trash(ids, !trash));
        }
    }

    fn item_inspector(&mut self, ui: &mut Ui, lib: &mut Library, item: Item) {
        if self.edit.as_ref().is_none_or(|e| e.id != item.id) {
            self.edit = Some(EditBuf {
                id: item.id.clone(),
                name: item.name.clone(),
                notes: item.annotation.clone(),
                url: item.url.clone(),
                new_tag: String::new(),
            });
        }
        let w = ui.available_width();

        // Preview.
        let aspect = if item.width > 0 && item.height > 0 { item.height as f32 / item.width as f32 } else { 1.0 };
        let ph = (w * aspect).clamp(80.0, 260.0);
        let (prect, presp) = ui.allocate_exact_size(vec2(w, ph), Sense::click());
        if let Some(tex) = self.loader.get(&item.id, Tier::Preview) {
            let ts = if item.width > 0 { vec2(item.width as f32, item.height as f32) } else { tex.size_vec2() };
            let scale = (prect.width() / ts.x).min(prect.height() / ts.y).min(1.0);
            let r = Rect::from_center_size(prect.center(), ts * scale);
            egui::Image::new((tex.id(), r.size())).corner_radius(4).paint_at(ui, r);
        } else if !thumbnail::is_supported(&item.ext) && item.thumbnail_path(lib.root()).is_none() {
            ext_placeholder(ui, prect, &item.ext);
        }
        if presp.double_clicked() {
            self.viewer = Some(item.id.clone());
        }
        if let Some(j) = job_for(&item, lib.root(), Tier::Preview) {
            self.frame_jobs.push(j);
        }

        // Palette.
        if !item.palettes.is_empty() {
            let d = 16.0;
            let (row, _) = ui.allocate_exact_size(vec2(w, d), Sense::hover());
            for (n, c) in item.palettes.iter().enumerate() {
                let center = Pos2::new(row.left() + d / 2.0 + n as f32 * (d + 6.0), row.center().y);
                if center.x > row.right() {
                    break;
                }
                let color = Color32::from_rgb(c.color[0], c.color[1], c.color[2]);
                ui.painter().circle_filled(center, d / 2.0, color);
                ui.painter().circle_stroke(center, d / 2.0, Stroke::new(1.0, Color32::from_white_alpha(25)));
                let hit = ui.interact(Rect::from_center_size(center, Vec2::splat(d)), ui.scope_id().with(("swatch", n)), Sense::click());
                let hex = logic::hex(c.color);
                if hit.clicked() {
                    self.filter.color = Some(ColorQuery::new(c.color));
                    self.color_text = hex.clone();
                    self.show_filter = true;
                }
                if hit.secondary_clicked() {
                    ui.ctx().copy_text(hex.clone());
                    self.flash(format!("Copied {hex}"));
                }
                hit.on_hover_text(format!("{hex}  ·  {}%\nClick to find similar colours, right-click to copy", c.ratio));
            }
        }

        let mut changed: Option<Item> = None;
        let edit = self.edit.as_mut().unwrap();
        let r = field(ui, &mut edit.name, "Title", 33.0, false, "item-name");
        if std::mem::take(&mut self.focus_title) {
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
        if r.lost_focus() && edit.url != item.url {
            let mut it = changed.clone().unwrap_or_else(|| item.clone());
            it.url = edit.url.trim().to_string();
            changed = Some(it);
        }

        // Tags.
        heading(ui, "Tags");
        let mut remove_tag = None;
        let mut add_tag = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for tag in &item.tags {
                if chip(ui, tag, true).clicked() {
                    remove_tag = Some(tag.clone());
                }
            }
            let te = TextEdit::singleline(&mut edit.new_tag)
                .id_salt("new-tag")
                .hint_text(RichText::new("New tag").color(t::TEXT_MUTED))
                .font(font(13.0))
                .frame(egui::Frame::NONE)
                .desired_width(90.0);
            let r = ui.add(te);
            if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !edit.new_tag.trim().is_empty() {
                add_tag = Some(edit.new_tag.trim().to_string());
                edit.new_tag.clear();
                r.request_focus();
            }
        });
        if let Some(tag) = remove_tag {
            let mut it = changed.clone().unwrap_or_else(|| item.clone());
            it.tags.retain(|x| *x != tag);
            changed = Some(it);
        }
        if let Some(tag) = add_tag.clone()
            && !item.tags.contains(&tag)
        {
            let mut it = changed.clone().unwrap_or_else(|| item.clone());
            it.tags.push(tag);
            changed = Some(it);
        }

        // Folders.
        heading(ui, "Folders");
        let mut remove_folder = None;
        let mut add_folder = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for fid in &item.folders {
                let name = logic::find_folder(&lib.metadata.folders, fid).map_or_else(|| fid.clone(), |f| f.name.clone());
                if chip(ui, &name, true).clicked() {
                    remove_folder = Some(fid.clone());
                }
            }
            let add = ui.add(egui::Button::new(RichText::new(format!("{}  Add Category", ic::PLUS)).size(12.0).color(t::TEXT_MUTED)).frame(false));
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
        if let Some(fid) = remove_folder {
            let mut it = changed.clone().unwrap_or_else(|| item.clone());
            it.folders.retain(|x| *x != fid);
            changed = Some(it);
        }
        if let Some(fid) = add_folder {
            let mut it = changed.clone().unwrap_or_else(|| item.clone());
            it.folders.push(fid);
            changed = Some(it);
        }

        divider(ui);
        heading(ui, "Properties");
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
        prop(ui, "Type", &item.ext.to_uppercase());
        prop(ui, "Date Imported", &logic::format_ms(item.modification_time));
        prop(ui, "Date Created", &logic::format_ms(item.btime));
        prop(ui, "Date Modified", &logic::format_ms(item.mtime));
        ui.add_space(4.0);
        if wide_button(ui, ic::EXPORT, "Show in Finder", true).clicked() {
            let _ = std::process::Command::new("open").arg("-R").arg(item.original_path(lib.root())).spawn();
        }

        if let Some(it) = changed {
            match lib.update_item(it) {
                Ok(()) => {
                    if let Some(tag) = add_tag {
                        let _ = lib.record_tag_use(&[tag]);
                    }
                    if let Some(e) = self.edit.as_mut() {
                        e.name = lib.item(&item.id).map_or(e.name.clone(), |i| i.name.clone());
                    }
                }
                Err(e) => self.flash(format!("Could not save: {e}")),
            }
        }
    }
}

impl App {
    fn smart_dialog_ui(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.smart_dialog.take() else { return };
        let mut keep = true;
        let modal = egui::Modal::new(egui::Id::unique("smart-dialog")).show(ctx, |ui| {
            ui.set_width(520.0);
            ui.label(RichText::new(if d.editing.is_some() { "Edit Smart Folder" } else { "New Smart Folder" }).size(16.0));
            ui.add_space(6.0);
            ui.add(TextEdit::singleline(&mut d.name).hint_text("Name").desired_width(f32::INFINITY));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Match");
                if ui.selectable_label(!d.any, "all rules").clicked() {
                    d.any = false;
                }
                if ui.selectable_label(d.any, "any rule").clicked() {
                    d.any = true;
                }
            });
            ui.add_space(4.0);
            if d.rules.is_empty() {
                d.rules.push(RuleDraft::default());
            }
            let mut remove = None;
            for (n, r) in d.rules.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    let label = SMART_PROPS.iter().find(|p| p.0 == r.property).map_or("", |p| p.1);
                    egui::ComboBox::from_id_salt(("prop", n)).selected_text(label).width(130.0).show_ui(ui, |ui| {
                        for (key, name, methods) in SMART_PROPS {
                            if ui.selectable_label(r.property == key, name).clicked() {
                                r.property = key;
                                r.method = methods[0];
                            }
                        }
                    });
                    let methods = SMART_PROPS.iter().find(|p| p.0 == r.property).map_or(&[][..], |p| p.2);
                    egui::ComboBox::from_id_salt(("method", n)).selected_text(r.method).width(110.0).show_ui(ui, |ui| {
                        for m in methods {
                            if ui.selectable_label(r.method == *m, *m).clicked() {
                                r.method = m;
                            }
                        }
                    });
                    let hint = match r.property {
                        "tags" => "tag, tag",
                        "type" => "png, video, font…",
                        "rating" => "1–5 or 0",
                        "shape" => "landscape, square…",
                        "color" => "#FF0000",
                        "createTime" => "days",
                        "width" | "fileSize" => "number or a-b",
                        _ => "text",
                    };
                    ui.add(TextEdit::singleline(&mut r.value).hint_text(hint).desired_width(170.0));
                    if ui.button(ic::X).clicked() {
                        remove = Some(n);
                    }
                });
            }
            if let Some(n) = remove {
                d.rules.remove(n);
            }
            if ui.button(format!("{}  Add Rule", ic::PLUS)).clicked() {
                d.rules.push(RuleDraft::default());
            }
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let rules: Vec<serde_json::Value> = d.rules.iter().filter_map(RuleDraft::to_json).collect();
                let ok = !d.name.trim().is_empty() && !rules.is_empty();
                let label = if d.editing.is_some() { "Save" } else { "Create" };
                if ui.add_enabled(ok, egui::Button::new(label)).clicked() {
                    let cond = serde_json::json!({"match": if d.any { "OR" } else { "AND" }, "boolean": "TRUE", "rules": rules});
                    let name = d.name.trim().to_string();
                    self.actions.push(match d.editing.clone() {
                        Some(id) => Action::UpdateSmart(id, name, vec![cond]),
                        None => Action::CreateSmart(name, vec![cond]),
                    });
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
            self.smart_dialog = Some(d);
        }
    }
}

fn divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 9.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0, t::DIVIDER));
}

fn heading(ui: &mut Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
    ui.painter().text(rect.left_center(), Align2::LEFT_CENTER, text, font(13.0), t::TEXT_MUTED);
}

fn prop(ui: &mut Ui, key: &str, value: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 13.0), Sense::hover());
    let p = ui.painter();
    p.text(rect.left_center(), Align2::LEFT_CENTER, key, font(12.0), t::TEXT);
    let g = galley(ui, value, mono(12.0), t::TEXT_VALUE, rect.width() - 90.0);
    p.galley(Pos2::new(rect.left() + 90.0, rect.center().y - g.size().y / 2.0), g, t::TEXT_VALUE);
}

/// Rating row: five stars; clicking the current rating clears it.
fn rating(ui: &mut Ui, star: u64) -> Option<u64> {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 14.0), Sense::hover());
    ui.painter().text(rect.left_center(), Align2::LEFT_CENTER, "Rating", font(12.0), t::TEXT);
    let mut out = None;
    for n in 1..=5u64 {
        let c = Pos2::new(rect.left() + 90.0 + 7.0 + (n - 1) as f32 * 17.0, rect.center().y);
        let hit = ui.interact(Rect::from_center_size(c, Vec2::splat(16.0)), ui.scope_id().with(("star", n)), Sense::click());
        let on = n <= star;
        let fam = egui::FontId::new(13.0, t::fill_family());
        let color = if on { Color32::from_rgb(0xff, 0xd6, 0x0a) } else if hit.hovered() { t::TEXT_VALUE } else { t::TEXT_DISABLED };
        ui.painter().text(c, Align2::CENTER_CENTER, ic::STAR, fam, color);
        if hit.clicked() {
            out = Some(if star == n { 0 } else { n });
        }
    }
    out
}

fn chip(ui: &mut Ui, text: &str, removable: bool) -> Response {
    let g = galley(ui, text, font(12.0), t::TEXT, 180.0);
    let extra = if removable { 14.0 } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 16.0 + extra, 22.0), Sense::click());
    let fill = if resp.hovered() { t::BUTTON_HOVER } else { t::BUTTON };
    ui.painter().rect_filled(rect, CornerRadius::same(5), fill);
    ui.painter().galley(Pos2::new(rect.left() + 8.0, rect.center().y - g.size().y / 2.0), g, t::TEXT);
    if removable && resp.hovered() {
        icon(ui, Pos2::new(rect.right() - 10.0, rect.center().y), ic::X, 10.0, t::TEXT_MUTED);
    }
    resp.on_hover_text(if removable { "Click to remove" } else { "" })
}

fn wide_button(ui: &mut Ui, glyph: &str, label: &str, enabled: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
    let fill = if enabled && resp.hovered() { t::BUTTON_HOVER } else { t::BUTTON };
    ui.painter().rect_filled(rect, t::RADIUS, fill);
    let color = if enabled { t::TEXT } else { t::TEXT_MUTED };
    let g = galley(ui, label, font(13.0), color, rect.width());
    let total = 22.0 + g.size().x;
    let x = rect.center().x - total / 2.0;
    icon(ui, Pos2::new(x + 8.0, rect.center().y), glyph, 16.0, color);
    ui.painter().galley(Pos2::new(x + 22.0, rect.center().y - g.size().y / 2.0), g, color);
    resp
}

// ---------------------------------------------------------------------------
// Viewer and frame

impl App {
    fn viewer(&mut self, ctx: &egui::Context, ui: &mut Ui, lib: &Library) {
        let Some(id) = self.viewer.clone() else { return };
        let Some(item) = lib.item(&id) else {
            self.viewer = None;
            return;
        };
        if self.view.id != id {
            self.view = ViewState { id: id.clone(), ..Default::default() };
            if item.ext == "gif" {
                self.view.frames = gif_frames(ctx, &item.original_path(lib.root()));
                self.view.started = Some(Instant::now());
            }
        }
        let screen = ui.max_rect();
        egui::Area::new(egui::Id::unique("viewer")).fixed_pos(screen.min).order(egui::Order::Foreground).show(ctx, |ui| {
            let p = ui.painter().clone();
            p.rect_filled(screen, 0.0, t::MAIN);
            let bar = Rect::from_min_size(screen.min, vec2(screen.width(), t::BAR_HEIGHT));
            let area = Rect::from_min_max(Pos2::new(screen.left(), bar.bottom()), screen.max);

            // The image: a GIF frame, the full render, or the preview.
            let gif = (!self.view.frames.is_empty()).then(|| {
                let total: f32 = self.view.frames.iter().map(|f| f.1).sum();
                let mut t = self.view.started.map_or(0.0, |s| s.elapsed().as_secs_f32()) % total.max(0.01);
                let mut pick = 0;
                for (n, (_, d)) in self.view.frames.iter().enumerate() {
                    if t < *d {
                        pick = n;
                        break;
                    }
                    t -= d;
                }
                ctx.request_repaint_after(std::time::Duration::from_millis(15));
                self.view.frames[pick].0.clone()
            });
            let tex = gif
                .or_else(|| self.loader.get(&id, Tier::Full).cloned())
                .or_else(|| self.loader.get(&id, Tier::Preview).cloned());
            let natural = if item.width > 0 && item.height > 0 {
                vec2(item.width as f32, item.height as f32)
            } else {
                tex.as_ref().map_or(Vec2::splat(512.0), |t| t.size_vec2())
            };
            let fit = ((area.width() - 80.0) / natural.x).min((area.height() - 80.0) / natural.y).min(1.0);
            let scale = self.view.zoom.unwrap_or(fit);

            // Scroll or pinch zooms around the pointer; dragging pans.
            let resp = ui.interact(area, ui.scope_id().with("viewer-canvas"), Sense::click_and_drag());
            let (scroll, pinch, pointer) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta(), i.pointer.hover_pos()));
            let factor = if pinch != 1.0 { pinch } else if resp.hovered() && scroll != 0.0 { (scroll / 300.0).exp() } else { 1.0 };
            if factor != 1.0 {
                let new = (scale * factor).clamp(0.02, 32.0);
                if let Some(pos) = pointer {
                    let center = area.center() + self.view.pan;
                    let off = pos - center;
                    self.view.pan += off - off * (new / scale);
                }
                self.view.zoom = Some(new);
            }
            if resp.dragged() {
                self.view.pan += resp.drag_delta();
                if self.view.zoom.is_none() {
                    self.view.zoom = Some(scale);
                }
            }
            if resp.double_clicked() {
                self.view.zoom = None;
                self.view.pan = Vec2::ZERO;
            }
            let scale = self.view.zoom.unwrap_or(fit);
            if self.view.zoom.is_none() {
                self.view.pan = Vec2::ZERO;
            }
            let r = Rect::from_center_size(area.center() + self.view.pan, natural * scale);
            let mut clip = ui.new_child(egui::UiBuilder::new().max_rect(area));
            clip.set_clip_rect(area);
            match &tex {
                Some(tex) => egui::Image::new((tex.id(), r.size())).paint_at(&clip, r),
                None => ext_placeholder(&clip, Rect::from_center_size(area.center(), Vec2::splat(260.0)), &item.ext),
            }

            // Bar on top: close, name, zoom controls.
            p.rect_filled(bar, 0.0, t::MAIN);
            window_drag(ui, bar, "viewer");
            let cy = bar.center().y;
            if icon_button(ui, Pos2::new(bar.left() + 98.0, cy), ic::CARET_LEFT, 17.0, true, "Back (Esc)").clicked() {
                self.viewer = None;
            }
            let name = format!("{}.{}", item.name, item.ext);
            let g = galley(ui, &name, font(14.0), t::TEXT, 360.0);
            p.galley(Pos2::new(bar.left() + 120.0, cy - g.size().y / 2.0), g, t::TEXT);
            let mut x = bar.right() - 24.0;
            if icon_button(ui, Pos2::new(x, cy), ic::PLUS, 15.0, true, "Zoom In (+)").clicked() {
                self.view.zoom = Some((scale * 1.25).min(32.0));
            }
            x -= 28.0;
            p.text(Pos2::new(x, cy), Align2::CENTER_CENTER, format!("{:.0}%", scale * 100.0), mono(12.0), t::TEXT_VALUE);
            x -= 30.0;
            if icon_button(ui, Pos2::new(x, cy), ic::MINUS, 15.0, true, "Zoom Out (−)").clicked() {
                self.view.zoom = Some((scale / 1.25).max(0.02));
            }
            x -= 40.0;
            let actual = ui.put(Rect::from_center_size(Pos2::new(x, cy), vec2(46.0, 24.0)), egui::Button::new("1:1"));
            if actual.on_hover_text("Actual Size (1)").clicked() {
                self.view.zoom = Some(1.0);
                self.view.pan = Vec2::ZERO;
            }
            x -= 50.0;
            if ui.put(Rect::from_center_size(Pos2::new(x, cy), vec2(46.0, 24.0)), egui::Button::new("Fit")).on_hover_text("Zoom to Fit (0)").clicked() {
                self.view.zoom = None;
            }

            // Keys: 0 fits, 1 is actual size, + and - zoom.
            let (k0, k1, kp, km) = ui.input(|i| {
                (i.key_pressed(Key::Num0), i.key_pressed(Key::Num1), i.key_pressed(Key::Plus) || i.key_pressed(Key::Equals), i.key_pressed(Key::Minus))
            });
            if k0 {
                self.view.zoom = None;
            }
            if k1 {
                self.view.zoom = Some(1.0);
                self.view.pan = Vec2::ZERO;
            }
            if kp {
                self.view.zoom = Some((scale * 1.25).min(32.0));
            }
            if km {
                self.view.zoom = Some((scale / 1.25).max(0.02));
            }
            if let Some(j) = job_for(item, lib.root(), Tier::Full) {
                self.frame_jobs.push(j);
            }
        });
    }

    fn empty_state(&mut self, ui: &mut Ui) {
        let r = ui.max_rect();
        window_drag(ui, Rect::from_min_size(r.min, vec2(r.width(), t::BAR_HEIGHT)), "empty");
        ui.painter().rect_filled(r, 0.0, t::MAIN);
        let c = r.center();
        ui.painter().text(c - vec2(0.0, 40.0), Align2::CENTER_CENTER, "Open an Eagle library", font(18.0), t::TEXT);
        if let Some(e) = &self.error {
            ui.painter().text(c - vec2(0.0, 14.0), Align2::CENTER_CENTER, e, font(12.0), t::TEXT_MUTED);
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
        let saved = Saved {
            library: self.lib.as_ref().map(|l| lock(l).root().to_path_buf()),
            tile: Some(self.tile),
            sort: Some(self.sort.label().to_string()),
            increase: Some(self.increase),
            show_sidebar: Some(self.show_sidebar),
            show_inspector: Some(self.show_inspector),
            collapsed: self.collapsed.iter().cloned().collect(),
            recent: self.recent.clone(),
        };
        eframe::set_value(storage, eframe::APP_KEY, &saved);
    }

    fn persist_egui_memory(&self) -> bool {
        false
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if ctx.cumulative_frame_nr() == 0 {
            // Come to the front on launch, like any Mac app.
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        self.loader.poll(&ctx);

        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if !dropped.is_empty() {
            self.import(dropped);
        }
        self.keyboard(&ctx);

        let Some(shared) = self.lib.clone() else {
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| self.empty_state(ui));
            return;
        };
        let mut guard = lock(&shared);
        let lib: &mut Library = &mut guard;
        // Items can vanish under the selection (deleted here or over the API).
        if self.selected.iter().any(|id| lib.item(id).is_none()) {
            self.selected.retain(|id| lib.item(id).is_some());
        }

        if self.show_sidebar {
            egui::Panel::left("sidebar")
                .exact_size(t::SIDEBAR_WIDTH)
                .resizable(false)
                .show_separator_line(false)
                .frame(egui::Frame::NONE)
                .show(ui, |ui| self.sidebar(ui, lib));
        }
        if self.show_inspector {
            egui::Panel::right("inspector")
                .exact_size(t::INSPECTOR_WIDTH)
                .resizable(false)
                .show_separator_line(false)
                .frame(egui::Frame::NONE)
                .show(ui, |ui| self.inspector(ui, lib));
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t::MAIN)).show(ui, |ui| {
            let full = ui.max_rect();
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
            if self.progress_bar(&mut pui, pb) {
                grid_top = pb.bottom();
            }
            if self.show_filter {
                let fb = Rect::from_min_size(Pos2::new(full.left(), bar.bottom()), vec2(full.width(), 40.0));
                let mut fui = ui.new_child(egui::UiBuilder::new().max_rect(fb));
                self.filter_bar(&mut fui, lib);
                grid_top = fb.bottom();
            }
            let grid = Rect::from_min_max(Pos2::new(full.left(), grid_top), full.max);
            let mut g = ui.new_child(egui::UiBuilder::new().max_rect(grid));
            g.set_clip_rect(grid);
            self.grid(&mut g, lib);

            if let Some((msg, at)) = &self.status {
                if at.elapsed().as_secs_f32() < 3.0 {
                    let gal = galley(ui, msg, font(13.0), t::TEXT, 400.0);
                    let r = Rect::from_center_size(Pos2::new(full.center().x, full.bottom() - 30.0), gal.size() + vec2(24.0, 14.0));
                    ui.painter().rect_filled(r, CornerRadius::same(8), t::ROW_SELECTED);
                    ui.painter().galley(r.min + vec2(12.0, 7.0), gal, t::TEXT);
                    ctx.request_repaint_after(std::time::Duration::from_millis(250));
                } else {
                    self.status = None;
                }
            }
        });
        if self.viewer.is_some() {
            let mut v = ui.new_child(egui::UiBuilder::new().max_rect(ctx.content_rect()));
            self.viewer(&ctx, &mut v, lib);
        }
        if let Some(d) = egui::DragAndDrop::payload::<DragItems>(&ctx)
            && let Some(pos) = ctx.pointer_latest_pos()
        {
            let text = if d.0.len() == 1 { "1 item".to_string() } else { format!("{} items", d.0.len()) };
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("drag-ghost")));
            let g = painter.layout_no_wrap(text, font(12.0), Color32::WHITE);
            let r = Rect::from_min_size(pos + vec2(12.0, 12.0), g.size() + vec2(16.0, 8.0));
            painter.rect_filled(r, CornerRadius::same(10), t::PRIMARY);
            painter.galley(r.min + vec2(8.0, 4.0), g, Color32::WHITE);
        }
        if self.smart_dialog.is_some() {
            self.smart_dialog_ui(&ctx);
        }
        if let Some(c) = self.confirm.clone() {
            let (title, detail, label) = match &c {
                Confirm::DeleteFolder(id) => {
                    let name = logic::find_folder(&lib.metadata.folders, id).map_or_else(String::new, |f| f.name.clone());
                    (
                        format!("Delete “{name}”?"),
                        "Its subfolders are deleted too. Items left in no folder move to the Trash.".to_string(),
                        "Delete",
                    )
                }
                Confirm::DeleteItems(ids) => (
                    format!("Delete {} item{} permanently?", ids.len(), if ids.len() == 1 { "" } else { "s" }),
                    "The files are removed from the library and cannot be restored.".to_string(),
                    "Delete",
                ),
                Confirm::DeleteTag(tag) => (
                    format!("Delete the tag “{tag}”?"),
                    "It is removed from every item. The items themselves stay.".to_string(),
                    "Delete Tag",
                ),
                Confirm::EmptyTrash => {
                    let n = lib.items().iter().filter(|i| i.is_deleted).count();
                    (format!("Empty the Trash ({n})?"), "The files are removed from the library and cannot be restored.".to_string(), "Empty Trash")
                }
            };
            let modal = egui::Modal::new(egui::Id::unique("confirm")).show(&ctx, |ui| {
                ui.set_width(320.0);
                ui.label(RichText::new(title).size(15.0));
                ui.add_space(4.0);
                ui.label(RichText::new(detail).color(t::TEXT_MUTED));
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(RichText::new(label).color(Color32::from_rgb(0xff, 0x66, 0x67))).clicked() {
                        self.actions.push(match c.clone() {
                            Confirm::DeleteFolder(id) => Action::DeleteFolder(id),
                            Confirm::DeleteItems(ids) => Action::DeletePermanently(ids),
                            Confirm::EmptyTrash => Action::EmptyTrash,
                            Confirm::DeleteTag(t) => Action::DeleteTag(t),
                        });
                        self.confirm = None;
                    }
                    if ui.button("Cancel").clicked() {
                        self.confirm = None;
                    }
                });
            });
            if modal.should_close() {
                self.confirm = None;
            }
        }
        self.apply_actions(lib);
        self.loader.want(std::mem::take(&mut self.frame_jobs));
        drop(guard);
        match self.switch_to.take() {
            Some(Switch::Open(p)) => self.open(p),
            Some(Switch::Pick) => self.pick_library(),
            Some(Switch::Create) => self.new_library(),
            None => {}
        }
    }
}
