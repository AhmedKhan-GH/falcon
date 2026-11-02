//! Writing a model back out, for the viewer's Export: Wavefront OBJ (with
//! an MTL of the material colours), binary STL, glTF 2.0 (binary GLB, or a
//! `.gltf` with its buffer embedded), binary PLY and ASCII OFF. Only what
//! falcon holds is written: positions, normals where the file had them,
//! vertex colours, triangles and material colours (OFF keeps positions and
//! triangles only). 3DM, which Eagle's viewer also writes, is not.

use super::math::{self, V3};
use super::{Mesh, geom};

/// Formats the viewer exports to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Obj,
    Stl,
    /// glTF 2.0 in one binary file.
    Glb,
    /// glTF 2.0 as JSON with the buffer embedded as a data URI.
    Gltf,
    Ply,
    /// ASCII Object File Format: positions and triangles.
    Off,
}

impl Format {
    /// In the order of Eagle's viewer's Export menu where it has them.
    pub const ALL: [Format; 6] = [Format::Glb, Format::Gltf, Format::Off, Format::Obj, Format::Ply, Format::Stl];

    pub fn ext(self) -> &'static str {
        match self {
            Format::Obj => "obj",
            Format::Stl => "stl",
            Format::Glb => "glb",
            Format::Gltf => "gltf",
            Format::Ply => "ply",
            Format::Off => "off",
        }
    }
}

/// A name usable after `o`, `usemtl` and `newmtl`: no spaces, never empty.
fn token(name: &str, fallback: &str) -> String {
    let t: String = name.trim().chars().map(|c| if c.is_whitespace() { '_' } else { c }).collect();
    if t.is_empty() { fallback.to_string() } else { t }
}

/// The OBJ text of `meshes`, referring to `mtl_name` when any has a
/// material, and the MTL text (empty when there are no materials).
pub fn obj<'a>(meshes: impl IntoIterator<Item = &'a Mesh>, mtl_name: &str) -> (String, String) {
    use std::fmt::Write;
    let meshes: Vec<&Mesh> = meshes.into_iter().collect();
    let mut out = String::from("# Exported by Falcon\n");
    let mut mtl = String::new();
    let mut seen: Vec<String> = Vec::new();
    if meshes.iter().any(|m| m.material.is_some()) {
        let _ = writeln!(out, "mtllib {mtl_name}");
    }
    let (mut v_base, mut n_base) = (1usize, 1usize);
    for (k, m) in meshes.iter().enumerate() {
        let _ = writeln!(out, "o {}", token(&m.name, &format!("mesh{}", k + 1)));
        for p in &m.positions {
            let _ = writeln!(out, "v {} {} {}", p[0], p[1], p[2]);
        }
        let normals = m.normals.as_ref().filter(|n| n.len() == m.positions.len());
        if let Some(ns) = normals {
            for n in ns {
                let _ = writeln!(out, "vn {} {} {}", n[0], n[1], n[2]);
            }
        }
        if let Some(mat) = &m.material {
            let name = token(&mat.name, &format!("material{}", seen.len() + 1));
            if !seen.contains(&name) {
                let c = mat.color;
                let _ = writeln!(mtl, "newmtl {name}\nKd {} {} {}\nd {}\n", c[0], c[1], c[2], mat.opacity);
                seen.push(name.clone());
            }
            let _ = writeln!(out, "usemtl {name}");
        }
        for t in m.indices.as_chunks::<3>().0 {
            let [a, b, c] = t.map(|i| i as usize);
            if normals.is_some() {
                let _ = writeln!(out, "f {0}//{3} {1}//{4} {2}//{5}", a + v_base, b + v_base, c + v_base, a + n_base, b + n_base, c + n_base);
            } else {
                let _ = writeln!(out, "f {} {} {}", a + v_base, b + v_base, c + v_base);
            }
        }
        v_base += m.positions.len();
        if let Some(ns) = normals {
            n_base += ns.len();
        }
    }
    (out, mtl)
}

/// Binary STL of `meshes`: an 80-byte header, the triangle count, then per
/// triangle its face normal, three corners and a zero attribute word.
pub fn stl<'a>(meshes: impl IntoIterator<Item = &'a Mesh>) -> Vec<u8> {
    let meshes: Vec<&Mesh> = meshes.into_iter().collect();
    let count: usize = meshes.iter().map(|m| m.triangle_count()).sum();
    let mut out = Vec::with_capacity(84 + count * 50);
    let mut header = b"Exported by Falcon".to_vec();
    header.resize(80, 0);
    out.extend_from_slice(&header);
    out.extend_from_slice(&(count as u32).to_le_bytes());
    for m in meshes {
        for t in m.indices.as_chunks::<3>().0 {
            let p: [V3; 3] = t.map(|i| m.positions[i as usize]);
            let n = geom::face_normal(p[0], p[1], p[2]);
            for v in std::iter::once(n).chain(p) {
                for c in v {
                    out.extend_from_slice(&c.to_le_bytes());
                }
            }
            out.extend_from_slice(&[0, 0]);
        }
    }
    out
}

/// A colour channel from the sRGB values falcon keeps to the linear ones
/// glTF stores (the reader's `srgb` undone).
fn linear(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.040_45 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// The JSON document and binary buffer of a glTF 2.0 asset holding
/// `meshes`, one node and mesh each, with 32-bit indices and the buffer
/// `uri` (`None` for a GLB, whose buffer is its BIN chunk).
fn gltf_parts<'a>(meshes: impl IntoIterator<Item = &'a Mesh>, uri: impl FnOnce(&[u8]) -> Option<String>) -> (serde_json::Value, Vec<u8>) {
    use serde_json::{Value, json};
    let mut bin: Vec<u8> = Vec::new();
    let (mut views, mut accessors, mut gl_meshes, mut nodes, mut materials) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::<Value>::new());
    let mut mat_names: Vec<String> = Vec::new();
    // Appends floats or indices as a buffer view and an accessor; returns its index.
    let mut push = |bin: &mut Vec<u8>, bytes: Vec<u8>, count: usize, ty: &str, component: u32, target: u32, minmax: Option<(V3, V3)>| {
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        views.push(json!({"buffer": 0, "byteOffset": bin.len(), "byteLength": bytes.len(), "target": target}));
        bin.extend_from_slice(&bytes);
        let mut a = json!({"bufferView": views.len() - 1, "componentType": component, "count": count, "type": ty});
        if let Some((lo, hi)) = minmax {
            a["min"] = json!(lo);
            a["max"] = json!(hi);
        }
        accessors.push(a);
        accessors.len() - 1
    };
    let floats = |vs: &[V3]| vs.iter().flatten().flat_map(|c| c.to_le_bytes()).collect::<Vec<u8>>();
    for (k, m) in meshes.into_iter().enumerate() {
        if m.positions.is_empty() || m.indices.is_empty() {
            continue;
        }
        let lo = m.positions.iter().fold([f32::INFINITY; 3], |a, p| [a[0].min(p[0]), a[1].min(p[1]), a[2].min(p[2])]);
        let hi = m.positions.iter().fold([f32::NEG_INFINITY; 3], |a, p| [a[0].max(p[0]), a[1].max(p[1]), a[2].max(p[2])]);
        const ARRAY: u32 = 34962;
        const ELEMENTS: u32 = 34963;
        const FLOAT: u32 = 5126;
        let mut attrs = serde_json::Map::new();
        attrs.insert("POSITION".into(), json!(push(&mut bin, floats(&m.positions), m.positions.len(), "VEC3", FLOAT, ARRAY, Some((lo, hi)))));
        if let Some(ns) = m.normals.as_ref().filter(|n| n.len() == m.positions.len()) {
            attrs.insert("NORMAL".into(), json!(push(&mut bin, floats(ns), ns.len(), "VEC3", FLOAT, ARRAY, None)));
        }
        if let Some(cs) = m.colors.as_ref().filter(|c| c.len() == m.positions.len()) {
            let lin: Vec<V3> = cs.iter().map(|c| c.map(linear)).collect();
            attrs.insert("COLOR_0".into(), json!(push(&mut bin, floats(&lin), lin.len(), "VEC3", FLOAT, ARRAY, None)));
        }
        let idx: Vec<u8> = m.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let indices = push(&mut bin, idx, m.indices.len(), "SCALAR", 5125, ELEMENTS, None);
        let mut prim = json!({"attributes": attrs, "indices": indices, "mode": 4});
        if let Some(mat) = &m.material {
            let at = mat_names.iter().position(|n| *n == mat.name).unwrap_or_else(|| {
                let c = mat.color.map(linear);
                let mut v = json!({"name": mat.name, "pbrMetallicRoughness": {"baseColorFactor": [c[0], c[1], c[2], mat.opacity.clamp(0.0, 1.0)], "metallicFactor": 0.0}});
                if mat.opacity < 1.0 {
                    v["alphaMode"] = json!("BLEND");
                }
                materials.push(v);
                mat_names.push(mat.name.clone());
                materials.len() - 1
            });
            prim["material"] = json!(at);
        }
        let name = if m.name.trim().is_empty() { format!("mesh{}", k + 1) } else { m.name.clone() };
        gl_meshes.push(json!({"name": name, "primitives": [prim]}));
        nodes.push(json!({"name": name, "mesh": gl_meshes.len() - 1}));
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let mut buffer = json!({"byteLength": bin.len()});
    if let Some(u) = uri(&bin) {
        buffer["uri"] = json!(u);
    }
    let mut doc = json!({
        "asset": {"version": "2.0", "generator": "Falcon"},
        "scene": 0,
        "scenes": [{"nodes": (0..nodes.len()).collect::<Vec<_>>()}],
        "nodes": nodes,
        "meshes": gl_meshes,
    });
    if !bin.is_empty() {
        doc["buffers"] = json!([buffer]);
        doc["bufferViews"] = json!(views);
        doc["accessors"] = json!(accessors);
    }
    if !materials.is_empty() {
        doc["materials"] = json!(materials);
    }
    (doc, bin)
}

/// A binary glTF (GLB) of `meshes`: the 12-byte header, the JSON chunk
/// (padded with spaces) and the BIN chunk.
pub fn glb<'a>(meshes: impl IntoIterator<Item = &'a Mesh>) -> Vec<u8> {
    let (doc, bin) = gltf_parts(meshes, |_| None);
    let mut json = serde_json::to_vec(&doc).unwrap_or_default();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let total = 12 + 8 + json.len() + if bin.is_empty() { 0 } else { 8 + bin.len() };
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json);
    if !bin.is_empty() {
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
    }
    out
}

/// A `.gltf` of `meshes`: the JSON with its buffer as a base64 data URI.
pub fn gltf<'a>(meshes: impl IntoIterator<Item = &'a Mesh>) -> String {
    use base64::Engine;
    let (doc, _) = gltf_parts(meshes, |bin| Some(format!("data:application/octet-stream;base64,{}", base64::engine::general_purpose::STANDARD.encode(bin))));
    serde_json::to_string_pretty(&doc).unwrap_or_default()
}

/// Binary little-endian PLY of `meshes` merged into one: positions, normals
/// when every mesh has them, and colours (vertex colours, else the mesh's
/// material colour, else the viewer's default) when any mesh has either;
/// then the triangles.
pub fn ply<'a>(meshes: impl IntoIterator<Item = &'a Mesh>) -> Vec<u8> {
    let meshes: Vec<&Mesh> = meshes.into_iter().filter(|m| !m.positions.is_empty()).collect();
    let vertices: usize = meshes.iter().map(|m| m.positions.len()).sum();
    let faces: usize = meshes.iter().map(|m| m.triangle_count()).sum();
    let normals = !meshes.is_empty() && meshes.iter().all(|m| m.normals.as_ref().is_some_and(|n| n.len() == m.positions.len()));
    let colors = meshes.iter().any(|m| m.colors.is_some() || m.material.is_some());
    let mut head = format!("ply\nformat binary_little_endian 1.0\ncomment Exported by Falcon\nelement vertex {vertices}\nproperty float x\nproperty float y\nproperty float z\n");
    if normals {
        head.push_str("property float nx\nproperty float ny\nproperty float nz\n");
    }
    if colors {
        head.push_str("property uchar red\nproperty uchar green\nproperty uchar blue\n");
    }
    head.push_str(&format!("element face {faces}\nproperty list uchar int vertex_indices\nend_header\n"));
    let mut out = head.into_bytes();
    let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
    for m in &meshes {
        let fallback = m.material.as_ref().map_or(super::DEFAULT_COLOR, |mat| mat.color);
        for (i, p) in m.positions.iter().enumerate() {
            for c in p {
                out.extend_from_slice(&c.to_le_bytes());
            }
            if normals && let Some(n) = m.normals.as_ref().and_then(|ns| ns.get(i)) {
                for c in n {
                    out.extend_from_slice(&c.to_le_bytes());
                }
            }
            if colors {
                let c = m.colors.as_ref().and_then(|cs| cs.get(i)).copied().unwrap_or(fallback);
                out.extend(c.map(byte));
            }
        }
    }
    let mut base = 0u32;
    for m in &meshes {
        for t in m.indices.as_chunks::<3>().0 {
            out.push(3);
            for i in t {
                out.extend_from_slice(&((base + i) as i32).to_le_bytes());
            }
        }
        base += m.positions.len() as u32;
    }
    out
}

/// ASCII OFF of `meshes` merged into one: `OFF`, the vertex and face counts
/// and a zero edge count, a line per vertex, then `3 a b c` per triangle
/// with zero-based indices offset per mesh.
pub fn off<'a>(meshes: impl IntoIterator<Item = &'a Mesh>) -> String {
    use std::fmt::Write;
    let meshes: Vec<&Mesh> = meshes.into_iter().filter(|m| !m.positions.is_empty()).collect();
    let vertices: usize = meshes.iter().map(|m| m.positions.len()).sum();
    let faces: usize = meshes.iter().map(|m| m.triangle_count()).sum();
    let mut out = format!("OFF\n{vertices} {faces} 0\n");
    for p in meshes.iter().flat_map(|m| &m.positions) {
        let _ = writeln!(out, "{} {} {}", p[0], p[1], p[2]);
    }
    let mut base = 0usize;
    for m in &meshes {
        for t in m.indices.as_chunks::<3>().0 {
            let [a, b, c] = t.map(|i| i as usize + base);
            let _ = writeln!(out, "3 {a} {b} {c}");
        }
        base += m.positions.len();
    }
    out
}

/// Writes `meshes` to `path` in `format`; an OBJ gets its MTL next to it
/// (same name, `.mtl`) when it has materials.
pub fn write<'a>(meshes: impl IntoIterator<Item = &'a Mesh>, format: Format, path: &std::path::Path) -> std::io::Result<()> {
    match format {
        Format::Stl => std::fs::write(path, stl(meshes)),
        Format::Glb => std::fs::write(path, glb(meshes)),
        Format::Gltf => std::fs::write(path, gltf(meshes)),
        Format::Ply => std::fs::write(path, ply(meshes)),
        Format::Off => std::fs::write(path, off(meshes)),
        Format::Obj => {
            let mtl_path = path.with_extension("mtl");
            let mtl_name = mtl_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let (obj, mtl) = obj(meshes, &mtl_name);
            std::fs::write(path, obj)?;
            if !mtl.is_empty() {
                std::fs::write(mtl_path, mtl)?;
            }
            Ok(())
        }
    }
}

/// A point where a ray meets a visible triangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub point: V3,
    /// The face's unit normal.
    pub normal: V3,
    /// The index of the mesh hit.
    pub mesh: usize,
}

/// The nearest triangle of the meshes for which `visible(i)` holds that the
/// ray from `origin` along `dir` meets (Möller–Trumbore), for the viewer's
/// measure tool.
pub fn pick(meshes: &[Mesh], visible: impl Fn(usize) -> bool, origin: V3, dir: V3) -> Option<Hit> {
    let mut best: Option<(f32, Hit)> = None;
    for (k, m) in meshes.iter().enumerate().filter(|(k, _)| visible(*k)) {
        for t in m.indices.as_chunks::<3>().0 {
            let [a, b, c] = t.map(|i| m.positions[i as usize]);
            let (e1, e2) = (math::sub(b, a), math::sub(c, a));
            let p = math::cross(dir, e2);
            let det = math::dot(e1, p);
            if det.abs() < 1e-12 {
                continue;
            }
            let inv = 1.0 / det;
            let s = math::sub(origin, a);
            let u = math::dot(s, p) * inv;
            if !(0.0..=1.0).contains(&u) {
                continue;
            }
            let q = math::cross(s, e1);
            let v = math::dot(dir, q) * inv;
            if v < 0.0 || u + v > 1.0 {
                continue;
            }
            let d = math::dot(e2, q) * inv;
            if d > 0.0 && best.is_none_or(|(bd, _)| d < bd) {
                let hit = Hit { point: math::add(origin, math::scale(dir, d)), normal: geom::face_normal(a, b, c), mesh: k };
                best = Some((d, hit));
            }
        }
    }
    best.map(|(_, h)| h)
}

/// What the measure tool shows for two picked points: their distance, the
/// angle between the faces (degrees) and, when the faces are parallel, the
/// distance between their planes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measure {
    pub distance: f32,
    pub angle: f32,
    pub parallel: Option<f32>,
}

pub fn measure(a: &Hit, b: &Hit) -> Measure {
    let distance = math::length(math::sub(b.point, a.point));
    let cos = math::dot(a.normal, b.normal).clamp(-1.0, 1.0);
    let angle = cos.acos().to_degrees();
    let parallel = (cos.abs() > 1.0 - 1e-5).then(|| math::dot(math::sub(b.point, a.point), a.normal).abs());
    Measure { distance, angle, parallel }
}

#[cfg(test)]
mod tests {
    use super::super::Material;
    use super::super::geom::tests::cube;
    use super::*;

    #[test]
    fn obj_round_trips_through_the_reader() {
        let mut m = cube(false);
        m.name = "box one".into();
        m.material = Some(Material { name: "red paint".into(), color: [1.0, 0.0, 0.0], opacity: 1.0 });
        let second = Mesh { name: "".into(), ..cube(false) };
        let (text, mtl) = obj([&m, &second], "x.mtl");
        assert!(text.starts_with("# Exported by Falcon\nmtllib x.mtl\no box_one\n"));
        assert!(mtl.contains("newmtl red_paint\nKd 1 0 0"));
        assert!(text.contains("o mesh2"));
        let side = |name: &str| (name == "x.mtl").then(|| mtl.clone().into_bytes());
        let back = super::super::parse(text.as_bytes(), "obj", &side).unwrap();
        assert_eq!(back.triangle_count(), 24);
        assert_eq!(back.meshes[0].material.as_ref().map(|m| m.color), Some([1.0, 0.0, 0.0]));
    }

    #[test]
    fn stl_round_trips_through_the_reader() {
        let m = cube(false);
        let bytes = stl([&m]);
        assert_eq!(bytes.len(), 84 + 12 * 50);
        let back = super::super::parse(&bytes, "stl", &|_| None).unwrap();
        assert_eq!(back.triangle_count(), 12);
        assert_eq!(back.bounds(), Some(([0.0; 3], [1.0; 3])));
    }

    fn painted() -> (Mesh, Mesh) {
        let mut a = cube(true);
        a.name = "box".into();
        a.material = Some(Material { name: "red".into(), color: [0.8, 0.2, 0.1], opacity: 1.0 });
        a.normals = Some(a.indices.as_chunks::<3>().0.iter().flat_map(|t| {
            let n = geom::face_normal(a.positions[t[0] as usize], a.positions[t[1] as usize], a.positions[t[2] as usize]);
            [n; 3]
        }).collect());
        let mut b = cube(false);
        b.positions.iter_mut().for_each(|p| p[0] += 2.0);
        b.colors = Some(vec![[0.0, 1.0, 0.5]; b.positions.len()]);
        (a, b)
    }

    fn close(a: V3, b: V3) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 2e-3)
    }

    #[test]
    fn glb_round_trips_through_the_reader() {
        let (a, b) = painted();
        let bytes = glb([&a, &b]);
        assert_eq!(&bytes[..4], b"glTF");
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize, bytes.len());
        assert_eq!(bytes.len() % 4, 0);
        let back = super::super::parse(&bytes, "glb", &|_| None).unwrap();
        assert_eq!((back.meshes.len(), back.triangle_count(), back.vertex_count()), (2, 24, a.positions.len() + b.positions.len()));
        assert_eq!(back.meshes[0].name, "box");
        let mat = back.meshes[0].material.as_ref().unwrap();
        assert!(close(mat.color, [0.8, 0.2, 0.1]), "sRGB survives the linear store: {:?}", mat.color);
        assert!(back.meshes[0].normals.is_some());
        assert!(close(back.meshes[1].colors.as_ref().unwrap()[0], [0.0, 1.0, 0.5]));
        assert_eq!(back.bounds(), Some(([0.0; 3], [3.0, 1.0, 1.0])));
        // Nothing visible still makes a well-formed file.
        let empty = glb(std::iter::empty());
        assert_eq!(u32::from_le_bytes(empty[8..12].try_into().unwrap()) as usize, empty.len());
    }

    #[test]
    fn gltf_embeds_its_buffer() {
        let (a, b) = painted();
        let text = gltf([&a, &b]);
        assert!(text.contains("data:application/octet-stream;base64,"));
        let back = super::super::gltf::parse_gltf(text.as_bytes(), &|_| None).unwrap();
        assert_eq!(back.triangle_count(), 24);
        assert!(close(back.meshes[0].material.as_ref().unwrap().color, [0.8, 0.2, 0.1]));
    }

    #[test]
    fn ply_round_trips_through_the_reader() {
        let (a, b) = painted();
        let bytes = ply([&a, &b]);
        let head = String::from_utf8_lossy(&bytes[..bytes.windows(11).position(|w| w == b"end_header\n").unwrap()]).into_owned();
        assert!(head.contains(&format!("element vertex {}\n", a.positions.len() + b.positions.len())));
        assert!(!head.contains("nx"), "one mesh has no normals, so none are written");
        let back = super::super::parse(&bytes, "ply", &|_| None).unwrap();
        assert_eq!(back.triangle_count(), 24);
        let cs = back.meshes[0].colors.as_ref().unwrap();
        assert!(close(cs[0], [204.0 / 255.0, 51.0 / 255.0, 26.0 / 255.0]), "the material colour per vertex: {:?}", cs[0]);
        assert!(close(cs[a.positions.len()], [0.0, 1.0, 128.0 / 255.0]));
        assert_eq!(back.bounds(), Some(([0.0; 3], [3.0, 1.0, 1.0])));
        // A plain mesh writes positions and faces only.
        let plain = ply([&cube(false)]);
        assert!(!String::from_utf8_lossy(&plain).contains("red"));
        assert_eq!(super::super::parse(&plain, "ply", &|_| None).unwrap().triangle_count(), 12);
    }

    #[test]
    fn off_round_trips_through_the_reader() {
        let (a, b) = painted();
        let text = off([&a, &b]);
        let nv = a.positions.len() + b.positions.len();
        assert!(text.starts_with(&format!("OFF\n{nv} 24 0\n")), "{}", &text[..20]);
        assert_eq!(text.lines().count(), 2 + nv + 24);
        let back = super::super::off::parse(text.as_bytes()).unwrap();
        let m = &back.meshes[0];
        let all: Vec<V3> = a.positions.iter().chain(&b.positions).copied().collect();
        assert_eq!(m.positions.len(), all.len());
        assert!(m.positions.iter().zip(&all).all(|(p, q)| close(*p, *q)));
        let want: Vec<u32> = a.indices.iter().copied().chain(b.indices.iter().map(|i| i + a.positions.len() as u32)).collect();
        assert_eq!(m.indices, want);
        assert_eq!(back.bounds(), Some(([0.0; 3], [3.0, 1.0, 1.0])));
    }

    #[test]
    fn off_offsets_indices_per_mesh() {
        let (a, b) = painted();
        let text = off([&a, &b]);
        let n = a.positions.len();
        let last = text.lines().last().unwrap();
        let ix: Vec<usize> = last.split(' ').skip(1).map(|s| s.parse().unwrap()).collect();
        assert!(ix.iter().all(|&i| i >= n && i < n + b.positions.len()), "{last}");
        assert!(off(std::iter::empty()).starts_with("OFF\n0 0 0\n"));
    }

    #[test]
    fn off_reader_takes_comments_polygons_and_colours() {
        let text = "OFF\n# comment\n\n4 1 0\n0 0 0\n1 0 0\n1 1 0 # c\n0 1 0\n4 0 1 2 3 255 0 0\n";
        let m = &super::super::off::parse(text.as_bytes()).unwrap().meshes[0];
        assert_eq!(m.indices, vec![0, 1, 2, 0, 2, 3]);
    }

    #[test]
    fn rays_hit_the_nearest_visible_face() {
        let meshes = vec![cube(false)];
        let h = pick(&meshes, |_| true, [0.5, 0.5, 5.0], [0.0, 0.0, -1.0]).unwrap();
        assert!((h.point[2] - 1.0).abs() < 1e-5, "{h:?}");
        assert!((h.normal[2].abs() - 1.0).abs() < 1e-5);
        assert!(pick(&meshes, |_| false, [0.5, 0.5, 5.0], [0.0, 0.0, -1.0]).is_none());
        assert!(pick(&meshes, |_| true, [5.0, 5.0, 5.0], [0.0, 0.0, -1.0]).is_none());
        let far = pick(&meshes, |_| true, [0.5, 0.5, -5.0], [0.0, 0.0, 1.0]).unwrap();
        let m = measure(&h, &far);
        assert!((m.distance - 1.0).abs() < 1e-5);
        assert!((m.parallel.unwrap() - 1.0).abs() < 1e-5);
        let side = pick(&meshes, |_| true, [5.0, 0.5, 0.5], [-1.0, 0.0, 0.0]).unwrap();
        let m = measure(&h, &side);
        assert!((m.angle - 90.0).abs() < 1e-3 && m.parallel.is_none());
    }
}
