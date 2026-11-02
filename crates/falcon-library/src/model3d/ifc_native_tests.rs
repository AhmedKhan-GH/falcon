//! web-ifc geometry checked against analytic volumes and extents. Every
//! test returns at once when falcon was built without web-ifc.

use super::super::ifc::{self, Report, full_geometry};
use super::super::{Model, geom};
use super::{MAX_GRAPH_DEPTH, sound_graph};

/// A whole file around `body` (instances from #10 on): a project whose
/// length unit is the metre with `prefix` (`$` or `.MILLI.`), and a 3D
/// context #2 with origin placement #4.
fn file(schema: &str, prefix: &str, body: &str) -> String {
    format!(
        "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('ViewDefinition [ReferenceView]'),'2;1');\n\
         FILE_NAME('t.ifc','2026-11-01T00:00:00',(''),(''),'falcon','falcon','');\nFILE_SCHEMA(('{schema}'));\nENDSEC;\nDATA;\n\
         #1=IFCPROJECT('0001',$,'P',$,$,$,$,(#2),#3);\n#2=IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,#4,$);\n\
         #3=IFCUNITASSIGNMENT((#5));\n#4=IFCAXIS2PLACEMENT3D(#6,$,$);\n#5=IFCSIUNIT(*,.LENGTHUNIT.,{prefix},.METRE.);\n\
         #6=IFCCARTESIANPOINT((0.,0.,0.));\n{body}ENDSEC;\nEND-ISO-10303-21;\n"
    )
}

/// Shared helpers: a 2D origin (#20), its placement (#21) and +Z (#22).
const PRELUDE: &str = "#20=IFCCARTESIANPOINT((0.,0.));\n#21=IFCAXIS2PLACEMENT2D(#20,$);\n#22=IFCDIRECTION((0.,0.,1.));\n";

/// A product `entity` (ids `n`..`n+3`) whose shape representation of type
/// `rep_type` holds `items`, placed at the origin.
fn prod(n: u32, entity: &str, rep_type: &str, items: &str) -> String {
    format!(
        "#{n}=IFCSHAPEREPRESENTATION(#2,'Body','{rep_type}',({items}));\n#{p}=IFCPRODUCTDEFINITIONSHAPE($,$,(#{n}));\n\
         #{l}=IFCLOCALPLACEMENT($,#4);\n#{e}={entity}('0002',$,'Thing',$,$,#{l},#{p},$,$);\n",
        p = n + 1,
        l = n + 2,
        e = n + 3
    )
}

/// A file with one product holding the item `#31` (and whatever `body` defines).
fn single(prefix: &str, entity: &str, rep_type: &str, body: &str) -> String {
    file("IFC4", prefix, &format!("{PRELUDE}{body}{}", prod(100, entity, rep_type, "#31")))
}

/// `#30` = `profile`, `#31` = it extruded `depth` along +Z.
fn extrusion(profile: &str, depth: &str) -> String {
    format!("#30={profile};\n#31=IFCEXTRUDEDAREASOLID(#30,#4,#22,{depth});\n")
}

fn read(src: &str) -> (Model, Report) {
    let (m, r) = ifc::read(src.as_bytes()).expect("readable");
    (m.expect("a model"), r)
}

fn extents(m: &Model) -> [f64; 3] {
    let (lo, hi) = m.bounds().expect("bounds");
    [0, 1, 2].map(|k| f64::from(hi[k] - lo[k]))
}

fn volume(m: &Model) -> f64 {
    geom::stats(m).volume.expect("closed mesh")
}

fn assert_vol(m: &Model, want: f64, rel: f64) {
    let v = volume(m);
    assert!((v - want).abs() <= rel * want, "volume {v}, want {want} (tolerance {rel})");
}

fn assert_ext(m: &Model, want: [f64; 3], tol: f64) {
    let e = extents(m);
    assert!((0..3).all(|k| (e[k] - want[k]).abs() <= tol), "extents {e:?}, want {want:?}");
}

fn finite(m: &Model) -> bool {
    m.meshes.iter().all(|mesh| mesh.positions.iter().all(|p| p.iter().all(|c| c.is_finite())))
}

/// The area of the regular polygon web-ifc meshes a circle of radius `r`
/// with. Asked for 12 segments it draws 11 (a cylinder has 40 triangles,
/// not 44), so a circle comes out 0.9% under the 12-gon's area.
fn ngon(r: f64) -> f64 {
    let n = 11.0;
    n / 2.0 * r * r * (2.0 * std::f64::consts::PI / n).sin()
}

fn rect(x: f64, y: f64) -> String {
    format!("IFCRECTANGLEPROFILEDEF(.AREA.,$,#21,{x:?},{y:?})")
}

#[test]
fn rectangle_extrusion_in_metres_and_millimetres() {
    if !full_geometry() {
        return;
    }
    let (m, r) = read(&single("$", "IFCBUILDINGELEMENTPROXY", "SweptSolid", &extrusion(&rect(1.0, 2.0), "3.")));
    assert_eq!((r.shown, r.skipped), (1, 0));
    assert_eq!(m.triangle_count(), 12);
    assert_vol(&m, 6.0, 1e-3);
    assert_ext(&m, [1.0, 3.0, 2.0], 1e-4);
    let (m, _) = read(&single(".MILLI.", "IFCBUILDINGELEMENTPROXY", "SweptSolid", &extrusion(&rect(1000.0, 2000.0), "3000.")));
    assert_vol(&m, 6.0, 1e-3);
    assert_ext(&m, [1.0, 3.0, 2.0], 1e-4);
}

/// Profile text, depth 2 extrusion, expected area (and tolerance).
fn profile_cases() -> Vec<(&'static str, String, f64, f64)> {
    let poly = |pts: &[[f64; 2]]| -> String {
        let mut s = String::new();
        let mut ids = Vec::new();
        for (k, p) in pts.iter().enumerate() {
            s += &format!("#{}=IFCCARTESIANPOINT(({:?},{:?}));\n", 40 + k, p[0], p[1]);
            ids.push(format!("#{}", 40 + k));
        }
        s += &format!("#60=IFCPOLYLINE(({}));\n", ids.join(","));
        s
    };
    let l_pts = [[0., 0.], [2., 0.], [2., 1.], [1., 1.], [1., 2.], [0., 2.], [0., 0.]];
    let sq = |a: f64| [[-a, -a], [a, -a], [a, a], [-a, a], [-a, -a]];
    let mut with_voids = poly(&sq(2.0));
    // The hole: points #70.., polyline #80.
    let hole = sq(1.0);
    let ids: Vec<String> = hole.iter().enumerate().map(|(k, _)| format!("#{}", 70 + k)).collect();
    for (k, p) in hole.iter().enumerate() {
        with_voids += &format!("#{}=IFCCARTESIANPOINT(({:?},{:?}));\n", 70 + k, p[0], p[1]);
    }
    with_voids += &format!("#80=IFCPOLYLINE(({}));\n", ids.join(","));
    vec![
        ("circle", String::new(), ngon(0.5), 1e-3),
        ("i shape", String::new(), 2.0 * 0.2 * 0.03 + (0.4 - 0.06) * 0.02, 1e-3),
        ("l shape", String::new(), 0.3 * 0.02 + (0.2 - 0.02) * 0.02, 1e-3),
        ("circle hollow", String::new(), ngon(0.5) - ngon(0.4), 1e-3),
        ("rectangle hollow", String::new(), 1.0 * 2.0 - 0.8 * 1.8, 1e-3),
        ("arbitrary closed", poly(&l_pts), 3.0, 1e-3),
        ("with voids", with_voids, 16.0 - 4.0, 1e-3),
    ]
}

#[test]
fn profile_kinds_extruded() {
    if !full_geometry() {
        return;
    }
    let profiles = [
        "IFCCIRCLEPROFILEDEF(.AREA.,$,#21,0.5)",
        "IFCISHAPEPROFILEDEF(.AREA.,$,#21,0.2,0.4,0.02,0.03,$,$,$)",
        "IFCLSHAPEPROFILEDEF(.AREA.,$,#21,0.3,0.2,0.02,$,$,$)",
        "IFCCIRCLEHOLLOWPROFILEDEF(.AREA.,$,#21,0.5,0.1)",
        "IFCRECTANGLEHOLLOWPROFILEDEF(.AREA.,$,#21,1.,2.,0.1,$,$)",
        "IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#60)",
        "IFCARBITRARYPROFILEDEFWITHVOIDS(.AREA.,$,#60,(#80))",
    ];
    for ((name, extra, area, tol), profile) in profile_cases().into_iter().zip(profiles) {
        let src = single("$", "IFCBUILDINGELEMENTPROXY", "SweptSolid", &format!("{extra}{}", extrusion(profile, "2.")));
        let (m, r) = read(&src);
        assert_eq!(r.shown, 1, "{name} renders");
        assert_vol(&m, area * 2.0, tol);
        assert!(m.bounds().is_some(), "{name}");
    }
}

#[test]
fn wall_with_an_opening() {
    if !full_geometry() {
        return;
    }
    let wall = "#30=IFCRECTANGLEPROFILEDEF(.AREA.,$,#21,4.,0.2);\n#31=IFCEXTRUDEDAREASOLID(#30,#4,#22,3.);\n";
    // The opening: 1 wide, 0.4 thick (through the wall), 2 high, from z=0.5.
    let opening = "#32=IFCCARTESIANPOINT((0.,0.,0.5));\n#33=IFCAXIS2PLACEMENT3D(#32,$,$);\n\
        #34=IFCRECTANGLEPROFILEDEF(.AREA.,$,#21,1.,0.4);\n#35=IFCEXTRUDEDAREASOLID(#34,#33,#22,2.);\n";
    let rels = "#110=IFCSHAPEREPRESENTATION(#2,'Body','SweptSolid',(#35));\n#111=IFCPRODUCTDEFINITIONSHAPE($,$,(#110));\n\
        #112=IFCOPENINGELEMENT('0003',$,'Hole',$,$,#102,#111,$,$);\n#113=IFCRELVOIDSELEMENT('0004',$,$,$,#103,#112);\n";
    let src = file("IFC4", "$", &format!("{PRELUDE}{wall}{}{opening}{rels}", prod(100, "IFCWALL", "SweptSolid", "#31")));
    let (m, r) = read(&src);
    assert_eq!((r.shown, r.skipped), (1, 0), "{r:?}");
    assert_vol(&m, 4.0 * 0.2 * 3.0 - 1.0 * 0.2 * 2.0, 1e-3);
    assert_ext(&m, [4.0, 3.0, 0.2], 1e-4);
}

#[test]
fn revolved_ring() {
    if !full_geometry() {
        return;
    }
    // A 0.2 x 0.4 rectangle centred at x=1, turned about the Y axis. The
    // angle is in radians (no plane angle unit is declared); 360. is not read.
    let body = "#30=IFCCARTESIANPOINT((1.,0.));\n#32=IFCAXIS2PLACEMENT2D(#30,$);\n\
        #33=IFCRECTANGLEPROFILEDEF(.AREA.,$,#32,0.2,0.4);\n#34=IFCDIRECTION((0.,1.,0.));\n\
        #35=IFCAXIS1PLACEMENT(#6,#34);\n#31=IFCREVOLVEDAREASOLID(#33,#4,#35,6.283185307179586);\n";
    let src = single("$", "IFCBUILDINGELEMENTPROXY", "SweptSolid", body);
    let (m, _) = read(&src);
    let exact = 2.0 * std::f64::consts::PI * 1.0 * 0.08;
    let v = volume(&m);
    // 12 segments: the polygonal ring is about 4.5% below the exact volume.
    assert!(v < exact && (v - exact).abs() < 0.08 * exact, "volume {v}, exact {exact}");
    // The axis is IFC's Y, which is the viewer's Z.
    assert_ext(&m, [2.2, 2.2, 0.4], 0.1);
}

#[test]
fn swept_disk_along_a_segment() {
    if !full_geometry() {
        return;
    }
    let body = "#30=IFCCARTESIANPOINT((0.,0.,0.));\n#32=IFCCARTESIANPOINT((3.,0.,0.));\n#33=IFCPOLYLINE((#30,#32));\n\
        #31=IFCSWEPTDISKSOLID(#33,0.1,$,$,$);\n";
    let (m, _) = read(&single("$", "IFCBUILDINGELEMENTPROXY", "AdvancedSweptSolid", body));
    assert_vol(&m, 3.0 * 0.01 * 3.0, 0.05);
    let e = extents(&m);
    assert!((e[0] - 3.0).abs() < 1e-4 && (0.17..=0.2001).contains(&e[1]) && (0.17..=0.2001).contains(&e[2]), "{e:?}");
}

/// A 2x2x2 box over x,y in -1..1, z in 0..2 as #31.
const BOX2: &str = "#30=IFCRECTANGLEPROFILEDEF(.AREA.,$,#21,2.,2.);\n#31=IFCEXTRUDEDAREASOLID(#30,#4,#22,2.);\n";

#[test]
fn clipping_result_with_a_half_space() {
    if !full_geometry() {
        return;
    }
    // The plane x=0 with its normal along +X.
    let body = format!(
        "{BOX2}#32=IFCDIRECTION((1.,0.,0.));\n#33=IFCDIRECTION((0.,1.,0.));\n#34=IFCAXIS2PLACEMENT3D(#6,#32,#33);\n\
         #35=IFCPLANE(#34);\n#36=IFCHALFSPACESOLID(#35,.F.);\n#37=IFCBOOLEANCLIPPINGRESULT(.DIFFERENCE.,#31,#36);\n"
    );
    let src = file("IFC4", "$", &format!("{PRELUDE}{body}{}", prod(100, "IFCBUILDINGELEMENTPROXY", "Clipping", "#37")));
    let (m, _) = read(&src);
    assert_vol(&m, 4.0, 1e-3);
    assert_ext(&m, [1.0, 2.0, 2.0], 1e-4);
}

#[test]
fn boolean_difference_of_two_extrusions() {
    if !full_geometry() {
        return;
    }
    // A 1x1 bar from z=-1 to 3 through the box (z 0..2): removes 1x1x2.
    let body = format!(
        "{BOX2}#32=IFCCARTESIANPOINT((0.,0.,-1.));\n#33=IFCAXIS2PLACEMENT3D(#32,$,$);\n#34=IFCRECTANGLEPROFILEDEF(.AREA.,$,#21,1.,1.);\n\
         #35=IFCEXTRUDEDAREASOLID(#34,#33,#22,4.);\n#36=IFCBOOLEANRESULT(.DIFFERENCE.,#31,#35);\n"
    );
    let src = file("IFC4", "$", &format!("{PRELUDE}{body}{}", prod(100, "IFCBUILDINGELEMENTPROXY", "CSG", "#36")));
    let (m, _) = read(&src);
    assert_vol(&m, 8.0 - 2.0, 1e-3);
    assert_ext(&m, [2.0, 2.0, 2.0], 1e-4);
}

#[test]
fn csg_block_and_union() {
    if !full_geometry() {
        return;
    }
    let body = "#30=IFCBLOCK(#4,1.,2.,3.);\n#31=IFCCSGSOLID(#30);\n";
    let (m, r) = read(&single("$", "IFCBUILDINGELEMENTPROXY", "CSG", body));
    assert_eq!(r.shown, 1);
    assert_vol(&m, 6.0, 1e-3);
    assert_ext(&m, [1.0, 3.0, 2.0], 1e-4);
    // Two disjoint blocks united.
    let body = "#30=IFCBLOCK(#4,1.,2.,3.);\n#32=IFCCARTESIANPOINT((5.,0.,0.));\n#33=IFCAXIS2PLACEMENT3D(#32,$,$);\n\
        #34=IFCBLOCK(#33,1.,2.,3.);\n#31=IFCBOOLEANRESULT(.UNION.,#30,#34);\n";
    let (m, _) = read(&single("$", "IFCBUILDINGELEMENTPROXY", "CSG", body));
    assert_vol(&m, 12.0, 1e-3);
    assert_ext(&m, [6.0, 3.0, 2.0], 1e-4);
}

#[test]
fn mapped_item_instanced_twice() {
    if !full_geometry() {
        return;
    }
    let map = "#30=IFCRECTANGLEPROFILEDEF(.AREA.,$,#21,1.,1.);\n#31=IFCEXTRUDEDAREASOLID(#30,#4,#22,1.);\n\
        #32=IFCSHAPEREPRESENTATION(#2,'Body','SweptSolid',(#31));\n#33=IFCREPRESENTATIONMAP(#4,#32);\n\
        #34=IFCCARTESIANPOINT((0.,0.,0.));\n#35=IFCCARTESIANTRANSFORMATIONOPERATOR3D($,$,#34,1.,$);\n\
        #36=IFCCARTESIANPOINT((3.,0.,0.));\n#37=IFCCARTESIANTRANSFORMATIONOPERATOR3D($,$,#36,1.,$);\n\
        #38=IFCMAPPEDITEM(#33,#35);\n#39=IFCMAPPEDITEM(#33,#37);\n";
    let one = file("IFC4", "$", &format!("{PRELUDE}{map}{}", prod(100, "IFCBUILDINGELEMENTPROXY", "MappedRepresentation", "#38")));
    let two = file("IFC4", "$", &format!("{PRELUDE}{map}{}", prod(100, "IFCBUILDINGELEMENTPROXY", "MappedRepresentation", "#38,#39")));
    let (m1, _) = read(&one);
    let (m2, r) = read(&two);
    assert_eq!(m1.triangle_count(), 12);
    assert_eq!(m2.triangle_count(), 24);
    assert_eq!(r.shown, 1);
    assert_vol(&m2, 2.0, 1e-3);
    assert_ext(&m2, [4.0, 1.0, 1.0], 1e-4);
}

#[test]
fn report_counts_shown_skipped_and_bare_products() {
    if !full_geometry() {
        return;
    }
    let wall = format!("{BOX2}{}", prod(100, "IFCWALL", "SweptSolid", "#31"));
    // A proxy whose only Body item is a polyline: nothing to draw.
    let line = "#40=IFCCARTESIANPOINT((0.,0.,0.));\n#41=IFCCARTESIANPOINT((1.,0.,0.));\n#42=IFCPOLYLINE((#40,#41));\n";
    let proxy = prod(110, "IFCBUILDINGELEMENTPROXY", "Curve3D", "#42");
    // A product with no representation at all.
    let bare = "#120=IFCLOCALPLACEMENT($,#4);\n#121=IFCBUILDINGELEMENTPROXY('0009',$,'Bare',$,$,#120,$,$,$);\n";
    let (m, r) = read(&file("IFC4", "$", &format!("{PRELUDE}{wall}{line}{proxy}{bare}")));
    assert_eq!((r.shown, r.skipped), (1, 1), "{r:?}");
    assert_eq!(r.unsupported, vec![("IfcPolyline".to_string(), 1)]);
    assert_eq!(r.triangles, 12);
    assert_eq!(m.triangle_count(), 12);
}

fn good_wall() -> String {
    single("$", "IFCWALL", "SweptSolid", &extrusion(&rect(4.0, 0.2), "3."))
}

/// Reads hostile input through the web-ifc path: it must neither panic nor
/// produce non-finite coordinates.
fn survives(bytes: &[u8]) -> Option<Model> {
    match ifc::read(bytes) {
        Ok((m, _)) => {
            if let Some(m) = &m {
                assert!(finite(m), "non-finite coordinates");
            }
            m
        }
        Err(_) => None,
    }
}

#[test]
fn hostile_input_truncated_garbage_and_empty() {
    if !full_geometry() {
        return;
    }
    let good = good_wall();
    let bytes = good.as_bytes();
    assert!(survives(bytes).is_some());
    for cut in (0..bytes.len()).step_by(7) {
        survives(&bytes[..cut]);
    }
    survives(b"");
    survives(b"\0\xff\xfe garbage \x01\x02 ISO-10303-21; DATA; #1=(;");
    survives(&[0xAB; 4096]);
    let mut bad = bytes.to_vec();
    for k in (0..bad.len()).step_by(11) {
        bad[k] = b'#';
    }
    survives(&bad);
}

#[test]
fn hostile_references_and_numbers() {
    if !full_geometry() {
        return;
    }
    let body = |b: &str, items: &str| file("IFC4", "$", &format!("{PRELUDE}{b}{}", prod(100, "IFCBUILDINGELEMENTPROXY", "SweptSolid", items)));
    // A placement that is its own parent.
    let own = body(&extrusion(&rect(1.0, 1.0), "1."), "#31").replace("#102=IFCLOCALPLACEMENT($,#4)", "#102=IFCLOCALPLACEMENT(#102,#4)");
    assert!(own.contains("#102=IFCLOCALPLACEMENT(#102"));
    survives(own.as_bytes());
    // Cycles: a mapped item whose representation map holds that same mapped
    // item, and booleans whose operand is themselves. web-ifc overflows its
    // stack or spins on these, which ends only its worker, and falcon's own
    // reader takes over.
    let cycle = "#32=IFCSHAPEREPRESENTATION(#2,'Body','MappedRepresentation',(#38));\n#33=IFCREPRESENTATIONMAP(#4,#32);\n\
        #35=IFCCARTESIANTRANSFORMATIONOPERATOR3D($,$,#6,1.,$);\n#38=IFCMAPPEDITEM(#33,#35);\n";
    let selfbool = format!("{BOX2}#36=IFCBOOLEANRESULT(.DIFFERENCE.,#36,#31);\n");
    let selfbool2 = format!("{BOX2}#36=IFCBOOLEANRESULT(.DIFFERENCE.,#31,#36);\n");
    for (b, item) in [(cycle.to_string(), "#38"), (selfbool, "#36"), (selfbool2, "#36")] {
        let src = body(&b, item);
        assert!(!sound_graph(&crate::model3d::step::parse(src.as_bytes()).unwrap()));
        let (m, report) = ifc::read(src.as_bytes()).unwrap();
        assert!(m.is_none() && report.fallback, "falcon's own reader has no extrusions");
    }
    assert!(!sound_graph(&crate::model3d::step::parse(own.as_bytes()).unwrap()));
    // Depths and sizes.
    for depth in ["0.", "-3.", "1.E30", "-1.E30", "1.E308", "1.E-300"] {
        survives(body(&extrusion(&rect(1.0, 2.0), depth), "#31").as_bytes());
    }
    for (x, y) in [(1.0e30, 1.0e30), (1.0e308, 1.0e308), (0.0, 0.0), (-1.0, 2.0), (1.0e-300, 1.0e-300)] {
        survives(body(&extrusion(&rect(x, y), "1."), "#31").as_bytes());
    }
    survives(body(&extrusion("IFCCIRCLEPROFILEDEF(.AREA.,$,#21,1.E308)", "1."), "#31").as_bytes());
    survives(body(&extrusion("IFCCIRCLEPROFILEDEF(.AREA.,$,#21,-2.)", "1."), "#31").as_bytes());
    // A direction of zero length.
    let zero = "#30=IFCRECTANGLEPROFILEDEF(.AREA.,$,#21,1.,1.);\n#32=IFCDIRECTION((0.,0.,0.));\n#31=IFCEXTRUDEDAREASOLID(#30,#4,#32,1.);\n";
    survives(body(zero, "#31").as_bytes());
}

#[test]
fn dangling_references_and_deep_chains_are_not_sound() {
    let good = good_wall();
    assert!(sound_graph(&crate::model3d::step::parse(good.as_bytes()).unwrap()));
    // A conversion-based unit whose measure is missing made web-ifc spin.
    let dangling = good.replace("#6=IFCCARTESIANPOINT((0.,0.,0.));", "#6=IFCCARTESIANPOINT((0.,0.,0.));\n#7=IFCCONVERSIONBASEDUNIT(#8,.LENGTHUNIT.,'inch',#999);");
    assert!(!sound_graph(&crate::model3d::step::parse(dangling.as_bytes()).unwrap()));
    if full_geometry() {
        // A stray reference nothing uses doesn't stop web-ifc.
        let (m, report) = ifc::read(dangling.as_bytes()).unwrap();
        assert!(m.is_some() && !report.fallback);
    }
    // A placement chain deeper than web-ifc is given.
    let mut chain = String::new();
    for k in 0..(MAX_GRAPH_DEPTH as u64 + 10) {
        chain += &format!("#{}=IFCLOCALPLACEMENT(#{},#4);\n", 100_000 + k, 100_001 + k);
    }
    let deep = file("IFC4", "$", &chain);
    assert!(!sound_graph(&crate::model3d::step::parse(deep.as_bytes()).unwrap()));
}

#[test]
fn twenty_thousand_products_share_one_representation() {
    if !full_geometry() {
        return;
    }
    let n = 20_000u32;
    let mut b = String::from(PRELUDE);
    b += &extrusion(&rect(1.0, 1.0), "1.");
    b += "#32=IFCSHAPEREPRESENTATION(#2,'Body','SweptSolid',(#31));\n#33=IFCPRODUCTDEFINITIONSHAPE($,$,(#32));\n";
    for k in 0..n {
        let id = 1000 + 3 * k;
        b += &format!(
            "#{id}=IFCCARTESIANPOINT(({}.,0.,0.));\n#{}=IFCAXIS2PLACEMENT3D(#{id},$,$);\n#{}=IFCLOCALPLACEMENT($,#{});\n",
            2 * k,
            id + 1_000_000,
            id + 2_000_000,
            id + 1_000_000
        );
        b += &format!("#{}=IFCBUILDINGELEMENTPROXY('0002',$,'P',$,$,#{},#33,$,$);\n", id + 3_000_000, id + 2_000_000);
    }
    let src = file("IFC4", "$", &b);
    let t = std::time::Instant::now();
    let (m, r) = read(&src);
    let took = t.elapsed();
    assert_eq!(r.shown, n as usize);
    assert_eq!(m.triangle_count(), 12 * n as usize);
    assert!(took.as_secs() < 30, "{took:?}");
}

#[test]
fn thumbnail_draws_an_extrusion() {
    if !full_geometry() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wall.ifc");
    std::fs::write(&path, good_wall()).unwrap();
    assert!(crate::model3d::raster::thumbnail(&path, "ifc").is_some());
}
