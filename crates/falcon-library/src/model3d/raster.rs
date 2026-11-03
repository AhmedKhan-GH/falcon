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
//! Textured meshes sample their picture per pixel ([`texture::Texture::sample`],
//! bilinear in linear light) at perspective-correct coordinates; pixels
//! below a texture's alpha cutoff are left out. Normal, bump and emissive
//! maps are read at the same coordinates ([`geom::normal_mapped`],
//! [`geom::bumped`]).
//!
//! See-through meshes are drawn after the rest, farthest first, each one's
//! back faces and then its front faces, and write depth as they go: what
//! Eagle's viewer (three.js) and the app's shader do.
//!
//! Triangles with any vertex nearer than the near plane are skipped rather
//! than clipped: the fitted camera keeps the whole bounding sphere in front
//! of the eye (the near plane is at most `d - 2r`), so nothing is lost.

use image::{Rgba, RgbaImage};

use super::geom;
use super::math::{self, V3};
use super::texture;
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
    draw_scene(&mut target, model.meshes.iter(), &[], &vp, near, dir, DEFAULT_COLOR);
    downsample(&target, &mut out);
    out
}

/// Draws `meshes` as the viewer shows them: a `w` x `h` picture through
/// the camera matrix `vp` (projection times view, clip depth 0..1), lit
/// from `to_eye` (unit, towards the camera), meshes without a colour in
/// `default_color`, on a transparent background. The viewer's snapshot.
/// [`render_view_lines`] adds the viewer's lines.
///
/// The square rasterizer draws a square of the longer side with the
/// picture's window scaled into its middle, which is then cut out.
pub fn render_view<'a>(meshes: impl IntoIterator<Item = &'a Mesh>, w: u32, h: u32, vp: &math::M4, to_eye: V3, default_color: V3) -> RgbaImage {
    render_view_lines(meshes, &[], w, h, vp, to_eye, default_color)
}

/// [`render_view`] with line segments drawn over the faces, each in its
/// colour, about a pixel wide and hidden where a face is in front of them
/// (the edges, grid and axes the viewer shows).
pub fn render_view_lines<'a>(meshes: impl IntoIterator<Item = &'a Mesh>, lines: &[([V3; 2], V3)], w: u32, h: u32, vp: &math::M4, to_eye: V3, default_color: V3) -> RgbaImage {
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
    // Clip w is the distance in front of the eye (1 for orthographic).
    draw_scene(&mut target, meshes, lines, &m, 1e-6, to_eye, default_color);
    let mut square = RgbaImage::new(side, side);
    downsample(&target, &mut square);
    image::imageops::crop_imm(&square, (side - w) / 2, (side - h) / 2, w, h).to_image()
}

/// Which of a mesh's faces to draw.
#[derive(Clone, Copy, PartialEq)]
enum Faces {
    All,
    Back,
    Front,
}

/// Opaque meshes, then lines, then the see-through meshes.
fn draw_scene<'a>(t: &mut Target, meshes: impl IntoIterator<Item = &'a Mesh>, lines: &[([V3; 2], V3)], vp: &math::M4, near: f32, to_eye: V3, default_color: V3) {
    let (blended, opaque): (Vec<&Mesh>, Vec<&Mesh>) = meshes.into_iter().partition(|m| m.surface.blend);
    for mesh in opaque {
        draw_mesh(t, mesh, vp, near, to_eye, default_color, Faces::All);
    }
    for (seg, color) in lines {
        draw_line(t, *seg, vp, near, *color);
    }
    let centers: Vec<V3> = blended.iter().map(|m| geom::center(m).unwrap_or_default()).collect();
    for i in geom::back_to_front(&centers, vp) {
        draw_mesh(t, blended[i], vp, near, to_eye, default_color, Faces::Back);
        draw_mesh(t, blended[i], vp, near, to_eye, default_color, Faces::Front);
    }
}

/// A straight-alpha colour `src` over `dst`.
fn over(src: [u8; 4], dst: [u8; 4]) -> [u8; 4] {
    let (sa, da) = (f32::from(src[3]) / 255.0, f32::from(dst[3]) / 255.0);
    let a = sa + da * (1.0 - sa);
    if a <= 0.0 {
        return [0; 4];
    }
    let mix = |k: usize| ((f32::from(src[k]) * sa + f32::from(dst[k]) * da * (1.0 - sa)) / a).round() as u8;
    [mix(0), mix(1), mix(2), (a * 255.0).round() as u8]
}

/// How far in front of a face (in clip depth) a line on it still shows,
/// as the viewer's GPU depth bias lets its edges show.
const LINE_BIAS: f32 = 2e-4;

/// Draws a segment into `t`, one sample per step along its longer axis and
/// `SS` samples across (about a pixel once filtered down), depth-tested.
/// A segment with an end behind the near plane is skipped.
fn draw_line(t: &mut Target, seg: [V3; 2], vp: &math::M4, near: f32, color: V3) {
    let size = t.size as f32;
    let ends = seg.map(|p| {
        let v = [p[0], p[1], p[2], 1.0];
        let c: [f32; 4] = std::array::from_fn(|r| (0..4).map(|k| vp[k][r] * v[k]).sum());
        (c[3] >= near && c.iter().all(|x| x.is_finite())).then(|| [(c[0] / c[3] * 0.5 + 0.5) * size, (0.5 - c[1] / c[3] * 0.5) * size, c[2] / c[3]])
    });
    let [Some(a), Some(b)] = ends else { return };
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let steps = dx.abs().max(dy.abs()).ceil();
    if !(steps.is_finite() && steps < LIMIT) {
        return;
    }
    let steps = steps.max(1.0) as u32;
    let across_x = dy.abs() > dx.abs();
    let px = pack(color);
    for i in 0..=steps {
        let f = i as f32 / steps as f32;
        let (x, y, z) = (a[0] + dx * f, a[1] + dy * f, a[2] + (b[2] - a[2]) * f);
        for k in 0..SS as i64 {
            let off = k - (SS as i64 - 1) / 2;
            let (sx, sy) = if across_x { (x.floor() as i64 + off, y.floor() as i64) } else { (x.floor() as i64, y.floor() as i64 + off) };
            if sx < 0 || sy < 0 || sx >= t.size as i64 || sy >= t.size as i64 {
                continue;
            }
            let at = (sy * t.size as i64 + sx) as usize;
            if z - LINE_BIAS <= t.depth[at] {
                t.depth[at] = t.depth[at].min(z);
                t.color[at] = px;
            }
        }
    }
}

struct Target {
    size: u32,
    depth: Vec<f32>,
    color: Vec<[u8; 4]>,
}

/// Screen position (pixels, y down), depth (0..1) and 1 / clip w of every
/// vertex, or `None` where it is behind the near plane or not finite.
fn project(mesh: &Mesh, vp: &math::M4, near: f32, size: f32) -> Vec<Option<[f32; 4]>> {
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
            (sx.abs() < LIMIT && sy.abs() < LIMIT).then_some([sx, sy, z, 1.0 / c[3]])
        })
        .collect()
}

fn pack(c: V3) -> [u8; 4] {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    [b(c[0]), b(c[1]), b(c[2]), 255]
}

/// The normal, bump and emissive maps to read, with the coordinates they
/// are read at; `None` for a mesh without coordinates.
struct Maps<'a> {
    uvs: &'a [[f32; 2]],
    normal: Option<&'a texture::Texture>,
    bump: Option<&'a texture::Texture>,
    emissive: Option<&'a texture::Texture>,
}

/// The surface's normal at `uv` through its normal or bump map, on the
/// triangle `i` of `mesh`, whose interpolated normal is `n`.
fn mapped_normal(maps: &Maps, mesh: &Mesh, i: [usize; 3], n: V3, uv: [f32; 2]) -> V3 {
    let edge = |a: usize, b: usize| math::sub(mesh.positions[b], mesh.positions[a]);
    let st = |a: usize, b: usize| [maps.uvs[b][0] - maps.uvs[a][0], maps.uvs[b][1] - maps.uvs[a][1]];
    let (q0, q1, st0, st1) = (edge(i[0], i[1]), edge(i[0], i[2]), st(i[0], i[1]), st(i[0], i[2]));
    if let Some(map) = maps.normal {
        geom::normal_mapped(n, q0, q1, st0, st1, map.sample_data(uv))
    } else if let Some(map) = maps.bump {
        // One pixel of the map either way along each axis.
        let (du, dv) = (1.0 / map.image.width.max(1) as f32, 1.0 / map.image.height.max(1) as f32);
        let h = |u: f32, v: f32| map.sample_data([u, v])[0];
        let dh = [(h(uv[0] + du, uv[1]) - h(uv[0] - du, uv[1])) / (2.0 * du), (h(uv[0], uv[1] + dv) - h(uv[0], uv[1] - dv)) / (2.0 * dv)];
        geom::bumped(n, q0, q1, st0, st1, dh)
    } else {
        n
    }
}

fn draw_mesh(t: &mut Target, mesh: &Mesh, vp: &math::M4, near: f32, to_eye: V3, default_color: V3, faces: Faces) {
    let verts = project(mesh, vp, near, t.size as f32);
    let base = mesh.material.as_ref().map_or(default_color, |m| m.color);
    let colors = mesh.colors.as_deref();
    let normals = mesh.normals.as_deref();
    let textured = mesh.textured();
    let maps = mesh.uvs.as_deref().map(|uvs| Maps { uvs, normal: mesh.surface.normal_map.as_ref(), bump: mesh.surface.bump_map.as_ref(), emissive: mesh.surface.emissive_map.as_ref() });
    let maps = maps.filter(|m| m.normal.is_some() || m.bump.is_some() || m.emissive.is_some());
    let emissive = mesh.surface.emissive;
    let (blend, opacity) = (mesh.surface.blend, mesh.opacity());
    let size = t.size as i64;
    for tri in mesh.indices.as_chunks::<3>().0 {
        let idx = tri.map(|i| i as usize);
        let (Some(a), Some(b), Some(c)) = (verts[idx[0]], verts[idx[1]], verts[idx[2]]) else { continue };
        let fx = |p: [f32; 4]| [(p[0] * FRAC as f32).round() as i64, (p[1] * FRAC as f32).round() as i64];
        let (p0, mut p1, mut p2) = (fx(a), fx(b), fx(c));
        let (z0, mut z1, mut z2) = (a[2], b[2], c[2]);
        let (q0, mut q1, mut q2) = (a[3], b[3], c[3]);
        let (i0, mut i1, mut i2) = (idx[0], idx[1], idx[2]);
        let mut area = (p1[0] - p0[0]) * (p2[1] - p0[1]) - (p1[1] - p0[1]) * (p2[0] - p0[0]);
        if area == 0 {
            continue;
        }
        // With y down, a triangle counter-clockwise to the eye has a
        // negative area here.
        let front = area < 0;
        if (faces == Faces::Back && front) || (faces == Faces::Front && !front) {
            continue;
        }
        if area < 0 {
            std::mem::swap(&mut p1, &mut p2);
            std::mem::swap(&mut z1, &mut z2);
            std::mem::swap(&mut q1, &mut q2);
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
        let per_pixel = colors.is_some() || normals.is_some() || textured.is_some() || maps.is_some() || blend || emissive != [0.0; 3];
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
                        let px = if per_pixel {
                            let mix = |f: &dyn Fn(usize) -> V3| {
                                let (u, v, q) = (f(i0), f(i1), f(i2));
                                std::array::from_fn(|c| w[0] * u[c] + w[1] * v[c] + w[2] * q[c])
                            };
                            let col = match textured {
                                // The picture, times the vertex colours
                                // and the tint, in linear light.
                                Some((tex, uvs)) => {
                                    let uv = perspective_uv(w, [q0, q1, q2], [uvs[i0], uvs[i1], uvs[i2]]);
                                    let texel = tex.sample(uv);
                                    let alpha = texel[3] * opacity;
                                    if tex.alpha_cutoff.is_some_and(|cut| alpha < cut) {
                                        None
                                    } else {
                                        let tint = colors.map_or(tex.tint, |cs| texture::modulate(mix(&|i| cs[i]), [tex.tint[0], tex.tint[1], tex.tint[2], 1.0]));
                                        Some((texture::modulate(tint, texel), if blend { alpha } else { 1.0 }))
                                    }
                                }
                                None => Some((colors.map_or(base, |cs| mix(&|i| cs[i])), opacity)),
                            };
                            col.map(|(col, alpha)| {
                                let mut n = match normals {
                                    Some(ns) => math::normalize(mix(&|i| ns[i])),
                                    None => geom::face_normal(mesh.positions[i0], mesh.positions[i1], mesh.positions[i2]),
                                };
                                let mut light = emissive;
                                if let Some(m) = &maps {
                                    let uv = perspective_uv(w, [q0, q1, q2], [m.uvs[i0], m.uvs[i1], m.uvs[i2]]);
                                    if m.normal.is_some() && !front {
                                        n = math::scale(n, -1.0);
                                    }
                                    n = mapped_normal(m, mesh, [i0, i1, i2], n, uv);
                                    if let Some(e) = m.emissive {
                                        let px = e.sample_data(uv);
                                        light = std::array::from_fn(|k| light[k] * px[k]);
                                    }
                                }
                                let mut px = pack(geom::add_light(geom::shade(col, n, to_eye), light));
                                px[3] = (alpha * 255.0).round() as u8;
                                px
                            })
                        } else {
                            Some(flat)
                        };
                        if let Some(px) = px {
                            t.depth[k] = z;
                            t.color[k] = if blend { over(px, t.color[k]) } else { px };
                        }
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

/// Texture coordinates at screen weights `w` of a triangle whose corners
/// have 1 / clip w `q` and coordinates `uv`: interpolated as uv / w and
/// 1 / w, so they stay straight on the surface under perspective.
fn perspective_uv(w: [f32; 3], q: [f32; 3], uv: [[f32; 2]; 3]) -> [f32; 2] {
    let d = w[0] * q[0] + w[1] * q[1] + w[2] * q[2];
    if !(d.is_finite() && d != 0.0) {
        return uv[0];
    }
    std::array::from_fn(|k| (w[0] * q[0] * uv[0][k] + w[1] * q[1] * uv[1][k] + w[2] * q[2] * uv[2][k]) / d)
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

    /// The split cube with each face's quad mapped over the whole of
    /// `image`.
    fn textured_cube(image: texture::Image) -> Model {
        let mut m = cube(true);
        let quad = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        m.uvs = Some((0..36).map(|i| quad[i % 6]).collect());
        m.texture = Some(texture::Texture::new(std::sync::Arc::new(image)));
        m.material = Some(Material { name: "grey".into(), color: [0.5; 3], opacity: 1.0 });
        Model { meshes: vec![m] }
    }

    fn solid(w: u32, h: u32, rgba: [u8; 4]) -> texture::Image {
        texture::Image { name: "t".into(), width: w, height: h, rgba: rgba.repeat((w * h) as usize) }
    }

    #[test]
    fn textured_cube_shows_the_texture_colour() {
        let img = render(&textured_cube(solid(4, 4, [0, 0, 255, 255])), 64);
        let p = img.get_pixel(32, 32);
        assert!(p[2] > 100 && p[0] == 0 && p[1] == 0, "the picture replaces the grey material: {p:?}");
        // Without coordinates the material colour is drawn.
        let mut m = textured_cube(solid(4, 4, [0, 0, 255, 255]));
        m.meshes[0].uvs = None;
        let p = *render(&m, 64).get_pixel(32, 32);
        assert!(p[0] == p[1] && p[1] == p[2] && p[0] > 50, "{p:?}");
    }

    #[test]
    fn both_halves_of_a_picture_show() {
        // Left half red, right half green on every face.
        let mut px = Vec::new();
        for _y in 0..8 {
            for x in 0..8 {
                px.extend_from_slice(&if x < 4 { [255, 0, 0, 255] } else { [0, 255, 0, 255] });
            }
        }
        let img = render(&textured_cube(texture::Image { name: "h".into(), width: 8, height: 8, rgba: px }), 128);
        let red = img.pixels().filter(|p| p[3] == 255 && p[0] > 80 && p[1] < 20).count();
        let green = img.pixels().filter(|p| p[3] == 255 && p[1] > 80 && p[0] < 20).count();
        assert!(red > 500 && green > 500, "{red} red, {green} green");
    }

    #[test]
    fn cutout_pixels_are_left_out() {
        let mut m = textured_cube(solid(2, 2, [255, 255, 255, 10]));
        m.meshes[0].texture.as_mut().unwrap().alpha_cutoff = Some(0.5);
        assert!(render(&m, 32).pixels().all(|p| p[3] == 0), "everything below the cutoff");
        // Without a cutoff the picture is opaque.
        m.meshes[0].texture.as_mut().unwrap().alpha_cutoff = None;
        assert_eq!(render(&m, 32).get_pixel(16, 16)[3], 255);
    }

    /// A square facing +z, the whole of a picture on it, seen head on, with
    /// the light at the eye.
    fn facing_square() -> Mesh {
        Mesh {
            positions: vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0], [-1.0, 1.0, 0.0]],
            indices: vec![0, 1, 2, 0, 2, 3],
            uvs: Some(vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]),
            material: Some(Material { name: "m".into(), color: [0.5; 3], opacity: 1.0 }),
            ..Default::default()
        }
    }

    fn head_on(meshes: &[Mesh]) -> RgbaImage {
        let view = math::look_at([0.0, 0.0, 4.0], [0.0; 3], [0.0, 1.0, 0.0]);
        let vp = math::mul(&math::perspective(FOV_DEG.to_radians(), 1.0, 0.1, 100.0), &view);
        render_view(meshes, 32, 32, &vp, [0.0, 0.0, 1.0], [1.0; 3])
    }

    #[test]
    fn normal_maps_change_the_shading_and_flat_ones_do_not() {
        let plain = *head_on(&[facing_square()]).get_pixel(16, 16);
        let with = |rgb: [u8; 4]| {
            let mut m = facing_square();
            m.surface.normal_map = Some(texture::Texture::new(std::sync::Arc::new(solid(2, 2, rgb))));
            *head_on(&[m]).get_pixel(16, 16)
        };
        assert!(with([128, 128, 255, 255])[0].abs_diff(plain[0]) <= 1, "flat");
        let tilted = with([255, 128, 128, 255]);
        assert!(tilted[0] < plain[0] - 20, "turned away from the light: {tilted:?} against {plain:?}");
        // A back face takes the map too, without going black.
        let mut back = facing_square();
        back.indices = vec![0, 2, 1, 0, 3, 2];
        back.surface.normal_map = Some(texture::Texture::new(std::sync::Arc::new(solid(2, 2, [128, 128, 255, 255]))));
        assert!(head_on(&[back]).get_pixel(16, 16)[0].abs_diff(plain[0]) <= 1);
    }

    #[test]
    fn bump_maps_show_their_slopes_and_normal_maps_win() {
        // A ramp of height across the picture, steep enough to see.
        let px: Vec<u8> = (0..64).flat_map(|x| [(x * 4) as u8, 0, 0, 255]).collect();
        let ramp = std::sync::Arc::new(texture::Image { name: "h".into(), width: 64, height: 1, rgba: px });
        let plain = *head_on(&[facing_square()]).get_pixel(16, 16);
        let mut m = facing_square();
        m.surface.bump_map = Some(texture::Texture::new(ramp.clone()));
        let bumped = *head_on(std::slice::from_ref(&m)).get_pixel(16, 16);
        assert!(bumped[0] < plain[0] - 3, "{bumped:?} against {plain:?}");
        m.surface.normal_map = Some(texture::Texture::new(std::sync::Arc::new(solid(2, 2, [128, 128, 255, 255]))));
        assert!(head_on(&[m]).get_pixel(16, 16)[0].abs_diff(plain[0]) <= 1, "the normal map is used instead");
    }

    #[test]
    fn own_light_shows_through_its_map() {
        let mut m = facing_square();
        m.material.as_mut().unwrap().color = [0.0; 3];
        let dark = *head_on(std::slice::from_ref(&m)).get_pixel(16, 16);
        m.surface.emissive = [1.0, 0.0, 0.0];
        let lit = *head_on(std::slice::from_ref(&m)).get_pixel(16, 16);
        assert!(lit[0] == 255 && lit[1] == dark[1], "{lit:?}");
        // The map multiplies it, and a black factor shows none of the map.
        m.surface.emissive_map = Some(texture::Texture::new(std::sync::Arc::new(solid(2, 2, [51, 255, 255, 255]))));
        let lit = *head_on(std::slice::from_ref(&m)).get_pixel(16, 16);
        assert!(lit[0].abs_diff(dark[0] + 51) <= 1, "{lit:?} over {dark:?}");
        m.surface.emissive = [0.0; 3];
        assert_eq!(*head_on(&[m]).get_pixel(16, 16), dark);
    }

    #[test]
    fn see_through_meshes_blend_over_what_is_behind() {
        let mut far = facing_square();
        far.material.as_mut().unwrap().color = [1.0, 0.0, 0.0];
        let behind = |m: &Mesh| {
            let mut m = m.clone();
            m.positions.iter_mut().for_each(|p| p[2] = -1.0);
            m
        };
        let far = behind(&far);
        let mut near = facing_square();
        near.material = Some(Material { name: "g".into(), color: [0.0, 1.0, 0.0], opacity: 0.5 });
        // Without the flag the opacity is ignored, as in Eagle.
        let p = *head_on(&[near.clone(), far.clone()]).get_pixel(16, 16);
        assert!(p[1] > 100 && p[0] < 20 && p[3] == 255, "{p:?}");
        near.surface.blend = true;
        // Whatever order the file has them in, the far one is drawn first.
        for scene in [[near.clone(), far.clone()], [far.clone(), near.clone()]] {
            let p = *head_on(&scene).get_pixel(16, 16);
            assert!(p[0] > 50 && p[1] > 50 && p[3] == 255, "both show: {p:?}");
        }
        // Alone it leaves the background partly clear.
        let p = *head_on(std::slice::from_ref(&near)).get_pixel(16, 16);
        assert!(p[3].abs_diff(128) <= 1 && p[1] > 100, "{p:?}");
        // The picture's alpha counts too, and a cutoff still removes pixels.
        near.material.as_mut().unwrap().opacity = 1.0;
        near.texture = Some(texture::Texture::new(std::sync::Arc::new(solid(2, 2, [0, 255, 0, 64]))));
        assert!(head_on(std::slice::from_ref(&near)).get_pixel(16, 16)[3].abs_diff(64) <= 1);
        near.texture.as_mut().unwrap().alpha_cutoff = Some(0.5);
        assert_eq!(head_on(&[near]).get_pixel(16, 16)[3], 0);
    }

    /// The back wall of a closed see-through box shows through its front:
    /// back faces are drawn first even though they come later in the file.
    #[test]
    fn a_closed_see_through_box_shows_both_walls() {
        let mut m = cube(false);
        m.material = Some(Material { name: "g".into(), color: [0.0, 1.0, 0.0], opacity: 0.5 });
        m.surface.blend = true;
        let p = *render(&Model { meshes: vec![m] }, 64).get_pixel(32, 32);
        assert!(p[3] > 180 && p[3] < 200, "two layers of a half: {p:?}");
    }

    #[test]
    fn maps_without_coordinates_are_ignored() {
        let mut m = facing_square();
        let plain = *head_on(std::slice::from_ref(&m)).get_pixel(16, 16);
        m.uvs = None;
        m.surface.normal_map = Some(texture::Texture::new(std::sync::Arc::new(solid(2, 2, [255, 128, 128, 255]))));
        assert_eq!(*head_on(std::slice::from_ref(&m)).get_pixel(16, 16), plain);
        let mut sane = m.clone();
        sane.sanitize();
        assert!(sane.surface.normal_map.is_none());
    }

    #[test]
    fn coordinates_are_perspective_correct() {
        // Halfway across the screen between a near corner (w = 1) and a
        // far one (w = 3), the surface point is a quarter of the way.
        let uv = perspective_uv([0.5, 0.5, 0.0], [1.0, 1.0 / 3.0, 1.0], [[0.0, 0.0], [1.0, 0.0], [9.0, 9.0]]);
        assert!((uv[0] - 0.25).abs() < 1e-6 && uv[1] == 0.0, "{uv:?}");
        // Equal depths interpolate linearly.
        assert_eq!(perspective_uv([0.25, 0.75, 0.0], [0.5; 3], [[0.0, 0.0], [1.0, 1.0], [0.0, 0.0]]), [0.75, 0.75]);
    }

    #[test]
    fn import_thumbnail_of_a_textured_obj() {
        let dir = tempfile::tempdir().unwrap();
        let obj = "mtllib c.mtl\nusemtl t\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\nf 1/1 2/2 3/3 4/4\n";
        std::fs::write(dir.path().join("c.obj"), obj).unwrap();
        std::fs::write(dir.path().join("c.mtl"), "newmtl t\nKd 1 1 1\nmap_Kd tex/c.png\n").unwrap();
        std::fs::create_dir(dir.path().join("tex")).unwrap();
        std::fs::write(dir.path().join("tex/c.png"), texture::tests::png(4, 4, [255, 128, 0, 255])).unwrap();
        let img = thumbnail(&dir.path().join("c.obj"), "obj").unwrap();
        let (w, h) = img.dimensions();
        let p = img.get_pixel(w / 2, h / 2);
        assert!(p[3] == 255 && p[0] > p[1] && p[1] > p[2] && p[2] < 20, "orange from the picture: {p:?}");
        // Without the picture, the white Kd.
        std::fs::remove_file(dir.path().join("tex/c.png")).unwrap();
        let img = thumbnail(&dir.path().join("c.obj"), "obj").unwrap();
        let p = img.get_pixel(img.width() / 2, img.height() / 2);
        assert!(p[0] == p[1] && p[1] == p[2], "{p:?}");
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
        let img = render(&cube_model(), CANVAS);
        let out = trim_and_pad(&img, PADDING).unwrap();
        assert!(out.width() > 2 * PADDING && out.width() <= CANVAS + 2 * PADDING);
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

    #[test]
    fn lines_draw_over_faces_and_hide_behind_them() {
        let eye = [0.5, 0.5, 4.0];
        let view = math::look_at(eye, [0.5, 0.5, 0.5], [0.0, 1.0, 0.0]);
        let proj = math::perspective(FOV_DEG.to_radians(), 1.0, 0.1, 10.0);
        let vp = math::mul(&proj, &view);
        let model = cube_model();
        // Green across the front face; blue across the middle, behind the cube.
        let lines = [([[0.0, 0.5, 1.0], [1.0, 0.5, 1.0]], [0.0, 1.0, 0.0]), ([[-3.0, 0.3, -0.5], [4.0, 0.3, -0.5]], [0.0, 0.0, 1.0])];
        let img = render_view_lines(&model.meshes, &lines, 80, 80, &vp, [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
        let mid = img.get_pixel(40, 40);
        assert!(mid[1] > 100 && mid[0] < 100, "the front line shows on the face: {mid:?}");
        let row = (0..80).find(|&y| img.get_pixel(2, y)[2] > 100).expect("the back line shows beside the cube");
        assert!(img.get_pixel(40, row)[2] < 100, "and is hidden behind it");
        assert!(img.get_pixel(40, row)[0] > 100);
        // Without lines it is the plain view.
        assert_eq!(render_view_lines(&model.meshes, &[], 80, 80, &vp, [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]), render_view(&model.meshes, 80, 80, &vp, [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]));
    }
}
