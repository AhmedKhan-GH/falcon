//! glTF 2.0: binary `.glb` and JSON `.gltf` with its buffers.
//!
//! Only geometry and the base colour are read (no textures, animation,
//! skins or sparse accessors). The default scene is walked, node transforms
//! are baked into positions and normals, and every primitive of every mesh
//! instance becomes one [`Mesh`].

use std::collections::HashMap;

use base64::Engine;
use serde::Deserialize;

use super::math::{self, M4, V3};
use super::{Material, Mesh, Model, ParseError, Result};

const GLB_MAGIC: u32 = 0x4654_6C67;
const CHUNK_JSON: u32 = 0x4E4F_534A;
const CHUNK_BIN: u32 = 0x004E_4942;

/// Deepest node nesting followed; deeper (or cyclic) branches are dropped.
const MAX_DEPTH: usize = 64;
/// Most mesh instances and output vertices, against files that instance a
/// big mesh exponentially often.
const MAX_INSTANCES: usize = 100_000;
const MAX_VERTICES: usize = 50_000_000;

type Side<'a> = &'a dyn Fn(&str) -> Option<Vec<u8>>;

#[derive(Deserialize, Default)]
#[serde(default)]
struct Doc {
    buffers: Vec<BufferDef>,
    #[serde(rename = "bufferViews")]
    buffer_views: Vec<ViewDef>,
    accessors: Vec<AccessorDef>,
    meshes: Vec<MeshDef>,
    materials: Vec<MaterialDef>,
    nodes: Vec<NodeDef>,
    scenes: Vec<SceneDef>,
    scene: Option<usize>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct BufferDef {
    uri: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ViewDef {
    buffer: usize,
    #[serde(rename = "byteOffset")]
    byte_offset: usize,
    #[serde(rename = "byteLength")]
    byte_length: usize,
    #[serde(rename = "byteStride")]
    byte_stride: Option<usize>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct AccessorDef {
    #[serde(rename = "bufferView")]
    buffer_view: Option<usize>,
    #[serde(rename = "byteOffset")]
    byte_offset: usize,
    #[serde(rename = "componentType")]
    component_type: u32,
    normalized: bool,
    count: usize,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct MeshDef {
    name: String,
    primitives: Vec<PrimDef>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct PrimDef {
    attributes: HashMap<String, usize>,
    indices: Option<usize>,
    material: Option<usize>,
    mode: Option<u32>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct MaterialDef {
    name: String,
    #[serde(rename = "pbrMetallicRoughness")]
    pbr: PbrDef,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct PbrDef {
    #[serde(rename = "baseColorFactor")]
    base_color_factor: Option<[f32; 4]>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct NodeDef {
    mesh: Option<usize>,
    children: Vec<usize>,
    matrix: Option<Vec<f32>>,
    translation: Option<[f32; 3]>,
    rotation: Option<[f32; 4]>,
    scale: Option<[f32; 3]>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SceneDef {
    nodes: Vec<usize>,
}

/// Reads a `.glb` file.
pub fn parse_glb(bytes: &[u8], side: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Model> {
    let u32_at = |o: usize| -> Result<u32> {
        let b = bytes.get(o..o + 4).ok_or_else(|| ParseError::new("truncated GLB"))?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    if bytes.len() < 12 || u32_at(0)? != GLB_MAGIC {
        return Err(ParseError::new("not a GLB file"));
    }
    let version = u32_at(4)?;
    if version != 2 {
        return Err(ParseError(format!("unsupported GLB version {version}")));
    }
    let total = u32_at(8)? as usize;
    if total < 12 || total > bytes.len() {
        return Err(ParseError::new("GLB length does not match the file"));
    }
    let mut json: Option<&[u8]> = None;
    let mut bin: Option<&[u8]> = None;
    let mut pos = 12;
    let mut first = true;
    while pos + 8 <= total {
        let len = u32_at(pos)? as usize;
        let kind = u32_at(pos + 4)?;
        let start = pos + 8;
        let end = start.checked_add(len).filter(|&e| e <= total).ok_or_else(|| ParseError::new("GLB chunk runs past the file"))?;
        if first && kind != CHUNK_JSON {
            return Err(ParseError::new("GLB does not start with a JSON chunk"));
        }
        first = false;
        match kind {
            CHUNK_JSON if json.is_none() => json = Some(&bytes[start..end]),
            CHUNK_BIN if bin.is_none() => bin = Some(&bytes[start..end]),
            _ => {}
        }
        // Chunks are 4-byte aligned.
        pos = end.next_multiple_of(4);
    }
    let json = json.ok_or_else(|| ParseError::new("GLB has no JSON chunk"))?;
    build(json, bin, side)
}

/// Reads a `.gltf` file; its buffers are data URIs or fetched with `side`.
pub fn parse_gltf(bytes: &[u8], side: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Model> {
    build(bytes, None, side)
}

fn build(json: &[u8], bin: Option<&[u8]>, side: Side) -> Result<Model> {
    let doc: Doc = serde_json::from_slice(json).map_err(|e| ParseError(format!("bad glTF JSON: {e}")))?;
    let mut buffers = Vec::with_capacity(doc.buffers.len());
    for (i, b) in doc.buffers.iter().enumerate() {
        let data = match &b.uri {
            None if i == 0 => bin.map(<[u8]>::to_vec),
            None => None,
            Some(uri) => load_uri(uri, side),
        };
        buffers.push(data.ok_or_else(|| ParseError(format!("glTF buffer {i} is missing")))?);
    }

    let mut out = Vec::new();
    let mut cache: HashMap<(usize, usize), Option<Prim>> = HashMap::new();
    let mut walker = Walker { doc: &doc, buffers: &buffers, cache: &mut cache, out: &mut out, instances: 0, vertices: 0 };
    let identity = identity();
    for root in roots(&doc) {
        walker.visit(root, &identity, 0);
    }
    if doc.nodes.is_empty() {
        for m in 0..doc.meshes.len() {
            walker.emit(m, &identity);
        }
    }
    Model { meshes: out }.finish()
}

fn identity() -> M4 {
    [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]
}

/// The nodes to start from: the default scene, else the first scene, else
/// every node that is nobody's child.
fn roots(doc: &Doc) -> Vec<usize> {
    if let Some(s) = doc.scene.and_then(|i| doc.scenes.get(i)).or(doc.scenes.first()) {
        return s.nodes.clone();
    }
    let mut child = vec![false; doc.nodes.len()];
    for n in &doc.nodes {
        for &c in &n.children {
            if let Some(f) = child.get_mut(c) {
                *f = true;
            }
        }
    }
    (0..doc.nodes.len()).filter(|&i| !child[i]).collect()
}

fn load_uri(uri: &str, side: Side) -> Option<Vec<u8>> {
    if let Some(rest) = uri.strip_prefix("data:") {
        let (head, data) = rest.split_once(',')?;
        return if head.split(';').any(|p| p.eq_ignore_ascii_case("base64")) {
            let clean: String = data.chars().filter(|c| !c.is_whitespace()).collect();
            base64::engine::general_purpose::STANDARD.decode(clean.trim_end_matches('=')).or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(clean.trim_end_matches('='))).ok()
        } else {
            Some(percent_decode(data))
        };
    }
    side(&String::from_utf8_lossy(&percent_decode(uri)))
}

fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push(h * 16 + l);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

fn hex(c: u8) -> Option<u8> {
    (c as char).to_digit(16).map(|d| d as u8)
}

/// Linear to sRGB for one channel.
fn srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// Reads accessor `idx` as `count * components` numbers (integers widened,
/// normalised ones mapped to 0..1 / -1..1). Only scalar and vector types.
fn read_accessor(doc: &Doc, buffers: &[Vec<u8>], idx: usize) -> Result<(Vec<f64>, usize)> {
    let bad = |m: &str| ParseError(format!("glTF accessor {idx}: {m}"));
    let acc = doc.accessors.get(idx).ok_or_else(|| bad("does not exist"))?;
    let ncomp = match acc.kind.as_str() {
        "SCALAR" => 1,
        "VEC2" => 2,
        "VEC3" => 3,
        "VEC4" => 4,
        _ => return Err(bad("unsupported type")),
    };
    let csize = match acc.component_type {
        5120 | 5121 => 1,
        5122 | 5123 => 2,
        5125 | 5126 => 4,
        _ => return Err(bad("unsupported component type")),
    };
    let esize = ncomp * csize;
    let total = acc.count.checked_mul(ncomp).filter(|&t| t <= 1 << 28).ok_or_else(|| bad("too many elements"))?;
    let Some(vi) = acc.buffer_view else {
        return Ok((vec![0.0; total], ncomp));
    };
    let view = doc.buffer_views.get(vi).ok_or_else(|| bad("buffer view does not exist"))?;
    let data = buffers.get(view.buffer).ok_or_else(|| bad("buffer does not exist"))?;
    let stride = view.byte_stride.filter(|&s| s > 0).unwrap_or(esize);
    let view_end = view.byte_offset.checked_add(view.byte_length).ok_or_else(|| bad("view overflows"))?.min(data.len());
    let start = view.byte_offset.checked_add(acc.byte_offset).ok_or_else(|| bad("offset overflows"))?;
    if acc.count > 0 {
        let last = (acc.count - 1).checked_mul(stride).and_then(|o| o.checked_add(start)).and_then(|o| o.checked_add(esize));
        if last.is_none_or(|l| l > view_end) {
            return Err(bad("reads past its buffer"));
        }
    }
    let mut out = Vec::with_capacity(total);
    for i in 0..acc.count {
        let base = start + i * stride;
        for c in 0..ncomp {
            let o = base + c * csize;
            out.push(component(&data[o..o + csize], acc.component_type, acc.normalized));
        }
    }
    Ok((out, ncomp))
}

fn component(b: &[u8], ct: u32, norm: bool) -> f64 {
    match ct {
        5120 => {
            let v = f64::from(b[0] as i8);
            if norm { (v / 127.0).max(-1.0) } else { v }
        }
        5121 => {
            let v = f64::from(b[0]);
            if norm { v / 255.0 } else { v }
        }
        5122 => {
            let v = f64::from(i16::from_le_bytes([b[0], b[1]]));
            if norm { (v / 32767.0).max(-1.0) } else { v }
        }
        5123 => {
            let v = f64::from(u16::from_le_bytes([b[0], b[1]]));
            if norm { v / 65535.0 } else { v }
        }
        5125 => {
            let v = f64::from(u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
            if norm { v / 4_294_967_295.0 } else { v }
        }
        _ => f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
    }
}

/// A primitive's geometry in its own node space, as triangles.
struct Prim {
    positions: Vec<V3>,
    normals: Option<Vec<V3>>,
    colors: Option<Vec<V3>>,
    indices: Vec<u32>,
    material: Option<Material>,
}

fn read_prim(doc: &Doc, buffers: &[Vec<u8>], p: &PrimDef) -> Result<Option<Prim>> {
    let mode = p.mode.unwrap_or(4);
    if !matches!(mode, 4..=6) {
        return Ok(None);
    }
    let Some(&pos_idx) = p.attributes.get("POSITION") else {
        return Ok(None);
    };
    let (pos, nc) = read_accessor(doc, buffers, pos_idx)?;
    if nc != 3 {
        return Err(ParseError::new("glTF POSITION is not a VEC3"));
    }
    let positions: Vec<V3> = pos.as_chunks::<3>().0.iter().map(|c| c.map(|v| v as f32)).collect();
    let n = positions.len();

    // Optional attributes that are broken or do not match are left out.
    let normals = p.attributes.get("NORMAL").and_then(|&i| read_accessor(doc, buffers, i).ok()).filter(|(v, nc)| *nc == 3 && v.len() == n * 3).map(|(v, _)| v.as_chunks::<3>().0.iter().map(|c| c.map(|x| x as f32)).collect::<Vec<V3>>());
    let colors = p.attributes.get("COLOR_0").and_then(|&i| read_accessor(doc, buffers, i).ok()).filter(|(v, nc)| (*nc == 3 || *nc == 4) && v.len() == n * *nc).map(|(v, nc)| v.chunks(nc).map(|c| [srgb(c[0] as f32), srgb(c[1] as f32), srgb(c[2] as f32)]).collect::<Vec<V3>>());

    let seq: Vec<u32> = match p.indices {
        Some(i) => {
            let (v, nc) = read_accessor(doc, buffers, i)?;
            if nc != 1 {
                return Err(ParseError::new("glTF indices are not scalar"));
            }
            v.into_iter().map(|x| x as u32).collect()
        }
        None => (0..n as u32).collect(),
    };
    let indices = match mode {
        4 => seq[..seq.len() / 3 * 3].to_vec(),
        5 => (0..seq.len().saturating_sub(2)).flat_map(|i| if i % 2 == 0 { [seq[i], seq[i + 1], seq[i + 2]] } else { [seq[i + 1], seq[i], seq[i + 2]] }).collect(),
        _ => (1..seq.len().saturating_sub(1)).flat_map(|i| [seq[0], seq[i], seq[i + 1]]).collect(),
    };

    let material = p.material.and_then(|m| doc.materials.get(m)).map(|m| {
        let f = m.pbr.base_color_factor.unwrap_or([1.0; 4]);
        Material { name: m.name.clone(), color: [srgb(f[0]), srgb(f[1]), srgb(f[2])], opacity: f[3] }
    });
    Ok(Some(Prim { positions, normals, colors, indices, material }))
}

struct Walker<'a> {
    doc: &'a Doc,
    buffers: &'a [Vec<u8>],
    cache: &'a mut HashMap<(usize, usize), Option<Prim>>,
    out: &'a mut Vec<Mesh>,
    instances: usize,
    vertices: usize,
}

impl Walker<'_> {
    fn visit(&mut self, node: usize, parent: &M4, depth: usize) {
        let Some(n) = self.doc.nodes.get(node) else { return };
        if depth >= MAX_DEPTH {
            return;
        }
        let world = math::mul(parent, &local_matrix(n));
        if let Some(m) = n.mesh {
            self.emit(m, &world);
        }
        for &c in &n.children {
            self.visit(c, &world, depth + 1);
        }
    }

    fn emit(&mut self, mesh: usize, world: &M4) {
        let Some(def) = self.doc.meshes.get(mesh) else { return };
        for (pi, pdef) in def.primitives.iter().enumerate() {
            if self.instances >= MAX_INSTANCES || self.vertices >= MAX_VERTICES {
                return;
            }
            let (doc, buffers) = (self.doc, self.buffers);
            let prim = self.cache.entry((mesh, pi)).or_insert_with(|| read_prim(doc, buffers, pdef).ok().flatten());
            let Some(prim) = prim else { continue };
            self.instances += 1;
            self.vertices += prim.positions.len();
            self.out.push(instance(&def.name, prim, world));
        }
    }
}

/// `prim` placed by `world`: positions through the matrix, normals through
/// its inverse transpose (renormalised), winding flipped for a mirroring
/// matrix.
fn instance(name: &str, prim: &Prim, world: &M4) -> Mesh {
    let positions = prim.positions.iter().map(|&p| math::transform_point(world, p)).collect();
    let (cof, det) = cofactor(world);
    let normals = prim.normals.as_ref().map(|ns| {
        ns.iter()
            .map(|&n| {
                let v = [0, 1, 2].map(|r| (0..3).map(|c| cof[r][c] * n[c]).sum::<f32>());
                math::normalize(if det < 0.0 { math::scale(v, -1.0) } else { v })
            })
            .collect()
    });
    let mut indices = prim.indices.clone();
    if det < 0.0 {
        for t in indices.as_chunks_mut::<3>().0 {
            t.swap(1, 2);
        }
    }
    Mesh { name: name.to_string(), positions, normals, colors: prim.colors.clone(), indices, material: prim.material.clone(), ..Mesh::default() }
}

/// The cofactor matrix of the upper 3x3 of `m` (`[row][col]`, which is the
/// inverse transpose up to the determinant) and the determinant. A
/// singular or non-finite matrix gives the matrix itself and a positive
/// determinant.
fn cofactor(m: &M4) -> ([[f32; 3]; 3], f32) {
    let at = |r: usize, c: usize| m[c % 3][r % 3];
    let mut cof = [[0.0f32; 3]; 3];
    for (r, row) in cof.iter_mut().enumerate() {
        for (c, cell) in row.iter_mut().enumerate() {
            *cell = at(r + 1, c + 1) * at(r + 2, c + 2) - at(r + 1, c + 2) * at(r + 2, c + 1);
        }
    }
    let det = (0..3).map(|c| at(0, c) * cof[0][c]).sum::<f32>();
    if det == 0.0 || !det.is_finite() {
        let mut plain = [[0.0f32; 3]; 3];
        for (r, row) in plain.iter_mut().enumerate() {
            for (c, cell) in row.iter_mut().enumerate() {
                *cell = if m[c][r].is_finite() { m[c][r] } else { 0.0 };
            }
        }
        return (plain, 1.0);
    }
    (cof, det)
}

/// A node's own transform: its `matrix`, or translation * rotation * scale.
fn local_matrix(n: &NodeDef) -> M4 {
    if let Some(v) = &n.matrix
        && v.len() == 16
    {
        let mut m = [[0.0; 4]; 4];
        for (c, col) in m.iter_mut().enumerate() {
            col.copy_from_slice(&v[c * 4..c * 4 + 4]);
        }
        return m;
    }
    let t = n.translation.unwrap_or([0.0; 3]);
    let s = n.scale.unwrap_or([1.0; 3]);
    let [mut x, mut y, mut z, mut w] = n.rotation.unwrap_or([0.0, 0.0, 0.0, 1.0]);
    let len = (x * x + y * y + z * z + w * w).sqrt();
    if len > 0.0 && len.is_finite() {
        (x, y, z, w) = (x / len, y / len, z / len, w / len);
    } else {
        (x, y, z, w) = (0.0, 0.0, 0.0, 1.0);
    }
    let c0 = [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + z * w), 2.0 * (x * z - y * w)];
    let c1 = [2.0 * (x * y - z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + x * w)];
    let c2 = [2.0 * (x * z + y * w), 2.0 * (y * z - x * w), 1.0 - 2.0 * (x * x + y * y)];
    [
        [c0[0] * s[0], c0[1] * s[0], c0[2] * s[0], 0.0],
        [c1[0] * s[1], c1[1] * s[1], c1[2] * s[1], 0.0],
        [c2[0] * s[2], c2[1] * s[2], c2[2] * s[2], 0.0],
        [t[0], t[1], t[2], 1.0],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_side(_: &str) -> Option<Vec<u8>> {
        None
    }

    /// Wraps `json` and `bin` in a GLB container.
    fn make_glb(json: &str, bin: &[u8]) -> Vec<u8> {
        let mut j = json.as_bytes().to_vec();
        while !j.len().is_multiple_of(4) {
            j.push(b' ');
        }
        let mut b = bin.to_vec();
        while !b.len().is_multiple_of(4) {
            b.push(0);
        }
        let total = 12 + 8 + j.len() + if b.is_empty() { 0 } else { 8 + b.len() };
        let mut out = Vec::new();
        out.extend_from_slice(&GLB_MAGIC.to_le_bytes());
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(j.len() as u32).to_le_bytes());
        out.extend_from_slice(&CHUNK_JSON.to_le_bytes());
        out.extend_from_slice(&j);
        if !b.is_empty() {
            out.extend_from_slice(&(b.len() as u32).to_le_bytes());
            out.extend_from_slice(&CHUNK_BIN.to_le_bytes());
            out.extend_from_slice(&b);
        }
        out
    }

    /// Positions (0,0,0) (1,0,0) (0,1,0) then u16 indices 0 1 2, padded.
    fn tri_bin() -> Vec<u8> {
        let mut b = Vec::new();
        for v in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for i in [0u16, 1, 2, 0] {
            b.extend_from_slice(&i.to_le_bytes());
        }
        b
    }

    /// JSON over `tri_bin`: `@B` is the buffer entry body, `@P` extra
    /// primitive members, `@X` extra top-level members.
    fn tri_json(buffer: &str, prim: &str, extra: &str) -> String {
        r#"{"asset":{"version":"2.0"},"buffers":[{"byteLength":44@B}],
        "bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36},{"buffer":0,"byteOffset":36,"byteLength":6}],
        "accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"},{"bufferView":1,"componentType":5123,"count":3,"type":"SCALAR"}],
        "meshes":[{"name":"tri","primitives":[{"attributes":{"POSITION":0},"indices":1@P}]}],
        "materials":[{"name":"m","pbrMetallicRoughness":{"baseColorFactor":[0.5,0.5,0.5,0.5]}}]@X}"#
            .replace("@B", buffer)
            .replace("@P", prim)
            .replace("@X", extra)
    }

    fn close(a: V3, b: V3) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-4)
    }

    #[test]
    fn single_triangle_glb() {
        let glb = make_glb(&tri_json("", "", ""), &tri_bin());
        let m = parse_glb(&glb, &no_side).unwrap();
        assert_eq!(m.meshes.len(), 1);
        let mesh = &m.meshes[0];
        assert_eq!(mesh.name, "tri");
        assert_eq!(mesh.indices, vec![0, 1, 2]);
        assert_eq!(mesh.positions[1], [1.0, 0.0, 0.0]);
        assert!(mesh.material.is_none());
    }

    #[test]
    fn node_translation_is_baked() {
        let extra = r#","nodes":[{"mesh":0,"translation":[1,2,3]}],"scene":0,"scenes":[{"nodes":[0]}]"#;
        let glb = make_glb(&tri_json("", "", extra), &tri_bin());
        let m = parse_glb(&glb, &no_side).unwrap();
        assert!(close(m.meshes[0].positions[0], [1.0, 2.0, 3.0]));
        assert!(close(m.meshes[0].positions[1], [2.0, 2.0, 3.0]));
    }

    #[test]
    fn parent_and_child_compose() {
        let extra = r#","nodes":[{"children":[1],"translation":[1,0,0],"scale":[2,2,2]},{"mesh":0,"translation":[0,1,0]}]"#;
        let glb = make_glb(&tri_json("", "", extra), &tri_bin());
        let m = parse_glb(&glb, &no_side).unwrap();
        // Roots are derived: node 1 is a child, so only node 0 starts a walk.
        assert_eq!(m.meshes.len(), 1);
        assert!(close(m.meshes[0].positions[1], [3.0, 2.0, 0.0]));
    }

    #[test]
    fn rotation_turns_positions_and_normals() {
        let mut bin = tri_bin();
        // Normals (0,0,1) x3 after the indices (offset 44).
        for v in [0.0f32, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0] {
            bin.extend_from_slice(&v.to_le_bytes());
        }
        let json = r#"{"buffers":[{"byteLength":80}],
        "bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36},{"buffer":0,"byteOffset":36,"byteLength":6},{"buffer":0,"byteOffset":44,"byteLength":36}],
        "accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"},{"bufferView":1,"componentType":5123,"count":3,"type":"SCALAR"},{"bufferView":2,"componentType":5126,"count":3,"type":"VEC3"}],
        "meshes":[{"primitives":[{"attributes":{"POSITION":0,"NORMAL":2},"indices":1}]}],
        "nodes":[{"mesh":0,"rotation":[0,0,0.70710678,0.70710678],"scale":[1,1,3]}]}"#;
        let m = parse_glb(&make_glb(json, &bin), &no_side).unwrap();
        let mesh = &m.meshes[0];
        // 90 degrees about Z: (1,0,0) -> (0,1,0).
        assert!(close(mesh.positions[1], [0.0, 1.0, 0.0]));
        // The z stretch must not change a z normal's direction or length.
        assert!(close(mesh.normals.as_ref().unwrap()[0], [0.0, 0.0, 1.0]));
    }

    #[test]
    fn gltf_with_data_uri() {
        let b64 = base64::engine::general_purpose::STANDARD.encode(tri_bin());
        let buffer = format!(r#","uri":"data:application/octet-stream;base64,{b64}""#);
        let m = parse_gltf(tri_json(&buffer, "", "").as_bytes(), &no_side).unwrap();
        assert_eq!(m.triangle_count(), 1);
    }

    #[test]
    fn gltf_with_external_buffer() {
        let buffer = r#","uri":"my%20tri.bin""#;
        let bin = tri_bin();
        let side = |name: &str| (name == "my tri.bin").then(|| bin.clone());
        let m = parse_gltf(tri_json(buffer, "", "").as_bytes(), &side).unwrap();
        assert_eq!(m.triangle_count(), 1);
        // Not served: an error.
        assert!(parse_gltf(tri_json(buffer, "", "").as_bytes(), &no_side).is_err());
    }

    #[test]
    fn base_color_factor_becomes_srgb() {
        let glb = make_glb(&tri_json("", r#","material":0"#, ""), &tri_bin());
        let mat = parse_glb(&glb, &no_side).unwrap().meshes[0].material.clone().unwrap();
        assert_eq!(mat.name, "m");
        // Linear 0.5 is about 0.7354 in sRGB.
        assert!(close(mat.color, [0.7354; 3]));
        assert!((mat.opacity - 0.5).abs() < 1e-6);
        assert_eq!(srgb(0.0), 0.0);
        assert!((srgb(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn triangle_strip_and_fan_become_lists() {
        let mut bin = Vec::new();
        for i in 0..4 {
            for v in [i as f32, 0.0, 0.0] {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        let json = |mode: u32| {
            format!(
                r#"{{"buffers":[{{"byteLength":48}}],"bufferViews":[{{"buffer":0,"byteLength":48}}],
                "accessors":[{{"bufferView":0,"componentType":5126,"count":4,"type":"VEC3"}}],
                "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"mode":{mode}}}]}}]}}"#
            )
        };
        let strip = parse_glb(&make_glb(&json(5), &bin), &no_side).unwrap();
        assert_eq!(strip.meshes[0].indices, vec![0, 1, 2, 2, 1, 3]);
        let fan = parse_glb(&make_glb(&json(6), &bin), &no_side).unwrap();
        assert_eq!(fan.meshes[0].indices, vec![0, 1, 2, 0, 2, 3]);
        // Points are skipped, leaving nothing.
        assert!(parse_glb(&make_glb(&json(0), &bin), &no_side).is_err());
    }

    #[test]
    fn bad_magic_is_an_error() {
        let mut glb = make_glb(&tri_json("", "", ""), &tri_bin());
        glb[0] = b'X';
        assert!(parse_glb(&glb, &no_side).is_err());
        assert!(parse_glb(&glb[..10], &no_side).is_err());
        // A chunk longer than the file.
        let mut glb = make_glb(&tri_json("", "", ""), &tri_bin());
        glb[12] = 0xFF;
        glb[13] = 0xFF;
        assert!(parse_glb(&glb, &no_side).is_err());
    }

    #[test]
    fn accessor_past_the_buffer_is_an_error() {
        let json = tri_json("", "", "").replace(r#""count":3,"type":"VEC3""#, r#""count":1000,"type":"VEC3""#);
        assert!(parse_glb(&make_glb(&json, &tri_bin()), &no_side).is_err());
        // Short BIN chunk.
        assert!(parse_glb(&make_glb(&tri_json("", "", ""), &tri_bin()[..20]), &no_side).is_err());
    }

    #[test]
    fn mesh_used_by_two_nodes_appears_twice() {
        let extra = r#","nodes":[{"mesh":0},{"mesh":0,"translation":[5,0,0]}],"scenes":[{"nodes":[0,1]}]"#;
        let m = parse_glb(&make_glb(&tri_json("", "", extra), &tri_bin()), &no_side).unwrap();
        assert_eq!(m.meshes.len(), 2);
        assert!(close(m.meshes[1].positions[0], [5.0, 0.0, 0.0]));
    }

    #[test]
    fn cycles_do_not_hang() {
        let extra = r#","nodes":[{"mesh":0,"children":[1]},{"children":[0]}],"scenes":[{"nodes":[0]}]"#;
        let m = parse_glb(&make_glb(&tri_json("", "", extra), &tri_bin()), &no_side).unwrap();
        assert!(!m.meshes.is_empty());
    }

    #[test]
    fn vertex_colors_normalized_u8() {
        let mut bin = tri_bin();
        bin.extend_from_slice(&[255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0]);
        let json = r#"{"buffers":[{"byteLength":56}],
        "bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36},{"buffer":0,"byteOffset":36,"byteLength":6},{"buffer":0,"byteOffset":44,"byteLength":9}],
        "accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"},{"bufferView":1,"componentType":5123,"count":3,"type":"SCALAR"},{"bufferView":2,"componentType":5121,"normalized":true,"count":3,"type":"VEC3"}],
        "meshes":[{"primitives":[{"attributes":{"POSITION":0,"COLOR_0":2},"indices":1}]}]}"#;
        let m = parse_glb(&make_glb(json, &bin), &no_side).unwrap();
        let c = m.meshes[0].colors.as_ref().unwrap();
        assert!(close(c[0], [1.0, 0.0, 0.0]));
        assert!(close(c[2], [0.0, 0.0, 1.0]));
    }
}
