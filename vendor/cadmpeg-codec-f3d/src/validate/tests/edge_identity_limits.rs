// SPDX-License-Identifier: Apache-2.0

fn native(valid: bool) -> crate::native::F3dNative {
    use crate::records::{
        decal::DesignRecordHeader,
        feature::scope::{DesignFeatureKind, DesignParameterScope},
        references::DesignClassTag,
    };
    let mut native = super::operand_group_carrier_limits::native_with_identity();
    if valid {
        native
            .design_parameter_scopes
            .push(DesignParameterScope::empty(
                "f3d:Design/BulkStream.dat:design-parameter-scope#10",
                DesignFeatureKind::Fillet,
                10,
            ));
        native.design_record_headers.push(DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:design-record-header#101".into(),
            record_index: 101,
            class_tag: DesignClassTag::try_from("297".to_owned()).unwrap(),
            byte_offset: 100,
        });
    }
    native
}

fn typed_reload_collection_cost(ir: &cadmpeg_ir::document::CadIr) -> u64 {
    fn nested_items(value: &serde_json::Value) -> u64 {
        match value {
            serde_json::Value::Array(values) => {
                u64::try_from(values.len()).unwrap() + values.iter().map(nested_items).sum::<u64>()
            }
            serde_json::Value::Object(values) => {
                u64::try_from(values.len()).unwrap()
                    + values.values().map(nested_items).sum::<u64>()
            }
            _ => 0,
        }
    }
    let record = &ir
        .native
        .namespace("f3d")
        .unwrap()
        .arenas()
        .get("design_edge_identity_operands")
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
            native
                .store(
                    &cadmpeg_test_support::service_decode_context(),
                    ir.native.namespace_mut("f3d"),
                )
                .unwrap();
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = if valid {
            typed_reload_collection_cost(&ir) + after_reload_items
        } else {
            after_reload_items
        };
        policy.limits.max_retained_bytes = max_retained;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_edge_identity_operands(&decode, &ctx, &mut Vec::new(), &[])
            .unwrap_err()
    })
}

#[test]
fn edge_identity_expected_index_refuses_collection_limit() {
    let error = edge_error(true, 4, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D expected edge identity operands")
    );
}

#[test]
fn edge_identity_slot_refuses_collection_limit() {
    let error = edge_error(true, 5, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D edge identity slots")
    );
}

#[test]
fn edge_identity_record_refuses_collection_limit() {
    let error = edge_error(true, 6, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D edge identity records")
    );
}

#[test]
fn edge_identity_invalid_finding_refuses_collection_limit() {
    let error = edge_error(false, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn edge_identity_invalid_entity_refuses_retained_limit() {
    let error = edge_error(false, u64::MAX, 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

#[test]
fn edge_identity_valid_slot_has_no_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native(true);
        native
            .store(
                &cadmpeg_test_support::service_decode_context(),
                ir.native.namespace_mut("f3d"),
            )
            .unwrap();
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        let mut findings = Vec::new();
        let records = crate::test_support::with_decode_context(|decode_ctx| {
            super::super::validate_edge_identity_operands(decode_ctx, &ctx, &mut findings, &[])
        })
        .unwrap();
        assert!(findings.is_empty());
        assert!(records.contains(&("f3d:Design/BulkStream.dat", 101)));
    })
}
