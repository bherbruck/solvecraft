// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::cloned_ref_to_slice_refs))]
//! Decode and project Design configuration records.

use cadmpeg_core::container::ContainerRole;

use crate::container::ContainerScan;
use crate::design::identity::neutral_configuration_id;
use crate::records::configuration::{
    ConfigurationScalar, DesignConfiguration, DesignConfigurationKind,
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, Visitor};
use std::collections::HashSet;
use std::fmt;

mod json;

fn configuration_property_key(
    ctx: &DecodeContext<'_>,
    prefix: &'static str,
    suffix: &str,
    operation: &'static str,
) -> Result<cadmpeg_core::text::NonBlankString, CodecError> {
    let len = prefix
        .len()
        .checked_add(suffix.len())
        .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, 1))?;

    let mut key = ctx.retained_string(len, operation)?;
    key.push_str(prefix);
    key.push_str(suffix);
    cadmpeg_core::text::NonBlankString::new(key)
        .ok_or_else(|| CodecError::malformed("configuration property key is blank"))
}

fn configuration_scalar_text(
    ctx: &DecodeContext<'_>,
    value: &ConfigurationScalar,
) -> Result<String, CodecError> {
    match value {
        ConfigurationScalar::String(text) => {
            ctx.copy_retained_text(text, "f3d configuration parameter value")
        }
        ConfigurationScalar::Number(number) => {
            ctx.format_retained(format_args!("{number}"), "f3d configuration scalar text")
        }
        _ => value.text_charged(ctx),
    }
}

struct ConfigurationMemberOrderSeed<'a, 'b> {
    ctx: &'a DecodeContext<'b>,
    refusal: &'a mut Option<CodecError>,
}

impl<'de> DeserializeSeed<'de> for ConfigurationMemberOrderSeed<'_, '_> {
    type Value = Vec<String>;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for ConfigurationMemberOrderSeed<'_, '_> {
    type Value = Vec<String>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("struct ConfigurationMemberOrder")
    }

    fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        let mut names = Vec::new();
        let mut seen_configurations = false;
        while let Some(field) = map.next_key_seed(json::ConfigurationFieldSeed)? {
            if field {
                if seen_configurations {
                    return Err(serde::de::Error::duplicate_field("configurations"));
                }
                seen_configurations = true;
                names = map.next_value_seed(OrderedVariantNamesSeed {
                    ctx: self.ctx,
                    refusal: &mut *self.refusal,
                })?;
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        Ok(names)
    }
}

struct OrderedVariantNamesSeed<'a, 'b> {
    ctx: &'a DecodeContext<'b>,
    refusal: &'a mut Option<CodecError>,
}

impl<'de> DeserializeSeed<'de> for OrderedVariantNamesSeed<'_, '_> {
    type Value = Vec<String>;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for OrderedVariantNamesSeed<'_, '_> {
    type Value = Vec<String>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a configuration-variant object")
    }

    fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        let mut names = Vec::new();
        let mut unique_storage = self
            .ctx
            .reserve_scoped(0, "f3d configuration unique names")
            .map_err(|error| {
                *self.refusal = Some(error);
                serde::de::Error::custom("configuration variant resource limit")
            })?;
        let mut unique = HashSet::new();
        while let Some(name) = map.next_key_seed(crate::design::json_value::TextSeed {
            ctx: self.ctx,
            refusal: &mut *self.refusal,
            operation: "f3d configuration variant unique name",
            entry: false,
            expected: "a string",
        })? {
            if unique.contains(&name) {
                let message = self.ctx.format_retained(
                    format_args!("duplicate configuration variant {name:?}"),
                    "f3d duplicate configuration variant diagnostic",
                );
                return match message {
                    Ok(message) => Err(serde::de::Error::custom(message)),
                    Err(error) => {
                        *self.refusal = Some(error);
                        Err(serde::de::Error::custom(
                            "configuration variant resource limit",
                        ))
                    }
                };
            }
            {
                let ctx = self.ctx;
                let charge = (|| -> Result<String, CodecError> {
                    let copy = unique_storage.with_storage(|| -> Result<String, CodecError> {
                        let copy =
                            ctx.copy_retained_text(&name, "f3d configuration variant unique name")?;
                        ctx.reserve_set(&mut unique, 1, "f3d configuration variant name index")?;
                        Ok(copy)
                    })?;
                    ctx.reserve_vec(&mut names, 1, "f3d configuration variant order")?;
                    Ok(copy)
                })();
                let copy = match charge {
                    Ok(copy) => copy,
                    Err(error) => {
                        *self.refusal = Some(error);
                        return Err(serde::de::Error::custom(
                            "configuration variant resource limit",
                        ));
                    }
                };
                unique.insert(copy);
            }
            map.next_value::<IgnoredAny>()?;
            names.push(name);
        }
        Ok(names)
    }
}

fn parse_configuration_variant_order(
    ctx: &DecodeContext<'_>,
    entry_name: &str,
    bytes: &[u8],
) -> Result<Vec<String>, CodecError> {
    let _reservation = Some(
        ({
            ctx.reserve_scoped(
                u64::try_from(bytes.len())
                    .map_err(|_| ctx.refuse_codec_limit("f3d configuration order JSON", 0, 1))?,
                "f3d configuration order JSON",
            )
        })?,
    );
    let mut refusal = None;
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let result = ConfigurationMemberOrderSeed {
        ctx,
        refusal: &mut refusal,
    }
    .deserialize(&mut deserializer)
    .and_then(|names| {
        deserializer.end()?;
        Ok(names)
    });
    match result {
        Ok(names) => Ok(names),
        Err(error) => match refusal {
            Some(error) => Err(error),
            None => Err(CodecError::Malformed(ctx.format_retained(
                format_args!("invalid F3D configuration variant order {entry_name}: {error}"),
                "f3d configuration order diagnostic",
            )?)),
        },
    }
}

/// Decode every JSON design-configuration table and rule entry.
pub(crate) fn decode_configurations(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<DesignConfiguration>, CodecError> {
    let mut configurations = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_asset_entry(entry, ContainerRole::DesignConfig))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let payload = json::parse_configuration_payload(ctx, &entry.name, bytes)?;
        let serde_json::Value::Object(payload) = payload else {
            return Err(CodecError::Malformed(ctx.format_retained(
                format_args!("F3D configuration JSON must be an object: {}", entry.name),
                "f3d configuration JSON root diagnostic",
            )?));
        };
        let kind = if entry.name.ends_with(".dsgcfgrule") {
            DesignConfigurationKind::Rule
        } else {
            DesignConfigurationKind::Table
        };
        let variant_order = if kind == DesignConfigurationKind::Table {
            parse_configuration_variant_order(ctx, &entry.name, bytes)?
        } else {
            Vec::new()
        };
        let entry_name = ctx.copy_retained_text(&entry.name, "f3d configuration entry name")?;
        let configuration =
            DesignConfiguration::try_new_charged(ctx, entry_name, kind, variant_order, payload)?;
        ctx.push_vec(
            &mut configurations,
            configuration,
            "f3d configuration record",
        )?;
    }
    let mut names = HashSet::new();
    for configuration in &configurations {
        if names.contains(configuration.entry_name().as_str()) {
            return Err(CodecError::Malformed(ctx.format_retained(
                format_args!(
                    "duplicate F3D configuration identity: {}",
                    configuration.entry_name()
                ),
                "f3d configuration identity diagnostic",
            )?));
        }

        ctx.reserve_set(&mut names, 1, "f3d configuration identity index")?;
        names.insert(configuration.entry_name().as_str());
    }
    Ok(configurations)
}

/// Project named variants from configuration-table JSON into the neutral
/// configuration arena. Rule documents remain in the native arena because a
/// rule is a selector, not a model variant.
pub(crate) fn project_configurations(
    ctx: &DecodeContext<'_>,
    native: &[DesignConfiguration],
) -> Result<Vec<cadmpeg_ir::features::DesignConfiguration>, CodecError> {
    use cadmpeg_ir::features::DesignConfiguration as NeutralConfiguration;
    use std::collections::BTreeMap;

    if native
        .iter()
        .filter(|configuration| !configuration.variants().is_empty())
        .count()
        > 1
    {
        return Err(CodecError::NotImplemented(
            "independent nonempty F3D configuration tables have no shared authored order".into(),
        ));
    }

    let mut projected = Vec::new();
    for table in native {
        let active = table.active();
        for (name, definition) in table.variants() {
            let mut properties = BTreeMap::new();
            for (parameter, value) in definition.parameters() {
                let key = configuration_property_key(
                    ctx,
                    "parameter:",
                    parameter,
                    "f3d configuration parameter key",
                )?;
                let value = configuration_scalar_text(ctx, value)?;
                ctx.insert_btree_map(
                    &mut properties,
                    key,
                    value,
                    "f3d configuration parameter property",
                )?;
            }
            for feature in definition.suppressed() {
                let key = configuration_property_key(
                    ctx,
                    "suppressed:",
                    feature,
                    "f3d configuration suppression key",
                )?;
                ctx.insert_btree_map(
                    &mut properties,
                    key,
                    "true".into(),
                    "f3d configuration suppression property",
                )?;
            }
            let material = definition
                .material()
                .map(|material| ctx.copy_retained_text(material, "f3d configuration material"))
                .transpose()?;
            let ordinal = u32::try_from(projected.len()).map_err(|_| {
                CodecError::Malformed("F3D configuration ordinal exceeds u32".into())
            })?;
            let name = ctx.copy_retained_text(name, "f3d configuration variant name")?;
            {
                ctx.reserve_vec(&mut projected, 1, "f3d projected configuration")?;
            }
            projected.push(NeutralConfiguration {
                id: neutral_configuration_id(ctx, table.entry_name(), &name)?,
                ordinal,
                active: active == Some(name.as_str()),
                source_index: None,
                name: Some(name),
                material,
                properties,
                parameter_overrides: BTreeMap::new(),
                parameter_values: BTreeMap::new(),
                feature_states: BTreeMap::new(),
                bodies: None,
                native_ref: Some(super::identity::configuration_entry_id(
                    ctx,
                    table.entry_name(),
                )?),
            });
        }
    }
    for (rule, payload) in native.iter().filter_map(|rule| Some((rule, rule.rule()?))) {
        let Some(condition) = payload.get("when").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(target) = payload.get("activate").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let mut matches = projected
            .iter_mut()
            .filter(|configuration| configuration.name.as_deref() == Some(target));
        let Some(configuration) = matches.next() else {
            continue;
        };
        if matches.next().is_some() {
            continue;
        }
        let key = configuration_property_key(
            ctx,
            "activation_rule:",
            rule.entry_name(),
            "f3d configuration activation rule key",
        )?;
        let condition =
            ctx.copy_retained_text(condition, "f3d configuration activation rule condition")?;
        ctx.insert_btree_map(
            &mut configuration.properties,
            key,
            condition,
            "f3d configuration activation rule property",
        )?;
    }
    ctx.stable_sort_by(
        &mut projected,
        |left, right| left.id.cmp(&right.id),
        |value| value.id.as_str().len(),
        "sort f3d configuration variants",
    )?;
    Ok(projected)
}

/// Replace name-keyed configuration properties with stable parameter references
/// when exactly one neutral parameter has the named source identity.
pub(crate) fn bind_configuration_parameter_overrides(
    ctx: &DecodeContext<'_>,
    configurations: &mut [cadmpeg_ir::features::DesignConfiguration],
    parameters: &[cadmpeg_ir::features::DesignParameter],
) -> Result<(), CodecError> {
    for configuration in configurations {
        let mut refusal = None;
        configuration.properties.retain(|key, expression| {
            if refusal.is_some() {
                return true;
            }
            let Some(name) = key.as_str().strip_prefix("parameter:") else {
                return true;
            };
            let mut matches = parameters.iter().filter(|parameter| parameter.name == name);
            let Some(parameter) = matches.next() else {
                return true;
            };
            if matches.next().is_some() {
                return true;
            }
            let id = {
                match ctx
                    .copy_retained(
                        parameter.id.as_str().as_bytes(),
                        "f3d configuration parameter override id",
                    )
                    .and_then(|bytes| {
                        String::from_utf8(bytes).map_err(|_| {
                            CodecError::malformed("validated configuration ID is not UTF-8")
                        })
                    })
                    .and_then(|value| {
                        cadmpeg_ir::features::ParameterId::try_from(value)
                            .map_err(CodecError::malformed)
                    }) {
                    Ok(id) => id,
                    Err(error) => {
                        refusal = Some(error);
                        return true;
                    }
                }
            };
            {
                if let Err(error) = ctx.admit_btree_entry(
                    &configuration.parameter_overrides,
                    &id,
                    "f3d configuration parameter override",
                ) {
                    refusal = Some(error);
                    return true;
                }
            }
            configuration
                .parameter_overrides
                .insert(id, std::mem::take(expression));
            false
        });
        if let Some(error) = refusal {
            return Err(error);
        }
    }
    Ok(())
}

/// Replace name-keyed suppression properties with stable feature references
/// when exactly one neutral feature has the named source identity.
pub(crate) fn bind_configuration_suppressed_features(
    ctx: &DecodeContext<'_>,
    configurations: &mut [cadmpeg_ir::features::DesignConfiguration],
    features: &[cadmpeg_ir::features::Feature],
) -> Result<(), CodecError> {
    for configuration in configurations {
        let mut refusal = None;
        configuration.properties.retain(|key, _| {
            if refusal.is_some() {
                return true;
            }
            let Some(name) = key.as_str().strip_prefix("suppressed:") else {
                return true;
            };
            let mut matches = features
                .iter()
                .filter(|feature| feature.name.as_deref() == Some(name));
            let Some(feature) = matches.next() else {
                return true;
            };
            if matches.next().is_some() {
                return true;
            }
            let projected = (|| -> Result<_, CodecError> {
                let id = feature
                    .id
                    .try_clone_for_decode(ctx, "f3d configuration suppressed feature id")?;
                {
                    ctx.admit_btree_entry(
                        &configuration.feature_states,
                        &id,
                        "f3d configuration suppressed feature state",
                    )?;
                }
                let mut dependencies = cadmpeg_ir::features::DistinctMembers::default();
                for dependency in &feature.dependencies {
                    let copied = dependency
                        .try_clone_for_decode(ctx, "f3d configuration suppressed dependency id")?;
                    dependencies.insert(ctx, copied, "f3d configuration suppressed dependency")?;
                }
                Ok((
                    id,
                    cadmpeg_ir::features::ConfigurationFeatureState {
                        evaluation: cadmpeg_ir::features::ConfigurationEvaluation::Suppressed {},
                        dependencies,
                        definition: feature
                            .evaluation
                            .definition()
                            .try_clone_for_decode(ctx, "f3d configuration suppressed definition")?,
                    },
                ))
            })();
            let (id, state) = match projected {
                Ok(projected) => projected,
                Err(error) => {
                    refusal = Some(error);
                    return true;
                }
            };
            configuration.feature_states.insert(id, state);
            false
        });
        if let Some(error) = refusal {
            return Err(error);
        }
    }
    Ok(())
}

pub(crate) fn unresolved_configuration_parameter_override_count(
    projected: &[cadmpeg_ir::features::DesignConfiguration],
) -> usize {
    projected
        .iter()
        .flat_map(|configuration| configuration.properties.keys())
        .filter(|key| key.as_str().starts_with("parameter:"))
        .count()
}

pub(crate) fn unresolved_configuration_suppressed_feature_count(
    projected: &[cadmpeg_ir::features::DesignConfiguration],
) -> usize {
    projected
        .iter()
        .flat_map(|configuration| configuration.properties.keys())
        .filter(|key| key.as_str().starts_with("suppressed:"))
        .count()
}

pub(crate) fn unresolved_configuration_rule_count(
    native: &[DesignConfiguration],
    projected: &[cadmpeg_ir::features::DesignConfiguration],
) -> usize {
    native
        .iter()
        .filter(|rule| rule.rule().is_some_and(|payload| !payload.is_empty()))
        .filter(|rule| {
            !projected.iter().any(|configuration| {
                configuration.properties.keys().any(|key| {
                    key.as_str().strip_prefix("activation_rule:") == Some(rule.entry_name())
                })
            })
        })
        .count()
}

pub(crate) fn unresolved_configuration_member_count(native: &[DesignConfiguration]) -> usize {
    native
        .iter()
        .map(DesignConfiguration::unknown_member_count)
        .sum()
}

#[cfg(test)]
mod tests {
    mod definition_copy;

    use super::{
        bind_configuration_parameter_overrides, bind_configuration_suppressed_features,
        parse_configuration_variant_order, project_configurations,
        unresolved_configuration_member_count, unresolved_configuration_parameter_override_count,
        unresolved_configuration_rule_count, unresolved_configuration_suppressed_feature_count,
    };
    use crate::records::configuration::{
        encode_configuration_payload, DesignConfiguration, DesignConfigurationKind,
    };
    use cadmpeg_ir::features::{
        DesignParameter as NeutralParameter, Feature, FeatureDefinition, FeatureId,
        FeatureOperation, ParameterId,
    };
    use std::collections::BTreeMap;

    #[test]
    fn configuration_variants_follow_serialized_member_order() {
        crate::test_support::with_decode_context(|decode_ctx| {
            let bytes =
                br#"{"configurations":{"Small":{},"Medium":{},"Large":{}},"active":"Medium"}"#;
            let payload: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            let variant_order =
                parse_configuration_variant_order(decode_ctx, "table.dsgcfg", bytes).unwrap();
            assert_eq!(variant_order, ["Small", "Medium", "Large"]);

            let table = crate::test_support::with_decode_context(|ctx| {
                DesignConfiguration::try_new_charged(
                    ctx,
                    "table.dsgcfg".into(),
                    DesignConfigurationKind::Table,
                    variant_order,
                    (payload).as_object().unwrap().clone(),
                )
            })
            .unwrap();
            let projected =
                project_configurations(decode_ctx, std::slice::from_ref(&table)).unwrap();
            let mut authored = projected
                .iter()
                .filter_map(|configuration| {
                    Some((configuration.name.as_deref()?, configuration.ordinal))
                })
                .collect::<Vec<_>>();
            authored.sort_by_key(|(_, ordinal)| *ordinal);
            assert_eq!(authored, [("Small", 0), ("Medium", 1), ("Large", 2)]);
            let encoded = encode_configuration_payload(&table).unwrap();
            assert_eq!(
                parse_configuration_variant_order(decode_ctx, "table.dsgcfg", &encoded).unwrap(),
                ["Small", "Medium", "Large"]
            );

            assert!(parse_configuration_variant_order(
                decode_ctx,
                "table.dsgcfg",
                br#"{"configurations":{"Small":{},"Small":{}}}"#
            )
            .is_err());
            assert!(parse_configuration_variant_order(
                decode_ctx,
                "table.dsgcfg",
                br#"{"configurations":null}"#
            )
            .is_err());
        });
    }

    #[test]
    fn configuration_identity_is_stable_across_table_order_and_delimiter_names() {
        let first = crate::ids::neutral_configuration_id("asset/a#b.dsgcfg", "c");
        let second = crate::ids::neutral_configuration_id("asset/a.dsgcfg", "b#c");
        assert_ne!(first, second);
    }

    #[test]
    fn configuration_unknown_members_are_counted_at_each_semantic_level() {
        let native = [
            crate::test_support::with_decode_context(|ctx| {
                DesignConfiguration::try_new_charged(
                    ctx,
                    "table.dsgcfg".into(),
                    DesignConfigurationKind::Table,
                    vec!["variant".into()],
                    (serde_json::json!({
                        "active": "variant",
                        "table_unknown": 1,
                        "configurations": {
                            "variant": {
                                "parameters": {},
                                "suppressed": [],
                                "material": "steel",
                                "variant_unknown": true
                            }
                        }
                    }))
                    .as_object()
                    .unwrap()
                    .clone(),
                )
            })
            .unwrap(),
            crate::test_support::with_decode_context(|ctx| {
                DesignConfiguration::try_new_charged(
                    ctx,
                    "rule.dsgcfgrule".into(),
                    DesignConfigurationKind::Rule,
                    Vec::new(),
                    (serde_json::json!({
                        "when": "width > 20 mm",
                        "activate": "variant",
                        "rule_unknown": null
                    }))
                    .as_object()
                    .unwrap()
                    .clone(),
                )
            })
            .unwrap(),
        ];
        assert_eq!(unresolved_configuration_member_count(&native), 3);
    }

    #[test]
    fn configuration_rule_without_the_typed_pair_is_retained_not_rejected() {
        crate::test_support::with_decode_context(|decode_ctx| {
            let native = [crate::test_support::with_decode_context(|ctx| {
                DesignConfiguration::try_new_charged(
                    ctx,
                    "partial.dsgcfgrule".into(),
                    DesignConfigurationKind::Rule,
                    Vec::new(),
                    (serde_json::json!({"when": "width > 20 mm", "vendorExtension": 7}))
                        .as_object()
                        .unwrap()
                        .clone(),
                )
            })
            .unwrap()];
            let projected =
                project_configurations(decode_ctx, &native).expect("empty rule projection");
            assert!(projected.is_empty());
            assert_eq!(unresolved_configuration_rule_count(&native, &projected), 1);
        });
    }

    #[test]
    fn configuration_rules_bind_only_one_named_variant() {
        crate::test_support::with_decode_context(|decode_ctx| {
            let table = |entry_name: &str, variant_name: &str| {
                crate::test_support::with_decode_context(|ctx| {
                    DesignConfiguration::try_new_charged(
                        ctx,
                        entry_name.into(),
                        DesignConfigurationKind::Table,
                        vec![variant_name.into()],
                        (serde_json::json!({"configurations": {variant_name: {}}}))
                            .as_object()
                            .unwrap()
                            .clone(),
                    )
                })
                .unwrap()
            };
            let rule = crate::test_support::with_decode_context(|ctx| {
                DesignConfiguration::try_new_charged(
                    ctx,
                    "rule.dsgcfgrule".into(),
                    DesignConfigurationKind::Rule,
                    Vec::new(),
                    (serde_json::json!({"when": "width > 20 mm", "activate": "wide"}))
                        .as_object()
                        .unwrap()
                        .clone(),
                )
            })
            .unwrap();
            let native = [table("table.dsgcfg", "wide"), rule.clone()];
            let projected =
                project_configurations(decode_ctx, &native).expect("ordered configuration table");
            assert_eq!(
                projected[0].properties["activation_rule:rule.dsgcfgrule"],
                "width > 20 mm"
            );
            assert_eq!(unresolved_configuration_rule_count(&native, &projected), 0);

            let ambiguous = [
                table("first.dsgcfg", "wide"),
                table("second.dsgcfg", "wide"),
                rule,
            ];
            let error = project_configurations(decode_ctx, &ambiguous)
                .expect_err("independent nonempty tables have no shared order");
            assert!(error
                .to_string()
                .contains("configuration tables have no shared authored order"));
        });
    }

    #[test]
    fn configuration_parameter_overrides_bind_only_unique_parameter_names() {
        crate::test_support::with_decode_context(|decode_ctx| {
            let table = crate::test_support::with_decode_context(|ctx| {
                DesignConfiguration::try_new_charged(
                    ctx,
                    "table.dsgcfg".into(),
                    DesignConfigurationKind::Table,
                    vec!["wide".into()],
                    (serde_json::json!({
                        "configurations": {"wide": {"parameters": {"width": "25 mm"}}}
                    }))
                    .as_object()
                    .unwrap()
                    .clone(),
                )
            })
            .unwrap();
            let parameter = NeutralParameter {
                id: ParameterId::mint("f3d:model:parameter#width").expect("identity grammar"),
                owner: None,
                ordinal: 0,
                name: "width".into(),
                expression: "10 mm".into(),
                display: None,
                value: None,
                dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                properties: BTreeMap::new(),
                pmi: None,
                native_ref: None,
            };
            let mut projected =
                project_configurations(decode_ctx, &[table]).expect("ordered configuration table");
            bind_configuration_parameter_overrides(
                decode_ctx,
                &mut projected,
                std::slice::from_ref(&parameter),
            )
            .unwrap();
            assert_eq!(projected[0].parameter_overrides[&parameter.id], "25 mm");
            assert!(projected[0].properties.is_empty());
            assert_eq!(
                unresolved_configuration_parameter_override_count(&projected),
                0
            );

            let duplicate = NeutralParameter {
                id: ParameterId::mint("f3d:model:parameter#other-width").expect("identity grammar"),
                ..parameter.clone()
            };
            let mut ambiguous = project_configurations(
                decode_ctx,
                &[crate::test_support::with_decode_context(|ctx| {
                    DesignConfiguration::try_new_charged(
                        ctx,
                        "other.dsgcfg".into(),
                        DesignConfigurationKind::Table,
                        vec!["wide".into()],
                        (serde_json::json!({
                            "configurations": {"wide": {"parameters": {"width": "25 mm"}}}
                        }))
                        .as_object()
                        .unwrap()
                        .clone(),
                    )
                })
                .unwrap()],
            )
            .expect("ordered configuration table");
            bind_configuration_parameter_overrides(
                decode_ctx,
                &mut ambiguous,
                &[parameter, duplicate],
            )
            .unwrap();
            assert!(ambiguous[0].parameter_overrides.is_empty());
            assert_eq!(
                unresolved_configuration_parameter_override_count(&ambiguous),
                1
            );
        });
    }

    #[test]
    fn configuration_suppression_binds_only_unique_feature_names() {
        crate::test_support::with_decode_context(|decode_ctx| {
            let table = crate::test_support::with_decode_context(|ctx| {
                DesignConfiguration::try_new_charged(
                    ctx,
                    "table.dsgcfg".into(),
                    DesignConfigurationKind::Table,
                    vec!["alternate".into()],
                    (serde_json::json!({
                        "configurations": {"alternate": {"suppressed": ["Fillet 1"]}}
                    }))
                    .as_object()
                    .unwrap()
                    .clone(),
                )
            })
            .unwrap();
            let feature = Feature {
                id: FeatureId::mint("f3d:model:feature#fillet-1").expect("identity grammar"),
                ordinal: 0,
                name: Some("Fillet 1".into()),
                suppressed: Some(false),
                dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                source_properties: BTreeMap::new(),
                source_tag: None,
                source_text: None,
                source_content: cadmpeg_ir::features::FeatureContent::default(),

                evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                    FeatureDefinition::Operation(FeatureOperation::Native {
                        kind: "Fillet".into(),
                        parameters: BTreeMap::new(),
                    }),
                ),
                native_ref: None,
            };
            let mut projected =
                project_configurations(decode_ctx, &[table]).expect("ordered configuration table");
            bind_configuration_suppressed_features(
                decode_ctx,
                &mut projected,
                std::slice::from_ref(&feature),
            )
            .unwrap();
            assert_eq!(
                projected[0].suppressed_features().collect::<Vec<_>>(),
                [&feature.id]
            );
            assert!(projected[0].properties.is_empty());
            assert_eq!(
                unresolved_configuration_suppressed_feature_count(&projected),
                0
            );

            let duplicate = Feature {
                id: FeatureId::mint("f3d:model:feature#other-fillet-1").expect("identity grammar"),
                ..feature.clone()
            };
            let mut ambiguous = project_configurations(
                decode_ctx,
                &[crate::test_support::with_decode_context(|ctx| {
                    DesignConfiguration::try_new_charged(
                        ctx,
                        "other.dsgcfg".into(),
                        DesignConfigurationKind::Table,
                        vec!["alternate".into()],
                        (serde_json::json!({
                            "configurations": {"alternate": {"suppressed": ["Fillet 1"]}}
                        }))
                        .as_object()
                        .unwrap()
                        .clone(),
                    )
                })
                .unwrap()],
            )
            .expect("ordered configuration table");
            bind_configuration_suppressed_features(
                decode_ctx,
                &mut ambiguous,
                &[feature, duplicate],
            )
            .unwrap();
            assert!(ambiguous[0].suppressed_features().next().is_none());
            assert_eq!(
                unresolved_configuration_suppressed_feature_count(&ambiguous),
                1
            );
        });
    }

    fn suppression_limit_fixture() -> (Vec<cadmpeg_ir::features::DesignConfiguration>, Feature) {
        crate::test_support::with_decode_context(|decode_ctx| {
            let table = crate::test_support::with_decode_context(|ctx| DesignConfiguration::try_new_charged(ctx, "table.dsgcfg".into(), DesignConfigurationKind::Table, vec!["alternate".into()], serde_json::json!({"configurations": {"alternate": {"suppressed": ["Fillet 1"]}}})
                .as_object()
                .unwrap()
                .clone()))
        .unwrap();
            let feature = Feature {
                id: FeatureId::mint("f3d:model:feature#fillet-1").unwrap(),
                ordinal: 0,
                name: Some("Fillet 1".into()),
                suppressed: Some(false),
                dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                source_properties: BTreeMap::new(),
                source_tag: None,
                source_text: None,
                source_content: cadmpeg_ir::features::FeatureContent::default(),
                evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                    FeatureDefinition::Operation(FeatureOperation::Native {
                        kind: "Fillet".into(),
                        parameters: BTreeMap::new(),
                    }),
                ),
                native_ref: None,
            };
            (
                project_configurations(decode_ctx, &[table]).unwrap(),
                feature,
            )
        })
    }

    #[test]
    fn configuration_suppressed_feature_id_refuses_retained_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let (mut configurations, feature) = suppression_limit_fixture();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = 0;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            bind_configuration_suppressed_features(&ctx, &mut configurations, &[feature]),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::RetainedBytes
                    && failure.operation == "f3d configuration suppressed feature id"
        ));
        assert!(configurations[0].feature_states.is_empty());
    }

    #[test]
    fn configuration_suppressed_feature_state_refuses_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let (mut configurations, feature) = suppression_limit_fixture();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            bind_configuration_suppressed_features(&ctx, &mut configurations, &[feature]),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == "f3d configuration suppressed feature state"
        ));
        assert!(configurations[0].feature_states.is_empty());
    }

    #[test]
    fn configuration_suppressed_dependency_refuses_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let (mut configurations, mut feature) = suppression_limit_fixture();
        feature
            .dependencies
            .insert(
                &cadmpeg_test_support::service_decode_context(),
                FeatureId::mint("f3d:model:feature#seed").unwrap(),
                "insert fixture member",
            )
            .expect("member insertion admission");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 1;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            bind_configuration_suppressed_features(&ctx, &mut configurations, &[feature]),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == "f3d configuration suppressed dependency"
        ));
        assert!(configurations[0].feature_states.is_empty());
    }

    #[test]
    fn configuration_suppressed_dependency_id_refuses_retained_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let (mut configurations, mut feature) = suppression_limit_fixture();
        let feature_id_bytes = feature.id.as_str().len();
        feature
            .dependencies
            .insert(
                &cadmpeg_test_support::service_decode_context(),
                FeatureId::mint("f3d:model:feature#seed").unwrap(),
                "insert fixture member",
            )
            .expect("member insertion admission");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = u64::try_from(feature_id_bytes).unwrap();
        let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "f3d configuration suppressed dependency id",
            |cap| {
                let mut configurations = configurations.clone();
                let feature = feature.clone();
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match ResourceDimension::RetainedBytes {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                bind_configuration_suppressed_features(&ctx, &mut configurations, &[feature])
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match ResourceDimension::RetainedBytes {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "f3d configuration suppressed dependency id",
            |cap| {
                let mut configurations = configurations.clone();
                let feature = feature.clone();
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match ResourceDimension::RetainedBytes {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                bind_configuration_suppressed_features(&ctx, &mut configurations, &[feature])
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match ResourceDimension::RetainedBytes {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            bind_configuration_suppressed_features(&ctx, &mut configurations, &[feature]),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::RetainedBytes
                    && failure.operation == "f3d configuration suppressed dependency id"
        ));
        assert!(configurations[0].feature_states.is_empty());
    }

    #[test]
    fn configuration_parameter_override_refuses_retained_and_collection_limits() {
        crate::test_support::with_decode_context(|decode_ctx| {
            use cadmpeg_core::decode::{
                DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
            };
            use cadmpeg_core::CodecError;

            let table = crate::test_support::with_decode_context(|ctx| DesignConfiguration::try_new_charged(ctx, "table.dsgcfg".into(), DesignConfigurationKind::Table, vec!["wide".into()], serde_json::json!({"configurations": {"wide": {"parameters": {"width": "25 mm"}}}})
                .as_object()
                .unwrap()
                .clone()))
        .unwrap();
            let parameter = NeutralParameter {
                id: ParameterId::mint("f3d:model:parameter#width").unwrap(),
                owner: None,
                ordinal: 0,
                name: "width".into(),
                expression: "10 mm".into(),
                display: None,
                value: None,
                dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                properties: BTreeMap::new(),
                pmi: None,
                native_ref: None,
            };
            for (retained, collections, dimension, operation) in [
                (
                    0,
                    1,
                    ResourceDimension::RetainedBytes,
                    "f3d configuration parameter override id",
                ),
                (
                    100,
                    0,
                    ResourceDimension::CollectionItems,
                    "f3d configuration parameter override",
                ),
            ] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = retained;
                policy.limits.max_collection_items = collections;
                let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
                    dimension,
                    operation,
                    |cap| {
                        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                        match dimension {
                            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                                policy.limits.max_retained_bytes = cap;
                            }
                            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                                policy.limits.max_collection_items = cap;
                            }
                            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                                policy.limits.max_materialized_bytes = cap;
                            }
                            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                                policy.limits.max_work_units = cap;
                            }
                            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                        }
                        let (ctx, _) =
                            DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                        let mut projected =
                            project_configurations(decode_ctx, std::slice::from_ref(&table))
                                .unwrap();
                        bind_configuration_parameter_overrides(
                            &ctx,
                            &mut projected,
                            std::slice::from_ref(&parameter),
                        )
                    },
                ) {
                    cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
                    error => panic!("unexpected refusal: {error:?}"),
                };
                policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
                match dimension {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = refusal_cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = refusal_cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = refusal_cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = refusal_cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
                    dimension,
                    operation,
                    |cap| {
                        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                        match dimension {
                            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                                policy.limits.max_retained_bytes = cap;
                            }
                            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                                policy.limits.max_collection_items = cap;
                            }
                            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                                policy.limits.max_materialized_bytes = cap;
                            }
                            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                                policy.limits.max_work_units = cap;
                            }
                            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                        }
                        let (ctx, _) =
                            DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                        let mut projected =
                            project_configurations(decode_ctx, std::slice::from_ref(&table))
                                .unwrap();
                        bind_configuration_parameter_overrides(
                            &ctx,
                            &mut projected,
                            std::slice::from_ref(&parameter),
                        )
                    },
                ) {
                    cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
                    error => panic!("unexpected refusal: {error:?}"),
                };
                policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
                match dimension {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = refusal_cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = refusal_cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = refusal_cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = refusal_cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let mut projected =
                    project_configurations(decode_ctx, std::slice::from_ref(&table)).unwrap();
                assert!(matches!(
                    bind_configuration_parameter_overrides(&ctx, &mut projected, std::slice::from_ref(&parameter)),
                    Err(CodecError::ResourceLimit(failure))
                        if failure.dimension == dimension && failure.operation == operation
                ));
                assert!(projected[0].parameter_overrides.is_empty());
            }
        });
    }

    #[test]
    fn configuration_projection_refuses_each_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let table = crate::test_support::with_decode_context(|ctx| {
            DesignConfiguration::try_new_charged(
                ctx,
                "table.dsgcfg".into(),
                DesignConfigurationKind::Table,
                vec!["wide".into()],
                serde_json::json!({"configurations": {"wide": {
                    "parameters": {"width": "25 mm"},
                    "suppressed": ["Fillet 1"],
                    "material": "Steel"
                }}})
                .as_object()
                .unwrap()
                .clone(),
            )
        })
        .unwrap();
        let rule = crate::test_support::with_decode_context(|ctx| {
            DesignConfiguration::try_new_charged(
                ctx,
                "rule.dsgcfgrule".into(),
                DesignConfigurationKind::Rule,
                Vec::new(),
                serde_json::json!({"when": "width > 20 mm", "activate": "wide"})
                    .as_object()
                    .unwrap()
                    .clone(),
            )
        })
        .unwrap();
        for (limit, operation) in [
            (0, "f3d configuration parameter property"),
            (1, "f3d configuration suppression property"),
            (2, "f3d projected configuration"),
            (3, "f3d configuration activation rule property"),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_collection_items = limit;

            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert!(
                matches!(
                    project_configurations(&ctx, &[table.clone(), rule.clone()]),
                    Err(CodecError::ResourceLimit(failure))
                        if failure.dimension == ResourceDimension::CollectionItems
                            && failure.operation == operation
                ),
                "limit {limit}, operation {operation}"
            );
        }
    }

    #[test]
    fn configuration_projection_text_copies_refuse_retained_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        for operation in [
            "f3d configuration parameter value",
            "f3d configuration material",
            "f3d configuration variant name",
            "f3d configuration activation rule condition",
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_retained_bytes = 4;

            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert!(matches!(
                ctx.copy_retained_text("input", operation),
                Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::RetainedBytes
                        && failure.operation == operation
            ));
        }
        for (prefix, operation) in [
            ("parameter:", "f3d configuration parameter key"),
            ("suppressed:", "f3d configuration suppression key"),
            ("activation_rule:", "f3d configuration activation rule key"),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_retained_bytes = 4;

            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert!(matches!(
                super::configuration_property_key(&ctx, prefix, "input", operation),
                Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::RetainedBytes
                        && failure.operation == operation
            ));
        }
    }

    #[test]
    fn configuration_variant_order_refuses_each_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let bytes = br#"{"configurations":{"Small":{},"Large":{}}}"#;
        for (collection_limit, retained_limit, materialized_limit, dimension, operation) in [
            (
                0,
                100,
                100,
                ResourceDimension::CollectionItems,
                "f3d configuration variant name index",
            ),
            (
                1,
                100,
                100,
                ResourceDimension::CollectionItems,
                "f3d configuration variant order",
            ),
            (
                100,
                0,
                100,
                ResourceDimension::RetainedBytes,
                "f3d configuration variant unique name",
            ),
            (
                100,
                100,
                0,
                ResourceDimension::MaterializedBytes,
                "f3d configuration order JSON",
            ),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_collection_items = collection_limit;
            policy.limits.max_retained_bytes = retained_limit;
            policy.limits.max_materialized_bytes = materialized_limit;
            let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
                dimension,
                operation,
                |cap| {
                    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                    match dimension {
                        cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                            policy.limits.max_retained_bytes = cap;
                        }
                        cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                            policy.limits.max_collection_items = cap;
                        }
                        cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                            policy.limits.max_materialized_bytes = cap;
                        }
                        cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                            policy.limits.max_work_units = cap;
                        }
                        dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                    }
                    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                    (parse_configuration_variant_order(&ctx, "table.dsgcfg", bytes)).map(|_| ())
                },
            ) {
                cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
                error => panic!("unexpected refusal: {error:?}"),
            };
            policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
            match dimension {
                cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                    policy.limits.max_retained_bytes = refusal_cap;
                }
                cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                    policy.limits.max_collection_items = refusal_cap;
                }
                cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                    policy.limits.max_materialized_bytes = refusal_cap;
                }
                cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                    policy.limits.max_work_units = refusal_cap;
                }
                dimension => panic!("unsupported refusal dimension: {dimension:?}"),
            }
            let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
                dimension,
                operation,
                |cap| {
                    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                    match dimension {
                        cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                            policy.limits.max_retained_bytes = cap;
                        }
                        cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                            policy.limits.max_collection_items = cap;
                        }
                        cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                            policy.limits.max_materialized_bytes = cap;
                        }
                        cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                            policy.limits.max_work_units = cap;
                        }
                        dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                    }
                    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                    (parse_configuration_variant_order(&ctx, "table.dsgcfg", bytes)).map(|_| ())
                },
            ) {
                cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
                error => panic!("unexpected refusal: {error:?}"),
            };
            policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
            match dimension {
                cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                    policy.limits.max_retained_bytes = refusal_cap;
                }
                cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                    policy.limits.max_collection_items = refusal_cap;
                }
                cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                    policy.limits.max_materialized_bytes = refusal_cap;
                }
                cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                    policy.limits.max_work_units = refusal_cap;
                }
                dimension => panic!("unsupported refusal dimension: {dimension:?}"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert!(
                matches!(
                    parse_configuration_variant_order(&ctx, "table.dsgcfg", bytes),
                    Err(CodecError::ResourceLimit(failure))
                        if failure.dimension == dimension && failure.operation == operation
                ),
                "operation {operation}"
            );
        }
    }

    #[test]
    fn configuration_duplicate_variant_diagnostic_refuses_retained_limit() {
        crate::test_support::with_decode_context(|decode_ctx| {
            use cadmpeg_core::decode::{
                DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
            };
            use cadmpeg_core::CodecError;

            let bytes = br#"{"configurations":{"Small":{},"Small":{}}}"#;
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            // Two input names and the unique-name copy consume 15 bytes.
            policy.limits.max_retained_bytes = 15;
            let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
                ResourceDimension::RetainedBytes,
                "f3d duplicate configuration variant diagnostic",
                |cap| {
                    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                    match ResourceDimension::RetainedBytes {
                        cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                            policy.limits.max_retained_bytes = cap;
                        }
                        cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                            policy.limits.max_collection_items = cap;
                        }
                        cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                            policy.limits.max_materialized_bytes = cap;
                        }
                        cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                            policy.limits.max_work_units = cap;
                        }
                        dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                    }
                    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                    (parse_configuration_variant_order(&ctx, "table.dsgcfg", bytes)).map(|_| ())
                },
            ) {
                cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
                error => panic!("unexpected refusal: {error:?}"),
            };
            policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
            match ResourceDimension::RetainedBytes {
                cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                    policy.limits.max_retained_bytes = refusal_cap;
                }
                cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                    policy.limits.max_collection_items = refusal_cap;
                }
                cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                    policy.limits.max_materialized_bytes = refusal_cap;
                }
                cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                    policy.limits.max_work_units = refusal_cap;
                }
                dimension => panic!("unsupported refusal dimension: {dimension:?}"),
            }
            let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
                ResourceDimension::RetainedBytes,
                "f3d duplicate configuration variant diagnostic",
                |cap| {
                    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                    match ResourceDimension::RetainedBytes {
                        cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                            policy.limits.max_retained_bytes = cap;
                        }
                        cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                            policy.limits.max_collection_items = cap;
                        }
                        cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                            policy.limits.max_materialized_bytes = cap;
                        }
                        cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                            policy.limits.max_work_units = cap;
                        }
                        dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                    }
                    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                    (parse_configuration_variant_order(&ctx, "table.dsgcfg", bytes)).map(|_| ())
                },
            ) {
                cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
                error => panic!("unexpected refusal: {error:?}"),
            };
            policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
            match ResourceDimension::RetainedBytes {
                cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                    policy.limits.max_retained_bytes = refusal_cap;
                }
                cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                    policy.limits.max_collection_items = refusal_cap;
                }
                cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                    policy.limits.max_materialized_bytes = refusal_cap;
                }
                cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                    policy.limits.max_work_units = refusal_cap;
                }
                dimension => panic!("unsupported refusal dimension: {dimension:?}"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert!(matches!(
                parse_configuration_variant_order(&ctx, "table.dsgcfg", bytes),
                Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::RetainedBytes
                        && failure.operation == "f3d duplicate configuration variant diagnostic"
            ));

            let arena = DecodeArena::new();
            let policy = DecodePolicy::default();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert_eq!(
                parse_configuration_variant_order(&ctx, "table.dsgcfg", bytes)
                    .unwrap_err()
                    .to_string(),
                parse_configuration_variant_order(decode_ctx, "table.dsgcfg", bytes)
                    .unwrap_err()
                    .to_string(),
            );
        });
    }
    #[test]
    fn configuration_ordering_refuses_sort_scratch_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        let names: Vec<_> = (0..21).rev().map(|index| format!("v{index:02}")).collect();
        let variants = names
            .iter()
            .map(|name| (name.clone(), serde_json::json!({})))
            .collect();
        let payload = serde_json::Map::from_iter([(
            "configurations".into(),
            serde_json::Value::Object(variants),
        )]);
        let table = crate::test_support::with_decode_context(|ctx| {
            DesignConfiguration::try_new_charged(
                ctx,
                "table.dsgcfg".into(),
                DesignConfigurationKind::Table,
                names,
                payload,
            )
        })
        .unwrap();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        // The sort scratch holds two index vectors for the 21 projected variants.
        policy.limits.max_materialized_bytes =
            u64::try_from(21 * 2 * std::mem::size_of::<usize>() - 1).unwrap();

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(
            matches!(project_configurations(&ctx, std::slice::from_ref(&table)),
            Err(CodecError::ResourceLimit(failure)) if failure.dimension == ResourceDimension::MaterializedBytes
                && failure.operation == "sort f3d configuration variants")
        );
    }
}
