// SPDX-License-Identifier: Apache-2.0

use super::extrude_limits::{profile_group, scope};
use crate::design::feature_project::project_fixed_sweep;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::path_features::DesignSweepConstruction;
use crate::records::feature::scope::{DesignScopePayload, DesignSweepScope};
use crate::records::topology::construction::DesignConstructionOperandRole;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::scalar::FiniteReal;

fn sweep_input() -> (
    crate::records::feature::scope::DesignParameterScope,
    [crate::records::topology::construction::DesignConstructionOperandGroup; 2],
) {
    let mut scope = scope();
    scope
        .try_edit(|draft| {
            draft.payload = DesignScopePayload::Sweep(Some(DesignSweepScope {
                construction: Some(DesignSweepConstruction {
                    operation: DesignExtrudeOperation::NewBody,
                    operation_offset: 128,
                    values: [0.0, 0.0, 1.0, 1.0, 0.0, 0.0]
                        .map(|value| FiniteReal::new(value).unwrap()),
                    record_indexes: [0; 6],
                    value_offsets: [128; 6],
                }),
                sweep_profile: None,
            }));
        })
        .unwrap();
    let mut profile = profile_group(100, 0);
    profile.operand_role = DesignConstructionOperandRole::Other(DesignOperandRole::PROFILE);
    let mut path = profile_group(101, 1);
    path.operand_role = DesignConstructionOperandRole::Other(DesignOperandRole::ROLE_0X5);
    (scope, [profile, path])
}

fn project(
    ctx: &DecodeContext<'_>,
    scope: &crate::records::feature::scope::DesignParameterScope,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
) -> Result<Option<FeatureDefinition>, CodecError> {
    project_fixed_sweep(scope, groups, &[], &[], &[], &[], ctx)
}

fn assert_sweep_limit(operation: &'static str, dimension: ResourceDimension) {
    let (scope, groups) = sweep_input();
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| project(decode_ctx, &scope, &groups))
            .unwrap()
            .unwrap(),
        FeatureDefinition::Operation(FeatureOperation::Sweep { .. })
    ));
    for limit in 0..256 {
        let mut policy = DecodePolicy::default();
        match dimension {
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
            _ => panic!("unsupported Sweep test resource dimension"),
        }
        let arena = DecodeArena::new();
        let refusal_cap =
            match cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match dimension {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                (project(&ctx, &scope, &groups)).map(|_| ())
            }) {
                cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
                error => panic!("unexpected refusal: {error:?}"),
            };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match dimension {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        if matches!(project(&ctx, &scope, &groups),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == dimension && failure.operation == operation
        ) {
            return;
        }
    }
    panic!("no Sweep refusal at {operation}");
}

#[test]
fn sweep_scope_group_refuses_collection_limit() {
    assert_sweep_limit("f3d Sweep scope group", ResourceDimension::CollectionItems);
}

#[test]
fn sweep_profile_group_refuses_collection_limit() {
    assert_sweep_limit(
        "f3d Sweep profile group",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn sweep_path_group_refuses_collection_limit() {
    assert_sweep_limit("f3d Sweep path group", ResourceDimension::CollectionItems);
}

#[test]
fn sweep_profile_id_refuses_retained_limit() {
    assert_sweep_limit("f3d Sweep profile id", ResourceDimension::RetainedBytes);
}

#[test]
fn sweep_body_group_refuses_collection_limit() {
    let (scope, [profile, path]) = sweep_input();
    let mut body = profile_group(102, 2);
    body.operand_role = DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_A);
    let groups = [profile, path, body];
    assert!(
        crate::test_support::with_decode_context(|decode_ctx| project(decode_ctx, &scope, &groups))
            .unwrap()
            .is_none()
    );
    for limit in 0..32 {
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let arena = DecodeArena::new();

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        if matches!(project(&ctx, &scope, &groups),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == "f3d Sweep body group"
        ) {
            return;
        }
    }
    panic!("no Sweep body group refusal");
}

fn guide_surface_input() -> (
    crate::records::feature::scope::DesignParameterScope,
    Vec<crate::records::topology::construction::DesignConstructionOperandGroup>,
    crate::records::topology::entity_selection::DesignEntitySelectionOperand,
) {
    let (mut scope, [mut selected, path]) = sweep_input();
    selected
        .try_set_members(vec![crate::records::identity::Located {
            value: 2788,
            offset: selected.members()[0].offset,
        }])
        .unwrap();
    let mut carrier = profile_group(102, 3);
    carrier.operand_role = DesignConstructionOperandRole::Other(DesignOperandRole::PROFILE);
    carrier
        .try_set_members(vec![crate::records::identity::Located {
            value: 2795,
            offset: carrier.members()[0].offset,
        }])
        .unwrap();
    let mut guide = profile_group(103, 4);
    guide.operand_role = DesignConstructionOperandRole::Other(DesignOperandRole::FACES);
    let profile = crate::records::topology::sketch_profile::DesignSketchProfileOperand::try_new(
        crate::records::topology::sketch_profile::DesignSketchProfileOperandDraft {
            scope_reference_ordinal: 3,
            record_index: 2795,
            byte_offset: 32_000,
            class_tag: "312".to_owned().try_into().unwrap(),
            asset_id: "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
                .to_owned()
                .try_into()
                .unwrap(),
            asset_id_offset: 32_040,
            entity_id: "0_2718".to_owned().try_into().unwrap(),
            entity_reference_offset: 32_080,
            region_selection: None,
            paired_class_tag: "258".to_owned().try_into().unwrap(),
            paired_byte_offset: 32_180,
        },
    )
    .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::Sweep(slot) = scope.payload_mut()
    {
        slot.get_or_insert_with(Default::default).sweep_profile = Some(profile);
    }
    let selection =
        crate::records::topology::entity_selection::DesignEntitySelectionOperand::try_new(
            crate::records::topology::entity_selection::DesignEntitySelectionOperandDraft {
                id: "f3d:Design/BulkStream.dat:entity-selection#2788".into(),
                scope_record_index: scope.record_index,
                group_record_index: selected.record_index,
                group_member_ordinal: 0,
                record_index: 2788,
                byte_offset: 31_000,
                class_tag: "310".to_owned().try_into().unwrap(),
                asset_id: "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                asset_id_offset: 31_040,
                context_id: "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                context_id_offset: 31_080,
                identity_record_index: 2791,
                identity_record_offset: 31_180,
                primary_identity: 2718,
                primary_identity_offset: 31209,
                secondary: Some(crate::records::identity::DesignSecondaryIdentity {
                    identity: crate::records::identity::Located {
                        value: 164,
                        offset: 31217,
                    },
                    curve_identity: None,
                }),
                historical_edge_candidates: Vec::new(),
                historical_face_candidates: Vec::new(),
                resolved_edge_slot: None,
                next_record_index: 2792,
                next_byte_offset: 31225,
            },
        )
        .unwrap();
    (scope, vec![selected, carrier, path, guide], selection)
}

fn assert_guide_surface_limit(operation: &'static str, dimension: ResourceDimension) {
    let (scope, groups, selection) = guide_surface_input();
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| project_fixed_sweep(
            &scope,
            &groups,
            &[],
            &[],
            std::slice::from_ref(&selection),
            &[],
            decode_ctx
        ))
        .unwrap()
        .unwrap(),
        FeatureDefinition::Operation(FeatureOperation::Sweep {
            orientation: Some(cadmpeg_ir::features::SweepOrientation::GuideSurface {
                faces: cadmpeg_ir::features::FaceSelection::Native(_),
            }),
            ..
        })
    ));
    for limit in 0..256 {
        let mut policy = DecodePolicy::default();
        match dimension {
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
            _ => panic!("unsupported Sweep guide resource dimension"),
        }
        let arena = DecodeArena::new();
        let refusal_cap =
            match cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match dimension {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                (project_fixed_sweep(
                    &scope,
                    &groups,
                    &[],
                    &[],
                    std::slice::from_ref(&selection),
                    &[],
                    &ctx,
                ))
                .map(|_| ())
            }) {
                cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
                error => panic!("unexpected refusal: {error:?}"),
            };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match dimension {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        if matches!(project_fixed_sweep(&scope, &groups, &[], &[], std::slice::from_ref(&selection), &[], &ctx),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == dimension && failure.operation == operation
        ) {
            return;
        }
    }
    panic!("no Sweep guide refusal at {operation}");
}

#[test]
fn sweep_guide_surface_group_refuses_collection_limit() {
    assert_guide_surface_limit(
        "f3d Sweep guide surface group",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn sweep_guide_surface_id_refuses_retained_limit() {
    assert_guide_surface_limit(
        "f3d Sweep guide surface id",
        ResourceDimension::RetainedBytes,
    );
}
