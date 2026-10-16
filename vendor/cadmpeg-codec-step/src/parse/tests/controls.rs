// SPDX-License-Identifier: Apache-2.0
//! Part 21 print-control and nesting tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

#[test]
fn parser_allows_print_controls_only_outside_anchor_and_reference_sections() {
    let source = b"ISO-10303-\n21;\\N\\HEADER;FILE_DESCRIPTION(('te\\N\\st'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#\n1=ITEM();ENDSEC;END-ISO-10303-21;";
    crate::test_support::with_service_context(source, crate::parse::parse_inner)
        .expect("print controls outside restricted sections");

    for section in [
        b"ANCHOR;\\N\\<a>=1;ENDSEC;".as_slice(),
        b"REFERENCE;\\N\\#1=<a>;ENDSEC;".as_slice(),
    ] {
        let source = [
            b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;".as_slice(),
            section,
            b"END-ISO-10303-21;".as_slice(),
        ]
        .concat();
        let error = crate::test_support::with_service_context(&source, crate::parse::parse_inner)
            .expect_err("restricted print control");
        assert!(error.to_string().contains("print control directive"));
    }
}

#[test]
fn parser_rejects_excessive_parameter_nesting_without_recursing_unboundedly() {
    let nested = format!("{}1{}", "(".repeat(300), ")".repeat(300));
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM({nested});ENDSEC;END-ISO-10303-21;"
    );
    let error =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .unwrap_err();
    assert!(matches!(
        error,
        crate::parse::ParseError::Resource(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RecursionDepth
                && limit.limit == cadmpeg_core::decode::DecodePolicy::service().limits.max_recursion_depth
                && limit.operation == "step_parse_parameter_nesting"
    ));
}
