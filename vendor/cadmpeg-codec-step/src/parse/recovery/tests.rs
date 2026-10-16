// SPDX-License-Identifier: Apache-2.0
//! Record admission, dependency closure, and exact omitted-source retention.

use std::io::Cursor;

use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::loss::StepLossCode;
use crate::parse::{parse_inner, ParseDiagnosticKind};
use crate::test_support::{with_policy_context, with_service_context};
use crate::StepCodec;

fn exchange(data: &str) -> String {
    format!("ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;{data}ENDSEC;END-ISO-10303-21;")
}

#[test]
fn bounded_defects_preserve_independent_instances_and_exact_bytes() {
    for defective in [
        "#2=ITEM(#0);",
        "#0=ITEM();",
        "#2=ITEM(1.E999);",
        "#2=ITEM(%,3);",
        "#2=ITEM('literal;semicolon', /* ; */ ,3);",
        "#2=ITEM('literal'\n';semicolon',,3);",
        "#2=ITEM_ENDSEC;",
        "#2=ENDSEC\n_NAME;",
        "#2=(A()A());",
        "#2=ITEM(#12BROKEN(3));",
    ] {
        let source = exchange(&format!(
            "#1=CARTESIAN_POINT('',(1.,2.,3.));{defective}#3=CARTESIAN_POINT('',(4.,5.,6.));"
        ));
        let (parsed, diagnostics) = with_service_context(source.as_bytes(), parse_inner)
            .expect("explicit statement boundaries permit recovery");
        assert_eq!(parsed.records().keys().copied().collect::<Vec<_>>(), [1, 3]);
        assert_eq!(parsed.omitted_spans().len(), 1);
        assert_eq!(&source[parsed.omitted_spans()[0].clone()], defective);
        assert!(diagnostics
            .iter()
            .any(|item| item.kind == ParseDiagnosticKind::RecordOmitted));
        let decoded_source = source.replace(
            "ENDSEC;END-ISO",
            "#4=GEOMETRIC_SET('',(#1,#3));ENDSEC;END-ISO",
        );
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(&decoded_source), &DecodeOptions::default())
            .expect("independent points decode");
        assert_eq!(decoded.ir().model.points.len(), 2);
        assert!(decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == StepLossCode::ParseRecordOmitted.kind()));
        let (ir, _, fidelity) = decoded.into_parts();
        let unknowns = ir.native_unknowns("step").expect("retained source");
        let omitted = unknowns
            .iter()
            .find(|record| record.id.as_str().starts_with("step:file:omitted#"))
            .expect("bounded defect retained");
        assert_eq!(
            fidelity
                .retained_record(omitted.id.as_str())
                .expect("fidelity payload")
                .data(),
            Some(defective.as_bytes())
        );
        assert_eq!(
            ir.source.as_ref().expect("source").attributes["bytes_unclassified"],
            "0"
        );
    }
}

#[test]
fn dangling_references_remove_transitive_dependents_without_removing_cycles() {
    let source =
        exchange("#1=ITEM(#90);#2=ITEM(#1);#3=ITEM((#2));#4=ITEM(#5);#5=ITEM(#4);#6=ITEM();");
    let (parsed, diagnostics) =
        with_service_context(source.as_bytes(), parse_inner).expect("bounded graph");
    assert_eq!(
        parsed.records().keys().copied().collect::<Vec<_>>(),
        [4, 5, 6]
    );
    assert_eq!(
        parsed
            .records()
            .iter()
            .filter(|(_, record)| parsed.data()[0].span.contains(&record.span.start))
            .map(|(&id, _)| id)
            .collect::<Vec<_>>(),
        [4, 5, 6]
    );
    assert_eq!(parsed.omitted_spans().len(), 3);
    assert_eq!(
        diagnostics
            .iter()
            .filter(|item| item.kind == ParseDiagnosticKind::RecordOmitted)
            .count(),
        3
    );
}

#[test]
fn duplicate_id_removes_every_definition_and_its_users() {
    let source = exchange("#1=ITEM();#2=ITEM(#1);#1=OTHER();#1=THIRD();#3=ITEM();");
    let (parsed, _) =
        with_service_context(source.as_bytes(), parse_inner).expect("bounded duplicates");
    assert_eq!(parsed.records().keys().copied().collect::<Vec<_>>(), [3]);
    assert_eq!(
        parsed
            .records()
            .iter()
            .filter(|(_, record)| parsed.data()[0].span.contains(&record.span.start))
            .map(|(&id, _)| id)
            .collect::<Vec<_>>(),
        [3]
    );
    assert_eq!(parsed.omitted_spans().len(), 4);
}

#[test]
fn malformed_definition_keeps_a_readable_instance_name_ambiguous() {
    for definitions in [
        "#1=ITEM();#1=ITEM(%,3);",
        "#1=ITEM(%,3);#1=ITEM();",
        "#1=ITEM(%,3);#1=ITEM();#1=OTHER();",
    ] {
        let source = exchange(&format!("{definitions}#2=ITEM(#1);#3=ITEM();"));
        let (parsed, diagnostics) =
            with_service_context(source.as_bytes(), parse_inner).expect("bounded definitions");
        assert_eq!(parsed.records().keys().copied().collect::<Vec<_>>(), [3]);
        assert_eq!(
            parsed
                .records()
                .iter()
                .filter(|(_, record)| parsed.data()[0].span.contains(&record.span.start))
                .map(|(&id, _)| id)
                .collect::<Vec<_>>(),
            [3]
        );
        assert_eq!(
            parsed.omitted_spans().len(),
            definitions.matches(';').count() + 1
        );
        assert!(diagnostics
            .iter()
            .any(|item| item.message.contains("unresolved instance reference #1")));
    }
}

#[test]
fn unterminated_literals_and_comments_remain_structural_refusals() {
    for data in ["#1=ITEM('open;#2=ITEM();", "#1=ITEM(/* open;#2=ITEM();"] {
        let source = exchange(data);
        let error = with_service_context(source.as_bytes(), parse_inner)
            .expect_err("a literal has no known continuation boundary");
        assert!(error.to_string().contains("cannot establish a boundary"));
    }
}

#[test]
fn eof_after_complete_records_keeps_the_population() {
    for ending in ["", "ENDSEC", "ENDSEC;", "ENDSEC;END-ISO-10303-21"] {
        let source =
            format!("ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();{ending}");
        let (parsed, diagnostics) =
            with_service_context(source.as_bytes(), parse_inner).expect("record closes before EOF");
        assert_eq!(parsed.records().len(), 1);
        assert!(diagnostics
            .iter()
            .any(|item| item.kind == ParseDiagnosticKind::EnvelopeIncomplete));
    }
}

#[test]
fn a_missing_semicolon_does_not_consume_the_next_assignment_or_section_end() {
    for defective in ["#2=ITEM(3)", "#2=ITEM(3"] {
        let source = exchange(&format!("{defective} #3=ITEM();"));
        let (parsed, _) = with_service_context(source.as_bytes(), parse_inner)
            .expect("next assignment supplies an explicit boundary");
        assert_eq!(parsed.records().keys().copied().collect::<Vec<_>>(), [3]);
        assert_eq!(source[parsed.omitted_spans()[0].clone()].trim(), defective);
        let source = exchange(&format!("{defective} "));
        let (parsed, _) = with_service_context(source.as_bytes(), parse_inner)
            .expect("section end supplies an explicit boundary");
        assert!(parsed.records().is_empty());
        assert_eq!(source[parsed.omitted_spans()[0].clone()].trim(), defective);
        let source = source.replace("ENDSEC;END-ISO", "E\nNdSeC;END-ISO");
        let (parsed, diagnostics) = with_service_context(source.as_bytes(), parse_inner)
            .expect("ignored controls preserve the section boundary");
        assert!(parsed.records().is_empty());
        assert_eq!(source[parsed.omitted_spans()[0].clone()].trim(), defective);
        assert!(!diagnostics
            .iter()
            .any(|item| item.kind == ParseDiagnosticKind::EnvelopeIncomplete));
    }
}

#[test]
fn draft_envelope_uses_declared_recovery_grammar_and_retains_header() {
    let source = "!* leading comment *!STEP;HEADER;FILE_IDENTIFICATION('synthetic');IMP_LEVEL('1.0');ENDSEC;DATA;!* data comment *!@1=CARTESIAN_POINT('',(1.,2.,3.));@2=ITEM(#1);@3=GEOMETRIC_SET('',(#1));ENDSEC;ENDSTEP;";
    let (parsed, diagnostics) =
        with_service_context(source.as_bytes(), parse_inner).expect("draft grammar");
    assert_eq!(parsed.records().len(), 3);
    assert!(parsed.schema_identifiers().next().is_none());
    assert!(diagnostics
        .iter()
        .any(|item| item.kind == ParseDiagnosticKind::DraftExchangeGrammar));
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("draft point decoded");
    assert_eq!(decoded.ir().model.points.len(), 1);
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::ParseDraftGrammar.kind()));
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("retained header")
        .iter()
        .any(|record| record.id.as_str().starts_with("step:file:header#")));
}

#[test]
fn usable_schema_identifiers_survive_invalid_entries_and_duplicates() {
    let source = exchange("#1=ITEM();").replace(
        "('AP242')",
        "(17,'bad name','AUTOMOTIVE_DESIGN','automotive_design')",
    );
    let (parsed, diagnostics) =
        with_service_context(source.as_bytes(), parse_inner).expect("usable schema");
    assert_eq!(
        parsed.schema_identifiers().collect::<Vec<_>>(),
        ["AUTOMOTIVE_DESIGN"]
    );
    assert!(diagnostics
        .iter()
        .any(|item| item.message.contains("duplicate excluded")));
}

#[test]
fn recovery_never_turns_a_resource_refusal_into_an_omission() {
    let source = exchange("#1=ITEM();#2=ITEM();");
    let mut policy = DecodePolicy::desktop();
    policy.limits.max_entities = 1;
    with_policy_context(source.as_bytes(), &policy, |source, ctx| {
        assert!(matches!(crate::parse::parse_with_context(source, ctx),
            Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::Entities));
    });
}

#[test]
fn strict_decode_rejects_the_reported_record_omission() {
    let source = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));ENDSEC;DATA;#1=ITEM(#0);#2=CARTESIAN_POINT('',(1.,2.,3.));ENDSEC;END-ISO-10303-21;";
    let mut options = DecodeOptions::default();
    options.policy.mode = cadmpeg_core::decode::DecodeMode::Strict;
    let error = StepCodec::default()
        .decode(&mut Cursor::new(source), &options)
        .expect_err("strict mode rejects a completed decode with a required omission");
    assert!(
        matches!(error, cadmpeg_ir::codec::DecodeFailure::StrictRejected { rejection }
        if rejection.loss().code == StepLossCode::ParseRecordOmitted.kind())
    );
}

#[test]
fn reference_walk_allocates_only_reference_outputs() {
    let value = crate::parse::Value::List(vec![crate::parse::Value::Integer(1); 32]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    with_policy_context(b"", &policy, |_, ctx| {
        super::visit_references(&value, ctx, &mut |_, _| panic!("scalar has no reference"))
            .expect("scalar traversal stores no collection items");
    });
}

#[test]
fn bounded_draft_header_defects_retain_source_and_independent_geometry() {
    for defective in [
        ".\n.\nFILE_DESCRIPTION('synthetic');",
        "FILE_DESCRIPTION('a'b);",
    ] {
        let source = format!("STEP;HEADER;{defective}IMP_LEVEL('1.0');ENDSEC;DATA;@1=CARTESIAN_POINT('',(1.,2.,3.));@2=GEOMETRIC_SET('',(#1));ENDSEC;ENDSTEP;");
        let (parsed, diagnostics) = with_service_context(source.as_bytes(), parse_inner)
            .expect("bounded HEADER statement admits independent DATA");
        assert_eq!(&source[parsed.omitted_spans()[0].clone()], defective);
        assert_eq!(parsed.records().len(), 2);
        assert!(diagnostics
            .iter()
            .any(|item| item.kind == ParseDiagnosticKind::HeaderMetadataNoncanonical));
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(&source), &DecodeOptions::default())
            .expect("independent point");
        assert_eq!(decoded.ir().model.points.len(), 1);
        let (ir, _, fidelity) = decoded.into_parts();
        let unknowns = ir.native_unknowns("step").expect("retained statements");
        let omitted = unknowns
            .iter()
            .find(|record| record.id.as_str().starts_with("step:file:omitted#"))
            .expect("omitted HEADER");
        assert_eq!(
            fidelity
                .retained_record(omitted.id.as_str())
                .expect("exact source")
                .data(),
            Some(defective.as_bytes())
        );
        assert_eq!(
            ir.source.as_ref().expect("source").attributes["bytes_unclassified"],
            "0"
        );
    }
}

#[test]
fn bounded_draft_preamble_is_retained_with_an_explicit_loss() {
    let source =
        "EXTRA!*synthetic*!STEP;HEADER;IMP_LEVEL('1.0');ENDSEC;DATA;@1=ITEM();ENDSEC;ENDSTEP;";
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("bounded draft preamble");
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::ParsePreambleOmitted.kind()));
    let (ir, _, fidelity) = decoded.into_parts();
    assert_eq!(
        fidelity
            .retained_record("step:file:omitted#0")
            .expect("exact prefix")
            .data(),
        Some(b"EXTRA".as_slice())
    );
    assert_eq!(
        ir.source.as_ref().expect("source").attributes["bytes_unclassified"],
        "0"
    );
}
