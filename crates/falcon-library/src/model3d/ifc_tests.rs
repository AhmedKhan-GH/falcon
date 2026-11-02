//! Hand-written IFC files covering each geometry kind falcon's own
//! reader takes, placements, units, styles and the fallbacks.

use super::*;
use crate::model3d::geom;

// These cover falcon's own reader; `ifc_native_tests.rs` covers web-ifc.
fn read(bytes: &[u8]) -> Result<(Option<Model>, Report)> {
    read_tessellated(bytes)
}

fn parse(bytes: &[u8]) -> Result<Model> {
    read(bytes)?.0.ok_or_else(|| ParseError::new(NO_GEOMETRY))
}

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

/// A unit cube as a triangulated face set (#10, #11), wound outward,
/// scaled by `s`.
fn cube_tfs(s: f64) -> String {
    let c = [[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.], [0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]];
    let pts: Vec<String> = c.iter().map(|p: &[f64; 3]| format!("({:?},{:?},{:?})", p[0] * s, p[1] * s, p[2] * s)).collect();
    format!(
        "#10=IFCCARTESIANPOINTLIST3D(({}),$);\n#11=IFCTRIANGULATEDFACESET(#10,$,.T.,\n((1,3,2),(1,4,3),(5,6,7),(5,7,8),(1,2,6),(1,6,5),\n(2,3,7),(2,7,6),(3,4,8),(3,8,7),(4,1,5),(4,5,8)),$);\n",
        pts.join(",")
    )
}

/// A product #15 of type `entity` showing the items `items` as its Body,
/// placed at the origin.
fn product(entity: &str, items: &str) -> String {
    format!(
        "#12=IFCSHAPEREPRESENTATION(#2,'Body','Tessellation',({items}));\n#13=IFCPRODUCTDEFINITIONSHAPE($,$,(#12));\n\
         #14=IFCLOCALPLACEMENT($,#4);\n#15={entity}('0002',$,'Thing',$,$,#14,#13,$,$);\n"
    )
}

fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() < 1e-4)
}

#[test]
fn triangulated_face_set_cube_with_a_style() {
    let style = "#16=IFCCOLOURRGB($,1.,0.,0.);\n#17=IFCSURFACESTYLESHADING(#16,0.25);\n#18=IFCSURFACESTYLE('Red',.BOTH.,(#17));\n#19=IFCSTYLEDITEM(#11,(#18),$);\n";
    let src = file("IFC4", "$", &format!("{}{}{style}", cube_tfs(1.0), product("IFCBUILDINGELEMENTPROXY", "#11")));
    let (m, r) = read(src.as_bytes()).unwrap();
    let m = m.unwrap();
    assert_eq!(r, Report { schema: "IFC4".into(), shown: 1, skipped: 0, triangles: 12, unsupported: vec![] });
    assert_eq!(m.meshes.len(), 1);
    assert_eq!(m.meshes[0].name, "IfcBuildingElementProxy");
    let mat = m.meshes[0].material.as_ref().unwrap();
    assert_eq!((mat.name.as_str(), mat.color, mat.opacity), ("Red", [1.0, 0.0, 0.0], 0.75));
    // Z-up turned to Y-up: IFC's +Y runs along the viewer's −Z.
    let (lo, hi) = m.bounds().unwrap();
    assert!(close(lo, [0.0, 0.0, -1.0]) && close(hi, [1.0, 1.0, 0.0]), "{lo:?} {hi:?}");
    let s = geom::stats(&m);
    assert!((s.volume.unwrap() - 1.0).abs() < 1e-6, "closed and wound consistently");
    // Through the registry too (web-ifc, when built in, reads it the same).
    assert_eq!(crate::model3d::parse(src.as_bytes(), "IFC", &|_| None).unwrap().triangle_count(), 12);
}

#[test]
fn triangulated_face_set_normals_and_point_index() {
    // Normals one per point are kept (turned to Y-up).
    let body = "#10=IFCCARTESIANPOINTLIST3D(((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)),$);\n\
        #11=IFCTRIANGULATEDFACESET(#10,((0.,0.,1.),(0.,0.,1.),(0.,0.,1.)),.F.,((1,2,3)),$);\n";
    let src = file("IFC4", "$", &format!("{body}{}", product("IFCPLATE", "#11")));
    let m = parse(src.as_bytes()).unwrap();
    assert_eq!(m.meshes[0].normals.as_deref(), Some(&[[0.0, 1.0, 0.0]; 3][..]));
    // A point index maps the coordinate index onto the points.
    let body = "#10=IFCCARTESIANPOINTLIST3D(((9.,9.,9.),(0.,0.,0.),(1.,0.,0.),(0.,1.,0.)),$);\n\
        #11=IFCTRIANGULATEDFACESET(#10,$,.F.,((1,2,3),(1,2,99)),(2,3,4));\n";
    let src = file("IFC4", "$", &format!("{body}{}", product("IFCPLATE", "#11")));
    let m = parse(src.as_bytes()).unwrap();
    assert_eq!(m.triangle_count(), 1, "the out-of-range triangle is dropped");
    let (_, hi) = m.bounds().unwrap();
    assert!(close(hi, [1.0, 0.0, 0.0]), "{hi:?}");
}

#[test]
fn polygonal_face_set_with_a_hole() {
    let body = "#10=IFCCARTESIANPOINTLIST3D(((0.,0.,0.),(4.,0.,0.),(4.,4.,0.),(0.,4.,0.),(1.,1.,0.),(2.,1.,0.),(2.,2.,0.),(1.,2.,0.)),$);\n\
        #11=IFCINDEXEDPOLYGONALFACEWITHVOIDS((1,2,3,4),((5,8,7,6)));\n#20=IFCINDEXEDPOLYGONALFACE((1,4,3,2));\n\
        #21=IFCPOLYGONALFACESET(#10,.F.,(#11),$);\n";
    let src = file("IFC4X3_ADD2", "$", &format!("{body}{}", product("IFCSLAB", "#21")));
    let (m, r) = read(src.as_bytes()).unwrap();
    let m = m.unwrap();
    assert_eq!(r.schema, "IFC4X3_ADD2");
    assert_eq!(m.triangle_count(), 8);
    let s = geom::stats(&m);
    assert!((s.surface - 15.0).abs() < 1e-6, "{}", s.surface);
    // The slab's default colour, named after its type.
    let mat = m.meshes[0].material.as_ref().unwrap();
    assert_eq!((mat.name.as_str(), mat.color), ("IfcSlab", default_color("IFCSLAB")));
    assert_ne!(default_color("IFCSLAB"), default_color("IFCWALL"));
    assert_ne!(default_color("IFCWINDOW"), default_color("IFCWALL"));
}

/// A box from (0,0,0) to (2,1,0.5) as a faceted brep (#30), its points
/// from #20 and faces from #40. One face is stored the other way round
/// with a false orientation, which must cancel out.
fn brep_box() -> String {
    let c = [[0., 0., 0.], [2., 0., 0.], [2., 1., 0.], [0., 1., 0.], [0., 0., 0.5], [2., 0., 0.5], [2., 1., 0.5], [0., 1., 0.5]];
    let quads = [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [2, 3, 7, 6], [1, 2, 6, 5], [0, 4, 7, 3]];
    let mut s = String::new();
    for (k, p) in c.iter().enumerate() {
        s += &format!("#{}=IFCCARTESIANPOINT(({:?},{:?},{:?}));\n", 20 + k, p[0], p[1], p[2]);
    }
    let mut faces = Vec::new();
    for (k, q) in quads.iter().enumerate() {
        let (loop_id, bound_id, face_id) = (40 + 3 * k, 41 + 3 * k, 42 + 3 * k);
        let mut q = q.to_vec();
        let orient = if k == 2 {
            q.reverse();
            ".F."
        } else {
            ".T."
        };
        let refs: Vec<String> = q.iter().map(|i| format!("#{}", 20 + i)).collect();
        s += &format!("#{loop_id}=IFCPOLYLOOP(({}));\n#{bound_id}=IFCFACEOUTERBOUND(#{loop_id},{orient});\n#{face_id}=IFCFACE((#{bound_id}));\n", refs.join(","));
        faces.push(format!("#{face_id}"));
    }
    s += &format!("#29=IFCCLOSEDSHELL(({}));\n#30=IFCFACETEDBREP(#29);\n", faces.join(","));
    s
}

#[test]
fn faceted_brep_box() {
    let src = file("IFC2X3", "$", &format!("{}{}", brep_box(), product("IFCWALLSTANDARDCASE", "#30")));
    let m = parse(src.as_bytes()).unwrap();
    assert_eq!(m.triangle_count(), 12);
    let s = geom::stats(&m);
    assert!((s.volume.unwrap() - 1.0).abs() < 1e-6, "{:?}", s.volume);
    assert!(close(s.size, [2.0, 0.5, 1.0]), "{:?}", s.size);
    assert_eq!(m.meshes[0].name, "IfcWallStandardCase");
}

#[test]
fn faceted_face_with_a_hole_in_a_surface_model() {
    let body = "#20=IFCCARTESIANPOINT((0.,0.,0.));#21=IFCCARTESIANPOINT((4.,0.,0.));#22=IFCCARTESIANPOINT((4.,0.,4.));#23=IFCCARTESIANPOINT((0.,0.,4.));\n\
        #24=IFCCARTESIANPOINT((1.,0.,1.));#25=IFCCARTESIANPOINT((3.,0.,1.));#26=IFCCARTESIANPOINT((3.,0.,3.));#27=IFCCARTESIANPOINT((1.,0.,3.));\n\
        #30=IFCPOLYLOOP((#20,#21,#22,#23,#20));#31=IFCFACEOUTERBOUND(#30,.T.);\n\
        #32=IFCPOLYLOOP((#24,#27,#26,#25));#33=IFCFACEBOUND(#32,.T.);\n\
        #34=IFCFACE((#33,#31));#35=IFCOPENSHELL((#34));#36=IFCSHELLBASEDSURFACEMODEL((#35));\n";
    let src = file("IFC4", "$", &format!("{body}{}", product("IFCWINDOW", "#36")));
    let m = parse(src.as_bytes()).unwrap();
    assert_eq!(m.triangle_count(), 8);
    assert!((geom::stats(&m).surface - 12.0).abs() < 1e-6);
}

#[test]
fn extruded_solid_only_falls_back() {
    let body = "#10=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,1.,2.);\n#11=IFCDIRECTION((0.,0.,1.));\n#20=IFCEXTRUDEDAREASOLID(#10,#4,#11,3.);\n";
    let src = file("IFC4", "$", &format!("{body}{}", product("IFCWALL", "#20")));
    assert_eq!(parse(src.as_bytes()).unwrap_err().0, NO_GEOMETRY);
    let (m, r) = read(src.as_bytes()).unwrap();
    assert!(m.is_none());
    assert_eq!((r.shown, r.skipped, r.triangles), (0, 1, 0));
    assert_eq!(r.unsupported, vec![("IfcExtrudedAreaSolid".to_string(), 1)]);
    // Without web-ifc the thumbnail renderer gives up too, so QuickLook gets
    // its turn; with it the wall is drawn.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wall.ifc");
    std::fs::write(&path, &src).unwrap();
    assert_eq!(crate::model3d::raster::thumbnail(&path, "ifc").is_some(), full_geometry());
    let r = summary(&path).unwrap();
    assert_eq!((r.shown, r.skipped), if full_geometry() { (1, 0) } else { (0, 1) });
}

#[test]
fn mixed_file_shows_the_tessellated_part() {
    let body = format!(
        "{}{}#50=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,1.,2.);\n#51=IFCDIRECTION((0.,0.,1.));\n#52=IFCEXTRUDEDAREASOLID(#50,#4,#51,3.);\n\
         #53=IFCSHAPEREPRESENTATION(#2,'Body','SweptSolid',(#52));\n#54=IFCPRODUCTDEFINITIONSHAPE($,$,(#53));\n\
         #55=IFCWALL('0003',$,'W',$,$,#14,#54,$,$);\n\
         #56=IFCOPENINGELEMENT('0004',$,'O',$,$,#14,#13,$,$);\n",
        cube_tfs(1.0),
        product("IFCCOLUMN", "#11")
    );
    let src = file("IFC4", "$", &body);
    let (m, r) = read(src.as_bytes()).unwrap();
    assert_eq!(m.unwrap().triangle_count(), 12);
    // The opening shares the cube's shape but is never drawn or counted.
    assert_eq!((r.shown, r.skipped, r.triangles), (1, 1, 12));
}

#[test]
fn body_is_preferred_and_axis_ignored() {
    let body = format!(
        "{}#60=IFCSHAPEREPRESENTATION(#2,'Axis','Curve2D',(#61));\n#61=IFCPOLYLINE((#6));\n\
         #12=IFCSHAPEREPRESENTATION(#2,'Body','Tessellation',(#11));\n#13=IFCPRODUCTDEFINITIONSHAPE($,$,(#60,#12));\n\
         #14=IFCLOCALPLACEMENT($,#4);\n#15=IFCBEAM('0002',$,'B',$,$,#14,#13,$,$);\n",
        cube_tfs(1.0)
    );
    let (m, r) = read(file("IFC4", "$", &body).as_bytes()).unwrap();
    assert_eq!(m.unwrap().triangle_count(), 12);
    assert!(r.unsupported.is_empty());
}

#[test]
fn mapped_items_and_placement_chains() {
    let body = format!(
        "{}#12=IFCSHAPEREPRESENTATION(#2,'Body','Tessellation',(#11));\n#16=IFCREPRESENTATIONMAP(#4,#12);\n\
         #17=IFCCARTESIANPOINT((10.,0.,0.));#18=IFCCARTESIANTRANSFORMATIONOPERATOR3D($,$,#17,2.,$);#19=IFCMAPPEDITEM(#16,#18);\n\
         #20=IFCCARTESIANPOINT((0.,5.,0.));#21=IFCDIRECTION((0.,1.,0.));\n\
         #22=IFCCARTESIANTRANSFORMATIONOPERATOR3D(#21,$,#20,$,$);#23=IFCMAPPEDITEM(#16,#22);\n\
         #24=IFCSHAPEREPRESENTATION(#2,'Body','MappedRepresentation',(#19,#23));#25=IFCPRODUCTDEFINITIONSHAPE($,$,(#24));\n\
         #30=IFCCARTESIANPOINT((100.,0.,0.));#31=IFCAXIS2PLACEMENT3D(#30,$,$);#32=IFCLOCALPLACEMENT($,#31);\n\
         #33=IFCCARTESIANPOINT((0.,0.,3.));#34=IFCAXIS2PLACEMENT3D(#33,$,$);#35=IFCLOCALPLACEMENT(#32,#34);\n\
         #36=IFCCARTESIANPOINT((1.,0.,0.));#37=IFCDIRECTION((0.,0.,1.));#38=IFCDIRECTION((0.,1.,0.));\n\
         #39=IFCAXIS2PLACEMENT3D(#36,#37,#38);#40=IFCLOCALPLACEMENT(#35,#39);\n\
         #41=IFCSITE('0001',$,'S',$,$,#32,$,$,.ELEMENT.,$,$,$,$,$);\n\
         #42=IFCFURNISHINGELEMENT('0005',$,'F',$,$,#40,#25,$);\n",
        cube_tfs(1.0)
    );
    let (m, r) = read(file("IFC4", "$", &body).as_bytes()).unwrap();
    let m = m.unwrap();
    assert_eq!((r.shown, r.triangles), (1, 24));
    // In IFC axes: the scaled copy lands on x 99..101, y 10..12, z 3..5
    // and the turned one on x 95..96, y -1..0, z 3..4; so x 95..101,
    // y -1..12, z 3..5, which is (x, z, -y) in the viewer.
    let (lo, hi) = m.bounds().unwrap();
    assert!(close(lo, [95.0, 3.0, -12.0]) && close(hi, [101.0, 5.0, 1.0]), "{lo:?} {hi:?}");
    assert!((geom::stats(&m).surface - (6.0 * 4.0 + 6.0)).abs() < 1e-4);
}

#[test]
fn millimetres_and_feet_become_metres() {
    let body = format!(
        "{}#12=IFCSHAPEREPRESENTATION(#2,'Body','Tessellation',(#11));\n#13=IFCPRODUCTDEFINITIONSHAPE($,$,(#12));\n\
         #16=IFCCARTESIANPOINT((2000.,0.,0.));#17=IFCAXIS2PLACEMENT3D(#16,$,$);#14=IFCLOCALPLACEMENT($,#17);\n\
         #15=IFCSLAB('0002',$,'S',$,$,#14,#13,$,$);\n",
        cube_tfs(1000.0)
    );
    let m = parse(file("IFC4", ".MILLI.", &body).as_bytes()).unwrap();
    let (lo, hi) = m.bounds().unwrap();
    assert!(close(lo, [2.0, 0.0, -1.0]) && close(hi, [3.0, 1.0, 0.0]), "{lo:?} {hi:?}");
    // A conversion-based foot.
    let src = file("IFC2X3", "$", &format!("{}{}", cube_tfs(1.0), product("IFCSLAB", "#11")))
        .replace("#3=IFCUNITASSIGNMENT((#5));", "#3=IFCUNITASSIGNMENT((#7));#7=IFCCONVERSIONBASEDUNIT(#8,.LENGTHUNIT.,'FOOT',#9);#8=IFCDIMENSIONALEXPONENTS(1,0,0,0,0,0,0);#9=IFCMEASUREWITHUNIT(IFCLENGTHMEASURE(0.3048),#5);");
    let s = geom::stats(&parse(src.as_bytes()).unwrap());
    assert!(close(s.size, [0.3048; 3]), "{:?}", s.size);
}

#[test]
fn georeferenced_models_are_moved_to_the_origin() {
    let body = format!(
        "{}#12=IFCSHAPEREPRESENTATION(#2,'Body','Tessellation',(#11));\n#13=IFCPRODUCTDEFINITIONSHAPE($,$,(#12));\n\
         #16=IFCCARTESIANPOINT((500000.,4000000.,0.));#17=IFCAXIS2PLACEMENT3D(#16,$,$);#14=IFCLOCALPLACEMENT($,#17);\n\
         #15=IFCSLAB('0002',$,'S',$,$,#14,#13,$,$);\n",
        cube_tfs(1.0)
    );
    let m = parse(file("IFC4", "$", &body).as_bytes()).unwrap();
    let (lo, hi) = m.bounds().unwrap();
    assert!(close(lo, [-0.5; 3]) && close(hi, [0.5; 3]), "{lo:?} {hi:?}");
}

#[test]
fn material_styles_colour_products_and_their_types() {
    let body = format!(
        "{}{}#70=IFCMATERIAL('Brick',$,$);\n#71=IFCCOLOURRGB($,0.5,0.25,0.);#72=IFCSURFACESTYLERENDERING(#71,$,$,$,$,$,$,$,.NOTDEFINED.);\n\
         #73=IFCSURFACESTYLE($,.BOTH.,(#72));#74=IFCSTYLEDITEM($,(#73),$);\n#75=IFCSTYLEDREPRESENTATION(#2,$,$,(#74));\n\
         #76=IFCMATERIALDEFINITIONREPRESENTATION($,$,(#75),#70);\n#77=IFCWALLTYPE('0009',$,'T',$,$,$,$,$,$,.STANDARD.);\n\
         #78=IFCRELASSOCIATESMATERIAL('000A',$,$,$,(#77),#80);#80=IFCMATERIALLAYERSETUSAGE(#81,.AXIS2.,.POSITIVE.,0.,$);\n\
         #81=IFCMATERIALLAYERSET((#82),$,$);#82=IFCMATERIALLAYER(#70,0.2,$,$,$,$,$);\n#79=IFCRELDEFINESBYTYPE('000B',$,$,$,(#15),#77);\n",
        cube_tfs(1.0),
        product("IFCWALL", "#11")
    );
    let m = parse(file("IFC4", "$", &body).as_bytes()).unwrap();
    let mat = m.meshes[0].material.as_ref().unwrap();
    assert_eq!((mat.name.as_str(), mat.color), ("Brick", [0.5, 0.25, 0.0]));
}

#[test]
fn corrupt_and_hostile_files_fail_cleanly() {
    let good = file("IFC4", "$", &format!("{}{}", cube_tfs(1.0), product("IFCSLAB", "#11")));
    // Truncated anywhere: an error (or, past the data section, a model).
    for n in (0..good.len()).step_by(7) {
        let _ = read(&good.as_bytes()[..n]);
    }
    assert!(read(&good.as_bytes()[..good.len() / 2]).is_err());
    assert!(parse(b"\x00\xff garbage").is_err());
    assert!(parse(b"").is_err());
    // Placements and mapped items that refer to themselves end.
    let body = format!(
        "{}#12=IFCSHAPEREPRESENTATION(#2,'Body','MappedRepresentation',(#11,#19));\n#16=IFCREPRESENTATIONMAP(#4,#12);#19=IFCMAPPEDITEM(#16,$);\n\
         #13=IFCPRODUCTDEFINITIONSHAPE($,$,(#12));#14=IFCLOCALPLACEMENT(#14,#4);\n#15=IFCSLAB('0002',$,'S',$,$,#14,#13,$,$);\n",
        cube_tfs(1.0)
    );
    let (m, _) = read(file("IFC4", "$", &body).as_bytes()).unwrap();
    // The cube itself plus one copy per level of mapping.
    assert_eq!(m.unwrap().triangle_count(), 12 * (MAX_MAP_DEPTH + 1));
    // Wrong types where references are expected, and dangling references.
    let body = "#10=IFCCARTESIANPOINTLIST3D((('a',1.,2.),(1.,$,0.)),$);\n#11=IFCTRIANGULATEDFACESET(#99,$,.T.,((1,2,3)),$);\n\
        #20=IFCPOLYGONALFACESET(#10,$,(#11,#4,5),$);#21=IFCFACETEDBREP(#4);#22=IFCMAPPEDITEM(#11,#11);\n\
        #12=IFCSHAPEREPRESENTATION(#2,'Body',$,(#11,#20,#21,#22,'x',#999));\n#13=IFCPRODUCTDEFINITIONSHAPE($,$,(#12,$));\n\
        #14=IFCLOCALPLACEMENT(#4,#99);\n#15=IFCSLAB('0002',$,'S',$,$,#14,#13,$,$);\n";
    let (m, r) = read(file("IFC4", "$", body).as_bytes()).unwrap();
    assert!(m.is_none());
    assert_eq!(r.skipped, 1);
    // Deep nesting inside an instance.
    let deep = file("IFC4", "$", &format!("#10=IFCX({}1{});\n", "(".repeat(100_000), ")".repeat(100_000)));
    assert!(read(deep.as_bytes()).is_err());
}

#[test]
fn display_names() {
    assert_eq!(display_name("IFCWALLSTANDARDCASE"), "IfcWallStandardCase");
    assert_eq!(display_name("IFCFOOBAR"), "IfcFoobar");
    assert_eq!(display_name("IFC"), "IFC");
    assert_eq!(display_name("OTHER"), "OTHER");
}
