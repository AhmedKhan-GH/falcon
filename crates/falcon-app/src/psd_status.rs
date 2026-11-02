//! Whether the PSD in the viewer has a merged image falcon can draw, found
//! out on a thread when it opens. The viewer chooses between the merged
//! image and QuickLook by it, and the inspector gives the reason when it
//! falls back.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};

use falcon_library::psd::{Composite, PsdError};

use crate::locale::tr;

/// The state for the PSD in the viewer.
#[derive(Default)]
pub struct PsdStatus {
    id: String,
    /// The picture's size, or why it can't be read, once known.
    result: Option<Result<(u32, u32), PsdError>>,
    probing: Option<Receiver<Result<(u32, u32), PsdError>>>,
}

impl PsdStatus {
    /// Starts over for item `id`; `path` is read on a thread.
    pub fn open(&mut self, id: &str, path: PathBuf, ctx: &eframe::egui::Context) {
        let (tx, rx) = channel();
        *self = PsdStatus { id: id.to_string(), result: None, probing: Some(rx) };
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Composite::open(&path).map(|c| c.size()));
            ctx.request_repaint();
        });
    }

    /// Takes the result in when the thread has it.
    pub fn poll(&mut self) {
        if let Some(rx) = &self.probing
            && let Ok(r) = rx.try_recv()
        {
            self.result = Some(r);
            self.probing = None;
        }
    }

    fn result(&self, id: &str) -> Option<&Result<(u32, u32), PsdError>> {
        self.result.as_ref().filter(|_| self.id == id)
    }

    /// Whether `id` has a merged image to draw; `None` until known.
    pub fn composite(&self, id: &str) -> Option<bool> {
        self.result(id).map(Result::is_ok)
    }

    /// The merged image's size, upright.
    pub fn size(&self, id: &str) -> Option<(u32, u32)> {
        self.result(id)?.as_ref().ok().copied()
    }

    /// Why the viewer shows QuickLook's render of `id`, if it does.
    pub fn fallback_reason(&self, id: &str) -> Option<String> {
        self.result(id)?.as_ref().err().map(|e| reason(e).to_string())
    }
}

fn reason(e: &PsdError) -> &'static str {
    match e {
        PsdError::NoComposite => tr("QuickLook, saved without a merged image"),
        PsdError::Compression(_) => tr("QuickLook, compression not supported"),
        PsdError::Mode { .. } => tr("QuickLook, colour mode not supported"),
        PsdError::Damaged(_) => tr("QuickLook, the merged image can't be read"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_only_for_the_item_it_was_opened_for() {
        let mut s = PsdStatus { id: "a".into(), result: Some(Err(PsdError::NoComposite)), probing: None };
        assert_eq!(s.composite("a"), Some(false));
        assert_eq!(s.composite("b"), None);
        assert_eq!(s.fallback_reason("b"), None);
        assert!(s.fallback_reason("a").is_some());
        s.result = Some(Ok((3, 4)));
        assert_eq!(s.size("a"), Some((3, 4)));
        assert_eq!(s.fallback_reason("a"), None);
    }
}
