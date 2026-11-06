//! Help ▸ Export Debug Report: asks where to save, gathers the facts and
//! writes the zip (see `debug_report.rs`).

use falcon_library::model::Folder;

use crate::app::{App, lock};
use crate::locale::{tr, trf};
use crate::debug_report::{self as report, Info, LibraryInfo};

fn count_folders(folders: &[Folder]) -> usize {
    folders.iter().map(|f| 1 + count_folders(&f.children)).sum()
}

impl App {
    pub(crate) fn export_debug_report(&mut self, ctx: &eframe::egui::Context) {
        let mut dialog = crate::dialog_owner::file_dialog().set_title(tr("Export Debug Report")).set_file_name("Falcon Debug Report.zip");
        if let Some(desktop) = report::desktop() {
            dialog = dialog.set_directory(desktop);
        }
        let Some(dest) = dialog.save_file() else { return };
        let info = self.debug_info(ctx);
        let root = self.ws.root.clone();
        match report::export(&info, root.as_deref(), &dest) {
            Ok(()) => {
                crate::open_with::reveal(&[dest]);
                self.flash(tr("Exported the debug report"));
            }
            Err(e) => {
                self.flash_error(trf("Could not export the debug report: {error}", &[("error", &e)]));
            }
        }
    }

    fn debug_info(&self, ctx: &eframe::egui::Context) -> Info {
        let library = self.ws.lib.as_ref().map(|l| {
            let lib = lock(l);
            let tags: std::collections::HashSet<&str> = lib.items().iter().flat_map(|i| i.tags.iter().map(String::as_str)).collect();
            LibraryInfo {
                path: lib.root().display().to_string(),
                items: lib.items().len(),
                folders: count_folders(&lib.metadata.folders),
                tags: tags.len(),
                files: report::library_files(lib.root()),
            }
        });
        let open: Vec<_> = self.others.iter(&self.ws).filter_map(|w| w.root.clone()).collect();
        let shown = self.ws.root.as_ref();
        let size = ctx.content_rect().size();
        Info {
            created: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            system: report::system(),
            window: (size.x, size.y),
            pixels_per_point: ctx.pixels_per_point(),
            library,
            other_libraries: open.iter().filter(|p| Some(*p) != shown).map(|p| p.display().to_string()).collect(),
            recent: self.recent.iter().map(|p| p.display().to_string()).collect(),
        }
    }
}
