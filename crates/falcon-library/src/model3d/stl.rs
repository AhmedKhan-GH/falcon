//! STL, ASCII and binary.

use std::collections::HashMap;

use super::math::V3;
use super::{Mesh, Model, ParseError, Result};

/// Triangles collected corner by corner, with equal positions welded into
/// one vertex.
#[derive(Default)]
struct Builder {
    positions: Vec<V3>,
    indices: Vec<u32>,
    seen: HashMap<[u32; 3], u32>,
}

impl Builder {
    fn vertex(&mut self, p: V3) {
        // -0.0 and 0.0 are the same place.
        let key = p.map(|c| if c == 0.0 { 0 } else { c.to_bits() });
        let positions = &mut self.positions;
        let i = *self.seen.entry(key).or_insert_with(|| {
            positions.push(p);
            (positions.len() - 1) as u32
        });
        self.indices.push(i);
    }

    fn into_mesh(self, name: String) -> Mesh {
        Mesh { name, positions: self.positions, indices: self.indices, ..Mesh::default() }
    }
}

pub fn parse(bytes: &[u8]) -> Result<Model> {
    let meshes = if let Some(count) = binary_count(bytes) {
        vec![parse_binary(bytes, count)]
    } else if starts_with_solid(bytes) {
        parse_ascii(bytes)
    } else if bytes.len() >= 84 {
        // Not "solid" and the size is off: take what whole triangles there are.
        let count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        if bytes.len() - 84 < 50 * count {
            return Err(ParseError::new("not an STL file"));
        }
        vec![parse_binary(bytes, count)]
    } else {
        return Err(ParseError::new("not an STL file"));
    };
    Model { meshes }.finish()
}

/// The triangle count when the size says this is a complete binary file.
fn binary_count(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < 84 {
        return None;
    }
    let count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
    (84 + 50 * count == bytes.len()).then_some(count)
}

fn starts_with_solid(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(512)];
    let head = head.trim_ascii_start();
    head.len() >= 5 && head[..5].eq_ignore_ascii_case(b"solid")
}

fn parse_binary(bytes: &[u8], count: usize) -> Mesh {
    let body = &bytes[84..];
    let mut b = Builder::default();
    for rec in body.as_chunks::<50>().0.iter().take(count) {
        for v in 1..4 {
            let f = |k: usize| {
                let o = v * 12 + k * 4;
                f32::from_le_bytes([rec[o], rec[o + 1], rec[o + 2], rec[o + 3]])
            };
            b.vertex([f(0), f(1), f(2)]);
        }
    }
    b.into_mesh(String::new())
}

fn parse_ascii(bytes: &[u8]) -> Vec<Mesh> {
    let text = String::from_utf8_lossy(bytes);
    let mut meshes = Vec::new();
    let mut cur: Option<(String, Builder)> = None;
    // The corners of the loop being read.
    let mut corners: Vec<V3> = Vec::new();
    let mut in_loop = false;
    for line in text.lines() {
        let line = line.trim();
        let mut words = line.split_whitespace();
        let Some(first) = words.next() else { continue };
        match first.to_ascii_lowercase().as_str() {
            "solid" => {
                if let Some((n, b)) = cur.take() {
                    meshes.push(b.into_mesh(n));
                }
                let name = line[first.len()..].trim().to_string();
                cur = Some((name, Builder::default()));
            }
            "endsolid" => {
                if let Some((n, b)) = cur.take() {
                    meshes.push(b.into_mesh(n));
                }
            }
            "outer" => {
                in_loop = true;
                corners.clear();
            }
            "vertex" if in_loop => {
                let v: Vec<f32> = words.filter_map(|w| w.parse().ok()).collect();
                if v.len() >= 3 {
                    corners.push([v[0], v[1], v[2]]);
                }
            }
            "endloop" => {
                in_loop = false;
                let (_, b) = cur.get_or_insert_with(|| (String::new(), Builder::default()));
                for k in 1..corners.len().saturating_sub(1) {
                    b.vertex(corners[0]);
                    b.vertex(corners[k]);
                    b.vertex(corners[k + 1]);
                }
                corners.clear();
            }
            _ => {}
        }
    }
    if let Some((n, b)) = cur.take() {
        meshes.push(b.into_mesh(n));
    }
    meshes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binary(header: &[u8], tris: &[[V3; 3]]) -> Vec<u8> {
        let mut out = vec![0u8; 80];
        out[..header.len()].copy_from_slice(header);
        out.extend((tris.len() as u32).to_le_bytes());
        for t in tris {
            out.extend([0u8; 12]);
            for v in t {
                for c in v {
                    out.extend(c.to_le_bytes());
                }
            }
            out.extend([0u8; 2]);
        }
        out
    }

    fn tetra() -> Vec<[V3; 3]> {
        let (a, b, c, d) = ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
        vec![[a, c, b], [a, b, d], [b, c, d], [c, a, d]]
    }

    #[test]
    fn binary_tetrahedron_is_welded() {
        let m = parse(&binary(b"tetra", &tetra())).unwrap();
        assert_eq!(m.meshes.len(), 1);
        assert_eq!(m.triangle_count(), 4);
        assert_eq!(m.vertex_count(), 4);
        assert!(m.meshes[0].normals.is_none() && m.meshes[0].colors.is_none() && m.meshes[0].material.is_none());
    }

    #[test]
    fn binary_header_starting_with_solid() {
        let m = parse(&binary(b"solid exported by something", &tetra())).unwrap();
        assert_eq!(m.triangle_count(), 4);
        assert_eq!(m.vertex_count(), 4);
    }

    #[test]
    fn truncated_binary_reads_whole_triangles() {
        let mut data = binary(b"x", &tetra());
        data.truncate(data.len() - 60);
        // Count says 4 but only 2 whole triangles remain: not enough for the
        // "at least count" fallback, so this is an error.
        assert!(parse(&data).is_err());
        // A count lower than the data present is truncated to the count.
        let mut data = binary(b"x", &tetra());
        data[80] = 2;
        let m = parse(&data).unwrap();
        assert_eq!(m.triangle_count(), 2);
    }

    #[test]
    fn ascii_cube_face() {
        let src = "solid face\n facet normal 0 0 1\n  outer loop\n   vertex 0 0 1\n   vertex 1 0 1\n   vertex 1 1 1\n  endloop\n endfacet\n\
                   FACET NORMAL 0 0 1\n  OUTER LOOP\n   VERTEX 0 0 1\n   VERTEX 1 1 1\n   VERTEX 0 1 1e0\n  ENDLOOP\n ENDFACET\nendsolid face\n";
        let m = parse(src.as_bytes()).unwrap();
        assert_eq!(m.meshes.len(), 1);
        assert_eq!(m.meshes[0].name, "face");
        assert_eq!(m.triangle_count(), 2);
        assert_eq!(m.vertex_count(), 4);
    }

    #[test]
    fn ascii_two_solids() {
        let tri = |n: &str| format!("solid {n}\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid {n}\n");
        let m = parse(format!("{}{}", tri("a"), tri("b")).as_bytes()).unwrap();
        assert_eq!(m.meshes.len(), 2);
        assert_eq!(m.meshes[0].name, "a");
        assert_eq!(m.meshes[1].name, "b");
    }

    #[test]
    fn ascii_polygon_fans() {
        let src = "solid q\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 1 1 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid q\n";
        let m = parse(src.as_bytes()).unwrap();
        assert_eq!(m.triangle_count(), 2);
        assert_eq!(m.vertex_count(), 4);
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(parse(b"hello world").is_err());
        assert!(parse(&[7u8; 200]).is_err());
        assert!(parse(b"").is_err());
    }
}
