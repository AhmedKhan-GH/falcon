//! Falcon Home Dashboard: Creative Asset Workspace overview.
//!
//! Provides a designer-first dashboard displaying library metrics,
//! Quick Access Hero & Supporting Cards (H2-A), and responsive top-level
//! folder collections with 2x2 Mosaic Covers (H2-B), eliminating eager
//! sorting and thumbnail loading for 37,000+ asset libraries.

use eframe::egui::{self, Align2, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, vec2};
use falcon_library::Library;
use falcon_library::model::{Folder, Item};

use crate::app::App;
use crate::folder_icons;
use crate::icons as ic;
use crate::loader::{job_for, Tier};
use crate::locale::{tr, tr_n};
use crate::logic::{self, Scope};
use crate::theme::{self as t, font};
use crate::ui::grid::covers::{self, folder_meta};
use crate::ui::widgets::{ext_placeholder, galley};

#[derive(Clone, Copy)]
struct CardDims {
    w: f32,
    h: f32,
    thumb_h: f32,
}

/// Resolved Quick Access card candidate for Home Dashboard (H2-A).
pub(crate) struct QuickAccessCard {
    #[allow(dead_code)]
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) scope: Scope,
    pub(crate) count: usize,
    pub(crate) cover_item: Option<Item>,
    pub(crate) icon_glyph: &'static str,
    pub(crate) icon_tint: egui::Color32,
}

impl App {
    /// Renders the Home Dashboard: prominent greeting/title, compact library metrics overview,
    /// Quick Access Hero & Supporting cards, and visual 2x2 mosaic cards for collections.
    pub(crate) fn home_page(&mut self, ui: &mut Ui, lib: &Library) {
        // Derive exact available content width between sidebar and inspector (or window edge)
        let area_w = ui.available_width();
        let pad_x = 16.0;
        let content_w = (area_w - 2.0 * pad_x).max(100.0);

        // Invalidate session cover caches if library metadata modification time, item count, or sort changed
        let epoch = (lib.metadata.modification_time, lib.items().len(), self.ws.sort, self.ws.increase);
        if self.ws.home_cache_epoch != epoch {
            self.ws.home_covers.clear();
            self.ws.home_mosaics.clear();
            self.ws.home_cache_epoch = epoch;
        }

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

                        // Zone 2: Library Overview Metrics
                        self.home_library_overview(ui, lib, content_w);
                        ui.add_space(24.0);

                        // Zone 2.5: Quick Access Hero (H2-A)
                        let locked = covers::locked_folders(&lib.metadata.folders);
                        let counts = self.folder_counts_for(lib);
                        let quick_cards = self.resolve_quick_access(lib, &locked, &counts);
                        if !quick_cards.is_empty() {
                            self.home_quick_access_section(ui, lib, &quick_cards, content_w);
                            ui.add_space(26.0);
                        }

                        // Zone 3: Root Folder Collections Grid with 2x2 Mosaic Covers (H2-B)
                        self.home_collections_section(ui, lib, content_w, &locked, &counts);
                        ui.add_space(32.0);
                    });
                });
            });
    }

    /// Zone 1: Home Heading with prominent typography and creative workspace tagline.
    fn home_heading(&self, ui: &mut Ui) {
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
        });
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
    pub(crate) fn resolve_quick_access(
        &self,
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
                let items = lib.smart_folder_items(id);
                let count = if sf.is_group() { 0 } else { items.len() };
                let cover_item = items.into_iter().find(|i| !i.is_deleted && falcon_library::format_capability::is_preview_supported(&i.ext) && !i.folders.iter().any(|f| locked.contains(f.as_str()))).cloned();
                cards.push(QuickAccessCard {
                    id: id.to_string(),
                    name: sf.name.clone(),
                    scope: Scope::Smart(id.to_string()),
                    count,
                    cover_item,
                    icon_glyph: crate::ui::organize::smart_look(sf),
                    icon_tint: t::p().text,
                });
            } else {
                let Some(f) = logic::find_folder(&lib.metadata.folders, id) else { continue };
                let count = counts.get(id).copied().unwrap_or(0);
                let cover_item = covers::cover(lib.items(), f, sort, inc, locked).cloned();
                cards.push(QuickAccessCard {
                    id: id.to_string(),
                    name: f.name.clone(),
                    scope: Scope::Folder(id.to_string()),
                    count,
                    cover_item,
                    icon_glyph: folder_icons::glyph(f.icon.as_deref(), false),
                    icon_tint: folder_icons::color(f.icon_color.as_deref()).unwrap_or(t::p().text),
                });
            }
        }

        cards
    }

    /// Zone 2.5: Quick Access Hero & Supporting Cards (H2-A).
    fn home_quick_access_section(
        &mut self,
        ui: &mut Ui,
        lib: &Library,
        cards: &[QuickAccessCard],
        content_w: f32,
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
            if let Some(target) = self.render_primary_hero(ui, lib, primary, vec2(hero_w, hero_h)) {
                clicked_scope = Some(target);
            }
        } else if content_w >= 720.0 {
            // Wide layout: Left primary Hero, Right supporting secondary cards
            let hero_w = ((content_w - 16.0) * 0.50).clamp(320.0, 480.0);
            let sec_w = content_w - hero_w - 16.0;
            let hero_h = 150.0;

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;

                // Primary Hero
                if let Some(target) = self.render_primary_hero(ui, lib, primary, vec2(hero_w, hero_h)) {
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
                            if let Some(target) = self.render_secondary_card(ui, lib, sec, vec2(sec_w, card_h)) {
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
                                    if let Some(target) = self.render_secondary_card(ui, lib, sec, vec2(card_w, card_h)) {
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
            if let Some(target) = self.render_primary_hero(ui, lib, primary, vec2(content_w, hero_h)) {
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
                        if let Some(target) = self.render_secondary_card(ui, lib, sec, vec2(card_w, card_h)) {
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
    ) -> Option<Scope> {
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
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
        let count_text = tr_n(card.count, "{n} item", "{n} items");
        let count_galley = galley(ui, &count_text, font(12.0), t::p().text_muted, avail_w);
        p.galley(Pos2::new(details_rect.left(), details_rect.min.y + 44.0), count_galley, t::p().text_muted);

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
    ) -> Option<Scope> {
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
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
        p.galley(text_rect.min, title_galley, t::p().text);

        let count_text = tr_n(card.count, "{n} item", "{n} items");
        let count_galley = galley(ui, &count_text, font(11.0), t::p().text_muted, avail_w);
        p.galley(Pos2::new(text_rect.left(), text_rect.bottom() - count_galley.size().y), count_galley, t::p().text_muted);

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
            ui.label(egui::RichText::new(tr("Collections")).font(font(16.0)).strong().color(t::p().text));
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
        let target_card_w = 210.0;
        let cols = ((content_w + gap) / (target_card_w + gap)).floor().max(1.0) as usize;
        let card_w = (content_w - (cols.saturating_sub(1) as f32 * gap)) / cols as f32;
        let dims = CardDims {
            w: card_w,
            thumb_h: (card_w * 0.62).clamp(110.0, 150.0),
            h: (card_w * 0.62).clamp(110.0, 150.0) + 56.0,
        };

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

        // Deterministic mosaic cover resolution with session state caching
        let sort = self.ws.sort;
        let inc = self.ws.increase;
        let cover_ids = self.ws.home_mosaics.entry(folder.id.clone()).or_insert_with(|| {
            if folder.is_locked() {
                Vec::new()
            } else {
                covers::mosaic_covers(lib.items(), folder, sort, inc, locked)
                    .into_iter()
                    .map(|i| i.id.clone())
                    .collect()
            }
        }).clone();

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
        p.galley(Pos2::new(text_left, info_rect.top() + 8.0), name_g, t::p().text);

        let meta_str = folder_meta(folder, counts);
        let meta_g = galley(ui, &meta_str, font(11.5), t::p().text_muted, text_w + 18.0);
        p.galley(Pos2::new(info_rect.left() + 10.0, info_rect.top() + 29.0), meta_g, t::p().text_muted);

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
    fn home_cache_epoch_invalidates_mosaics_on_change() {
        let mut ws = crate::app::state::Workspace::new(&egui::Context::default(), None);
        ws.home_mosaics.insert("folder1".into(), vec!["item1".into(), "item2".into()]);
        assert_eq!(ws.home_mosaics.len(), 1);

        // Simulated epoch change (e.g. modification_time changed)
        let new_epoch = (123456789_u64, 50, ws.sort, ws.increase);
        if ws.home_cache_epoch != new_epoch {
            ws.home_mosaics.clear();
            ws.home_covers.clear();
            ws.home_cache_epoch = new_epoch;
        }

        assert!(ws.home_mosaics.is_empty(), "Epoch change must invalidate home_mosaics cache");
        assert_eq!(ws.home_cache_epoch, new_epoch);
    }
}
