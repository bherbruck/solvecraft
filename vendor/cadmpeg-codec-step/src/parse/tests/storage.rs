// SPDX-License-Identifier: Apache-2.0
//! Each retained value buffer has one admission owner.

use std::collections::BTreeMap;
use std::mem::size_of;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::lex::Lexer;
use crate::parse::{AnchorResolver, ParseError, Parser, ReferenceResolver, ResolveError, Value};
use crate::test_support::{with_policy_context, with_service_context};

fn parser<'input, 'ctx, 'arena>(
    source: &'input [u8],
    ctx: &'ctx DecodeContext<'arena>,
) -> Parser<'input, 'ctx, 'arena> {
    let mut parser = Parser {
        lexer: Lexer::new(source, ctx),
        current: None,
        last_end: 0,
        depth: 0,
        diagnostics: Vec::new(),
        omitted_entity_names: None,
        defer_lexical_errors: false,
        budget: ctx,
    };
    parser.current = parser.lex_next().expect("fixture lexes");
    parser
}

#[test]
fn moved_text_lexemes_keep_their_single_byte_admission() {
    for source in [
        b"'TEXT'".as_slice(),
        b".TEXT.".as_slice(),
        b"#TEXT".as_slice(),
        b"@TEXT".as_slice(),
        b"<TEXT>".as_slice(),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = if source.starts_with(b"'") { 8 } else { 4 };
        with_policy_context(source, &policy, |source, ctx| {
            parser(source, ctx)
                .value()
                .expect("moving four admitted bytes allocates no second buffer");
        });
    }
}

#[test]
fn moved_record_name_keeps_its_single_byte_admission() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 4;
    with_policy_context(b"ITEM()", &policy, |source, ctx| {
        let partial = parser(source, ctx)
            .partial(true)
            .expect("four-byte name fits once");
        assert_eq!(partial.name, "ITEM");
        assert!(partial.parameters.is_empty());
    });
}

#[test]
fn parameter_and_list_slots_have_one_storage_admission() {
    // Each retained vector grows to its four-slot minimum capacity on first push.
    for (source, slots) in [(b"ITEM(1,2)".as_slice(), 4), (b"ITEM((1,2))".as_slice(), 8)] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 4 + u64_from_index(slots * size_of::<Value>());
        with_policy_context(source, &policy, |source, ctx| {
            parser(source, ctx)
                .partial(true)
                .expect("name plus container slots fit exactly");
        });
    }
}

#[test]
fn typed_parameter_box_is_admitted_before_construction() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 7 + u64_from_index(size_of::<Value>());
    with_policy_context(b"MEASURE(1)", &policy, |source, ctx| {
        assert_eq!(
            parser(source, ctx)
                .value()
                .expect("name plus box fit exactly"),
            Value::Typed("MEASURE".into(), Box::new(Value::Integer(1)))
        );
    });
    policy.limits.max_retained_bytes -= 1;
    with_policy_context(b"MEASURE(1)", &policy, |source, ctx| {
        assert!(
            matches!(parser(source, ctx).value(), Err(ParseError::Resource(CodecError::ResourceLimit(refusal)))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_parse_typed_value_storage")
        );
    });
}

#[test]
fn anchor_leaf_text_copy_has_one_storage_admission() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 4;
    with_policy_context(b"", &policy, |_, ctx| {
        let anchors = BTreeMap::new();
        let value = Value::Enumeration("TEXT".into());
        assert_eq!(
            AnchorResolver::new(&anchors, ctx)
                .expect("empty resolver scope fits")
                .resolve_root(&value)
                .expect("one four-byte copy"),
            value
        );
    });
}

#[test]
fn anchor_memo_retrieval_text_has_one_storage_admission() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 4;
    with_policy_context(b"", &policy, |_, ctx| {
        let anchors = BTreeMap::from([("a".into(), Value::Enumeration("TEXT".into()))]);
        let mut resolver = AnchorResolver::new(&anchors, ctx).expect("empty resolver scope fits");
        resolver.memo.insert("a", (anchors["a"].clone(), 1));
        assert_eq!(
            resolver
                .resolve_root(&Value::Resource("#a".into()))
                .expect("one four-byte memo copy"),
            anchors["a"]
        );
    });
}

#[test]
fn anchor_memo_population_text_has_one_storage_admission() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 4;
    with_policy_context(b"", &policy, |_, ctx| {
        let anchors = BTreeMap::from([("a".into(), Value::Enumeration("TEXT".into()))]);
        let mut resolver = AnchorResolver::new(&anchors, ctx).expect("empty resolver scope fits");
        assert_eq!(
            resolver
                .resolve_root(&Value::Resource("#a".into()))
                .expect("leaf and memo buffers fit once each"),
            anchors["a"]
        );
    });
}

#[test]
fn reference_leaf_text_copy_has_one_storage_admission() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 4;
    with_policy_context(b"", &policy, |_, ctx| {
        let anchors = BTreeMap::new();
        let value = Value::Resource("TEXT".into());
        let mut resolver = ReferenceResolver::new(&[], &anchors, ctx).expect("empty bindings");
        assert_eq!(
            resolver
                .resolve_value(&value, 0)
                .expect("one four-byte copy"),
            value
        );
    });
}

#[test]
fn binding_and_reference_snapshot_text_are_admitted_once_per_copy() {
    let text = "A".repeat(16_384);
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;3');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<a>=.{text}.;ENDSEC;REFERENCE;@2=<#a>;ENDSEC;DATA;#1=ITEM(@2);ENDSEC;END-ISO-10303-21;");
    let mut policy = DecodePolicy::service();
    // Lexeme, binding, anchor output, snapshot, anchor reference pass, record output.
    policy.limits.max_retained_bytes = 6 * u64_from_index(text.len()) + 4096;
    with_policy_context(source.as_bytes(), &policy, |source, ctx| {
        let (exchange, _) = crate::parse::parse_with_context(source, ctx)
            .expect("six text buffers plus fixed structures fit");
        assert_eq!(
            exchange.records()[&1].partials[0].parameters.as_slice(),
            [Value::Enumeration(text.clone())]
        );
    });
}

#[test]
fn copied_value_list_and_box_admit_their_storage() {
    with_service_context(b"", |_, ctx| {
        let value = Value::List(vec![Value::Typed(
            "MEASURE".into(),
            Box::new(Value::String(b"TEXT".to_vec())),
        )]);
        assert_eq!(
            crate::parse::try_clone_value(&value, ctx, "step_test_owned_copy")
                .expect("copy fits service"),
            value
        );
    });
}

#[test]
fn anchor_typed_name_copy_refuses_work_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 3;
    with_policy_context(b"", &policy, |_, ctx| {
        let anchors = BTreeMap::new();
        let value = Value::Typed("MEASURE".into(), Box::new(Value::Integer(1)));
        assert!(
            matches!(AnchorResolver::new(&anchors, ctx).expect("empty resolver scope fits").resolve_root(&value),
            Err(ResolveError::Resource(CodecError::ResourceLimit(refusal)))
                if refusal.dimension == ResourceDimension::WorkUnits
                    && refusal.operation == "step_anchor_typed_name_copy"
                    && refusal.used == 3 && refusal.additional == 7)
        );
    });
}

#[test]
fn reference_typed_name_copy_refuses_work_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 3;
    with_policy_context(b"", &policy, |_, ctx| {
        let anchors = BTreeMap::new();
        let value = Value::Typed("MEASURE".into(), Box::new(Value::Integer(1)));
        let mut resolver = ReferenceResolver::new(&[], &anchors, ctx).expect("empty bindings");
        assert!(matches!(resolver.resolve_value(&value, 0),
            Err(ResolveError::Resource(CodecError::ResourceLimit(refusal)))
                if refusal.dimension == ResourceDimension::WorkUnits
                    && refusal.operation == "step_reference_typed_name_copy"
                    && refusal.used == 3 && refusal.additional == 7));
    });
}
