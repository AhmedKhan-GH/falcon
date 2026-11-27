//! Optional Finder-style clickable breadcrumb path bar at the bottom of the
//! central content area (Settings ▸ Appearance ▸ Show Path Bar).

use eframe::egui::{self, Pos2, Rect, RichText, Sense, Stroke, Ui, vec2};
use falcon_library::Library;

use crate::app::App;
use crate::icons as ic;
use crate::locale::tr;
use crate::logic::{self, Scope};
use crate::theme::{self as t, font};
use crate::ui::widgets::galley;

/// Height in logical points reserved for the path bar.
pub const PATH_BAR_HEIGHT: f32 = 24.0;

struct Segment {
    scope: Option<Scope>,
    name: String,
    icon: &'static str,
}

impl App {
    /// Renders the clickable breadcrumb path bar into `rect`.
    pub(crate) fn path_bar(&mut self, ui: &mut Ui, lib: &Library, rect: Rect) {
        let p = ui.painter();
        // Top divider stroke and background
        p.hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0, t::p().divider));
        p.rect_filled(rect, 0.0, t::p().panel);

        let mut segments: Vec<Segment> = Vec::new();

        match &self.ws.scope {
            Scope::Folder(id) => {
                let chain = logic::ancestor_chain(&lib.metadata.folders, id);
                if chain.is_empty() {
                    let name = logic::find_folder(&lib.metadata.folders, id)
                        .map_or_else(|| tr("Folder").to_string(), |f| f.name.clone());
                    segments.push(Segment {
                        scope: Some(Scope::Folder(id.clone())),
                        name,
                        icon: ic::FOLDER,
                    });
                } else {
                    for (fid, name) in chain.into_iter().rev() {
                        segments.push(Segment {
                            scope: Some(Scope::Folder(fid)),
                            name,
                            icon: ic::FOLDER,
                        });
                    }
                }
            }
            Scope::Smart(id) => {
                let name = lib.smart_folders().iter().find(|s| &s.id == id).map_or_else(
                    || tr("Smart Folder").to_string(),
                    |s| s.name.clone(),
                );
                segments.push(Segment {
                    scope: Some(Scope::Smart(id.clone())),
                    name,
                    icon: ic::FOLDER_STAR,
                });
            }
            Scope::Tag(tag) => {
                segments.push(Segment {
                    scope: Some(Scope::Tag(tag.clone())),
                    name: tag.clone(),
                    icon: ic::BOOKMARK_SIMPLE,
                });
            }
            Scope::Home => {
                segments.push(Segment {
                    scope: Some(Scope::Home),
                    name: tr("Home").to_string(),
                    icon: ic::HOUSE,
                });
            }
            Scope::All => {
                segments.push(Segment {
                    scope: Some(Scope::All),
                    name: tr("All").to_string(),
                    icon: ic::IMAGES,
                });
            }
            Scope::Uncategorized => {
                segments.push(Segment {
                    scope: Some(Scope::Uncategorized),
                    name: tr("Uncategorized").to_string(),
                    icon: ic::FOLDER_DASHED,
                });
            }
            Scope::Untagged => {
                segments.push(Segment {
                    scope: Some(Scope::Untagged),
                    name: tr("Untagged").to_string(),
                    icon: ic::BOOKMARK_SIMPLE,
                });
            }
            Scope::Recent => {
                segments.push(Segment {
                    scope: Some(Scope::Recent),
                    name: tr("Recently Used").to_string(),
                    icon: ic::CLOCK,
                });
            }
            Scope::Random(_) => {
                segments.push(Segment {
                    scope: Some(Scope::Random(0)),
                    name: tr("Random").to_string(),
                    icon: ic::ARROWS_LEFT_RIGHT,
                });
            }
            Scope::AllTags => {
                segments.push(Segment {
                    scope: Some(Scope::AllTags),
                    name: tr("All Tags").to_string(),
                    icon: ic::BOOKMARK_SIMPLE,
                });
            }
            Scope::Duplicates => {
                segments.push(Segment {
                    scope: Some(Scope::Duplicates),
                    name: tr("Duplicates").to_string(),
                    icon: ic::COPY,
                });
            }
            Scope::Trash => {
                segments.push(Segment {
                    scope: Some(Scope::Trash),
                    name: tr("Trash").to_string(),
                    icon: ic::TRASH,
                });
            }
        }

        // When in Preview / Viewer mode, append the item being viewed
        if let Some(viewed_id) = &self.ws.viewer
            && let Some(item) = lib.item(viewed_id)
        {
            segments.push(Segment {
                scope: None,
                name: item.name.clone(),
                icon: ic::FILE,
            });
        }

        let full_path = segments.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(" > ");
        let count = segments.len();
        let mut clicked_scope = None;

        let content_rect = rect.shrink2(vec2(8.0, 1.0));
        let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(content_rect));

        egui::ScrollArea::horizontal()
            .auto_shrink([false, false])
            .show(&mut child_ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = vec2(4.0, 0.0);

                    for (idx, seg) in segments.into_iter().enumerate() {
                        let is_last = idx + 1 == count;
                        let text_color = if is_last { t::p().text } else { t::p().text_muted };

                        let icon_font = font(12.0);
                        let text_font = font(11.5);
                        let name_g = galley(ui, &seg.name, text_font, text_color, 400.0);
                        let seg_width = 16.0 + name_g.size().x + 8.0;

                        let (seg_rect, resp) = ui.allocate_exact_size(vec2(seg_width, 20.0), Sense::click());
                        let p = ui.painter();

                        if resp.hovered() && seg.scope.is_some() {
                            p.rect_filled(seg_rect, 4.0, t::p().row_hover);
                        }

                        // Icon
                        p.text(
                            Pos2::new(seg_rect.left() + 4.0, seg_rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            seg.icon,
                            icon_font,
                            if is_last { t::p().primary } else { t::p().text_muted },
                        );

                        // Label
                        p.galley(
                            Pos2::new(seg_rect.left() + 20.0, seg_rect.center().y - name_g.size().y / 2.0),
                            name_g,
                            text_color,
                        );

                        let resp = resp.on_hover_text(&full_path);

                        if resp.clicked()
                            && let Some(target) = seg.scope
                        {
                            clicked_scope = Some(target);
                        }

                        // Chevron between segments
                        if !is_last {
                            ui.label(
                                RichText::new(ic::CARET_RIGHT)
                                    .font(font(10.0))
                                    .color(t::p().text_muted),
                            );
                        }
                    }
                });
            });

        if let Some(target) = clicked_scope {
            self.go(target);
        }
    }
}
