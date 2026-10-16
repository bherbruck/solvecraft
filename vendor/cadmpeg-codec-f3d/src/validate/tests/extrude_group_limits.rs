// SPDX-License-Identifier: Apache-2.0

fn group() -> crate::records::topology::extrude_selection::DesignExtrudeSelectionGroup {
    use crate::records::topology::extrude_selection::{
        DesignExtrudeSelectionGroup, DesignExtrudeSelectionGroupWire,
    };
    DesignExtrudeSelectionGroup::try_from(DesignExtrudeSelectionGroupWire {
        id: "f3d:Design/BulkStream.dat:design-extrude-selection-group#9".into(),
        scope_record_index: 7,
        scope_reference_ordinal: 0,
        record_index: 9,
        byte_offset: 0,
        class_tag: "277".into(),
        member_count_offset: 32,
        members: vec![10, 11],
        member_offsets: vec![37, 48],
        opaque_index: 1,
        opaque_index_offset: 58,
        opaque_scalar: 0.0,
        opaque_scalar_offset: 62,
        variant: false,
        paired_class_tag: "259".into(),
        paired_byte_offset: 111,
    })
    .unwrap()
}

fn scope() -> crate::records::feature::scope::DesignParameterScope {
    use crate::records::feature::scope::{
        DesignParameterScope, DesignParameterScopeDraft, DesignScopePayload,
    };
    DesignParameterScope::try_new(
        DesignParameterScopeDraft {
            id: "f3d:Design/BulkStream.dat:design-parameter-scope#7".into(),
            byte_offset: 100,
            class_tag: "301".to_owned().try_into().unwrap(),
            record_index: 7,
            frame_length: 200,
            kind_offset: 0,
            payload: DesignScopePayload::Extrude(None),
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            history_state_id: None,
            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: 150,
            reference_members: crate::records::identity::ReferenceRun::unlocated(vec![9]),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: "261".to_owned().try_into().unwrap(),
            paired_byte_offset: 0,
        }
        .with_fixture_layout(),
    )
    .unwrap()
}

pub(super) fn native(valid: bool, duplicate: bool) -> crate::native::F3dNative {
    let group = group();
    let mut native = crate::native::F3dNative::default();
    native.design_extrude_selection_groups.push(group.clone());
    if duplicate {
        native.design_extrude_selection_groups.push(group);
    }
    if valid {
        native.design_parameter_scopes.push(scope());
        for (index, offset, tag) in [(9, 0, "277"), (10, 1000, "277"), (11, 1100, "277")] {
            native
                .design_record_headers
                .push(crate::records::decal::DesignRecordHeader {
                    id: format!("f3d:Design/BulkStream.dat:design-record-header#{index}"),
                    record_index: index,
                    class_tag: tag.to_owned().try_into().unwrap(),
                    byte_offset: offset,
                });
        }
    }
    native
}

fn group_error(
    valid: bool,
    duplicate: bool,
    max_items: u64,
    max_retained: u64,
) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native(valid, duplicate);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        policy.limits.max_retained_bytes = max_retained;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_extrude_selection_groups(&ctx, &mut Vec::new()).unwrap_err()
    })
}

#[test]
fn extrude_group_slot_refuses_collection_limit() {
    let error = group_error(true, false, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D Extrude selection group slots")
    );
}

#[test]
fn extrude_group_invalid_finding_refuses_collection_limit() {
    let error = group_error(false, false, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn extrude_group_invalid_entity_refuses_retained_limit() {
    let error = group_error(false, false, u64::MAX, 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

#[test]
fn extrude_group_duplicate_slot_finding_refuses_collection_limit() {
    let error = group_error(true, true, 1, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn extrude_group_valid_slot_has_no_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native(true, false);
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        let mut findings = Vec::new();
        super::super::validate_extrude_selection_groups(&ctx, &mut findings).unwrap();
        assert!(findings.is_empty());
    })
}

fn group_members_error(max_items: u64, max_retained: u64) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native(false, false);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        policy.limits.max_retained_bytes = max_retained;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_extrude_selection_group_members(&ctx, &mut Vec::new()).unwrap_err()
    })
}

#[test]
fn extrude_group_missing_member_finding_refuses_collection_limit() {
    let error = group_members_error(0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn extrude_group_missing_member_entity_refuses_retained_limit() {
    let error = group_members_error(u64::MAX, 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}
