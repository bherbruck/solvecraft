// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::PositiveLength;

const EPS_CIRCLE: f64 = 1.0e-6;

#[test]
fn circle_angle_ordering_refuses_sort_scratch_limit() {
    let points: Vec<_> = (0..21)
        .rev()
        .map(|index| {
            let angle = f64::from(index) * std::f64::consts::TAU / 21.0;
            Point2::new(angle.cos(), angle.sin())
        })
        .collect();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    // The sort scratch holds two index vectors for the 21 retained angles.
    policy.limits.max_materialized_bytes =
        u64::try_from(21 * 2 * std::mem::size_of::<usize>() - 1).unwrap();

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(crate::design::geometry::arrangement_circle_angles(&points,
        Point2::new(0.0, 0.0), PositiveLength::new(1.0).unwrap(), EPS_CIRCLE, &ctx),
        Err(CodecError::ResourceLimit(failure)) if failure.dimension == ResourceDimension::MaterializedBytes
            && failure.operation == "sort f3d arrangement circle angles")
    );
}
