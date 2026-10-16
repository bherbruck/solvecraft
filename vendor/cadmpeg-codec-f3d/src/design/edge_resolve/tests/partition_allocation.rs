// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::design::edge_resolve::{
    partition_unique_incomplete_edge_group, scope_partition_edge_group_candidates, EdgeGroupMember,
};
use cadmpeg_core::CodecError;

fn assert_partition_refusal(operation: &'static str, through_scope: bool) {
    let deleted = [17, 18];
    let groups = [
        vec![EdgeGroupMember {
            identity: 10,
            resolved_edge: Some(17),
            deleted_boundary_edges: &deleted,
        }],
        vec![EdgeGroupMember {
            identity: 12,
            resolved_edge: None,
            deleted_boundary_edges: &deleted,
        }],
    ];
    let source_group = group(2, 10);
    let target_group = group(3, 12);
    let mut source_operand = recipe_edge_operand(10, &[], &deleted);
    source_operand.resolved_edge_slot = Some(17);
    let target_operand = recipe_edge_operand(12, &[], &deleted);
    let scope_groups = [source_group, target_group];
    let operands = [source_operand, target_operand];
    for limit in 0..64 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = if through_scope {
            scope_partition_edge_group_candidates(
                &scope_groups[1],
                &scope_groups,
                &operands,
                scope_groups[1].members(),
                &ctx,
            )
        } else {
            partition_unique_incomplete_edge_group(1, &groups, &ctx)
        };
        match result {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected partition refusal at {operation}: {other:?}"),
        }
    }
    panic!("no partition refusal at {operation}");
}

#[test]
fn partition_group_refuses_collection_limit() {
    assert_partition_refusal("f3d edge partition group", true);
}

#[test]
fn partition_member_refuses_collection_limit() {
    assert_partition_refusal("f3d edge partition member", true);
}

#[test]
fn partition_identity_refuses_collection_limit() {
    assert_partition_refusal("f3d edge partition identity", false);
}

#[test]
fn partition_deleted_edge_refuses_collection_limit() {
    assert_partition_refusal("f3d edge partition deleted edge", false);
}

#[test]
fn partition_reserved_edge_refuses_collection_limit() {
    assert_partition_refusal("f3d edge partition reserved edge", false);
}

#[test]
fn partition_target_edge_refuses_collection_limit() {
    assert_partition_refusal("f3d edge partition target edge", false);
}
