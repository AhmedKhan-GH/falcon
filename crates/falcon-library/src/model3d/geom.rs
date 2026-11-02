//! Geometry shared by the viewer and the thumbnail renderer: welding
//! duplicate vertices, face and smooth normals, feature edges, the
//! measurements Eagle's viewer lists, and the lighting both draw with.

use std::collections::HashMap;

use super::math::{self, V3};
use super::{Mesh, Model};

/// For each vertex of `positions`, the index of the first vertex at exactly
/// the same place. Formats such as STL repeat a corner for every triangle
/// that uses it; welding lets them share normals and edges.
pub fn weld(positions: &[V3]) -> Vec<u32> {
    let mut first: HashMap<[u32; 3], u32> = HashMap::with_capacity(positions.len());
    positions
        .iter()
        .enumerate()
        .map(|(i, p)| {
            // -0.0 and 0.0 are the same place.
            let key = p.map(|c| if c == 0.0 { 0 } else { c.to_bits() });
            *first.entry(key).or_insert(i as u32)
        })
        .collect()
}

/// The unit normal of the triangle (a, b, c), counter-clockwise front;
/// zero for a degenerate triangle.
pub fn face_normal(a: V3, b: V3, c: V3) -> V3 {
    math::normalize(math::cross(math::sub(b, a), math::sub(c, a)))
}

/// Twice the area of the triangle (a, b, c).
fn double_area(a: V3, b: V3, c: V3) -> f32 {
    math::length(math::cross(math::sub(b, a), math::sub(c, a)))
}

/// One normal per vertex for smooth shading: the file's own normals when it
/// has them, otherwise the area-weighted average of the faces meeting at
/// the vertex's place (welded, so a corner repeated per triangle still
/// shades smoothly).
pub fn smooth_normals(mesh: &Mesh) -> Vec<V3> {
    if let Some(n) = &mesh.normals {
        return n.iter().map(|&v| math::normalize(v)).collect();
    }
    let welded = weld(&mesh.positions);
    let mut acc = vec![[0.0f32; 3]; mesh.positions.len()];
    for t in mesh.indices.as_chunks::<3>().0.iter() {
        let [a, b, c] = t.map(|i| mesh.positions[i as usize]);
        // The unnormalised cross product weighs each face by its area.
        let n = math::cross(math::sub(b, a), math::sub(c, a));
        for &i in t {
            let w = welded[i as usize] as usize;
            acc[w] = math::add(acc[w], n);
        }
    }
    welded.iter().map(|&w| math::normalize(acc[w as usize])).collect()
}

/// The edges to draw for Eagle's "Show Edges": the segments where the
/// faces on either side meet at more than `threshold_deg` degrees, plus
/// the open edges with a face on one side only. Edges inside a flat area
/// (the diagonals of a triangulated quad) are left out.
pub fn feature_edges(mesh: &Mesh, threshold_deg: f32) -> Vec<[V3; 2]> {
    let welded = weld(&mesh.positions);
    let cos_limit = threshold_deg.to_radians().cos();
    // Undirected edge (smaller welded index first) → the normal of the
    // first face seen and whether the edge is to be drawn.
    let mut edges: HashMap<(u32, u32), (V3, u8, bool)> = HashMap::new();
    let mut order: Vec<(u32, u32)> = Vec::new();
    for t in mesh.indices.as_chunks::<3>().0.iter() {
        let w = t.map(|i| welded[i as usize]);
        let [a, b, c] = w.map(|i| mesh.positions[i as usize]);
        let n = face_normal(a, b, c);
        if n == [0.0; 3] {
            continue;
        }
        for (p, q) in [(w[0], w[1]), (w[1], w[2]), (w[2], w[0])] {
            if p == q {
                continue;
            }
            let key = (p.min(q), p.max(q));
            match edges.get_mut(&key) {
                None => {
                    edges.insert(key, (n, 1, false));
                    order.push(key);
                }
                Some(e) => {
                    e.1 = e.1.saturating_add(1);
                    if math::dot(e.0, n) <= cos_limit {
                        e.2 = true;
                    }
                }
            }
        }
    }
    order
        .into_iter()
        .filter(|k| edges[k].1 == 1 || edges[k].2)
        .map(|(p, q)| [mesh.positions[p as usize], mesh.positions[q as usize]])
        .collect()
}

/// Whether every edge of the mesh is shared by exactly two triangles that
/// run along it in opposite directions: a closed, consistently wound
/// surface, the only kind with a volume.
pub fn is_closed(mesh: &Mesh) -> bool {
    let welded = weld(&mesh.positions);
    let mut directed: HashMap<(u32, u32), u32> = HashMap::new();
    for t in mesh.indices.as_chunks::<3>().0.iter() {
        let w = t.map(|i| welded[i as usize]);
        for (p, q) in [(w[0], w[1]), (w[1], w[2]), (w[2], w[0])] {
            *directed.entry((p, q)).or_default() += 1;
        }
    }
    !directed.is_empty() && directed.iter().all(|(&(p, q), &n)| n == 1 && directed.get(&(q, p)) == Some(&1))
}

/// What Eagle's model viewer lists under Details.
#[derive(Debug, Clone, PartialEq)]
pub struct Stats {
    pub vertices: usize,
    pub triangles: usize,
    /// The bounding box's size along X, Y and Z.
    pub size: V3,
    /// The enclosed volume, when every mesh is closed.
    pub volume: Option<f64>,
    /// The total area of the triangles.
    pub surface: f64,
}

pub fn stats(model: &Model) -> Stats {
    let size = model.bounds().map_or([0.0; 3], |(lo, hi)| math::sub(hi, lo));
    let mut surface = 0.0f64;
    let mut volume = 0.0f64;
    let mut closed = !model.meshes.is_empty();
    for m in &model.meshes {
        closed &= is_closed(m);
        for t in m.indices.as_chunks::<3>().0.iter() {
            let [a, b, c] = t.map(|i| m.positions[i as usize]);
            surface += f64::from(double_area(a, b, c)) / 2.0;
            // The signed volume of the tetrahedron from the origin.
            let [a, b, c] = [a, b, c].map(|v| v.map(f64::from));
            volume += (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6.0;
        }
    }
    Stats { vertices: model.vertex_count(), triangles: model.triangle_count(), size, volume: closed.then_some(volume.abs()), surface }
}

/// The lighting of Eagle's viewer: a grey ambient light and a grey
/// directional light shining from the camera, each at 0x88 (0.533), with a
/// faint highlight. Both sides of a face are lit. `n` is the unit normal,
/// `to_eye` the unit direction from the surface to the camera, `base` the
/// surface colour; the result is a display colour in 0..1. The WGSL shader
/// in the app computes the same thing.
pub fn shade(base: V3, n: V3, to_eye: V3) -> V3 {
    let d = math::dot(n, to_eye).abs();
    let spec = SPECULAR * d.powi(SHININESS);
    base.map(|c| (c * (AMBIENT + DIFFUSE * d) + spec).min(1.0))
}

/// The normal `n` turned by a tangent-space normal map's stored values
/// `texel` (0..1, green up in the picture), on a surface where going along
/// `q0` moves the picture coordinates (u right, v down) by `st0` and going
/// along `q1` by `st1`. No tangents are stored: the frame comes from those
/// differences, as in Schüler's "Normal Mapping Without Precomputed
/// Tangents". `n` comes back alone where the coordinates don't change. The
/// shader in the app computes the same from screen derivatives.
pub fn normal_mapped(n: V3, q0: V3, q1: V3, st0: [f32; 2], st1: [f32; 2], texel: V3) -> V3 {
    let (p1, p0) = (math::cross(q1, n), math::cross(n, q0));
    let t = math::add(math::scale(p1, st0[0]), math::scale(p0, st1[0]));
    let b = math::add(math::scale(p1, st0[1]), math::scale(p0, st1[1]));
    let det = math::dot(t, t).max(math::dot(b, b));
    if !(det.is_finite() && det > 0.0) {
        return n;
    }
    let k = det.sqrt().recip();
    let (x, y, z) = (texel[0] * 2.0 - 1.0, texel[1] * 2.0 - 1.0, texel[2] * 2.0 - 1.0);
    // v runs down the picture, the map's green up it.
    let tilted = math::add(math::add(math::scale(t, x * k), math::scale(b, -y * k)), math::scale(n, z));
    let out = math::normalize(tilted);
    if out == [0.0; 3] { n } else { out }
}

/// The normal `n` tilted by a height field that changes by `dh` (per unit
/// of u, per unit of v) in picture coordinates, on a surface as in
/// [`normal_mapped`]: Mikkelsen's surface gradient, heights in the model's
/// units.
pub fn bumped(n: V3, q0: V3, q1: V3, st0: [f32; 2], st1: [f32; 2], dh: [f32; 2]) -> V3 {
    let (p1, p0) = (math::cross(q1, n), math::cross(n, q0));
    let det = math::dot(q0, p1);
    if !(det.is_finite() && det != 0.0) {
        return n;
    }
    // Per unit along the surface: how u and v change, then the height.
    let du = math::add(math::scale(p1, st0[0] / det), math::scale(p0, st1[0] / det));
    let dv = math::add(math::scale(p1, st0[1] / det), math::scale(p0, st1[1] / det));
    let slope = math::add(math::scale(du, dh[0]), math::scale(dv, dh[1]));
    let out = math::normalize(math::sub(n, slope));
    if out == [0.0; 3] || !out.iter().all(|c| c.is_finite()) { n } else { out }
}

/// `shade`'s result with the surface's own light added, in display values
/// as in Eagle's viewer.
pub fn add_light(lit: V3, emissive: V3) -> V3 {
    std::array::from_fn(|k| (lit[k] + emissive[k]).min(1.0))
}

/// The middle of the box around the vertices of `mesh` that triangles use.
pub fn center(mesh: &Mesh) -> Option<V3> {
    let mut b: Option<(V3, V3)> = None;
    for &i in &mesh.indices {
        let p = *mesh.positions.get(i as usize)?;
        if !p.iter().all(|c| c.is_finite()) {
            continue;
        }
        let (lo, hi) = b.get_or_insert((p, p));
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    b.map(|(lo, hi)| math::scale(math::add(lo, hi), 0.5))
}

/// The order to draw see-through parts in: the indices of `centers`,
/// farthest from the camera `vp` (projection times view) first.
pub fn back_to_front(centers: &[V3], vp: &math::M4) -> Vec<usize> {
    let depth = |p: V3| {
        let v = [p[0], p[1], p[2], 1.0];
        let z: f32 = (0..4).map(|k| vp[k][2] * v[k]).sum();
        let w: f32 = (0..4).map(|k| vp[k][3] * v[k]).sum();
        // Clip w is the distance in front of the eye under perspective;
        // behind the eye counts as nearest.
        if w > 0.0 { z / w } else { f32::NEG_INFINITY }
    };
    let keyed: Vec<f32> = centers.iter().map(|&c| depth(c)).collect();
    let mut order: Vec<usize> = (0..centers.len()).collect();
    order.sort_by(|&a, &b| keyed[b].total_cmp(&keyed[a]));
    order
}

/// Ambient and directional light strengths (0x88 / 255).
pub const AMBIENT: f32 = 136.0 / 255.0;
pub const DIFFUSE: f32 = 136.0 / 255.0;
/// The highlight's strength and sharpness.
pub const SPECULAR: f32 = 0.045;
pub const SHININESS: i32 = 30;

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A flat map value leaves the normal alone; tilting towards red (+u)
    /// tips it along +x, towards green (up the picture, -v) along +y.
    #[test]
    fn normal_maps_tilt_along_the_picture() {
        let n = [0.0, 0.0, 1.0];
        // +x is +u, and +y goes up the picture, so v falls.
        let (q0, q1, st0, st1) = ([2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [1.0, 0.0], [0.0, -1.0]);
        let flat = normal_mapped(n, q0, q1, st0, st1, [0.5, 0.5, 1.0]);
        assert!(math::length(math::sub(flat, n)) < 1e-6, "{flat:?}");
        let right = normal_mapped(n, q0, q1, st0, st1, [1.0, 0.5, 0.5]);
        assert!(right[0] > 0.99 && right[1].abs() < 1e-5, "{right:?}");
        let up = normal_mapped(n, q0, q1, st0, st1, [0.5, 1.0, 0.5]);
        assert!(up[1] > 0.99 && up[0].abs() < 1e-5, "{up:?}");
        // The same result whatever two directions the frame is built from.
        let other = normal_mapped(n, [1.0, 1.0, 0.0], [0.0, 3.0, 0.0], [0.5, -0.5], [0.0, -1.5], [1.0, 0.5, 0.5]);
        assert!(math::length(math::sub(other, right)) < 1e-5, "{other:?}");
        // Coordinates that don't change give no frame.
        assert_eq!(normal_mapped(n, q0, q1, [0.0; 2], [0.0; 2], [1.0, 0.5, 0.5]), n);
    }

    /// Height rising to the right leans the surface to the left.
    #[test]
    fn bumps_lean_away_from_rising_height() {
        let n = [0.0, 0.0, 1.0];
        let (q0, q1, st0, st1) = ([2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [1.0, 0.0], [0.0, -1.0]);
        assert_eq!(bumped(n, q0, q1, st0, st1, [0.0, 0.0]), n);
        // u goes 0..1 over 2 units, so dh/du = 2 is a slope of 1: 45 degrees.
        let b = bumped(n, q0, q1, st0, st1, [2.0, 0.0]);
        assert!((b[0] + 0.5f32.sqrt()).abs() < 1e-5 && (b[2] - 0.5f32.sqrt()).abs() < 1e-5, "{b:?}");
        // Height rising down the picture, so falling along +y: leans up.
        let b = bumped(n, q0, q1, st0, st1, [0.0, 2.0]);
        assert!(b[1] > 0.7, "{b:?}");
        // A degenerate frame leaves it alone.
        assert_eq!(bumped(n, [0.0; 3], q1, st0, st1, [1.0, 1.0]), n);
    }

    #[test]
    fn see_through_parts_sort_far_to_near() {
        let view = math::look_at([0.0, 0.0, 5.0], [0.0; 3], [0.0, 1.0, 0.0]);
        let proj = math::perspective(1.0, 1.0, 0.1, 100.0);
        let vp = math::mul(&proj, &view);
        assert_eq!(back_to_front(&[[0.0, 0.0, 1.0], [0.0, 0.0, -3.0], [0.0, 0.0, 9.0], [0.0, 0.0, 0.0]], &vp), [1, 3, 0, 2]);
    }

    #[test]
    fn centers_use_the_vertices_of_triangles() {
        let m = Mesh { positions: vec![[0.0; 3], [2.0, 0.0, 0.0], [0.0, 4.0, 0.0], [100.0; 3]], indices: vec![0, 1, 2], ..Default::default() };
        assert_eq!(center(&m), Some([1.0, 2.0, 0.0]));
        assert_eq!(center(&Mesh::default()), None);
    }

    #[test]
    fn own_light_adds_and_clamps() {
        assert_eq!(add_light([0.25, 0.5, 0.75], [0.25, 0.75, 0.0]), [0.5, 1.0, 0.75]);
    }

    /// A unit cube from (0,0,0) to (1,1,1), wound outward, with its corners
    /// repeated per triangle the way STL stores them when `split` is true.
    pub(crate) fn cube(split: bool) -> Mesh {
        let corners: [V3; 8] =
            [[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.], [0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]];
        let quads = [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [2, 3, 7, 6], [1, 2, 6, 5], [0, 4, 7, 3]];
        let tris: Vec<u32> = quads.iter().flat_map(|q| [q[0], q[1], q[2], q[0], q[2], q[3]]).collect();
        if split {
            Mesh { positions: tris.iter().map(|&i| corners[i as usize]).collect(), indices: (0..36).collect(), ..Default::default() }
        } else {
            Mesh { positions: corners.to_vec(), indices: tris, ..Default::default() }
        }
    }

    #[test]
    fn welding_joins_repeated_corners() {
        let w = weld(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [-0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
        assert_eq!(w, vec![0, 1, 0, 1]);
    }

    #[test]
    fn cube_measures() {
        for split in [false, true] {
            let model = Model { meshes: vec![cube(split)] };
            let s = stats(&model);
            assert_eq!(s.triangles, 12);
            assert_eq!(s.size, [1.0, 1.0, 1.0]);
            assert!((s.surface - 6.0).abs() < 1e-9);
            assert!((s.volume.unwrap() - 1.0).abs() < 1e-9, "closed when split too");
        }
        // Without one face it is open and has no volume.
        let mut open = cube(false);
        open.indices.truncate(30);
        assert_eq!(stats(&Model { meshes: vec![open] }).volume, None);
    }

    #[test]
    fn cube_edges_skip_the_face_diagonals() {
        // 12 cube edges; the 6 diagonals lie in flat faces.
        assert_eq!(feature_edges(&cube(true), 1.0).len(), 12);
        // An open square shows its four sides, not its diagonal.
        let sq = Mesh {
            positions: vec![[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]],
            indices: vec![0, 1, 2, 0, 2, 3],
            ..Default::default()
        };
        assert_eq!(feature_edges(&sq, 1.0).len(), 4);
        // At a 90 degree threshold the cube's right angles still count;
        // past it they don't, leaving nothing.
        assert_eq!(feature_edges(&cube(false), 89.0).len(), 12);
        assert_eq!(feature_edges(&cube(false), 91.0).len(), 0);
    }

    #[test]
    fn smooth_normals_point_out_of_the_corners() {
        let n = smooth_normals(&cube(true));
        // The corner at the origin points away from the cube's centre.
        let k = -1.0 / 3f32.sqrt();
        assert!(n[0].iter().all(|c| (c - k).abs() < 1e-5), "{:?}", n[0]);
        // File normals win.
        let mut m = cube(false);
        m.normals = Some(vec![[0.0, 2.0, 0.0]; 8]);
        assert_eq!(smooth_normals(&m)[3], [0.0, 1.0, 0.0]);
    }

    #[test]
    fn shading_is_two_sided_and_brightest_head_on() {
        let base = super::super::DEFAULT_COLOR;
        let head_on = shade(base, [0.0, 0.0, 1.0], [0.0, 0.0, 1.0]);
        let back = shade(base, [0.0, 0.0, -1.0], [0.0, 0.0, 1.0]);
        let grazing = shade(base, [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        assert_eq!(head_on, back);
        assert!(head_on[0] > grazing[0]);
        // Edge-on, only the ambient light is left.
        assert!((grazing[0] - base[0] * AMBIENT).abs() < 1e-6);
        assert!(head_on[0] <= 1.0);
    }

    #[test]
    fn model_finish_drops_empty_meshes_and_bad_indices() {
        let mut bad = cube(false);
        bad.indices.extend([0, 1, 99]);
        bad.colors = Some(vec![[1.0; 3]; 3]);
        let m = Model { meshes: vec![bad, Mesh::default()] }.finish().unwrap();
        assert_eq!(m.meshes.len(), 1);
        assert_eq!(m.triangle_count(), 12);
        assert_eq!(m.meshes[0].colors, None);
        assert!(Model::default().finish().is_err());
        let (c, r) = m.bounding_sphere().unwrap();
        assert_eq!(c, [0.5, 0.5, 0.5]);
        assert!((r - 0.75f32.sqrt()).abs() < 1e-6);
    }
}
