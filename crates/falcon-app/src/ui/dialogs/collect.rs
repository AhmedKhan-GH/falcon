//! The collect window, a small floating window for gathering items.

use eframe::egui;

use crate::app::{App, lock};
use crate::logic;

impl App {
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
}
