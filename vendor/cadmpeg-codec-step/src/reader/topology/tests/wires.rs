// SPDX-License-Identifier: Apache-2.0
//! STEP B-rep, shell, face, and wire tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::loss::StepLossCode;
use crate::test_support::exchange::equivalent_seam_source;
use crate::StepCodec;

#[test]
fn base_edges_without_curve_carriers_remain_topological_edges() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace("#19=EDGE_CURVE('',#6,#7,#57,.T.);", "#19=EDGE('',#6,#7);")
            .replace("#20=EDGE_CURVE('',#7,#8,#17,.T.);", "#20=EDGE('',#7,#8);")
            .replace("#21=EDGE_CURVE('',#8,#6,#18,.T.);", "#21=EDGE('',#8,#6);")
            .replace("#16=LINE('',#3,#13);", "#16=UNSUPPORTED_CURVE('',());")
            .replace("#17=LINE('',#4,#14);", "#17=UNSUPPORTED_CURVE('',());")
            .replace("#18=LINE('',#5,#15);", "#18=UNSUPPORTED_CURVE('',());")
            .replace(
                "#57=SURFACE_CURVE('',#16,(#56),.PCURVE_S1.);",
                "#57=UNSUPPORTED_CURVE('',());",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode base edges");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.edges.len(), 3);
    assert!(decoded
        .ir()
        .model
        .edges
        .iter()
        .all(|edge| edge.curve().is_none()));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::EdgeNoSurfaceOrCurveForPcurve.kind()
            && loss
                .message
                .contains("edge #19 has no decoded surface or curve carrier")
    }));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::DecodeWarning.kind()
            && loss
                .message
                .contains("STEP edge #19 has no 3D curve carrier")
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn unresolved_vertex_point_does_not_enter_a_topology_draft() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#3=CARTESIAN_POINT('',(0.,0.,0.));",
                "#3=UNSUPPORTED_POINT('',());",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode missing vertex point carrier");

    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded.ir().model.vertices.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| loss
        .message
        .contains("VERTEX_POINT #6 has unresolved point carrier #3")));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn shell_wire_edge_applies_edge_and_occurrence_sense() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
                "#31=SHELL_BASED_WIREFRAME_MODEL('',(#33));",
            )
            .replace(
                "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
                "#33=WIRE_SHELL('',(#25));",
            )
            .replace(
                "#19=EDGE_CURVE('',#6,#7,#57,.T.);",
                "#19=EDGE_CURVE('',#6,#7,#57,.F.);",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode oriented wire edge");

    let first = decoded
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| edge.id.as_str().contains("19-wire-31-33-22-0"))
        .expect("first wire edge");
    assert_eq!(first.start.as_str(), "step:data:vertex#7-wire-31-shell-33");
    assert_eq!(first.end.as_str(), "step:data:vertex#6-wire-31-shell-33");
}

#[test]
fn disconnected_edge_loop_is_not_committed() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#19=EDGE_CURVE('',#6,#7,#57,.T.);",
                "#19=EDGE_CURVE('',#6,#8,#57,.T.);",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode disconnected edge loop");

    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.as_str() == "step:data:shell_based_surface_model#31"));
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("conflicts with decoded topology")));
}

#[test]
fn single_edge_loop_must_close_at_its_endpoint() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#25=EDGE_LOOP('',(#22,#23,#24));",
                "#25=EDGE_LOOP('',(#22));",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode open single-edge loop");

    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| loss
        .message
        .contains("STEP topology root #31 rejected: edge loop continuity #25")));
}

#[test]
fn seam_edge_preserves_its_explicit_pcurve_reference() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#22=ORIENTED_EDGE('',*,*,#19,.T.);",
                "#22=SEAM_EDGE('',*,*,#19,.T.,#56);",
            )
            .replace(
                "#57=SURFACE_CURVE('',#16,(#56),.PCURVE_S1.);",
                "#57=SEAM_CURVE('',#16,(#56),.PCURVE_S1.);",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode seam edge");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert!(decoded.ir().model.coedges.iter().any(|coedge| {
        coedge
            .pcurves
            .iter()
            .any(|use_| use_.pcurve.as_str() == "step:data:pcurve#56")
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn seam_edge_does_not_guess_an_unlisted_pcurve_reference() {
    let source = equivalent_seam_source()
        .replace(
            "#22=ORIENTED_EDGE('',*,*,#19,.T.);",
            "#22=SEAM_EDGE('',*,*,#19,.T.,#75);",
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#72=CARTESIAN_POINT('',(0.,0.));\n#73=LINE('',#72,#53);\n#74=DEFINITIONAL_REPRESENTATION('',(#73),#50);\n#75=PCURVE('',#28,#74);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode seam edge with an unlisted pcurve");

    assert!(decoded.ir().model.coedges.iter().all(|coedge| {
        coedge
            .pcurves
            .iter()
            .all(|use_| use_.pcurve.as_str() != "step:data:pcurve#75")
    }));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::SeamEdgePcurveUnresolved.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Warning
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn seam_edge_rejects_an_explicit_pcurve_outside_its_curve() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#22=ORIENTED_EDGE('',*,*,#19,.T.);",
            "#22=SEAM_EDGE('',*,*,#19,.T.,#75);",
        )
        .replace(
            "#57=SURFACE_CURVE('',#16,(#56),.PCURVE_S1.);",
            "#57=SEAM_CURVE('',#16,(#56),.PCURVE_S1.);",
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#72=CARTESIAN_POINT('',(0.,0.));\n#73=LINE('',#72,#53);\n#74=DEFINITIONAL_REPRESENTATION('',(#73),#50);\n#75=PCURVE('',#28,#74);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode seam edge with an unlisted pcurve");

    assert!(decoded.ir().model.coedges.iter().all(|coedge| {
        coedge
            .pcurves
            .iter()
            .all(|use_| use_.pcurve.as_str() != "step:data:pcurve#75")
    }));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::SeamEdgePcurveUnresolved.kind()
            && loss.message.contains("SEAM_EDGE #22")
            && loss.message.contains("belongs to its edge curve")
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn surface_curve_without_a_basis_keeps_a_curve_less_edge_and_reports_loss() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace("#16=LINE('',#3,#13);", "#16=UNSUPPORTED_CURVE('',());")
            .replace(
                "#57=SURFACE_CURVE('',#16,(#56),.PCURVE_S1.);",
                "#57=SURFACE_CURVE('',*,(#56),.PCURVE_S1.);",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode surface curve without basis");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert!(decoded
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| edge.id.as_str() == "step:data:edge#19")
        .is_some_and(|edge| edge.curve().is_none()));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("STEP edge curve #19: surface-curve #57 has no resolvable basis")
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn subedge_inherits_parent_edge_geometry_without_losing_topology() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#19=EDGE_CURVE('',#6,#7,#57,.T.);",
                "#19=(EDGE('',#6,#7) SUBEDGE('',#58));",
            )
            .replace(
                "#57=SURFACE_CURVE('',#16,(#56),.PCURVE_S1.);",
                "#57=SURFACE_CURVE('',#18,(#56),.PCURVE_S1.);\n#58=EDGE_CURVE('',#6,#7,#18,.F.);",
            )
            .replace(
                "#20=EDGE_CURVE('',#7,#8,#17,.T.);",
                "#20=EDGE_CURVE('',#7,#8,#16,.T.);",
            )
            .replace(
                "#21=EDGE_CURVE('',#8,#6,#18,.T.);",
                "#21=EDGE_CURVE('',#8,#6,#17,.T.);",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode subedge");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert!(decoded.ir().model.edges.iter().any(|edge| {
        edge.id.as_str() == "step:data:edge#19"
            && edge
                .curve()
                .is_some_and(|curve| curve.as_str() == "step:data:curve#18")
    }));
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .all(|record| record.id.as_str() != "step:data:subedge#19"));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn shell_based_wireframe_model_owns_wire_shell_edges() {
    use cadmpeg_ir::topology::BodyKind;

    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
                "#31=SHELL_BASED_WIREFRAME_MODEL('',(#33));",
            )
            .replace(
                "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
                "#33=WIRE_SHELL('',(#25));",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode shell-based wireframe model");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.bodies[0].kind, BodyKind::Wire);
    assert_eq!(decoded.ir().model.edges.len(), 3);
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn shell_based_wireframe_model_retains_vertex_shells() {
    use cadmpeg_ir::topology::BodyKind;

    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
                "#31=SHELL_BASED_WIREFRAME_MODEL('',(#33));",
            )
            .replace(
                "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
                "#33=VERTEX_SHELL('',#34);\n#34=VERTEX_LOOP('',#6);",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode vertex shell");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.bodies[0].kind, BodyKind::Wire);
    assert_eq!(decoded.ir().model.shells[0].free_vertices().len(), 1);
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn connected_edge_sub_set_is_accepted_as_a_wire_boundary() {
    use cadmpeg_ir::topology::BodyKind;

    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=EDGE_BASED_WIREFRAME_MODEL('',(#33));",
        )
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#33=CONNECTED_EDGE_SUB_SET('',(#19,#20,#21),#34);\n#34=CONNECTED_EDGE_SET('',(#19,#20,#21));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode connected edge sub set");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.bodies[0].kind, BodyKind::Wire);
    assert_eq!(decoded.ir().model.edges.len(), 3);
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("has no resolvable parent")));
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.as_str() == "step:data:connected_edge_set#34"));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn connected_edge_sub_set_keeps_topology_when_parent_is_invalid() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
                "#31=EDGE_BASED_WIREFRAME_MODEL('',(#33));",
            )
            .replace(
                "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
                "#33=CONNECTED_EDGE_SUB_SET('',(#19,#20,#21),#34);\n#34=UNSUPPORTED_SET('',());",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode subset with invalid parent");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("parent #34 does not resolve")));
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.as_str() == "step:data:connected_edge_sub_set#33"));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn connected_edge_set_resolves_direct_oriented_and_seam_members() {
    use cadmpeg_ir::topology::BodyKind;

    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=EDGE_BASED_WIREFRAME_MODEL('',(#70));",
        )
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#70=CONNECTED_EDGE_SET('',(#71,#72,#73));\n#71=ORIENTED_EDGE('',*,*,#19,.F.);\n#72=SEAM_EDGE('',*,*,#20,.T.,#56);\n#73=ORIENTED_EDGE('',*,*,#21,.T.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode direct oriented and seam edge members");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.bodies[0].kind, BodyKind::Wire);
    assert_eq!(decoded.ir().model.edges.len(), 3);
    let reversed = decoded
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| edge.id.as_str().starts_with("step:data:edge#71-"))
        .expect("oriented edge carrier");
    assert!(reversed.start.as_str().contains("vertex#7"));
    assert!(reversed.end.as_str().contains("vertex#6"));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_edge_and_oriented_edge_instances_use_named_attributes() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#19=EDGE_CURVE('',#6,#7,#57,.T.);",
                "#19=(EDGE('',#6,#7) EDGE_CURVE('',#57,.T.));",
            )
            .replace(
                "#22=ORIENTED_EDGE('',*,*,#19,.T.);",
                "#22=(EDGE('',*,*) ORIENTED_EDGE('',#19,.T.));",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex edge instances");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.edges.len(), 3);
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_vertex_point_instances_retain_their_point_carriers() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#6=VERTEX_POINT('',#3);",
                "#6=(VERTEX('',*) VERTEX_POINT('',#3));",
            )
            .replace(
                "#7=VERTEX_POINT('',#4);",
                "#7=(VERTEX('',*) VERTEX_POINT('',#4));",
            )
            .replace(
                "#8=VERTEX_POINT('',#5);",
                "#8=(VERTEX('',*) VERTEX_POINT('',#5));",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex vertex points");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.vertices.len(), 3);
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn shared_edge_wire_model_marks_every_representation_typed() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=EDGE_BASED_WIREFRAME_MODEL('',(#70));\n#70=CONNECTED_EDGE_SET('',(#19,#20,#21));\n#71=MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#31),#2);",
        )
        .replace("#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);", "#33=OPEN_SHELL('',(#29));");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode shared wire model representations");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert!(!decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| {
            record.id.as_str() == "step:data:manifold_surface_shape_representation#71"
        }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_representation_items_reach_edge_based_wire_models() {
    use cadmpeg_ir::topology::BodyKind;

    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=EDGE_BASED_WIREFRAME_MODEL('',(#70));\n#70=CONNECTED_EDGE_SET('',(#19,#20,#21));\n#71=(MANIFOLD_SURFACE_SHAPE_REPRESENTATION() REPRESENTATION('',(#31),#2) SHAPE_REPRESENTATION());",
        )
        .replace("#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);", "#33=OPEN_SHELL('',(#29));");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex wire representation");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.bodies[0].kind, BodyKind::Wire);
    assert_eq!(decoded.ir().model.edges.len(), 3);
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}
