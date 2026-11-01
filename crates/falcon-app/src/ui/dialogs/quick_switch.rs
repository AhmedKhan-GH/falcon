//! Go to Folder: Eagle's quick switcher, a dialog near the top of the
//! window with Folders, Tags, Smart Folders and Items tabs. The rows each
//! tab lists come from `crate::quick_switch`.

use eframe::egui::{self, Align2, CornerRadius, Key, Modifiers, Pos2, Rect, RichText, Sense, TextEdit, Vec2, vec2};
use falcon_library::Library;

use crate::app::App;
use crate::logic::{self, Scope};
use crate::theme::{self as t, font};
use crate::ui::widgets::{galley};
use crate::folder_icons;
use crate::quick_switch::{self as qs, Entry, Tab};
use crate::search_state::push_history;

const WIDTH: f32 = 480.0;
const ROW_H: f32 = 28.0;
const LIST_H: f32 = 340.0;
const INDENT: f32 = 14.0;

/// The switcher's state while it is open.
#[derive(Default)]
pub(crate) struct QuickSwitch {
    open: bool,
    tab: Tab,
    keyword: String,
    index: usize,
    /// Put the cursor in the search field, text selected, next frame.
    focus: bool,
    /// Scroll the highlighted row into view next frame.
    reveal: bool,
    /// The rows for `(tab, keyword)`, kept until either changes.
    cache: Option<(Tab, String, Vec<Entry>)>,
    /// Folders, tags, smart folders and items, counted when it opens.
    counts: Option<[usize; 4]>,
}

impl QuickSwitch {
    /// Opens the switcher, or closes it when it is open. It keeps the last
    /// keyword, selected, as Eagle does.
    pub(crate) fn toggle(&mut self) {
        if self.open {
            self.open = false;
        } else {
            self.open = true;
            self.focus = true;
            self.index = 0;
            self.reveal = true;
            self.cache = None;
            self.counts = None;
        }
    }

    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) fn close(&mut self) {
        self.open = false;
    }

    fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
        self.index = 0;
        self.reveal = true;
    }
}

fn rows(tab: Tab, lib: &Library, history: &crate::search_state::SearchPrefs, keyword: &str) -> Vec<Entry> {
    match tab {
        Tab::Folders => qs::nodes_list(&qs::folder_nodes(&lib.metadata.folders), &history.folder_history, keyword, qs::RECENT_FOLDERS_MIN),
        Tab::Smart => qs::nodes_list(&qs::smart_nodes(&lib.smart_folders()), &history.smart_history, keyword, qs::RECENT_SMART_MIN),
        Tab::Tags => {
            let tags: Vec<String> = logic::tag_counts(lib.items()).into_iter().map(|(t, _)| t).collect();
            qs::tags_list(&tags, &lib.metadata.tags_groups, keyword)
        }
        Tab::Items => qs::items_list(lib.items(), &lib.metadata.folders, keyword),
    }
}

impl App {
    /// Opens the highlighted row and closes the switcher.
    fn quick_switch_open(&mut self, lib: &Library, tab: Tab, e: &Entry) {
        self.ws.viewer = None;
        match tab {
            Tab::Folders => {
                self.go(Scope::Folder(e.id.clone()));
                push_history(&mut self.search.folder_history, &e.id);
            }
            Tab::Smart => {
                self.go(Scope::Smart(e.id.clone()));
                push_history(&mut self.search.smart_history, &e.id);
            }
            Tab::Tags => self.go(Scope::Tag(e.id.clone())),
            Tab::Items => {
                let folder = lib.item(&e.id).and_then(|i| i.folders.iter().find(|f| logic::find_folder(&lib.metadata.folders, f).is_some()));
                self.go(folder.map_or(Scope::All, |f| Scope::Folder(f.clone())));
                self.ws.selected = vec![e.id.clone()];
                self.ws.anchor = Some(e.id.clone());
                self.ws.scroll_to = Some(e.id.clone());
            }
        }
        self.quick.close();
    }

    pub(crate) fn quick_switch_ui(&mut self, ctx: &egui::Context, lib: &Library) {
        if !self.quick.open {
            return;
        }
        let counts = *self.quick.counts.get_or_insert_with(|| {
            let live = lib.items().iter().filter(|i| !i.is_deleted).count();
            [lib.all_folders().len(), logic::tag_counts(lib.items()).len(), qs::smart_nodes(&lib.smart_folders()).len(), live]
        });
        if self.quick.cache.as_ref().is_none_or(|(t, k, _)| *t != self.quick.tab || *k != self.quick.keyword) {
            let list = rows(self.quick.tab, lib, &self.search, &self.quick.keyword);
            self.quick.cache = Some((self.quick.tab, self.quick.keyword.clone(), list));
        }
        let len = self.quick.cache.as_ref().map_or(0, |c| c.2.len());
        self.quick.index = self.quick.index.min(len.saturating_sub(1));

        // Keys, taken before the search field sees them.
        let mut chosen = false;
        let mut close = false;
        let mut tab_to: Option<Tab> = None;
        let mut moved = false;
        ctx.input_mut(|i| {
            for (key, d) in [(Key::ArrowUp, -1), (Key::ArrowDown, 1)] {
                if i.consume_key(Modifiers::NONE, key) {
                    self.quick.index = qs::step(self.quick.index, d, len);
                    moved = true;
                }
            }
            if i.consume_key(Modifiers::SHIFT, Key::Tab) {
                tab_to = Some(self.quick.tab.prev());
            } else if i.consume_key(Modifiers::NONE, Key::Tab) {
                tab_to = Some(self.quick.tab.next());
            }
            chosen = i.consume_key(Modifiers::NONE, Key::Enter);
            close = i.consume_key(Modifiers::NONE, Key::Escape);
        });
        if let Some(t) = tab_to {
            self.quick.set_tab(t);
            self.quick.cache = None;
            ctx.request_repaint();
        }
        if moved {
            self.quick.reveal = true;
        }

        let id = egui::Id::unique("quick-switch");
        let top = egui::Modal::default_area(id).anchor(Align2::CENTER_TOP, vec2(0.0, 80.0));
        let mut clicked: Option<usize> = None;
        let modal = egui::Modal::new(id).area(top).show(ctx, |ui| {
            ui.set_width(WIDTH.min(ctx.content_rect().width() - 40.0));
            let mut out = TextEdit::singleline(&mut self.quick.keyword)
                .id_salt("quick-switch-keyword")
                .hint_text("Go to a folder, tag or smart folder")
                .desired_width(f32::INFINITY)
                .show(ui);
            if self.quick.focus {
                out.response.request_focus();
                let end = egui::text::CCursor::new(self.quick.keyword.chars().count());
                out.state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), end)));
                out.state.store(ui.ctx(), out.response.id);
                self.quick.focus = false;
            }
            if out.response.changed() {
                self.quick.index = 0;
                self.quick.reveal = true;
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (n, tab) in Tab::ALL.into_iter().enumerate() {
                    let text = RichText::new(format!("{} ({})", tab.label(), counts[n])).size(12.0);
                    let text = if tab == self.quick.tab { text.color(t::p().text) } else { text.color(t::p().text_muted) };
                    if ui.add(egui::Button::new(text).selected(tab == self.quick.tab).frame(tab == self.quick.tab)).clicked() {
                        self.quick.set_tab(tab);
                        self.quick.cache = None;
                    }
                }
            });
            ui.add_space(4.0);
            let list: &[Entry] = self.quick.cache.as_ref().map_or(&[], |c| c.2.as_slice());
            if list.is_empty() {
                ui.add_space(12.0);
                ui.vertical_centered(|ui| ui.label(RichText::new(self.quick.tab.empty_text(!self.quick.keyword.is_empty())).color(t::p().text_muted)));
                ui.add_space(12.0);
                return;
            }
            let moving = ui.input(|i| i.pointer.delta() != Vec2::ZERO);
            let (index, reveal) = (self.quick.index, self.quick.reveal);
            let mut hover: Option<usize> = None;
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::ScrollArea::vertical().max_height(LIST_H).auto_shrink([false, true]).show_rows(ui, ROW_H, list.len(), |ui, range| {
                if reveal {
                    let top = ui.cursor().top() + (index as f32 - range.start as f32) * ROW_H;
                    ui.scroll_to_rect(Rect::from_min_size(Pos2::new(ui.max_rect().left(), top), vec2(ui.available_width(), ROW_H)), None);
                }
                for n in range {
                    let e = &list[n];
                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
                    if resp.hovered() && moving {
                        hover = Some(n);
                    }
                    if resp.clicked() {
                        clicked = Some(n);
                    }
                    let p = ui.painter();
                    if n == index {
                        p.rect_filled(rect, CornerRadius::same(5), t::p().row_selected);
                    } else if resp.hovered() {
                        p.rect_filled(rect, CornerRadius::same(5), t::p().row_hover);
                    }
                    let mut x = rect.left() + 8.0 + e.depth as f32 * INDENT;
                    if let Some(c) = folder_icons::color(e.color.as_deref()) {
                        p.circle_filled(Pos2::new(x + 4.0, rect.center().y), 4.0, c);
                        x += 14.0;
                    }
                    let mut right = rect.right() - 8.0;
                    if let Some(d) = &e.detail {
                        let g = galley(ui, d, font(11.0), t::p().text_muted, 140.0);
                        right -= g.size().x;
                        ui.painter().galley(Pos2::new(right, rect.center().y - g.size().y / 2.0), g, t::p().text_muted);
                        right -= 10.0;
                    }
                    if e.recent {
                        let g = galley(ui, "recent", font(10.0), t::p().text_muted, 60.0);
                        right -= g.size().x;
                        ui.painter().galley(Pos2::new(right, rect.center().y - g.size().y / 2.0), g, t::p().text_muted);
                        right -= 10.0;
                    }
                    let g = galley(ui, &e.name, font(13.0), t::p().text, right - x);
                    ui.painter().galley(Pos2::new(x, rect.center().y - g.size().y / 2.0), g, t::p().text);
                }
            });
            if let Some(n) = hover {
                self.quick.index = n;
            }
            self.quick.reveal = false;
        });
        if modal.should_close() || close {
            self.quick.close();
            return;
        }
        let pick = clicked.or(chosen.then_some(self.quick.index));
        if let Some(n) = pick {
            let entry = self.quick.cache.as_ref().and_then(|c| c.2.get(n)).cloned();
            if let Some(e) = entry {
                self.ws.query.clear();
                let tab = self.quick.tab;
                self.quick_switch_open(lib, tab, &e);
            }
        }
    }
}
