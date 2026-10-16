// SPDX-License-Identifier: Apache-2.0
//! Part 21 parser work-budget and storage-accounting tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::collections::BTreeMap;
use std::fmt::Write as _;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::super::{
    AnchorEntry, AnchorResolver, ReferenceEntry, ReferenceName, ReferenceResolver, ResolveError,
    Value,
};

#[test]
fn nested_value_copy_refuses_inner_collection_limit() {
    let value = Value::List(vec![Value::List(vec![Value::Integer(1)])]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    assert!(matches!(
        super::super::try_clone_value(&value, &ctx, "step_test_value_copy"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_test_value_copy"
    ));
}

#[test]
fn typed_value_copy_refuses_nested_slot_limit() {
    let value = Value::Typed("MEASURE".into(), Box::new(Value::Integer(1)));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    assert!(matches!(
        super::super::try_clone_value(&value, &ctx, "step_test_typed_copy"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_test_typed_copy"
    ));
}

#[test]
fn header_string_validation_refuses_retained_limit() {
    const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(SOURCE, crate::parse::parse_inner)
            .expect("valid header");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 2;
    let (ctx, _) =
        DecodeContext::from_root_bytes(SOURCE, &arena, &policy).expect("root fits retained policy");
    assert!(matches!(
        super::super::validate_header(exchange.header(), &ctx),
        Err(super::super::ValidationError::Resource(CodecError::ResourceLimit(refusal)))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_string_text"
    ));
}

#[test]
fn header_string_refusal_reaches_parse_caller() {
    const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let refused = (1..=8192).any(|limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(SOURCE, &arena, &policy)
            .expect("root fits retained policy");
        matches!(
            crate::parse::parse_with_context(SOURCE, &ctx),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "step_string_text"
        )
    });
    assert!(
        refused,
        "header text must refuse through parse_with_context"
    );
}

#[test]
fn section_language_string_validation_refuses_retained_limit() {
    const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));SECTION_LANGUAGE($,'ENG');ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(SOURCE, crate::parse::parse_inner)
            .expect("valid section language");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 2;
    let (ctx, _) =
        DecodeContext::from_root_bytes(SOURCE, &arena, &policy).expect("root fits retained policy");
    assert!(matches!(
        super::super::validate_header_sections(
            super::super::ImplementationLevel::Edition3Class2,
            exchange.header(),
            &[String::from("AP242")],
            &ctx,
        ),
        Err(super::super::ValidationError::Resource(CodecError::ResourceLimit(refusal)))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_string_text"
    ));
}

#[test]
fn data_section_name_validation_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(b"DATA", &arena, &policy)
        .expect("root fits retained policy");
    let parameters = [
        Value::String(b"section".to_vec()),
        Value::List(vec![Value::String(b"AP242".to_vec())]),
    ];
    let mut section_names = std::collections::BTreeSet::new();
    assert!(matches!(
        super::super::valid_data_parameters(
            &parameters,
            &[String::from("AP242")],
            super::super::ImplementationLevel::LegacyEdition1,
            &mut section_names,
            &ctx,
        ),
        Err(super::super::ValidationError::Resource(CodecError::ResourceLimit(refusal)))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_string_text"
    ));
}

const EXTENDED_HEADER_SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));SCHEMA_POPULATION((('part.step',$,$)));FILE_POPULATION('AP242','INCLUDE_ALL_COMPATIBLE',('main'));SECTION_LANGUAGE('main','eng');SECTION_CONTEXT('main',('design'));ENDSEC;DATA('main',('AP242'));#1=ITEM();ENDSEC;END-ISO-10303-21;";

fn validation_refuses(
    operation: &str,
    dimension: ResourceDimension,
    source: &[u8],
    run: impl Fn(
        &crate::parse::Exchange,
        &DecodeContext<'_>,
    ) -> Result<(), super::super::ValidationError>,
) {
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid header source");
    let refused = (0..=1024).any(|limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
            _ => unreachable!("test only selects collection or retained limits"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
            .expect("root fits selected policy");
        matches!(
            run(&exchange, &ctx),
            Err(super::super::ValidationError::Resource(CodecError::ResourceLimit(refusal)))
                if refusal.dimension == dimension && refusal.operation == operation
        )
    });
    assert!(refused, "no {dimension:?} limit refused {operation}");
}

fn header_validation_refuses(operation: &str, dimension: ResourceDimension) {
    validation_refuses(
        operation,
        dimension,
        EXTENDED_HEADER_SOURCE,
        |exchange, ctx| super::super::validate_header(exchange.header(), ctx).map(|_| ()),
    );
}

fn header_section_validation_refuses(operation: &str, dimension: ResourceDimension) {
    validation_refuses(
        operation,
        dimension,
        EXTENDED_HEADER_SOURCE,
        |exchange, ctx| {
            super::super::validate_header_sections(
                super::super::ImplementationLevel::Edition3Class2,
                exchange.header(),
                &[String::from("AP242")],
                ctx,
            )
            .map(|_| ())
        },
    );
}

macro_rules! header_limit_test {
    ($name:ident, $operation:literal, $dimension:expr) => {
        #[test]
        fn $name() {
            header_validation_refuses($operation, $dimension);
        }
    };
}

macro_rules! section_limit_test {
    ($name:ident, $operation:literal, $dimension:expr) => {
        #[test]
        fn $name() {
            header_section_validation_refuses($operation, $dimension);
        }
    };
}

header_limit_test!(
    schema_identifier_normalization_refuses_retained_limit,
    "step_schema_identifier_normalized",
    ResourceDimension::RetainedBytes
);
header_limit_test!(
    schema_identifier_name_set_refuses_collection_limit,
    "step_schema_identifier_names",
    ResourceDimension::CollectionItems
);
header_limit_test!(
    admitted_schema_identifier_vec_refuses_collection_limit,
    "step_schema_identifiers",
    ResourceDimension::CollectionItems
);
section_limit_test!(
    file_population_sections_refuse_collection_limit,
    "step_file_population_sections",
    ResourceDimension::CollectionItems
);
section_limit_test!(
    header_data_references_refuse_collection_limit,
    "step_header_data_references",
    ResourceDimension::CollectionItems
);
section_limit_test!(
    section_language_names_refuse_collection_limit,
    "step_section_language_names",
    ResourceDimension::CollectionItems
);
section_limit_test!(
    section_context_names_refuse_collection_limit,
    "step_section_context_names",
    ResourceDimension::CollectionItems
);
section_limit_test!(
    section_language_name_copy_refuses_retained_limit,
    "step_section_language_name_copy",
    ResourceDimension::RetainedBytes
);
section_limit_test!(
    section_context_name_copy_refuses_retained_limit,
    "step_section_context_name_copy",
    ResourceDimension::RetainedBytes
);

#[test]
fn implementation_level_diagnostic_text_refuses_retained_limit() {
    validation_refuses(
        "step_implementation_level_diagnostic_text",
        ResourceDimension::RetainedBytes,
        UNVERIFIED_LEVEL_SOURCE,
        |exchange, ctx| super::super::validate_header(exchange.header(), ctx).map(|_| ()),
    );
}

#[test]
fn schema_oid_diagnostic_text_refuses_retained_limit() {
    let (exchange, _) = crate::test_support::with_service_context(
        SCHEMA_DIAGNOSTIC_SOURCE,
        crate::parse::parse_inner,
    )
    .expect("valid out-of-range schema identifier");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(SCHEMA_DIAGNOSTIC_SOURCE, &arena, &policy)
        .expect("root fits retained policy");
    assert!(matches!(
        super::super::schema_object_identifier_diagnostics(
            &exchange.schema_identifiers,
            exchange.header()[2].offset,
            &ctx,
        )
        .next(),
        Some(Err(CodecError::ResourceLimit(refusal)))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_schema_oid_diagnostic_text"
    ));
}

#[test]
fn schema_name_matching_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 4;
    let (ctx, _) = DecodeContext::from_root_bytes(b"AP242", &arena, &policy)
        .expect("root fits retained policy");
    assert!(matches!(
        super::super::schema_identifier_matches(&[String::from("AP242")], "AP242", &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::MaterializedBytes
                && refusal.operation == "step_schema_name_matching"
    ));
}

#[test]
fn matching_schema_names_refuse_collection_limit() {
    let (exchange, _) = crate::test_support::with_service_context(
        EXTENDED_HEADER_SOURCE,
        crate::parse::parse_inner,
    )
    .expect("valid schema header");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(EXTENDED_HEADER_SOURCE, &arena, &policy)
        .expect("root fits collection policy");
    assert!(matches!(
        super::super::schema_names_for_matching(&exchange.schema_identifiers, &ctx),
        Err(super::super::ParseError::Resource(CodecError::ResourceLimit(refusal)))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_schema_matching_names"
    ));
}

#[test]
fn matching_schema_name_text_refuses_retained_limit() {
    let (exchange, _) = crate::test_support::with_service_context(
        EXTENDED_HEADER_SOURCE,
        crate::parse::parse_inner,
    )
    .expect("valid schema header");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 4;
    let (ctx, _) = DecodeContext::from_root_bytes(EXTENDED_HEADER_SOURCE, &arena, &policy)
        .expect("root fits retained policy");
    assert!(matches!(
        super::super::schema_names_for_matching(&exchange.schema_identifiers, &ctx),
        Err(super::super::ParseError::Resource(CodecError::ResourceLimit(refusal)))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_schema_matching_name"
    ));
}

#[test]
fn data_section_name_set_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"DATA", &arena, &policy)
        .expect("root fits collection policy");
    let parameters = [
        Value::String(b"main".to_vec()),
        Value::List(vec![Value::String(b"AP242".to_vec())]),
    ];
    let mut section_names = std::collections::BTreeSet::new();
    assert!(matches!(
        super::super::valid_data_parameters(
            &parameters,
            &[String::from("AP242")],
            super::super::ImplementationLevel::LegacyEdition1,
            &mut section_names,
            &ctx,
        ),
        Err(super::super::ValidationError::Resource(CodecError::ResourceLimit(refusal)))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_data_section_names"
    ));
}

#[test]
fn header_record_vector_refuses_collection_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(1);ENDSEC;END-ISO-10303-21;";
    let arena = DecodeArena::new();
    let refusal_at_header = (0..64).any(|limit| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
            .expect("root fits selected policy");
        matches!(
            crate::parse::parse_with_context(source, &ctx),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "step_parse_header_records"
        )
    });
    assert!(
        refusal_at_header,
        "one limit must refuse the header allocation"
    );
}

fn collection_refusal_reaches_parser(source: &[u8], operation: &str) {
    let arena = DecodeArena::new();
    let refused = (0..1024).any(|limit| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
            .expect("root fits selected policy");
        matches!(
            crate::parse::parse_with_context(source, &ctx),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == operation
        )
    });
    assert!(refused, "no collection limit refused {operation}");
}

const VECTOR_SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(#2);#2=ITEM();ENDSEC;END-ISO-10303-21;";
const ANCHOR_VECTOR_SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;3');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<shape>=#1 {source:<part.step#shape>} {width:@100};ENDSEC;REFERENCE;@100=<part.step#width>;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
const COMPLEX_VECTOR_SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=(NAMED_UNIT(#2)SOLID_ANGLE_UNIT()SI_UNIT($,.STERADIAN.));#2=DIMENSIONAL_EXPONENTS(0.,0.,0.,0.,0.,0.,0.);ENDSEC;END-ISO-10303-21;";
const OMITTED_VECTOR_SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT((0.,0.,0.));ENDSEC;END-ISO-10303-21;";
const SCHEMA_DIAGNOSTIC_SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AUTOMOTIVE_DESIGN_CC2 { 1 2 10303 214 -1 1 5 4 }'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
const UNVERIFIED_LEVEL_SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;9');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
const SIGNATURE_VECTOR_SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('signatures'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\nENDSEC;";

macro_rules! parser_vector_limit_test {
    ($name:ident, $source:expr, $operation:literal) => {
        #[test]
        fn $name() {
            collection_refusal_reaches_parser($source, $operation);
        }
    };
}

parser_vector_limit_test!(
    parameter_vector_refuses_collection_limit,
    VECTOR_SOURCE,
    "step_parse_parameter"
);
parser_vector_limit_test!(
    record_partial_vector_refuses_collection_limit,
    COMPLEX_VECTOR_SOURCE,
    "step_parse_record_partials"
);
// Map insertion admits node storage and the collection item with one operation.
parser_vector_limit_test!(
    record_table_refuses_collection_limit,
    VECTOR_SOURCE,
    "step_parse_record_table_storage"
);
parser_vector_limit_test!(
    data_section_vector_refuses_collection_limit,
    VECTOR_SOURCE,
    "step_parse_data_sections"
);
parser_vector_limit_test!(
    anchor_tag_vector_refuses_collection_limit,
    ANCHOR_VECTOR_SOURCE,
    "step_parse_anchor_tags"
);
parser_vector_limit_test!(
    anchor_vector_refuses_collection_limit,
    ANCHOR_VECTOR_SOURCE,
    "step_parse_anchors"
);
parser_vector_limit_test!(
    anchor_binding_map_refuses_collection_limit,
    ANCHOR_VECTOR_SOURCE,
    "step_anchor_binding_items"
);

#[test]
fn anchor_binding_reference_copy_allocates_no_retained_storage() {
    let (exchange, _) =
        crate::test_support::with_service_context(ANCHOR_VECTOR_SOURCE, crate::parse::parse_inner)
            .expect("valid anchor source");
    let value = &exchange.anchors()[0].value;
    assert_eq!(value, &Value::Reference(1));
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        assert_eq!(
            crate::parse::try_clone_value(value, ctx, "step_anchor_binding_value_copy")
                .expect("a reference copy allocates no buffer"),
            *value
        );
    });
}
parser_vector_limit_test!(
    reference_entry_vector_refuses_collection_limit,
    ANCHOR_VECTOR_SOURCE,
    "step_parse_reference_entries"
);
parser_vector_limit_test!(
    external_reference_id_set_refuses_collection_limit,
    ANCHOR_VECTOR_SOURCE,
    "step_parse_external_reference_ids"
);
parser_vector_limit_test!(
    entity_index_name_refuses_collection_limit,
    VECTOR_SOURCE,
    "step_entity_index_names"
);
parser_vector_limit_test!(
    entity_index_id_refuses_collection_limit,
    VECTOR_SOURCE,
    "step_entity_index_ids"
);
parser_vector_limit_test!(
    reference_id_vector_refuses_collection_limit,
    ANCHOR_VECTOR_SOURCE,
    "step_parse_reference_ids"
);
parser_vector_limit_test!(
    value_reference_id_vector_refuses_collection_limit,
    ANCHOR_VECTOR_SOURCE,
    "step_parse_value_reference_ids"
);
parser_vector_limit_test!(
    complex_diagnostic_vector_refuses_collection_limit,
    COMPLEX_VECTOR_SOURCE,
    "step_parse_diagnostics"
);
parser_vector_limit_test!(
    canonical_partial_name_vector_refuses_collection_limit,
    COMPLEX_VECTOR_SOURCE,
    "step_parse_canonical_partial_names"
);

#[test]
fn complex_partial_diagnostic_text_refuses_retained_limit() {
    let refused = {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "step_parse_complex_partial_diagnostic_text",
            |limit| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = limit;
                let (ctx, _) =
                    DecodeContext::from_root_bytes(COMPLEX_VECTOR_SOURCE, &arena, &policy)
                        .expect("input fits retained policy");

                (crate::parse::parse_with_context(COMPLEX_VECTOR_SOURCE, &ctx)).map(|_| ())
            },
        );
        matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "step_parse_complex_partial_diagnostic_text")
    };
    assert!(
        refused,
        "complex partial diagnostic text must reach the parser caller"
    );
}
parser_vector_limit_test!(
    omitted_name_diagnostic_vector_refuses_collection_limit,
    OMITTED_VECTOR_SOURCE,
    "step_parse_diagnostics"
);
parser_vector_limit_test!(
    schema_identifier_diagnostic_vector_refuses_collection_limit,
    SCHEMA_DIAGNOSTIC_SOURCE,
    "step_parse_diagnostics"
);
parser_vector_limit_test!(
    implementation_level_diagnostic_vector_refuses_collection_limit,
    UNVERIFIED_LEVEL_SOURCE,
    "step_parse_diagnostics"
);
parser_vector_limit_test!(
    signature_span_vector_refuses_collection_limit,
    SIGNATURE_VECTOR_SOURCE,
    "step_parse_signature_spans"
);

#[test]
fn anchor_list_slots_are_admitted_before_vector_allocation() {
    let value = Value::List((0..8).map(Value::Integer).collect());
    let anchors = BTreeMap::new();
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(b"anchor", &arena, &service)
        .expect("root fits service profile");
    assert_eq!(
        AnchorResolver::new(&anchors, &ctx)
            .expect("empty resolver scope fits")
            .resolve_root(&value)
            .expect("service admits list"),
        value
    );
    let mut limited = service;
    limited.limits.max_collection_items = 8;
    let (ctx, _) = DecodeContext::from_root_bytes(b"anchor", &arena, &limited)
        .expect("root fits selected profile");
    let error = AnchorResolver::new(&anchors, &ctx)
        .expect("empty resolver scope fits")
        .resolve_root(&value)
        .expect_err("one list node plus eight slots exceed eight items");
    assert!(
        matches!(error, ResolveError::Resource(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "step_anchor_list_items")
    );
}

#[test]
fn anchor_memo_entry_is_admitted_before_the_clone() {
    let anchors = BTreeMap::from([(
        "a".to_string(),
        Value::List(vec![Value::Integer(1), Value::Integer(2)]),
    )]);
    let value = Value::Resource("#a".into());
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(b"anchor", &arena, &service)
        .expect("root fits service profile");
    assert_eq!(
        AnchorResolver::new(&anchors, &ctx)
            .expect("empty resolver scope fits")
            .resolve_root(&value)
            .expect("service admits memo"),
        anchors["a"]
    );
    let mut limited = service;
    limited.limits.max_collection_items = 9;
    let (ctx, _) = DecodeContext::from_root_bytes(b"anchor", &arena, &limited)
        .expect("root fits selected profile");
    let error = AnchorResolver::new(&anchors, &ctx)
        .expect("empty resolver scope fits")
        .resolve_root(&value)
        .expect_err("memo entry exceeds nine prior admitted items");
    assert!(
        matches!(error, ResolveError::Resource(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "step_anchor_memo_entry")
    );
}

#[test]
fn anchor_typed_wrapper_is_charged_before_its_clone() {
    let value = Value::Typed("LENGTH_MEASURE".into(), Box::new(Value::Integer(7)));
    let anchors = BTreeMap::new();
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(b"anchor", &arena, &service)
        .expect("root fits service profile");
    assert_eq!(
        AnchorResolver::new(&anchors, &ctx)
            .expect("empty resolver scope fits")
            .resolve_root(&value)
            .expect("service admits typed value"),
        value
    );
    let mut limited = service;
    limited.limits.max_retained_bytes =
        cadmpeg_core::decode::u64_from_index(std::mem::size_of::<Value>());
    let (ctx, _) = DecodeContext::from_root_bytes(b"anchor", &arena, &limited)
        .expect("root fits selected profile");
    let error = AnchorResolver::new(&anchors, &ctx)
        .expect("empty resolver scope fits")
        .resolve_root(&value)
        .expect_err("typed wrapper exceeds the leaf's retained bytes");
    assert!(
        matches!(error, ResolveError::Resource(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "step_anchor_typed_name_copy")
    );
}

#[test]
fn anchor_reference_stack_refuses_collection_limit() {
    let anchors = BTreeMap::from([("a".into(), Value::Integer(7))]);
    let value = Value::Resource("#a".into());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"anchor", &arena, &policy)
        .expect("root fits selected profile");
    let error = AnchorResolver::new(&anchors, &ctx)
        .expect("empty resolver scope fits")
        .resolve_root(&value)
        .expect_err("reference stack needs one item");
    assert!(matches!(
        error,
        ResolveError::Resource(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "step_anchor_reference_stack"
    ));
}

#[test]
fn cyclic_anchor_error_text_refuses_retained_limit() {
    let anchors = BTreeMap::from([("a".into(), Value::Resource("#a".into()))]);
    let value = Value::Resource("#a".into());
    let arena = DecodeArena::new();
    let refused = {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "step_cyclic_anchor_error_text",
            |limit| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = limit;
                let (ctx, _) = DecodeContext::from_root_bytes(b"anchor", &arena, &policy)
                    .expect("root fits retained policy");
                let mut resolver =
                    AnchorResolver::new(&anchors, &ctx).expect("empty resolver scope fits");

                (resolver.resolve_root(&value))
                    .map(|_| ())
                    .map_err(|error| match error {
                        ResolveError::Resource(error) => error,
                        error @ ResolveError::Syntax(_) => {
                            panic!("unexpected anchor error: {error:?}")
                        }
                    })
            },
        );
        matches!(Err::<(), ResolveError>(ResolveError::Resource(error)), Err(ResolveError::Resource(CodecError::ResourceLimit(refusal)))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "step_cyclic_anchor_error_text")
    };
    assert!(
        refused,
        "cyclic anchor error text must reach the resolver caller"
    );
}

#[test]
fn anchor_reference_stack_refuses_retained_limit() {
    let anchors = BTreeMap::from([("a".into(), Value::Integer(7))]);
    let value = Value::Resource("#a".into());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"anchor", &arena, &policy)
        .expect("root fits selected profile");
    let error = AnchorResolver::new(&anchors, &ctx)
        .expect("empty resolver scope fits")
        .resolve_root(&value)
        .expect_err("reference stack needs scoped pointer storage");
    assert!(matches!(
        error,
        ResolveError::Resource(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.operation == "step_anchor_reference_stack_storage"
    ));
}

#[test]
fn reference_bindings_are_admitted_before_map_allocation() {
    let anchors = BTreeMap::new();
    let references = [ReferenceEntry {
        name: ReferenceName::Value(2),
        uri: "#a".into(),
    }];
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(b"reference", &arena, &service)
        .expect("root fits service profile");
    ReferenceResolver::new(&references, &anchors, &ctx).expect("service admits the binding map");
    let mut limited = service;
    limited.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"reference", &arena, &limited)
        .expect("root fits selected profile");
    let error = ReferenceResolver::new(&references, &anchors, &ctx)
        .err()
        .expect("binding must be admitted before collection");
    assert!(
        matches!(error, ResolveError::Resource(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "step_reference_binding_items")
    );
}

#[test]
fn reference_stack_item_is_admitted_before_push() {
    let anchors = BTreeMap::from([("a".into(), Value::Integer(7))]);
    let references = [ReferenceEntry {
        name: ReferenceName::Value(2),
        uri: "#a".into(),
    }];
    let value = Value::ExternalReference(2);
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(b"reference", &arena, &service)
        .expect("root fits service profile");
    assert_eq!(
        ReferenceResolver::new(&references, &anchors, &ctx)
            .expect("create resolver")
            .resolve_value(&value, 0)
            .expect("service admits reference"),
        Value::Integer(7)
    );
    let mut limited = service;
    limited.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(b"reference", &arena, &limited)
        .expect("root fits selected profile");
    let error = ReferenceResolver::new(&references, &anchors, &ctx)
        .expect("binding fits selected profile")
        .resolve_value(&value, 0)
        .expect_err("stack item exceeds the remaining collection allowance");
    assert!(
        matches!(error, ResolveError::Resource(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "step_reference_stack")
    );
}

#[test]
fn reference_stack_refuses_retained_limit() {
    let anchors = BTreeMap::from([("a".into(), Value::Integer(7))]);
    let references = [ReferenceEntry {
        name: ReferenceName::Value(2),
        uri: "#a".into(),
    }];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let binding_bytes = 11 * (std::mem::size_of::<ReferenceName>() + std::mem::size_of::<&str>())
        + 16 * std::mem::size_of::<usize>()
        + 2 * std::mem::align_of::<usize>();
    let stack_bytes = 4 * std::mem::size_of::<ReferenceName>();
    policy.limits.max_materialized_bytes =
        u64::try_from(binding_bytes + stack_bytes - 1).expect("small storage");
    let (ctx, _) = DecodeContext::from_root_bytes(b"reference", &arena, &policy)
        .expect("root fits selected profile");
    let error = ReferenceResolver::new(&references, &anchors, &ctx)
        .expect("binding storage fits selected profile")
        .resolve_value(&Value::ExternalReference(2), 0)
        .expect_err("reference stack needs retained storage");
    assert!(matches!(
        error,
        ResolveError::Resource(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.operation == "step_reference_stack_storage"
    ));
}

#[test]
fn reference_list_slots_are_admitted_before_vector_allocation() {
    let anchors = BTreeMap::new();
    let value = Value::List(vec![
        Value::Integer(1),
        Value::Integer(2),
        Value::Integer(3),
    ]);
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(b"reference", &arena, &service)
        .expect("root fits service profile");
    assert_eq!(
        ReferenceResolver::new(&[], &anchors, &ctx)
            .expect("create resolver")
            .resolve_value(&value, 0)
            .expect("service admits list"),
        value
    );
    let mut limited = service;
    limited.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(b"reference", &arena, &limited)
        .expect("root fits selected profile");
    let error = ReferenceResolver::new(&[], &anchors, &ctx)
        .expect("create resolver")
        .resolve_value(&value, 0)
        .expect_err("three list slots exceed two admitted items");
    assert!(
        matches!(error, ResolveError::Resource(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "step_reference_list_items")
    );
}

#[test]
fn reference_typed_wrapper_is_charged_before_its_clone() {
    let anchors = BTreeMap::new();
    let value = Value::Typed("LENGTH_MEASURE".into(), Box::new(Value::Integer(7)));
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(b"reference", &arena, &service)
        .expect("root fits service profile");
    assert_eq!(
        ReferenceResolver::new(&[], &anchors, &ctx)
            .expect("create resolver")
            .resolve_value(&value, 0)
            .expect("service admits typed value"),
        value
    );
    let mut limited = service;
    limited.limits.max_retained_bytes =
        cadmpeg_core::decode::u64_from_index(std::mem::size_of::<Value>());
    let (ctx, _) = DecodeContext::from_root_bytes(b"reference", &arena, &limited)
        .expect("root fits selected profile");
    let error = ReferenceResolver::new(&[], &anchors, &ctx)
        .expect("create resolver")
        .resolve_value(&value, 0)
        .expect_err("typed wrapper exceeds the leaf's retained bytes");
    assert!(
        matches!(error, ResolveError::Resource(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "step_reference_typed_name_copy")
    );
}

#[test]
fn reference_anchor_copy_is_charged_before_building_bindings() {
    let mut anchors = [AnchorEntry {
        name: "a".into(),
        value: Value::Integer(7),
        tags: Vec::new(),
    }];
    let mut records = BTreeMap::new();
    let references = [ReferenceEntry {
        name: ReferenceName::Value(2),
        uri: "#a".into(),
    }];
    let arena = DecodeArena::new();
    let mut limited = DecodePolicy::service();
    limited.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"reference", &arena, &limited)
        .expect("root fits selected profile");
    let error =
        super::super::resolve_local_references(&mut anchors, &mut records, &references, &ctx)
            .expect_err("anchor copy must be admitted before cloning");
    assert!(
        matches!(error, ResolveError::Resource(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "step_reference_anchor_copies")
    );
}

#[test]
fn parser_propagates_binary_lexeme_resource_refusal() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(\"0A1F2\");ENDSEC;END-ISO-10303-21;";
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes =
        4 + cadmpeg_core::decode::u64_from_index(4 * std::mem::size_of::<String>() + "AP242".len());
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("root fits the test policy");
    let error = crate::parse::parse_with_context(source, &ctx)
        .expect_err("binary lexeme must refuse before its digit allocation");
    assert!(
        matches!(&error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes && limit.operation == "step_binary_lexeme_temp"),
        "{error:?}"
    );
}

#[test]
fn parser_uses_the_decode_session_work_budget() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','','',(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_work_units = 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("root fits the test policy");
    let error = crate::parse::parse_with_context(source, &ctx).expect_err("budget must refuse");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
    ));
}

#[test]
fn parser_accounts_for_owned_value_storage() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(1);ENDSEC;END-ISO-10303-21;";
    let mut value_storage_limit = None;
    for max_retained_bytes in 1..=4096 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = max_retained_bytes;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
                .expect("root fits the test policy");
        let error = crate::parse::parse_with_context(source, &ctx)
            .expect_err("owned value storage must consume retained bytes");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            continue;
        };
        if limit.operation == "step_parse_parameter" {
            value_storage_limit = Some(limit);
            break;
        }
    }
    let limit = value_storage_limit.expect("value storage must have a retained-byte gate");
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::RetainedBytes
    );
    assert!(limit.additional > 0);
    assert!(limit.used <= limit.limit);
}

#[test]
fn parser_accounts_for_record_table_storage() {
    let records = (1..=64).fold(String::new(), |mut records, id| {
        write!(records, "#{id}=ITEM();").expect("write record fixture");
        records
    });
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;"
    );
    crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
        .expect("record-table fixture must parse");
    let mut record_table_limit = None;
    for max_retained_bytes in (1..=131_072).step_by(64) {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = max_retained_bytes;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            source.as_bytes(),
            &arena,
            &policy,
        )
        .expect("root fits the test policy");
        let error = crate::parse::parse_with_context(source.as_bytes(), &ctx)
            .expect_err("record-table storage must consume retained bytes");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            continue;
        };
        if limit.operation == "step_parse_record_table_storage" {
            record_table_limit = Some(limit);
            break;
        }
    }
    let limit = record_table_limit.expect("record-table storage must be charged");
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::RetainedBytes
    );
    assert!(limit.additional > 0);
    assert!(limit.used <= limit.limit);
}

#[test]
fn parser_accounts_for_anchor_tag_collection_storage() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<a>=1 {tag:2};ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    crate::test_support::with_service_context(source, crate::parse::parse_inner)
        .expect("anchor-tag fixture must parse");
    let mut tag_limit = None;
    for max_retained_bytes in 1..=8192 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = max_retained_bytes;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
                .expect("root fits the test policy");
        let error = crate::parse::parse_with_context(source, &ctx)
            .expect_err("anchor-tag storage must consume retained bytes");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            continue;
        };
        if limit.operation == "step_anchor_tag_storage" {
            tag_limit = Some(limit);
            break;
        }
    }
    let limit = tag_limit.expect("anchor-tag storage must have a retained-byte gate");
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::RetainedBytes
    );
    assert!(limit.additional > 0);
}

#[test]
fn anchor_materialization_uses_the_decode_session_budget() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<a>=(1,2,3,4,5,6,7,8);ENDSEC;DATA;#1=ITEM(<#a>);ENDSEC;END-ISO-10303-21;";
    let mut materialization_limit = None;
    for max_work_units in 1..=1024 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_work_units = max_work_units;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
                .expect("root fits the test policy");
        let error = crate::parse::parse_with_context(source, &ctx)
            .expect_err("anchor materialization must consume shared work");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            continue;
        };
        if limit.operation == "step_anchor_materialization" {
            materialization_limit = Some(limit);
            break;
        }
    }
    let limit = materialization_limit.expect("anchor materialization must have a budget gate");
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert!(limit.additional > 0);
    assert!(limit.used <= limit.limit);
}

#[test]
fn local_reference_materialization_uses_the_decode_session_budget() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;3');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<a>=(1,2,3,4,5,6,7,8);ENDSEC;REFERENCE;@2=<#a>;ENDSEC;DATA;#1=ITEM(@2);ENDSEC;END-ISO-10303-21;";
    crate::test_support::with_service_context(source, crate::parse::parse_inner)
        .expect("local-reference fixture must parse");
    let mut materialization_limit = None;
    for max_work_units in 1..=2048 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_work_units = max_work_units;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
                .expect("root fits the test policy");
        let error = crate::parse::parse_with_context(source, &ctx)
            .expect_err("local reference materialization must consume shared work");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            continue;
        };
        if limit.operation == "step_reference_materialization" {
            materialization_limit = Some(limit);
            break;
        }
    }
    let limit = materialization_limit.expect("local references must have a budget gate");
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert!(limit.additional > 0);
    assert!(limit.used <= limit.limit);
}

#[test]
fn parser_bounds_exponential_anchor_expansion() {
    let mut anchors = String::from("<a0>=(1,1);\n");
    for index in 1..40 {
        writeln!(anchors, "<a{index}>=(<#a{}>,<#a{}>);", index - 1, index - 1)
            .expect("write anchor fixture");
    }
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;{anchors}ENDSEC;DATA;#1=ITEM(<#a39>);ENDSEC;END-ISO-10303-21;"
    );
    let error =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .unwrap_err();
    assert!(matches!(
        error,
        crate::parse::ParseError::Resource(CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && limit.used <= limit.limit
                && limit.additional > limit.limit - limit.used
    ));
}

#[test]
fn parser_bounds_aggregate_anchor_materialization() {
    let mut anchors = String::from("<a0>=(1,1);\n");
    for index in 1..18 {
        writeln!(anchors, "<a{index}>=(<#a{}>,<#a{}>);", index - 1, index - 1)
            .expect("write anchor fixture");
    }
    let records = (1..=8).fold(String::new(), |mut records, id| {
        write!(records, "#{id}=ITEM(<#a17>);").expect("write anchor record fixture");
        records
    });
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;{anchors}ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;"
    );
    let error =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .unwrap_err();
    assert!(matches!(
        error,
        crate::parse::ParseError::Resource(CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && limit.used <= limit.limit
                && limit.additional > limit.limit - limit.used
    ));
}
