// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::scopes::combine::{
    exact_combine_operation, take_external_reference_identity,
};
use crate::design::decode::scopes::parameter_scope::parse_parameter_scope;
use crate::design::feature_project::project_combine;
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::combine::{
    DesignCombineBodySelection, DesignCombineForm, DesignCombineOperation,
};
use crate::records::feature::scope::DesignParameterScope;
use crate::test_support::indexed_header;
use crate::test_support::lp_utf16;
use crate::test_support::push_reference_u64;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

#[test]
fn external_reference_text_fields_refuse_retained_limits() {
    let asset = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
    let link = "component-body-link";
    let property = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";
    let version = "urn:test:version:2";
    let mut bytes = vec![1];
    bytes.extend_from_slice(&7_u64.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    lp_utf16(&mut bytes, asset);
    bytes.push(0);
    lp_utf16(&mut bytes, link);
    bytes.push(1);
    lp_utf16(&mut bytes, property);
    lp_utf16(&mut bytes, version);
    let total = asset.len() + link.len() + property.len() + version.len();
    for limit in [
        asset.len() - 1,
        asset.len() + link.len() - 1,
        asset.len() + link.len() + property.len() - 1,
        total - 1,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = u64_from_index(limit);

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = take_external_reference_identity(&ctx, &bytes, &mut 0);
        assert!(
            matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::RetainedBytes
                && failure.operation == "f3d Design UTF-16 text")
        );
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = u64_from_index(total);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut cursor = 0;
    let identity = take_external_reference_identity(&ctx, &bytes, &mut cursor)
        .unwrap()
        .expect("admitted external reference");
    assert_eq!(cursor, bytes.len());
    assert_eq!(identity.link_name, link);
    assert_eq!(identity.version.unwrap().version_urn.value, version);
}

#[test]
fn combine_selector_identity_text_refuses_retained_limits() {
    let scope_index = 90_u32;
    let mut bytes = vec![0_u8; 363];
    bytes[0..4].copy_from_slice(&3_u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"329");
    bytes[7..11].copy_from_slice(&scope_index.to_le_bytes());
    bytes[29] = 1;
    bytes[30] = 1;
    bytes[31..35].copy_from_slice(&2_u32.to_le_bytes());
    bytes[35] = 1;
    bytes[36..44].copy_from_slice(&700_u64.to_le_bytes());
    indexed_header(&mut bytes, *b"261", scope_index);
    indexed_header(&mut bytes, *b"304", 91);
    bytes.extend_from_slice(&[0; 9]);
    bytes.push(1);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&24_u32.to_le_bytes());
    bytes.extend_from_slice(b"DcFeatureOperationIdFlag");
    bytes.extend_from_slice(&23_u32.to_le_bytes());
    bytes.extend_from_slice(b"IntrinsicMetaTypeuint64");
    bytes.extend_from_slice(&7_u64.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    push_reference_u64(&mut bytes, 92);
    indexed_header(&mut bytes, *b"261", 91);
    indexed_header(&mut bytes, *b"312", 92);
    bytes.extend_from_slice(&[0; 14]);
    push_reference_u64(&mut bytes, 95);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    lp_utf16(&mut bytes, "11111111-1111-4111-8111-111111111111");
    lp_utf16(&mut bytes, "22222222-2222-4222-8222-222222222222");
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    push_reference_u64(&mut bytes, 5_001);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&6_001_u64.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&7_u32.to_le_bytes());
    lp_utf16(&mut bytes, "11111111-1111-4111-8111-111111111111");
    bytes.push(0);
    lp_utf16(&mut bytes, "component-body-link");
    bytes.push(1);
    lp_utf16(&mut bytes, "33333333-3333-4333-8333-333333333333");
    lp_utf16(&mut bytes, "urn:example:version:4");
    bytes.extend_from_slice(&9_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&11_u64.to_le_bytes());
    bytes.extend_from_slice(&48_u32.to_le_bytes());
    bytes.extend_from_slice(&12_u64.to_le_bytes());
    push_reference_u64(&mut bytes, 94);
    bytes.extend_from_slice(&[0; 2]);
    push_reference_u64(&mut bytes, 93);
    bytes.push(0);
    push_reference_u64(&mut bytes, u64::from(scope_index));
    indexed_header(&mut bytes, *b"261", 92);
    indexed_header(&mut bytes, *b"304", 93);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    push_reference_u64(&mut bytes, 94);
    bytes.extend_from_slice(&[0; 6]);
    indexed_header(&mut bytes, *b"261", 93);
    indexed_header(&mut bytes, *b"312", 94);
    lp_utf16(&mut bytes, "44444444-4444-4444-8444-444444444444");
    lp_utf16(&mut bytes, "55555555-5555-4555-8555-555555555555");
    indexed_header(&mut bytes, *b"261", 94);
    let mut scope = DesignParameterScope::empty(
        "scope",
        crate::records::feature::scope::DesignFeatureKind::Combine,
        scope_index,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = 0;
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.class_tag = "329".to_owned().try_into().unwrap();
    scope.paired_class_tag = "261".to_owned().try_into().unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 363;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![91, 92, 93, 94]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    for limit in [35_u64, 71_u64] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = limit;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = exact_combine_operation(&ctx, &bytes, &records, &scope);
        assert!(
            matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::RetainedBytes
                && failure.operation == "f3d Design UTF-16 text")
        );
    }
}

#[test]
fn combine_tools_refuse_collection_limit() {
    fn operation_record(bytes: &mut Vec<u8>, record_index: u32, selection_record_index: u32) {
        indexed_header(bytes, *b"283", record_index);
        bytes.extend_from_slice(&[0; 9]);
        bytes.push(1);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&24u32.to_le_bytes());
        bytes.extend_from_slice(b"DcFeatureOperationIdFlag");
        bytes.extend_from_slice(&23u32.to_le_bytes());
        bytes.extend_from_slice(b"IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&7u64.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&selection_record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        indexed_header(bytes, *b"259", record_index);
    }
    fn target_record(bytes: &mut Vec<u8>, record_index: u32, selection_record_index: u32) {
        indexed_header(bytes, *b"283", record_index);
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&selection_record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        indexed_header(bytes, *b"259", record_index);
    }
    let references = [91u32, 92, 93, 94, 95, 96];
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"382", 90);
    bytes.extend_from_slice(&[0; 9]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(0);
    bytes.push(1);
    bytes.extend_from_slice(&[0; 7]);
    bytes.resize(64, 0);
    bytes.extend_from_slice(
        &(u32::try_from(references.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    for reference in references {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&17u32.to_le_bytes());
    lp_utf16(&mut bytes, "Combine");
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&2u32.to_le_bytes());
    tail[31..35].copy_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    indexed_header(&mut bytes, *b"259", 90);
    for (ordinal, pair) in references.chunks_exact(2).enumerate() {
        if ordinal == 2 {
            target_record(&mut bytes, pair[0], pair[1]);
        } else {
            operation_record(&mut bytes, pair[0], pair[1]);
        }
        indexed_header(&mut bytes, *b"389", pair[1]);
        lp_utf16(&mut bytes, "00000000-0000-0000-0000-000000000001");
        lp_utf16(&mut bytes, "10000000-0000-0000-0000-000000000001");
        indexed_header(&mut bytes, *b"306", pair[1]);
    }
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 90,
        class_tag: crate::records::references::DesignClassTag::try_from("382".to_owned()).unwrap(),
        byte_offset: 0,
    };
    let scope = parse_parameter_scope(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        &records,
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
    .expect("Combine scope");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = exact_combine_operation(&ctx, &bytes, &records, &scope);
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d Combine additional tools"
    ));
    let arena = DecodeArena::new();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let operation = exact_combine_operation(&ctx, &bytes, &records, &scope)
        .unwrap()
        .expect("two admitted Combine tools");
    assert_eq!(operation.tools.first.record_index, 92);
    assert_eq!(operation.tools.additional[0].record_index, 94);
}

#[test]
fn combine_scope_projects_ordered_target_tools_and_retention() {
    fn operation_record(bytes: &mut Vec<u8>, record_index: u32, selection_record_index: u32) {
        indexed_header(bytes, *b"283", record_index);
        bytes.extend_from_slice(&[0; 9]);
        bytes.push(1);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&24u32.to_le_bytes());
        bytes.extend_from_slice(b"DcFeatureOperationIdFlag");
        bytes.extend_from_slice(&23u32.to_le_bytes());
        bytes.extend_from_slice(b"IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&7u64.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&selection_record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        indexed_header(bytes, *b"259", record_index);
    }
    fn target_record(bytes: &mut Vec<u8>, record_index: u32, selection_record_index: u32) {
        indexed_header(bytes, *b"283", record_index);
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&selection_record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        indexed_header(bytes, *b"259", record_index);
    }
    fn selection_record(bytes: &mut Vec<u8>, record_index: u32, suffix: u8) {
        indexed_header(bytes, *b"389", record_index);
        lp_utf16(
            bytes,
            &format!("00000000-0000-0000-0000-0000000000{suffix:02x}"),
        );
        lp_utf16(
            bytes,
            &format!("10000000-0000-0000-0000-0000000000{suffix:02x}"),
        );
        indexed_header(bytes, *b"306", record_index);
    }

    let scope_record_index = 90u32;
    let references = [91u32, 92, 93, 94, 95, 96];
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"382", scope_record_index);
    bytes.extend_from_slice(&[0; 9]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(0);
    bytes.push(1);
    bytes.extend_from_slice(&[0; 7]);
    bytes.resize(64, 0);
    bytes.extend_from_slice(
        &(u32::try_from(references.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    for reference in references {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&17u32.to_le_bytes());
    lp_utf16(&mut bytes, "Combine");
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&2u32.to_le_bytes());
    tail[31..35].copy_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    indexed_header(&mut bytes, *b"259", scope_record_index);
    for (ordinal, pair) in references.chunks_exact(2).enumerate() {
        if ordinal == 2 {
            target_record(&mut bytes, pair[0], pair[1]);
        } else {
            operation_record(&mut bytes, pair[0], pair[1]);
        }
        selection_record(&mut bytes, pair[1], u8::try_from(pair[1]).unwrap());
    }

    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: scope_record_index,
        class_tag: crate::records::references::DesignClassTag::try_from("382".to_owned()).unwrap(),
        byte_offset: 0,
    };
    let mut scope = parse_parameter_scope(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
    .expect("Combine scope");
    let operation = exact_combine_operation(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
    )
    .unwrap()
    .expect("Combine construction");
    assert_eq!(
        operation,
        DesignCombineOperation {
            form: DesignCombineForm::Standard,
            operation: cadmpeg_ir::features::BooleanKind::Join,
            operation_offset: 20,
            keep_tools: true,
            keep_tools_offset: 25,
            target_record_index: 96,
            tools: crate::records::feature::combine::DesignCombineTools {
                first: DesignCombineBodySelection {
                    record_index: 92,
                    external_identity: None,
                },
                additional: vec![DesignCombineBodySelection {
                    record_index: 94,
                    external_identity: None,
                },]
            },
        }
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::Combine(slot) =
        scope.payload_mut()
    {
        *slot = Some(operation);
    }
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| project_combine(
            decode_ctx,
            &scope,
            "Design1/BulkStream.dat"
        ))
        .unwrap(),
        Some(cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Combine {
                operands: cadmpeg_ir::features::CombineOperands::new(
                    cadmpeg_ir::features::BodySelection::Native(
                        "Design1/BulkStream.dat:design-record#96".into(),
                    ),
                    cadmpeg_ir::features::BodySelection::NativeSet(
                        vec![
                            "Design1/BulkStream.dat:design-record#92".into(),
                            "Design1/BulkStream.dat:design-record#94".into(),
                        ]
                        .try_into()
                        .unwrap()
                    ),
                    &cadmpeg_test_support::service_decode_context(),
                )
                .expect("operand admission")
                .unwrap(),

                op: cadmpeg_ir::features::BooleanKind::Join,
                keep_tools: true,
            }
        ))
    );

    let mut compact_bytes = bytes.clone();
    compact_bytes[4..7].copy_from_slice(b"387");
    compact_bytes[11..21].fill(0);
    compact_bytes[21..25].copy_from_slice(&1u32.to_le_bytes());
    compact_bytes[25] = 0;
    compact_bytes[26..29].fill(0);
    compact_bytes[29..31].copy_from_slice(&[1, 0]);
    compact_bytes[31..35].copy_from_slice(&1u32.to_le_bytes());
    compact_bytes[35] = 1;
    compact_bytes[36..44].copy_from_slice(&200u64.to_le_bytes());
    compact_bytes[44..46].fill(0);
    let mut compact_scope = scope.clone();
    compact_scope.class_tag =
        crate::records::references::DesignClassTag::try_from("387".to_owned()).unwrap();
    compact_scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap();
    compact_scope
        .try_edit(|draft| {
            draft.frame_length = 328;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let compact = exact_combine_operation(
        &cadmpeg_test_support::service_decode_context(),
        &compact_bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&compact_bytes),
        &compact_scope,
    )
    .unwrap()
    .expect("compact Combine construction");
    assert_eq!(compact.operation, cadmpeg_ir::features::BooleanKind::Join);
    assert_eq!(compact.operation_offset, 21);
    assert!(!compact.keep_tools);
    assert_eq!(compact.form, DesignCombineForm::Compact);
    assert_eq!(compact.target_record_index, 96);
    assert_eq!(
        compact
            .tools
            .iter()
            .map(|tool| tool.record_index)
            .collect::<Vec<_>>(),
        [92, 94]
    );

    let mut malformed_compact_tail = compact_bytes;
    malformed_compact_tail[45] = 1;
    assert!(exact_combine_operation(
        &cadmpeg_test_support::service_decode_context(),
        &malformed_compact_tail,
        &crate::design::test_support::indexed_record_offsets_for_test(&malformed_compact_tail),
        &compact_scope,
    )
    .unwrap()
    .is_none());
}

#[test]
fn combine_extended_reference_scope_retains_external_tool_identity() {
    fn operation_record(bytes: &mut Vec<u8>, record_index: u32, selection_record_index: u32) {
        indexed_header(bytes, *b"304", record_index);
        bytes.extend_from_slice(&[0; 9]);
        bytes.push(1);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&24u32.to_le_bytes());
        bytes.extend_from_slice(b"DcFeatureOperationIdFlag");
        bytes.extend_from_slice(&23u32.to_le_bytes());
        bytes.extend_from_slice(b"IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&7u64.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&selection_record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        indexed_header(bytes, *b"261", record_index);
    }
    fn target_record(bytes: &mut Vec<u8>, record_index: u32, selection_record_index: u32) {
        indexed_header(bytes, *b"304", record_index);
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&selection_record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        indexed_header(bytes, *b"261", record_index);
    }
    fn external_selection_record(bytes: &mut Vec<u8>, scope: u32, record_index: u32) {
        const ASSET: &str = "11111111-1111-4111-8111-111111111111";
        const CONTEXT: &str = "22222222-2222-4222-8222-222222222222";
        indexed_header(bytes, *b"312", record_index);
        bytes.extend_from_slice(&[0; 14]);
        push_reference_u64(bytes, u64::from(record_index + 3));
        bytes.extend_from_slice(&1u32.to_le_bytes());
        lp_utf16(bytes, ASSET);
        lp_utf16(bytes, CONTEXT);
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        push_reference_u64(bytes, 5_001);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&6_001u64.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&7u32.to_le_bytes());
        lp_utf16(bytes, ASSET);
        bytes.push(0);
        lp_utf16(bytes, "component-body-link");
        bytes.push(1);
        lp_utf16(bytes, "33333333-3333-4333-8333-333333333333");
        lp_utf16(bytes, "urn:example:version:4");
        bytes.extend_from_slice(&9u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&11u64.to_le_bytes());
        bytes.extend_from_slice(&48u32.to_le_bytes());
        bytes.extend_from_slice(&12u64.to_le_bytes());
        push_reference_u64(bytes, u64::from(record_index + 2));
        bytes.extend_from_slice(&[0; 2]);
        push_reference_u64(bytes, u64::from(record_index + 1));
        bytes.push(0);
        push_reference_u64(bytes, u64::from(scope));
        indexed_header(bytes, *b"261", record_index);
    }
    fn simple_selection_record(bytes: &mut Vec<u8>, record_index: u32) {
        indexed_header(bytes, *b"312", record_index);
        lp_utf16(bytes, "44444444-4444-4444-8444-444444444444");
        lp_utf16(bytes, "55555555-5555-4555-8555-555555555555");
        indexed_header(bytes, *b"261", record_index);
    }

    let scope_record_index = 90u32;
    let mut bytes = vec![0; 363];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"329");
    bytes[7..11].copy_from_slice(&scope_record_index.to_le_bytes());
    bytes[29] = 1;
    bytes[30] = 1;
    bytes[31..35].copy_from_slice(&2u32.to_le_bytes());
    bytes[35] = 1;
    bytes[36..44].copy_from_slice(&700u64.to_le_bytes());
    bytes[44..46].fill(0);
    indexed_header(&mut bytes, *b"261", scope_record_index);
    operation_record(&mut bytes, 91, 92);
    external_selection_record(&mut bytes, scope_record_index, 92);
    target_record(&mut bytes, 93, 94);
    simple_selection_record(&mut bytes, 94);

    let mut scope = DesignParameterScope::empty(
        "scope",
        crate::records::feature::scope::DesignFeatureKind::Combine,
        scope_record_index,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = 0;
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("329".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("261".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 363;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![91, 92, 93, 94]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let operation = exact_combine_operation(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        &records,
        &scope,
    )
    .unwrap()
    .expect("extended-reference Combine construction");
    assert_eq!(operation.form, DesignCombineForm::ExtendedReference);
    assert_eq!(operation.operation, cadmpeg_ir::features::BooleanKind::Cut);
    assert_eq!(operation.operation_offset, 31);
    assert!(operation.keep_tools);
    assert_eq!(operation.keep_tools_offset, 30);
    assert_eq!(operation.target_record_index, 94);
    assert!(operation.tools.additional.is_empty());
    let tool = &operation.tools.first;
    let identity = tool
        .external_identity
        .as_ref()
        .expect("cross-document body identity");
    assert_eq!(identity.occurrence_reference(), 5_001);
    assert_eq!(identity.external_body_reference(), 6_001);
    assert_eq!(identity.external_segment(), 7);
    assert_eq!(identity.external_link_name(), "component-body-link");
    assert_eq!(
        identity
            .external_version()
            .map(|version| version.property_key.value.as_str()),
        Some("33333333-3333-4333-8333-333333333333")
    );
    assert_eq!(
        identity
            .external_version()
            .map(|version| version.version_urn.value.as_str()),
        Some("urn:example:version:4")
    );
    assert_eq!(identity.tail_values(), [11, 12]);
    assert_eq!(
        identity.tail_value_offsets()[1],
        identity.tail_value_offsets()[0] + 12
    );

    let mut malformed_scope = scope.clone();
    malformed_scope
        .try_edit(|draft| {
            draft.frame_length = 367;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert!(exact_combine_operation(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        &records,
        &malformed_scope
    )
    .unwrap()
    .is_none());
    let mut malformed_reference = bytes.clone();
    malformed_reference[35] = 0;
    assert!(exact_combine_operation(
        &cadmpeg_test_support::service_decode_context(),
        &malformed_reference,
        &crate::design::test_support::indexed_record_offsets_for_test(&malformed_reference),
        &scope,
    )
    .unwrap()
    .is_none());

    let mut malformed_external_asset = bytes;
    malformed_external_asset[usize::try_from(identity.external_asset_id_offset()).unwrap()] = b'g';
    let operation = exact_combine_operation(
        &cadmpeg_test_support::service_decode_context(),
        &malformed_external_asset,
        &crate::design::test_support::indexed_record_offsets_for_test(&malformed_external_asset),
        &scope,
    )
    .unwrap()
    .expect("operation remains exact when only the optional external identity is malformed");
    assert!(operation.tools.first.external_identity.is_none());
}
