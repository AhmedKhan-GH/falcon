//! OFF (Object File Format), ASCII: an `OFF` line, `<vertices> <faces>
//! <edges>`, one `x y z` line per vertex and one `n a b c …` line per face.
//!
//! This reader exists for the exporter's round-trip test. It is not wired
//! into file-type detection, thumbnails or the viewer's loader, so falcon
//! does not open `.off` files. Polygons are fan-triangulated; per-face
//! colour values after the indices are ignored.

use super::{Mesh, Model, ParseError, Result};

pub fn parse(bytes: &[u8]) -> Result<Model> {
    let text = std::str::from_utf8(bytes).map_err(|_| ParseError("OFF is not text".into()))?;
    let mut lines = text.lines().map(|l| l.split('#').next().unwrap_or("").trim()).filter(|l| !l.is_empty());
    let bad = |what: &str| ParseError(format!("OFF: {what}"));
    if lines.next() != Some("OFF") {
        return Err(bad("missing OFF header"));
    }
    let counts: Vec<usize> = lines.next().ok_or_else(|| bad("missing counts"))?.split_whitespace().map_while(|t| t.parse().ok()).collect();
    let (&nv, &nf) = (counts.first().ok_or_else(|| bad("bad counts"))?, counts.get(1).ok_or_else(|| bad("bad counts"))?);
    let mut mesh = Mesh::default();
    for _ in 0..nv {
        let line = lines.next().ok_or_else(|| bad("too few vertices"))?;
        let v: Vec<f32> = line.split_whitespace().take(3).map_while(|t| t.parse().ok()).collect();
        let [x, y, z] = v[..] else { return Err(bad("bad vertex")) };
        mesh.positions.push([x, y, z]);
    }
    for _ in 0..nf {
        let line = lines.next().ok_or_else(|| bad("too few faces"))?;
        let mut t = line.split_whitespace();
        let n: usize = t.next().and_then(|s| s.parse().ok()).ok_or_else(|| bad("bad face"))?;
        let ix: Vec<u32> = t.take(n).map_while(|s| s.parse().ok()).collect();
        if ix.len() != n {
            return Err(bad("bad face"));
        }
        for k in 1..n.saturating_sub(1) {
            mesh.indices.extend([ix[0], ix[k], ix[k + 1]]);
        }
    }
    Model { meshes: vec![mesh] }.finish()
}
