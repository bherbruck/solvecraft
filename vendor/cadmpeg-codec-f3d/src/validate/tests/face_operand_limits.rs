// SPDX-License-Identifier: Apache-2.0

#[derive(Clone, Copy, PartialEq, Eq)]
enum Case {
    Group,
    Expected,
    Faces,
    Unreferenced,
    Referenced,
    Alternate,
    NodeOffsets,
    Nodes,
    Record,
    Invalid,
    OverflowNodeOffset,
}

fn face_error(case: Case, max_items: u64, max_retained: u64) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use crate::records::{
            decal::DesignRecordHeader,
            feature::scope::{DesignFeatureKind, DesignParameterScope},
            identity::{RecordedValue, ReferenceRun},
            recipes::{ConstructionRecipe, ConstructionRecipeKind},
            references::DesignClassTag,
            sketch_links::PersistentSubentityTag,
            topology::face::{DesignFaceOperand, DesignFaceOperandDraft},
        };
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let stream = "f3d:Design/BulkStream.dat";
        let recipe_id = format!("{stream}:construction-recipe#0");
        let mut prefix = Vec::new();
        if matches!(case, Case::Referenced | Case::Alternate) {
            prefix.extend_from_slice(&[0; 10]);
            for word in [1u32, 3, 4, 1, 2] {
                prefix.extend_from_slice(&word.to_le_bytes());
            }
            prefix.extend_from_slice(b"13");
            for word in [0u32, 1, 1, 0, 0] {
                prefix.extend_from_slice(&word.to_le_bytes());
            }
            assert_eq!(
                crate::test_support::with_decode_context(|ctx| {
                    crate::design::decode::dimension_frames::decode_recipe_references_charged(
                        ctx, &prefix, 1_043,
                    )
                    .expect("recipe references")
                })
                .len(),
                1
            );
        }
        let prefix_len = u64::try_from(prefix.len()).unwrap();
        let program_offset = 1_063 + prefix_len;
        let program = if matches!(case, Case::NodeOffsets | Case::Nodes) {
            vec![-1, -1, 2]
        } else {
            vec![0, -1]
        };
        let next_byte_offset = program_offset + u64::try_from(program.len()).unwrap() * 4;
        let mut operand = DesignFaceOperand::try_new(DesignFaceOperandDraft {
            id: format!("{stream}:design-face-operand#100"),
            scope_record_index: 10,
            scope_reference_ordinal: 2,
            group: None,
            record_index: 100,
            byte_offset: 1_000,
            class_tag: DesignClassTag::try_from("365".to_owned()).unwrap(),
            paired_byte_offset: 1_016,
            paired_class_tag: DesignClassTag::try_from("366".to_owned()).unwrap(),
            recipe_record_index: 103,
            recipe_record_byte_offset: 1_032,
            recipe_id: recipe_id.clone(),
            recipe_prefix_offset: 1_043,
            recipe_references: crate::test_support::with_decode_context(|ctx| {
                crate::design::decode::dimension_frames::decode_recipe_references_charged(
                    ctx, &prefix, 1_043,
                )
                .expect("recipe references")
            }),
            recipe_prefix_bytes: prefix,
            recipe_kind: ConstructionRecipeKind::Face,
            recipe_program_offset: program_offset,
            recipe_program: program,
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
            next_byte_offset,
        })
        .unwrap();
        if case == Case::OverflowNodeOffset {
            operand.recipe_program_offset = u64::MAX;
            operand.recipe_program = vec![0, -1, -1, 2];
        }
        let mut native = crate::native::F3dNative {
            design_face_operands: vec![operand.clone()],
            ..Default::default()
        };
        if case == Case::Group {
            native.design_construction_operand_groups =
                super::construction_group_limits::native(false, false)
                    .design_construction_operand_groups;
        }
        if case == Case::Record {
            let mut scope = DesignParameterScope::empty(
                &format!("{stream}:design-parameter-scope#10"),
                DesignFeatureKind::OffsetFaces,
                10,
            );
            scope
                .try_edit(|draft| {
                    draft.reference_members = ReferenceRun::unlocated(vec![1, 2, 100]);
                    draft.layout_fixture_references();
                    draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
                    draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
                    draft.layout_fixture_tail();
                })
                .unwrap();
            native.design_parameter_scopes.push(scope);
            native.design_record_headers.push(DesignRecordHeader {
                id: format!("{stream}:design-record-header#100"),
                record_index: 100,
                class_tag: DesignClassTag::try_from("365".to_owned()).unwrap(),
                byte_offset: 1_000,
            });
        }
        if matches!(
            case,
            Case::Faces | Case::Unreferenced | Case::Referenced | Case::Alternate | Case::Record
        ) {
            native.construction_recipes.push(ConstructionRecipe {
                id: recipe_id,
                byte_offset: 1_047 + prefix_len,
                kind: ConstructionRecipeKind::Face,
                design: None,
                recipe_index: 0,
                record_index: if case == Case::Record {
                    None
                } else {
                    Some(RecordedValue {
                        value: 1,
                        offset: 0,
                    })
                },
            });
        }
        if matches!(
            case,
            Case::Faces | Case::Unreferenced | Case::Referenced | Case::Alternate
        ) {
            native
                .persistent_subentity_tags
                .push(PersistentSubentityTag {
                    id: format!("{stream}:persistent-subentity-tag#1"),
                    target: cadmpeg_ir::attributes::AttributeTarget::Face(
                        ir.model.faces[0].id.clone(),
                    ),
                    selector: 1,
                    token: cadmpeg_core::text::NonBlankString::new(
                        if matches!(case, Case::Referenced | Case::Alternate) {
                            "13"
                        } else {
                            "1"
                        },
                    )
                    .unwrap(),
                    design_references: vec![1],
                    ordinal: 0,
                });
            if case == Case::Alternate {
                native
                    .persistent_subentity_tags
                    .push(PersistentSubentityTag {
                        id: format!("{stream}:persistent-subentity-tag#2"),
                        target: cadmpeg_ir::attributes::AttributeTarget::Face(
                            ir.model.faces[1].id.clone(),
                        ),
                        selector: 2,
                        token: cadmpeg_core::text::NonBlankString::new("13").unwrap(),
                        design_references: vec![1],
                        ordinal: 1,
                    });
            }
        }
        let expected = if matches!(case, Case::Expected | Case::Record) {
            vec![operand]
        } else {
            Vec::new()
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        policy.limits.max_retained_bytes = max_retained;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_face_operands(&ctx, &mut Vec::new(), &expected).unwrap_err()
    })
}

macro_rules! refuse_items {
    ($name:ident, $case:expr, $limit:expr, $operation:literal) => {
        #[test]
        fn $name() {
            let error = face_error($case, $limit, u64::MAX);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == $operation));
        }
    };
}

refuse_items!(
    face_operand_group_index_refuses_collection_limit,
    Case::Group,
    0,
    "index F3D face operand groups"
);
refuse_items!(
    face_operand_expected_index_refuses_collection_limit,
    Case::Expected,
    0,
    "index F3D expected face operands"
);
refuse_items!(
    face_operand_expected_faces_refuse_collection_limit,
    Case::Faces,
    0,
    "collect F3D expected operand faces"
);
refuse_items!(
    face_operand_unreferenced_faces_refuse_collection_limit,
    Case::Unreferenced,
    1,
    "collect F3D unreferenced operand faces"
);
refuse_items!(
    face_operand_referenced_faces_refuse_collection_limit,
    Case::Referenced,
    4,
    "index F3D referenced operand faces"
);
refuse_items!(
    face_operand_alternate_faces_refuse_collection_limit,
    Case::Alternate,
    8,
    "collect F3D alternate selector operand faces"
);
refuse_items!(
    face_operand_node_offsets_refuse_collection_limit,
    Case::NodeOffsets,
    0,
    "collect F3D face recipe node offsets"
);
refuse_items!(
    face_operand_nodes_refuse_collection_limit,
    Case::Nodes,
    1,
    "collect F3D face recipe nodes"
);
refuse_items!(
    face_operand_record_refuses_collection_limit,
    Case::Record,
    1,
    "index F3D face operand records"
);
refuse_items!(
    face_operand_invalid_finding_refuses_collection_limit,
    Case::Invalid,
    0,
    "collect F3D native validation findings"
);

#[test]
fn face_operand_invalid_entity_refuses_retained_limit() {
    let error = face_error(Case::Invalid, u64::MAX, 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

#[test]
fn face_operand_rejects_overflowed_node_offset() {
    let error = face_error(Case::OverflowNodeOffset, u64::MAX, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::Malformed(ref message)
        if message == "F3D face recipe node offset overflows")
    );
}
