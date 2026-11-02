//! Convert WebP (`falcon_library::webp_convert`): the item menu's submenu
//! (PNG or JPG), shown when the item is a WebP, which converts every WebP in
//! the selection after Eagle's warning that the files will grow; then the
//! conversion behind the progress modal. Old WebPs go to the Trash.

use eframe::egui::{self, RichText, Ui};
use falcon_library::model::Item;
use falcon_library::webp_convert::{Converted, Format};

use crate::app::{App, lock};
use crate::theme as t;
use crate::locale::{tr, tr_nf, trf};
use crate::ui::dialogs::progress::Outcome;

/// The confirmation waiting for an answer: the WebP items and the format.
#[derive(Default)]
pub(crate) struct State {
    pub(super) confirm: Option<(Vec<String>, Format)>,
}

/// The WebP items among `targets`, in order.
pub(crate) fn webps<'a>(targets: &[String], item: impl Fn(&str) -> Option<&'a Item>) -> Vec<String> {
    targets.iter().filter(|id| item(id).is_some_and(|i| i.ext.eq_ignore_ascii_case("webp"))).cloned().collect()
}

/// The toast for a finished conversion.
pub(crate) fn summary(done: usize, skipped: usize, format: Format, stopped: bool) -> String {
    let fmt = format.ext().to_uppercase();
    let mut msg = if stopped {
        tr_nf(done, "Stopped after converting {n} item to {format}", "Stopped after converting {n} items to {format}", &[("format", &fmt)])
    } else {
        tr_nf(done, "Converted {n} item to {format}", "Converted {n} items to {format}", &[("format", &fmt)])
    };
    if skipped > 0 {
        msg = tr_nf(skipped, "{message}; skipped {n} item (animated)", "{message}; skipped {n} items (animated)", &[("message", &msg)]);
    }
    msg
}

impl App {
    /// The Convert WebP submenu, for a WebP item.
    pub(crate) fn webp_menu(&mut self, ui: &mut Ui, item: &Item, targets: &[String]) {
        if !item.ext.eq_ignore_ascii_case("webp") {
            return;
        }
        crate::ui::menus::search::submenu(ui, tr("Convert WebP"), |ui| {
            for format in [Format::Png, Format::Jpg] {
                if ui.button(format.ext().to_uppercase()).clicked()
                    && let Some(shared) = self.ws.lib.clone()
                {
                    let ids = {
                        let lib = lock(&shared);
                        webps(targets, |id| lib.item(id))
                    };
                    if !ids.is_empty() {
                        self.tools.webp.confirm = Some((ids, format));
                    }
                }
            }
        });
    }

    /// Eagle's warning before converting.
    pub(crate) fn webp_dialog(&mut self, ctx: &egui::Context) {
        let Some((ids, format)) = self.tools.webp.confirm.clone() else { return };
        let fmt = format.ext().to_uppercase();
        let mut answer = None;
        let modal = egui::Modal::new(egui::Id::unique("webp-convert-confirm")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.label(RichText::new(tr_nf(ids.len(), "Convert {n} WebP image to {format}", "Convert {n} WebP images to {format}", &[("format", &fmt)])).size(15.0));
            ui.add_space(4.0);
            ui.label(RichText::new(trf("WebP files are usually smaller, so the {format} files will take more space. Each WebP replaced goes to the Trash; animated WebPs are skipped.", &[("format", &fmt)])).color(t::p().text_muted));
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr("Convert")).clicked() {
                    answer = Some(true);
                }
                if ui.button(tr("Cancel")).clicked() {
                    answer = Some(false);
                }
            });
        });
        if modal.should_close() && answer.is_none() {
            answer = Some(false);
        }
        if let Some(go) = answer {
            self.tools.webp.confirm = None;
            if go {
                self.start_webp_convert(ids, format);
            }
        }
    }

    /// Converts one item at a time behind the progress modal.
    fn start_webp_convert(&mut self, ids: Vec<String>, format: Format) {
        let Some(shared) = self.ws.lib.clone() else { return };
        if self.operation_running() {
            return;
        }
        let h = self.ws.op.start(&self.ctx, tr("Converting WebP"), ids.len(), true);
        std::thread::spawn(move || {
            let (mut done, mut skipped, mut failures, mut kept) = (0, 0, Vec::new(), 0);
            let mut forget = Vec::new();
            for (n, id) in ids.iter().enumerate() {
                if h.cancelled() {
                    break;
                }
                let name = lock(&shared).item(id).map(|i| format!("{}.{}", i.name, i.ext)).unwrap_or_default();
                h.step(n, name.clone());
                let result = lock(&shared).convert_webp(id, format, crate::auto_import::trash);
                match result {
                    Ok(Converted::Done) => done += 1,
                    Ok(Converted::KeptOld(p)) => {
                        done += 1;
                        kept += 1;
                        crate::errlog::error(&format!("Convert WebP: couldn't move {} to the Trash", p.display()));
                    }
                    Ok(Converted::Animated) => skipped += 1,
                    Ok(Converted::NotWebp) => {}
                    Err(e) => failures.push(format!("{name}: {e}")),
                }
                forget.push(id.clone());
            }
            let stopped = h.cancelled() && done + skipped + failures.len() < ids.len();
            let message = summary(done, skipped, format, stopped);
            let mut error = None;
            if !failures.is_empty() {
                for f in &failures {
                    crate::errlog::error(&format!("Convert WebP: {f}"));
                }
                error = Some(tr_nf(
                    failures.len(),
                    "{message}; {n} item couldn't be converted ({reason})",
                    "{message}; {n} items couldn't be converted ({reason})",
                    &[("message", &message), ("reason", &failures[0])],
                ));
            } else if kept > 0 {
                error = Some(tr_nf(
                    kept,
                    "{message}; {n} old WebP file couldn't go to the Trash and was left in its item folder",
                    "{message}; {n} old WebP files couldn't go to the Trash and were left in their item folders",
                    &[("message", &message)],
                ));
            }
            h.finish(Outcome { message, error, forget, changed: done > 0, ..Default::default() });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries() {
        assert_eq!(summary(1, 0, Format::Png, false), "Converted 1 item to PNG");
        assert_eq!(summary(3, 2, Format::Jpg, false), "Converted 3 items to JPG; skipped 2 items (animated)");
        assert_eq!(summary(2, 0, Format::Png, true), "Stopped after converting 2 items to PNG");
    }

    #[test]
    fn only_webps_are_taken() {
        let item = |id: &str, ext: &str| Item { id: id.into(), ext: ext.into(), ..empty() };
        let items = [item("A", "webp"), item("B", "png"), item("C", "WEBP")];
        let got = webps(&["A".into(), "B".into(), "C".into(), "D".into()], |id| items.iter().find(|i| i.id == id));
        assert_eq!(got, ["A", "C"]);
    }

    fn empty() -> Item {
        Item::from_json_str(r#"{"id":"X","name":"x","size":0,"btime":0,"mtime":0,"ext":"png","tags":[],"folders":[],"isDeleted":false,"url":"","annotation":"","modificationTime":0,"height":0,"width":0,"lastModified":0,"palettes":[]}"#).unwrap()
    }
}
