//! What the inspector lists for an IFC item: how many products falcon
//! draws and skips (through web-ifc when it is built in), the triangle
//! count, the schema, and why there is no 3D preview when nothing could
//! be drawn. Files are read on one
//! background thread at a time and the results kept by item.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};

use falcon_library::model3d::ifc::{self, Report};

use crate::locale::{tr, tr_key};

/// The outcome of reading one file: the report, or the reader's error.
pub(crate) type Facts = Result<Report, String>;

/// Results kept before the cache starts over.
const KEEP: usize = 512;

#[derive(Default)]
pub(crate) struct IfcFacts {
    done: HashMap<String, Facts>,
    /// The key being read and where its result arrives.
    running: Option<(String, Receiver<Facts>)>,
}

impl IfcFacts {
    /// The facts for the file at `path`, known by `key` (item id and size);
    /// `None` while it is being read. Starts reading it when no other file
    /// is being read; otherwise it is started on a later frame.
    pub fn get(&mut self, ctx: &eframe::egui::Context, key: &str, path: PathBuf) -> Option<&Facts> {
        self.poll();
        if self.done.contains_key(key) {
            return self.done.get(key);
        }
        if self.running.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            let c = ctx.clone();
            let spawned = std::thread::Builder::new().name("falcon-ifc-facts".into()).spawn(move || {
                let _ = tx.send(ifc::summary(&path).map_err(|e| e.0));
                c.request_repaint();
            });
            match spawned {
                Ok(_) => self.running = Some((key.to_string(), rx)),
                Err(e) => {
                    self.done.insert(key.to_string(), Err(e.to_string()));
                    return self.done.get(key);
                }
            }
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }
        None
    }

    fn poll(&mut self) {
        let Some((key, rx)) = &self.running else { return };
        let facts = match rx.try_recv() {
            Ok(f) => f,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(String::new()),
        };
        if self.done.len() >= KEEP {
            self.done.clear();
        }
        self.done.insert(key.clone(), facts);
        self.running = None;
    }
}

/// The inspector rows for an IFC item, as (label key, value); `None` is
/// still being read.
pub(crate) fn fact_rows(facts: Option<&Facts>) -> Vec<(&'static str, String)> {
    let preview = tr_key("3D Preview");
    let Some(facts) = facts else { return vec![(preview, tr("Reading…").to_string())] };
    let r = match facts {
        Ok(r) => r,
        Err(_) => return vec![(preview, tr("The file could not be read").to_string())],
    };
    let mut rows = Vec::new();
    if r.fallback {
        rows.push((preview, tr("IFC: web-ifc could not read this file").to_string()));
    } else if r.shown == 0 {
        // With web-ifc every solid kind is read, so an empty file has no
        // 3D geometry at all; without it only the tessellated part is.
        let why = if ifc::full_geometry() { tr("IFC: no 3D geometry") } else { tr("IFC: no tessellated geometry") };
        rows.push((preview, why.to_string()));
    }
    if r.shown > 0 {
        rows.push((tr_key("Products Shown"), r.shown.to_string()));
    }
    if r.skipped > 0 {
        rows.push((tr_key("Products Skipped"), r.skipped.to_string()));
        let kinds: Vec<String> = r.unsupported.iter().take(3).map(|(k, n)| format!("{k} ({n})")).collect();
        if !kinds.is_empty() {
            rows.push((tr_key("Not Read"), kinds.join(", ")));
        }
    }
    if r.shown > 0 {
        rows.push((tr_key("Triangles"), r.triangles.to_string()));
    }
    if !r.schema.is_empty() {
        rows.push((tr_key("Schema"), r.schema.clone()));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(rows: &[(&'static str, String)]) -> Vec<&'static str> {
        rows.iter().map(|r| r.0).collect()
    }

    #[test]
    fn rows_for_each_outcome() {
        assert_eq!(labels(&fact_rows(None)), ["3D Preview"]);
        assert_eq!(labels(&fact_rows(Some(&Err("bad".into())))), ["3D Preview"]);
        let mixed = Report { schema: "IFC4".into(), shown: 3, skipped: 2, triangles: 120, unsupported: vec![("IfcExtrudedAreaSolid".into(), 2)], fallback: false };
        let rows = fact_rows(Some(&Ok(mixed)));
        assert_eq!(labels(&rows), ["Products Shown", "Products Skipped", "Not Read", "Triangles", "Schema"]);
        assert_eq!(rows[2].1, "IfcExtrudedAreaSolid (2)");
        assert_eq!(rows[3].1, "120");
        let none = Report { schema: String::new(), shown: 0, skipped: 1, triangles: 0, unsupported: vec![], fallback: false };
        let rows = fact_rows(Some(&Ok(none)));
        assert_eq!(labels(&rows), ["3D Preview", "Products Skipped"]);
        let why = if ifc::full_geometry() { "IFC: no 3D geometry" } else { "IFC: no tessellated geometry" };
        assert_eq!(rows[0].1, why);
        // web-ifc failed: the reason shows along with what the own reader found.
        let fell_back = Report { schema: "IFC4".into(), shown: 2, skipped: 1, triangles: 24, unsupported: vec![("IfcBooleanResult".into(), 1)], fallback: true };
        let rows = fact_rows(Some(&Ok(fell_back)));
        assert_eq!(labels(&rows), ["3D Preview", "Products Shown", "Products Skipped", "Not Read", "Triangles", "Schema"]);
        assert_eq!(rows[0].1, "IFC: web-ifc could not read this file");
    }
}
