//! IFC geometry through web-ifc (`falcon-ifc`), as Eagle's viewer reads it:
//! every solid, sweep, boolean (openings cut from walls and slabs), CSG
//! primitive and b-spline surface web-ifc can mesh. falcon's own STEP
//! parse of the same file supplies the schema and, for products that come
//! out empty, whether they had a shape at all and which geometry types it
//! used, so the inspector's counts mean the same as with falcon's own
//! reader. A product whose openings cut it away completely is neither
//! shown nor skipped.

use std::collections::{HashMap, HashSet};

use super::ifc::{HIDDEN, Report, display_name, rep_rank};
use super::step::{StepFile, Value};
use super::{Material, Mesh, Model, ParseError, Result, Surface};

/// A placed model this far from the origin (in metres) is moved to it.
const RECENTRE_BEYOND: f64 = 10_000.0;

/// How deeply mapped items are followed when listing geometry types.
const MAX_MAP_DEPTH: usize = 8;

/// Whether web-ifc is built in.
pub fn available() -> bool {
    falcon_ifc::available()
}

/// Reads the file through web-ifc, in its worker process. `Ok(None)` when
/// web-ifc is not built in or could not read the file (it refused it,
/// crashed, ran out of time or memory), so the caller falls back to
/// falcon's own reader.
pub(super) fn read(file: &StepFile, bytes: &[u8]) -> Result<Option<(Option<Model>, Report)>> {
    if !available() {
        return Ok(None);
    }
    let mut limits = falcon_ifc::Limits::default();
    if !sound_graph(file) {
        limits.timeout = SUSPECT_TIMEOUT;
    }
    let scene = match falcon_ifc::read(bytes, limits) {
        Ok(s) => s,
        Err(falcon_ifc::Error::TooLarge) => return Err(ParseError::new("IFC: the model is too large")),
        Err(_) => return Ok(None),
    };
    let mut report = Report { schema: file.schemas.first().cloned().unwrap_or_default(), ..Default::default() };
    let mut unsupported: HashMap<String, usize> = HashMap::new();
    // Products with openings: one that comes out empty was cut away whole
    // (a frame placeholder filled by its opening, say), not left unread.
    let voided: HashSet<u64> = file
        .entities
        .values()
        .filter(|e| e.name == "IFCRELVOIDSELEMENT")
        .filter_map(|e| e.arg(4).as_ref_id())
        .collect();
    for p in &scene.products {
        if p.geometries > 0 {
            report.shown += 1;
            continue;
        }
        let upper = p.type_name.to_ascii_uppercase();
        if HIDDEN.contains(&upper.as_str()) || voided.contains(&u64::from(p.id)) {
            continue;
        }
        let Some(kinds) = shape_kinds(file, u64::from(p.id)) else { continue };
        report.skipped += 1;
        for k in kinds {
            *unsupported.entry(k).or_default() += 1;
        }
    }
    let mut unsupported: Vec<(String, usize)> = unsupported.into_iter().collect();
    unsupported.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    report.unsupported = unsupported;
    let types: HashMap<u32, String> = scene.products.iter().map(|p| (p.id, display_name(&p.type_name.to_ascii_uppercase()))).collect();
    let model = build(&scene, &types);
    report.triangles = model.as_ref().map_or(0, Model::triangle_count);
    Ok(Some((model, report)))
}

/// How long a chain of references counts as sound.
const MAX_GRAPH_DEPTH: usize = 10_000;

/// How long web-ifc gets on a file whose graph isn't sound. It can spin
/// forever on one, so a bad file shouldn't hold up a read for the full
/// timeout, but real files with a stray reference read fine.
const SUSPECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(if cfg!(test) { 2 } else { 30 });

/// Whether every reference resolves, none leads back to itself and no
/// chain is deeper than [`MAX_GRAPH_DEPTH`].
pub(super) fn sound_graph(file: &StepFile) -> bool {
    fn refs(v: &Value, out: &mut Vec<u64>) {
        match v {
            Value::Ref(id) => out.push(*id),
            Value::List(l) | Value::Typed(_, l) => l.iter().for_each(|x| refs(x, out)),
            _ => {}
        }
    }
    // 1 while on the current path, 2 once finished.
    let mut state: HashMap<u64, u8> = HashMap::with_capacity(file.entities.len());
    let mut ids: Vec<u64> = file.entities.keys().copied().collect();
    ids.sort_unstable();
    let mut buf = Vec::new();
    for root in ids {
        if state.contains_key(&root) {
            continue;
        }
        // Each frame: an entity and the references still to visit.
        let mut stack: Vec<(u64, Vec<u64>)> = Vec::new();
        let children = |id: u64, buf: &mut Vec<u64>| -> Option<Vec<u64>> {
            buf.clear();
            for a in &file.get(id)?.args {
                refs(a, buf);
            }
            Some(buf.clone())
        };
        state.insert(root, 1);
        stack.push((root, children(root, &mut buf).unwrap_or_default()));
        while let Some((id, todo)) = stack.last_mut() {
            let Some(next) = todo.pop() else {
                state.insert(*id, 2);
                stack.pop();
                continue;
            };
            match state.get(&next) {
                Some(1) => return false,
                Some(_) => continue,
                None => {}
            }
            let Some(kids) = children(next, &mut buf) else { return false };
            if stack.len() >= MAX_GRAPH_DEPTH {
                return false;
            }
            state.insert(next, 1);
            stack.push((next, kids));
        }
    }
    true
}

/// The geometry types of a product's drawable shape representations, one
/// entry per type; `None` when it has no drawable shape.
fn shape_kinds(file: &StepFile, id: u64) -> Option<Vec<String>> {
    let e = file.get(id)?;
    let pds = file.deref_named(e.arg(6), "IFCPRODUCTDEFINITIONSHAPE")?;
    let mut kinds = Vec::new();
    let mut any = false;
    for r in pds.arg(2).as_list().unwrap_or_default() {
        let Some(re) = file.deref(r) else { continue };
        if rep_rank(re.arg(1).as_str().unwrap_or("")).is_none() {
            continue;
        }
        any = true;
        for it in re.arg(3).as_list().unwrap_or_default() {
            item_kinds(file, it, 0, &mut kinds);
        }
    }
    kinds.sort();
    kinds.dedup();
    any.then_some(kinds)
}

fn item_kinds(file: &StepFile, v: &Value, depth: usize, out: &mut Vec<String>) {
    let Some(e) = file.deref(v) else { return };
    if e.name == "IFCMAPPEDITEM" {
        if depth >= MAX_MAP_DEPTH {
            return;
        }
        let Some(rep) = file.deref_named(e.arg(0), "IFCREPRESENTATIONMAP").and_then(|m| file.deref(m.arg(1))) else { return };
        for it in rep.arg(3).as_list().unwrap_or_default() {
            item_kinds(file, it, depth + 1, out);
        }
    } else {
        out.push(display_name(&e.name));
    }
}

/// One mesh per product type and colour, in metres with Y up.
fn build(scene: &falcon_ifc::Scene, types: &HashMap<u32, String>) -> Option<Model> {
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for g in &scene.geometries {
        for &i in &g.indices {
            let q = g.positions[i as usize];
            for k in 0..3 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
        }
    }
    if lo[0] > hi[0] {
        return None;
    }
    let centre = [0, 1, 2].map(|k| (lo[k] + hi[k]) / 2.0);
    let shift = if centre.iter().any(|c| c.abs() > RECENTRE_BEYOND) { centre } else { [0.0; 3] };
    let mut meshes: Vec<Mesh> = Vec::new();
    let mut by_key: HashMap<(String, [u64; 4]), usize> = HashMap::new();
    for g in &scene.geometries {
        let name = types.get(&g.product).cloned().unwrap_or_default();
        let key = (name.clone(), g.color.map(f64::to_bits));
        let k = *by_key.entry(key).or_insert_with(|| {
            let c = g.color;
            let opacity = if c[3].is_finite() { c[3] as f32 } else { 1.0 };
            let material = Material { name: name.clone(), color: [c[0] as f32, c[1] as f32, c[2] as f32], opacity };
            let surface = Surface { blend: (opacity - 1.0).abs() > 1e-6, ..Surface::default() };
            meshes.push(Mesh { name, material: Some(material), surface, ..Default::default() });
            meshes.len() - 1
        });
        let mesh = &mut meshes[k];
        let base = mesh.positions.len() as u32;
        mesh.positions.extend(g.positions.iter().map(|q| [(q[0] - shift[0]) as f32, (q[1] - shift[1]) as f32, (q[2] - shift[2]) as f32]));
        mesh.indices.extend(g.indices.iter().map(|i| base + i));
    }
    Model { meshes }.finish().ok()
}

#[cfg(test)]
#[path = "ifc_native_tests.rs"]
mod native_tests;
