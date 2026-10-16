// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]
use cadmpeg_test_support::EditableDecodeResult;

use super::{resolve_uri, root_reference_notes, ReferenceTarget, ROOT_NAME};

fn resolve_uri_for_test<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    base_member: &'a str,
    uri: &'a str,
) -> Result<ReferenceTarget<'a>, cadmpeg_core::CodecError> {
    let mut member_bytes = ctx.reserve_scoped(0, "step_zip_uri_member_temp")?;
    resolve_uri(ctx, &mut member_bytes, base_member, uri)
}

#[test]
fn resolves_archive_relative_uris_and_fragments() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("test context");
    assert_eq!(
        resolve_uri_for_test(&ctx, ROOT_NAME, "parts/child.p21#target").unwrap(),
        ReferenceTarget::Internal {
            member: "parts/child.p21".into(),
            query: None,
            fragment: Some("target"),
        }
    );
    assert_eq!(
        resolve_uri_for_test(&ctx, "parts/child.p21", "../shared.p21#value").unwrap(),
        ReferenceTarget::Internal {
            member: "shared.p21".into(),
            query: None,
            fragment: Some("value"),
        }
    );
    assert_eq!(
        resolve_uri_for_test(&ctx, ROOT_NAME, "https://example.invalid/part.p21#root").unwrap(),
        ReferenceTarget::External
    );
    assert_eq!(
        resolve_uri_for_test(&ctx, "parts/sub/child.p21", "./../shared.p21#value").unwrap(),
        ReferenceTarget::Internal {
            member: "parts/shared.p21".into(),
            query: None,
            fragment: Some("value"),
        }
    );
    assert_eq!(
        resolve_uri_for_test(&ctx, ROOT_NAME, "parts/child.p21?query=../outside#target").unwrap(),
        ReferenceTarget::Internal {
            member: "parts/child.p21".into(),
            query: Some("query=../outside"),
            fragment: Some("target"),
        }
    );
}

#[test]
fn rejects_archive_relative_traversal() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("test context");
    assert!(resolve_uri_for_test(&ctx, ROOT_NAME, "../outside.p21").is_err());
    assert!(resolve_uri_for_test(&ctx, ROOT_NAME, "parts//child.p21").is_err());
}

#[test]
fn uri_components_refuse_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("test context");
    let error = resolve_uri_for_test(&ctx, ROOT_NAME, "parts/child.p21")
        .expect_err("two URI components exceed one item");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && limit.operation == "step_zip_uri_components"
    ));
}

#[test]
fn uri_member_refuses_temporary_byte_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes =
        u64::try_from(4 * std::mem::size_of::<&str>()).expect("pointer size fits u64");
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("test context");
    let error = resolve_uri_for_test(&ctx, ROOT_NAME, "a")
        .expect_err("one URI component leaves no temporary byte for its member");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
                && limit.operation == "step_zip_uri_member"
    ));
}

#[test]
fn uri_base_member_refuses_temporary_byte_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes =
        u64::try_from(ROOT_NAME.len() - 1).expect("member and component slots fit u64");
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("test context");
    let error = resolve_uri_for_test(&ctx, ROOT_NAME, "#target")
        .expect_err("base member needs one more temporary byte");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
                && limit.operation == "step_zip_uri_member"
    ));
}

#[test]
fn root_reference_note_refuses_retained_byte_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes =
        4 + cadmpeg_core::decode::u64_from_index(4 * std::mem::size_of::<String>());
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("test context");
    let mut notes = Vec::new();
    let error = super::push_reference_note(
        &ctx,
        &mut notes,
        "internal resource ",
        crate::parse::ReferenceName::Entity(1),
        "part.p21",
        None,
        None,
    )
    .expect_err("the note requires more than four retained bytes");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "step_zip_reference_note"
    ));
}

use std::io::{Cursor, Read as _};

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::StepCodec;

fn step_zip(entries: &[(&str, &[u8], CompressionMethod)]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for &(name, bytes, method) in entries {
        writer
            .start_file(
                name,
                SimpleFileOptions::default().compression_method(method),
            )
            .unwrap();
        std::io::Write::write_all(&mut writer, bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn step_zip_with_comment(entries: &[(&str, &[u8], CompressionMethod)], comment: &str) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for &(name, bytes, method) in entries {
        writer
            .start_file(
                name,
                SimpleFileOptions::default().compression_method(method),
            )
            .unwrap();
        std::io::Write::write_all(&mut writer, bytes).unwrap();
    }
    writer.set_comment(comment).unwrap();
    writer.finish().unwrap().into_inner()
}

fn duplicate_first_central_record(mut bytes: Vec<u8>) -> Vec<u8> {
    let end = bytes
        .windows(4)
        .rposition(|signature| signature == b"PK\x05\x06")
        .expect("ZIP end record");
    let central_start = cadmpeg_core::decode::index_from_u32(u32::from_le_bytes(
        bytes[end + 16..end + 20].try_into().unwrap(),
    ));
    let name_len = usize::from(u16::from_le_bytes(
        bytes[central_start + 28..central_start + 30]
            .try_into()
            .unwrap(),
    ));
    let extra_len = usize::from(u16::from_le_bytes(
        bytes[central_start + 30..central_start + 32]
            .try_into()
            .unwrap(),
    ));
    let comment_len = usize::from(u16::from_le_bytes(
        bytes[central_start + 32..central_start + 34]
            .try_into()
            .unwrap(),
    ));
    let record_len = 46 + name_len + extra_len + comment_len;
    let record = bytes[central_start..central_start + record_len].to_vec();
    let central_end = end;
    bytes.splice(central_end..central_end, record.iter().copied());
    let new_end = end + record_len;
    let count = u16::from_le_bytes(bytes[new_end + 10..new_end + 12].try_into().unwrap());
    bytes[new_end + 8..new_end + 10].copy_from_slice(&(count + 1).to_le_bytes());
    bytes[new_end + 10..new_end + 12].copy_from_slice(&(count + 1).to_le_bytes());
    let size = u32::from_le_bytes(bytes[new_end + 12..new_end + 16].try_into().unwrap());
    bytes[new_end + 12..new_end + 16].copy_from_slice(
        &(size + u32::try_from(record_len).expect("test record length fits u32")).to_le_bytes(),
    );
    bytes
}

fn mark_entries_encrypted(mut bytes: Vec<u8>) -> Vec<u8> {
    let locations = {
        let mut archive = ZipArchive::new(Cursor::new(&bytes)).unwrap();
        (0..archive.len())
            .map(|index| {
                let file = archive.by_index(index).unwrap();
                (
                    usize::try_from(file.header_start()).expect("test header offset fits memory"),
                    usize::try_from(file.central_header_start())
                        .expect("test central offset fits memory"),
                )
            })
            .collect::<Vec<_>>()
    };
    for (local, central) in locations {
        let local_flags = u16::from_le_bytes(bytes[local + 6..local + 8].try_into().unwrap()) | 1;
        bytes[local + 6..local + 8].copy_from_slice(&local_flags.to_le_bytes());
        let central_flags =
            u16::from_le_bytes(bytes[central + 8..central + 10].try_into().unwrap()) | 1;
        bytes[central + 8..central + 10].copy_from_slice(&central_flags.to_le_bytes());
    }
    bytes
}

fn corrupt_first_payload(mut bytes: Vec<u8>) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let data_start = usize::try_from(archive.by_index(0).unwrap().data_start().unwrap())
        .expect("test data offset fits memory");
    bytes[data_start] ^= 1;
    bytes
}

#[test]
pub(crate) fn codec_detects_and_inspects_ap242_exchange_structure() {
    let bytes = include_bytes!("../../tests/fixtures/ap242_minimal.p21");
    let codec = StepCodec::default();

    assert_eq!(
        cadmpeg_test_support::detection::confidence(&codec, bytes),
        Confidence::High
    );
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&codec, b"PK\x03\x04"),
        Confidence::Low
    );

    let summary = codec
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("inspect minimal AP242");
    assert_eq!(summary.format(), "step");
    assert_eq!(summary.container_kind, "iso-10303-21-clear-text");
    assert_eq!(summary.entries.len(), 2);
    assert_eq!(summary.entries[0].name, "HEADER");
    assert_eq!(summary.entries[1].name, "DATA[0]");
    assert_eq!(summary.entries[1].attributes["entity_count"], "2");
    assert_eq!(
        summary.entries[1].attributes["unknown_entities"],
        "EXAMPLE_RECORD:1,OPAQUE_TARGET:1"
    );
    assert!(summary
        .notes
        .iter()
        .any(|note| note.contains("AP242") && note.contains("dialect step:ap242-e2")));
}

#[test]
fn codec_detection_matches_part21_trivia_and_keyword_rules() {
    let source = b"/* preamble */\n  iso-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;ENDSEC;END-ISO-10303-21;";
    let codec = StepCodec::default();

    assert_eq!(
        cadmpeg_test_support::detection::confidence(&codec, source),
        Confidence::High
    );
    codec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("Part 21 leading trivia and case-insensitive magic");
}

#[test]
fn zip_root_detection_uses_structured_entry_names() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let root = include_bytes!("../../tests/fixtures/ap242_minimal.p21");
    let marker_payload = b"payload contains ISO-10303.p21 but has no such entry";
    let codec = StepCodec::default();

    let root_after_payload = step_zip(&[
        ("preview.bin", marker_payload, CompressionMethod::Stored),
        (ROOT_NAME, root, CompressionMethod::Stored),
    ]);
    assert!(super::has_root_marker(
        &ctx,
        cadmpeg_core::decode::View::over_retained(&root_after_payload)
    )
    .expect("admitted ZIP detection"));
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&codec, &root_after_payload),
        Confidence::Medium
    );

    let marker_in_payload = step_zip(&[("preview.bin", marker_payload, CompressionMethod::Stored)]);
    assert!(!super::has_root_marker(
        &ctx,
        cadmpeg_core::decode::View::over_retained(&marker_in_payload)
    )
    .expect("admitted ZIP detection"));
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&codec, &marker_in_payload),
        Confidence::Low
    );

    let marker_in_filename = step_zip(&[(
        "parts/ISO-10303.p21.preview",
        b"ancillary",
        CompressionMethod::Stored,
    )]);
    assert!(!super::has_root_marker(
        &ctx,
        cadmpeg_core::decode::View::over_retained(&marker_in_filename)
    )
    .expect("admitted ZIP detection"));
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&codec, &marker_in_filename),
        Confidence::Low
    );

    let marker_in_comment = step_zip_with_comment(
        &[("preview.bin", b"ancillary", CompressionMethod::Stored)],
        ROOT_NAME,
    );
    assert!(!super::has_root_marker(
        &ctx,
        cadmpeg_core::decode::View::over_retained(&marker_in_comment)
    )
    .expect("admitted ZIP detection"));
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&codec, &marker_in_comment),
        Confidence::Low
    );
}

#[test]
fn codec_decodes_step_zip_root_and_reports_archive_members() {
    let root = include_bytes!("../../tests/fixtures/ap242_minimal.p21");
    let bytes = step_zip(&[
        ("ISO-10303.p21", root, CompressionMethod::Deflated),
        ("parts/child.p21", b"subsidiary", CompressionMethod::Stored),
        ("preview.bin", b"ancillary", CompressionMethod::Stored),
    ]);
    let codec = StepCodec::default();

    assert_eq!(
        cadmpeg_test_support::detection::confidence(&codec, &bytes),
        Confidence::Medium
    );
    let summary = codec
        .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
        .expect("inspect STEP ZIP");
    assert_eq!(summary.container_kind, "iso-10303-21-zip");
    assert_eq!(
        summary
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        ["ISO-10303.p21", "parts/child.p21", "preview.bin"]
    );
    assert_eq!(summary.entries[0].role.as_str(), "root-exchange");
    let cadmpeg_core::container::EntryStorage::Compressed { method, .. } =
        &summary.entries[0].storage
    else {
        panic!("a deflated zip entry stores compressed bytes");
    };
    assert_eq!(method.as_str(), "deflate");
    assert_eq!(summary.entries[1].role.as_str(), "subsidiary-exchange");
    assert!(summary.entries[0].attributes["logical_sections"].contains("HEADER"));

    let result = codec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("decode STEP ZIP root");
    let source = result.ir().source.as_ref().expect("STEP source metadata");
    assert_eq!(source.format(), "step");
    assert_eq!(source.attributes["container_kind"], "iso-10303-21-zip");
    assert_eq!(source.attributes["archive_root"], "ISO-10303.p21");
    assert_eq!(source.attributes["archive_entries"], "3");
    assert!(result
        .report()
        .notes
        .iter()
        .any(|note| note == "container root ISO-10303.p21; archive entries=3"));
}

#[test]
fn codec_resolves_root_references_relative_to_the_archive_directory() {
    let root = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('zip references'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;#10=<parts/child.p21?query=../outside#target>;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let bytes = step_zip(&[
        ("ISO-10303.p21", root, CompressionMethod::Stored),
        ("parts/child.p21", b"child", CompressionMethod::Stored),
    ]);
    let codec = StepCodec::default();
    let summary = codec
        .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
        .expect("inspect relative ZIP reference");
    assert!(summary
        .notes
        .iter()
        .any(|note| note == "internal resource #10 -> parts/child.p21?query=../outside#target"));

    let missing = step_zip(&[("ISO-10303.p21", root, CompressionMethod::Stored)]);
    assert!(matches!(
        codec.inspect(&mut Cursor::new(missing), &InspectOptions::default()),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));

    let escaping = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('zip references'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;#10=<../outside.p21#target>;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let escaping = step_zip(&[("ISO-10303.p21", escaping, CompressionMethod::Stored)]);
    assert!(matches!(
        codec.inspect(&mut Cursor::new(escaping), &InspectOptions::default()),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn codec_checks_forwarded_root_references_without_decoding_the_subsidiary() {
    let root = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('zip forwarded reference'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<target>=<parts/child.p21#target>;ENDSEC;REFERENCE;#10=<#target>;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let bytes = step_zip(&[
        ("ISO-10303.p21", root, CompressionMethod::Stored),
        (
            "parts/child.p21",
            b"not a STEP exchange structure",
            CompressionMethod::Stored,
        ),
    ]);
    let codec = StepCodec::default();

    let summary = codec
        .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
        .expect("inspect forwarded ZIP reference");
    assert!(summary
        .notes
        .iter()
        .any(|note| note == "internal resource #10 -> parts/child.p21#target"));

    let missing = step_zip(&[("ISO-10303.p21", root, CompressionMethod::Stored)]);
    assert!(matches!(
        codec.inspect(&mut Cursor::new(missing), &InspectOptions::default()),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));

    let result = codec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("decode root without parsing subsidiary");
    assert!(result
        .report()
        .notes
        .iter()
        .any(|note| note == "internal resource #10 -> parts/child.p21#target"));
}

#[test]
fn root_reference_notes_use_the_contextually_parsed_exchange() {
    let root = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('zip references'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;#10=<parts/child.p21#target>;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let bytes = step_zip(&[
        (ROOT_NAME, root, CompressionMethod::Stored),
        ("parts/child.p21", b"child", CompressionMethod::Stored),
    ]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, view) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("ZIP fits the service profile");
    let opened = super::open_root(&ctx, view).expect("open ZIP root");
    let (exchange, _) = crate::parse::parse_with_context(opened.view.window(), &ctx)
        .expect("parse root under the active context");
    assert_eq!(
        root_reference_notes(&ctx, &opened.archive, &exchange).expect("resolve parsed references"),
        vec!["internal resource #10 -> parts/child.p21#target"]
    );
}

#[test]
fn root_reference_note_refuses_collection_limit() {
    let root = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('zip references'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;#10=<parts/child.p21#target>;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let bytes = step_zip(&[
        (ROOT_NAME, root, CompressionMethod::Stored),
        ("parts/child.p21", b"child", CompressionMethod::Stored),
    ]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let service = cadmpeg_core::decode::DecodePolicy::service();
    let (service_ctx, view) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &service)
            .expect("ZIP fits the service profile");
    let opened = super::open_root(&service_ctx, view).expect("open ZIP root");
    let (exchange, _) = crate::parse::parse_with_context(opened.view.window(), &service_ctx)
        .expect("parse root under the service context");
    let mut limited = service;
    limited.limits.max_collection_items = 2;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &limited)
        .expect("ZIP fits the limited profile");
    let error = root_reference_notes(&ctx, &opened.archive, &exchange)
        .expect_err("two URI components leave no item for the note");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && limit.operation == "step_zip_reference_notes"
    ));
}

#[test]
fn caller_composition_resolves_forwarded_zip_target_without_root_import() {
    let root = include_bytes!("tests/data/ce02_composition_root.p21");
    let subsidiary = include_bytes!("tests/data/ce02_composition_subsidiary.p21");
    let bytes = step_zip(&[
        (ROOT_NAME, root, CompressionMethod::Stored),
        (
            "parts/ce02_composition_subsidiary.p21",
            subsidiary,
            CompressionMethod::Stored,
        ),
    ]);

    let (root_exchange, root_diagnostics) =
        crate::test_support::with_service_context(root, crate::parse::parse_inner)
            .expect("parse ZIP root");
    let (subsidiary_exchange, subsidiary_diagnostics) =
        crate::test_support::with_service_context(subsidiary, crate::parse::parse_inner)
            .expect("parse ZIP subsidiary");
    assert!(root_diagnostics.is_empty());
    assert!(subsidiary_diagnostics.is_empty());
    assert_eq!(root_exchange.references()[0].name.to_string(), "#10");
    assert_eq!(root_exchange.references()[0].uri, "#target");
    assert_eq!(
        root_exchange.anchors()[0].value,
        crate::parse::Value::Resource("parts/ce02_composition_subsidiary.p21#remote_point".into())
    );
    assert_eq!(
        root_exchange.schema_identifiers().collect::<Vec<_>>(),
        subsidiary_exchange.schema_identifiers().collect::<Vec<_>>()
    );

    let crate::parse::Value::Resource(resource_uri) = &root_exchange.anchors()[0].value else {
        panic!("root forwarding anchor");
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(root, &arena, &policy)
        .expect("test context");
    let ReferenceTarget::Internal {
        member,
        query,
        fragment,
    } = resolve_uri_for_test(&ctx, ROOT_NAME, resource_uri)
        .expect("resolve forwarded ZIP resource")
    else {
        panic!("forwarded ZIP resource became external");
    };
    assert_eq!(member, "parts/ce02_composition_subsidiary.p21");
    assert_eq!(query, None);
    assert_eq!(fragment, Some("remote_point"));
    let anchor = subsidiary_exchange
        .anchors()
        .iter()
        .find(|anchor| anchor.name == "remote_point")
        .expect("subsidiary target anchor");
    let crate::parse::Value::Reference(target_id) = anchor.value else {
        panic!("subsidiary anchor is not an entity");
    };
    let target = subsidiary_exchange
        .records()
        .get(&target_id)
        .expect("subsidiary target entity");
    assert!(target
        .partials
        .iter()
        .any(|partial| partial.name == "CARTESIAN_POINT"));
    assert_eq!(target_id, 12);

    let mut archive = ZipArchive::new(Cursor::new(&bytes)).expect("open composition ZIP");
    let mut extracted = Vec::new();
    archive
        .by_name(&member)
        .expect("open addressed subsidiary member")
        .read_to_end(&mut extracted)
        .expect("read addressed subsidiary member");
    assert_eq!(extracted, subsidiary);

    let result = StepCodec::default()
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("decode ZIP root without importing target");
    assert_eq!(
        result.ir().source.as_ref().unwrap().attributes["entity_instances"],
        "6"
    );
    assert!(result
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.id.as_str() == "step:data:point#4"));
    assert!(!result
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.id.as_str() == "step:data:point#12"));
    assert!(result.report().notes.iter().any(|note| note
        == "internal resource #10 -> parts/ce02_composition_subsidiary.p21#remote_point"));

    let target_result = StepCodec::default()
        .decode(&mut Cursor::new(extracted), &DecodeOptions::default())
        .expect("decode subsidiary independently");
    assert!(target_result
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.id.as_str() == "step:data:point#12"));
}

#[test]
fn forwarded_reference_retains_unparsed_subsidiary_as_a_resource() {
    let root = include_bytes!("tests/data/ce02_root_forwarded_unparsed.p21");
    let subsidiary = include_bytes!("tests/data/ce02_subsidiary_unparsed.p21");
    let (root_exchange, root_diagnostics) =
        crate::test_support::with_service_context(root, crate::parse::parse_inner)
            .expect("parse CE-02 root");
    assert!(root_diagnostics.is_empty());
    assert_eq!(
        root_exchange.anchors()[0].value,
        crate::parse::Value::Resource("parts/ce02_subsidiary_unparsed.p21#remote_item".into())
    );
    assert_eq!(root_exchange.references()[0].uri, "#target");

    let bytes = step_zip(&[
        (ROOT_NAME, root, CompressionMethod::Stored),
        (
            "parts/ce02_subsidiary_unparsed.p21",
            subsidiary,
            CompressionMethod::Stored,
        ),
    ]);
    let codec = StepCodec::default();
    let summary = codec
        .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
        .expect("inspect root without parsing subsidiary");
    assert_eq!(summary.entries.len(), 2);
    assert!(summary
        .notes
        .iter()
        .any(|note| note
            == "internal resource #10 -> parts/ce02_subsidiary_unparsed.p21#remote_item"));

    let result = codec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("decode root without parsing subsidiary");
    let source = result.ir().source.as_ref().expect("STEP source metadata");
    assert_eq!(source.attributes["archive_root"], ROOT_NAME);
    assert_eq!(source.attributes["entity_instances"], "1");
    assert_eq!(
        result
            .ir()
            .native_unknowns("step")
            .expect("STEP unknown arena")
            .len(),
        1
    );
    assert!(result
        .report()
        .notes
        .iter()
        .any(|note| note == "external reference #10 -> #target"));
    assert!(result
        .report()
        .notes
        .iter()
        .any(|note| note
            == "internal resource #10 -> parts/ce02_subsidiary_unparsed.p21#remote_item"));
}

#[test]
fn codec_keeps_external_reference_graph_resource_local() {
    let root = include_bytes!("tests/data/er03_root.p21");
    let subsidiary = include_bytes!("tests/data/er03_subsidiary.p21");
    let bytes = step_zip(&[
        (ROOT_NAME, root, CompressionMethod::Stored),
        (
            "parts/er03_subsidiary.p21",
            subsidiary,
            CompressionMethod::Stored,
        ),
    ]);
    let codec = StepCodec::default();

    let summary = codec
        .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
        .expect("inspect resource-composition witness");
    assert_eq!(summary.entries.len(), 2);
    assert_eq!(summary.entries[0].name, ROOT_NAME);
    assert!(!summary.entries[1]
        .attributes
        .contains_key("logical_sections"));
    assert!(summary
        .notes
        .iter()
        .any(|note| note == "internal resource #10 -> parts/er03_subsidiary.p21#remote_item"));

    let result = codec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("decode root without composing subsidiary");
    let source = result.ir().source.as_ref().expect("STEP source metadata");
    assert_eq!(source.attributes["entity_instances"], "1");
    assert_eq!(
        result
            .ir()
            .native_unknowns("step")
            .expect("STEP unknown arena")
            .len(),
        1
    );
    assert!(result
        .report()
        .notes
        .contains(&"external reference #10 -> parts/er03_subsidiary.p21#remote_item".into()));
    assert!(result
        .report()
        .notes
        .contains(&"internal resource #10 -> parts/er03_subsidiary.p21#remote_item".into()));
}

#[test]
fn valid_resource_pair_keeps_target_anchor_and_root_graph_separate() {
    let root = include_bytes!("tests/data/er03_root_valid.p21");
    let subsidiary = include_bytes!("tests/data/er03_subsidiary_valid.p21");
    let (root_exchange, root_diagnostics) =
        crate::test_support::with_service_context(root, crate::parse::parse_inner)
            .expect("parse valid root");
    assert!(root_diagnostics.is_empty());
    assert_eq!(
        root_exchange.references()[0].uri,
        "parts/er03_subsidiary_valid.p21#remote_item"
    );
    assert_eq!(
        root_exchange.records()[&1].partials[0]
            .parameters
            .as_slice(),
        vec![crate::parse::Value::Reference(10)]
    );

    let (subsidiary_exchange, subsidiary_diagnostics) =
        crate::test_support::with_service_context(subsidiary, crate::parse::parse_inner)
            .expect("parse valid subsidiary");
    assert!(subsidiary_diagnostics.is_empty());
    assert_eq!(
        subsidiary_exchange.anchors()[0].value,
        crate::parse::Value::Reference(1)
    );
    assert_eq!(
        subsidiary_exchange.records()[&1].partials[0]
            .parameters
            .as_slice(),
        vec![crate::parse::Value::String(b"remote".to_vec())]
    );

    let bytes = step_zip(&[
        (ROOT_NAME, root, CompressionMethod::Stored),
        (
            "parts/er03_subsidiary_valid.p21",
            subsidiary,
            CompressionMethod::Stored,
        ),
    ]);
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode valid root without composing subsidiary");
    let source = result.ir().source.as_ref().expect("STEP source metadata");
    assert_eq!(source.attributes["entity_instances"], "1");
    assert_eq!(
        result
            .ir()
            .native_unknowns("step")
            .expect("STEP unknown arena")
            .len(),
        1
    );
    assert!(result
        .report()
        .notes
        .contains(&"internal resource #10 -> parts/er03_subsidiary_valid.p21#remote_item".into()));
    assert!(result
        .report()
        .notes
        .contains(&"external reference #10 -> parts/er03_subsidiary_valid.p21#remote_item".into()));
}

#[test]
fn distinct_external_resources_keep_reused_numeric_targets_separate() {
    let root = include_bytes!("tests/data/er03_root_distinct_resources.p21");
    let alpha = include_bytes!("tests/data/er03_subsidiary_alpha.p21");
    let beta = include_bytes!("tests/data/er03_subsidiary_beta.p21");
    let (root_exchange, root_diagnostics) =
        crate::test_support::with_service_context(root, crate::parse::parse_inner)
            .expect("parse identity root");
    assert!(root_diagnostics.is_empty());
    assert_eq!(root_exchange.references().len(), 2);
    assert_eq!(
        root_exchange.references()[0].uri,
        "parts/er03_subsidiary_alpha.p21#remote_item"
    );
    assert_eq!(
        root_exchange.references()[1].uri,
        "parts/er03_subsidiary_beta.p21#remote_item"
    );
    assert_eq!(
        root_exchange.records()[&1].partials[0]
            .parameters
            .as_slice(),
        vec![
            crate::parse::Value::Reference(10),
            crate::parse::Value::Reference(11)
        ]
    );

    for (bytes, value) in [
        (alpha.as_slice(), b"alpha".as_slice()),
        (beta.as_slice(), b"beta".as_slice()),
    ] {
        let (exchange, diagnostics) =
            crate::test_support::with_service_context(bytes, crate::parse::parse_inner)
                .expect("parse subsidiary");
        assert!(diagnostics.is_empty());
        assert_eq!(exchange.anchors()[0].name, "remote_item");
        assert_eq!(
            exchange.anchors()[0].value,
            crate::parse::Value::Reference(1)
        );
        assert_eq!(
            exchange.records()[&1].partials[0].parameters.as_slice(),
            vec![crate::parse::Value::String(value.to_vec())]
        );
    }

    let bytes = step_zip(&[
        (ROOT_NAME, root, CompressionMethod::Stored),
        (
            "parts/er03_subsidiary_alpha.p21",
            alpha,
            CompressionMethod::Stored,
        ),
        (
            "parts/er03_subsidiary_beta.p21",
            beta,
            CompressionMethod::Stored,
        ),
    ]);
    let codec = StepCodec::default();
    let summary = codec
        .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
        .expect("inspect distinct resource identity witness");
    assert_eq!(summary.entries.len(), 3);
    for note in [
        "internal resource #10 -> parts/er03_subsidiary_alpha.p21#remote_item",
        "internal resource #11 -> parts/er03_subsidiary_beta.p21#remote_item",
    ] {
        assert!(summary.notes.iter().any(|candidate| candidate == note));
    }

    let result = EditableDecodeResult::from(
        codec
            .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
            .expect("decode root without composing subsidiaries"),
    );
    let source = result.ir().source.as_ref().expect("STEP source metadata");
    assert_eq!(source.attributes["entity_instances"], "1");
    let unknown = result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena");
    assert_eq!(unknown.len(), 1);
    assert_eq!(unknown[0].id.as_str(), "step:data:item#1");
    assert_eq!(
        result
            .source_fidelity()
            .retained_record(unknown[0].id.as_str())
            .expect("root occurrence source fidelity")
            .data(),
        Some(b"#1=ITEM(#10,#11);".as_slice())
    );
    for note in [
        "external reference #10 -> parts/er03_subsidiary_alpha.p21#remote_item",
        "external reference #11 -> parts/er03_subsidiary_beta.p21#remote_item",
        "internal resource #10 -> parts/er03_subsidiary_alpha.p21#remote_item",
        "internal resource #11 -> parts/er03_subsidiary_beta.p21#remote_item",
    ] {
        assert!(result
            .report()
            .notes
            .iter()
            .any(|candidate| candidate == note));
    }
}

#[test]
fn valid_forwarded_root_anchor_keeps_archive_target_resource_qualified() {
    let root = include_bytes!("tests/data/ce02_root_anchor_valid.p21");
    let subsidiary = include_bytes!("tests/data/ce02_subsidiary_valid.p21");
    let (root_exchange, root_diagnostics) =
        crate::test_support::with_service_context(root, crate::parse::parse_inner)
            .expect("parse CE-02 root");
    assert!(root_diagnostics.is_empty());
    assert_eq!(
        root_exchange.anchors()[0].value,
        crate::parse::Value::Resource("parts/ce02_subsidiary_valid.p21#remote_item".into())
    );
    assert_eq!(root_exchange.references()[0].uri, "#target");

    let (subsidiary_exchange, subsidiary_diagnostics) =
        crate::test_support::with_service_context(subsidiary, crate::parse::parse_inner)
            .expect("parse CE-02 subsidiary");
    assert!(subsidiary_diagnostics.is_empty());
    assert_eq!(
        subsidiary_exchange.anchors()[0].value,
        crate::parse::Value::Reference(1)
    );
    assert_eq!(
        subsidiary_exchange.records()[&1].partials[0]
            .parameters
            .as_slice(),
        vec![crate::parse::Value::String(b"remote".to_vec())]
    );

    let bytes = step_zip(&[
        (ROOT_NAME, root, CompressionMethod::Stored),
        (
            "parts/ce02_subsidiary_valid.p21",
            subsidiary,
            CompressionMethod::Stored,
        ),
    ]);
    let codec = StepCodec::default();
    let summary = codec
        .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
        .expect("inspect valid forwarded archive");
    assert_eq!(summary.entries.len(), 2);
    assert!(!summary.entries[1]
        .attributes
        .contains_key("logical_sections"));
    assert!(
        summary
            .notes
            .iter()
            .any(|note| note
                == "internal resource #10 -> parts/ce02_subsidiary_valid.p21#remote_item")
    );

    let result = codec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("decode root with valid forwarded target");
    let source = result.ir().source.as_ref().expect("STEP source metadata");
    assert_eq!(source.attributes["entity_instances"], "1");
    assert_eq!(
        result
            .ir()
            .native_unknowns("step")
            .expect("STEP unknown arena")
            .len(),
        1
    );
    assert!(result
        .report()
        .notes
        .contains(&"internal resource #10 -> parts/ce02_subsidiary_valid.p21#remote_item".into()));
    assert!(result
        .report()
        .notes
        .contains(&"external reference #10 -> #target".into()));

    let missing = step_zip(&[(ROOT_NAME, root, CompressionMethod::Stored)]);
    assert!(matches!(
        codec.inspect(&mut Cursor::new(missing), &InspectOptions::default()),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn codec_rejects_step_zip_without_root_or_with_unsupported_layout() {
    let root = include_bytes!("../../tests/fixtures/ap242_minimal.p21");
    let codec = StepCodec::default();

    let missing_root = step_zip(&[("part.p21", root, CompressionMethod::Stored)]);
    assert!(matches!(
        codec.inspect(&mut Cursor::new(missing_root), &InspectOptions::default()),
        Err(cadmpeg_core::CodecError::WrongFormat(_))
    ));

    let unsafe_path = step_zip(&[
        ("ISO-10303.p21", root, CompressionMethod::Stored),
        ("../outside.bin", b"unsafe", CompressionMethod::Stored),
    ]);
    assert!(matches!(
        codec.inspect(&mut Cursor::new(unsafe_path), &InspectOptions::default()),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));

    let zstd = step_zip(&[("ISO-10303.p21", root, CompressionMethod::Zstd)]);
    let summary = codec
        .inspect(&mut Cursor::new(zstd), &InspectOptions::default())
        .expect("recover safely readable noncanonical root compression");
    assert!(summary
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::StepLossCode::ContainerMemberNoncanonical.kind()));

    let unicode_name = step_zip(&[
        ("ISO-10303.p21", root, CompressionMethod::Stored),
        ("π-preview.bin", b"ancillary", CompressionMethod::Stored),
    ]);
    let summary = codec
        .inspect(&mut Cursor::new(unicode_name), &InspectOptions::default())
        .expect("recover ancillary Unicode declaration");
    assert!(summary
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::StepLossCode::ContainerMemberNoncanonical.kind()));

    let duplicate_name = duplicate_first_central_record(step_zip(&[(
        "ISO-10303.p21",
        root,
        CompressionMethod::Stored,
    )]));
    assert!(matches!(
        codec.inspect(&mut Cursor::new(duplicate_name), &InspectOptions::default()),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));

    let encrypted = mark_entries_encrypted(step_zip(&[(
        "ISO-10303.p21",
        root,
        CompressionMethod::Stored,
    )]));
    assert!(matches!(
        codec.inspect(&mut Cursor::new(encrypted), &InspectOptions::default()),
        Err(cadmpeg_core::CodecError::NotImplemented(message)) if message.contains("encrypted ZIP entry")
    ));

    let checksum_mismatch = corrupt_first_payload(step_zip(&[(
        "ISO-10303.p21",
        root,
        CompressionMethod::Stored,
    )]));
    assert!(matches!(
        codec.inspect(
            &mut Cursor::new(checksum_mismatch),
            &InspectOptions::default()
        ),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
pub(crate) fn codec_inspects_edition3_sections_and_external_references() {
    let bytes = include_bytes!("../../tests/fixtures/ap242_ed3_sections.p21");
    let summary = StepCodec::default()
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("inspect edition 3 sections");

    assert_eq!(
        summary
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        [
            "HEADER",
            "ANCHOR",
            "REFERENCE",
            "DATA[0]",
            "DATA[1]",
            "SIGNATURE"
        ]
    );
    let references = summary
        .entries
        .iter()
        .find(|entry| entry.name == "REFERENCE")
        .unwrap();
    assert_eq!(references.attributes["external_count"], "1");
    assert_eq!(
        references.attributes["external_uris"],
        "https://example.invalid/external-part"
    );
    assert_eq!(summary.entries[3].attributes["unknown_entities"], "");
    assert_eq!(
        summary.entries[4].attributes["unknown_entities"],
        "EXAMPLE_RECORD:1"
    );
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(bytes, crate::parse::parse_inner)
            .expect("parse opaque signature payload");
    assert!(diagnostics.is_empty());
    assert_eq!(exchange.signatures().len(), 1);
    let signature = exchange.signatures()[0].clone();
    assert!(bytes[signature.clone()]
        .windows(b"MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=".len())
        .any(|bytes| bytes == b"MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA="));
    let decoded = EditableDecodeResult::from(
        StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode signature fixture"),
    );
    let unknowns = decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena");
    let signature_unknown = unknowns
        .iter()
        .find(|record| record.id.as_str() == "step:file:signature#0")
        .expect("retained signature");
    assert_eq!(
        decoded
            .source_fidelity()
            .retained_record(signature_unknown.id.as_str())
            .and_then(|record| record.data()),
        Some(&bytes[signature.clone()])
    );
    assert_eq!(
        exchange.records()[&2].partials[0].parameters.as_slice(),
        vec![crate::parse::Value::Reference(1)]
    );
}

#[test]
fn forwarded_anchor_lookup_refuses_caller_work_limit() {
    const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<aa>=<https://example.invalid/a>;<ab>=<https://example.invalid/b>;<ac>=<https://example.invalid/c>;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(SOURCE, crate::parse::parse_inner)
            .expect("valid anchor exchange");
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 5;
    crate::test_support::with_policy_context(SOURCE, &policy, |_, ctx| {
        let error = super::forwarded_reference_uri(&exchange, "#ac", ctx)
            .expect_err("late anchor comparison exceeds work allowance");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.operation == "step_zip_anchor_lookup"
                && refusal.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
        );
    });
    crate::test_support::with_service_context(SOURCE, |_, ctx| {
        assert_eq!(
            super::forwarded_reference_uri(&exchange, "#ac", ctx).unwrap(),
            "https://example.invalid/c"
        );
        assert_eq!(
            super::forwarded_reference_uri(&exchange, "#missing", ctx).unwrap(),
            "#missing"
        );
    });
}

#[test]
fn zip_detection_resource_refusals_reach_the_caller() {
    let bytes = step_zip(&[(ROOT_NAME, b"root", CompressionMethod::Stored)]);
    for dimension in [
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
    ] {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        match dimension {
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = 0;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = 0;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            _ => panic!("test dimension"),
        }
        let (ctx, root) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                .expect("root");
        assert!(
            matches!(super::has_root_marker(&ctx, root), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.dimension == dimension)
        );
    }
}

#[test]
fn zip_detection_reads_names_without_payload_admission() {
    for (local_offset, central_offset, value) in [(6, 8, 1), (8, 10, 12)] {
        let mut bytes = step_zip(&[(ROOT_NAME, b"root", CompressionMethod::Stored)]);
        let central = bytes
            .windows(4)
            .position(|bytes| bytes == b"PK\x01\x02")
            .expect("central directory");
        cadmpeg_test_support::bytes::put_u16(&mut bytes, local_offset, value);
        cadmpeg_test_support::bytes::put_u16(&mut bytes, central + central_offset, value);
        assert_eq!(
            cadmpeg_test_support::detection::confidence(&StepCodec::default(), &bytes),
            Confidence::Medium
        );
    }
}

#[test]
fn repeated_root_anchor_forwarding_reaches_the_actual_member() {
    let root = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('zip'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<first>=<#second>;<second>=<parts/child.p21#shape>;ENDSEC;REFERENCE;#10=<#first>;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let bytes = step_zip(&[
        ("ISO-10303.p21", root, CompressionMethod::Stored),
        (
            "parts/child.p21",
            b"opaque subsidiary",
            CompressionMethod::Stored,
        ),
    ]);
    let summary = StepCodec::default()
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .unwrap();
    assert!(summary
        .notes
        .iter()
        .any(|note| note == "internal resource #10 -> parts/child.p21#shape"));
    let missing = step_zip(&[("ISO-10303.p21", root, CompressionMethod::Stored)]);
    assert!(StepCodec::default()
        .inspect(&mut Cursor::new(missing), &InspectOptions::default())
        .is_err());
}

#[test]
fn optional_archive_profile_defects_preserve_geometry_and_declarations() {
    let root = include_bytes!("../writer/tests/data/periodic_two_rims.p21");
    let baseline = StepCodec::default()
        .decode(&mut Cursor::new(root), &DecodeOptions::default())
        .unwrap();
    for (name, compression) in [
        ("preview.bin", CompressionMethod::Zstd),
        ("π-preview.bin", CompressionMethod::Stored),
    ] {
        let bytes = step_zip(&[
            ("ISO-10303.p21", root, CompressionMethod::Stored),
            (name, b"preview", compression),
        ]);
        let result = EditableDecodeResult::from(
            StepCodec::default()
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap(),
        );
        assert_eq!(result.ir().model, baseline.ir().model);
        assert!(
            result
                .report()
                .losses
                .iter()
                .any(|loss| loss.code
                    == crate::loss::StepLossCode::ContainerMemberNoncanonical.kind())
        );
        assert!(result
            .source_fidelity()
            .retained_records()
            .keys()
            .any(|id| id.as_str().contains(":zip-declaration#")));
        assert!(cadmpeg_ir::validate_neutral_with_source_fidelity(
            result.ir(),
            result.source_fidelity(),
            Vec::new()
        )
        .unwrap()
        .is_ok());
    }
}

#[test]
fn damaged_first_ancillary_local_frame_keeps_required_root_readable() {
    let root = include_bytes!("../writer/tests/data/periodic_two_rims.p21");
    let codec = StepCodec::default();
    let baseline = codec
        .decode(&mut Cursor::new(root), &DecodeOptions::default())
        .unwrap();
    let mut bytes = step_zip(&[
        ("preview.bin", b"preview", CompressionMethod::Stored),
        (ROOT_NAME, root, CompressionMethod::Deflated),
    ]);
    bytes[0] ^= 1;
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&codec, &bytes),
        Confidence::Medium
    );
    let result = EditableDecodeResult::from(
        codec
            .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
            .unwrap(),
    );
    assert_eq!(result.ir().model, baseline.ir().model);
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::StepLossCode::ContainerMemberNoncanonical.kind()));
    assert!(result
        .source_fidelity()
        .retained_records()
        .keys()
        .any(|id| id.as_str().contains(":zip-declaration#")));
    let mut strict = DecodeOptions::default();
    strict.policy.mode = cadmpeg_core::decode::DecodeMode::Strict;
    assert!(matches!(
        codec.decode(&mut Cursor::new(&bytes), &strict),
        Err(cadmpeg_ir::codec::DecodeFailure::StrictRejected { .. })
    ));
}

#[test]
fn zip_root_parse_obeys_caller_recursion_ceiling() {
    let root = b"ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM((((((((((1))))))))));ENDSEC;END-ISO-10303-21;";
    let bytes = step_zip(&[(ROOT_NAME, root, CompressionMethod::Stored)]);
    let mut options = DecodeOptions::default();
    options.policy.limits.max_recursion_depth = 6;
    for input in [root.as_slice(), bytes.as_slice()] {
        assert!(
            matches!(StepCodec::default().decode(&mut Cursor::new(input), &options), Err(cadmpeg_ir::codec::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(refusal))) if refusal.dimension == cadmpeg_core::decode::ResourceDimension::RecursionDepth)
        );
        let inspect = InspectOptions {
            limits: options.policy.limits,
        };
        assert!(
            matches!(StepCodec::default().inspect(&mut Cursor::new(input), &inspect), Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) if refusal.dimension == cadmpeg_core::decode::ResourceDimension::RecursionDepth)
        );
    }
    StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("ordinary limits admit the same bounded root");
}
