//! 3MF: a zip holding the model as XML.
//!
//! Reads the core specification plus the colour parts of the materials
//! extension: base materials and colour groups, applied per object or per
//! triangle. Build items and components are flattened into world-space
//! meshes, one per distinct colour of each object instance.

use std::collections::HashMap;
use std::io::Read;

use super::math::{self, M4, V3};
use super::{Material, Mesh, Model, ParseError, Result};

/// Most bytes read from one zip entry, against zip bombs.
const MAX_PART: u64 = 1 << 30;
/// Deepest component nesting followed.
const MAX_DEPTH: usize = 32;
/// Most object instances made, against component fan-out.
const MAX_INSTANCES: usize = 100_000;

pub fn parse(bytes: &[u8]) -> Result<Model> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| ParseError(format!("not a 3MF (zip) file: {e}")))?;
    let name = find_model_part(&mut zip).ok_or_else(|| ParseError::new("the 3MF has no 3D model part"))?;
    let data = read_part(&mut zip, &name).ok_or_else(|| ParseError(format!("cannot read {name} in the 3MF")))?;
    let doc = parse_document(&String::from_utf8_lossy(&data));
    doc.flatten()
}

fn entry_name(zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, wanted: &str) -> Option<String> {
    let wanted = wanted.trim_start_matches('/');
    (0..zip.len()).find_map(|i| {
        let f = zip.by_index_raw(i).ok()?;
        let full = String::from_utf8_lossy(f.name_raw()).into_owned();
        (full.trim_start_matches('/').eq_ignore_ascii_case(wanted) && !f.is_dir()).then_some(full)
    })
}

fn read_part(zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, name: &str) -> Option<Vec<u8>> {
    let real = entry_name(zip, name)?;
    let f = zip.by_name(&real).ok()?;
    let mut out = Vec::new();
    f.take(MAX_PART).read_to_end(&mut out).ok()?;
    Some(out)
}

fn find_model_part(zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>) -> Option<String> {
    if let Some(rels) = read_part(zip, "_rels/.rels") {
        let mut target = None;
        scan(&String::from_utf8_lossy(&rels), &mut |t| {
            if !t.close && t.name == "Relationship" && target.is_none() && t.attr("Type").is_some_and(|ty| ty.ends_with("/3dmodel")) {
                target = t.attr("Target").map(|s| s.trim_start_matches('/').to_string());
            }
        });
        if let Some(t) = target
            && entry_name(zip, &t).is_some()
        {
            return Some(t);
        }
    }
    if entry_name(zip, "3D/3dmodel.model").is_some() {
        return Some("3D/3dmodel.model".into());
    }
    (0..zip.len()).find_map(|i| {
        let f = zip.by_index_raw(i).ok()?;
        let full = String::from_utf8_lossy(f.name_raw()).into_owned();
        (!f.is_dir() && full.to_ascii_lowercase().ends_with(".model")).then_some(full)
    })
}

// ---- XML ----------------------------------------------------------------

struct Tag {
    /// Local name (prefix removed).
    name: String,
    /// True for a closing tag; a self-closing tag is reported as open then
    /// close.
    close: bool,
    attrs: Vec<(String, String)>,
}

impl Tag {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    fn num(&self, name: &str) -> Option<f32> {
        self.attr(name)?.trim().parse().ok()
    }

    fn id(&self, name: &str) -> Option<u32> {
        self.attr(name)?.trim().parse().ok()
    }
}

fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// Calls `f` for every tag of `s`, tolerating comments, declarations,
/// CDATA, stray text and a truncated end.
fn scan(s: &str, f: &mut dyn FnMut(Tag)) {
    let b = s.as_bytes();
    let mut i = 0;
    while let Some(off) = s[i..].find('<') {
        i += off;
        let rest = &s[i..];
        if rest.starts_with("<!--") {
            match rest.find("-->") {
                Some(e) => i += e + 3,
                None => return,
            }
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            match rest.find("]]>") {
                Some(e) => i += e + 3,
                None => return,
            }
            continue;
        }
        if rest.starts_with("<?") || rest.starts_with("<!") {
            match rest.find('>') {
                Some(e) => i += e + 1,
                None => return,
            }
            continue;
        }
        i += 1;
        let close = b.get(i) == Some(&b'/');
        if close {
            i += 1;
        }
        let start = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'/' && b[i] != b'>' {
            i += 1;
        }
        let name = local(&s[start..i]).to_string();
        let mut attrs = Vec::new();
        let mut selfclose = false;
        loop {
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            match b.get(i) {
                None => return,
                Some(b'>') => {
                    i += 1;
                    break;
                }
                Some(b'/') => {
                    selfclose = true;
                    i += 1;
                }
                Some(_) => {
                    let ks = i;
                    while i < b.len() && !b[i].is_ascii_whitespace() && !matches!(b[i], b'=' | b'>' | b'/') {
                        i += 1;
                    }
                    let key = local(&s[ks..i]).to_string();
                    while i < b.len() && b[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    if b.get(i) != Some(&b'=') {
                        if i == ks {
                            i += 1;
                        }
                        continue;
                    }
                    i += 1;
                    while i < b.len() && b[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    let val = match b.get(i) {
                        Some(&q) if q == b'"' || q == b'\'' => {
                            let vs = i + 1;
                            match s[vs..].find(q as char) {
                                Some(e) => {
                                    i = vs + e + 1;
                                    unescape(&s[vs..vs + e])
                                }
                                None => return,
                            }
                        }
                        _ => {
                            let vs = i;
                            while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' {
                                i += 1;
                            }
                            unescape(&s[vs..i])
                        }
                    };
                    if !key.is_empty() {
                        attrs.push((key, val));
                    }
                }
            }
        }
        if close {
            f(Tag { name, close: true, attrs: Vec::new() });
        } else {
            f(Tag { name: name.clone(), close: false, attrs });
            if selfclose {
                f(Tag { name, close: true, attrs: Vec::new() });
            }
        }
    }
}

// ---- Document -------------------------------------------------------------

/// Corner indices and the triangle's property (group id, index), if any.
type Triangle = ([u32; 3], Option<(u32, usize)>);

#[derive(Default)]
struct Object {
    name: String,
    pid: Option<u32>,
    pindex: usize,
    positions: Vec<V3>,
    /// Corner indices and the triangle's own (pid, p1), when it has one.
    triangles: Vec<Triangle>,
    components: Vec<(u32, M4)>,
}

#[derive(Default)]
struct Document {
    objects: HashMap<u32, Object>,
    order: Vec<u32>,
    /// Property groups by id: base materials and colour groups.
    groups: HashMap<u32, Vec<Material>>,
    build: Vec<(u32, M4)>,
    has_build: bool,
}

const IDENTITY: M4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// A 3MF transform (12 numbers, row-vector convention) as a column-major
/// matrix; identity when absent or malformed.
fn parse_transform(s: Option<&str>) -> M4 {
    let Some(s) = s else { return IDENTITY };
    let nums: Vec<f32> = s.split_whitespace().filter_map(|t| t.parse().ok()).collect();
    if nums.len() != 12 || nums.iter().any(|n| !n.is_finite()) {
        return IDENTITY;
    }
    let mut m = IDENTITY;
    for (c, col) in m.iter_mut().enumerate() {
        for (r, cell) in col.iter_mut().take(3).enumerate() {
            *cell = nums[c * 3 + r];
        }
    }
    m
}

/// `#RRGGBB` or `#RRGGBBAA` as (colour, opacity).
fn parse_color(s: &str) -> Option<(V3, f32)> {
    let h = s.trim().strip_prefix('#')?;
    if !(h.len() == 6 || h.len() == 8) || !h.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok().map(|v| f32::from(v) / 255.0);
    let a = if h.len() == 8 { byte(6)? } else { 1.0 };
    Some(([byte(0)?, byte(2)?, byte(4)?], a))
}

fn color_material(name: Option<&str>, spec: &str) -> Option<Material> {
    let (color, opacity) = parse_color(spec)?;
    let name = match name {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => format!("#{}", spec.trim().trim_start_matches('#').get(..6).unwrap_or("").to_ascii_uppercase()),
    };
    Some(Material { name, color, opacity })
}

fn parse_document(xml: &str) -> Document {
    let mut doc = Document::default();
    let mut cur: Option<(u32, Object)> = None;
    let mut group: Option<(u32, Vec<Material>)> = None;
    let mut in_build = false;
    scan(xml, &mut |t| {
        if t.close {
            match t.name.as_str() {
                "object" => {
                    if let Some((id, o)) = cur.take()
                        && doc.objects.insert(id, o).is_none()
                    {
                        doc.order.push(id);
                    }
                }
                "basematerials" | "colorgroup" => {
                    if let Some((id, g)) = group.take() {
                        doc.groups.insert(id, g);
                    }
                }
                "build" => in_build = false,
                _ => {}
            }
            return;
        }
        match t.name.as_str() {
            "object" => {
                if let Some(id) = t.id("id") {
                    cur = Some((
                        id,
                        Object {
                            name: t.attr("name").unwrap_or("").to_string(),
                            pid: t.id("pid"),
                            pindex: t.id("pindex").unwrap_or(0) as usize,
                            ..Object::default()
                        },
                    ));
                }
            }
            "vertex" => {
                if let Some((_, o)) = cur.as_mut() {
                    let p = [t.num("x").unwrap_or(0.0), t.num("y").unwrap_or(0.0), t.num("z").unwrap_or(0.0)];
                    o.positions.push(if p.iter().all(|c| c.is_finite()) { p } else { [0.0; 3] });
                }
            }
            "triangle" => {
                if let Some((_, o)) = cur.as_mut()
                    && let (Some(a), Some(b), Some(c)) = (t.id("v1"), t.id("v2"), t.id("v3"))
                {
                    let p1 = t.id("p1").map(|p| p as usize);
                    let prop = match t.id("pid") {
                        Some(pid) => Some((pid, p1.unwrap_or(0))),
                        None => o.pid.map(|pid| (pid, p1.unwrap_or(o.pindex))),
                    };
                    o.triangles.push(([a, b, c], prop));
                }
            }
            "component" => {
                if let (Some((_, o)), Some(id)) = (cur.as_mut(), t.id("objectid")) {
                    o.components.push((id, parse_transform(t.attr("transform"))));
                }
            }
            "basematerials" | "colorgroup" => {
                if let Some(id) = t.id("id") {
                    group = Some((id, Vec::new()));
                }
            }
            "base" => {
                if let Some((_, g)) = group.as_mut() {
                    // An unreadable entry keeps its slot so indices stay right.
                    g.push(t.attr("displaycolor").and_then(|c| color_material(t.attr("name"), c)).unwrap_or(Material { name: String::new(), color: super::DEFAULT_COLOR, opacity: 1.0 }));
                }
            }
            "color" => {
                if let Some((_, g)) = group.as_mut() {
                    g.push(t.attr("color").and_then(|c| color_material(None, c)).unwrap_or(Material { name: String::new(), color: super::DEFAULT_COLOR, opacity: 1.0 }));
                }
            }
            "build" => {
                in_build = true;
                doc.has_build = true;
            }
            "item" if in_build => {
                if let Some(id) = t.id("objectid") {
                    doc.build.push((id, parse_transform(t.attr("transform"))));
                }
            }
            _ => {}
        }
    });
    doc
}

impl Document {
    /// The material a property (group id, index) names. Entries that could
    /// not be read count as no material.
    fn material(&self, prop: Option<(u32, usize)>) -> Option<&Material> {
        let (pid, idx) = prop?;
        let m = self.groups.get(&pid)?.get(idx)?;
        (m.color != super::DEFAULT_COLOR || !m.name.is_empty()).then_some(m)
    }

    fn flatten(&self) -> Result<Model> {
        let mut meshes = Vec::new();
        let mut instances = 0usize;
        if self.has_build && !self.build.is_empty() {
            for &(id, m) in &self.build {
                self.instantiate(id, &m, 0, &mut Vec::new(), &mut instances, &mut meshes);
            }
        } else {
            let used: std::collections::HashSet<u32> = self.objects.values().flat_map(|o| o.components.iter().map(|c| c.0)).collect();
            for id in self.order.iter().filter(|id| !used.contains(id)) {
                self.instantiate(*id, &IDENTITY, 0, &mut Vec::new(), &mut instances, &mut meshes);
            }
        }
        Model { meshes }.finish()
    }

    fn instantiate(&self, id: u32, m: &M4, depth: usize, stack: &mut Vec<u32>, instances: &mut usize, out: &mut Vec<Mesh>) {
        if depth > MAX_DEPTH || stack.contains(&id) || *instances >= MAX_INSTANCES {
            return;
        }
        let Some(o) = self.objects.get(&id) else { return };
        *instances += 1;
        self.emit_mesh(o, m, out);
        stack.push(id);
        for (child, cm) in &o.components {
            self.instantiate(*child, &math::mul(m, cm), depth + 1, stack, instances, out);
        }
        stack.pop();
    }

    /// One mesh per distinct colour among the triangles of `o`.
    fn emit_mesh(&self, o: &Object, m: &M4, out: &mut Vec<Mesh>) {
        if o.triangles.is_empty() {
            return;
        }
        struct Bucket {
            material: Option<Material>,
            remap: HashMap<u32, u32>,
            mesh: Mesh,
        }
        let mut buckets: Vec<Bucket> = Vec::new();
        let mut by_key: HashMap<Option<(String, [u32; 3], u32)>, usize> = HashMap::new();
        for (tri, prop) in &o.triangles {
            if tri.iter().any(|&v| v as usize >= o.positions.len()) {
                continue;
            }
            let mat = self.material(*prop);
            let key = mat.map(|x| (x.name.clone(), x.color.map(f32::to_bits), x.opacity.to_bits()));
            let bi = *by_key.entry(key).or_insert_with(|| {
                buckets.push(Bucket {
                    material: mat.cloned(),
                    remap: HashMap::new(),
                    mesh: Mesh { name: o.name.clone(), ..Mesh::default() },
                });
                buckets.len() - 1
            });
            let b = &mut buckets[bi];
            for &v in tri {
                let next = b.mesh.positions.len() as u32;
                let ni = *b.remap.entry(v).or_insert_with(|| {
                    b.mesh.positions.push(math::transform_point(m, o.positions[v as usize]));
                    next
                });
                b.mesh.indices.push(ni);
            }
        }
        out.extend(buckets.into_iter().map(|mut b| {
            b.mesh.material = b.material;
            b.mesh
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn make_zip(files: &[(&str, &str)]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, body) in files {
            w.start_file(*name, SimpleFileOptions::default()).unwrap();
            w.write_all(body.as_bytes()).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    fn model_zip(xml: &str) -> Vec<u8> {
        make_zip(&[("3D/3dmodel.model", xml)])
    }

    const TRI: &str = r#"<vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices>"#;

    fn doc(resources: &str, build: &str) -> String {
        format!(r#"<?xml version="1.0"?><model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02"><resources>{resources}</resources><build>{build}</build></model>"#)
    }

    #[test]
    fn single_triangle() {
        let xml = doc(&format!(r#"<object id="1" name="tri"><mesh>{TRI}<triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object>"#), r#"<item objectid="1"/>"#);
        let m = parse(&model_zip(&xml)).unwrap();
        assert_eq!(m.meshes.len(), 1);
        assert_eq!(m.meshes[0].name, "tri");
        assert_eq!(m.meshes[0].indices, vec![0, 1, 2]);
        assert_eq!(m.meshes[0].positions[1], [1.0, 0.0, 0.0]);
        assert!(m.meshes[0].material.is_none());
    }

    #[test]
    fn item_transform_translates() {
        let xml = doc(&format!(r#"<object id="1"><mesh>{TRI}<triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object>"#), r#"<item objectid="1" transform="1 0 0 0 1 0 0 0 1 10 20 30"/>"#);
        let m = parse(&model_zip(&xml)).unwrap();
        assert_eq!(m.meshes[0].positions[0], [10.0, 20.0, 30.0]);
        assert_eq!(m.meshes[0].positions[1], [11.0, 20.0, 30.0]);
    }

    #[test]
    fn object_base_material() {
        let xml = doc(
            &format!(r##"<basematerials id="5"><base name="Red" displaycolor="#FF0000"/><base name="Blue" displaycolor="#0000FF80"/></basematerials><object id="1" pid="5" pindex="1"><mesh>{TRI}<triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object>"##),
            r#"<item objectid="1"/>"#,
        );
        let m = parse(&model_zip(&xml)).unwrap();
        let mat = m.meshes[0].material.clone().unwrap();
        assert_eq!(mat.name, "Blue");
        assert_eq!(mat.color, [0.0, 0.0, 1.0]);
        assert!((mat.opacity - 128.0 / 255.0).abs() < 1e-6);
    }

    #[test]
    fn per_triangle_colors_split() {
        let xml = doc(
            &format!(
                r##"<colorgroup id="2"><color color="#00FF00"/><color color="#112233"/></colorgroup><object id="1"><mesh>{TRI}<triangles><triangle v1="0" v2="1" v3="2" pid="2" p1="0"/><triangle v1="2" v2="1" v3="0" pid="2" p1="1"/><triangle v1="0" v2="2" v3="1"/></triangles></mesh></object>"##
            ),
            r#"<item objectid="1"/>"#,
        );
        let m = parse(&model_zip(&xml)).unwrap();
        assert_eq!(m.meshes.len(), 3);
        assert_eq!(m.meshes[0].material.as_ref().unwrap().name, "#00FF00");
        assert_eq!(m.meshes[1].material.as_ref().unwrap().color, [17.0 / 255.0, 34.0 / 255.0, 51.0 / 255.0]);
        assert!(m.meshes[2].material.is_none());
    }

    #[test]
    fn components_twice() {
        let xml = doc(
            &format!(
                r#"<object id="1"><mesh>{TRI}<triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object><object id="2"><components><component objectid="1"/><component objectid="1" transform="1 0 0 0 1 0 0 0 1 5 0 0"/></components></object>"#
            ),
            r#"<item objectid="2" transform="1 0 0 0 1 0 0 0 1 0 0 100"/>"#,
        );
        let m = parse(&model_zip(&xml)).unwrap();
        assert_eq!(m.meshes.len(), 2);
        assert_eq!(m.meshes[0].positions[0], [0.0, 0.0, 100.0]);
        assert_eq!(m.meshes[1].positions[0], [5.0, 0.0, 100.0]);
    }

    #[test]
    fn no_build_uses_unreferenced_objects_and_survives_cycles() {
        let xml = doc(
            &format!(r#"<object id="1"><mesh>{TRI}<triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh><components><component objectid="2"/></components></object><object id="2"><components><component objectid="1"/></components></object>"#),
            "",
        );
        // Both are used as components of each other, so nothing is a root.
        assert!(parse(&model_zip(&xml)).is_err());
        let xml = doc(&format!(r#"<object id="1"><mesh>{TRI}<triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object>"#), "");
        assert_eq!(parse(&model_zip(&xml)).unwrap().meshes.len(), 1);
    }

    #[test]
    fn model_found_through_rels() {
        let xml = doc(&format!(r#"<object id="1"><mesh>{TRI}<triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object>"#), r#"<item objectid="1"/>"#);
        let rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Target="/Other/part.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/></Relationships>"#;
        let z = make_zip(&[("_rels/.rels", rels), ("Other/part.model", &xml), ("3D/3dmodel.model", "<model/>")]);
        assert_eq!(parse(&z).unwrap().triangle_count(), 1);
    }

    #[test]
    fn namespaced_document() {
        let xml = format!(
            r#"<?xml version='1.0'?><!-- c --><m:model xmlns:m='urn:x'><m:resources><m:object id='1' name='a &amp; b'><m:mesh>{}<m:triangles><m:triangle v1='0' v2='1' v3='2'/></m:triangles></m:mesh></m:object></m:resources><m:build><m:item objectid='1'/></m:build></m:model>"#,
            TRI.replace("<vertex", "<m:vertex").replace("</vertices", "</m:vertices").replace("<vertices", "<m:vertices")
        );
        let m = parse(&model_zip(&xml)).unwrap();
        assert_eq!(m.meshes[0].name, "a & b");
        assert_eq!(m.triangle_count(), 1);
    }

    #[test]
    fn not_a_zip() {
        assert!(parse(b"hello, this is not a zip").is_err());
    }

    #[test]
    fn missing_model() {
        let z = make_zip(&[("readme.txt", "hi")]);
        let e = parse(&z).unwrap_err();
        assert!(e.0.contains("no 3D model"), "{e}");
    }
}
