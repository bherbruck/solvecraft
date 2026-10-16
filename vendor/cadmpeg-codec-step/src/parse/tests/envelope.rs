// SPDX-License-Identifier: Apache-2.0
//! Part 21 header, implementation-level, and envelope tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::io::Cursor;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use cadmpeg_core::decode::{InspectOptions, View};
use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};

use crate::StepCodec;

fn assert_unsupported_dialect(
    error: impl Into<cadmpeg_ir::DecodeFailure>,
    expected_id: &str,
    expected_message: &str,
) {
    let error = error.into();
    let cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::UnsupportedDialect {
        dialects,
        message,
        ..
    }) = error
    else {
        panic!("expected a typed STEP dialect refusal, found {error:?}");
    };
    assert_eq!(dialects.primary().dialect().as_str(), expected_id);
    assert_eq!(
        dialects.primary().admission(),
        &cadmpeg_core::dialect::Admission::Refused
    );
    assert_eq!(message, expected_message);
}

#[test]
fn parser_enforces_the_part21_header_contract() {
    let cases = [
        (
            "ISO-10303-21;HEADER;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "HEADER has no FILE_SCHEMA",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242','ap242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "FILE_SCHEMA has invalid or duplicate schema identifiers",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242','AP24\\X\\32'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "FILE_SCHEMA has invalid or duplicate schema identifiers",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(());ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "FILE_SCHEMA has invalid or duplicate schema identifiers",
        ),
    ];

    for (source, message) in cases {
        if source.contains("'AP242','ap242'") || source.contains("'AP242','AP24\\X\\32'") {
            let (parsed, diagnostics) = crate::test_support::with_service_context(
                source.as_bytes(),
                crate::parse::parse_inner,
            )
            .expect("duplicate identifiers preserve one usable schema");
            assert_eq!(parsed.schema_identifiers().count(), 1);
            assert!(diagnostics
                .iter()
                .any(|item| item.message.contains("duplicate excluded")));
            continue;
        }
        let error =
            crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
                .expect_err("invalid header");
        assert!(
            error.to_string().contains(message),
            "expected {message:?}, got {error}"
        );
    }
}

#[test]
fn parser_validates_header_string_bounds_timestamps_and_schema_identifiers() {
    fn source(file_name: &str, schema: &str, extra: &str) -> String {
        format!(
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME({file_name});FILE_SCHEMA(({schema}));{extra}ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;"
        )
    }

    let valid = source(
        "'name','2026-02-28T23:59:59.123+02:30',('author'),('organization'),'preprocessor','',''",
        "'AP242 { 1 0 10303 442 3 1 4 }'",
        "SCHEMA_POPULATION((('part.step','2026-02-28T00:00:00Z','YWJjZA==')));",
    );
    crate::test_support::with_service_context(valid.as_bytes(), crate::parse::parse_inner)
        .expect("valid header metadata");

    let schema_oid = source(
        "'name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','',''",
        "' AUTOMOTIVE_DESIGN_CC2 { 1 2 10303 214 1 1 5 4 } '",
        "",
    );
    crate::test_support::with_service_context(schema_oid.as_bytes(), crate::parse::parse_inner)
        .expect("schema object identifier");

    let named_schema_oid = source(
        "'name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','',''",
        "' AUTOMOTIVE_DESIGN_CC2 { iso standard 10303 part(214) version(1) } '",
        "",
    );
    crate::test_support::with_service_context(
        named_schema_oid.as_bytes(),
        crate::parse::parse_inner,
    )
    .expect("named schema object identifier");

    let invalid = [
        source(
            "'name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','',''",
            "'AP242 { 1 invalid_ }'",
            "",
        ),
        source(
            "'name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','',''",
            "'AP242 { }'",
            "",
        ),
        source(
            "'name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','',''",
            "'AP242 { 1 }'",
            "",
        ),
        source(
            "'name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','',''",
            "'AP242 { 01 0 }'",
            "",
        ),
        source(
            "'name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','',''",
            "'AP-242'",
            "",
        ),
    ];
    for source in invalid {
        assert!(crate::test_support::with_service_context(
            source.as_bytes(),
            crate::parse::parse_inner
        )
        .is_err());
    }

    for source in [
        source(
            "'name','2026-02-30T23:59:59',('author'),('organization'),'preprocessor','',''",
            "'AP242'",
            "",
        ),
        source(
            "'name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','',''",
            "'AP242'",
            "SCHEMA_POPULATION((('part.step','2026-02-28T00:00:00Z','not*base64')));",
        ),
    ] {
        let (_, diagnostics) =
            crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
                .expect("recover header metadata");
        assert!(!diagnostics.is_empty());
    }

    let long_description = "x".repeat(257);
    let long_description_source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('{long_description}'),'4;2');FILE_NAME('name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;"
    );
    let (_, diagnostics) = crate::test_support::with_service_context(
        long_description_source.as_bytes(),
        crate::parse::parse_inner,
    )
    .expect("recover long description");
    assert!(!diagnostics.is_empty());

    let long_schema = "A".repeat(1025);
    let long_schema_source = source(
        "'name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','',''",
        &format!("'{long_schema}'"),
        "",
    );
    assert!(crate::test_support::with_service_context(
        long_schema_source.as_bytes(),
        crate::parse::parse_inner
    )
    .is_err());

    let malformed_data_schema = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA('main',('AP242 { 1 invalid }'));#1=ITEM();ENDSEC;END-ISO-10303-21;";
    assert!(crate::test_support::with_service_context(
        malformed_data_schema.as_bytes(),
        crate::parse::parse_inner
    )
    .is_err());
}

#[test]
fn parser_selects_usable_schema_identifiers() {
    let cases = [
        ("' AP242 ','AP242'", "'AP242'", "trimmed duplicate"),
        ("'9AP242'", "'9AP242'", "leading digit"),
        ("'_AP242'", "'_AP242'", "leading underscore"),
        (
            "'AP242 { 1 0 10303 442 invalid_ 1 4 }'",
            "'AP242'",
            "unparseable OID component",
        ),
        (
            "'AP242 { 1 0 10303 442 -01 1 4 }'",
            "'AP242'",
            "negative OID component with a leading zero",
        ),
    ];
    let mut admitted = Vec::new();
    for (schema, data_schema, description) in cases {
        let source = format!(
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(({schema}));ENDSEC;DATA('main',({data_schema}));#1=ITEM();ENDSEC;END-ISO-10303-21;"
        );
        match crate::test_support::with_service_context(
            source.as_bytes(),
            crate::parse::parse_inner,
        ) {
            Ok(_) => admitted.push(description),
            Err(error) => assert!(
                error
                    .to_string()
                    .contains("FILE_SCHEMA has no usable schema identifiers"),
                "{description}: unexpected error: {error}"
            ),
        }
    }
    assert_eq!(admitted, ["trimmed duplicate"]);
}

#[test]
fn parser_recovers_an_out_of_range_schema_object_identifier_component() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AUTOMOTIVE_DESIGN_CC2 { 1 2 10303 214 -1 1 5 4 }'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("an out-of-range component is recoverable");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        crate::parse::ParseDiagnosticKind::SchemaObjectIdentifierOutOfRange
    );
    assert_eq!(
        diagnostics[0].offset,
        source
            .windows(11)
            .position(|window| window == b"FILE_SCHEMA")
            .unwrap()
    );
    assert_eq!(
        diagnostics[0].message,
        "FILE_SCHEMA identifier AUTOMOTIVE_DESIGN_CC2 has an out-of-range object identifier component -1; the object identifier is not admitted"
    );
    assert_eq!(exchange.header()[2].name, "FILE_SCHEMA");
    assert_eq!(
        exchange.schema_identifiers().collect::<Vec<_>>(),
        ["AUTOMOTIVE_DESIGN_CC2 { 1 2 10303 214 -1 1 5 4 }"]
    );
}

#[test]
fn parser_recovers_every_out_of_range_schema_object_identifier_component() {
    // A component outside the range that its position permits is recoverable,
    // and the diagnostic names the first such component in source order. The
    // root component states a root number as `0`, `1`, or `2`, or as a
    // registered ASN.1 identifier for one of those numbers; every other root
    // component is out of range. Under root `0` or `1` the second number is in
    // `0..=39`. Every other position permits every non-negative number. A root
    // component that states no root number constrains no second component.
    let cases = [
        ("AP242 { -1 40 }", Some("-1")),
        ("AP242 { 1 40 }", Some("40")),
        ("AP242 { 3 1 }", Some("3")),
        ("AP242 { 0 40 }", Some("40")),
        ("AP242 { iso 40 }", Some("40")),
        ("AP242 { 1 part(214) }", Some("part(214)")),
        ("AP242 { foo 40 }", Some("foo")),
        ("AP242 { iSo 40 }", Some("iSo")),
        ("AP242 { 1 39 }", None),
        ("AP242 { 2 40 }", None),
        ("AP242 { iso 39 }", None),
        ("AP242 { iso standard 10303 part(214) version(1) }", None),
        ("AP242 { 1 0 10303 442 3 1 4 }", None),
        ("AP242", None),
    ];
    for (identifier, component) in cases {
        let source = format!(
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('{identifier}'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;"
        );
        let (_, diagnostics) =
            crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
                .unwrap_or_else(|error| panic!("{identifier} is admissible: {error}"));
        let expected = component.map_or_else(Vec::new, |component| {
            vec![format!(
                "FILE_SCHEMA identifier AP242 has an out-of-range object identifier component {component}; the object identifier is not admitted"
            )]
        });
        assert_eq!(
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.clone())
                .collect::<Vec<_>>(),
            expected,
            "{identifier}"
        );
    }
}

#[test]
fn parser_admits_a_valid_schema_object_identifier_without_a_loss() {
    let source = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AUTOMOTIVE_DESIGN_CC2 { 1 2 10303 214 1 1 5 4 }'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (_, diagnostics) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid schema object identifier");
    assert!(diagnostics.is_empty());
}

#[test]
fn parser_does_not_admit_a_recovered_object_identifier_as_an_identifier() {
    // Each identifier holds one out-of-range component: a negative number
    // after the root, a root number that is not `0`, `1`, or `2`, and a root
    // ASN.1 identifier that is not a registered root. All three take the one
    // disposition: the header keeps the schema name, charges one diagnostic,
    // and admits the identifier neither as a DATA section schema name nor as a
    // `FILE_POPULATION` governing schema.
    for identifier in [
        "AUTOMOTIVE_DESIGN_CC2 { 1 2 10303 214 -1 1 5 4 }",
        "AUTOMOTIVE_DESIGN_CC2 { 3 40 }",
        "AUTOMOTIVE_DESIGN_CC2 { foo 40 }",
    ] {
        let header = format!(
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('{identifier}'));"
        );

        let data_by_identifier = format!(
            "{header}ENDSEC;DATA('main',('{identifier}'));#1=ITEM();ENDSEC;END-ISO-10303-21;"
        );
        let error = crate::test_support::with_service_context(
            data_by_identifier.as_bytes(),
            crate::parse::parse_inner,
        )
        .expect_err("the object identifier is not an admitted DATA section schema");
        assert!(
            error
                .to_string()
                .contains("DATA section schema is not listed in FILE_SCHEMA"),
            "{identifier}: unexpected error: {error}"
        );

        let population_by_identifier = format!(
            "{header}FILE_POPULATION('{identifier}','INCLUDE_ALL_COMPATIBLE',('main'));ENDSEC;DATA('main',('AUTOMOTIVE_DESIGN_CC2'));#1=ITEM();ENDSEC;END-ISO-10303-21;"
        );
        let (_, diagnostics) = crate::test_support::with_service_context(
            population_by_identifier.as_bytes(),
            crate::parse::parse_inner,
        )
        .expect("recover population metadata");
        assert!(diagnostics.iter().any(|diagnostic| diagnostic
            .message
            .contains("FILE_POPULATION has invalid parameters")));

        let by_name = format!(
            "{header}FILE_POPULATION('AUTOMOTIVE_DESIGN_CC2','INCLUDE_ALL_COMPATIBLE',('main'));ENDSEC;DATA('main',('AUTOMOTIVE_DESIGN_CC2'));#1=ITEM();ENDSEC;END-ISO-10303-21;"
        );
        let (_, diagnostics) = crate::test_support::with_service_context(
            by_name.as_bytes(),
            crate::parse::parse_inner,
        )
        .unwrap_or_else(|error| panic!("{identifier} keeps its schema name: {error}"));
        assert_eq!(diagnostics.len(), 1, "{identifier}");
        assert_eq!(
            diagnostics[0].kind,
            crate::parse::ParseDiagnosticKind::SchemaObjectIdentifierOutOfRange,
            "{identifier}"
        );
    }
}

#[test]
fn parser_retains_unset_file_name_tail_metadata() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'',$,$);FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("unset producer metadata");
    assert!(matches!(
        exchange.header()[1].parameters[6],
        crate::parse::Value::Omitted
    ));
}

#[test]
fn parser_allows_an_empty_data_population_in_edition_three() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;END-ISO-10303-21;";
    crate::test_support::with_service_context(source, crate::parse::parse_inner)
        .expect("edition three permits no DATA section");
}

#[test]
fn parser_enforces_legacy_implementation_level_restrictions() {
    let cases = [
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;END-ISO-10303-21;",
            "historical implementation levels require one DATA section",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA('section');#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "2;1 forbids DATA section parameters",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "2;1 forbids ANCHOR and REFERENCE sections",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;YWJjZA==ENDSEC;",
            "2;1 forbids SIGNATURE sections",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'3;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "3;1 forbids ANCHOR and REFERENCE sections",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'3;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;END-ISO-10303-21;",
            "historical implementation levels require one DATA section",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'3;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;YWJjZA==ENDSEC;",
            "3;1 forbids SIGNATURE sections",
        ),
    ];

    for (source, message) in cases {
        if message.contains("forbids ANCHOR and REFERENCE") {
            let (_, diagnostics) = crate::test_support::with_service_context(
                source.as_bytes(),
                crate::parse::parse_inner,
            )
            .expect("readable sections use the reported recovery grammar");
            assert!(diagnostics
                .iter()
                .any(|item| item.message.contains(message)));
            continue;
        }
        let error =
            crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
                .expect_err("invalid level");
        assert!(
            error.to_string().contains(message),
            "expected {message:?}, got {error}"
        );
    }
}

#[test]
fn parser_recovers_an_unknown_implementation_level_with_a_diagnostic() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'1;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (_, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("the declaration is framed");
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        crate::parse::ParseDiagnosticKind::ImplementationLevelUnverified
    );
    assert!(diagnostics[0].message.contains("1;1"));
    assert!(diagnostics[0].message.contains("4;3 grammar"));
}

#[test]
fn unknown_implementation_level_uses_the_retained_substitution_for_later_sections() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'1;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<item>=#1;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("the substituted 4;3 grammar admits ANCHOR");
    assert_eq!(exchange.implementation_level(), "1;1");
    assert_eq!(exchange.anchors().len(), 1);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        crate::parse::ParseDiagnosticKind::ImplementationLevelUnverified
    );
}

#[test]
fn parser_accepts_historical_implementation_level_spellings() {
    for level in ["1", "2", "2;1", "2;2", "3;1", "3;2"] {
        let source = format!(
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'{level}');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;"
        );
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("historical implementation level");
    }
}

#[test]
fn parser_enforces_edition_three_conformance_classes() {
    let cases = [
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<item>=#1;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "4;1 forbids ANCHOR and REFERENCE sections",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;YWJjZA==ENDSEC;",
            "4;1 forbids SIGNATURE sections",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;@10=<part.step#value>;ENDSEC;DATA;#1=ITEM(@10);ENDSEC;END-ISO-10303-21;",
            "this implementation level forbids value instances and EXPRESS constants",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(#PI);ENDSEC;END-ISO-10303-21;",
            "this implementation level forbids value instances and EXPRESS constants",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'3;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(<external>);ENDSEC;END-ISO-10303-21;",
            "resource values are only valid in edition-3 anchor items",
        ),
    ];

    for (source, message) in cases {
        if message.contains("forbids ANCHOR and REFERENCE") {
            let (_, diagnostics) = crate::test_support::with_service_context(
                source.as_bytes(),
                crate::parse::parse_inner,
            )
            .expect("readable sections use the reported recovery grammar");
            assert!(diagnostics
                .iter()
                .any(|item| item.message.contains(message)));
            continue;
        }
        let error =
            crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
                .expect_err("invalid conformance class");
        assert!(
            error.to_string().contains(message),
            "expected {message:?}, got {error}"
        );
    }

    let class_three = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;3');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;@10=<part.step#value>;ENDSEC;DATA;#1=ITEM(@10,#PI);ENDSEC;END-ISO-10303-21;";
    crate::test_support::with_service_context(class_three, crate::parse::parse_inner)
        .expect("class three value occurrences");
}

#[test]
fn parser_allows_multiple_schema_identifiers_at_legacy_level() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('CONFIG_CONTROL_DESIGN','GEOMETRIC_VALIDATION_PROPERTIES_MIM'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    crate::test_support::with_service_context(source, crate::parse::parse_inner)
        .expect("2;1 permits multiple schema identifiers");
}

#[test]
fn parser_validates_optional_header_entities_and_data_section_targets() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));SCHEMA_POPULATION((('part.step',$,$)));FILE_POPULATION('AP242','INCLUDE_ALL_COMPATIBLE',('main'));SECTION_LANGUAGE('main','eng');SECTION_CONTEXT('main',('design'));!VENDOR(('metadata'));ENDSEC;DATA('main',('AP242'));#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid optional header entities");
    assert_eq!(
        exchange
            .records()
            .iter()
            .filter(|(_, record)| exchange.data()[0].span.contains(&record.span.start))
            .map(|(&id, _)| id)
            .collect::<Vec<_>>(),
        vec![1]
    );
    assert_eq!(exchange.header()[7].name, "!VENDOR");

    let invalid = [
        (
            "SCHEMA_POPULATION((('part.step',$)));",
            "SCHEMA_POPULATION has invalid parameters",
        ),
        (
            "FILE_POPULATION('AP214','INCLUDE_ALL_COMPATIBLE',('main'));",
            "FILE_POPULATION has invalid parameters",
        ),
        (
            "SECTION_LANGUAGE('missing','eng');",
            "header section reference names an unknown DATA section",
        ),
        (
            "SECTION_LANGUAGE('main','english');",
            "SECTION_LANGUAGE has invalid parameters",
        ),
        (
            "!VENDOR(('metadata'));SECTION_CONTEXT('main',('design'));",
            "built-in HEADER entities must precede user-defined entities",
        ),
    ];
    for (extra, message) in invalid {
        let source = format!(
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));{extra}ENDSEC;DATA('main',('AP242'));#1=ITEM();ENDSEC;END-ISO-10303-21;"
        );
        let (_, diagnostics) =
            crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
                .expect("recover header metadata");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains(message)),
            "expected {message:?}, got {diagnostics:?}"
        );
    }

    let legacy = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));SCHEMA_POPULATION((('part.step',$,$)));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    crate::test_support::with_service_context(legacy, crate::parse::parse_inner)
        .expect("2;1 permits SCHEMA_POPULATION");
}

#[test]
fn parser_enforces_data_section_parameter_shape_and_multiplicity() {
    let cases = [
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;DATA;#2=ITEM();ENDSEC;END-ISO-10303-21;",
            "multiple DATA sections require section parameters",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;DATA('section',('AP242'));#2=ITEM();ENDSEC;END-ISO-10303-21;",
            "multiple DATA sections require section parameters",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA('section',('AP242'));#1=ITEM();ENDSEC;DATA('section',('AP242'));#2=ITEM();ENDSEC;END-ISO-10303-21;",
            "DATA section names must be unique",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA('section');#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "DATA section parameters must contain a name and one schema",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA('section',('AP214'));#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "DATA section schema is not listed in FILE_SCHEMA",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242','AP214'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "an unnamed DATA section requires one FILE_SCHEMA identifier",
        ),
    ];

    for (source, message) in cases {
        let error =
            crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
                .expect_err("invalid DATA section");
        assert!(
            error.to_string().contains(message),
            "expected {message:?}, got {error}"
        );
    }

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242','AP214'));ENDSEC;DATA('section-1',('AP242'));#1=ITEM();ENDSEC;DATA('section-2',('AP214'));#2=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid named DATA sections");
    assert_eq!(exchange.data().len(), 2);
}

#[test]
fn codec_uses_the_first_schema_identifier_for_dialect_selection() {
    let cases = [
        (
            "'AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }','AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 4 1 4 }'",
            "dialect step:ap242-e1",
        ),
        (
            "'AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 14 1 4 }'",
            "dialect step:unknown",
        ),
        (
            "'AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { iso standard 10303 part(442) version(3) }'",
            "dialect step:unknown",
        ),
        (
            "'OTHER_SCHEMA { 1 0 10303 442 4 1 4 }'",
            "dialect step:unknown",
        ),
        (
            "'ap242_managed_model_based_3d_engineering_mim_lf { 1 0 10303 442 4 1 4 }'",
            "dialect step:ap242-e3",
        ),
    ];

    for (identifiers, expected_dialect) in cases {
        let first_identifier = identifiers.split(',').next().expect("first schema");
        let source = format!(
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(({identifiers}));ENDSEC;DATA('section',({first_identifier}));#1=ITEM();ENDSEC;END-ISO-10303-21;"
        );
        let summary = StepCodec::default()
            .inspect(
                &mut Cursor::new(source.as_bytes()),
                &InspectOptions::default(),
            )
            .expect("inspect schema identifiers");
        assert!(
            summary
                .notes
                .iter()
                .any(|note| note.ends_with(expected_dialect)),
            "expected {expected_dialect} in {:?}",
            summary.notes
        );
    }
}

#[test]
fn part28_configuration_witnesses_are_refused_before_schema_admission() {
    const CONFIGURATION_NAMESPACE: &str = "urn:oid:1.0.10303.28.2.1.2";
    const AP238_NAMESPACE: &str = "urn:oid:1.0.10303.238.2.0.1";
    const UOS_NAME: &str = "iso_10303_28_terse";
    const EXPRESS_SCHEMA: &str = "integrated_cnc_schema";

    // The configuration and the positive exchange are the published AP238
    // Part 28 binding. The negative exchange changes only the AP-required
    // schema value, so both inputs remain well-formed XML candidates.
    let configuration =
        roxmltree::Document::parse(include_str!("data/ce03_part28_ap238_configuration.xml"))
            .expect("parse AP238 Part 28 configuration");
    let configuration_root = configuration.root_element();
    assert_eq!(
        configuration_root.tag_name().namespace(),
        Some(CONFIGURATION_NAMESPACE)
    );
    let schema = configuration_root
        .children()
        .find(|node| node.has_tag_name((CONFIGURATION_NAMESPACE, "schema")))
        .expect("AP238 schema configuration");
    assert_eq!(schema.attribute("targetNamespace"), Some(AP238_NAMESPACE));
    let option = configuration_root
        .children()
        .find(|node| node.has_tag_name((CONFIGURATION_NAMESPACE, "option")))
        .expect("AP238 mapping options");
    assert_eq!(option.attribute("exp-attribute"), Some("attribute-content"));
    assert_eq!(option.attribute("tagless"), Some("true"));
    let uos_element = configuration_root
        .children()
        .find(|node| node.has_tag_name((CONFIGURATION_NAMESPACE, "uosElement")))
        .expect("AP238 UOS configuration");
    assert_eq!(uos_element.attribute("name"), Some(UOS_NAME));
    let schema_attribute = uos_element
        .children()
        .find(|node| node.has_tag_name((CONFIGURATION_NAMESPACE, "add_attribute")))
        .expect("AP238 schema attribute configuration");
    assert_eq!(schema_attribute.attribute("name"), Some("schema"));
    assert_eq!(schema_attribute.attribute("usage"), Some("required"));

    let cases = [
        (
            include_bytes!("data/ce03_part28_ap238_e2.xml").as_slice(),
            true,
        ),
        (
            include_bytes!("data/ce03_part28_ap238_invalid_schema.xml").as_slice(),
            false,
        ),
    ];
    let codec = StepCodec::default();
    for (bytes, schema_matches_ap238) in cases {
        let document = roxmltree::Document::parse(
            std::str::from_utf8(bytes).expect("Part 28 witness is UTF-8"),
        )
        .expect("Part 28 witness is well-formed XML");
        let root = document.root_element();
        assert_eq!(root.tag_name().namespace(), Some(AP238_NAMESPACE));
        assert_eq!(root.tag_name().name(), UOS_NAME);
        assert_eq!(
            root.attribute("schema") == Some(EXPRESS_SCHEMA),
            schema_matches_ap238
        );
        assert_eq!(
            cadmpeg_test_support::detection::confidence(&codec, bytes),
            Confidence::Medium
        );
        let error = codec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect_err("Part 28 refusal");
        assert_unsupported_dialect(error, "step:part28-xml", "STEP Part 28 XML encoding");
    }
}

#[test]
fn part28_schema_mapping_witnesses_stop_at_the_caller_boundary() {
    const AP238_NAMESPACE: &str = "urn:oid:1.0.10303.238.2.0.1";
    const EXPRESS_SCHEMA: &str = "integrated_cnc_schema";

    // The accepted witness is the published AP238 attribute-content example.
    // Its Location value is a reference to the entity with id2, and its
    // Coordinates value is an aggregate of three lexical numbers. The
    // selected derived schema supplies those meanings and their constraints.
    let accepted = roxmltree::Document::parse(include_str!("data/ce03_part28_ap238_e2.xml"))
        .expect("parse accepted Part 28 mapping witness");
    let accepted_root = accepted.root_element();
    let accepted_axis = accepted
        .descendants()
        .find(|node| node.has_tag_name((AP238_NAMESPACE, "Axis2_placement_3d")))
        .expect("accepted axis placement");
    assert_eq!(accepted_axis.attribute("Location"), Some("id2"));
    let accepted_location = accepted
        .descendants()
        .find(|node| node.attribute("id") == Some("id2"))
        .expect("accepted location target");
    assert_eq!(
        accepted_location.tag_name(),
        roxmltree::ExpandedName::from((AP238_NAMESPACE, "Cartesian_point"))
    );
    assert_eq!(
        accepted_location
            .attribute("Coordinates")
            .expect("accepted coordinates")
            .split_whitespace()
            .collect::<Vec<_>>(),
        ["3.5", "-3.5", "-4.16875"]
    );
    assert_eq!(accepted_root.attribute("schema"), Some(EXPRESS_SCHEMA));

    // Changing only the reference lexical value leaves a well-formed XML
    // candidate, but the selected schema has no target for the reference.
    let rejected =
        roxmltree::Document::parse(include_str!("data/ce04_part28_ap238_missing_reference.xml"))
            .expect("parse rejected Part 28 mapping witness");
    let rejected_axis = rejected
        .descendants()
        .find(|node| node.has_tag_name((AP238_NAMESPACE, "Axis2_placement_3d")))
        .expect("rejected axis placement");
    let rejected_location = rejected_axis
        .attribute("Location")
        .expect("rejected location reference");
    assert_eq!(rejected_location, "id-missing");
    assert!(!rejected
        .descendants()
        .any(|node| node.attribute("id") == Some(rejected_location)));
    assert_eq!(
        rejected.root_element().attribute("schema"),
        Some(EXPRESS_SCHEMA)
    );

    // The AP238 EXPRESS aggregate bound rejects a fourth coordinate even
    // though the attribute remains syntactically well-formed XML.
    let invalid_aggregate =
        roxmltree::Document::parse(include_str!("data/ce04_part28_ap238_invalid_aggregate.xml"))
            .expect("parse invalid aggregate mapping witness");
    let invalid_coordinates = invalid_aggregate
        .descendants()
        .find(|node| node.has_tag_name((AP238_NAMESPACE, "Cartesian_point")))
        .and_then(|node| node.attribute("Coordinates"))
        .expect("invalid coordinate aggregate");
    assert_eq!(invalid_coordinates.split_whitespace().count(), 4);

    // XML Schema identity constraints reject duplicate instance IDs even
    // when the duplicate values would otherwise look like valid references.
    let duplicate_id =
        roxmltree::Document::parse(include_str!("data/ce04_part28_ap238_duplicate_id.xml"))
            .expect("parse duplicate-id mapping witness");
    assert_eq!(
        duplicate_id
            .descendants()
            .filter(|node| node.attribute("id") == Some("id2"))
            .count(),
        2
    );

    // The unbound witness keeps the same element names and values but omits
    // the required schema selector. Without the exact configuration and
    // derived schema, the lexical values have no unique EXPRESS meaning.
    let unbound =
        roxmltree::Document::parse(include_str!("data/ce04_part28_ap238_unbound_schema.xml"))
            .expect("parse unbound Part 28 mapping witness");
    let unbound_axis = unbound
        .descendants()
        .find(|node| node.has_tag_name((AP238_NAMESPACE, "Axis2_placement_3d")))
        .expect("unbound axis placement");
    assert_eq!(unbound_axis.attribute("Location"), Some("id2"));
    assert_eq!(
        unbound.root_element().attribute("schema"),
        None,
        "the AP238 configuration requires this selector"
    );
    assert_eq!(
        unbound_axis.tag_name().name(),
        accepted_axis.tag_name().name()
    );
    assert_eq!(
        unbound_axis.attribute("Location"),
        accepted_axis.attribute("Location")
    );

    // The STEP codec does not guess the mapping or build a partial graph for
    // any of the three caller outcomes.
    let codec = StepCodec::default();
    for bytes in [
        include_bytes!("data/ce03_part28_ap238_e2.xml").as_slice(),
        include_bytes!("data/ce04_part28_ap238_missing_reference.xml").as_slice(),
        include_bytes!("data/ce04_part28_ap238_invalid_aggregate.xml").as_slice(),
        include_bytes!("data/ce04_part28_ap238_duplicate_id.xml").as_slice(),
        include_bytes!("data/ce04_part28_ap238_unbound_schema.xml").as_slice(),
    ] {
        assert_eq!(
            cadmpeg_test_support::detection::confidence(&codec, bytes),
            Confidence::Medium
        );
        let error = codec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect_err("Part 28 refusal");
        assert_unsupported_dialect(error, "step:part28-xml", "STEP Part 28 XML encoding");
    }
}

#[test]
fn codec_refuses_out_of_envelope_encodings_by_name() {
    let codec = StepCodec::default();
    let cases: &[(&[u8], &str, &str)] = &[
        (
            b"\x89HDF\r\n\x1a\ncontent",
            "step:part26-hdf5",
            "STEP Part 26 binary/HDF5 encoding",
        ),
        (
            b"<?xml version='1.0'?><iso_10303_28/>",
            "step:part28-xml",
            "STEP Part 28 XML encoding",
        ),
        (
            include_bytes!("data/ce03_part28_ap242.xml").as_slice(),
            "step:part28-xml",
            "STEP Part 28 XML encoding",
        ),
        (
            include_bytes!("data/ce03_part28_ap238_step_tools.xml").as_slice(),
            "step:part28-xml",
            "STEP Part 28 XML encoding",
        ),
        (
            include_bytes!("data/ce03_part28_configured_uos.xml").as_slice(),
            "step:part28-xml",
            "STEP Part 28 XML encoding",
        ),
        (
            include_bytes!("data/bm01_ap242_bo_model_ed2.stpx").as_slice(),
            "step:ap242-bo-model-xml",
            "AP242 BO-Model XML sidecar",
        ),
    ];
    for &(bytes, dialect, reason) in cases {
        let error = codec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap_err();
        assert_unsupported_dialect(error, dialect, reason);
    }
    assert_eq!(
        cadmpeg_test_support::detection::confidence(
            &codec,
            b"<?xml version='1.0'?><iso_10303_28/>"
        ),
        Confidence::Medium
    );
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&codec, b"\x89HDF\r\n\x1a\ncontent"),
        Confidence::Medium
    );
    let mut hdf5_user_block = vec![0u8; 512];
    hdf5_user_block.extend_from_slice(b"\x89HDF\r\n\x1a\nGeometry_encoding");
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&codec, &hdf5_user_block),
        Confidence::Medium
    );
    let error = codec
        .decode(&mut Cursor::new(hdf5_user_block), &DecodeOptions::default())
        .expect_err("Part 26 refusal");
    assert_unsupported_dialect(
        error,
        "step:part26-hdf5",
        "STEP Part 26 binary/HDF5 encoding",
    );
    let mut invalid_hdf5_offset = vec![0u8; 256];
    invalid_hdf5_offset.extend_from_slice(b"\x89HDF\r\n\x1a\nGeometry_encoding");
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&codec, &invalid_hdf5_offset),
        Confidence::No
    );
    assert!(matches!(
        codec.decode(
            &mut Cursor::new(invalid_hdf5_offset),
            &DecodeOptions::default()
        ),
        Err(cadmpeg_ir::DecodeFailure::Codec(
            cadmpeg_core::CodecError::WrongFormat(message)
        ))
            if message == "missing ISO-10303-21 magic"
    ));
    assert_eq!(
        cadmpeg_test_support::detection::confidence(
            &codec,
            include_bytes!("data/ce03_part28_ap242.xml")
        ),
        Confidence::Medium
    );
    assert_eq!(
        cadmpeg_test_support::detection::confidence(
            &codec,
            include_bytes!("data/ce03_part28_ap238_step_tools.xml")
        ),
        Confidence::Medium
    );
    assert_eq!(
        cadmpeg_test_support::detection::confidence(
            &codec,
            include_bytes!("data/ce03_part28_configured_uos.xml")
        ),
        Confidence::Medium
    );
    let canonical = include_bytes!("data/bm01_ap242_bo_model_ed2.stpx");
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&codec, canonical),
        Confidence::Medium
    );

    let lookalike = include_bytes!("data/bm01_ap242_bo_model_wrong_namespace.stpx");
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&codec, lookalike),
        Confidence::No
    );
    assert!(matches!(
        codec.decode(&mut Cursor::new(lookalike), &DecodeOptions::default()),
        Err(cadmpeg_ir::DecodeFailure::Codec(
            cadmpeg_core::CodecError::WrongFormat(_)
        ))
    ));
}

#[test]
fn bo_model_detection_requires_root_namespace_binding() {
    let codec = StepCodec::default();
    assert_eq!(
        cadmpeg_test_support::detection::confidence(
            &codec,
            include_bytes!("data/bm01_ap242_bo_model_ed2.stpx")
        ),
        Confidence::Medium
    );

    let false_positives: &[&[u8]] = &[
        b"<?xml version='1.0'?><note>business_object_model</note>",
        b"<?xml version='1.0'?><!-- ap242_bo_model --><note/>",
        b"<?xml version='1.0'?><note marker='http://standards.iso.org/iso/ts/10303/-3001/-ed-2/tech/xml-schema/bo_model'/>",
        b"<?xml version='1.0'?><note><child xmlns:n0='http://standards.iso.org/iso/ts/10303/-3001/-ed-2/tech/xml-schema/bo_model'/></note>",
    ];
    for xml in false_positives {
        assert_eq!(
            cadmpeg_test_support::detection::confidence(&codec, xml),
            Confidence::No
        );
        assert!(matches!(
            codec.decode(&mut Cursor::new(xml), &DecodeOptions::default()),
            Err(cadmpeg_ir::DecodeFailure::Codec(
                cadmpeg_core::CodecError::WrongFormat(_)
            ))
        ));
    }
}

#[test]
fn codec_refuses_schema_marked_part26_hdf5_population() {
    let encoded = include_bytes!("data/ce05_part26_population.h5.b64")
        .iter()
        .copied()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect::<Vec<_>>();
    let bytes = STANDARD.decode(encoded).expect("Part 26 HDF5 witness");
    assert!(bytes.starts_with(b"\x89HDF\r\n\x1a\n"));
    for marker in [
        "Geometry_encoding",
        "Geometry_population",
        "iso_10303_26_schema",
        "iso_10303-26_data",
        "iso_10303_26_data_set_names",
        "set_unset_bitmap",
        "Entity-Instance-Identifier",
        "_HDF_INSTANCE_REFERENCE_HANDLE_",
        "_HDF5_dataset_index_",
        "_HDF5_instance_index_",
        "select_bitmap",
        "Aggr-properties-1",
    ] {
        assert!(
            bytes
                .windows(marker.len())
                .any(|window| window == marker.as_bytes()),
            "Part 26 witness is missing {marker}"
        );
    }

    let codec = StepCodec::default();
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&codec, &bytes),
        Confidence::Medium
    );
    let error = codec
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect_err("Part 26 refusal");
    assert_unsupported_dialect(
        error,
        "step:part26-hdf5",
        "STEP Part 26 binary/HDF5 encoding",
    );
}

#[test]
fn part26_mapping_witness_decodes_hdf5_schema_population_and_references() {
    let encoded = include_bytes!("data/ce05_part26_population.h5.b64")
        .iter()
        .copied()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect::<Vec<_>>();
    let bytes = STANDARD.decode(encoded).expect("Part 26 HDF5 witness");
    let file = hdf5_reader::Hdf5File::from_vec(bytes).expect("valid HDF5 witness");

    let root = file.root_group().expect("HDF5 root group");
    let mut root_groups = root
        .groups()
        .expect("HDF5 root groups")
        .into_iter()
        .map(|group| group.name().to_owned())
        .collect::<Vec<_>>();
    root_groups.sort_unstable();
    assert_eq!(root_groups, ["Geometry_encoding", "Geometry_population"]);

    let schema = file
        .group("/Geometry_encoding")
        .expect("Part 26 schema group");
    assert_eq!(
        schema
            .attribute("iso_10303_26_schema")
            .expect("schema identifier")
            .read_string()
            .expect("schema identifier string"),
        "Geometry_schema"
    );

    let population = file
        .group("/Geometry_population")
        .expect("Part 26 population group");
    assert_eq!(
        population
            .attribute("iso_10303-26_data")
            .expect("population schema identifier")
            .read_string()
            .expect("population schema identifier string"),
        "Geometry_schema"
    );
    assert_eq!(
        population
            .attribute("iso_10303_26_data_set_names")
            .expect("population dataset-name table")
            .read_strings()
            .expect("population dataset-name strings"),
        ["Point", "Line", "Land_survey"]
    );

    let point = file
        .dataset("/Geometry_population/Point_objects/Point_instances")
        .expect("Point instance dataset");
    assert_eq!(point.shape(), [4]);
    let point_bytes = point.read_raw_bytes().expect("Point rows");
    assert_eq!(point_bytes.len(), 4 * 24);
    for (row, (id, x, y)) in [
        (0, 0.0, 0.0),
        (1, 100.0, 0.0),
        (2, 100.0, 100.0),
        (3, 0.0, 100.0),
    ]
    .into_iter()
    .enumerate()
    {
        let offset = row * 24;
        assert_eq!(View::i32_le_at(&point_bytes, offset), Some(7));
        assert_eq!(View::i32_le_at(&point_bytes, offset + 4), Some(id));
        assert_eq!(View::f64_le_at(&point_bytes, offset + 8), Some(x));
        assert_eq!(View::f64_le_at(&point_bytes, offset + 16), Some(y));
    }

    let line = file
        .dataset("/Geometry_population/Line_objects/Line_instances")
        .expect("Line instance dataset");
    assert_eq!(line.shape(), [4]);
    let line_bytes = line.read_raw_bytes().expect("Line rows");
    assert_eq!(line_bytes.len(), 4 * 46);
    for (row, (id, start, end, select, colour)) in [
        (4, (0, 0), (0, 1), 2, 1),
        (5, (0, 1), (0, 2), 2, 3),
        (6, (0, 2), (0, 3), 1, 0),
        (7, (0, 3), (0, 0), 1, 0),
    ]
    .into_iter()
    .enumerate()
    {
        let offset = row * 46;
        assert_eq!(View::i32_le_at(&line_bytes, offset), Some(7));
        assert_eq!(View::i32_le_at(&line_bytes, offset + 4), Some(id));
        assert_eq!(View::i32_le_at(&line_bytes, offset + 8), Some(start.0));
        assert_eq!(View::i32_le_at(&line_bytes, offset + 12), Some(start.1));
        assert_eq!(View::i32_le_at(&line_bytes, offset + 16), Some(end.0));
        assert_eq!(View::i32_le_at(&line_bytes, offset + 20), Some(end.1));
        assert_eq!(View::i32_le_at(&line_bytes, offset + 24), Some(select));
        assert_eq!(View::i16_le_at(&line_bytes, offset + 44), Some(colour));
    }

    let survey = file
        .dataset("/Geometry_population/Land_survey_objects/Land_survey_instances")
        .expect("Land_survey instance dataset");
    assert_eq!(survey.shape(), [1]);
    let survey_bytes = survey.read_raw_bytes().expect("Land_survey rows");
    assert_eq!(survey_bytes.len(), 24);
    assert_eq!(View::i32_le_at(&survey_bytes, 0), Some(15));
    assert_eq!(View::i32_le_at(&survey_bytes, 4), Some(25));
}

#[test]
fn bo_model_does_not_compose_with_explicit_part21_file_reference() {
    let codec = StepCodec::default();
    let part21 = include_bytes!("data/bm02_part21_base.p21");

    let result = codec
        .decode(&mut Cursor::new(part21), &DecodeOptions::default())
        .expect("decode the Part 21 side of the pair");
    assert_eq!(result.ir().model.product_definitions.len(), 1);
    assert_eq!(
        result.ir().model.product_definitions[0]
            .source_name
            .as_deref(),
        Some("P21 value")
    );
    assert_eq!(
        result.ir().model.product_definitions[0].label.as_deref(),
        Some("P21 value")
    );
    assert_eq!(
        result.ir().model.product_definitions[0]
            .part_number
            .as_deref(),
        Some("P21_PRODUCT")
    );
    assert!(!result
        .report()
        .notes
        .iter()
        .any(|note| note.contains("bm02-model")));

    for (xml, value) in [
        (
            include_bytes!("data/bm02_bo_model_resource.stpx").as_slice(),
            "XML value",
        ),
        (
            include_bytes!("data/bm02_bo_model_conflicting_value.stpx").as_slice(),
            "XML override",
        ),
    ] {
        assert_eq!(
            cadmpeg_test_support::detection::confidence(&codec, xml),
            Confidence::Medium
        );
        assert!(xml
            .windows(b"ExternalItem".len())
            .any(|window| { window == b"ExternalItem" }));
        assert!(xml
            .windows(b"bm02-model.p21".len())
            .any(|window| { window == b"bm02-model.p21" }));
        assert!(xml
            .windows(b"P21_PRODUCT".len())
            .any(|window| { window == b"P21_PRODUCT" }));
        assert!(xml
            .windows(value.len())
            .any(|window| window == value.as_bytes()));
        let error = codec
            .decode(&mut Cursor::new(xml), &DecodeOptions::default())
            .expect_err("BO-Model refusal");
        assert_unsupported_dialect(
            error,
            "step:ap242-bo-model-xml",
            "AP242 BO-Model XML sidecar",
        );
    }
}
