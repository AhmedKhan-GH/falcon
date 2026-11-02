//! The Emoji submenu of the folder and smart folder menus: a grid of the
//! curated emoji (`folder_emoji.rs`) drawn from their textures, and Remove
//! Emoji. A falcon addition.

use eframe::egui::{self, Sense, Ui, Vec2};

use crate::app::App;
use crate::folder_emoji::CURATED;
use crate::locale::tr;
use crate::theme as t;

const CELL: f32 = 28.0;
const PER_ROW: f32 = 8.0;

/// The smart folder `id` in a tree of them.
pub(crate) fn find_smart<'a>(list: &'a [serde_json::Value], id: &str) -> Option<&'a serde_json::Value> {
    list.iter().find_map(|v| {
        if v["id"].as_str() == Some(id) {
            return Some(v);
        }
        find_smart(v["children"].as_array()?, id)
    })
}

impl App {
    /// The "Emoji" submenu for the folders or smart folders `ids`. The
    /// current emoji is marked only when all of them share it.
    pub(crate) fn emoji_submenu(&mut self, ui: &mut Ui, ids: &[String]) {
        crate::ui::menus::search::submenu(ui, tr("Emoji"), |ui| {
            let store = &self.folder_emoji.store;
            let any = ids.iter().any(|id| store.get(id).is_some());
            let current = ids.first().and_then(|id| store.get(id)).filter(|c| ids.iter().all(|id| store.get(id) == Some(*c)));
            if any && ui.button(tr("Remove Emoji")).clicked() {
                for id in ids {
                    self.folder_emoji.store.clear(id);
                }
                ui.close();
            }
            if let Some(c) = self.emoji_grid(ui, current) {
                for id in ids {
                    self.folder_emoji.store.set(id, c);
                }
                ui.close();
            }
        });
    }

    /// A duplicated folder and its subfolders take the original's emoji.
    /// Both trees have the same shape.
    pub(crate) fn copy_folder_emoji(&mut self, from: &falcon_library::model::Folder, to: &falcon_library::model::Folder) {
        self.folder_emoji.store.copy(&from.id, &to.id);
        for (a, b) in from.children.iter().zip(&to.children) {
            self.copy_folder_emoji(a, b);
        }
    }

    /// The same for a duplicated smart folder.
    pub(crate) fn copy_smart_emoji(&mut self, from: &serde_json::Value, to: &serde_json::Value) {
        if let (Some(a), Some(b)) = (from["id"].as_str(), to["id"].as_str()) {
            self.folder_emoji.store.copy(a, b);
        }
        if let (Some(a), Some(b)) = (from["children"].as_array(), to["children"].as_array()) {
            for (x, y) in a.iter().zip(b) {
                self.copy_smart_emoji(x, y);
            }
        }
    }

    /// The grid of emoji; drawing it starts reading the font. Returns the pick.
    fn emoji_grid(&mut self, ui: &mut Ui, current: Option<char>) -> Option<char> {
        let ctx = ui.ctx().clone();
        let mut pick = None;
        // Filtering: one button per emoji, so the search finds it by name.
        if crate::ui::menus::search::filtering(ui) {
            for (c, name) in CURATED {
                let clicked = match self.folder_emoji.texture(&ctx, *c) {
                    Some(tex) => ui.add(egui::Button::image_and_text(egui::Image::new(&tex).fit_to_exact_size(Vec2::splat(16.0)), tr(name))).clicked(),
                    None => ui.button(tr(name)).clicked(),
                };
                if clicked {
                    pick = Some(*c);
                }
            }
            return pick;
        }
        egui::ScrollArea::vertical().id_salt("emoji-grid").max_height(320.0).show(ui, |ui| {
            ui.set_width(PER_ROW * (CELL + 4.0));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                for (c, name) in CURATED {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(CELL), Sense::click());
                    if Some(*c) == current {
                        ui.painter().rect_filled(rect, t::RADIUS, t::p().row_selected);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(rect, t::RADIUS, t::p().row_hover);
                    }
                    if let Some(tex) = self.folder_emoji.texture(&ctx, *c) {
                        egui::Image::new(&tex).paint_at(ui, egui::Rect::from_center_size(rect.center(), Vec2::splat(20.0)));
                    }
                    if resp.clicked() {
                        pick = Some(*c);
                    }
                    resp.on_hover_text(tr(name));
                }
            });
        });
        pick
    }
}
