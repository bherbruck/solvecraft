// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::CodecError;

fn assert_unique_refusal(operation: &'static str, group_route: bool) {
    let mut first = recipe_edge_operand(10, &[], &[]);
    first.resolved_edge_slot = Some(17);
    let mut second = recipe_edge_operand(11, &[], &[]);
    second.resolved_edge_slot = Some(18);
    let candidate_sets = [
        crate::design::edge_resolve::EdgeAssignmentCandidates::Edges(vec![17]),
        crate::design::edge_resolve::EdgeAssignmentCandidates::Context,
        crate::design::edge_resolve::EdgeAssignmentCandidates::Edges(vec![18]),
    ];
    for limit in 0..64 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = if group_route {
            crate::design::edge_resolve::unique_edge_group_assignment(&[&first, &second], &ctx)
        } else {
            crate::design::edge_resolve::unique_edge_assignment_with_context(&candidate_sets, &ctx)
        };
        match result {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected unique assignment refusal at {operation}: {other:?}"),
        }
    }
    panic!("no unique assignment refusal at {operation}");
}

#[test]
fn unique_edge_candidate_set_refuses_collection_limit() {
    assert_unique_refusal("f3d unique edge candidate set", true);
}

#[test]
fn unique_edge_assignment_candidate_refuses_collection_limit() {
    assert_unique_refusal("f3d unique edge assignment candidate", false);
}

#[test]
fn unique_edge_assignment_set_refuses_collection_limit() {
    assert_unique_refusal("f3d unique edge assignment set", false);
}
