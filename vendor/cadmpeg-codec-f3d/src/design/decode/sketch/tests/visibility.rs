// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::sketch::decode_sketch_visibility_member;
use crate::design::decode::sketch::CURRENT_SKETCH_CONTAINER_VERSION;
use crate::design::decode::sketch::SKETCH_CONTAINER_MEMBER_BASE_TYPE_GUID;
use crate::design::decode::sketch::SKETCH_CONTAINER_MEMBER_TYPE_GUID;
use crate::design::decode::sketch::SKETCH_CONTAINER_MEMBER_VERSION;
use crate::design::decode::sketch::SKETCH_CONTAINER_TYPE_GUID;
use crate::records::entity_header::DESIGN_MODULE_SKETCH;

const ENTITY_SUFFIX: u64 = 201;

fn decode_sketch_visibilities_in_stream(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
) -> Result<
    Vec<(
        u64,
        crate::records::sketch_placement::DesignSketchVisibility,
    )>,
    cadmpeg_core::CodecError,
> {
    crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::sketch::decode_sketch_visibilities_in_stream(ctx, bytes, meta)
    })
}

fn member(stream_ordinal: u32, visible: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"256");
    bytes.extend_from_slice(&ENTITY_SUFFIX.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.push(1);
    bytes.extend_from_slice(&203u64.to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&stream_ordinal.to_le_bytes());
    bytes.push(0);
    bytes.push(visible);
    bytes.push(1);
    bytes
}

#[test]
fn sketch_visibility_member_decodes_both_boolean_values() {
    let hidden =
        decode_sketch_visibility_member(&member(1, 0), 0, ENTITY_SUFFIX).expect("hidden member");
    assert_eq!(hidden.stream_ordinal.get(), 1);
    assert_eq!(hidden.stream_ordinal_offset(), 30);
    assert_eq!(hidden.visible_offset(), 35);
    assert!(!hidden.visible);

    let visible =
        decode_sketch_visibility_member(&member(513, 1), 0, ENTITY_SUFFIX).expect("visible member");
    assert_eq!(visible.stream_ordinal.get(), 513);
    assert!(visible.visible);
}

#[test]
fn sketch_visibility_member_rejects_invalid_ordinal_or_owner() {
    assert!(decode_sketch_visibility_member(&member(1, 2), 0, ENTITY_SUFFIX).is_none());
    assert!(decode_sketch_visibility_member(&member(0, 1), 0, ENTITY_SUFFIX).is_none());
    assert!(decode_sketch_visibility_member(&member(1, 1), 0, ENTITY_SUFFIX + 1).is_none());

    let mut external_owner = member(1, 1);
    external_owner[28] = 1;
    assert!(decode_sketch_visibility_member(&external_owner, 0, ENTITY_SUFFIX).is_none());
}

fn visibility_stream() -> (Vec<u8>, crate::metastream::MetaStream) {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"256");
    bytes.extend_from_slice(&ENTITY_SUFFIX.to_le_bytes());
    bytes.extend_from_slice(&[0; 5]);
    bytes.push(0);
    bytes.extend_from_slice(&5u32.to_le_bytes());
    for unit in "0_201".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    let mut visibility = member(1, 1);
    visibility[4..7].copy_from_slice(b"257");
    bytes.extend_from_slice(&visibility);

    let segment_type = |type_guid: &str,
                        base_type_guid: Option<&str>,
                        version: u32,
                        module: &str,
                        entity_ids: Vec<u64>| {
        crate::records::entity_header::SegmentTypeData {
            byte_offset: 0,
            type_guid: type_guid.to_owned().try_into().expect("type GUID"),
            type_guid_offset: 0,
            base_type_guid: base_type_guid.map_or(
                crate::records::entity_header::BaseTypeGuid::Absent,
                |value| crate::records::entity_header::BaseTypeGuid::Guid {
                    value: value.to_owned().try_into().expect("base GUID"),
                    offset: 0,
                },
            ),
            version,
            version_offset: 0,
            module: module.into(),
            entities: crate::records::identity::ReferenceRun::located(
                entity_ids
                    .into_iter()
                    .map(|value| crate::records::identity::Located { value, offset: 0 })
                    .collect(),
            ),
        }
    };
    let metadata = crate::metastream::MetaStream {
        types: vec![
            segment_type(
                SKETCH_CONTAINER_TYPE_GUID,
                Some(SKETCH_CONTAINER_MEMBER_TYPE_GUID),
                CURRENT_SKETCH_CONTAINER_VERSION,
                DESIGN_MODULE_SKETCH,
                vec![ENTITY_SUFFIX],
            ),
            segment_type(
                SKETCH_CONTAINER_MEMBER_TYPE_GUID,
                Some(SKETCH_CONTAINER_MEMBER_BASE_TYPE_GUID),
                SKETCH_CONTAINER_MEMBER_VERSION,
                "Geometry",
                Vec::new(),
            ),
        ],
        records: vec![crate::metastream::RecordIndexEntry {
            entity_id: ENTITY_SUFFIX,
            bulk_offset: 0,
        }],
        secondary_records: Vec::new(),
    };

    (bytes, metadata)
}

#[test]
fn sketch_visibility_accepts_settled_container_header() {
    let (bytes, metadata) = visibility_stream();

    let visibilities =
        decode_sketch_visibilities_in_stream(&bytes, &metadata).expect("settled header");
    assert_eq!(visibilities.len(), 1);
    assert_eq!(visibilities[0].0, ENTITY_SUFFIX);
    assert!(visibilities[0].1.visible);
}

#[test]
fn sketch_visibility_output_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let (bytes, metadata) = visibility_stream();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 7;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = crate::design::decode::sketch::decode_sketch_visibilities_in_stream(
        &ctx, &bytes, &metadata,
    )
    .expect_err("collection limit must refuse decoded visibility");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(failure)
        if failure.dimension == ResourceDimension::CollectionItems
            && failure.operation == "f3d sketch visibility records")
    );
}
