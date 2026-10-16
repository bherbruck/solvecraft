// SPDX-License-Identifier: Apache-2.0
//! STEP geometry, pcurve, NURBS, unit, replica, and trim tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use crate::ids::kind;
use std::fmt::Write as _;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::eval::{model_curve_point_by_id, model_surface_point_by_id};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use crate::export::write_step;
use crate::ids;
use crate::loss::StepLossCode;
use crate::test_support::exchange::decode_inline;
use crate::{StepCodec, StepSchema, StepWriteOptions};

#[test]
fn rigid_transform_rejects_reflections() {
    assert!(!Transform::affine([
        [-1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .unwrap()
    .is_proper_rigid());
}

#[test]
fn placement_axis_normalization_preserves_extreme_finite_directions() {
    for vector in [
        Vector3::new(f64::MAX, 0.0, 0.0),
        Vector3::new(2.0_f64.powi(-800), 0.0, 0.0),
        Vector3::new(f64::from_bits(1), 0.0, 0.0),
    ] {
        assert_eq!(
            super::super::normalize(vector).map(|unit| *unit.as_raw()),
            Some(Vector3::new(1.0, 0.0, 0.0))
        );
    }
    assert!(super::super::normalize(Vector3::new(0.0, 0.0, 0.0)).is_none());
    assert!(super::super::normalize(Vector3::new(f64::NAN, 1.0, 0.0)).is_none());
}

#[test]
fn placement_projection_keeps_reciprocal_normalization_bits() {
    let old_normalize = |vector: Vector3| {
        let scale = vector.x.abs().max(vector.y.abs()).max(vector.z.abs());
        let scaled = Vector3::new(vector.x / scale, vector.y / scale, vector.z / scale);
        scaled.scale(1.0 / scaled.norm())
    };
    let axis = Vector3::new(3.0, 4.0, 5.0);
    let reference = Vector3::new(4.0, 2.0, 1.0);
    let admitted_axis = super::super::normalize(axis).expect("axis");
    let admitted_reference = super::super::normalize(reference).expect("reference");
    let old_axis = old_normalize(old_normalize(axis));
    let old_reference = old_normalize(old_normalize(reference));
    let expected = old_normalize(old_reference - old_axis.scale(old_reference.dot(old_axis)));
    let actual =
        super::super::project_axis(admitted_reference, admitted_axis).expect("projected reference");
    assert_eq!(actual.as_raw().x.to_bits(), expected.x.to_bits());
    assert_eq!(actual.as_raw().y.to_bits(), expected.y.to_bits());
    assert_eq!(actual.as_raw().z.to_bits(), expected.z.to_bits());
}

#[test]
fn placement_2d_axes_keep_hypot_normalization_bits() {
    use cadmpeg_ir::units::HypotDirection2;
    let old_normalize = |u: f64, v: f64| {
        let length = u.hypot(v);
        Point2::new(u / length, v / length)
    };
    let source_x = old_normalize(3.0, 4.0);
    let source_y = old_normalize(2.0, 1.0);
    let old_x = old_normalize(source_x.u, source_x.v);
    let old_y = old_normalize(source_y.u, source_y.v);
    let mut old_perpendicular = Point2::new(-old_x.v, old_x.u);
    if old_y.u * old_perpendicular.u + old_y.v * old_perpendicular.v < 0.0 {
        old_perpendicular = Point2::new(-old_perpendicular.u, -old_perpendicular.v);
    }
    let x_axis = HypotDirection2::normalized_with_length([3.0, 4.0])
        .expect("x direction")
        .0;
    let y_axis = HypotDirection2::normalized_with_length([2.0, 1.0])
        .expect("y direction")
        .0;
    let (actual_x, actual_y) = super::super::base_axis_2d(Some(x_axis), Some(y_axis));
    let [actual_x_u, actual_x_v] = actual_x.get();
    let [actual_y_u, actual_y_v] = actual_y.get();
    assert_eq!(actual_x_u.to_bits(), old_x.u.to_bits());
    assert_eq!(actual_x_v.to_bits(), old_x.v.to_bits());
    assert_eq!(actual_y_u.to_bits(), old_perpendicular.u.to_bits());
    assert_eq!(actual_y_v.to_bits(), old_perpendicular.v.to_bits());
}

#[test]
fn placement_reference_is_projected_and_angular_trims_use_context_units() {
    let result = decode_inline(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#2=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));
#3=PLANE_ANGLE_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(0.017453292519943295),#2);
#4=(CONVERSION_BASED_UNIT('degree',#3) NAMED_UNIT(*) PLANE_ANGLE_UNIT());
#5=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#4)) REPRESENTATION_CONTEXT('model','3D'));
#10=CARTESIAN_POINT('',(0.,0.,0.));
#11=DIRECTION('',(0.,0.,1.));
#12=DIRECTION('',(1.,0.,1.));
#13=AXIS2_PLACEMENT_3D('',#10,#11,#12);
#14=CIRCLE('',#13,2.);
#15=TRIMMED_CURVE('',#14,(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(90.)),.T.,.PARAMETER.);
#16=GEOMETRIC_CURVE_SET('',(#15));
#17=SHAPE_REPRESENTATION('',(#16),#5);",
    );
    let circle = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#14")
        .expect("circle");
    let Some(SolvedCurveGeometry::Circle(circle_curve)) = circle.geometry.solved() else {
        panic!("decoded carrier is not a circle")
    };
    let axis = *circle_curve.frame().axis().as_raw();
    let ref_direction = *circle_curve.frame().reference().as_raw();
    let dot = axis.x * ref_direction.x + axis.y * ref_direction.y + axis.z * ref_direction.z;
    assert!(dot.abs() < 1.0e-12);
    assert!(result
        .ir()
        .model
        .procedural_curves
        .iter()
        .any(|curve| match curve.definition() { cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset(matched_payload) => matches!((&matched_payload.parameter_range().endpoints(),), ([start, end],) if start.abs() < 1.0e-12 && (end - std::f64::consts::FRAC_PI_2).abs() < 1.0e-12), _ => false }));
    assert!(result.report().losses.iter().all(|loss| {
        !loss
            .message
            .contains("LINE #14 parameter scale did not resolve")
    }));
}

#[test]
fn omitted_placement_reference_uses_the_first_projected_axis() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.6,0.8,0.));
#3=AXIS2_PLACEMENT_3D('',#1,#2,$);
#4=CIRCLE('',#3,2.);
#5=GEOMETRIC_CURVE_SET('',(#4));
#6=SHAPE_REPRESENTATION('',(#5),$);",
    );
    let circle = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#4")
        .expect("circle");
    let Some(SolvedCurveGeometry::Circle(circle_curve)) = circle.geometry.solved() else {
        panic!("decoded carrier is not a circle");
    };
    let ref_direction = *circle_curve.frame().reference().as_raw();
    assert!((ref_direction.x - 0.8).abs() < 1.0e-12);
    assert!((ref_direction.y + 0.6).abs() < 1.0e-12);
    assert!(ref_direction.z.abs() < 1.0e-12);
}

#[test]
fn near_parallel_omitted_reference_uses_a_stable_projected_axis() {
    let result = decode_inline(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#2=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));
#3=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#2)) REPRESENTATION_CONTEXT('model','3D'));
#10=CARTESIAN_POINT('',(0.,0.,0.));
#11=DIRECTION('',(-1.,0.0000000612905015206,0.0000000692801624183));
#12=AXIS2_PLACEMENT_3D('',#10,#11,$);
#13=CIRCLE('',#12,2.);
#14=GEOMETRIC_CURVE_SET('',(#13));
#15=SHAPE_REPRESENTATION('',(#14),#3);",
    );
    let circle = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#13")
        .expect("circle");
    let Some(SolvedCurveGeometry::Circle(circle_curve)) = circle.geometry.solved() else {
        panic!("decoded carrier is not a circle");
    };
    let axis = *circle_curve.frame().axis().as_raw();
    let ref_direction = *circle_curve.frame().reference().as_raw();
    let dot = axis.x * ref_direction.x + axis.y * ref_direction.y + axis.z * ref_direction.z;
    assert!(ref_direction.y > 0.999_999_999);
    assert!(dot.abs() < 1.0e-12);
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn placement_reference_witness_covers_default_axes_and_invalid_parallel_input() {
    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("data/pc06_placement_reference.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode placement reference witness");

    let expected = [
        ("#6", (0.8, -0.6, 0.0)),
        ("#9", (0.0, 1.0, 0.0)),
        ("#12", (0.0, 1.0, 0.0)),
    ];
    for (source_id, (x, y, z)) in expected {
        let curve = decoded
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id.as_str() == format!("step:data:curve{source_id}"))
            .expect("witness circle");
        let Some(SolvedCurveGeometry::Circle(circle_curve)) = curve.geometry.solved() else {
            panic!("witness carrier is not a circle");
        };
        let ref_direction = *circle_curve.frame().reference().as_raw();
        assert!((ref_direction.x - x).abs() < 1.0e-12);
        assert!((ref_direction.y - y).abs() < 1.0e-12);
        assert!((ref_direction.z - z).abs() < 1.0e-12);
    }

    let near_axis = decoded
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#15")
        .expect("near-axis witness circle");
    let Some(SolvedCurveGeometry::Circle(circle_curve)) = near_axis.geometry.solved() else {
        panic!("near-axis witness carrier is not a circle");
    };
    let ref_direction = *circle_curve.frame().reference().as_raw();
    assert!(ref_direction.y > 0.999_999_999);

    let parallel_reference = decoded
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#18")
        .expect("parallel-reference witness circle");
    let Some(SolvedCurveGeometry::Circle(circle_curve)) = parallel_reference.geometry.solved()
    else {
        panic!("parallel-reference witness carrier is not a circle");
    };
    let ref_direction = *circle_curve.frame().reference().as_raw();
    assert!((ref_direction.x - 1.0).abs() < 1.0e-12);
    assert!(ref_direction.y.abs() < 1.0e-12);
    assert!(ref_direction.z.abs() < 1.0e-12);
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::PlacementReferenceInferred.kind()
            && loss.message.contains("AXIS2_PLACEMENT_3D #17")
    }));
}

#[test]
fn parallel_axis_reference_direction_is_reported_and_inferred() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(0.,0.,2.));
#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#5=CIRCLE('',#4,2.);
#6=GEOMETRIC_CURVE_SET('',(#5));
#7=SHAPE_REPRESENTATION('',(#6),$);",
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::PlacementReferenceInferred.kind()
            && loss.message.contains("AXIS2_PLACEMENT_3D #4")
    }));
}

#[test]
fn trimmed_curve_replica_keeps_parent_parameterization_for_both_selectors() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(1.,0.,0.));
#3=DIRECTION('',(0.,1.,0.));
#4=DIRECTION('',(0.,0.,1.));
#5=VECTOR('',#2,2.);
#6=LINE('',#1,#5);
#7=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#2,#3,#1,3.,#4);
#8=CURVE_REPLICA('',#6,#7);
#9=TRIMMED_CURVE('',#8,(PARAMETER_VALUE(1.)),(PARAMETER_VALUE(2.)),.T.,.PARAMETER.);
#10=CARTESIAN_POINT('',(6.,0.,0.));
#11=CARTESIAN_POINT('',(12.,0.,0.));
#12=TRIMMED_CURVE('',#8,(#10),(#11),.T.,.CARTESIAN.);
#13=GEOMETRIC_CURVE_SET('',(#9,#12));
#14=SHAPE_REPRESENTATION('',(#13),$);",
    );

    for (curve_id, expected) in [(9u64, [2.0, 4.0]), (12u64, [2.0, 4.0])] {
        let construction_id = ids::construction(kind!("trimmed_curve"), curve_id);
        assert!(result.ir().model.procedural_curves.iter().any(|curve| {
            curve.id.as_str() == construction_id.as_str()
                && match curve.definition() { cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset(matched_payload) => matches!((&matched_payload.parameter_range().endpoints(),), (parameter_range,) if *parameter_range == expected), _ => false }
        }));
    }

    assert!(result.ir().model.procedural_curves.iter().any(|curve| {
        matches!(
            curve.definition(),
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Replica { source, .. }
                if result.ir().model.procedural_curve_owner(&curve.id).map(CurveId::as_str)
                    == Some("step:data:curve#8")
                    && source.as_str() == "step:data:curve#6"
        )
    }));
    let index = ModelIndex::build(result.ir(), cadmpeg_ir::index::StandardIndex);
    assert_eq!(
        model_curve_point_by_id(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &index,
            &CurveId::mint("step:data:curve#9").expect("identity grammar"),
            0.0
        )
        .map(cadmpeg_ir::features::FinitePoint3::get),
        Ok(Point3::new(6.0, 0.0, 0.0))
    );
    assert_eq!(
        model_curve_point_by_id(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &index,
            &CurveId::mint("step:data:curve#9").expect("identity grammar"),
            2.0
        )
        .map(cadmpeg_ir::features::FinitePoint3::get),
        Ok(Point3::new(12.0, 0.0, 0.0))
    );

    let mut output = Vec::new();
    write_step(
        result.ir(),
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write trimmed replica");
    let text = String::from_utf8(output.clone()).expect("STEP output is UTF-8");
    assert!(text.contains("CURVE_REPLICA"));
    assert!(text.contains("TRIMMED_CURVE"));
    let round_trip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode trimmed replica");
    assert!(round_trip.ir().model.procedural_curves.iter().any(|curve| {
        matches!(
            curve.definition(),
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Replica { source, .. }
                if source.as_str().starts_with("step:data:curve#")
        )
    }));
}

#[test]
fn transformed_curves_and_surfaces_round_trip_through_step_replicas() {
    let transform = Transform::affine([
        [0.0, -2.0, 0.0, 10.0],
        [2.0, 0.0, 0.0, 20.0],
        [0.0, 0.0, 2.0, 30.0],
    ])
    .expect("affine transform");
    let curve_geometry = SolvedCurveGeometry::Transformed(
        cadmpeg_ir::geometry::PlacedCurve::try_new(
            Box::new(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    Point3::new(1.0, 2.0, 3.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            )),
            transform,
        )
        .expect("placed curve"),
    );
    let surface_geometry = SolvedSurfaceGeometry::Transformed(
        cadmpeg_ir::geometry::PlacedSurface::try_new(
            Box::new(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(1.0, 2.0, 3.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            )),
            transform,
        )
        .expect("placed surface"),
    );
    let mut source = CadIr::empty();
    source.model.curves.push(Curve {
        parameter_range: None,
        id: CurveId::mint("test:model:curve#transformed-curve").expect("identity grammar"),
        geometry: CurveGeometry::Solved(curve_geometry.clone()),
        source_object: None,
    });
    source.model.surfaces.push(Surface {
        id: SurfaceId::mint("test:model:surface#transformed-surface").expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(surface_geometry.clone()),
        source_object: None,
    });

    let mut output = Vec::new();
    write_step(
        &source,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write replicas");
    let text = String::from_utf8(output.clone()).expect("STEP output is UTF-8");
    assert!(text.contains("CURVE_REPLICA"));
    assert!(text.contains("SURFACE_REPLICA"));
    assert!(text.contains("CARTESIAN_TRANSFORMATION_OPERATOR_3D"));
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode replicas");
    assert!(decoded
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.geometry.solved() == Some(&curve_geometry)));
    assert!(decoded
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| surface.geometry.solved() == Some(&surface_geometry)));
}

#[test]
fn surface_replica_dependencies_resolve_before_trimmed_surfaces() {
    let decoded = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(1.,0.,0.));
#3=DIRECTION('',(0.,1.,0.));
#4=DIRECTION('',(0.,0.,1.));
#5=AXIS2_PLACEMENT_3D('',#1,#4,#2);
#6=PLANE('',#5);
#7=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#2,#3,#1,2.,#4);
#8=SURFACE_REPLICA('',#9,#7);
#9=SURFACE_REPLICA('',#6,#7);
#10=RECTANGULAR_TRIMMED_SURFACE('',#8,0.,1.,0.,1.,.T.,.T.);
#11=GEOMETRIC_SET('',(#10));
#12=SHAPE_REPRESENTATION('',(#11),#13);
#13=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));",
    );

    assert!(decoded.ir().model.surfaces.iter().any(|surface| {
        surface.id.as_str() == "step:data:surface#10"
            && matches!(
                surface.geometry.solved(),
                Some(SolvedSurfaceGeometry::Transformed(_))
            )
    }));
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
                == Some("step:data:surface#10")
                && match surface.definition() { cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Subset(matched_payload) => matches!((matched_payload.support(), &matched_payload.parameter_ranges().map(cadmpeg_ir::geometry::DirectedParameterRange::endpoints), matched_payload.u_sense(), matched_payload.v_sense(),), (support, [[0.0, 1.0], [0.0, 1.0]], Some(true), Some(true),) if support.as_str() == "step:data:surface#8"), _ => false }
        }));
    assert!(decoded.report().losses.iter().all(|loss| {
        !loss
            .message
            .contains("RECTANGULAR_TRIMMED_SURFACE #10 has invalid or unresolved")
    }));

    assert!(decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| {
            matches!(
                surface.definition(),
                cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Replica { source, .. }
                    if decoded.ir().model.procedural_surface_owner(&surface.id).map(SurfaceId::as_str)
                        == Some("step:data:surface#8")
                        && source.as_str() == "step:data:surface#9"
            )
        }));
    let index = ModelIndex::build(decoded.ir(), cadmpeg_ir::index::StandardIndex);
    assert_eq!(
        model_surface_point_by_id(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &index,
            &SurfaceId::mint("step:data:surface#10").expect("identity grammar"),
            0.0,
            0.0
        )
        .map(cadmpeg_ir::features::FinitePoint3::get),
        Ok(Point3::new(0.0, 0.0, 0.0))
    );
    assert_eq!(
        model_surface_point_by_id(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &index,
            &SurfaceId::mint("step:data:surface#10").expect("identity grammar"),
            1.0,
            1.0
        )
        .map(cadmpeg_ir::features::FinitePoint3::get),
        Ok(Point3::new(4.0, 4.0, 0.0))
    );

    let mut output = Vec::new();
    write_step(
        decoded.ir(),
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write trimmed surface replica");
    let text = String::from_utf8(output.clone()).expect("STEP output is UTF-8");
    assert!(text.contains("SURFACE_REPLICA"));
    assert!(text.contains("RECTANGULAR_TRIMMED_SURFACE"));
    let round_trip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode trimmed surface replica");
    assert!(round_trip
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| {
            matches!(
                surface.definition(),
                cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Replica { source, .. }
                    if source.as_str().starts_with("step:data:surface#")
            )
        }));
}

#[test]
fn forward_replica_dependencies_resolve_to_nested_transforms() {
    let decoded = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(1.,0.,0.));
#3=DIRECTION('',(0.,1.,0.));
#4=DIRECTION('',(0.,0.,1.));
#5=VECTOR('',#2,1.);
#6=LINE('',#1,#5);
#7=CARTESIAN_POINT('',(10.,20.,30.));
#8=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#2,#3,#7,2.,#4);
#9=CURVE_REPLICA('',#10,#8);
#10=CURVE_REPLICA('',#6,#8);
#11=AXIS2_PLACEMENT_3D('',#1,#4,#2);
#12=PLANE('',#11);
#13=SURFACE_REPLICA('',#14,#8);
#14=SURFACE_REPLICA('',#12,#8);",
    );
    let transform = Transform::affine([
        [2.0, 0.0, 0.0, 10.0],
        [0.0, 2.0, 0.0, 20.0],
        [0.0, 0.0, 2.0, 30.0],
    ])
    .expect("affine transform");
    let base_curve = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    ));
    let expected_curve = CurveGeometry::Solved(SolvedCurveGeometry::Transformed(
        cadmpeg_ir::geometry::PlacedCurve::try_new(
            Box::new(SolvedCurveGeometry::Transformed(
                cadmpeg_ir::geometry::PlacedCurve::try_new(
                    Box::new(base_curve.solved().expect("solved carrier").clone()),
                    transform,
                )
                .expect("placed curve"),
            )),
            transform,
        )
        .expect("placed curve"),
    ));
    let expected_surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(
        cadmpeg_ir::geometry::PlacedSurface::try_new(
            Box::new(SolvedSurfaceGeometry::Transformed(
                cadmpeg_ir::geometry::PlacedSurface::try_new(
                    Box::new(SolvedSurfaceGeometry::Plane(
                        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                            Point3::new(0.0, 0.0, 0.0),
                            Vector3::new(0.0, 0.0, 1.0),
                            Vector3::new(1.0, 0.0, 0.0),
                        )
                        .unwrap(),
                    )),
                    transform,
                )
                .expect("placed surface"),
            )),
            transform,
        )
        .expect("placed surface"),
    ));
    assert!(decoded.ir().model.curves.iter().any(|curve| {
        curve.id.as_str() == "step:data:curve#9"
            && curve.geometry.solved() == expected_curve.solved()
    }));
    assert_eq!(
        decoded
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id.as_str() == "step:data:curve#6")
            .and_then(|curve| curve.source_object.as_ref())
            .map(|source| source.object_id.as_str()),
        Some("#10")
    );
    assert!(decoded
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id.as_str() == "step:data:surface#13"
            && surface.geometry.solved() == expected_surface.solved()));
    assert_eq!(
        decoded
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == "step:data:surface#12")
            .and_then(|surface| surface.source_object.as_ref())
            .map(|source| source.object_id.as_str()),
        Some("#14")
    );
}

#[test]
fn cartesian_transformation_operator_derives_optional_axes() {
    let decoded = decode_inline(
        "#1=CARTESIAN_POINT('',(10.,20.,30.));
#2=CARTESIAN_TRANSFORMATION_OPERATOR_3D('', $,$,#1,$,$);
#3=CARTESIAN_POINT('',(0.,0.,0.));
#4=DIRECTION('',(1.,0.,0.));
#5=VECTOR('',#4,1.);
#6=LINE('',#3,#5);
#7=CURVE_REPLICA('',#6,#2);
#8=DIRECTION('',(1.,1.,0.));
#9=DIRECTION('',(0.,0.,1.));
#10=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#8,$,#3,2.,#9);
#11=CURVE_REPLICA('',#6,#10);
#12=GEOMETRIC_CURVE_SET('',(#7,#11));
#13=SHAPE_REPRESENTATION('',(#12),$);",
    );

    let transform_for = |id: &str| {
        decoded
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id.as_str() == id)
            .and_then(|curve| match curve.geometry.solved() {
                Some(SolvedCurveGeometry::Transformed(placed)) => Some(*placed.transform()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing transformed curve {id}"))
    };
    let assert_rows = |actual: Transform, expected: [[f64; 4]; 4]| {
        for (row, values) in expected.iter().enumerate() {
            for (column, expected) in values.iter().enumerate() {
                assert!(
                    (actual.rows()[row][column] - expected).abs() < 1.0e-12,
                    "matrix coefficient [{row}][{column}] was {}, expected {expected}",
                    actual.rows()[row][column]
                );
            }
        }
    };

    assert_rows(
        transform_for("step:data:curve#7"),
        [
            [1.0, 0.0, 0.0, 10.0],
            [0.0, 1.0, 0.0, 20.0],
            [0.0, 0.0, 1.0, 30.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    );
    let root_two = 2.0_f64.sqrt();
    assert_rows(
        transform_for("step:data:curve#11"),
        [
            [root_two, -root_two, 0.0, 0.0],
            [root_two, root_two, 0.0, 0.0],
            [0.0, 0.0, 2.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    );
}

#[test]
fn pcurve_replica_derives_orthogonal_two_dimensional_axes() {
    use cadmpeg_ir::geometry::pcurve::PcurveGeometry;

    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#55=DEFINITIONAL_REPRESENTATION('',(#54),#50);",
            "#55=DEFINITIONAL_REPRESENTATION('',(#73),#50);\n#71=DIRECTION('',(1.,1.));\n#72=CARTESIAN_TRANSFORMATION_OPERATOR_2D('',#71,$,#51,1.);\n#73=CURVE_REPLICA('',#54,#72);",
        )
        .replace(
            "#4=CARTESIAN_POINT('',(10.,0.,0.));",
            "#4=CARTESIAN_POINT('',(0.7071067811865476,0.7071067811865476,0.));",
        )
        .replace(
            "#16=LINE('',#3,#13);",
            "#74=DIRECTION('',(0.7071067811865476,0.7071067811865476,0.));\n#75=VECTOR('',#74,1.);\n#16=LINE('',#3,#75);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode pcurve replica");
    let pcurve = decoded
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.as_str() == "step:data:pcurve#56")
        .expect("replica pcurve");
    let PcurveGeometry::Transformed(placed) = &pcurve.geometry else {
        panic!("pcurve replica lost its transformation")
    };
    let transform = placed.transform();
    let root_two = 2.0_f64.sqrt();
    assert!((transform.rows()[0][0] - 1.0 / root_two).abs() < 1.0e-12);
    assert!((transform.rows()[0][1] + 1.0 / root_two).abs() < 1.0e-12);
    assert!((transform.rows()[1][0] - 1.0 / root_two).abs() < 1.0e-12);
    assert!((transform.rows()[1][1] - 1.0 / root_two).abs() < 1.0e-12);
}

#[test]
fn long_forward_curve_replica_chain_resolves_with_a_worklist() {
    let mut records = String::from(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(1.,0.,0.));
#3=DIRECTION('',(0.,1.,0.));
#4=DIRECTION('',(0.,0.,1.));
#5=VECTOR('',#2,1.);
#6=LINE('',#1,#5);
#7=CARTESIAN_POINT('',(10.,20.,30.));
#8=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#2,#3,#7,2.,#4);
",
    );
    // The head replica carries exactly MAX_GEOMETRY_NESTING placements over the
    // line, the deepest chain the IR admits.
    for id in 9..=263 {
        writeln!(records, "#{id}=CURVE_REPLICA('',#{},#8);", id + 1).expect("append curve replica");
    }
    records.push_str("#264=CURVE_REPLICA('',#6,#8);");

    let decoded = decode_inline(&records);
    assert!(decoded.ir().model.curves.iter().any(|curve| {
        curve.id.as_str() == "step:data:curve#9"
            && matches!(
                curve.geometry.solved(),
                Some(SolvedCurveGeometry::Transformed(_))
            )
    }));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("CURVE_REPLICA #9 has invalid or unresolved parent/operator")
    }));
}

#[test]
fn long_forward_offset_surface_chain_resolves_with_a_worklist() {
    let mut records = String::from(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#5=PLANE('',#4);
",
    );
    for id in 6..=261 {
        writeln!(records, "#{id}=OFFSET_SURFACE('',#{},1.,.F.);", id + 1)
            .expect("append offset surface");
    }
    records.push_str("#262=OFFSET_SURFACE('',#5,1.,.F.);\n#263=GEOMETRIC_SET('',(#6));");

    let decoded = decode_inline(&records);
    assert!(decoded
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id.as_str() == "step:data:surface#6"));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("OFFSET_SURFACE #6 has invalid or unresolved support parameters")
    }));
}

#[test]
fn replicas_retain_bounded_parent_relations() {
    let decoded = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(1.,0.,0.));
#3=DIRECTION('',(0.,1.,0.));
#4=DIRECTION('',(0.,0.,1.));
#5=VECTOR('',#2,1.);
#6=LINE('',#1,#5);
#7=TRIMMED_CURVE('',#6,(PARAMETER_VALUE(1.)),(PARAMETER_VALUE(2.)),.T.,.PARAMETER.);
#8=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#2,#3,#1,3.,#4);
#9=CURVE_REPLICA('',#7,#8);
#10=AXIS2_PLACEMENT_3D('',#1,#4,#2);
#11=PLANE('',#10);
#12=RECTANGULAR_TRIMMED_SURFACE('',#11,1.,2.,3.,4.,.T.,.T.);
#13=SURFACE_REPLICA('',#12,#8);
#14=GEOMETRIC_SET('',(#9,#13));
#15=SHAPE_REPRESENTATION('',(#14),#16);
#16=(GEOMETRIC_REPRESENTATION_CONTEXT(3) REPRESENTATION_CONTEXT('',''));",
    );

    assert!(decoded.ir().model.procedural_curves.iter().any(|curve| {
        matches!(
            curve.definition(),
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Replica { source, .. }
                if decoded.ir().model.procedural_curve_owner(&curve.id).map(CurveId::as_str)
                    == Some("step:data:curve#9")
                    && source.as_str() == "step:data:curve#7"
        )
    }));
    assert!(decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| {
            matches!(
                surface.definition(),
                cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Replica { source, .. }
                    if decoded.ir().model.procedural_surface_owner(&surface.id).map(SurfaceId::as_str)
                        == Some("step:data:surface#13")
                        && source.as_str() == "step:data:surface#12"
            )
        }));
    let index = ModelIndex::build(decoded.ir(), cadmpeg_ir::index::StandardIndex);
    assert_eq!(
        model_curve_point_by_id(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &index,
            &CurveId::mint("step:data:curve#9").expect("identity grammar"),
            0.0
        )
        .map(cadmpeg_ir::features::FinitePoint3::get),
        Ok(Point3::new(3.0, 0.0, 0.0))
    );
    assert_eq!(
        model_surface_point_by_id(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &index,
            &SurfaceId::mint("step:data:surface#13").expect("identity grammar"),
            0.0,
            0.0
        )
        .map(cadmpeg_ir::features::FinitePoint3::get),
        Ok(Point3::new(3.0, 9.0, 0.0))
    );

    let mut output = Vec::new();
    write_step(
        decoded.ir(),
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write replicas of bounded parents");
    let text = String::from_utf8(output).expect("STEP output is UTF-8");
    assert!(text.contains("CURVE_REPLICA"));
    assert!(text.contains("SURFACE_REPLICA"));
    assert!(text.contains("TRIMMED_CURVE"));
    assert!(text.contains("RECTANGULAR_TRIMMED_SURFACE"));
}

#[test]
fn mapped_representation_dag_is_memoized() {
    let depth = 32_u64;
    let mut records = String::from(
        "#1=APPLICATION_CONTEXT('');\n\
#2=PRODUCT('p','p','',());\n\
#3=PRODUCT_DEFINITION_FORMATION('','',#2);\n\
#4=PRODUCT_DEFINITION('','',#3,#1);\n\
#5=PRODUCT_DEFINITION_SHAPE('','',#4);\n\
#6=SHAPE_DEFINITION_REPRESENTATION(#5,#100);\n",
    );
    for level in 0..depth {
        let representation = 100 + level;
        let next = representation + 1;
        let map = 1_000 + level;
        let first = 2_000 + level * 2;
        let second = first + 1;
        write!(
            records,
            "#{representation}=SHAPE_REPRESENTATION('',(#{first},#{second}),$);\n\
#{map}=REPRESENTATION_MAP($,#{next});\n\
#{first}=MAPPED_ITEM('',#{map},$);\n\
#{second}=MAPPED_ITEM('',#{map},$);\n"
        )
        .expect("write mapped representation fixture");
    }
    write!(
        records,
        "#{}=SHAPE_REPRESENTATION('',(#9000),$);\n\
#9000=MANIFOLD_SOLID_BREP('',#9001);\n\
#9001=CLOSED_SHELL('',(#9002));\n\
#9002=ADVANCED_FACE('',(#9003),#9004,.T.);\n\
#9003=FACE_OUTER_BOUND('',#9005,.T.);\n\
#9005=VERTEX_LOOP('',#9006);\n\
#9006=VERTEX_POINT('',#9007);\n\
#9007=CARTESIAN_POINT('',(0.,0.,0.));\n\
#9004=PLANE('',#9008);\n\
#9008=AXIS2_PLACEMENT_3D('',#9007,$,$);",
        100 + depth
    )
    .expect("write terminal representation fixture");

    let result = decode_inline(&records);
    assert_eq!(result.ir().model.product_definitions.len(), 1);
    assert_eq!(result.ir().model.product_definitions[0].bodies.len(), 1);
    assert_eq!(
        result.ir().model.product_definitions[0].bodies[0].as_str(),
        "step:data:body#9000"
    );
}

#[test]
fn a_curve_replica_chain_past_the_admitted_depth_is_refused_at_decode() {
    let mut records = String::from(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(1.,0.,0.));
#3=DIRECTION('',(0.,1.,0.));
#4=DIRECTION('',(0.,0.,1.));
#5=VECTOR('',#2,1.);
#6=LINE('',#1,#5);
#7=CARTESIAN_POINT('',(10.,20.,30.));
#8=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#2,#3,#7,2.,#4);
",
    );
    // #9 carries one placement more than the IR admits; #10 carries exactly the
    // admitted depth.
    for id in 9..=264 {
        writeln!(records, "#{id}=CURVE_REPLICA('',#{},#8);", id + 1).expect("append curve replica");
    }
    records.push_str("#265=CURVE_REPLICA('',#6,#8);");

    let decoded = decode_inline(&records);
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("CURVE_REPLICA #9 nests past the admitted inline basis depth")
    }));
    assert!(decoded.ir().model.curves.iter().any(|curve| {
        curve.id.as_str() == "step:data:curve#9"
            && matches!(
                curve.geometry.solved(),
                Some(SolvedCurveGeometry::Unknown { .. })
            )
    }));
    assert!(decoded.ir().model.curves.iter().any(|curve| {
        curve.id.as_str() == "step:data:curve#10"
            && matches!(
                curve.geometry.solved(),
                Some(SolvedCurveGeometry::Transformed(_))
            )
    }));
}

#[test]
fn a_surface_replica_chain_past_the_admitted_depth_is_refused_at_decode() {
    let mut records = String::from(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(1.,0.,0.));
#3=DIRECTION('',(0.,1.,0.));
#4=DIRECTION('',(0.,0.,1.));
#5=AXIS2_PLACEMENT_3D('',#1,#4,#2);
#6=PLANE('',#5);
#7=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#2,#3,#1,1.,#4);
",
    );
    // #8 carries one placement more than the IR admits; #9 carries exactly the
    // admitted depth.
    for id in 8..=263 {
        writeln!(records, "#{id}=SURFACE_REPLICA('',#{},#7);", id + 1)
            .expect("append surface replica");
    }
    records.push_str("#264=SURFACE_REPLICA('',#6,#7);\n#265=GEOMETRIC_SET('',(#8));");

    let decoded = decode_inline(&records);
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("SURFACE_REPLICA #8 nests past the admitted inline basis depth")
    }));
    assert!(decoded.ir().model.surfaces.iter().any(|surface| {
        surface.id.as_str() == "step:data:surface#8"
            && matches!(
                surface.geometry.solved(),
                Some(SolvedSurfaceGeometry::Unknown { .. })
            )
    }));
    assert!(decoded.ir().model.surfaces.iter().any(|surface| {
        surface.id.as_str() == "step:data:surface#9"
            && matches!(
                surface.geometry.solved(),
                Some(SolvedSurfaceGeometry::Transformed(_))
            )
    }));
}

#[test]
fn two_dimensional_replicas_do_not_create_three_dimensional_fallbacks() {
    let original = include_str!("../../../../tests/fixtures/ap214_sheet.p21");
    for definition in ["#71", "#74"] {
        let nested = if definition == "#74" {
            "\n#74=CURVE_REPLICA('',#71,#72);"
        } else {
            ""
        };
        let source = original.replace("#55=DEFINITIONAL_REPRESENTATION('',(#54),#50);", &format!(
            "#55=DEFINITIONAL_REPRESENTATION('',({definition}),#50);\n#72=CARTESIAN_TRANSFORMATION_OPERATOR_2D('', $,$,#51,1.);\n#71=CURVE_REPLICA('',#54,#72);{nested}"
        ));
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        assert!(decoded
            .ir()
            .model
            .pcurves
            .iter()
            .any(|pcurve| pcurve.id.as_str() == "step:data:pcurve#56"));
        assert!(!decoded
            .ir()
            .model
            .curves
            .iter()
            .any(|curve| curve.id.as_str() == "step:data:curve#71"
                || curve.id.as_str() == "step:data:curve#74"));
        assert!(!decoded.report().losses.iter().any(|loss| loss
            .message
            .contains("CURVE_REPLICA #71 has invalid")
            || loss.message.contains("CURVE_REPLICA #74 has invalid")));
        let check = cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).unwrap();
        assert!(
            check
                .findings
                .iter()
                .all(|finding| finding.severity < cadmpeg_ir::report::Severity::Error),
            "{:#?}",
            check.findings
        );
    }
}
