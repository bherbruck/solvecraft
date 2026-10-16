// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::feature_project::project_extrude;
use crate::design::test_support::parameter_record;
use crate::records::feature::extrude::{
    DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart,
};
use crate::records::feature::scope::{
    DesignExtrudeScope, DesignParameterScope, DesignParameterScopeDraft, DesignScopePayload,
};
use crate::records::topology::construction::{
    DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
    DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
    DesignConstructionOperandRole,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PlanarProfileRef, ProfileRef};

pub(super) fn scope() -> DesignParameterScope {
    scope_with_extent(DesignExtrudeExtent::OneSidedDistance)
}

fn scope_with_extent(extent: DesignExtrudeExtent) -> DesignParameterScope {
    DesignParameterScope::try_new(
        DesignParameterScopeDraft {
            id: "f3d:Design/BulkStream.dat:scope#12".into(),
            byte_offset: 100,
            class_tag: "301".to_owned().try_into().unwrap(),
            record_index: 12,
            frame_length: 200,
            kind_offset: 210,
            payload: DesignScopePayload::Extrude(Some(DesignExtrudeScope {
                extrude_prologue: Some(DesignExtrudePrologue::ReferenceAware {
                    reference: None,
                    operation: DesignExtrudeOperation::NewBody,
                    operation_offset: 128,
                    direction_face_extend_values: [1, 2],
                    side_extent_discriminators: [1, 0],
                    side_extent_discriminator_offsets: [177, 190],
                    first_side_target_ordinal: None,
                    extent,
                    direction_face_extend_offsets: [132, 136],
                    direction_reversed: false,
                    direction_reversed_offset: 140,
                    solid_operation: true,
                    solid_operation_offset: 141,
                    start: DesignExtrudeStart::ProfilePlane,
                    start_offset: 142,
                }),
                ..Default::default()
            })),
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            history_state_id: None,
            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: 180,
            reference_members: crate::records::identity::ReferenceRun::from_columns(
                vec![100, 101],
                vec![185, 196],
                "reference_members",
            )
            .unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: "261".to_owned().try_into().unwrap(),
            paired_byte_offset: 300,
        }
        .with_fixture_layout(),
    )
    .unwrap()
}

pub(super) fn profile_group(index: u32, ordinal: u32) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("f3d:Design/BulkStream.dat:operand-group#{index}"),
        scope_record_index: 12,
        scope_reference_ordinal: ordinal,
        record_index: index,
        byte_offset: 1000,
        class_tag: "332".to_owned().try_into().unwrap(),
        members: vec![crate::records::identity::Located {
            value: index + 100,
            offset: 1026,
        }],
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
                member_count_offset: 1021,
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: Vec::new(),
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                opaque_index: 1,
                opaque_index_offset: 1072,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 1076,
                variant: false,
            },
        )
        .unwrap(),
        operand_role: DesignConstructionOperandRole::ExtrudeProfile,
        role_offset: 1054,
        paired_class_tag: "259".to_owned().try_into().unwrap(),
        paired_byte_offset: 1125,
    })
    .unwrap()
}

fn parameters() -> [crate::records::parameters::DesignParameter; 2] {
    [
        parse_design_parameter_record(&parameter_record(
            Some(44),
            "value",
            "AlongDistance",
            Some("mm"),
            "d1",
            0.55,
        ))
        .unwrap(),
        parse_design_parameter_record(&parameter_record(
            Some(44),
            "value",
            "TaperAngle",
            Some("deg"),
            "d2",
            0.2,
        ))
        .unwrap(),
    ]
}

fn assert_profile_fallback(
    groups: &[DesignConstructionOperandGroup],
    expected_native: &str,
    operation: &'static str,
) {
    let scope = scope();
    let parameters = parameters();
    let owned = [(0, &parameters[0]), (1, &parameters[1])];
    let definition = crate::test_support::with_decode_context(|decode_ctx| {
        project_extrude(decode_ctx, &scope, &owned, groups, &[], &[], &[])
    })
    .unwrap()
    .unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile: ProfileRef::Planar(PlanarProfileRef::Native(ref native)), ..
        }) if native == expected_native
    ));
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            operation,
            |cap| {
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = cap;
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                (project_extrude(&ctx, &scope, &owned, groups, &[], &[], &[])).map(|_| ())
            },
        );
        assert!(
            matches!(Err::<(), cadmpeg_core::CodecError>(error), Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::RetainedBytes
                    && failure.operation == operation)
        );
    }
}

#[test]
fn extrude_single_profile_group_id_refuses_retained_limit() {
    let first = profile_group(100, 0);
    assert_profile_fallback(
        std::slice::from_ref(&first),
        &first.id,
        "f3d Extrude profile group id",
    );
}

#[test]
fn extrude_multiple_profile_fallback_scope_id_refuses_retained_limit() {
    let groups = [profile_group(100, 0), profile_group(101, 1)];
    assert_profile_fallback(
        &groups,
        "f3d:Design/BulkStream.dat:scope#12",
        "f3d Extrude fallback scope id",
    );
}

fn assert_face_fallback(role: DesignConstructionOperandRole, operation: &'static str) {
    let scope = scope_with_extent(DesignExtrudeExtent::OneSidedToFace);
    let profile = profile_group(100, 0);
    let mut selection = profile_group(101, 1);
    selection.operand_role = role;
    let groups = [profile, selection];
    let side_offset = parse_design_parameter_record(&parameter_record(
        Some(44),
        "value",
        "Side1Offset",
        Some("mm"),
        "d3",
        0.0,
    ))
    .unwrap();
    let taper = parse_design_parameter_record(&parameter_record(
        Some(44),
        "value",
        "TaperAngle",
        Some("deg"),
        "d2",
        0.2,
    ))
    .unwrap();
    let owned = [(0, &side_offset), (1, &taper)];
    let definition = crate::test_support::with_decode_context(|decode_ctx| {
        project_extrude(decode_ctx, &scope, &owned, &groups, &[], &[], &[])
    })
    .unwrap()
    .unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Extrude { .. })
    ));
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            operation,
            |cap| {
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = cap;
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                (project_extrude(&ctx, &scope, &owned, &groups, &[], &[], &[])).map(|_| ())
            },
        );
        assert!(
            matches!(Err::<(), cadmpeg_core::CodecError>(error), Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::RetainedBytes
                    && failure.operation == operation)
        );
    }
}

#[test]
fn extrude_selected_face_group_id_refuses_retained_limit() {
    assert_face_fallback(
        DesignConstructionOperandRole::ExtrudeFaces {
            encoding: crate::records::topology::extrude_selection::DesignExtrudeFaceEncoding::Faces,
            usage: crate::records::topology::extrude_selection::DesignExtrudeFaceRole::Termination,
        },
        "f3d Extrude selected face group id",
    );
}

#[test]
fn extrude_target_shape_group_id_refuses_retained_limit() {
    assert_face_fallback(
        DesignConstructionOperandRole::Other(
            crate::records::topology::extrude_selection::DesignOperandRole::ROLE_0X5,
        ),
        "f3d Extrude target shape group id",
    );
}
