// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::CodecError;

fn assert_main_group_refusal_with_limit(
    operation: &'static str,
    surface_patch: bool,
    retained: bool,
) {
    let mut group = group(2, 10);
    let mut first = recipe_edge_operand(10, &[], &[]);
    first.resolved_edge_slot = Some(17);
    let mut operands = vec![first];
    if surface_patch {
        group
            .try_set_members(vec![
                crate::records::identity::Located {
                    value: 10,
                    offset: 0,
                },
                crate::records::identity::Located {
                    value: 11,
                    offset: 11,
                },
            ])
            .unwrap();
        operands[0].surface_patch_recipe_structure = Some(
            crate::records::topology::edge_recipe::DesignSurfacePatchRecipeStructure {
                clauses: std::array::from_fn(|_| {
                    crate::records::topology::edge_recipe::DesignSurfacePatchRecipeClause {
                        fields: Vec::new(),
                        face_reference_ordinals: [0, 0],
                        edge_reference_ordinals: [0, 0],
                        entries: Vec::new(),
                    }
                }),
            },
        );
        let mut second = recipe_edge_operand(11, &[], &[]);
        second.resolved_edge_slot = Some(18);
        second.surface_patch_recipe_structure = operands[0].surface_patch_recipe_structure.clone();
        operands.push(second);
    }
    let feature_id =
        cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#main-edge-group").unwrap();
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
                resolved_edge_group(
                    &group,
                    std::slice::from_ref(&group),
                    &operands,
                    &[],
                    Some(7),
                    &feature_id,
                    &ctx,
                )
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == operation && failure.dimension == (if retained { ResourceDimension::RetainedBytes } else { ResourceDimension::CollectionItems })));
    }
}

fn assert_main_group_refusal(operation: &'static str, surface_patch: bool) {
    assert_main_group_refusal_with_limit(operation, surface_patch, false);
}

#[test]
fn generic_surface_patch_member_index_refuses_collection_limit() {
    assert_main_group_refusal("f3d generic surface patch member index", true);
}

#[test]
fn generic_surface_patch_matched_operand_refuses_collection_limit() {
    assert_main_group_refusal("f3d generic surface patch matched operand", true);
}

#[test]
fn generic_surface_patch_resolved_slot_refuses_collection_limit() {
    assert_main_group_refusal("f3d generic surface patch resolved slot", true);
}

#[test]
fn generic_surface_patch_distinct_slot_refuses_collection_limit() {
    assert_main_group_refusal("f3d generic surface patch distinct slot", true);
}

#[test]
fn generic_surface_patch_historical_edge_refuses_collection_limit() {
    assert_main_group_refusal("f3d generic surface patch historical edge", true);
}

#[test]
fn generic_surface_patch_historical_group_id_refuses_retained_limit() {
    assert_main_group_refusal_with_limit(
        "f3d generic surface patch historical group id",
        true,
        true,
    );
}

#[test]
fn edge_group_member_identity_refuses_collection_limit() {
    assert_main_group_refusal("f3d edge group member identity", false);
}

#[test]
fn matched_edge_group_operand_refuses_collection_limit() {
    assert_main_group_refusal("f3d matched edge group operand", false);
}

#[test]
fn exact_edge_group_slot_refuses_collection_limit() {
    assert_main_group_refusal("f3d exact edge group slot", false);
}

#[test]
fn resolved_edge_group_historical_edge_refuses_collection_limit() {
    assert_main_group_refusal("f3d resolved edge group historical edge", false);
}

#[test]
fn resolved_edge_group_historical_group_id_refuses_retained_limit() {
    assert_main_group_refusal_with_limit(
        "f3d resolved edge group historical group id",
        false,
        true,
    );
}

#[test]
fn edge_group_matched_identity_refuses_collection_limit() {
    let group = group(2, 10);
    let operand = identity(10, &[]);
    let feature_id =
        cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#identity-index").unwrap();
    for limit in 0..128 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match resolved_edge_group(
            &group,
            std::slice::from_ref(&group),
            &[],
            std::slice::from_ref(&operand),
            Some(7),
            &feature_id,
            &ctx,
        ) {
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == "f3d edge group matched identity" =>
            {
                return
            }
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected matched identity refusal: {other:?}"),
        }
    }
    panic!("no matched identity refusal");
}

fn assert_identity_transition_refusal(operation: &'static str) {
    let group = group(2, 10);
    let operand = identity(10, &[(17, 3.0), (18, 5.0)]);
    let feature_id =
        cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#transition-index").unwrap();
    for limit in 0..128 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match resolved_edge_treatment_group_with_corners(
            &group,
            crate::design::edge_resolve::EdgeTreatmentInputs {
                groups: std::slice::from_ref(&group),
                operands: &[],
                identity_operands: std::slice::from_ref(&operand),
                vertex_operands: &[],
                histories: &[],
                previous_state_id: Some(7),
                feature_id: &feature_id,
                treatment_radius: None,
            },
            &ctx,
        ) {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected {operation} refusal: {other:?}"),
        }
    }
    panic!("no {operation} refusal");
}

#[test]
fn single_identity_transition_slot_refuses_collection_limit() {
    assert_identity_transition_refusal("f3d single identity transition slot");
}

#[test]
fn group_identity_transition_slot_refuses_collection_limit() {
    assert_identity_transition_refusal("f3d group identity transition slot");
}

#[test]
fn compared_identity_transition_slot_refuses_collection_limit() {
    assert_identity_transition_refusal("f3d compared identity transition slot");
}

fn assert_combined_edge_refusal(operation: &'static str) {
    let group = group(2, 10);
    let recipe = recipe_edge_operand(10, &[], &[]);
    let identity = identity(10, &[]);
    let feature_id =
        cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#combined-edge").unwrap();
    for limit in 0..128 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match resolved_edge_group(
            &group,
            std::slice::from_ref(&group),
            std::slice::from_ref(&recipe),
            std::slice::from_ref(&identity),
            Some(7),
            &feature_id,
            &ctx,
        ) {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected {operation} refusal: {other:?}"),
        }
    }
    panic!("no {operation} refusal");
}

#[test]
fn combined_edge_group_slot_refuses_collection_limit() {
    assert_combined_edge_refusal("f3d combined edge group slot");
}

#[test]
fn partial_edge_group_member_refuses_collection_limit() {
    assert_combined_edge_refusal("f3d partial edge group member");
}

fn assert_complete_identity_refusal(operation: &'static str, retained: bool) {
    let group = group(2, 10);
    let mut operand = identity(10, &[]);
    operand.resolved_edge_slot = Some(17);
    let feature_id =
        cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#complete-identity").unwrap();
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
                resolved_edge_group(
                    &group,
                    std::slice::from_ref(&group),
                    &[],
                    std::slice::from_ref(&operand),
                    Some(7),
                    &feature_id,
                    &ctx,
                )
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == operation && failure.dimension == (if retained { ResourceDimension::RetainedBytes } else { ResourceDimension::CollectionItems })));
    }
}

#[test]
fn identity_edge_slot_index_refuses_collection_limit() {
    assert_complete_identity_refusal("f3d identity edge slot index", false);
}

#[test]
fn identity_historical_edge_refuses_collection_limit() {
    assert_complete_identity_refusal("f3d identity historical edge", false);
}

#[test]
fn identity_historical_group_id_refuses_retained_limit() {
    assert_complete_identity_refusal("f3d identity historical group id", true);
}

#[derive(Clone, Copy)]
enum IdentityHistoricalRoute {
    Radius,
    Group,
    Single,
}

fn assert_identity_historical_refusal(
    route: IdentityHistoricalRoute,
    operation: &'static str,
    retained: bool,
) {
    let group = group(2, 10);
    let mut operand = identity(10, &[(17, 3.0), (18, 5.0)]);
    if matches!(route, IdentityHistoricalRoute::Single) {
        let mut draft = operand.into_draft();
        draft.layout = crate::records::topology::edge_identity::DesignEdgeIdentityLayout::Full;
        draft.asset_id_offset = draft.byte_offset + draft.layout.local_id_offset() + 18;
        draft.context_id_offset = draft.asset_id_offset + 76;
        operand = DesignEdgeIdentityOperand::try_new(draft).unwrap();
    }
    let radius = matches!(route, IdentityHistoricalRoute::Radius).then_some(3.0);
    let feature_id =
        cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#identity-history").unwrap();
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
                resolved_edge_treatment_group_with_corners(
                    &group,
                    crate::design::edge_resolve::EdgeTreatmentInputs {
                        groups: std::slice::from_ref(&group),
                        operands: &[],
                        identity_operands: std::slice::from_ref(&operand),
                        vertex_operands: &[],
                        histories: &[],
                        previous_state_id: Some(7),
                        feature_id: &feature_id,
                        treatment_radius: radius,
                    },
                    &ctx,
                )
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == operation && failure.dimension == (if retained { ResourceDimension::RetainedBytes } else { ResourceDimension::CollectionItems })));
    }
}

#[test]
fn radius_identity_historical_edge_refuses_collection_limit() {
    assert_identity_historical_refusal(
        IdentityHistoricalRoute::Radius,
        "f3d radius identity historical edge",
        false,
    );
}

#[test]
fn radius_identity_historical_group_id_refuses_retained_limit() {
    assert_identity_historical_refusal(
        IdentityHistoricalRoute::Radius,
        "f3d radius identity historical group id",
        true,
    );
}

#[test]
fn group_identity_historical_edge_refuses_collection_limit() {
    assert_identity_historical_refusal(
        IdentityHistoricalRoute::Group,
        "f3d group identity historical edge",
        false,
    );
}

#[test]
fn group_identity_historical_group_id_refuses_retained_limit() {
    assert_identity_historical_refusal(
        IdentityHistoricalRoute::Group,
        "f3d group identity historical group id",
        true,
    );
}

#[test]
fn single_identity_historical_edge_refuses_collection_limit() {
    assert_identity_historical_refusal(
        IdentityHistoricalRoute::Single,
        "f3d single identity historical edge",
        false,
    );
}

#[test]
fn single_identity_historical_group_id_refuses_retained_limit() {
    assert_identity_historical_refusal(
        IdentityHistoricalRoute::Single,
        "f3d single identity historical group id",
        true,
    );
}

fn assert_combined_historical_refusal(operation: &'static str, retained: bool) {
    let mut group = group(2, 10);
    group
        .try_set_members(vec![
            crate::records::identity::Located {
                value: 10,
                offset: 0,
            },
            crate::records::identity::Located {
                value: 11,
                offset: 11,
            },
        ])
        .unwrap();
    let mut first_recipe = recipe_edge_operand(10, &[], &[]);
    first_recipe.resolved_edge_slot = Some(17);
    let mut second_recipe = recipe_edge_operand(11, &[18], &[]);
    second_recipe.terminal_reference_edge_slots = vec![vec![18], vec![19]];
    let first_identity = identity(10, &[]);
    let mut second_identity = identity(11, &[]);
    second_identity.resolved_edge_slot = Some(18);
    let recipes = [first_recipe, second_recipe];
    let identities = [first_identity, second_identity];
    let feature_id =
        cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#combined-history").unwrap();
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
                resolved_edge_group(
                    &group,
                    std::slice::from_ref(&group),
                    &recipes,
                    &identities,
                    Some(7),
                    &feature_id,
                    &ctx,
                )
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == operation && failure.dimension == (if retained { ResourceDimension::RetainedBytes } else { ResourceDimension::CollectionItems })));
    }
}

#[test]
fn combined_historical_edge_refuses_collection_limit() {
    assert_combined_historical_refusal("f3d combined historical edge", false);
}

#[test]
fn combined_historical_group_id_refuses_retained_limit() {
    assert_combined_historical_refusal("f3d combined historical group id", true);
}

fn assert_native_group_refusal(standard_recipe: bool) {
    let group = group(2, 10);
    let mut operand = recipe_edge_operand(10, &[], &[]);
    if standard_recipe {
        operand.recipe_structure = Some(
            crate::records::topology::edge_recipe::DesignEdgeRecipeStructure {
                root: 1,
                sides: Vec::new(),
            },
        );
    }
    let feature_id =
        cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#native-group").unwrap();
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "f3d native edge group id",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                resolved_edge_group(
                    &group,
                    std::slice::from_ref(&group),
                    std::slice::from_ref(&operand),
                    &[],
                    standard_recipe.then_some(7),
                    &feature_id,
                    &ctx,
                )
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == "f3d native edge group id" && failure.dimension == (ResourceDimension::RetainedBytes)));
    }
}

#[test]
fn no_state_native_edge_group_id_refuses_retained_limit() {
    assert_native_group_refusal(false);
}

#[test]
fn standard_recipe_native_edge_group_id_refuses_retained_limit() {
    assert_native_group_refusal(true);
}

#[test]
fn edge_assignment_reference_candidate_refuses_collection_limit() {
    for limit in 0..16 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match crate::design::edge_resolve::edge_group_assignment_candidates(
            &[],
            [&[17, 18][..], &[18, 19][..]],
            &ctx,
        ) {
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == "f3d edge assignment reference candidate" =>
            {
                return
            }
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected assignment reference refusal: {other:?}"),
        }
    }
    panic!("no assignment reference refusal");
}

#[test]
fn corroborated_edge_candidate_refuses_collection_limit() {
    let selectors = [
        crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext {
            selector: 0,
            clauses: Vec::new(),
            incidence_matching_edge_slots: vec![17, 18],
            boundary_count_matching_edge_slots: Vec::new(),
        },
    ];
    for limit in 0..16 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match crate::design::edge_resolve::edge_assignment_candidates(
            &selectors,
            [&[17, 18][..]],
            &ctx,
        ) {
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == "f3d corroborated edge candidate" =>
            {
                return
            }
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected corroborated edge refusal: {other:?}"),
        }
    }
    panic!("no corroborated edge refusal");
}
