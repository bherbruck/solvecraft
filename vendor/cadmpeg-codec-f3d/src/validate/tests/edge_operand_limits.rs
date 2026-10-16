// SPDX-License-Identifier: Apache-2.0

fn operand() -> crate::records::topology::edge_identity::DesignEdgeOperand {
    serde_json::from_value(serde_json::json!({
        "id": "f3d:Design/BulkStream.dat:design-edge-operand#10",
        "scope_record_index": 1,
        "scope_reference_ordinal": 0,
        "record_index": 2,
        "byte_offset": 10,
        "class_tag": "346",
        "paired_byte_offset": 20,
        "paired_class_tag": "262",
        "recipe_record_index": 5,
        "recipe_record_byte_offset": 30,
        "recipe_id": "f3d:Design/BulkStream.dat:construction-recipe#0",
        "recipe_prefix_offset": 41,
        "recipe_prefix_bytes": "",
        "recipe_references": [],
        "recipe_program_offset": 50,
        "recipe_program": [],
        "next_record_index": 6,
        "next_byte_offset": 100
    }))
    .unwrap()
}

fn native(valid: bool) -> crate::native::F3dNative {
    use crate::records::{
        decal::DesignRecordHeader,
        feature::scope::{DesignFeatureKind, DesignParameterScope},
        identity::{RecordedValue, ReferenceRun},
        recipes::{ConstructionRecipe, ConstructionRecipeKind},
        references::DesignClassTag,
    };
    let mut native = crate::native::F3dNative {
        design_edge_operands: vec![operand()],
        ..Default::default()
    };
    if valid {
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#1",
            DesignFeatureKind::Fillet,
            1,
        );
        scope
            .try_edit(|draft| {
                draft.reference_members = ReferenceRun::unlocated(vec![2]);
                draft.layout_fixture_references();
                draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
                draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
                draft.layout_fixture_tail();
            })
            .unwrap();
        native.design_parameter_scopes.push(scope);
        native.design_record_headers.push(DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:design-record-header#2".into(),
            record_index: 2,
            class_tag: DesignClassTag::try_from("346".to_owned()).unwrap(),
            byte_offset: 10,
        });
        native.construction_recipes.push(ConstructionRecipe {
            id: "f3d:Design/BulkStream.dat:construction-recipe#0".into(),
            byte_offset: 45,
            kind: ConstructionRecipeKind::Edge,
            design: None,
            recipe_index: 0,
            record_index: Some(RecordedValue {
                value: 5,
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
        .get("design_edge_operands")
        .unwrap()[0];
    let fields = record.fields();
    // The record and its identity field count one item each.
    2 + u64::try_from(fields.len()).unwrap() + fields.values().map(nested_items).sum::<u64>()
}

fn edge_error(valid: bool, after_reload_items: u64, max_retained: u64) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native(valid);
        if valid {
            ir.native
                .namespace_mut("f3d")
                .set_arena(
                    &cadmpeg_test_support::service_decode_context(),
                    "design_edge_operands",
                    &native.design_edge_operands,
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
        super::super::validate_edge_operands(&decode, &ctx, &mut Vec::new()).unwrap_err()
    })
}

#[test]
fn edge_operand_expected_index_refuses_collection_limit() {
    let error = edge_error(true, 1, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D expected edge operands")
    );
}

#[test]
fn edge_operand_slot_refuses_collection_limit() {
    let error = edge_error(true, 2, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D edge operand slots")
    );
}

#[test]
fn edge_operand_record_refuses_collection_limit() {
    let error = edge_error(true, 3, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D edge operand records")
    );
}

#[test]
fn edge_operand_invalid_finding_refuses_collection_limit() {
    let error = edge_error(false, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn edge_operand_invalid_entity_refuses_retained_limit() {
    let error = edge_error(false, u64::MAX, 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}
