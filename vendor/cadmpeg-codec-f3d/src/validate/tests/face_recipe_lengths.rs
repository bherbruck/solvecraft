// SPDX-License-Identifier: Apache-2.0
use crate::test_support::native_test::f3d_native_mut;

#[test]
fn only_a_face_recipe_kind_states_a_program_operand_length() {
    use crate::records::recipes::ConstructionRecipeKind;

    assert_eq!(
        crate::validate::recipe_program_operand_length(ConstructionRecipeKind::Face),
        Some(16)
    );
    assert_eq!(
        crate::validate::recipe_program_operand_length(ConstructionRecipeKind::BoundedFace),
        Some(24)
    );
    for kind in [
        ConstructionRecipeKind::Body,
        ConstructionRecipeKind::Edge,
        ConstructionRecipeKind::Vertex,
    ] {
        assert_eq!(crate::validate::recipe_program_operand_length(kind), None);
    }
}

/// The face-operand route refuses a recipe kind that states no face operand.
///
/// `validate_face_operands` reads the operand length through
/// `recipe_program_operand_length`, so a `Body`, `Edge` or `Vertex` recipe
/// states no program offset and the operand is invalid. The `Face` fixture is
/// the control: it differs only in the recipe kind and the operand length that
/// kind states, and it raises no finding.
#[test]
fn a_face_operand_whose_recipe_kind_states_no_face_operand_is_refused() {
    use crate::records::recipes::ConstructionRecipeKind;

    let admitted = face_findings(ConstructionRecipeKind::Face, 16, 1_047);
    assert!(admitted.is_empty(), "{admitted:#?}");
    assert_eq!(
        face_findings(ConstructionRecipeKind::Body, 16, 1_047).len(),
        1
    );
}

fn face_findings(
    kind: crate::records::recipes::ConstructionRecipeKind,
    operand_length: u64,
    recipe_byte_offset: u64,
) -> Vec<cadmpeg_ir::report::check::Finding> {
    use crate::records::{
        decal::DesignRecordHeader,
        feature::scope::{DesignFeatureKind, DesignParameterScope},
        recipes::ConstructionRecipe,
        references::DesignClassTag,
        topology::{face::DesignFaceOperand, face::DesignFaceOperandDraft},
    };

    let stream = "f3d:Design/BulkStream.dat";
    let operand_id = format!("{stream}:design-face-operand#100");
    let recipe_id = format!("{stream}:construction-recipe#0");

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let mut scope = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#10"),
        DesignFeatureKind::OffsetFaces,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![1, 2, 100]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let class_tag = DesignClassTag::try_from("365".to_owned()).unwrap();
    let recipe_program_offset = recipe_byte_offset.saturating_add(operand_length);
    let operand = DesignFaceOperand::try_new(DesignFaceOperandDraft {
        id: operand_id.clone(),
        scope_record_index: 10,
        scope_reference_ordinal: 2,
        group: None,
        record_index: 100,
        byte_offset: recipe_byte_offset - 47,
        class_tag: class_tag.clone(),
        paired_byte_offset: recipe_byte_offset - 31,
        paired_class_tag: DesignClassTag::try_from("366".to_owned()).unwrap(),
        recipe_record_index: 103,
        recipe_record_byte_offset: recipe_byte_offset - 15,
        recipe_id: recipe_id.clone(),
        recipe_prefix_offset: recipe_byte_offset - 4,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: Vec::new(),
        recipe_kind: kind,
        recipe_program_offset,
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
        next_byte_offset: recipe_program_offset.saturating_add(8),
    })
    .unwrap();
    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_parameter_scopes = vec![scope];
        native.design_record_headers = vec![DesignRecordHeader {
            id: format!("{stream}:design-record-header#100"),
            record_index: 100,
            class_tag,
            byte_offset: recipe_byte_offset - 47,
        }];
        native.construction_recipes = vec![ConstructionRecipe {
            id: recipe_id.clone(),
            byte_offset: recipe_byte_offset,
            kind,
            design: None,
            recipe_index: 0,
            record_index: None,
        }];
        native.design_face_operands = vec![operand];
    }
    crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native_charged(ctx, &ir).expect("service native validation")
    })
    .into_iter()
    .filter(|finding| {
        finding.message == "Fusion Design face operand has an invalid scope or recipe frame"
            && finding.entity.as_deref() == Some(operand_id.as_str())
    })
    .collect::<Vec<_>>()
}

#[test]
fn face_operand_rejects_overflowed_program_origin() {
    let findings = face_findings(
        crate::records::recipes::ConstructionRecipeKind::Face,
        16,
        u64::MAX - 15,
    );
    assert_eq!(findings.len(), 1);
}

#[test]
fn face_operand_rejects_overflowed_program_extent() {
    let findings = face_findings(
        crate::records::recipes::ConstructionRecipeKind::Face,
        16,
        u64::MAX - 20,
    );
    assert_eq!(findings.len(), 1);
}
