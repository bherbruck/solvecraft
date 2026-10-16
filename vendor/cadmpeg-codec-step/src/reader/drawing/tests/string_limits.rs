// SPDX-License-Identifier: Apache-2.0
//! Resource limits for STEP drawing value text.

use std::collections::{BTreeMap, HashSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::parse::Value;
use crate::reader::RecordExt;

const HEADER: &str = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;";
const TAIL: &str = "ENDSEC;END-ISO-10303-21;";

fn exchange(records: &str) -> (String, crate::parse::Exchange) {
    let source = format!("{HEADER}{records}{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid drawing exchange");
    (source, exchange)
}

fn value_refusal(value: &Value, limit: u64, operation: &str) {
    let (source, exchange) = exchange("#1=ITEM();");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("root fits retained policy");
    assert!(matches!(
        super::super::value_text(&exchange, value, &mut Vec::new(), 1, "value", &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == operation
    ));
}

#[test]
fn drawing_string_value_refuses_retained_limit() {
    value_refusal(
        &Value::String(b"drawing text".to_vec()),
        1,
        "step_string_text",
    );
}

#[test]
fn drawing_constant_value_refuses_retained_limit() {
    value_refusal(
        &Value::ConstantEntity("long-name".into()),
        1,
        "step_drawing_value_text",
    );
}

#[test]
fn drawing_list_text_refuses_retained_limit() {
    value_refusal(
        &Value::List(vec![Value::Integer(1), Value::Integer(2)]),
        5,
        "step_drawing_value_text",
    );
}

#[test]
fn drawing_binary_text_refuses_retained_limit() {
    let (source, exchange) = exchange("#1=ITEM(\"0FF\");");
    let value = exchange
        .records()
        .get(&1)
        .and_then(|record| record.parameter(0))
        .expect("binary parameter");
    let Value::Binary(binary) = value else {
        panic!("expected binary parameter");
    };
    let prefix_len = format!("binary:{}:", binary.bit_len()).len();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(prefix_len).expect("prefix fits u64");
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("root fits retained policy");
    assert!(matches!(
        super::super::value_text(&exchange, value, &mut Vec::new(), 1, "binary", &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_drawing_value_text"
    ));
}

#[test]
fn drawing_decode_propagates_string_refusal() {
    let (source, exchange) = exchange("#1=DRAWING_DEFINITION('Main','detail');");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "step_string_text",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
                .expect("root fits retained policy");
            (super::super::decode(
                &exchange,
                &mut cadmpeg_ir::document::CadIr::empty(),
                &HashSet::new(),
                &BTreeMap::new(),
                &ctx,
            ))
            .map(|_| ())
        },
    );
    assert!(
        matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_string_text")
    );
}

#[test]
fn drawing_sheet_usage_sequence_propagates_string_refusal() {
    let (source, exchange) = exchange("#1=DRAWING_DEFINITION('','');#2=DRAWING_REVISION('',#1,'');#3=REPRESENTATION_CONTEXT('','');#4=PRESENTATION_VIEW('',(),#3);#5=DRAWING_SHEET_REVISION('',(),#3,#2);#6=DRAWING_SHEET_REVISION_USAGE(#5,#2,'sequence');");
    let arena = DecodeArena::new();
    let refused = {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "step_string_text",
            |limit| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = limit;
                let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
                    .expect("root fits retained policy");

                (super::super::decode(
                    &exchange,
                    &mut cadmpeg_ir::document::CadIr::empty(),
                    &HashSet::new(),
                    &BTreeMap::new(),
                    &ctx,
                ))
                .map(|_| ())
            },
        );
        matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "step_string_text")
    };
    assert!(
        refused,
        "no retained limit refused the drawing sequence string"
    );
}
