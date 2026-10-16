// SPDX-License-Identifier: Apache-2.0
use super::parse_configuration_payload;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn refuse(bytes: &[u8], policy: DecodePolicy, dimension: ResourceDimension, operation: &str) {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(parse_configuration_payload(&ctx, "table.dsgcfg", bytes),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == dimension && failure.operation == operation),
        "{operation}"
    );
}

#[test]
fn configuration_json_array_members_refuse_collection_limit() {
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;
    refuse(
        b"[null,true]",
        policy,
        ResourceDimension::CollectionItems,
        "f3d configuration JSON array member",
    );
}

#[test]
fn configuration_json_object_entries_refuse_collection_limit() {
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;
    refuse(
        br#"{"a":null,"b":true}"#,
        policy,
        ResourceDimension::CollectionItems,
        "f3d configuration JSON object entry",
    );
}

#[test]
fn configuration_json_key_refuses_retained_limit() {
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 1;
    refuse(
        br#"{"\u00e9":null}"#,
        policy,
        ResourceDimension::RetainedBytes,
        "f3d configuration JSON key",
    );
}

#[test]
fn configuration_json_string_refuses_retained_limit() {
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 1;
    refuse(
        br#""\u00e9""#,
        policy,
        ResourceDimension::RetainedBytes,
        "f3d configuration JSON text",
    );
}

#[test]
fn configuration_json_nested_values_refuse_depth_limit() {
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 2;
    refuse(
        b"[[null]]",
        policy,
        ResourceDimension::RecursionDepth,
        "f3d configuration JSON depth",
    );
}

#[test]
fn configuration_json_values_refuse_work_limit() {
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = 2;
    refuse(
        b"[null,true]",
        policy,
        ResourceDimension::WorkUnits,
        "f3d configuration JSON value work",
    );
}

#[test]
fn configuration_json_scratch_refuses_materialized_limit() {
    let mut policy = DecodePolicy::default();
    policy.limits.max_materialized_bytes = 3;
    refuse(
        b"null",
        policy,
        ResourceDimension::MaterializedBytes,
        "f3d configuration JSON",
    );
}

#[test]
fn configuration_json_diagnostic_refuses_retained_limit() {
    let bytes = b"[";
    let error = serde_json::from_slice::<serde_json::Value>(bytes).unwrap_err();
    let message = format!("invalid F3D configuration JSON table.dsgcfg: {error}");
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = u64::try_from(message.len() - 1).unwrap();
    refuse(
        bytes,
        policy,
        ResourceDimension::RetainedBytes,
        "f3d configuration JSON diagnostic",
    );
}

#[test]
fn configuration_json_raw_text_refuses_retained_limit() {
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes =
        u64::try_from("$serde_json::private::RawValue".len()).unwrap() + 3;
    refuse(
        br#"{"$serde_json::private::RawValue":"null"}"#,
        policy,
        ResourceDimension::RetainedBytes,
        "f3d configuration raw JSON text",
    );
}

#[test]
fn configuration_json_values_and_diagnostics_match_serde() {
    for text in [
        "null",
        "true",
        "false",
        "-0.0",
        "-9223372036854775808",
        "18446744073709551615",
        "1.25e-200",
        "[{},[null,true,false],\"\\u00e9\"]",
        "{\"a\":1,\"a\":2,\"\\u00e9\":\"\\ud83d\\ude00\"}",
        "{\"$serde_json::private::RawValue\":\"[true,null]\"}",
        "{\"$serde_json::private::RawValue\":\"[\"}",
        "{\"$serde_json::private::RawValue\":\"null true\"}",
        "{\"$serde_json::private::RawValue\":false}",
        "{\"$serde_json::private::RawValue\":\"null\",\"x\":1}",
        "{\"a\":0,\"$serde_json::private::RawValue\":\"null\"}",
        "[",
        "{",
        "[true,]",
        "{\"a\":}",
        "null true",
        "1e400",
    ] {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let actual = parse_configuration_payload(&ctx, "table.dsgcfg", text.as_bytes());
        match serde_json::from_str::<serde_json::Value>(text) {
            Ok(expected) => {
                let actual = actual.unwrap();
                assert_eq!(actual, expected, "{text}");
                assert_eq!(
                    serde_json::to_vec(&actual).unwrap(),
                    serde_json::to_vec(&expected).unwrap()
                );
            }
            Err(expected) => assert!(
                matches!(actual, Err(CodecError::Malformed(message))
                if message == format!("invalid F3D configuration JSON table.dsgcfg: {expected}")),
                "{text}"
            ),
        }
    }
}

#[test]
fn configuration_numeric_scalar_text_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 2;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let scalar = crate::records::configuration::ConfigurationScalar::Number(
        serde_json::Number::from_f64(2.5).unwrap(),
    );
    assert!(
        matches!(super::super::configuration_scalar_text(&ctx, &scalar),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::RetainedBytes
                && failure.operation == "f3d configuration scalar text")
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            super::super::configuration_scalar_text(decode_ctx, &scalar)
        })
        .unwrap(),
        "2.5"
    );
}

#[test]
fn configuration_json_array_capacity_refuses_retained_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        4 * cadmpeg_core::decode::u64_from_index(std::mem::size_of::<serde_json::Value>()) - 1;
    refuse(
        b"[null]",
        policy,
        ResourceDimension::RetainedBytes,
        "f3d configuration JSON array allocation",
    );
}

#[test]
fn configuration_json_object_node_refuses_retained_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    refuse(
        br#"{"a":null}"#,
        policy,
        ResourceDimension::RetainedBytes,
        "f3d configuration JSON object allocation",
    );
}

#[test]
fn configuration_json_escaped_text_scratch_refuses_capacity_rounding() {
    let bytes = br#""\naaaaaaaa""#;
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index(bytes.len());
    refuse(
        bytes,
        policy,
        ResourceDimension::MaterializedBytes,
        "f3d configuration JSON",
    );
}
