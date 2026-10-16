// SPDX-License-Identifier: Apache-2.0

fn native(valid: bool) -> crate::native::F3dNative {
    use crate::records::{
        mesh::DesignRelaxedGuidText,
        references::DesignClassTag,
        topology::extrude_selection::{
            DesignExtrudeSelectionMember, DesignExtrudeSelectionMemberDraft,
        },
    };
    let mut native = super::extrude_group_limits::native(valid, false);
    native.design_extrude_selection_members.push(
        DesignExtrudeSelectionMember::try_new(DesignExtrudeSelectionMemberDraft {
            id: "f3d:Design/BulkStream.dat:extrude-selection-member#10".into(),
            group_record_index: 9,
            group_member_ordinal: 0,
            record_index: 10,
            byte_offset: 1_000,
            class_tag: DesignClassTag::try_from("277".to_owned()).unwrap(),
            local_id: 42,
            local_id_offset: 1_021,
            asset_id: DesignRelaxedGuidText::try_from(
                "11111111-2222-4333-8444-555555555555".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 1_033,
            context_id: DesignRelaxedGuidText::try_from(
                "ffffffff-eeee-4ddd-8ccc-bbbbbbbbbbbb".to_owned(),
            )
            .unwrap(),
            context_id_offset: 1_105,
            tail_slot_present: false,
            tail_slot_offset: 1_181,
            resolved_geometry: None,
            operand_identity_ids: Vec::new(),
            historical: None,
            next_record_index: 11,
            next_byte_offset: 1_190,
        })
        .unwrap(),
    );
    native
}

fn with_matching_identity(native: &mut crate::native::F3dNative) {
    use crate::records::{
        mesh::DesignRelaxedGuidText,
        references::DesignClassTag,
        topology::construction::{
            DesignConstructionOperandIdentity, DesignConstructionOperandIdentityDraft,
            DesignConstructionPersistentIdentity, DesignConstructionPersistentIdentityDraft,
            DesignIdentityWrapper,
        },
    };
    native.design_construction_operand_identities.push(
        DesignConstructionOperandIdentity::try_new(DesignConstructionOperandIdentityDraft {
            id: "f3d:Design/BulkStream.dat:operand-identity#9".into(),
            group_record_index: 9,
            wrappers: vec![DesignIdentityWrapper {
                record_index: 9,
                byte_offset: 976,
                class_tag: DesignClassTag::try_from("384".to_owned()).unwrap(),
            }],
            following_record_index: 10,
            following_byte_offset: 1_000,
            following_class_tag: DesignClassTag::try_from("277".to_owned()).unwrap(),
            tracking_path: None,
            persistent_identity: Some(
                DesignConstructionPersistentIdentity::try_new(
                    DesignConstructionPersistentIdentityDraft {
                        local_id: 42,
                        local_id_offset: 1_021,
                        asset_id: DesignRelaxedGuidText::try_from(
                            "11111111-2222-4333-8444-555555555555".to_owned(),
                        )
                        .unwrap(),
                        asset_id_offset: 1_033,
                        context_id: DesignRelaxedGuidText::try_from(
                            "ffffffff-eeee-4ddd-8ccc-bbbbbbbbbbbb".to_owned(),
                        )
                        .unwrap(),
                        context_id_offset: 1_105,
                        tail_slot_present: false,
                        tail_slot_offset: 1_181,
                        next_record_index: 11,
                        next_byte_offset: 1_190,
                    },
                )
                .unwrap(),
            ),
        })
        .unwrap(),
    );
}

fn member_error(
    valid: bool,
    identity: bool,
    max_items: u64,
    max_retained: u64,
) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = native(valid);
        if identity {
            with_matching_identity(&mut native);
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        policy.limits.max_retained_bytes = max_retained;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_extrude_selection_members(&ctx, &mut Vec::new()).unwrap_err()
    })
}

#[test]
fn extrude_member_history_states_refuse_collection_limit() {
    crate::test_support::with_decode_context(|service_ctx| {
        use crate::{
            history_records::{AsmDeltaState, AsmHistory, AsmTopologyCache},
            records::topology::{body_recipe::AsmHistoricalEntityKind, fillet::HistoricalBinding},
        };
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = native(true);
        native.design_extrude_selection_members[0].historical = Some(HistoricalBinding {
            kind: AsmHistoricalEntityKind::Edge,
            entity_ref: 42,
            state_ids: vec![1],
        });
        native.asm_histories.push(AsmHistory {
            id: "f3d:asm-history#1".into(),
            byte_offset: 0,
            preamble: None,
            record_table_binding_budget_exceeded: false,
            states: vec![AsmDeltaState {
                id: "f3d:asm-delta-state#1".into(),
                parent: "f3d:asm-history#1".into(),
                byte_offset: 0,
                state_id: 1,
                version_flag: 1,
                state_flag: 0,
                previous_ref: None,
                next_ref: None,
                node_index: 0,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: Vec::new(),
                entity_versions: Vec::new(),
                topology_cache: AsmTopologyCache::Released,
                transition: None,
            }],
        });
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 4;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        let error =
            super::super::validate_extrude_selection_members(&ctx, &mut Vec::new()).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D Extrude selection history states")
        );
    })
}

#[test]
fn extrude_member_identity_index_refuses_collection_limit() {
    let error = member_error(true, true, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D Extrude selection identities")
    );
}

#[test]
fn extrude_member_slot_refuses_collection_limit() {
    let error = member_error(true, false, 2, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D Extrude selection member slots")
    );
}

#[test]
fn extrude_member_record_refuses_collection_limit() {
    let error = member_error(true, false, 3, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D Extrude selection member records")
    );
}

#[test]
fn extrude_member_invalid_finding_refuses_collection_limit() {
    let error = member_error(false, false, 2, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn extrude_member_invalid_entity_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D validation entity",
        |cap| Err::<(), cadmpeg_core::CodecError>(member_error(false, false, u64::MAX, cap)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

#[test]
fn extrude_member_valid_slot_has_no_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native(true);
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        let mut findings = Vec::new();
        super::super::validate_extrude_selection_members(&ctx, &mut findings).unwrap();
        assert!(findings.is_empty());
    })
}
