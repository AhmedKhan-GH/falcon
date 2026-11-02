//! IFC (`.ifc`, the STEP text form). Geometry comes from web-ifc, the
//! engine Eagle's viewer uses, when falcon is built with it
//! (`scripts/fetch-web-ifc.sh`; see [`super::ifc_native`]): extruded,
//! revolved and swept solids, booleans and openings, CSG primitives,
//! advanced breps and everything tessellated.
//!
//! Without web-ifc, or when it cannot read a file, falcon's own reader
//! below draws the tessellated part: `IfcTriangulatedFaceSet`,
//! `IfcPolygonalFaceSet` (faces with holes) and faceted breps
//! (`IfcFacetedBrep`, shells, `IfcFace` with `IfcPolyLoop` bounds), also
//! inside shell- and face-based surface models. Products are placed
//! through `IfcProductDefinitionShape`, their shape representations (Body
//! first), `IfcMappedItem` (representation map origin and transformation
//! operator) and the `IfcLocalPlacement` chain. Lengths are converted to
//! metres from the project's length unit and the model is turned from
//! IFC's Z-up to the viewer's Y-up. Colours come from `IfcStyledItem`
//! surface styles on the geometry, else from the product's (or its
//! type's) material style, else from a default per entity type. Swept
//! solids, CSG, booleans and advanced breps are not read by it: such
//! products are counted as skipped, and a file with nothing drawable is
//! an error ([`NO_GEOMETRY`]), so the item keeps its QuickLook still.

use std::collections::{HashMap, HashSet};

use super::earclip::{self, P3};
use super::math::V3;
use super::step::{self, Entity, StepFile, Value};
use super::{DEFAULT_COLOR, Material, Mesh, Model, ParseError, Result};

/// The reason given for a file with nothing falcon can draw.
pub const NO_GEOMETRY: &str = "IFC: no tessellated geometry";

/// Whether falcon reads IFC geometry through web-ifc (every solid kind)
/// rather than only the tessellated part.
pub fn full_geometry() -> bool {
    super::ifc_native::available()
}

/// Files larger than this are not read (the whole instance graph is held
/// in memory).
pub const MAX_BYTES: usize = 256 << 20;

/// How deeply mapped items may nest (also stops cycles).
const MAX_MAP_DEPTH: usize = 8;

/// A placed model this far from the origin (in metres) is moved to it, so
/// georeferenced coordinates keep their precision as `f32`.
const RECENTRE_BEYOND: f64 = 10_000.0;

/// What reading a file found, for the inspector.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    /// The schema named in the header, e.g. `IFC4`.
    pub schema: String,
    /// Products drawn (with at least one triangle).
    pub shown: usize,
    /// Products with a shape that has no tessellated geometry.
    pub skipped: usize,
    pub triangles: usize,
    /// The geometry types that were not read, by display name, most
    /// common first.
    pub unsupported: Vec<(String, usize)>,
}

/// Reads an IFC file as a model.
pub fn parse(bytes: &[u8]) -> Result<Model> {
    let (model, _) = read(bytes)?;
    model.ok_or_else(|| ParseError::new(NO_GEOMETRY))
}

/// Reads the model and the report; the model is `None` when nothing could
/// be drawn. Only unreadable files are errors.
pub fn read(bytes: &[u8]) -> Result<(Option<Model>, Report)> {
    if bytes.len() > MAX_BYTES {
        return Err(ParseError::new("IFC: the file is too large"));
    }
    let file = step::parse(bytes)?;
    if let Some(r) = super::ifc_native::read(&file, bytes)? {
        return Ok(r);
    }
    Ok(Reader::new(&file).run())
}

/// Reads the file with falcon's own (tessellated-only) reader, never
/// through web-ifc.
pub fn read_tessellated(bytes: &[u8]) -> Result<(Option<Model>, Report)> {
    if bytes.len() > MAX_BYTES {
        return Err(ParseError::new("IFC: the file is too large"));
    }
    let file = step::parse(bytes)?;
    Ok(Reader::new(&file).run())
}

/// The report for the file at `path`, without keeping the model.
pub fn summary(path: &std::path::Path) -> Result<Report> {
    let bytes = std::fs::read(path).map_err(ParseError::new)?;
    read(&bytes).map(|(_, r)| r)
}

/// Entity types never drawn: voids, spaces and helpers that would hide
/// the building.
pub(super) const HIDDEN: &[&str] = &[
    "IFCOPENINGELEMENT",
    "IFCOPENINGSTANDARDCASE",
    "IFCVOIDINGFEATURE",
    "IFCSPACE",
    "IFCSPATIALZONE",
    "IFCVIRTUALELEMENT",
    "IFCANNOTATION",
    "IFCGRID",
];

/// Display names and default colours for common entity types.
const TYPES: &[(&str, &str, Option<V3>)] = &[
    ("IFCWALL", "IfcWall", Some([0.92, 0.90, 0.85])),
    ("IFCWALLSTANDARDCASE", "IfcWallStandardCase", Some([0.92, 0.90, 0.85])),
    ("IFCWALLELEMENTEDCASE", "IfcWallElementedCase", Some([0.92, 0.90, 0.85])),
    ("IFCCURTAINWALL", "IfcCurtainWall", Some([0.55, 0.70, 0.80])),
    ("IFCSLAB", "IfcSlab", Some([0.68, 0.68, 0.68])),
    ("IFCSLABSTANDARDCASE", "IfcSlabStandardCase", Some([0.68, 0.68, 0.68])),
    ("IFCSLABELEMENTEDCASE", "IfcSlabElementedCase", Some([0.68, 0.68, 0.68])),
    ("IFCROOF", "IfcRoof", Some([0.64, 0.36, 0.30])),
    ("IFCWINDOW", "IfcWindow", Some([0.55, 0.75, 0.92])),
    ("IFCWINDOWSTANDARDCASE", "IfcWindowStandardCase", Some([0.55, 0.75, 0.92])),
    ("IFCDOOR", "IfcDoor", Some([0.60, 0.42, 0.28])),
    ("IFCDOORSTANDARDCASE", "IfcDoorStandardCase", Some([0.60, 0.42, 0.28])),
    ("IFCPLATE", "IfcPlate", Some([0.62, 0.80, 0.90])),
    ("IFCCOLUMN", "IfcColumn", Some([0.62, 0.65, 0.72])),
    ("IFCCOLUMNSTANDARDCASE", "IfcColumnStandardCase", Some([0.62, 0.65, 0.72])),
    ("IFCBEAM", "IfcBeam", Some([0.56, 0.60, 0.70])),
    ("IFCBEAMSTANDARDCASE", "IfcBeamStandardCase", Some([0.56, 0.60, 0.70])),
    ("IFCMEMBER", "IfcMember", Some([0.50, 0.55, 0.62])),
    ("IFCMEMBERSTANDARDCASE", "IfcMemberStandardCase", Some([0.50, 0.55, 0.62])),
    ("IFCSTAIR", "IfcStair", Some([0.76, 0.72, 0.64])),
    ("IFCSTAIRFLIGHT", "IfcStairFlight", Some([0.76, 0.72, 0.64])),
    ("IFCRAMP", "IfcRamp", Some([0.76, 0.72, 0.64])),
    ("IFCRAMPFLIGHT", "IfcRampFlight", Some([0.76, 0.72, 0.64])),
    ("IFCRAILING", "IfcRailing", Some([0.35, 0.35, 0.38])),
    ("IFCCOVERING", "IfcCovering", Some([0.85, 0.82, 0.74])),
    ("IFCFOOTING", "IfcFooting", Some([0.55, 0.55, 0.52])),
    ("IFCPILE", "IfcPile", Some([0.55, 0.55, 0.52])),
    ("IFCFURNISHINGELEMENT", "IfcFurnishingElement", Some([0.72, 0.55, 0.38])),
    ("IFCFURNITURE", "IfcFurniture", Some([0.72, 0.55, 0.38])),
    ("IFCSITE", "IfcSite", Some([0.46, 0.62, 0.38])),
    ("IFCGEOGRAPHICELEMENT", "IfcGeographicElement", Some([0.46, 0.62, 0.38])),
    ("IFCBUILDING", "IfcBuilding", None),
    ("IFCBUILDINGSTOREY", "IfcBuildingStorey", None),
    ("IFCBUILDINGELEMENTPROXY", "IfcBuildingElementProxy", None),
    ("IFCFLOWSEGMENT", "IfcFlowSegment", Some([0.55, 0.66, 0.56])),
    ("IFCFLOWFITTING", "IfcFlowFitting", Some([0.55, 0.66, 0.56])),
    ("IFCFLOWTERMINAL", "IfcFlowTerminal", Some([0.80, 0.80, 0.84])),
    ("IFCPIPESEGMENT", "IfcPipeSegment", Some([0.55, 0.66, 0.56])),
    ("IFCDUCTSEGMENT", "IfcDuctSegment", Some([0.66, 0.70, 0.74])),
    ("IFCTRIANGULATEDFACESET", "IfcTriangulatedFaceSet", None),
    ("IFCPOLYGONALFACESET", "IfcPolygonalFaceSet", None),
    ("IFCFACETEDBREP", "IfcFacetedBrep", None),
    ("IFCEXTRUDEDAREASOLID", "IfcExtrudedAreaSolid", None),
    ("IFCEXTRUDEDAREASOLIDTAPERED", "IfcExtrudedAreaSolidTapered", None),
    ("IFCREVOLVEDAREASOLID", "IfcRevolvedAreaSolid", None),
    ("IFCSWEPTDISKSOLID", "IfcSweptDiskSolid", None),
    ("IFCSURFACECURVESWEPTAREASOLID", "IfcSurfaceCurveSweptAreaSolid", None),
    ("IFCFIXEDREFERENCESWEPTAREASOLID", "IfcFixedReferenceSweptAreaSolid", None),
    ("IFCBOOLEANRESULT", "IfcBooleanResult", None),
    ("IFCBOOLEANCLIPPINGRESULT", "IfcBooleanClippingResult", None),
    ("IFCADVANCEDBREP", "IfcAdvancedBrep", None),
    ("IFCADVANCEDBREPWITHVOIDS", "IfcAdvancedBrepWithVoids", None),
    ("IFCBSPLINESURFACEWITHKNOTS", "IfcBSplineSurfaceWithKnots", None),
    ("IFCCSGSOLID", "IfcCsgSolid", None),
    ("IFCBLOCK", "IfcBlock", None),
    ("IFCSPHERE", "IfcSphere", None),
    ("IFCHALFSPACESOLID", "IfcHalfSpaceSolid", None),
    ("IFCPOLYGONALBOUNDEDHALFSPACE", "IfcPolygonalBoundedHalfSpace", None),
    ("IFCBOUNDINGBOX", "IfcBoundingBox", None),
    ("IFCPOLYLINE", "IfcPolyline", None),
    ("IFCINDEXEDPOLYCURVE", "IfcIndexedPolyCurve", None),
];

/// An entity name as IFC writes it (`IFCWALL` → `IfcWall`); names not in
/// the table keep only their first letter after `Ifc` in capitals.
pub fn display_name(upper: &str) -> String {
    if let Some(t) = TYPES.iter().find(|t| t.0 == upper) {
        return t.1.to_string();
    }
    match upper.strip_prefix("IFC") {
        Some(rest) if !rest.is_empty() => {
            let lower = rest.to_ascii_lowercase();
            format!("Ifc{}{}", lower[..1].to_ascii_uppercase(), &lower[1..])
        }
        _ => upper.to_string(),
    }
}

/// How a shape representation ranks by its identifier: Body first, then
/// Facetation, then anything else; `None` for the ones that are never
/// drawn (axis, footprint, bounding box, annotation and the like).
pub(super) fn rep_rank(identifier: &str) -> Option<u8> {
    match identifier.to_ascii_lowercase().as_str() {
        "body" => Some(0),
        "facetation" => Some(1),
        "axis" | "footprint" | "box" | "annotation" | "profile" | "clearance" | "cog" | "lighting" | "reference" | "plan" => None,
        _ => Some(2),
    }
}

fn default_color(upper: &str) -> V3 {
    TYPES.iter().find(|t| t.0 == upper).and_then(|t| t.2).unwrap_or(DEFAULT_COLOR)
}

fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: P3, b: P3) -> P3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn normalize(a: P3) -> Option<P3> {
    let l = dot(a, a).sqrt();
    (l > 1e-12 && l.is_finite()).then(|| [a[0] / l, a[1] / l, a[2] / l])
}

/// An affine transform: three rows of a 4×4 matrix.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Xf([[f64; 4]; 3]);

const IDENTITY: Xf = Xf([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]]);

impl Xf {
    /// The frame with axes `x`, `y`, `z` (as columns) at `o`.
    fn from_axes(o: P3, x: P3, y: P3, z: P3) -> Xf {
        Xf([[x[0], y[0], z[0], o[0]], [x[1], y[1], z[1], o[1]], [x[2], y[2], z[2], o[2]]])
    }

    /// A right-handed frame at `o` with Z along `z` and X as close to `x`
    /// as possible.
    fn frame(o: P3, z: Option<P3>, x: Option<P3>) -> Xf {
        let z = z.and_then(normalize).unwrap_or([0.0, 0.0, 1.0]);
        let x0 = x.and_then(normalize).unwrap_or([1.0, 0.0, 0.0]);
        let x = normalize(sub(x0, scale(z, dot(x0, z))))
            .or_else(|| normalize(sub([1.0, 0.0, 0.0], scale(z, z[0]))))
            .or_else(|| normalize(sub([0.0, 1.0, 0.0], scale(z, z[1]))))
            .unwrap_or([1.0, 0.0, 0.0]);
        let y = cross(z, x);
        Xf::from_axes(o, x, y, z)
    }

    /// `self` after `inner`: `inner` is applied first.
    fn then(&self, inner: &Xf) -> Xf {
        let (a, b) = (&self.0, &inner.0);
        let mut m = [[0.0; 4]; 3];
        for r in 0..3 {
            for c in 0..4 {
                m[r][c] = (0..3).map(|k| a[r][k] * b[k][c]).sum::<f64>() + if c == 3 { a[r][3] } else { 0.0 };
            }
        }
        Xf(m)
    }

    fn point(&self, p: P3) -> P3 {
        let m = &self.0;
        [0, 1, 2].map(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3])
    }

    fn vector(&self, v: P3) -> P3 {
        let m = &self.0;
        [0, 1, 2].map(|r| m[r][0] * v[0] + m[r][1] * v[1] + m[r][2] * v[2])
    }
}

fn scale(a: P3, s: f64) -> P3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// A list of two or three numbers as a point (2D points get Z = 0).
fn coords(v: &Value) -> Option<P3> {
    let l = v.as_list()?;
    let n = |k: usize| l.get(k).and_then(Value::as_f64);
    let p = [n(0)?, n(1)?, n(2).unwrap_or(0.0)];
    p.iter().all(|c| c.is_finite()).then_some(p)
}

/// Triangles and vertices built for one product, in file units and world
/// (IFC) axes, with the material they are drawn in.
#[derive(Default)]
struct Piece {
    material: Option<Material>,
    positions: Vec<P3>,
    normals: Option<Vec<P3>>,
    indices: Vec<u32>,
}

impl Piece {
    fn new(material: Option<Material>) -> Piece {
        Piece { material, ..Default::default() }
    }
}

/// What meshes are grouped by: entity type, material name and colour.
type GroupKey = (String, String, [u32; 3]);

struct Reader<'a> {
    f: &'a StepFile,
    /// World transforms of local placements, by id.
    placements: HashMap<u64, Xf>,
    /// Surface styles by the id of the geometry they are attached to.
    item_styles: HashMap<u64, Material>,
    /// Material styles by product (or type) id.
    product_styles: HashMap<u64, Material>,
    /// Points of `IfcCartesianPoint` by id.
    points: HashMap<u64, Option<P3>>,
}

impl<'a> Reader<'a> {
    fn new(f: &'a StepFile) -> Reader<'a> {
        let mut r = Reader { f, placements: HashMap::new(), item_styles: HashMap::new(), product_styles: HashMap::new(), points: HashMap::new() };
        r.collect_styles();
        r
    }

    fn entities_named(&self, name: &'a str) -> impl Iterator<Item = (u64, &'a Entity)> + 'a {
        self.f.entities.iter().filter(move |(_, e)| e.name == name).map(|(&id, e)| (id, e))
    }

    fn point(&mut self, v: &Value) -> Option<P3> {
        let id = v.as_ref_id()?;
        if let Some(p) = self.points.get(&id) {
            return *p;
        }
        let p = self.f.deref_named(v, "IFCCARTESIANPOINT").and_then(|e| coords(e.arg(0)));
        self.points.insert(id, p);
        p
    }

    fn direction(&self, v: &Value) -> Option<P3> {
        self.f.deref_named(v, "IFCDIRECTION").and_then(|e| coords(e.arg(0)))
    }

    /// An `IfcAxis2Placement3D` (or 2D) as a transform.
    fn axis_placement(&mut self, v: &Value) -> Xf {
        let Some(e) = self.f.deref(v) else { return IDENTITY };
        match e.name.as_str() {
            "IFCAXIS2PLACEMENT3D" => {
                let o = self.point(e.arg(0)).unwrap_or([0.0; 3]);
                Xf::frame(o, self.direction(e.arg(1)), self.direction(e.arg(2)))
            }
            "IFCAXIS2PLACEMENT2D" => {
                let o = self.point(e.arg(0)).unwrap_or([0.0; 3]);
                Xf::frame(o, None, self.direction(e.arg(1)))
            }
            "IFCAXIS1PLACEMENT" => {
                let o = self.point(e.arg(0)).unwrap_or([0.0; 3]);
                Xf::frame(o, self.direction(e.arg(1)), None)
            }
            _ => IDENTITY,
        }
    }

    /// The world transform of an `IfcLocalPlacement`, composed through its
    /// `PlacementRelTo` chain.
    fn placement(&mut self, v: &Value) -> Xf {
        let mut chain = Vec::new();
        let mut seen = HashSet::new();
        let mut base = IDENTITY;
        let mut cur = v.as_ref_id();
        while let Some(id) = cur {
            if let Some(x) = self.placements.get(&id) {
                base = *x;
                break;
            }
            let Some(e) = self.f.get(id).filter(|e| e.name == "IFCLOCALPLACEMENT") else { break };
            if !seen.insert(id) {
                break;
            }
            chain.push(id);
            cur = e.arg(0).as_ref_id();
        }
        let mut x = base;
        for id in chain.into_iter().rev() {
            let e = self.f.get(id).expect("found above");
            x = x.then(&self.axis_placement(e.arg(1)));
            self.placements.insert(id, x);
        }
        x
    }

    /// An `IfcCartesianTransformationOperator3D` (uniform or not, or 2D)
    /// as a transform.
    fn operator(&mut self, v: &Value) -> Xf {
        let Some(e) = self.f.deref(v) else { return IDENTITY };
        if !e.name.starts_with("IFCCARTESIANTRANSFORMATIONOPERATOR") {
            return IDENTITY;
        }
        let three_d = e.name.contains("3D");
        let o = self.point(e.arg(2)).unwrap_or([0.0; 3]);
        let s = e.arg(3).as_f64().unwrap_or(1.0);
        let z = if three_d { self.direction(e.arg(4)) } else { None };
        let mut x = Xf::frame(o, z, self.direction(e.arg(0)));
        let (s2, s3) = match e.name.as_str() {
            "IFCCARTESIANTRANSFORMATIONOPERATOR3DNONUNIFORM" => (e.arg(5).as_f64().unwrap_or(s), e.arg(6).as_f64().unwrap_or(s)),
            "IFCCARTESIANTRANSFORMATIONOPERATOR2DNONUNIFORM" => (e.arg(4).as_f64().unwrap_or(s), s),
            _ => (s, s),
        };
        for row in &mut x.0 {
            row[0] *= s;
            row[1] *= s2;
            row[2] *= s3;
        }
        x
    }

    /// The colour of a list of presentation styles (or one style).
    fn style_material(&self, v: &Value, depth: usize) -> Option<Material> {
        if depth > 4 {
            return None;
        }
        let refs: Vec<&Value> = match v {
            Value::List(l) => l.iter().collect(),
            other => vec![other],
        };
        for r in refs {
            let Some(e) = self.f.deref(r) else { continue };
            match e.name.as_str() {
                "IFCPRESENTATIONSTYLEASSIGNMENT" => {
                    if let Some(m) = self.style_material(e.arg(0), depth + 1) {
                        return Some(m);
                    }
                }
                "IFCSURFACESTYLE" => {
                    let name = e.arg(0).as_str().unwrap_or("").to_string();
                    for s in e.arg(2).as_list().unwrap_or_default() {
                        let Some(se) = self.f.deref(s) else { continue };
                        if se.name != "IFCSURFACESTYLERENDERING" && se.name != "IFCSURFACESTYLESHADING" {
                            continue;
                        }
                        let Some(c) = self.f.deref_named(se.arg(0), "IFCCOLOURRGB") else { continue };
                        let ch = |k: usize| c.arg(k).as_f64().unwrap_or(0.8).clamp(0.0, 1.0) as f32;
                        let transparency = se.arg(1).as_f64().unwrap_or(0.0).clamp(0.0, 1.0) as f32;
                        return Some(Material { name, color: [ch(1), ch(2), ch(3)], opacity: 1.0 - transparency });
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Styles on geometry, and material styles of products and types.
    fn collect_styles(&mut self) {
        for (_, e) in self.entities_named("IFCSTYLEDITEM") {
            if let (Some(item), Some(m)) = (e.arg(0).as_ref_id(), self.style_material(e.arg(1), 0)) {
                self.item_styles.entry(item).or_insert(m);
            }
        }
        // IfcMaterial → its style, through its definition representation.
        let mut material_styles: HashMap<u64, Material> = HashMap::new();
        for (_, e) in self.entities_named("IFCMATERIALDEFINITIONREPRESENTATION") {
            let Some(mat_id) = e.arg(3).as_ref_id() else { continue };
            let mat_name = self.f.get(mat_id).and_then(|m| m.arg(0).as_str()).unwrap_or("").to_string();
            'reps: for rep in e.arg(2).as_list().unwrap_or_default() {
                let Some(re) = self.f.deref(rep) else { continue };
                for item in re.arg(3).as_list().unwrap_or_default() {
                    let Some(si) = self.f.deref_named(item, "IFCSTYLEDITEM") else { continue };
                    if let Some(mut m) = self.style_material(si.arg(1), 0) {
                        if m.name.is_empty() {
                            m.name = mat_name.clone();
                        }
                        material_styles.insert(mat_id, m);
                        break 'reps;
                    }
                }
            }
        }
        if material_styles.is_empty() {
            return;
        }
        for (_, e) in self.entities_named("IFCRELASSOCIATESMATERIAL") {
            let Some(m) = self.material_of(e.arg(5), 0).and_then(|id| material_styles.get(&id)) else { continue };
            for o in e.arg(4).as_list().unwrap_or_default() {
                if let Some(id) = o.as_ref_id() {
                    self.product_styles.entry(id).or_insert_with(|| m.clone());
                }
            }
        }
        // Products take their type's material when they have none.
        for (_, e) in self.entities_named("IFCRELDEFINESBYTYPE") {
            let Some(m) = e.arg(5).as_ref_id().and_then(|t| self.product_styles.get(&t)).cloned() else { continue };
            for o in e.arg(4).as_list().unwrap_or_default() {
                if let Some(id) = o.as_ref_id() {
                    self.product_styles.entry(id).or_insert_with(|| m.clone());
                }
            }
        }
    }

    /// The first `IfcMaterial` behind a material select (layer sets,
    /// lists, constituents, profiles).
    fn material_of(&self, v: &Value, depth: usize) -> Option<u64> {
        if depth > 4 {
            return None;
        }
        let id = v.as_ref_id()?;
        let e = self.f.get(id)?;
        let first = |k: usize| e.arg(k).as_list().and_then(|l| l.first());
        match e.name.as_str() {
            "IFCMATERIAL" => Some(id),
            "IFCMATERIALLAYERSETUSAGE" | "IFCMATERIALPROFILESETUSAGE" => self.material_of(e.arg(0), depth + 1),
            "IFCMATERIALLAYERSET" | "IFCMATERIALLIST" => self.material_of(first(0)?, depth + 1),
            "IFCMATERIALLAYER" => self.material_of(e.arg(0), depth + 1),
            "IFCMATERIALCONSTITUENTSET" | "IFCMATERIALPROFILESET" => self.material_of(first(2)?, depth + 1),
            "IFCMATERIALCONSTITUENT" | "IFCMATERIALPROFILE" => self.material_of(e.arg(2), depth + 1),
            _ => None,
        }
    }

    /// The scale from the project's length unit to metres.
    fn length_scale(&self) -> f64 {
        let project_units = self.entities_named("IFCPROJECT").next().and_then(|(_, p)| p.arg(8).as_ref_id());
        let assignments: Vec<&Entity> = project_units
            .and_then(|id| self.f.get(id))
            .into_iter()
            .chain(self.entities_named("IFCUNITASSIGNMENT").map(|(_, e)| e))
            .collect();
        for a in assignments {
            for u in a.arg(0).as_list().unwrap_or_default() {
                if let Some(s) = self.unit_scale(u, 0) {
                    return s;
                }
            }
        }
        1.0
    }

    /// The size of a length unit in metres; `None` for other units.
    fn unit_scale(&self, v: &Value, depth: usize) -> Option<f64> {
        let e = self.f.deref(v)?;
        if e.arg(1).as_enum() != Some("LENGTHUNIT") || depth > 4 {
            return None;
        }
        match e.name.as_str() {
            "IFCSIUNIT" => {
                if e.arg(3).as_enum() != Some("METRE") {
                    return None;
                }
                Some(match e.arg(2).as_enum() {
                    Some("KILO") => 1e3,
                    Some("HECTO") => 1e2,
                    Some("DECA") => 1e1,
                    Some("DECI") => 1e-1,
                    Some("CENTI") => 1e-2,
                    Some("MILLI") => 1e-3,
                    Some("MICRO") => 1e-6,
                    _ => 1.0,
                })
            }
            "IFCCONVERSIONBASEDUNIT" | "IFCCONVERSIONBASEDUNITWITHOFFSET" => {
                let m = self.f.deref_named(e.arg(3), "IFCMEASUREWITHUNIT")?;
                let value = m.arg(0).as_f64()?;
                let base = self.unit_scale(m.arg(1), depth + 1).unwrap_or(1.0);
                (value > 0.0 && value.is_finite()).then_some(value * base)
            }
            _ => None,
        }
    }

    /// `IfcCartesianPointList3D` coordinates.
    fn point_list(&self, v: &Value) -> Option<Vec<P3>> {
        let e = self.f.deref(v)?;
        if !e.name.starts_with("IFCCARTESIANPOINTLIST") {
            return None;
        }
        e.arg(0).as_list()?.iter().map(coords).collect()
    }

    /// Adds the triangles of one representation item to `out`, and the
    /// geometry types it can't read to `skipped`.
    fn item(&mut self, v: &Value, xf: &Xf, depth: usize, inherited: Option<&Material>, out: &mut Vec<Piece>, skipped: &mut Vec<String>) {
        let Some(id) = v.as_ref_id() else { return };
        let Some(e) = self.f.get(id) else { return };
        let style = self.item_styles.get(&id).cloned().or_else(|| inherited.cloned());
        match e.name.as_str() {
            "IFCTRIANGULATEDFACESET" | "IFCTRIANGULATEDIRREGULARNETWORK" => {
                if let Some(p) = self.triangulated(e, xf, style) {
                    out.push(p);
                }
            }
            "IFCPOLYGONALFACESET" => {
                if let Some(p) = self.polygonal(e, xf, style) {
                    out.push(p);
                }
            }
            "IFCFACETEDBREP" | "IFCFACETEDBREPWITHVOIDS" => {
                let mut p = Piece::new(style);
                let mut shells = vec![e.arg(0).clone()];
                shells.extend(e.arg(1).as_list().unwrap_or_default().iter().cloned());
                for s in &shells {
                    self.shell(s, xf, &mut p);
                }
                out.push(p);
            }
            "IFCCLOSEDSHELL" | "IFCOPENSHELL" | "IFCCONNECTEDFACESET" => {
                let mut p = Piece::new(style);
                self.shell(v, xf, &mut p);
                out.push(p);
            }
            "IFCSHELLBASEDSURFACEMODEL" | "IFCFACEBASEDSURFACEMODEL" => {
                let mut p = Piece::new(style);
                for s in e.arg(0).as_list().unwrap_or_default() {
                    self.shell(s, xf, &mut p);
                }
                out.push(p);
            }
            "IFCMAPPEDITEM" => {
                if depth >= MAX_MAP_DEPTH {
                    return;
                }
                let Some(map) = self.f.deref_named(e.arg(0), "IFCREPRESENTATIONMAP") else { return };
                let origin = self.axis_placement(map.arg(0));
                let target = self.operator(e.arg(1));
                let inner = xf.then(&target).then(&origin);
                let Some(rep) = self.f.deref(map.arg(1)) else { return };
                for it in rep.arg(3).as_list().unwrap_or_default() {
                    self.item(it, &inner, depth + 1, style.as_ref(), out, skipped);
                }
            }
            other => skipped.push(display_name(other)),
        }
    }

    /// An `IfcTriangulatedFaceSet`. The coordinate index, normals and point
    /// index are told apart by their shape, which covers both the IFC4
    /// and IFC4.3 attribute orders.
    fn triangulated(&mut self, e: &Entity, xf: &Xf, style: Option<Material>) -> Option<Piece> {
        let pts = self.point_list(e.arg(0))?;
        fn nested(v: &Value) -> Option<&[Value]> {
            v.as_list().filter(|l| !l.is_empty() && l.iter().all(|x| x.as_list().is_some()))
        }
        let mut index = None;
        let mut normals = None;
        let mut pn = None;
        for a in e.args.iter().skip(1) {
            if let Some(l) = nested(a) {
                let ints = l.iter().all(|t| t.as_list().is_some_and(|t| t.iter().all(|x| x.as_int().is_some())));
                if ints && index.is_none() {
                    index = Some(l);
                } else if !ints && normals.is_none() {
                    normals = Some(l);
                }
            } else if let Some(l) = a.as_list().filter(|l| !l.is_empty() && l.iter().all(|x| x.as_int().is_some())) {
                pn = Some(l);
            }
        }
        let index = index?;
        let lookup = |k: i64| -> Option<u32> {
            let k = match pn {
                Some(pn) => pn.get(usize::try_from(k - 1).ok()?)?.as_int()?,
                None => k,
            };
            let k = usize::try_from(k - 1).ok()?;
            (k < pts.len()).then_some(k as u32)
        };
        let mut p = Piece::new(style);
        p.positions = pts.iter().map(|&q| xf.point(q)).collect();
        for t in index {
            let t = t.as_list().unwrap_or_default();
            if t.len() < 3 {
                continue;
            }
            let ids: Option<Vec<u32>> = t.iter().take(3).map(|x| x.as_int().and_then(lookup)).collect();
            if let Some(ids) = ids {
                p.indices.extend(ids);
            }
        }
        if pn.is_none()
            && let Some(n) = normals
            && n.len() == pts.len()
        {
            let ns: Option<Vec<P3>> = n.iter().map(|v| coords(v).map(|c| xf.vector(c))).collect();
            p.normals = ns;
        }
        Some(p)
    }

    /// An `IfcPolygonalFaceSet`: each indexed face (with voids) cut into
    /// triangles over the shared points.
    fn polygonal(&mut self, e: &Entity, xf: &Xf, style: Option<Material>) -> Option<Piece> {
        let pts = self.point_list(e.arg(0))?;
        let pn: Option<Vec<i64>> = e.arg(3).as_list().map(|l| l.iter().filter_map(Value::as_int).collect());
        let resolve = |k: &Value| -> Option<usize> {
            let k = k.as_int()?;
            let k = match &pn {
                Some(pn) if !pn.is_empty() => *pn.get(usize::try_from(k - 1).ok()?)?,
                _ => k,
            };
            let k = usize::try_from(k - 1).ok()?;
            (k < pts.len()).then_some(k)
        };
        let loop_of = |v: &Value| -> Option<Vec<usize>> { v.as_list()?.iter().map(resolve).collect() };
        let mut p = Piece::new(style);
        p.positions = pts.iter().map(|&q| xf.point(q)).collect();
        for f in e.arg(2).as_list().unwrap_or_default() {
            let Some(fe) = self.f.deref(f) else { continue };
            if !fe.name.starts_with("IFCINDEXEDPOLYGONALFACE") {
                continue;
            }
            let Some(outer) = loop_of(fe.arg(0)) else { continue };
            let holes: Vec<Vec<usize>> = if fe.name == "IFCINDEXEDPOLYGONALFACEWITHVOIDS" {
                fe.arg(1).as_list().unwrap_or_default().iter().filter_map(loop_of).collect()
            } else {
                Vec::new()
            };
            let all: Vec<usize> = outer.iter().chain(holes.iter().flatten()).copied().collect();
            let o3: Vec<P3> = outer.iter().map(|&k| pts[k]).collect();
            let h3: Vec<Vec<P3>> = holes.iter().map(|h| h.iter().map(|&k| pts[k]).collect()).collect();
            let hr: Vec<&[P3]> = h3.iter().map(Vec::as_slice).collect();
            for t in earclip::triangulate_3d(&o3, &hr) {
                p.indices.extend(t.map(|k| all[k] as u32));
            }
        }
        Some(p)
    }

    /// The faces of a shell (or connected face set) into `p`.
    fn shell(&mut self, v: &Value, xf: &Xf, p: &mut Piece) {
        let Some(e) = self.f.deref(v) else { return };
        if !matches!(e.name.as_str(), "IFCCLOSEDSHELL" | "IFCOPENSHELL" | "IFCCONNECTEDFACESET") {
            return;
        }
        for face in e.arg(0).as_list().unwrap_or_default() {
            self.face(face, xf, p);
        }
    }

    /// One planar face with its poly-loop bounds, cut into triangles.
    fn face(&mut self, v: &Value, xf: &Xf, p: &mut Piece) {
        let Some(e) = self.f.deref(v) else { return };
        if e.name != "IFCFACE" && e.name != "IFCFACESURFACE" {
            return;
        }
        let mut outer: Option<Vec<P3>> = None;
        let mut loops: Vec<Vec<P3>> = Vec::new();
        for b in e.arg(0).as_list().unwrap_or_default() {
            let Some(be) = self.f.deref(b) else { continue };
            let Some(pl) = self.f.deref_named(be.arg(0), "IFCPOLYLOOP") else { continue };
            let refs = pl.arg(0).as_list().unwrap_or_default();
            let mut pts: Vec<P3> = refs.iter().filter_map(|r| self.point(r)).collect();
            // A closing point that repeats the first.
            if pts.len() > 3 && pts.first() == pts.last() {
                pts.pop();
            }
            if pts.len() < 3 {
                continue;
            }
            if be.arg(1).as_enum() == Some("F") {
                pts.reverse();
            }
            if be.name == "IFCFACEOUTERBOUND" && outer.is_none() {
                outer = Some(pts);
            } else {
                loops.push(pts);
            }
        }
        let outer = match outer {
            Some(o) => o,
            None => {
                // No outer bound marked: the largest loop is the outline.
                let area = |l: &Vec<P3>| {
                    let n = earclip::newell(l);
                    dot(n, n)
                };
                let Some(k) = (0..loops.len()).max_by(|&a, &b| area(&loops[a]).total_cmp(&area(&loops[b]))) else { return };
                loops.swap_remove(k)
            }
        };
        let hr: Vec<&[P3]> = loops.iter().map(Vec::as_slice).collect();
        let tris = earclip::triangulate_3d(&outer, &hr);
        if tris.is_empty() {
            return;
        }
        let base = p.positions.len() as u32;
        p.positions.extend(outer.iter().chain(loops.iter().flatten()).map(|&q| xf.point(q)));
        for t in tris {
            p.indices.extend(t.map(|k| base + k as u32));
        }
    }

    /// The triangles of a product's shape: the Body representation first,
    /// then other drawable ones if Body has none.
    fn product(&mut self, e: &Entity, pds: &Entity, style: Option<&Material>, skipped: &mut Vec<String>) -> Vec<Piece> {
        let xf = self.placement(e.arg(5));
        let mut ranked: Vec<(u8, &Entity)> = Vec::new();
        for r in pds.arg(2).as_list().unwrap_or_default() {
            let Some(re) = self.f.deref(r) else { continue };
            let Some(rank) = rep_rank(re.arg(1).as_str().unwrap_or("")) else { continue };
            ranked.push((rank, re));
        }
        ranked.sort_by_key(|r| r.0);
        let mut k = 0;
        while k < ranked.len() {
            let rank = ranked[k].0;
            let mut out = Vec::new();
            while k < ranked.len() && ranked[k].0 == rank {
                for it in ranked[k].1.arg(3).as_list().unwrap_or_default() {
                    self.item(it, &xf, 0, style, &mut out, skipped);
                }
                k += 1;
            }
            out.retain(|p| !p.indices.is_empty());
            if !out.is_empty() {
                return out;
            }
        }
        Vec::new()
    }

    fn run(mut self) -> (Option<Model>, Report) {
        let f = self.f;
        let mut report = Report { schema: f.schemas.first().cloned().unwrap_or_default(), ..Default::default() };
        let unit = self.length_scale();
        let mut ids: Vec<u64> = f.entities.keys().copied().collect();
        ids.sort_unstable();
        // Meshes in progress (with whether every piece had normals), by
        // (type, material name, colour bits).
        let mut groups: Vec<(GroupKey, Mesh, bool)> = Vec::new();
        let mut where_: HashMap<GroupKey, usize> = HashMap::new();
        let mut unsupported: HashMap<String, usize> = HashMap::new();
        let mut world: Vec<Vec<[f64; 3]>> = Vec::new();
        for id in ids {
            let e = &f.entities[&id];
            if !e.name.starts_with("IFC") || HIDDEN.contains(&e.name.as_str()) {
                continue;
            }
            let Some(pds) = f.deref_named(e.arg(6), "IFCPRODUCTDEFINITIONSHAPE") else { continue };
            let style = self.product_styles.get(&id).cloned();
            let mut skipped = Vec::new();
            let pieces = self.product(e, pds, style.as_ref(), &mut skipped);
            if pieces.is_empty() {
                report.skipped += 1;
                for s in skipped {
                    *unsupported.entry(s).or_default() += 1;
                }
                continue;
            }
            report.shown += 1;
            let type_name = display_name(&e.name);
            for piece in pieces {
                let mat = piece.material.unwrap_or_else(|| Material { name: type_name.clone(), color: default_color(&e.name), opacity: 1.0 });
                let key = (type_name.clone(), mat.name.clone(), mat.color.map(f32::to_bits));
                let g = *where_.entry(key.clone()).or_insert_with(|| {
                    groups.push((key, Mesh { name: type_name.clone(), material: Some(mat.clone()), normals: Some(Vec::new()), ..Default::default() }, true));
                    world.push(Vec::new());
                    groups.len() - 1
                });
                let (_, mesh, with_normals) = &mut groups[g];
                let base = world[g].len() as u32;
                report.triangles += piece.indices.len() / 3;
                mesh.indices.extend(piece.indices.iter().map(|i| base + i));
                // Z-up to Y-up, in metres.
                world[g].extend(piece.positions.iter().map(|q| [q[0] * unit, q[2] * unit, -q[1] * unit]));
                match (piece.normals, mesh.normals.as_mut()) {
                    (Some(ns), Some(out)) if *with_normals => out.extend(ns.iter().map(|n| {
                        let n = normalize(*n).unwrap_or([0.0, 0.0, 1.0]);
                        [n[0] as f32, n[2] as f32, -n[1] as f32]
                    })),
                    _ => {
                        *with_normals = false;
                        mesh.normals = None;
                    }
                }
            }
        }
        let mut unsupported: Vec<(String, usize)> = unsupported.into_iter().collect();
        unsupported.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        report.unsupported = unsupported;
        if report.triangles == 0 {
            return (None, report);
        }
        // Move far-off (georeferenced) models to the origin.
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for q in world.iter().flatten() {
            for k in 0..3 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
        }
        let centre = [0, 1, 2].map(|k| (lo[k] + hi[k]) / 2.0);
        let shift = if centre.iter().any(|c| c.abs() > RECENTRE_BEYOND) { centre } else { [0.0; 3] };
        let mut meshes = Vec::with_capacity(groups.len());
        for ((_, mut mesh, _), pts) in groups.into_iter().zip(world) {
            mesh.positions = pts.iter().map(|q| [(q[0] - shift[0]) as f32, (q[1] - shift[1]) as f32, (q[2] - shift[2]) as f32]).collect();
            meshes.push(mesh);
        }
        match (Model { meshes }).finish() {
            Ok(m) => {
                report.triangles = m.triangle_count();
                (Some(m), report)
            }
            Err(_) => {
                report.triangles = 0;
                (None, report)
            }
        }
    }
}

#[cfg(test)]
#[path = "ifc_tests.rs"]
mod tests;
