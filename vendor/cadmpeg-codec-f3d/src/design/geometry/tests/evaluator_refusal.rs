// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::CodecError;

#[test]
fn sketch_nurbs_point_refuses_polynomial_input_copy_limit() {
    let curve = PcurveNurbs::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        None,
        false,
    )
    .expect("fixture pcurve construction admission")
    .unwrap();
    let geometry = SketchGeometry::try_from(SketchGeometryDefinition::Nurbs { curve }).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;
    // Pole and weight copies have separate admission operations; knots are borrowed.
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::sketch_geometry_point(&geometry, 0.5, &ctx).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "f3d nurbs evaluator poles"));
}

#[test]
fn sketch_nurbs_point_refuses_rational_input_copy_limit() {
    let curve = PcurveNurbs::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        Some(vec![1.0, 1.0]),
        false,
    )
    .expect("fixture pcurve construction admission")
    .unwrap();
    let geometry = SketchGeometry::try_from(SketchGeometryDefinition::Nurbs { curve }).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 2;
    // Pole and weight copies have separate admission operations; knots are borrowed.
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::sketch_geometry_point(&geometry, 0.5, &ctx).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "f3d nurbs evaluator weights"));
}

#[test]
fn certified_nurbs_tubes_refuse_point_copy_limit() {
    let curve = PcurveNurbs::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        None,
        false,
    )
    .expect("fixture pcurve construction admission")
    .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;

    // Pole and weight copies have separate admission operations; knots are borrowed.
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        super::super::certified_nurbs_tubes(&curve, 0.5, &ctx),
        Err(CodecError::ResourceLimit(limit)) if limit.operation == "f3d nurbs tube points"
    ));
}

#[test]
fn certified_nurbs_tubes_refuse_weight_copy_limit() {
    let curve = PcurveNurbs::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        Some(vec![1.0, 1.0]),
        false,
    )
    .expect("fixture pcurve construction admission")
    .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 2;

    // Pole and weight copies have separate admission operations; knots are borrowed.
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        super::super::certified_nurbs_tubes(&curve, 0.5, &ctx),
        Err(CodecError::ResourceLimit(limit)) if limit.operation == "f3d nurbs tube weights"
    ));
}

#[test]
fn sketch_nurbs_endpoints_refuse_pole_copy_limit() {
    let curve = PcurveNurbs::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        None,
        false,
    )
    .expect("fixture pcurve construction admission")
    .unwrap();
    let sketch_id = SketchId::mint("synthetic:test:id#nurbs-endpoint-sketch").unwrap();
    let entity = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#nurbs-endpoint-curve").unwrap(),
        sketch_id,
        SketchGeometry::try_from(SketchGeometryDefinition::Nurbs { curve }).unwrap(),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;

    // Pole and weight copies have separate admission operations; knots are borrowed.
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        super::super::sketch_entity_endpoints(&entity, &ctx),
        Err(CodecError::ResourceLimit(limit)) if limit.operation == "f3d nurbs evaluator poles"
    ));
}

#[test]
fn closed_sketch_nurbs_endpoints_propagate_collection_refusal() {
    let curve = PcurveNurbs::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        None,
        false,
    )
    .expect("fixture pcurve construction admission")
    .unwrap();
    let sketch_id = SketchId::mint("synthetic:test:id#closed-nurbs-sketch").unwrap();
    let entity = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#closed-nurbs-curve").unwrap(),
        sketch_id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Nurbs { curve }).unwrap(),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;

    // Pole and weight copies have separate admission operations; knots are borrowed.
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        closed_sketch_profiles(&ctx, &sketch_id, &[entity], 0.01),
        Err(CodecError::ResourceLimit(limit)) if limit.operation == "f3d nurbs evaluator poles"
    ));
}

#[test]
fn coincident_nurbs_loci_propagate_endpoint_refusal() {
    let curve = PcurveNurbs::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        None,
        false,
    )
    .expect("fixture pcurve construction admission")
    .unwrap();
    let sketch_id = SketchId::mint("synthetic:test:id#coincident-nurbs-sketch").unwrap();
    let nurbs = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#coincident-nurbs-curve").unwrap(),
        sketch_id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Nurbs { curve }).unwrap(),
    );
    let point = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#coincident-nurbs-point").unwrap(),
        sketch_id,
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(0.0, 0.0),
        })
        .unwrap(),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;

    // Pole and weight copies have separate admission operations; knots are borrowed.
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::design::dimensions::exact_coincident_loci(&[&nurbs, &point], &ctx),
        Err(CodecError::ResourceLimit(limit)) if limit.operation == "f3d nurbs evaluator poles"
    ));
}

#[test]
fn sketch_nurbs_point_preserves_caller_scratch_refusal() {
    let curve = PcurveNurbs::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        3,
        vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        vec![
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(2.0, 0.0),
            Point2::new(3.0, 0.0),
        ],
        None,
        false,
    )
    .expect("fixture pcurve construction admission")
    .unwrap();
    let geometry = SketchGeometry::try_from(SketchGeometryDefinition::Nurbs { curve }).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let error = super::super::sketch_geometry_point(&geometry, 0.5, ctx).unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
                && ctx.resource_refusal() == Some(limit)));
    });
}

#[test]
fn certified_nurbs_tubes_preserve_caller_scratch_refusal() {
    let curve = PcurveNurbs::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        3,
        vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        vec![
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(2.0, 0.0),
            Point2::new(3.0, 0.0),
        ],
        None,
        false,
    )
    .expect("fixture pcurve construction admission")
    .unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let error = super::super::certified_nurbs_tubes(&curve, 0.5, ctx)
            .err()
            .expect("caller scratch refusal");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
                && ctx.resource_refusal() == Some(limit)));
    });
}
