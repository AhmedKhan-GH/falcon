//! Wavefront OBJ, with colours from its MTL library.

use std::borrow::Cow;
use std::collections::HashMap;

use super::math::V3;
use super::{DEFAULT_COLOR, Material, Mesh, Model, Result};

const NO_NORMAL: u32 = u32::MAX;

/// The lines of `text` with `\` continuations joined; a line is only
/// copied when it is continued.
fn logical_lines(text: &str) -> impl Iterator<Item = Cow<'_, str>> {
    let mut lines = text.lines();
    std::iter::from_fn(move || {
        let first = lines.next()?;
        let Some(head) = first.trim_end().strip_suffix('\\') else {
            return Some(Cow::Borrowed(first));
        };
        let mut joined = String::from(head);
        for next in lines.by_ref() {
            joined.push(' ');
            match next.trim_end().strip_suffix('\\') {
                Some(h) => joined.push_str(h),
                None => {
                    joined.push_str(next);
                    break;
                }
            }
        }
        Some(Cow::Owned(joined))
    })
}

/// A line without its trailing `#` comment.
fn strip_comment(line: &str) -> &str {
    line.find('#').map_or(line, |i| &line[..i])
}

fn floats<'a, const N: usize>(it: &mut impl Iterator<Item = &'a str>) -> Option<[f32; N]> {
    let mut out = [0.0f32; N];
    for slot in &mut out {
        *slot = it.next()?.parse().ok()?;
    }
    Some(out)
}

/// One part under construction: its own compact vertex list.
struct Builder {
    mesh: Mesh,
    colors: Vec<V3>,
    all_normals: bool,
    lookup: HashMap<u64, u32>,
}

#[derive(Default)]
struct MtlEntry {
    color: Option<V3>,
    opacity: Option<f32>,
    tr: Option<f32>,
}

fn parse_mtl(bytes: &[u8], out: &mut HashMap<String, Material>) {
    let text = String::from_utf8_lossy(bytes);
    let mut cur: Option<(String, MtlEntry)> = None;
    let flush = |cur: &mut Option<(String, MtlEntry)>, out: &mut HashMap<String, Material>| {
        if let Some((name, e)) = cur.take() {
            let opacity = e.opacity.or(e.tr.map(|t| 1.0 - t)).unwrap_or(1.0).clamp(0.0, 1.0);
            let color = e.color.unwrap_or(DEFAULT_COLOR);
            out.entry(name.clone()).or_insert(Material { name, color, opacity });
        }
    };
    for line in logical_lines(&text) {
        let line = strip_comment(&line).trim();
        let (key, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let rest = rest.trim();
        match key {
            "newmtl" => {
                flush(&mut cur, out);
                cur = Some((rest.to_string(), MtlEntry::default()));
            }
            "Kd" => {
                if let Some((_, e)) = cur.as_mut() {
                    let mut it = rest.split_ascii_whitespace();
                    if let Some([r, g, b]) = floats::<3>(&mut it) {
                        e.color = Some([r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0)]);
                    }
                }
            }
            "d" => {
                if let (Some((_, e)), Ok(v)) = (cur.as_mut(), rest.split_ascii_whitespace().next().unwrap_or("").parse::<f32>()) {
                    e.opacity = Some(v);
                }
            }
            "Tr" => {
                if let (Some((_, e)), Ok(v)) = (cur.as_mut(), rest.split_ascii_whitespace().next().unwrap_or("").parse::<f32>()) {
                    e.tr = Some(v);
                }
            }
            _ => {}
        }
    }
    flush(&mut cur, out);
}

/// Resolves an OBJ index (1-based, or negative from the end) to 0-based.
fn resolve(tok: &str, len: usize) -> Option<u32> {
    let i: i64 = tok.parse().ok()?;
    let r = if i > 0 { i - 1 } else if i < 0 { len as i64 + i } else { return None };
    (r >= 0 && (r as usize) < len).then_some(r as u32)
}

pub fn parse(bytes: &[u8], side: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Model> {
    let text = String::from_utf8_lossy(bytes);
    let mut positions: Vec<V3> = Vec::new();
    let mut vcolors: Vec<V3> = Vec::new();
    let mut has_color = false;
    let mut normals: Vec<V3> = Vec::new();
    let mut materials: HashMap<String, Material> = HashMap::new();

    let mut obj_name = String::new();
    let mut mat_name: Option<String> = None;
    let mut builders: Vec<Builder> = Vec::new();
    let mut by_key: HashMap<(String, Option<String>), usize> = HashMap::new();
    // Index into `builders` of the current run; None until a face needs one.
    let mut current: Option<usize> = None;
    let mut corners: Vec<(u32, u32)> = Vec::new();

    for line in logical_lines(&text) {
        let line = strip_comment(&line);
        let mut it = line.split_ascii_whitespace();
        let Some(key) = it.next() else { continue };
        match key {
            "v" => {
                let Some([x, y, z]) = floats::<3>(&mut it) else { continue };
                positions.push([x, y, z]);
                if let Some([r, g, b]) = floats::<3>(&mut it) {
                    has_color = true;
                    vcolors.push([r, g, b]);
                } else {
                    vcolors.push(DEFAULT_COLOR);
                }
            }
            "vn" => {
                let Some(n) = floats::<3>(&mut it) else { continue };
                normals.push(n);
            }
            "o" | "g" => {
                let name = line.trim().get(key.len()..).unwrap_or("").trim();
                if name != obj_name {
                    obj_name = name.to_string();
                    current = None;
                }
            }
            "usemtl" => {
                let name = line.trim().get(key.len()..).unwrap_or("").trim();
                if mat_name.as_deref() != Some(name) {
                    mat_name = Some(name.to_string());
                    current = None;
                }
            }
            "mtllib" => {
                let rest = line.trim().get(key.len()..).unwrap_or("").trim();
                if rest.is_empty() {
                    continue;
                }
                if let Some(b) = side(rest) {
                    parse_mtl(&b, &mut materials);
                } else {
                    for name in rest.split_ascii_whitespace() {
                        if let Some(b) = side(name) {
                            parse_mtl(&b, &mut materials);
                        }
                    }
                }
            }
            "f" => {
                corners.clear();
                for tok in it {
                    let mut parts = tok.split('/');
                    let Some(p) = parts.next().and_then(|t| resolve(t, positions.len())) else {
                        corners.clear();
                        break;
                    };
                    let _vt = parts.next();
                    let n = match parts.next() {
                        Some(t) if !t.is_empty() => match resolve(t, normals.len()) {
                            Some(n) => n,
                            None => {
                                corners.clear();
                                break;
                            }
                        },
                        _ => NO_NORMAL,
                    };
                    corners.push((p, n));
                }
                if corners.len() < 3 {
                    continue;
                }
                let bi = match current {
                    Some(b) => b,
                    None => {
                        let key = (obj_name.clone(), mat_name.clone());
                        let b = *by_key.entry(key).or_insert_with(|| {
                            let material = mat_name.as_ref().map(|n| {
                                materials.get(n).cloned().unwrap_or_else(|| Material { name: n.clone(), color: DEFAULT_COLOR, opacity: 1.0 })
                            });
                            builders.push(Builder {
                                mesh: Mesh { name: obj_name.clone(), material, ..Mesh::default() },
                                colors: Vec::new(),
                                all_normals: true,
                                lookup: HashMap::new(),
                            });
                            builders.len() - 1
                        });
                        current = Some(b);
                        b
                    }
                };
                let b = &mut builders[bi];
                let mut idx = |(p, n): (u32, u32)| -> u32 {
                    let k = (u64::from(p) << 32) | u64::from(n);
                    *b.lookup.entry(k).or_insert_with(|| {
                        let i = b.mesh.positions.len() as u32;
                        b.mesh.positions.push(positions[p as usize]);
                        b.colors.push(vcolors[p as usize]);
                        let nv = if n == NO_NORMAL {
                            b.all_normals = false;
                            [0.0, 0.0, 1.0]
                        } else {
                            normals[n as usize]
                        };
                        b.mesh.normals.get_or_insert_with(Vec::new).push(nv);
                        i
                    })
                };
                let first = idx(corners[0]);
                let mut prev = idx(corners[1]);
                for &c in &corners[2..] {
                    let cur = idx(c);
                    b.mesh.indices.extend_from_slice(&[first, prev, cur]);
                    prev = cur;
                }
            }
            _ => {}
        }
    }

    let meshes = builders
        .into_iter()
        .map(|mut b| {
            if !b.all_normals {
                b.mesh.normals = None;
            }
            if has_color {
                b.mesh.colors = Some(b.colors);
            }
            b.mesh
        })
        .collect();
    Model { meshes }.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &str) -> Option<Vec<u8>> {
        None
    }

    #[test]
    fn quad_is_fan_triangulated() {
        let m = parse(b"v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nf 1 2 3 4\n", &none).unwrap();
        assert_eq!(m.meshes.len(), 1);
        assert_eq!(m.meshes[0].indices, vec![0, 1, 2, 0, 2, 3]);
        assert_eq!(m.meshes[0].positions.len(), 4);
        assert!(m.meshes[0].material.is_none());
        assert!(m.meshes[0].normals.is_none());
    }

    #[test]
    fn double_slash_normals() {
        let src = b"v 0 0 0\r\nv 1 0 0\r\nv 0 1 0\r\nvn 0 0 1\r\nf 1//1 2//1 3//1\r\n";
        let m = parse(src, &none).unwrap();
        assert_eq!(m.meshes[0].normals.as_ref().unwrap(), &vec![[0.0, 0.0, 1.0]; 3]);
        let m = parse(b"v 0 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nvn 0 0 1\nf 1/1/1 2/1/1 3/1/1\n", &none).unwrap();
        assert!(m.meshes[0].normals.is_some());
    }

    #[test]
    fn negative_indices() {
        let m = parse(b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf -3 -2 -1\n", &none).unwrap();
        assert_eq!(m.meshes[0].indices, vec![0, 1, 2]);
        assert_eq!(m.meshes[0].positions[2], [0.0, 1.0, 0.0]);
    }

    #[test]
    fn usemtl_runs_make_meshes_with_mtl_colours() {
        let src = b"mtllib a.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nv 1 1 0\nusemtl red\nf 1 2 3\nusemtl blue\nf 2 4 3\n";
        let side = |n: &str| -> Option<Vec<u8>> {
            (n == "a.mtl").then(|| b"newmtl red\nKd 1 0 0\nnewmtl blue\r\nKd 0 0 1\nTr 0.25\n".to_vec())
        };
        let m = parse(src, &side).unwrap();
        assert_eq!(m.meshes.len(), 2);
        let a = m.meshes[0].material.as_ref().unwrap();
        assert_eq!((a.name.as_str(), a.color, a.opacity), ("red", [1.0, 0.0, 0.0], 1.0));
        let b = m.meshes[1].material.as_ref().unwrap();
        assert_eq!(b.color, [0.0, 0.0, 1.0]);
        assert!((b.opacity - 0.75).abs() < 1e-6);
        assert_eq!(m.meshes[1].positions.len(), 3);
    }

    #[test]
    fn missing_mtl_falls_back_to_default() {
        let m = parse(b"mtllib gone.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nusemtl wood\nf 1 2 3\n", &none).unwrap();
        let mat = m.meshes[0].material.as_ref().unwrap();
        assert_eq!(mat.name, "wood");
        assert_eq!(mat.color, DEFAULT_COLOR);
        assert_eq!(mat.opacity, 1.0);
    }

    #[test]
    fn vertex_colours() {
        let m = parse(b"v 0 0 0 1 0 0\nv 1 0 0\nv 0 1 0 0 1 0\nf 1 2 3\n", &none).unwrap();
        let c = m.meshes[0].colors.as_ref().unwrap();
        assert_eq!(c[0], [1.0, 0.0, 0.0]);
        assert_eq!(c[1], DEFAULT_COLOR);
        assert_eq!(c[2], [0.0, 1.0, 0.0]);
    }

    #[test]
    fn garbage_and_empty_fail() {
        assert!(parse(b"", &none).is_err());
        assert!(parse(b"\x00\xff not an obj \x01\n# c\nhello world\n", &none).is_err());
    }

    #[test]
    fn out_of_range_face_is_skipped() {
        let m = parse(b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 9\nf 1 2 3\n# c\n", &none).unwrap();
        assert_eq!(m.meshes[0].indices, vec![0, 1, 2]);
        assert!(parse(b"v 0 0 0\nv 1 0 0\nf 1 2 3\n", &none).is_err());
    }

    #[test]
    fn names_and_continuations() {
        let m = parse(b"o part\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 \\\n 3 # tail\n", &none).unwrap();
        assert_eq!(m.meshes[0].name, "part");
        assert_eq!(m.meshes[0].triangle_count(), 1);
    }
}
