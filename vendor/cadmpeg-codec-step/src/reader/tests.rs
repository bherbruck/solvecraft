// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]
use cadmpeg_test_support::{wire, EditableDecodeResult};

use super::{
    byte_accounting, claim_trivia, decode_exchange_mode, implicit_face_plane_work,
    semantic_input_work, ByteClass, Packaging, ValueExt,
};
use crate::loss::StepLossCode;
use std::collections::HashSet;

const REFERENCE_NOTE_LIMIT_SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;3');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;@100=<part.step#width>;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
const DIAGNOSTIC_LOSS_LIMIT_SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;9');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";

#[test]
fn step_integer_number_refuses_inexact_f64() {
    assert_eq!(
        crate::parse::Value::Integer(1_i64 << 53).number(),
        Some(9_007_199_254_740_992.0)
    );
    assert_eq!(
        crate::parse::Value::Integer((1_i64 << 53) + 1).number(),
        None
    );
}

#[test]
fn malformed_face_names_preserve_geometry_and_report_source_only_metadata() {
    let original = include_str!("../writer/tests/data/periodic_two_rims.p21");
    let expected = StepCodec::default()
        .decode(&mut Cursor::new(original), &DecodeOptions::default())
        .unwrap();
    assert!(!expected.ir().model.faces.is_empty());
    for name in ["7", "1.E999", "\"0ZZ\"", "<uri>"] {
        let changed = original.replace("ADVANCED_FACE('',", &format!("ADVANCED_FACE({name},"));
        assert_ne!(changed, original);
        let result = StepCodec::default()
            .decode(&mut Cursor::new(&changed), &DecodeOptions::default())
            .unwrap();
        assert_eq!(result.ir().model, expected.ir().model);
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == StepLossCode::MetadataStringInvalid.kind()));
        assert_eq!(
            result.ir().source.as_ref().unwrap().attributes["bytes_unclassified"],
            "0"
        );
        assert!(cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
            .unwrap()
            .is_ok());
    }
}

#[test]
fn byte_census_keeps_metadata_admission_after_an_omitted_record() {
    let source = b"ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(#0);#2=CARTESIAN_POINT(1.E999,(1.,2.,3.));#3=CARTESIAN_POINT('',(4.,5.,6.));#4=GEOMETRIC_SET('',(#2,#3));ENDSEC;END-ISO-10303-21;";
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("bounded defect and unreadable metadata remain recoverable");
    assert_eq!(decoded.ir().model.points.len(), 2);
    assert_eq!(
        decoded.ir().source.as_ref().expect("source").attributes["bytes_unclassified"],
        "0"
    );
}

#[test]
fn orphan_point_keeps_its_exact_record_when_no_carrier_owns_it() {
    let source = b"ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT('',(1.,2.,3.));ENDSEC;END-ISO-10303-21;";
    let decoded = EditableDecodeResult::from(
        StepCodec::default()
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("bounded standalone coordinate"),
    );
    assert!(decoded.ir().model.points.is_empty());
    let unknowns = decoded
        .ir()
        .native_unknowns("step")
        .expect("retained source");
    let point = unknowns
        .iter()
        .find(|record| record.id.as_str() == "step:data:cartesian_point#1")
        .expect("orphan point source");
    assert_eq!(
        decoded
            .source_fidelity()
            .retained_record(point.id.as_str())
            .expect("exact point source")
            .data(),
        Some(b"#1=CARTESIAN_POINT('',(1.,2.,3.));".as_slice())
    );
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::OpaqueRecordPreserved.kind()));
}

#[test]
fn opaque_reference_does_not_transfer_a_coordinate() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT('',(1.,2.,3.));#2=UNKNOWN_ITEM(#1);ENDSEC;END-ISO-10303-21;";
    let decoded = EditableDecodeResult::from(
        StepCodec::default()
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("bounded opaque reference"),
    );
    assert!(decoded.ir().model.points.is_empty());
    let unknowns = decoded
        .ir()
        .native_unknowns("step")
        .expect("retained source");
    assert_eq!(unknowns.len(), 2);
    let point_id = "step:data:cartesian_point#1";
    let item = unknowns
        .iter()
        .find(|record| record.id.as_str() == "step:data:unknown_item#2")
        .expect("opaque user");
    assert!(item.links.iter().any(|link| link.as_str() == point_id));
    assert_eq!(
        decoded
            .source_fidelity()
            .retained_record(point_id)
            .expect("exact coordinate source")
            .data(),
        Some(b"#1=CARTESIAN_POINT('',(1.,2.,3.));".as_slice())
    );
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::OpaqueRecordPreserved.kind()
            && loss.message.contains("CARTESIAN_POINT")
    }));
}

#[test]
fn record_display_name_refuses_retained_byte_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=(ALPHA() BETA());ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid complex record");
    let record = exchange.records().get(&1).expect("record one");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 9;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("root fits policy");
    assert!(
        matches!(ctx.join_display_retained(record.partials.iter().map(|partial| partial.name.as_str()), "+", "step_record_display_name"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_record_display_name")
    );
}

#[test]
fn inspect_opaque_offsets_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (exchange, _) = crate::test_support::with_service_context(
        DIAGNOSTIC_LOSS_LIMIT_SOURCE,
        crate::parse::parse_inner,
    )
    .expect("valid exchange with one record");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(DIAGNOSTIC_LOSS_LIMIT_SOURCE, &arena, &policy)
        .expect("root fits collection policy");
    assert!(matches!(
        super::inspect_opaque_offsets(&exchange, &HashSet::new(), &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_inspect_opaque_offsets"
    ));
}

#[test]
fn decode_loss_notes_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (exchange, diagnostics) = crate::test_support::with_service_context(
        DIAGNOSTIC_LOSS_LIMIT_SOURCE,
        crate::parse::parse_inner,
    )
    .expect("valid exchange with diagnostic");
    assert!(!diagnostics.is_empty());
    let arena = DecodeArena::new();
    let refused = (0..64).any(|limit| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(DIAGNOSTIC_LOSS_LIMIT_SOURCE, &arena, &policy)
            .expect("root fits collection policy");
        matches!(
            super::StepDecodeSession::new(&exchange, &diagnostics, &ctx, super::DecodeMode::Inspect),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "step_decode_loss_notes"
        )
    });
    assert!(refused, "no collection limit refused decode loss note");
}

#[test]
fn decode_diagnostic_message_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (exchange, diagnostics) = crate::test_support::with_service_context(
        DIAGNOSTIC_LOSS_LIMIT_SOURCE,
        crate::parse::parse_inner,
    )
    .expect("valid exchange with diagnostic");
    assert!(!diagnostics.is_empty());
    let arena = DecodeArena::new();
    let refused = {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "step_decode_diagnostic_message",
            |limit| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = limit;
                let (ctx, _) =
                    DecodeContext::from_root_bytes(DIAGNOSTIC_LOSS_LIMIT_SOURCE, &arena, &policy)
                        .expect("root fits retained policy");

                (super::StepDecodeSession::new(
                    &exchange,
                    &diagnostics,
                    &ctx,
                    super::DecodeMode::Inspect,
                ))
                .map(|_| ())
            },
        );
        matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "step_decode_diagnostic_message")
    };
    assert!(
        refused,
        "no retained limit refused decode diagnostic message"
    );
}

#[test]
fn decode_reference_notes_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (exchange, diagnostics) = crate::test_support::with_service_context(
        REFERENCE_NOTE_LIMIT_SOURCE,
        crate::parse::parse_inner,
    )
    .expect("valid reference exchange");
    let arena = DecodeArena::new();
    let refused = (0..64).any(|limit| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(REFERENCE_NOTE_LIMIT_SOURCE, &arena, &policy)
            .expect("root fits collection policy");
        matches!(
            super::StepDecodeSession::new(&exchange, &diagnostics, &ctx, super::DecodeMode::Inspect),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "step_decode_reference_notes"
        )
    });
    assert!(refused, "no collection limit refused reference note");
}

#[test]
fn decode_reference_note_text_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (exchange, diagnostics) = crate::test_support::with_service_context(
        REFERENCE_NOTE_LIMIT_SOURCE,
        crate::parse::parse_inner,
    )
    .expect("valid reference exchange");
    let arena = DecodeArena::new();
    let refused = {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "step_decode_reference_note_text",
            |limit| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = limit;
                let (ctx, _) =
                    DecodeContext::from_root_bytes(REFERENCE_NOTE_LIMIT_SOURCE, &arena, &policy)
                        .expect("root fits retained policy");

                (super::StepDecodeSession::new(
                    &exchange,
                    &diagnostics,
                    &ctx,
                    super::DecodeMode::Inspect,
                ))
                .map(|_| ())
            },
        );
        matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "step_decode_reference_note_text")
    };
    assert!(refused, "no retained limit refused reference note text");
}

#[test]
fn byte_accounting_reports_an_unrecognized_suffix() {
    let input = include_bytes!("../../tests/fixtures/ap242_minimal.p21");
    let (mut exchange, _) =
        crate::test_support::with_service_context(input, crate::parse::parse_inner)
            .expect("parse accounting fixture");
    let mut extended = input.to_vec();
    extended.push(0xc3);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&extended, &arena, &policy)
        .expect("root fits the test policy");

    let accounting = byte_accounting(&extended, &exchange, &HashSet::new(), &[], &ctx)
        .expect("byte accounting allocation");

    assert_eq!(accounting.unclassified, 1);
    assert_eq!(
        accounting.structural + accounting.typed + accounting.opaque + accounting.unclassified,
        extended.len()
    );

    let result = decode_exchange_mode(
        &extended,
        &mut exchange,
        &[],
        super::DecodeMode::Decode(Packaging::Bare),
        &ctx,
    )
    .expect("synthesized unknown record conversion")
    .decoded;
    assert!(result.body.losses.iter().any(|loss| {
        loss.code == StepLossCode::ByteAccountingUnclassified.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Error
            && loss.message.contains("1 byte(s) unclassified")
    }));
}

#[test]
fn byte_accounting_claims_controls_inside_print_directives() {
    let input = b"1\\\x01N\x02\\2";
    let mut classes = vec![ByteClass::Unclassified; input.len()];

    claim_trivia(input, 1..input.len(), &mut classes, false)
        .expect("print directive fits the trivia range");

    assert!(classes[1..6]
        .iter()
        .all(|class| *class == ByteClass::Structural));
}

#[test]
fn byte_accounting_propagates_binary_lexeme_resource_refusal() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(\"0A1F2\");ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("test exchange parses");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index(source.len()) + 4;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("root fits the test policy");
    let error = byte_accounting(source, &exchange, &HashSet::new(), &[], &ctx)
        .expect_err("binary lexer must refuse before temporary digit allocation");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes && limit.operation == "step_binary_lexeme_temp")
    );
}

#[test]
fn semantic_work_counts_nested_source_graph_nodes() {
    let simple = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let nested = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(((1,2),TYPE((3,4))));ENDSEC;END-ISO-10303-21;";
    let (simple_exchange, _) =
        crate::test_support::with_service_context(simple, crate::parse::parse_inner)
            .expect("simple exchange");
    let (nested_exchange, _) =
        crate::test_support::with_service_context(nested, crate::parse::parse_inner)
            .expect("nested exchange");

    assert!(
        semantic_input_work(&nested_exchange).expect("nested work fits")
            > semantic_input_work(&simple_exchange).expect("simple work fits")
    );
}

#[test]
fn implicit_face_plane_work_scales_with_point_count() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=POLY_LOOP('',(#2,#3,#4,#5));#2=ITEM();#3=ITEM();#4=ITEM();#5=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("polygon exchange");

    assert_eq!(implicit_face_plane_work(&exchange).expect("work fits"), 4);
}

use std::fmt::Write as _;
use std::io::Cursor;

use cadmpeg_core::decode::DecodeMode;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::ids::CurveId;

use crate::test_support::exchange::decode_inline;
use crate::StepCodec;

#[test]
fn semantic_decode_uses_the_decode_session_work_budget() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let mut semantic_operation = None;
    for max_work_units in 1..=2048 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_work_units = max_work_units;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
                .expect("root fits the test policy");
        let error = crate::reader::decode(source, &ctx, Packaging::Bare)
            .expect_err("a small work budget must refuse one decode stage");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            continue;
        };
        if !matches!(
            limit.operation,
            "step_lex_token"
                | "step_string_boundary_scan"
                | "step_string_normalization"
                | "step_string_lexeme"
                | "step_schema_matching_name"
                | "step_parse_record"
                | "step_parse_parameter"
                | "step_anchor_materialization"
                | "step_reference_materialization"
                | "step_entity_name_admission"
        ) {
            semantic_operation = Some(limit.operation);
            break;
        }
    }
    assert_eq!(semantic_operation, Some("step_entity_index_name_storage"));
}

#[test]
fn semantic_decode_admits_ir_entities_at_stage_boundaries() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));ENDSEC;DATA;#1=CARTESIAN_POINT('',(1.,2.,3.));#2=VERTEX_POINT('',#1);ENDSEC;END-ISO-10303-21;";
    let mut entity_limit = None;
    for max_entities in 1..=64 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_entities = max_entities;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
                .expect("root fits the test policy");
        let error = crate::reader::decode(source, &ctx, Packaging::Bare)
            .expect_err("a model entity must be admitted before the next semantic stage");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            continue;
        };
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities
            && limit.operation == "step_dependency_decode"
        {
            entity_limit = Some(limit);
            break;
        }
    }
    let limit = entity_limit.expect("IR entities must be charged at a semantic boundary");
    assert_eq!(limit.additional, 1);
    assert!(limit.used <= limit.limit);
}

#[test]
fn implicit_face_plane_work_is_charged_before_plane_inference() {
    let point_references = (2..=17)
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(",");
    let point_records = (2..=17).fold(String::new(), |mut records, id| {
        writeln!(records, "#{id}=CARTESIAN_POINT('',({id}.,0.,0.));").expect("write point fixture");
        records
    });
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=POLY_LOOP('',({point_references}));{point_records}ENDSEC;END-ISO-10303-21;"
    );
    let mut plane_limit = None;
    for max_work_units in 1..=65_536 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_work_units = max_work_units;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            source.as_bytes(),
            &arena,
            &policy,
        )
        .expect("root fits the test policy");
        let error = crate::reader::decode(source.as_bytes(), &ctx, Packaging::Bare)
            .expect_err("bounded implicit-plane work must be refused at some budget");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            continue;
        };
        if limit.operation == "step_implicit_face_plane" {
            plane_limit = Some(limit);
            break;
        }
    }
    let limit = plane_limit.expect("implicit face-plane work must have a stable budget gate");
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(limit.additional, 16);
    assert!(limit.used <= limit.limit);
}

#[test]
pub(crate) fn decode_preserves_named_opaque_records_with_exact_byte_spans() {
    let bytes = include_bytes!("../../tests/fixtures/ap242_minimal.p21");
    let result = EditableDecodeResult::from(
        StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode parsed STEP document"),
    );

    assert_eq!(result.ir().source.as_ref().unwrap().format(), "step");
    let unknowns = result.ir().native_unknowns("step").unwrap();
    assert_eq!(unknowns.len(), 2);
    assert_eq!(unknowns[0].id.as_str(), "step:data:example_record#1");
    let retained = result
        .source_fidelity()
        .retained_record(unknowns[0].id.as_str())
        .expect("opaque payload is retained in source fidelity");
    assert_eq!(
        retained.data(),
        Some(
            &bytes[usize::try_from(retained.offset()).expect("retained offset fits memory")
                ..usize::try_from(retained.offset() + retained.byte_len())
                    .expect("retained end fits memory")],
        )
    );
    assert!(unknowns[0]
        .links
        .iter()
        .any(|link| link.as_str() == "step:data:opaque_target#2"));
    assert!(!result.report().geometry_transferred());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("EXAMPLE_RECORD")));
}

#[test]
fn decode_retains_signature_opaque_without_verification_result() {
    let bytes = include_bytes!("../signature/tests/data/sg04_openssl_detached.p21");
    let result = EditableDecodeResult::from(
        StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode signature witness"),
    );

    let signature = result
        .ir()
        .native_unknowns("step")
        .unwrap()
        .into_iter()
        .find(|record| record.id.as_str() == "step:file:signature#0")
        .expect("signature is retained as an opaque source record");
    let retained = result
        .source_fidelity()
        .retained_record(signature.id.as_str())
        .expect("signature source fidelity");
    assert_eq!(
        retained.data(),
        Some(
            &bytes[usize::try_from(retained.offset()).expect("retained offset fits memory")
                ..usize::try_from(retained.offset() + retained.byte_len())
                    .expect("retained end fits memory")],
        )
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::OpaqueRecordPreserved.kind()
            && loss.message.contains("SIGNATURE")
    }));
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message.contains("signature valid")
            || loss.message.contains("signature invalid")
            || loss.message.contains("signature indeterminate")
    }));
}

#[test]
fn decode_user_defined_entities_as_named_opaque_records() {
    let bytes = include_bytes!("tests/data/ud01_user_defined_entity.p21");
    let result = EditableDecodeResult::from(
        StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode user-defined entity witness"),
    );

    assert_eq!(result.ir().model.entity_count(), 0);
    let unknowns = result.ir().native_unknowns("step").unwrap();
    assert_eq!(unknowns.len(), 2);

    let target = unknowns
        .iter()
        .find(|record| record.id.as_str() == "step:data:!vendor_target#1")
        .expect("user-defined target record");
    assert!(target.links.is_empty());
    let target_source = result
        .source_fidelity()
        .retained_record(target.id.as_str())
        .expect("retained user-defined target span");
    assert_eq!(
        target_source.data(),
        Some(b"#1=!VENDOR_TARGET('target');".as_slice())
    );

    let entity = unknowns
        .iter()
        .find(|record| record.id.as_str() == "step:data:!vendor_entity#2")
        .expect("user-defined entity record");
    assert_eq!(
        entity
            .links
            .iter()
            .map(cadmpeg_ir::ids::Identity::as_str)
            .collect::<Vec<_>>(),
        vec!["step:data:!vendor_target#1".to_string()]
    );
    let entity_source = result
        .source_fidelity()
        .retained_record(entity.id.as_str())
        .expect("retained user-defined entity span");
    assert_eq!(
        entity_source.data(),
        Some(b"#2=!VENDOR_ENTITY('vendor payload',#1,!VENDOR_TYPE((#1)));".as_slice())
    );

    assert!(result.report().losses.iter().any(|loss| {
        loss.message
            .contains("!VENDOR_ENTITY instance(s) as named opaque STEP records")
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.findings.is_empty(), "{:#?}", validation.findings);
}

#[test]
fn opaque_links_retain_typed_step_targets() {
    let result = decode_inline(
        "#1=EXAMPLE_RECORD('',#2);
        #2=LINE('typed target',#3,#5);
        #3=CARTESIAN_POINT('',(0.,0.,0.));
        #4=DIRECTION('',(1.,0.,0.));
        #5=VECTOR('',#4,1.);",
    );
    let unknowns = result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown records");
    assert_eq!(unknowns.len(), 1);
    assert_eq!(
        unknowns[0]
            .links
            .iter()
            .map(cadmpeg_ir::ids::Identity::as_str)
            .collect::<Vec<_>>(),
        vec!["step:data:curve#2".to_string()]
    );
}

#[test]
fn opaque_links_retain_fallback_carrier_targets() {
    let result = decode_inline(
        "#1=TRIMMED_CURVE('',#99,(0.),(1.),.T.,.PARAMETER.);
         #2=EXAMPLE_RECORD('',#1);
         #99=EXAMPLE_RECORD('missing basis');",
    );
    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id.as_str() == "step:data:curve#1"));

    let unknowns = result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown records");
    let example = unknowns
        .iter()
        .find(|record| record.id.as_str() == "step:data:example_record#2")
        .expect("opaque record referencing fallback carrier");
    assert!(example
        .links
        .iter()
        .any(|link| link.as_str() == "step:data:curve#1"));

    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(!validation.findings.iter().any(|finding| {
        finding.check == cadmpeg_ir::report::check::Check::CarrierReachability
            && finding.entity.as_deref() == Some("step:data:curve#1")
    }));
}

#[test]
pub(crate) fn decode_accounts_for_every_part21_byte() {
    let bytes = include_bytes!("../../tests/fixtures/ap242_semantic_pmi.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode byte-accounting fixture");
    let attributes = &result.ir().source.as_ref().unwrap().attributes;
    let count = |name: &str| attributes[name].parse::<usize>().unwrap();

    assert!(count("bytes_structural") > 0);
    assert!(count("bytes_typed") > 0);
    assert_eq!(count("bytes_named_opaque"), 0);
    assert_eq!(count("bytes_unclassified"), 0);
    assert_eq!(
        count("bytes_structural")
            + count("bytes_typed")
            + count("bytes_named_opaque")
            + count("bytes_unclassified"),
        bytes.len()
    );
}

#[test]
fn every_repository_step_fixture_has_complete_byte_accounting() {
    let fixtures: &[(&str, &[u8])] = &[
        (
            "ap203_sheet",
            include_bytes!("../../tests/fixtures/ap203_sheet.p21"),
        ),
        (
            "ap214_sheet",
            include_bytes!("../../tests/fixtures/ap214_sheet.p21"),
        ),
        (
            "ap242_assembly",
            include_bytes!("../../tests/fixtures/ap242_assembly.p21"),
        ),
        (
            "ap242_conversion_units",
            include_bytes!("../../tests/fixtures/ap242_conversion_units.p21"),
        ),
        (
            "ap242_ed3_sections",
            include_bytes!("../../tests/fixtures/ap242_ed3_sections.p21"),
        ),
        (
            "ap242_degree_cone",
            include_bytes!("../../tests/fixtures/ap242_degree_cone.p21"),
        ),
        (
            "ap242_external_documents",
            include_bytes!("../../tests/fixtures/ap242_external_documents.p21"),
        ),
        (
            "ap242_geometry",
            include_bytes!("../../tests/fixtures/ap242_geometry.p21"),
        ),
        (
            "ap242_geometric_set",
            include_bytes!("../../tests/fixtures/ap242_geometric_set.p21"),
        ),
        (
            "ap242_mapped_assembly",
            include_bytes!("../../tests/fixtures/ap242_mapped_assembly.p21"),
        ),
        (
            "ap242_minimal",
            include_bytes!("../../tests/fixtures/ap242_minimal.p21"),
        ),
        (
            "ap242_presentation_pmi",
            include_bytes!("../../tests/fixtures/ap242_presentation_pmi.p21"),
        ),
        (
            "ap242_semantic_pmi",
            include_bytes!("../../tests/fixtures/ap242_semantic_pmi.p21"),
        ),
        (
            "ap242_tessellation",
            include_bytes!("../../tests/fixtures/ap242_tessellation.p21"),
        ),
        (
            "ap242_vertex_loop",
            include_bytes!("../../tests/fixtures/ap242_vertex_loop.p21"),
        ),
        (
            "complex_instance",
            include_bytes!("../../tests/fixtures/complex_instance.p21"),
        ),
        (
            "strings",
            include_bytes!("../../tests/fixtures/strings.p21"),
        ),
    ];
    for &(name, bytes) in fixtures {
        let result = StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let attributes = &result.ir().source.as_ref().unwrap().attributes;
        let count = |key: &str| attributes[key].parse::<usize>().unwrap();
        assert_eq!(count("bytes_unclassified"), 0, "{name}");
        assert_eq!(
            count("bytes_structural")
                + count("bytes_typed")
                + count("bytes_named_opaque")
                + count("bytes_unclassified"),
            bytes.len(),
            "{name}"
        );
    }
}

#[test]
fn unowned_pcurve_dependencies_are_retained_as_one_opaque_closure() {
    let source = String::from_utf8(include_bytes!("../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#69=PCURVE('',#28,#70);\n#70=DEFINITIONAL_REPRESENTATION('',(#71),#50);\n#71=LINE('',#51,#53);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = EditableDecodeResult::from(
        StepCodec::default()
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("decode unowned pcurve"),
    );
    let unknowns = decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena");
    assert!(unknowns
        .iter()
        .any(|record| record.id.as_str() == "step:data:pcurve#69"));
    assert!(unknowns
        .iter()
        .any(|record| record.id.as_str() == "step:data:definitional_representation#70"));
    let line = unknowns
        .iter()
        .find(|record| record.id.as_str() == "step:data:line#71")
        .expect("unowned pcurve line is retained");
    assert!(decoded
        .source_fidelity()
        .retained_record(line.id.as_str())
        .expect("unowned pcurve line payload is retained")
        .data()
        .is_some_and(|data| data.starts_with(b"#71=LINE")));
    assert!(decoded
        .ir()
        .model
        .pcurves
        .iter()
        .all(|pcurve| pcurve.id.as_str() != "step:data:pcurve#69"));
}

#[test]
fn a_protected_unowned_pcurve_stays_opaque() {
    let source = String::from_utf8(include_bytes!("../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#19=EDGE_CURVE('',#6,#7,#57,.T.);",
            "#19=EDGE_CURVE('',#6,#7,#72,.T.);",
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#69=PCURVE('',#28,#55);\n#72=TRIMMED_CURVE('',#16,(#69,0.),(#69,10.),.T.,.PARAMETER.);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode protected unowned pcurve");
    assert!(decoded
        .ir()
        .model
        .pcurves
        .iter()
        .all(|pcurve| { pcurve.id.as_str() != "step:data:pcurve#69" }));
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("protected_pcurves=1")));
    let unknowns = decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena");
    assert!(unknowns
        .iter()
        .any(|record| record.id.as_str() == "step:data:pcurve#69"));
}

#[test]
fn failed_mandatory_point_root_remains_opaque_and_unbound() {
    let source = String::from_utf8(include_bytes!("../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#3=CARTESIAN_POINT('',(0.,0.,0.));",
            "#3=UNSUPPORTED_POINT('',(0.,0.,0.));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode source with unsupported mandatory vertex point");

    assert!(decoded.ir().model.bodies.is_empty());
    let unknowns = decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena");
    assert!(unknowns
        .iter()
        .any(|record| record.id.as_str() == "step:data:unsupported_point#3"));
    assert!(unknowns
        .iter()
        .any(|record| record.id.as_str() == "step:data:shell_based_surface_model#31"));
    assert!(decoded.report().losses.iter().any(|loss| loss
        .message
        .contains("STEP topology root #31 rejected: vertex point #3")));
}

#[test]
fn unsupported_invisibility_relation_is_retained_as_opaque() {
    let decoded = decode_inline(
        "#1=INVISIBILITY((#2));
         #2=STYLED_ITEM('',(),#3);
         #3=GEOMETRIC_CURVE_SET('',());",
    );

    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.as_str() == "step:data:invisibility#1"));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("INVISIBILITY #1 targets unsupported item #2")
    }));
}

#[test]
fn retention_reports_every_deleted_carrier_category() {
    let source = String::from_utf8(include_bytes!("../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#69=PCURVE('',#78,#70);\n#70=DEFINITIONAL_REPRESENTATION('',(#71,#84),#50);\n#71=LINE('',#51,#53);\n#74=CARTESIAN_POINT('',(20.,20.,0.));\n#75=DIRECTION('',(0.,0.,1.));\n#76=DIRECTION('',(1.,0.,0.));\n#77=AXIS2_PLACEMENT_3D('',#74,#75,#76);\n#78=PLANE('',#77);\n#79=DIRECTION('',(1.,0.,0.));\n#80=VECTOR('',#79,1.);\n#83=LINE('',#74,#80);\n#84=OPAQUE_REFERENCE(#83,#86);\n#86=POLY_LOOP('',(#74));\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode carrier retention fixture");
    let message = decoded
        .report()
        .losses
        .iter()
        .find(|loss| loss.message.contains("unowned STEP carrier retention"))
        .map(|loss| loss.message.as_str())
        .expect("carrier retention report");
    for category in ["deleted pcurves=1", "points=1", "curves=1", "surfaces=1"] {
        assert!(message.contains(category), "missing {category}: {message}");
    }
    assert!(decoded
        .ir()
        .model
        .pcurves
        .iter()
        .all(|pcurve| pcurve.id.as_str() != "step:data:pcurve#69"));
    assert!(decoded
        .ir()
        .model
        .curves
        .iter()
        .all(|curve| curve.id.as_str() != "step:data:curve#83"));
    assert!(decoded
        .ir()
        .model
        .surfaces
        .iter()
        .all(|surface| surface.id.as_str() != "step:data:surface#78"));
    assert!(decoded
        .ir()
        .model
        .points
        .iter()
        .all(|point| point.id.as_str() != "step:data:point#74"));
}

#[test]
fn decode_charges_one_loss_for_an_out_of_range_schema_object_identifier() {
    let source = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AUTOMOTIVE_DESIGN_CC2 { 1 2 10303 214 -1 1 5 4 }'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("an out-of-range component does not refuse the file");

    let losses = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == StepLossCode::SchemaObjectIdentifierOutOfRange.kind())
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Warning);
    assert_eq!(
        losses[0].message,
        "FILE_SCHEMA identifier AUTOMOTIVE_DESIGN_CC2 has an out-of-range object identifier component -1; the object identifier is not admitted"
    );
    let provenance = losses[0].provenance.as_ref().expect("source provenance");
    assert_eq!(wire::field::<String>(&provenance, "format"), "step");
    assert_eq!(
        provenance.offset,
        cadmpeg_core::decode::u64_from_index(source.find("FILE_SCHEMA").unwrap())
    );
    assert_eq!(provenance.tag.as_deref(), Some("schema_identifier"));
    assert_eq!(
        result.ir().source.as_ref().unwrap().attributes["schema"],
        "AUTOMOTIVE_DESIGN_CC2 { 1 2 10303 214 -1 1 5 4 }"
    );
}

#[test]
fn decode_does_not_charge_a_loss_for_a_valid_schema_object_identifier() {
    let source = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AUTOMOTIVE_DESIGN_CC2 { 1 2 10303 214 1 1 5 4 }'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("valid schema object identifier");
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::SchemaObjectIdentifierOutOfRange.kind()));
}

#[test]
fn decode_reports_the_substituted_grammar_for_an_unknown_implementation_level() {
    let source = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'1;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("the framed implementation-level declaration is recoverable");

    let losses = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == StepLossCode::ImplementationLevelUnverified.kind())
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("1;1"));
    assert!(losses[0].message.contains("4;3 grammar"));
    assert_eq!(
        losses[0].code.strict_floor(),
        Some(cadmpeg_ir::report::Severity::Warning)
    );
    let provenance = losses[0].provenance.as_ref().expect("source provenance");
    assert_eq!(
        provenance.offset,
        cadmpeg_core::decode::u64_from_index(source.find("FILE_DESCRIPTION").unwrap())
    );
    assert_eq!(provenance.tag.as_deref(), Some("implementation_level"));
}

#[test]
fn strict_decode_rejects_an_unverified_implementation_level() {
    let source = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'1;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let mut options = DecodeOptions::default();
    options.policy.mode = DecodeMode::Strict;
    let error = StepCodec::default()
        .decode(&mut Cursor::new(source), &options)
        .expect_err("strict mode rejects a guessed implementation grammar");
    assert!(matches!(
        error,
        cadmpeg_ir::codec::DecodeFailure::StrictRejected { .. }
    ));
    assert!(error
        .to_string()
        .contains("parse.implementation-level-unverified"));
}

#[test]
fn decode_salvages_noncanonical_complex_partial_order_with_provenance() {
    let bytes = include_bytes!("../../tests/fixtures/noncanonical_solid_angle.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("salvage mode accepts recoverable source order");
    let losses = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == StepLossCode::ParseNoncanonicalSyntax.kind())
        .collect::<Vec<_>>();

    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Warning);
    let provenance = losses[0].provenance.as_ref().expect("source provenance");
    assert_eq!(wire::field::<String>(&provenance, "format"), "step");
    assert_eq!(
        wire::field_or_default::<Option<String>>(&provenance, "stream").as_deref(),
        None
    );
    assert_eq!(
        provenance.offset,
        cadmpeg_core::decode::u64_from_index(
            bytes.windows(2).position(|window| window == b"#1").unwrap()
        )
    );
    assert_eq!(provenance.tag.as_deref(), Some("complex_entity"));
    assert_eq!(result.ir().native_unknowns("step").unwrap().len(), 0);
    assert_eq!(
        result.ir().source.as_ref().unwrap().attributes["bytes_named_opaque"],
        "0"
    );
}

#[test]
fn strict_decode_rejects_noncanonical_complex_partial_order() {
    let bytes = include_bytes!("../../tests/fixtures/noncanonical_solid_angle.p21");
    let mut options = DecodeOptions::default();
    options.policy.mode = DecodeMode::Strict;
    let error = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &options)
        .expect_err("strict mode rejects noncanonical source order");

    assert!(matches!(
        error,
        cadmpeg_ir::codec::DecodeFailure::StrictRejected { .. }
    ));
}

#[test]
fn strict_decode_rejects_omitted_entity_name_recovery() {
    // The schema is a declared dialect so that the refusal this test asserts is
    // the omitted-name one. A bare 'AP242' declares no registry row and would
    // refuse first on `source.dialect-unverified`.
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));ENDSEC;DATA;#1=CARTESIAN_POINT((0.,0.,0.));ENDSEC;END-ISO-10303-21;";
    let mut options = DecodeOptions::default();
    options.policy.mode = DecodeMode::Strict;
    let error = StepCodec::default()
        .decode(&mut Cursor::new(source), &options)
        .expect_err("strict mode rejects omitted-name recovery");

    assert!(matches!(
        error,
        cadmpeg_ir::codec::DecodeFailure::StrictRejected { .. }
    ));
    assert!(error.to_string().contains("parse.noncanonical-syntax"));
}

#[test]
fn omitted_geometry_names_preserve_intersection_curve_topology() {
    let mut source =
        String::from_utf8(include_bytes!("../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));",
                "#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#69)) REPRESENTATION_CONTEXT('model','3D'));",
            )
            .replace(
                "#57=SURFACE_CURVE('',#16,(#56),.PCURVE_S1.);",
                "#57=INTERSECTION_CURVE(#16,(#56),.PCURVE_S1.);",
            )
            .replace(
                "ENDSEC;\nEND-ISO-10303-21;",
                "#69=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\nENDSEC;\nEND-ISO-10303-21;",
            );
    for (id, entity) in [
        ("3", "CARTESIAN_POINT"),
        ("4", "CARTESIAN_POINT"),
        ("5", "CARTESIAN_POINT"),
        ("6", "VERTEX_POINT"),
        ("7", "VERTEX_POINT"),
        ("8", "VERTEX_POINT"),
        ("9", "DIRECTION"),
        ("10", "DIRECTION"),
        ("11", "DIRECTION"),
        ("12", "DIRECTION"),
        ("13", "VECTOR"),
        ("14", "VECTOR"),
        ("15", "VECTOR"),
        ("16", "LINE"),
        ("17", "LINE"),
        ("18", "LINE"),
        ("19", "EDGE_CURVE"),
        ("20", "EDGE_CURVE"),
        ("21", "EDGE_CURVE"),
        ("22", "ORIENTED_EDGE"),
        ("23", "ORIENTED_EDGE"),
        ("24", "ORIENTED_EDGE"),
        ("25", "EDGE_LOOP"),
        ("26", "FACE_OUTER_BOUND"),
        ("27", "AXIS2_PLACEMENT_3D"),
        ("28", "PLANE"),
        ("29", "ADVANCED_FACE"),
        ("30", "OPEN_SHELL"),
        ("31", "SHELL_BASED_SURFACE_MODEL"),
        ("33", "ORIENTED_OPEN_SHELL"),
        ("51", "CARTESIAN_POINT"),
        ("52", "DIRECTION"),
        ("53", "VECTOR"),
        ("54", "LINE"),
        ("55", "DEFINITIONAL_REPRESENTATION"),
        ("56", "PCURVE"),
    ] {
        let named = format!("#{id}={entity}('',");
        let unnamed = format!("#{id}={entity}(");
        let previous_len = source.len();
        source = source.replace(&named, &unnamed);
        assert!(
            source.len() < previous_len,
            "fixture record #{id} was not converted to omitted-name syntax"
        );
    }

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode omitted-name intersection curve");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    let edge = decoded
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| edge.id.as_str() == "step:data:edge#19")
        .expect("omitted-name intersection edge");
    assert_eq!(
        edge.curve().map(CurveId::as_str),
        Some("step:data:curve#16")
    );
    assert!(decoded.ir().model.coedges.iter().any(|coedge| {
        coedge
            .pcurves
            .iter()
            .any(|use_| use_.pcurve.as_str() == "step:data:pcurve#56")
    }));
    let name_loss = decoded
        .report()
        .losses
        .iter()
        .find(|loss| {
            loss.code == StepLossCode::ParseNoncanonicalSyntax.kind()
                && loss
                    .message
                    .contains("recovered 37 simple named carrier instance(s)")
        })
        .expect("omitted-name recovery loss");
    assert_eq!(
        name_loss
            .provenance
            .as_ref()
            .and_then(|provenance| provenance.tag.as_deref()),
        Some("entity_name")
    );
    assert!(decoded.report().losses.iter().all(|loss| {
        !loss
            .message
            .contains("INTERSECTION_CURVE #57 has no decoded 3D curve")
    }));

    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn step_source_ids_keep_the_hash_prefixed_spelling() {
    for id in [0, 1, 42, u64::MAX] {
        assert_eq!(super::step_source_id(id).as_str(), format!("#{id}"));
    }
}

#[test]
fn reference_walk_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeSet;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"reference", &arena, &policy)
        .expect("root fits collection policy");
    let error = super::collect_references(
        &crate::parse::Value::Reference(7),
        &mut BTreeSet::new(),
        &ctx,
    )
    .expect_err("reference needs one set item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "step_reference_walk_ids"));
}

#[test]
fn reference_walk_refuses_depth_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeSet;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(b"reference", &arena, &policy)
        .expect("root fits depth policy");
    let error = super::collect_references(
        &crate::parse::Value::List(vec![crate::parse::Value::Reference(7)]),
        &mut BTreeSet::new(),
        &ctx,
    )
    .expect_err("nested reference needs another depth level");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RecursionDepth
            && limit.operation == "step_reference_walk"));
}

#[test]
fn record_closure_pending_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("root fits collection policy");
    let error =
        super::record_closure([1], &exchange, &ctx).expect_err("pending root needs one item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "step_record_closure_pending"));
}

#[test]
fn record_closure_ids_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("root fits collection policy");
    let error = super::record_closure([1], &exchange, &ctx)
        .expect_err("closure needs one item after the pending root");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "step_record_closure_ids"));
}

#[test]
fn opaque_kind_name_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 3;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("root fits retained policy");
    let error = super::opaque_record_id(1, &exchange.records()[&1], &ctx)
        .expect_err("four-byte kind needs more retained bytes");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "step_opaque_kind_name"));
}

#[test]
fn opaque_identity_text_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 4;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("root fits retained policy");
    let error = super::opaque_record_id(1, &exchange.records()[&1], &ctx)
        .expect_err("the identity text needs more than the kind name");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "step_opaque_identity_text"));
}

#[test]
fn opaque_target_map_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let decoded = crate::test_support::exchange::decode_inline(
        "#1=EXAMPLE_RECORD('',#2);#2=LINE('target',#3,#5);#3=CARTESIAN_POINT('',(0.,0.,0.));#4=DIRECTION('',(1.,0.,0.));#5=VECTOR('',#4,1.);",
    );
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "step_opaque_target_records",
        |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (ctx, _) =
                DecodeContext::from_root_bytes(b"target", &arena, &policy).expect("root");
            super::record_targets(decoded.ir(), |id| id == 2, &ctx)
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "step_opaque_target_records"));
}

#[test]
fn opaque_kind_count_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeMap;

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("root fits collection policy");
    let error = super::count_unknown_kind(&mut BTreeMap::new(), &exchange.records()[&1], &ctx)
        .expect_err("new kind needs one map item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "step_opaque_kind_counts"));
}

#[test]
fn opaque_record_collections_refuse_caller_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeSet;

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=EXAMPLE_RECORD(#2);#2=EXAMPLE_RECORD();ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid opaque exchange");
    let arena = DecodeArena::new();
    let mut observed = BTreeSet::new();
    for limit in 0..1024 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
            .expect("root fits collection policy");
        if let Err(CodecError::ResourceLimit(refusal)) = super::decode_exchange_mode(
            source,
            &mut exchange.clone(),
            &diagnostics,
            super::DecodeMode::Decode(Packaging::Bare),
            &ctx,
        ) {
            if refusal.dimension == ResourceDimension::CollectionItems {
                observed.insert(refusal.operation);
            }
        }
        if [
            "step_opaque_ids",
            "step_opaque_sources",
            "step_opaque_records",
            "step_opaque_links",
        ]
        .iter()
        .all(|operation| observed.contains(operation))
        {
            break;
        }
    }
    for operation in [
        "step_opaque_ids",
        "step_opaque_sources",
        "step_opaque_records",
        "step_opaque_links",
    ] {
        assert!(
            observed.contains(operation),
            "missing refusal at {operation}; observed {observed:?}"
        );
    }
}

fn stage_refuses_at_collection_limit(
    operation: &str,
    make_stage: impl Fn() -> super::StageOutcome<()>,
) -> bool {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid exchange");
    let arena = DecodeArena::new();
    for limit in 0..128 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
            .expect("root fits collection policy");
        let Ok(mut session) = super::StepDecodeSession::new(
            &exchange,
            &diagnostics,
            &ctx,
            super::DecodeMode::Inspect,
        ) else {
            continue;
        };
        let mut stage = make_stage();
        if matches!(
            session.absorb(&mut stage),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == operation
        ) {
            return true;
        }
    }
    false
}

#[test]
fn stage_claims_refuse_collection_limit() {
    assert!(stage_refuses_at_collection_limit(
        "step_stage_claims",
        || super::StageOutcome {
            value: (),
            claims: HashSet::from([1]),
            losses: Vec::new(),
            notes: Vec::new(),
        }
    ));
}

#[test]
fn stage_losses_refuse_collection_limit() {
    assert!(stage_refuses_at_collection_limit(
        "step_stage_losses",
        || super::StageOutcome {
            value: (),
            claims: HashSet::new(),
            losses: vec![StepLossCode::DecodeWarning.note("test")],
            notes: Vec::new(),
        }
    ));
}

#[test]
fn stage_notes_refuse_collection_limit() {
    assert!(stage_refuses_at_collection_limit(
        "step_stage_notes",
        || super::StageOutcome {
            value: (),
            claims: HashSet::new(),
            losses: Vec::new(),
            notes: vec!["stage note".into()],
        }
    ));
}

#[test]
fn byte_accounting_note_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=EXAMPLE_RECORD();ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid exchange");
    let arena = DecodeArena::new();
    let refused = (0..1024).any(|limit| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
            .expect("root fits collection policy");
        matches!(
            super::decode_exchange_mode(
                source,
                &mut exchange.clone(),
                &diagnostics,
                super::DecodeMode::Inspect,
                &ctx,
            ),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "step_byte_accounting_note"
        )
    });
    assert!(refused, "byte accounting note must charge its vector item");
}

#[test]
fn opaque_preservation_loss_text_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=EXAMPLE_RECORD();ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid exchange");
    let arena = DecodeArena::new();
    let refused = {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "step_opaque_preservation_loss_text",
            |limit| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = limit;
                let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
                    .expect("root fits retained policy");

                (super::decode_exchange_mode(
                    source,
                    &mut exchange.clone(),
                    &diagnostics,
                    super::DecodeMode::Inspect,
                    &ctx,
                ))
                .map(|_| ())
            },
        );
        matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "step_opaque_preservation_loss_text")
    };
    assert!(refused, "opaque loss text must charge retained bytes");
}

#[test]
fn dialect_match_copy_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeSet;
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid exchange");
    let arena = DecodeArena::new();
    let refused = (0..256).any(|limit| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
            .expect("root fits collection policy");
        let Ok(session) = super::StepDecodeSession::new(
            &exchange,
            &diagnostics,
            &ctx,
            super::DecodeMode::Inspect,
        ) else {
            return false;
        };
        matches!(
            session.into_result(cadmpeg_ir::SourceFidelity::default(), BTreeSet::new()),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "copy STEP dialect layer"
        )
    });
    assert!(refused, "dialect declaration copy must charge each item");
}

#[test]
fn dialect_match_copy_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeSet;
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid exchange");
    let arena = DecodeArena::new();
    let refused = {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "copy STEP dialect layer",
            |limit| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = limit;
                let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
                    .expect("root fits retained policy");
                let session = super::StepDecodeSession::new(
                    &exchange,
                    &diagnostics,
                    &ctx,
                    super::DecodeMode::Inspect,
                )?;

                (session.into_result(cadmpeg_ir::SourceFidelity::default(), BTreeSet::new()))
                    .map(|_| ())
            },
        );
        matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "copy STEP dialect layer")
    };
    assert!(
        refused,
        "dialect declaration copy must charge retained text"
    );
}

#[test]
fn owned_pcurve_identity_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeSet;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"pcurve", &arena, &policy)
        .expect("root fits collection policy");
    let error = super::insert_retained_identity(&mut BTreeSet::new(), "step:data:pcurve#1", &ctx)
        .expect_err("owned ID needs one set item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "step_owned_pcurve_ids"));
}

#[test]
fn owned_pcurve_identity_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeSet;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(b"pcurve", &arena, &policy)
        .expect("root fits retained policy");
    let error = super::insert_retained_identity(&mut BTreeSet::new(), "step:data:pcurve#1", &ctx)
        .expect_err("owned identity text exceeds three bytes");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "step_owned_pcurve_identity"));
}

#[test]
fn unowned_pcurve_set_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=PCURVE('',#2,#3);#2=ITEM();#3=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid exchange");
    let mut ir = cadmpeg_ir::CadIr::empty();
    let carrier_index = crate::test_support::with_service_context(b"", |_, ctx| {
        super::index::CarrierIndex::from_ir(&ir, ctx)
    })
    .expect("carrier index");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("root fits collection policy");
    let error = super::retain_unowned_carriers(
        &exchange,
        &mut ir,
        &carrier_index,
        &mut HashSet::new(),
        &mut Vec::new(),
        &ctx,
    )
    .expect_err("unowned pcurve needs one set item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "step_unowned_pcurves"));
}

fn point_ir(with_source: bool) -> cadmpeg_ir::CadIr {
    let mut ir = cadmpeg_ir::CadIr::empty();
    let identity =
        cadmpeg_ir::ids::Identity::new("step:data:point#1").expect("valid point identity");
    let point = cadmpeg_ir::topology::Point::new(
        cadmpeg_ir::ids::PointId::from(identity),
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0))
            .expect("finite point"),
        with_source.then(|| super::step_source_association(1, None)),
    );
    ir.model.points.push(point);
    ir
}

#[test]
fn unowned_direct_carriers_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT('',(0.,0.,0.));ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid exchange");
    let mut ir = point_ir(false);
    let carrier_index = crate::test_support::with_service_context(b"", |_, ctx| {
        super::index::CarrierIndex::from_ir(&ir, ctx)
    })
    .expect("carrier index");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("root fits collection policy");
    let error = super::retain_unowned_carriers(
        &exchange,
        &mut ir,
        &carrier_index,
        &mut HashSet::new(),
        &mut Vec::new(),
        &ctx,
    )
    .expect_err("free point needs one carrier set item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "step_unowned_direct_carriers"));
}

#[test]
fn protected_root_walk_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT('',(0.,0.,0.));#2=PCURVE('',#3,#4);#3=ITEM();#4=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid exchange");
    let mut ir = point_ir(true);
    let carrier_index = crate::test_support::with_service_context(b"", |_, ctx| {
        super::index::CarrierIndex::from_ir(&ir, ctx)
    })
    .expect("carrier index");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("root fits collection policy");
    let error = super::retain_unowned_carriers(
        &exchange,
        &mut ir,
        &carrier_index,
        &mut HashSet::new(),
        &mut Vec::new(),
        &ctx,
    )
    .expect_err("protected root needs a pending slot");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "step_record_closure_pending"));
}

#[test]
fn byte_coverage_does_not_spend_collection_items_per_byte() {
    let source = vec![b' '; 100_000];
    let (exchange, _) = crate::test_support::with_service_context(
        b"ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;ENDSEC;END-ISO-10303-21;",
        crate::parse::parse_inner,
    )
    .expect("empty exchange");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&source, &arena, &policy)
        .expect("context");
    let counts =
        byte_accounting(&source, &exchange, &HashSet::new(), &[], &ctx).expect("byte budget");
    assert_eq!(counts.structural, source.len());
    assert_eq!(counts.unclassified, 0);
}

mod retention;
