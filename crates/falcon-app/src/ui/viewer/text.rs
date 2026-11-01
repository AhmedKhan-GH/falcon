//! The viewer for text (`.txt`) items, after Eagle's text editor: the
//! item's name (editable, to rename it) above its text on a centred page,
//! the text editable in place and saved by itself half a second after
//! typing stops (also when the editor loses focus, the viewer closes or
//! moves on, ⌘S, or the library closes). Saves go through
//! `Library::save_text`. The text size zooms in Eagle's five steps, links
//! open in the browser, and a change made to the file outside is read
//! again (see the desktop app chapter of the docs).

use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use eframe::egui::{self, Galley, Key, Margin, Modifiers, Pos2, Rect, RichText, Sense, TextBuffer, TextEdit, Ui, text::LayoutJob, vec2};
use falcon_library::Library;
use falcon_library::model::Item;

use crate::app::App;
use crate::ui::viewer::audio::{bar_name, bar_shell};
use crate::icons as ic;
use crate::logic::ViewZoom;
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
pub(crate) struct TextView {
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
    /// When the file was last checked for outside changes.
    polled: f64,
    /// When the wheel last zoomed.
    wheel_at: f64,
    /// The title being edited.
    title: String,
    /// Put the cursor in the text on the next frame (a new text file).
    focus: bool,
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
            polled: 0.0,
            wheel_at: f64::NEG_INFINITY,
            title: String::new(),
            focus: false,
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

/// Lays the text out with Eagle's line spacing, links in the accent colour
/// and underlined.
fn body_job(text: &str, size: f32, wrap: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    let plain = egui::TextFormat { font_id: font(size), color: t::p().text, line_height: Some(size * LINE), ..Default::default() };
    let link = egui::TextFormat { color: t::p().primary, underline: egui::Stroke::new(1.0, t::p().primary), ..plain.clone() };
    let mut at = 0;
    for r in textdoc::find_links(text) {
        job.append(&text[at..r.start], 0.0, plain.clone());
        job.append(&text[r.clone()], 0.0, link.clone());
        at = r.end;
    }
    job.append(&text[at..], 0.0, plain);
    job
}

/// The link under `pos` (relative to the galley), if the pointer is over
/// one of its characters rather than just nearest to it.
fn link_at(galley: &Galley, text: &str, pos: egui::Vec2) -> Option<String> {
    let near = galley.cursor_from_pos(pos).index.0;
    let hit = [near.saturating_sub(1), near].into_iter().find(|&c| {
        let a = galley.pos_from_cursor(egui::text::CCursor::new(c));
        let b = galley.pos_from_cursor(egui::text::CCursor::new(c + 1));
        a.y_range().contains(pos.y) && b.min.y == a.min.y && (a.min.x..=b.min.x).contains(&pos.x)
    })?;
    let byte = text.char_indices().nth(hit).map(|(b, _)| b)?;
    textdoc::find_links(text).into_iter().find(|r| r.contains(&byte)).map(|r| textdoc::link_target(&text[r]))
}

impl App {
    /// Draws the viewer for a text item and returns true; false for any
    /// other item.
    pub(crate) fn text_viewer(&mut self, parent: &mut Ui, lib: &Library, item: &Item, screen: Rect) -> bool {
        if !item.ext.eq_ignore_ascii_case("txt") {
            return false;
        }
        let path = item.original_path(lib.root());
        if self.ws.text.as_ref().is_none_or(|v| v.id != item.id) {
            self.ws.text = Some(TextView::open(&item.id, &path));
        }
        let ctx = parent.ctx().clone();
        let now = ctx.input(|i| i.time);
        let ui = &mut crate::ui::widgets::viewer_ui(parent, screen, "text-viewer");
        ui.painter().rect_filled(screen, 0.0, t::p().main);
        let bar = Rect::from_min_size(screen.min, vec2(screen.width(), t::BAR_HEIGHT));
        let area = Rect::from_min_max(Pos2::new(screen.left(), bar.bottom()), screen.max);
        let editor_id = egui::Id::unique(("text-editor", &item.id));
        let title_id = egui::Id::unique(("text-title", &item.id));
        let editing = ctx.memory(|m| m.has_focus(editor_id) || m.has_focus(title_id));
        let view = self.ws.text.as_mut().expect("opened above");
        // ⌘S saves at once, as in Eagle; F5 reads the file again while
        // nothing is being typed.
        if ctx.memory(|m| m.has_focus(editor_id)) && ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::S)) {
            view.save_now = true;
        }
        let reload = !editing && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F5));
        // A change made outside (another editor, a sync tool) is read
        // again, checked once a second.
        if reload || now - view.polled >= textdoc::POLL_INTERVAL {
            view.polled = now;
            let changed = modified(&path) != view.stamp;
            let disk_empty = std::fs::metadata(&path).is_ok_and(|m| m.len() == 0);
            if reload || textdoc::should_reload(changed, view.autosave.wait(now).is_some(), disk_empty, view.text.is_empty()) {
                view.read(&path);
            } else if changed {
                view.stamp = modified(&path);
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(textdoc::POLL_INTERVAL));

        // Zoom: ⌘= and ⌘− step, ⌘0 and ⌘9 go back to the smallest size,
        // and the wheel with ⌘, ⌃ or ⌥ (or a pinch) steps at most every
        // 200 ms, as in Eagle.
        let mut zoom = self.text_zoom;
        match self.ws.view.zoom_key.take() {
            Some(ViewZoom::In) => zoom = textdoc::zoom_step(zoom, true),
            Some(ViewZoom::Out) => zoom = textdoc::zoom_step(zoom, false),
            Some(ViewZoom::Fit | ViewZoom::Actual) => zoom = 0,
            _ => {}
        }
        if ui.rect_contains_pointer(area) {
            let (pinch, alt, wheel) = ctx.input(|i| (i.zoom_delta(), i.modifiers.alt, i.smooth_scroll_delta.y));
            let dir = if pinch != 1.0 { Some(pinch > 1.0) } else if alt && wheel != 0.0 { Some(wheel > 0.0) } else { None };
            if alt {
                // The page must not scroll while ⌥ zooms it.
                ctx.input_mut(|i| {
                    i.smooth_scroll_delta = egui::Vec2::ZERO;
                });
            }
            if let Some(zoom_in) = dir
                && now - view.wheel_at >= textdoc::WHEEL_ZOOM_GAP
            {
                view.wheel_at = now;
                zoom = textdoc::zoom_step(zoom, zoom_in);
            }
        }
        let scale = textdoc::zoom_scale(zoom);

        // Under the page: a click beside the text puts the cursor in
        // it, and a right-click opens the item menu.
        let bg = ui.interact(area, ui.scope_id().with("text-bg"), Sense::click());
        if bg.clicked() {
            ctx.memory_mut(|m| m.request_focus(editor_id));
        }
        let mut page = ui.new_child(egui::UiBuilder::new().max_rect(area));
        page.set_clip_rect(area.intersect(ui.clip_rect()));
        let mut rename = None;
        let mut open_link = None;
        egui::ScrollArea::vertical().id_salt(("text-scroll", &item.id)).auto_shrink(false).show(&mut page, |ui| {
            let side = ((ui.available_width() - PAGE_WIDTH) / 2.0).max(0.0) + PAD;
            let width = (ui.available_width() - 2.0 * side).max(40.0);
            ui.add_space(PAD);
            ui.horizontal(|ui| {
                ui.add_space(side);
                ui.vertical(|ui| {
                    ui.set_width(width);
                    // The title renames the item when it loses focus;
                    // emptied, it goes back to the name.
                    if !ctx.memory(|m| m.has_focus(title_id)) {
                        view.title.clone_from(&item.name);
                    }
                    let title = TextEdit::singleline(&mut view.title)
                        .id(title_id)
                        .frame(egui::Frame::NONE)
                        .margin(Margin::ZERO)
                        .font(font(TITLE * scale))
                        .text_color(t::p().text)
                        .desired_width(width)
                        .show(ui);
                    if title.response.response.lost_focus() {
                        let name = view.title.trim();
                        if !name.is_empty() && name != item.name {
                            rename = Some(name.to_string());
                        }
                        view.title.clone_from(&item.name);
                    }
                    if let Some(why) = view.error.as_deref().or(view.read_only) {
                        ui.add_space(4.0);
                        ui.label(RichText::new(why).size(12.0).color(t::p().text_muted));
                    }
                    ui.add_space(12.0 * scale);
                    let size = BODY * scale;
                    let mut layouter = |ui: &Ui, buf: &dyn TextBuffer, wrap: f32| -> Arc<Galley> { ui.fonts_mut(|f| f.layout_job(body_job(buf.as_str(), size, wrap))) };
                    let editable = view.read_only.is_none() && view.error.is_none();
                    let out = TextEdit::multiline(&mut view.text)
                        .id(editor_id)
                        .frame(egui::Frame::NONE)
                        .margin(Margin::ZERO)
                        .desired_width(width)
                        .min_size(vec2(width, (area.height() - 160.0).max(size * LINE)))
                        .lock_focus(true)
                        .interactive(editable)
                        .layouter(&mut layouter)
                        .show(ui);
                    let r = &out.response.response;
                    if std::mem::take(&mut view.focus) {
                        r.request_focus();
                    }
                    if r.changed() {
                        view.autosave.edited(now);
                    }
                    if r.lost_focus() {
                        view.save_now = true;
                    }
                    // Links open in the browser with ⌘-click (a plain
                    // click when the text can't be edited).
                    if let Some(p) = r.hover_pos()
                        && let Some(url) = link_at(&out.galley, &view.text, p - out.galley_pos)
                    {
                        let cmd = ctx.input(|i| i.modifiers.command);
                        if cmd || !editable {
                            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        // A read-only editor senses no clicks of its own.
                        let clicked = r.clicked() || (!editable && ctx.input(|i| i.pointer.primary_clicked()));
                        if clicked && (cmd || !editable) {
                            open_link = Some(url.clone());
                        }
                        r.clone().on_hover_text_at_pointer(if editable { format!("⌘-click to open {url}") } else { format!("Click to open {url}") });
                    }
                });
            });
            ui.add_space(PAD);
        });
        bg.context_menu(|ui| {
            self.item_menu(ui, lib, item, std::slice::from_ref(&item.id));
        });
        if let Some(url) = open_link {
            crate::open_with::open_url(&url);
        }
        if let Some(name) = rename {
            self.ws.actions.push(crate::app::Action::RenameItem(item.id.clone(), name));
        }
        if let Some(wait) = self.ws.text.as_ref().and_then(|v| v.autosave.wait(now)) {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
        }

        // The bar: the shared buttons, then the text size at the right.
        let (left, mut right) = bar_shell(self, ui, item, bar);
        let cy = bar.center().y;
        if crate::ui::widgets::icon_button(ui, Pos2::new(right - 12.0, cy), ic::PLUS, 15.0, zoom + 1 < textdoc::ZOOM_LEVELS, "Larger text (⌘=)").clicked() {
            zoom = textdoc::zoom_step(zoom, true);
        }
        let label = format!("{}%", (scale * 100.0).round());
        let g = ui.painter().layout_no_wrap(label, font(12.0), t::p().text_value);
        let lx = right - 26.0 - g.size().x;
        let reset = ui.interact(Rect::from_min_size(Pos2::new(lx, cy - 10.0), vec2(g.size().x, 20.0)), ui.scope_id().with("text-zoom-reset"), Sense::click());
        ui.painter().galley(Pos2::new(lx, cy - g.size().y / 2.0), g, t::p().text_value);
        if reset.on_hover_text("Smallest text (⌘0)").clicked() {
            zoom = 0;
        }
        if crate::ui::widgets::icon_button(ui, Pos2::new(lx - 14.0, cy), ic::MINUS, 15.0, zoom > 0, "Smaller text (⌘−)").clicked() {
            zoom = textdoc::zoom_step(zoom, false);
        }
        right = lx - 30.0;
        bar_name(ui, item, bar, left, right);
        self.text_zoom = zoom;
        true
    }

    /// Saves the text viewer's typing once it is due, or at once when the
    /// viewer has moved off the item (its state is then dropped). Runs
    /// each frame with the library locked.
    pub(crate) fn text_saves(&mut self, lib: &mut Library, now: f64) {
        let Some(view) = self.ws.text.as_mut() else { return };
        let gone = self.ws.viewer.as_deref() != Some(view.id.as_str());
        if (gone || view.save_now || view.autosave.due(now))
            && let Err(e) = view.save(lib)
        {
            self.flash_error(format!("Could not save the text: {e}"));
        }
        if gone {
            self.ws.text = None;
        }
    }

    /// File ▸ New ▸ Text File, as Eagle's `newFileFromTemplate("txt")`:
    /// an empty file named "Untitled.txt" is imported into the open folder
    /// (taking its auto-tags), then opened in the viewer with the cursor in
    /// the text. The blank file is made in a temporary folder of its own
    /// and removed once imported.
    pub(crate) fn new_text_file(&mut self, lib: &mut Library, folders: Vec<String>) -> falcon_library::Result<()> {
        // Typing in the text open now is saved before it is replaced.
        if let Some(open) = &mut self.ws.text {
            open.save(lib)?;
        }
        let dir = std::env::temp_dir().join("falcon-new").join(falcon_library::ids::new_id());
        let file = dir.join(textdoc::NEW_FILE_NAME);
        let made = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&file, b""));
        let id = made.map_err(|source| falcon_library::Error::Io { path: file.clone(), source }).and_then(|()| lib.import_file(&file, &self.prefs.import_options(folders)));
        let _ = std::fs::remove_dir_all(&dir);
        let id = id?;
        let mut view = TextView::open(&id, &lib.item(&id).map(|i| i.original_path(lib.root())).unwrap_or_default());
        view.focus = true;
        self.ws.text = Some(view);
        self.ws.viewer = Some(id.clone());
        self.ws.selected = vec![id.clone()];
        self.ws.anchor = Some(id.clone());
        self.ws.scroll_to = Some(id);
        Ok(())
    }
}
