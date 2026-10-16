// SPDX-License-Identifier: Apache-2.0
//! STEP B-rep, shell, face, and wire tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::io::Cursor;

use cadmpeg_core::decode::DecodeMode;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::CadIr;

use crate::export::Builder;
use crate::loss::StepLossCode;
use crate::test_support::exchange::export;
use crate::{StepCodec, StepSchema};

#[test]
fn disconnected_source_shell_is_partitioned_into_connected_ir_shells() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#30=OPEN_SHELL('',(#29));",
            "#30=OPEN_SHELL('',(#29,#92));",
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#70=CARTESIAN_POINT('',(20.,0.,0.));\n#71=CARTESIAN_POINT('',(30.,0.,0.));\n#72=CARTESIAN_POINT('',(20.,10.,0.));\n#73=VERTEX_POINT('',#70);\n#74=VERTEX_POINT('',#71);\n#75=VERTEX_POINT('',#72);\n#76=DIRECTION('',(0.,0.,1.));\n#77=DIRECTION('',(1.,0.,0.));\n#78=DIRECTION('',(-1.,1.,0.));\n#79=DIRECTION('',(0.,-1.,0.));\n#80=VECTOR('',#77,10.);\n#81=VECTOR('',#78,14.142135623730951);\n#82=VECTOR('',#79,10.);\n#83=LINE('',#70,#80);\n#84=LINE('',#71,#81);\n#85=LINE('',#72,#82);\n#86=EDGE_CURVE('',#73,#74,#83,.T.);\n#87=EDGE_CURVE('',#74,#75,#84,.T.);\n#88=EDGE_CURVE('',#75,#73,#85,.T.);\n#89=ORIENTED_EDGE('',*,*,#86,.T.);\n#90=ORIENTED_EDGE('',*,*,#87,.T.);\n#91=ORIENTED_EDGE('',*,*,#88,.T.);\n#93=EDGE_LOOP('',(#89,#90,#91));\n#94=FACE_OUTER_BOUND('',#93,.T.);\n#95=AXIS2_PLACEMENT_3D('',#70,#76,#77);\n#96=PLANE('',#95);\n#92=ADVANCED_FACE('',(#94),#96,.T.);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode disconnected source shell");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.regions.len(), 1);
    assert_eq!(decoded.ir().model.shells.len(), 2);
    assert_eq!(decoded.ir().model.faces.len(), 2);
    let source_loss = decoded
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == StepLossCode::ShellDisconnectedFaces.kind())
        .expect("source topology loss");
    assert!(source_loss.message.contains("OPEN_SHELL #30"));
    assert!(source_loss
        .message
        .contains("2 disconnected face components"));
    assert!(source_loss.provenance.is_some());
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn disconnected_brep_outer_shell_is_rejected_without_role_corruption() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#30=OPEN_SHELL('',(#29));",
            "#30=CLOSED_SHELL('',(#29,#92));",
        )
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=BREP_WITH_VOIDS('',#30,(#34));\n#34=CLOSED_SHELL('',(#29));",
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#70=CARTESIAN_POINT('',(20.,0.,0.));\n#71=CARTESIAN_POINT('',(30.,0.,0.));\n#72=CARTESIAN_POINT('',(20.,10.,0.));\n#73=VERTEX_POINT('',#70);\n#74=VERTEX_POINT('',#71);\n#75=VERTEX_POINT('',#72);\n#76=DIRECTION('',(0.,0.,1.));\n#77=DIRECTION('',(1.,0.,0.));\n#78=DIRECTION('',(-1.,1.,0.));\n#79=DIRECTION('',(0.,-1.,0.));\n#80=VECTOR('',#77,10.);\n#81=VECTOR('',#78,14.142135623730951);\n#82=VECTOR('',#79,10.);\n#83=LINE('',#70,#80);\n#84=LINE('',#71,#81);\n#85=LINE('',#72,#82);\n#86=EDGE_CURVE('',#73,#74,#83,.T.);\n#87=EDGE_CURVE('',#74,#75,#84,.T.);\n#88=EDGE_CURVE('',#75,#73,#85,.T.);\n#89=ORIENTED_EDGE('',*,*,#86,.T.);\n#90=ORIENTED_EDGE('',*,*,#87,.T.);\n#91=ORIENTED_EDGE('',*,*,#88,.T.);\n#93=EDGE_LOOP('',(#89,#90,#91));\n#94=FACE_OUTER_BOUND('',#93,.T.);\n#95=AXIS2_PLACEMENT_3D('',#70,#76,#77);\n#96=PLANE('',#95);\n#92=ADVANCED_FACE('',(#94),#96,.T.);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode disconnected BREP outer shell");

    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("STEP topology root #31 rejected: connected outer shell #30")
    }));
}

#[test]
fn brep_with_voids_keeps_outer_first_and_void_order_independent_of_the_set() {
    let source = include_bytes!("data/br02_outer_void_roles.p21");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode outer and void shell witness");

    let region = &decoded.ir().model.regions[0];
    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(region.shells.len(), 3);
    assert_eq!(region.shells[0].as_str(), "step:data:shell#30");
    assert_eq!(
        region.shells[1..]
            .iter()
            .map(cadmpeg_ir::ids::ShellId::as_str)
            .collect::<Vec<_>>(),
        ["step:data:shell#31", "step:data:shell#32"]
    );
    let face_sense = |document: &CadIr, shell_id: &cadmpeg_ir::ids::ShellId| {
        let shell = document
            .model
            .shells
            .iter()
            .find(|shell| &shell.id == shell_id)
            .expect("shell carrier");
        let face_id = shell.faces().first().expect("shell face");
        document
            .model
            .faces
            .iter()
            .find(|face| &face.id == face_id)
            .expect("face carrier")
            .sense
    };
    assert_eq!(
        face_sense(decoded.ir(), &region.shells[0]),
        cadmpeg_ir::topology::Sense::Forward
    );
    for shell_id in region.shells.iter().skip(1) {
        assert_eq!(
            face_sense(decoded.ir(), shell_id),
            cadmpeg_ir::topology::Sense::Reversed
        );
    }

    let reordered_source = String::from_utf8(source.to_vec())
        .expect("witness is UTF-8")
        .replace(
            "#50=BREP_WITH_VOIDS('outer and voids',#30,(#34,#33));",
            "#50=BREP_WITH_VOIDS('outer and voids',#30,(#33,#34));",
        );
    let reordered = StepCodec::default()
        .decode(
            &mut Cursor::new(reordered_source),
            &DecodeOptions::default(),
        )
        .expect("decode reordered outer and void shell witness");
    assert_eq!(reordered.ir().model.regions[0].shells, region.shells);

    for document in [decoded.ir(), reordered.ir()] {
        let validation = cadmpeg_ir::validate_neutral(document, Vec::new())
            .expect("resource allocation did not fail");
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn shared_source_face_gets_one_owner_scoped_face_per_shell() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
                "#31=SHELL_BASED_SURFACE_MODEL('',(#33,#34));",
            )
            .replace(
                "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
                "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);\n#34=OPEN_SHELL('',(#29));",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode shared face shells");

    assert_eq!(
        decoded.ir().model.bodies.len(),
        2,
        "{:#?}",
        decoded.report().losses
    );
    assert_eq!(decoded.ir().model.faces.len(), 2);
    assert_eq!(
        decoded
            .ir()
            .model
            .faces
            .iter()
            .map(|face| face.id.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        2
    );
    assert!(decoded
        .ir()
        .model
        .faces
        .iter()
        .all(|face| face.color.is_some()));
    assert_eq!(decoded.ir().model.presentation_layers[0].items.len(), 2);
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn brep_with_voids_scopes_edges_and_vertices_per_shell_after_shared_shell_use() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#30=OPEN_SHELL('',(#29));", "#30=CLOSED_SHELL('',(#29));")
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);\n#34=CLOSED_SHELL('',(#29));\n#70=BREP_WITH_VOIDS('',#30,(#34));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode scoped BREP with voids");

    assert!(decoded.ir().model.bodies.iter().any(|body| {
        body.id.as_str() == "step:data:body#70"
            && body.kind == cadmpeg_ir::topology::BodyKind::Solid
    }));
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("root #70 rejected")));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn first_brep_with_voids_scopes_all_shell_carriers() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace("#30=OPEN_SHELL('',(#29));", "#30=CLOSED_SHELL('',(#29));")
            .replace(
                "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
                "#31=BREP_WITH_VOIDS('',#30,(#34));\n#34=CLOSED_SHELL('',(#29));",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode first BREP with voids");

    assert!(decoded
        .ir()
        .model
        .bodies
        .iter()
        .any(|body| body.id.as_str() == "step:data:body#31"));
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("root #31 rejected")));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

fn oriented_closed_shell_source(derived_slot: bool) -> String {
    let oriented_shell = if derived_slot {
        "#33=ORIENTED_CLOSED_SHELL('',*,#30,.F.);"
    } else {
        "#33=ORIENTED_CLOSED_SHELL('',#30,.F.);"
    };
    String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#30=OPEN_SHELL('',(#29));", "#30=CLOSED_SHELL('',(#29));")
        .replace("#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);", oriented_shell)
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=BREP_WITH_VOIDS('',#33,());",
        )
}

#[test]
fn oriented_shell_reads_the_derived_cfs_faces_slot() {
    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(oriented_closed_shell_source(true)),
            &DecodeOptions::default(),
        )
        .expect("decode specification-form oriented closed shell");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert!(decoded
        .ir()
        .model
        .bodies
        .iter()
        .any(|body| body.kind == cadmpeg_ir::topology::BodyKind::Solid));
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::OrientedShellOmitsCfsFaces.kind()));
}

#[test]
fn oriented_shell_without_the_derived_slot_is_read_and_reported() {
    let source = oriented_closed_shell_source(false);
    let record_offset = source
        .find("#33=ORIENTED_CLOSED_SHELL")
        .expect("oriented shell record");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode noncanonical oriented closed shell");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    let losses = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == StepLossCode::OrientedShellOmitsCfsFaces.kind())
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("ORIENTED_CLOSED_SHELL #33"));
    assert_eq!(
        losses[0]
            .provenance
            .as_ref()
            .expect("oriented shell provenance")
            .offset,
        cadmpeg_core::decode::u64_from_index(record_offset)
    );
}

#[test]
fn strict_decode_rejects_an_oriented_shell_missing_its_derived_slot() {
    let mut options = DecodeOptions::default();
    options.policy.mode = DecodeMode::Strict;
    let error = StepCodec::default()
        .decode(
            &mut Cursor::new(oriented_closed_shell_source(false)),
            &options,
        )
        .expect_err("strict mode rejects a noncanonical oriented shell");

    assert!(matches!(
        error,
        cadmpeg_ir::codec::DecodeFailure::StrictRejected { .. }
    ));
}

#[test]
fn failed_void_shell_does_not_commit_the_outer_brep() {
    let source = String::from_utf8(
        include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
        "#31=BREP_WITH_VOIDS('',#30,(#34));",
    )
    .replace(
        "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
        "#33=OPEN_SHELL('',(#29));\n#34=OPEN_SHELL('',(#99));\n#99=UNSUPPORTED_FACE('',());",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode BREP with invalid void shell");

    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.as_str() == "step:data:brep_with_voids#31"));
    assert!(decoded.report().losses.iter().any(|loss| loss
        .message
        .contains("STEP topology root #31 rejected: face carrier #99")));
}

#[test]
fn complex_oriented_open_shell_preserves_shell_sense() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
                "#33=(OPEN_SHELL('',(#29)) ORIENTED_OPEN_SHELL('',*,#30,.F.));",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex oriented shell");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(
        decoded.ir().model.faces[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn aliased_topology_root_reuses_the_committed_body_identity() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#70=SHELL_BASED_SURFACE_MODEL('',(#33));\n#71=MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#70),#2);\n#72=PRODUCT('P','alias part','',());\n#73=PRODUCT_DEFINITION_FORMATION('','',#72);\n#74=APPLICATION_CONTEXT('mechanical design');\n#75=PRODUCT_DEFINITION_CONTEXT('part definition',#74,'design');\n#76=PRODUCT_DEFINITION('part','',#73,#75);\n#77=PRODUCT_DEFINITION_SHAPE('','',#76);\n#78=SHAPE_DEFINITION_REPRESENTATION(#77,#71);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode aliased topology root");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.product_definitions[0].bodies.len(), 1);
    assert_eq!(
        decoded.ir().model.product_definitions[0].bodies[0].as_str(),
        "step:data:body#31"
    );
    assert!(!decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.as_str() == "step:data:shell_based_surface_model#70"));
}

#[test]
fn topology_root_identity_uses_kind_shell_and_orientation_not_record_order() {
    use cadmpeg_ir::topology::BodyKind;

    let source = include_bytes!("data/br01_topology_root_identity.p21");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode topology root identity witness");

    let bodies = &decoded.ir().model.bodies;
    assert_eq!(bodies.len(), 3, "{:#?}", decoded.report().losses);
    assert!(bodies
        .iter()
        .any(|body| { body.id.as_str() == "step:data:body#32" && body.kind == BodyKind::Sheet }));
    assert!(bodies
        .iter()
        .any(|body| { body.id.as_str() == "step:data:body#34" && body.kind == BodyKind::Solid }));
    assert!(bodies
        .iter()
        .any(|body| { body.id.as_str() == "step:data:body#35" && body.kind == BodyKind::Sheet }));

    let source = String::from_utf8(source.to_vec())
        .expect("witness is UTF-8")
        .replace(
            "#35=SHELL_BASED_SURFACE_MODEL('reversed physical order',(#31));\n#33=SHELL_BASED_SURFACE_MODEL('alias',(#30));\n#34=MANIFOLD_SOLID_BREP('different root kind',#30);\n#32=SHELL_BASED_SURFACE_MODEL('first root',(#30));",
            "#32=SHELL_BASED_SURFACE_MODEL('first root',(#30));\n#34=MANIFOLD_SOLID_BREP('different root kind',#30);\n#33=SHELL_BASED_SURFACE_MODEL('alias',(#30));\n#35=SHELL_BASED_SURFACE_MODEL('reversed physical order',(#31));",
        );
    let reordered = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode physically reordered topology roots");

    let reordered_bodies = &reordered.ir().model.bodies;
    assert_eq!(
        reordered_bodies.len(),
        3,
        "{:#?}",
        reordered.report().losses
    );
    assert!(reordered_bodies
        .iter()
        .any(|body| { body.id.as_str() == "step:data:body#32" && body.kind == BodyKind::Sheet }));
    assert!(reordered_bodies
        .iter()
        .any(|body| { body.id.as_str() == "step:data:body#34" && body.kind == BodyKind::Solid }));
    assert!(reordered_bodies
        .iter()
        .any(|body| { body.id.as_str() == "step:data:body#35" && body.kind == BodyKind::Sheet }));
}

#[test]
fn shared_edge_references_are_scoped_by_independent_roots_not_record_order() {
    let source = include_bytes!("data/tp01_shared_edge_ownership.p21");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode shared edge ownership witness");

    let body_ids = decoded
        .ir()
        .model
        .bodies
        .iter()
        .map(|body| body.id.as_str().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        body_ids,
        ["step:data:body#32", "step:data:body#33"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    );
    let edge_ids = decoded
        .ir()
        .model
        .edges
        .iter()
        .map(|edge| edge.id.as_str().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(edge_ids.contains("step:data:edge#19-root-32-shell-31"));
    assert!(edge_ids.contains("step:data:edge#19-root-33-shell-30"));
    assert_eq!(
        edge_ids
            .iter()
            .filter(|id| id.contains("root-32") || id.contains("root-33"))
            .count(),
        6
    );
    let vertex_ids = decoded
        .ir()
        .model
        .vertices
        .iter()
        .map(|vertex| vertex.id.as_str().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(vertex_ids.contains("step:data:vertex#6-root-32-shell-31"));
    assert!(vertex_ids.contains("step:data:vertex#6-root-33-shell-30"));
    assert!(
        cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
            .expect("resource allocation did not fail")
            .is_ok()
    );

    let reordered_source = String::from_utf8(source.to_vec())
        .expect("witness is UTF-8")
        .replace(
            "#32=SHELL_BASED_SURFACE_MODEL('second physical root',(#31));\n#33=SHELL_BASED_SURFACE_MODEL('first physical root',(#30));",
            "#33=SHELL_BASED_SURFACE_MODEL('first physical root',(#30));\n#32=SHELL_BASED_SURFACE_MODEL('second physical root',(#31));",
        );
    let reordered = StepCodec::default()
        .decode(
            &mut Cursor::new(reordered_source),
            &DecodeOptions::default(),
        )
        .expect("decode physically reordered shared edge witness");
    assert_eq!(reordered.ir().model.bodies.len(), 2);
    let reordered_edge_ids = reordered
        .ir()
        .model
        .edges
        .iter()
        .map(|edge| edge.id.as_str().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(edge_ids, reordered_edge_ids);
}

#[test]
fn topology_root_kind_preserves_distinct_body_kinds_for_shared_shells() {
    use cadmpeg_ir::topology::BodyKind;

    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace("#30=OPEN_SHELL('',(#29));", "#30=CLOSED_SHELL('',(#29));")
            .replace(
                "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
                "#33=ORIENTED_CLOSED_SHELL('',*,#30,.F.);",
            )
            .replace(
                "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
                "#31=SHELL_BASED_SURFACE_MODEL('',(#30));",
            )
            .replace(
                "ENDSEC;\nEND-ISO-10303-21;",
                "#70=MANIFOLD_SOLID_BREP('',#30);\nENDSEC;\nEND-ISO-10303-21;",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode shared shell roots");

    assert_eq!(decoded.ir().model.bodies.len(), 2);
    assert!(decoded
        .ir()
        .model
        .bodies
        .iter()
        .any(|body| body.kind == BodyKind::Sheet));
    assert!(decoded
        .ir()
        .model
        .bodies
        .iter()
        .any(|body| body.kind == BodyKind::Solid));
}

#[test]
fn reused_shell_in_a_distinct_root_gets_a_new_owner_scope() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#70=SHELL_BASED_SURFACE_MODEL('',(#33,#71));\n#71=OPEN_SHELL('',(#29));\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode reused shell root");

    assert_eq!(
        decoded.ir().model.bodies.len(),
        3,
        "{:#?}",
        decoded.report().losses
    );
    assert_eq!(
        decoded
            .ir()
            .model
            .shells
            .iter()
            .filter(|shell| shell.id.as_str().contains("root-70"))
            .count(),
        2
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn distinct_roots_with_shared_topology_get_owner_scopes() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#70=SHELL_BASED_SURFACE_MODEL('',(#71));\n#71=OPEN_SHELL('',(#29));\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode independent roots sharing topology");

    assert_eq!(
        decoded.ir().model.bodies.len(),
        2,
        "{:#?}",
        decoded.report().losses
    );
    assert!(decoded
        .ir()
        .model
        .edges
        .iter()
        .any(|edge| edge.id.as_str().contains("root-70")));
    assert!(decoded
        .ir()
        .model
        .edges
        .iter()
        .any(|edge| edge.id.as_str().contains("root-31")));
    assert!(decoded
        .ir()
        .model
        .vertices
        .iter()
        .any(|vertex| vertex.id.as_str().contains("root-70")));
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
fn missing_vertex_carrier_salvages_complete_sheet_member_but_rejects_solid() {
    let source = include_bytes!("data/tp05_missing_vertex_carrier.p21");
    let sheet = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode partial sheet witness");
    assert_eq!(sheet.ir().model.bodies.len(), 1);
    assert_eq!(sheet.ir().model.faces.len(), 1);
    assert_eq!(sheet.ir().model.vertices.len(), 3);
    assert!(sheet
        .ir()
        .model
        .vertices
        .iter()
        .all(|vertex| !vertex.id.as_str().contains("#6")));
    assert!(sheet.report().losses.iter().any(|loss| loss
        .message
        .contains("VERTEX_POINT #6 has unresolved point carrier #3")));
    assert!(
        cadmpeg_ir::validate_neutral(sheet.ir(), sheet.report().losses.clone())
            .expect("resource allocation did not fail")
            .is_ok()
    );

    let solid_source = String::from_utf8(source.to_vec())
        .expect("witness is UTF-8")
        .replace(
            "#100=SHELL_BASED_SURFACE_MODEL('partial sheet salvage',(#30,#97));",
            "#100=MANIFOLD_SOLID_BREP('partial solid refusal',#30);",
        );
    let solid = StepCodec::default()
        .decode(&mut Cursor::new(solid_source), &DecodeOptions::default())
        .expect("decode partial solid witness");
    assert!(solid.ir().model.bodies.is_empty());
    assert!(solid.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TopologyRootRejected.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Error
    }));
    assert!(
        cadmpeg_ir::validate_neutral(solid.ir(), solid.report().losses.clone())
            .expect("resource allocation did not fail")
            .is_ok()
    );
}

#[test]
fn rejected_solid_root_reports_an_error_severity_loss() {
    let source = String::from_utf8(
        include_bytes!("../../../../tests/fixtures/ap242_vertex_loop.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace("#10=VERTEX_POINT('',#8);", "#10=VERTEX_POINT('',#4);");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("salvage mode accepts a destroyed solid");

    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TopologyRootRejected.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Error
    }));
}

#[test]
fn strict_decode_rejects_a_destroyed_solid() {
    let source = String::from_utf8(
        include_bytes!("../../../../tests/fixtures/ap242_vertex_loop.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace("#10=VERTEX_POINT('',#8);", "#10=VERTEX_POINT('',#4);");
    let mut options = DecodeOptions::default();
    options.policy.mode = DecodeMode::Strict;

    let error = StepCodec::default()
        .decode(&mut Cursor::new(source), &options)
        .expect_err("strict mode rejects a destroyed solid");
    assert!(matches!(
        error,
        cadmpeg_ir::codec::DecodeFailure::StrictRejected { .. }
    ));
}

#[test]
pub(crate) fn every_region_of_a_body_is_retained_as_a_shape_item() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let body = ir.model.bodies[0].id.clone();
    let mut region = ir.model.regions[0].clone();
    region.id = "zzzz:test:region#second"
        .try_into()
        .expect("valid identity");
    ir.model.bodies[0].regions.push(region.id.clone());
    ir.model.regions.push(region);
    let mut builder = Builder::new(&ir, StepSchema::Ap242Edition3).unwrap();
    builder.build();
    assert_eq!(builder.body_item_refs[body.as_str()].len(), 2);
}

#[test]
fn advanced_brep_representation_reuses_its_committed_solid_body() {
    let source = export(&unit_cube().expect("unit cube fixture is admitted")).replace(
        "ADVANCED_BREP_SHAPE_REPRESENTATION",
        "ADVANCED_BREP_REPRESENTATION",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode advanced B-rep representation");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert!(!result
        .ir()
        .native_unknowns("step")
        .unwrap()
        .iter()
        .any(|record| record.id.as_str().contains("advanced_brep_representation")));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn advanced_brep_mapped_representation_reuses_its_committed_solid_body() {
    let mut source = export(&unit_cube().expect("unit cube fixture is admitted")).replace(
        "ADVANCED_BREP_SHAPE_REPRESENTATION",
        "ADVANCED_BREP_REPRESENTATION",
    );
    let representation_line = source
        .lines()
        .find(|line| line.contains("ADVANCED_BREP_REPRESENTATION("))
        .expect("written advanced B-rep representation");
    let representation = representation_line
        .split_once('=')
        .and_then(|(id, _)| id.trim().strip_prefix('#'))
        .and_then(|id| id.parse::<u64>().ok())
        .expect("advanced B-rep representation id");
    let context = representation_line
        .split_once('=')
        .and_then(|(_, record)| record.strip_suffix(';'))
        .and_then(|record| record.strip_suffix(')'))
        .and_then(|record| record.rsplit_once(','))
        .map(|(_, context)| context.trim())
        .expect("advanced B-rep representation context");
    let next_id = source
        .lines()
        .filter_map(|line| {
            line.strip_prefix('#')
                .and_then(|line| line.split_once('='))
                .and_then(|(id, _)| id.trim().parse::<u64>().ok())
        })
        .max()
        .expect("written STEP entity")
        + 1;
    let map = next_id;
    let mapped_item = next_id + 1;
    let mapped_representation = next_id + 2;
    let records = format!(
        "#{map}=REPRESENTATION_MAP($,#{representation});\n\
#{mapped_item}=MAPPED_ITEM('',#{map},$);\n\
#{mapped_representation}=(ADVANCED_BREP_REPRESENTATION() REPRESENTATION('mapped',(#{mapped_item}),{context}) REPRESENTATION_ITEM('mapped'));\n"
    );
    let end = source.rfind("ENDSEC;").expect("STEP data section end");
    source.insert_str(end, &records);

    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode mapped advanced B-rep representation");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("ADVANCED_BREP_REPRESENTATION instance(s) as named opaque STEP records")
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}
