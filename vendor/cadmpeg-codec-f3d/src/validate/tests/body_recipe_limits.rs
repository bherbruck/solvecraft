// SPDX-License-Identifier: Apache-2.0

fn operand() -> crate::records::topology::body_recipe::DesignBodyRecipeOperand {
    use crate::records::{
        mesh::DesignRelaxedGuidText,
        references::DesignClassTag,
        topology::body_recipe::{
            DesignBodyRecipeOperand, DesignBodyRecipeOperandDraft, DesignBodyRecipeReference,
            DesignOperandOwner,
        },
    };
    DesignBodyRecipeOperand::try_new(DesignBodyRecipeOperandDraft {
        id: "f3d:Design/BulkStream.dat:design-body-recipe-operand#100".into(),
        scope_record_index: 10,
        owner: DesignOperandOwner::ScopeReference {
            scope_reference_ordinal: 6,
        },
        record_index: 100,
        byte_offset: 1_000,
        class_tag: DesignClassTag::try_from("365".to_owned()).unwrap(),
        asset_id: DesignRelaxedGuidText::try_from(
            "11111111-1111-4111-8111-111111111111".to_owned(),
        )
        .unwrap(),
        asset_id_offset: 1_056,
        context_id: DesignRelaxedGuidText::try_from(
            "22222222-2222-4222-8222-222222222222".to_owned(),
        )
        .unwrap(),
        context_id_offset: 1_136,
        selector_tail: None,
        references: vec![DesignBodyRecipeReference {
            design_reference: 300,
            design_reference_offset: 1_025,
            form: 3,
            form_offset: 1_033,
            candidate_faces: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            preceding_body_slots: Vec::new(),
        }],
        nested_record_index: 103,
        nested_record_index_offset: 1_038,
        recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#0".into(),
        resolved_face_slot: None,
        resolved_body_state_id: None,
        resolved_body_slot: None,
        resolved_body_face_slots: Vec::new(),
        next_record_index: 104,
        next_byte_offset: 1_300,
    })
    .unwrap()
}

fn native(valid: bool) -> crate::native::F3dNative {
    use crate::records::{
        decal::DesignRecordHeader,
        feature::scope::{DesignFeatureKind, DesignParameterScope},
        identity::{RecordedValue, ReferenceRun},
        recipes::{
            ConstructionRecipe, ConstructionRecipeDesign, ConstructionRecipeKind,
            ConstructionRecipeSelector,
        },
        references::DesignClassTag,
    };
    let mut native = crate::native::F3dNative {
        design_body_recipe_operands: vec![operand()],
        ..Default::default()
    };
    if valid {
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#10",
            DesignFeatureKind::Hole,
            10,
        );
        scope
            .try_edit(|draft| {
                draft.reference_members = ReferenceRun::unlocated(vec![1, 2, 3, 4, 5, 6, 100]);
                draft.layout_fixture_references();
                draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
                draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
                draft.layout_fixture_tail();
            })
            .unwrap();
        native.design_parameter_scopes.push(scope);
        native.design_record_headers.push(DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:design-record-header#100".into(),
            record_index: 100,
            class_tag: DesignClassTag::try_from("365".to_owned()).unwrap(),
            byte_offset: 1_000,
        });
        native.construction_recipes.push(ConstructionRecipe {
            id: "f3d:Design/BulkStream.dat:construction-recipe#0".into(),
            byte_offset: 1_220,
            kind: ConstructionRecipeKind::Body,
            design: Some(ConstructionRecipeDesign {
                id: RecordedValue {
                    value: "301".into(),
                    offset: 1_197,
                },
                selector: Some(ConstructionRecipeSelector {
                    value: 104,
                    byte_offset: 1_200,
                }),
            }),
            recipe_index: 0,
            record_index: Some(RecordedValue {
                value: 103,
                offset: 0,
            }),
        });
    }
    native
}

fn nested_items(value: &serde_json::Value) -> u64 {
    match value {
        serde_json::Value::Array(values) => {
            u64::try_from(values.len()).unwrap() + values.iter().map(nested_items).sum::<u64>()
        }
        serde_json::Value::Object(values) => {
            u64::try_from(values.len()).unwrap() + values.values().map(nested_items).sum::<u64>()
        }
        _ => 0,
    }
}

fn reload_items(ir: &cadmpeg_ir::CadIr) -> u64 {
    let record = &ir
        .native
        .namespace("f3d")
        .unwrap()
        .arenas()
        .get("design_body_recipe_operands")
        .unwrap()[0];
    let fields = record.fields();
    // The record and its identity field count one item each.
    2 + u64::try_from(fields.len()).unwrap() + fields.values().map(nested_items).sum::<u64>()
}

fn body_recipe_error(
    valid: bool,
    after_reload_items: u64,
    max_retained: u64,
) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native(valid);
        if valid {
            ir.native
                .namespace_mut("f3d")
                .set_arena(
                    &cadmpeg_test_support::service_decode_context(),
                    "design_body_recipe_operands",
                    &native.design_body_recipe_operands,
                )
                .unwrap();
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = if valid {
            reload_items(&ir) + after_reload_items
        } else {
            after_reload_items
        };
        policy.limits.max_retained_bytes = max_retained;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_body_recipe_operands(&decode, &ctx, &mut Vec::new()).unwrap_err()
    })
}

#[test]
fn body_recipe_expected_index_refuses_collection_limit() {
    let error = body_recipe_error(true, 4, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D expected body recipe operands")
    );
}

#[test]
fn body_recipe_member_slot_refuses_collection_limit() {
    let error = body_recipe_error(true, 5, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D body recipe member slots")
    );
}

#[test]
fn body_recipe_record_refuses_collection_limit() {
    let error = body_recipe_error(true, 6, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D body recipe records")
    );
}

#[test]
fn body_recipe_invalid_finding_refuses_collection_limit() {
    let error = body_recipe_error(false, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn body_recipe_invalid_entity_refuses_retained_limit() {
    let error = body_recipe_error(false, u64::MAX, 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}
