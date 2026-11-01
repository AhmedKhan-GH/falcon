//! A software rasterizer for 3D model thumbnails, as Eagle makes them at
//! import: the model is drawn in the viewer's home view on a 1080 square
//! with a transparent background, the empty border is cropped away and 40
//! transparent pixels are added on every side.
//!
//! The view is the viewer's: Y up, the eye in direction (-1.5, 2, 3) from the
//! centre of the bounding sphere, 45 degrees of vertical field of view, at
//! the distance where the sphere fills the view. Lighting is
//! [`geom::shade`] with the light at the eye.
//!
//! Triangles are filled with fixed-point edge functions (a consistent
//! tie-break rule, so shared edges leave neither cracks nor double hits)
//! into a 2x supersampled z-buffer, which is then box-filtered down with
//! premultiplied alpha so the silhouette is antialiased.
//!
//! Triangles with any vertex nearer than the near plane are skipped rather
//! than clipped: the fitted camera keeps the whole bounding sphere in front
//! of the eye (the near plane is at most `d - 2r`), so nothing is lost.

use image::{Rgba, RgbaImage};

use super::geom;
use super::math::{self, V3};
use super::{DEFAULT_COLOR, Mesh, Model};

/// Side of the square Eagle renders models on.
pub const CANVAS: u32 = 1080;
/// Transparent border added around the cropped render.
pub const PADDING: u32 = 40;

const FOV_DEG: f32 = 45.0;
const HOME_DIRECTION: V3 = [-1.5, 2.0, 3.0];
/// Samples per pixel side.
const SS: u32 = 2;
/// Sub-pixel precision of the fixed-point coordinates.
const FRAC: i64 = 256;
/// Triangles reaching further than this many pixels off the canvas are
/// skipped (they only occur for cameras inside the model).
const LIMIT: f32 = 1.0e6;

/// Draws `model` on a transparent `size` x `size` image.
pub fn render(model: &Model, size: u32) -> RgbaImage {
    let size = size.max(1);
    let mut out = RgbaImage::new(size, size);
    let Some((center, radius)) = model.bounding_sphere() else { return out };
    if !(radius.is_finite() && radius > 0.0) {
        return out;
    }
    let s = size * SS;
    let dir = math::normalize(HOME_DIRECTION);
    let dist = radius / (FOV_DEG / 2.0).to_radians().sin();
    let eye = math::add(center, math::scale(dir, dist));
    let near = (dist - 2.0 * radius).max(dist * 0.001);
    let far = dist + 2.0 * radius;
    let view = math::look_at(eye, center, [0.0, 1.0, 0.0]);
    let proj = math::perspective(FOV_DEG.to_radians(), 1.0, near, far);
    let vp = math::mul(&proj, &view);

    let mut target = Target { size: s, depth: vec![f32::INFINITY; (s * s) as usize], color: vec![[0; 4]; (s * s) as usize] };
    for mesh in &model.meshes {
        draw_mesh(&mut target, mesh, &vp, near, dir, DEFAULT_COLOR);
    }
    downsample(&target, &mut out);
    out
}

/// Draws `meshes` as the viewer shows them: a `w` x `h` picture through
/// the camera matrix `vp` (projection times view, clip depth 0..1), lit
/// from `to_eye` (unit, towards the camera), meshes without a colour in
/// `default_color`, on a transparent background. The viewer's snapshot.
///
/// The square rasterizer draws a square of the longer side with the
/// picture's window scaled into its middle, which is then cut out.
pub fn render_view<'a>(meshes: impl IntoIterator<Item = &'a Mesh>, w: u32, h: u32, vp: &math::M4, to_eye: V3, default_color: V3) -> RgbaImage {
    let (w, h) = (w.max(1), h.max(1));
    let side = w.max(h);
    let (sx, sy) = (w as f32 / side as f32, h as f32 / side as f32);
    let mut m = *vp;
    for col in &mut m {
        col[0] *= sx;
        col[1] *= sy;
    }
    let s = side * SS;
    let mut target = Target { size: s, depth: vec![f32::INFINITY; (s * s) as usize], color: vec![[0; 4]; (s * s) as usize] };
    for mesh in meshes {
        // Clip w is the distance in front of the eye (1 for orthographic).
        draw_mesh(&mut target, mesh, &m, 1e-6, to_eye, default_color);
    }
    let mut square = RgbaImage::new(side, side);
    downsample(&target, &mut square);
    image::imageops::crop_imm(&square, (side - w) / 2, (side - h) / 2, w, h).to_image()
}

struct Target {
    size: u32,
    depth: Vec<f32>,
    color: Vec<[u8; 4]>,
}

/// Screen position (pixels, y down) and depth (0..1) of every vertex, or
/// `None` where it is behind the near plane or not finite.
fn project(mesh: &Mesh, vp: &math::M4, near: f32, size: f32) -> Vec<Option<[f32; 3]>> {
    mesh.positions
        .iter()
        .map(|p| {
            let v = [p[0], p[1], p[2], 1.0];
            let c: [f32; 4] = std::array::from_fn(|r| (0..4).map(|k| vp[k][r] * v[k]).sum());
            // w is the distance in front of the eye.
            if c[3] < near || !c.iter().all(|x| x.is_finite()) {
                return None;
            }
            let (x, y, z) = (c[0] / c[3], c[1] / c[3], c[2] / c[3]);
            let (sx, sy) = ((x * 0.5 + 0.5) * size, (0.5 - y * 0.5) * size);
            (sx.abs() < LIMIT && sy.abs() < LIMIT).then_some([sx, sy, z])
        })
        .collect()
}

fn pack(c: V3) -> [u8; 4] {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    [b(c[0]), b(c[1]), b(c[2]), 255]
}

fn draw_mesh(t: &mut Target, mesh: &Mesh, vp: &math::M4, near: f32, to_eye: V3, default_color: V3) {
    let verts = project(mesh, vp, near, t.size as f32);
    let base = mesh.material.as_ref().map_or(default_color, |m| m.color);
    let colors = mesh.colors.as_deref();
    let normals = mesh.normals.as_deref();
    let size = t.size as i64;
    for tri in mesh.indices.as_chunks::<3>().0 {
        let idx = tri.map(|i| i as usize);
        let (Some(a), Some(b), Some(c)) = (verts[idx[0]], verts[idx[1]], verts[idx[2]]) else { continue };
        let fx = |p: [f32; 3]| [(p[0] * FRAC as f32).round() as i64, (p[1] * FRAC as f32).round() as i64];
        let (p0, mut p1, mut p2) = (fx(a), fx(b), fx(c));
        let (z0, mut z1, mut z2) = (a[2], b[2], c[2]);
        let (i0, mut i1, mut i2) = (idx[0], idx[1], idx[2]);
        let mut area = (p1[0] - p0[0]) * (p2[1] - p0[1]) - (p1[1] - p0[1]) * (p2[0] - p0[0]);
        if area == 0 {
            continue;
        }
        if area < 0 {
            std::mem::swap(&mut p1, &mut p2);
            std::mem::swap(&mut z1, &mut z2);
            std::mem::swap(&mut i1, &mut i2);
            area = -area;
        }
        let x0 = (p0[0].min(p1[0]).min(p2[0]) / FRAC).max(0);
        let x1 = ((p0[0].max(p1[0]).max(p2[0]) + FRAC - 1) / FRAC).min(size - 1);
        let y0 = (p0[1].min(p1[1]).min(p2[1]) / FRAC).max(0);
        let y1 = ((p0[1].max(p1[1]).max(p2[1]) + FRAC - 1) / FRAC).min(size - 1);
        if x0 > x1 || y0 > y1 {
            continue;
        }
        // Edge i is opposite vertex i: e_i(p) = (b-a) x (p-a), positive
        // inside. Steps per pixel and the tie-break bias per edge.
        let edges = [(p1, p2), (p2, p0), (p0, p1)];
        let step_x: [i64; 3] = edges.map(|(a, b)| -(b[1] - a[1]) * FRAC);
        let step_y: [i64; 3] = edges.map(|(a, b)| (b[0] - a[0]) * FRAC);
        // A point exactly on an edge belongs to the triangle on one side
        // only: the edge owns it when its direction passes this test, which
        // holds for exactly one of the two directions of a shared edge.
        let owns: [bool; 3] = edges.map(|(a, b)| {
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            dy > 0 || (dy == 0 && dx > 0)
        });
        let px0 = x0 * FRAC + FRAC / 2;
        let py0 = y0 * FRAC + FRAC / 2;
        let mut row: [i64; 3] = std::array::from_fn(|i| {
            let (a, b) = edges[i];
            (b[0] - a[0]) * (py0 - a[1]) - (b[1] - a[1]) * (px0 - a[0])
        });
        // Per-pixel attributes, or one colour for the whole face.
        let per_pixel = colors.is_some() || normals.is_some();
        let flat = if per_pixel {
            [0; 4]
        } else {
            let n = geom::face_normal(mesh.positions[i0], mesh.positions[i1], mesh.positions[i2]);
            pack(geom::shade(base, n, to_eye))
        };
        let inv_area = 1.0 / area as f32;
        for y in y0..=y1 {
            let mut e = row;
            let line = (y * size) as usize;
            for x in x0..=x1 {
                let inside = (0..3).all(|i| e[i] > 0 || (e[i] == 0 && owns[i]));
                if inside {
                    let w = [e[0] as f32 * inv_area, e[1] as f32 * inv_area, e[2] as f32 * inv_area];
                    let z = w[0] * z0 + w[1] * z1 + w[2] * z2;
                    let k = line + x as usize;
                    if (0.0..=1.0).contains(&z) && z < t.depth[k] {
                        t.depth[k] = z;
                        t.color[k] = if per_pixel {
                            let mix = |f: &dyn Fn(usize) -> V3| {
                                let (u, v, q) = (f(i0), f(i1), f(i2));
                                std::array::from_fn(|c| w[0] * u[c] + w[1] * v[c] + w[2] * q[c])
                            };
                            let col = colors.map_or(base, |cs| mix(&|i| cs[i]));
                            let n = match normals {
                                Some(ns) => math::normalize(mix(&|i| ns[i])),
                                None => geom::face_normal(mesh.positions[i0], mesh.positions[i1], mesh.positions[i2]),
                            };
                            pack(geom::shade(col, n, to_eye))
                        } else {
                            flat
                        };
                    }
                }
                for i in 0..3 {
                    e[i] += step_x[i];
                }
            }
            for i in 0..3 {
                row[i] += step_y[i];
            }
        }
    }
}

/// Averages each `SS` x `SS` block with premultiplied alpha.
fn downsample(t: &Target, out: &mut RgbaImage) {
    let n = SS * SS;
    for (x, y, px) in out.enumerate_pixels_mut() {
        let (mut a, mut c) = (0u32, [0u32; 3]);
        for dy in 0..SS {
            for dx in 0..SS {
                let s = t.color[((y * SS + dy) * t.size + x * SS + dx) as usize];
                let sa = u32::from(s[3]);
                a += sa;
                for k in 0..3 {
                    c[k] += u32::from(s[k]) * sa;
                }
            }
        }
        if a > 0 {
            let rgb = c.map(|v| (v as f32 / a as f32).round() as u8);
            *px = Rgba([rgb[0], rgb[1], rgb[2], (a as f32 / n as f32).round() as u8]);
        }
    }
}

/// Crops `img` to the bounding box of its pixels with alpha above 0 and adds
/// `pad` transparent pixels on every side; `None` when nothing is visible.
pub fn trim_and_pad(img: &RgbaImage, pad: u32) -> Option<RgbaImage> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (x, y, p) in img.enumerate_pixels() {
        if p[3] > 0 {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    if x0 > x1 {
        return None;
    }
    let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
    let mut out = RgbaImage::new(w + 2 * pad, h + 2 * pad);
    image::imageops::replace(&mut out, &image::imageops::crop_imm(img, x0, y0, w, h).to_image(), i64::from(pad), i64::from(pad));
    Some(out)
}

/// Eagle's import thumbnail of the 3D file at `path`: the render on a
/// [`CANVAS`] square, trimmed and padded by [`PADDING`]. `None` when the
/// file can't be read as a model or shows nothing.
pub fn thumbnail(path: &std::path::Path, ext: &str) -> Option<RgbaImage> {
    let model = super::load(path, ext).ok()?;
    trim_and_pad(&render(&model, CANVAS), PADDING)
}

#[cfg(test)]
mod tests {
    use super::super::Material;
    use super::super::geom::tests::cube;
    use super::*;
    use std::collections::HashMap;

    fn cube_model() -> Model {
        Model { meshes: vec![cube(false)] }
    }

    fn lum(p: &Rgba<u8>) -> u32 {
        u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2])
    }

    #[test]
    fn cube_fills_the_middle_and_leaves_the_corners_clear() {
        let img = render(&cube_model(), 64);
        assert_eq!(img.dimensions(), (64, 64));
        assert_eq!(img.get_pixel(32, 32)[3], 255);
        for (x, y) in [(0, 0), (63, 0), (0, 63), (63, 63)] {
            assert_eq!(img.get_pixel(x, y)[3], 0);
        }
        // Edges are antialiased: some partial alpha exists.
        assert!(img.pixels().any(|p| p[3] > 0 && p[3] < 255));
    }

    #[test]
    fn visible_faces_differ_in_brightness() {
        let img = render(&cube_model(), 128);
        let mut counts: HashMap<[u8; 3], u32> = HashMap::new();
        for p in img.pixels().filter(|p| p[3] == 255) {
            *counts.entry([p[0], p[1], p[2]]).or_default() += 1;
        }
        // Top, front and left faces: three large flat areas.
        let mut big: Vec<_> = counts.into_iter().filter(|&(_, n)| n > 300).collect();
        big.sort();
        assert_eq!(big.len(), 3, "{big:?}");
        // The front (+z) faces the eye most directly of the three.
        let top = img.get_pixel(64, 40);
        let front = img.get_pixel(90, 100);
        assert_ne!(top, front);
    }

    #[test]
    fn material_colour_is_used() {
        let mut m = cube(false);
        m.material = Some(Material { name: "red".into(), color: [1.0, 0.0, 0.0], opacity: 1.0 });
        let img = render(&Model { meshes: vec![m] }, 64);
        let p = img.get_pixel(32, 32);
        assert!(p[0] > 100 && p[1] == 0 && p[2] == 0, "{p:?}");
    }

    #[test]
    fn vertex_colours_are_used() {
        let mut m = cube(false);
        m.colors = Some(vec![[0.0, 1.0, 0.0]; 8]);
        let img = render(&Model { meshes: vec![m] }, 64);
        let p = img.get_pixel(32, 32);
        assert!(p[1] > 100 && p[0] == 0 && p[2] == 0, "{p:?}");
    }

    #[test]
    fn file_normals_change_the_shading() {
        let flat = render(&cube_model(), 64);
        let mut m = cube(false);
        m.normals = Some(vec![[0.0, 1.0, 0.0]; 8]);
        let lit = render(&Model { meshes: vec![m] }, 64);
        assert_ne!(lum(flat.get_pixel(32, 32)), 0);
        // All normals up: one uniform colour over the whole cube.
        let c = *lit.get_pixel(32, 32);
        assert!(lit.pixels().filter(|p| p[3] == 255).all(|p| *p == c));
    }

    #[test]
    fn no_cracks_between_triangles() {
        // No pixel is left clear (or partly covered) with opaque neighbours
        // on all four sides.
        let img = render(&cube_model(), 128);
        let mut holes = 0;
        for y in 1..127 {
            for x in 1..127 {
                let full = |dx: i32, dy: i32| img.get_pixel((x as i32 + dx) as u32, (y as i32 + dy) as u32)[3] == 255;
                if img.get_pixel(x, y)[3] != 255 && full(-1, 0) && full(1, 0) && full(0, -1) && full(0, 1) {
                    holes += 1;
                }
            }
        }
        assert_eq!(holes, 0);
    }

    #[test]
    fn a_flat_square_still_renders() {
        let sq = Mesh {
            positions: vec![[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]],
            indices: vec![0, 1, 2, 0, 2, 3],
            ..Default::default()
        };
        let img = render(&Model { meshes: vec![sq] }, 64);
        assert!(img.pixels().filter(|p| p[3] == 255).count() > 100);
    }

    #[test]
    fn trimming_crops_and_pads() {
        let mut img = RgbaImage::new(10, 8);
        img.put_pixel(3, 2, Rgba([9, 8, 7, 255]));
        img.put_pixel(5, 4, Rgba([1, 2, 3, 10]));
        let out = trim_and_pad(&img, 2).unwrap();
        assert_eq!(out.dimensions(), (3 + 4, 3 + 4));
        assert_eq!(*out.get_pixel(2, 2), Rgba([9, 8, 7, 255]));
        assert_eq!(*out.get_pixel(4, 4), Rgba([1, 2, 3, 10]));
        assert_eq!(out.get_pixel(0, 0)[3], 0);
        assert!(trim_and_pad(&RgbaImage::new(5, 5), 3).is_none());
    }

    #[test]
    fn missing_file_gives_none() {
        assert!(thumbnail(std::path::Path::new("/nonexistent/falcon/none.obj"), "obj").is_none());
    }

    #[test]
    fn canvas_render_is_trimmed_and_padded() {
        let t = std::time::Instant::now();
        let img = render(&cube_model(), CANVAS);
        let out = trim_and_pad(&img, PADDING).unwrap();
        assert!(out.width() > 2 * PADDING && out.width() <= CANVAS + 2 * PADDING);
        assert!(t.elapsed().as_secs_f32() < 5.0);
    }

    #[test]
    fn views_render_at_their_own_shape() {
        let eye = [0.5, 0.5, 4.0];
        let view = math::look_at(eye, [0.5, 0.5, 0.5], [0.0, 1.0, 0.0]);
        let proj = math::perspective(FOV_DEG.to_radians(), 2.0, 0.1, 10.0);
        let vp = math::mul(&proj, &view);
        let model = cube_model();
        let img = render_view(&model.meshes, 80, 40, &vp, [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
        assert_eq!(img.dimensions(), (80, 40));
        let mid = img.get_pixel(40, 20);
        assert_eq!(mid[3], 255);
        assert!(mid[0] > mid[1].saturating_add(150) && mid[1] == mid[2], "the default colour is used: {mid:?}");
        // Seen square-on, the cube is as tall as it is wide on the picture.
        let wide = (0..80).filter(|&x| img.get_pixel(x, 20)[3] > 128).count() as i32;
        let tall = (0..40).filter(|&y| img.get_pixel(40, y)[3] > 128).count() as i32;
        assert!((wide - tall).abs() <= 2, "{wide} x {tall}");
        assert_eq!(img.get_pixel(0, 0)[3], 0);
    }
}
