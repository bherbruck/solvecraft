use solvecraft_geom::{Loop2, Plane, Region2, Vec2, Vec3};

use super::*;
use crate::{BoolOp, boolean, box_solid, cylinder, fillet, measure, revolve, sphere, step_export, torus};

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-9)
}

/// Export → import → same volume, area and face count.
fn round_trip(b: &Body) -> StepImport {
    let text = step_export(&[b], "test").unwrap();
    step_validate(&text).unwrap();
    assert_eq!(crate::step_orientation_errors(&text).unwrap(), Vec::<String>::new());
    let imp = step_import(&text).unwrap();
    assert_eq!(imp.bodies.len(), 1, "{:?}", imp.warnings);
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    let (m0, m1) = (measure(b).unwrap(), measure(&imp.bodies[0].body).unwrap());
    assert!(rel(m1.volume, m0.volume) < 1e-3, "volume {} vs {}", m1.volume, m0.volume);
    assert!(rel(m1.area, m0.area) < 1e-3, "area {} vs {}", m1.area, m0.area);
    // Pieces of one analytic surface are written as one face (volume and area above check
    // that nothing was lost).
    assert!(imp.bodies[0].file_faces <= b.face_count());
    assert!(imp.bodies[0].closed);
    imp
}

#[test]
fn round_trip_primitives() {
    round_trip(&box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap());
    round_trip(&cylinder(Vec3::new(1.0, 2.0, 3.0), Vec3::new(0.0, 1.0, 1.0), 5.0, 12.0).unwrap());
    round_trip(&sphere(Vec3::new(1.0, 2.0, 3.0), 10.0).unwrap());
    round_trip(&torus(Vec3::ZERO, 20.0, 4.0).unwrap());
}

#[test]
fn round_trip_features() {
    let plate = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 10.0)).unwrap();
    let pin = cylinder(Vec3::new(20.0, 15.0, -1.0), Vec3::Z, 5.0, 12.0).unwrap();
    let holed = boolean(&plate, &pin, BoolOp::Cut).unwrap().unwrap();
    round_trip(&holed);
    let edge = Vec3::new(20.0, 0.0, 10.0);
    round_trip(&fillet(&plate, &[edge], 3.0).unwrap());
    let ring = Region2 { outer: Loop2::circle(Vec2::new(10.0, 0.0), 3.0), holes: vec![] };
    let donut = revolve(&Plane::XZ, &[ring], Vec2::ZERO, Vec2::Y, 1.5).unwrap();
    round_trip(&donut[0]);
}

#[test]
fn several_bodies_and_names() {
    let a = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
    let b = box_solid(Vec3::new(20.0, 0.0, 0.0), Vec3::new(30.0, 10.0, 10.0)).unwrap();
    let text = step_export(&[&a, &b], "test").unwrap();
    let imp = step_import(&text).unwrap();
    assert_eq!(imp.bodies.len(), 2);
    assert!(imp.bodies.iter().all(|b| !b.name.is_empty()));
    let vol: f64 = imp.bodies.iter().map(|b| measure(&b.body).unwrap().volume).sum();
    assert!(rel(vol, 2000.0) < 1e-6);
}

#[test]
fn imported_bodies_take_features() {
    let plate = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 10.0)).unwrap();
    let imp = step_import(&step_export(&[&plate], "t").unwrap()).unwrap();
    let body = &imp.bodies[0].body;
    // Fillet an edge, cut a hole through it.
    let rounded = fillet(body, &[Vec3::new(20.0, 0.0, 10.0)], 2.0).unwrap();
    let v_fillet = 40.0 * (4.0 - std::f64::consts::PI);
    assert!(rel(measure(&rounded).unwrap().volume, 12000.0 - v_fillet) < 1e-3);
    let pin = cylinder(Vec3::new(20.0, 15.0, -1.0), Vec3::Z, 5.0, 12.0).unwrap();
    let holed = boolean(&rounded, &pin, BoolOp::Cut).unwrap().unwrap();
    let want = 12000.0 - v_fillet - std::f64::consts::PI * 25.0 * 10.0;
    assert!(rel(measure(&holed).unwrap().volume, want) < 1e-3);
}

const HEAD: &str = "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\nFILE_NAME('t','',(''),(''),'','hand','');\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));\nENDSEC;\nDATA;\n";
const TAIL: &str = "ENDSEC;\nEND-ISO-10303-21;\n";

/// A hand-written unit cube brep (`#100`…) in a representation with the given unit and items.
fn cube_entities() -> String {
    let mut s = String::new();
    let pts = [(0, 0, 0), (1, 0, 0), (1, 1, 0), (0, 1, 0), (0, 0, 1), (1, 0, 1), (1, 1, 1), (0, 1, 1)];
    for (i, (x, y, z)) in pts.iter().enumerate() {
        s += &format!("#{}=CARTESIAN_POINT('',({x}.,{y}.,{z}.));\n#{}=VERTEX_POINT('',#{});\n", 200 + i, 210 + i, 200 + i);
    }
    // 12 edges as lines (direction vector filler; trimmed by vertices).
    let edges = [(0, 1), (1, 2), (2, 3), (3, 0), (4, 5), (5, 6), (6, 7), (7, 4), (0, 4), (1, 5), (2, 6), (3, 7)];
    s += "#290=DIRECTION('',(1.,0.,0.));\n#291=VECTOR('',#290,1.);\n";
    for (k, (a, b)) in edges.iter().enumerate() {
        s += &format!("#{}=LINE('',#{},#291);\n#{}=EDGE_CURVE('',#{},#{},#{},.T.);\n", 300 + k, 200 + a, 320 + k, 210 + a, 210 + b, 300 + k);
    }
    // Faces: (edge, forward?) loops counter-clockwise seen from outside, and plane frames.
    let faces: [(&[(usize, bool)], (i32, i32, i32), (i32, i32, i32)); 6] = [
        (&[(3, false), (2, false), (1, false), (0, false)], (0, 0, 0), (0, 0, -1)),
        (&[(4, true), (5, true), (6, true), (7, true)], (0, 0, 1), (0, 0, 1)),
        (&[(0, true), (9, true), (4, false), (8, false)], (0, 0, 0), (0, -1, 0)),
        (&[(1, true), (10, true), (5, false), (9, false)], (1, 0, 0), (1, 0, 0)),
        (&[(2, true), (11, true), (6, false), (10, false)], (1, 1, 0), (0, 1, 0)),
        (&[(3, true), (8, true), (7, false), (11, false)], (0, 1, 0), (-1, 0, 0)),
    ];
    let mut face_ids = Vec::new();
    for (f, (lp, o, n)) in faces.iter().enumerate() {
        let base = 400 + f * 20;
        let oes: Vec<String> = lp
            .iter()
            .enumerate()
            .map(|(j, (e, fwd))| {
                s += &format!("#{}=ORIENTED_EDGE('',*,*,#{},{});\n", base + j, 320 + e, if *fwd { ".T." } else { ".F." });
                format!("#{}", base + j)
            })
            .collect();
        s += &format!("#{}=EDGE_LOOP('',({}));\n#{}=FACE_OUTER_BOUND('',#{},.T.);\n", base + 5, oes.join(","), base + 6, base + 5);
        s += &format!("#{}=CARTESIAN_POINT('',({}.,{}.,{}.));\n#{}=DIRECTION('',({}.,{}.,{}.));\n", base + 7, o.0, o.1, o.2, base + 8, n.0, n.1, n.2);
        s += &format!("#{}=AXIS2_PLACEMENT_3D('',#{},#{},$);\n#{}=PLANE('',#{});\n", base + 9, base + 7, base + 8, base + 10, base + 9);
        s += &format!("#{}=ADVANCED_FACE('',(#{}),#{},.T.);\n", base + 11, base + 6, base + 10);
        face_ids.push(format!("#{}", base + 11));
    }
    s += &format!("#100=CLOSED_SHELL('',({}));\n#101=MANIFOLD_SOLID_BREP('Cube',#100);\n", face_ids.join(","));
    s
}

fn context(id: u64, unit: &str) -> String {
    format!(
        "#{id}=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{}))GLOBAL_UNIT_ASSIGNED_CONTEXT((#{},#{}))REPRESENTATION_CONTEXT('',''));\n#{}=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-6),#{},'','');\n#{}={unit};\n#{}=(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.));\n",
        id + 1,
        id + 2,
        id + 3,
        id + 1,
        id + 2,
        id + 2,
        id + 3
    )
}

fn product(pd: u64, name: &str) -> String {
    format!(
        "#{}=PRODUCT('{name}','{name}','',(#{}));\n#{}=PRODUCT_CONTEXT('',#9,'mechanical');\n#{}=PRODUCT_DEFINITION_FORMATION('','',#{});\n#{pd}=PRODUCT_DEFINITION('design','',#{},#8);\n#{}=PRODUCT_DEFINITION_SHAPE('','',#{pd});\n",
        pd + 1,
        pd + 2,
        pd + 2,
        pd + 3,
        pd + 1,
        pd + 3,
        pd + 4
    )
}

#[test]
fn hand_written_part_in_inches_with_colour() {
    let mut t = String::from(HEAD);
    t += "#8=PRODUCT_DEFINITION_CONTEXT('part definition',#9,'design');\n#9=APPLICATION_CONTEXT('');\n";
    t += &cube_entities();
    t += &context(
        20,
        "(CONVERSION_BASED_UNIT('INCH',#30)LENGTH_UNIT()NAMED_UNIT(*));\n#30=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#31);\n#31=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))",
    );
    t += &product(50, "Widget");
    t += "#60=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#101),#20);\n#61=SHAPE_DEFINITION_REPRESENTATION(#54,#62);\n#62=SHAPE_REPRESENTATION('',(),#20);\n#63=SHAPE_REPRESENTATION_RELATIONSHIP('','',#62,#60);\n";
    t += "#70=STYLED_ITEM('',(#71),#101);\n#71=PRESENTATION_STYLE_ASSIGNMENT((#72));\n#72=SURFACE_STYLE_USAGE(.BOTH.,#73);\n#73=SURFACE_SIDE_STYLE('',(#74));\n#74=SURFACE_STYLE_FILL_AREA(#75);\n#75=FILL_AREA_STYLE('',(#76));\n#76=FILL_AREA_STYLE_COLOUR('',#77);\n#77=COLOUR_RGB('',1.,0.5,0.);\n";
    t += TAIL;
    let imp = step_import(&t).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    assert_eq!(imp.bodies.len(), 1);
    let b = &imp.bodies[0];
    assert_eq!(b.name, "Cube");
    assert_eq!(b.path, vec!["Widget".to_string()]);
    assert_eq!(b.color, Some([1.0, 0.5, 0.0]));
    let m = measure(&b.body).unwrap();
    assert!(rel(m.volume, 25.4f64.powi(3)) < 1e-9, "{}", m.volume);
    assert_eq!(m.faces, 6);
    assert_eq!(imp.tree.len(), 1);
    assert_eq!(imp.tree[0].name, "Widget");
}

#[test]
fn hand_written_assembly() {
    let mut t = String::from(HEAD);
    t += "#8=PRODUCT_DEFINITION_CONTEXT('part definition',#9,'design');\n#9=APPLICATION_CONTEXT('');\n";
    t += &cube_entities();
    t += &context(20, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.CENTI.,.METRE.))");
    t += &context(30, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
    // Part (cube, in cm) and assembly (mm) with two placed instances of it.
    t += &product(50, "Block");
    t += &product(70, "Assembly");
    t += "#60=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#101,#65),#20);\n#61=SHAPE_DEFINITION_REPRESENTATION(#54,#60);\n";
    t += "#65=AXIS2_PLACEMENT_3D('',#66,$,$);\n#66=CARTESIAN_POINT('',(0.,0.,0.));\n";
    t += "#80=SHAPE_REPRESENTATION('',(#81,#84),#30);\n#81=AXIS2_PLACEMENT_3D('',#82,$,$);\n#82=CARTESIAN_POINT('',(0.,0.,0.));\n";
    t += "#84=AXIS2_PLACEMENT_3D('',#85,#86,#87);\n#85=CARTESIAN_POINT('',(100.,0.,0.));\n#86=DIRECTION('',(0.,0.,1.));\n#87=DIRECTION('',(0.,1.,0.));\n";
    t += "#88=SHAPE_DEFINITION_REPRESENTATION(#74,#80);\n";
    for (k, item) in [(0u64, 81u64), (1, 84)] {
        let n = 600 + k * 10;
        t += &format!("#{n}=NEXT_ASSEMBLY_USAGE_OCCURRENCE('{k}','Block:{k}','',#70,#50,$);\n");
        t += &format!("#{}=PRODUCT_DEFINITION_SHAPE('','',#{n});\n", n + 1);
        t += &format!("#{}=ITEM_DEFINED_TRANSFORMATION('','',#65,#{item});\n", n + 2);
        t += &format!(
            "#{}=(REPRESENTATION_RELATIONSHIP('','',#60,#80)REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#{})SHAPE_REPRESENTATION_RELATIONSHIP());\n",
            n + 3,
            n + 2
        );
        t += &format!("#{}=CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#{},#{});\n", n + 4, n + 3, n + 1);
    }
    t += TAIL;
    let imp = step_import(&t).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    assert_eq!(imp.bodies.len(), 2);
    let boxes: Vec<_> = imp.bodies.iter().map(|b| measure(&b.body).unwrap()).collect();
    for m in &boxes {
        assert!(rel(m.volume, 1000.0) < 1e-9, "{}", m.volume);
    }
    // First at the origin, second moved 100 mm along X and turned 90° about Z.
    assert!(boxes[0].bbox.min.dist(Vec3::ZERO) < 1e-6 && boxes[0].bbox.max.dist(Vec3::new(10.0, 10.0, 10.0)) < 1e-6);
    assert!(boxes[1].bbox.min.dist(Vec3::new(90.0, 0.0, 0.0)) < 1e-6, "{:?}", boxes[1].bbox);
    assert!(boxes[1].bbox.max.dist(Vec3::new(100.0, 10.0, 10.0)) < 1e-6, "{:?}", boxes[1].bbox);
    assert_eq!(imp.tree.len(), 1);
    let root = &imp.tree[0];
    assert_eq!(root.name, "Assembly");
    assert_eq!(root.children.len(), 2);
    assert!(root.children.iter().all(|c| c.name == "Block" && c.bodies.len() == 1));
    assert_eq!(imp.bodies[1].path, vec!["Assembly".to_string(), "Block".to_string()]);
    assert!((root.children[1].transform[3][0] - 100.0).abs() < 1e-9);
}

#[test]
fn unsupported_face_is_left_out_and_reported() {
    let mut t = String::from(HEAD);
    let cube = cube_entities().replace("#410=PLANE('',#409);", "#410=OFFSET_SURFACE('',#409,1.,.F.);");
    t += &cube;
    t += "#60=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#101),#20);\n";
    t += &context(20, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
    t += TAIL;
    let imp = step_import(&t).unwrap();
    assert_eq!(imp.bodies.len(), 1);
    assert!(!imp.bodies[0].closed);
    assert_eq!(imp.bodies[0].body.face_count(), 5);
    assert!(imp.warnings.iter().any(|w| w.contains("OFFSET_SURFACE")), "{:?}", imp.warnings);
    assert!(imp.warnings.iter().any(|w| w.contains("open body")), "{:?}", imp.warnings);
}

#[test]
fn hostile_files_never_panic() {
    let good = step_export(&[&box_solid(Vec3::ZERO, Vec3::new(5.0, 5.0, 5.0)).unwrap()], "t").unwrap();
    let mut cases: Vec<String> = vec![
        String::new(),
        "ISO-10303-21;".into(),
        format!("{HEAD}{TAIL}"),
        format!("{HEAD}#1=MANIFOLD_SOLID_BREP('',#1);\n#2=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#1),#3);\n{TAIL}"),
        format!(
            "{HEAD}#1=MANIFOLD_SOLID_BREP('',#2);\n#2=CLOSED_SHELL('',(#3));\n#3=ADVANCED_FACE('',(#4),#5,.T.);\n#4=FACE_BOUND('',#6,.T.);\n#6=EDGE_LOOP('',(#7));\n#7=ORIENTED_EDGE('',*,*,#7,.T.);\n#5=PLANE('',#5);\n#9=SHAPE_REPRESENTATION('',(#1),#10);\n{TAIL}"
        ),
        format!(
            "{HEAD}#1=CARTESIAN_POINT('',(1.E308,1.E308,1.E308,1.,2.));\n#2=MANIFOLD_SOLID_BREP('',#1);\n#3=SHAPE_REPRESENTATION('',(#2),$);\n{TAIL}"
        ),
        format!(
            "{HEAD}#1=NEXT_ASSEMBLY_USAGE_OCCURRENCE('','','',#2,#2,$);\n#2=PRODUCT_DEFINITION('','',#3,#3);\n#3=PRODUCT_DEFINITION_FORMATION('','',#3);\n{TAIL}"
        ),
    ];
    // Truncations and byte flips of a real file.
    for cut in (0..good.len()).step_by(97) {
        cases.push(good.get(..cut).unwrap_or("").to_string());
    }
    for (i, from, to) in [(0, "1.0", "-1.0"), (1, "#1", "#9999"), (2, ".T.", ".F."), (3, "(0.0", "(nan"), (4, "B_SPLINE", "X_SPLINE")] {
        let _ = i;
        cases.push(good.replacen(from, to, 3));
    }
    cases.push(good.replace("CLOSED_SHELL", "OPEN_SHELL"));
    for c in &cases {
        let r = std::panic::catch_unwind(|| step_import(c));
        assert!(r.is_ok(), "panicked on {:?}", c.get(..c.len().min(200)));
    }
}

#[test]
fn assembly_cycles_are_bounded() {
    let mut t = String::from(HEAD);
    t += "#8=PRODUCT_DEFINITION_CONTEXT('part definition',#9,'design');\n#9=APPLICATION_CONTEXT('');\n";
    t += &cube_entities();
    t += &context(20, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
    t += &product(50, "A");
    t += &product(70, "B");
    t += "#60=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#101),#20);\n#61=SHAPE_DEFINITION_REPRESENTATION(#54,#60);\n";
    // A contains B contains A.
    t += "#90=NEXT_ASSEMBLY_USAGE_OCCURRENCE('1','','',#50,#70,$);\n#91=NEXT_ASSEMBLY_USAGE_OCCURRENCE('2','','',#70,#50,$);\n";
    t += TAIL;
    let r = step_import(&t);
    // Every product is somebody's component: the solid is still imported (outside the tree).
    let imp = r.unwrap();
    assert_eq!(imp.bodies.len(), 1);
}

#[test]
fn exponential_assemblies_are_bounded() {
    // P0 uses P1 twice, P1 uses P2 twice, … 30 levels: 2^30 occurrences if followed blindly.
    let mut t = String::from(HEAD);
    t += "#8=PRODUCT_DEFINITION_CONTEXT('part definition',#9,'design');\n#9=APPLICATION_CONTEXT('');\n";
    t += &cube_entities();
    t += &context(20, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
    let levels = 30u64;
    for l in 0..=levels {
        t += &product(1000 + l * 10, &format!("P{l}"));
        if l < levels {
            for k in 0..2 {
                t += &format!("#{}=NEXT_ASSEMBLY_USAGE_OCCURRENCE('','','',#{},#{},$);\n", 2000 + l * 10 + k, 1000 + l * 10, 1000 + (l + 1) * 10);
            }
        }
    }
    t += &format!("#60=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#101),#20);\n#61=SHAPE_DEFINITION_REPRESENTATION(#{},#60);\n", 1000 + levels * 10 + 4);
    t += TAIL;
    let imp = step_import(&t).unwrap();
    assert!(!imp.bodies.is_empty() && imp.bodies.len() <= MAX_BODIES);
    assert!(imp.warnings.iter().any(|w| w.contains("left out")), "{:?}", imp.warnings);
}

#[test]
fn validator_catches_broken_files() {
    assert!(step_validate(&format!("{HEAD}#1=SHAPE_DEFINITION_REPRESENTATION(#2);\n#2=B();\n{TAIL}")).is_ok());
    assert!(
        step_validate(&format!("{HEAD}#1=SHAPE_DEFINITION_REPRESENTATION(#2);\n#2=B();\n#3=C();\n{TAIL}")).unwrap_err().contains("not referenced")
    );
    assert!(step_validate(&format!("{HEAD}#1=SHAPE_DEFINITION_REPRESENTATION(#2);\n#2=B();\n#2=C();\n{TAIL}")).unwrap_err().contains("duplicate"));
    assert!(step_validate(&format!("{HEAD}#1=A((#7));\n{TAIL}")).unwrap_err().contains("undefined"));
    assert!(step_validate(&format!("{HEAD}#1=SHAPE_DEFINITION_REPRESENTATION(#5);\n#5=B();\n{TAIL}")).unwrap_err().contains("dense"));
}

#[test]
fn split_band_edges_are_seams() {
    // A quarter pipe (torus R 10, r 3) whose face wraps around the tube with a seam edge: it is
    // split for the kernel, and the split edge is drawn as a seam.
    let mut t = String::from(HEAD);
    t += "#1=CARTESIAN_POINT('',(0.,0.,0.));\n#2=DIRECTION('',(0.,0.,1.));\n#3=DIRECTION('',(1.,0.,0.));\n#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);\n";
    t += "#5=TOROIDAL_SURFACE('',#4,10.,3.);\n";
    t += "#10=CARTESIAN_POINT('',(13.,0.,0.));\n#11=CARTESIAN_POINT('',(0.,13.,0.));\n#12=VERTEX_POINT('',#10);\n#13=VERTEX_POINT('',#11);\n";
    // Tube circles at azimuth 0 (plane XZ) and 90 degrees (plane YZ), the seam around Z.
    t += "#20=CARTESIAN_POINT('',(10.,0.,0.));\n#21=DIRECTION('',(0.,-1.,0.));\n#22=AXIS2_PLACEMENT_3D('',#20,#21,#3);\n#23=CIRCLE('',#22,3.);\n";
    t += "#24=CARTESIAN_POINT('',(0.,10.,0.));\n#25=DIRECTION('',(1.,0.,0.));\n#26=DIRECTION('',(0.,1.,0.));\n#27=AXIS2_PLACEMENT_3D('',#24,#25,#26);\n#28=CIRCLE('',#27,3.);\n";
    t += "#29=CIRCLE('',#4,13.);\n";
    t += "#30=EDGE_CURVE('',#12,#12,#23,.T.);\n#31=EDGE_CURVE('',#13,#13,#28,.T.);\n#32=EDGE_CURVE('',#12,#13,#29,.T.);\n";
    t += "#40=ORIENTED_EDGE('',*,*,#30,.T.);\n#41=ORIENTED_EDGE('',*,*,#32,.T.);\n#42=ORIENTED_EDGE('',*,*,#31,.F.);\n#43=ORIENTED_EDGE('',*,*,#32,.F.);\n";
    t += "#44=EDGE_LOOP('',(#40,#41,#42,#43));\n#45=FACE_OUTER_BOUND('',#44,.T.);\n#46=ADVANCED_FACE('',(#45),#5,.F.);\n";
    t += "#47=PLANE('',#22);\n#48=ORIENTED_EDGE('',*,*,#30,.F.);\n#49=EDGE_LOOP('',(#48));\n#50=FACE_OUTER_BOUND('',#49,.T.);\n#51=ADVANCED_FACE('',(#50),#47,.T.);\n";
    t += "#52=PLANE('',#27);\n#53=ORIENTED_EDGE('',*,*,#31,.T.);\n#54=EDGE_LOOP('',(#53));\n#55=FACE_OUTER_BOUND('',#54,.T.);\n#56=ADVANCED_FACE('',(#55),#52,.T.);\n";
    t += "#60=CLOSED_SHELL('',(#46,#51,#56));\n#61=MANIFOLD_SOLID_BREP('Pipe',#60);\n#62=SHAPE_REPRESENTATION('',(#61),$);\n";
    t += TAIL;
    let imp = step_import(&t).unwrap();
    let ib = &imp.bodies[0];
    assert_eq!(ib.file_faces, 3);
    assert_eq!(ib.body.face_count(), 4, "{:?}", imp.warnings);
    // The two halves of the tube face make a quarter torus: area π²·R·r.
    let curved: f64 = ib.body.faces(1e-3).unwrap().iter().filter(|f| f.plane_normal.is_none()).map(|f| f.area).sum();
    let want = std::f64::consts::PI.powi(2) * 30.0;
    assert!(rel(curved, want) < 2e-3, "{curved} vs {want}");
    let m = ib.body.display_mesh(ib.body.size() * 1e-3).unwrap();
    // Seams: the file's own seam (inside the tube face) and the split edge.
    assert!(m.seams.iter().filter(|s| **s).count() >= 2, "{:?}", m.seams);
}

/// Revolved faces of a mirrored tool keep their orientation through STEP (the writer used to
/// drop an inverted or mirrored revolution's sense).
#[test]
fn mirrored_revolved_faces_round_trip() {
    for mirror in [false, true] {
        let pl = Plane { origin: Vec3::new(20.0, 15.0, 0.0), ..Plane::XZ };
        let prof =
            Region2 { outer: Loop2::polygon(&[Vec2::new(0.0, 4.0), Vec2::new(3.0, 5.0), Vec2::new(3.0, 12.0), Vec2::new(0.0, 12.0)]), holes: vec![] };
        let mut tool = revolve(&pl, &[prof], Vec2::ZERO, Vec2::Y, std::f64::consts::TAU).unwrap().pop().unwrap();
        if mirror {
            tool =
                crate::transform_matrix(&tool, [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, -1.0, 0.0], [0.0, 0.0, 16.0, 1.0]]).unwrap();
        }
        let plate = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 10.0)).unwrap();
        round_trip(&boolean(&plate, &tool, BoolOp::Cut).unwrap().unwrap());
    }
}

/// Faces whose boundaries are nowhere near their surfaces, or wild B-spline surfaces, mesh
/// within bounded work (the mesher once looped for ever on such input) and are reported if
/// they cannot be meshed.
#[test]
fn hostile_boundaries_mesh_in_bounded_time() {
    let t0 = std::time::Instant::now();
    // A wild 12 × 12 B-spline patch (alternating ±50 mm) bounded by four straight edges that
    // lie far from it, plus a plane face bounded by a circle 500 mm off the plane. The bound
    // catches a hang, not a slow machine (it took ~50 s at 24 × 24 with the machine loaded).
    let n = 12;
    let mut t = String::from(HEAD);
    let mut rows = Vec::new();
    for i in 0..n {
        let mut row = Vec::new();
        for j in 0..n {
            let id = 1000 + i * n + j;
            let z = if (i * 7 + j * 13) % 2 == 0 { 50.0 } else { -50.0 };
            t += &format!("#{id}=CARTESIAN_POINT('',({}.,{}.,{z}));\n", i * 10, j * 10);
            row.push(format!("#{id}"));
        }
        rows.push(format!("({})", row.join(",")));
    }
    let knots = |m: usize| {
        let mut mult = vec!["4".to_string()];
        mult.extend(std::iter::repeat_n("1".to_string(), m - 4));
        mult.push("4".into());
        let ks: Vec<String> = (0..=(m - 3)).map(|k| format!("{k}.")).collect();
        (mult.join(","), ks.join(","))
    };
    let (mu, ku) = knots(n);
    t += &format!(
        "#10=B_SPLINE_SURFACE_WITH_KNOTS('',3,3,({}),.UNSPECIFIED.,.F.,.F.,.F.,({mu}),({mu}),({ku}),({ku}),.UNSPECIFIED.);\n",
        rows.join(",")
    );
    let corners = [(0, 0), (110, 0), (110, 110), (0, 110)];
    for (k, (x, y)) in corners.iter().enumerate() {
        t += &format!("#{}=CARTESIAN_POINT('',({x}.,{y}.,300.));\n#{}=VERTEX_POINT('',#{});\n", 20 + k, 30 + k, 20 + k);
    }
    t += "#40=DIRECTION('',(1.,0.,0.));\n#41=VECTOR('',#40,1.);\n";
    for k in 0..4 {
        t += &format!(
            "#{}=LINE('',#{},#41);\n#{}=EDGE_CURVE('',#{},#{},#{},.T.);\n#{}=ORIENTED_EDGE('',*,*,#{},.T.);\n",
            50 + k,
            20 + k,
            60 + k,
            30 + k,
            30 + (k + 1) % 4,
            50 + k,
            70 + k,
            60 + k
        );
    }
    t += "#80=EDGE_LOOP('',(#70,#71,#72,#73));\n#81=FACE_OUTER_BOUND('',#80,.T.);\n#82=ADVANCED_FACE('',(#81),#10,.T.);\n";
    t += "#90=CARTESIAN_POINT('',(0.,0.,0.));\n#91=DIRECTION('',(0.,0.,1.));\n#92=AXIS2_PLACEMENT_3D('',#90,#91,#40);\n#93=PLANE('',#92);\n";
    t += "#94=CARTESIAN_POINT('',(0.,0.,500.));\n#95=AXIS2_PLACEMENT_3D('',#94,#91,#40);\n#96=CIRCLE('',#95,30.);\n#97=CARTESIAN_POINT('',(30.,0.,500.));\n#98=VERTEX_POINT('',#97);\n";
    t += "#99=EDGE_CURVE('',#98,#98,#96,.T.);\n#100=ORIENTED_EDGE('',*,*,#99,.T.);\n#101=EDGE_LOOP('',(#100));\n#102=FACE_OUTER_BOUND('',#101,.T.);\n#103=ADVANCED_FACE('',(#102),#93,.T.);\n";
    t += "#110=OPEN_SHELL('',(#82,#103));\n#111=SHELL_BASED_SURFACE_MODEL('Wild',(#110));\n#112=SHAPE_REPRESENTATION('',(#111),$);\n";
    t += TAIL;
    let r = std::panic::catch_unwind(|| step_import(&t));
    assert!(r.is_ok(), "panicked");
    if let Ok(Ok(imp)) = r {
        for b in &imp.bodies {
            let _ = measure(&b.body);
        }
    }
    assert!(t0.elapsed().as_secs_f64() < 180.0, "took {:?}", t0.elapsed());
}

/// A cylinder written as a surface of revolution, oriented the ISO 10303-42 way (normal
/// ∂S/∂angle × ∂S/∂profile, outward here, so `same_sense` .T.). Fusion reads such faces this
/// way; reading them with truck's profile-first normal turned faces inside out.
#[test]
fn surface_of_revolution_sense_follows_iso() {
    let mut t = String::from(HEAD);
    t += "#1=CARTESIAN_POINT('',(0.,0.,0.));\n#2=DIRECTION('',(0.,0.,1.));\n#3=DIRECTION('',(1.,0.,0.));\n#4=AXIS1_PLACEMENT('',#1,#2);\n";
    t += "#5=CARTESIAN_POINT('',(5.,0.,0.));\n#6=VECTOR('',#2,1.);\n#7=LINE('',#5,#6);\n#8=SURFACE_OF_REVOLUTION('',#7,#4);\n";
    t += "#10=CARTESIAN_POINT('',(5.,0.,10.));\n#11=VERTEX_POINT('',#5);\n#12=VERTEX_POINT('',#10);\n";
    t += "#13=AXIS2_PLACEMENT_3D('',#1,#2,#3);\n#14=CIRCLE('',#13,5.);\n#15=CARTESIAN_POINT('',(0.,0.,10.));\n#16=AXIS2_PLACEMENT_3D('',#15,#2,#3);\n#17=CIRCLE('',#16,5.);\n";
    t += "#20=EDGE_CURVE('',#11,#11,#14,.T.);\n#21=EDGE_CURVE('',#12,#12,#17,.T.);\n#22=EDGE_CURVE('',#11,#12,#7,.T.);\n";
    t += "#30=ORIENTED_EDGE('',*,*,#20,.T.);\n#31=ORIENTED_EDGE('',*,*,#22,.T.);\n#32=ORIENTED_EDGE('',*,*,#21,.F.);\n#33=ORIENTED_EDGE('',*,*,#22,.F.);\n";
    t += "#34=EDGE_LOOP('',(#30,#31,#32,#33));\n#35=FACE_OUTER_BOUND('',#34,.T.);\n#36=ADVANCED_FACE('',(#35),#8,.T.);\n";
    t += "#40=PLANE('',#13);\n#41=ORIENTED_EDGE('',*,*,#20,.F.);\n#42=EDGE_LOOP('',(#41));\n#43=FACE_OUTER_BOUND('',#42,.T.);\n#44=ADVANCED_FACE('',(#43),#40,.F.);\n";
    t += "#50=PLANE('',#16);\n#51=ORIENTED_EDGE('',*,*,#21,.T.);\n#52=EDGE_LOOP('',(#51));\n#53=FACE_OUTER_BOUND('',#52,.T.);\n#54=ADVANCED_FACE('',(#53),#50,.T.);\n";
    t += "#60=CLOSED_SHELL('',(#36,#44,#54));\n#61=MANIFOLD_SOLID_BREP('Cyl',#60);\n#62=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#61),#70);\n";
    t += &context(70, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
    t += TAIL;
    assert_eq!(step_orientation_errors(&t).unwrap(), Vec::<String>::new());
    let imp = step_import(&t).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    let m = measure(&imp.bodies[0].body).unwrap();
    let want = std::f64::consts::PI * 25.0 * 10.0;
    assert!(rel(m.volume, want) < 1e-3, "{} vs {want}", m.volume);
    // The same face with the opposite sense is inside out.
    let flipped = t.replace("#36=ADVANCED_FACE('',(#35),#8,.T.);", "#36=ADVANCED_FACE('',(#35),#8,.F.);");
    let bad = step_import(&flipped).unwrap();
    let v = measure(&bad.bodies[0].body).map(|m| m.volume).unwrap_or(0.0);
    assert!(!bad.warnings.is_empty() || rel(v, want) > 1e-2, "{v}");
}

/// A tube revolved about a line it touches (a horn torus, as a tight sweep makes) is written as
/// one rational B-spline face whose loop passes the pole twice, with no zero-length edge there:
/// no TOROIDAL_SURFACE (it would be degenerate) and no surface of revolution.
#[test]
fn horn_torus_written_as_one_spline_face() {
    let prof = Region2 { outer: Loop2::circle(Vec2::new(5.0, 0.0), 5.0), holes: vec![] };
    let b = revolve(&Plane::XZ, &[prof], Vec2::ZERO, Vec2::Y, std::f64::consts::FRAC_PI_2).unwrap().pop().unwrap();
    let text = step_export(&[&b], "t").unwrap();
    assert!(!text.contains("TOROIDAL_SURFACE") && !text.contains("SURFACE_OF_REVOLUTION"));
    assert_eq!(text.matches("ADVANCED_FACE(").count(), 3, "{text}");
    let imp = round_trip(&b);
    let v = std::f64::consts::PI * 25.0 * std::f64::consts::TAU * 5.0 / 4.0;
    assert!(rel(measure(&imp.bodies[0].body).unwrap().volume, v) < 1e-3);
}

/// Booleans and blends build the same body every time, so an export is byte-identical from
/// run to run (truck used to emit a boolean's faces in hash order of their addresses). The
/// tee of oracle 35 with its rolling-ball fillet round the junction.
#[test]
fn tee_fillet_exports_byte_identical() {
    let main = cylinder(Vec3::new(-40.0, 0.0, 0.0), Vec3::X, 15.0, 80.0).unwrap();
    let branch = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 35.0).unwrap();
    let export = || {
        let tee = boolean(&main, &branch, BoolOp::Union).unwrap().unwrap();
        let f = fillet(&tee, &[Vec3::new(0.0, 10.0, 125f64.sqrt())], 3.0).unwrap();
        step_export(&[&f], "tee").unwrap()
    };
    let first = export();
    for _ in 0..2 {
        assert!(export() == first, "export differs between runs");
    }
}
