// SPDX-License-Identifier: Apache-2.0
use crate::parse::Value;
use crate::reader::geometry::{
    curve_endpoint_seed, decode_pcurve_geometry, directrix_geometry_parameter_scale,
    edge_parameter_range, nurbs_surface_parameter_domain, pcurve_trim_parameter, si_prefix,
    surface_parameter_scales_for_step, surface_periodic_domains, trimmed_curve_parameter_range,
    trimmed_pcurve_parameterization, unit_scale_mm, unit_scale_radians,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::nurbs_curve_parameter_near_point;

use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes},
    pcurve::{PcurveGeometry, PcurveNurbs, PcurveNurbsPoles},
    Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::{FiniteReal, PositiveReal};
use cadmpeg_ir::transform::{Transform, Transform2};
use std::collections::{BTreeMap, BTreeSet};

const EPS_ENDPOINT_WITNESS: f64 = 1.0e-6;

fn source_curve_refusal(
    collection_limit: u64,
    depth_limit: Option<u64>,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CIRCLE();#2=TRIMMED_CURVE('',#1,(),(),.T.,.PARAMETER.);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid source curve exchange");
    let scales = super::super::UnitScales {
        default_length: PositiveReal::ONE,
        default_angle: PositiveReal::ONE,
        length: BTreeMap::new(),
        angle: BTreeMap::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    if let Some(limit) = depth_limit {
        policy.limits.max_recursion_depth = limit;
    }
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits policy");
    super::super::resolve_source_curve_parameter_scales(&exchange, &scales, &ctx)
        .expect_err("source curve admission exceeds the limit")
}

#[test]
fn source_curve_parameter_active_refuses_collection_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    assert!(matches!(
        source_curve_refusal(1, None),
        cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_source_curve_parameter_active"
    ));
}

#[test]
fn source_curve_parameter_scales_refuse_collection_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    assert!(matches!(
        source_curve_refusal(0, None),
        cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_source_curve_parameter_scales"
    ));
}

#[test]
fn source_curve_parameter_walk_refuses_depth_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    assert!(matches!(
        source_curve_refusal(2, Some(0)),
        cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_source_curve_parameter_walk"
    ));
}

fn parameter_inference_ir(with_edge: bool) -> CadIr {
    use cadmpeg_ir::geometry::analytic::LineCurve;
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::ids::{EdgeId, PointId, VertexId};
    use cadmpeg_ir::topology::{Edge, EdgeCarrier, Point, Vertex};

    let mut ir = CadIr::empty();
    for (number, x) in [(1, 0.0), (2, 1.0)] {
        ir.model.points.push(Point::new(
            PointId::from(crate::ids::data(crate::ids::kind!("point"), number)),
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(x, 0.0, 0.0))
                .expect("finite point"),
            None,
        ));
        ir.model.vertices.push(Vertex {
            id: VertexId::from(crate::ids::data(crate::ids::kind!("vertex"), number)),
            point: PointId::from(crate::ids::data(crate::ids::kind!("point"), number)),
            tolerance: None,
        });
    }
    if with_edge {
        let curve = CurveId::from(crate::ids::data(crate::ids::kind!("curve"), 3));
        ir.model.curves.push(Curve {
            parameter_range: None,
            id: curve.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0))
                    .expect("finite line"),
            )),
            source_object: None,
        });
        ir.model.edges.push(Edge {
            id: EdgeId::from(crate::ids::data(crate::ids::kind!("edge"), 4)),
            carrier: EdgeCarrier::unbounded(Some(curve)),
            start: VertexId::from(crate::ids::data(crate::ids::kind!("vertex"), 1)),
            end: VertexId::from(crate::ids::data(crate::ids::kind!("vertex"), 2)),
            tolerance: None,
        });
    }
    ir
}

fn assert_parameter_inference_refusal(
    dimension: cadmpeg_core::decode::ResourceDimension,
    operation: &str,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_core::CodecError;
    let error = cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |limit| {
        let mut ir = parameter_inference_ir(true);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = limit;
            }
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = limit;
            }
            other => panic!("unexpected inference dimension: {other:?}"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("root");
        super::super::infer_edge_parameter_ranges(&mut ir, &ctx)
    });
    assert!(matches!(error, CodecError::ResourceLimit(refusal)
        if refusal.dimension == dimension && refusal.operation == operation));
}

#[test]
fn parameter_inference_without_edges_needs_no_collection_storage() {
    let mut ir = parameter_inference_ir(false);
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        for _ in 0..1000 {
            super::super::infer_edge_parameter_ranges(&mut ir, ctx)
                .expect("no edge interval needs inference");
        }
        assert_eq!(ir.model.points.len(), 2);
        assert_eq!(ir.model.vertices.len(), 2);
    });
}

#[test]
fn streamed_parameter_inference_finds_the_stored_line_endpoints() {
    let mut ir = parameter_inference_ir(true);
    crate::test_support::with_service_context(b"", |_, ctx| {
        super::super::infer_edge_parameter_ranges(&mut ir, ctx).expect("line interval");
    });
    assert_eq!(
        ir.model.edges[0].param_range().expect("inferred interval"),
        [0.0, 1.0]
    );
}

#[test]
fn parameter_inference_range_refuses_collection_limit() {
    assert_parameter_inference_refusal(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "step_parameter_inference_ranges",
    );
}

#[test]
fn parameter_inference_edge_curve_refuses_retained_limit() {
    assert_parameter_inference_refusal(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "step_parameter_inference_edge_curve",
    );
}

#[test]
fn periodic_nurbs_surface_parameter_periods_keep_usize_counts() {
    let surface = NurbsSurface::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 2.0, 2.0], true),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], true),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
                vec![Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 1.0, 0.0)],
            ],
            None,
        ),
        false,
    )
    .expect("fixture constructor admission")
    .expect("periodic NURBS surface");
    assert_eq!(
        nurbs_surface_parameter_domain(1, surface.u_knots(), surface.u_count()),
        Some([0.0, 2.0])
    );
    assert_eq!(
        surface_periodic_domains(&SolvedSurfaceGeometry::Nurbs(surface)),
        [Some([0.0, 2.0]), Some([0.0, 1.0])]
    );
}

#[test]
fn periodic_curve_trim_shifts_a_wide_finite_seam_endpoint() {
    let max = f64::MAX;
    let curve = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![-max, -max, max, max],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
        true,
    )
    .expect("fixture constructor admission")
    .expect("wide periodic curve");
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve));
    assert_eq!(
        trimmed_curve_parameter_range(&geometry, max * 0.5, -max, true),
        [max * 0.5, max]
    );
}

#[test]
fn periodic_edge_range_keeps_a_finite_sweep_across_a_wide_seam() {
    let max = f64::MAX;
    let curve = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![-max, -max, max, max],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
        true,
    )
    .expect("fixture constructor admission")
    .expect("wide periodic curve");
    let start = FiniteReal::new(max * 0.5).expect("finite start");
    let end = FiniteReal::new(-max).expect("finite end");
    assert_eq!(
        edge_parameter_range(&SolvedCurveGeometry::Nurbs(curve), start, end),
        Some([max * 0.5, max])
    );
}

#[test]
fn periodic_pcurve_trim_shifts_a_wide_finite_seam_endpoint() {
    let max = f64::MAX;
    let nurbs = PcurveNurbs::new(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![-max, -max, max, max],
        PcurveNurbsPoles::Polynomial {
            points: vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        },
        true,
    )
    .expect("fixture pcurve construction admission")
    .expect("wide periodic pcurve");
    let geometry = PcurveGeometry::Nurbs { nurbs };
    assert_eq!(
        trimmed_pcurve_parameterization(&geometry, max * 0.5, -max, true),
        ([max * 0.5, max], true)
    );
}

#[test]
fn edge_parameter_range_rejects_reversed_nonperiodic_interval() {
    let line = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    ));
    let [two, five] = [2.0, 5.0].map(|value| FiniteReal::new(value).expect("finite parameter"));
    assert_eq!(
        edge_parameter_range(line.solved().expect("solved carrier"), two, five),
        Some([2.0, 5.0])
    );
    assert_eq!(
        edge_parameter_range(line.solved().expect("solved carrier"), five, two),
        None
    );
    assert_eq!(
        edge_parameter_range(line.solved().expect("solved carrier"), two, two),
        None
    );
}

#[test]
fn edge_parameter_range_normalizes_periodic_interval_in_constant_time() {
    let circle = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
        )
        .unwrap(),
    ));
    let [start, end] = [
        1.5 + 20_000.0 * std::f64::consts::TAU,
        0.5 - 20_000.0 * std::f64::consts::TAU,
    ]
    .map(|value| FiniteReal::new(value).expect("finite parameter"));
    let range = edge_parameter_range(circle.solved().expect("solved carrier"), start, end)
        .expect("periodic interval");
    assert!((range[0] - 1.5).abs() < 1.0e-10);
    assert!((range[1] - (0.5 + std::f64::consts::TAU)).abs() < 1.0e-10);
}

#[test]
fn periodic_edge_range_reduces_finite_endpoints_separately_when_difference_overflows() {
    let circle = SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
        )
        .expect("finite circle"),
    );
    let start = FiniteReal::new(-f64::MAX).expect("finite start");
    let end = FiniteReal::new(f64::MAX).expect("finite end");
    let range = edge_parameter_range(&circle, start, end).expect("finite cyclic sweep");
    assert!(range[0].is_finite() && range[1].is_finite());
    assert!(range[1] > range[0]);
    assert!(range[1] - range[0] < std::f64::consts::TAU);
}

#[test]
fn nonperiodic_nurbs_endpoint_seed_selects_the_terminal_branch() {
    let nurbs = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        3,
        vec![0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(3.0, 0.0, 0.0),
            Point3::new(4.0, 0.0, 0.0),
            Point3::new(5.0, 0.0, 0.0),
            Point3::new(3.0, 0.0, 0.0),
        ],
        None,
        false,
    )
    .expect("fixture constructor admission")
    .unwrap();
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs.clone()));
    let start_point = cadmpeg_ir::eval::decode::nurbs_curve_point_at(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
        &nurbs,
        0.0,
    )
    .expect("start point");
    let end_point = cadmpeg_ir::eval::decode::nurbs_curve_point_at(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
        &nurbs,
        1.0,
    )
    .expect("end point");
    let start_seed = curve_endpoint_seed(geometry.solved().expect("solved carrier"), false, 0.0);
    let start = nurbs_curve_parameter_near_point(
        &cadmpeg_test_support::service_decode_context(),
        &nurbs,
        start_point.get(),
        EPS_ENDPOINT_WITNESS,
        start_seed,
    )
    .expect("resource allocation did not fail")
    .expect("start witness")
    .get();
    let start_seed_end = nurbs_curve_parameter_near_point(
        &cadmpeg_test_support::service_decode_context(),
        &nurbs,
        end_point.get(),
        EPS_ENDPOINT_WITNESS,
        start,
    )
    .expect("resource allocation did not fail")
    .expect("unanchored end witness")
    .get();
    assert!((start_seed_end - 1.0).abs() > 0.1);
    let end_seed = curve_endpoint_seed(geometry.solved().expect("solved carrier"), true, start);
    let end = nurbs_curve_parameter_near_point(
        &cadmpeg_test_support::service_decode_context(),
        &nurbs,
        end_point.get(),
        EPS_ENDPOINT_WITNESS,
        end_seed,
    )
    .expect("resource allocation did not fail")
    .expect("end witness")
    .get();

    assert!(start.abs() < 1.0e-12);
    assert!((end - 1.0).abs() < 1.0e-12);
}

#[test]
fn surface_parameter_units_follow_the_surface_chart() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");

    let ir = CadIr::empty();
    let plane = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    ));
    let cylinder = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .unwrap(),
    ));
    let sphere = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
        cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .unwrap(),
    ));
    let transformed = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(
        cadmpeg_ir::geometry::PlacedSurface::try_new(
            Box::new(cylinder.clone().solved().expect("solved carrier").clone()),
            Transform::identity(),
        )
        .expect("placed surface"),
    ));
    assert_eq!(
        surface_parameter_scales_for_step(
            &ir,
            &SurfaceId::mint("test:model:surface#plane").expect("identity grammar"),
            &plane,
            10.0,
            0.25,
            &BTreeMap::new(),
            &ctx,
        )
        .expect("scale evaluation"),
        Some([10.0, 10.0])
    );
    assert_eq!(
        surface_parameter_scales_for_step(
            &ir,
            &SurfaceId::mint("test:model:surface#cylinder").expect("identity grammar"),
            &cylinder,
            10.0,
            0.25,
            &BTreeMap::new(),
            &ctx,
        )
        .expect("scale evaluation"),
        Some([0.25, 10.0])
    );
    assert_eq!(
        surface_parameter_scales_for_step(
            &ir,
            &SurfaceId::mint("test:model:surface#sphere").expect("identity grammar"),
            &sphere,
            10.0,
            0.25,
            &BTreeMap::new(),
            &ctx,
        )
        .expect("scale evaluation"),
        Some([0.25, 0.25])
    );
    assert_eq!(
        surface_parameter_scales_for_step(
            &ir,
            &SurfaceId::mint("test:model:surface#transformed").expect("identity grammar"),
            &transformed,
            10.0,
            0.25,
            &BTreeMap::new(),
            &ctx,
        )
        .expect("scale evaluation"),
        Some([0.25, 10.0])
    );
    assert_eq!(
        surface_parameter_scales_for_step(
            &ir,
            &SurfaceId::mint("test:model:surface#unknown").expect("identity grammar"),
            &SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            10.0,
            0.25,
            &BTreeMap::new(),
            &ctx,
        )
        .expect("scale evaluation"),
        None
    );
}

#[test]
fn procedural_surface_units_follow_the_evaluated_parameter_order() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");

    let mut ir = CadIr::empty();
    let directrix = CurveId::mint("test:model:curve#line").expect("identity grammar");
    ir.model.curves.push(Curve {
        parameter_range: None,
        id: directrix.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    let sweep = SurfaceId::mint("test:model:surface#sweep").expect("identity grammar");
    let revolution = SurfaceId::mint("test:model:surface#revolution").expect("identity grammar");
    ir.model.surfaces.extend([
        Surface {
            id: sweep.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            source_object: None,
        },
        Surface {
            id: revolution.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            source_object: None,
        },
    ]);
    let _attached = ir.model.add_procedural_surface(
        &cadmpeg_ir::document::admission::StandardAdmission,
        &sweep,
        ProceduralSurface::new(
            ProceduralSurfaceId::mint("test:model:procedural-surface#sweep-construction")
                .expect("identity grammar"),
            ProceduralSurfaceDefinition::LinearSweep(
                cadmpeg_ir::geometry::surface_payloads::LinearSweepSurfaceConstruction::try_new(
                    directrix.clone(),
                    Vector3::new(0.0, 1.0, 0.0),
                )
                .unwrap(),
            ),
            None,
        ),
    );
    let _attached = ir.model.add_procedural_surface(
        &cadmpeg_ir::document::admission::StandardAdmission,
        &revolution,
        ProceduralSurface::new(
            ProceduralSurfaceId::mint("test:model:procedural-surface#revolution-construction")
                .expect("identity grammar"),
            ProceduralSurfaceDefinition::AxisRevolution(
                cadmpeg_ir::geometry::surface_payloads::AxisRevolutionSurfaceConstruction::try_new(
                    directrix,
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
            ),
            None,
        ),
    );
    let length_scale = 0.001;
    let angle_scale = std::f64::consts::PI / 180.0;

    assert_eq!(
        surface_parameter_scales_for_step(
            &ir,
            &sweep,
            &ir.model.surfaces[0].geometry,
            length_scale,
            angle_scale,
            &BTreeMap::new(),
            &ctx,
        )
        .expect("scale evaluation"),
        Some([length_scale, 1.0])
    );
    assert_eq!(
        surface_parameter_scales_for_step(
            &ir,
            &revolution,
            &ir.model.surfaces[1].geometry,
            length_scale,
            angle_scale,
            &BTreeMap::new(),
            &ctx,
        )
        .expect("scale evaluation"),
        Some([angle_scale, length_scale])
    );
}

#[test]
fn directrix_parameter_units_follow_step_curve_equations() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");

    let angle_scale = std::f64::consts::PI / 180.0;
    let parabola = CurveGeometry::Solved(SolvedCurveGeometry::Parabola(
        cadmpeg_ir::geometry::analytic::ParabolaCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .unwrap(),
    ));
    let hyperbola = CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(
        cadmpeg_ir::geometry::analytic::HyperbolaCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
            1.0,
        )
        .unwrap(),
    ));
    let polyline = CurveGeometry::Solved(SolvedCurveGeometry::Polyline(
        cadmpeg_ir::geometry::sampled::PolylineCurve::new(
            cadmpeg_ir::geometry::sampled::PolylineSamples::Unparameterized {
                points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)]
                    .try_into()
                    .expect("nonempty polyline fixture"),
            },
            0.0,
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("polyline construction admission")
        .unwrap(),
    ));

    assert_eq!(
        directrix_geometry_parameter_scale(
            parabola.solved().expect("solved carrier"),
            0.001,
            angle_scale,
            &ctx,
        )
        .expect("scale evaluation"),
        Some(1.0)
    );
    assert_eq!(
        directrix_geometry_parameter_scale(
            hyperbola.solved().expect("solved carrier"),
            0.001,
            angle_scale,
            &ctx,
        )
        .expect("scale evaluation"),
        Some(1.0)
    );
    assert_eq!(
        directrix_geometry_parameter_scale(
            polyline.solved().expect("solved carrier"),
            0.001,
            angle_scale,
            &ctx,
        )
        .expect("scale evaluation"),
        Some(1.0)
    );
}

#[test]
fn unresolved_procedural_directrix_has_no_assumed_parameter_units() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");

    let mut ir = CadIr::empty();
    let child = CurveId::mint("test:model:curve#unknown-child").expect("identity grammar");
    ir.model.curves.push(Curve {
        parameter_range: None,
        id: child.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
        source_object: None,
    });
    let directrix = CurveId::mint("test:model:curve#composite").expect("identity grammar");
    ir.model.curves.push(Curve {
        parameter_range: None,
        id: directrix.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Composite {
            segments: cadmpeg_ir::geometry::CompositeCurveSegments::try_from(vec![
                cadmpeg_ir::geometry::CompositeCurveSegment {
                    curve: child,
                    same_sense: true,
                    transition: cadmpeg_ir::geometry::CompositeCurveTransition::Discontinuous,
                },
            ])
            .unwrap(),
            self_intersect: None,
        }),
        source_object: None,
    });
    let surface = SurfaceId::mint("test:model:surface#sweep").expect("identity grammar");
    ir.model.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
        source_object: None,
    });
    let _attached = ir.model.add_procedural_surface(
        &cadmpeg_ir::document::admission::StandardAdmission,
        &surface,
        ProceduralSurface::new(
            ProceduralSurfaceId::mint("test:model:procedural-surface#sweep-construction")
                .expect("identity grammar"),
            ProceduralSurfaceDefinition::LinearSweep(
                cadmpeg_ir::geometry::surface_payloads::LinearSweepSurfaceConstruction::try_new(
                    directrix,
                    Vector3::new(0.0, 1.0, 0.0),
                )
                .unwrap(),
            ),
            None,
        ),
    );

    assert_eq!(
        surface_parameter_scales_for_step(
            &ir,
            &surface,
            &ir.model.surfaces[0].geometry,
            0.001,
            std::f64::consts::PI / 180.0,
            &BTreeMap::new(),
            &ctx,
        )
        .expect("scale evaluation"),
        None
    );
}

#[test]
fn axis_revolution_surface_parameter_units_use_plane_angle_for_u() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");

    let surface_id = SurfaceId::mint("test:model:surface#surface").expect("identity grammar");
    let directrix = CurveId::mint("test:model:curve#directrix").expect("identity grammar");
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        parameter_range: None,
        id: directrix.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
        source_object: None,
    });
    let _attached = ir.model.add_procedural_surface(
        &cadmpeg_ir::document::admission::StandardAdmission,
        &surface_id,
        ProceduralSurface::new(
            ProceduralSurfaceId::mint("test:model:procedural-surface#construction")
                .expect("identity grammar"),
            ProceduralSurfaceDefinition::AxisRevolution(
                cadmpeg_ir::geometry::surface_payloads::AxisRevolutionSurfaceConstruction::try_new(
                    directrix,
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
            ),
            None,
        ),
    );

    assert_eq!(
        surface_parameter_scales_for_step(
            &ir,
            &surface_id,
            &ir.model.surfaces[0].geometry,
            10.0,
            std::f64::consts::PI / 180.0,
            &BTreeMap::new(),
            &ctx,
        )
        .expect("scale evaluation"),
        Some([std::f64::consts::PI / 180.0, 10.0])
    );
}

#[test]
fn anisotropic_circle_scaling_preserves_its_native_parameterization() {
    let original = PcurveGeometry::Circle(
        cadmpeg_ir::geometry::pcurve::CirclePcurve::try_new(
            Point2::new(1.0, -2.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 1.0),
            3.0,
        )
        .unwrap(),
    );
    let mut scaled = original.clone();
    assert!(scaled.try_scale_coordinates([2.0, 3.0]).is_ok());
    assert!(matches!(scaled, PcurveGeometry::Harmonic(_)));
    for parameter in [0.0, 0.25, 1.0, 2.0] {
        let expected = cadmpeg_ir::eval::decode::pcurve_uv(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &original,
            parameter,
        )
        .unwrap();
        let actual = cadmpeg_ir::eval::decode::pcurve_uv(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &scaled,
            parameter,
        )
        .unwrap();
        assert!((actual.u - expected.u * 2.0).abs() < 1.0e-12);
        assert!((actual.v - expected.v * 3.0).abs() < 1.0e-12);
    }
}

#[test]
fn anisotropic_replica_scaling_conjugates_the_parent_map() {
    let original = PcurveGeometry::Transformed(
        cadmpeg_ir::geometry::pcurve::PlacedPcurve::try_new(
            Box::new(PcurveGeometry::Line(
                cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                    Point2::new(1.0, 2.0),
                    Point2::new(3.0, 4.0),
                )
                .unwrap(),
            )),
            Transform2::affine([[0.0, -2.0, 10.0], [2.0, 0.0, 20.0]]).expect("affine transform"),
        )
        .expect("placed pcurve"),
    );
    let mut scaled = original.clone();
    assert!(scaled.try_scale_coordinates([2.0, 3.0]).is_ok());
    for parameter in [0.0, 0.5, 1.0] {
        let expected = cadmpeg_ir::eval::decode::pcurve_uv(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &original,
            parameter,
        )
        .unwrap();
        let actual = cadmpeg_ir::eval::decode::pcurve_uv(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &scaled,
            parameter,
        )
        .unwrap();
        assert!((actual.u - expected.u * 2.0).abs() < 1.0e-12);
        assert!((actual.v - expected.v * 3.0).abs() < 1.0e-12);
    }
}

#[test]
fn anisotropic_parabola_scaling_scales_both_axes_and_keeps_the_parameter() {
    let original = PcurveGeometry::Parabola(
        cadmpeg_ir::geometry::pcurve::ParabolaPcurve::try_new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 1.0),
            1.0,
        )
        .unwrap(),
    );
    let mut scaled = original.clone();
    assert!(scaled.try_scale_coordinates([2.0, 3.0]).is_ok());
    // A parabola evaluates as `vertex + x_axis * t * t / (4 * focal) + y_axis * t`,
    // so a diagonal scale carries through the vertex and both axes and leaves
    // the focal distance and the parameter alone.
    assert!(
        matches!(&scaled, PcurveGeometry::Parabola(parabola)
            if *parabola.vertex() == Point2::new(0.0, 0.0)
                && *parabola.x_axis() == Point2::new(2.0, 0.0)
                && *parabola.y_axis() == Point2::new(0.0, 3.0)
                && parabola.focal_distance().get() == 1.0),
        "{scaled:?}"
    );
    for parameter in [0.0, 0.25, 1.0, 2.0] {
        let expected = cadmpeg_ir::eval::decode::pcurve_uv(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &original,
            parameter,
        )
        .unwrap();
        let actual = cadmpeg_ir::eval::decode::pcurve_uv(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &scaled,
            parameter,
        )
        .unwrap();
        assert_eq!(actual, Point2::new(expected.u * 2.0, expected.v * 3.0));
    }
}

#[test]
fn every_iso_si_prefix_resolves_to_its_exact_factor() {
    let expected = [
        ("EXA", 1e18),
        ("PETA", 1e15),
        ("TERA", 1e12),
        ("GIGA", 1e9),
        ("MEGA", 1e6),
        ("KILO", 1e3),
        ("HECTO", 1e2),
        ("DECA", 1e1),
        ("DECI", 1e-1),
        ("CENTI", 1e-2),
        ("MILLI", 1e-3),
        ("MICRO", 1.0e-6),
        ("NANO", 1.0e-9),
        ("PICO", 1.0e-12),
        ("FEMTO", 1e-15),
        ("ATTO", 1e-18),
    ];
    for (prefix, factor) in expected {
        assert_eq!(si_prefix(prefix), Some(factor), "prefix {prefix}");
    }
}

#[test]
fn prefixed_plane_angle_units_scale_to_radians() {
    crate::test_support::with_service_context(b"", |_, ctx| {
        let (exchange, _) = crate::test_support::with_service_context(
        b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;\
#1=(NAMED_UNIT(*) SI_UNIT(.MILLI.,.RADIAN.));\
#2=(NAMED_UNIT(*) SI_UNIT($,.RADIAN.));\
ENDSEC;END-ISO-10303-21;",
        crate::parse::parse_inner)
    .expect("parse plane-angle units");
        let mut active = BTreeSet::new();
        assert_eq!(
            unit_scale_radians(1, &exchange, &mut active, ctx)
                .expect("unit scale evaluation")
                .map(PositiveReal::get),
            Some(1.0e-3)
        );
        assert!(active.is_empty());
        assert_eq!(
            unit_scale_radians(2, &exchange, &mut active, ctx)
                .expect("unit scale evaluation")
                .map(PositiveReal::get),
            Some(1.0)
        );
        assert!(active.is_empty());
    });
}

#[test]
fn conversion_based_plane_angle_units_multiply_prefixed_base_scales() {
    crate::test_support::with_service_context(b"", |_, ctx| {
        let (exchange, _) = crate::test_support::with_service_context(
        b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;\
#1=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT(.MILLI.,.RADIAN.));\
#2=PLANE_ANGLE_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(2.),#1);\
#3=(CONVERSION_BASED_UNIT('two milli-radians',#2) NAMED_UNIT(*) PLANE_ANGLE_UNIT());\
#4=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\
#5=PLANE_ANGLE_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(2.),#4);\
#6=(CONVERSION_BASED_UNIT('two radians',#5) NAMED_UNIT(*) PLANE_ANGLE_UNIT());\
ENDSEC;END-ISO-10303-21;",
        crate::parse::parse_inner)
    .expect("parse conversion-based plane-angle units");
        let mut active = BTreeSet::new();
        assert_eq!(
            unit_scale_radians(3, &exchange, &mut active, ctx)
                .expect("unit scale evaluation")
                .map(PositiveReal::get),
            Some(2.0e-3)
        );
        assert!(active.is_empty());
        assert_eq!(
            unit_scale_radians(6, &exchange, &mut active, ctx)
                .expect("unit scale evaluation")
                .map(PositiveReal::get),
            Some(2.0)
        );
        assert!(active.is_empty());
    });
}

#[test]
fn recursive_unit_and_pcurve_failures_release_active_ids() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let (exchange, _) = crate::test_support::with_service_context(
        b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;\
#1=CONVERSION_BASED_UNIT('',#2);\
#2=UNKNOWN_FACTOR();\
#3=LINE('',#4,#5);\
#4=UNKNOWN_POINT();\
#5=UNKNOWN_VECTOR();\
#6=CURVE_REPLICA('',#6,#7);\
#7=UNKNOWN_OPERATOR();\
ENDSEC;END-ISO-10303-21;",
        crate::parse::parse_inner)
    .expect("parse recursive failure graph");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let mut active = BTreeSet::new();
    assert!(unit_scale_mm(1, &exchange, &mut active, &ctx)
        .expect("unit scale evaluation")
        .is_none());
    assert!(active.is_empty());
    assert!(unit_scale_radians(1, &exchange, &mut active, &ctx)
        .expect("unit scale evaluation")
        .is_none());
    assert!(active.is_empty());

    let mut losses = Vec::new();
    assert!(decode_pcurve_geometry(
        3,
        &exchange,
        super::super::PcurveSources {
            points: &BTreeMap::new(),
            vectors: &BTreeMap::new(),
            placements: &BTreeMap::new(),
            transformations: &BTreeMap::new(),
            angle_scale: 1.0
        },
        &mut losses,
        &mut active,
        0,
        &ctx
    )
    .expect("no resource refusal")
    .is_none());
    assert!(active.is_empty());
    assert!(decode_pcurve_geometry(
        6,
        &exchange,
        super::super::PcurveSources {
            points: &BTreeMap::new(),
            vectors: &BTreeMap::new(),
            placements: &BTreeMap::new(),
            transformations: &BTreeMap::new(),
            angle_scale: 1.0
        },
        &mut losses,
        &mut active,
        0,
        &ctx
    )
    .expect("no resource refusal")
    .is_none());
    assert!(active.is_empty());
}

#[test]
fn pcurve_trim_select_ignores_cartesian_point_coordinates() {
    let value = Value::List(vec![
        Value::Typed(
            "CARTESIAN_POINT".into(),
            Box::new(Value::List(vec![
                Value::Real(cadmpeg_ir::scalar::FiniteReal::new(17.0).expect("finite fixture")),
                Value::Real(cadmpeg_ir::scalar::FiniteReal::new(23.0).expect("finite fixture")),
            ])),
        ),
        Value::Real(cadmpeg_ir::scalar::FiniteReal::new(0.25).expect("finite fixture")),
    ]);
    assert_eq!(
        pcurve_trim_parameter(&value).map(FiniteReal::get),
        Some(0.25)
    );
}

#[test]
fn pcurve_trim_select_prefers_parameter_value() {
    let value = Value::List(vec![
        Value::Real(cadmpeg_ir::scalar::FiniteReal::new(17.0).expect("finite fixture")),
        Value::Typed(
            "PARAMETER_VALUE".into(),
            Box::new(Value::Real(
                cadmpeg_ir::scalar::FiniteReal::new(0.25).expect("finite fixture"),
            )),
        ),
    ]);
    assert_eq!(
        pcurve_trim_parameter(&value).map(FiniteReal::get),
        Some(0.25)
    );
}

#[test]
fn numerical_followup_periodic_edge_preserves_small_domain_phase() {
    use cadmpeg_ir::geometry::{nurbs::NurbsCurve, SolvedCurveGeometry};
    use cadmpeg_ir::math::Point3;
    for d in [1e-12, 1.0, 1e12] {
        let curve = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![-0.25 * d, 0., 0.25 * d, 0.5 * d, 0.75 * d, d, 1.25 * d],
            vec![
                Point3::new(0., 0., 0.),
                Point3::new(1., 0., 0.),
                Point3::new(1., 1., 0.),
                Point3::new(0., 1., 0.),
                Point3::new(0., 0., 0.),
            ],
            None,
            true,
        )
        .expect("fixture constructor admission")
        .unwrap();
        let [start, end] =
            [0.8 * d, 0.9 * d].map(|value| FiniteReal::new(value).expect("finite parameter"));
        let range =
            super::super::edge_parameter_range(&SolvedCurveGeometry::Nurbs(curve), start, end)
                .unwrap();
        assert!((range[0] / d - 0.8).abs() < 16. * f64::EPSILON);
        assert!((range[1] / d - 0.9).abs() < 16. * f64::EPSILON);
    }
}
