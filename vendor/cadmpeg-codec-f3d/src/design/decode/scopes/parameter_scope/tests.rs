// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::u64_from_index;

use super::named_parameter_scope_tail_is_valid;
use super::{copy_sketch_entity_id, first_marked_reference_offsets};
use crate::test_support::lp_utf16;

fn named_scope_tail(lane_value: u64) -> Vec<u8> {
    let label = "Canvas";
    let label_code_units = label.encode_utf16().count();
    let marker = 19 + label_code_units * 2;
    let mut bytes = vec![0; marker + 59];
    bytes[0..4].copy_from_slice(&1u32.to_le_bytes());
    let mut label_bytes = Vec::new();
    lp_utf16(&mut label_bytes, label);
    bytes[8..8 + label_bytes.len()].copy_from_slice(&label_bytes);
    bytes[marker] = 1;
    bytes[marker + 1] = 0xd5;
    bytes[marker + 2..marker + 10].copy_from_slice(&lane_value.to_le_bytes());
    bytes[marker + 12..marker + 16].copy_from_slice(&u32::MAX.to_le_bytes());
    bytes[marker + 16..marker + 20].copy_from_slice(&0xfcu32.to_le_bytes());
    bytes[marker + 20..marker + 28].copy_from_slice(&0.25f64.to_le_bytes());
    bytes[marker + 28..marker + 32].copy_from_slice(&0xfcu32.to_le_bytes());
    bytes[marker + 32] = 1;
    bytes[marker + 33] = 0xd4;
    bytes[marker + 34..marker + 42].copy_from_slice(&lane_value.to_le_bytes());
    bytes[marker + 42..marker + 46].copy_from_slice(&[0, 1, 0, 0]);
    bytes[marker + 46] = 1;
    bytes[marker + 47] = 0xd3;
    bytes[marker + 48..marker + 56].copy_from_slice(&lane_value.to_le_bytes());
    bytes
}

#[test]
fn named_scope_tail_requires_one_repeated_binary_lane_value() {
    for lane_value in [0, 1] {
        let bytes = named_scope_tail(lane_value);
        assert_eq!(
            named_parameter_scope_tail_is_valid(
                &cadmpeg_test_support::service_decode_context(),
                &bytes,
                0,
                bytes.len(),
                bytes.len()
            )
            .unwrap(),
            Some(true)
        );
    }

    let mut mismatched = named_scope_tail(0);
    let marker = mismatched.len() - 59;
    mismatched[marker + 34..marker + 42].copy_from_slice(&1u64.to_le_bytes());
    assert_eq!(
        named_parameter_scope_tail_is_valid(
            &cadmpeg_test_support::service_decode_context(),
            &mismatched,
            0,
            mismatched.len(),
            mismatched.len()
        )
        .unwrap(),
        Some(false)
    );

    let outside_domain = named_scope_tail(2);
    assert_eq!(
        named_parameter_scope_tail_is_valid(
            &cadmpeg_test_support::service_decode_context(),
            &outside_domain,
            0,
            outside_domain.len(),
            outside_domain.len()
        )
        .unwrap(),
        Some(false)
    );
}

#[test]
fn named_scope_tail_refuses_temporary_text_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let bytes = named_scope_tail(0);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_materialized_bytes = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = named_parameter_scope_tail_is_valid(&ctx, &bytes, 0, bytes.len(), bytes.len());
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::MaterializedBytes
                && failure.operation == "f3d Design temporary UTF-16 text"
    ));
}

#[test]
fn sketch_scope_reference_offsets_refuse_second_unique_marker() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut frame = vec![0; 24];
    for (at, suffix) in [(0, 42u32), (12, 43)] {
        frame[at] = 1;
        frame[at + 1..at + 5].copy_from_slice(&suffix.to_le_bytes());
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = first_marked_reference_offsets(&ctx, &frame);
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d Sketch scope reference offsets"
    ));
    let admitted =
        first_marked_reference_offsets(&cadmpeg_test_support::service_decode_context(), &frame)
            .unwrap();
    assert_eq!(admitted.get(&42), Some(&0));
    assert_eq!(admitted.get(&43), Some(&12));
}

#[test]
fn sketch_scope_entity_id_copy_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let source =
        crate::records::identity::DesignEntityId::try_from("entity_42".to_owned()).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = u64_from_index(source.as_str().len()) - 1;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = copy_sketch_entity_id(&ctx, &source);
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::RetainedBytes
                && failure.operation == "f3d Sketch scope entity ID"
    ));
    let copied =
        copy_sketch_entity_id(&cadmpeg_test_support::service_decode_context(), &source).unwrap();
    assert_eq!(copied, source);
}
