// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::CodecError;

fn context(
    edge: i64,
) -> crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext {
    serde_json::from_value(serde_json::json!({
        "reference_ordinal": 0, "result_faces": [], "result_shared_edge_slots": [],
        "preceding_faces": [], "shared_edge_slots": [], "changed_shared_edge_slots": [],
        "changed_reference_edge_slots": [edge],
    }))
    .unwrap()
}

fn assert_contextual_refusal(operation: &'static str) {
    let structure = || crate::records::topology::edge_recipe::DesignEdgeRecipeStructure {
        root: 2,
        sides: Vec::new(),
    };
    let mut first = recipe_edge_operand(10, &[], &[]);
    first.preceding_boundary_edge_slots = vec![17, 18];
    first.recipe_structure = Some(structure());
    first.recipe_references = vec![recipe_reference(&[])];
    first.recipe_reference_contexts = vec![context(17)];
    let mut second = recipe_edge_operand(11, &[17, 18], &[17, 18]);
    second.preceding_boundary_edge_slots = vec![17, 18];
    second.recipe_structure = Some(structure());
    second.recipe_references = vec![recipe_reference(&[]), recipe_reference(&[])];
    second.recipe_reference_contexts = vec![context(17), context(18)];
    for limit in 0..96 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match contextual_deleted_edge_group_candidates(&[&first, &second], &ctx) {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected contextual deletion refusal at {operation}: {other:?}"),
        }
    }
    panic!("no contextual deletion refusal at {operation}");
}

#[test]
fn contextual_deleted_edge_refuses_collection_limit() {
    assert_contextual_refusal("f3d contextual deleted edge");
}

#[test]
fn contextual_deleted_candidate_refuses_collection_limit() {
    assert_contextual_refusal("f3d contextual deleted candidate");
}

#[test]
fn contextual_deleted_candidate_set_refuses_collection_limit() {
    assert_contextual_refusal("f3d contextual deleted candidate set");
}
