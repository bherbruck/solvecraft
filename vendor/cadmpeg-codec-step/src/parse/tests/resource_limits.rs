// SPDX-License-Identifier: Apache-2.0
//! Local depth and expansion ceilings retain their resource failure class.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::parse::{
    AnchorResolver, ReferenceEntry, ReferenceName, ReferenceResolver, ResolveError, Value,
};
use crate::test_support::{with_policy_context, with_service_context};

fn assert_local_refusal(error: ResolveError, operation: &'static str) {
    let parsed = error.into_parse_error(0).into_codec_error();
    assert!(matches!(parsed, CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::Codec(operation)
            && refusal.operation == operation));
}

#[test]
fn parameter_local_depth_refuses_as_resource() {
    std::thread::Builder::new().stack_size(16 * 1024 * 1024).spawn(|| {
        let nested = format!("{}1{}", "(".repeat(257), ")".repeat(257));
        let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM({nested});ENDSEC;END-ISO-10303-21;");
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 1024;
        with_policy_context(source.as_bytes(), &policy, |source, ctx| {
            assert!(matches!(crate::parse::parse_with_context(source, ctx),
                Err(CodecError::ResourceLimit(refusal)) if refusal.operation == "step_parse_parameter_depth_limit"));
        });
    }).expect("fixture operation succeeds").join().expect("fixture operation succeeds");
}

#[test]
fn anchor_local_depth_refuses_as_resource() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1024;
    with_policy_context(b"", &policy, |_, ctx| {
        let anchors = BTreeMap::new();
        let mut stack_storage = ctx
            .reserve_scoped(0, "test anchor stack")
            .expect("empty stack scope fits");
        let error = AnchorResolver::new(&anchors, ctx)
            .expect("empty resolver scope fits")
            .resolve(
                &Value::Integer(1),
                &mut Vec::new(),
                &mut stack_storage,
                10,
                256,
            )
            .expect_err("local ceiling refuses");
        assert_local_refusal(error, "step_anchor_depth_limit");
    });
}

#[test]
fn reference_local_depth_refuses_as_resource() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1024;
    with_policy_context(b"", &policy, |_, ctx| {
        let anchors = BTreeMap::new();
        let error = ReferenceResolver::new(&[], &anchors, ctx)
            .expect("fixture operation succeeds")
            .resolve_value(&Value::Integer(1), 256)
            .expect_err("local ceiling refuses");
        assert_local_refusal(error, "step_reference_depth_limit");
    });
}

#[test]
fn anchor_memo_output_node_slice_refuses_as_resource() {
    with_service_context(b"", |_, ctx| {
        let anchors = BTreeMap::from([(
            "a".into(),
            Value::List(vec![Value::Integer(1), Value::Integer(2)]),
        )]);
        let mut resolver = AnchorResolver::new(&anchors, ctx).expect("empty resolver scope fits");
        let value = Value::Resource("#a".into());
        resolver
            .resolve_root(&value)
            .expect("fixture operation succeeds");
        resolver.remaining_nodes = 2;
        assert_local_refusal(
            resolver
                .resolve_root(&value)
                .expect_err("local ceiling refuses"),
            "step_anchor_output_node_limit",
        );
    });
}

#[test]
fn anchor_first_expansion_output_node_slice_refuses_as_resource() {
    with_service_context(b"", |_, ctx| {
        let anchors = BTreeMap::from([(
            "a".into(),
            Value::List(vec![Value::Integer(1), Value::Integer(2)]),
        )]);
        let mut resolver = AnchorResolver::new(&anchors, ctx).expect("empty resolver scope fits");
        resolver.remaining_nodes = 2;
        assert_local_refusal(
            resolver
                .resolve_root(&Value::Resource("#a".into()))
                .expect_err("local ceiling refuses"),
            "step_anchor_output_node_limit",
        );
    });
}

#[test]
fn reference_output_node_slice_refuses_before_leaf_copy() {
    with_service_context(b"", |_, ctx| {
        let anchors = BTreeMap::from([("a".into(), Value::Enumeration("TEXT".into()))]);
        let references = [ReferenceEntry {
            name: ReferenceName::Value(2),
            uri: "#a".into(),
        }];
        let mut resolver =
            ReferenceResolver::new(&references, &anchors, ctx).expect("fixture operation succeeds");
        resolver.remaining_nodes = 1;
        assert_eq!(
            resolver
                .resolve_value(&Value::ExternalReference(2), 0)
                .expect("fixture operation succeeds"),
            anchors["a"]
        );
        assert_local_refusal(
            resolver
                .resolve_value(&Value::ExternalReference(2), 0)
                .expect_err("local ceiling refuses"),
            "step_reference_output_node_limit",
        );
    });
}

#[test]
fn reference_output_node_slice_refuses_before_list_storage() {
    with_service_context(b"", |_, ctx| {
        let anchors = BTreeMap::from([(
            "a".into(),
            Value::List(vec![Value::Integer(1), Value::Integer(2)]),
        )]);
        let references = [ReferenceEntry {
            name: ReferenceName::Value(2),
            uri: "#a".into(),
        }];
        let mut resolver =
            ReferenceResolver::new(&references, &anchors, ctx).expect("fixture operation succeeds");
        resolver.remaining_nodes = 2;
        assert_local_refusal(
            resolver
                .resolve_value(&Value::ExternalReference(2), 0)
                .expect_err("local ceiling refuses"),
            "step_reference_output_node_limit",
        );
    });
}

#[test]
fn value_node_count_depth_refuses_in_the_depth_dimension() {
    let mut value = Value::Integer(1);
    for _ in 0..256 {
        value = Value::List(vec![value]);
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1024;
    with_policy_context(b"", &policy, |_, ctx| {
        assert_local_refusal(
            crate::parse::value_node_count(&value, 1_000_000, ctx)
                .expect_err("local ceiling refuses"),
            "step_value_node_count_depth_limit",
        );
    });
}

#[test]
fn value_node_count_nodes_refuse_in_the_node_dimension() {
    with_service_context(b"", |_, ctx| {
        assert_local_refusal(
            crate::parse::value_node_count(&Value::Integer(1), 0, ctx)
                .expect_err("local ceiling refuses"),
            "step_value_node_count_node_limit",
        );
    });
}

#[test]
fn unknown_record_omits_non_finite_real_at_lex_admission() {
    const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=UNKNOWN_ITEM(1.E9999);ENDSEC;END-ISO-10303-21;";
    with_service_context(SOURCE, |source, ctx| {
        let (exchange, diagnostics) = crate::parse::parse_with_context(source, ctx)
            .expect("literal failure has a bounded record");
        assert!(exchange.records().is_empty());
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == crate::parse::ParseDiagnosticKind::RecordOmitted
                && diagnostic.message.contains("finite binary64 range")
        }));
    });
}
