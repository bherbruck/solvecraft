// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

fn assert_surface_patch_refusal(operation: &'static str, retained: bool, contradictory: bool) {
    let mut group = group(2, 10);
    if !contradictory {
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
    }
    let mut first = recipe_edge_operand(10, &[], &[]);
    first.recipe_references = if contradictory {
        vec![recipe_reference(&[17]), recipe_reference(&[18])]
    } else {
        vec![recipe_reference(&[17])]
    };
    let mut second = recipe_edge_operand(11, &[], &[]);
    second.recipe_references = vec![recipe_reference(&[18])];
    let operands = [first, second];
    let feature_id =
        cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#surface-patch").unwrap();
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
                resolved_surface_patch_edge_group(
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

macro_rules! surface_patch_collection_refusal {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            assert_surface_patch_refusal($operation, false, false);
        }
    };
}

surface_patch_collection_refusal!(
    surface_patch_member_index_refuses_collection_limit,
    "f3d surface patch edge member index"
);
surface_patch_collection_refusal!(
    surface_patch_operand_refuses_collection_limit,
    "f3d surface patch matched edge operand"
);
surface_patch_collection_refusal!(
    surface_patch_recipe_edge_refuses_collection_limit,
    "f3d surface patch recipe edge"
);
surface_patch_collection_refusal!(
    surface_patch_distinct_edge_refuses_collection_limit,
    "f3d surface patch distinct recipe edge"
);
surface_patch_collection_refusal!(
    surface_patch_stable_slot_refuses_collection_limit,
    "f3d surface patch stable edge slot"
);
surface_patch_collection_refusal!(
    surface_patch_historical_edge_refuses_collection_limit,
    "f3d surface patch historical edge"
);

#[test]
fn surface_patch_recipe_edge_id_refuses_retained_limit() {
    assert_surface_patch_refusal("f3d surface patch recipe edge id", true, false);
}

#[test]
fn surface_patch_historical_group_id_refuses_retained_limit() {
    assert_surface_patch_refusal("f3d surface patch historical group id", true, false);
}

#[test]
fn surface_patch_native_group_id_refuses_retained_limit() {
    assert_surface_patch_refusal("f3d surface patch native group id", true, true);
}
