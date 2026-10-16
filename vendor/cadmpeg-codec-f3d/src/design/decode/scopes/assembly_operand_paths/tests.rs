// SPDX-License-Identifier: Apache-2.0

use super::{exact_assembly_operand_path, exact_assembly_operand_path_envelope};
use crate::records::feature::assembly::DesignAssemblyOperandPathLink;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

const GUID: &str = "11111111-1111-1111-1111-111111111111";

fn path_link() -> DesignAssemblyOperandPathLink {
    DesignAssemblyOperandPathLink {
        locator_reference_offset: 0,
        locator_record_index: 64,
        locator_class_tag: "304".to_owned().try_into().unwrap(),
        locator_byte_offset: 0,
        locator_scope_reference_offset: 0,
        wrapper_record_index: 66,
        wrapper_reference_offset: 0,
        wrapper_class_tag: "382".to_owned().try_into().unwrap(),
        wrapper_byte_offset: 0,
        path_reference_offset: 0,
    }
}

fn path_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"329");
    bytes.extend_from_slice(&65_u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    for unit in GUID.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

fn context(arena: &DecodeArena, collection_limit: u64) -> DecodeContext<'_> {
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = collection_limit;
    DecodeContext::from_root_bytes(&[], arena, &policy)
        .unwrap()
        .0
}

#[test]
fn assembly_path_occurrences_refuse_collection_limit() {
    let bytes = path_bytes();
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let error =
        exact_assembly_operand_path(&ctx, &bytes, 0, 65, bytes.len(), path_link()).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(failure)
        if failure.dimension == ResourceDimension::CollectionItems
            && failure.operation == "f3d assembly path occurrences")
    );

    let arena = DecodeArena::new();
    let ctx = context(&arena, 1);
    let path = exact_assembly_operand_path(&ctx, &bytes, 0, 65, bytes.len(), path_link())
        .unwrap()
        .unwrap();
    assert_eq!(path.occurrence_guids().len(), 1);
}

fn write_reference(bytes: &mut [u8], at: usize, index: u32) {
    bytes[at] = 1;
    bytes[at + 1..at + 5].copy_from_slice(&index.to_le_bytes());
}

fn envelope_bytes(scope_record_index: u32) -> Vec<u8> {
    let mut locator = vec![0; 190];
    locator[0..4].copy_from_slice(&3_u32.to_le_bytes());
    locator[4..7].copy_from_slice(b"304");
    locator[7..11].copy_from_slice(&64_u32.to_le_bytes());
    write_reference(&mut locator, 21, 164);
    for ordinal in 0..16 {
        let value = if ordinal % 5 == 0 { 1.0_f64 } else { 0.0 };
        locator[33 + ordinal * 8..41 + ordinal * 8].copy_from_slice(&value.to_le_bytes());
    }
    write_reference(&mut locator, 162, scope_record_index);
    write_reference(&mut locator, 173, 66);
    locator[184..188].copy_from_slice(&2_u32.to_le_bytes());
    locator.extend_from_slice(&path_bytes());

    let mut wrapper = vec![0; 37];
    wrapper[0..4].copy_from_slice(&3_u32.to_le_bytes());
    wrapper[4..7].copy_from_slice(b"382");
    wrapper[7..11].copy_from_slice(&66_u32.to_le_bytes());
    wrapper[21] = 1;
    wrapper[22..26].copy_from_slice(&1_u32.to_le_bytes());
    write_reference(&mut wrapper, 26, 65);
    locator.extend_from_slice(&wrapper);
    locator.extend_from_slice(&3_u32.to_le_bytes());
    locator.extend_from_slice(b"396");
    locator.extend_from_slice(&67_u32.to_le_bytes());
    locator
}

#[test]
fn assembly_path_spans_refuse_collection_limit() {
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#0",
        DesignFeatureKind::Assemble,
        10,
    );
    let bytes = envelope_bytes(scope.record_index);
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let error = exact_assembly_operand_path_envelope(&ctx, &bytes, &scope, 64, 0, 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(failure)
        if failure.dimension == ResourceDimension::CollectionItems
            && failure.operation == "f3d assembly path spans")
    );

    let arena = DecodeArena::new();
    let ctx = context(&arena, 2);
    let path = exact_assembly_operand_path_envelope(&ctx, &bytes, &scope, 64, 0, 0)
        .unwrap()
        .unwrap();
    assert_eq!(path.record_index, 65);
}
