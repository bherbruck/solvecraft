// SPDX-License-Identifier: Apache-2.0

pub(super) fn native(valid: bool, duplicate: bool) -> crate::native::F3dNative {
    use crate::records::{
        decal::DesignRecordHeader,
        feature::scope::{DesignFeatureKind, DesignParameterScope},
        identity::{Located, ReferenceRun},
        topology::construction::{
            DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
            DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
            DesignConstructionOperandRole,
        },
    };
    let stream = "f3d:Design/BulkStream.dat";
    let mut scope = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#10"),
        DesignFeatureKind::Hole,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = ReferenceRun::unlocated(vec![100, 101]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let group = DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("{stream}:design-construction-operand-group#100"),
        scope_record_index: 10,
        scope_reference_ordinal: 0,
        record_index: 100,
        byte_offset: 1_000,
        class_tag: "277".to_owned().try_into().unwrap(),
        members: vec![Located {
            value: 101,
            offset: 1_026,
        }],
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
                member_count_offset: 1_021,
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: Vec::new(),
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
        .unwrap(),
        operand_role: DesignConstructionOperandRole::Other(
            crate::records::topology::extrude_selection::DesignOperandRole::BODIES_A,
        ),
        role_offset: 1_040,
        paired_class_tag: "258".to_owned().try_into().unwrap(),
        paired_byte_offset: 1_080,
    })
    .unwrap();
    let mut native = crate::native::F3dNative::default();
    native
        .design_construction_operand_groups
        .push(group.clone());
    if duplicate {
        native.design_construction_operand_groups.push(group);
    }
    if valid {
        native.design_parameter_scopes.push(scope);
        for (index, offset) in [(100, 1_000), (101, 1_100)] {
            native.design_record_headers.push(DesignRecordHeader {
                id: format!("{stream}:design-record-header#{index}"),
                record_index: index,
                class_tag: "277".to_owned().try_into().unwrap(),
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
        super::super::validate_construction_operand_groups(&ctx, &mut Vec::new()).unwrap_err()
    })
}

#[test]
fn construction_group_member_index_refuses_collection_limit() {
    let error = group_error(true, false, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D construction operand group members")
    );
}

#[test]
fn construction_group_slot_refuses_collection_limit() {
    let error = group_error(true, false, 1, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D construction operand group slots")
    );
}

#[test]
fn construction_group_invalid_finding_refuses_collection_limit() {
    let error = group_error(false, false, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn construction_group_invalid_entity_refuses_retained_limit() {
    let error = group_error(false, false, u64::MAX, 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

#[test]
fn construction_group_duplicate_slot_finding_refuses_collection_limit() {
    let error = group_error(true, true, 3, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

fn tail_findings(byte_offset: u64, flag: bool) -> Vec<cadmpeg_ir::report::check::Finding> {
    use crate::records::{
        decal::DesignRecordHeader,
        identity::Located,
        sketch_placement::SketchPlacementMatrix,
        topology::construction::{
            DesignConstructionOperandDualTransform, DesignConstructionOperandFlag,
            DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
        },
    };
    crate::test_support::with_decode_context(|decode| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = native(true, false);
        let class_tag: crate::records::references::DesignClassTag =
            "280".to_owned().try_into().unwrap();
        let mut frame = DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
                member_count_offset: 1_021,
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: vec![Located {
                    value: 102,
                    offset: 1_030,
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
        if flag {
            frame
                .try_set_trailing_flags(vec![DesignConstructionOperandFlag {
                    record_index: 102,
                    byte_offset,
                    class_tag: class_tag.clone(),
                    value: true,
                    value_offset: byte_offset.saturating_add(22),
                }])
                .unwrap();
        } else {
            frame
                .try_set_trailing_dual_transforms(vec![DesignConstructionOperandDualTransform {
                    record_index: 102,
                    byte_offset,
                    class_tag: class_tag.clone(),
                    first_transform: SketchPlacementMatrix::IDENTITY,
                    first_transform_offset: byte_offset.saturating_add(21),
                    second_transform: SketchPlacementMatrix::IDENTITY,
                    second_transform_offset: byte_offset.saturating_add(149),
                }])
                .unwrap();
        }
        native.design_construction_operand_groups[0].frame = frame;
        native.design_record_headers.push(DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:design-record-header#102".into(),
            record_index: 102,
            class_tag,
            byte_offset,
        });
        let ctx = super::super::Ctx::new(&ir, &native, decode).unwrap();
        let mut findings = Vec::new();
        super::super::validate_construction_operand_groups(&ctx, &mut findings).unwrap();
        findings
    })
}

#[test]
fn construction_group_preserves_representable_tail_offsets() {
    assert!(tail_findings(2_000, false).is_empty());
    assert!(tail_findings(2_000, true).is_empty());
}

#[test]
fn construction_group_rejects_overflowed_first_transform() {
    assert_eq!(tail_findings(u64::MAX, false).len(), 1);
}

#[test]
fn construction_group_rejects_overflowed_second_transform() {
    assert_eq!(tail_findings(u64::MAX - 100, false).len(), 1);
}

#[test]
fn construction_group_rejects_overflowed_flag() {
    assert_eq!(tail_findings(u64::MAX, true).len(), 1);
}
