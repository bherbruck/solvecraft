// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::operands::{
    face_source_carrier_layout, face_source_reference_headers, parse_face_source_carrier_prefix,
};
use crate::test_support::write_marked_reference;

#[test]
fn face_source_reference_headers_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    let (service, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let records =
        crate::design::decode::sketch::IndexedRecordOffsets::build(&service, &[]).unwrap();
    policy.limits.max_collection_items = 1;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        face_source_reference_headers(&ctx, &[], 0, [7u32, 8].iter(), &records),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "f3d face source reference headers"
    ));
}

#[test]
fn face_source_output_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let group = crate::records::topology::face::DesignFaceSourceGroup {
        id: "f3d:design:design-face-source-group#0".to_owned(),
        scope_record_index: 12,
        carrier_reference_ordinal: 0,
        carrier_record_index: 7,
        carrier_span: crate::records::identity::NonEmptyByteSpan::new(0, 11).unwrap(),
        carrier_class_tag: crate::records::references::DesignClassTag::try_from("398".to_owned())
            .unwrap(),
        paired_record_index: 7,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("462".to_owned())
            .unwrap(),
        source_members: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut out = Vec::new();
    assert!(matches!(
        ctx.push_vec(&mut out, group, "f3d face source group output"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "f3d face source group output"
    ));
    assert!(out.is_empty());
}

fn indexed_header(bytes: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32) {
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(class_tag);
    bytes.extend_from_slice(&record_index.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
}

fn source_carrier(
    class_tag: &[u8; 3],
    record_index: u32,
    scope_record_index: u32,
    source_count: usize,
    scalar_offset: usize,
    discriminator: u32,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, class_tag, record_index);
    bytes.resize(scalar_offset + 16, 0);
    write_marked_reference(&mut bytes, 21, scope_record_index);
    bytes[32..36].copy_from_slice(
        &(u32::try_from(source_count).expect("fixture value fits u32")).to_le_bytes(),
    );
    for ordinal in 0..source_count {
        let offset = 36 + ordinal * 11;
        write_marked_reference(
            &mut bytes,
            offset,
            200 + u32::try_from(ordinal).expect("fixture value fits u32"),
        );
    }
    bytes[scalar_offset..scalar_offset + 4].copy_from_slice(&discriminator.to_le_bytes());
    bytes[scalar_offset + 4..scalar_offset + 12].copy_from_slice(&0.125f64.to_le_bytes());
    bytes[scalar_offset + 12..scalar_offset + 16].copy_from_slice(&discriminator.to_le_bytes());
    bytes
}

#[test]
fn face_source_carriers_use_generation_keyed_prefixes() {
    for (class_tag, source_count, scalar_offset, discriminator, paired_class_tag) in [
        (*b"398", 4, 80, 100, "462"),
        (*b"394", 2, 58, 109, "311"),
        (*b"356", 2, 58, 109, "309"),
    ] {
        let layout = face_source_carrier_layout(class_tag_str(&class_tag)).unwrap();
        assert_eq!(layout.source_count, source_count);
        assert_eq!(layout.scalar_offset, scalar_offset);
        assert_eq!(layout.scalar_discriminator, discriminator);
        assert_eq!(layout.paired_class_tag, paired_class_tag);

        let bytes = source_carrier(
            &class_tag,
            100,
            12,
            source_count,
            scalar_offset,
            discriminator,
        );
        let references = crate::test_support::with_decode_context(|ctx| {
            parse_face_source_carrier_prefix(ctx, &bytes, 0, 12, layout).expect("service admission")
        })
        .unwrap();
        assert_eq!(
            references,
            (0..source_count)
                .map(|ordinal| (
                    36 + ordinal * 11,
                    200 + u32::try_from(ordinal).expect("fixture value fits u32")
                ))
                .collect::<Vec<_>>()
        );
    }
}

fn class_tag_str(class_tag: &[u8; 3]) -> &str {
    std::str::from_utf8(class_tag).unwrap()
}

#[test]
fn face_source_carrier_prefix_rejects_wrong_count_and_discriminator() {
    let layout = face_source_carrier_layout("398").unwrap();
    let mut bytes = source_carrier(b"398", 100, 12, 4, 80, 100);

    bytes[32..36].copy_from_slice(&3u32.to_le_bytes());
    assert!(
        crate::test_support::with_decode_context(|ctx| parse_face_source_carrier_prefix(
            ctx, &bytes, 0, 12, layout
        )
        .expect("service admission"))
        .is_none()
    );

    let layout = face_source_carrier_layout("398").unwrap();
    let mut bytes = source_carrier(b"398", 100, 12, 4, 80, 100);
    bytes[80..84].copy_from_slice(&101u32.to_le_bytes());
    assert!(
        crate::test_support::with_decode_context(|ctx| parse_face_source_carrier_prefix(
            ctx, &bytes, 0, 12, layout
        )
        .expect("service admission"))
        .is_none()
    );
}

#[test]
fn face_source_reference_storage_has_fixed_capacity() {
    for (tag, count, offset, discriminator) in [(b"398", 4, 80, 100), (b"394", 2, 58, 109)] {
        let layout = face_source_carrier_layout(class_tag_str(tag)).unwrap();
        let bytes = source_carrier(tag, 100, 12, count, offset, discriminator);
        let references = crate::test_support::with_decode_context(|ctx| {
            parse_face_source_carrier_prefix(ctx, &bytes, 0, 12, layout).expect("service admission")
        })
        .unwrap();
        assert_eq!(references.len(), count);
        assert_eq!(references.capacity(), 4);
    }
}
