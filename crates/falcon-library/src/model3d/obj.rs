//! Wavefront OBJ, with colours and diffuse textures from its MTL library.
//!
//! A material's `map_Kd` picture is looked up next to its MTL file, then
//! next to the model (never outside the model's folder); its `-o` and `-s`
//! options are baked into the texture coordinates (`-clamp` is ignored,
//! textures always repeat). The picture replaces the `Kd` colour where it is drawn, as in
//! Eagle's viewer.
//!
//! `map_bump` (or `bump`) is read the same way into the surface's bump map;
//! its `-bm` strength is ignored, as Eagle's viewer ignores it. The
//! coordinates take their `-o` and `-s` from `map_Kd`, else from the bump
//! map. A material whose `d` or `Tr` is not 1 draws see-through. `map_Ke`,
//! `Ke`, `norm` and `map_d` are not read, as in Eagle's viewer. Statement
//! names are compared without regard to case.

use std::borrow::Cow;
use std::collections::HashMap;

use super::math::V3;
use super::texture::{Loader, Texture};
use super::{DEFAULT_COLOR, Material, Mesh, Model, Result, Surface};

const NO_NORMAL: u32 = u32::MAX;
const NO_UV: u32 = u32::MAX;

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
    uvs: Vec<[f32; 2]>,
    all_normals: bool,
    any_uv: bool,
    /// The material's texture map: placement and wrap.
    map: Option<MapOpts>,
    lookup: HashMap<(u32, u32, u32), u32>,
}

#[derive(Default)]
struct MtlEntry {
    color: Option<V3>,
    opacity: Option<f32>,
    tr: Option<f32>,
    map: Option<String>,
    bump: Option<String>,
}

/// A material's `map_Kd` and bump statements.
#[derive(Default)]
struct MapSet {
    kd: Option<MapOpts>,
    bump: Option<MapOpts>,
}

/// A `map_Kd` statement: the picture's path (relative to the MTL file's
/// folder `dir`) and its options.
#[derive(Clone, Debug, PartialEq)]
struct MapOpts {
    dir: String,
    path: String,
    offset: [f32; 2],
    scale: [f32; 2],
}

/// Reads a `map_Kd` line's options and file name. Options take a fixed
/// number of words, `-o`, `-s` and `-t` one to three numbers; the rest is
/// the file name (which may hold spaces).
fn map_opts(rest: &str, dir: &str) -> Option<MapOpts> {
    let mut m = MapOpts { dir: dir.to_string(), path: String::new(), offset: [0.0; 2], scale: [1.0; 2] };
    let words: Vec<&str> = rest.split_ascii_whitespace().collect();
    let mut i = 0;
    while i < words.len() && words[i].starts_with('-') {
        let opt = words[i].to_ascii_lowercase();
        i += 1;
        match opt.as_str() {
            "-o" | "-s" | "-t" => {
                let mut v = Vec::new();
                while v.len() < 3 && i < words.len() && let Ok(f) = words[i].parse::<f32>() {
                    v.push(f);
                    i += 1;
                }
                let at = |k: usize, d: f32| v.get(k).copied().filter(|f| f.is_finite()).unwrap_or(d);
                match opt.as_str() {
                    "-o" => m.offset = [at(0, 0.0), at(1, 0.0)],
                    "-s" => m.scale = [at(0, 1.0), at(1, 1.0)],
                    _ => {}
                }
            }
            "-clamp" => i += 1,
            "-mm" => i += 2,
            _ => i += 1,
        }
    }
    // The file name keeps its own spacing.
    let name_start = words.get(i).and_then(|w| rest.find(w))?;
    m.path = rest[name_start..].trim().to_string();
    (!m.path.is_empty()).then_some(m)
}

fn parse_mtl(bytes: &[u8], dir: &str, out: &mut HashMap<String, Material>, maps: &mut HashMap<String, MapSet>) {
    let text = String::from_utf8_lossy(bytes);
    let mut cur: Option<(String, MtlEntry)> = None;
    let flush = |cur: &mut Option<(String, MtlEntry)>, out: &mut HashMap<String, Material>, maps: &mut HashMap<String, MapSet>| {
        if let Some((name, e)) = cur.take() {
            let opacity = e.opacity.or(e.tr.map(|t| 1.0 - t)).unwrap_or(1.0).clamp(0.0, 1.0);
            let color = e.color.unwrap_or(DEFAULT_COLOR);
            if !out.contains_key(&name) {
                let set = MapSet {
                    kd: e.map.as_deref().and_then(|r| map_opts(r, dir)),
                    bump: e.bump.as_deref().and_then(|r| map_opts(r, dir)),
                };
                if set.kd.is_some() || set.bump.is_some() {
                    maps.insert(name.clone(), set);
                }
            }
            out.entry(name.clone()).or_insert(Material { name, color, opacity });
        }
    };
    for line in logical_lines(&text) {
        let line = strip_comment(&line).trim();
        let (key, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let rest = rest.trim();
        match key {
            _ if key.eq_ignore_ascii_case("newmtl") => {
                flush(&mut cur, out, maps);
                cur = Some((rest.to_string(), MtlEntry::default()));
            }
            _ if key.eq_ignore_ascii_case("Kd") => {
                if let Some((_, e)) = cur.as_mut() {
                    let mut it = rest.split_ascii_whitespace();
                    if let Some([r, g, b]) = floats::<3>(&mut it) {
                        e.color = Some([r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0)]);
                    }
                }
            }
            _ if key.eq_ignore_ascii_case("d") => {
                if let (Some((_, e)), Ok(v)) = (cur.as_mut(), rest.split_ascii_whitespace().next().unwrap_or("").parse::<f32>())
                    && v.is_finite()
                {
                    e.opacity = Some(v);
                }
            }
            _ if key.eq_ignore_ascii_case("Tr") => {
                if let (Some((_, e)), Ok(v)) = (cur.as_mut(), rest.split_ascii_whitespace().next().unwrap_or("").parse::<f32>())
                    && v.is_finite()
                {
                    e.tr = Some(v);
                }
            }
            k if k.eq_ignore_ascii_case("map_Kd") => {
                if let Some((_, e)) = cur.as_mut() {
                    e.map = Some(rest.to_string());
                }
            }
            k if k.eq_ignore_ascii_case("map_bump") || k.eq_ignore_ascii_case("bump") => {
                if let Some((_, e)) = cur.as_mut() {
                    e.bump = Some(rest.to_string());
                }
            }
            _ => {}
        }
    }
    flush(&mut cur, out, maps);
}

/// The folder part of a relative path (`""` for a bare name).
fn dir_of(path: &str) -> &str {
    path.rfind(['/', '\\']).map_or("", |i| &path[..i])
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
    let mut texcoords: Vec<[f32; 2]> = Vec::new();
    let mut materials: HashMap<String, Material> = HashMap::new();
    let mut maps: HashMap<String, MapSet> = HashMap::new();
    let mut loader = Loader::new(side);
    let mut textures: HashMap<String, Option<Texture>> = HashMap::new();
    let mut bumps: HashMap<String, Option<Texture>> = HashMap::new();

    let mut obj_name = String::new();
    let mut mat_name: Option<String> = None;
    let mut builders: Vec<Builder> = Vec::new();
    let mut by_key: HashMap<(String, Option<String>), usize> = HashMap::new();
    // Index into `builders` of the current run; None until a face needs one.
    let mut current: Option<usize> = None;
    let mut corners: Vec<(u32, u32, u32)> = Vec::new();

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
            "vt" => {
                let Some(uv) = floats::<2>(&mut it).or_else(|| floats::<1>(&mut line.split_ascii_whitespace().skip(1)).map(|[u]| [u, 0.0])) else { continue };
                texcoords.push(uv);
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
                    parse_mtl(&b, dir_of(rest), &mut materials, &mut maps);
                } else {
                    for name in rest.split_ascii_whitespace() {
                        if let Some(b) = side(name) {
                            parse_mtl(&b, dir_of(name), &mut materials, &mut maps);
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
                    let t = parts.next().filter(|t| !t.is_empty()).and_then(|t| resolve(t, texcoords.len())).unwrap_or(NO_UV);
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
                    corners.push((p, t, n));
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
                            let set = mat_name.as_ref().and_then(|n| maps.get(n));
                            let kd = set.and_then(|s| s.kd.clone());
                            let bump = set.and_then(|s| s.bump.clone());
                            let map = kd.clone().or_else(|| bump.clone());
                            let blend = material.as_ref().is_some_and(|m| (m.opacity - 1.0).abs() > 1e-6);
                            let texture = match (&mat_name, &kd) {
                                (Some(n), Some(m)) => textures
                                    .entry(n.clone())
                                    .or_insert_with(|| {
                                        let image = loader.file_in(&m.dir, &m.path)?;
                                        Some(Texture::new(image))
                                    })
                                    .clone(),
                                _ => None,
                            };
                            let bump_map = match (&mat_name, &bump) {
                                (Some(n), Some(m)) => bumps
                                    .entry(n.clone())
                                    .or_insert_with(|| {
                                        let image = loader.for_map(|l| l.file_in(&m.dir, &m.path))?;
                                        Some(Texture::new(image))
                                    })
                                    .clone(),
                                _ => None,
                            };
                            builders.push(Builder {
                                mesh: Mesh {
                                    name: obj_name.clone(),
                                    material,
                                    texture,
                                    surface: Surface { bump_map, blend, ..Surface::default() },
                                    ..Mesh::default()
                                },
                                colors: Vec::new(),
                                uvs: Vec::new(),
                                all_normals: true,
                                any_uv: false,
                                map,
                                lookup: HashMap::new(),
                            });
                            builders.len() - 1
                        });
                        current = Some(b);
                        b
                    }
                };
                let b = &mut builders[bi];
                let mut idx = |(p, t, n): (u32, u32, u32)| -> u32 {
                    *b.lookup.entry((p, t, n)).or_insert_with(|| {
                        let i = b.mesh.positions.len() as u32;
                        b.mesh.positions.push(positions[p as usize]);
                        b.colors.push(vcolors[p as usize]);
                        if t == NO_UV {
                            b.uvs.push([0.0; 2]);
                        } else {
                            b.any_uv = true;
                            b.uvs.push(texcoords[t as usize]);
                        }
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
            if b.any_uv {
                // Through the map's placement, then v flipped to run down
                // the picture.
                let (o, s) = b.map.as_ref().map_or(([0.0; 2], [1.0; 2]), |m| (m.offset, m.scale));
                b.mesh.uvs = Some(b.uvs.iter().map(|uv| [uv[0] * s[0] + o[0], 1.0 - (uv[1] * s[1] + o[1])]).collect());
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

    const TEXTURED: &[u8] = b"mtllib m.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 0 1\nusemtl wood\nf 1/1 2/2 3/3\n";

    #[test]
    fn map_kd_textures_the_mesh() {
        let png = crate::model3d::texture::tests::png_2x2();
        let side = |n: &str| match n {
            "m.mtl" => Some(b"newmtl wood\nKd 0.5 0.25 0\nmap_Kd -clamp on -s 2 2 1 -o 0.5 0 tex/wood grain.png\n".to_vec()),
            "tex/wood grain.png" => Some(png.clone()),
            _ => None,
        };
        let m = parse(TEXTURED, &side).unwrap();
        let mesh = &m.meshes[0];
        let t = mesh.texture.as_ref().expect("texture decoded");
        assert_eq!((t.image.width, t.image.height), (2, 2));
        assert_eq!(t.tint, [1.0; 3], "the picture replaces Kd");
        // Scaled by 2, offset by 0.5 in u, then v flipped.
        assert_eq!(mesh.uvs.as_deref(), Some(&[[0.5, 1.0], [2.5, 1.0], [0.5, -1.0]][..]));
        assert_eq!(mesh.material.as_ref().unwrap().color, [0.5, 0.25, 0.0]);
    }

    #[test]
    fn map_kd_options_and_names() {
        let m = map_opts("-blendu on -bm 0.5 -mm 0 1 -o 0.1 0.2 -imfchan l my file.png", "").unwrap();
        assert_eq!((m.path.as_str(), m.offset, m.scale), ("my file.png", [0.1, 0.2], [1.0, 1.0]));
        assert_eq!(map_opts("wood.png", "sub").unwrap().dir, "sub");
        assert!(map_opts("-clamp on", "").is_none());
    }

    #[test]
    fn missing_or_outside_textures_keep_the_colour() {
        let asked = std::cell::RefCell::new(Vec::new());
        let png = crate::model3d::texture::tests::png_2x2();
        for map in ["gone.png", "../up/x.png", "/abs/x.png"] {
            let side = |n: &str| {
                asked.borrow_mut().push(n.to_string());
                match n {
                    "m.mtl" => Some(format!("newmtl wood\nKd 0.5 0.25 0\nmap_Kd {map}\n").into_bytes()),
                    "up/x.png" => Some(png.clone()),
                    _ => None,
                }
            };
            let m = parse(TEXTURED, &side).unwrap();
            assert!(m.meshes[0].texture.is_none(), "{map}");
            assert_eq!(m.meshes[0].material.as_ref().unwrap().color, [0.5, 0.25, 0.0]);
            assert!(m.meshes[0].uvs.is_some());
        }
        assert!(asked.borrow().iter().all(|n| !n.contains("..") && !n.starts_with('/')), "{:?}", asked.borrow());
    }

    #[test]
    fn maps_resolve_next_to_their_mtl_first() {
        let png = crate::model3d::texture::tests::png_2x2();
        let src = String::from_utf8_lossy(TEXTURED).replace("mtllib m.mtl", "mtllib mats/m.mtl");
        let side = |n: &str| match n {
            "mats/m.mtl" => Some(b"newmtl wood\nmap_Kd w.png\n".to_vec()),
            "mats/w.png" => Some(png.clone()),
            _ => None,
        };
        assert!(parse(src.as_bytes(), &side).unwrap().meshes[0].texture.is_some());
        // Faces without vt have no coordinates.
        let m = parse(b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n", &none).unwrap();
        assert!(m.meshes[0].uvs.is_none());
    }

    fn bump_model(mtl: &str, with_vt: bool) -> Model {
        let png = crate::model3d::texture::tests::png_2x2();
        let mtl = mtl.to_string();
        let side = move |n: &str| match n {
            "m.mtl" => Some(mtl.clone().into_bytes()),
            "b.png" => Some(png.clone()),
            _ => None,
        };
        let src: &[u8] = if with_vt { TEXTURED } else { b"mtllib m.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nusemtl wood\nf 1 2 3\n" };
        parse(src, &side).unwrap()
    }

    #[test]
    fn map_bump_with_options_and_aliases() {
        for stmt in ["map_bump -bm 0.5 -s 2 2 b.png", "bump b.png", "MAP_BUMP b.png", "Bump -bm 3 b.png"] {
            let m = bump_model(&format!("newmtl wood\n{stmt}\n"), true);
            let mesh = &m.meshes[0];
            assert!(mesh.surface.bump_map.is_some(), "{stmt}");
            assert!(mesh.texture.is_none());
            assert!(mesh.uvs.is_some());
            assert!(!mesh.surface.blend);
        }
        let m = bump_model("newmtl wood\nmap_bump -clamp on b.png\n", true);
        assert!(m.meshes[0].surface.bump_map.is_some());
    }

    #[test]
    fn missing_bump_keeps_the_mesh() {
        let m = bump_model("newmtl wood\nKd 1 0 0\nmap_bump gone.png\n", true);
        assert_eq!(m.meshes.len(), 1);
        assert!(m.meshes[0].surface.bump_map.is_none());
        assert_eq!(m.meshes[0].material.as_ref().unwrap().color, [1.0, 0.0, 0.0]);
        // Without vt coordinates no map survives.
        let m = bump_model("newmtl wood\nmap_bump b.png\n", false);
        assert!(m.meshes[0].surface.bump_map.is_none());
    }

    #[test]
    fn bump_placement_is_used_without_map_kd() {
        let m = bump_model("newmtl wood\nmap_bump -s 2 2 b.png\n", true);
        assert_eq!(m.meshes[0].uvs.as_deref(), Some(&[[0.0, 1.0], [2.0, 1.0], [0.0, -1.0]][..]));
        let m = bump_model("newmtl wood\nmap_bump -s 2 2 b.png\nmap_Kd -s 3 3 b.png\n", true);
        assert_eq!(m.meshes[0].uvs.as_deref(), Some(&[[0.0, 1.0], [3.0, 1.0], [0.0, -2.0]][..]));
    }

    #[test]
    fn opacity_sets_blend() {
        for (stmt, blend) in [("d 0.5", true), ("Tr 0.25", true), ("d 1", false), ("d garbage", false), ("d nan", false), ("Tr inf", false), ("Tr 0", false)] {
            let m = bump_model(&format!("newmtl wood\n{stmt}\n"), true);
            assert_eq!(m.meshes[0].surface.blend, blend, "{stmt}");
        }
    }

    #[test]
    fn names_and_continuations() {
        let m = parse(b"o part\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 \\\n 3 # tail\n", &none).unwrap();
        assert_eq!(m.meshes[0].name, "part");
        assert_eq!(m.meshes[0].triangle_count(), 1);
    }
}
