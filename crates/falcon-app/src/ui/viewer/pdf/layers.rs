//! The reader's switchable layers, as pdf.js's layers list: a checkbox per
//! optional content group. One `LayerSwitches` lives in each open document,
//! so a document always opens on its own default configuration.

use eframe::egui::{self, Ui};

use super::{Open, PAGE_BUDGET, PageCache, Req, THUMB_BUDGET};
use crate::locale::tr;
use crate::theme::{self as t, font};

/// Which layers are on, and which of the worker's pictures still count.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct LayerSwitches {
    on: Vec<bool>,
    defaults: Vec<bool>,
    /// Goes up with each switch; pictures drawn for an earlier one are stale.
    pub generation: u32,
    /// The document's layers turned out not to be switchable.
    pub stuck: bool,
}

impl LayerSwitches {
    pub fn new(layers: &[falcon_pdf::Layer]) -> LayerSwitches {
        let defaults: Vec<bool> = layers.iter().map(|l| l.visible).collect();
        LayerSwitches { on: defaults.clone(), defaults, generation: 0, stuck: false }
    }

    pub fn is_on(&self, i: usize) -> bool {
        self.on.get(i).copied().unwrap_or(false)
    }

    /// Whether any layer differs from the default configuration.
    pub fn changed(&self) -> bool {
        self.on != self.defaults
    }

    /// Sets layer `i`; returns the state to apply if that changed anything.
    fn set(&mut self, i: usize, on: bool) -> Option<Vec<bool>> {
        if self.stuck || self.on.get(i).is_none_or(|&was| was == on) {
            return None;
        }
        self.on[i] = on;
        self.generation += 1;
        Some(self.on.clone())
    }

    /// Back to the default configuration; returns the state to apply if
    /// that changed anything.
    fn reset(&mut self) -> Option<Vec<bool>> {
        if !self.changed() {
            return None;
        }
        self.on.clone_from(&self.defaults);
        self.generation += 1;
        Some(self.on.clone())
    }

    /// The worker couldn't switch them: back to the default, for good.
    fn give_up(&mut self) -> Vec<bool> {
        self.stuck = true;
        self.on.clone_from(&self.defaults);
        self.generation += 1;
        self.on.clone()
    }
}

impl Open {
    /// Has the worker draw `on`, dropping every picture of the old state.
    /// The page stays where it is, since layers don't change page sizes.
    fn apply_layers(&mut self, on: Vec<bool>) {
        let _ = self.tx.send(Req::Layers(on));
        self.pages = PageCache::new(PAGE_BUDGET);
        self.thumbs = PageCache::new(THUMB_BUDGET);
        self.broken.clear();
        self.sent.clear();
    }

    /// The worker reported that a switch failed.
    pub(super) fn layers_failed(&mut self) {
        let on = self.layers.give_up();
        self.apply_layers(on);
    }

    /// The sidebar's list.
    pub(super) fn draw_layers(&mut self, ui: &mut Ui) {
        if self.info.layers.is_empty() {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(tr("No layers")).font(font(12.0)).color(t::p().text_muted));
            return;
        }
        let mut switched = None;
        for (i, l) in self.info.layers.iter().enumerate() {
            let mut on = self.layers.is_on(i);
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                let text = egui::RichText::new(&l.name).font(font(12.0)).color(if on { t::p().text } else { t::p().text_muted });
                if ui.add_enabled(!self.layers.stuck, egui::Checkbox::new(&mut on, text)).changed() {
                    switched = Some((i, on));
                }
            });
        }
        if let Some(state) = switched.and_then(|(i, on)| self.layers.set(i, on)) {
            self.apply_layers(state);
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            if self.layers.stuck {
                ui.add(egui::Label::new(egui::RichText::new(tr("This document's layers can't be switched.")).font(font(11.0)).color(t::p().text_muted)).wrap());
            } else if ui.add_enabled(self.layers.changed(), egui::Button::new(egui::RichText::new(tr("Reset Layers")).font(font(11.0)))).clicked()
                && let Some(state) = self.layers.reset()
            {
                self.apply_layers(state);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Job, Reply, spawn};
    use super::*;

    fn layer(visible: bool) -> falcon_pdf::Layer {
        falcon_pdf::Layer { name: "x".into(), visible }
    }

    #[test]
    fn switches_start_at_the_default_and_come_back() {
        let mut s = LayerSwitches::new(&[layer(true), layer(false)]);
        assert!(s.is_on(0) && !s.is_on(1) && !s.changed());
        assert_eq!(s.set(0, true), None, "already on");
        assert_eq!(s.generation, 0);
        assert_eq!(s.set(9, true), None, "no such layer");
        assert_eq!(s.set(0, false), Some(vec![false, false]));
        assert_eq!(s.generation, 1);
        assert!(s.changed());
        assert_eq!(s.reset(), Some(vec![true, false]));
        assert_eq!(s.generation, 2);
        assert_eq!(s.reset(), None, "nothing to reset");
    }

    #[test]
    fn failing_returns_to_the_default_for_good() {
        let mut s = LayerSwitches::new(&[layer(true)]);
        s.set(0, false);
        assert_eq!(s.give_up(), vec![true]);
        assert!(s.stuck && !s.changed());
        assert_eq!(s.set(0, false), None);
    }

    /// One page, a red square in the first of two layers and a blue one in
    /// the second.
    #[cfg(target_os = "macos")]
    fn two_layers() -> Vec<u8> {
        let content = "/OC /A BDC 1 0 0 rg 50 50 100 100 re f EMC /OC /B BDC 0 0 1 rg 250 50 100 100 re f EMC";
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [4 0 R 5 0 R] /D << >> >> >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200] /Resources << /Properties << /A 4 0 R /B 5 0 R >> >> /Contents 6 0 R >>".to_string(),
            "<< /Type /OCG /Name (Red) >>".to_string(),
            "<< /Type /OCG /Name (Blue) >>".to_string(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        ];
        let mut out = b"%PDF-1.5\n".to_vec();
        let mut offsets = Vec::new();
        for (k, body) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", k + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(b"xref\n0 7\n0000000000 65535 f \n");
        for o in offsets {
            out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<< /Size 7 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
        out
    }

    /// The reader's worker redraws for a switch, labelling the picture with
    /// the switch's generation, and a stale request can't get in between.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_worker_draws_the_switched_layers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("two.pdf");
        std::fs::write(&path, two_layers()).unwrap();
        let (tx, rx) = spawn(path, egui::Context::default(), 4096);
        let job = |layers| Job { page: 0, key: 100, turns: 0, gray: false, layers };
        let page = |want: u32| loop {
            match rx.recv_timeout(std::time::Duration::from_secs(20)).unwrap() {
                Reply::Page(j, Some(b)) if j.layers == want => break b,
                Reply::LayersFailed => panic!("a switch failed"),
                _ => {}
            }
        };
        tx.send(Req::Want(vec![job(0)])).unwrap();
        let first = page(0);
        let at = |b: &falcon_pdf::Bitmap, x: f32| {
            let scale = b.width as f32 / 400.0;
            b.pixel((x * scale) as usize, (100.0 * scale) as usize)
        };
        assert_eq!(at(&first, 100.0), [255, 0, 0, 255]);
        tx.send(Req::Layers(vec![false, true])).unwrap();
        tx.send(Req::Want(vec![job(1)])).unwrap();
        let off = page(1);
        assert_eq!(at(&off, 100.0), [255, 255, 255, 255]);
        assert_eq!(at(&off, 300.0), [0, 0, 255, 255]);
        tx.send(Req::Layers(vec![true, true])).unwrap();
        tx.send(Req::Want(vec![job(2)])).unwrap();
        assert_eq!(page(2).rgba, first.rgba);
    }

    /// A worker told a list the document doesn't have says so.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_worker_reports_a_failed_switch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("two.pdf");
        std::fs::write(&path, two_layers()).unwrap();
        let (tx, rx) = spawn(path, egui::Context::default(), 4096);
        tx.send(Req::Layers(vec![false])).unwrap();
        while !matches!(rx.recv_timeout(std::time::Duration::from_secs(20)).unwrap(), Reply::LayersFailed) {}
    }
}
