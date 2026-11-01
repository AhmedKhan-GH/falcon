//! Export and copy-out: Export to Computer for items, folders and smart
//! folders, with the replace-or-keep-both prompt and a background copy
//! whose progress shows under the toolbar. Paths and names are worked out
//! in `falcon_library::export`; this module asks, runs and reports.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui::{self, CornerRadius, Pos2, Rect, RichText, Ui};
use falcon_library::model::Item;
use falcon_library::Library;

use crate::app::App;
use crate::icons as ic;
use crate::locale::{tr, tr_key, tr_n, tr_nf, trf};
use crate::logic::{self, Scope};
use crate::theme::{self as t, font};
use crate::ui::widgets::{galley, wide_button};
use falcon_library::export::{self, Collision, Convert, Format, Plan};
use falcon_library::pack::{self, PackSource};
use crate::ui::grid::GRID_PAD_X;
use crate::ui::dialogs::progress::Outcome;

/// An export asked for from a menu or a key, handled once the frame has
/// the library (`App::export_dialogs`).
pub(crate) enum Request {
    /// Export to Computer: these items' originals.
    Items(Vec<String>),
    /// Export to Computer: a folder or smart folder as a directory.
    Tree(Target),
    /// Export to Computer: several folders, each as a directory in the
    /// one place picked.
    Trees(Vec<String>),
    /// Export as .eaglepack: these items.
    PackItems(Vec<String>),
    /// Export as .eaglepack: a folder with its subfolders, or a smart
    /// folder as one folder.
    PackTree(Target),
    /// Export to CSV: these items' metadata.
    Csv(Vec<String>),
    /// One of the Copy variants, for these items.
    Copy(CopyKind, Vec<String>),
    /// Export in a chosen format and size: open the dialog for these items.
    Format(Vec<String>),
}

/// What the Copy variants put on the clipboard (Eagle's Edit ▸ Copy …).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum CopyKind {
    /// Each original's path, one per line.
    Path,
    /// Each item's `<id>.info` folder, one per line.
    FolderPath,
    Name,
    /// Each item's local API link (Eagle's Copy Link).
    Link,
    /// Each item's `eagle://item/<id>` link (Eagle's Copy Eagle Link).
    EagleLink,
    /// The items' source URLs (falcon's own).
    SourceUrl,
    /// The first item's thumbnail as an image; several items' thumbnail
    /// files as files, as Eagle copies them.
    Thumbnail,
    /// The first item as a `data:` URL.
    Base64,
}

/// A folder or smart folder to export.
#[derive(Clone)]
pub(crate) enum Target {
    Folder(String),
    Smart(String),
}

/// Two plans for the same export, waiting for the user to choose how to
/// treat files that are already there.
struct Clash {
    replace: Plan,
    keep: Plan,
    /// What the prompt says.
    title: String,
    detail: String,
    /// Show the copied files in Finder, not the top folder.
    files: bool,
    /// Convert the images instead of copying them.
    convert: Option<Convert>,
}

/// Longest-side presets of the Export in Format dialog.
const SIZES: [u32; 6] = [4096, 2048, 1920, 1280, 1024, 512];

/// The Export in Format dialog's choices. Kept between uses.
#[derive(Clone)]
struct FormatDraft {
    format: Format,
    quality: u8,
    /// `None`: the original size.
    longest: Option<u32>,
    custom: bool,
}

impl Default for FormatDraft {
    fn default() -> Self {
        FormatDraft { format: Format::Jpeg, quality: 90, longest: None, custom: false }
    }
}

/// A background export's counters.
#[derive(Default)]
struct Job {
    /// Packing a .eaglepack rather than copying files.
    packing: bool,
    total: usize,
    done: usize,
    failed: usize,
    current: String,
    /// Set by the worker when it ends: the message to show and the paths
    /// to show in Finder.
    finished: Option<(String, Vec<PathBuf>)>,
    running: bool,
}

/// Export state kept per workspace.
#[derive(Default)]
pub(crate) struct ExportState {
    pub request: Option<Request>,
    clash: Option<Clash>,
    job: Arc<Mutex<Job>>,
    /// The items the Export in Format dialog is open for.
    format_for: Option<Vec<String>>,
    format: FormatDraft,
}

/// Several folder exports into `dest` as one, revealing `dest` when done.
fn merge_plans(dest: &std::path::Path, plans: impl Iterator<Item = Plan>, how: Collision) -> Plan {
    let mut all = Plan { dirs: Vec::new(), steps: Vec::new(), top: dest.to_path_buf(), overwrite: how == Collision::Replace };
    for p in plans {
        all.dirs.extend(p.dirs);
        all.steps.extend(p.steps);
    }
    all
}

/// "1 item", "3 items": English only; localised text uses `tr_n`.
pub(crate) fn plural(n: usize, one: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {one}s") }
}

/// What an export says when done: "Exported 3 items" (and failures).
fn exported(done: usize, failed: usize) -> String {
    if failed > 0 {
        tr_nf(done, "Exported {n} item, {failed} failed", "Exported {n} items, {failed} failed", &[("failed", &failed)])
    } else {
        tr_n(done, "Exported {n} item", "Exported {n} items")
    }
}

/// The save dialog for a pack, suggesting `<name>.eaglepack` (Eagle drops
/// a leading dot from the name and falls back to "Untitled").
fn pick_pack(name: &str) -> Option<PathBuf> {
    let name = name.trim_start_matches('.').replace('/', " ");
    let name = if name.trim().is_empty() { "Untitled".to_string() } else { name };
    rfd::FileDialog::new().set_title(tr("Export as .eaglepack")).add_filter("Eagle Package", &["eaglepack"]).set_file_name(format!("{name}.eaglepack")).save_file()
}

fn pick_folder() -> Option<PathBuf> {
    rfd::FileDialog::new().set_title(tr("Export to Computer")).set_can_create_directories(true).pick_folder()
}

impl App {
    /// Handles a pending export request and draws the export prompts.
    pub(crate) fn export_dialogs(&mut self, ctx: &egui::Context, lib: &Library) {
        if let Some(req) = self.ws.export.request.take() {
            self.handle_export(lib, req);
        }
        self.clash_dialog(ctx);
        self.format_dialog(ctx, lib);
    }

    /// Export in Format: format, quality and longest side, then a folder.
    fn format_dialog(&mut self, ctx: &egui::Context, lib: &Library) {
        let Some(ids) = self.ws.export.format_for.clone() else { return };
        let n = ids.iter().filter_map(|id| lib.item(id)).filter(|i| export::convertible(i)).count();
        let mut d = self.ws.export.format.clone();
        let mut go = None;
        let modal = egui::Modal::new(egui::Id::unique("export-format")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.label(RichText::new(tr("Export in Format")).size(15.0));
            ui.add_space(4.0);
            let skipped = ids.len() - n;
            let mut note = tr_n(n, "{n} image will be converted.", "{n} images will be converted.");
            if skipped > 0 {
                note.push(' ');
                note.push_str(&tr_n(skipped, "{n} item that is not an image will be skipped.", "{n} items that are not images will be skipped."));
            }
            ui.label(RichText::new(note).color(t::p().text_muted));
            ui.add_space(8.0);
            egui::Grid::new("export-format-grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label(tr("Format"));
                ui.horizontal(|ui| {
                    for f in Format::ALL {
                        ui.selectable_value(&mut d.format, f, f.label());
                    }
                });
                ui.end_row();
                ui.label(tr("Quality"));
                ui.add_enabled(d.format.lossy(), egui::Slider::new(&mut d.quality, 1..=100));
                ui.end_row();
                ui.label(tr("Longest side"));
                ui.horizontal(|ui| {
                    let text = match (d.custom, d.longest) {
                        (true, _) => tr("Custom").to_string(),
                        (false, Some(px)) => format!("{px} px"),
                        (false, None) => tr("Original size").to_string(),
                    };
                    egui::ComboBox::from_id_salt("export-format-size").selected_text(text).show_ui(ui, |ui| {
                        if ui.selectable_label(!d.custom && d.longest.is_none(), tr("Original size")).clicked() {
                            (d.custom, d.longest) = (false, None);
                        }
                        for px in SIZES {
                            if ui.selectable_label(!d.custom && d.longest == Some(px), format!("{px} px")).clicked() {
                                (d.custom, d.longest) = (false, Some(px));
                            }
                        }
                        if ui.selectable_label(d.custom, tr("Custom")).clicked() {
                            d.custom = true;
                            d.longest.get_or_insert(1600);
                        }
                    });
                    if d.custom {
                        let mut px = d.longest.unwrap_or(1600);
                        ui.add(egui::DragValue::new(&mut px).range(16..=16384).suffix(" px"));
                        d.longest = Some(px);
                    }
                });
                ui.end_row();
            });
            ui.label(RichText::new(tr("Images are only made smaller, never larger.")).color(t::p().text_muted).size(12.0));
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(n > 0, egui::Button::new(tr("Export…"))).clicked() {
                    go = Some(true);
                }
                if ui.button(tr("Cancel")).clicked() {
                    go = Some(false);
                }
            });
        });
        self.ws.export.format = d.clone();
        if modal.should_close() && go.is_none() {
            go = Some(false);
        }
        let Some(go) = go else { return };
        self.ws.export.format_for = None;
        if !go {
            return;
        }
        let convert = Convert { format: d.format, quality: d.quality, longest: d.longest };
        let Some(dest) = pick_folder() else { return };
        let replace = export::plan_converted(lib, &ids, &dest, d.format, Collision::Replace);
        let clashes = replace.clashes();
        if clashes == 0 {
            self.start_copy(replace, true, Some(convert));
            return;
        }
        let keep = export::plan_converted(lib, &ids, &dest, d.format, Collision::KeepBoth);
        let detail = tr_nf(
            clashes,
            "{n} file in “{path}” has the same name as an exported image.",
            "{n} files in “{path}” have the same name as an exported image.",
            &[("path", &dest.display())],
        );
        self.ws.export.clash = Some(Clash { replace, keep, title: tr("Some files already exist").into(), detail, files: true, convert: Some(convert) });
    }

    fn handle_export(&mut self, lib: &Library, req: Request) {
        if self.export_busy() {
            self.flash(tr("An export is still running"));
            return;
        }
        match req {
            Request::Items(ids) => {
                if ids.is_empty() {
                    return;
                }
                let Some(dest) = pick_folder() else { return };
                let replace = export::plan_items(lib, &ids, &dest, Collision::Replace);
                let clashes = replace.clashes();
                if clashes == 0 {
                    self.start_copy(replace, true, None);
                    return;
                }
                let keep = export::plan_items(lib, &ids, &dest, Collision::KeepBoth);
                let detail = tr_nf(
                    clashes,
                    "{n} file in “{path}” has the same name as an exported item.",
                    "{n} files in “{path}” have the same name as an exported item.",
                    &[("path", &dest.display())],
                );
                self.ws.export.clash = Some(Clash { replace, keep, title: tr("Some files already exist").into(), detail, files: true, convert: None });
            }
            Request::Tree(target) => {
                let hidden = crate::organize::hidden_folders(&lib.metadata.folders, &self.ws.org.unlocked);
                let plan = |dest: &std::path::Path, how| match &target {
                    Target::Folder(id) => export::plan_folder(lib, id, dest, how, &hidden),
                    Target::Smart(id) => export::plan_smart(lib, id, dest, how, &hidden),
                };
                let Some(dest) = pick_folder() else { return };
                let Some(replace) = plan(&dest, Collision::Replace) else { return };
                // Eagle asks when the folder's directory is already there.
                if !replace.top.exists() {
                    self.start_copy(replace, false, None);
                    return;
                }
                let Some(keep) = plan(&dest, Collision::KeepBoth) else { return };
                let name = replace.top.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let detail = trf(
                    "“{path}” already has a folder named “{name}”. Replace overwrites files of the same names inside it; Keep Both numbers the new copies.",
                    &[("path", &dest.display()), ("name", &name)],
                );
                self.ws.export.clash = Some(Clash { replace, keep, title: tr("The folder already exists").into(), detail, files: false, convert: None });
            }
            Request::Trees(ids) => {
                let hidden = crate::organize::hidden_folders(&lib.metadata.folders, &self.ws.org.unlocked);
                let plan = |dest: &std::path::Path, how| merge_plans(dest, ids.iter().filter_map(|id| export::plan_folder(lib, id, dest, how, &hidden)), how);
                let Some(dest) = pick_folder() else { return };
                let replace = plan(&dest, Collision::Replace);
                let present: Vec<String> = replace
                    .dirs
                    .iter()
                    .filter(|d| d.parent() == Some(dest.as_path()) && d.exists())
                    .filter_map(|d| d.file_name().map(|n| n.to_string_lossy().into_owned()))
                    .collect();
                if present.is_empty() {
                    self.start_copy(replace, false, None);
                    return;
                }
                let keep = plan(&dest, Collision::KeepBoth);
                let names = present.iter().map(|n| format!("“{n}”")).collect::<Vec<_>>().join(", ");
                let detail = tr_nf(
                    present.len(),
                    "“{path}” already has a folder named {names}. Replace overwrites files of the same names inside; Keep Both numbers the new copies.",
                    "“{path}” already has folders named {names}. Replace overwrites files of the same names inside; Keep Both numbers the new copies.",
                    &[("path", &dest.display()), ("names", &names)],
                );
                self.ws.export.clash = Some(Clash { replace, keep, title: tr("Some folders already exist").into(), detail, files: false, convert: None });
            }
            Request::Csv(ids) => {
                if ids.is_empty() {
                    return;
                }
                let picked = rfd::FileDialog::new().set_title(tr("Export to CSV")).add_filter("CSV", &["csv"]).set_file_name("falcon-export.csv").save_file();
                let Some(dest) = picked else { return };
                match export::write_csv(lib, &ids, &dest) {
                    Ok(n) => {
                        self.flash(tr_n(n, "Exported {n} item to CSV", "Exported {n} items to CSV"));
                        crate::open_with::reveal(&[dest]);
                    }
                    Err(e) => self.flash_error(trf("Export failed: {error}", &[("error", &e)])),
                }
            }
            Request::Copy(kind, ids) => self.copy_variant(lib, kind, &ids),
            Request::Format(ids) => {
                if ids.iter().filter_map(|id| lib.item(id)).any(export::convertible) {
                    self.ws.export.format_for = Some(ids);
                } else {
                    self.flash(tr("Only images can be exported in another format"));
                }
            }
            Request::PackItems(ids) => {
                if !ids.is_empty() {
                    self.start_pack(lib, PackSource::Items(&ids), "Untitled");
                }
            }
            Request::PackTree(Target::Folder(id)) => {
                let name = logic::find_folder(&lib.metadata.folders, &id).map(|f| f.name.clone()).unwrap_or_default();
                self.start_pack(lib, PackSource::Folder(&id), &name);
            }
            Request::PackTree(Target::Smart(id)) => {
                let name = crate::organize::find(&lib.smart_folders(), &id).map(|f| f.name.clone()).unwrap_or_default();
                self.start_pack(lib, PackSource::Smart(&id), &name);
            }
        }
    }

    /// The port Copy Link names: the running API's, else the first one
    /// Preferences sets (the link works once the API is on).
    pub(crate) fn api_link_port(&self) -> u16 {
        self.server.as_ref().map(|h| h.port).or_else(|| self.prefs.api_ports().map(|r| *r.start())).unwrap_or(falcon_server::DEFAULT_PORT)
    }

    fn copy_variant(&mut self, lib: &Library, kind: CopyKind, ids: &[String]) {
        let items: Vec<&Item> = ids.iter().filter_map(|id| lib.item(id)).collect();
        let Some(first) = items.first() else { return };
        let n = items.len();
        // Base64 and a single thumbnail read files on this thread; one still
        // in iCloud would hold the window until it downloads.
        if matches!(kind, CopyKind::Base64 | CopyKind::Thumbnail) && n == 1 {
            let mut files = vec![first.original_path(lib.root())];
            files.extend(first.thumbnail_path(lib.root()));
            if files.iter().any(|f| crate::icloud::is_dataless(f)) {
                self.flash(tr("This item is still in iCloud; open it once to download it, then copy again"));
                return;
            }
        }
        let text = match kind {
            CopyKind::Path => Some((export::paths_text(lib, &items), tr_n(n, "Copied {n} path", "Copied {n} paths"))),
            CopyKind::FolderPath => Some((export::folder_paths_text(lib, &items), tr_n(n, "Copied {n} folder path", "Copied {n} folder paths"))),
            CopyKind::Name => Some((items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>().join("\n"), tr_n(n, "Copied {n} name", "Copied {n} names"))),
            CopyKind::Link => {
                let port = self.api_link_port();
                Some((items.iter().map(|i| crate::links::api_item_link(port, &i.id)).collect::<Vec<_>>().join("\n"), tr_n(n, "Copied {n} link", "Copied {n} links")))
            }
            CopyKind::EagleLink => Some((items.iter().map(|i| crate::links::item_link(&i.id)).collect::<Vec<_>>().join("\n"), tr_n(n, "Copied {n} Eagle link", "Copied {n} Eagle links"))),
            CopyKind::SourceUrl => {
                let links: Vec<&str> = items.iter().map(|i| i.url.trim()).filter(|u| !u.is_empty()).collect();
                if links.is_empty() {
                    self.flash(if n == 1 { tr("This item has no source URL") } else { tr("These items have no source URL") });
                    return;
                }
                Some((links.join("\n"), tr_n(links.len(), "Copied {n} link", "Copied {n} links")))
            }
            CopyKind::Base64 => match export::data_url(lib, first) {
                Some(url) => Some((url, tr("Copied Base64").to_string())),
                None => {
                    self.flash(tr("No image to copy"));
                    return;
                }
            },
            CopyKind::Thumbnail => None,
        };
        if let Some((text, message)) = text {
            self.ctx.copy_text(text);
            self.flash_brief(message);
            return;
        }
        // Copy Thumbnail.
        let Ok(mut clip) = arboard::Clipboard::new() else { return };
        let ok = if n == 1 {
            export::thumbnail_image(lib, first).is_some_and(|img| {
                let (w, h) = img.dimensions();
                clip.set_image(arboard::ImageData { width: w as usize, height: h as usize, bytes: img.into_raw().into() }).is_ok()
            })
        } else {
            let files: Vec<PathBuf> = items.iter().map(|i| i.thumbnail_path(lib.root()).unwrap_or_else(|| i.original_path(lib.root()))).collect();
            clip.set().file_list(&files).is_ok()
        };
        self.flash_brief(if !ok {
            tr("No thumbnail to copy").to_string()
        } else if n == 1 {
            tr("Copied thumbnail").to_string()
        } else {
            tr_n(n, "Copied {n} thumbnail", "Copied {n} thumbnails")
        });
    }

    /// The Copy (the files), Copy File Path and Copy submenu entries of the
    /// item menu, which the viewer's right-click menu shares.
    pub(crate) fn item_copy_menu(&mut self, ui: &mut Ui, targets: &[String]) {
        let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::Copy);
        if ui.add(egui::Button::new(tr("Copy")).shortcut_text(keys)).clicked() {
            self.copy_items(targets);
        }
        let mut ask = |kind| self.ws.export.request = Some(Request::Copy(kind, targets.to_vec()));
        if ui.button(tr("Copy File Path")).clicked() {
            ask(CopyKind::Path);
        }
        crate::ui::menus::search::submenu(ui, tr("Copy"), |ui| {
            ui.set_min_width(160.0);
            for (label, kind) in [
                (tr_key("Link"), CopyKind::Link),
                (tr_key("Eagle Link"), CopyKind::EagleLink),
                (tr_key("Source URL"), CopyKind::SourceUrl),
                (tr_key("Folder Path"), CopyKind::FolderPath),
                (tr_key("Thumbnail"), CopyKind::Thumbnail),
            ] {
                if ui.button(tr(label)).clicked() {
                    ask(kind);
                }
            }
            // One item only, as in Eagle.
            if ui.add_enabled(targets.len() == 1, egui::Button::new("Base64")).clicked() {
                ask(CopyKind::Base64);
            }
            if ui.button(tr("Name")).clicked() {
                ask(CopyKind::Name);
            }
        });
    }

    /// Asks where to save a pack of `source`, then writes it on a
    /// background thread. As in Eagle, an empty smart folder exports
    /// nothing.
    fn start_pack(&mut self, lib: &Library, source: PackSource<'_>, name: &str) {
        let plan = match pack::plan_pack(lib, source) {
            Ok(p) => p,
            Err(e) => {
                self.flash_error(trf("Export failed: {error}", &[("error", &e)]));
                return;
            }
        };
        if plan.is_empty() && matches!(source, PackSource::Smart(_)) {
            self.flash(tr("Nothing to export"));
            return;
        }
        if self.operation_running() {
            return;
        }
        let Some(dest) = pick_pack(name) else { return };
        let name = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let h = self.ws.op.start(&self.ctx, tr("Exporting Eagle Package"), plan.len().max(1), true);
        h.step(0, name);
        std::thread::spawn(move || {
            // Stopping leaves nothing behind: the archive is built in a
            // temporary file beside `dest` and only moved into place when whole.
            let result = pack::write_pack_until(&plan, &dest, |n| {
                h.step(n, dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                !h.cancelled()
            });
            h.finish(match result {
                Ok(Some(n)) => Outcome { message: exported(n, 0), reveal: vec![dest], exported: true, ..Default::default() },
                Ok(None) => Outcome { message: tr("Export cancelled").into(), ..Default::default() },
                Err(e) => Outcome { error: Some(trf("Export failed: {error}", &[("error", &e)])), ..Default::default() },
            });
        });
    }

    fn clash_dialog(&mut self, ctx: &egui::Context) {
        let Some(c) = &self.ws.export.clash else { return };
        let (title, detail) = (c.title.clone(), c.detail.clone());
        let mut choice: Option<Option<Collision>> = None;
        let modal = egui::Modal::new(egui::Id::unique("export-clash")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(RichText::new(title).size(15.0));
            ui.add_space(4.0);
            ui.label(RichText::new(detail).color(t::p().text_muted));
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(RichText::new(tr("Replace")).color(t::p().icon_red)).clicked() {
                    choice = Some(Some(Collision::Replace));
                }
                if ui.button(tr("Keep Both")).clicked() {
                    choice = Some(Some(Collision::KeepBoth));
                }
                if ui.button(tr("Cancel")).clicked() {
                    choice = Some(None);
                }
            });
        });
        if modal.should_close() && choice.is_none() {
            choice = Some(None);
        }
        let Some(choice) = choice else { return };
        let Some(c) = self.ws.export.clash.take() else { return };
        match choice {
            Some(Collision::Replace) => self.start_copy(c.replace, c.files, c.convert),
            Some(Collision::KeepBoth) => self.start_copy(c.keep, c.files, c.convert),
            None => {}
        }
    }

    fn export_busy(&self) -> bool {
        self.ws.export.job.lock().unwrap_or_else(|p| p.into_inner()).running
    }

    /// Runs a copy plan (or, with `convert`, a conversion) on a background
    /// thread. When it ends, Finder shows the copied files (`files`) or the
    /// plan's top folder.
    fn start_copy(&mut self, plan: Plan, files: bool, convert: Option<Convert>) {
        if plan.steps.is_empty() {
            self.flash(tr("Nothing to export"));
            return;
        }
        let job = self.ws.export.job.clone();
        *job.lock().unwrap_or_else(|p| p.into_inner()) = Job { packing: false, total: plan.steps.len(), running: true, ..Default::default() };
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let on_step = |step: &export::Step, ok: bool| {
                let mut j = job.lock().unwrap_or_else(|p| p.into_inner());
                j.done += 1;
                j.failed += usize::from(!ok);
                j.current = step.to.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                drop(j);
                ctx.request_repaint();
            };
            let result = match &convert {
                Some(c) => export::run_converted(&plan, c, on_step),
                None => export::run(&plan, on_step),
            };
            let mut j = job.lock().unwrap_or_else(|p| p.into_inner());
            j.finished = Some(match result {
                Ok(written) => {
                    let failed = j.failed;
                    // Eagle shows the exported files in Finder, or the folder
                    // when there are many.
                    let reveal = if !files || written.len() > 500 { vec![plan.top.clone()] } else { written.clone() };
                    (exported(written.len(), failed), reveal)
                }
                Err(e) => (trf("Export failed: {error}", &[("error", &e)]), Vec::new()),
            });
            drop(j);
            ctx.request_repaint();
        });
    }

    /// The export progress bar, drawn where the import bar goes; reports
    /// the result once the worker is done.
    pub(crate) fn export_bar(&mut self, ui: &mut Ui, rect: Rect) -> bool {
        let (packing, total, done, current, finished) = {
            let mut j = self.ws.export.job.lock().unwrap_or_else(|p| p.into_inner());
            let finished = j.finished.take();
            if finished.is_some() {
                j.running = false;
            }
            (j.packing, j.total, j.done, j.current.clone(), finished)
        };
        if let Some((msg, reveal)) = finished {
            // Only an export that wrote something reveals it.
            if !reveal.is_empty() {
                self.export_done_notice(&msg);
            }
            self.flash(msg);
            crate::open_with::reveal(&reveal);
            return false;
        }
        if !self.export_busy() || total == 0 {
            return false;
        }
        let p = ui.painter();
        p.rect_filled(rect, 0.0, t::p().main);
        let track = Rect::from_min_max(Pos2::new(rect.left() + GRID_PAD_X, rect.bottom() - 6.0), Pos2::new(rect.right() - GRID_PAD_X, rect.bottom() - 3.0));
        p.rect_filled(track, CornerRadius::same(2), t::p().button);
        let f = done as f32 / total as f32;
        let fill = Rect::from_min_max(track.min, Pos2::new(track.left() + track.width() * f, track.bottom()));
        p.rect_filled(fill, CornerRadius::same(2), t::p().primary);
        let text = if packing {
            trf("Packing {done} of {total}  ·  {current}", &[("done", &done), ("total", &total), ("current", &current)])
        } else {
            trf("Exporting {done} of {total}  ·  {current}", &[("done", &done), ("total", &total), ("current", &current)])
        };
        let g = galley(ui, &text, font(12.0), t::p().text_muted, rect.width() - 40.0);
        p.galley(Pos2::new(rect.left() + GRID_PAD_X, rect.top() + 3.0), g, t::p().text_muted);
        true
    }

    /// The export keys, read in `App::keyboard` (grid only).
    pub(crate) fn export_keys(&mut self, on: &impl Fn(crate::shortcuts::Command) -> bool) {
        use crate::shortcuts::Command as C;
        if on(C::ExportComputer) && !self.ws.selected.is_empty() {
            self.ws.export.request = Some(Request::Items(self.ws.selected.clone()));
        }
        if on(C::ExportFormat) && !self.ws.selected.is_empty() {
            self.ws.export.request = Some(Request::Format(self.ws.selected.clone()));
        }
        if on(C::ExportPack) && !self.ws.selected.is_empty() {
            self.ws.export.request = Some(Request::PackItems(self.ws.selected.clone()));
        }
        for (c, kind) in [
            (C::CopyPath, CopyKind::Path),
            (C::CopyFolderPath, CopyKind::FolderPath),
            (C::CopyName, CopyKind::Name),
            (C::CopyLink, CopyKind::Link),
            (C::CopyEagleLink, CopyKind::EagleLink),
            (C::CopySourceUrl, CopyKind::SourceUrl),
            (C::CopyThumbnail, CopyKind::Thumbnail),
            (C::CopyBase64, CopyKind::Base64),
        ] {
            if on(c) && !self.ws.selected.is_empty() {
                self.ws.export.request = Some(Request::Copy(kind, self.ws.selected.clone()));
            }
        }
        // The selection, or else every item shown.
        if on(C::ExportCsv) {
            let ids = if self.ws.selected.is_empty() { self.ws.order.clone() } else { self.ws.selected.clone() };
            self.ws.export.request = Some(Request::Csv(ids));
        }
    }

    /// The Export submenu of a folder or smart folder menu (disabled while
    /// the folder is locked).
    pub(crate) fn tree_export_menu(&mut self, ui: &mut Ui, target: Target, enabled: bool) {
        ui.add_enabled_ui(enabled, |ui| {
            crate::ui::menus::search::submenu(ui, tr("Export"), |ui| self.tree_export_entries(ui, &target));
        });
    }

    fn tree_export_entries(&mut self, ui: &mut Ui, target: &Target) {
        ui.set_min_width(200.0);
        if ui.button(tr("Export to Computer…")).clicked() {
            self.ws.export.request = Some(Request::Tree(target.clone()));
        }
        if ui.button(tr("Export as .eaglepack…")).clicked() {
            self.ws.export.request = Some(Request::PackTree(target.clone()));
        }
    }

    /// The inspector's Export button for the open folder or smart folder.
    pub(crate) fn scope_export_button(&mut self, ui: &mut Ui, lib: &Library) {
        let target = match &self.ws.scope {
            Scope::Folder(id) => logic::find_folder(&lib.metadata.folders, id)
                .filter(|f| !crate::organize::is_shut(f, &self.ws.org.unlocked))
                .map(|f| Target::Folder(f.id.clone())),
            Scope::Smart(id) => Some(Target::Smart(id.clone())),
            _ => None,
        };
        let resp = wide_button(ui, ic::EXPORT, tr("Export"), target.is_some());
        let Some(target) = target else { return };
        egui::Popup::menu(&resp).show(|ui| {
            self.tree_export_entries(ui, &target);
            // The items shown, as filtered and sorted.
            if ui.add_enabled(!self.ws.order.is_empty(), egui::Button::new(tr("Export to CSV…"))).clicked() {
                self.ws.export.request = Some(Request::Csv(self.ws.order.clone()));
            }
        });
    }

    /// The Export submenu of the item context menu.
    pub(crate) fn item_export_menu(&mut self, ui: &mut Ui, targets: &[String]) {
        crate::ui::menus::search::submenu(ui, tr("Export"), |ui| {
            ui.set_min_width(200.0);
            if ui.button(tr("Export to Computer…")).clicked() {
                self.ws.export.request = Some(Request::Items(targets.to_vec()));
            }
            if ui.button(tr("Export as .eaglepack…")).clicked() {
                self.ws.export.request = Some(Request::PackItems(targets.to_vec()));
            }
            if ui.button(tr("Export in Format…")).clicked() {
                self.ws.export.request = Some(Request::Format(targets.to_vec()));
            }
            if ui.button(tr("Export to CSV…")).clicked() {
                self.ws.export.request = Some(Request::Csv(targets.to_vec()));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::exported;

    #[test]
    fn export_messages() {
        assert_eq!(exported(1, 0), "Exported 1 item");
        assert_eq!(exported(4, 2), "Exported 4 items, 2 failed");
    }
}
