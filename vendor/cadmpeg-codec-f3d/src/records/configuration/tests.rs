// SPDX-License-Identifier: Apache-2.0

use super::{
    encode_configuration_payload, DesignConfiguration, DesignConfigurationWire,
    CONFIGURATION_CLONE_COUNT,
};
use serde_json::{json, Value};

fn scalar_text_refusal(
    scalar: &super::ConfigurationScalar,
    retained_bytes: u64,
) -> cadmpeg_core::CodecError {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = retained_bytes;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    scalar
        .text_charged(&ctx)
        .expect_err("configuration scalar text must exceed retained budget")
}

#[test]
fn configuration_string_scalar_refuses_retained_limit() {
    let error = scalar_text_refusal(&super::ConfigurationScalar::String("abc".into()), 2);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "project F3D configuration scalar text")
    );
}

#[test]
fn configuration_number_scalar_refuses_retained_limit() {
    let error = scalar_text_refusal(
        &super::ConfigurationScalar::Number(serde_json::Number::from(123)),
        2,
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "project F3D configuration scalar text")
    );
}

#[test]
fn configuration_bool_scalar_refuses_retained_limit() {
    let error = scalar_text_refusal(&super::ConfigurationScalar::Bool(true), 3);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "project F3D configuration scalar text")
    );
}

#[test]
fn configuration_null_scalar_refuses_retained_limit() {
    let error = scalar_text_refusal(&super::ConfigurationScalar::Null, 3);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "project F3D configuration scalar text")
    );
}

fn wire(kind: &str, order: &[&str], payload: Value) -> Value {
    let name = if kind == "rule" {
        "entry.dsgcfgrule"
    } else {
        "entry.dsgcfg"
    };
    Value::Object(
        [
            (
                "id".into(),
                crate::ids::configuration_entry_id(
                    name,
                    &cadmpeg_ir::identity_component!("configuration"),
                )
                .into(),
            ),
            ("entry_name".into(), name.into()),
            ("kind".into(), kind.into()),
            ("variant_order".into(), json!(order)),
            ("payload".into(), payload),
        ]
        .into_iter()
        .collect(),
    )
}

#[test]
fn configuration_borrowed_wire_matches_owned_wire_bytes() {
    for (kind, order, payload) in [
        ("table", vec![], json!({})),
        (
            "table",
            vec!["b", "a"],
            json!({
                "active": "b", "before": [1, null], "after": false,
                "configurations": {
                    "a": {"z": [true], "material": "steel", "parameters": {"a": null, "b": -0.0}},
                    "b": {"suppressed": ["x"], "before": 2, "after": "y"}
                }
            }),
        ),
        (
            "rule",
            vec![],
            json!({"when": {"x": [1, 2]}, "activate": "a"}),
        ),
    ] {
        let record: DesignConfiguration =
            serde_json::from_value(wire(kind, &order, payload)).unwrap();
        let owned = DesignConfigurationWire::from(record.clone());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
    let name = "a 😀# b.dsgcfg";
    let mut escaped = wire("table", &[], json!({}));
    escaped["entry_name"] = json!(name);
    escaped["id"] = json!(crate::ids::configuration_entry_id(
        name,
        &cadmpeg_ir::identity_component!("configuration")
    ));
    let record: DesignConfiguration = serde_json::from_value(escaped).unwrap();
    let owned = DesignConfigurationWire::from(record.clone());
    assert_eq!(
        serde_json::to_vec(&record).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn configuration_variant_sort_work_limit_refuses_before_serialization() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let record: DesignConfiguration = serde_json::from_value(wire(
        "table",
        &["b", "a"],
        json!({"configurations":{"a":{},"b":{}}}),
    ))
    .unwrap();
    let native = crate::native::F3dNative {
        design_configurations: vec![record],
        ..Default::default()
    };
    let arena = DecodeArena::new();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "sort F3D configuration variants",
        |limit| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = limit;
            let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            native
                .store(&limited, &mut cadmpeg_ir::NativeNamespace::default())
                .map_err(Into::into)
        },
    );
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "sort F3D configuration variants"
    ));

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    native.store(&service, &mut namespace).unwrap();
    assert_eq!(namespace.arenas()["design_configurations"].len(), 1);
}

#[test]
fn configuration_native_retained_limit_refuses_before_record_clone() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let record: DesignConfiguration = serde_json::from_value(wire(
        "table",
        &["one"],
        json!({"active":"one", "configurations":{"one":{"parameters":{"x":1},"suppressed":["part"]}}}),
    ))
    .unwrap();
    let arena_name = "design_configurations";
    let needed = serde_json::to_vec(&record).unwrap().len() + arena_name.len();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(needed).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    CONFIGURATION_CLONE_COUNT.with(|count| count.set(0));
    let error = namespace
        .set_arena(&limited, arena_name, std::slice::from_ref(&record))
        .unwrap_err();
    CONFIGURATION_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "serialize native record"
    ));

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    namespace
        .set_arena(&service, arena_name, std::slice::from_ref(&record))
        .unwrap();
    assert_eq!(namespace.arenas()[arena_name].len(), 1);
}

#[test]
fn configuration_payload_retains_absence_empty_fields_and_extension_values() {
    for (order, payload) in [
        (vec![], json!({})),
        (vec![], json!({"configurations": {}})),
        (vec![], json!({"configurations": {"v": {}}})),
        (
            vec!["v"],
            json!({"configurations": {"v": {"parameters": {}, "suppressed": [], "material": ""}}}),
        ),
        (
            vec![""],
            json!({"active": "", "configurations": {"": {"suppressed": ["", "x", "x"]}}}),
        ),
    ] {
        let expected = wire("table", &order, payload.clone());
        let admitted: DesignConfiguration = serde_json::from_value(expected.clone()).unwrap();
        assert_eq!(serde_json::to_value(&admitted).unwrap(), expected);
        assert_eq!(
            serde_json::from_slice::<Value>(&encode_configuration_payload(&admitted).unwrap())
                .unwrap(),
            payload
        );
    }
    for value in [
        Value::Null,
        json!(false),
        json!(2.5),
        json!(""),
        json!([]),
        json!({"nested": [1, null]}),
    ] {
        let payload = json!({"configurations": {"v": {"unknown": value}}, "unknown": value});
        let expected = wire("table", &["v"], payload.clone());
        let admitted: DesignConfiguration = serde_json::from_value(expected.clone()).unwrap();
        assert_eq!(admitted.unknown_member_count(), 2);
        assert_eq!(serde_json::to_value(&admitted).unwrap(), expected);
        assert_eq!(
            serde_json::from_slice::<Value>(&encode_configuration_payload(&admitted).unwrap())
                .unwrap(),
            payload
        );

        let rule = wire(
            "rule",
            &[],
            json!({"when": value, "activate": value, "configurations": value}),
        );
        let admitted: DesignConfiguration = serde_json::from_value(rule.clone()).unwrap();
        assert!(admitted.variants().is_empty());
        assert_eq!(serde_json::to_value(&admitted).unwrap(), rule);
    }
}

#[test]
fn configuration_scalar_projection_preserves_exact_text() {
    let admitted: DesignConfiguration = serde_json::from_value(wire(
        "table",
        &["v"],
        json!({
            "configurations": {"v": {"parameters": {
                "boolean": false, "null": null, "number": u64::MAX,
                "text": " 25 mm ", "zero": -0.0
            }}}
        }),
    ))
    .unwrap();
    let (_, variant) = &admitted.variants()[0];
    let actual: Vec<_> = variant
        .parameters()
        .map(|(key, value)| {
            (
                key.as_str(),
                crate::test_support::with_decode_context(|ctx| value.text_charged(ctx).unwrap()),
            )
        })
        .collect();
    assert_eq!(
        actual,
        [
            ("boolean", "false".into()),
            ("null", "null".into()),
            ("number", "18446744073709551615".into()),
            ("text", " 25 mm ".into()),
            ("zero", "-0.0".into()),
        ]
    );
}

#[test]
fn configuration_order_owns_each_member_once() {
    let payload = json!({"active": "b", "configurations": {"a": {}, "b": {}, "c": {}}});
    for order in [
        vec![],
        vec!["a"],
        vec!["a", "a", "c"],
        vec!["a", "b", "missing"],
        vec!["a", "b", "c", "d"],
    ] {
        assert!(serde_json::from_value::<DesignConfiguration>(wire(
            "table",
            &order,
            payload.clone()
        ))
        .is_err());
    }
    let admitted: DesignConfiguration =
        serde_json::from_value(wire("table", &["c", "a", "b"], payload)).unwrap();
    assert_eq!(
        admitted
            .variants()
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["c", "a", "b"]
    );
    assert_eq!(admitted.active(), Some("b"));
    assert_eq!(
        encode_configuration_payload(&admitted).unwrap(),
        br#"{"active":"b","configurations":{"c":{},"a":{},"b":{}}}"#
    );
}

#[test]
fn configuration_known_fields_reject_wrong_json_kinds() {
    for value in [
        Value::Null,
        json!(false),
        json!(1),
        json!("s"),
        json!([]),
        json!({}),
    ] {
        for field in ["parameters", "suppressed", "material"] {
            let payload = json!({"configurations": {"v": {field: value}}});
            let valid = match field {
                "parameters" => value.is_object(),
                "suppressed" => value.is_array(),
                "material" => value.is_string(),
                _ => unreachable!(),
            };
            assert_eq!(
                serde_json::from_value::<DesignConfiguration>(wire("table", &["v"], payload))
                    .is_ok(),
                valid,
                "{field}: {value}"
            );
        }
    }
    for value in [json!({}), json!([])] {
        assert!(serde_json::from_value::<DesignConfiguration>(wire(
            "table",
            &["v"],
            json!({"configurations": {"v": {"parameters": {"p": value}}}})
        ))
        .is_err());
    }
    for value in [Value::Null, json!(false), json!(1), json!({}), json!([])] {
        assert!(serde_json::from_value::<DesignConfiguration>(wire(
            "table",
            &["v"],
            json!({"configurations": {"v": {"suppressed": [value]}}})
        ))
        .is_err());
    }
}

mod admission;
