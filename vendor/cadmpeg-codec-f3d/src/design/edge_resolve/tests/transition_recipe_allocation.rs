// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::CodecError;

fn assert_transition_recipe_refusal(operation: &'static str, through_group: bool) {
    let operand = recipe_edge_operand(10, &[17, 18], &[]);
    let selection_group = group(2, 10);
    let persistent = identity(10, &[(17, 3.0), (18, 3.0)]);
    let feature_id = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#fillet").unwrap();
    for limit in 0..128 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = if through_group {
            resolved_edge_treatment_group_with_corners(
                &selection_group,
                crate::design::edge_resolve::EdgeTreatmentInputs {
                    groups: std::slice::from_ref(&selection_group),
                    operands: std::slice::from_ref(&operand),
                    identity_operands: std::slice::from_ref(&persistent),
                    vertex_operands: &[],
                    histories: &[],
                    previous_state_id: Some(7),
                    feature_id: &feature_id,
                    treatment_radius: None,
                },
                &ctx,
            )
            .map(|_| true)
        } else {
            transition_chain_is_supported_by_recipe(&[17], 1, &[&operand], &ctx)
        };
        match result {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected transition recipe refusal at {operation}: {other:?}"),
        }
    }
    panic!("no transition recipe refusal at {operation}");
}

#[test]
fn transition_recipe_member_refuses_collection_limit() {
    assert_transition_recipe_refusal("f3d transition recipe member", true);
}

#[test]
fn transition_recipe_edge_refuses_collection_limit() {
    assert_transition_recipe_refusal("f3d transition recipe edge", false);
}
