// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use crate::design::feature_project::{
    project_delete_face, project_split, project_split_face, selected_work_planes,
};
use crate::ids::feature_input_topology_id;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::{
    construction::DesignConstructionOperandGroupFrame, extrude_selection::DesignOperandRole,
};
use cadmpeg_ir::features::FaceSelection;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::features::FeatureOperation;

fn group(
    scope_record_index: u32,
    scope_reference_ordinal: u32,
    record_index: u32,
    members: Vec<u32>,
    role: DesignOperandRole,
) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: format!("f3d:Design/BulkStream.dat:group#{record_index}"),
            scope_record_index,
            scope_reference_ordinal,
            record_index,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("262".to_owned())
                .unwrap(),

            members: members
                .into_iter()
                .enumerate()
                .map(|(index, value)| crate::records::identity::Located {
                    value,
                    offset: u64_from_index(index) * 11,
                })
                .collect(),
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 0,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 18,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 22,
                    variant: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(role),
            role_offset: 0,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "258".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap()
}

fn split_body_scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#77",
        crate::records::feature::scope::DesignFeatureKind::Split,
        77,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![100, 101, 102, 103]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn split_body_groups(tool_role: DesignOperandRole) -> [DesignConstructionOperandGroup; 2] {
    [
        group(77, 0, 100, vec![101], tool_role),
        group(77, 2, 102, vec![103], DesignOperandRole::BODIES_A),
    ]
}

fn split_body_face_tool() -> crate::records::topology::face::DesignFaceOperand {
    use crate::records::recipes::ConstructionRecipeKind;
    crate::records::topology::face::DesignFaceOperand::try_new(
        crate::records::topology::face::DesignFaceOperandDraft {
            id: "f3d:Design/BulkStream.dat:face-operand#101".into(),
            scope_record_index: 77,
            scope_reference_ordinal: 1,
            group: Some(crate::records::topology::body_recipe::DesignOperandGroup {
                group_record_index: 100,
                group_member_ordinal: 0,
            }),
            record_index: 101,
            byte_offset: 1200,
            class_tag: "297".to_owned().try_into().unwrap(),
            paired_byte_offset: 1250,
            paired_class_tag: "259".to_owned().try_into().unwrap(),
            recipe_record_index: 104,
            recipe_record_byte_offset: 1300,
            recipe_id: "f3d:Design/BulkStream.dat:recipe#104".into(),
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
            resolved_face_slots: Vec::new(),
            resolved_active_face: None,
            next_record_index: 105,
            next_byte_offset: 1411,
        },
    )
    .unwrap()
}

fn assert_split_body_refusal(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    operands: &[crate::records::topology::face::DesignFaceOperand],
    operation: &'static str,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            operation,
            |cap| {
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = cap;
                let arena = DecodeArena::new();

                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                (project_split(&ctx, scope, groups, operands)).map(|_| ())
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
fn split_body_path_tool_group_id_refuses_retained_limit() {
    let scope = split_body_scope();
    let groups = split_body_groups(DesignOperandRole::ROLE_0X21);
    let definition = crate::test_support::with_decode_context(|decode_ctx| {
        project_split(decode_ctx, &scope, &groups, &[])
    })
    .unwrap()
    .unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::SplitBody {
            tools: FaceSelection::Native(ref tool), ..
        }) if tool == &groups[0].id
    ));
    assert_split_body_refusal(&scope, &groups, &[], "f3d SplitBody path tool group id");
}

#[test]
fn split_body_target_group_id_refuses_retained_limit() {
    let scope = split_body_scope();
    let groups = split_body_groups(DesignOperandRole::ROLE_0X21);
    assert_split_body_refusal(&scope, &groups, &[], "f3d SplitBody target group id");
}

#[test]
fn split_body_face_tool_id_refuses_retained_limit() {
    let scope = split_body_scope();
    let groups = split_body_groups(DesignOperandRole::ROLE_0X9);
    let tool = split_body_face_tool();
    let definition = crate::test_support::with_decode_context(|decode_ctx| {
        project_split(decode_ctx, &scope, &groups, std::slice::from_ref(&tool))
    })
    .unwrap()
    .unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::SplitBody {
            tools: FaceSelection::Native(ref native), ..
        }) if native == &tool.id
    ));
    assert_split_body_refusal(
        &scope,
        &groups,
        std::slice::from_ref(&tool),
        "f3d SplitBody face tool id",
    );
}

#[test]
fn split_body_historical_face_tool_id_refuses_retained_limit() {
    let mut scope = split_body_scope();
    scope
        .try_edit(|draft| {
            draft.previous_history_state_id = Some(7);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let groups = split_body_groups(DesignOperandRole::ROLE_0X9);
    let mut tool = split_body_face_tool();
    tool.resolved_face_slots = vec![42];
    let definition = crate::test_support::with_decode_context(|decode_ctx| {
        project_split(decode_ctx, &scope, &groups, std::slice::from_ref(&tool))
    })
    .unwrap()
    .unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::SplitBody {
            tools: FaceSelection::Historical { ref native, .. }, ..
        }) if native.as_str() == tool.id
    ));
    assert_split_body_refusal(
        &scope,
        &groups,
        std::slice::from_ref(&tool),
        "f3d SplitBody historical face tool id",
    );
}

#[test]
fn delete_face_fallback_group_id_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#77",
        crate::records::feature::scope::DesignFeatureKind::DeleteFace,
        77,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 258;
            draft.kind_offset = draft.byte_offset + 161;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![100, 200]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.reference_count_offset = draft.kind_offset - 34;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let selected = group(77, 0, 100, vec![200], DesignOperandRole::ROLE_0X10);
    let definition = crate::test_support::with_decode_context(|decode_ctx| {
        project_delete_face(decode_ctx, &scope, std::slice::from_ref(&selected), &[])
    })
    .unwrap()
    .unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::DeleteFace {
            faces: FaceSelection::Native(ref native), ..
        }) if native == &selected.id
    ));
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            "f3d DeleteFace fallback group id",
            |cap| {
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = cap;
                let arena = DecodeArena::new();

                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                (project_delete_face(&ctx, &scope, std::slice::from_ref(&selected), &[]))
                    .map(|_| ())
            },
        );
        assert!(
            matches!(Err::<(), cadmpeg_core::CodecError>(error), Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::RetainedBytes
                    && failure.operation == "f3d DeleteFace fallback group id")
        );
    }
}

fn compact_split_face_fixture() -> (DesignParameterScope, [DesignConstructionOperandGroup; 2]) {
    let scope_record_index = 77;
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#77",
        crate::records::feature::scope::DesignFeatureKind::SplitFace,
        scope_record_index,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("277".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 407;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated((100..112).collect());
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();

    let groups = [
        group(
            scope_record_index,
            0,
            100,
            vec![101],
            DesignOperandRole::ROLE_0X21,
        ),
        group(
            scope_record_index,
            2,
            102,
            (103..112).collect(),
            DesignOperandRole::ROLE_0X10,
        ),
    ];
    (scope, groups)
}

#[test]
fn class_277_258_compact_split_face_frame_projects() {
    let (mut scope, groups) = compact_split_face_fixture();
    let definition = crate::test_support::with_decode_context(|decode_ctx| {
        project_split_face(decode_ctx, &scope, &[scope.clone()], &groups, &[], &[], &[])
    })
    .expect("projection resource budget")
    .expect("class-277 SplitFace frame");
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::SplitFace {
            targets: FaceSelection::Native(targets),
            tool: cadmpeg_ir::features::SplitFaceTool::Path(
                cadmpeg_ir::features::PathRef::Native(tool)
            ),
        }) if targets.ends_with("group#102") && tool.ends_with("group#100")
    ));

    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("418".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("266".to_owned()).unwrap();
    assert!(
        crate::test_support::with_decode_context(|decode_ctx| project_split_face(
            decode_ctx,
            &scope,
            &[scope.clone()],
            &groups,
            &[],
            &[],
            &[]
        ))
        .expect("projection resource budget")
        .is_some()
    );

    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("277".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("266".to_owned()).unwrap();
    assert!(
        crate::test_support::with_decode_context(|decode_ctx| project_split_face(
            decode_ctx,
            &scope,
            &[scope.clone()],
            &groups,
            &[],
            &[],
            &[]
        ))
        .expect("projection resource budget")
        .is_none()
    );
}

fn assert_split_face_retained_refusal(operation: &'static str) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let (scope, groups) = compact_split_face_fixture();
    let scopes = [scope.clone()];
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            operation,
            |cap| {
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = cap;
                let arena = DecodeArena::new();

                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                (project_split_face(&ctx, &scope, &scopes, &groups, &[], &[], &[])).map(|_| ())
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
fn split_face_path_tool_group_id_refuses_retained_limit() {
    assert_split_face_retained_refusal("f3d SplitFace path tool group id");
}

#[test]
fn split_face_target_group_id_refuses_retained_limit() {
    assert_split_face_retained_refusal("f3d SplitFace target group id");
}

fn selected_plane_fixture() -> (
    DesignParameterScope,
    DesignConstructionOperandGroup,
    crate::records::topology::entity_selection::DesignEntitySelectionOperand,
    DesignParameterScope,
) {
    let (scope, groups) = compact_split_face_fixture();
    let mut plane = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#601",
        crate::records::feature::scope::DesignFeatureKind::WorkPlane,
        601,
    );
    plane.with_work_plane_transform(
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
        .try_into()
        .unwrap(),
    );
    let selection =
        crate::records::topology::entity_selection::DesignEntitySelectionOperand::try_new(
            crate::records::topology::entity_selection::DesignEntitySelectionOperandDraft {
                id: "f3d:Design/BulkStream.dat:entity-selection#101".into(),
                scope_record_index: 77,
                group_record_index: 100,
                group_member_ordinal: 0,
                record_index: 101,
                byte_offset: 0,
                class_tag: "372".to_owned().try_into().unwrap(),
                asset_id: "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                asset_id_offset: 0,
                context_id: "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                context_id_offset: 0,
                identity_record_index: 104,
                identity_record_offset: 0,
                primary_identity: 600,
                primary_identity_offset: 21,
                secondary: None,
                historical_edge_candidates: Vec::new(),
                historical_face_candidates: Vec::new(),
                resolved_edge_slot: None,
                next_record_index: 105,
                next_byte_offset: 29,
            },
        )
        .unwrap();
    (scope, groups[0].clone(), selection, plane)
}

fn assert_selected_plane_limit(operation: &'static str) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let (scope, group, selection, plane) = selected_plane_fixture();
    let selected = crate::test_support::with_decode_context(|decode_ctx| {
        selected_work_planes(
            decode_ctx,
            &scope,
            &group,
            std::slice::from_ref(&selection),
            std::slice::from_ref(&plane),
        )
    })
    .unwrap()
    .unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].record_index, plane.record_index);
    for limit in 0..4 {
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let arena = DecodeArena::new();

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        if matches!(
            selected_work_planes(&ctx, &scope, &group, std::slice::from_ref(&selection), std::slice::from_ref(&plane)),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == operation
        ) {
            return;
        }
    }
    panic!("no selected work plane refusal at {operation}");
}

#[test]
fn selected_work_plane_target_index_refuses_collection_limit() {
    assert_selected_plane_limit("f3d selected work plane target index");
}

#[test]
fn selected_work_plane_refuses_collection_limit() {
    assert_selected_plane_limit("f3d selected work plane");
}

#[test]
fn direct_single_identity_split_face_member_projects_historical_edge_path() {
    let scope_record_index = 77;
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#77",
        crate::records::feature::scope::DesignFeatureKind::SplitFace,
        scope_record_index,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("277".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 407;
            draft.previous_history_state_id = Some(7);
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated((100..112).collect());
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();

    let groups = [
        group(
            scope_record_index,
            0,
            100,
            vec![101],
            DesignOperandRole::ROLE_0X21,
        ),
        group(
            scope_record_index,
            2,
            102,
            (103..112).collect(),
            DesignOperandRole::ROLE_0X10,
        ),
    ];
    let selections = [
        crate::records::topology::entity_selection::DesignEntitySelectionOperand::try_new(
            crate::records::topology::entity_selection::DesignEntitySelectionOperandDraft {
                id: "f3d:Design/BulkStream.dat:entity-selection#101".into(),
                scope_record_index,
                group_record_index: 100,
                group_member_ordinal: 0,
                record_index: 101,
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("277".to_owned())
                    .unwrap(),
                asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
                )
                .unwrap(),
                asset_id_offset: 0,
                context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
                )
                .unwrap(),
                context_id_offset: 0,
                identity_record_index: 104,
                identity_record_offset: 0,
                primary_identity: 225,
                primary_identity_offset: 21,
                secondary: None,
                historical_edge_candidates: Vec::new(),
                historical_face_candidates: Vec::new(),
                resolved_edge_slot: Some(42),
                next_record_index: 103,
                next_byte_offset: 29,
            },
        )
        .unwrap(),
    ];

    let definition = crate::test_support::with_decode_context(|decode_ctx| {
        project_split_face(
            decode_ctx,
            &scope,
            &[scope.clone()],
            &groups,
            &selections,
            &[],
            &[],
        )
    })
    .expect("projection resource budget")
    .expect("class-277 direct edge path");
    let FeatureDefinition::Operation(FeatureOperation::SplitFace {
        tool:
            cadmpeg_ir::features::SplitFaceTool::Path(cadmpeg_ir::features::PathRef::HistoricalEdges {
                state,
                edges,
                native,
            }),
        ..
    }) = definition
    else {
        panic!("expected historical edge path");
    };
    let feature = crate::ids::neutral_feature_id(&scope);
    let prefix = crate::ids::history_input_prefix(&feature.key(), 7);
    assert_eq!(state, feature_input_topology_id(&feature, 7),);
    assert_eq!(
        edges.as_slice(),
        vec![crate::ids::history_input_edge_id(&prefix, 42)],
    );
    assert_eq!(native.as_str(), groups[0].id);
}

fn historical_split_face_path_fixture() -> (
    DesignParameterScope,
    DesignConstructionOperandGroup,
    crate::records::topology::entity_selection::DesignEntitySelectionOperand,
) {
    let scope_record_index = 77;
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#77",
        crate::records::feature::scope::DesignFeatureKind::SplitFace,
        scope_record_index,
    );
    scope.class_tag = "277".to_owned().try_into().unwrap();
    scope.paired_class_tag = "258".to_owned().try_into().unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 407;
            draft.previous_history_state_id = Some(7);
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated((100..112).collect());
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let group = group(
        scope_record_index,
        0,
        100,
        vec![101],
        DesignOperandRole::ROLE_0X21,
    );
    let selection =
        crate::records::topology::entity_selection::DesignEntitySelectionOperand::try_new(
            crate::records::topology::entity_selection::DesignEntitySelectionOperandDraft {
                id: "f3d:Design/BulkStream.dat:entity-selection#101".into(),
                scope_record_index,
                group_record_index: 100,
                group_member_ordinal: 0,
                record_index: 101,
                byte_offset: 0,
                class_tag: "277".to_owned().try_into().unwrap(),
                asset_id: "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                asset_id_offset: 0,
                context_id: "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                context_id_offset: 0,
                identity_record_index: 104,
                identity_record_offset: 0,
                primary_identity: 225,
                primary_identity_offset: 21,
                secondary: None,
                historical_edge_candidates: Vec::new(),
                historical_face_candidates: Vec::new(),
                resolved_edge_slot: Some(42),
                next_record_index: 103,
                next_byte_offset: 29,
            },
        )
        .unwrap();
    (scope, group, selection)
}

fn assert_historical_split_face_path_refusal(
    operation: &'static str,
    collection_limit: Option<u64>,
    retained_limit: Option<u64>,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let (scope, group, selection) = historical_split_face_path_fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    if let Some(limit) = collection_limit {
        policy.limits.max_collection_items = limit;
    }
    if let Some(limit) = retained_limit {
        let feature = crate::ids::neutral_feature_id(&scope);
        let prefix = crate::ids::history_input_prefix(&feature.key(), 7);
        let state_bytes = if operation == "f3d SplitFace path group id" {
            crate::ids::feature_input_topology_id(&feature, 7)
                .as_str()
                .len()
        } else {
            0
        };
        policy.limits.max_retained_bytes = limit
            + u64::try_from(feature.as_str().len() + prefix.as_str().len() + state_bytes).unwrap();
    }
    let dimension = if collection_limit.is_some() {
        ResourceDimension::CollectionItems
    } else {
        ResourceDimension::RetainedBytes
    };
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
            (super::super::resolved_split_face_path(
                &ctx,
                &scope,
                &group,
                std::slice::from_ref(&selection),
                &[],
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
    let result = super::super::resolved_split_face_path(
        &ctx,
        &scope,
        &group,
        std::slice::from_ref(&selection),
        &[],
    );
    assert!(
        matches!(result, Err(CodecError::ResourceLimit(ref failure))
        if failure.operation == operation && failure.dimension == dimension),
        "expected {operation} refusal, got {result:?}"
    );
}

#[test]
fn split_face_path_edge_slot_refuses_collection_limit() {
    assert_historical_split_face_path_refusal("f3d SplitFace path edge slot", Some(0), None);
}

#[test]
fn split_face_historical_edge_refuses_collection_limit() {
    assert_historical_split_face_path_refusal("f3d SplitFace historical edge", Some(1), None);
}

#[test]
fn split_face_historical_edge_id_refuses_retained_limit() {
    let (scope, _, _) = historical_split_face_path_fixture();
    let feature = crate::ids::neutral_feature_id(&scope);
    let prefix = crate::ids::history_input_prefix(&feature.key(), 7);
    let edge = crate::ids::history_input_edge_id(&prefix, 42);
    assert_historical_split_face_path_refusal(
        "f3d SplitFace historical edge id",
        None,
        Some(u64::try_from(edge.as_str().len() - 1).unwrap()),
    );
}

#[test]
fn split_face_path_group_id_refuses_retained_limit() {
    let (scope, group, _) = historical_split_face_path_fixture();
    let feature = crate::ids::neutral_feature_id(&scope);
    let prefix = crate::ids::history_input_prefix(&feature.key(), 7);
    let edge = crate::ids::history_input_edge_id(&prefix, 42);
    let total = edge.as_str().len() + group.id.len() - 1;
    assert_historical_split_face_path_refusal(
        "f3d SplitFace path group id",
        None,
        Some(u64::try_from(total).unwrap()),
    );
}

#[test]
fn draft_historical_face_group_id_refuses_retained_limit() {
    use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
    use crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate;
    use crate::records::topology::fillet::HistoricalBinding;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (scope, group, mut selection) = historical_split_face_path_fixture();
    selection.historical_face_candidates = vec![DesignEntitySelectionFaceCandidate {
        history_id: "history".into(),
        historical: HistoricalBinding {
            kind: AsmHistoricalEntityKind::Coedge,
            entity_ref: 225,
            state_ids: vec![7],
        },
        face_slot: 158,
    }];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    let feature = crate::ids::neutral_feature_id(&scope);
    let prefix = crate::ids::history_input_prefix(&feature.key(), 7);
    let state = crate::ids::feature_input_topology_id(&feature, 7);
    let face = crate::ids::history_input_face_id(
        &prefix,
        selection.historical_face_candidates[0].face_slot,
    );
    policy.limits.max_retained_bytes = u64::try_from(
        group.id.len() - 1
            + feature.as_str().len()
            + prefix.as_str().len()
            + state.as_str().len()
            + face.as_str().len(),
    )
    .unwrap();

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = super::super::selected_historical_face_selection(
        &ctx,
        &scope,
        &group,
        std::slice::from_ref(&selection),
        &[],
    );
    assert!(
        matches!(result, Err(CodecError::ResourceLimit(ref failure))
        if failure.operation == "f3d Draft historical face group id"
            && failure.dimension == ResourceDimension::RetainedBytes),
        "expected Draft historical face group ID refusal, got {result:?}"
    );
}
