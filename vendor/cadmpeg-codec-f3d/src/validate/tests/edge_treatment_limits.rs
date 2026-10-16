// SPDX-License-Identifier: Apache-2.0

fn group_error(max_items: u64, max_retained: u64) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = crate::native::F3dNative::default();
        native
            .design_parameter_scopes
            .push(DesignParameterScope::empty(
                "f3d:Design/BulkStream.dat:design-parameter-scope#10",
                DesignFeatureKind::Fillet,
                10,
            ));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        policy.limits.max_retained_bytes = max_retained;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_edge_treatment_groups(
            &ctx,
            &mut Vec::new(),
            &std::collections::HashSet::new(),
            &std::collections::HashSet::new(),
            &std::collections::HashSet::new(),
        )
        .unwrap_err()
    })
}

#[test]
fn edge_treatment_incomplete_group_finding_refuses_collection_limit() {
    let error = group_error(0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn edge_treatment_incomplete_group_entity_refuses_retained_limit() {
    let error = group_error(u64::MAX, 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}
