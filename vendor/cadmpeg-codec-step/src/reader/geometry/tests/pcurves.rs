// SPDX-License-Identifier: Apache-2.0
//! STEP geometry, pcurve, NURBS, unit, replica, and trim tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{pcurve::PcurveGeometry, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::Point2;

use crate::loss::StepLossCode;
use crate::test_support::exchange::{decode_inline, equivalent_seam_source};
use crate::StepCodec;

const EPS_PCURVE_PARAMETERS: f64 = 1.0e-12;

#[test]
fn invalid_single_pcurve_is_omitted_instead_of_invalidating_topology() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace("#52=DIRECTION('',(1.,0.));", "#52=DIRECTION('',(0.,1.));");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode source with invalid pcurve");
    assert!(decoded
        .ir()
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.pcurves.is_empty()));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveEndpointsDiscontinuous.kind()
            && loss.message.contains("one optional pcurve")
            && loss.message.contains("not continuous")
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn pcurve_requires_one_two_dimensional_definition_and_rejects_replica_cycles() {
    let source = include_bytes!("data/tp07_pcurve_recursion.p21");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode pcurve recursion witness");

    assert_eq!(decoded.ir().model.pcurves.len(), 1);
    let pcurve = decoded
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.as_str() == "step:data:pcurve#31")
        .expect("valid pcurve");
    assert_eq!(
        pcurve.geometry,
        PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                Point2::new(2.0, 3.0),
                Point2::new(2.0, 0.0)
            )
            .unwrap()
        )
    );
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("CURVE_REPLICA #34 has invalid or unresolved parent/operator")
    }));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("PCURVE #36 has no decoded surface or 2D curve")
    }));
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.as_str() == "step:data:pcurve#33"));
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.as_str() == "step:data:pcurve#36"));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn surface_curve_retains_direct_surface_support() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#57=SURFACE_CURVE('',#16,(#56),.PCURVE_S1.);",
                "#57=SURFACE_CURVE('',#16,(#70),.CURVE_3D.);\n#70=PLANE('',#27);",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode direct surface-curve support");

    let support = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#70")
        .expect("direct surface support carrier");
    assert_eq!(
        support
            .source_object
            .as_ref()
            .map(|source| source.object_id.as_str()),
        Some("#57")
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn pcurve_trimmed_carrier_is_not_promoted_to_a_3d_curve() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#69=PCURVE('',#28,#70);\n#70=DEFINITIONAL_REPRESENTATION('',(#72),#50);\n#71=LINE('',#51,#53);\n#72=TRIMMED_CURVE('',#71,(0.),(1.),.T.,.PARAMETER.);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode pcurve trimmed carrier");

    assert!(decoded.ir().model.curves.iter().all(|curve| {
        curve.id.as_str() != "step:data:curve#71" && curve.id.as_str() != "step:data:curve#72"
    }));
    assert!(decoded.report().losses.iter().all(|loss| {
        !loss
            .message
            .contains("TRIMMED_CURVE #72 has invalid or unresolved basis/trim selectors")
    }));
}

#[test]
fn trimmed_curve_resolves_a_surface_curve_basis_carrier() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#70=TRIMMED_CURVE('',#57,(0.),(1.),.T.,.PARAMETER.);\n#71=GEOMETRIC_SET('',(#70));\n#72=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#71),#2);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode surface-curve trim");

    assert!(decoded.ir().model.curves.iter().any(|curve| {
        curve.id.as_str() == "step:data:curve#70"
            && matches!(curve.geometry.solved(), Some(SolvedCurveGeometry::Line(_)))
    }));
    assert!(decoded.ir().model.procedural_curves.iter().any(|curve| {
        decoded
            .ir()
            .model
            .procedural_curve_owner(&curve.id)
            .map(CurveId::as_str)
            == Some("step:data:curve#70")
            && match curve.definition() { cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset(matched_payload) => matches!((matched_payload.source(),), (source,) if source.as_str() == "step:data:curve#16"), _ => false }
    }));
    assert!(decoded.report().losses.iter().all(|loss| {
        !loss
            .message
            .contains("TRIMMED_CURVE #70 has invalid or unresolved basis/trim selectors")
    }));
}

#[test]
fn pcurve_trimmed_opposed_sense_has_an_ordered_parameter_range() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#53=VECTOR('',#52,1.);",
            "#53=VECTOR('',#52,10.);",
        )
        .replace(
            "#54=LINE('',#51,#53);",
            "#54=TRIMMED_CURVE('',#71,(PARAMETER_VALUE(1.)),(PARAMETER_VALUE(0.)),.F.,.PARAMETER.);\n#71=LINE('',#51,#53);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode opposed-sense pcurve trim");
    let pcurve = decoded
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.as_str() == "step:data:pcurve#56")
        .expect("trimmed pcurve");
    assert!(
        matches!(&pcurve.geometry, cadmpeg_ir::geometry::pcurve::PcurveGeometry::Trimmed(trimmed_pcurve)
        if {
            let [start, end] = &trimmed_pcurve.parameter_range().endpoints();
            *start == 0.0 && *end == 1.0
        })
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn pcurve_trimmed_stale_range_recovers_the_edge_use_interval() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#53=VECTOR('',#52,1.);", "#53=VECTOR('',#52,10.);")
        .replace(
            "#54=LINE('',#51,#53);",
            "#54=TRIMMED_CURVE('',#71,(PARAMETER_VALUE(-1.)),(PARAMETER_VALUE(2.)),.T.,.PARAMETER.);\n#71=LINE('',#51,#53);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode stale pcurve trim");
    let pcurve = cadmpeg_ir::ids::PcurveId::mint("step:data:pcurve#56").expect("identity grammar");
    let use_ = decoded
        .ir()
        .model
        .coedges
        .iter()
        .flat_map(|coedge| &coedge.pcurves)
        .find(|use_| use_.pcurve == pcurve)
        .expect("stale trimmed pcurve use");
    assert_eq!(
        use_.parameter_range
            .map(cadmpeg_ir::geometry::DirectedParameterRange::endpoints),
        Some([0.0, 1.0])
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn cylindrical_pcurve_coordinates_follow_surface_parameter_units() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("SI_UNIT(.MILLI.,.METRE.)", "SI_UNIT(.CENTI.,.METRE.)")
        .replace(
            "#4=CARTESIAN_POINT('',(10.,0.,0.));",
            "#4=CARTESIAN_POINT('',(1.,0.,0.));",
        )
        .replace("#13=VECTOR('',#10,10.);", "#13=VECTOR('',#10,1.);")
        .replace(
            "#27=AXIS2_PLACEMENT_3D('',#3,#9,#10);",
            "#70=CARTESIAN_POINT('',(0.,-1.,0.));\n#71=DIRECTION('',(1.,0.,0.));\n#72=DIRECTION('',(0.,1.,0.));\n#27=AXIS2_PLACEMENT_3D('',#70,#71,#72);",
        )
        .replace("#28=PLANE('',#27);", "#28=CYLINDRICAL_SURFACE('',#27,1.);")
        .replace("#52=DIRECTION('',(1.,0.));", "#52=DIRECTION('',(0.,1.));");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode cylindrical pcurve");

    let pcurve = decoded
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.as_str() == "step:data:pcurve#56")
        .expect("cylindrical pcurve");
    assert!(
        matches!(&pcurve.geometry, cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(line_pcurve)
        if {
            let direction = line_pcurve.direction().as_raw();
            direction.u.abs() < EPS_PCURVE_PARAMETERS && (direction.v - 10.0).abs() < EPS_PCURVE_PARAMETERS
        })
    );
}

#[test]
fn periodic_surface_pcurve_selection_seeds_line_parameter_branches() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#3=CARTESIAN_POINT('',(0.,0.,0.));",
            "#3=CARTESIAN_POINT('',(-1.,0.,0.));",
        )
        .replace(
            "#4=CARTESIAN_POINT('',(10.,0.,0.));",
            "#4=CARTESIAN_POINT('',(-0.5403023058681398,0.,0.8414709848078965));",
        )
        .replace("#16=LINE('',#3,#13);", "#16=CIRCLE('',#27,1.);")
        .replace(
            "#27=AXIS2_PLACEMENT_3D('',#3,#9,#10);",
            "#70=CARTESIAN_POINT('',(0.,0.,0.));\n#71=DIRECTION('',(0.,1.,0.));\n#72=DIRECTION('',(1.,0.,0.));\n#27=AXIS2_PLACEMENT_3D('',#70,#71,#72);",
        )
        .replace("#28=PLANE('',#27);", "#28=CYLINDRICAL_SURFACE('',#27,1.);");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode periodic cylindrical pcurve");

    assert!(decoded.ir().model.coedges.iter().any(|coedge| {
        coedge
            .pcurves
            .iter()
            .any(|use_| use_.pcurve.as_str() == "step:data:pcurve#56")
    }));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveAssociationAmbiguous.kind()
            && loss.message.contains("curve #57")
            && loss.message.contains("no pcurve")
    }));
}

#[test]
fn unsupported_optional_pcurve_does_not_discard_valid_topology() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace("#54=LINE('',#51,#53);", "#54=UNSUPPORTED_CURVE('',#51);");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode sheet with unsupported optional pcurve");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert_eq!(decoded.ir().model.edges.len(), 3);
    assert!(decoded
        .ir()
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.pcurves.is_empty()));
    assert!(decoded.report().losses.iter().any(|loss| loss
        .message
        .contains("PCURVE #56 has no decoded surface or 2D curve")));
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("conflicts with decoded topology")));
}

#[test]
fn linear_extrusion_pcurve_uses_directrix_and_dimensionless_sweep_parameters() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));",
                "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT($,.METRE.));",
            )
            .replace(
                "#28=PLANE('',#27);",
                "#28=SURFACE_OF_LINEAR_EXTRUSION('',#16,#70);",
            )
            .replace(
                "#4=CARTESIAN_POINT('',(10.,0.,0.));",
                "#4=CARTESIAN_POINT('',(10.,0.,2.));",
            )
            .replace(
                "#57=SURFACE_CURVE('',#16,(#56),.PCURVE_S1.);",
                "#57=SURFACE_CURVE('',#72,(#56),.PCURVE_S1.);",
            )
            .replace(
                "#52=DIRECTION('',(1.,0.));",
                "#52=DIRECTION('',(1.,1.));",
            )
            .replace(
                "#53=VECTOR('',#52,1.);",
                "#53=VECTOR('',#52,1.4142135623730951);",
            )
            .replace(
                "ENDSEC;\nEND-ISO-10303-21;",
                "#70=VECTOR('',#71,2.);\n#71=DIRECTION('',(0.,0.,1.));\n#72=LINE('',#3,#73);\n#73=VECTOR('',#74,10.198039027185569);\n#74=DIRECTION('',(5.,0.,1.));\nENDSEC;\nEND-ISO-10303-21;",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode linear extrusion pcurve");
    let pcurve = decoded
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.as_str() == "step:data:pcurve#56")
        .expect("linear extrusion pcurve");
    assert_eq!(
        pcurve.geometry,
        PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                Point2::new(0.0, 0.0),
                Point2::new(10_000.0, 1.0)
            )
            .unwrap()
        )
    );
    assert!(decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| {
            decoded
                .ir()
                .model
                .procedural_surface_owner(&surface.id)
                .map(SurfaceId::as_str)
                == Some("step:data:surface#28")
                && matches!(
                    surface.definition(),
                    cadmpeg_ir::geometry::ProceduralSurfaceDefinition::LinearSweep { .. }
                )
        }));
}

#[test]
fn decode_maps_a_two_dimensional_polyline_to_a_pcurve_nurbs() {
    use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
    use cadmpeg_ir::math::Point2;

    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#52=DIRECTION('',(1.,0.));",
                "#52=CARTESIAN_POINT('',(1.,2.));",
            )
            .replace(
                "#4=CARTESIAN_POINT('',(10.,0.,0.));",
                "#4=CARTESIAN_POINT('',(3.,2.,0.));",
            )
            .replace(
                "#16=LINE('',#3,#13);",
                "#70=CARTESIAN_POINT('',(1.,2.,0.));\n#16=B_SPLINE_CURVE_WITH_KNOTS('',1,(#3,#70,#4),.UNSPECIFIED.,.F.,.F.,(2,1,2),(0.,1.,2.),.PIECEWISE_BEZIER_KNOTS.);",
            )
            .replace("#53=VECTOR('',#52,1.);", "#53=CARTESIAN_POINT('',(3.,2.));")
            .replace("#54=LINE('',#51,#53);", "#54=POLYLINE('',(#51,#52,#53));");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode polyline pcurve");

    assert!(matches!(
        &decoded.ir().model.pcurves[0].geometry,
        PcurveGeometry::Nurbs { nurbs }
            if nurbs.degree() == 1
                && nurbs.weights().is_none()
                && !nurbs.periodic()
                && nurbs.control_points() == [
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 2.0),
            Point2::new(3.0, 2.0),
        ]
    ));
    assert_eq!(decoded.ir().model.bodies.len(), 1);
}

#[test]
fn planar_pcurve_coordinates_follow_the_document_length_unit() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace("SI_UNIT(.MILLI.,.METRE.)", "SI_UNIT(.CENTI.,.METRE.)");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode non-millimetre planar pcurve");

    let pcurve = decoded
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.as_str() == "step:data:pcurve#56")
        .expect("planar pcurve");
    assert!(
        matches!(&pcurve.geometry, cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(line_pcurve)
        if {
            let direction = line_pcurve.direction().as_raw();
            (direction.u - 10.0).abs() < EPS_PCURVE_PARAMETERS
        })
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn degree_valued_cylindrical_pcurve_is_not_reinterpreted() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#3=CARTESIAN_POINT('',(0.,0.,0.));",
            "#3=CARTESIAN_POINT('',(-1.,0.,0.));",
        )
        .replace(
            "#4=CARTESIAN_POINT('',(10.,0.,0.));",
            "#4=CARTESIAN_POINT('',(-1.,10.,0.));",
        )
        .replace(
            "#10=DIRECTION('',(1.,0.,0.));",
            "#10=DIRECTION('',(0.,1.,0.));",
        )
        .replace(
            "#27=AXIS2_PLACEMENT_3D('',#3,#9,#10);",
            "#70=CARTESIAN_POINT('',(0.,0.,0.));\n#71=DIRECTION('',(0.,1.,0.));\n#72=DIRECTION('',(1.,0.,0.));\n#27=AXIS2_PLACEMENT_3D('',#70,#71,#72);",
        )
        .replace("#28=PLANE('',#27);", "#28=CYLINDRICAL_SURFACE('',#27,1.);")
        .replace(
            "#51=CARTESIAN_POINT('',(0.,0.));",
            "#51=CARTESIAN_POINT('',(180.,0.));",
        )
        .replace("#52=DIRECTION('',(1.,0.));", "#52=DIRECTION('',(0.,1.));")
        .replace("#53=VECTOR('',#52,1.);", "#53=VECTOR('',#52,10.);");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode degree-valued cylindrical pcurve");

    assert!(decoded
        .ir()
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.pcurves.is_empty()));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveEndpointsDiscontinuous.kind()
            && loss.message.contains("curve #57")
            && loss.message.contains("pcurve is omitted")
    }));
}

#[test]
fn cylindrical_pcurve_uses_surface_parameter_without_degree_repair() {
    let source = include_bytes!("data/pc01_surface_parameter.p21");
    let valid = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode pi-valued cylindrical pcurve");
    let pcurve = valid
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.as_str() == "step:data:pcurve#34")
        .expect("surface-chart pcurve");
    assert!(
        matches!(&pcurve.geometry, cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(line_pcurve)
                if {
                    let origin = line_pcurve.origin().as_raw();
        let direction = line_pcurve.direction().as_raw();
                    (origin.u - std::f64::consts::PI).abs() < EPS_PCURVE_PARAMETERS
                        && origin.v.abs() < EPS_PCURVE_PARAMETERS
                        && direction.u.abs() < EPS_PCURVE_PARAMETERS
                        && (direction.v - 10.0).abs() < EPS_PCURVE_PARAMETERS
                })
    );

    let invalid_source = String::from_utf8(source.to_vec())
        .expect("fixture is UTF-8")
        .replace("3.141592653589793", "180.");
    let invalid = StepCodec::default()
        .decode(
            &mut Cursor::new(invalid_source.as_bytes()),
            &DecodeOptions::default(),
        )
        .expect("decode degree-looking cylindrical pcurve");
    assert!(invalid.ir().model.pcurves.is_empty());
    assert!(invalid.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveEndpointsDiscontinuous.kind()
            && loss.message.contains("curve #33")
            && loss.message.contains("pcurve is omitted")
    }));
}

#[test]
fn inconsistent_optional_pcurve_is_omitted_and_retained_as_source_data() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#51=CARTESIAN_POINT('',(0.,0.));",
                "#51=CARTESIAN_POINT('',(0.,1.));",
            )
            .replace(
                "#54=LINE('',#51,#53);",
                "#54=TRIMMED_CURVE('',#71,(0.),(10.),.T.,.PARAMETER.);",
            )
            .replace(
                "#68=STYLED_ITEM('',(#66),#19);",
                "#68=STYLED_ITEM('',(#66),#19);\n#71=LINE('',#51,#53);",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode inconsistent optional pcurve");

    assert!(decoded
        .ir()
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.pcurves.is_empty()));
    assert!(
        decoded.report().losses.iter().any(|loss| {
            loss.code == StepLossCode::PcurveEndpointsDiscontinuous.kind()
                && loss.severity == cadmpeg_ir::report::Severity::Error
                && loss.message.contains("optional pcurve")
        }),
        "{:#?}",
        decoded.report().losses
    );
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.as_str() == "step:data:pcurve#56"));

    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

fn distinct_seam_source() -> String {
    equivalent_seam_source().replace(
        "#71=LINE('',#51,#53);",
        "#71=POLYLINE('',(#51,#72,#73));\n#72=CARTESIAN_POINT('',(5.,5.));\n#73=CARTESIAN_POINT('',(10.,0.));",
    )
}

fn seam_source_with_one_endpoint_continuous_candidate() -> String {
    equivalent_seam_source().replace(
        "#71=LINE('',#51,#53);",
        "#71=LINE('',#72,#53);\n#72=CARTESIAN_POINT('',(0.,5.));",
    )
}

#[test]
fn equivalent_same_surface_pcurve_candidates_remain_detached() {
    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(equivalent_seam_source()),
            &DecodeOptions::default(),
        )
        .expect("decode equivalent seam pcurves");

    assert!(decoded
        .ir()
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.pcurves.is_empty()));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveAssociationAmbiguous.kind()
            && loss.message.contains("2 pcurves")
    }));

    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn seam_edge_uses_its_explicit_pcurve_reference() {
    let source = include_bytes!("data/tp02_seam_edge_selection.p21");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode seam edge selection witness");
    let coedge = decoded
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id.as_str().contains("#22"))
        .expect("SEAM_EDGE coedge");
    assert_eq!(coedge.pcurves.len(), 1);
    assert_eq!(coedge.pcurves[0].pcurve.as_str(), "step:data:pcurve#69");
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == StepLossCode::SeamEdgePcurveUnresolved.kind() }));

    let reordered_source = String::from_utf8(source.to_vec())
        .expect("witness is UTF-8")
        .replace(
            "#57=SEAM_CURVE('',#16,(#56,#69),.PCURVE_S1.);",
            "#57=SEAM_CURVE('',#16,(#69,#56),.PCURVE_S1.);",
        );
    let reordered = StepCodec::default()
        .decode(
            &mut Cursor::new(reordered_source),
            &DecodeOptions::default(),
        )
        .expect("decode reordered seam edge selection witness");
    let reordered_coedge = reordered
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id.as_str().contains("#22"))
        .expect("reordered SEAM_EDGE coedge");
    assert_eq!(
        reordered_coedge.pcurves[0].pcurve.as_str(),
        "step:data:pcurve#69"
    );
}

#[test]
fn invalid_seam_edge_reference_does_not_fall_back_to_another_pcurve() {
    let source = String::from_utf8(include_bytes!("data/tp02_seam_edge_selection.p21").to_vec())
        .expect("witness is UTF-8")
        .replace(
            "#22=SEAM_EDGE('',*,*,#19,.T.,#69);",
            "#22=SEAM_EDGE('',*,*,#19,.T.,#70);",
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#70=PCURVE('',#28,#55);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode invalid seam edge reference witness");
    let coedge = decoded
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id.as_str().contains("#22"))
        .expect("invalid SEAM_EDGE coedge");
    assert!(coedge.pcurves.is_empty());
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::SeamEdgePcurveUnresolved.kind()));
}

#[test]
fn reordered_same_surface_pcurve_candidates_remain_detached() {
    let source = equivalent_seam_source().replace(
        "#57=SEAM_CURVE('',#16,(#56,#69),.PCURVE_S1.);",
        "#57=SEAM_CURVE('',#16,(#69,#56),.PCURVE_S1.);",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode reordered equivalent seam pcurves");

    assert!(decoded
        .ir()
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.pcurves.is_empty()));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveAssociationAmbiguous.kind()
            && loss.message.contains("2 pcurves")
    }));
}

#[test]
fn distinct_tied_seam_pcurve_candidates_are_reported_not_guessed() {
    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(distinct_seam_source()),
            &DecodeOptions::default(),
        )
        .expect("decode distinct tied seam pcurves");

    assert!(decoded
        .ir()
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.pcurves.is_empty()));
    let losses: Vec<_> = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == StepLossCode::PcurveAssociationAmbiguous.kind())
        .collect();
    assert_eq!(
        losses.len(),
        1,
        "unexpected losses: {:#?}",
        decoded.report().losses
    );
    assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Warning);
    assert_eq!(
        losses[0].message,
        "curve #57 associates 2 pcurves with surface #28; Part 42 provides no non-seam selector, so the coedge has no pcurve"
    );
}

#[test]
fn endpoint_continuity_does_not_break_a_multiple_candidate_tie() {
    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(seam_source_with_one_endpoint_continuous_candidate()),
            &DecodeOptions::default(),
        )
        .expect("decode endpoint-continuous seam pcurve");

    assert!(decoded
        .ir()
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.pcurves.is_empty()));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveAssociationAmbiguous.kind()
            && loss.message.contains("2 pcurves")
    }));

    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn an_unambiguous_pcurve_still_binds() {
    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode unambiguous pcurve");

    assert!(decoded.ir().model.coedges.iter().any(|coedge| {
        coedge
            .pcurves
            .iter()
            .any(|use_| use_.pcurve.as_str() == "step:data:pcurve#56")
    }));
}

#[test]
fn ambiguous_pcurves_do_not_reject_the_body() {
    use cadmpeg_ir::topology::BodyKind;

    let source = distinct_seam_source()
        .replace("#30=OPEN_SHELL('',(#29));", "#30=CLOSED_SHELL('',(#29));")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=MANIFOLD_SOLID_BREP('',#30);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode solid with ambiguous seam pcurves");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.bodies[0].kind, BodyKind::Solid);
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::PcurveAssociationAmbiguous.kind()));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn intersection_curve_binds_its_basis_curve_and_pcurves() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#57=SURFACE_CURVE('',#16,(#56),.PCURVE_S1.);",
                "#57=INTERSECTION_CURVE('',#16,(#56),.PCURVE_S1.);",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode intersection curve");

    let edge = decoded
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| edge.id.as_str() == "step:data:edge#19")
        .expect("intersection-curve edge");
    assert_eq!(
        edge.curve().map(CurveId::as_str),
        Some("step:data:curve#16")
    );
    assert!(decoded.ir().model.coedges.iter().any(|coedge| {
        coedge
            .pcurves
            .iter()
            .any(|use_| use_.pcurve.as_str() == "step:data:pcurve#56")
    }));
    assert!(decoded.report().losses.iter().all(|loss| !loss
        .message
        .contains("surface-curve #57 has no resolvable basis")));
}

#[test]
fn quasi_uniform_pcurve_is_decoded_from_its_2d_representation() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#51=CARTESIAN_POINT('',(0.,0.));\n#52=DIRECTION('',(1.,0.));",
            "#51=CARTESIAN_POINT('',(0.,0.));\n#52=DIRECTION('',(1.,0.));\n#58=CARTESIAN_POINT('',(10.,0.));",
        )
        .replace(
            "#54=LINE('',#51,#53);",
            "#54=QUASI_UNIFORM_CURVE('',1,(#51,#58),.UNSPECIFIED.,.F.,.F.);",
        )
        .replace(
            "#55=DEFINITIONAL_REPRESENTATION('',(#54),#50);",
            "#55=(DEFINITIONAL_REPRESENTATION()REPRESENTATION('',(#54),#50)SHAPE_REPRESENTATION());",
        );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode quasi-uniform pcurve");

    assert!(result.ir().model.pcurves.iter().any(|pcurve| {
        matches!(
            &pcurve.geometry,
            cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs }
                if nurbs.degree() == 1
                    && nurbs.knots().as_slice() == [0.0, 0.0, 1.0, 1.0]
                    && nurbs.control_points().len() == 2
                    && nurbs.weights().is_none()
                    && !nurbs.periodic()
        )
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn direct_boundary_curve_builds_a_curve_bounded_surface() {
    for boundary_type in ["BOUNDARY_CURVE", "OUTER_BOUNDARY_CURVE"] {
        let source = format!(
            "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n\
#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));\n\
#3=CARTESIAN_POINT('',(0.,0.,0.));\n\
#4=DIRECTION('',(0.,0.,1.));\n\
#5=DIRECTION('',(1.,0.,0.));\n\
#6=AXIS2_PLACEMENT_3D('',#3,#4,#5);\n\
#7=CIRCLE('',#6,5.);\n\
#8=COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#7);\n\
#9={boundary_type}('',(#8),.F.);\n\
#10=PLANE('',#6);\n\
#11=CURVE_BOUNDED_SURFACE('bounded',#10,(#9),.F.);\n\
#12=GEOMETRIC_SET('',(#11));\n\
#13=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#12),#2);\n",
        );
        let result = decode_inline(&source);

        let boundary = result
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id.as_str() == "step:data:curve#9")
            .expect("boundary curve carrier");
        assert!(matches!(
            &boundary.geometry,
            CurveGeometry::Solved(SolvedCurveGeometry::Composite { segments, .. })
                if segments.len() == 1 && segments[0].curve.as_str() == "step:data:curve#7"
        ));

        let bounded = result
            .ir()
            .model
            .procedural_surfaces
            .iter()
            .find(|surface| surface.id.as_str() == "step:construction:curve_bounded_surface#11")
            .expect("curve-bounded surface");
        assert!(matches!(
            bounded.definition(),
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::CurveBounded { boundaries, .. }
                if boundaries == &[CurveId::mint("step:data:curve#9".to_owned()).expect("identity grammar")]
        ));
        assert!(!result.report().losses.iter().any(|loss| {
            loss.message
                .contains("has invalid, cyclic, or unresolved segments")
        }));
        let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("resource allocation did not fail");
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn complex_surface_curve_pcurve_is_retained_by_curve_bounded_surface() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap203_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#36=COMPOSITE_CURVE('nested edge',(#33),.F.);",
            "#36=(BOUNDED_CURVE() CURVE() GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('nested edge') SURFACE_CURVE(#16,(#44),.PCURVE_S1.));",
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#38=(GEOMETRIC_REPRESENTATION_CONTEXT(2) PARAMETRIC_REPRESENTATION_CONTEXT() REPRESENTATION_CONTEXT('uv','2D'));\n#39=CARTESIAN_POINT('',(0.,0.));\n#40=DIRECTION('',(1.,0.));\n#41=VECTOR('',#40,1.);\n#42=LINE('',#39,#41);\n#43=DEFINITIONAL_REPRESENTATION('',(#42),#38);\n#44=PCURVE('',#28,#43);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex surface curve boundary");

    let (boundaries, boundary_pcurves) = decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find_map(|surface| match surface.definition() {
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::CurveBounded {
                boundaries,
                boundary_pcurves,
                ..
            } => Some((boundaries, boundary_pcurves)),
            _ => None,
        })
        .expect("curve-bounded surface");
    assert_eq!(
        boundaries,
        &[cadmpeg_ir::ids::CurveId::mint("step:data:curve#34").expect("identity grammar")]
    );
    assert!(
        decoded
            .ir()
            .model
            .pcurves
            .iter()
            .any(|pcurve| pcurve.id.as_str() == "step:data:pcurve#44"),
        "pcurves={:#?}, losses={:#?}",
        decoded.ir().model.pcurves,
        decoded.report().losses
    );
    assert_eq!(
        boundary_pcurves,
        &[cadmpeg_ir::ids::PcurveId::mint("step:data:pcurve#44").expect("identity grammar")]
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn free_surface_curve_keeps_its_three_dimensional_basis_reachable() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#80=CARTESIAN_POINT('',(20.,0.,0.));\n#81=DIRECTION('',(1.,0.,0.));\n#82=VECTOR('',#81,1.);\n#83=LINE('',#80,#82);\n#84=SURFACE_CURVE('',#83,(#56),.PCURVE_S1.);\n#85=GEOMETRIC_SET('free surface curve',(#84));\nENDSEC;\nEND-ISO-10303-21;",
        );
    let result = StepCodec::default()
        .decode(
            &mut Cursor::new(source.as_bytes()),
            &DecodeOptions::default(),
        )
        .expect("decode free surface curve");

    let basis = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#83")
        .expect("surface-curve basis");
    assert_eq!(
        basis
            .source_object
            .as_ref()
            .map(|source| source.object_id.as_str()),
        Some("#84")
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(!validation.findings.iter().any(|finding| {
        finding.check == cadmpeg_ir::report::check::Check::CarrierReachability
            && finding.entity.as_deref() == Some("step:data:curve#83")
    }));
}
