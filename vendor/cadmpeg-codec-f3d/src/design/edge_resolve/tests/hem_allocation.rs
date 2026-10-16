// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::CodecError;

fn assert_hem_candidate_refusal(operation: &'static str) {
    let references: [&[i64]; 4] = [&[], &[167], &[106], &[110]];
    for limit in 0..32 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match unique_hem_transition_edge_candidate(&[63, 106, 110, 167], references, &ctx) {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected Hem refusal at {operation}: {other:?}"),
        }
    }
    panic!("no Hem refusal at {operation}");
}

#[test]
fn hem_reference_set_refuses_collection_limit() {
    assert_hem_candidate_refusal("f3d hem reference edge set");
}

#[test]
fn hem_support_edge_refuses_collection_limit() {
    assert_hem_candidate_refusal("f3d hem support edge");
}

#[test]
fn hem_candidate_edge_refuses_collection_limit() {
    assert_hem_candidate_refusal("f3d hem candidate edge");
}

#[test]
fn hem_historical_group_id_refuses_retained_limit() {
    let selection_group = group(2, 10);
    let mut operand = recipe_edge_operand(10, &[63, 106, 110, 167], &[]);
    operand.recipe_reference_contexts = [None, Some(167), Some(106), Some(110)]
        .into_iter()
        .enumerate()
        .map(|(ordinal, edge)| {
            serde_json::from_value(serde_json::json!({
                "reference_ordinal": ordinal,
                "result_faces": if edge.is_some() { vec!["f3d:model:face#1"] } else { Vec::new() },
                "result_shared_edge_slots": [],
                "preceding_faces": [],
                "preceding_support_face_slots": if edge.is_some() { vec![2] } else { Vec::new() },
                "shared_edge_slots": [],
                "changed_shared_edge_slots": [],
                "changed_reference_edge_slots": edge.into_iter().collect::<Vec<_>>(),
            }))
            .unwrap()
        })
        .collect();
    let feature_id = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#hem").unwrap();
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "f3d hem historical group id",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                crate::design::edge_resolve::resolved_hem_edge_group(
                    &selection_group,
                    std::slice::from_ref(&selection_group),
                    std::slice::from_ref(&operand),
                    &[],
                    Some(7),
                    &feature_id,
                    &ctx,
                )
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == "f3d hem historical group id" && failure.dimension == (ResourceDimension::RetainedBytes)));
    }
}
