// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::CodecError;

fn assert_fixed_fillet_refusal(operation: &'static str, variable: bool, complete: bool) {
    let mut scope_value = serde_json::to_value(fixed_scope()).unwrap();
    if variable {
        scope_value["fixed_fillet_parameters"]["groups"][0]["radii"] =
            serde_json::json!([1.0, 2.0, 3.0]);
        scope_value["fixed_fillet_parameters"]["groups"][0]["radius_record_indexes"] =
            serde_json::json!([5, 6, 7]);
        scope_value["fixed_fillet_parameters"]["groups"][0]["radius_offsets"] =
            serde_json::json!([0, 8, 16]);
        scope_value["fixed_fillet_parameters"]["groups"][0]["intermediate_parameters"] =
            serde_json::json!([0.5]);
        scope_value["fixed_fillet_parameters"]["groups"][0]
            ["intermediate_parameter_record_indexes"] = serde_json::json!([8]);
        scope_value["fixed_fillet_parameters"]["groups"][0]["intermediate_parameter_offsets"] =
            serde_json::json!([24]);
    }
    let scope: DesignParameterScope = serde_json::from_value(scope_value).unwrap();
    let selection_group = group(2, 10);
    let mut operand = recipe_edge_operand(10, &[], &[]);
    operand.recipe_state_id = Some(7);
    operand.resolved_edge_slot = Some(17);
    let identity = identity(10, &[(17, 3.0), (18, 5.0), (19, 3.0)]);
    for limit in 0..256 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let operands = if complete {
            std::slice::from_ref(&operand)
        } else {
            &[]
        };
        let identities = if complete {
            &[][..]
        } else {
            std::slice::from_ref(&identity)
        };
        match project_fixed_fillet(
            &scope,
            std::slice::from_ref(&selection_group),
            operands,
            identities,
            &ctx,
        ) {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected fixed Fillet refusal at {operation}: {other:?}"),
        }
    }
    panic!("no fixed Fillet refusal at {operation}");
}

#[test]
fn fixed_fillet_scope_group_refuses_collection_limit() {
    assert_fixed_fillet_refusal("f3d fixed fillet scope group", false, false);
}

#[test]
fn fixed_fillet_complete_group_refuses_collection_limit() {
    assert_fixed_fillet_refusal("f3d fixed fillet complete group", false, true);
}

#[test]
fn fixed_fillet_identity_refuses_collection_limit() {
    assert_fixed_fillet_refusal("f3d fixed fillet identity", false, false);
}

#[test]
fn fixed_fillet_output_group_refuses_collection_limit() {
    assert_fixed_fillet_refusal("f3d fixed fillet output group", false, false);
}

#[test]
fn fixed_fillet_radius_point_refuses_collection_limit() {
    assert_fixed_fillet_refusal("f3d fixed fillet radius point", true, true);
}
