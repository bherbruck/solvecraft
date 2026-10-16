// SPDX-License-Identifier: Apache-2.0
use super::super::rescope_fidelity;
use cadmpeg_ir::annotations::{AnnotationBuilder, StreamHandle};
use cadmpeg_ir::source_fidelity::{RetainedSourceRecord, SourceFidelity};

const ID: &str = "f3d:native:record#one";

fn source(stream: &str) -> SourceFidelity {
    let mut annotations = AnnotationBuilder::new();
    let handle = StreamHandle::new(
        &cadmpeg_test_support::service_decode_context(),
        cadmpeg_ir::StreamName::try_from(stream.to_owned()).unwrap(),
        "fixture stream handle",
    )
    .unwrap();
    annotations
        .note(
            &cadmpeg_test_support::service_decode_context(),
            ID,
            &handle,
            7,
            Some("retained"),
        )
        .unwrap();
    annotations
        .derived(
            &cadmpeg_test_support::service_decode_context(),
            ID,
            "geometry",
        )
        .unwrap();
    let mut fidelity = SourceFidelity::with_annotations(annotations.build());
    fidelity
        .insert_retained_record(
            cadmpeg_ir::ids::UnknownId::mint(ID).unwrap(),
            RetainedSourceRecord::from_bytes(
                stream,
                7,
                cadmpeg_ir::source_fidelity::RetainedBytes::Inline {
                    data: vec![1, 2, 3],
                },
            )
            .unwrap(),
        )
        .unwrap();
    fidelity
}

#[test]
fn source_rescoping_preserves_bytes_extent_tag_and_exactness() {
    let input = source("member");
    let original = input.retained_record(ID).unwrap().clone();
    let output = rescope_fidelity(
        &cadmpeg_test_support::service_decode_context(),
        input.clone(),
        "part/occurrence-0",
    )
    .unwrap();
    let id = "f3d:xref/part/occurrence-0/native:record#one";
    let retained = output.retained_record(id).unwrap();
    assert_eq!(retained.data(), original.data());
    assert_eq!(retained.offset(), 7);
    assert_eq!(retained.end_offset(), 10);
    assert_eq!(retained.sha256(), original.sha256());
    assert_eq!(retained.stream(), "f3d:xref/part%2Foccurrence-0/member");
    let provenance = &output.annotations.provenance[id];
    assert_eq!(provenance.stream(), retained.stream());
    assert_eq!(provenance.offset, 7);
    assert_eq!(provenance.tag.as_deref(), Some("retained"));
    assert_eq!(
        output.annotations.exactness()[id],
        input.annotations.exactness()[ID]
    );
}

#[test]
fn occurrence_and_stream_boundaries_cannot_alias() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let first = rescope_fidelity(&ctx, source("b/c"), "a").unwrap();
    let second = rescope_fidelity(&ctx, source("c"), "a/b").unwrap();
    let escaped = rescope_fidelity(&ctx, source("c"), "a%2Fb").unwrap();
    let owner = |fidelity: &SourceFidelity| {
        fidelity
            .retained_records()
            .values()
            .next()
            .unwrap()
            .stream()
            .to_owned()
    };
    assert_eq!(owner(&first), "f3d:xref/a/b/c");
    assert_eq!(owner(&second), "f3d:xref/a%2Fb/c");
    assert_eq!(owner(&escaped), "f3d:xref/a%252Fb/c");
}

#[test]
fn source_rescoping_refuses_annotation_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = rescope_fidelity(&ctx, source("member"), "part").unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index remapped annotation identities")
    );
}

#[test]
fn source_rescoping_refuses_identity_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = rescope_fidelity(&ctx, source("member"), "part").unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "rescope F3Z identity")
    );
}

#[test]
fn source_rescoping_preserves_absent_provenance_tag() {
    let mut builder = AnnotationBuilder::new();
    let stream = StreamHandle::new(
        &cadmpeg_test_support::service_decode_context(),
        cadmpeg_ir::StreamName::try_from("member".to_owned()).unwrap(),
        "fixture stream handle",
    )
    .unwrap();
    builder
        .note(
            &cadmpeg_test_support::service_decode_context(),
            ID,
            &stream,
            7,
            None,
        )
        .unwrap();
    let output = rescope_fidelity(
        &cadmpeg_test_support::service_decode_context(),
        SourceFidelity::with_annotations(builder.build()),
        "part",
    )
    .unwrap();
    assert_eq!(
        output.annotations.provenance["f3d:xref/part/native:record#one"].tag,
        None
    );
}

#[test]
fn source_rescoping_refuses_fidelity_owner_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = rescope_fidelity(&ctx, SourceFidelity::default(), "part").unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3Z fidelity owner")
    );
}

#[test]
fn source_rescoping_refuses_retained_record_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let (_, records) = source("member").into_parts();
    let mut input = SourceFidelity::default();
    for (id, record) in records {
        input.insert_retained_record(id, record).unwrap();
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = rescope_fidelity(&ctx, input, "part").unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3Z rescoped retained records")
    );
}

#[test]
fn source_rescoping_refuses_provenance_stream_handle_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 5;
    for _ in 0..128 {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = rescope_fidelity(&ctx, source("member"), "part").unwrap_err();
        let cadmpeg_core::CodecError::ResourceLimit(first) = error else {
            panic!("stream-handle storage must refuse");
        };
        assert_eq!(first.dimension, ResourceDimension::CollectionItems);
        assert_eq!(first.limit, policy.limits.max_collection_items);
        assert!(matches!(ctx.finish_session(),
            Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == first));
        if first.operation == "allocate annotation stream handle" {
            return;
        }
        let next = first.used.checked_add(first.additional).unwrap();
        assert!(next > policy.limits.max_collection_items);
        policy.limits.max_collection_items = next;
    }
    panic!("stream-handle admission must be reached");
}

#[test]
fn fidelity_append_charged_preserves_source_metadata() {
    let mut charged = SourceFidelity::default();
    charged
        .append(
            &cadmpeg_test_support::service_decode_context(),
            source("member"),
        )
        .unwrap();
    let mut plain = SourceFidelity::default();
    plain
        .append(
            &cadmpeg_test_support::service_decode_context(),
            source("member"),
        )
        .unwrap();
    assert_eq!(charged, plain);
}

#[test]
fn fidelity_append_refuses_provenance_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut builder = AnnotationBuilder::new();
    let stream = StreamHandle::new(
        &cadmpeg_test_support::service_decode_context(),
        cadmpeg_ir::StreamName::try_from("member".to_owned()).unwrap(),
        "fixture stream handle",
    )
    .unwrap();
    builder
        .note(
            &cadmpeg_test_support::service_decode_context(),
            ID,
            &stream,
            7,
            None,
        )
        .unwrap();
    let other = SourceFidelity::with_annotations(builder.build());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let expected = other.clone();
    let mut target = SourceFidelity::default();
    target.append(&ctx, other).unwrap();
    assert_eq!(target, expected);
}

#[test]
fn fidelity_append_refuses_retained_record_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let (_, records) = source("member").into_parts();
    let mut other = SourceFidelity::default();
    for (id, record) in records {
        other.insert_retained_record(id, record).unwrap();
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let expected = other.clone();
    let mut target = SourceFidelity::default();
    target.append(&ctx, other).unwrap();
    assert_eq!(target, expected);
}

#[test]
fn fidelity_append_refuses_new_destination_nodes_without_mutation() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    for records in [false, true] {
        let mut target = rescope_fidelity(
            &cadmpeg_test_support::service_decode_context(),
            source("left"),
            "left",
        )
        .unwrap();
        let mut incoming = source("right");
        if !records {
            target = SourceFidelity::with_annotations(target.annotations);
            incoming = SourceFidelity::with_annotations(incoming.annotations);
        }
        let before = target.clone();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = target.append(&ctx, incoming).unwrap_err();
        let operation = if records {
            "append source records"
        } else {
            "append source provenance"
        };
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation)
        );
        assert_eq!(target, before);
    }
}
