// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::operands::parse_work_point_sketch_point_frame;
use crate::test_support::indexed_header;
use crate::test_support::lp_utf16;

#[test]
fn work_point_input_copy_refuses_collection_limit() {
    let input =
        crate::records::feature::work_geometry::DesignWorkPointInput::try_new(7, 14, None).unwrap();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        &ctx.try_collect_retained_with(std::slice::from_ref(&input), "f3d WorkPoint input copy", |input| input.try_clone_for_decode(&ctx, "f3d WorkPoint input copy")),
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && failure.operation == "f3d WorkPoint input copy"
    ));
}

#[test]
fn work_point_input_copy_charges_box_and_carrier_text() {
    use crate::records::feature::work_geometry::{
        DesignWorkPointInput, DesignWorkPointInputCarrier,
    };
    let input = DesignWorkPointInput::try_new(
        7,
        14,
        Some(Box::new(DesignWorkPointInputCarrier::EdgeRecipe {
            operand_id: "edge".into(),
        })),
    )
    .unwrap();
    let bytes = u64::try_from(std::mem::size_of::<DesignWorkPointInputCarrier>()).unwrap() + 4;
    for retained in [bytes - 1, bytes] {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = retained;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let copy = input.try_clone_for_decode(&ctx, "work point owned carrier copy");
        if retained == bytes {
            assert_eq!(copy.unwrap(), input);
        } else {
            assert!(
                matches!(copy, Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes && limit.operation == "work point owned carrier copy")
            );
        }
    }
}

#[test]
fn direct_sketch_point_selection_reads_owner_and_persistent_ids() {
    let record_index = 100;
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"338", record_index);
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&(record_index + 3).to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "7b3dec6f-f69c-4bfa-a537-9274f341c66e");
    lp_utf16(&mut bytes, "2b40eee7-408c-429b-9216-8b6f7e9a62c9");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    indexed_header(&mut bytes, *b"258", record_index);
    indexed_header(&mut bytes, *b"294", record_index + 1);
    indexed_header(&mut bytes, *b"303", record_index + 2);
    indexed_header(&mut bytes, *b"305", record_index + 3);
    bytes.extend_from_slice(&[0; 9]);
    bytes.push(1);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&1627u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&379u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    indexed_header(&mut bytes, *b"288", record_index + 4);

    for limit in [35, 71] {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = limit;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            parse_work_point_sketch_point_frame(&ctx, &bytes, record_index, 0),
            Some(Err(cadmpeg_core::CodecError::ResourceLimit(failure)))
                if failure.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                    && failure.operation == "f3d Design UTF-16 text"
        ));
    }
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let selection = parse_work_point_sketch_point_frame(&ctx, &bytes, record_index, 0)
        .expect("direct sketch-point selection")
        .unwrap();
    assert_eq!(selection.sketch_record_index, 1627);
    assert_eq!(selection.point_persistent_id, 379);
    assert_eq!(selection.identity_record_index, record_index + 3);
    assert_eq!(selection.next_record_index, record_index + 4);
    assert_eq!(selection.identity_record_offset, 229);
    assert_eq!(selection.sketch_record_index_offset, 254);
    assert_eq!(selection.point_persistent_id_offset, 262);
    assert_eq!(selection.next_byte_offset, 270);
}
