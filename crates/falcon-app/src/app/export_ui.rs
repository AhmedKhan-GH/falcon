//! Export and copy-out: Export to Computer for items, folders and smart
//! folders, with the replace-or-keep-both prompt and a background copy
//! whose progress shows under the toolbar. Paths and names are worked out
//! in `falcon_library::export`; this module asks, runs and reports.

use super::*;
use falcon_library::export::{self, Collision, Plan};
use falcon_library::pack::{self, PackSource};

/// An export asked for from a menu or a key, handled once the frame has
/// the library (`App::export_dialogs`).
pub(super) enum Request {
    /// Export to Computer: these items' originals.
    Items(Vec<String>),
    /// Export to Computer: a folder or smart folder as a directory.
    Tree(Target),
    /// Export as .eaglepack: these items.
    PackItems(Vec<String>),
    /// Export as .eaglepack: a folder with its subfolders, or a smart
    /// folder as one folder.
    PackTree(Target),
}

/// A folder or smart folder to export.
#[derive(Clone)]
pub(super) enum Target {
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
}

/// A background export's counters.
#[derive(Default)]
struct Job {
    verb: &'static str,
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
pub(super) struct ExportState {
    pub request: Option<Request>,
    clash: Option<Clash>,
    job: Arc<Mutex<Job>>,
}

fn plural(n: usize, one: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {one}s") }
}

/// What an export says when done: "Exported 3 items" (and failures).
fn exported(done: usize, failed: usize) -> String {
    let base = format!("Exported {}", plural(done, "item"));
    if failed > 0 { format!("{base}, {failed} failed") } else { base }
}

/// The save dialog for a pack, suggesting `<name>.eaglepack` (Eagle drops
/// a leading dot from the name and falls back to "Untitled").
fn pick_pack(name: &str) -> Option<PathBuf> {
    let name = name.trim_start_matches('.').replace('/', " ");
    let name = if name.trim().is_empty() { "Untitled".to_string() } else { name };
    rfd::FileDialog::new().set_title("Export as .eaglepack").add_filter("Eagle Package", &["eaglepack"]).set_file_name(format!("{name}.eaglepack")).save_file()
}

fn pick_folder() -> Option<PathBuf> {
    rfd::FileDialog::new().set_title("Export to Computer").set_can_create_directories(true).pick_folder()
}

impl App {
    /// Handles a pending export request and draws the export prompts.
    pub(super) fn export_dialogs(&mut self, ctx: &egui::Context, lib: &Library) {
        if let Some(req) = self.ws.export.request.take() {
            self.handle_export(lib, req);
        }
        self.clash_dialog(ctx);
    }

    fn handle_export(&mut self, lib: &Library, req: Request) {
        if self.export_busy() {
            self.flash("An export is still running");
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
                    self.start_copy(replace, true);
                    return;
                }
                let keep = export::plan_items(lib, &ids, &dest, Collision::KeepBoth);
                let detail = format!(
                    "{} in “{}” {} the same name as an exported item.",
                    plural(clashes, "file"),
                    dest.display(),
                    if clashes == 1 { "has" } else { "have" }
                );
                self.ws.export.clash = Some(Clash { replace, keep, title: "Some files already exist".into(), detail, files: true });
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
                    self.start_copy(replace, false);
                    return;
                }
                let Some(keep) = plan(&dest, Collision::KeepBoth) else { return };
                let name = replace.top.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let detail = format!(
                    "“{}” already has a folder named “{name}”. Replace overwrites files of the same names inside it; Keep Both numbers the new copies.",
                    dest.display()
                );
                self.ws.export.clash = Some(Clash { replace, keep, title: "The folder already exists".into(), detail, files: false });
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

    /// Asks where to save a pack of `source`, then writes it on a
    /// background thread. As in Eagle, an empty smart folder exports
    /// nothing.
    fn start_pack(&mut self, lib: &Library, source: PackSource<'_>, name: &str) {
        let plan = match pack::plan_pack(lib, source) {
            Ok(p) => p,
            Err(e) => {
                self.flash(format!("Export failed: {e}"));
                return;
            }
        };
        if plan.is_empty() && matches!(source, PackSource::Smart(_)) {
            self.flash("Nothing to export");
            return;
        }
        let Some(dest) = pick_pack(name) else { return };
        let job = self.ws.export.job.clone();
        let current = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        *job.lock().unwrap_or_else(|p| p.into_inner()) = Job { verb: "Packing", total: plan.len().max(1), running: true, current, ..Default::default() };
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let result = pack::write_pack(&plan, &dest, |n| {
                job.lock().unwrap_or_else(|p| p.into_inner()).done = n;
                ctx.request_repaint();
            });
            job.lock().unwrap_or_else(|p| p.into_inner()).finished = Some(match result {
                Ok(n) => (exported(n, 0), vec![dest]),
                Err(e) => (format!("Export failed: {e}"), Vec::new()),
            });
            ctx.request_repaint();
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
                if ui.button(RichText::new("Replace").color(t::p().icon_red)).clicked() {
                    choice = Some(Some(Collision::Replace));
                }
                if ui.button("Keep Both").clicked() {
                    choice = Some(Some(Collision::KeepBoth));
                }
                if ui.button("Cancel").clicked() {
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
            Some(Collision::Replace) => self.start_copy(c.replace, c.files),
            Some(Collision::KeepBoth) => self.start_copy(c.keep, c.files),
            None => {}
        }
    }

    fn export_busy(&self) -> bool {
        self.ws.export.job.lock().unwrap_or_else(|p| p.into_inner()).running
    }

    /// Runs a copy plan on a background thread. When it ends, Finder shows
    /// the copied files (`files`) or the plan's top folder.
    fn start_copy(&mut self, plan: Plan, files: bool) {
        if plan.steps.is_empty() {
            self.flash("Nothing to export");
            return;
        }
        let job = self.ws.export.job.clone();
        *job.lock().unwrap_or_else(|p| p.into_inner()) = Job { verb: "Exporting", total: plan.steps.len(), running: true, ..Default::default() };
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let result = export::run(&plan, |step, ok| {
                let mut j = job.lock().unwrap_or_else(|p| p.into_inner());
                j.done += 1;
                j.failed += usize::from(!ok);
                j.current = step.to.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                drop(j);
                ctx.request_repaint();
            });
            let mut j = job.lock().unwrap_or_else(|p| p.into_inner());
            j.finished = Some(match result {
                Ok(written) => {
                    let failed = j.failed;
                    // Eagle shows the exported files in Finder, or the folder
                    // when there are many.
                    let reveal = if !files || written.len() > 500 { vec![plan.top.clone()] } else { written.clone() };
                    (exported(written.len(), failed), reveal)
                }
                Err(e) => (format!("Export failed: {e}"), Vec::new()),
            });
            drop(j);
            ctx.request_repaint();
        });
    }

    /// The export progress bar, drawn where the import bar goes; reports
    /// the result once the worker is done.
    pub(super) fn export_bar(&mut self, ui: &mut Ui, rect: Rect) -> bool {
        let (verb, total, done, current, finished) = {
            let mut j = self.ws.export.job.lock().unwrap_or_else(|p| p.into_inner());
            let finished = j.finished.take();
            if finished.is_some() {
                j.running = false;
            }
            (j.verb, j.total, j.done, j.current.clone(), finished)
        };
        if let Some((msg, reveal)) = finished {
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
        let text = format!("{verb} {done} of {total}  ·  {current}");
        let g = galley(ui, &text, font(12.0), t::p().text_muted, rect.width() - 40.0);
        p.galley(Pos2::new(rect.left() + GRID_PAD_X, rect.top() + 3.0), g, t::p().text_muted);
        true
    }

    /// The export keys, read in `App::keyboard` (grid only).
    pub(super) fn export_keys(&mut self, on: &impl Fn(crate::shortcuts::Command) -> bool) {
        use crate::shortcuts::Command as C;
        if on(C::ExportComputer) && !self.ws.selected.is_empty() {
            self.ws.export.request = Some(Request::Items(self.ws.selected.clone()));
        }
        if on(C::ExportPack) && !self.ws.selected.is_empty() {
            self.ws.export.request = Some(Request::PackItems(self.ws.selected.clone()));
        }
    }

    /// The Export submenu of a folder or smart folder menu (disabled while
    /// the folder is locked).
    pub(super) fn tree_export_menu(&mut self, ui: &mut Ui, target: Target, enabled: bool) {
        ui.add_enabled_ui(enabled, |ui| {
            ui.menu_button("Export", |ui| self.tree_export_entries(ui, &target));
        });
    }

    fn tree_export_entries(&mut self, ui: &mut Ui, target: &Target) {
        ui.set_min_width(200.0);
        if ui.button("Export to Computer…").clicked() {
            self.ws.export.request = Some(Request::Tree(target.clone()));
        }
        if ui.button("Export as .eaglepack…").clicked() {
            self.ws.export.request = Some(Request::PackTree(target.clone()));
        }
    }

    /// The inspector's Export button for the open folder or smart folder.
    pub(super) fn scope_export_button(&mut self, ui: &mut Ui, lib: &Library) {
        let target = match &self.ws.scope {
            Scope::Folder(id) => logic::find_folder(&lib.metadata.folders, id)
                .filter(|f| !crate::organize::is_shut(f, &self.ws.org.unlocked))
                .map(|f| Target::Folder(f.id.clone())),
            Scope::Smart(id) => Some(Target::Smart(id.clone())),
            _ => None,
        };
        let resp = wide_button(ui, ic::EXPORT, "Export", target.is_some());
        let Some(target) = target else { return };
        egui::Popup::menu(&resp).show(|ui| self.tree_export_entries(ui, &target));
    }

    /// The Export submenu of the item context menu.
    pub(super) fn item_export_menu(&mut self, ui: &mut Ui, targets: &[String]) {
        ui.menu_button("Export", |ui| {
            ui.set_min_width(200.0);
            if ui.button("Export to Computer…").clicked() {
                self.ws.export.request = Some(Request::Items(targets.to_vec()));
            }
            if ui.button("Export as .eaglepack…").clicked() {
                self.ws.export.request = Some(Request::PackItems(targets.to_vec()));
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
