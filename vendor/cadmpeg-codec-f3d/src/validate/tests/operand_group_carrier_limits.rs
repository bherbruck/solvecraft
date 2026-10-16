// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;

fn native(trailing_only: bool) -> crate::native::F3dNative {
    use crate::records::{
        identity::Located,
        topology::construction::{
            DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
        },
    };
    let mut native = super::construction_group_limits::native(false, false);
    if trailing_only {
        let mut member = native.design_construction_operand_groups[0].clone();
        member.id = "f3d:Design/BulkStream.dat:design-construction-operand-group#101".into();
        member.record_index = 101;
        member.scope_reference_ordinal = 1;
        native.design_construction_operand_groups.push(member);
        native.design_construction_operand_groups[0].frame =
            DesignConstructionOperandGroupFrame::try_from(
                DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 1_021,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: vec![Located {
                        value: 102,
                        offset: 1_038,
                    }],
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 1_058,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 1_062,
                    variant: false,
                },
            )
            .unwrap();
    }
    native
}

pub(super) fn native_with_identity() -> crate::native::F3dNative {
    use crate::records::{
        mesh::DesignRelaxedGuidText,
        references::DesignClassTag,
        topology::edge_identity::{
            DesignEdgeIdentityLayout, DesignEdgeIdentityOperand, DesignEdgeIdentityOperandDraft,
        },
    };
    let mut native = native(false);
    native.design_edge_identity_operands.push(
        DesignEdgeIdentityOperand::try_new(DesignEdgeIdentityOperandDraft {
            id: "f3d:Design/BulkStream.dat:edge-identity#101".into(),
            scope_record_index: 10,
            group_record_index: 100,
            group_member_ordinal: 0,
            record_index: 101,
            byte_offset: 100,
            class_tag: DesignClassTag::try_from("297".to_owned()).unwrap(),
            layout: DesignEdgeIdentityLayout::Full,
            local_id: 13,
            asset_id: DesignRelaxedGuidText::try_from(
                "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 142,
            context_id: DesignRelaxedGuidText::try_from(
                "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
            )
            .unwrap(),
            context_id_offset: 218,
            historical: None,
            treatment_radius_candidates: Vec::new(),
            transition_edge_candidates: Vec::new(),
            resolved_edge_slots: Vec::new(),
            resolved_edge_slot: None,
            resolution_identity_id: None,
        })
        .unwrap(),
    );
    native
}

#[test]
fn operand_group_identity_members_refuse_collection_limit() {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native_with_identity();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        let error = super::super::validate_operand_group_carriers(
            &ctx,
            &mut Vec::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
        )
        .unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D operand group identity members")
        );
    })
}

fn carrier_error(
    trailing_only: bool,
    max_items: u64,
    max_retained: u64,
) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native(trailing_only);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        policy.limits.max_retained_bytes = max_retained;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_operand_group_carriers(
            &ctx,
            &mut Vec::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
        )
        .unwrap_err()
    })
}

#[test]
fn operand_group_missing_member_refuses_finding_limit() {
    let error = carrier_error(false, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn operand_group_missing_member_refuses_entity_limit() {
    let error = carrier_error(false, u64::MAX, 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

#[test]
fn operand_group_missing_trailing_carrier_refuses_finding_limit() {
    let error = carrier_error(true, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn operand_group_missing_trailing_carrier_refuses_entity_limit() {
    let error = carrier_error(true, u64::MAX, 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

#[test]
fn operand_group_missing_member_preserves_finding_text() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native(false);
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        let mut findings = Vec::new();
        super::super::validate_operand_group_carriers(
            &ctx,
            &mut findings,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
        )
        .unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].message,
            "Fusion Design construction operand group has no exact typed member"
        );
    })
}

#[test]
fn operand_group_exact_identity_member_has_no_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native_with_identity();
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        let mut findings = Vec::new();
        let exact_members = HashSet::from([("f3d:Design/BulkStream.dat", 101)]);
        super::super::validate_operand_group_carriers(
            &ctx,
            &mut findings,
            &HashSet::new(),
            &exact_members,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
        )
        .unwrap();
        assert!(findings.is_empty());
    })
}
