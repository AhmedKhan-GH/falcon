//! Autodesk 3D Studio `.3ds` files (binary, little-endian chunks).
//!
//! Read: the editor chunk with its named triangle-mesh objects (vertex list,
//! face list, per-material face groups, smoothing groups) and its materials
//! (name, diffuse colour in the 8-bit or float, linear or gamma-corrected
//! forms, transparency, and the diffuse texture map: its file name, scale, offset and rotation; when that image sits next to the model
//! its average colour stands in for the diffuse colour and the mesh keeps
//! the picture as its texture). The mapping coordinates (0x4140) are kept,
//! with the material's map scale, offset and rotation baked in and v
//! flipped into picture space. One mesh is made per (object, material) pair; faces
//! in no material group go to a mesh without a material. With smoothing
//! groups, each triangle gets its own three vertices and normals averaged
//! over the faces that meet at the same place and share a smoothing bit;
//! without them the vertices stay shared and the viewer shades flat.
//!
//! A material with transparency draws see-through. Only the diffuse map and
//! the transparency are read, as in Eagle's viewer: the bump, opacity and
//! other map chunks are skipped, so no mesh gets a bump or normal map.
//!
//! Skipped: the keyframer (animation), lights, cameras, the master scale,
//! and everything else unknown (bump, opacity and other maps among it). The
//! vertices of a 3DS mesh are already stored in world space, so
//! the local axis matrix (0x4160) is parsed and ignored, not applied.
//! Coordinates are kept as stored (Z up).

use std::collections::HashMap;

use super::geom::{self, face_normal};
use super::math::{self, V3};
use super::texture::{Loader, Texture};
use super::{DEFAULT_COLOR, Material, Mesh, Model, ParseError, Result};

const MAIN: u16 = 0x4D4D;
const EDITOR: u16 = 0x3D3D;
const OBJECT: u16 = 0x4000;
const TRI_MESH: u16 = 0x4100;
const VERTICES: u16 = 0x4110;
const FACES: u16 = 0x4120;
const FACE_MATERIAL: u16 = 0x4130;
const TEX_COORDS: u16 = 0x4140;
const SMOOTHING: u16 = 0x4150;
const LOCAL_AXES: u16 = 0x4160;
const MATERIAL: u16 = 0xAFFF;
const MAT_NAME: u16 = 0xA000;
const MAT_DIFFUSE: u16 = 0xA020;
const MAT_TRANSPARENCY: u16 = 0xA050;
const MAT_TEXTURE: u16 = 0xA200;
const MAT_TEXTURE_FILE: u16 = 0xA300;
/// The map's tiling flags, which falcon ignores: textures always repeat.
#[cfg(test)]
const MAP_TILING: u16 = 0xA351;
const MAP_U_SCALE: u16 = 0xA354;
const MAP_V_SCALE: u16 = 0xA356;
const MAP_U_OFFSET: u16 = 0xA358;
const MAP_V_OFFSET: u16 = 0xA35A;
const MAP_ROTATION: u16 = 0xA35C;
const COLOR_FLOAT: u16 = 0x0010;
const COLOR_BYTE: u16 = 0x0011;
const COLOR_BYTE_GAMMA: u16 = 0x0012;
const COLOR_FLOAT_GAMMA: u16 = 0x0013;
const PERCENT_INT: u16 = 0x0030;
const PERCENT_FLOAT: u16 = 0x0031;

/// The sub-chunks of a chunk body: (id, body). Every step moves on by at
/// least the 6-byte header; a bad length ends the walk with an error, and a
/// tail shorter than a header is ignored.
struct Chunks<'a> {
    data: &'a [u8],
    pos: usize,
}

fn chunks(data: &[u8]) -> Chunks<'_> {
    Chunks { data, pos: 0 }
}

impl<'a> Iterator for Chunks<'a> {
    type Item = Result<(u16, &'a [u8])>;

    fn next(&mut self) -> Option<Self::Item> {
        let rest = self.data.get(self.pos..)?;
        if rest.len() < 6 {
            self.pos = self.data.len();
            return None;
        }
        let id = u16::from_le_bytes([rest[0], rest[1]]);
        let len = u32::from_le_bytes([rest[2], rest[3], rest[4], rest[5]]) as usize;
        if len < 6 || len > rest.len() {
            self.pos = self.data.len();
            return Some(Err(ParseError::new("bad chunk length in 3DS file")));
        }
        self.pos += len;
        Some(Ok((id, &rest[6..len])))
    }
}

/// A cursor over a chunk body; every read is bounds-checked.
struct Reader<'a> {
    b: &'a [u8],
}

fn short() -> ParseError {
    ParseError::new("truncated 3DS chunk")
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if n > self.b.len() {
            return Err(short());
        }
        let (head, tail) = self.b.split_at(n);
        self.b = tail;
        Ok(head)
    }

    /// `count` records of `size` bytes, checked against what is left before
    /// anything is allocated for them.
    fn records(&mut self, count: usize, size: usize) -> Result<&'a [u8]> {
        let n = count.checked_mul(size).ok_or_else(short)?;
        self.take(n)
    }

    fn u16(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn f32(&mut self) -> Result<f32> {
        let b = self.take(4)?;
        Ok(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// A NUL-terminated string (Latin-1 in old files; read lossily).
    fn cstr(&mut self) -> Result<String> {
        let end = self.b.iter().position(|&c| c == 0).ok_or_else(short)?;
        let s = String::from_utf8_lossy(&self.b[..end]).into_owned();
        self.b = &self.b[end + 1..];
        Ok(s)
    }

    fn rest(self) -> &'a [u8] {
        self.b
    }
}

fn f32_at(b: &[u8], i: usize) -> f32 {
    f32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

#[derive(Debug, Clone, Default)]
struct Mat {
    name: String,
    color: Option<V3>,
    /// The colour came from a gamma-corrected chunk, which wins over a
    /// linear one.
    gamma: bool,
    opacity: f32,
    /// Diffuse texture file name.
    texture: Option<String>,
    /// How the texture map is laid on.
    map: MapTransform,
    /// The decoded texture, once the picture was found.
    loaded: Option<Texture>,
}

/// A texture map's transform, as the file stores them (v up).
#[derive(Debug, Clone, Copy)]
struct MapTransform {
    scale: [f32; 2],
    offset: [f32; 2],
    /// Degrees.
    rotation: f32,
}

impl Default for MapTransform {
    fn default() -> Self {
        MapTransform { scale: [1.0; 2], offset: [0.0; 2], rotation: 0.0 }
    }
}

impl MapTransform {
    /// A coordinate in the file's v-up space through the map, then v
    /// flipped to picture space. The map is three.js's `texture.repeat`,
    /// `rotation` and `offset` around the centre (0, 0), which Eagle's
    /// viewer sets from these chunks: rotate by the angle, then scale, then
    /// move (three.js composes its UV matrix as translate * scale * rotate).
    /// A scale of 0 (a degenerate map) counts as 1.
    fn apply(&self, uv: [f32; 2]) -> [f32; 2] {
        let (s, c) = self.rotation.to_radians().sin_cos();
        let (u, v) = (c * uv[0] + s * uv[1], -s * uv[0] + c * uv[1]);
        let k = |x: f32| if x == 0.0 { 1.0 } else { x };
        [u * k(self.scale[0]) + self.offset[0], 1.0 - (v * k(self.scale[1]) + self.offset[1])]
    }
}

#[derive(Default)]
struct Object {
    name: String,
    positions: Vec<V3>,
    faces: Vec<[u16; 3]>,
    /// (material name, face indices), in file order.
    groups: Vec<(String, Vec<u16>)>,
    /// One smoothing mask per face, when the file has them.
    smoothing: Option<Vec<u32>>,
    /// Mapping coordinates as stored (v up), one per vertex.
    uvs: Option<Vec<[f32; 2]>>,
}

#[derive(Default)]
struct Scene {
    materials: Vec<Mat>,
    objects: Vec<Object>,
}

/// Reads a 3DS file; `side` fetches a texture image next to it by name.
pub fn parse(bytes: &[u8], side: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Model> {
    let mut top = chunks(bytes);
    let main = match top.next() {
        Some(Ok((MAIN, body))) => body,
        _ => return Err(ParseError::new("not a 3DS file")),
    };
    let mut scene = Scene::default();
    for c in chunks(main) {
        let (id, body) = c?;
        if id == EDITOR {
            read_editor(body, &mut scene)?;
        }
    }
    let mut loader = Loader::new(side);
    for m in &mut scene.materials {
        if let Some(image) = m.texture.as_deref().and_then(|t| loader.file(t)) {
            if let Some(c) = image.average() {
                m.color = Some(c);
            }
            m.loaded = Some(Texture { image, tint: [1.0; 3], alpha_cutoff: None });
        }
    }
    Model { meshes: build(&scene) }.finish()
}

fn read_editor(body: &[u8], scene: &mut Scene) -> Result<()> {
    for c in chunks(body) {
        let (id, body) = c?;
        match id {
            OBJECT => read_object(body, scene)?,
            MATERIAL => scene.materials.push(read_material(body)?),
            _ => {}
        }
    }
    Ok(())
}

fn read_object(body: &[u8], scene: &mut Scene) -> Result<()> {
    let mut r = Reader { b: body };
    let name = r.cstr()?;
    for c in chunks(r.rest()) {
        let (id, body) = c?;
        if id == TRI_MESH {
            scene.objects.push(read_mesh(name.clone(), body)?);
        }
    }
    Ok(())
}

fn read_mesh(name: String, body: &[u8]) -> Result<Object> {
    let mut o = Object { name, ..Object::default() };
    for c in chunks(body) {
        let (id, body) = c?;
        match id {
            VERTICES => o.positions = read_vertices(body)?,
            FACES => read_faces(body, &mut o)?,
            TEX_COORDS => o.uvs = Some(read_tex_coords(body)?),
            LOCAL_AXES => {
                Reader { b: body }.take(48)?;
            }
            _ => {}
        }
    }
    Ok(o)
}

fn read_vertices(body: &[u8]) -> Result<Vec<V3>> {
    let mut r = Reader { b: body };
    let n = r.u16()? as usize;
    let data = r.records(n, 12)?;
    Ok(data.as_chunks::<12>().0.iter().map(|v| [f32_at(v, 0), f32_at(v, 4), f32_at(v, 8)]).collect())
}

fn read_tex_coords(body: &[u8]) -> Result<Vec<[f32; 2]>> {
    let mut r = Reader { b: body };
    let n = r.u16()? as usize;
    let data = r.records(n, 8)?;
    Ok(data.as_chunks::<8>().0.iter().map(|v| [f32_at(v, 0), f32_at(v, 4)].map(|x| if x.is_finite() { x } else { 0.0 })).collect())
}

fn read_faces(body: &[u8], o: &mut Object) -> Result<()> {
    let mut r = Reader { b: body };
    let n = r.u16()? as usize;
    let data = r.records(n, 8)?;
    o.faces = data.as_chunks::<8>().0.iter().map(|f| [0, 2, 4].map(|k| u16::from_le_bytes([f[k], f[k + 1]]))).collect();
    for c in chunks(r.rest()) {
        let (id, body) = c?;
        match id {
            FACE_MATERIAL => {
                let mut r = Reader { b: body };
                let name = r.cstr()?;
                let n = r.u16()? as usize;
                let data = r.records(n, 2)?;
                let faces = data.as_chunks::<2>().0.iter().map(|f| u16::from_le_bytes([f[0], f[1]])).collect();
                o.groups.push((name, faces));
            }
            SMOOTHING => {
                // One u32 per face; a short chunk leaves the rest at 0.
                let mut masks: Vec<u32> = body.as_chunks::<4>().0.iter().take(o.faces.len()).map(|m| u32::from_le_bytes([m[0], m[1], m[2], m[3]])).collect();
                masks.resize(o.faces.len(), 0);
                o.smoothing = Some(masks);
            }
            _ => {}
        }
    }
    Ok(())
}

fn read_material(body: &[u8]) -> Result<Mat> {
    let mut m = Mat { opacity: 1.0, ..Mat::default() };
    for c in chunks(body) {
        let (id, body) = c?;
        match id {
            MAT_NAME => m.name = Reader { b: body }.cstr()?,
            MAT_DIFFUSE => read_diffuse(body, &mut m)?,
            MAT_TRANSPARENCY => read_transparency(body, &mut m)?,
            MAT_TEXTURE => {
                for c in chunks(body) {
                    let (id, body) = c?;
                    let mut r = Reader { b: body };
                    match id {
                        MAT_TEXTURE_FILE => m.texture = Some(r.cstr()?),
                        MAP_U_SCALE => m.map.scale[0] = finite(r.f32()?, 1.0),
                        MAP_V_SCALE => m.map.scale[1] = finite(r.f32()?, 1.0),
                        MAP_U_OFFSET => m.map.offset[0] = finite(r.f32()?, 0.0),
                        MAP_V_OFFSET => m.map.offset[1] = finite(r.f32()?, 0.0),
                        MAP_ROTATION => m.map.rotation = finite(r.f32()?, 0.0),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    Ok(m)
}

fn finite(x: f32, default: f32) -> f32 {
    if x.is_finite() { x } else { default }
}

fn read_diffuse(body: &[u8], m: &mut Mat) -> Result<()> {
    for c in chunks(body) {
        let (id, body) = c?;
        let mut r = Reader { b: body };
        let (color, gamma) = match id {
            COLOR_FLOAT | COLOR_FLOAT_GAMMA => ([r.f32()?, r.f32()?, r.f32()?], id == COLOR_FLOAT_GAMMA),
            COLOR_BYTE | COLOR_BYTE_GAMMA => {
                let b = r.take(3)?;
                ([b[0], b[1], b[2]].map(|c| f32::from(c) / 255.0), id == COLOR_BYTE_GAMMA)
            }
            _ => continue,
        };
        if m.color.is_none() || (gamma && !m.gamma) {
            m.color = Some(color.map(|c| if c.is_finite() { c.clamp(0.0, 1.0) } else { 0.0 }));
            m.gamma = gamma;
        }
    }
    Ok(())
}

fn read_transparency(body: &[u8], m: &mut Mat) -> Result<()> {
    for c in chunks(body) {
        let (id, body) = c?;
        let mut r = Reader { b: body };
        let percent = match id {
            PERCENT_INT => f32::from(r.u16()?) / 100.0,
            PERCENT_FLOAT => r.f32()?,
            _ => continue,
        };
        if percent.is_finite() {
            m.opacity = 1.0 - percent.clamp(0.0, 1.0);
        }
    }
    Ok(())
}

fn build(scene: &Scene) -> Vec<Mesh> {
    scene.objects.iter().flat_map(|o| build_object(o, &scene.materials)).collect()
}

/// A face with its corners in range of the vertex list.
struct Face {
    corners: [usize; 3],
    smooth: u32,
}

fn build_object(o: &Object, materials: &[Mat]) -> Vec<Mesh> {
    let n = o.positions.len();
    let faces: Vec<Option<Face>> = o
        .faces
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let corners = f.map(usize::from);
            let smooth = o.smoothing.as_ref().and_then(|s| s.get(i)).copied().unwrap_or(0);
            corners.iter().all(|&c| c < n).then_some(Face { corners, smooth })
        })
        .collect();
    // Which group each face is in: the first that lists it.
    let mut group_of: Vec<Option<usize>> = vec![None; faces.len()];
    let mut names: Vec<&str> = Vec::new();
    for (name, list) in &o.groups {
        let g = names.iter().position(|n| n == name).unwrap_or_else(|| {
            names.push(name);
            names.len() - 1
        });
        for &f in list {
            if let Some(slot) = group_of.get_mut(usize::from(f)).filter(|s| s.is_none()) {
                *slot = Some(g);
            }
        }
    }
    // One coordinate per vertex, or none.
    let uvs = o.uvs.as_deref().filter(|u| u.len() == n);
    let normals = o.smoothing.is_some().then(|| corner_normals(&o.positions, &faces));
    let mut meshes = Vec::new();
    // One mesh per group name, then the faces in none.
    for g in (0..names.len()).map(Some).chain([None]) {
        let members: Vec<usize> = (0..faces.len()).filter(|&i| group_of[i] == g && faces[i].is_some()).collect();
        if members.is_empty() {
            continue;
        }
        let mat = g.and_then(|g| materials.iter().find(|m| m.name == names[g]));
        let material = mat.map(|m| Material {
            name: m.name.clone(),
            color: m.color.unwrap_or(DEFAULT_COLOR),
            opacity: m.opacity,
        });
        let mut mesh = match &normals {
            Some(nrm) => split_mesh(&o.positions, &faces, nrm, uvs, &members),
            None => shared_mesh(&o.positions, &faces, uvs, &members),
        };
        mesh.name = o.name.clone();
        mesh.material = material;
        // The map moves the coordinates; a mesh whose picture couldn't be
        // read gets them with just the flip.
        let map = mat.map(|m| m.map).unwrap_or_default();
        if let Some(uvs) = &mut mesh.uvs {
            for uv in uvs {
                *uv = map.apply(*uv);
            }
        }
        mesh.texture = mat.and_then(|m| m.loaded.clone());
        mesh.surface.blend = mat.is_some_and(|m| (m.opacity - 1.0).abs() > 1e-6);
        meshes.push(mesh);
    }
    meshes
}

/// The listed faces over the vertices they use, as in the file.
fn shared_mesh(positions: &[V3], faces: &[Option<Face>], uvs: Option<&[[f32; 2]]>, members: &[usize]) -> Mesh {
    let mut remap: Vec<Option<u32>> = vec![None; positions.len()];
    let mut mesh = Mesh::default();
    let mut out_uvs = Vec::new();
    for f in members.iter().filter_map(|&i| faces[i].as_ref()) {
        for &c in &f.corners {
            let slot = &mut remap[c];
            let i = *slot.get_or_insert_with(|| {
                mesh.positions.push(positions[c]);
                out_uvs.extend(uvs.map(|u| u[c]));
                (mesh.positions.len() - 1) as u32
            });
            mesh.indices.push(i);
        }
    }
    mesh.uvs = uvs.map(|_| out_uvs);
    mesh
}

/// The listed faces with three own vertices each, carrying `normals`
/// (three per face, in face order).
fn split_mesh(positions: &[V3], faces: &[Option<Face>], normals: &[[V3; 3]], uvs: Option<&[[f32; 2]]>, members: &[usize]) -> Mesh {
    let mut mesh = Mesh::default();
    let mut out_uvs = Vec::with_capacity(if uvs.is_some() { members.len() * 3 } else { 0 });
    let mut out_normals = Vec::with_capacity(members.len() * 3);
    for &i in members {
        let Some(f) = &faces[i] else { continue };
        for k in 0..3 {
            mesh.indices.push(mesh.positions.len() as u32);
            mesh.positions.push(positions[f.corners[k]]);
            out_normals.push(normals[i][k]);
            out_uvs.extend(uvs.map(|u| u[f.corners[k]]));
        }
    }
    mesh.normals = Some(out_normals);
    mesh.uvs = uvs.map(|_| out_uvs);
    mesh
}

/// The normal at each corner of each face (zeros for dropped faces): the
/// normalised sum of the area-weighted normals of the faces that meet at
/// the same place and share a smoothing bit; a face in no smoothing group
/// keeps its own normal.
fn corner_normals(positions: &[V3], faces: &[Option<Face>]) -> Vec<[V3; 3]> {
    let weld = geom::weld(positions);
    let tri = |f: &Face| f.corners.map(|c| positions[c]);
    // Per welded place: the summed (unnormalised) normals by smoothing mask.
    // At most MAX_MASKS distinct masks are kept per place, so a file with
    // thousands of masks at one vertex costs linear time, not quadratic.
    const MAX_MASKS: usize = 256;
    let mut at: HashMap<u32, Vec<(u32, V3)>> = HashMap::new();
    let mut slot: HashMap<(u32, u32), usize> = HashMap::new();
    for f in faces.iter().flatten() {
        let [a, b, c] = tri(f);
        let area_normal = math::cross(math::sub(b, a), math::sub(c, a));
        for &corner in &f.corners {
            let place = weld[corner];
            let list = at.entry(place).or_default();
            match slot.get(&(place, f.smooth)) {
                Some(&k) => list[k].1 = math::add(list[k].1, area_normal),
                None if list.len() < MAX_MASKS => {
                    slot.insert((place, f.smooth), list.len());
                    list.push((f.smooth, area_normal));
                }
                None => {}
            }
        }
    }
    let mut cache: HashMap<(u32, u32), V3> = HashMap::new();
    faces
        .iter()
        .map(|f| {
            let Some(f) = f else { return [[0.0; 3]; 3] };
            let [a, b, c] = tri(f);
            if f.smooth == 0 {
                return [face_normal(a, b, c); 3];
            }
            f.corners.map(|corner| {
                let place = weld[corner];
                *cache.entry((place, f.smooth)).or_insert_with(|| {
                    let sum = at.get(&place).into_iter().flatten().filter(|(m, _)| m & f.smooth != 0).fold([0.0; 3], |s, (_, n)| math::add(s, *n));
                    let n = math::normalize(sum);
                    if n == [0.0; 3] { face_normal(a, b, c) } else { n }
                })
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(bytes: &[u8]) -> Result<Model> {
        super::parse(bytes, &|_| None)
    }

    #[test]
    fn texture_next_to_the_file_gives_the_colour() {
        let mut png = Vec::new();
        image::RgbImage::from_pixel(2, 2, image::Rgb([0, 0, 255])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let mat = cat(&[chunk(MAT_NAME, &cstr("t")), red_byte(), chunk(MAT_TEXTURE, &chunk(MAT_TEXTURE_FILE, &cstr("BLUE.PNG")))]);
        let f = file(&[mat], &[("t", all(12))], None);
        let side = |n: &str| (n == "BLUE.PNG").then(|| png.clone());
        assert_eq!(super::parse(&f, &side).unwrap().meshes[0].material.as_ref().unwrap().color, [0.0, 0.0, 1.0]);
        // Missing image: the diffuse colour.
        assert_eq!(parse(&f).unwrap().meshes[0].material.as_ref().unwrap().color, [1.0, 0.0, 0.0]);
    }

    /// Cube corner `i` maps to this, in the file's v-up space.
    fn corner_uv(i: usize) -> [f32; 2] {
        [i as f32 * 0.1, i as f32 * 0.05]
    }

    fn cube_uvs() -> Vec<[f32; 2]> {
        (0..8).map(corner_uv).collect()
    }

    fn near(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5
    }

    /// Every vertex of the model has the coordinate `want` gives for its
    /// corner of the cube.
    fn uvs_match(m: &Model, want: impl Fn(usize) -> [f32; 2]) -> bool {
        m.meshes.iter().all(|mesh| {
            let uvs = mesh.uvs.as_ref().unwrap();
            uvs.len() == mesh.positions.len() && mesh.positions.iter().zip(uvs).all(|(p, uv)| near(*uv, want((0..8).find(|&i| corner(i) == *p).unwrap())))
        })
    }

    /// A texture map chunk: file name, then the sub-chunks given.
    fn map_chunk(name: &str, subs: &[Vec<u8>]) -> Vec<u8> {
        chunk(MAT_TEXTURE, &cat(&[chunk(MAT_TEXTURE_FILE, &cstr(name)), subs.concat()]))
    }

    fn float_chunk(id: u16, v: f32) -> Vec<u8> {
        chunk(id, &v.to_le_bytes())
    }

    #[test]
    fn mapping_coordinates_are_read_and_flipped() {
        // Shared vertices (no smoothing groups) and split ones (with).
        for smooth in [None, Some(&[1u32; 12][..])] {
            let m = parse(&file_uv(&[], &[], smooth, &cube_uvs())).unwrap();
            assert_eq!(m.meshes[0].positions.len(), if smooth.is_some() { 36 } else { 8 });
            assert!(uvs_match(&m, |i| [corner_uv(i)[0], 1.0 - corner_uv(i)[1]]));
            // No texture without a material that has one.
            assert!(m.meshes[0].texture.is_none());
        }
        // A list that is not one per vertex is dropped.
        let m = parse(&file_uv(&[], &[], None, &cube_uvs()[..5])).unwrap();
        assert!(m.meshes[0].uvs.is_none());
        // Groups split a mesh; each part keeps its own vertices' coordinates.
        let mats = [material("a", red_byte())];
        let m = parse(&file_uv(&mats, &[("a", all(5))], None, &cube_uvs())).unwrap();
        assert_eq!(m.meshes.len(), 2);
        assert!(uvs_match(&m, |i| [corner_uv(i)[0], 1.0 - corner_uv(i)[1]]));
    }

    #[test]
    fn texture_map_gives_texture_and_transform() {
        let png = crate::model3d::texture::tests::png_2x2();
        let side = |n: &str| (n == "tex.png").then(|| png.clone());
        let flags = |f: u16| chunk(MAP_TILING, &f.to_le_bytes());
        let with = |subs: &[Vec<u8>]| {
            let mat = cat(&[chunk(MAT_NAME, &cstr("t")), red_byte(), map_chunk("tex.png", subs)]);
            super::parse(&file_uv(&[mat], &[("t", all(12))], None, &cube_uvs()), &side).unwrap()
        };
        let m = with(&[flags(0x0010)]);
        let mesh = &m.meshes[0];
        let t = mesh.texture.as_ref().unwrap();
        assert_eq!((t.image.width, t.image.height, t.tint, t.alpha_cutoff), (2, 2, [1.0; 3], None));
        // The material colour is the picture's average, not the diffuse red.
        let avg = mesh.material.as_ref().unwrap().color;
        assert!((avg[1] - 0.5).abs() < 1e-3 && (avg[0] - 0.5).abs() < 1e-3);
        assert!(mesh.textured().is_some());
        // Scale then offset (no rotation), then v flipped.
        let m = with(&[float_chunk(MAP_U_SCALE, 2.0), float_chunk(MAP_V_SCALE, 3.0), float_chunk(MAP_U_OFFSET, 0.25), float_chunk(MAP_V_OFFSET, 0.5)]);
        assert!(uvs_match(&m, |i| [corner_uv(i)[0] * 2.0 + 0.25, 1.0 - (corner_uv(i)[1] * 3.0 + 0.5)]));
        // Rotation of 90 degrees: (u, v) -> (v, -u), as three.js does about (0, 0).
        let m = with(&[float_chunk(MAP_ROTATION, 90.0)]);
        assert!(uvs_match(&m, |i| [corner_uv(i)[1], 1.0 + corner_uv(i)[0]]));
    }

    #[test]
    fn missing_texture_keeps_the_diffuse_colour_and_the_coordinates() {
        let mat = cat(&[chunk(MAT_NAME, &cstr("t")), red_byte(), map_chunk("gone.png", &[chunk(MAP_TILING, &0x10u16.to_le_bytes())])]);
        let m = parse(&file_uv(&[mat], &[("t", all(12))], None, &cube_uvs())).unwrap();
        let mesh = &m.meshes[0];
        assert!(mesh.texture.is_none());
        assert_eq!(mesh.material.as_ref().unwrap().color, [1.0, 0.0, 0.0]);
        assert!(uvs_match(&m, |i| [corner_uv(i)[0], 1.0 - corner_uv(i)[1]]));
    }

    #[test]
    fn texture_outside_the_model_folder_is_not_read() {
        let png = crate::model3d::texture::tests::png_2x2();
        let asked = std::cell::RefCell::new(Vec::new());
        let side = |n: &str| {
            asked.borrow_mut().push(n.to_string());
            (!n.contains("..")).then(|| png.clone())
        };
        let mat = cat(&[chunk(MAT_NAME, &cstr("t")), red_byte(), map_chunk("../up/tex.png", &[])]);
        super::parse(&file(&[mat], &[("t", all(12))], None), &side).unwrap();
        assert!(asked.borrow().iter().all(|n| !n.contains("..")));
    }

    #[test]
    fn import_thumbnail_draws_the_red_cube() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cube.3ds");
        std::fs::write(&path, red_cube()).unwrap();
        let img = super::super::raster::thumbnail(&path, "3ds").unwrap();
        assert!(img.pixels().any(|p| p[3] == 255 && p[0] > 100 && p[1] < 40 && p[2] < 40));
    }

    #[test]
    fn many_smoothing_masks_at_one_vertex_stay_fast() {
        let faces: Vec<[u16; 3]> = (0..65535u32).map(|i| [0, (i % 6 + 1) as u16, (i % 6 + 2) as u16]).collect();
        let smooth: Vec<u32> = (0..65535u32).map(|i| i + 1).collect();
        let mut obj = cstr("fan");
        obj.extend(chunk(TRI_MESH, &mesh_chunk(&faces, &[], Some(&smooth))));
        let f = chunk(MAIN, &chunk(EDITOR, &chunk(OBJECT, &obj)));
        let t = std::time::Instant::now();
        assert_eq!(parse(&f).unwrap().triangle_count(), 65535);
        assert!(t.elapsed().as_secs() < 20);
    }

    fn chunk(id: u16, body: &[u8]) -> Vec<u8> {
        let mut out = id.to_le_bytes().to_vec();
        out.extend(((body.len() + 6) as u32).to_le_bytes());
        out.extend(body);
        out
    }

    fn cat(parts: &[Vec<u8>]) -> Vec<u8> {
        parts.concat()
    }

    fn cstr(s: &str) -> Vec<u8> {
        let mut v = s.as_bytes().to_vec();
        v.push(0);
        v
    }

    /// Cube corner `i` has x, y, z from bits 0, 1, 2.
    fn corner(i: usize) -> V3 {
        [(i & 1) as f32, (i >> 1 & 1) as f32, (i >> 2 & 1) as f32]
    }

    fn index(axis: usize, s: usize, u: usize, v: usize) -> usize {
        s << axis | u << ((axis + 1) % 3) | v << ((axis + 2) % 3)
    }

    /// 12 counter-clockwise triangles, two per side; side = axis * 2 + s.
    /// The diagonal avoids the (0, 0) corner of each side.
    fn cube_faces() -> Vec<[u16; 3]> {
        let mut out = Vec::new();
        for axis in 0..3 {
            for s in 0..2 {
                let p = |u, v| index(axis, s, u, v) as u16;
                if s == 1 {
                    out.push([p(0, 0), p(1, 0), p(0, 1)]);
                    out.push([p(1, 0), p(1, 1), p(0, 1)]);
                } else {
                    out.push([p(0, 0), p(0, 1), p(1, 0)]);
                    out.push([p(1, 0), p(0, 1), p(1, 1)]);
                }
            }
        }
        out
    }

    fn color_chunk(id: u16, body: &[u8]) -> Vec<u8> {
        chunk(MAT_DIFFUSE, &chunk(id, body))
    }

    fn red_byte() -> Vec<u8> {
        color_chunk(COLOR_BYTE, &[255, 0, 0])
    }

    fn mesh_chunk(faces: &[[u16; 3]], groups: &[(&str, Vec<u16>)], smooth: Option<&[u32]>) -> Vec<u8> {
        mesh_chunk_uv(faces, groups, smooth, &[[0.0; 2]; 8])
    }

    fn mesh_chunk_uv(faces: &[[u16; 3]], groups: &[(&str, Vec<u16>)], smooth: Option<&[u32]>, uvs: &[[f32; 2]]) -> Vec<u8> {
        let mut verts = 8u16.to_le_bytes().to_vec();
        for i in 0..8 {
            verts.extend(corner(i).iter().flat_map(|c| c.to_le_bytes()));
        }
        let mut fl = (faces.len() as u16).to_le_bytes().to_vec();
        for f in faces {
            fl.extend(f.iter().flat_map(|i| i.to_le_bytes()));
            fl.extend([0u8; 2]);
        }
        for (name, list) in groups {
            let mut g = cstr(name);
            g.extend((list.len() as u16).to_le_bytes());
            g.extend(list.iter().flat_map(|i| i.to_le_bytes()));
            fl.extend(chunk(FACE_MATERIAL, &g));
        }
        if let Some(s) = smooth {
            fl.extend(chunk(SMOOTHING, &s.iter().flat_map(|m| m.to_le_bytes()).collect::<Vec<_>>()));
        }
        let mut uv = (uvs.len() as u16).to_le_bytes().to_vec();
        uv.extend(uvs.iter().flatten().flat_map(|c| c.to_le_bytes()));
        cat(&[chunk(VERTICES, &verts), chunk(FACES, &fl), chunk(TEX_COORDS, &uv), chunk(LOCAL_AXES, &[0u8; 48])])
    }

    fn file(materials: &[Vec<u8>], groups: &[(&str, Vec<u16>)], smooth: Option<&[u32]>) -> Vec<u8> {
        file_uv(materials, groups, smooth, &[[0.0; 2]; 8])
    }

    fn file_uv(materials: &[Vec<u8>], groups: &[(&str, Vec<u16>)], smooth: Option<&[u32]>, uvs: &[[f32; 2]]) -> Vec<u8> {
        let mut obj = cstr("cube");
        obj.extend(chunk(TRI_MESH, &mesh_chunk_uv(&cube_faces(), groups, smooth, uvs)));
        let mut editor = chunk(OBJECT, &obj);
        for m in materials {
            editor.extend(chunk(MATERIAL, m));
        }
        chunk(MAIN, &chunk(EDITOR, &editor))
    }

    fn material(name: &str, color: Vec<u8>) -> Vec<u8> {
        cat(&[chunk(MAT_NAME, &cstr(name)), color])
    }

    fn all(n: u16) -> Vec<u16> {
        (0..n).collect()
    }

    fn red_cube() -> Vec<u8> {
        file(&[material("red", red_byte())], &[("red", all(12))], None)
    }

    #[test]
    fn coloured_cube() {
        let m = parse(&red_cube()).unwrap();
        assert_eq!(m.meshes.len(), 1);
        assert_eq!(m.triangle_count(), 12);
        assert_eq!(m.bounds(), Some(([0.0; 3], [1.0; 3])));
        let mat = m.meshes[0].material.as_ref().unwrap();
        assert_eq!((mat.name.as_str(), mat.color, mat.opacity), ("red", [1.0, 0.0, 0.0], 1.0));
        assert_eq!(m.meshes[0].name, "cube");
        assert_eq!(m.meshes[0].positions.len(), 8);
        assert!(m.meshes[0].normals.is_none());
    }

    #[test]
    fn colour_forms() {
        let f32s = |c: [f32; 3]| c.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>();
        for (c, want) in [
            (color_chunk(COLOR_FLOAT, &f32s([0.0, 0.5, 1.0])), [0.0, 0.5, 1.0]),
            (color_chunk(COLOR_FLOAT_GAMMA, &f32s([1.0, 0.0, 0.25])), [1.0, 0.0, 0.25]),
            (color_chunk(COLOR_BYTE_GAMMA, &[0, 255, 0]), [0.0, 1.0, 0.0]),
        ] {
            let m = parse(&file(&[material("c", c)], &[("c", all(12))], None)).unwrap();
            assert_eq!(m.meshes[0].material.as_ref().unwrap().color, want);
        }
        // The gamma-corrected form wins over the linear one.
        let both = chunk(MAT_DIFFUSE, &cat(&[chunk(COLOR_BYTE, &[255, 0, 0]), chunk(COLOR_BYTE_GAMMA, &[0, 0, 255])]));
        let m = parse(&file(&[material("c", both)], &[("c", all(12))], None)).unwrap();
        assert_eq!(m.meshes[0].material.as_ref().unwrap().color, [0.0, 0.0, 1.0]);
    }

    #[test]
    fn transparency_and_texture() {
        let mut mat = material("glass", red_byte());
        mat.extend(chunk(MAT_TRANSPARENCY, &chunk(PERCENT_INT, &25u16.to_le_bytes())));
        let m = parse(&file(&[mat], &[("glass", all(12))], None)).unwrap();
        assert_eq!(m.meshes[0].material.as_ref().unwrap().opacity, 0.75);
        assert!(m.meshes[0].surface.blend);
        let mut mat = material("glass", red_byte());
        mat.extend(chunk(MAT_TRANSPARENCY, &chunk(PERCENT_FLOAT, &0.5f32.to_le_bytes())));
        let m = parse(&file(&[mat], &[("glass", all(12))], None)).unwrap();
        assert_eq!(m.meshes[0].material.as_ref().unwrap().opacity, 0.5);
        let mat = read_material(&cat(&[chunk(MAT_NAME, &cstr("t")), chunk(MAT_TEXTURE, &chunk(MAT_TEXTURE_FILE, &cstr("wood.jpg")))])).unwrap();
        assert_eq!(mat.texture.as_deref(), Some("wood.jpg"));
    }

    #[test]
    fn blend_follows_transparency() {
        let transparent = |bytes: Vec<u8>| {
            let mut mat = material("glass", red_byte());
            mat.extend(chunk(MAT_TRANSPARENCY, &bytes));
            mat
        };
        // Shared vertices, and split ones with smoothing groups.
        for smooth in [None, Some(&[1u32; 12][..])] {
            let m = parse(&file(&[transparent(chunk(PERCENT_INT, &50u16.to_le_bytes()))], &[("glass", all(12))], smooth)).unwrap();
            assert!(m.meshes[0].surface.blend);
            assert!(m.meshes[0].surface.bump_map.is_none() && m.meshes[0].surface.normal_map.is_none());
            let m = parse(&file(&[transparent(chunk(PERCENT_INT, &0u16.to_le_bytes()))], &[("glass", all(12))], smooth)).unwrap();
            assert!(!m.meshes[0].surface.blend);
            let m = parse(&file(&[transparent(chunk(PERCENT_FLOAT, &f32::NAN.to_le_bytes()))], &[("glass", all(12))], smooth)).unwrap();
            assert!(!m.meshes[0].surface.blend);
        }
        assert!(!parse(&red_cube()).unwrap().meshes[0].surface.blend);
    }

    #[test]
    fn faces_split_across_materials() {
        let mats = [material("red", red_byte()), material("blue", color_chunk(COLOR_BYTE, &[0, 0, 255]))];
        // Faces 0..4 red, 4..9 blue, 11 in a group without a material, the
        // rest (9, 10) in none.
        let groups = [("red", all(4)), ("blue", (4..9).collect()), ("ghost", vec![11])];
        let m = parse(&file(&mats, &groups, None)).unwrap();
        let counts: Vec<_> = m.meshes.iter().map(|m| (m.material.as_ref().map(|m| m.name.clone()), m.triangle_count())).collect();
        assert_eq!(counts, [(Some("red".into()), 4), (Some("blue".into()), 5), (None, 1), (None, 2)]);
    }

    #[test]
    fn ungrouped_faces_make_a_mesh_without_material() {
        let m = parse(&file(&[material("red", red_byte())], &[("red", all(4))], None)).unwrap();
        let counts: Vec<_> = m.meshes.iter().map(|m| (m.material.is_some(), m.triangle_count())).collect();
        assert_eq!(counts, [(true, 4), (false, 8)]);
    }

    #[test]
    fn one_smoothing_group_rounds_the_corners() {
        let m = parse(&file(&[], &[], Some(&[1; 12]))).unwrap();
        assert_eq!(m.triangle_count(), 12);
        assert_eq!(m.meshes[0].positions.len(), 36, "triangles are un-shared");
        let ns = normals_at(&m, [0.0; 3]);
        assert_eq!(ns.len(), 3);
        let d = 1.0 / 3f32.sqrt();
        for n in ns {
            for c in n {
                assert!((c + d).abs() < 1e-4, "{n:?}");
            }
        }
    }

    fn normals_at(m: &Model, p: V3) -> Vec<V3> {
        let mesh = &m.meshes[0];
        let n = mesh.normals.as_ref().unwrap();
        mesh.indices.iter().filter(|&&i| mesh.positions[i as usize] == p).map(|&i| n[i as usize]).collect()
    }

    #[test]
    fn distinct_groups_keep_faces_flat() {
        let smooth: Vec<u32> = (0..12).map(|i| 1 << (i / 2)).collect();
        check_flat(&parse(&file(&[], &[], Some(&smooth))).unwrap());
    }

    #[test]
    fn smoothing_group_zero_is_flat() {
        check_flat(&parse(&file(&[], &[], Some(&[0; 12]))).unwrap());
    }

    fn check_flat(m: &Model) {
        let mesh = &m.meshes[0];
        let n = mesh.normals.as_ref().unwrap();
        for t in mesh.indices.as_chunks::<3>().0 {
            let [a, b, c] = t.map(|i| mesh.positions[i as usize]);
            let want = face_normal(a, b, c);
            for &i in t {
                assert_eq!(n[i as usize], want);
            }
        }
    }

    #[test]
    fn smoothing_only_joins_faces_sharing_a_bit() {
        // Sides 2 (y = 0) and 4 (z = 0) share bit 1 on top of their own, the
        // others are alone: their common edge is rounded, the rest stays flat.
        let smooth: Vec<u32> = (0..12).map(|i| (1 << (i / 2 + 1)) | if i / 2 == 2 || i / 2 == 4 { 1 } else { 0 }).collect();
        let m = parse(&file(&[], &[], Some(&smooth))).unwrap();
        let at = normals_at(&m, [0.0; 3]);
        let d = 1.0 / 2f32.sqrt();
        let rounded = at.iter().filter(|n| n[0].abs() < 1e-4 && (n[1] + d).abs() < 1e-4 && (n[2] + d).abs() < 1e-4).count();
        assert_eq!(rounded, 2, "{at:?}");
        assert!(at.iter().any(|n| (n[0] + 1.0).abs() < 1e-4), "the x = 0 side stays flat");
    }

    #[test]
    fn truncations_and_corruption_do_not_panic() {
        let full = file(&[material("red", red_byte())], &[("red", all(12))], Some(&[1; 12]));
        assert!(parse(&full).is_ok());
        for n in 0..full.len() {
            let _ = parse(&full[..n]);
        }
        for i in 0..full.len() {
            let mut f = full.clone();
            f[i] ^= 0xFF;
            let _ = parse(&f);
        }
    }

    #[test]
    fn bad_chunk_lengths_are_errors() {
        for len in [0u32, 1, 5, u32::MAX] {
            let mut f = red_cube();
            f[2..6].copy_from_slice(&len.to_le_bytes());
            assert!(parse(&f).is_err(), "main length {len}");
            // The editor chunk right inside it.
            let mut f = red_cube();
            f[8..12].copy_from_slice(&len.to_le_bytes());
            assert!(parse(&f).is_err(), "editor length {len}");
        }
    }

    #[test]
    fn huge_counts_are_refused() {
        let mut verts = 65535u16.to_le_bytes().to_vec();
        verts.extend([0u8; 24]);
        assert!(read_vertices(&verts).is_err());
        let mut o = Object::default();
        let mut faces = 65535u16.to_le_bytes().to_vec();
        faces.extend([0u8; 16]);
        assert!(read_faces(&faces, &mut o).is_err());
        let mut group = cstr("m");
        group.extend(65535u16.to_le_bytes());
        let mut faces = 0u16.to_le_bytes().to_vec();
        faces.extend(chunk(FACE_MATERIAL, &group));
        assert!(read_faces(&faces, &mut o).is_err());
        // Through the whole reader.
        let mut obj = cstr("x");
        obj.extend(chunk(TRI_MESH, &chunk(VERTICES, &verts)));
        let f = chunk(MAIN, &chunk(EDITOR, &chunk(OBJECT, &obj)));
        assert!(parse(&f).is_err());
    }

    #[test]
    fn out_of_range_faces_are_dropped() {
        let mut o = Object {
            positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            faces: vec![[0, 1, 2], [0, 1, 9]],
            smoothing: Some(vec![1, 1]),
            ..Object::default()
        };
        let meshes = build_object(&o, &[]);
        assert_eq!(meshes.len(), 1);
        assert_eq!(meshes[0].triangle_count(), 1);
        o.smoothing = None;
        assert_eq!(build_object(&o, &[])[0].triangle_count(), 1);
    }

    #[test]
    fn not_a_3ds_file() {
        assert!(parse(b"hello").is_err());
        assert!(parse(b"").is_err());
        assert!(parse(&chunk(EDITOR, &[])).is_err());
        assert!(parse(&chunk(MAIN, &[])).is_err(), "no triangles");
    }

    #[test]
    fn keyframer_is_skipped() {
        let f = red_cube();
        let junk = chunk(0xB000, &[0xFF; 40]);
        let with = chunk(MAIN, &cat(&[junk.clone(), f[6..].to_vec(), junk]));
        assert_eq!(parse(&with).unwrap().triangle_count(), 12);
    }
}
