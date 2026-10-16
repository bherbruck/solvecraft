// SPDX-License-Identifier: Apache-2.0
//! STEP B-rep, shell, face, and wire tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};

use crate::export::write_step;
use crate::{StepCodec, StepSchema, StepWriteOptions};

#[test]
pub(crate) fn decode_and_write_singular_vertex_loops() {
    let bytes = include_bytes!("../../../../tests/fixtures/ap242_vertex_loop.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode vertex loops");
    assert_eq!(result.ir().model.loops.len(), 2);
    assert!(result
        .ir()
        .model
        .loops
        .iter()
        .all(|loop_| loop_.singular_vertex().is_some()));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
    let mut encoded = Vec::new();
    write_step(
        result.ir(),
        &mut encoded,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write vertex loops");
    assert_eq!(
        String::from_utf8(encoded)
            .unwrap()
            .matches("VERTEX_LOOP")
            .count(),
        2
    );
}

#[test]
pub(crate) fn decode_builds_a_valid_connected_sheet_brep() {
    use cadmpeg_ir::topology::{BodyKind, Sense};

    let bytes = include_bytes!("../../../../tests/fixtures/ap214_sheet.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode AP214 sheet");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(result.ir().model.regions.len(), 1);
    assert_eq!(result.ir().model.shells.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 3);
    assert_eq!(result.ir().model.edges.len(), 3);
    assert!(result
        .ir()
        .model
        .edges
        .iter()
        .all(|edge| { edge.param_range().is_some_and(|range| range[0] < range[1]) }));
    assert_eq!(result.ir().model.vertices.len(), 3);
    assert_eq!(result.ir().model.pcurves.len(), 1);
    assert_eq!(
        result
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
    assert!(
        matches!(result.ir().model.pcurves[0].geometry, cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(line_pcurve)
                if {
                    let origin = line_pcurve.origin().as_raw();
        let direction = line_pcurve.direction().as_raw();
                    *origin == cadmpeg_ir::math::Point2::new(0.0, 0.0)
                        && *direction == cadmpeg_ir::math::Point2::new(1.0, 0.0)
                })
    );
    assert!(result
        .ir()
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.sense == Sense::Forward));
    assert_eq!(result.ir().model.faces[0].sense, Sense::Reversed);
    assert!(result
        .ir()
        .model
        .appearance_bindings
        .iter()
        .any(|binding| matches!(
            binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Edge(_)
        )));
    assert_eq!(
        result.ir().model.faces[0].color,
        Some(cadmpeg_ir::topology::Color::new(0.9, 0.1, 0.1, 1.0).expect("valid color"))
    );
    assert_eq!(result.ir().model.presentation_layers.len(), 1);
    assert_eq!(
        result.ir().model.presentation_layers[0].name,
        "machined faces"
    );
    assert!(matches!(
        result.ir().model.presentation_layers[0].items.as_slice(),
        [cadmpeg_ir::PresentationItem::Face { .. }]
    ));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);

    let mut output = Vec::new();
    let report = write_step(
        result.ir(),
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write sheet pcurve");
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.message.contains("coedge pcurve(s) use unsupported")));
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode written pcurve");
    assert_eq!(roundtrip.ir().model.pcurves.len(), 1);
    assert_eq!(roundtrip.ir().model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(roundtrip.ir().model.presentation_layers.len(), 1);
    assert_eq!(
        roundtrip.ir().model.presentation_layers[0].name,
        "machined faces"
    );
    assert!(roundtrip
        .ir()
        .model
        .appearance_bindings
        .iter()
        .any(|binding| matches!(
            binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Edge(_)
        )));
    assert_eq!(
        roundtrip
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
}

#[test]
pub(crate) fn decode_builds_a_valid_ap203_sheet_brep() {
    use cadmpeg_ir::topology::BodyKind;

    let bytes = include_bytes!("../../../../tests/fixtures/ap203_sheet.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode AP203 sheet");

    assert_eq!(
        result.ir().source.as_ref().unwrap().attributes["schema"],
        "CONFIG_CONTROL_DESIGN"
    );
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.edges.len(), 3);
    assert_eq!(result.ir().model.vertices.len(), 3);
    let composite = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#34")
        .expect("outer composite curve");
    assert!(matches!(
        &composite.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Composite {
            segments,
            self_intersect: Some(false)
        }) if segments.len() == 1
            && segments[0].curve.as_str() == "step:data:curve#36"
            && segments[0].same_sense
            && segments[0].transition
                == cadmpeg_ir::geometry::CompositeCurveTransition::ContSameGradient
    ));
    assert!(result
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| matches!(
            surface.definition(),
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::CurveBounded {
                support,
                boundaries,
                implicit_outer: false,
                ..
            } if support.as_str() == "step:data:surface#28"
                && boundaries.as_slice() == [cadmpeg_ir::ids::CurveId::mint("step:data:curve#34").expect("identity grammar")]
        )));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);

    let mut encoded = Vec::new();
    write_step(
        result.ir(),
        &mut encoded,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write composite curve graph");
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("decode written composite curve graph");
    assert!(roundtrip.ir().model.curves.iter().any(|curve| matches!(
        curve.geometry,
        CurveGeometry::Solved(SolvedCurveGeometry::Composite { .. })
    )));
}

#[test]
fn decode_builds_a_face_based_surface_model() {
    use cadmpeg_ir::topology::BodyKind;

    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
                "#31=FACE_BASED_SURFACE_MODEL('',(#30));",
            )
            .replace(
                "#30=OPEN_SHELL('',(#29));",
                "#30=CONNECTED_FACE_SET('',(#29));",
            );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode face-based surface model");

    assert_eq!(
        result.ir().model.bodies.len(),
        1,
        "{:#?}",
        result.report().losses
    );
    assert_eq!(result.ir().model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("does not resolve to a complete")));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_builds_faceted_brep_polygon_loops() {
    use cadmpeg_ir::topology::BodyKind;

    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#25=EDGE_LOOP('',(#22,#23,#24));",
                "#25=POLY_LOOP('',(#3,#4,#5,#3));",
            )
            .replace("#30=OPEN_SHELL('',(#29));", "#30=CLOSED_SHELL('',(#29));")
            .replace(
                "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
                "#31=FACETED_BREP('',#30);",
            );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode faceted brep");

    assert_eq!(
        result.ir().model.bodies.len(),
        1,
        "{:#?}",
        result.report().losses
    );
    assert_eq!(result.ir().model.bodies[0].kind, BodyKind::Solid);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 3);
    assert!(result
        .ir()
        .model
        .edges
        .iter()
        .all(|edge| edge.curve().is_none()));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn sheet_root_salvages_independent_shells() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33,#34));",
        )
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);\n#34=ORIENTED_OPEN_SHELL('',*,#99,.T.);\n#99=UNSUPPORTED_SHELL('',());",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode sheet with one invalid shell");

    assert_eq!(
        decoded.ir().model.bodies.len(),
        1,
        "{:#?}",
        decoded.report().losses
    );
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("omitted 1 unresolved shell")));
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("shell carrier #34")));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_builds_a_sheet_from_a_geometric_surface_set() {
    use cadmpeg_ir::topology::BodyKind;

    let bytes = include_bytes!("../../../../tests/fixtures/ap242_geometric_set.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode geometric surface set");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert!(result.ir().model.faces[0].loops.is_empty());
    assert_eq!(
        result.ir().model.faces[0].surface.as_str(),
        "step:data:surface#11"
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_geometric_set_representation_uses_its_named_items() {
    use cadmpeg_ir::topology::BodyKind;

    let mut source = String::from_utf8(include_bytes!(
        "../../../../tests/fixtures/ap242_geometric_set.p21"
    )
    .to_vec())
    .expect("fixture is UTF-8")
    .replace(
        "#12=GEOMETRIC_SET('',(#11));",
        "#12=GEOMETRIC_SET('',(#11,#14,#15));",
    )
    .replace(
        "#13=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#12),#2);",
        "#13=(GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION() REPRESENTATION('',(#12),#2) SHAPE_REPRESENTATION());",
    );
    let end = source.rfind("ENDSEC;").expect("STEP data section end");
    source.insert_str(
        end,
        "#14=(CIRCLE('',#6,5.) CURVE() GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('free circle'));\n#15=UNSUPPORTED_ITEM('');\n",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex geometric surface set");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(result.ir().model.faces.len(), 1);
    let free_circle = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#14")
        .expect("free complex representation member");
    assert_eq!(
        free_circle
            .source_object
            .as_ref()
            .and_then(|source| source.name.as_deref()),
        Some("free circle")
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains(
            "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #13 omitted unsupported or unresolved member(s): #15",
        )
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
pub(crate) fn reader_recovers_a_valid_solid_from_writer_output() {
    use cadmpeg_ir::topology::BodyKind;

    let source = unit_cube().expect("unit cube fixture is admitted");
    let mut bytes = Vec::new();
    write_step(
        &source,
        &mut bytes,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode generated cube STEP");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.bodies[0].kind, BodyKind::Solid);
    assert_eq!(result.ir().model.faces.len(), 6);
    assert_eq!(result.ir().model.edges.len(), 12);
    assert_eq!(result.ir().model.vertices.len(), 8);
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}
