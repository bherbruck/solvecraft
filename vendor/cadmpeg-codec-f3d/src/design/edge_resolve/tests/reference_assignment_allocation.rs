// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::CodecError;

fn context(
    edge: i64,
) -> crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext {
    serde_json::from_value(serde_json::json!({
        "reference_ordinal": 0,
        "result_faces": [],
        "result_shared_edge_slots": [],
        "preceding_faces": [],
        "shared_edge_slots": [],
        "changed_shared_edge_slots": [],
        "changed_reference_edge_slots": [edge],
    }))
    .unwrap()
}

fn assert_reference_assignment_refusal(operation: &'static str, route: u8) {
    let mut first = recipe_edge_operand(10, &[], &[17]);
    first.recipe_reference_contexts = vec![context(17)];
    let mut second = recipe_edge_operand(11, &[], &[18]);
    second.recipe_reference_contexts = vec![context(18)];
    let references = [vec![17], vec![18]];
    let deleted = [vec![17], vec![18]];
    for limit in 0..128 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = match route {
            0 => crate::design::edge_resolve::changed_reference_edge_group_candidates(
                &[&first, &second],
                &ctx,
            ),
            1 => crate::design::edge_resolve::deleted_reference_edge_group_candidates(
                &[&first, &second],
                &ctx,
            ),
            _ => crate::design::edge_resolve::unique_deleted_reference_assignment(
                &references,
                &deleted,
                &ctx,
            ),
        };
        match result {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected reference assignment refusal at {operation}: {other:?}"),
        }
    }
    panic!("no reference assignment refusal at {operation}");
}

#[test]
fn changed_reference_candidate_refuses_collection_limit() {
    assert_reference_assignment_refusal("f3d changed reference candidate", 0);
}

#[test]
fn changed_reference_candidate_set_refuses_collection_limit() {
    assert_reference_assignment_refusal("f3d changed reference candidate set", 0);
}

#[test]
fn deleted_reference_candidate_refuses_collection_limit() {
    assert_reference_assignment_refusal("f3d deleted reference candidate", 1);
}

#[test]
fn deleted_reference_candidate_set_refuses_collection_limit() {
    assert_reference_assignment_refusal("f3d deleted reference candidate set", 1);
}

#[test]
fn deleted_reference_boundary_edge_refuses_collection_limit() {
    assert_reference_assignment_refusal("f3d deleted reference boundary edge", 1);
}

#[test]
fn deleted_reference_boundary_set_refuses_collection_limit() {
    assert_reference_assignment_refusal("f3d deleted reference boundary set", 1);
}

#[test]
fn deleted_reference_shared_edge_refuses_collection_limit() {
    assert_reference_assignment_refusal("f3d deleted reference shared edge", 2);
}

#[test]
fn deleted_reference_shared_set_refuses_collection_limit() {
    assert_reference_assignment_refusal("f3d deleted reference shared set", 2);
}
