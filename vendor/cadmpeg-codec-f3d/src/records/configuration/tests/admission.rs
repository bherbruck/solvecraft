// SPDX-License-Identifier: Apache-2.0

use crate::records::configuration::{
    DesignConfiguration, DesignConfigurationKind, DesignConfigurationWire,
};
use crate::test_support::native_test::reject_changed_id;

#[test]
fn configuration_identity_scope_is_independent_of_its_source_entry_name() {
    let name = "Design/Config #1.dsgcfg";
    let valid = serde_json::json!({
        "id": "f3d:xref/component-0/configuration:entry#Design/Config%20%231.dsgcfg",
        "entry_name": name, "kind": "table", "variant_order": [], "payload": {}
    });
    let record: DesignConfiguration = serde_json::from_value(valid.clone()).unwrap();
    assert_eq!(serde_json::to_value(record).unwrap(), valid);
    for id in [
        "f3d::entry#Design/Config%20%231.dsgcfg",
        "f3d:scope with space:entry#Design/Config%20%231.dsgcfg",
        "f3d:scope:extra:entry#Design/Config%20%231.dsgcfg",
        "other:configuration:entry#Design/Config%20%231.dsgcfg",
        "f3d:configuration:other#Design/Config%20%231.dsgcfg",
        "f3d:xref/component-0/configuration:entry#Other.dsgcfg",
    ] {
        let mut invalid = valid.clone();
        invalid["id"] = id.into();
        assert!(
            serde_json::from_value::<DesignConfiguration>(invalid).is_err(),
            "{id}"
        );
    }
}

#[test]
fn configuration_admission_checks_wire_and_variant_order() {
    use crate::records::configuration::DesignConfiguration;
    let wire = serde_json::json!({"id":crate::ids::configuration_entry_id("table.dsgcfg", &cadmpeg_ir::identity_component!("configuration")), "entry_name":"table.dsgcfg", "kind":"table",
        "variant_order":["first","second"], "payload":{"configurations":{"first":{},"second":{}}}});
    let configuration: DesignConfiguration = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&configuration).unwrap(), wire);
    let mut invalid = wire.clone();
    invalid["variant_order"] = serde_json::json!(["first"]);
    assert!(serde_json::from_value::<DesignConfiguration>(invalid).is_err());
    let mut invalid = wire.clone();
    invalid["payload"] = serde_json::json!([]);
    assert!(serde_json::from_value::<DesignConfiguration>(invalid)
        .unwrap_err()
        .to_string()
        .contains("payload"));
    let mut invalid = wire;
    invalid["kind"] = "rule".into();
    assert!(serde_json::from_value::<DesignConfiguration>(invalid).is_err());
}

#[test]
fn configuration_parameter_overrides_require_scalar_values() {
    use crate::records::configuration::{DesignConfiguration, DesignConfigurationKind};
    let admit = |payload: serde_json::Value| {
        crate::test_support::with_decode_context(|ctx| {
            DesignConfiguration::try_new_charged(
                ctx,
                "table.dsgcfg".into(),
                DesignConfigurationKind::Table,
                vec!["variant".into()],
                payload.as_object().unwrap().clone(),
            )
        })
    };
    assert!(admit(
        serde_json::json!({"configurations": {"variant": {"parameters": {
            "string": "25 mm", "number": 2.5, "boolean": true, "null": null
        }}}})
    )
    .is_ok());
    for value in [
        serde_json::json!(["25 mm"]),
        serde_json::json!({"value": "25 mm"}),
    ] {
        let error = admit(serde_json::json!({
            "configurations": {"variant": {"parameters": {"width": value}}}
        }))
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("parameter overrides must be JSON scalars"));
    }
}

#[test]
fn configuration_kind_requires_its_exact_entry_extension() {
    use crate::records::configuration::{DesignConfiguration, DesignConfigurationKind};
    for (kind, valid, invalid) in [
        (
            DesignConfigurationKind::Table,
            "folder/table.dsgcfg",
            "folder/table.dsgcfgrule",
        ),
        (
            DesignConfigurationKind::Rule,
            "folder/rule.dsgcfgrule",
            "folder/rule.dsgcfg",
        ),
    ] {
        let admit = |name: &str| {
            crate::test_support::with_decode_context(|ctx| {
                DesignConfiguration::try_new_charged(
                    ctx,
                    name.into(),
                    kind,
                    Vec::new(),
                    serde_json::Map::new(),
                )
            })
        };
        let record = admit(valid).unwrap();
        let wire = serde_json::to_value(&record).unwrap();
        assert_eq!(
            wire["id"],
            crate::ids::configuration_entry_id(
                valid,
                &cadmpeg_ir::identity_component!("configuration")
            )
        );
        assert_eq!(
            serde_json::to_value(
                serde_json::from_value::<DesignConfiguration>(wire.clone()).unwrap()
            )
            .unwrap(),
            wire
        );
        for name in [invalid, "entry", "entry.DSGCFG", "entry.dsgcfg/suffix"] {
            let error = admit(name).unwrap_err().to_string();
            assert!(error.contains("entry_name"), "{error}");
            let mut malformed = wire.clone();
            malformed["entry_name"] = name.into();
            malformed["id"] = crate::ids::configuration_entry_id(
                name,
                &cadmpeg_ir::identity_component!("configuration"),
            )
            .into();
            assert!(serde_json::from_value::<DesignConfiguration>(malformed).is_err());
        }
    }
}

#[test]
fn configuration_id_binds_the_escaped_entry_name() {
    let name = "Design/Config #1.dsgcfg";
    let wire = DesignConfigurationWire {
        id: crate::ids::configuration_entry_id(
            name,
            &cadmpeg_ir::identity_component!("configuration"),
        ),
        entry_name: name.into(),
        kind: DesignConfigurationKind::Table,
        variant_order: Vec::new(),
        payload: serde_json::json!({}),
    };
    for id in ["id", "f3d:configuration:entry#Design/Config #1.dsgcfg"] {
        assert!(DesignConfiguration::try_from(DesignConfigurationWire {
            id: id.into(),
            ..wire.clone()
        })
        .is_err());
    }
    let record = DesignConfiguration::try_from(wire).unwrap();
    let mut changed_name = serde_json::to_value(&record).unwrap();
    changed_name["entry_name"] = serde_json::json!("Other.dsgcfg");
    assert!(serde_json::from_value::<DesignConfiguration>(changed_name).is_err());
    reject_changed_id(
        record,
        &["id", "f3d:configuration:entry#Design/Config #1.dsgcfg"],
    );
}

#[test]
fn configuration_parameter_map_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let payload = serde_json::json!({
        "configurations": {
            "variant": {"parameters": {"length": 1, "width": 2}}
        }
    });
    let error = DesignConfiguration::try_new_charged(
        &ctx,
        "table.dsgcfg".into(),
        DesignConfigurationKind::Table,
        vec!["variant".into()],
        payload.as_object().unwrap().clone(),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "admit configuration parameter"
    ));
}

#[test]
fn configuration_suppressed_list_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let payload = serde_json::json!({"configurations": {"variant": {"suppressed": ["part"]}}});
    let error = DesignConfiguration::try_new_charged(
        &ctx,
        "table.dsgcfg".into(),
        DesignConfigurationKind::Table,
        vec!["variant".into()],
        payload.as_object().unwrap().clone(),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "admit suppressed configuration member"
    ));
}

#[test]
fn configuration_variant_map_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let payload = serde_json::json!({"configurations": {"variant": {}}});
    let error = DesignConfiguration::try_new_charged(
        &ctx,
        "table.dsgcfg".into(),
        DesignConfigurationKind::Table,
        Vec::new(),
        payload.as_object().unwrap().clone(),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "admit configuration variant"
    ));
}

#[test]
fn configuration_implicit_order_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let payload = serde_json::json!({"configurations": {"variant": {}}});
    let error = DesignConfiguration::try_new_charged(
        &ctx,
        "table.dsgcfg".into(),
        DesignConfigurationKind::Table,
        Vec::new(),
        payload.as_object().unwrap().clone(),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "order configuration variants"
    ));
}

#[test]
fn configuration_explicit_order_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let payload = serde_json::json!({"configurations": {"variant": {}}});
    let error = DesignConfiguration::try_new_charged(
        &ctx,
        "table.dsgcfg".into(),
        DesignConfigurationKind::Table,
        vec!["variant".into()],
        payload.as_object().unwrap().clone(),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "order configuration variants"
    ));
}
