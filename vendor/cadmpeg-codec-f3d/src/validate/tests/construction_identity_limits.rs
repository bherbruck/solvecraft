// SPDX-License-Identifier: Apache-2.0

fn native(valid: bool) -> crate::native::F3dNative {
    use crate::records::{
        decal::DesignRecordHeader,
        identity::Located,
        references::DesignClassTag,
        topology::construction::{
            DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
            DesignConstructionOperandIdentity, DesignConstructionOperandIdentityDraft,
            DesignIdentityWrapper,
        },
    };
    let mut native = super::construction_group_limits::native(valid, false);
    native.design_construction_operand_groups[0].frame =
        DesignConstructionOperandGroupFrame::try_from(DesignConstructionOperandGroupFrameDraft {
            member_count_offset: 1_021,
            auxiliary_records: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_records: vec![Located {
                value: 101,
                offset: 1_025,
            }],
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 1,
            opaque_index_offset: 1_058,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 1_062,
            variant: false,
        })
        .unwrap();
    if valid {
        native.design_record_headers[1].class_tag =
            DesignClassTag::try_from("384".to_owned()).unwrap();
        native.design_record_headers.push(DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:design-record-header#102".into(),
            record_index: 102,
            class_tag: DesignClassTag::try_from("395".to_owned()).unwrap(),
            byte_offset: 1_124,
        });
    }
    native.design_construction_operand_identities.push(
        DesignConstructionOperandIdentity::try_new(DesignConstructionOperandIdentityDraft {
            id: "f3d:Design/BulkStream.dat:operand-identity#101".into(),
            group_record_index: 100,
            wrappers: vec![DesignIdentityWrapper {
                record_index: 101,
                byte_offset: 1_100,
                class_tag: DesignClassTag::try_from("384".to_owned()).unwrap(),
            }],
            following_record_index: 102,
            following_byte_offset: 1_124,
            following_class_tag: DesignClassTag::try_from("395".to_owned()).unwrap(),
            tracking_path: None,
            persistent_identity: None,
        })
        .unwrap(),
    );
    native
}

fn identity_error(valid: bool, max_items: u64, max_retained: u64) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native(valid);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        policy.limits.max_retained_bytes = max_retained;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_construction_operand_identities(&ctx, &mut Vec::new()).unwrap_err()
    })
}

#[test]
fn construction_identity_group_refuses_collection_limit() {
    let error = identity_error(true, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D construction operand identity groups")
    );
}

#[test]
fn construction_identity_invalid_finding_refuses_collection_limit() {
    let error = identity_error(false, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn construction_identity_invalid_entity_refuses_retained_limit() {
    let error = identity_error(false, u64::MAX, 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

#[test]
fn construction_identity_valid_group_has_no_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native(true);
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        let mut findings = Vec::new();
        let groups =
            super::super::validate_construction_operand_identities(&ctx, &mut findings).unwrap();
        assert!(findings.is_empty());
        assert!(groups.contains(&("f3d:Design/BulkStream.dat", 100)));
    })
}
