//! The viewer for text (`.txt`) items, after Eagle's text editor: the
//! item's name above its text on a centred page, the text editable in
//! place and saved by itself half a second after typing stops (also when
//! the editor loses focus, the viewer closes or moves on, ⌘S, or the
//! library closes). Saves go through `Library::save_text` (see the
//! desktop app chapter of the docs).

use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use eframe::egui::{self, Galley, Key, Margin, Modifiers, Pos2, Rect, RichText, Sense, TextBuffer, TextEdit, Ui, text::LayoutJob, vec2};
use falcon_library::Library;
use falcon_library::model::Item;

use super::App;
use super::media_view::top_bar;
use crate::textdoc::{self, Autosave};
use crate::theme::{self as t, font};

/// Eagle's page: at most 920 points wide, 30 points of padding.
const PAGE_WIDTH: f32 = 920.0;
const PAD: f32 = 30.0;
/// Body text at 14 points with 1.6 line spacing, the title at 20.
const BODY: f32 = 14.0;
const LINE: f32 = 1.6;
const TITLE: f32 = 20.0;

/// The text on screen and what is known of the file under it.
pub(super) struct TextView {
    /// The item shown; the state is dropped (after a last save) once the
    /// viewer shows something else.
    pub id: String,
    text: String,
    /// The text as last read from or written to the file.
    saved: String,
    read_only: Option<&'static str>,
    /// Why the file couldn't be read, if it couldn't.
    error: Option<String>,
    autosave: Autosave,
    /// Save at the next chance, without waiting (focus lost, ⌘S).
    save_now: bool,
    /// The file's modification time as last read or written.
    stamp: Option<SystemTime>,
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl TextView {
    /// Reads the item's original.
    fn open(id: &str, path: &Path) -> TextView {
        let mut v = TextView {
            id: id.to_string(),
            text: String::new(),
            saved: String::new(),
            read_only: None,
            error: None,
            autosave: Autosave::default(),
            save_now: false,
            stamp: None,
        };
        v.read(path);
        v
    }

    fn read(&mut self, path: &Path) {
        match std::fs::read(path) {
            Ok(bytes) => {
                let loaded = textdoc::decode(bytes);
                self.text = loaded.text.clone();
                self.saved = loaded.text;
                self.read_only = loaded.read_only;
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        self.stamp = modified(path);
        self.autosave.clear();
    }

    /// The text to write, if it differs from the file and may be saved.
    fn unsaved(&self) -> Option<&str> {
        (self.read_only.is_none() && self.error.is_none() && textdoc::needs_save(&self.text, &self.saved)).then_some(self.text.as_str())
    }

    /// Writes the unsaved text, if any, and forgets the waiting edit.
    pub fn save(&mut self, lib: &mut Library) -> falcon_library::Result<()> {
        self.autosave.clear();
        self.save_now = false;
        let Some(text) = self.unsaved().map(str::to_string) else { return Ok(()) };
        lib.save_text(&self.id, &text)?;
        self.saved = text;
        self.stamp = lib.item(&self.id).and_then(|i| modified(&i.original_path(lib.root())));
        Ok(())
    }
}

/// Lays the text out with Eagle's line spacing.
fn body_job(text: &str, size: f32, wrap: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    let format = egui::TextFormat { font_id: font(size), color: t::p().text, line_height: Some(size * LINE), ..Default::default() };
    job.append(text, 0.0, format);
    job
}

impl App {
    /// Draws the viewer for a text item and returns true; false for any
    /// other item.
    pub(super) fn text_viewer(&mut self, parent: &mut Ui, lib: &Library, item: &Item, screen: Rect) -> bool {
        if !item.ext.eq_ignore_ascii_case("txt") {
            return false;
        }
        let path = item.original_path(lib.root());
        if self.ws.text.as_ref().is_none_or(|v| v.id != item.id) {
            self.ws.text = Some(TextView::open(&item.id, &path));
        }
        let ctx = parent.ctx().clone();
        let now = ctx.input(|i| i.time);
        let ui = &mut super::viewer_ui(parent, screen, "text-viewer");
        ui.painter().rect_filled(screen, 0.0, t::p().main);
        let bar = Rect::from_min_size(screen.min, vec2(screen.width(), t::BAR_HEIGHT));
        let area = Rect::from_min_max(Pos2::new(screen.left(), bar.bottom()), screen.max);
        let editor_id = egui::Id::unique(("text-editor", &item.id));
        // ⌘S saves at once, as in Eagle.
        if ctx.memory(|m| m.has_focus(editor_id)) && ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::S)) {
            self.ws.text.as_mut().expect("opened above").save_now = true;
        }

        // Under the page: a click beside the text puts the cursor in
        // it, and a right-click opens the item menu.
        let bg = ui.interact(area, ui.scope_id().with("text-bg"), Sense::click());
        if bg.clicked() {
            ctx.memory_mut(|m| m.request_focus(editor_id));
        }
        bg.context_menu(|ui| {
            self.item_menu(ui, lib, item, std::slice::from_ref(&item.id));
        });
        let mut page = ui.new_child(egui::UiBuilder::new().max_rect(area));
        page.set_clip_rect(area.intersect(ui.clip_rect()));
        let view = self.ws.text.as_mut().expect("opened above");
        egui::ScrollArea::vertical().id_salt(("text-scroll", &item.id)).auto_shrink(false).show(&mut page, |ui| {
            let side = ((ui.available_width() - PAGE_WIDTH) / 2.0).max(0.0) + PAD;
            let width = (ui.available_width() - 2.0 * side).max(40.0);
            ui.add_space(PAD);
            ui.horizontal(|ui| {
                ui.add_space(side);
                ui.vertical(|ui| {
                    ui.set_width(width);
                    ui.label(RichText::new(&item.name).font(font(TITLE)).strong().color(t::p().text));
                    if let Some(why) = view.error.as_deref().or(view.read_only) {
                        ui.add_space(4.0);
                        ui.label(RichText::new(why).size(12.0).color(t::p().text_muted));
                    }
                    ui.add_space(12.0);
                    let mut layouter = |ui: &Ui, buf: &dyn TextBuffer, wrap: f32| -> Arc<Galley> { ui.fonts_mut(|f| f.layout_job(body_job(buf.as_str(), BODY, wrap))) };
                    let out = TextEdit::multiline(&mut view.text)
                        .id(editor_id)
                        .frame(egui::Frame::NONE)
                        .margin(Margin::ZERO)
                        .desired_width(width)
                        .min_size(vec2(width, (area.height() - 160.0).max(BODY * LINE)))
                        .lock_focus(true)
                        .interactive(view.read_only.is_none() && view.error.is_none())
                        .layouter(&mut layouter)
                        .show(ui);
                    let r = &out.response.response;
                    if r.changed() {
                        view.autosave.edited(now);
                    }
                    if r.lost_focus() {
                        view.save_now = true;
                    }
                });
            });
            ui.add_space(PAD);
        });
        if let Some(wait) = self.ws.text.as_ref().and_then(|v| v.autosave.wait(now)) {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
        }
        top_bar(self, ui, item, bar);
        true
    }

    /// Saves the text viewer's typing once it is due, or at once when the
    /// viewer has moved off the item (its state is then dropped). Runs
    /// each frame with the library locked.
    pub(super) fn text_saves(&mut self, lib: &mut Library, now: f64) {
        let Some(view) = self.ws.text.as_mut() else { return };
        let gone = self.ws.viewer.as_deref() != Some(view.id.as_str());
        if (gone || view.save_now || view.autosave.due(now))
            && let Err(e) = view.save(lib)
        {
            self.flash(format!("Could not save the text: {e}"));
        }
        if gone {
            self.ws.text = None;
        }
    }
}
