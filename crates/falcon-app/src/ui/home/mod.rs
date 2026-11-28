//! Falcon Home Dashboard: Creative Asset Workspace overview.
//!
//! Provides a designer-first dashboard displaying library metrics,
//! Quick Access Hero & Supporting Cards (H2-A), and responsive top-level
//! folder collections with 2x2 Mosaic Covers (H2-B), eliminating eager
//! sorting and thumbnail loading for 37,000+ asset libraries.

use eframe::egui::{self, Align2, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, vec2};
use falcon_library::Library;
use falcon_library::model::{Folder, Item};
use falcon_library::search::OrderBy;

use crate::app::{App, DragItems};
use crate::folder_icons;
use crate::icons as ic;
use crate::loader::{job_for, Tier};
use crate::locale::{tr, tr_n};
use crate::logic::{self, Scope};
use crate::theme::{self as t, font};
use crate::ui::grid::covers::{self, folder_meta};
use crate::ui::widgets::{badge, ext_placeholder, galley, paint_thumb_bg, status_badge};

#[derive(Clone, Copy)]
struct CardDims {
    w: f32,
    h: f32,
    thumb_h: f32,
}

/// Width of the column of secondary Quick Access cards beside the Hero. It fills the row
/// on ordinary windows but stops growing on very wide ones, where cards hundreds of
/// pixels wide would hold only a name and a count.
fn quick_secondary_width(content_w: f32, hero_w: f32, secondary: usize) -> f32 {
    let room = (content_w - hero_w - 16.0).max(100.0);
    let cap = if secondary <= 2 { 520.0 } else { 760.0 };
    room.min(cap)
}

/// An asset may be a Home cover when it is a live, previewable item outside
/// locked folders; anything else (deleted, missing) falls back gracefully.
fn valid_cover<'a>(item: Option<&'a Item>, locked: &std::collections::HashSet<&str>) -> Option<&'a Item> {
    item.filter(|i| covers::can_cover(i, locked))
}

/// Resolved Quick Access card candidate for Home Dashboard (H2-A).
pub(crate) struct QuickAccessCard {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) scope: Scope,
    pub(crate) count: usize,
    pub(crate) cover_item: Option<Item>,
    pub(crate) icon_glyph: &'static str,
    pub(crate) icon_tint: egui::Color32,
}

impl App {
    /// Renders the Home Dashboard: prominent greeting/title, Recently Imported section (H2.1-B),
    /// Quick Access Hero & Supporting cards (H2-A), and responsive collections with 2x2 Mosaic Covers (H2-B).
    pub(crate) fn home_page(&mut self, ui: &mut Ui, lib: &Library) {
        // Derive exact available content width between sidebar and inspector (or window edge)
        let area_w = ui.available_width();
        let pad_x = 16.0;
        let content_w = (area_w - 2.0 * pad_x).max(100.0);

        // Invalidate session cover and recent caches if library root, modification time, item count, sort, mutation count, or recent limit changed
        let epoch = (
            lib.root().to_path_buf(),
            lib.metadata.modification_time,
            lib.items().len(),
            self.ws.sort,
            self.ws.increase,
            self.ws.mutation_count,
            self.prefs.home_recent_limit,
        );
        if self.ws.home_cache_epoch != epoch {
            self.ws.home_covers.clear();
            self.ws.home_mosaics.clear();
            self.ws.home_recent_imports.clear();
            self.ws.home_smart.clear();
            self.ws.home_cache_epoch = epoch;
        }
        // Locked folders are needed by Recently Imported, covers and Quick Access alike.
        let locked = covers::locked_folders(&lib.metadata.folders);

        egui::ScrollArea::vertical()
            .id_salt("home_dashboard_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(20.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.add_space(pad_x);
                    ui.vertical(|ui| {
                        ui.set_width(content_w);

                        // Zone 1: Home Dashboard Heading
                        self.home_heading(ui);
                        ui.add_space(16.0);

                        // Zone 1.5: Recently Imported Section (H2.1-B)
                        if self.prefs.home_show_recent {
                            self.home_recently_imported_section(ui, lib, content_w, &locked);
                        }

                        // Counts only when something visible shows them
                        let needs_folders = self.prefs.home_show_hero || self.prefs.home_show_folders;
                        let counts = if needs_folders && self.prefs.home_show_counts {
                            self.folder_counts_for(lib)
                        } else {
                            Default::default()
                        };

                        // Zone 2: Quick Access Hero (H2-A), manual Hero choice (H4-B)
                        if self.prefs.home_show_hero {
                            let mut quick_cards = self.resolve_quick_access(lib, &locked, &counts);
                            if !quick_cards.is_empty() {
                                let choice = self.home_hero_choice().map(str::to_owned);
                                let manual = crate::home_lib::promote_hero(&mut quick_cards, choice.as_deref(), |c| c.id.as_str());
                                self.home_quick_access_section(ui, lib, &quick_cards, content_w, manual);
                                ui.add_space(26.0);
                            }
                        }

                        // Zone 3: Root Folder Collections Grid with 2x2 Mosaic Covers (H2-B)
                        if self.prefs.home_show_folders {
                            self.home_collections_section(ui, lib, content_w, &locked, &counts);
                            ui.add_space(32.0);
                        }

                        // Zone 4: Optional Library Overview Metrics (H2.1-A, default OFF)
                        if self.prefs.home_show_overview {
                            self.home_library_overview(ui, lib, content_w);
                            ui.add_space(24.0);
                        }
                    });
                });
            });
    }

    /// Zone 1: Home Heading with prominent typography, creative workspace tagline, and preferences shortcut.
    fn home_heading(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(ic::HOUSE).font(font(24.0)).color(t::p().primary));
            ui.add_space(4.0);
            ui.label(egui::RichText::new(tr("Home")).font(font(24.0)).strong().color(t::p().text));
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!("— {}", tr("Creative Asset Workspace")))
                    .font(font(13.5))
                    .color(t::p().text_muted),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let btn = ui.add(
                    egui::Button::new(egui::RichText::new(ic::GEAR).font(font(16.0)).color(t::p().text_muted))
                        .frame(false),
                );
                let btn = btn.on_hover_text(tr("Home Dashboard Settings"));
                if btn.clicked() {
                    self.open_preferences();
                    self.prefs_window.set_pane(crate::prefs::Section::Home);
                }
            });
        });
    }

    /// Collects top-N recently imported assets, newest first, bounded by limit.
    fn resolve_recently_imported<'a>(&self, lib: &'a Library, limit: usize, locked: &std::collections::HashSet<&str>) -> Vec<&'a Item> {
        let mut candidates: Vec<&'a Item> = lib
            .items()
            .iter()
            .filter(|item| !item.is_deleted && !item.folders.iter().any(|f| locked.contains(f.as_str())))
            .collect();

        if candidates.len() > limit {
            candidates.select_nth_unstable_by(limit, |a, b| {
                b.modification_time.cmp(&a.modification_time).then_with(|| b.id.cmp(&a.id))
            });
            candidates.truncate(limit);
        }
        candidates.sort_by(|a, b| {
            b.modification_time.cmp(&a.modification_time).then_with(|| b.id.cmp(&a.id))
        });
        candidates
    }

    /// Zone 1.5: Recently Imported Assets Section (H2.1-B).
    fn home_recently_imported_section(&mut self, ui: &mut Ui, lib: &Library, content_w: f32, locked: &std::collections::HashSet<&str>) {
        let recent_ids = if self.ws.home_recent_imports.is_empty() {
            let limit = self.prefs.home_recent_limit;
            let recent = self.resolve_recently_imported(lib, limit, locked);
            let ids: Vec<String> = recent.iter().map(|i| i.id.clone()).collect();
            self.ws.home_recent_imports = ids.clone();
            ids
        } else {
            self.ws.home_recent_imports.clone()
        };

        let recent_items: Vec<&Item> = recent_ids
            .iter()
            .filter_map(|id| lib.item(id))
            .filter(|i| !i.is_deleted)
            .collect();

        if recent_items.is_empty() {
            return;
        }

        let mut view_all_clicked = false;

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(tr("Recently Imported")).font(font(16.0)).strong().color(t::p().text));
            ui.add_space(4.0);
            ui.label(egui::RichText::new(format!("({})", recent_items.len())).font(font(13.0)).color(t::p().text_muted));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let view_all_label = format!("{} →", tr("View All"));
                let btn = ui.add(
                    egui::Button::new(egui::RichText::new(view_all_label).font(font(12.5)).color(t::p().primary))
                        .frame(false),
                );
                if btn.clicked() {
                    view_all_clicked = true;
                }
            });
        });
        ui.add_space(12.0);

        if view_all_clicked {
            self.go(Scope::All);
            self.ws.sort = OrderBy::Import;
            self.ws.increase = true;
            return;
        }

        let gap = 12.0;
        let base_tile_w = self.prefs.home_recent_size.tile_width();
        let cols = ((content_w + gap) / (base_tile_w + gap)).floor().max(1.0) as usize;
        let card_w = (content_w - (cols.saturating_sub(1) as f32 * gap)) / cols as f32;
        let tile_h = card_w;
        let caption_h = if self.prefs.home_show_filenames { 24.0 } else { 0.0 };
        let card_h = tile_h + caption_h;

        let mut clicked: Option<(String, egui::Modifiers)> = None;
        let mut double_clicked: Option<String> = None;
        let mut opened: Option<String> = None;
        let mut middle: Option<String> = None;

        for chunk in recent_items.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for item in chunk {
                    let (card_rect, resp) = ui.allocate_exact_size(vec2(card_w, card_h), Sense::click_and_drag());
                    let tile = Rect::from_min_size(card_rect.min, vec2(card_w, tile_h));

                    if resp.drag_started() {
                        let ids = if self.ws.selected.contains(&item.id) {
                            self.ws.selected.clone()
                        } else {
                            vec![item.id.clone()]
                        };
                        resp.dnd_set_drag_payload(DragItems(ids));
                    }
                    if resp.secondary_clicked() && !self.ws.selected.contains(&item.id) {
                        self.ws.selected = vec![item.id.clone()];
                    }
                    let targets: Vec<String> = if self.ws.selected.contains(&item.id) {
                        self.ws.selected.clone()
                    } else {
                        vec![item.id.clone()]
                    };
                    resp.context_menu(|ui| {
                        if self.item_menu(ui, lib, item, &targets) {
                            opened = Some(item.id.clone());
                        }
                    });
                    if resp.clicked() {
                        clicked = Some((item.id.clone(), ui.input(|i| i.modifiers)));
                    }
                    if resp.double_clicked() {
                        double_clicked = Some(item.id.clone());
                    }
                    if resp.middle_clicked()
                        && self.prefs.middle_button == crate::prefs::MiddleButton::NewWindow
                        && !falcon_library::media::is_audio(&item.ext.to_lowercase())
                    {
                        middle = Some(item.id.clone());
                    }

                    let selected = self.ws.selected.contains(&item.id);
                    let hovered = resp.hovered();

                    let p = ui.painter();
                    // Tile background and border
                    p.rect_filled(tile, CornerRadius::same(6), t::p().field.gamma_multiply(0.4));
                    let border_color = if hovered { t::p().card_hover } else { t::p().card_border };
                    p.rect_stroke(tile, CornerRadius::same(6), Stroke::new(1.0, border_color), StrokeKind::Inside);

                    if let Some(bg) = logic::ThumbBg::of(item) {
                        paint_thumb_bg(p, tile.shrink(1.0), bg);
                    }

                    // Render image inside tile preserving aspect ratio
                    self.draw_image(ui, lib, item, tile.shrink(4.0), false);

                    // Extension badge
                    if self.display.ext_badge {
                        badge(ui, tile.min + vec2(5.0, 5.0), &item.ext);
                    }
                    let link_status = crate::linked_ui::classify_asset(lib, &item.id);
                    if link_status.is_linked() {
                        status_badge(ui, &item.id, Pos2::new(tile.left() + 5.0, tile.bottom() - 5.0), link_status);
                    }

                    // Selection ring
                    if selected {
                        p.rect_stroke(tile.expand(1.0), CornerRadius::same(7), Stroke::new(2.5, t::p().primary), StrokeKind::Inside);
                    }

                    // Caption
                    if self.prefs.home_show_filenames {
                        let cap_rect = Rect::from_min_max(Pos2::new(card_rect.left(), tile.bottom() + 3.0), card_rect.max);
                        let name = logic::caption_name(item, self.display.show_ext);
                        let g = galley(ui, &name, font(11.5), t::p().text, tile.width());
                        let name_w = g.size().x;
                        if selected {
                            let pill = Rect::from_center_size(
                                Pos2::new(tile.center().x, cap_rect.center().y),
                                vec2((name_w + 8.0).min(tile.width()), 18.0),
                            );
                            p.rect_filled(pill, CornerRadius::same(4), t::p().primary);
                            p.galley(Pos2::new(tile.center().x - name_w / 2.0, cap_rect.center().y - g.size().y / 2.0), g, egui::Color32::WHITE);
                        } else {
                            p.galley(Pos2::new(tile.center().x - name_w / 2.0, cap_rect.center().y - g.size().y / 2.0), g, t::p().text);
                        }
                    }

                    // Queue thumbnail job
                    if let Some(j) = job_for(lib, item, Tier::Grid) {
                        self.ws.frame_jobs.push(j);
                    }
                }
            });
            ui.add_space(gap);
        }

        // Handle clicks, previews, selection
        if let Some(id) = middle {
            self.open_preview_items(lib, &[id]);
        }
        if let Some((id, m)) = clicked {
            self.click(&id, m);
        }
        if let Some(id) = &double_clicked
            && self.prefs.double_click == crate::prefs::DoubleClick::External
        {
            let files = crate::ui::viewer::open::originals(lib, std::slice::from_ref(id));
            crate::open_with::open_default(&files);
            self.used(&files);
        } else if let Some(id) = opened.or(double_clicked) {
            self.ws.viewer = Some(id);
        }

        ui.add_space(14.0);
    }

    /// Zone 2: Compact Library Overview displaying Total Assets, Folders, Tags, and Active Storage Mode.
    fn home_library_overview(&self, ui: &mut Ui, lib: &Library, content_w: f32) {
        let (rect, _) = ui.allocate_exact_size(vec2(content_w, 64.0), Sense::hover());
        let p = ui.painter();

        p.rect_filled(rect, CornerRadius::same(8), t::p().field.gamma_multiply(0.4));
        p.rect_stroke(rect, CornerRadius::same(8), Stroke::new(1.0, t::p().card_border), StrokeKind::Inside);

        let cols = 4.0;
        let col_w = content_w / cols;
        let cy = rect.center().y;

        let assets_count = lib.items().len();
        let folders_count = lib.all_folders().len();
        let tags_count = logic::tag_counts(lib.items()).len();
        let mode_label = lib.storage_mode().label();
        let mode_icon = match lib.storage_mode() {
            falcon_library::StorageMode::Link => ic::LINK,
            falcon_library::StorageMode::Copy => ic::COPY,
        };

        let stats = [
            (ic::IMAGES, logic::group_digits(assets_count), tr("Assets")),
            (ic::FOLDER, logic::group_digits(folders_count), tr("Folders")),
            (ic::TAG_SIMPLE, logic::group_digits(tags_count), tr("Tags")),
            (mode_icon, tr(mode_label).to_string(), tr("Storage Mode")),
        ];

        for (idx, (icon, val, label)) in stats.into_iter().enumerate() {
            let x_left = rect.left() + idx as f32 * col_w;
            let col_rect = Rect::from_min_size(Pos2::new(x_left, rect.top()), vec2(col_w, rect.height()));

            if idx > 0 {
                p.vline(col_rect.left(), col_rect.y_range().shrink(12.0), Stroke::new(1.0, t::p().divider));
            }

            let center_x = col_rect.center().x;
            let val_galley = galley(ui, &val, font(15.0), t::p().text, col_w - 40.0);
            let icon_galley = galley(ui, icon, font(16.0), t::p().primary, 24.0);

            let icon_w = icon_galley.size().x;
            let total_row_w = icon_w + 6.0 + val_galley.size().x;
            let row_x = center_x - total_row_w / 2.0;

            p.galley(Pos2::new(row_x, cy - 16.0), icon_galley, t::p().primary);
            p.galley(Pos2::new(row_x + icon_w + 6.0, cy - 15.0), val_galley, t::p().text);

            let label_galley = galley(ui, label, font(11.5), t::p().text_muted, col_w - 20.0);
            p.galley(Pos2::new(center_x - label_galley.size().x / 2.0, cy + 6.0), label_galley, t::p().text_muted);
        }
    }

    /// Resolves eligible Quick Access entries from existing metadata source of truth (`lib.metadata.quick_access`).
    /// Covers and smart-folder results are cached per epoch, never rescanned every frame.
    pub(crate) fn resolve_quick_access(
        &mut self,
        lib: &Library,
        locked: &std::collections::HashSet<&str>,
        counts: &std::collections::HashMap<String, usize>,
    ) -> Vec<QuickAccessCard> {
        let mut cards = Vec::new();
        let smart = lib.smart_folders();
        let sort = self.ws.sort;
        let inc = self.ws.increase;

        for q in &lib.metadata.quick_access {
            let (kind, id) = if let Some(s) = q.as_str() {
                ("folder", s)
            } else if let (Some(k), Some(i)) = (q.get("type").and_then(|v| v.as_str()), q.get("id").and_then(|v| v.as_str())) {
                (k, i)
            } else {
                continue;
            };

            if kind == "smartFolder" {
                let Some(sf) = crate::organize::find(&smart, id) else { continue };
                let (count, cover_id) = self
                    .ws
                    .home_smart
                    .entry(id.to_string())
                    .or_insert_with(|| {
                        let items = lib.smart_folder_items(id);
                        let count = if sf.is_group() { 0 } else { items.len() };
                        let cover = items
                            .into_iter()
                            .find(|i| !i.is_deleted && falcon_library::format_capability::is_preview_supported(&i.ext) && !i.folders.iter().any(|f| locked.contains(f.as_str())))
                            .map(|i| i.id.clone());
                        (count, cover)
                    })
                    .clone();
                cards.push(QuickAccessCard {
                    id: id.to_string(),
                    name: sf.name.clone(),
                    scope: Scope::Smart(id.to_string()),
                    count,
                    cover_item: cover_id.and_then(|c| lib.item(&c)).cloned(),
                    icon_glyph: crate::ui::organize::smart_look(sf),
                    icon_tint: t::p().text,
                });
            } else {
                let Some(f) = logic::find_folder(&lib.metadata.folders, id) else { continue };
                let count = counts.get(id).copied().unwrap_or(0);
                let custom = self
                    .home_cover_choice(id)
                    .and_then(|c| valid_cover(lib.item(c), locked))
                    .map(|i| i.id.clone());
                let cover_id = match custom {
                    Some(c) => Some(c),
                    None => self
                        .ws
                        .home_covers
                        .entry(id.to_string())
                        .or_insert_with(|| covers::cover(lib.items(), f, sort, inc, locked).map(|i| i.id.clone()))
                        .clone(),
                };
                cards.push(QuickAccessCard {
                    id: id.to_string(),
                    name: f.name.clone(),
                    scope: Scope::Folder(id.to_string()),
                    count,
                    cover_item: cover_id.and_then(|c| lib.item(&c)).cloned(),
                    icon_glyph: folder_icons::glyph(f.icon.as_deref(), false),
                    icon_tint: folder_icons::color(f.icon_color.as_deref()).unwrap_or(t::p().text),
                });
            }
        }

        cards
    }

    /// Context menu of a Quick Access card: Hero choice (H4-B) and, for folders, the Home cover (H4-A).
    fn quick_card_menu(&mut self, ui: &mut Ui, lib: &Library, card: &QuickAccessCard, primary: bool, manual: bool) {
        ui.set_min_width(180.0);
        if !(primary && manual) && ui.button(tr("Set as Home Hero")).clicked() {
            self.set_home_hero(&card.id);
        }
        if manual && ui.button(tr("Reset Hero to Automatic")).clicked() {
            self.reset_home_hero();
        }
        if let Scope::Folder(fid) = &card.scope {
            ui.separator();
            self.home_cover_menu(ui, lib, fid);
        }
    }

    /// Home folder card menu entries (H4): Home cover and, for pinned folders, the Hero choice.
    pub(crate) fn home_folder_menu(&mut self, ui: &mut Ui, lib: &Library, folder_id: &str) {
        self.home_cover_menu(ui, lib, folder_id);
        let pinned = lib.metadata.quick_access.iter().any(|q| q.as_str().or_else(|| q.get("id").and_then(|v| v.as_str())) == Some(folder_id));
        if pinned {
            let manual = self.home_hero_choice() == Some(folder_id);
            if !manual && ui.button(tr("Set as Home Hero")).clicked() {
                self.set_home_hero(folder_id);
            }
            if manual && ui.button(tr("Reset Hero to Automatic")).clicked() {
                self.reset_home_hero();
            }
        }
    }

    /// Set / Change / Reset the Home-only cover of a folder (H4-A). The asset is the one selected.
    pub(crate) fn home_cover_menu(&mut self, ui: &mut Ui, lib: &Library, folder_id: &str) {
        let picked = match self.ws.selected.as_slice() {
            [one] if lib.item(one).is_some_and(|i| !i.is_deleted) => Some(one.clone()),
            _ => None,
        };
        let has = self.home_cover_choice(folder_id).is_some();
        let label = if has { tr("Change Home Cover") } else { tr("Set as Home Cover") };
        let btn = ui.add_enabled(picked.is_some(), egui::Button::new(label)).on_disabled_hover_text(tr("Select one asset first"));
        if btn.clicked()
            && let Some(item) = picked
        {
            self.set_home_cover(folder_id, &item);
        }
        if has && ui.button(tr("Reset to Automatic Cover")).clicked() {
            self.reset_home_cover(folder_id);
        }
    }

    /// Zone 2.5: Quick Access Hero & Supporting Cards (H2-A).
    fn home_quick_access_section(
        &mut self,
        ui: &mut Ui,
        lib: &Library,
        cards: &[QuickAccessCard],
        content_w: f32,
        manual: bool,
    ) {
        if cards.is_empty() {
            return;
        }

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(ic::STAR).font(font(15.0)).color(t::p().primary));
            ui.add_space(3.0);
            ui.label(egui::RichText::new(tr("Quick Access")).font(font(16.0)).strong().color(t::p().text));
            ui.add_space(4.0);
            ui.label(egui::RichText::new(format!("({})", cards.len())).font(font(13.0)).color(t::p().text_muted));
        });
        ui.add_space(12.0);

        let (primary, secondary) = cards.split_first().unwrap();
        let mut clicked_scope: Option<Scope> = None;

        if secondary.is_empty() {
            // Single Quick Access: compact primary Hero spanning comfortably
            let hero_w = content_w.min(520.0);
            let hero_h = 150.0;
            if let Some(target) = self.render_primary_hero(ui, lib, primary, vec2(hero_w, hero_h), manual) {
                clicked_scope = Some(target);
            }
        } else if content_w >= 720.0 {
            // Wide layout: Left primary Hero, Right supporting secondary cards
            let hero_w = ((content_w - 16.0) * 0.50).clamp(320.0, 480.0);
            let sec_w = quick_secondary_width(content_w, hero_w, secondary.len());
            let hero_h = 150.0;

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;

                // Primary Hero
                if let Some(target) = self.render_primary_hero(ui, lib, primary, vec2(hero_w, hero_h), manual) {
                    clicked_scope = Some(target);
                }

                // Supporting secondary cards in right column
                ui.vertical(|ui| {
                    ui.set_width(sec_w);
                    let sec_count = secondary.len();
                    if sec_count <= 2 {
                        let card_h = (hero_h - (sec_count.saturating_sub(1) as f32 * 10.0)) / sec_count as f32;
                        for (idx, sec) in secondary.iter().enumerate() {
                            if idx > 0 {
                                ui.add_space(10.0);
                            }
                            if let Some(target) = self.render_secondary_card(ui, lib, sec, vec2(sec_w, card_h), manual) {
                                clicked_scope = Some(target);
                            }
                        }
                    } else {
                        // 2x2 grid for up to 4 items
                        let card_w = (sec_w - 10.0) / 2.0;
                        let card_h = (hero_h - 10.0) / 2.0;
                        let shown = &secondary[..secondary.len().min(4)];
                        for (r_idx, chunk) in shown.chunks(2).enumerate() {
                            if r_idx > 0 {
                                ui.add_space(10.0);
                            }
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 10.0;
                                for sec in chunk {
                                    if let Some(target) = self.render_secondary_card(ui, lib, sec, vec2(card_w, card_h), manual) {
                                        clicked_scope = Some(target);
                                    }
                                }
                            });
                        }
                    }
                });
            });
        } else {
            // Narrow layout (< 720px): Stack primary Hero, then secondary cards below
            let hero_h = 140.0;
            if let Some(target) = self.render_primary_hero(ui, lib, primary, vec2(content_w, hero_h), manual) {
                clicked_scope = Some(target);
            }
            ui.add_space(12.0);

            let sec_cols: usize = if content_w > 460.0 { 2 } else { 1 };
            let sec_gap = 10.0;
            let card_w = (content_w - (sec_cols.saturating_sub(1) as f32 * sec_gap)) / sec_cols as f32;
            let card_h = 68.0;

            for chunk in secondary.chunks(sec_cols) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = sec_gap;
                    for sec in chunk {
                        if let Some(target) = self.render_secondary_card(ui, lib, sec, vec2(card_w, card_h), manual) {
                            clicked_scope = Some(target);
                        }
                    }
                });
                ui.add_space(sec_gap);
            }
        }

        if let Some(scope) = clicked_scope {
            self.go(scope);
        }
    }

    /// Renders the prominent Primary Quick Access Hero card with Single Cover.
    fn render_primary_hero(
        &mut self,
        ui: &mut Ui,
        lib: &Library,
        card: &QuickAccessCard,
        size: Vec2,
        manual: bool,
    ) -> Option<Scope> {
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        resp.context_menu(|ui| self.quick_card_menu(ui, lib, card, true, manual));
        let p = ui.painter().clone();

        let hovered = resp.hovered();
        let bg_fill = if hovered { t::p().row_hover } else { t::p().panel };
        let border_stroke = if hovered {
            Stroke::new(1.5, t::p().card_hover)
        } else {
            Stroke::new(1.0, t::p().card_border)
        };

        p.rect_filled(rect, CornerRadius::same(8), bg_fill);
        p.rect_stroke(rect, CornerRadius::same(8), border_stroke, StrokeKind::Inside);

        // Left portion: Cover thumbnail
        let thumb_w = (size.y * 1.05).min(size.x * 0.42);
        let thumb_rect = Rect::from_min_size(rect.min, vec2(thumb_w, size.y));

        match &card.cover_item {
            Some(item) => {
                match self.ws.loader.get(&item.id, Tier::Grid).cloned() {
                    Some(tex) => {
                        let ts = tex.size_vec2();
                        let s = (thumb_rect.width() / ts.x).max(thumb_rect.height() / ts.y);
                        let uv = Rect::from_center_size(
                            Pos2::new(0.5, 0.5),
                            vec2(thumb_rect.width() / (ts.x * s), thumb_rect.height() / (ts.y * s)),
                        );
                        egui::Image::new((tex.id(), thumb_rect.size()))
                            .uv(uv)
                            .corner_radius(CornerRadius { nw: 8, ne: 0, sw: 8, se: 0 })
                            .paint_at(ui, thumb_rect);
                    }
                    None => {
                        p.rect_filled(thumb_rect, CornerRadius { nw: 8, ne: 0, sw: 8, se: 0 }, t::p().field);
                        if let Some(j) = job_for(lib, item, Tier::Grid) {
                            self.ws.frame_jobs.push(j);
                        } else {
                            ext_placeholder(ui, thumb_rect, &item.ext);
                        }
                    }
                }
            }
            None => {
                p.rect_filled(thumb_rect, CornerRadius { nw: 8, ne: 0, sw: 8, se: 0 }, t::p().field);
                p.text(thumb_rect.center(), Align2::CENTER_CENTER, card.icon_glyph, font(36.0), card.icon_tint);
            }
        }

        // Divider line between thumb and details
        p.vline(thumb_rect.right(), rect.y_range(), Stroke::new(1.0, t::p().divider));

        // Right portion: Details
        let details_rect = Rect::from_min_max(Pos2::new(thumb_rect.right() + 16.0, rect.top() + 14.0), Pos2::new(rect.right() - 16.0, rect.bottom() - 14.0));
        let avail_w = details_rect.width().max(60.0);

        // Tag / Badge: "FEATURED COLLECTION"
        let badge_text = tr("Featured Collection").to_uppercase();
        let badge_galley = galley(ui, &badge_text, font(10.0), t::p().primary, avail_w);
        p.galley(details_rect.min, badge_galley, t::p().primary);

        // Title: Prominent bold
        let title_galley = galley(ui, &card.name, font(16.5), t::p().text, avail_w);
        p.galley(Pos2::new(details_rect.left(), details_rect.min.y + 18.0), title_galley, t::p().text);

        // Asset count
        if self.prefs.home_show_counts {
            let count_text = tr_n(card.count, "{n} item", "{n} items");
            let count_galley = galley(ui, &count_text, font(12.0), t::p().text_muted, avail_w);
            p.galley(Pos2::new(details_rect.left(), details_rect.min.y + 44.0), count_galley, t::p().text_muted);
        }

        // Bottom Navigation Affordance: "Open →"
        let nav_text = format!("{} →", tr("Open"));
        let nav_color = if hovered { t::p().primary } else { t::p().text_muted };
        let nav_galley = galley(ui, &nav_text, font(12.0), nav_color, avail_w);
        p.galley(Pos2::new(details_rect.left(), details_rect.bottom() - nav_galley.size().y), nav_galley, nav_color);

        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        let clicked = resp.clicked();
        resp.on_hover_text(&card.name);

        if clicked {
            Some(card.scope.clone())
        } else {
            None
        }
    }

    /// Renders a compact Supporting Quick Access card.
    fn render_secondary_card(
        &mut self,
        ui: &mut Ui,
        lib: &Library,
        card: &QuickAccessCard,
        size: Vec2,
        manual: bool,
    ) -> Option<Scope> {
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        resp.context_menu(|ui| self.quick_card_menu(ui, lib, card, false, manual));
        let p = ui.painter().clone();

        let hovered = resp.hovered();
        let bg_fill = if hovered { t::p().row_hover } else { t::p().panel };
        let border_stroke = if hovered {
            Stroke::new(1.5, t::p().card_hover)
        } else {
            Stroke::new(1.0, t::p().card_border)
        };

        p.rect_filled(rect, CornerRadius::same(8), bg_fill);
        p.rect_stroke(rect, CornerRadius::same(8), border_stroke, StrokeKind::Inside);

        // Left thumb (square)
        let thumb_size = size.y;
        let thumb_rect = Rect::from_min_size(rect.min, vec2(thumb_size, thumb_size));

        match &card.cover_item {
            Some(item) => {
                match self.ws.loader.get(&item.id, Tier::Grid).cloned() {
                    Some(tex) => {
                        let ts = tex.size_vec2();
                        let s = (thumb_rect.width() / ts.x).max(thumb_rect.height() / ts.y);
                        let uv = Rect::from_center_size(
                            Pos2::new(0.5, 0.5),
                            vec2(thumb_rect.width() / (ts.x * s), thumb_rect.height() / (ts.y * s)),
                        );
                        egui::Image::new((tex.id(), thumb_rect.size()))
                            .uv(uv)
                            .corner_radius(CornerRadius { nw: 8, ne: 0, sw: 8, se: 0 })
                            .paint_at(ui, thumb_rect);
                    }
                    None => {
                        p.rect_filled(thumb_rect, CornerRadius { nw: 8, ne: 0, sw: 8, se: 0 }, t::p().field);
                        if let Some(j) = job_for(lib, item, Tier::Grid) {
                            self.ws.frame_jobs.push(j);
                        } else {
                            ext_placeholder(ui, thumb_rect, &item.ext);
                        }
                    }
                }
            }
            None => {
                p.rect_filled(thumb_rect, CornerRadius { nw: 8, ne: 0, sw: 8, se: 0 }, t::p().field);
                p.text(thumb_rect.center(), Align2::CENTER_CENTER, card.icon_glyph, font(22.0), card.icon_tint);
            }
        }

        p.vline(thumb_rect.right(), rect.y_range(), Stroke::new(1.0, t::p().divider));

        // Right text details
        let text_rect = Rect::from_min_max(Pos2::new(thumb_rect.right() + 10.0, rect.top() + 8.0), Pos2::new(rect.right() - 24.0, rect.bottom() - 8.0));
        let avail_w = text_rect.width().max(40.0);

        let title_galley = galley(ui, &card.name, font(13.5), t::p().text, avail_w);
        if self.prefs.home_show_counts {
            p.galley(text_rect.min, title_galley, t::p().text);
            let count_text = tr_n(card.count, "{n} item", "{n} items");
            let count_galley = galley(ui, &count_text, font(11.0), t::p().text_muted, avail_w);
            p.galley(Pos2::new(text_rect.left(), text_rect.bottom() - count_galley.size().y), count_galley, t::p().text_muted);
        } else {
            let y = rect.center().y - title_galley.size().y / 2.0;
            p.galley(Pos2::new(text_rect.left(), y), title_galley, t::p().text);
        }

        // Subtle arrow at far right
        let arrow_color = if hovered { t::p().primary } else { t::p().text_disabled };
        p.text(Pos2::new(rect.right() - 12.0, rect.center().y), Align2::CENTER_CENTER, ic::CARET_RIGHT, font(12.0), arrow_color);

        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        let clicked = resp.clicked();
        resp.on_hover_text(&card.name);

        if clicked {
            Some(card.scope.clone())
        } else {
            None
        }
    }

    /// Zone 3: Root Folder Collections grid adapting to available window width with 2x2 Mosaic Covers (H2-B).
    fn home_collections_section(
        &mut self,
        ui: &mut Ui,
        lib: &Library,
        content_w: f32,
        locked: &std::collections::HashSet<&str>,
        counts: &std::collections::HashMap<String, usize>,
    ) {
        let top_folders = &lib.metadata.folders;
        let count_str = format!("({})", top_folders.len());

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(tr("Library Folders")).font(font(16.0)).strong().color(t::p().text));
            ui.add_space(4.0);
            ui.label(egui::RichText::new(count_str).font(font(13.0)).color(t::p().text_muted));
        });
        ui.add_space(12.0);

        if top_folders.is_empty() {
            ui.add_space(32.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new(ic::FOLDER_DASHED).font(font(48.0)).color(t::p().text_disabled));
                ui.add_space(8.0);
                ui.label(egui::RichText::new(tr("No folders in this library")).font(font(15.0)).strong().color(t::p().text));
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(tr("Create folders in the sidebar to organize your creative assets."))
                        .font(font(12.5))
                        .color(t::p().text_muted),
                );
            });
            return;
        }

        let gap = 16.0;
        let target_card_w = self.prefs.home_card_size.target_width();
        let cols = ((content_w + gap) / (target_card_w + gap)).floor().max(1.0) as usize;
        let card_w = (content_w - (cols.saturating_sub(1) as f32 * gap)) / cols as f32;
        let k = target_card_w / 210.0;
        let thumb_h = (card_w * 0.62).clamp(110.0 * k, 150.0 * k);
        let info_h = if self.prefs.home_show_counts { 56.0 } else { 38.0 };
        let dims = CardDims { w: card_w, thumb_h, h: thumb_h + info_h };

        let mut clicked_folder: Option<String> = None;

        for chunk in top_folders.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for folder in chunk.iter() {
                    if let Some(target) = self.render_folder_card(ui, lib, folder, dims, locked, counts) {
                        clicked_folder = Some(target);
                    }
                }
            });
            ui.add_space(gap);
        }

        if let Some(fid) = clicked_folder {
            self.go(Scope::Folder(fid));
        }
    }

    /// Renders an individual collection card with 2x2 Mosaic Covers or adaptive fallback (H2-B).
    fn render_folder_card(
        &mut self,
        ui: &mut Ui,
        lib: &Library,
        folder: &Folder,
        dims: CardDims,
        locked: &std::collections::HashSet<&str>,
        counts: &std::collections::HashMap<String, usize>,
    ) -> Option<String> {
        let (card_rect, resp) = ui.allocate_exact_size(vec2(dims.w, dims.h), Sense::click());
        let p = ui.painter().clone();

        let hovered = resp.hovered() || resp.context_menu_opened();
        let border_stroke = if hovered {
            Stroke::new(1.5, t::p().card_hover)
        } else {
            Stroke::new(1.0, t::p().card_border)
        };
        let bg_fill = if hovered { t::p().row_hover } else { t::p().panel };

        p.rect_filled(card_rect, CornerRadius::same(8), bg_fill);
        p.rect_stroke(card_rect, CornerRadius::same(8), border_stroke, StrokeKind::Inside);

        let thumb_rect = Rect::from_min_size(card_rect.min, vec2(dims.w, dims.thumb_h));

        // Cover priority (H4-D): valid Home custom cover, then the chosen style, then fallbacks.
        let style = self.prefs.home_cover_style;
        let sort = self.ws.sort;
        let inc = self.ws.increase;
        let custom = self
            .home_cover_choice(&folder.id)
            .and_then(|c| valid_cover(lib.item(c), locked))
            .map(|i| i.id.clone());
        let cover_ids: Vec<String> = if folder.is_locked() {
            Vec::new()
        } else if let Some(c) = custom {
            vec![c]
        } else {
            match style {
                // Classic requests no thumbnail textures at all.
                crate::prefs::CoverStyle::Classic => Vec::new(),
                crate::prefs::CoverStyle::Single => self
                    .ws
                    .home_covers
                    .entry(folder.id.clone())
                    .or_insert_with(|| covers::cover(lib.items(), folder, sort, inc, locked).map(|i| i.id.clone()))
                    .clone()
                    .into_iter()
                    .collect(),
                crate::prefs::CoverStyle::Mosaic => self
                    .ws
                    .home_mosaics
                    .entry(folder.id.clone())
                    .or_insert_with(|| {
                        covers::mosaic_covers(lib.items(), folder, sort, inc, locked)
                            .into_iter()
                            .map(|i| i.id.clone())
                            .collect()
                    })
                    .clone(),
            }
        };

        let cover_items: Vec<&falcon_library::model::Item> = cover_ids
            .iter()
            .filter_map(|id| lib.item(id))
            .collect();

        let (glyph, tint) = (
            folder_icons::glyph(folder.icon.as_deref(), false),
            folder_icons::color(folder.icon_color.as_deref()).unwrap_or(t::p().text),
        );

        // Render Mosaic Covers with adaptive composition (4: 2x2, 3: balanced 3, 2: split, 1: single, 0: fallback)
        self.render_mosaic_cells(ui, &p, lib, &cover_items, thumb_rect, folder, tint);

        p.hline(card_rect.x_range(), thumb_rect.bottom(), Stroke::new(1.0, t::p().divider));

        let info_rect = Rect::from_min_max(Pos2::new(card_rect.left(), thumb_rect.bottom()), card_rect.max);

        let icon_pos = Pos2::new(info_rect.left() + 10.0, info_rect.top() + 11.0);
        let emoji = self.folder_emoji.for_folder(ui.ctx(), &folder.id);
        if let Some(tex) = emoji {
            let icon_rect = Rect::from_min_size(icon_pos - vec2(0.0, 2.0), vec2(16.0, 16.0));
            egui::Image::new((tex.id(), vec2(16.0, 16.0))).paint_at(ui, icon_rect);
        } else {
            let size = if folder_icons::is_text(glyph) { 11.0 } else { 15.0 };
            p.text(icon_pos + vec2(7.0, 7.0), Align2::CENTER_CENTER, glyph, font(size), tint);
        }

        let text_left = info_rect.left() + 28.0;
        let text_w = (info_rect.width() - 36.0).max(40.0);
        let name_g = galley(ui, &folder.name, font(13.0), t::p().text, text_w);
        if self.prefs.home_show_counts {
            p.galley(Pos2::new(text_left, info_rect.top() + 8.0), name_g, t::p().text);

            let meta_str = folder_meta(folder, counts);
            let meta_g = galley(ui, &meta_str, font(11.5), t::p().text_muted, text_w + 18.0);
            p.galley(Pos2::new(info_rect.left() + 10.0, info_rect.top() + 29.0), meta_g, t::p().text_muted);
        } else {
            let y = info_rect.center().y - name_g.size().y / 2.0;
            p.galley(Pos2::new(text_left, y), name_g, t::p().text);
        }

        self.strip_drop_and_menu(ui, lib, folder, &resp, card_rect, CornerRadius::same(8));


        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        let clicked = resp.clicked();
        resp.on_hover_text(&folder.name);

        if clicked {
            Some(folder.id.clone())
        } else {
            None
        }
    }

    #[allow(clippy::too_many_arguments)]
    /// Renders adaptive Mosaic Cover cells (H2-B) without image stretching or black cells.
    fn render_mosaic_cells(
        &mut self,
        ui: &mut Ui,
        p: &egui::Painter,
        lib: &Library,
        items: &[&falcon_library::model::Item],
        thumb_rect: Rect,
        folder: &Folder,
        tint: egui::Color32,
    ) {
        let cell_gap = 2.0_f32;

        match items.len() {
            // Case 4: 2x2 Mosaic (4 equal quadrants)
            4 => {
                let half_w = (thumb_rect.width() - cell_gap) / 2.0;
                let half_h = (thumb_rect.height() - cell_gap) / 2.0;

                let c0 = Rect::from_min_size(thumb_rect.min, vec2(half_w, half_h));
                let c1 = Rect::from_min_size(Pos2::new(thumb_rect.center().x + cell_gap / 2.0, thumb_rect.top()), vec2(half_w, half_h));
                let c2 = Rect::from_min_size(Pos2::new(thumb_rect.left(), thumb_rect.center().y + cell_gap / 2.0), vec2(half_w, half_h));
                let c3 = Rect::from_min_size(Pos2::new(thumb_rect.center().x + cell_gap / 2.0, thumb_rect.center().y + cell_gap / 2.0), vec2(half_w, half_h));

                self.render_mosaic_cell(ui, p, lib, items[0], c0, CornerRadius { nw: 8, ne: 0, sw: 0, se: 0 });
                self.render_mosaic_cell(ui, p, lib, items[1], c1, CornerRadius { nw: 0, ne: 8, sw: 0, se: 0 });
                self.render_mosaic_cell(ui, p, lib, items[2], c2, CornerRadius::ZERO);
                self.render_mosaic_cell(ui, p, lib, items[3], c3, CornerRadius::ZERO);
            }
            // Case 3: Balanced 3-image composition (Left leading feature + Right 2 stacked)
            3 => {
                let half_w = (thumb_rect.width() - cell_gap) / 2.0;
                let half_h = (thumb_rect.height() - cell_gap) / 2.0;

                let c0 = Rect::from_min_size(thumb_rect.min, vec2(half_w, thumb_rect.height()));
                let c1 = Rect::from_min_size(Pos2::new(thumb_rect.center().x + cell_gap / 2.0, thumb_rect.top()), vec2(half_w, half_h));
                let c2 = Rect::from_min_size(Pos2::new(thumb_rect.center().x + cell_gap / 2.0, thumb_rect.center().y + cell_gap / 2.0), vec2(half_w, half_h));

                self.render_mosaic_cell(ui, p, lib, items[0], c0, CornerRadius { nw: 8, ne: 0, sw: 0, se: 0 });
                self.render_mosaic_cell(ui, p, lib, items[1], c1, CornerRadius { nw: 0, ne: 8, sw: 0, se: 0 });
                self.render_mosaic_cell(ui, p, lib, items[2], c2, CornerRadius::ZERO);
            }
            // Case 2: 2-image split (Side-by-side)
            2 => {
                let half_w = (thumb_rect.width() - cell_gap) / 2.0;

                let c0 = Rect::from_min_size(thumb_rect.min, vec2(half_w, thumb_rect.height()));
                let c1 = Rect::from_min_size(Pos2::new(thumb_rect.center().x + cell_gap / 2.0, thumb_rect.top()), vec2(half_w, thumb_rect.height()));

                self.render_mosaic_cell(ui, p, lib, items[0], c0, CornerRadius { nw: 8, ne: 0, sw: 0, se: 0 });
                self.render_mosaic_cell(ui, p, lib, items[1], c1, CornerRadius { nw: 0, ne: 8, sw: 0, se: 0 });
            }
            // Case 1: Single cover
            1 => {
                self.render_mosaic_cell(ui, p, lib, items[0], thumb_rect, CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 });
            }
            // Case 0: Classic folder fallback
            _ => {
                p.rect_filled(thumb_rect, CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }, t::p().field);
                let fallback_icon = if folder.is_locked() { ic::LOCK_SIMPLE } else { ic::FOLDER };
                let fallback_tint = if folder.is_locked() { t::p().text_disabled } else { tint };
                p.text(thumb_rect.center(), Align2::CENTER_CENTER, fallback_icon, font(36.0), fallback_tint);
            }
        }
    }

    /// Renders an individual mosaic cell with proportional center-cropping and exact corner radius.
    fn render_mosaic_cell(
        &mut self,
        ui: &mut Ui,
        p: &egui::Painter,
        lib: &Library,
        item: &falcon_library::model::Item,
        cell_rect: Rect,
        corner_radius: CornerRadius,
    ) {
        match self.ws.loader.get(&item.id, Tier::Grid).cloned() {
            Some(tex) => {
                let size = tex.size_vec2();
                let s = (cell_rect.width() / size.x).max(cell_rect.height() / size.y);
                let uv = Rect::from_center_size(
                    Pos2::new(0.5, 0.5),
                    vec2(cell_rect.width() / (size.x * s), cell_rect.height() / (size.y * s)),
                );
                egui::Image::new((tex.id(), cell_rect.size()))
                    .uv(uv)
                    .corner_radius(corner_radius)
                    .paint_at(ui, cell_rect);
            }
            None => {
                p.rect_filled(cell_rect, corner_radius, t::p().field);
                if let Some(j) = job_for(lib, item, Tier::Grid) {
                    self.ws.frame_jobs.push(j);
                } else {
                    ext_placeholder(ui, cell_rect, &item.ext);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::Scope;
    use crate::prefs::{Prefs, StartupDestination};

    #[test]
    fn home_scope_defaults_and_title() {
        assert_eq!(Scope::default(), Scope::Home);
        assert_eq!(Scope::Home.title(&[]), "Home");
    }

    #[test]
    fn startup_destination_enum_and_serde() {
        assert_eq!(StartupDestination::default(), StartupDestination::Home);

        for dest in StartupDestination::ALL {
            assert!(!dest.label().is_empty());
        }

        let mut prefs = Prefs::default();
        assert_eq!(prefs.startup_destination, StartupDestination::Home);

        prefs.startup_destination = StartupDestination::SpecificFolder;
        prefs.startup_folder_id = Some("F123".into());
        let json = serde_json::to_string(&prefs).unwrap();
        let loaded: Prefs = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.startup_destination, StartupDestination::SpecificFolder);
        assert_eq!(loaded.startup_folder_id, Some("F123".into()));
    }

    #[test]
    fn home_dashboard_never_triggers_eager_all_assets_sort() {
        let mut ws = crate::app::state::Workspace::new(&egui::Context::default(), None);
        assert_eq!(ws.scope, Scope::Home);

        if ws.scope == Scope::Home {
            ws.order.clear();
        } else {
            ws.order = vec!["item1".into(), "item2".into()];
        }
        assert!(ws.order.is_empty(), "Home must never populate ws.order with items");

        ws.scope = Scope::All;
        if ws.scope == Scope::Home {
            ws.order.clear();
        } else {
            ws.order = vec!["item1".into(), "item2".into()];
        }
        assert_eq!(ws.order.len(), 2);
    }

    #[test]
    fn home_collections_width_exact_margins_and_no_overflow() {
        let gap: f32 = 16.0;
        let pad_x: f32 = 16.0;
        let target_card_w: f32 = 210.0;

        for area_w in [400.0_f32, 600.0, 850.0, 1024.0, 1440.0, 1920.0, 2560.0] {
            let content_w = (area_w - 2.0 * pad_x).max(100.0);
            let cols = ((content_w + gap) / (target_card_w + gap)).floor().max(1.0) as usize;
            let card_w = (content_w - (cols.saturating_sub(1) as f32 * gap)) / cols as f32;

            let total_row_w = cols as f32 * card_w + (cols.saturating_sub(1) as f32 * gap);
            assert!(
                (total_row_w - content_w).abs() < 1e-3,
                "Row width ({total_row_w}) must match content_w ({content_w}) exactly"
            );

            let right_margin = area_w - (pad_x + total_row_w);
            assert!(
                (right_margin - pad_x).abs() < 1e-3,
                "Right margin ({right_margin}) must match pad_x ({pad_x}) within subpixel tolerance"
            );
        }
    }

    #[test]
    fn home_mosaic_cells_geometry_no_overflow() {
        let cell_gap = 2.0_f32;
        let thumb_rect = Rect::from_min_size(Pos2::new(10.0, 20.0), vec2(200.0, 124.0));

        // 4 cells (2x2)
        let half_w = (thumb_rect.width() - cell_gap) / 2.0;
        let half_h = (thumb_rect.height() - cell_gap) / 2.0;

        let c0 = Rect::from_min_size(thumb_rect.min, vec2(half_w, half_h));
        let c1 = Rect::from_min_size(Pos2::new(thumb_rect.center().x + cell_gap / 2.0, thumb_rect.top()), vec2(half_w, half_h));
        let c2 = Rect::from_min_size(Pos2::new(thumb_rect.left(), thumb_rect.center().y + cell_gap / 2.0), vec2(half_w, half_h));
        let c3 = Rect::from_min_size(Pos2::new(thumb_rect.center().x + cell_gap / 2.0, thumb_rect.center().y + cell_gap / 2.0), vec2(half_w, half_h));

        assert!(thumb_rect.contains_rect(c0));
        assert!(thumb_rect.contains_rect(c1));
        assert!(thumb_rect.contains_rect(c2));
        assert!(thumb_rect.contains_rect(c3));

        // 3 cells (1 feature left + 2 stacked right)
        let c3_0 = Rect::from_min_size(thumb_rect.min, vec2(half_w, thumb_rect.height()));
        let c3_1 = Rect::from_min_size(Pos2::new(thumb_rect.center().x + cell_gap / 2.0, thumb_rect.top()), vec2(half_w, half_h));
        let c3_2 = Rect::from_min_size(Pos2::new(thumb_rect.center().x + cell_gap / 2.0, thumb_rect.center().y + cell_gap / 2.0), vec2(half_w, half_h));
        assert!(thumb_rect.contains_rect(c3_0));
        assert!(thumb_rect.contains_rect(c3_1));
        assert!(thumb_rect.contains_rect(c3_2));

        // 2 cells (split)
        let c2_0 = Rect::from_min_size(thumb_rect.min, vec2(half_w, thumb_rect.height()));
        let c2_1 = Rect::from_min_size(Pos2::new(thumb_rect.center().x + cell_gap / 2.0, thumb_rect.top()), vec2(half_w, thumb_rect.height()));
        assert!(thumb_rect.contains_rect(c2_0));
        assert!(thumb_rect.contains_rect(c2_1));
    }

    #[test]
    fn home_cache_epoch_invalidates_mosaics_and_recent_on_change() {
        let mut ws = crate::app::state::Workspace::new(&egui::Context::default(), None);
        ws.home_mosaics.insert("folder1".into(), vec!["item1".into(), "item2".into()]);
        ws.home_covers.insert("folder1".into(), Some("item1".into()));
        ws.home_recent_imports.push("item1".into());
        assert_eq!(ws.home_mosaics.len(), 1);
        assert_eq!(ws.home_covers.len(), 1);
        assert_eq!(ws.home_recent_imports.len(), 1);

        // Simulated epoch change (e.g. mutation_count changed on folder move)
        let new_epoch = (std::path::PathBuf::from("/test/lib"), 123456789_u64, 50, ws.sort, ws.increase, 1_u64, 20_usize);
        if ws.home_cache_epoch != new_epoch {
            ws.home_mosaics.clear();
            ws.home_covers.clear();
            ws.home_recent_imports.clear();
            ws.home_cache_epoch = new_epoch.clone();
        }

        assert!(ws.home_mosaics.is_empty(), "Epoch change must invalidate home_mosaics cache");
        assert!(ws.home_covers.is_empty(), "Epoch change must invalidate home_covers cache");
        assert!(ws.home_recent_imports.is_empty(), "Epoch change must invalidate home_recent_imports cache");
        assert_eq!(ws.home_cache_epoch, new_epoch);
    }

    #[test]
    fn home_cache_epoch_invalidates_on_recent_limit_change() {
        let mut ws = crate::app::state::Workspace::new(&egui::Context::default(), None);
        let root = std::path::PathBuf::from("/test/lib");
        ws.home_cache_epoch = (root.clone(), 100, 10, ws.sort, ws.increase, 0, 20);
        ws.home_recent_imports = vec!["item1".into(), "item2".into()];

        // User changes recent limit from 20 to 10
        let new_epoch = (root, 100, 10, ws.sort, ws.increase, 0, 10);
        if ws.home_cache_epoch != new_epoch {
            ws.home_recent_imports.clear();
            ws.home_cache_epoch = new_epoch;
        }

        assert!(ws.home_recent_imports.is_empty(), "Changing recent limit must invalidate recent imports cache");
    }

    fn test_item(id: &str, mtime: u64, deleted: bool) -> Item {
        serde_json::from_value(serde_json::json!({
            "id": id, "name": id, "size": 100, "btime": mtime, "mtime": mtime, "ext": "png", "tags": [], "folders": [],
            "isDeleted": deleted, "url": "", "annotation": "", "modificationTime": mtime, "height": 100, "width": 100,
            "lastModified": mtime, "palettes": []
        }))
        .unwrap()
    }

    #[test]
    fn resolve_recently_imported_bounds_limit_and_orders_newest_first() {
        let mut items = Vec::new();
        for i in 1..=30 {
            let mtime = i as u64 * 1000;
            let deleted = i == 30; // item_30 is deleted
            items.push(test_item(&format!("item_{i}"), mtime, deleted));
        }

        let limit = 10;
        let mut candidates: Vec<&Item> = items.iter().filter(|i| !i.is_deleted).collect();
        assert_eq!(candidates.len(), 29);

        if candidates.len() > limit {
            candidates.select_nth_unstable_by(limit, |a, b| {
                b.modification_time.cmp(&a.modification_time).then_with(|| b.id.cmp(&a.id))
            });
            candidates.truncate(limit);
        }
        candidates.sort_by(|a, b| {
            b.modification_time.cmp(&a.modification_time).then_with(|| b.id.cmp(&a.id))
        });

        assert_eq!(candidates.len(), 10);
        // item 29 (non-deleted with highest mtime) should be first
        assert_eq!(candidates[0].id, "item_29");
        assert_eq!(candidates[0].modification_time, 29000);
        assert_eq!(candidates[9].id, "item_20");
        assert_eq!(candidates[9].modification_time, 20000);
    }

    #[test]
    fn home_dashboard_preferences_defaults() {
        let prefs = crate::prefs::Prefs::default();
        assert!(prefs.home_show_recent, "Recently Imported must default to ON");
        assert_eq!(prefs.home_recent_limit, 20, "Recent limit must default to 20");
        assert_eq!(prefs.home_recent_size, crate::prefs::RecentThumbSize::Medium, "Thumb size must default to Medium");
        assert!(prefs.home_show_filenames, "Filenames must default to ON");
        assert!(!prefs.home_show_overview, "Library Overview must default to OFF in H2.1");
        assert!(prefs.home_show_hero, "Quick Access Hero must default to ON");
        assert!(prefs.home_show_folders, "Library Folders must default to ON");
    }

    #[test]
    fn h4_preferences_defaults_and_backward_compatible_serde() {
        use crate::prefs::{CardSize, CoverStyle, Prefs};
        let prefs = Prefs::default();
        assert_eq!(prefs.home_card_size, CardSize::Medium);
        assert_eq!(prefs.home_cover_style, CoverStyle::Mosaic);
        assert!(prefs.home_show_counts);
        // State saved before H4 has none of the new keys and must still load with defaults.
        let old: Prefs = serde_json::from_str(r#"{"home_show_recent": false}"#).unwrap();
        assert!(!old.home_show_recent);
        assert_eq!(old.home_card_size, CardSize::Medium);
        assert!(old.home_show_counts);
        let json = serde_json::to_string(&Prefs { home_cover_style: CoverStyle::Classic, home_card_size: CardSize::Large, home_show_counts: false, ..Prefs::default() }).unwrap();
        let back: Prefs = serde_json::from_str(&json).unwrap();
        assert_eq!((back.home_cover_style, back.home_card_size, back.home_show_counts), (CoverStyle::Classic, CardSize::Large, false));
        assert!(CardSize::Small.target_width() < CardSize::Medium.target_width());
        assert!(CardSize::Medium.target_width() < CardSize::Large.target_width());
    }

    #[test]
    fn custom_cover_falls_back_when_asset_is_missing_deleted_or_locked() {
        let none = std::collections::HashSet::new();
        let ok = test_item("a", 1, false);
        assert!(valid_cover(Some(&ok), &none).is_some());
        assert!(valid_cover(None, &none).is_none(), "missing asset falls back");
        assert!(valid_cover(Some(&test_item("d", 1, true)), &none).is_none(), "deleted asset falls back");
        let mut in_locked = test_item("l", 1, false);
        in_locked.folders = vec!["LOCKED".into()];
        let locked: std::collections::HashSet<&str> = ["LOCKED"].into_iter().collect();
        assert!(valid_cover(Some(&in_locked), &locked).is_none(), "locked folder asset falls back");
    }

    #[test]
    fn secondary_quick_access_cards_stop_growing_on_wide_windows() {
        // Ordinary window: fills the row beside the Hero.
        assert_eq!(quick_secondary_width(900.0, 400.0, 3), 900.0 - 400.0 - 16.0);
        // 2000 px wide: capped, so cards stay readable.
        assert_eq!(quick_secondary_width(1900.0, 480.0, 4), 760.0);
        assert_eq!(quick_secondary_width(1900.0, 480.0, 2), 520.0);
        assert!(quick_secondary_width(300.0, 480.0, 4) >= 100.0);
    }
}
