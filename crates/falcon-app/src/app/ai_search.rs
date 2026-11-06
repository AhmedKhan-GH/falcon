//! AI Search's part of the app: keeping the worker on the shown library,
//! starting searches and taking their answers. The grid shows the results
//! through `scope_order`; the worker and the model are off the UI thread.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::TryRecvError;

use eframe::egui::Pos2;
use falcon_clip::{Phase, Query, SearchError};
use falcon_library::Library;
use falcon_library::model::SavedAiRules;

use crate::ai_search::{Answer, Engine};
use crate::app::{App, Shared, lock};
use crate::locale::tr;
use crate::logic::Scope;
use crate::prefs::Section;

/// What the app keeps across libraries.
#[derive(Default)]
pub(crate) struct AiSearch {
    pub(crate) engine: Option<Arc<Engine>>,
    /// What the worker was last told, so it's only told about changes.
    library: Option<Shared>,
    enabled: Option<bool>,
}

impl AiSearch {
    pub(crate) fn start(ctx: &eframe::egui::Context) -> AiSearch {
        AiSearch { engine: Engine::start(ctx), ..Default::default() }
    }
}

impl App {
    /// Once a frame: follows the shown library and the preference, and takes
    /// a search's answer.
    pub(crate) fn ai_tick(&mut self) {
        let Some(engine) = self.ai.engine.clone() else { return };
        engine.tick();
        let same = match (&self.ai.library, &self.ws.lib) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.ai.library.clone_from(&self.ws.lib);
            engine.set_library(self.ws.lib.clone());
        }
        if self.ai.enabled != Some(self.prefs.ai_search) {
            self.ai.enabled = Some(self.prefs.ai_search);
            engine.set_enabled(self.prefs.ai_search);
        }
        // Held rules run once the index has all of the shown library. Only
        // the root is read under the lock, and the search is the worker's.
        if self.ws.ai.waiting && self.ai_view_allowed() && self.ws.ai.pending.is_none() {
            let status = engine.status();
            let ready = status.phase == Phase::Ready && self.ws.lib.as_ref().is_some_and(|lib| status.index_ready(lock(lib).root()));
            if ready && let Some(query) = self.ws.ai.release() {
                self.ai_start(query);
            }
        }
        let answer = match &self.ws.ai.pending {
            Some(rx) => match rx.try_recv() {
                Ok(answer) => Some(answer),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(Err(SearchError::Failed(tr("the search stopped").to_string()))),
            },
            None => None,
        };
        let Some(answer) = answer else { return };
        match self.ws.ai.answer(answer) {
            Answer::Shown | Answer::Failed(None) => {}
            Answer::Next(query) => self.ai_start(query),
            Answer::Failed(Some(message)) => self.flash_error(message),
        }
    }

    /// Puts a saved filter's AI rules in place, after its filter and search.
    /// The box reads the description unless the filter's own search text is
    /// in it; the rules wait in `ai_tick` until they can run.
    pub(crate) fn ai_restore(&mut self, rules: SavedAiRules, lib: &Library) {
        let keyword = !self.ws.query.trim().is_empty();
        if rules.is_active() || keyword {
            self.ws.ai.mode = rules.semantic_text().is_some() && !keyword;
        }
        if self.ws.ai.mode
            && let Some(text) = rules.semantic_text()
        {
            self.ws.search_typed = text.to_string();
        }
        self.ws.ai.image = rules.image_item().map(|id| lib.item(id).map_or_else(|| id.to_string(), |item| item.name.clone()));
        self.ws.ai.restore(rules);
    }

    /// The chip's Remove: ends the search and what the box says of it.
    pub(crate) fn ai_remove_rules(&mut self) {
        if std::mem::take(&mut self.ws.ai.mode) {
            self.ws.set_query("");
        }
        self.ws.ai.clear_results();
    }

    /// Whether searches can be started: the model is in and turned on.
    pub(crate) fn ai_ready(&self) -> bool {
        self.ai.engine.as_ref().is_some_and(|e| e.status().searchable())
    }

    /// Whether this view can show AI results: not the tag or duplicates pages.
    pub(crate) fn ai_view_allowed(&self) -> bool {
        !matches!(self.ws.scope, Scope::AllTags | Scope::Duplicates)
    }

    /// Opens Preferences on the AI Search pane.
    pub(crate) fn open_ai_preferences(&mut self) {
        self.open_preferences();
        self.prefs_window.set_pane(Section::AiSearch);
    }

    fn ai_start(&mut self, query: Query) {
        let Some(engine) = &self.ai.engine else { return };
        self.ws.ai.pending = Some(engine.search(query));
    }

    /// The toolbar button: turns the search box's AI mode on or off, or opens
    /// Preferences if the model isn't ready.
    pub(crate) fn ai_button_clicked(&mut self) {
        if !self.ws.ai.mode && !self.ai_ready() {
            self.open_ai_preferences();
            return;
        }
        self.ws.ai.mode = !self.ws.ai.mode;
        self.focus_search = true;
        if self.ws.ai.mode {
            let text = self.ws.search_typed.clone();
            self.ws.query.clear();
            self.ws.search_debounce.reset();
            self.ai_text_search(&text);
        } else {
            self.ws.ai.clear_results();
            self.ws.set_query("");
        }
    }

    /// Searches by a description; blank text ends the search. What the grid
    /// shows stays until the answer comes.
    pub(crate) fn ai_text_search(&mut self, text: &str) {
        if text.trim().is_empty() {
            self.ws.ai.clear_results();
        } else {
            self.ws.ai.image = None;
            self.ws.ai.begin(SavedAiRules::for_text(text.trim()));
            if self.ai_ready() {
                self.ai_start(Query::Text(text.to_string()));
            } else {
                self.ws.ai.pending = None;
                self.ws.ai.waiting = true;
            }
        }
    }

    /// Shows the items that look like this picture.
    fn ai_image_search(&mut self, label: String, query: Query) {
        if !self.ai_view_allowed() {
            self.go(Scope::All);
        }
        self.ws.viewer = None;
        self.ws.query.clear();
        self.ws.search_typed.clear();
        self.ws.search_debounce.reset();
        self.ws.ai.image = Some(label);
        // Only an item can be saved: the format holds no other picture.
        self.ws.ai.begin(if let Query::Item(id) = &query { SavedAiRules::for_item(id) } else { SavedAiRules::default() });
        self.ai_start(query);
    }

    /// Visual Match from an item.
    pub(crate) fn ai_similar_to_item(&mut self, id: &str, name: &str) {
        self.ai_image_search(name.to_string(), Query::Item(id.to_string()));
    }

    /// Visual Match with a picture on disk.
    pub(crate) fn ai_similar_to_file(&mut self, path: PathBuf) {
        let label = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        self.ai_image_search(label, Query::ImageFile(path));
    }

    /// The camera's Choose Image.
    pub(crate) fn ai_choose_image(&mut self) {
        let extensions = ["jpg", "jpeg", "png", "webp", "gif", "bmp", "tif", "tiff", "heic", "avif"];
        if let Some(path) = crate::dialog_owner::file_dialog().set_title(tr("Choose Image")).add_filter(tr("Images"), &extensions).pick_file() {
            self.ai_similar_to_file(path);
        }
    }

    /// The camera's Paste Image: a copied file or copied pixels.
    pub(crate) fn ai_paste_image(&mut self) {
        let Ok(mut clip) = arboard::Clipboard::new() else { return };
        if let Ok(files) = clip.get().file_list()
            && let Some(first) = files.into_iter().next()
        {
            self.ai_similar_to_file(first);
            return;
        }
        let picture = clip.get_image().ok().and_then(|img| image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.into_owned()));
        match picture {
            Some(buffer) => self.ai_image_search(tr("Pasted image").to_string(), Query::Image(Box::new(image::DynamicImage::ImageRgba8(buffer)))),
            None => self.flash(tr("The clipboard has no picture")),
        }
    }

    /// The Visual Match command: the selected item, else a picture to choose.
    pub(crate) fn ai_visual_match_command(&mut self, name_of: impl Fn(&str) -> Option<String>) {
        if !self.ai_ready() {
            self.open_ai_preferences();
            return;
        }
        match self.ws.selected.as_slice() {
            [id] => {
                let id = id.clone();
                if let Some(name) = name_of(&id) {
                    self.ai_similar_to_item(&id, &name);
                }
            }
            _ => self.ai_choose_image(),
        }
    }

    /// A file dropped on the search box is searched by, not imported.
    /// Returns the drop when it's for the import.
    pub(crate) fn ai_take_drop(&mut self, dropped: Vec<PathBuf>, at: Option<Pos2>) -> Vec<PathBuf> {
        let on_box = self.ws.viewer.is_none() && at.zip(self.ws.ai.search_rect).is_some_and(|(pos, rect)| rect.contains(pos));
        if !on_box || !self.ai_ready() {
            return dropped;
        }
        if let Some(first) = dropped.into_iter().find(|p| !p.is_dir()) {
            self.ai_similar_to_file(first);
        }
        Vec::new()
    }
}
