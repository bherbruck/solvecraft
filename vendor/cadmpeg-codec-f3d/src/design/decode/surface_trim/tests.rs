// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]
use cadmpeg_core::decode::u64_from_index;

use super::exact_surface_trim_operation;
use crate::records::feature::scope::DesignParameterScope;
use crate::test_support::indexed_header;
use crate::test_support::lp_utf16;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::io::{Cursor, Write};
use zip::CompressionMethod;

fn surface_trim_selection_and_cell_table() -> (Vec<u8>, DesignParameterScope) {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"344", 811);
    bytes.extend_from_slice(&[0; 11]);
    bytes.push(1);
    bytes.extend_from_slice(&814u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "00000000-0000-0000-0000-000000000000");
    lp_utf16(&mut bytes, "00000000-0000-0000-0000-000000000000");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    indexed_header(&mut bytes, *b"257", 811);
    bytes.extend_from_slice(&[0; 10]);
    indexed_header(&mut bytes, *b"358", 812);
    bytes.extend_from_slice(&[0; 10]);
    indexed_header(&mut bytes, *b"272", 813);
    bytes.extend_from_slice(&[0; 10]);
    indexed_header(&mut bytes, *b"344", 814);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&123u64.to_le_bytes());
    indexed_header(&mut bytes, *b"288", 815);
    indexed_header(&mut bytes, *b"271", 816);
    indexed_header(&mut bytes, *b"325", 817);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for (record_index, ordinal) in [(819u32, 1u64), (820, 4)] {
        bytes.push(1);
        bytes.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&ordinal.to_le_bytes());
    }
    bytes.extend_from_slice(&5u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    indexed_header(&mut bytes, *b"257", 817);
    indexed_header(&mut bytes, *b"286", 819);
    indexed_header(&mut bytes, *b"351", 820);

    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#800",
        crate::records::feature::scope::DesignFeatureKind::SurfaceTrim,
        800,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![801, 804, 808, 811]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope)
}

#[test]
fn surface_trim_output_refuses_identifier_and_collection_limits() {
    const ENTRY: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let (bytes, mut scope) = surface_trim_selection_and_cell_table();
    scope.id = format!(
        "{}:design-parameter-scope#800",
        crate::ids::native_scope(ENTRY)
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    zip.start_file(ENTRY, stored).unwrap();
    zip.write_all(&bytes).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let operations = super::decode_surface_trim_operations(
            &cadmpeg_test_support::service_decode_context(),
            scan,
            &[scope.clone()],
        )
        .unwrap();
        assert_eq!(operations.len(), 1);
        assert_eq!(
            operations[0].id,
            format!(
                "{}:design-surface-trim-operation#{}",
                crate::ids::native_scope(ENTRY),
                scope.byte_offset(),
            ),
        );
        let scope_len = u64_from_index(crate::ids::native_scope(ENTRY).len());
        for (items, retained, dimension, operation) in [
            (
                27,
                u64::MAX,
                ResourceDimension::CollectionItems,
                "f3d surface-trim operations",
            ),
            (
                u64::MAX,
                scope_len + 2 * 36,
                ResourceDimension::RetainedBytes,
                "f3d native stream key",
            ),
            (
                u64::MAX,
                scope_len * 2 + 2 * 36,
                ResourceDimension::RetainedBytes,
                "f3d surface-trim operation identifier",
            ),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_collection_items = items;
            policy.limits.max_retained_bytes = retained;
            let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
                dimension,
                operation,
                |cap| {
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
                        cadmpeg_core::decode::ResourceDimension::RecursionDepth => {
                            policy.limits.max_recursion_depth = cap;
                        }
                        dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                    }
                    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                    (super::decode_surface_trim_operations(&ctx, scan, &[scope.clone()]))
                        .map(|_| ())
                },
            ) {
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
                cadmpeg_core::decode::ResourceDimension::RecursionDepth => {
                    policy.limits.max_recursion_depth = refusal_cap;
                }
                dimension => panic!("unsupported refusal dimension: {dimension:?}"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = super::decode_surface_trim_operations(&ctx, scan, &[scope.clone()]);
            assert!(
                matches!(
                    &result,
                    Err(CodecError::ResourceLimit(failure))
                        if failure.dimension == dimension && failure.operation == operation
                ),
                "item limit {items}, retained limit {retained}: {result:?}"
            );
        }
    });
}

#[test]
fn surface_trim_decodes_selection_chain_and_cell_table() {
    let (bytes, scope) = surface_trim_selection_and_cell_table();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default()).unwrap();
    let operation = exact_surface_trim_operation(
        &ctx,
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
    )
    .unwrap()
    .expect("exact SurfaceTrim cell carrier");

    assert_eq!(operation.selection_record_index, 811);
    assert_eq!(operation.selection_next_record_index, 815);
    assert_eq!(
        operation
            .chain_records
            .iter()
            .map(|record| (
                record.record_index,
                record.class_tag.as_str(),
                record.frame_length
            ))
            .collect::<Vec<_>>(),
        vec![(815, "288", 11), (816, "271", 11)]
    );
    assert_eq!(operation.cell_table_record_index, 817);
    assert_eq!(operation.cell_table_class_tag.as_str(), "325");
    assert_eq!(operation.cell_table_paired_class_tag.as_str(), "257");
    assert_eq!(operation.cell_entries().len(), 2);
    assert_eq!(
        operation
            .cell_entries()
            .iter()
            .map(|entry| (entry.record_index, entry.ordinal))
            .collect::<Vec<_>>(),
        vec![(819, 1), (820, 4)]
    );
    assert_eq!(operation.trailing_value, 5);
    assert_eq!(
        operation.trailing_zero_offset,
        operation.trailing_value_offset + 4
    );
}

#[test]
fn surface_trim_rejects_cell_ordinal_outside_partition() {
    let (mut bytes, scope) = surface_trim_selection_and_cell_table();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default()).unwrap();
    let table_start = bytes
        .windows(11)
        .position(|window| window == [3, 0, 0, 0, b'3', b'2', b'5', 0x31, 3, 0, 0])
        .expect("cell table header");
    let ordinal = table_start + 25 + 11;
    assert_eq!(
        bytes[ordinal..ordinal + 8],
        1u64.to_le_bytes(),
        "first cell ordinal"
    );
    bytes[ordinal..ordinal + 8].copy_from_slice(&6u64.to_le_bytes());
    assert!(exact_surface_trim_operation(
        &ctx,
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope
    )
    .unwrap()
    .is_none());
}

#[test]
fn surface_trim_rejects_nonzero_cell_table_tail() {
    let (mut bytes, scope) = surface_trim_selection_and_cell_table();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default()).unwrap();
    let table_start = bytes
        .windows(11)
        .position(|window| window == [3, 0, 0, 0, b'3', b'2', b'5', 0x31, 3, 0, 0])
        .expect("cell table header");
    let tail_zero = table_start + 67;
    bytes[tail_zero] = 1;
    assert!(exact_surface_trim_operation(
        &ctx,
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope
    )
    .unwrap()
    .is_none());
}

fn surface_trim_refusal(maximum: u64) -> CodecError {
    let (bytes, scope) = surface_trim_selection_and_cell_table();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = maximum;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    exact_surface_trim_operation(
        &ctx,
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
    )
    .expect_err("two cell entries exceed the selected collection limit")
}

#[test]
fn surface_trim_cell_entries_refuse_collection_limit() {
    let error = surface_trim_refusal(1);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "f3d surface-trim cell entries"));
}

#[test]
fn surface_trim_record_indices_refuse_collection_limit() {
    let error = surface_trim_refusal(3);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "f3d surface-trim cell record indices"));
}

#[test]
fn surface_trim_ordinals_refuse_collection_limit() {
    let error = surface_trim_refusal(5);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "f3d surface-trim cell ordinals"));
}
