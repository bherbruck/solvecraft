// SPDX-License-Identifier: Apache-2.0

fn path_error(max_items: u64, max_retained_bytes: u64) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use crate::records::feature::{
            extrude::DesignExtrudeOperation, path_features::DesignPipeConstruction,
            scope::DesignScopePayload, surface_ops::DesignPipeSectionShape,
        };
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = super::construction_group_limits::native(true, false);
        native.design_parameter_scopes[0]
            .try_edit(|draft| {
                draft.payload = DesignScopePayload::Pipe(Some(DesignPipeConstruction {
                    operation: DesignExtrudeOperation::NewBody,
                    operation_offset: 0,
                    section_shape: DesignPipeSectionShape::Circular,
                    section_shape_offset: 0,
                    filled: true,
                    filled_offset: 0,
                    values: crate::test_support::reals([0.0; 4]),
                    record_indexes: [100, 101, 102, 103],
                    value_offsets: [0; 4],
                }));
            })
            .unwrap();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        policy.limits.max_retained_bytes = max_retained_bytes;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_path_feature_operand_roles(&ctx, &mut Vec::new()).unwrap_err()
    })
}

#[test]
fn path_feature_operand_group_refuses_collection_limit() {
    let error = path_error(0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D path-feature operand groups")
    );
}

#[test]
fn path_feature_operand_role_refuses_collection_limit() {
    let error = path_error(1, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D path-feature operand roles")
    );
}

#[test]
fn path_feature_invalid_finding_refuses_collection_limit() {
    let error = path_error(2, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn path_feature_invalid_entity_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D validation entity",
        |cap| Err::<(), cadmpeg_core::CodecError>(path_error(u64::MAX, cap)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}
