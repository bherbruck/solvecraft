// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::direct_face::{
    DesignOffsetFacesOperation, DesignShellOperation, DesignThickenOperation,
};
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn scope(kind: DesignFeatureKind) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty("f3d:test:scope#12", kind, 12);
    match scope.payload_mut() {
        DesignScopePayloadMut::OffsetFaces(slot) => {
            *slot = Some(DesignOffsetFacesOperation {
                distance: cadmpeg_ir::scalar::FiniteReal::new(0.25).unwrap(),
                distance_record_index: 300,
                distance_offset: 0,
            });
        }
        DesignScopePayloadMut::Thicken(slot) => {
            *slot = Some(DesignThickenOperation {
                signed_thickness: cadmpeg_ir::scalar::FiniteReal::new(0.5).unwrap(),
                thickness_record_index: 300,
                thickness_offset: 0,
            });
        }
        DesignScopePayloadMut::Shell(slot) => {
            *slot = Some(DesignShellOperation {
                thickness: cadmpeg_ir::scalar::PositiveReal::new(0.5).unwrap(),
                thickness_record_index: 300,
                thickness_offset: 0,
                outward: true,
                outward_offset: 0,
            });
        }
        _ => panic!("unsupported direct-face scope fixture"),
    }
    scope
}

fn group(role: DesignOperandRole) -> DesignConstructionOperandGroup {
    serde_json::from_value(serde_json::json!({
        "id": "f3d:test:group#100",
        "scope_record_index": 12,
        "scope_reference_ordinal": 0,
        "record_index": 100,
        "byte_offset": 1000,
        "class_tag": "332",
        "members": [200],
        "member_offsets": [1026],
        "frame": {
            "member_count_offset": 1021,
            "opaque_index": 1,
            "opaque_index_offset": 1072,
            "opaque_scalar": 0.0,
            "opaque_scalar_offset": 1076,
            "variant": false
        },
        "role": role.raw(),
        "role_offset": 1054,
        "paired_class_tag": "259",
        "paired_byte_offset": 1125
    }))
    .unwrap()
}

fn assert_retained_refusal(
    operation: &'static str,
    project: impl for<'a> Fn(
        &'a DecodeContext<'a>,
    ) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError>,
) {
    assert!(
        crate::test_support::with_decode_context(|decode_ctx| project(decode_ctx))
            .unwrap()
            .is_some()
    );
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(project(&ctx), Err(CodecError::ResourceLimit(failure))
        if failure.dimension == ResourceDimension::RetainedBytes
            && failure.operation == operation)
    );
}

#[test]
fn offset_faces_native_group_id_refuses_retained_limit() {
    let scope = scope(DesignFeatureKind::OffsetFaces);
    let group = group(DesignOperandRole::ROLE_0X10);
    assert_retained_refusal("f3d OffsetFaces native group id", |ctx| {
        super::super::project_offset_faces(ctx, &scope, &[], &[], std::slice::from_ref(&group))
    });
}

#[test]
fn thicken_native_group_id_refuses_retained_limit() {
    let scope = scope(DesignFeatureKind::Thicken);
    let group = group(DesignOperandRole::ROLE_0X5);
    assert_retained_refusal("f3d Thicken native group id", |ctx| {
        super::super::project_thicken(ctx, &scope, &[], std::slice::from_ref(&group))
    });
}

#[test]
fn shell_native_face_group_id_refuses_retained_limit() {
    let scope = scope(DesignFeatureKind::Shell);
    let group = group(DesignOperandRole::ROLE_0X10);
    assert_retained_refusal("f3d Shell native face group id", |ctx| {
        super::super::project_shell(ctx, &scope, &[], std::slice::from_ref(&group))
    });
}

#[test]
fn shell_native_body_group_id_refuses_retained_limit() {
    let scope = scope(DesignFeatureKind::Shell);
    let group = group(DesignOperandRole::BODIES_A);
    assert_retained_refusal("f3d Shell native body group id", |ctx| {
        super::super::project_shell(ctx, &scope, &[], std::slice::from_ref(&group))
    });
}

fn direct_face_operand(
    index: u32,
    ordinal: u32,
    slots: Vec<i64>,
) -> crate::records::topology::face::DesignFaceOperand {
    use crate::records::recipes::ConstructionRecipeKind;
    use crate::records::topology::face::{DesignFaceOperand, DesignFaceOperandDraft};
    DesignFaceOperand::try_new(DesignFaceOperandDraft {
        id: format!("f3d:test:face-operand#{index}"),
        scope_record_index: 12,
        scope_reference_ordinal: ordinal,
        group: None,
        record_index: index,
        byte_offset: 1200,
        class_tag: "297".to_owned().try_into().unwrap(),
        paired_byte_offset: 1250,
        paired_class_tag: "259".to_owned().try_into().unwrap(),
        recipe_record_index: index + 3,
        recipe_record_byte_offset: 1300,
        recipe_id: format!("f3d:test:recipe#{index}"),
        recipe_prefix_offset: 1311,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: Vec::new(),
        recipe_kind: ConstructionRecipeKind::Face,
        recipe_program_offset: 1350,
        recipe_program: vec![0, -1],
        recipe_nodes: Vec::new(),
        candidate_faces: Vec::new(),
        unreferenced_candidate_faces: Vec::new(),
        alternate_selector_candidate_faces: Vec::new(),
        preceding_candidate_faces: Vec::new(),
        changed_candidate_faces: Vec::new(),
        historical_support_contexts: Vec::new(),
        resolved_face_slots: slots,
        resolved_active_face: None,
        next_record_index: index + 4,
        next_byte_offset: 1411,
    })
    .unwrap()
}

fn assert_direct_face_refusal(
    operation: &'static str,
    dimension: ResourceDimension,
    partial: bool,
    historical: bool,
) {
    let mut scope = scope(DesignFeatureKind::OffsetFaces);
    if historical {
        scope
            .try_edit(|draft| {
                draft.previous_history_state_id = Some(7);
                draft.layout_fixture_tail();
            })
            .unwrap();
    }
    let mut operands = vec![direct_face_operand(101, 0, vec![42])];
    if partial {
        operands.push(direct_face_operand(102, 1, Vec::new()));
    }
    for limit in 0..512 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        match dimension {
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
            _ => panic!("unsupported direct-face limit"),
        }
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
                (super::super::direct_face_selection(&ctx, &scope, &operands)).map(|_| ())
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
        let result = super::super::direct_face_selection(&ctx, &scope, &operands);
        if matches!(result, Err(CodecError::ResourceLimit(ref failure))
            if failure.operation == operation && failure.dimension == dimension)
        {
            return;
        }
    }
    panic!("no direct-face refusal for {operation}");
}

#[test]
fn direct_face_operand_refuses_collection_limit() {
    assert_direct_face_refusal(
        "f3d direct face operand",
        ResourceDimension::CollectionItems,
        false,
        true,
    );
}

#[test]
fn direct_face_member_refuses_collection_limit() {
    assert_direct_face_refusal(
        "f3d direct face member",
        ResourceDimension::CollectionItems,
        false,
        true,
    );
}

#[test]
fn direct_historical_face_id_refuses_retained_limit() {
    assert_direct_face_refusal(
        "f3d direct historical face id",
        ResourceDimension::RetainedBytes,
        false,
        true,
    );
}

#[test]
fn direct_historical_face_refuses_collection_limit() {
    assert_direct_face_refusal(
        "f3d direct historical face",
        ResourceDimension::CollectionItems,
        false,
        true,
    );
}

#[test]
fn direct_historical_native_id_refuses_retained_limit() {
    assert_direct_face_refusal(
        "f3d direct historical native id",
        ResourceDimension::RetainedBytes,
        false,
        true,
    );
}

#[test]
fn direct_unresolved_face_id_refuses_retained_limit() {
    assert_direct_face_refusal(
        "f3d direct unresolved face id",
        ResourceDimension::RetainedBytes,
        true,
        true,
    );
}

#[test]
fn direct_unresolved_face_refuses_collection_limit() {
    assert_direct_face_refusal(
        "f3d direct unresolved face",
        ResourceDimension::CollectionItems,
        true,
        true,
    );
}

#[test]
fn direct_partial_historical_face_refuses_collection_limit() {
    assert_direct_face_refusal(
        "f3d direct partial historical face",
        ResourceDimension::CollectionItems,
        true,
        true,
    );
}

#[test]
fn direct_partial_native_id_refuses_retained_limit() {
    assert_direct_face_refusal(
        "f3d direct partial native id",
        ResourceDimension::RetainedBytes,
        true,
        true,
    );
}

#[test]
fn direct_native_id_refuses_retained_limit() {
    assert_direct_face_refusal(
        "f3d direct native id",
        ResourceDimension::RetainedBytes,
        false,
        false,
    );
}
