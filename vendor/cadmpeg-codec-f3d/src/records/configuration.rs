// SPDX-License-Identifier: Apache-2.0
//! Admitted configuration documents and their authored variant order.

use crate::records::admission::RecordAdmission;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// JSON configuration payload stored in a Fusion design-configuration entry.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "DesignConfigurationWire")]
pub(crate) struct DesignConfiguration {
    entry_name: String,
    identity_scope: cadmpeg_ir::ids::IdentityComponent,
    payload: ConfigurationPayload,
}

#[cfg(test)]
thread_local! {
    static CONFIGURATION_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for DesignConfiguration {
    fn clone(&self) -> Self {
        CONFIGURATION_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            entry_name: self.entry_name.clone(),
            identity_scope: self.identity_scope.clone(),
            payload: self.payload.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum ConfigurationPayload {
    Table {
        active: Option<String>,
        variants: Option<ConfigurationVariants>,
        extensions: Map<String, Value>,
    },
    Rule(Map<String, Value>),
}

/// A table's actual members, in authored order.
#[derive(Debug, Clone, PartialEq)]
struct ConfigurationVariants {
    entries: Vec<(String, ConfigurationVariant)>,
    /// Empty or singleton legacy records can omit their unambiguous order.
    explicit_order: bool,
}

/// Typed variant properties and retained extension members.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ConfigurationVariant {
    parameters: Option<BTreeMap<String, ConfigurationScalar>>,
    suppressed: Option<Vec<String>>,
    material: Option<String>,
    extensions: Map<String, Value>,
}

/// A parameter override contains one JSON scalar.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ConfigurationScalar {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(String),
}

impl ConfigurationScalar {
    pub(crate) fn text_charged(&self, ctx: &DecodeContext<'_>) -> Result<String, CodecError> {
        struct ScalarText<'a>(&'a ConfigurationScalar);
        impl std::fmt::Display for ScalarText<'_> {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self.0 {
                    ConfigurationScalar::Null => formatter.write_str("null"),
                    ConfigurationScalar::Bool(value) => std::fmt::Display::fmt(value, formatter),
                    ConfigurationScalar::Number(value) => std::fmt::Display::fmt(value, formatter),
                    ConfigurationScalar::String(value) => formatter.write_str(value),
                }
            }
        }
        ctx.format_retained(
            format_args!("{}", ScalarText(self)),
            "project F3D configuration scalar text",
        )
    }

    fn value(&self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Bool(value) => Value::Bool(*value),
            Self::Number(value) => Value::Number(value.clone()),
            Self::String(value) => Value::String(value.clone()),
        }
    }
}

impl ConfigurationVariant {
    fn admit(
        admission: RecordAdmission<'_, '_>,
        entry_name: &str,
        name: &str,
        value: Value,
    ) -> Result<Self, CodecError> {
        let Value::Object(mut fields) = value else {
            return Err(CodecError::malformed(format_args!(
                "F3D configuration variant `{name}` must be an object: {entry_name}"
            )));
        };
        let parameters = match fields.remove("parameters") {
            Some(Value::Object(parameters)) => {
                let mut admitted = BTreeMap::new();
                for (key, value) in parameters {
                    let value = match value {
                        Value::Null => ConfigurationScalar::Null,
                        Value::Bool(value) => ConfigurationScalar::Bool(value),
                        Value::Number(value) => ConfigurationScalar::Number(value),
                        Value::String(value) => ConfigurationScalar::String(value),
                        Value::Array(_) | Value::Object(_) => {
                            return Err(CodecError::malformed(format_args!(
                                "F3D configuration variant `{name}` parameter overrides must be JSON scalars: {entry_name}"
                            )));
                        }
                    };
                    admission.insert_btree_map(
                        &mut admitted,
                        key,
                        value,
                        "admit configuration parameter",
                    )?;
                }
                Some(admitted)
            }
            Some(_) => {
                return Err(CodecError::malformed(format_args!(
                    "F3D configuration variant `{name}` parameters must be an object: {entry_name}"
                )))
            }
            None => None,
        };
        let suppressed_error = || {
            CodecError::malformed(format_args!(
            "F3D configuration variant `{name}` suppressed list must contain strings: {entry_name}"
        ))
        };
        let suppressed = match fields.remove("suppressed") {
            Some(Value::Array(values)) => {
                let mut suppressed = Vec::new();
                for value in values {
                    let Value::String(value) = value else {
                        return Err(suppressed_error());
                    };
                    {
                        admission.reserve_vec(
                            &mut suppressed,
                            1,
                            "admit suppressed configuration member",
                        )?;
                    }
                    suppressed.push(value);
                }
                Some(suppressed)
            }
            Some(_) => return Err(suppressed_error()),
            None => None,
        };
        let material = match fields.remove("material") {
            Some(Value::String(value)) => Some(value),
            Some(_) => {
                return Err(CodecError::malformed(format_args!(
                    "F3D configuration variant `{name}` material must be a string: {entry_name}"
                )))
            }
            None => None,
        };
        Ok(Self {
            parameters,
            suppressed,
            material,
            extensions: fields,
        })
    }

    pub(crate) fn parameters(&self) -> impl Iterator<Item = (&String, &ConfigurationScalar)> {
        self.parameters
            .iter()
            .flat_map(|parameters| parameters.iter())
    }

    pub(crate) fn suppressed(&self) -> impl Iterator<Item = &String> {
        self.suppressed.iter().flatten()
    }

    pub(crate) fn material(&self) -> Option<&str> {
        self.material.as_deref()
    }

    fn payload(&self) -> Map<String, Value> {
        let mut payload = self.extensions.clone();
        if let Some(parameters) = &self.parameters {
            payload.insert(
                "parameters".into(),
                Value::Object(
                    parameters
                        .iter()
                        .map(|(name, value)| (name.clone(), value.value()))
                        .collect(),
                ),
            );
        }
        if let Some(suppressed) = &self.suppressed {
            payload.insert(
                "suppressed".into(),
                Value::Array(suppressed.iter().cloned().map(Value::String).collect()),
            );
        }
        if let Some(material) = &self.material {
            payload.insert("material".into(), Value::String(material.clone()));
        }
        payload
    }
}

/// Serialized configuration identity and payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignConfigurationWire {
    id: String,
    entry_name: String,
    kind: DesignConfigurationKind,
    #[serde(default)]
    variant_order: Vec<String>,
    payload: Value,
}

struct ConfigurationIdentity<'a>(&'a DesignConfiguration);

impl std::fmt::Display for ConfigurationIdentity<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use std::fmt::Write;

        write!(formatter, "f3d:{}:entry#", self.0.identity_scope.as_str())?;
        for character in self.0.entry_name.chars() {
            if matches!(character, ':' | '#' | '%') || character.is_whitespace() {
                let mut bytes = [0; 4];
                for byte in character.encode_utf8(&mut bytes).as_bytes() {
                    write!(formatter, "%{byte:02X}")?;
                }
            } else {
                formatter.write_char(character)?;
            }
        }
        Ok(())
    }
}

impl Serialize for ConfigurationIdentity<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

struct ConfigurationOrder<'a>(&'a DesignConfiguration);

impl Serialize for ConfigurationOrder<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match &self.0.payload {
            ConfigurationPayload::Table {
                variants: Some(variants),
                ..
            } if variants.explicit_order => {
                serializer.collect_seq(variants.entries.iter().map(|(name, _)| name))
            }
            _ => serializer.collect_seq(std::iter::empty::<&String>()),
        }
    }
}

impl Serialize for ConfigurationScalar {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Null => serializer.serialize_none(),
            Self::Bool(value) => serializer.serialize_bool(*value),
            Self::Number(value) => value.serialize(serializer),
            Self::String(value) => serializer.serialize_str(value),
        }
    }
}

struct ConfigurationVariantView<'a>(&'a ConfigurationVariant);

impl Serialize for ConfigurationVariantView<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let variant = self.0;
        let mut map = serializer.serialize_map(Some(
            variant.extensions.len()
                + usize::from(variant.material.is_some())
                + usize::from(variant.parameters.is_some())
                + usize::from(variant.suppressed.is_some()),
        ))?;
        let mut material_written = false;
        let mut parameters_written = false;
        let mut suppressed_written = false;
        for (name, value) in &variant.extensions {
            if name.as_str() > "material" && !material_written {
                if let Some(material) = &variant.material {
                    map.serialize_entry("material", material)?;
                }
                material_written = true;
            }
            if name.as_str() > "parameters" && !parameters_written {
                if let Some(parameters) = &variant.parameters {
                    map.serialize_entry("parameters", parameters)?;
                }
                parameters_written = true;
            }
            if name.as_str() > "suppressed" && !suppressed_written {
                if let Some(suppressed) = &variant.suppressed {
                    map.serialize_entry("suppressed", suppressed)?;
                }
                suppressed_written = true;
            }
            map.serialize_entry(name, value)?;
        }
        if !material_written {
            if let Some(material) = &variant.material {
                map.serialize_entry("material", material)?;
            }
        }
        if !parameters_written {
            if let Some(parameters) = &variant.parameters {
                map.serialize_entry("parameters", parameters)?;
            }
        }
        if !suppressed_written {
            if let Some(suppressed) = &variant.suppressed {
                map.serialize_entry("suppressed", suppressed)?;
            }
        }
        map.end()
    }
}

struct ConfigurationPayloadView<'a>(&'a ConfigurationPayload);

struct SortedConfigurationVariants<'a>(&'a [(String, ConfigurationVariant)]);

impl Serialize for SortedConfigurationVariants<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        let mut previous: Option<&str> = None;
        while let Some((name, variant)) = self
            .0
            .iter()
            .filter(|(name, _)| previous.is_none_or(|previous| name.as_str() > previous))
            .min_by(|(left, _), (right, _)| left.cmp(right))
        {
            map.serialize_entry(name, &ConfigurationVariantView(variant))?;
            previous = Some(name);
        }
        map.end()
    }
}

impl Serialize for ConfigurationPayloadView<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            ConfigurationPayload::Rule(fields) => fields.serialize(serializer),
            ConfigurationPayload::Table {
                active,
                variants,
                extensions,
            } => {
                let mut map = serializer.serialize_map(Some(
                    extensions.len()
                        + usize::from(active.is_some())
                        + usize::from(variants.is_some()),
                ))?;
                let mut active_written = false;
                let mut variants_written = false;
                for (name, value) in extensions {
                    if name.as_str() > "active" && !active_written {
                        if let Some(active) = active {
                            map.serialize_entry("active", active)?;
                        }
                        active_written = true;
                    }
                    if name.as_str() > "configurations" && !variants_written {
                        if let Some(variants) = variants {
                            map.serialize_entry(
                                "configurations",
                                &SortedConfigurationVariants(&variants.entries),
                            )?;
                        }
                        variants_written = true;
                    }
                    map.serialize_entry(name, value)?;
                }
                if !active_written {
                    if let Some(active) = active {
                        map.serialize_entry("active", active)?;
                    }
                }
                if !variants_written {
                    if let Some(variants) = variants {
                        map.serialize_entry(
                            "configurations",
                            &SortedConfigurationVariants(&variants.entries),
                        )?;
                    }
                }
                map.end()
            }
        }
    }
}

impl Serialize for DesignConfiguration {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(5))?;
        map.serialize_entry("id", &ConfigurationIdentity(self))?;
        map.serialize_entry("entry_name", &self.entry_name)?;
        map.serialize_entry("kind", &self.kind())?;
        map.serialize_entry("variant_order", &ConfigurationOrder(self))?;
        map.serialize_entry("payload", &ConfigurationPayloadView(&self.payload))?;
        map.end()
    }
}

/// Native Fusion design-configuration entry family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignConfigurationKind {
    Table,
    Rule,
}

impl DesignConfiguration {
    pub(crate) fn try_new_charged(
        ctx: &DecodeContext<'_>,
        entry_name: String,
        kind: DesignConfigurationKind,
        variant_order: Vec<String>,
        payload: Map<String, Value>,
    ) -> Result<Self, CodecError> {
        Self::try_new_with_admission(
            RecordAdmission::Charged(ctx),
            entry_name,
            kind,
            variant_order,
            payload,
        )
    }

    fn try_new_with_admission(
        admission: RecordAdmission<'_, '_>,
        entry_name: String,
        kind: DesignConfigurationKind,
        variant_order: Vec<String>,
        mut payload: Map<String, Value>,
    ) -> Result<Self, CodecError> {
        let extension = match kind {
            DesignConfigurationKind::Table => ".dsgcfg",
            DesignConfigurationKind::Rule => ".dsgcfgrule",
        };
        if !entry_name.ends_with(extension) {
            return Err(CodecError::malformed(format_args!(
                "configuration.entry_name must end with {extension} for {kind:?}"
            )));
        }
        if kind == DesignConfigurationKind::Rule {
            if !variant_order.is_empty() {
                return Err(CodecError::malformed(format_args!(
                    "F3D configuration rule carries a variant order: {entry_name}"
                )));
            }
            return Ok(Self {
                entry_name,
                identity_scope: cadmpeg_ir::identity_component!("configuration"),
                payload: ConfigurationPayload::Rule(payload),
            });
        }
        let variants = match payload.remove("configurations") {
            Some(Value::Object(variants)) => Some(variants),
            Some(_) => {
                return Err(CodecError::malformed(format_args!(
                    "F3D configuration table `configurations` must be an object: {entry_name}"
                )))
            }
            None => None,
        };
        let active = match payload.remove("active") {
            Some(Value::String(active)) => {
                if !variants
                    .as_ref()
                    .is_some_and(|variants| variants.contains_key(&active))
                {
                    return Err(CodecError::malformed(format_args!(
                        "F3D active configuration `{active}` is not a named variant: {entry_name}"
                    )));
                }
                Some(active)
            }
            Some(_) => {
                return Err(CodecError::malformed(format_args!(
                    "F3D configuration table `active` must be a string: {entry_name}"
                )))
            }
            None => None,
        };
        let invalid_order = || {
            CodecError::malformed(format_args!(
                "F3D configuration variant order does not match its table: {entry_name}"
            ))
        };
        let variants = match variants {
            Some(variants) => {
                let mut admitted = BTreeMap::new();
                for (name, value) in variants {
                    let value = ConfigurationVariant::admit(admission, &entry_name, &name, value)?;
                    admission.insert_btree_map(
                        &mut admitted,
                        name,
                        value,
                        "admit configuration variant",
                    )?;
                }
                let mut variants = admitted;
                let explicit_order = !variant_order.is_empty();
                let entries = if !explicit_order && variants.len() <= 1 {
                    let mut entries = Vec::new();
                    for variant in variants {
                        {
                            admission.reserve_vec(
                                &mut entries,
                                1,
                                "order configuration variants",
                            )?;
                        }
                        entries.push(variant);
                    }
                    entries
                } else {
                    let mut entries = Vec::new();
                    for name in variant_order {
                        let variant = variants.remove_entry(&name).ok_or_else(&invalid_order)?;
                        {
                            admission.reserve_vec(
                                &mut entries,
                                1,
                                "order configuration variants",
                            )?;
                        }
                        entries.push(variant);
                    }
                    if !variants.is_empty() {
                        return Err(invalid_order());
                    }
                    entries
                };
                Some(ConfigurationVariants {
                    entries,
                    explicit_order,
                })
            }
            None if variant_order.is_empty() => None,
            None => return Err(invalid_order()),
        };
        Ok(Self {
            entry_name,
            identity_scope: cadmpeg_ir::identity_component!("configuration"),
            payload: ConfigurationPayload::Table {
                active,
                variants,
                extensions: payload,
            },
        })
    }

    pub(crate) fn id(&self) -> String {
        crate::ids::configuration_entry_id(&self.entry_name, &self.identity_scope)
    }

    pub(crate) fn id_charged(&self, ctx: &DecodeContext<'_>) -> Result<String, CodecError> {
        crate::ids::configuration_entry_id_charged(ctx, &self.entry_name, &self.identity_scope)
    }

    pub(crate) fn entry_name(&self) -> &String {
        &self.entry_name
    }

    pub(crate) fn kind(&self) -> DesignConfigurationKind {
        match self.payload {
            ConfigurationPayload::Table { .. } => DesignConfigurationKind::Table,
            ConfigurationPayload::Rule(_) => DesignConfigurationKind::Rule,
        }
    }

    /// Actual named definitions in authored order; rules have no variants.
    pub(crate) fn variants(&self) -> &[(String, ConfigurationVariant)] {
        match &self.payload {
            ConfigurationPayload::Table {
                variants: Some(variants),
                ..
            } => &variants.entries,
            _ => &[],
        }
    }

    pub(crate) fn active(&self) -> Option<&str> {
        match &self.payload {
            ConfigurationPayload::Table { active, .. } => active.as_deref(),
            ConfigurationPayload::Rule(_) => None,
        }
    }

    pub(crate) fn rule(&self) -> Option<&Map<String, Value>> {
        match &self.payload {
            ConfigurationPayload::Rule(payload) => Some(payload),
            ConfigurationPayload::Table { .. } => None,
        }
    }

    /// The native wire keeps a missing legacy order as an empty list.
    #[cfg(test)]
    pub(crate) fn variant_order(&self) -> Vec<String> {
        match &self.payload {
            ConfigurationPayload::Table {
                variants: Some(variants),
                ..
            } if variants.explicit_order => variants
                .entries
                .iter()
                .map(|(name, _)| name.clone())
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Rebuild the complete object payload, including extension members.
    pub(crate) fn payload(&self) -> Map<String, Value> {
        match &self.payload {
            ConfigurationPayload::Rule(payload) => payload.clone(),
            ConfigurationPayload::Table {
                active,
                variants,
                extensions,
            } => {
                let mut payload = extensions.clone();
                if let Some(active) = active {
                    payload.insert("active".into(), Value::String(active.clone()));
                }
                if let Some(variants) = variants {
                    payload.insert(
                        "configurations".into(),
                        Value::Object(
                            variants
                                .entries
                                .iter()
                                .map(|(name, value)| (name.clone(), Value::Object(value.payload())))
                                .collect(),
                        ),
                    );
                }
                payload
            }
        }
    }

    pub(crate) fn unknown_member_count(&self) -> usize {
        match &self.payload {
            ConfigurationPayload::Rule(payload) => payload
                .keys()
                .filter(|key| !matches!(key.as_str(), "when" | "activate"))
                .count(),
            ConfigurationPayload::Table { extensions, .. } => {
                extensions.len()
                    + self
                        .variants()
                        .iter()
                        .map(|(_, variant)| variant.extensions.len())
                        .sum::<usize>()
            }
        }
    }
}

impl TryFrom<DesignConfigurationWire> for DesignConfiguration {
    type Error = String;
    fn try_from(wire: DesignConfigurationWire) -> Result<Self, String> {
        let suffix = format!(
            ":entry#{}",
            crate::ids::identity_key_component(&wire.entry_name)
        );
        let scope = wire
            .id
            .strip_prefix("f3d:")
            .and_then(|id| id.strip_suffix(&suffix))
            .ok_or_else(|| format!("configuration.id {} must identify entry_name", wire.id))?;
        let scope = cadmpeg_ir::ids::IdentityComponent::try_new(scope)
            .map_err(|error| format!("configuration.id {}: {error}", wire.id))?;
        let Value::Object(payload) = wire.payload else {
            return Err("payload must be an object".into());
        };
        let mut record = Self::try_new_with_admission(
            RecordAdmission::Admitted,
            wire.entry_name,
            wire.kind,
            wire.variant_order,
            payload,
        )
        .map_err(|error| error.to_string())?;
        record.identity_scope = scope;
        Ok(record)
    }
}

#[cfg(test)]
impl From<DesignConfiguration> for DesignConfigurationWire {
    fn from(value: DesignConfiguration) -> Self {
        Self {
            id: value.id(),
            kind: value.kind(),
            variant_order: value.variant_order(),
            payload: Value::Object(value.payload()),
            entry_name: value.entry_name,
        }
    }
}

struct OrderedConfigurationVariants<'a>(&'a [(String, ConfigurationVariant)]);

impl Serialize for OrderedConfigurationVariants<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (name, value) in self.0 {
            map.serialize_entry(name, &ConfigurationVariantView(value))?;
        }
        map.end()
    }
}

struct OrderedConfigurationPayload<'a>(&'a DesignConfiguration);

impl Serialize for OrderedConfigurationPayload<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let object = self.0.payload();
        let mut map = serializer.serialize_map(Some(object.len()))?;
        for (name, value) in object {
            if name == "configurations" && self.0.kind() == DesignConfigurationKind::Table {
                map.serialize_entry(&name, &OrderedConfigurationVariants(self.0.variants()))?;
            } else {
                map.serialize_entry(&name, &value)?;
            }
        }
        map.end()
    }
}

/// Encode the native source payload with its authored variant order.
pub(crate) fn encode_configuration_payload(
    configuration: &DesignConfiguration,
) -> Result<Vec<u8>, CodecError> {
    serde_json::to_vec(&OrderedConfigurationPayload(configuration)).map_err(|error| {
        CodecError::malformed(format_args!(
            "cannot encode F3D configuration JSON {}: {error}",
            configuration.entry_name()
        ))
    })
}

#[cfg(test)]
mod tests;
