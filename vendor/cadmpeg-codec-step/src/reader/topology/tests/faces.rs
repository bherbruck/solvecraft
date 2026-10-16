// SPDX-License-Identifier: Apache-2.0
//! STEP B-rep, shell, face, and wire tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use crate::ids::kind;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point3, Vector3};

use crate::export::write_step;
use crate::ids;
use crate::loss::StepLossCode;
use crate::test_support::exchange::export;
use crate::{StepCodec, StepSchema, StepWriteOptions};

#[test]
fn base_face_with_polygon_loop_gets_an_inferred_plane() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#25=EDGE_LOOP('',(#22,#23,#24));",
                "#25=POLY_LOOP('',(#3,#4,#5));",
            )
            .replace(
                "#29=ADVANCED_FACE('',(#26),#28,.T.);",
                "#29=FACE('',(#26));",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode base face");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    let surface = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#implicit-face-29")
        .expect("implicit face plane");
    let Some(SolvedSurfaceGeometry::Plane(plane_surface)) = surface.geometry.solved() else {
        panic!("implicit face did not produce a plane");
    };
    let origin = plane_surface.origin();
    let normal = plane_surface.frame().axis().as_raw();
    let u_axis = plane_surface.frame().reference().as_raw();
    assert_eq!(*normal, Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(*origin, Point3::new(10.0 / 3.0, 10.0 / 3.0, 0.0));
    assert_eq!(*u_axis, Vector3::new(1.0, 0.0, 0.0));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn implicit_face_plane_uses_poly_loop_orientation_and_rejects_non_planar_points() {
    let source = String::from_utf8(include_bytes!("data/tp06_implicit_face_plane.p21").to_vec())
        .expect("fixture is UTF-8");
    let base = StepCodec::default()
        .decode(&mut Cursor::new(source.clone()), &DecodeOptions::default())
        .expect("decode implicit face witness");
    let base_surface = base
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#implicit-face-8")
        .expect("base implicit plane");
    let Some(SolvedSurfaceGeometry::Plane(plane_surface)) = base_surface.geometry.solved() else {
        panic!("base face did not produce a plane");
    };
    let origin = plane_surface.origin().get();
    let normal = *plane_surface.frame().axis().as_raw();
    assert_eq!(normal, Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(origin, Point3::new(2.0, 1.5, 0.0));

    let rotated = source.replace(
        "#6=POLY_LOOP('outer',(#3,#4,#5,#11));",
        "#6=POLY_LOOP('outer',(#4,#5,#11,#3));",
    );
    let rotated = StepCodec::default()
        .decode(&mut Cursor::new(rotated), &DecodeOptions::default())
        .expect("decode rotated implicit face witness");
    let rotated_surface = rotated
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#implicit-face-8")
        .expect("rotated implicit plane");
    assert_eq!(rotated_surface.geometry, base_surface.geometry);

    let reversed = source.replace(
        "#7=FACE_OUTER_BOUND('outer',#6,.T.);",
        "#7=FACE_OUTER_BOUND('outer',#6,.F.);",
    );
    let reversed = StepCodec::default()
        .decode(&mut Cursor::new(reversed), &DecodeOptions::default())
        .expect("decode reversed implicit face witness");
    let reversed_surface = reversed
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#implicit-face-8")
        .expect("reversed implicit plane");
    let Some(SolvedSurfaceGeometry::Plane(plane_surface)) = reversed_surface.geometry.solved()
    else {
        panic!("reversed face did not produce a plane");
    };
    let normal = *plane_surface.frame().axis().as_raw();
    assert_eq!(normal, Vector3::new(0.0, 0.0, -1.0));

    let non_planar = source.replace(
        "#5=CARTESIAN_POINT('c',(4.,3.,0.));",
        "#5=CARTESIAN_POINT('c',(4.,3.,1.));",
    );
    let non_planar = StepCodec::default()
        .decode(&mut Cursor::new(non_planar), &DecodeOptions::default())
        .expect("decode non-planar implicit face witness");
    assert!(non_planar.ir().model.bodies.is_empty());
    assert!(!non_planar
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id.as_str() == "step:data:surface#implicit-face-8"));
    assert!(non_planar.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TopologyRootRejected.kind()
            && loss.message.contains("implicit face plane")
    }));
}

#[test]
fn complex_face_bound_partials_keep_attributes_when_reordered() {
    let source = String::from_utf8(include_bytes!("data/tp08_face_bound_partial.p21").to_vec())
        .expect("fixture is UTF-8");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source.clone()), &DecodeOptions::default())
        .expect("decode complex face-bound witness");
    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert_eq!(decoded.ir().model.loops.len(), 1);
    assert_eq!(
        decoded
            .ir()
            .model
            .faces
            .iter()
            .find(|face| face.id == decoded.ir().model.loops[0].face)
            .map(|face| face.loop_role(&decoded.ir().model.loops[0].id))
            .unwrap_or_default(),
        cadmpeg_ir::topology::LoopBoundaryRole::Outer
    );
    assert!(decoded.ir().model.surfaces.iter().any(|surface| {
        surface.id.as_str() == "step:data:surface#implicit-face-8"
            && matches!(
                surface.geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
            )
    }));

    let reordered = source.replace(
        "#7=(FACE_BOUND('outer',#6,.T.) FACE_OUTER_BOUND());",
        "#7=(FACE_OUTER_BOUND() FACE_BOUND('outer',#6,.T.));",
    );
    let reordered = StepCodec::default()
        .decode(&mut Cursor::new(reordered), &DecodeOptions::default())
        .expect("decode reordered complex face-bound witness");
    assert!(reordered
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::ParseNoncanonicalSyntax.kind()));
    assert_eq!(reordered.ir().model.bodies.len(), 1);
    assert_eq!(reordered.ir().model.loops.len(), 1);
    assert_eq!(
        reordered
            .ir()
            .model
            .faces
            .iter()
            .find(|face| face.id == reordered.ir().model.loops[0].face)
            .map(|face| face.loop_role(&reordered.ir().model.loops[0].id))
            .unwrap_or_default(),
        cadmpeg_ir::topology::LoopBoundaryRole::Outer
    );
    assert!(reordered.ir().model.surfaces.iter().any(|surface| {
        surface.id.as_str() == "step:data:surface#implicit-face-8"
            && matches!(
                surface.geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
            )
    }));
    let validation =
        cadmpeg_ir::validate_neutral(reordered.ir(), reordered.report().losses.clone())
            .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn edge_loop_without_explicit_surface_rejects_implicit_plane() {
    let source = String::from_utf8(include_bytes!("data/tp06_edge_loop_base_face.p21").to_vec())
        .expect("fixture is UTF-8");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode edge-loop base-face witness");

    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded
        .ir()
        .model
        .surfaces
        .iter()
        .all(|surface| surface.id.as_str() != "step:data:surface#implicit-face-26"));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TopologyRootRejected.kind()
            && loss.message.contains("implicit face plane")
    }));
}

#[test]
fn non_planar_base_face_is_rejected_without_an_inferred_surface() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#25=EDGE_LOOP('',(#22,#23,#24));",
                "#25=POLY_LOOP('',(#3,#4,#5));",
            )
            .replace(
                "#29=ADVANCED_FACE('',(#26),#28,.T.);",
                "#29=FACE('',(#26));",
            )
            .replace(
                "#25=POLY_LOOP('',(#3,#4,#5));",
                "#70=CARTESIAN_POINT('',(5.,5.,1.));\n#25=POLY_LOOP('',(#3,#4,#5,#70));",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode non-planar base face");

    assert!(decoded.ir().model.bodies.is_empty());
    assert!(!decoded
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id.as_str() == "step:data:surface#implicit-face-29"));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TopologyRootRejected.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Error
    }));
}

#[test]
fn complex_outer_face_bound_uses_inherited_attributes() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#26=FACE_OUTER_BOUND('',#25,.T.);",
                "#26=(FACE_BOUND('',#25,.T.) FACE_OUTER_BOUND());",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex face bound");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    let surface = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#28")
        .expect("explicit face plane");
    assert!(matches!(
        surface.geometry,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
    ));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn implicit_face_plane_uses_all_coplanar_poly_loops() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#25=EDGE_LOOP('',(#22,#23,#24));",
            "#25=POLY_LOOP('',(#3,#4,#5));",
        )
        .replace(
            "#29=ADVANCED_FACE('',(#26),#28,.T.);",
            "#70=CARTESIAN_POINT('',(2.,2.,0.));\n#71=CARTESIAN_POINT('',(2.,3.,0.));\n#72=CARTESIAN_POINT('',(3.,2.,0.));\n#73=POLY_LOOP('',(#70,#71,#72));\n#74=FACE_BOUND('',#73,.F.);\n#29=FACE('',(#74,#26));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode base face with a hole");

    let surface = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#implicit-face-29")
        .expect("implicit face plane");
    let Some(SolvedSurfaceGeometry::Plane(plane_surface)) = surface.geometry.solved() else {
        panic!("implicit face did not produce a plane");
    };
    let origin = plane_surface.origin().get();
    let normal = *plane_surface.frame().axis().as_raw();
    assert_eq!(normal, Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(origin, Point3::new(17.0 / 6.0, 17.0 / 6.0, 0.0));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn implicit_face_plane_is_independent_of_coplanar_bound_set_order() {
    let first = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("data/tp06_coplanar_bounds_first.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode coplanar first-order witness");
    let reordered = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("data/tp06_coplanar_bounds_reordered.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode coplanar reordered witness");

    let first_surface = first
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#implicit-face-14")
        .expect("first implicit face plane");
    let reordered_surface = reordered
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#implicit-face-14")
        .expect("reordered implicit face plane");
    assert_eq!(first_surface.geometry, reordered_surface.geometry);
    assert_eq!(first.ir().model.bodies.len(), 1);
    assert_eq!(reordered.ir().model.bodies.len(), 1);
    assert!(
        cadmpeg_ir::validate_neutral(first.ir(), first.report().losses.clone())
            .expect("resource allocation did not fail")
            .is_ok()
    );
    assert!(
        cadmpeg_ir::validate_neutral(reordered.ir(), reordered.report().losses.clone())
            .expect("resource allocation did not fail")
            .is_ok()
    );
}

#[test]
fn implicit_face_plane_rejects_non_coplanar_poly_loop_bounds_in_any_order() {
    use std::collections::BTreeSet;

    for input in [
        include_bytes!("data/tp06_non_coplanar_bounds_first.p21").as_slice(),
        include_bytes!("data/tp06_non_coplanar_bounds_reordered.p21").as_slice(),
    ] {
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(input), &DecodeOptions::default())
            .expect("decode non-coplanar witness");
        assert!(decoded.ir().model.bodies.is_empty());
        assert!(decoded.ir().model.faces.is_empty());
        assert!(decoded.ir().model.surfaces.is_empty());
        assert!(decoded.report().losses.iter().any(|loss| {
            loss.code == StepLossCode::TopologyRootRejected.kind()
                && loss.message.contains("implicit face plane")
        }));
        let unknown_ids = decoded
            .ir()
            .native_unknowns("step")
            .expect("STEP native namespace")
            .iter()
            .map(|record| record.id.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            unknown_ids,
            BTreeSet::from([
                "step:data:face#14".to_string(),
                "step:data:face_bound#8".to_string(),
                "step:data:face_bound#13".to_string(),
                "step:data:open_shell#15".to_string(),
                "step:data:poly_loop#7".to_string(),
                "step:data:poly_loop#12".to_string(),
                "step:data:shell_based_surface_model#16".to_string(),
            ])
        );
    }
}

#[test]
fn nearly_collinear_implicit_face_is_rejected_without_a_fabricated_plane() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#5=CARTESIAN_POINT('',(0.,10.,0.));",
                "#5=CARTESIAN_POINT('',(20.,0.0000000000002,0.));",
            )
            .replace(
                "#29=ADVANCED_FACE('',(#26),#28,.T.);",
                "#29=FACE('',(#26));",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode nearly collinear base face");

    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded.ir().model.surfaces.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TopologyRootRejected.kind()
            && loss.message.contains("implicit face plane")
    }));
}

#[test]
fn implicit_face_plane_keeps_base_orientation_across_oriented_face() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#25=EDGE_LOOP('',(#22,#23,#24));",
                "#25=POLY_LOOP('',(#3,#4,#5));",
            )
            .replace(
                "#29=ADVANCED_FACE('',(#26),#28,.T.);",
                "#29=FACE('',(#26));",
            )
            .replace("#30=OPEN_SHELL('',(#29));", "#30=OPEN_SHELL('',(#34));")
            .replace(
                "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
                "#31=SHELL_BASED_SURFACE_MODEL('',(#33));\n#34=ORIENTED_FACE('',#29,.F.);",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode oriented base face");

    let surface = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#implicit-face-34")
        .expect("implicit face plane");
    let Some(SolvedSurfaceGeometry::Plane(plane_surface)) = surface.geometry.solved() else {
        panic!("implicit face did not produce a plane");
    };
    let normal = *plane_surface.frame().axis().as_raw();
    assert_eq!(normal, Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(
        decoded.ir().model.faces[0].sense,
        cadmpeg_ir::topology::Sense::Forward
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn oriented_face_subtype_composes_face_orientation() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace("#30=OPEN_SHELL('',(#29));", "#30=OPEN_SHELL('',(#34));")
            .replace(
                "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
                "#31=SHELL_BASED_SURFACE_MODEL('',(#33));\n#34=ORIENTED_FACE('',#29,.F.);",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode oriented face");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert!(decoded
        .ir()
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.sense == cadmpeg_ir::topology::Sense::Reversed));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn nested_oriented_faces_compose_back_to_the_base_orientation() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#30=OPEN_SHELL('',(#29));", "#30=OPEN_SHELL('',(#35));")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));\n#34=ORIENTED_FACE('',#29,.F.);\n#35=ORIENTED_FACE('',#34,.F.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode nested oriented faces");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert_eq!(
        decoded.ir().model.faces[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
    assert!(decoded
        .ir()
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.sense == cadmpeg_ir::topology::Sense::Forward));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn subface_subtype_reuses_parent_surface_and_own_bounds() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace("#30=OPEN_SHELL('',(#29));", "#30=OPEN_SHELL('',(#34));")
            .replace(
                "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
                "#31=SHELL_BASED_SURFACE_MODEL('',(#33));\n#34=SUBFACE('',(#26),#29);",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode subface");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_advanced_face_uses_its_explicit_surface_carrier() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace("#28=PLANE('',#27);", "#28=CYLINDRICAL_SURFACE('',#27,5.);")
            .replace(
                "#29=ADVANCED_FACE('',(#26),#28,.T.);",
                "#29=(FACE('',(#26)) FACE_SURFACE('',#28,.T.) ADVANCED_FACE());",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex advanced face");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert!(decoded.ir().model.surfaces.iter().any(|surface| {
        surface.id.as_str() == "step:data:surface#28"
            && matches!(
                surface.geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(_))
            )
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn connected_face_sub_set_validates_and_uses_its_own_members() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
                "#31=FACE_BASED_SURFACE_MODEL('',(#34));",
            )
            .replace(
                "#30=OPEN_SHELL('',(#29));",
                "#30=CONNECTED_FACE_SET('',(#29));",
            )
            .replace(
                "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
                "#34=CONNECTED_FACE_SUB_SET('',(#29),#30);",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode connected face subset");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("CONNECTED_FACE_SUB_SET #34")));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
pub(crate) fn face_outer_bound_is_canonicalized_ahead_of_inner_bounds() {
    use cadmpeg_ir::ids::LoopId;
    use cadmpeg_ir::topology::Loop;

    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let face = ir.model.faces[0].id.clone();
    let vertex = ir.model.vertices[0].id.clone();
    let inner = LoopId::mint("zzzz:test:loop#inner").expect("identity grammar");
    ir.model.loops.push(Loop {
        id: inner.clone(),
        face: face.clone(),
        boundary: cadmpeg_ir::topology::LoopBoundary::Vertex {
            vertex,
            pcurves: Vec::new(),
        },
    });
    let outer = ir.model.faces[0].loops.iter().next().unwrap().clone();
    ir.model.faces[0].loops = cadmpeg_ir::topology::FaceLoops::classified(outer, vec![inner]);
    let output = export(&ir);
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(output.as_bytes(), crate::parse::parse_inner)
            .unwrap();
    assert!(diagnostics.is_empty());
    let (face_step, outer_bound, inner_bound, outer_loop) = exchange
        .records()
        .iter()
        .find_map(|(&face_step, record)| {
            let partial = record.partials.first();
            if partial.name != "ADVANCED_FACE" {
                return None;
            }
            let crate::parse::Value::List(bounds) = partial.parameters.get(1)? else {
                return None;
            };
            if bounds.len() != 2 {
                return None;
            }
            let crate::parse::Value::Reference(first) = bounds[0] else {
                return None;
            };
            let crate::parse::Value::Reference(second) = bounds[1] else {
                return None;
            };
            let first_record = exchange.records().get(&first)?.partials.first();
            let second_record = exchange.records().get(&second)?.partials.first();
            let (outer, inner) = if first_record.name == "FACE_OUTER_BOUND" {
                (first, second)
            } else if second_record.name == "FACE_OUTER_BOUND" {
                (second, first)
            } else {
                return None;
            };
            let crate::parse::Value::Reference(outer_loop) =
                exchange.records().get(&outer)?.partials[0]
                    .parameters
                    .get(1)?
            else {
                return None;
            };
            Some((face_step, outer, inner, outer_loop))
        })
        .expect("face with outer and inner bounds");
    let ordered = format!("(#{outer_bound},#{inner_bound})");
    let reversed = format!("(#{inner_bound},#{outer_bound})");
    let reordered = output.replacen(&ordered, &reversed, 1);
    assert_ne!(reordered, output);
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(reordered), &DecodeOptions::default())
        .expect("decode reversed face bounds");
    let face = decoded
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.as_str() == ids::data(kind!("face"), face_step).as_str())
        .expect("decoded face");
    assert_eq!(
        face.loops
            .iter()
            .next()
            .expect("a decoded face states a loop")
            .as_str(),
        ids::data(
            kind!("loop"),
            cadmpeg_ir::ids::IdentityKey::from(outer_loop)
                .dash(crate::ids::key_word!("face"))
                .dash(face_step),
        )
        .as_str()
    );
}

#[test]
fn duplicate_face_outer_bound_witnesses_recover_unspecified_loops_in_any_order() {
    for (input, first_loop, second_loop) in [
        (
            include_bytes!("data/tp10_duplicate_outer_first.p21").as_slice(),
            6,
            8,
        ),
        (
            include_bytes!("data/tp10_duplicate_outer_reordered.p21").as_slice(),
            8,
            6,
        ),
    ] {
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(input), &DecodeOptions::default())
            .expect("decode duplicate outer-bound witness");
        assert!(decoded.report().losses.iter().any(|loss| {
            loss.code == StepLossCode::FaceMultipleOuterBounds.kind()
                && loss.message.contains("face #10")
                && loss.message.contains("without outer/inner classification")
        }));
        assert!(!decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == StepLossCode::TopologyRootRejected.kind()));
        assert_eq!(decoded.ir().model.faces.len(), 1);
        assert_eq!(decoded.ir().model.loops.len(), 2);
        assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
            .unwrap()
            .findings
            .iter()
            .all(|finding| finding.severity < cadmpeg_ir::report::Severity::Error));
        // Recovery keeps the original role claims as exact source records.
        let decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
        let unknowns = decoded
            .ir()
            .native
            .namespace("step")
            .unwrap()
            .arena_as::<cadmpeg_ir::NativeUnknownRecord>("unknowns")
            .unwrap();
        for step in [7, 9, 10] {
            let source = std::str::from_utf8(input).unwrap();
            let start = source.find(&format!("#{step}=")).unwrap();
            let end = start + source[start..].find(';').unwrap() + 1;
            let (id, retained) = decoded
                .source_fidelity()
                .retained_records()
                .iter()
                .find(|(_, record)| record.data() == Some(&input[start..end]))
                .expect("noncanonical topology source survives");
            assert_eq!(
                retained.offset(),
                cadmpeg_core::decode::u64_from_index(start)
            );
            assert!(!unknowns
                .iter()
                .find(|record| record.id == *id)
                .unwrap()
                .links
                .is_empty());
        }
        let face = decoded.ir().model.faces.first().expect("one face");
        let cadmpeg_ir::topology::FaceLoops::Unspecified { loops } = &face.loops else {
            panic!(
                "several outer bounds state no classification: {:?}",
                face.loops
            );
        };
        assert_eq!(
            loops
                .iter()
                .map(cadmpeg_ir::ids::LoopId::as_str)
                .collect::<Vec<_>>(),
            [
                ids::data(
                    kind!("loop"),
                    cadmpeg_ir::ids::IdentityKey::from(first_loop)
                        .dash(crate::ids::key_word!("face"))
                        .dash(10),
                )
                .as_str(),
                ids::data(
                    kind!("loop"),
                    cadmpeg_ir::ids::IdentityKey::from(second_loop)
                        .dash(crate::ids::key_word!("face"))
                        .dash(10),
                )
                .as_str(),
            ]
        );
    }
}

#[test]
fn failed_face_bounds_do_not_duplicate_the_shared_surface() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.faces[0].surface = ir.model.faces[1].surface.clone();
    ir.model.faces[0].loops = cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new());
    let output = export(&ir);
    // Five face-owned surfaces remain after sharing, and the displaced carrier
    // is retained once as standalone construction geometry.
    assert_eq!(output.matches("= PLANE(").count(), 6);
}

#[test]
fn advanced_face_name_transfers_through_inherited_representation_item() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#29=ADVANCED_FACE('',(#26),#28,.T.);",
                "#29=ADVANCED_FACE('named face',(#26),#28,.T.);",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode named face");
    assert_eq!(
        decoded.ir().model.faces[0].name.as_deref(),
        Some("named face")
    );

    let mut output = Vec::new();
    write_step(
        decoded.ir(),
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write named face");
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode written named face");
    assert_eq!(
        roundtrip.ir().model.faces[0].name.as_deref(),
        Some("named face")
    );
}

#[test]
fn face_name_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let source = export(&unit_cube().expect("unit cube fixture is admitted"));
    let named = source.replacen("ADVANCED_FACE('", "ADVANCED_FACE('budgeted face ", 1);
    assert_ne!(named, source, "STEP export contains an advanced face");
    let (exchange, _) =
        crate::test_support::with_service_context(named.as_bytes(), crate::parse::parse_inner)
            .expect("valid named-face exchange");
    let arena = DecodeArena::new();
    let refused = {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "step_string_text",
            |limit| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = limit;
                let (ctx, _) = DecodeContext::from_root_bytes(named.as_bytes(), &arena, &policy)
                    .expect("root fits retained policy");
                let mut ir = cadmpeg_ir::document::CadIr::empty();
                crate::reader::geometry::decode(&exchange, &mut ir, &ctx)?;
                let index = crate::reader::index::CarrierIndex::from_ir(&ir, &ctx)?;
                super::super::decode(&exchange, &mut ir, &index, &ctx)?;
                Ok::<(), CodecError>(())
            },
        );
        matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "step_string_text")
    };
    assert!(refused, "no retained limit refused the advanced face name");
}

#[test]
fn complex_advanced_face_name_uses_representation_item_partial() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#29=ADVANCED_FACE('',(#26),#28,.T.);",
            "#29=(FACE('',(#26)) FACE_SURFACE('',#28,.T.) ADVANCED_FACE() REPRESENTATION_ITEM('complex named face'));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex named face");
    assert_eq!(
        decoded.ir().model.faces[0].name.as_deref(),
        Some("complex named face")
    );
}

#[test]
fn unsupported_mandatory_carriers_preserve_topology_as_unknown() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace("#16=LINE('',#3,#13);", "#16=UNSUPPORTED_CURVE('',#3);")
            .replace("#28=PLANE('',#27);", "#28=UNSUPPORTED_SURFACE('',#27);");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode sheet with unknown mandatory carriers");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert_eq!(decoded.ir().model.edges.len(), 3);
    assert!(matches!(
        decoded
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id.as_str() == "step:data:curve#16")
            .map(|curve| &curve.geometry),
        Some(CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: Some(record) }))
            if record.as_str() == "step:data:unsupported_curve#16"
    ));
    assert!(matches!(
        decoded
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == "step:data:surface#28")
            .map(|surface| &surface.geometry),
        Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: Some(record) }))
            if record.as_str() == "step:data:unsupported_surface#28"
    ));
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("conflicts with decoded topology")));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn unsupported_surface_carrier_on_face_surface_preserves_topology_as_unknown() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace("#28=PLANE('',#27);", "#28=UNSUPPORTED_SURFACE('',#27);")
            .replace(
                "#29=ADVANCED_FACE('',(#26),#28,.T.);",
                "#29=FACE_SURFACE('',(#26),#28,.T.);",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode FACE_SURFACE with unknown carrier");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert!(matches!(
        decoded
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == "step:data:surface#28")
            .map(|surface| &surface.geometry),
        Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: Some(record) }))
            if record.as_str() == "step:data:unsupported_surface#28"
    ));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}
