// SPDX-License-Identifier: Apache-2.0

fn operand() -> crate::records::feature::work_geometry::DesignEdgeTreatmentVertexOperand {
    use crate::records::{
        feature::work_geometry::{
            DesignEdgeTreatmentVertexOperand, DesignVertexRecipe, DesignVertexRecipeDraft,
        },
        references::DesignClassTag,
    };
    DesignEdgeTreatmentVertexOperand {
        id: "f3d:Design/BulkStream.dat:edge-treatment-vertex-operand#100".into(),
        scope_record_index: 10,
        scope_reference_ordinal: 0,
        group_record_index: 20,
        group_member_ordinal: 0,
        recipe: DesignVertexRecipe::try_new(DesignVertexRecipeDraft {
            record_index: 100,
            byte_offset: 1_000,
            class_tag: DesignClassTag::try_from("306".to_owned()).unwrap(),
            paired_byte_offset: 1_016,
            paired_class_tag: DesignClassTag::try_from("261".to_owned()).unwrap(),
            recipe_record_index: 103,
            recipe_record_byte_offset: 1_032,
            recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#0".into(),
            recipe_prefix_offset: 1_043,
            recipe_prefix_bytes: Vec::new(),
            recipe_references: Vec::new(),
            recipe_program_offset: 1_050,
            recipe_program: vec![0],
            resolution: None,
            next_record_index: 105,
            next_byte_offset: 1_054,
        })
        .unwrap(),
    }
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
        .get("design_edge_treatment_vertex_operands")
        .unwrap()[0];
    let fields = record.fields();
    // The record and its identity field count one item each.
    2 + u64::try_from(fields.len()).unwrap() + fields.values().map(nested_items).sum::<u64>()
}

fn vertex_error(stored: bool, extra_items: u64, max_retained: u64) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = crate::native::F3dNative {
            design_edge_treatment_vertex_operands: vec![operand()],
            ..Default::default()
        };
        if stored {
            ir.native
                .namespace_mut("f3d")
                .set_arena(
                    &cadmpeg_test_support::service_decode_context(),
                    "design_edge_treatment_vertex_operands",
                    &native.design_edge_treatment_vertex_operands,
                )
                .unwrap();
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = if stored {
            reload_items(&ir) + extra_items
        } else {
            extra_items
        };
        policy.limits.max_retained_bytes = max_retained;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_edge_treatment_vertex_operands(&decode, &ctx, &mut Vec::new())
            .unwrap_err()
    })
}

#[test]
fn vertex_operand_expected_index_refuses_collection_limit() {
    let error = vertex_error(true, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D expected edge treatment vertex operands")
    );
}

#[test]
fn vertex_operand_generated_id_refuses_retained_limit() {
    let error = vertex_error(false, u64::MAX, 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D native record ID")
    );
}

#[test]
fn vertex_operand_invalid_finding_refuses_collection_limit() {
    let error = vertex_error(false, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn vertex_operand_invalid_entity_refuses_retained_limit() {
    let native = operand();
    let stream = super::super::design_stream(&native.id);
    let expected_id = crate::ids::native_scoped_id(
        stream,
        "edge-treatment-vertex-operand",
        native.recipe.byte_offset(),
    );
    let error = vertex_error(false, u64::MAX, u64::try_from(expected_id.len()).unwrap());
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

fn vertex_recipe_valid_at(recipe_offset: u64) -> bool {
    use crate::records::{
        decal::DesignRecordHeader,
        feature::{
            scope::{DesignFeatureKind, DesignParameterScope},
            work_geometry::DesignVertexRecipe,
        },
        recipes::{ConstructionRecipe, ConstructionRecipeKind},
    };
    crate::test_support::with_decode_context(|decode| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut draft = operand().recipe.into_draft();
        draft.byte_offset = recipe_offset - 47;
        draft.paired_byte_offset = recipe_offset - 31;
        draft.recipe_record_byte_offset = recipe_offset - 15;
        draft.recipe_prefix_offset = recipe_offset - 4;
        draft.recipe_program_offset = recipe_offset.saturating_add(18);
        draft.next_byte_offset = draft.recipe_program_offset.saturating_add(4);
        let vertex = DesignVertexRecipe::try_new(draft).unwrap();
        let stream = super::super::design_stream(&vertex.recipe_id);
        let scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:scope#10",
            DesignFeatureKind::WorkPoint,
            10,
        );
        let native = crate::native::F3dNative {
            design_record_headers: vec![DesignRecordHeader {
                id: "f3d:Design/BulkStream.dat:header#100".into(),
                record_index: 100,
                class_tag: vertex.class_tag.clone(),
                byte_offset: vertex.byte_offset(),
            }],
            construction_recipes: vec![ConstructionRecipe {
                id: vertex.recipe_id.clone(),
                byte_offset: recipe_offset,
                kind: ConstructionRecipeKind::Vertex,
                design: None,
                recipe_index: 0,
                record_index: None,
            }],
            ..Default::default()
        };
        let ctx = super::super::Ctx::new(&ir, &native, decode).unwrap();
        super::super::valid_vertex_recipe(&ctx, &scope, stream, 100, &vertex).unwrap()
    })
}

#[test]
fn vertex_recipe_preserves_representable_program_layout() {
    assert!(vertex_recipe_valid_at(1_047));
}

#[test]
fn vertex_recipe_rejects_overflowed_program_origin() {
    assert!(!vertex_recipe_valid_at(u64::MAX - 17));
}

#[test]
fn vertex_recipe_rejects_overflowed_program_extent() {
    assert!(!vertex_recipe_valid_at(u64::MAX - 20));
}
