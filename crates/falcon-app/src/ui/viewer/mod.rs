//! The detail viewer: the shell that picks a viewer for the open item, and one module per kind of file.

pub(crate) mod audio;
pub(crate) mod comments;
pub(crate) mod crop;
pub(crate) mod exposure;
pub(crate) mod font;
pub(crate) mod gif;
pub(crate) mod image;
pub(crate) mod model;
pub(crate) mod model_tools;
pub(crate) mod modes;
pub(crate) mod open;
pub(crate) mod pdf;
pub(crate) mod preview_window;
pub(crate) mod text;
pub(crate) mod tiff;
pub(crate) mod video;
pub(crate) mod web;

use eframe::egui::{self, Align2, Pos2, Rect, Ui, Vec2, vec2};
use falcon_library::Library;
use falcon_library::model::Item;

use crate::app::{App, ViewState};
use crate::loader::Tier;
use crate::theme::{self as t, font};
use crate::ui::widgets::{ext_placeholder, galley, viewer_ui};

impl App {
    /// The open item's viewer, drawn by `parent` (the central panel) over
    /// `screen`: as in Eagle's detail mode it takes the place of the grid,
    /// between the sidebar and the inspector.
    pub(crate) fn viewer(&mut self, parent: &mut Ui, lib: &Library, screen: Rect) {
        let ctx = &parent.ctx().clone();
        let Some(id) = self.ws.viewer.clone() else { return };
        let Some(item) = lib.item(&id) else {
            self.ws.viewer = None;
            return;
        };
        if self.ws.view.id != id {
            self.ws.view = ViewState { id: id.clone(), file: item.original_path(lib.root()), ..Default::default() };
        }
        // A middle click anywhere in the viewer enters or leaves
        // presentation mode, as Eagle's `onDetailClick`, in every viewer.
        if middle_clicked_in(ctx, screen) {
            self.toggle_presentation(ctx);
        }
        // An original still in iCloud downloads in the background first.
        if self.icloud_viewer(parent, lib, item, screen) {
            return;
        }
        // An animation playing in colour starts again in grayscale, and back.
        if self.ws.view.anim.as_ref().is_some_and(|a| a.gray != self.modes.grayscale) {
            self.ws.view.anim = None;
        }
        if self.ws.view.anim.is_none()
            && let Some(kind) = crate::anim::kind(&item.ext)
        {
            let mut player = crate::anim::Player::open(ctx, item.original_path(lib.root()), kind, crate::anim::Opts::viewer());
            player.gray = self.modes.grayscale;
            self.ws.view.anim = Some(player);
        }
        if self.text_viewer(parent, lib, item, screen)
            || self.video_viewer(parent, lib, item, screen)
            || self.pdf_viewer(parent, lib, item, screen)
            || self.web_viewer(parent, lib, item, screen)
            || self.model_viewer(parent, lib, item, screen)
            || self.media_viewer(parent, lib, item, screen)
        {
            return;
        }
        let ui = &mut viewer_ui(parent, screen, "viewer");
        let p = ui.painter().clone();
        p.rect_filled(screen, 0.0, t::p().main);
        let bar = Rect::from_min_size(screen.min, vec2(screen.width(), t::BAR_HEIGHT));
        let area = Rect::from_min_max(Pos2::new(screen.left(), bar.bottom()), screen.max);
        let (area, gif_bar) = self.gif_split(item, area);

        // The image: a frame of an animation, the full render, or the preview.
        let now = ctx.input(|i| i.time);
        let gif = self.ws.view.anim.as_mut().and_then(|a| {
            a.update(now);
            a.texture().cloned()
        });
        let page = self.tiff_view(ctx, lib, item);
        let tex = gif
            .or(page)
            .or_else(|| self.ws.loader.get(&id, Tier::Full).cloned())
            .or_else(|| self.ws.loader.get(&id, Tier::Preview).cloned());
        self.image_canvas(ui, lib, item, tex, bar, area, gif_bar);
    }

    /// Eagle's "cannot preview" page: the file's type, a line saying so
    /// and a button listing the apps that can open it.
    pub(crate) fn cannot_preview(&mut self, ui: &mut Ui, item: &Item, root: &std::path::Path, area: Rect) {
        let c = area.center();
        ext_placeholder(ui, Rect::from_center_size(c - vec2(0.0, 70.0), Vec2::splat(150.0)), &item.ext);
        let name = format!("{}.{}", item.name, item.ext);
        let line = if crate::icloud::is_missing(&item.original_path(root)) { crate::icloud::MISSING_HINT } else { "Falcon can't show a preview of this file. Try opening it in another app." };
        self.notice_lines(ui, area, &name, line);
    }

    /// The title, line and Open With button under the picture of a page
    /// shown instead of a preview.
    pub(crate) fn notice_lines(&mut self, ui: &mut Ui, area: Rect, title: &str, line: &str) {
        let b = notice_text(ui, area, title, line);
        let file = self.ws.view.file.clone();
        egui::Popup::menu(&b).show(|ui| {
            ui.set_min_width(200.0);
            self.open_menu(ui, std::slice::from_ref(&file));
        });
    }
}

/// The title and line of a notice page, centred under its picture, and
/// the Open With Another App button under them.
pub(crate) fn notice_text(ui: &mut Ui, area: Rect, title: &str, line: &str) -> egui::Response {
    let c = area.center();
    let g = galley(ui, title, font(15.0), t::p().text, (area.width() - 40.0).max(1.0));
    ui.painter().galley(Pos2::new(c.x - g.size().x / 2.0, c.y + 14.0), g, t::p().text);
    ui.painter().text(c + vec2(0.0, 48.0), Align2::CENTER_CENTER, line, font(12.5), t::p().text_muted);
    ui.put(Rect::from_center_size(c + vec2(0.0, 84.0), vec2(210.0, 28.0)), egui::Button::new("Open With Another App…"))
}

/// Whether the middle button was clicked over `screen` this frame (not
/// over a menu or popup above it).
fn middle_clicked_in(ctx: &egui::Context, screen: Rect) -> bool {
    let over_popup = ctx.pointer_interact_pos().and_then(|p| ctx.layer_id_at(p)).is_some_and(|l| l.order != egui::Order::Background && l.order != egui::Order::Middle);
    !over_popup && ctx.input(|i| i.pointer.button_clicked(egui::PointerButton::Middle) && i.pointer.interact_pos().is_some_and(|p| screen.contains(p)))
}
