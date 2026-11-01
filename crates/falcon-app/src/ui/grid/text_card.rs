//! Drawing a `.txt` item's card as its text (`crate::text_cards`).

use std::sync::Arc;

use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{Pos2, Rect, Ui};
use falcon_library::Library;
use falcon_library::model::Item;

use crate::app::App;
use crate::text_cards::{self as tc, MIN_SIDE};
use crate::theme::{self as t, font};
use crate::ui::widgets::ext_placeholder;

impl App {
    /// The item's name as a heading over its first lines, clipped to
    /// `rect`. Too small a card shows the file-type placeholder.
    pub(crate) fn text_card(&mut self, ui: &Ui, lib: &Library, item: &Item, rect: Rect) {
        if rect.width().min(rect.height()) < MIN_SIDE {
            ext_placeholder(ui, rect, &item.ext);
            return;
        }
        let read: Option<Arc<str>> = match tc::record_text(item) {
            Some(_) => None,
            None => Some(self.shell.text_cards.read(item, lib.root())),
        };
        let text = tc::record_text(item).or(read.as_deref()).unwrap_or_default();
        let look = tc::look(rect.width());
        let width = (rect.width() - 2.0 * look.side).max(1.0);
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        // Eagle's heading sets lines 1.2 apart, the paragraphs 1.5.
        let wrapped = |s: &str, size: f32, spacing: f32, color| {
            let mut job = LayoutJob::default();
            job.wrap.max_width = width;
            job.append(s, 0.0, TextFormat { font_id: font(size), color, line_height: Some(size * spacing), ..Default::default() });
            painter.layout_job(job)
        };
        let x = rect.left() + look.side;
        let mut y = rect.top() + look.top;
        let heading = wrapped(item.name.trim(), look.heading, 1.2, t::p().text);
        let h = heading.size().y;
        painter.galley(Pos2::new(x, y), heading, t::p().text);
        y += h + look.heading * 0.75;
        let body = t::p().text.gamma_multiply(0.9);
        for line in tc::lines(text) {
            if y > rect.bottom() {
                break;
            }
            let g = wrapped(line, look.body, 1.5, body);
            let h = if line.is_empty() { look.body * 1.5 } else { g.size().y };
            painter.galley(Pos2::new(x, y), g, body);
            y += h + look.body * 0.5;
        }
    }
}
