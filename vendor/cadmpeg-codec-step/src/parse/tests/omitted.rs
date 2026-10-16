// SPDX-License-Identifier: Apache-2.0
//! Part 21 omitted-name recovery tests.

#[test]
fn user_defined_name_prefix_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=!VENDOR_ENTITY(!VENDOR_TYPE(#2));#2=KNOWN();ENDSEC;END-ISO-10303-21;";
    let refused = {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "step_parse_user_name_prefix",
            |limit| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = limit;
                let (ctx, _) = DecodeContext::from_root_bytes(SOURCE, &arena, &policy)
                    .expect("root fits selected policy");

                (crate::parse::parse_with_context(SOURCE, &ctx)).map(|_| ())
            },
        );
        matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "step_parse_user_name_prefix")
    };
    assert!(refused, "user-defined names must charge prefixed text");
}

#[test]
fn expected_name_error_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    const SOURCE: &[u8] = b"WRONG;";
    let refused = {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "step_parse_expected_name_error",
            |limit| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = limit;
                let (ctx, _) = DecodeContext::from_root_bytes(SOURCE, &arena, &policy)
                    .expect("root fits retained policy");

                (crate::parse::parse_with_context(SOURCE, &ctx)).map(|_| ())
            },
        );
        matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "step_parse_expected_name_error")
    };
    assert!(refused, "expected-name diagnostic must charge its text");
}

#[test]
fn omitted_name_recovery_accounts_for_inserted_parameter_storage() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT((0.,0.,0.));ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("recover omitted name");
    let parameters = &exchange.records()[&1].partials[0].parameters;

    assert_eq!(parameters.len(), 2);
    assert_eq!(diagnostics.len(), 1);

    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "step_parse_parameter",
        |cap| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::default();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
                    .expect("root fits the test policy");
            crate::parse::parse_with_context(source, &ctx)
        },
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("parameter slot storage refusal");
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::RetainedBytes
    );
    assert!(limit.additional > 0);
}

#[test]
fn parser_recovers_omitted_repositioned_tessellated_item_name() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=REPOSITIONED_TESSELLATED_ITEM(#2);#2=KNOWN();ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("recover omitted repositioned item name");
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        crate::parse::ParseDiagnosticKind::OmittedEntityName
    );
    assert_eq!(
        exchange.records()[&1].partials[0].parameters.as_slice(),
        vec![
            crate::parse::Value::String(Vec::new()),
            crate::parse::Value::Reference(2),
        ]
    );
}

#[test]
fn parser_retains_user_defined_entity_and_type_names() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=!VENDOR_ENTITY(!VENDOR_TYPE(#2));#2=KNOWN();ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("user-defined names");

    assert!(diagnostics.is_empty());
    assert_eq!(exchange.records()[&1].partials[0].name, "!VENDOR_ENTITY");
    assert_eq!(
        exchange.records()[&1].partials[0].parameters.as_slice(),
        vec![crate::parse::Value::Typed(
            "!VENDOR_TYPE".into(),
            Box::new(crate::parse::Value::Reference(2)),
        )]
    );
}

#[test]
fn parser_retains_user_defined_typed_parameter_from_witness() {
    let source = include_bytes!("../../reader/tests/data/ud01_user_defined_entity.p21");
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("user-defined type witness");

    assert!(diagnostics.is_empty());
    assert_eq!(
        exchange.records()[&2].partials[0].parameters[2],
        crate::parse::Value::Typed(
            "!VENDOR_TYPE".into(),
            Box::new(crate::parse::Value::List(vec![
                crate::parse::Value::Reference(1),
            ])),
        )
    );
}

#[test]
fn parser_does_not_repair_non_carrier_first_parameters() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=!VENDOR_ENTITY(1,#2);#2=KNOWN();ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("non-carrier entity");

    assert!(diagnostics.is_empty());
    assert_eq!(
        exchange.records()[&1].partials[0].parameters.as_slice(),
        vec![
            crate::parse::Value::Integer(1),
            crate::parse::Value::Reference(2),
        ]
    );
}

#[test]
fn parser_recovers_omitted_geometry_name_without_shifting_context_fields() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT((0.,1.,2.));#2=GEOMETRIC_REPRESENTATION_CONTEXT(3);#3=MAPPED_ITEM(#1,#2);#4=SEAM_EDGE(*,*,#1,.T.,$);#5=SHAPE_REPRESENTATION((#1),$);#6=CLOSED_SHELL($,(#1));ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("omitted geometry name is recoverable");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        crate::parse::ParseDiagnosticKind::OmittedEntityName
    );
    assert!(diagnostics[0]
        .message
        .contains("recovered 4 simple named carrier instance(s)"));
    assert_eq!(
        exchange.records()[&1].partials[0].parameters.as_slice(),
        vec![
            crate::parse::Value::String(Vec::new()),
            crate::parse::Value::List(vec![
                crate::parse::Value::Real(
                    cadmpeg_ir::scalar::FiniteReal::new(0.0).expect("finite fixture")
                ),
                crate::parse::Value::Real(
                    cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite fixture")
                ),
                crate::parse::Value::Real(
                    cadmpeg_ir::scalar::FiniteReal::new(2.0).expect("finite fixture")
                ),
            ]),
        ]
    );
    assert_eq!(
        exchange.records()[&2].partials[0].parameters.as_slice(),
        vec![crate::parse::Value::Integer(3)]
    );
    assert_eq!(
        exchange.records()[&3].partials[0].parameters[0],
        crate::parse::Value::String(Vec::new())
    );
    assert_eq!(
        exchange.records()[&4].partials[0].parameters[0],
        crate::parse::Value::String(Vec::new())
    );
    assert_eq!(
        exchange.records()[&5].partials[0].parameters[0],
        crate::parse::Value::String(Vec::new())
    );
    assert_eq!(
        exchange.records()[&6].partials[0].parameters.as_slice(),
        vec![
            crate::parse::Value::Omitted,
            crate::parse::Value::List(vec![crate::parse::Value::Reference(1)]),
        ]
    );
}

#[test]
fn parser_recovers_omitted_shape_representation_with_parameters_name() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=SHAPE_REPRESENTATION_WITH_PARAMETERS((#2),#3);#2=KNOWN();#3=KNOWN();ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("recover omitted parameterized shape name");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        crate::parse::ParseDiagnosticKind::OmittedEntityName
    );
    assert_eq!(
        exchange.records()[&1].partials[0].parameters.as_slice(),
        vec![
            crate::parse::Value::String(Vec::new()),
            crate::parse::Value::List(vec![crate::parse::Value::Reference(2)]),
            crate::parse::Value::Reference(3),
        ]
    );
}

#[test]
fn present_invalid_names_never_shift_geometry_parameters() {
    use crate::parse::Value;
    for name in [
        "7",
        ".BAD.",
        "\"01\"",
        "\"41\"",
        "<bad-uri>",
        "$",
        "*",
        "1.E999",
    ] {
        let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT({name},(1.,2.,3.));#2=ADVANCED_FACE({name},(),#3,.T.);#3=PLANE('',#4);#4=AXIS2_PLACEMENT_3D('',#1,$,$);ENDSEC;END-ISO-10303-21;");
        let (exchange, diagnostics) =
            crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
                .expect("bounded invalid name");
        let point = &exchange.records()[&1].partials[0].parameters;
        assert_eq!(point.len(), 2, "{name}");
        assert!(matches!(&point[1], Value::List(coordinates) if coordinates.len() == 3));
        let face = &exchange.records()[&2].partials[0].parameters;
        assert_eq!(face.len(), 4, "{name}");
        assert_eq!(face[2], Value::Reference(3));
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.kind == crate::parse::ParseDiagnosticKind::OmittedEntityName),
            "{name}"
        );
    }
}

#[test]
fn name_admission_preserves_brep_spline_and_transformation_layouts() {
    for (kind, tail) in [
        ("MANIFOLD_SOLID_BREP", "#2"),
        ("FACETED_BREP", "#2"),
        ("BREP_WITH_VOIDS", "#2,()"),
        ("UNIFORM_CURVE", "1,(#2,#2),.UNSPECIFIED.,.F.,.F."),
        ("B_SPLINE_CURVE_WITH_KNOTS", "1,(#2,#2),.UNSPECIFIED.,.F.,.F.,(2,2),(0.,1.),.UNSPECIFIED."),
        ("B_SPLINE_SURFACE_WITH_KNOTS", "1,1,((#2,#2),(#2,#2)),.UNSPECIFIED.,.F.,.F.,.F.,(2,2),(2,2),(0.,1.),(0.,1.),.UNSPECIFIED."),
        ("CARTESIAN_TRANSFORMATION_OPERATOR_2D", "$,$,#2,1."),
        ("CARTESIAN_TRANSFORMATION_OPERATOR_2D", "'description','',$,$,#2,1."),
        ("CARTESIAN_TRANSFORMATION_OPERATOR_3D", "$,$,#2,1.,$"),
        ("CARTESIAN_TRANSFORMATION_OPERATOR_3D", "'description','',$,$,#2,1.,$"),
    ] {
        let source = |parameters: &str| format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1={kind}({parameters});#2=KNOWN();ENDSEC;END-ISO-10303-21;");
        let canonical = source(&format!("'',{tail}"));
        let (expected, _) = crate::test_support::with_service_context(canonical.as_bytes(), crate::parse::parse_inner).unwrap();
        for name in ["7", "1.E999", "\"0ZZ\"", "<uri>"] {
            let changed = source(&format!("{name},{tail}"));
            let (actual, diagnostics) = crate::test_support::with_service_context(changed.as_bytes(), crate::parse::parse_inner).unwrap();
            assert_eq!(&actual.records()[&1].partials[0].parameters[1..], &expected.records()[&1].partials[0].parameters[1..], "{kind}");
            assert_eq!(diagnostics.len(), 1, "{kind}");
            assert_eq!(diagnostics[0].kind, crate::parse::ParseDiagnosticKind::EntityNameUnreadable, "{kind} present name keeps its slot");
        }
        // A dollar or string first attribute is ambiguous in a short mapping.
        if !tail.starts_with('$') && !tail.starts_with('\'') {
            let omitted = source(tail);
            let (actual, diagnostics) = crate::test_support::with_service_context(omitted.as_bytes(), crate::parse::parse_inner).unwrap();
            assert_eq!(actual.records()[&1].partials[0].parameters.as_slice(), expected.records()[&1].partials[0].parameters.as_slice());
            assert_eq!(diagnostics.len(), 1);
        }
    }
    let required = b"ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=UNIFORM_CURVE(1.E999,(),.UNSPECIFIED.,.F.,.F.);ENDSEC;END-ISO-10303-21;";
    let (parsed, diagnostics) =
        crate::test_support::with_service_context(required, crate::parse::parse_inner)
            .expect("unreadable required literal omits the bounded record");
    assert!(parsed.records().is_empty());
    assert!(diagnostics
        .iter()
        .any(|item| item.kind == crate::parse::ParseDiagnosticKind::RecordOmitted));
}

#[test]
fn omitted_name_recovery_keeps_required_coordinate_literals_strict() {
    for data in [
        "#1=CARTESIAN_POINT((0.,0.,1.E999));",
        "#1=CARTESIAN_POINT('',(0.,0.,1.E999));",
        "#1=CARTESIAN_POINT();#2=CARTESIAN_POINT('',(0.,0.,1.E999));",
    ] {
        let source = format!(
            "ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;{data}ENDSEC;END-ISO-10303-21;"
        );
        let (parsed, diagnostics) =
            crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
                .expect("unreadable coordinate omits only its containing instance");
        let expected = if data.starts_with("#1=CARTESIAN_POINT();") {
            vec![1]
        } else {
            Vec::new()
        };
        assert_eq!(
            parsed.records().keys().copied().collect::<Vec<_>>(),
            expected,
            "{data}"
        );
        assert!(
            diagnostics
                .iter()
                .any(|item| item.kind == crate::parse::ParseDiagnosticKind::RecordOmitted),
            "{data}"
        );
    }
}
