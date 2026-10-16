// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::sketch::{
    decode_persistent_references_from_stream, finish_persistent_references,
};
use crate::records::references::PersistentReference;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

fn reference_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    for (name, value) in [("crv_primary_id", 71u64), ("pt_tag", 29)] {
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(&23u32.to_le_bytes());
        bytes.extend_from_slice(b"IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn scan_references(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<PersistentReference>, cadmpeg_core::CodecError> {
    let mut indexed = Vec::new();
    decode_persistent_references_from_stream(ctx, 0, "BulkStream.dat", bytes, &mut indexed)?;
    finish_persistent_references(ctx, indexed)
}

#[test]
fn persistent_reference_collections_refuse_collection_limit() {
    let bytes = reference_bytes();
    for (limit, operation) in [
        (0, "f3d persistent reference index"),
        (1, "f3d persistent reference index"),
        (2, "f3d persistent reference output"),
        (3, "f3d persistent reference output"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = scan_references(&ctx, &bytes)
            .expect_err("collection limit must refuse persistent references");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(failure)
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == operation)
        );
    }
}

#[test]
fn persistent_reference_id_refuses_retained_limit() {
    let bytes = reference_bytes();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "f3d persistent reference ID",
        |cap| {
            let mut policy = DecodePolicy::default();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            scan_references(&ctx, &bytes).map(|_| ())
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = scan_references(&ctx, &bytes)
        .expect_err("retained limit must refuse persistent reference ID");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(failure)
        if failure.dimension == ResourceDimension::RetainedBytes
            && failure.operation == "f3d persistent reference ID")
    );
}

#[test]
fn persistent_reference_scan_preserves_byte_order_after_kind_scan() {
    let bytes = reference_bytes();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let references = scan_references(&ctx, &bytes).expect("two typed references");
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].value, 71);
    assert_eq!(references[1].value, 29);
    assert_eq!(
        references[0].id,
        crate::ids::native_persistent_reference_id("BulkStream.dat", 0)
    );
}
