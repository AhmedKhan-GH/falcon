//! The viewer for web pages (`.url` bookmarks, `.html`,
//! `.mhtml`), after Eagle's web view: on macOS the page itself, live in a
//! `WKWebView` (see `crate::webview`), with back, forward, reload, Copy URL
//! and Open in Browser in the bar. MHTML archives, bookmarks that don't
//! point at a web address, and systems without WebKit get a still: the
//! QuickLook render of the file with the same Copy URL and open buttons.

use eframe::egui::{self, Align2, Pos2, Rect, Ui, Vec2, vec2};
use falcon_library::Library;
use falcon_library::model::Item;

use crate::app::App;
use crate::icons as ic;
use crate::loader::{Tier, job_for};
use crate::locale::tr;
use crate::theme::{self as t, font, mono};
use crate::ui::viewer::audio::bar_shell;
use crate::ui::widgets::{ext_placeholder, galley, icon_button, viewer_ui};
use crate::webview::{Nav, Page, Want, WebView, is_web, is_web_address, page_for};

impl App {
    /// Draws the web viewer for a web item and returns true; false for
    /// anything else.
    pub(crate) fn web_viewer(&mut self, parent: &mut Ui, lib: &Library, item: &Item, screen: Rect) -> bool {
        if !is_web(&item.ext) {
            return false;
        }
        let file = item.original_path(lib.root());
        let Some(page) = page_for(&item.ext, &item.url, &file) else { return false };
        let live = !matches!(page, Page::Still { .. }) && WebView::available();
        let ui = &mut viewer_ui(parent, screen, "web-viewer");
        ui.painter().rect_filled(screen, 0.0, t::p().main);
        let bar = Rect::from_min_size(screen.min, vec2(screen.width(), t::BAR_HEIGHT));
        let area = Rect::from_min_max(Pos2::new(screen.left(), bar.bottom()), screen.max);

        let nav = if live { self.web.nav() } else { Nav::default() };
        // The address shown: where the page is now, else where it started.
        let url = nav.url.clone().or_else(|| match &page {
            Page::Remote(u) => Some(u.clone()),
            Page::Local(_) => None,
            Page::Still { url } => url.clone(),
        });
        let (left, mut right) = bar_shell(self, ui, item, bar);
        let cy = bar.center().y;
        // Open in Browser: the address, or the HTML file in its default
        // app (a browser); an MHTML archive opens in its own app.
        let (open_tip, can_open) = match &page {
            Page::Local(_) => (tr("Open in Browser"), true),
            Page::Still { .. } if !item.ext.eq_ignore_ascii_case("url") => (tr("Open in Default App"), true),
            _ => (tr("Open in Browser"), url.as_deref().is_some_and(is_web_address)),
        };
        if icon_button(ui, Pos2::new(right - 13.0, cy), ic::GLOBE, 16.0, can_open, open_tip).clicked() {
            match (&page, &url) {
                (Page::Remote(_), Some(u)) | (Page::Still { .. }, Some(u)) if is_web_address(u) => crate::open_with::open_url(u),
                _ => crate::open_with::open_default(std::slice::from_ref(&file)),
            }
        }
        right -= 26.0;
        if icon_button(ui, Pos2::new(right - 13.0, cy), ic::COPY, 15.0, url.is_some(), tr("Copy URL")).clicked()
            && let Some(u) = &url
        {
            ui.ctx().copy_text(u.clone());
            self.flash_brief(tr("URL copied"));
        }
        right -= 26.0;
        if live {
            let (reload, tip) = if nav.loading { (ic::X, tr("Stop")) } else { (ic::ARROW_CLOCKWISE, tr("Reload")) };
            if icon_button(ui, Pos2::new(right - 13.0, cy), reload, 15.0, true, tip).clicked() {
                if nav.loading {
                    self.web.stop();
                } else {
                    self.web.reload();
                }
            }
            right -= 26.0;
            if icon_button(ui, Pos2::new(right - 13.0, cy), ic::ARROW_RIGHT, 15.0, nav.can_forward, tr("Forward")).clicked() {
                self.web.forward();
            }
            right -= 26.0;
            if icon_button(ui, Pos2::new(right - 13.0, cy), ic::ARROW_LEFT, 15.0, nav.can_back, tr("Back")).clicked() {
                self.web.back();
            }
            right -= 26.0;
        }
        title_and_url(ui, item, nav.title.as_deref(), url.as_deref(), bar, left, right - 8.0);

        if live {
            ui.painter().text(area.center(), Align2::CENTER_CENTER, tr("Loading…"), font(13.0), t::p().text_muted);
            self.web.want(Want { key: item.id.clone(), page, rect: area });
        } else {
            self.web_still(ui, lib, item, &page, area);
        }
        true
    }

    /// The still page: the QuickLook render of the file, fitted, with a
    /// line saying why the page isn't live.
    fn web_still(&mut self, ui: &mut Ui, lib: &Library, item: &Item, page: &Page, area: Rect) {
        if let Some(j) = job_for(item, lib.root(), Tier::Full) {
            self.ws.frame_jobs.insert(0, j);
        }
        let note = match page {
            Page::Still { url: Some(u) } if item.ext.eq_ignore_ascii_case("url") && !is_web_address(u) => {
                tr("This link isn't a web address, so it isn't opened here.")
            }
            Page::Still { url: None } if item.ext.eq_ignore_ascii_case("url") => tr("This bookmark has no address."),
            Page::Still { .. } => tr("Web archives are shown as a picture. Open it in its own app to browse it."),
            _ => tr("Pages are shown as a picture on this system. Open it in the browser to browse it."),
        };
        let pic = Rect::from_min_max(area.min + vec2(24.0, 20.0), area.max - vec2(24.0, 52.0));
        let tex = self.ws.loader.get(&item.id, Tier::Full).cloned().or_else(|| self.ws.loader.get(&item.id, Tier::Preview).cloned());
        match tex {
            Some(tex) if pic.width() > 1.0 && pic.height() > 1.0 => {
                let size = tex.size_vec2();
                let scale = (pic.width() / size.x).min(pic.height() / size.y).min(1.0);
                let r = Rect::from_center_size(pic.center(), size * scale);
                ui.painter().image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), egui::Color32::WHITE);
            }
            _ => ext_placeholder(ui, Rect::from_center_size(pic.center(), Vec2::splat(150.0)), &item.ext),
        }
        ui.painter().text(Pos2::new(area.center().x, area.bottom() - 26.0), Align2::CENTER_CENTER, note, font(12.5), t::p().text_muted);
    }
}

/// The page's title (or the item's name) and its address, muted, in the
/// bar between `left` and `right`.
fn title_and_url(ui: &Ui, item: &Item, title: Option<&str>, url: Option<&str>, bar: Rect, left: f32, right: f32) {
    let width = (right - left).max(0.0);
    let name = title.map_or_else(|| format!("{}.{}", item.name, item.ext), str::to_string);
    let g = galley(ui, &name, font(14.0), t::p().text, width.min(360.0));
    let used = g.size().x;
    let cy = bar.center().y;
    ui.painter().galley(Pos2::new(left, cy - g.size().y / 2.0), g, t::p().text);
    if let Some(u) = url {
        let room = width - used - 12.0;
        if room > 40.0 {
            let g = galley(ui, u, mono(11.5), t::p().text_muted, room);
            ui.painter().galley(Pos2::new(left + used + 12.0, cy - g.size().y / 2.0), g, t::p().text_muted);
        }
    }
}
