//! Export and copy-out: Export to Computer for items, folders and smart
//! folders, with the replace-or-keep-both prompt and a background copy
//! whose progress shows under the toolbar. Paths and names are worked out
//! in `falcon_library::export`; this module asks, runs and reports.

use super::*;
use falcon_library::export::{self, Collision, Plan};

/// An export asked for from a menu or a key, handled once the frame has
/// the library (`App::export_dialogs`).
pub(super) enum Request {
    /// Export to Computer: these items' originals.
    Items(Vec<String>),
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
        }
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
    }

    /// The Export submenu of the item context menu.
    pub(super) fn item_export_menu(&mut self, ui: &mut Ui, targets: &[String]) {
        ui.menu_button("Export", |ui| {
            ui.set_min_width(200.0);
            if ui.button("Export to Computer…").clicked() {
                self.ws.export.request = Some(Request::Items(targets.to_vec()));
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
