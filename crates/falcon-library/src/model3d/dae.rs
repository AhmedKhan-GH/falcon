//! COLLADA (`.dae`), versions 1.4.1 and 1.5: XML read with `roxmltree`.
//!
//! Read: the meshes of `library_geometries` (`triangles`, `polylist`,
//! `polygons`, `tristrips` and `trifans`, polygons fan-triangulated, holes
//! of `ph` ignored), their sources through accessors with offset, stride
//! and named params, positions, normals and vertex colours; the node tree
//! of the visual scene with `matrix`, `translate`, `rotate`, `scale` and
//! `lookat` composed into world transforms, `instance_geometry`,
//! `instance_node` and `instance_controller` (the skin's or morph's base
//! mesh in its bind pose, after the skin's bind shape matrix); the
//! `up_axis` and `unit` of the root `asset`, turned into falcon's Y-up
//! metres the way three.js's loader (used by Eagle's viewer) does;
//! material base colours from the `library_effects` diffuse (emission for
//! `constant`), or, when the diffuse is a texture whose image sits next to
//! the file, that image's average colour.
//!
//! Not read: animation, skinning weights and joints, morph targets,
//! cameras, lights, `skew`, lines, texture coordinates (falcon's mesh has
//! no UVs, so textures are not mapped) and transparency. A missing normal
//! input leaves `normals` empty; the viewer then shades flat or generates
//! smooth normals as for any other format.

use std::collections::HashMap;
use std::rc::Rc;

use roxmltree::{Document, Node};

use super::math::{self, M4, V3};
use super::{DEFAULT_COLOR, Material, Mesh, Model, ParseError, Result};

/// Deepest node nesting followed (also stops `instance_node` cycles).
const MAX_DEPTH: usize = 32;
/// Most nodes visited, against `instance_node` fan-out.
const MAX_VISITS: usize = 100_000;
/// Most triangles produced over all instances.
const MAX_TRIANGLES: usize = 50_000_000;
/// Most XML nodes parsed.
const MAX_XML_NODES: u32 = 20_000_000;

type Side<'s> = &'s dyn Fn(&str) -> Option<Vec<u8>>;

pub fn parse(bytes: &[u8], side: Side) -> Result<Model> {
    let text = String::from_utf8_lossy(bytes);
    let opts = roxmltree::ParsingOptions { allow_dtd: true, nodes_limit: MAX_XML_NODES, ..Default::default() };
    let doc = Document::parse_with_options(&text, opts).map_err(|e| ParseError(format!("not a COLLADA file: {e}")))?;
    let root = doc.root_element();
    if root.tag_name().name() != "COLLADA" {
        return Err(ParseError::new("not a COLLADA file"));
    }
    let mut r = Reader::new(root, side);
    r.read()?;
    Model { meshes: r.meshes }.finish()
}

// ---- XML helpers -----------------------------------------------------------

fn child<'a, 'i>(n: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| c.is_element() && c.tag_name().name() == name)
}

fn children<'a, 'i: 'a>(n: Node<'a, 'i>, name: &'a str) -> impl Iterator<Item = Node<'a, 'i>> + 'a {
    n.children().filter(move |c| c.is_element() && c.tag_name().name() == name)
}

/// `#id` (or a bare id) to the id.
fn url_id(s: &str) -> &str {
    s.trim().trim_start_matches('#')
}

fn floats(n: Node) -> Vec<f32> {
    n.text().unwrap_or("").split_ascii_whitespace().map(|t| t.parse::<f32>().ok().filter(|v| v.is_finite()).unwrap_or(0.0)).collect()
}

/// Integers of a `p` or `vcount`; anything that is not a valid index
/// becomes `u32::MAX`, which no source is long enough to reach.
fn indices(n: Node) -> Vec<u32> {
    n.text().unwrap_or("").split_ascii_whitespace().map(|t| t.parse::<u32>().unwrap_or(u32::MAX)).collect()
}

// ---- transforms --------------------------------------------------------------

const IDENTITY: M4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// A COLLADA `matrix` (16 values, row by row) as falcon's column-major
/// matrix.
fn row_major(v: &[f32]) -> M4 {
    let mut m = IDENTITY;
    if v.len() >= 16 {
        for (r, row) in v.chunks(4).take(4).enumerate() {
            for (c, &x) in row.iter().enumerate() {
                m[c][r] = x;
            }
        }
    }
    m
}

fn translation(t: V3) -> M4 {
    let mut m = IDENTITY;
    m[3] = [t[0], t[1], t[2], 1.0];
    m
}

fn scaling(s: V3) -> M4 {
    let mut m = IDENTITY;
    for (k, &v) in s.iter().enumerate() {
        m[k][k] = v;
    }
    m
}

/// Rotation by `deg` degrees about `axis`, counter-clockwise looking down
/// the axis.
fn rotation(axis: V3, deg: f32) -> M4 {
    let a = math::normalize(axis);
    if a == [0.0; 3] {
        return IDENTITY;
    }
    let mut m = IDENTITY;
    for (k, col) in m.iter_mut().take(3).enumerate() {
        let mut e = [0.0; 3];
        e[k] = 1.0;
        let r = math::rotate_about(e, [0.0; 3], a, deg.to_radians());
        *col = [r[0], r[1], r[2], 0.0];
    }
    m
}

/// `lookat`: eye, target, up; places the node at the eye looking at the
/// target (the inverse of a view matrix).
fn lookat(v: &[f32]) -> M4 {
    let [ex, ey, ez, tx, ty, tz, ux, uy, uz] = v[..] else { return IDENTITY };
    let eye = [ex, ey, ez];
    let f = math::normalize(math::sub([tx, ty, tz], eye));
    let s = math::normalize(math::cross(f, [ux, uy, uz]));
    let u = math::cross(s, f);
    if s == [0.0; 3] || f == [0.0; 3] {
        return translation(eye);
    }
    [[s[0], s[1], s[2], 0.0], [u[0], u[1], u[2], 0.0], [-f[0], -f[1], -f[2], 0.0], [ex, ey, ez, 1.0]]
}

/// The node's own transform: its transform elements in document order.
fn local_transform(node: Node) -> M4 {
    let mut m = IDENTITY;
    for c in node.children().filter(|c| c.is_element()) {
        let v = floats(c);
        let t = match c.tag_name().name() {
            "matrix" => row_major(&v),
            "translate" if v.len() >= 3 => translation([v[0], v[1], v[2]]),
            "scale" if v.len() >= 3 => scaling([v[0], v[1], v[2]]),
            "rotate" if v.len() >= 4 => rotation([v[0], v[1], v[2]], v[3]),
            "lookat" if v.len() >= 9 => lookat(&v[..9]),
            _ => continue,
        };
        m = math::mul(&m, &t);
    }
    m
}

fn det3(m: &M4) -> f32 {
    let c = |i: usize, j: usize| m[j][i];
    c(0, 0) * (c(1, 1) * c(2, 2) - c(1, 2) * c(2, 1)) - c(0, 1) * (c(1, 0) * c(2, 2) - c(1, 2) * c(2, 0)) + c(0, 2) * (c(1, 0) * c(2, 1) - c(1, 1) * c(2, 0))
}

/// The matrix that carries normals: the inverse transpose of the upper
/// 3×3, up to scale (the cofactor matrix), applied as `n' = N n`.
fn normal_matrix(m: &M4) -> [[f32; 3]; 3] {
    let a = |r: usize, c: usize| m[c][r];
    let mut n = [[0.0; 3]; 3];
    for (r, row) in n.iter_mut().enumerate() {
        for (c, cell) in row.iter_mut().enumerate() {
            let (r1, r2) = ((r + 1) % 3, (r + 2) % 3);
            let (c1, c2) = ((c + 1) % 3, (c + 2) % 3);
            *cell = a(r1, c1) * a(r2, c2) - a(r1, c2) * a(r2, c1);
        }
    }
    n
}

fn apply3(n: &[[f32; 3]; 3], v: V3) -> V3 {
    let k = |r: usize| n[r][0] * v[0] + n[r][1] * v[1] + n[r][2] * v[2];
    math::normalize([k(0), k(1), k(2)])
}

/// The root `asset`'s up axis and unit as one matrix into Y-up metres.
fn asset_matrix(root: Node) -> M4 {
    let Some(asset) = child(root, "asset") else { return IDENTITY };
    let up = child(asset, "up_axis").and_then(|n| n.text()).unwrap_or("Y_UP").trim().to_ascii_uppercase();
    let axes = match up.as_str() {
        // (x, y, z) -> (x, z, -y): three.js turns Z-up scenes -90° about X.
        "Z_UP" => [[1.0, 0.0, 0.0, 0.0], [0.0, 0.0, -1.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]],
        // (x, y, z) -> (-y, x, z): +X becomes up.
        "X_UP" => [[0.0, 1.0, 0.0, 0.0], [-1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]],
        _ => IDENTITY,
    };
    let meter = child(asset, "unit").and_then(|n| n.attribute("meter")).and_then(|s| s.trim().parse::<f32>().ok()).filter(|m| m.is_finite() && *m > 0.0).unwrap_or(1.0);
    math::mul(&scaling([meter; 3]), &axes)
}

// ---- meshes ----------------------------------------------------------------

/// One primitive of a geometry, in the geometry's own space.
struct Prim {
    positions: Vec<V3>,
    normals: Option<Vec<V3>>,
    colors: Option<Vec<V3>>,
    indices: Vec<u32>,
    /// The `material` symbol, bound per instance.
    material: Option<String>,
}

/// A source's values through its accessor, `k` components per element
/// (missing ones are 0).
fn read_source(src: Node, k: usize) -> Vec<V3> {
    let Some(arr) = child(src, "float_array") else { return Vec::new() };
    let data = floats(arr);
    let acc = child(src, "technique_common").and_then(|t| child(t, "accessor"));
    let attr = |name: &str, default: usize| acc.and_then(|a| a.attribute(name)).and_then(|s| s.trim().parse::<usize>().ok()).unwrap_or(default);
    let stride = attr("stride", 1).max(1);
    let offset = attr("offset", 0);
    let avail = data.len().saturating_sub(offset) / stride;
    let count = attr("count", avail).min(avail);
    // The components to read: the named params, else the first ones.
    let mut cols: Vec<usize> = acc.map(|a| children(a, "param").enumerate().filter(|(_, p)| p.attribute("name").is_some_and(|n| !n.is_empty())).map(|(i, _)| i).collect()).unwrap_or_default();
    if cols.is_empty() {
        cols = (0..stride.min(k)).collect();
    }
    cols.retain(|&c| c < stride);
    cols.truncate(k);
    (0..count)
        .map(|i| {
            let base = offset + i * stride;
            let mut v = [0.0; 3];
            for (slot, &c) in v.iter_mut().zip(&cols) {
                *slot = data[base + c];
            }
            v
        })
        .collect()
}

struct Inputs {
    position: Option<(Vec<V3>, usize)>,
    normal: Option<(Vec<V3>, usize)>,
    color: Option<(Vec<V3>, usize)>,
    /// Indices per corner in `p`.
    stride: usize,
}

fn read_inputs(prim: Node, mesh: Node, ids: &HashMap<&str, Node>) -> Inputs {
    let mut out = Inputs { position: None, normal: None, color: None, stride: 1 };
    let source = |id: &str, k: usize| ids.get(url_id(id)).map(|n| read_source(*n, k)).unwrap_or_default();
    for input in children(prim, "input") {
        let offset = input.attribute("offset").and_then(|s| s.trim().parse::<usize>().ok()).unwrap_or(0);
        // A corner never has more than a few dozen indices; a silly offset
        // is ignored rather than allowed to stretch every read.
        if offset > 64 {
            continue;
        }
        out.stride = out.stride.max(offset + 1);
        let src = input.attribute("source").unwrap_or("");
        match input.attribute("semantic").unwrap_or("") {
            "VERTEX" => {
                // The `vertices` element of this mesh: POSITION and friends,
                // all indexed at this offset.
                let Some(v) = children(mesh, "vertices").find(|v| v.attribute("id") == Some(url_id(src))) else { continue };
                for vi in children(v, "input") {
                    let s = vi.attribute("source").unwrap_or("");
                    match vi.attribute("semantic").unwrap_or("") {
                        "POSITION" => out.position = Some((source(s, 3), offset)),
                        "NORMAL" if out.normal.is_none() => out.normal = Some((source(s, 3), offset)),
                        "COLOR" if out.color.is_none() => out.color = Some((source(s, 3), offset)),
                        _ => {}
                    }
                }
            }
            "NORMAL" => out.normal = Some((source(src, 3), offset)),
            "COLOR" if out.color.is_none() => out.color = Some((source(src, 3), offset)),
            _ => {}
        }
    }
    out
}

/// Builds a primitive from polygons given as lists of corners, each corner
/// being the start of its indices in `p`.
struct Builder<'a> {
    inputs: &'a Inputs,
    p: &'a [u32],
    prim: Prim,
    seen: HashMap<(u32, u32, u32), u32>,
}

impl<'a> Builder<'a> {
    fn new(inputs: &'a Inputs, p: &'a [u32], material: Option<String>) -> Self {
        let prim = Prim { positions: Vec::new(), normals: inputs.normal.as_ref().map(|_| Vec::new()), colors: inputs.color.as_ref().map(|_| Vec::new()), indices: Vec::new(), material };
        Builder { inputs, p, prim, seen: HashMap::new() }
    }

    /// The output vertex of corner number `c` of `p`, if its indices are
    /// all in range.
    fn corner(&mut self, c: usize) -> Option<u32> {
        let base = c.checked_mul(self.inputs.stride)?;
        let at = |src: &Option<(Vec<V3>, usize)>| -> Option<Option<u32>> {
            match src {
                None => Some(None),
                Some((vals, off)) => {
                    let i = *self.p.get(base + off)?;
                    ((i as usize) < vals.len()).then_some(Some(i))
                }
            }
        };
        let (pos, nrm, col) = (at(&self.inputs.position)??, at(&self.inputs.normal)?, at(&self.inputs.color)?);
        let key = (pos, nrm.unwrap_or(0), col.unwrap_or(0));
        if let Some(&v) = self.seen.get(&key) {
            return Some(v);
        }
        let v = self.prim.positions.len() as u32;
        self.prim.positions.push(self.inputs.position.as_ref()?.0[pos as usize]);
        if let (Some(out), Some((vals, _)), Some(i)) = (self.prim.normals.as_mut(), &self.inputs.normal, nrm) {
            out.push(vals[i as usize]);
        }
        if let (Some(out), Some((vals, _)), Some(i)) = (self.prim.colors.as_mut(), &self.inputs.color, col) {
            out.push(vals[i as usize]);
        }
        self.seen.insert(key, v);
        Some(v)
    }

    /// A polygon of `n` corners starting at corner `first`, fanned.
    fn polygon(&mut self, first: usize, n: usize) {
        let Some(vs) = (first..first + n).map(|c| self.corner(c)).collect::<Option<Vec<u32>>>() else { return };
        for k in 1..n.saturating_sub(1) {
            self.prim.indices.extend([vs[0], vs[k], vs[k + 1]]);
        }
    }

    fn strip(&mut self, n: usize) {
        let Some(vs) = (0..n).map(|c| self.corner(c)).collect::<Option<Vec<u32>>>() else { return };
        for k in 0..n.saturating_sub(2) {
            let t = if k % 2 == 0 { [vs[k], vs[k + 1], vs[k + 2]] } else { [vs[k + 1], vs[k], vs[k + 2]] };
            self.prim.indices.extend(t);
        }
    }

    fn corners(&self) -> usize {
        self.p.len() / self.inputs.stride
    }
}

/// The primitives of a `mesh` element.
fn read_mesh(mesh: Node, ids: &HashMap<&str, Node>) -> Vec<Prim> {
    let mut out = Vec::new();
    for prim in mesh.children().filter(|c| c.is_element()) {
        let kind = prim.tag_name().name();
        if !matches!(kind, "triangles" | "polylist" | "polygons" | "tristrips" | "trifans") {
            continue;
        }
        let inputs = read_inputs(prim, mesh, ids);
        if inputs.position.is_none() {
            continue;
        }
        let material = prim.attribute("material").map(str::to_string);
        // Each `p` (or the `p` of each `ph`) as its own polygon or strip.
        let lists: Vec<Vec<u32>> = match kind {
            "triangles" | "polylist" => child(prim, "p").map(indices).into_iter().collect(),
            _ => prim.children().filter(|c| c.is_element()).filter_map(|c| match c.tag_name().name() {
                "p" => Some(indices(c)),
                "ph" => child(c, "p").map(indices),
                _ => None,
            }).collect(),
        };
        let mut b = Builder::new(&inputs, &[], material);
        for p in &lists {
            b.p = p;
            match kind {
                "triangles" => {
                    for t in 0..b.corners() / 3 {
                        b.polygon(t * 3, 3);
                    }
                }
                "polylist" => {
                    let counts = child(prim, "vcount").map(indices).unwrap_or_default();
                    let mut at = 0usize;
                    for &n in &counts {
                        let n = n as usize;
                        if n > b.corners().saturating_sub(at) {
                            break;
                        }
                        b.polygon(at, n);
                        at += n;
                    }
                }
                "tristrips" => {
                    let n = b.corners();
                    b.strip(n);
                }
                // polygons and trifans: one polygon per `p`.
                _ => {
                    let n = b.corners();
                    b.polygon(0, n);
                }
            }
        }
        out.push(b.prim);
    }
    out
}

// ---- materials -------------------------------------------------------------

/// Undoes `%xx` escapes of a URI path.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The average colour of the image a model refers to by `uri`, looked up
/// next to the model: by its relative path, else by its file name alone.
pub(super) fn texture_color(side: Side, uri: &str) -> Option<V3> {
    let path = percent_decode(uri.trim());
    let path = path.strip_prefix("file://").unwrap_or(&path).replace('\\', "/");
    let name = path.rsplit('/').next().unwrap_or(&path).to_string();
    let bytes = side(&path).or_else(|| side(&name))?;
    let img = image::load_from_memory(&bytes).ok()?.thumbnail(64, 64).to_rgba8();
    let (mut sum, mut weight) = ([0.0f64; 3], 0.0f64);
    for px in img.pixels() {
        let a = f64::from(px[3]) / 255.0;
        for k in 0..3 {
            sum[k] += f64::from(px[k]) / 255.0 * a;
        }
        weight += a;
    }
    (weight > 0.0).then(|| sum.map(|s| (s / weight) as f32))
}

// ---- the scene ---------------------------------------------------------------

struct Reader<'a, 'i, 's> {
    root: Node<'a, 'i>,
    ids: HashMap<&'a str, Node<'a, 'i>>,
    side: Side<'s>,
    up: M4,
    prims: HashMap<String, Rc<Vec<Prim>>>,
    materials: HashMap<String, Option<Material>>,
    visits: usize,
    triangles: usize,
    meshes: Vec<Mesh>,
}

impl<'a, 'i, 's> Reader<'a, 'i, 's> {
    fn new(root: Node<'a, 'i>, side: Side<'s>) -> Self {
        let mut ids = HashMap::new();
        for n in root.descendants().filter(|n| n.is_element()) {
            if let Some(id) = n.attribute("id") {
                ids.entry(id).or_insert(n);
            }
        }
        Reader { root, ids, side, up: asset_matrix(root), prims: HashMap::new(), materials: HashMap::new(), visits: 0, triangles: 0, meshes: Vec::new() }
    }

    fn read(&mut self) -> Result<()> {
        let scene = child(self.root, "scene")
            .and_then(|s| child(s, "instance_visual_scene"))
            .and_then(|i| i.attribute("url"))
            .and_then(|u| self.ids.get(url_id(u)).copied())
            .or_else(|| child(self.root, "library_visual_scenes").and_then(|l| child(l, "visual_scene")));
        match scene {
            Some(vs) => {
                let up = self.up;
                for node in children(vs, "node") {
                    self.visit(node, up, 0)?;
                }
            }
            None => {
                // No scene: every geometry once, untransformed.
                let geoms: Vec<Node> = child(self.root, "library_geometries").map(|l| children(l, "geometry").collect()).unwrap_or_default();
                for g in geoms {
                    let up = self.up;
                    self.emit(g, &up, None, "")?;
                }
            }
        }
        Ok(())
    }

    fn visit(&mut self, node: Node<'a, 'i>, parent: M4, depth: usize) -> Result<()> {
        self.visits += 1;
        if depth > MAX_DEPTH || self.visits > MAX_VISITS {
            return Ok(());
        }
        let world = math::mul(&parent, &local_transform(node));
        let name = node.attribute("name").or(node.attribute("id")).unwrap_or("");
        for c in node.children().filter(|c| c.is_element()) {
            let target = || c.attribute("url").and_then(|u| self.ids.get(url_id(u)).copied());
            match c.tag_name().name() {
                "node" => self.visit(c, world, depth + 1)?,
                "instance_node" => {
                    if let Some(n) = target().filter(|n| n.tag_name().name() == "node") {
                        self.visit(n, world, depth + 1)?;
                    }
                }
                "instance_geometry" => {
                    if let Some(g) = target().filter(|n| n.tag_name().name() == "geometry") {
                        self.emit(g, &world, child(c, "bind_material"), name)?;
                    }
                }
                "instance_controller" => {
                    if let Some((g, bind)) = target().and_then(|ctl| self.controller_base(ctl)) {
                        let m = math::mul(&world, &bind);
                        self.emit(g, &m, child(c, "bind_material"), name)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// The geometry under a controller (following skins of morphs) and the
    /// skin's bind shape matrix.
    fn controller_base(&self, mut ctl: Node<'a, 'i>) -> Option<(Node<'a, 'i>, M4)> {
        let mut bind = IDENTITY;
        for _ in 0..8 {
            match ctl.tag_name().name() {
                "geometry" => return Some((ctl, bind)),
                "controller" => {
                    let inner = ctl.children().find(|c| c.is_element() && matches!(c.tag_name().name(), "skin" | "morph"))?;
                    if inner.tag_name().name() == "skin"
                        && let Some(b) = child(inner, "bind_shape_matrix")
                    {
                        bind = math::mul(&bind, &row_major(&floats(b)));
                    }
                    ctl = *self.ids.get(url_id(inner.attribute("source")?))?;
                }
                _ => return None,
            }
        }
        None
    }

    fn geometry_prims(&mut self, g: Node<'a, 'i>) -> Rc<Vec<Prim>> {
        let key = g.attribute("id").map(str::to_string).unwrap_or_else(|| format!("@{}", g.range().start));
        if let Some(p) = self.prims.get(&key) {
            return p.clone();
        }
        let prims = Rc::new(child(g, "mesh").map(|m| read_mesh(m, &self.ids)).unwrap_or_default());
        self.prims.insert(key, prims.clone());
        prims
    }

    fn emit(&mut self, g: Node<'a, 'i>, m: &M4, bind: Option<Node<'a, 'i>>, node_name: &str) -> Result<()> {
        let prims = self.geometry_prims(g);
        let name = if node_name.is_empty() { g.attribute("name").or(g.attribute("id")).unwrap_or("") } else { node_name };
        let nm = normal_matrix(m);
        let flip = det3(m) < 0.0;
        for prim in prims.iter() {
            self.triangles += prim.indices.len() / 3;
            if self.triangles > MAX_TRIANGLES {
                return Err(ParseError::new("the COLLADA file has too many triangles"));
            }
            let material = prim.material.as_deref().and_then(|sym| self.bound_material(bind, sym));
            let mut indices = prim.indices.clone();
            if flip {
                for t in indices.as_chunks_mut::<3>().0 {
                    t.swap(1, 2);
                }
            }
            self.meshes.push(Mesh {
                name: name.to_string(),
                positions: prim.positions.iter().map(|&p| math::transform_point(m, p)).collect(),
                normals: prim.normals.as_ref().map(|ns| ns.iter().map(|&n| apply3(&nm, n)).collect()),
                colors: prim.colors.clone(),
                indices,
                material,
                ..Mesh::default()
            });
        }
        Ok(())
    }

    /// The material an instance binds to the primitive symbol `sym`; files
    /// without a binding often name the material itself.
    fn bound_material(&mut self, bind: Option<Node<'a, 'i>>, sym: &str) -> Option<Material> {
        let target = bind
            .and_then(|b| child(b, "technique_common"))
            .and_then(|t| children(t, "instance_material").find(|im| im.attribute("symbol") == Some(sym)))
            .and_then(|im| im.attribute("target"))
            .map(url_id)
            .unwrap_or(sym)
            .to_string();
        if let Some(m) = self.materials.get(&target) {
            return m.clone();
        }
        let m = self.material(&target);
        self.materials.insert(target, m.clone());
        m
    }

    fn material(&self, id: &str) -> Option<Material> {
        let mat = self.ids.get(id).copied().filter(|n| n.tag_name().name() == "material")?;
        let name = mat.attribute("name").unwrap_or(id).to_string();
        let effect = child(mat, "instance_effect").and_then(|i| i.attribute("url")).and_then(|u| self.ids.get(url_id(u)).copied());
        let color = effect.and_then(|e| self.effect_color(e)).unwrap_or(DEFAULT_COLOR);
        Some(Material { name, color, opacity: 1.0 })
    }

    fn effect_color(&self, effect: Node<'a, 'i>) -> Option<V3> {
        let profile = child(effect, "profile_COMMON")?;
        let technique = child(profile, "technique")?;
        let shader = technique.children().find(|c| c.is_element() && matches!(c.tag_name().name(), "phong" | "lambert" | "blinn" | "constant"))?;
        let slot = if shader.tag_name().name() == "constant" { "emission" } else { "diffuse" };
        let value = child(shader, slot)?;
        if let Some(c) = child(value, "color") {
            let v = floats(c);
            return (v.len() >= 3).then(|| [v[0], v[1], v[2]].map(|x| x.clamp(0.0, 1.0)));
        }
        let tex = child(value, "texture")?.attribute("texture")?;
        let uri = self.texture_uri(profile, tex)?;
        texture_color(self.side, &uri)
    }

    /// The image file a `texture` attribute names: through the profile's
    /// sampler and surface params (1.4.1), the sampler's `instance_image`
    /// (1.5), or an image id used directly.
    fn texture_uri(&self, profile: Node<'a, 'i>, tex: &str) -> Option<String> {
        let param = |sid: &str| children(profile, "newparam").find(|p| p.attribute("sid") == Some(sid));
        let mut image = None;
        if let Some(sampler) = param(tex).and_then(|p| child(p, "sampler2D")) {
            if let Some(url) = child(sampler, "instance_image").and_then(|i| i.attribute("url")) {
                image = self.ids.get(url_id(url)).copied();
            } else if let Some(surface_sid) = child(sampler, "source").and_then(|s| s.text()) {
                let init = param(surface_sid.trim()).and_then(|p| child(p, "surface")).and_then(|s| child(s, "init_from")).and_then(|i| i.text());
                image = init.and_then(|id| self.ids.get(id.trim()).copied());
            }
        }
        let image = image.or_else(|| self.ids.get(tex).copied()).filter(|n| n.tag_name().name() == "image")?;
        let init = child(image, "init_from")?;
        // 1.5 wraps the path in `ref`.
        let text = child(init, "ref").and_then(|r| r.text()).or(init.text())?;
        Some(text.trim().to_string()).filter(|s| !s.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &str) -> Option<Vec<u8>> {
        None
    }

    const CUBE_POS: &str = "-1 -1 -1 1 -1 -1 1 1 -1 -1 1 -1 -1 -1 1 1 -1 1 1 1 1 -1 1 1";
    /// Six outward quads over the corners above.
    const QUADS: [[u32; 4]; 6] = [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [2, 3, 7, 6], [1, 2, 6, 5], [0, 4, 7, 3]];
    const NORMALS: [V3; 6] = [[0.0, 0.0, -1.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [-1.0, 0.0, 0.0]];

    /// A cube document: `prim` is the primitive element, `node` the inner
    /// part of the scene node, `asset` the asset's contents.
    fn doc(asset: &str, prim: &str, node: &str, extra: &str) -> String {
        let normals: Vec<String> = NORMALS.iter().map(|n| format!("{} {} {}", n[0], n[1], n[2])).collect();
        format!(
            r##"<?xml version="1.0" encoding="utf-8"?>
<COLLADA xmlns="http://www.collada.org/2005/11/COLLADASchema" version="1.4.1">
  <asset>{asset}</asset>
  <library_effects>
    <effect id="red-fx"><profile_COMMON><technique sid="common"><phong>
      <diffuse><color>1 0 0 1</color></diffuse></phong></technique></profile_COMMON></effect>
    <effect id="tex-fx"><profile_COMMON>
      <newparam sid="surf"><surface type="2D"><init_from>green-img</init_from></surface></newparam>
      <newparam sid="samp"><sampler2D><source>surf</source></sampler2D></newparam>
      <technique sid="common"><lambert><diffuse><texture texture="samp" texcoord="UV"/></diffuse></lambert></technique>
    </profile_COMMON></effect>
  </library_effects>
  <library_images><image id="green-img"><init_from>textures/green%20tex.png</init_from></image></library_images>
  <library_materials>
    <material id="red-mat" name="Red"><instance_effect url="#red-fx"/></material>
    <material id="tex-mat" name="Tex"><instance_effect url="#tex-fx"/></material>
  </library_materials>
  <library_geometries>
    <geometry id="cube" name="Cube"><mesh>
      <source id="cube-pos"><float_array id="cube-pos-arr" count="24">{CUBE_POS}</float_array>
        <technique_common><accessor source="#cube-pos-arr" count="8" stride="3">
          <param name="X" type="float"/><param name="Y" type="float"/><param name="Z" type="float"/></accessor></technique_common></source>
      <source id="cube-nrm"><float_array id="cube-nrm-arr" count="18">{normals}</float_array>
        <technique_common><accessor source="#cube-nrm-arr" count="6" stride="3">
          <param name="X" type="float"/><param name="Y" type="float"/><param name="Z" type="float"/></accessor></technique_common></source>
      <vertices id="cube-vtx"><input semantic="POSITION" source="#cube-pos"/></vertices>
      {prim}
    </mesh></geometry>
  </library_geometries>
  {extra}
  <library_visual_scenes><visual_scene id="scene">
    <node id="n" name="CubeNode">{node}</node>
  </visual_scene></library_visual_scenes>
  <scene><instance_visual_scene url="#scene"/></scene>
</COLLADA>"##,
            normals = normals.join(" ")
        )
    }

    const BIND_RED: &str = r##"<instance_geometry url="#cube"><bind_material><technique_common><instance_material symbol="m" target="#red-mat"/></technique_common></bind_material></instance_geometry>"##;

    /// A polylist of the six quads with normals at offset 1.
    fn polylist() -> String {
        let p: Vec<String> = QUADS.iter().enumerate().flat_map(|(f, q)| q.iter().map(move |v| format!("{v} {f}"))).collect();
        format!(
            r##"<polylist material="m" count="6"><input semantic="VERTEX" source="#cube-vtx" offset="0"/><input semantic="NORMAL" source="#cube-nrm" offset="1"/><vcount>4 4 4 4 4 4</vcount><p>{}</p></polylist>"##,
            p.join(" ")
        )
    }

    fn triangles() -> String {
        let p: Vec<String> = QUADS.iter().flat_map(|q| [q[0], q[1], q[2], q[0], q[2], q[3]]).map(|v| v.to_string()).collect();
        format!(r##"<triangles material="m" count="12"><input semantic="VERTEX" source="#cube-vtx" offset="0"/><p>{}</p></triangles>"##, p.join(" "))
    }

    fn polygons() -> String {
        let ps: Vec<String> = QUADS.iter().map(|q| format!("<p>{} {} {} {}</p>", q[0], q[1], q[2], q[3])).collect();
        format!(r##"<polygons material="m" count="6"><input semantic="VERTEX" source="#cube-vtx" offset="0"/>{}</polygons>"##, ps.concat())
    }

    fn read(s: &str) -> Model {
        parse(s.as_bytes(), &none).unwrap()
    }

    fn close(a: V3, b: V3) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() < 1e-4)
    }

    /// Every triangle faces away from the model's centre.
    fn outward(m: &Model) -> bool {
        let (lo, hi) = m.bounds().unwrap();
        let c = math::scale(math::add(lo, hi), 0.5);
        m.meshes.iter().all(|mesh| {
            mesh.indices.as_chunks::<3>().0.iter().all(|t| {
                let [a, b, d] = t.map(|i| mesh.positions[i as usize]);
                let n = super::super::geom::face_normal(a, b, d);
                math::dot(n, math::sub(math::scale(math::add(math::add(a, b), d), 1.0 / 3.0), c)) > 0.0
            })
        })
    }

    #[test]
    fn polylist_cube_with_normals_and_colour() {
        let m = read(&doc("", &polylist(), BIND_RED, ""));
        assert_eq!(m.meshes.len(), 1);
        assert_eq!(m.triangle_count(), 12);
        assert_eq!(m.bounds(), Some(([-1.0; 3], [1.0; 3])));
        let mesh = &m.meshes[0];
        assert_eq!(mesh.name, "CubeNode");
        let mat = mesh.material.as_ref().unwrap();
        assert_eq!((mat.name.as_str(), mat.color), ("Red", [1.0, 0.0, 0.0]));
        // One vertex per (corner, face normal) pair: 24.
        assert_eq!(mesh.positions.len(), 24);
        let normals = mesh.normals.as_ref().unwrap();
        assert!(normals.iter().all(|n| NORMALS.iter().any(|&f| close(*n, f))));
        assert!(outward(&m));
    }

    #[test]
    fn triangles_and_polygons_read_the_same_cube() {
        for prim in [triangles(), polygons()] {
            let m = read(&doc("", &prim, BIND_RED, ""));
            assert_eq!(m.triangle_count(), 12);
            assert_eq!(m.vertex_count(), 8);
            assert!(m.meshes[0].normals.is_none());
            assert_eq!(m.meshes[0].material.as_ref().unwrap().color, [1.0, 0.0, 0.0]);
            assert!(outward(&m));
        }
    }

    #[test]
    fn strips_and_fans() {
        let strip = r##"<tristrips material="m" count="1"><input semantic="VERTEX" source="#cube-vtx" offset="0"/><p>0 1 3 2</p></tristrips>"##;
        let fan = r##"<trifans material="m" count="1"><input semantic="VERTEX" source="#cube-vtx" offset="0"/><p>0 3 2 1</p></trifans>"##;
        for prim in [strip, fan] {
            let m = read(&doc("", prim, BIND_RED, ""));
            assert_eq!(m.triangle_count(), 2);
            let mesh = &m.meshes[0];
            for t in mesh.indices.as_chunks::<3>().0 {
                let [a, b, c] = t.map(|i| mesh.positions[i as usize]);
                let n = super::super::geom::face_normal(a, b, c);
                assert_eq!(n[2].signum(), if prim == strip { 1.0 } else { -1.0 }, "{prim}");
            }
        }
    }

    #[test]
    fn node_transforms_compose() {
        // translate then scale: points are scaled first, then moved.
        let node = format!("<translate>10 0 0</translate><scale>2 2 2</scale>{BIND_RED}");
        let m = read(&doc("", &triangles(), &node, ""));
        assert_eq!(m.bounds(), Some(([8.0, -2.0, -2.0], [12.0, 2.0, 2.0])));
        // The same as a row-major matrix.
        let node = format!("<matrix>2 0 0 10 0 2 0 0 0 0 2 0 0 0 0 1</matrix>{BIND_RED}");
        assert_eq!(read(&doc("", &triangles(), &node, "")).bounds(), Some(([8.0, -2.0, -2.0], [12.0, 2.0, 2.0])));
        // Rotating 90° about Z carries a stretched X onto Y.
        let node = format!("<rotate>0 0 1 90</rotate><scale>3 1 1</scale>{BIND_RED}");
        let (lo, hi) = read(&doc("", &triangles(), &node, "")).bounds().unwrap();
        assert!(close(lo, [-1.0, -3.0, -1.0]) && close(hi, [1.0, 3.0, 1.0]), "{lo:?} {hi:?}");
        // Normals turn with the node.
        let node = format!("<rotate>0 0 1 90</rotate>{BIND_RED}");
        let m = read(&doc("", &polylist(), &node, ""));
        assert!(outward(&m));
        let mesh = &m.meshes[0];
        for t in mesh.indices.as_chunks::<3>().0 {
            let [a, b, c] = t.map(|i| mesh.positions[i as usize]);
            let f = super::super::geom::face_normal(a, b, c);
            assert!(t.iter().all(|&i| close(mesh.normals.as_ref().unwrap()[i as usize], f)));
        }
    }

    #[test]
    fn nested_and_instanced_nodes() {
        let lib = format!(r##"<library_nodes><node id="part"><translate>0 5 0</translate>{BIND_RED}</node></library_nodes>"##);
        let node = r##"<translate>10 0 0</translate><node><instance_node url="#part"/></node><instance_node url="#part"/>"##;
        let m = read(&doc("", &triangles(), node, &lib));
        assert_eq!(m.meshes.len(), 2);
        assert_eq!(m.bounds(), Some(([9.0, 4.0, -1.0], [11.0, 6.0, 1.0])));
    }

    #[test]
    fn mirrored_instances_keep_faces_outward() {
        let node = format!("<scale>-1 1 1</scale>{BIND_RED}");
        let m = read(&doc("", &polylist(), &node, ""));
        assert!(outward(&m));
        assert!(m.meshes[0].normals.as_ref().unwrap().iter().all(|n| NORMALS.iter().any(|&f| close(*n, f))));
    }

    #[test]
    fn z_up_and_units_become_y_up_metres() {
        let node = format!("<translate>0 0 10</translate>{BIND_RED}");
        let asset = r#"<unit name="centimeter" meter="0.01"/><up_axis>Z_UP</up_axis>"#;
        let (lo, hi) = read(&doc(asset, &triangles(), &node, "")).bounds().unwrap();
        // File z (up) 9..11 cm becomes y 0.09..0.11 m.
        assert!(close(lo, [-0.01, 0.09, -0.01]) && close(hi, [0.01, 0.11, 0.01]), "{lo:?} {hi:?}");
        let (lo, hi) = read(&doc("<up_axis>X_UP</up_axis>", &triangles(), &format!("<translate>10 0 0</translate>{BIND_RED}"), "")).bounds().unwrap();
        assert!(close(lo, [-1.0, 9.0, -1.0]) && close(hi, [1.0, 11.0, 1.0]), "{lo:?} {hi:?}");
        // Converting keeps faces outward and normals unit length.
        let m = read(&doc(asset, &polylist(), BIND_RED, ""));
        assert!(outward(&m));
        assert!(m.meshes[0].normals.as_ref().unwrap().iter().all(|n| (math::length(*n) - 1.0).abs() < 1e-4));
    }

    #[test]
    fn diffuse_texture_gives_its_average_colour() {
        let mut png = Vec::new();
        image::RgbImage::from_pixel(4, 4, image::Rgb([0, 255, 0])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let side = |name: &str| (name == "textures/green tex.png").then(|| png.clone());
        let node = r##"<instance_geometry url="#cube"><bind_material><technique_common><instance_material symbol="m" target="#tex-mat"/></technique_common></bind_material></instance_geometry>"##;
        let m = parse(doc("", &triangles(), node, "").as_bytes(), &side).unwrap();
        let mat = m.meshes[0].material.as_ref().unwrap();
        assert_eq!(mat.name, "Tex");
        assert!(close(mat.color, [0.0, 1.0, 0.0]));
        // Without the image the default colour stands in.
        let m = read(&doc("", &triangles(), node, ""));
        assert_eq!(m.meshes[0].material.as_ref().unwrap().color, DEFAULT_COLOR);
        // 1.5's instance_image and init_from/ref, found by file name alone.
        let side = |name: &str| (name == "g.png").then(|| png.clone());
        let extra = r##"<library_images><image id="img15"><init_from><ref>file:///C:/work/g.png</ref></init_from></image></library_images>
            <library_effects><effect id="fx15"><profile_COMMON><newparam sid="s"><sampler2D><instance_image url="#img15"/></sampler2D></newparam>
            <technique sid="t"><blinn><diffuse><texture texture="s" texcoord="UV"/></diffuse></blinn></technique></profile_COMMON></effect></library_effects>
            <library_materials><material id="m15"><instance_effect url="#fx15"/></material></library_materials>"##;
        let node = r##"<instance_geometry url="#cube"><bind_material><technique_common><instance_material symbol="m" target="#m15"/></technique_common></bind_material></instance_geometry>"##;
        let m = parse(doc("", &triangles(), node, extra).as_bytes(), &side).unwrap();
        assert!(close(m.meshes[0].material.as_ref().unwrap().color, [0.0, 1.0, 0.0]));
    }

    #[test]
    fn controllers_show_the_bind_pose() {
        let extra = r##"<library_controllers><controller id="skin"><skin source="#cube">
            <bind_shape_matrix>1 0 0 0 0 1 0 3 0 0 1 0 0 0 0 1</bind_shape_matrix></skin></controller></library_controllers>"##;
        let node = r##"<instance_controller url="#skin"><bind_material><technique_common><instance_material symbol="m" target="#red-mat"/></technique_common></bind_material></instance_controller>"##;
        let m = read(&doc("", &triangles(), node, extra));
        assert_eq!(m.bounds(), Some(([-1.0, 2.0, -1.0], [1.0, 4.0, 1.0])));
        assert_eq!(m.meshes[0].material.as_ref().unwrap().color, [1.0, 0.0, 0.0]);
    }

    #[test]
    fn without_a_scene_every_geometry_is_shown() {
        let s = doc("", &triangles(), BIND_RED, "");
        let start = s.find("<library_visual_scenes>").unwrap();
        let end = s.find("</COLLADA>").unwrap();
        let s = format!("{}{}", &s[..start], &s[end..]);
        let m = read(&s);
        assert_eq!(m.triangle_count(), 12);
        // The primitive's symbol names no material, so none is applied.
        assert!(m.meshes[0].material.is_none());
    }

    #[test]
    fn bad_indices_and_accessors_are_dropped() {
        // An index past the positions drops its triangle only.
        let prim = r##"<triangles material="m" count="2"><input semantic="VERTEX" source="#cube-vtx" offset="0"/><p>0 1 2 0 1 99 0 -1 2</p></triangles>"##;
        assert_eq!(read(&doc("", prim, BIND_RED, "")).triangle_count(), 1);
        // A vcount longer than the indices stops at the end.
        let prim = r##"<polylist material="m"><input semantic="VERTEX" source="#cube-vtx" offset="0"/><vcount>3 4000000000 3</vcount><p>0 1 2 0 2 3</p></polylist>"##;
        assert_eq!(read(&doc("", prim, BIND_RED, "")).triangle_count(), 1);
        // A huge accessor count and a zero stride do not stretch the source.
        let s = doc("", &triangles(), BIND_RED, "").replace(r#"count="8" stride="3""#, r#"count="4000000000" stride="0""#);
        assert!(parse(s.as_bytes(), &none).is_ok_and(|m| m.vertex_count() <= 24));
        // A huge offset is ignored.
        let prim = r##"<triangles material="m"><input semantic="VERTEX" source="#cube-vtx" offset="0"/><input semantic="NORMAL" source="#cube-nrm" offset="4000000000"/><p>0 1 2</p></triangles>"##;
        assert_eq!(read(&doc("", prim, BIND_RED, "")).triangle_count(), 1);
    }

    #[test]
    fn instance_cycles_end() {
        let lib = format!(r##"<library_nodes><node id="a">{BIND_RED}<instance_node url="#b"/><instance_node url="#b"/></node><node id="b"><instance_node url="#a"/><instance_node url="#a"/></node></library_nodes>"##);
        let m = read(&doc("", &triangles(), r##"<instance_node url="#a"/>"##, &lib));
        assert!(m.meshes.len() <= MAX_VISITS);
    }

    #[test]
    fn import_thumbnail_draws_the_red_cube() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Cube.DAE");
        std::fs::write(&path, doc("<up_axis>Z_UP</up_axis>", &polylist(), BIND_RED, "")).unwrap();
        let img = super::super::raster::thumbnail(&path, "DAE").unwrap();
        assert!(img.pixels().any(|p| p[3] == 255 && p[0] > 100 && p[1] < 40 && p[2] < 40));
    }

    #[test]
    fn truncated_and_corrupt_files_fail_cleanly() {
        let full = doc("", &polylist(), BIND_RED, "");
        let b = full.as_bytes();
        for n in 0..b.len() {
            let _ = parse(&b[..n], &none);
        }
        // Damage one byte at a time across the geometry and scene.
        let start = full.find("<library_geometries>").unwrap();
        for i in (start..b.len()).step_by(3) {
            let mut d = b.to_vec();
            d[i] = b"<>\"/0 9x"[i % 8];
            let _ = parse(&d, &none);
        }
        assert!(parse(b"", &none).is_err());
        assert!(parse(b"<html/>", &none).is_err());
        assert!(parse(&[0xff; 64], &none).is_err());
        let empty = r#"<COLLADA version="1.5.0"><library_geometries/></COLLADA>"#;
        assert!(parse(empty.as_bytes(), &none).is_err());
    }
}
