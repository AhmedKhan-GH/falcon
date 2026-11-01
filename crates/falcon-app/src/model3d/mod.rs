//! The 3D model viewer's state: the settings it keeps while falcon runs,
//! the model being read on a background thread, its camera, and the
//! vertex data handed to the GPU (`gpu.rs`). The viewer itself is drawn by
//! `ui/viewer/model.rs`; the camera maths are in `logic/viewer3d.rs`, the
//! readers and geometry in `falcon_library::model3d`.

pub(crate) mod gpu;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};

use eframe::egui_wgpu::wgpu;
use falcon_library::model3d::geom::{self, Stats};
use falcon_library::model3d::math::V3;
use falcon_library::model3d::{self as m3d, Model};

use crate::logic::{Camera, Projection, UpAxis};

/// How faces are shaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum Shading {
    /// The file's normals where it has them, flat faces elsewhere (what
    /// Eagle's viewer shows).
    #[default]
    Auto,
    Flat,
    Smooth,
}

/// The viewer's colour theme (the setting Eagle's viewer calls dark mode):
/// follow falcon's theme, or force light or dark. It sets the background
/// (unless one is chosen) and the panels' colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum ViewerTheme {
    #[default]
    Auto,
    Light,
    Dark,
}

/// The viewer's display settings, kept across models while falcon runs,
/// as Eagle's viewer keeps them.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Settings {
    pub shading: Shading,
    /// Eagle's "Show Edges", off by default, black, at a 1° threshold.
    pub edges: bool,
    pub edge_color: [u8; 3],
    pub edge_threshold: f32,
    pub grid: bool,
    pub axes: bool,
    pub projection: Projection,
    pub up: UpAxis,
    /// Orbit about a fixed up vector (Eagle's default) or freely.
    pub fixed_up: bool,
    /// The details panel.
    pub info: bool,
    /// The background; `None` is the viewer's own (Eagle's dark theme
    /// leaves the model viewer transparent).
    pub background: Option<[u8; 3]>,
    /// The colour of meshes without one, Eagle's (200, 200, 200).
    pub default_color: [u8; 3],
    pub theme: ViewerTheme,
    /// The model tree navigator (files, meshes, materials) on the left.
    pub tree: bool,
    /// The measure tool: clicks on the model pick points.
    pub measuring: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            shading: Shading::Auto,
            edges: false,
            edge_color: [0, 0, 0],
            edge_threshold: 1.0,
            grid: false,
            axes: false,
            projection: Projection::Perspective,
            up: UpAxis::Y,
            fixed_up: true,
            info: true,
            background: None,
            default_color: [200, 200, 200],
            theme: ViewerTheme::Auto,
            tree: false,
            measuring: false,
        }
    }
}

impl Settings {
    /// The background drawn: the chosen colour, else the theme's (white
    /// for light, Eagle's dark grey for dark), else `None` (the viewer's
    /// own, following falcon's theme).
    pub fn effective_background(&self) -> Option<[u8; 3]> {
        self.background.or(match self.theme {
            ViewerTheme::Auto => None,
            ViewerTheme::Light => Some([255, 255, 255]),
            ViewerTheme::Dark => Some([42, 43, 46]),
        })
    }
}

/// Floats per vertex: position, normal, colour.
pub(crate) const VERTEX_FLOATS: usize = 9;

static GENERATION: AtomicU64 = AtomicU64::new(1);

/// A fresh tag for vertex data, so the GPU side uploads it once.
fn generation() -> u64 {
    GENERATION.fetch_add(1, Ordering::Relaxed)
}

/// Vertex data ready to upload, tagged with its generation.
#[derive(Clone)]
pub(crate) struct Vertices {
    pub generation: u64,
    pub bytes: Arc<Vec<u8>>,
    pub count: u32,
}

impl Vertices {
    fn new(floats: &[f32]) -> Vertices {
        let mut bytes = Vec::with_capacity(floats.len() * 4);
        for f in floats {
            bytes.extend_from_slice(&f.to_ne_bytes());
        }
        Vertices { generation: generation(), bytes: Arc::new(bytes), count: (floats.len() / VERTEX_FLOATS) as u32 }
    }
}

fn unit(c: [u8; 3]) -> V3 {
    c.map(|v| f32::from(v) / 255.0)
}

/// The triangles of `model` as a plain triangle list (no index buffer),
/// with a normal and colour per corner: vertex colours first, then the
/// material's, then `default_color`. Meshes marked in `hidden` are left out.
pub(crate) fn mesh_vertices(model: &Model, shading: Shading, default_color: V3, hidden: &[bool]) -> Vec<f32> {
    let mut out = Vec::with_capacity(model.triangle_count() * 3 * VERTEX_FLOATS);
    for (k, mesh) in model.meshes.iter().enumerate() {
        if hidden.get(k) == Some(&true) {
            continue;
        }
        let smooth = match shading {
            Shading::Flat => None,
            Shading::Smooth => Some(geom::smooth_normals(mesh)),
            Shading::Auto => mesh.normals.is_some().then(|| geom::smooth_normals(mesh)),
        };
        let base = mesh.material.as_ref().map_or(default_color, |m| m.color);
        for t in mesh.indices.as_chunks::<3>().0 {
            let p = t.map(|i| mesh.positions[i as usize]);
            let face = geom::face_normal(p[0], p[1], p[2]);
            for (k, &i) in t.iter().enumerate() {
                let n = smooth.as_ref().map_or(face, |s| s[i as usize]);
                let c = mesh.colors.as_ref().map_or(base, |c| c[i as usize]);
                out.extend_from_slice(&p[k]);
                out.extend_from_slice(&n);
                out.extend_from_slice(&c);
            }
        }
    }
    out
}

/// Line segments in the same vertex layout, all in `color`.
pub(crate) fn line_vertices(out: &mut Vec<f32>, segments: &[[V3; 2]], color: V3) {
    for s in segments {
        for p in s {
            out.extend_from_slice(p);
            out.extend_from_slice(&[0.0; 3]);
            out.extend_from_slice(&color);
        }
    }
}

/// The colour of the grid's lines over `background` (`None`: the
/// viewer's), light on dark and dark on light.
pub(crate) fn grid_color(background: [u8; 3]) -> V3 {
    let luma = 0.299 * f32::from(background[0]) + 0.587 * f32::from(background[1]) + 0.114 * f32::from(background[2]);
    if luma < 128.0 { [0.32, 0.33, 0.35] } else { [0.72, 0.72, 0.74] }
}

/// The line buffer: edges, then grid, then axes; the vertex ranges of each.
#[derive(Clone)]
pub(crate) struct Lines {
    pub vertices: Vertices,
    pub edges: std::ops::Range<u32>,
    pub grid: std::ops::Range<u32>,
    pub axes: std::ops::Range<u32>,
}

/// What a line buffer was built from, so it is rebuilt only on change.
#[derive(Clone, PartialEq)]
struct LinesKey {
    edges: Option<([u8; 3], u32, Vec<bool>)>,
    grid: Option<(UpAxis, [u8; 3])>,
    axes: bool,
}

/// What the background read sends back.
type Loaded = Result<(Arc<Model>, Stats), String>;

/// What the triangle data was built from: shading, default colour and the
/// hidden meshes.
type MeshKey = (Shading, [u8; 3], Vec<bool>);
/// A feature edge and the mesh it belongs to.
type MeshEdge = (usize, [V3; 2]);

/// A model being read or shown.
pub(crate) struct ModelView {
    pub id: String,
    loading: Option<Receiver<Loaded>>,
    pub model: Option<Arc<Model>>,
    pub stats: Option<Stats>,
    pub error: Option<String>,
    pub camera: Camera,
    /// The camera still has to be fitted (once the view's size is known).
    pub needs_fit: bool,
    mesh: Option<(MeshKey, Vertices)>,
    /// Feature edges by threshold (in hundredths of a degree), with the
    /// mesh each belongs to.
    edges: Option<(u32, Arc<Vec<MeshEdge>>)>,
    lines: Option<(LinesKey, Lines)>,
    /// Meshes hidden in the tree navigator, by index.
    pub hidden: Vec<bool>,
    /// The measure tool's picked points (at most two).
    pub picks: Vec<m3d::export::Hit>,
}

impl ModelView {
    /// Starts reading the model at `path` on a background thread.
    pub fn open(ctx: &eframe::egui::Context, id: &str, path: std::path::PathBuf, ext: &str, up: UpAxis) -> ModelView {
        let (tx, rx) = std::sync::mpsc::channel();
        let ext = ext.to_string();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new().name("falcon-model3d".into()).spawn(move || {
            let r = m3d::load(&path, &ext).map(|m| {
                let s = geom::stats(&m);
                (Arc::new(m), s)
            });
            let _ = tx.send(r.map_err(|e| e.0));
            ctx.request_repaint();
        });
        let mut view = ModelView {
            id: id.to_string(),
            loading: Some(rx),
            model: None,
            stats: None,
            error: None,
            camera: Camera::home(up),
            needs_fit: true,
            mesh: None,
            edges: None,
            lines: None,
            hidden: Vec::new(),
            picks: Vec::new(),
        };
        if let Err(e) = spawned {
            view.loading = None;
            view.error = Some(e.to_string());
        }
        view
    }

    /// Takes the background read's result once it is there. Returns true
    /// while still reading.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = &self.loading else { return false };
        match rx.try_recv() {
            Ok(Ok((m, s))) => {
                self.model = Some(m);
                self.stats = Some(s);
            }
            Ok(Err(e)) => self.error = Some(e),
            Err(TryRecvError::Empty) => return true,
            Err(TryRecvError::Disconnected) => self.error = Some("the model could not be read".into()),
        }
        self.loading = None;
        false
    }

    /// Fits the visible meshes in a view `aspect` wide per unit high.
    pub fn fit(&mut self, aspect: f32) {
        let all: Vec<usize> = (0..self.model.as_ref().map_or(0, |m| m.meshes.len())).filter(|&k| !self.is_hidden(k)).collect();
        self.fit_meshes(&all, aspect);
        self.needs_fit = false;
    }

    /// Fits the meshes `which` (indices) in the view.
    pub fn fit_meshes(&mut self, which: &[usize], aspect: f32) {
        let Some(model) = &self.model else { return };
        let part = Model { meshes: which.iter().filter_map(|&k| model.meshes.get(k).cloned()).collect() };
        if let Some((c, r)) = part.bounding_sphere() {
            self.camera.fit(c, r, aspect);
        }
    }

    pub fn is_hidden(&self, k: usize) -> bool {
        self.hidden.get(k) == Some(&true)
    }

    /// Shows or hides mesh `k`.
    pub fn set_hidden(&mut self, k: usize, hide: bool) {
        let n = self.model.as_ref().map_or(0, |m| m.meshes.len());
        self.hidden.resize(n, false);
        if let Some(h) = self.hidden.get_mut(k) {
            *h = hide;
        }
        self.picks.retain(|p| p.mesh != k || !hide);
    }

    /// Shows only the meshes for which `keep` holds; when exactly those are
    /// already the visible ones, shows every mesh again (a second isolate
    /// undoes the first).
    pub fn isolate(&mut self, keep: impl Fn(usize) -> bool) {
        let n = self.model.as_ref().map_or(0, |m| m.meshes.len());
        let want: Vec<bool> = (0..n).map(|k| !keep(k)).collect();
        let now: Vec<bool> = (0..n).map(|k| self.is_hidden(k)).collect();
        self.hidden = if now == want { vec![false; n] } else { want };
        let hidden = self.hidden.clone();
        self.picks.retain(|p| hidden.get(p.mesh) != Some(&true));
    }

    /// The visible meshes, for export and snapshots.
    pub fn visible_meshes(&self) -> Vec<&m3d::Mesh> {
        self.model.iter().flat_map(|m| m.meshes.iter().enumerate()).filter(|(k, _)| !self.is_hidden(*k)).map(|(_, m)| m).collect()
    }

    /// Goes back to the home view for `up` and fits.
    pub fn home(&mut self, up: UpAxis, aspect: f32) {
        self.camera = Camera::home(up);
        self.fit(aspect);
    }

    pub fn radius(&self) -> f32 {
        self.model.as_ref().and_then(|m| m.bounding_sphere()).map_or(1.0, |(_, r)| r)
    }

    /// The triangle data for `settings`, rebuilt when shading or the
    /// default colour changes.
    pub fn mesh(&mut self, settings: &Settings) -> Option<Vertices> {
        let model = self.model.as_ref()?;
        let key = (settings.shading, settings.default_color, self.hidden.clone());
        if self.mesh.as_ref().is_none_or(|(k, _)| *k != key) {
            let v = Vertices::new(&mesh_vertices(model, settings.shading, unit(settings.default_color), &self.hidden));
            self.mesh = Some((key, v));
        }
        self.mesh.as_ref().map(|(_, v)| v.clone())
    }

    /// The line data (edges, grid, axes) for `settings`.
    pub fn lines(&mut self, settings: &Settings, background: [u8; 3]) -> Option<Lines> {
        let model = self.model.clone()?;
        let threshold = (settings.edge_threshold.clamp(0.0, 180.0) * 100.0).round() as u32;
        let key = LinesKey {
            edges: settings.edges.then_some((settings.edge_color, threshold, self.hidden.clone())),
            grid: settings.grid.then_some((settings.up, background)),
            axes: settings.axes,
        };
        if self.lines.as_ref().is_some_and(|(k, _)| *k == key) {
            return self.lines.as_ref().map(|(_, l)| l.clone());
        }
        let mut floats = Vec::new();
        if let Some((color, t, hidden)) = &key.edges {
            if self.edges.as_ref().is_none_or(|(k, _)| k != t) {
                let segs: Vec<MeshEdge> =
                    model.meshes.iter().enumerate().flat_map(|(k, m)| geom::feature_edges(m, *t as f32 / 100.0).into_iter().map(move |s| (k, s))).collect();
                self.edges = Some((*t, Arc::new(segs)));
            }
            if let Some((_, segs)) = &self.edges {
                let shown: Vec<[V3; 2]> = segs.iter().filter(|(k, _)| hidden.get(*k) != Some(&true)).map(|(_, s)| *s).collect();
                line_vertices(&mut floats, &shown, unit(*color));
            }
        }
        let e = (floats.len() / VERTEX_FLOATS) as u32;
        if let (Some((up, bg)), Some(bounds)) = (key.grid, model.bounds()) {
            line_vertices(&mut floats, &crate::logic::grid_lines(bounds, up), grid_color(bg));
        }
        let g = (floats.len() / VERTEX_FLOATS) as u32;
        if key.axes {
            for (seg, color) in crate::logic::axes(self.radius()) {
                line_vertices(&mut floats, &[seg], color);
            }
        }
        let a = (floats.len() / VERTEX_FLOATS) as u32;
        let lines = Lines { vertices: Vertices::new(&floats), edges: 0..e, grid: e..g, axes: g..a };
        self.lines = Some((key, lines.clone()));
        Some(lines)
    }
}

/// The viewer's state on `App`: whether the window draws with wgpu (the
/// viewer needs it; without it 3D items show their still), the settings,
/// and the open model.
pub(crate) struct Model3dState {
    pub target: Option<wgpu::TextureFormat>,
    pub settings: Settings,
    pub view: Option<ModelView>,
}

impl Model3dState {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Model3dState {
        Model3dState { target: cc.wgpu_render_state.as_ref().map(|r| r.target_format), settings: Settings::default(), view: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use falcon_library::model3d::{Material, Mesh};

    fn tri(colors: Option<Vec<V3>>, material: Option<Material>) -> Model {
        Model {
            meshes: vec![Mesh {
                positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                normals: Some(vec![[0.0, 0.0, 2.0]; 3]),
                colors,
                indices: vec![0, 1, 2],
                material,
                ..Default::default()
            }],
        }
    }

    #[test]
    fn vertices_take_the_nearest_colour() {
        let grey = [0.5; 3];
        let v = mesh_vertices(&tri(None, None), Shading::Flat, grey, &[]);
        assert_eq!(v.len(), 3 * VERTEX_FLOATS);
        assert_eq!(&v[6..9], &grey);
        let red = Material { name: "r".into(), color: [1.0, 0.0, 0.0], opacity: 1.0 };
        let v = mesh_vertices(&tri(None, Some(red.clone())), Shading::Flat, grey, &[]);
        assert_eq!(&v[6..9], &[1.0, 0.0, 0.0]);
        let v = mesh_vertices(&tri(Some(vec![[0.0, 1.0, 0.0]; 3]), Some(red)), Shading::Flat, grey, &[]);
        assert_eq!(&v[6..9], &[0.0, 1.0, 0.0]);
    }

    #[test]
    fn shading_picks_the_normals() {
        // Flat: the face normal. Auto and Smooth: the file's, normalised.
        let flat = mesh_vertices(&tri(None, None), Shading::Flat, [1.0; 3], &[]);
        assert_eq!(&flat[3..6], &[0.0, 0.0, 1.0]);
        let mut m = tri(None, None);
        m.meshes[0].normals = Some(vec![[0.0, 3.0, 0.0]; 3]);
        assert_eq!(&mesh_vertices(&m, Shading::Auto, [1.0; 3], &[])[3..6], &[0.0, 1.0, 0.0]);
        assert_eq!(&mesh_vertices(&m, Shading::Flat, [1.0; 3], &[])[3..6], &[0.0, 0.0, 1.0]);
        m.meshes[0].normals = None;
        assert_eq!(&mesh_vertices(&m, Shading::Auto, [1.0; 3], &[])[3..6], &[0.0, 0.0, 1.0]);
    }

    #[test]
    fn lines_are_two_vertices_each() {
        let mut out = Vec::new();
        line_vertices(&mut out, &[[[0.0; 3], [1.0, 0.0, 0.0]]], [1.0, 0.0, 0.0]);
        assert_eq!(out.len(), 2 * VERTEX_FLOATS);
        let v = Vertices::new(&out);
        assert_eq!(v.count, 2);
        assert_eq!(v.bytes.len(), out.len() * 4);
        assert_ne!(Vertices::new(&out).generation, v.generation);
        assert!(grid_color([0, 0, 0])[0] < grid_color([255, 255, 255])[0]);
    }

    #[test]
    fn the_theme_sets_the_background_unless_one_is_chosen() {
        let mut s = Settings::default();
        assert_eq!(s.effective_background(), None);
        s.theme = ViewerTheme::Dark;
        assert_eq!(s.effective_background(), Some([42, 43, 46]));
        s.background = Some([1, 2, 3]);
        assert_eq!(s.effective_background(), Some([1, 2, 3]));
    }

    #[test]
    fn defaults_follow_eagle() {
        let s = Settings::default();
        assert!(!s.edges && !s.grid && !s.axes && s.fixed_up);
        assert_eq!((s.edge_color, s.edge_threshold, s.default_color), ([0, 0, 0], 1.0, [200, 200, 200]));
        assert_eq!(s.up, UpAxis::Y);
    }

    #[test]
    fn hidden_meshes_are_left_out_and_isolate_toggles() {
        let mut m = tri(None, None);
        m.meshes.push(m.meshes[0].clone());
        assert_eq!(mesh_vertices(&m, Shading::Flat, [1.0; 3], &[true, false]).len(), 3 * VERTEX_FLOATS);
        let ctx = eframe::egui::Context::default();
        let mut v = ModelView::open(&ctx, "x", std::path::PathBuf::from("/nonexistent/x.obj"), "obj", UpAxis::Y);
        v.model = Some(Arc::new(m));
        v.isolate(|k| k == 1);
        assert!(v.is_hidden(0) && !v.is_hidden(1));
        assert_eq!(v.visible_meshes().len(), 1);
        v.isolate(|k| k == 1);
        assert!(!v.is_hidden(0) && !v.is_hidden(1), "isolating the same again shows everything");
        v.set_hidden(0, true);
        assert!(v.is_hidden(0));
    }
}
