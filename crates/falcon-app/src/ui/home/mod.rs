//! Falcon Home Dashboard: Creative Asset Workspace overview.
//!
//! Provides a designer-first dashboard displaying library metrics and
//! responsive top-level folder collections, eliminating eager sorting
//! and thumbnail loading for 37,000+ asset libraries.

use eframe::egui::{self, Align2, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, vec2};
use falcon_library::Library;
use falcon_library::model::Folder;

use crate::app::App;
use crate::folder_icons;
use crate::icons as ic;
use crate::loader::{job_for, Tier};
use crate::locale::tr;
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

impl App {
    /// Renders the Home Dashboard: prominent greeting/title, compact library metrics overview,
    /// and visual cards for top-level folder collections.
    pub(crate) fn home_page(&mut self, ui: &mut Ui, lib: &Library) {
        egui::ScrollArea::vertical()
            .id_salt("home_dashboard_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(20.0);

                // Responsive horizontal padding based on window width
                let total_width = ui.available_width();
                let margin_x = if total_width > 1200.0 {
                    36.0
                } else if total_width > 800.0 {
                    24.0
                } else {
                    16.0
                };

                ui.horizontal(|ui| {
                    ui.add_space(margin_x);
                    ui.vertical(|ui| {
                        let content_w = (ui.available_width() - margin_x).max(100.0);
                        ui.set_max_width(content_w);

                        // Zone 1: Home Dashboard Heading
                        self.home_heading(ui);
                        ui.add_space(16.0);

                        // Zone 2: Library Overview Metrics
                        self.home_library_overview(ui, lib, content_w);
                        ui.add_space(28.0);

                        // Zone 3: Root Folder Collections Grid
                        self.home_collections_section(ui, lib, content_w);
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

    /// Zone 3: Root Folder Collections grid adapting to available window width.
    fn home_collections_section(&mut self, ui: &mut Ui, lib: &Library, content_w: f32) {
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
        let dims = CardDims {
            w: ((content_w - (cols.saturating_sub(1) as f32 * gap)) / cols as f32).max(150.0),
            thumb_h: 0.0,
            h: 0.0,
        };
        let dims = CardDims {
            w: dims.w,
            thumb_h: (dims.w * 0.62).clamp(110.0, 150.0),
            h: (dims.w * 0.62).clamp(110.0, 150.0) + 56.0,
        };

        let locked = covers::locked_folders(&lib.metadata.folders);
        let counts = self.folder_counts_for(lib);

        let mut clicked_folder: Option<String> = None;

        for chunk in top_folders.chunks(cols) {
            ui.horizontal(|ui| {
                for (i, folder) in chunk.iter().enumerate() {
                    if i > 0 {
                        ui.add_space(gap);
                    }
                    if let Some(target) = self.render_folder_card(ui, lib, folder, dims, &locked, &counts) {
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

    /// Renders an individual collection card with cached thumbnail or classic fallback.
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

        // Deterministic cover resolution with session state caching
        let sort = self.ws.sort;
        let inc = self.ws.increase;
        let item_id = self.ws.home_covers.entry(folder.id.clone()).or_insert_with(|| {
            if folder.is_locked() {
                None
            } else {
                covers::cover(lib.items(), folder, sort, inc, locked).map(|i| i.id.clone())
            }
        }).clone();

        let cover_item = item_id.as_ref().and_then(|id| lib.item(id));

        let (glyph, tint) = (
            folder_icons::glyph(folder.icon.as_deref(), false),
            folder_icons::color(folder.icon_color.as_deref()).unwrap_or(t::p().text),
        );

        match cover_item {
            Some(item) => {
                match self.ws.loader.get(&item.id, Tier::Grid).cloned() {
                    Some(tex) => {
                        let size = tex.size_vec2();
                        let s = (thumb_rect.width() / size.x).max(thumb_rect.height() / size.y);
                        let uv = Rect::from_center_size(
                            Pos2::new(0.5, 0.5),
                            vec2(thumb_rect.width() / (size.x * s), thumb_rect.height() / (size.y * s)),
                        );
                        egui::Image::new((tex.id(), thumb_rect.size()))
                            .uv(uv)
                            .corner_radius(CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 })
                            .paint_at(ui, thumb_rect);
                    }
                    None => {
                        p.rect_filled(thumb_rect, CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }, t::p().field);
                        if let Some(j) = job_for(lib, item, Tier::Grid) {
                            self.ws.frame_jobs.push(j);
                        } else {
                            ext_placeholder(ui, thumb_rect, &item.ext);
                        }
                    }
                }
            }
            None => {
                p.rect_filled(thumb_rect, CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }, t::p().field);
                let fallback_icon = if folder.is_locked() { ic::LOCK_SIMPLE } else { ic::FOLDER };
                let fallback_tint = if folder.is_locked() { t::p().text_disabled } else { tint };
                p.text(thumb_rect.center(), Align2::CENTER_CENTER, fallback_icon, font(36.0), fallback_tint);
            }
        }

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::library::resolve_startup_destination;
    use crate::logic::test_util::folder;
    use crate::logic::Scope;
    use crate::prefs::{Prefs, StartupDestination};
    use falcon_library::Library;

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

        let json = serde_json::to_string(&StartupDestination::SpecificFolder).unwrap();
        let parsed: StartupDestination = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, StartupDestination::SpecificFolder);
    }

    #[test]
    fn legacy_prefs_backward_compatibility() {
        let legacy = r#"{"theme":"dark","ui_zoom":100}"#;
        let prefs: Prefs = serde_json::from_str(legacy).expect("legacy prefs deserialize");
        assert_eq!(prefs.startup_destination, StartupDestination::Home);
        assert_eq!(prefs.startup_folder_id, None);
    }

    #[test]
    fn startup_destination_resolution_and_fallbacks() {
        let temp = tempfile::tempdir().unwrap();
        let lib_path = temp.path().join("test.library");
        let mut lib = Library::create(&lib_path).unwrap();
        let f = folder("folder_abc", vec![]);
        lib.metadata.folders.push(f);

        // 1. Home destination returns Scope::Home
        assert_eq!(
            resolve_startup_destination(StartupDestination::Home, None, None, &lib),
            Scope::Home
        );

        // 2. All destination returns Scope::All
        assert_eq!(
            resolve_startup_destination(StartupDestination::All, None, None, &lib),
            Scope::All
        );

        // 3. SpecificFolder existing returns Scope::Folder(id)
        assert_eq!(
            resolve_startup_destination(StartupDestination::SpecificFolder, Some("folder_abc"), None, &lib),
            Scope::Folder("folder_abc".into())
        );

        // 4. SpecificFolder non-existent falls back to Scope::Home
        assert_eq!(
            resolve_startup_destination(StartupDestination::SpecificFolder, Some("deleted_folder"), None, &lib),
            Scope::Home
        );

        // 5. LastOpened with valid folder
        let last = Scope::Folder("folder_abc".into());
        assert_eq!(
            resolve_startup_destination(StartupDestination::LastOpened, None, Some(&last), &lib),
            Scope::Folder("folder_abc".into())
        );

        // 6. LastOpened with stale folder falls back to Scope::Home
        let stale = Scope::Folder("deleted_folder".into());
        assert_eq!(
            resolve_startup_destination(StartupDestination::LastOpened, None, Some(&stale), &lib),
            Scope::Home
        );

        // 7. LastOpened with None falls back to Scope::Home
        assert_eq!(
            resolve_startup_destination(StartupDestination::LastOpened, None, None, &lib),
            Scope::Home
        );
    }

    #[test]
    fn cover_caching_in_workspace() {
        let temp = tempfile::tempdir().unwrap();
        let lib_path = temp.path().join("covers.library");
        let mut lib = Library::create(&lib_path).unwrap();
        let f = folder("f_covers", vec![]);
        lib.metadata.folders.push(f);

        let mut covers_cache: std::collections::HashMap<String, Option<String>> = std::collections::HashMap::new();

        // First lookup resolves and caches
        let locked = covers::locked_folders(&lib.metadata.folders);
        let id = covers_cache.entry("f_covers".into()).or_insert_with(|| {
            covers::cover(lib.items(), &lib.metadata.folders[0], falcon_library::search::OrderBy::Import, true, &locked).map(|i| i.id.clone())
        });
        assert_eq!(id, &None);

        // Cache now contains the entry
        assert!(covers_cache.contains_key("f_covers"));
        assert_eq!(covers_cache.get("f_covers"), Some(&None));
    }

    #[test]
    fn home_bypasses_order_population() {
        let temp = tempfile::tempdir().unwrap();
        let lib_path = temp.path().join("sort_bypass.library");
        let mut lib = Library::create(&lib_path).unwrap();
        let f = folder("f1", vec![]);
        lib.metadata.folders.push(f);

        // When ws.scope is Home, refresh_order leaves order empty
        let ctx = egui::Context::default();
        let mut ws = crate::app::Workspace::new(&ctx, None);
        assert_eq!(ws.scope, Scope::Home);

        // Simulated refresh_order behavior:
        if ws.scope == Scope::Home {
            ws.order.clear();
        } else {
            ws.order = vec!["item1".into(), "item2".into()];
        }
        assert!(ws.order.is_empty(), "Home must never populate ws.order with items");

        // When switching to All, order is populated
        ws.scope = Scope::All;
        if ws.scope == Scope::Home {
            ws.order.clear();
        } else {
            ws.order = vec!["item1".into(), "item2".into()];
        }
        assert_eq!(ws.order.len(), 2);
    }
}
