//! 3D models: readers for the formats falcon can show itself (OBJ with MTL
//! colours, STL, PLY, GLB, 3MF, FBX, COLLADA, 3DS and the tessellated
//! part of IFC), the triangle model they produce, the measurements Eagle's
//! model viewer shows, and the geometry the viewer and the thumbnail renderer
//! share (normals, feature edges, shading).
//!
//! IFC files without tessellated geometry are not read; those items keep
//! their QuickLook still.

pub mod dae;
pub mod earclip;
pub mod export;
pub mod fbx;
pub mod geom;
pub mod gltf;
pub mod ifc;
pub mod math;
pub mod obj;
pub mod off;
pub mod ply;
pub mod raster;
pub mod step;
pub mod stl;
pub mod threemf;
pub mod threeds;

use std::path::Path;

use math::V3;

/// Extensions read here, lower case: Eagle's model formats. A `.gltf` is a
/// plain item with a QuickLook thumbnail, as in Eagle.
pub const EXTENSIONS: &[&str] = &["obj", "stl", "ply", "glb", "3mf", "fbx", "dae", "3ds", "ifc"];

/// Whether files with extension `ext` (any case) are read here.
pub fn is_supported(ext: &str) -> bool {
    EXTENSIONS.iter().any(|e| e.eq_ignore_ascii_case(ext))
}

/// The colour a mesh without a material is drawn in, as in Eagle's viewer
/// (200, 200, 200), in 0..1.
pub const DEFAULT_COLOR: V3 = [200.0 / 255.0, 200.0 / 255.0, 200.0 / 255.0];

/// Why a file could not be read as a model.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ParseError(pub String);

impl ParseError {
    pub fn new(e: impl std::fmt::Display) -> Self {
        ParseError(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, ParseError>;

/// A material as far as the viewer uses it: a name and a base colour.
#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub name: String,
    /// Base (diffuse) colour, each channel 0..1, in the file's own (sRGB)
    /// values.
    pub color: V3,
    /// 1 is opaque.
    pub opacity: f32,
}

/// One part of a model: triangles over shared vertices, drawn in one
/// material.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub name: String,
    pub positions: Vec<V3>,
    /// Normals from the file, one per position, when it has them.
    pub normals: Option<Vec<V3>>,
    /// Vertex colours (0..1), one per position, when the file has them;
    /// they replace the material colour.
    pub colors: Option<Vec<V3>>,
    /// Three indices into `positions` per triangle.
    pub indices: Vec<u32>,
    pub material: Option<Material>,
}

impl Mesh {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Drops triangles that point past the vertices and lists that are
    /// not one per vertex, so the rest of the code can index freely.
    pub fn sanitize(&mut self) {
        let n = self.positions.len();
        let whole = self.indices.len() / 3 * 3;
        self.indices.truncate(whole);
        if self.indices.iter().any(|&i| i as usize >= n) {
            self.indices = self.indices.as_chunks::<3>().0.iter().filter(|t| t.iter().all(|&i| (i as usize) < n)).flatten().copied().collect();
        }
        if self.normals.as_ref().is_some_and(|v| v.len() != n) {
            self.normals = None;
        }
        if self.colors.as_ref().is_some_and(|v| v.len() != n) {
            self.colors = None;
        }
    }
}

/// A whole model: its meshes in file order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Model {
    pub meshes: Vec<Mesh>,
}

impl Model {
    pub fn vertex_count(&self) -> usize {
        self.meshes.iter().map(|m| m.positions.len()).sum()
    }

    pub fn triangle_count(&self) -> usize {
        self.meshes.iter().map(Mesh::triangle_count).sum()
    }

    /// The smallest box around every vertex used by a triangle, as
    /// (min, max); `None` for a model with no triangles.
    pub fn bounds(&self) -> Option<(V3, V3)> {
        let mut b: Option<(V3, V3)> = None;
        for m in &self.meshes {
            for &i in &m.indices {
                let p = m.positions[i as usize];
                if !p.iter().all(|c| c.is_finite()) {
                    continue;
                }
                let (lo, hi) = b.get_or_insert((p, p));
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
        }
        b
    }

    /// The sphere around the bounding box, as (centre, radius).
    pub fn bounding_sphere(&self) -> Option<(V3, f32)> {
        let (lo, hi) = self.bounds()?;
        let c = math::scale(math::add(lo, hi), 0.5);
        Some((c, math::length(math::sub(hi, c))))
    }

    /// Drops meshes left without triangles and fixes bad indices; every
    /// reader calls this last.
    pub fn finish(mut self) -> Result<Model> {
        for m in &mut self.meshes {
            m.sanitize();
        }
        self.meshes.retain(|m| !m.indices.is_empty());
        if self.meshes.is_empty() {
            return Err(ParseError::new("the file has no triangles"));
        }
        Ok(self)
    }
}

/// Reads the model at `path`, picking the reader by `ext`. Files a model
/// refers to (an OBJ's MTL, a GLB's external buffers) are looked up next to it.
pub fn load(path: &Path, ext: &str) -> Result<Model> {
    let bytes = std::fs::read(path).map_err(ParseError::new)?;
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let side = move |name: &str| -> Option<Vec<u8>> {
        // Only plain names next to the model: no climbing out of its folder.
        let rel = Path::new(name);
        if rel.is_absolute() || rel.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return None;
        }
        std::fs::read(dir.join(rel)).ok()
    };
    parse(&bytes, ext, &side)
}

/// Reads a model from `bytes` by extension. `side` fetches a file the model
/// refers to by its relative name.
pub fn parse(bytes: &[u8], ext: &str, side: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Model> {
    match ext.to_ascii_lowercase().as_str() {
        "obj" => obj::parse(bytes, side),
        "stl" => stl::parse(bytes),
        "ply" => ply::parse(bytes),
        "glb" => gltf::parse_glb(bytes, side),
        "3mf" => threemf::parse(bytes),
        "fbx" => fbx::parse(bytes),
        "dae" => dae::parse(bytes, side),
        "3ds" => threeds::parse(bytes, side),
        "ifc" => ifc::parse(bytes),
        other => Err(ParseError(format!("cannot read .{other} models"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_reads_files_and_their_neighbours() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("models");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("cube.mtl"), "newmtl red\nKd 1 0 0\n").unwrap();
        // A material library outside the model's folder is not read.
        std::fs::write(dir.path().join("outside.mtl"), "newmtl blue\nKd 0 0 1\n").unwrap();
        let obj = "mtllib cube.mtl\nmtllib ../outside.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nusemtl red\nf 1 2 3\nusemtl blue\nf 1 3 2\n";
        std::fs::write(sub.join("Cube.OBJ"), obj).unwrap();
        let m = load(&sub.join("Cube.OBJ"), "OBJ").unwrap();
        assert_eq!(m.meshes.len(), 2);
        assert_eq!(m.meshes[0].material.as_ref().unwrap().color, [1.0, 0.0, 0.0]);
        assert_eq!(m.meshes[1].material.as_ref().unwrap().color, DEFAULT_COLOR, "../ is refused");
        // ASCII STL through the same door.
        std::fs::write(sub.join("t.stl"), "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid t\n").unwrap();
        assert_eq!(load(&sub.join("t.stl"), "stl").unwrap().triangle_count(), 1);
        assert!(load(&sub.join("missing.ply"), "ply").is_err());
        assert!(parse(b"", "fbx", &|_| None).is_err());
        assert!(parse(b"", "dae", &|_| None).is_err());
        assert!(is_supported("GLB") && is_supported("FBX") && is_supported("Dae") && is_supported("3DS"));
    }
}
