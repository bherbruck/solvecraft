// SPDX-License-Identifier: Apache-2.0
//! STEP geometry, pcurve, NURBS, unit, replica, and trim tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use crate::ids::kind;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::eval::{
    model_curve_point_by_id, model_surface_partials_by_id, model_surface_point_by_id,
};
use cadmpeg_ir::geometry::{
    pcurve::PcurveGeometry, Curve, SolvedCurveGeometry, SolvedSurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, SurfaceId};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point2, Point3, Vector3};

use crate::export::write_step;
use crate::ids;
use crate::loss::StepLossCode;
use crate::test_support::exchange::decode_inline;
use crate::{StepCodec, StepSchema, StepWriteOptions};

const EPS_TESSELLATED_CURVE_POINT: f64 = 1.0e-12;
const EPS_APLL_POINT: f64 = 1.0e-12;
const EPS_TP03_PARAMETER_SCALE: f64 = 1.0e-12;

#[test]
fn apll_point_name_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=APLL_POINT('leader',(1.,2.,3.),.NONE.);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(SOURCE, crate::parse::parse_inner)
            .expect("valid APLL exchange");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "step_string_text",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(SOURCE, &arena, &policy)
                .expect("root fits retained policy");
            let mut ir = cadmpeg_ir::document::CadIr::empty();
            (super::super::decode(&exchange, &mut ir, &ctx)).map(|_| ())
        },
    );
    assert!(
        matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_string_text")
    );
}

#[test]
fn tessellated_curve_name_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.)));#2=TESSELLATED_CURVE_SET('curve name',#1,((1,2)));ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(SOURCE, crate::parse::parse_inner)
            .expect("valid tessellation exchange");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "step_string_text",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(SOURCE, &arena, &policy)
                .expect("root fits retained policy");
            let mut ir = cadmpeg_ir::document::CadIr::empty();
            (super::super::decode(&exchange, &mut ir, &ctx)).map(|_| ())
        },
    );
    assert!(
        matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_string_text")
    );
}

fn assert_association_name_refuses(
    source: &[u8],
    run: impl FnOnce(
        &crate::parse::Exchange,
        &mut cadmpeg_ir::document::CadIr,
        &crate::reader::index::CarrierIndex,
        &super::super::OwnedCarriers,
        &mut Vec<cadmpeg_ir::report::loss::LossNote>,
        &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<(), cadmpeg_core::CodecError>,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid association exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("root fits retained policy");
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let index = crate::reader::index::CarrierIndex::from_ir(&ir, &ctx)
        .expect("empty model has no carrier index entries");
    let owned = super::super::topology_owned_carriers(&ir, &index, &ctx)
        .expect("empty model has no owned carriers");
    let mut losses = Vec::new();
    assert!(matches!(
        run(&exchange, &mut ir, &index, &owned, &mut losses, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_string_text"
    ));
}

#[test]
fn geometric_set_member_name_refuses_retained_limit() {
    const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT('member',(0.,0.,0.));#2=GEOMETRIC_SET('',(#1));ENDSEC;END-ISO-10303-21;";
    assert_association_name_refuses(SOURCE, super::super::associate_free_geometric_set_members);
}

#[test]
fn representation_member_name_refuses_retained_limit() {
    const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT('member',(0.,0.,0.));#2=REPRESENTATION('',(#1),$);ENDSEC;END-ISO-10303-21;";
    assert_association_name_refuses(SOURCE, super::super::associate_free_representation_members);
}

#[test]
fn presentation_carrier_name_refuses_retained_limit() {
    const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT('member',(0.,0.,0.));ENDSEC;END-ISO-10303-21;";
    assert_association_name_refuses(SOURCE, |exchange, ir, index, owned, losses, ctx| {
        super::super::associate_presentation_carrier(
            exchange,
            ir,
            index,
            owned,
            (1, 1),
            losses,
            ctx,
        )
    });
}

fn assert_tessellated_curve_polyline(curve: &Curve, expected: &[(f64, f64, f64)]) {
    let Some(SolvedCurveGeometry::Polyline(polyline)) = curve.geometry.solved() else {
        panic!("expected tessellated curve to transfer as a polyline");
    };
    assert!(polyline.parameters().is_none());
    assert!(polyline.chordal_deflection().get().abs() < EPS_TESSELLATED_CURVE_POINT);
    assert_eq!(polyline.point_count(), expected.len());
    for (point, &(x, y, z)) in polyline.points().zip(expected) {
        assert!((point.x - x).abs() < EPS_TESSELLATED_CURVE_POINT);
        assert!((point.y - y).abs() < EPS_TESSELLATED_CURVE_POINT);
        assert!((point.z - z).abs() < EPS_TESSELLATED_CURVE_POINT);
    }
}

#[test]
pub(crate) fn procedural_step_geometry_round_trips_as_native_entities() {
    let source = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!(
                "../../../../tests/fixtures/ap242_geometry.p21"
            )),
            &DecodeOptions::default(),
        )
        .expect("decode procedural geometry");

    let mut bytes = Vec::new();
    let report = write_step(
        source.ir(),
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write procedural geometry");
    let text = String::from_utf8(bytes.clone()).expect("utf8 STEP");
    for entity in [
        "GEOMETRIC_SET",
        "TRIMMED_CURVE",
        "OFFSET_CURVE_3D",
        "SURFACE_OF_LINEAR_EXTRUSION",
        "SURFACE_OF_REVOLUTION",
        "OFFSET_SURFACE",
        "DEGENERATE_TOROIDAL_SURFACE",
    ] {
        assert!(text.contains(entity), "missing {entity}");
    }
    assert!(!report.losses.iter().any(|loss| loss
        .message
        .contains("reduced to their solved STEP carriers")));
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.message.contains("normalized to positive STEP radii")));

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode written procedural geometry");
    assert_eq!(decoded.ir().model.procedural_curves.len(), 3);
    assert_eq!(decoded.ir().model.procedural_surfaces.len(), 4);

    let bounded = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!(
                "../../../../tests/fixtures/ap242_geometric_set.p21"
            )),
            &DecodeOptions::default(),
        )
        .expect("decode curve-bounded surface");
    let mut bytes = Vec::new();
    let report = write_step(
        bounded.ir(),
        &mut bytes,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write curve-bounded surface");
    let text = String::from_utf8(bytes.clone()).expect("utf8 STEP");
    assert!(!text.contains("CURVE_BOUNDED_SURFACE"));
    assert!(!text.contains("GEOMETRIC_SET"));
    assert!(report.losses.iter().any(|loss| loss
        .message
        .contains("geometry carrier(s) were not written")));
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode written curve-bounded surface");
    assert!(!decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| matches!(
            surface.definition(),
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::CurveBounded { .. }
        )));
}

#[test]
fn complex_swept_surfaces_decode_named_partials() {
    let source = String::from_utf8(
        include_bytes!("../../../../tests/fixtures/ap242_geometry.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#23=SURFACE_OF_LINEAR_EXTRUSION('linear sweep',#8,#5);",
        "#23=(SURFACE() SURFACE_OF_LINEAR_EXTRUSION('linear sweep',#8,#5) SWEPT_SURFACE());",
    )
    .replace(
        "#25=SURFACE_OF_REVOLUTION('full revolution',#8,#24);",
        "#25=(SURFACE() SURFACE_OF_REVOLUTION('full revolution',#8,#24) SWEPT_SURFACE());",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex swept surfaces");

    assert!(decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| { surface.id.as_str() == "step:construction:swept_surface#23" }));
    assert!(decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| { surface.id.as_str() == "step:construction:swept_surface#25" }));
}

#[test]
fn linear_extrusion_surface_selects_endpoint_continuous_pcurve() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#28=PLANE('',#27);",
                "#69=VECTOR('',#9,1.);\n#28=SURFACE_OF_LINEAR_EXTRUSION('',#16,#69);",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode linear-extrusion sheet");

    assert_eq!(decoded.ir().model.pcurves.len(), 1);
    assert_eq!(
        decoded
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
    let surface_id = SurfaceId::mint("step:data:surface#28").expect("identity grammar");
    let index = ModelIndex::build(decoded.ir(), cadmpeg_ir::index::StandardIndex);
    assert_eq!(
        model_surface_point_by_id(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &index,
            &surface_id,
            10.0,
            0.0
        )
        .map(cadmpeg_ir::features::FinitePoint3::get),
        Ok(Point3::new(10.0, 0.0, 0.0))
    );
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveAssociationAmbiguous.kind()
            && loss.message.contains("curve #57")
            && loss.message.contains("no pcurve")
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn linear_extrusion_pcurve_uses_source_directrix_parameterization() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#28=PLANE('',#27);",
            "#69=VECTOR('',#9,1.);\n#70=CARTESIAN_POINT('',(0.,0.));\n#71=CARTESIAN_POINT('',(1.,0.));\n#28=SURFACE_OF_LINEAR_EXTRUSION('',#16,#69);",
        )
        .replace(
            "#54=LINE('',#51,#53);",
            "#54=B_SPLINE_CURVE_WITH_KNOTS('',1,(#70,#71),.UNSPECIFIED.,.F.,.F.,(2,2),(0.,1.),.PIECEWISE_BEZIER_KNOTS.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode source-parameterized linear-extrusion pcurve");

    assert_eq!(decoded.ir().model.pcurves.len(), 1);
    assert_eq!(
        decoded
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
    let used_id = decoded
        .ir()
        .model
        .coedges
        .iter()
        .flat_map(|coedge| coedge.pcurves.iter())
        .next()
        .expect("source-parameterized linear-extrusion pcurve use")
        .pcurve
        .clone();
    assert_eq!(used_id.as_str(), "step:data:pcurve#56");
    let used = decoded
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id == used_id)
        .expect("source-parameterized linear-extrusion pcurve");
    assert!(matches!(
        &used.geometry,
        cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs }
            if nurbs.degree() == 1
                && nurbs.control_points()
                    == [Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)]
    ));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveAssociationAmbiguous.kind()
            && loss.message.contains("curve #57")
            && loss.message.contains("no pcurve")
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn directrix_parameter_scale_witness_uses_line_vector_and_plane_angle_units() {
    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("data/tp03_directrix_parameter_scales.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode directrix parameter scale witness");

    let line_pcurve = decoded
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.as_str() == "step:data:pcurve#19")
        .expect("line-directrix pcurve");
    let PcurveGeometry::Line(line_pcurve) = &line_pcurve.geometry else {
        panic!("line-directrix witness did not retain a line pcurve");
    };
    let direction = line_pcurve.direction().as_raw();
    assert!((direction.u - 10.0).abs() < EPS_TP03_PARAMETER_SCALE);
    assert!(direction.v.abs() < EPS_TP03_PARAMETER_SCALE);

    let revolution_pcurve = decoded
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.as_str() == "step:data:pcurve#29")
        .expect("circle-directrix pcurve");
    let PcurveGeometry::Line(line_pcurve) = &revolution_pcurve.geometry else {
        panic!("circle-directrix witness did not retain a line pcurve");
    };
    let direction = line_pcurve.direction().as_raw();
    let degree_to_radian = std::f64::consts::PI / 180.0;
    assert!((direction.u - degree_to_radian).abs() < EPS_TP03_PARAMETER_SCALE);
    assert!((direction.v - degree_to_radian).abs() < EPS_TP03_PARAMETER_SCALE);

    assert_eq!(decoded.ir().model.procedural_surfaces.len(), 2);
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn linear_extrusion_surface_evaluates_a_nurbs_directrix() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#28=PLANE('',#27);",
            "#69=VECTOR('',#9,1.);\n#70=CARTESIAN_POINT('',(0.,0.,0.));\n#71=CARTESIAN_POINT('',(10.,0.,0.));\n#72=B_SPLINE_CURVE_WITH_KNOTS('',1,(#70,#71),.UNSPECIFIED.,.F.,.F.,(2,2),(0.,10.),.PIECEWISE_BEZIER_KNOTS.);\n#28=SURFACE_OF_LINEAR_EXTRUSION('',#72,#69);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode NURBS linear-extrusion sheet");

    let surface_id = SurfaceId::mint("step:data:surface#28").expect("identity grammar");
    let index = ModelIndex::build(decoded.ir(), cadmpeg_ir::index::StandardIndex);
    assert_eq!(
        model_surface_point_by_id(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &index,
            &surface_id,
            5.0,
            0.0
        )
        .map(cadmpeg_ir::features::FinitePoint3::get),
        Ok(Point3::new(5.0, 0.0, 0.0))
    );
    let partials = model_surface_partials_by_id(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
        &index,
        &surface_id,
        5.0,
        0.0,
    )
    .expect("NURBS linear sweep partials");
    assert!((partials.du.x - 1.0).abs() < 1.0e-12);
    assert!(partials.du.y.abs() < 1.0e-12);
    assert!(partials.du.z.abs() < 1.0e-12);
    assert_eq!(partials.dv, Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(decoded.ir().model.pcurves.len(), 1);
    assert_eq!(
        decoded
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
fn swept_surface_chart_ignores_pcurve_population() {
    let check = |source: &[u8], expected_pcurve_u: f64| {
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("decode swept-surface chart witness");
        let surface_id = SurfaceId::mint("step:data:surface#9").expect("identity grammar");
        let index = ModelIndex::build(decoded.ir(), cadmpeg_ir::index::StandardIndex);
        assert_eq!(
            model_surface_point_by_id(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &index,
                &surface_id,
                5.0,
                0.0
            )
            .map(cadmpeg_ir::features::FinitePoint3::get),
            Ok(Point3::new(5.0, 0.0, 0.0))
        );
        let partials = model_surface_partials_by_id(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &index,
            &surface_id,
            5.0,
            0.0,
        )
        .expect("swept-surface chart partials");
        assert_eq!(partials.du, Vector3::new(1.0, 0.0, 0.0));
        assert_eq!(partials.dv, Vector3::new(0.0, 0.0, 1.0));
        let pcurve = decoded
            .ir()
            .model
            .pcurves
            .iter()
            .find(|pcurve| pcurve.id.as_str() == "step:data:pcurve#22")
            .expect("swept-surface pcurve");
        assert!(matches!(&pcurve.geometry, PcurveGeometry::Line(line_pcurve)
        if {
            let direction = line_pcurve.direction().as_raw();
            direction.u == expected_pcurve_u && direction.v == 0.0
        }));
    };

    check(include_bytes!("data/pc03_chart_valid.p21"), 10.0);
    check(include_bytes!("data/pc03_chart_population.p21"), 100.0);
}

#[test]
fn surface_of_revolution_selects_profile_parameter_pcurve() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#28=PLANE('',#27);",
                "#69=AXIS1_PLACEMENT('',#3,#9);\n#28=SURFACE_OF_REVOLUTION('',#16,#69);",
            )
            .replace("#52=DIRECTION('',(1.,0.));", "#52=DIRECTION('',(0.,1.));");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode surface of revolution sheet");

    let surface_id = SurfaceId::mint("step:data:surface#28").expect("identity grammar");
    let index = ModelIndex::build(decoded.ir(), cadmpeg_ir::index::StandardIndex);
    assert_eq!(
        model_surface_point_by_id(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &index,
            &surface_id,
            0.0,
            10.0
        )
        .map(cadmpeg_ir::features::FinitePoint3::get),
        Ok(Point3::new(10.0, 0.0, 0.0))
    );
    assert_eq!(decoded.ir().model.pcurves.len(), 1);
    assert_eq!(
        decoded
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveAssociationAmbiguous.kind()
            && loss.message.contains("curve #57")
            && loss.message.contains("no pcurve")
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn reversed_step_ellipse_axes_are_canonicalized() {
    use cadmpeg_ir::geometry::SolvedCurveGeometry;

    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap242_geometry.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace("#10=ELLIPSE('',#6,6.,2.);", "#10=ELLIPSE('',#6,2.,6.);");
    let result = StepCodec::default()
        .decode(
            &mut Cursor::new(source.as_bytes()),
            &DecodeOptions::default(),
        )
        .expect("decode reversed ellipse");
    let ellipse = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#10")
        .expect("ellipse carrier");
    assert!(
        matches!(ellipse.geometry.solved(), Some(SolvedCurveGeometry::Ellipse(ellipse_curve))
                if {
                    let major_radius = ellipse_curve.major_radius().get();
        let minor_radius = ellipse_curve.minor_radius().get();
                    major_radius == 6.0 && minor_radius == 2.0
                })
    );
}

#[test]
fn reversed_step_ellipse_trim_preserves_source_parameterization() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#5=ELLIPSE('',#4,2.,6.);
#6=TRIMMED_CURVE('',#5,(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(1.5707963267948966)),.T.,.PARAMETER.);
#7=GEOMETRIC_CURVE_SET('',(#6));
#8=SHAPE_REPRESENTATION('',(#7),$);",
    );
    let index = ModelIndex::build(result.ir(), cadmpeg_ir::index::StandardIndex);
    let start = model_curve_point_by_id(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
        &index,
        &CurveId::mint("step:data:curve#6").expect("identity grammar"),
        0.0,
    )
    .expect("trimmed ellipse start");
    let end = model_curve_point_by_id(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
        &index,
        &CurveId::mint("step:data:curve#6").expect("identity grammar"),
        std::f64::consts::FRAC_PI_2,
    )
    .expect("trimmed ellipse end");
    assert!((start.x - 2.0).abs() < 1.0e-12);
    assert!(start.y.abs() < 1.0e-12);
    assert!(end.x.abs() < 1.0e-12);
    assert!((end.y - 6.0).abs() < 1.0e-12);
    assert!(result.ir().model.procedural_curves.iter().any(|curve| {
        match curve.definition() { cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset(matched_payload) => matches!((&matched_payload.parameter_range().endpoints(),), ([start, end],) if curve.id.as_str() == "step:construction:trimmed_curve#6"
                && (*start + std::f64::consts::FRAC_PI_2).abs() < 1.0e-12
                && end.abs() < 1.0e-12), _ => false }
    }));
}

#[test]
fn ellipse_witness_preserves_source_axes_through_canonical_carriers() {
    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("data/pc07_ellipse_canonicalization.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode ellipse canonicalization witness");

    let reversed = decoded
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#9")
        .expect("reversed ellipse");
    assert!(matches!(reversed
        .geometry
        .solved(), Some(SolvedCurveGeometry::Ellipse(ellipse_curve))
            if {
                let major_direction = ellipse_curve.frame().reference().as_raw();
    let major_radius = ellipse_curve.major_radius().get();
    let minor_radius = ellipse_curve.minor_radius().get();
                *major_direction == Vector3::new(0.0, 1.0, 0.0)
                    && major_radius == 6.0
                    && minor_radius == 2.0
            }));

    let ordered = decoded
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#10")
        .expect("ordered ellipse");
    assert!(
        matches!(ordered.geometry.solved(), Some(SolvedCurveGeometry::Ellipse(ellipse_curve))
                if {
                    let major_direction = ellipse_curve.frame().reference().as_raw();
        let major_radius = ellipse_curve.major_radius().get();
        let minor_radius = ellipse_curve.minor_radius().get();
                    *major_direction == Vector3::new(1.0, 0.0, 0.0)
                        && major_radius == 6.0
                        && minor_radius == 2.0
                })
    );

    for (curve_id, expected_range) in [
        (13u64, [-std::f64::consts::FRAC_PI_2, 0.0]),
        (14u64, [-std::f64::consts::FRAC_PI_2, 0.0]),
        (18u64, [-std::f64::consts::FRAC_PI_2, 0.0]),
        (20u64, [-std::f64::consts::FRAC_PI_2, 0.0]),
    ] {
        let construction_id =
            ProceduralCurveId::from(ids::construction(kind!("trimmed_curve"), curve_id));
        let construction = decoded
            .ir()
            .model
            .procedural_curves
            .iter()
            .find(|curve| curve.id == construction_id)
            .expect("trimmed ellipse construction");
        assert!(match construction.definition() {
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset(matched_payload) =>
                matches!((&matched_payload.parameter_range().endpoints(),), (parameter_range,) if parameter_range
                .iter()
                .zip(expected_range.iter())
                .all(|(actual, expected)| (*actual - *expected).abs() < 1.0e-12)),
            _ => false,
        });
    }

    let numeric_start = model_curve_point_by_id(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
        &ModelIndex::build(decoded.ir(), cadmpeg_ir::index::StandardIndex),
        &CurveId::mint("step:data:curve#13").expect("identity grammar"),
        0.0,
    )
    .expect("numeric trim start");
    assert!((numeric_start.x - 2.0).abs() < 1.0e-12);
    assert!(numeric_start.y.abs() < 1.0e-12);
    let cartesian_end = model_curve_point_by_id(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
        &ModelIndex::build(decoded.ir(), cadmpeg_ir::index::StandardIndex),
        &CurveId::mint("step:data:curve#14").expect("identity grammar"),
        std::f64::consts::FRAC_PI_2,
    )
    .expect("Cartesian trim end");
    assert!(cartesian_end.x.abs() < 1.0e-12);
    assert!((cartesian_end.y - 6.0).abs() < 1.0e-12);

    let index = ModelIndex::build(decoded.ir(), cadmpeg_ir::index::StandardIndex);
    let replica_start = model_curve_point_by_id(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
        &index,
        &CurveId::mint("step:data:curve#17").expect("identity grammar"),
        -std::f64::consts::FRAC_PI_2,
    )
    .expect("replica start");
    assert!((replica_start.x - 12.0).abs() < 1.0e-12);
    assert!(replica_start.y.abs() < 1.0e-12);

    assert!(decoded.report().losses.is_empty());
}

#[test]
fn annotation_plane_keeps_its_neutral_plane_reachable() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#69=AXIS2_PLACEMENT_3D('',#3,#9,#10);\n#70=PLANE('',#69);\n#71=ANNOTATION_PLANE('',(),#70,());\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode annotation plane");
    let plane = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#70")
        .expect("annotation plane surface");
    assert_eq!(
        plane
            .source_object
            .as_ref()
            .map(|association| association.object_id.as_str()),
        Some("#71")
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(!validation.findings.iter().any(|finding| {
        finding.check == cadmpeg_ir::report::check::Check::CarrierReachability
            && finding.entity.as_deref() == Some("step:data:surface#70")
    }));
}

#[test]
fn conical_surface_accepts_a_finite_zero_half_angle() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#5=CONICAL_SURFACE('',#4,0.,0.);
#6=GEOMETRIC_SET('',(#5));
#7=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#6),#8);
#8=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));",
    );

    assert!(result.ir().model.surfaces.iter().any(|surface| {
        matches!(surface.geometry.solved(), Some(SolvedSurfaceGeometry::Cone(cone_surface))
        if {
            let half_angle = cone_surface.half_angle().get();
            half_angle == 0.0
        })
    }));
    assert!(result.report().losses.iter().all(|loss| !loss
        .message
        .contains("CONICAL_SURFACE #5 has invalid geometry")));
}

#[test]
fn conical_surface_accepts_a_negative_semi_angle() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#5=CONICAL_SURFACE('',#4,5.,-0.7155849933176748);
#6=GEOMETRIC_SET('',(#5));
#7=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#6),#8);
#8=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));",
    );

    assert!(result.ir().model.surfaces.iter().any(|surface| {
        matches!(surface.geometry.solved(), Some(SolvedSurfaceGeometry::Cone(cone_surface))
        if {
            let axis = *cone_surface.frame().axis().as_raw();
            cone_surface.half_angle().get() == -0.715_584_993_317_674_8
                && cone_surface.radius().get() == 5.0
                && (axis.x, axis.y, axis.z) == (0.0, 0.0, 1.0)
        })
    }));
    assert!(result.report().losses.iter().all(|loss| !loss
        .message
        .contains("CONICAL_SURFACE #5 has invalid geometry")));
}

#[test]
fn complex_geometry_instances_decode_named_partials() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#27=AXIS2_PLACEMENT_3D('',#3,#9,#10);",
                "#27=(AXIS2_PLACEMENT_3D('',#3,#9,#10) PLACEMENT());",
            )
            .replace(
                "#28=PLANE('',#27);",
                "#28=(GEOMETRIC_REPRESENTATION_ITEM() PLANE('',#27) SURFACE());",
            )
            .replace("#16=LINE('',#3,#13);", "#16=(CURVE() LINE('',#3,#13));")
            .replace("#54=LINE('',#51,#53);", "#54=(CURVE() LINE('',#51,#53));")
            .replace(
                "#56=PCURVE('',#28,#55);",
                "#56=(CURVE() PCURVE('',#28,#55));",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex geometry instances");

    assert!(decoded.ir().model.curves.iter().any(|curve| {
        curve.id.as_str() == "step:data:curve#16"
            && matches!(curve.geometry.solved(), Some(SolvedCurveGeometry::Line(_)))
    }));
    assert!(decoded.ir().model.surfaces.iter().any(|surface| {
        surface.id.as_str() == "step:data:surface#28"
            && matches!(
                surface.geometry.solved(),
                Some(SolvedSurfaceGeometry::Plane(_))
            )
    }));
    assert_eq!(decoded.ir().model.pcurves.len(), 1);
    assert!(matches!(
        &decoded.ir().model.pcurves[0].geometry,
        cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(_)
    ));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_points_and_directions_decode_named_partials() {
    let source =
        String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#3=CARTESIAN_POINT('',(0.,0.,0.));",
                "#3=(CARTESIAN_POINT('',(0.,0.,0.)) GEOMETRIC_REPRESENTATION_ITEM() POINT(''));",
            )
            .replace(
                "#9=DIRECTION('',(0.,0.,1.));",
                "#9=(DIRECTION('',(0.,0.,1.)) GEOMETRIC_REPRESENTATION_ITEM());",
            );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex points and directions");

    assert_eq!(decoded.ir().model.vertices.len(), 3);
    assert!(decoded.ir().model.surfaces.iter().any(|surface| {
        surface.id.as_str() == "step:data:surface#28"
            && matches!(
                surface.geometry.solved(),
                Some(SolvedSurfaceGeometry::Plane(_))
            )
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn geometric_set_owns_catias_composite_trimmed_curve_chain() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(17.,23.,13.));
#2=CARTESIAN_POINT('',(21.8769469654,17.9785073637,13.));
#3=CARTESIAN_POINT('',(21.8769469654,28.0214926363,13.));
#4=DIRECTION('',(0.,0.,1.));
#5=AXIS2_PLACEMENT_3D('',#1,#4,$);
#6=CIRCLE('',#5,7.);
#7=TRIMMED_CURVE('',#6,(#2),(#3),.T.,.CARTESIAN.);
#8=COMPOSITE_CURVE_SEGMENT(.DISCONTINUOUS.,.T.,#7);
#9=COMPOSITE_CURVE('',(#8),.U.);
#10=GEOMETRIC_SET('NONE',(#9));
#11=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#10),#12);
#12=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));",
    );

    let composite = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#9")
        .expect("composite curve");
    let source = composite
        .source_object
        .as_ref()
        .expect("geometric-set owner");
    assert_eq!(source.format, cadmpeg_ir::CodecFormat::Step);
    assert_eq!(source.object_id, "#9");
    assert_eq!(source.name, None);

    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn geometric_surface_representation_salvages_valid_sibling_sets() {
    let source = String::from_utf8(
        include_bytes!("../../../../tests/fixtures/ap242_geometric_set.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#13=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#12),#2);",
        "#13=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#12,#99),#2);\n#99=UNSUPPORTED_SET('',());",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode geometric set with malformed sibling");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| { loss.message.contains("skipped non-set member #99") }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_shape_representation_is_typed_for_free_representation_items() {
    let decoded = decode_inline(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));
#3=CARTESIAN_POINT('free point',(1.,2.,3.));
#4=(REPRESENTATION('free shape',(#3),#2) SHAPE_REPRESENTATION());",
    );

    assert_eq!(decoded.ir().model.points.len(), 1);
    assert_eq!(
        decoded.ir().model.points[0]
            .source_object
            .as_ref()
            .and_then(|source| source.name.as_deref()),
        Some("free point")
    );
    assert!(!decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.as_str() == "step:data:shape_representation#4"));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn unreferenced_curve_is_associated_as_free_geometry() {
    let decoded = decode_inline(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT($,.METRE.));
#2=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));
#3=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#2)) REPRESENTATION_CONTEXT('model','3D'));
#10=CARTESIAN_POINT('',(0.,0.,0.));
#11=DIRECTION('',(0.,0.,1.));
#12=DIRECTION('',(1.,0.,0.));
#13=AXIS2_PLACEMENT_3D('',#10,#11,#12);
#14=CIRCLE('',#13,2.);",
    );
    let curve = decoded
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#14")
        .expect("unreferenced circle");
    assert_eq!(
        curve
            .source_object
            .as_ref()
            .map(|source| source.object_id.as_str()),
        Some("#14")
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn apll_leader_points_transfer_coordinates_and_keep_source_records() {
    let decoded = decode_inline(
        "#20=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#21=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));
#22=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#20,#21)) REPRESENTATION_CONTEXT('model','3D'));
#10=CARTESIAN_POINT('',(0.,0.,0.));
#11=DIRECTION('',(0.,0.,1.));
#12=DIRECTION('',(1.,0.,0.));
#13=AXIS2_PLACEMENT_3D('',#10,#11,#12);
#14=PLANE('',#13);
#1=APLL_POINT('leader',(1.,2.,3.),.NONE.);
#2=APLL_POINT_WITH_SURFACE('surface',(4.,5.,6.),.POSITIVE_ARROWHEAD.,#14);
#3=(APLL_POINT(.NONE.) CARTESIAN_POINT((7.,8.,9.)) GEOMETRIC_REPRESENTATION_ITEM() POINT() REPRESENTATION_ITEM('complex leader'));
#4=APLL_POINT((10.,11.,12.),.NONE.);
#5=ANNOTATION_TO_MODEL_LEADER_LINE('model',(#1,#2,#3));
#6=ANNOTATION_TO_ANNOTATION_LEADER_LINE('annotation',(#4));
#7=AUXILIARY_LEADER_LINE('auxiliary',(#1,#2),#5);",
    );

    assert_eq!(decoded.ir().model.points.len(), 4);
    for (id, expected) in [
        ("#1", (1.0, 2.0, 3.0)),
        ("#2", (4.0, 5.0, 6.0)),
        ("#3", (7.0, 8.0, 9.0)),
        ("#4", (10.0, 11.0, 12.0)),
    ] {
        let point = decoded
            .ir()
            .model
            .points
            .iter()
            .find(|point| {
                point
                    .source_object
                    .as_ref()
                    .is_some_and(|source| source.object_id == id)
            })
            .unwrap_or_else(|| panic!("missing APLL point {id}"));
        assert!((point.position().get().x - expected.0).abs() < EPS_APLL_POINT);
        assert!((point.position().get().y - expected.1).abs() < EPS_APLL_POINT);
        assert!((point.position().get().z - expected.2).abs() < EPS_APLL_POINT);
    }
    let named_point = decoded
        .ir()
        .model
        .points
        .iter()
        .find(|point| {
            point
                .source_object
                .as_ref()
                .is_some_and(|source| source.object_id == "#1")
        })
        .expect("named APLL point");
    assert_eq!(
        named_point
            .source_object
            .as_ref()
            .and_then(|source| source.name.as_deref()),
        Some("leader")
    );
    let complex_point = decoded
        .ir()
        .model
        .points
        .iter()
        .find(|point| {
            point
                .source_object
                .as_ref()
                .is_some_and(|source| source.object_id == "#3")
        })
        .expect("complex APLL point");
    assert_eq!(
        complex_point
            .source_object
            .as_ref()
            .and_then(|source| source.name.as_deref()),
        Some("complex leader")
    );
    let unknowns = decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena");
    for (id, kind) in [
        (1, "apll_point"),
        (2, "apll_point_with_surface"),
        (3, "apll_point"),
        (4, "apll_point"),
        (5, "annotation_to_model_leader_line"),
        (6, "annotation_to_annotation_leader_line"),
        (7, "auxiliary_leader_line"),
    ] {
        assert!(
            unknowns.iter().any(|record| {
                (id == 3 && record.id.as_str().ends_with("#3") && record.id.as_str().contains(kind))
                    || (id != 3 && record.id.as_str() == format!("step:data:{kind}#{id}"))
            }),
            "missing retained source record #{id}"
        );
    }
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn invalid_apll_leader_point_stays_source_native() {
    let decoded = decode_inline(
        "#1=APLL_POINT('invalid',(1.,2.),.NONE.);
#2=ANNOTATION_TO_MODEL_LEADER_LINE('invalid',(#1));",
    );

    assert!(decoded.ir().model.points.is_empty());
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.as_str() == "step:data:apll_point#1"));
}

#[test]
fn tessellated_curve_set_transfers_each_line_strip_as_a_polyline() {
    let decoded = decode_inline(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.CENTI.,.METRE.));
#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));
#3=COORDINATES_LIST('',3,((1.,0.,0.),(2.,0.,0.),(2.,1.,0.),(3.,1.,0.)));
#4=TESSELLATED_CURVE_SET('display curve',#3,((1,2,3),(3,4)));
#5=(REPRESENTATION_ITEM('complex curve') TESSELLATED_CURVE_SET(#3,((4,1))));
#6=REPRESENTATION('display',(#4,#5),#2);",
    );

    let first = decoded
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#4")
        .expect("first tessellated line strip");
    let second = decoded
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#4-strip-1")
        .expect("second tessellated line strip");
    let complex = decoded
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#5")
        .expect("complex tessellated line strip");
    assert_tessellated_curve_polyline(
        first,
        &[(10.0, 0.0, 0.0), (20.0, 0.0, 0.0), (20.0, 10.0, 0.0)],
    );
    assert_tessellated_curve_polyline(second, &[(20.0, 10.0, 0.0), (30.0, 10.0, 0.0)]);
    assert_tessellated_curve_polyline(complex, &[(30.0, 10.0, 0.0), (10.0, 0.0, 0.0)]);
    assert_eq!(
        first
            .source_object
            .as_ref()
            .map(|source| source.object_id.as_str()),
        Some("#4")
    );
    assert_eq!(
        first
            .source_object
            .as_ref()
            .and_then(|source| source.name.as_deref()),
        Some("display curve")
    );
    assert!(!decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| {
            record.id.as_str().ends_with("#3")
                || record.id.as_str().ends_with("#4")
                || record.id.as_str().ends_with("#5")
        }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn tessellated_curve_set_with_invalid_indices_stays_source_native() {
    let decoded = decode_inline(
        "#1=COORDINATES_LIST('',3,((1.,0.,0.),(2.,0.,0.)));
#2=TESSELLATED_CURVE_SET(#1,((0,1)));",
    );

    assert!(!decoded.ir().model.curves.iter().any(|curve| {
        curve
            .source_object
            .as_ref()
            .is_some_and(|source| source.object_id == "#2")
    }));
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.as_str().ends_with("#2")));
}
