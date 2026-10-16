// SPDX-License-Identifier: Apache-2.0
//! Part 21 complex-instance partial-order tests.

#![allow(clippy::unwrap_used)]

#[test]
fn parser_omits_duplicate_complex_partial_names() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=(B()A()B());ENDSEC;END-ISO-10303-21;";
    let (parsed, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("duplicate partials omit the bounded instance");
    assert!(parsed.records().is_empty());
    assert!(diagnostics.iter().any(|item| item.kind
        == crate::parse::ParseDiagnosticKind::RecordOmitted
        && item.message.contains("duplicate complex partial name")));
}

#[test]
fn parser_reports_recoverable_noncanonical_complex_partial_order() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=(NAMED_UNIT(#2)SOLID_ANGLE_UNIT()SI_UNIT($,.STERADIAN.));#2=DIMENSIONAL_EXPONENTS(0.,0.,0.,0.,0.,0.,0.);ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("noncanonical partial order is recoverable");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].offset,
        source
            .windows(2)
            .position(|window| window == b"#1")
            .unwrap()
    );
    assert_eq!(
        diagnostics[0].kind,
        crate::parse::ParseDiagnosticKind::ComplexPartialsNotAlphabetical
    );
    assert_eq!(
        diagnostics[0].message,
        "complex partial records are not alphabetical: observed (NAMED_UNIT, SOLID_ANGLE_UNIT, SI_UNIT), expected (NAMED_UNIT, SI_UNIT, SOLID_ANGLE_UNIT)"
    );
    assert_eq!(
        exchange.records()[&1]
            .partials
            .iter()
            .map(|partial| partial.name.as_str())
            .collect::<Vec<_>>(),
        ["NAMED_UNIT", "SOLID_ANGLE_UNIT", "SI_UNIT"]
    );
}
