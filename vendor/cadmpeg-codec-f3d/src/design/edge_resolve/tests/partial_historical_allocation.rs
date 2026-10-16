// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::CodecError;

fn assert_partial_refusal(operation: &'static str, retained: bool) {
    let state =
        cadmpeg_ir::ids::FeatureInputTopologyId::mint("f3d:history-input:state#feature").unwrap();
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            if retained {
                ResourceDimension::RetainedBytes
            } else {
                ResourceDimension::CollectionItems
            },
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                if retained {
                    policy.limits.max_retained_bytes = cap;
                } else {
                    policy.limits.max_collection_items = cap;
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                partial_historical_edge_selection(
                    [("operand-a", Some(17)), ("operand-b", None)],
                    41,
                    cadmpeg_ir::identity_key!("feature").as_str(),
                    state.clone(),
                    "group",
                    &ctx,
                )
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == operation && failure.dimension == (if retained { ResourceDimension::RetainedBytes } else { ResourceDimension::CollectionItems })));
    }
}

#[test]
fn partial_edge_slot_refuses_collection_limit() {
    assert_partial_refusal("f3d partial edge slot", false);
}

#[test]
fn partial_unresolved_id_refuses_retained_limit() {
    assert_partial_refusal("f3d partial unresolved id", true);
}

#[test]
fn partial_unresolved_member_refuses_collection_limit() {
    assert_partial_refusal("f3d partial unresolved member", false);
}

#[test]
fn partial_historical_edge_refuses_collection_limit() {
    assert_partial_refusal("f3d partial historical edge", false);
}

#[test]
fn partial_native_id_refuses_retained_limit() {
    assert_partial_refusal("f3d partial native id", true);
}
