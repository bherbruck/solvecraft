// SPDX-License-Identifier: Apache-2.0
//! Header metadata must not determine whether DATA geometry survives.

use crate::parse::{parse_inner, ParseDiagnosticKind};
use crate::test_support::with_service_context;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::EditableDecodeResult;
use std::io::Cursor;

#[test]
fn header_metadata_defects_preserve_data_and_source_records() {
    let description = "FILE_DESCRIPTION(('test'),'2;1');";
    let name = "FILE_NAME('','',(''),(''),'','','');";
    let schema = "FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));";
    let cases = [
        format!("{description}{schema}"),
        format!("{schema}{name}{description}"),
        schema.to_string(),
        format!("{description}{name}{name}{schema}"),
        format!("{description}{description}{name}{schema}"),
        format!("FILE_DESCRIPTION($,$);{name}{schema}"),
        format!("{description}FILE_NAME();{schema}"),
        format!("{description}FILE_NAME('','bad date',(''),(''),'','','');{schema}"),
        format!("{description}{name}{schema}SECTION_LANGUAGE();"),
        format!("{description}{name}{schema}SECTION_CONTEXT();"),
        format!("{description}{name}{schema}VENDOR_METADATA('retained');"),
        format!("FILE_DESCRIPTION((\"41\"),'2;1');{name}{schema}"),
        format!("FILE_DESCRIPTION((\"1F\"),'2;1');{name}{schema}"),
        format!("FILE_DESCRIPTION((\"0ZZ\"),'2;1');{name}{schema}"),
        format!("FILE_DESCRIPTION((\"\"),'2;1');{name}{schema}"),
        format!(
            "FILE_DESCRIPTION(('{}'),'2;1');{name}{schema}",
            "A".repeat(40_000)
        ),
        format!("{description}{name}{schema}FILE_NOTE(<arbitrary-uri>);"),
        format!("{description}FILE_NAME(999999999999999999999999999999999);{schema}"),
        format!("{description}FILE_NAME(1.E999);{schema}"),
        format!("FILE_DESCRIPTION((1.E999),'2;1');{name}{schema}"),
    ];
    for header in cases {
        let source = format!("ISO-10303-21;HEADER;{header}ENDSEC;DATA;#1=CARTESIAN_POINT('point',(1.,2.,3.));ENDSEC;END-ISO-10303-21;");
        let (exchange, diagnostics) =
            with_service_context(source.as_bytes(), parse_inner).expect("recoverable metadata");
        assert_eq!(exchange.records().len(), 1, "{header}");
        assert_eq!(
            exchange.records()[&1].partials.first().name,
            "CARTESIAN_POINT"
        );
        assert!(
            diagnostics.iter().any(
                |diagnostic| diagnostic.kind == ParseDiagnosticKind::HeaderMetadataNoncanonical
            ),
            "{header}"
        );
        for record in exchange.header() {
            assert!(source[record.offset..].starts_with(&record.name));
        }
    }
}

#[test]
fn reordered_schema_diagnostic_uses_the_schema_offset() {
    let source = b"ISO-10303-21;HEADER;FILE_SCHEMA(('AP242 { 3 40 }'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (_, diagnostics) =
        with_service_context(source, parse_inner).expect("schema name survives invalid OID");
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.kind == ParseDiagnosticKind::SchemaObjectIdentifierOutOfRange)
        .expect("OID diagnostic");
    assert!(source[diagnostic.offset..].starts_with(b"FILE_SCHEMA"));
}

#[test]
fn header_recovery_does_not_hide_lost_framing_or_ambiguous_schema() {
    for header in [
        "FILE_SCHEMA(('AP242'));FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));",
        "FILE_SCHEMA(('AP242'));FILE_NAME('unterminated);",
        "FILE_SCHEMA($);",
        "FILE_SCHEMA((1.E999));",
        "FILE_SCHEMA(VENDOR());",
    ] {
        let source =
            format!("ISO-10303-21;HEADER;{header}ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;");
        assert!(
            with_service_context(source.as_bytes(), parse_inner).is_err(),
            "{header}"
        );
    }
}

#[test]
fn header_literal_recovery_preserves_geometry_and_exact_source() {
    let original = include_str!("../../writer/tests/data/periodic_two_rims.p21");
    let codec = crate::StepCodec::default();
    let expected = codec
        .decode(&mut Cursor::new(original), &DecodeOptions::default())
        .unwrap();
    assert!(!expected.ir().model.faces.is_empty());
    let name_start = original.find("FILE_NAME").unwrap();
    let name_end = name_start + original[name_start..].find('\n').unwrap();
    for literal in [
        "999999999999999999999999999999999",
        "1.E999",
        "VENDOR()",
        "VENDOR(1.,2.)",
        "VENDOR(VENDOR())",
        "-1.E999",
        "\"0ZZ\"",
        "<uri>",
    ] {
        let record = format!("FILE_NAME({literal});");
        let source = format!(
            "{}{}{}",
            &original[..name_start],
            record,
            &original[name_end..]
        );
        let recovered = EditableDecodeResult::from(
            codec
                .decode(&mut Cursor::new(&source), &DecodeOptions::default())
                .unwrap(),
        );
        assert_eq!(recovered.ir().model, expected.ir().model);
        assert_eq!(
            recovered.ir().source.as_ref().unwrap().attributes["bytes_unclassified"],
            "0"
        );
        assert!(recovered
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::StepLossCode::HeaderMetadataNoncanonical.kind()));
        let id = crate::ids::header(name_start);
        assert_eq!(
            recovered
                .source_fidelity()
                .retained_record(id.as_str())
                .unwrap()
                .data(),
            Some(record.as_bytes())
        );
        assert!(cadmpeg_ir::validate_neutral(recovered.ir(), Vec::new())
            .unwrap()
            .is_ok());
    }
    let source = original.replace("(0.,0.,10.)", "(0.,0.,1.E999)");
    assert_ne!(source, original, "control changes a DATA coordinate");
    let recovered = EditableDecodeResult::from(
        codec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("invalid required coordinate omits its bounded dependent graph"),
    );
    assert!(recovered
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == crate::loss::StepLossCode::ParseRecordOmitted.kind() }));
    assert!(recovered
        .ir()
        .native_unknowns("step")
        .unwrap()
        .iter()
        .any(|record| {
            record.id.as_str().starts_with("step:file:omitted#")
                && recovered
                    .source_fidelity()
                    .retained_record(record.id.as_str())
                    .and_then(|record| record.data())
                    .is_some_and(|bytes| {
                        bytes
                            .windows(b"(0.,0.,1.E999)".len())
                            .any(|window| window == b"(0.,0.,1.E999)")
                    })
        }));
}

#[test]
fn unverified_implementation_level_does_not_impose_edition3_schema_cardinality() {
    let original = include_str!("../../../tests/fixtures/ap214_sheet.p21").replace(
        "'AUTOMOTIVE_DESIGN'",
        "'AUTOMOTIVE_DESIGN','SHAPE_APPEARANCE_LAYER_MIM'",
    );
    let codec = crate::StepCodec::default();
    let expected = codec
        .decode(&mut Cursor::new(&original), &DecodeOptions::default())
        .unwrap();
    for source in [
        original.replace("'2;1'", "' '"),
        original
            .lines()
            .filter(|line| !line.starts_with("FILE_DESCRIPTION"))
            .collect::<Vec<_>>()
            .join("\n"),
    ] {
        let recovered = codec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        assert_eq!(recovered.ir().model, expected.ir().model);
        assert!(!recovered.report().losses.is_empty());
    }
    // A verified declaration still carries the edition's conformance rules.
    assert!(
        with_service_context(original.replace("'2;1'", "'4;3'").as_bytes(), parse_inner).is_err()
    );
}

#[test]
fn missing_file_name_keeps_geometry_and_retains_header_bytes() {
    let original = include_str!("../../../tests/fixtures/ap214_sheet.p21");
    let source = original
        .lines()
        .filter(|line| !line.starts_with("FILE_NAME"))
        .collect::<Vec<_>>()
        .join("\n");
    let codec = crate::StepCodec::default();
    let expected = codec
        .decode(&mut Cursor::new(original), &DecodeOptions::default())
        .unwrap();
    let recovered = EditableDecodeResult::from(
        codec
            .decode(&mut Cursor::new(&source), &DecodeOptions::default())
            .unwrap(),
    );
    assert!(!expected.ir().model.faces.is_empty());
    assert_eq!(recovered.ir().model, expected.ir().model);
    assert!(recovered
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::StepLossCode::HeaderMetadataNoncanonical.kind()));
    let offset = source.find("FILE_DESCRIPTION").unwrap();
    let id = crate::ids::header(offset);
    let retained = recovered
        .source_fidelity()
        .retained_record(id.as_str())
        .unwrap();
    // The implementation-level string itself contains a semicolon. Compare
    // against the complete physical header line instead of splitting tokens.
    let record = source[offset..].lines().next().unwrap();
    assert_eq!(retained.data(), Some(record.as_bytes()));
}

#[test]
fn unverified_header_interpretation_keeps_geometry_and_exact_source() {
    let original = include_str!("../../../tests/fixtures/ap214_sheet.p21");
    let codec = crate::StepCodec::default();
    let expected = codec
        .decode(&mut Cursor::new(original), &DecodeOptions::default())
        .expect("canonical sheet");
    for (source, code) in [
        (
            original.replace("'2;1'", "' '"),
            crate::loss::StepLossCode::ImplementationLevelUnverified,
        ),
        (
            original.replace("'2;1'", "'1;1'"),
            crate::loss::StepLossCode::ImplementationLevelUnverified,
        ),
        (
            original.replace("'AUTOMOTIVE_DESIGN'", "'AUTOMOTIVE_DESIGN { 3 40 }'"),
            crate::loss::StepLossCode::SchemaObjectIdentifierOutOfRange,
        ),
    ] {
        let recovered = EditableDecodeResult::from(
            codec
                .decode(&mut Cursor::new(&source), &DecodeOptions::default())
                .expect("readable header interpretation"),
        );
        assert_eq!(recovered.ir().model, expected.ir().model);
        assert!(recovered
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == code.kind()));
        for name in ["FILE_DESCRIPTION", "FILE_NAME", "FILE_SCHEMA"] {
            let offset = source.find(name).expect("header record");
            let id = crate::ids::header(offset);
            let retained = recovered
                .source_fidelity()
                .retained_record(id.as_str())
                .expect("exact header source survives unverified interpretation");
            let record = source[offset..].lines().next().expect("header line");
            assert_eq!(retained.data(), Some(record.as_bytes()));
        }
    }
}

#[test]
fn invalid_header_resource_encoding_preserves_bounded_data() {
    let source = b"ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));FILE_NOTE(<\xff>);ENDSEC;DATA;#1=CARTESIAN_POINT('',(1.,2.,3.));ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = with_service_context(source, parse_inner).expect("bounded descriptive URI");
    assert_eq!(exchange.records().len(), 1);
    for source in [
        &b"ISO-10303-21;HEADER;FILE_SCHEMA((\"41\"));ENDSEC;DATA;ENDSEC;END-ISO-10303-21;"[..],
        &b"ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));FILE_NOTE(\"41);ENDSEC;DATA;ENDSEC;END-ISO-10303-21;"[..],
        &b"ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));FILE_NOTE(<broken);ENDSEC;DATA;ENDSEC;END-ISO-10303-21;"[..],
    ] { assert!(with_service_context(source, parse_inner).is_err()); }
}

#[test]
fn unusable_presentation_literals_preserve_shape_and_exact_record() {
    let original = include_str!("../../writer/tests/data/periodic_two_rims.p21");
    let codec = crate::StepCodec::default();
    let expected = codec
        .decode(&mut Cursor::new(original), &DecodeOptions::default())
        .unwrap();
    for record in [
        "#900001=COLOUR_RGB('',1.E999,0.,0.);",
        "#900001=SURFACE_STYLE_TRANSPARENT(1.E999);",
        "#900001=SURFACE_STYLE_REFLECTANCE_AMBIENT_DIFFUSE(1.E999,0.5);",
        "#900001=SURFACE_STYLE_REFLECTANCE_AMBIENT_DIFFUSE_SPECULAR(0.5,1.E999,0.5,0.5,$);",
        "#900001=TEXT_STYLE_WITH_BOX_CHARACTERISTICS('',$,((.BOX_HEIGHT.,1.E999)));",
        "#900001=TEXT_STYLE_WITH_BOX_CHARACTERISTICS('',#900002,((.BOX_HEIGHT.,1.E999)));",
        "#900001=CURVE_STYLE('',.CONTINUOUS.,LENGTH_MEASURE(1.E999),$);",
    ] {
        let data_end = original.rfind("ENDSEC;").unwrap();
        let source = format!(
            "{}{}\n{}",
            &original[..data_end],
            record,
            &original[data_end..]
        );
        let recovered = EditableDecodeResult::from(
            codec
                .decode(&mut Cursor::new(&source), &DecodeOptions::default())
                .unwrap(),
        );
        assert_eq!(recovered.ir().model, expected.ir().model);
        assert!(recovered
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::StepLossCode::ParseNoncanonicalSyntax.kind()));
        assert!(recovered
            .source_fidelity()
            .retained_records()
            .values()
            .any(|record_source| record_source.data() == Some(record.as_bytes())));
        assert_eq!(
            recovered.ir().source.as_ref().unwrap().attributes["bytes_unclassified"],
            "0"
        );
    }
}
