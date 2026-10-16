// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::CodecError;

fn assert_radius_refusal(operation: &'static str, identity_route: bool) {
    let mut recipe = recipe_edge_operand(10, &[], &[]);
    recipe.resolved_edge_slot = Some(19);
    recipe.treatment_radius_candidates = vec![
        crate::records::topology::edge_identity::DesignEdgeTreatmentRadiusCandidate {
            edge_slot: 17,
            radius: cadmpeg_ir::scalar::PositiveReal::new(3.0).unwrap(),
        },
    ];
    let mut persistent = identity(10, &[(17, 3.0), (18, 3.0)]);
    persistent.resolved_edge_slot = Some(19);
    for limit in 0..64 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = if identity_route {
            radius_edge_identity_group_candidates(&[&persistent], 3.0, &ctx)
        } else {
            crate::design::edge_resolve::radius_edge_group_candidates(&[&recipe], 3.0, &ctx)
        };
        match result {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected radius refusal at {operation}: {other:?}"),
        }
    }
    panic!("no radius refusal at {operation}");
}

#[test]
fn radius_recipe_edge_refuses_collection_limit() {
    assert_radius_refusal("f3d radius recipe edge", false);
}

#[test]
fn radius_candidate_edge_refuses_collection_limit() {
    assert_radius_refusal("f3d radius candidate edge", false);
}

#[test]
fn radius_identity_resolved_edge_refuses_collection_limit() {
    assert_radius_refusal("f3d radius identity resolved edge", true);
}

#[test]
fn radius_identity_candidate_edge_refuses_collection_limit() {
    assert_radius_refusal("f3d radius identity candidate edge", true);
}

#[test]
fn radius_identity_chain_edge_refuses_collection_limit() {
    assert_radius_refusal("f3d radius identity chain edge", true);
}
