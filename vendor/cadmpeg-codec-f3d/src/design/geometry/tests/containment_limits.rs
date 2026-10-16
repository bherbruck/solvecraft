// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchEntity, SketchEntityId, SketchGeometry, SketchId};
const EPS_CONTAINMENT: f64 = 1.0e-6;
fn fixture(limit: u64, operation: &'static str) {
    let entity = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:entity#containment").unwrap(),
        SketchId::mint("synthetic:test:sketch#containment").unwrap(),
        SketchGeometry::nurbs(
            PcurveNurbs::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                vec![
                    Point2::new(1.0, 2.0),
                    Point2::new(2.0, 4.0),
                    Point2::new(3.0, 2.0),
                ],
                Some(vec![1.0, 0.5, 1.0]),
                false,
            )
            .expect("fixture pcurve construction admission")
            .unwrap(),
        ),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(crate::design::geometry::point_on_sketch_entity(&ctx, Point2::new(3.0, 2.0), &entity, EPS_CONTAINMENT), Err(CodecError::ResourceLimit(failure)) if failure.operation == operation && failure.dimension == ResourceDimension::CollectionItems)
    );
}
#[test]
fn geometry_nurbs_containment_poles_refuse_collection_limit() {
    fixture(2, "f3d nurbs evaluator poles");
}
#[test]
fn geometry_nurbs_containment_weights_refuse_collection_limit() {
    fixture(5, "f3d nurbs evaluator weights");
}
