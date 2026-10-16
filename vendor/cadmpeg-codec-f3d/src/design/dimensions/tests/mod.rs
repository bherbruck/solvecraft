// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]

mod atomic_limits;
mod frame_relations;
mod limits;
mod linear;
mod offset;
mod offset_binding;
mod owner_scoped;
mod recipe;
mod relations;

fn project_dimension_constraints(
    inputs: &crate::design::dimensions::DimensionConstraintInputs<'_>,
    spatial_sketches: &[cadmpeg_ir::sketches::SpatialSketch],
) -> Vec<cadmpeg_ir::sketches::SketchConstraint> {
    crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::dimensions::project_dimension_constraints(
            decode_ctx,
            inputs,
            spatial_sketches,
            1.0e-6,
        )
    })
    .expect("resource allocation did not fail")
}

fn project_spatial_dimension_constraints(
    inputs: &crate::design::dimensions::DimensionConstraintInputs<'_>,
    spatial_sketches: &[cadmpeg_ir::sketches::SpatialSketch],
    spatial_entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
) -> Vec<cadmpeg_ir::sketches::SpatialSketchConstraint> {
    crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::dimensions::project_spatial_dimension_constraints(
            decode_ctx,
            inputs,
            spatial_sketches,
            spatial_entities,
            1.0e-6,
        )
    })
    .expect("resource allocation did not fail")
}

mod numerical_ranges;
mod parameter_iteration;

fn assert_dimension_refusal(
    operation: &'static str,
    dimension: cadmpeg_core::decode::ResourceDimension,
    run: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<(), cadmpeg_core::CodecError>,
) {
    let error = crate::test_support::resource_refusal_at(dimension, operation, 0, run);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == dimension && limit.operation == operation)
    );
}

mod spatial_parallel_limits;

mod spatial_carrier_limits;

mod spatial_repeated_limits;

mod spatial_reflection_limits;

mod null_locus_limits;

mod two_locus_limits;

mod nurbs_containment_limits;

mod relation_kind_limits;
