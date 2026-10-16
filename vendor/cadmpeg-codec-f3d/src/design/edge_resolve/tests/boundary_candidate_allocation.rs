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

fn assert_boundary_refusal(operation: &'static str, route: u8) {
    use crate::records::topology::edge_recipe::{
        DesignEdgeRecipeSelectorContext, DesignEdgeRecipeStructure, DesignTopologyRecipeSide,
    };
    let selector = |edge| DesignEdgeRecipeSelectorContext {
        selector: 0,
        clauses: vec![None, None],
        incidence_matching_edge_slots: Vec::new(),
        boundary_count_matching_edge_slots: vec![edge],
    };
    let first = [selector(17)];
    let second = [selector(18)];
    let mut deleted = recipe_edge_operand(10, &[17], &[17]);
    deleted.preceding_boundary_edge_slots = vec![17];
    deleted.recipe_structure = Some(DesignEdgeRecipeStructure {
        root: 2,
        sides: Vec::new(),
    });
    deleted.recipe_references = vec![recipe_reference(&[])];
    deleted.recipe_reference_contexts = vec![context(17)];
    let mut result = recipe_edge_operand(11, &[], &[]);
    result.recipe_structure = Some(DesignEdgeRecipeStructure {
        root: 2,
        sides: (0..2)
            .map(|_| DesignTopologyRecipeSide {
                header_value: 2,
                scalars: vec![1, 0],
                payload_prefix: vec![0],
                entries: Vec::new(),
            })
            .collect(),
    });
    result.recipe_references = vec![recipe_reference(&[]), recipe_reference(&[])];
    result.recipe_reference_contexts = vec![context(19), context(19)];
    result.result_boundary_edge_slots = vec![19];
    for limit in 0..64 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let candidate = match route {
            0 => crate::design::edge_resolve::common_deleted_edge_group_candidates(
                [(true, &[17, 18][..]), (true, &[18, 17][..])],
                &ctx,
            ),
            1 => crate::design::edge_resolve::deleted_boundary_edge_group_candidates(
                &[&deleted],
                &ctx,
            ),
            2 => crate::design::edge_resolve::result_boundary_reference_edge_group_candidates(
                &[&result],
                &ctx,
            ),
            _ => crate::design::edge_resolve::changed_boundary_count_edge_group_candidates(
                [first.as_slice(), second.as_slice()],
                &ctx,
            ),
        };
        match candidate {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected boundary refusal at {operation}: {other:?}"),
        }
    }
    panic!("no boundary refusal at {operation}");
}

#[test]
fn common_deleted_candidate_refuses_collection_limit() {
    assert_boundary_refusal("f3d common deleted candidate", 0);
}

#[test]
fn common_deleted_normalized_edge_refuses_collection_limit() {
    assert_boundary_refusal("f3d common deleted normalized edge", 0);
}

#[test]
fn deleted_boundary_contextual_edge_refuses_collection_limit() {
    assert_boundary_refusal("f3d deleted boundary contextual edge", 1);
}

#[test]
fn deleted_boundary_edge_refuses_collection_limit() {
    assert_boundary_refusal("f3d deleted boundary edge", 1);
}

#[test]
fn result_boundary_candidate_refuses_collection_limit() {
    assert_boundary_refusal("f3d result boundary candidate", 2);
}

#[test]
fn boundary_count_candidate_refuses_collection_limit() {
    assert_boundary_refusal("f3d boundary-count candidate", 3);
}
