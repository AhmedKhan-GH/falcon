//! Writing a model back out, for the viewer's Export: Wavefront OBJ (with
//! an MTL of the material colours) and binary STL. Only what falcon holds
//! is written: positions, normals where the file had them, triangles and
//! material colours.

use super::math::{self, V3};
use super::{Mesh, geom};

/// Formats the viewer exports to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Obj,
    Stl,
}

impl Format {
    pub fn ext(self) -> &'static str {
        match self {
            Format::Obj => "obj",
            Format::Stl => "stl",
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

/// Writes `meshes` to `path` in `format`; an OBJ gets its MTL next to it
/// (same name, `.mtl`) when it has materials.
pub fn write<'a>(meshes: impl IntoIterator<Item = &'a Mesh>, format: Format, path: &std::path::Path) -> std::io::Result<()> {
    match format {
        Format::Stl => std::fs::write(path, stl(meshes)),
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
