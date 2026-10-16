// SPDX-License-Identifier: Apache-2.0
//! Build configuration JSON values under the caller decode budget.

use crate::design::json_value::ValueSeed;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde::de::{DeserializeSeed, Visitor};
use serde_json::Value;
use std::fmt;

pub(super) struct ConfigurationFieldSeed;

impl<'de> DeserializeSeed<'de> for ConfigurationFieldSeed {
    type Value = bool;
    fn deserialize<D>(self, deserializer: D) -> Result<bool, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(self)
    }
}

impl Visitor<'_> for ConfigurationFieldSeed {
    type Value = bool;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a string")
    }
    fn visit_str<E>(self, field: &str) -> Result<bool, E>
    where
        E: serde::de::Error,
    {
        Ok(field == "configurations")
    }
}

pub(super) fn parse_configuration_payload(
    ctx: &DecodeContext<'_>,
    entry_name: &str,
    bytes: &[u8],
) -> Result<Value, CodecError> {
    let _scratch = crate::design::json_value::reserve_json_scratch(
        ctx,
        bytes.len(),
        "f3d configuration JSON",
    )?;
    let mut refusal = None;
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let parsed = ValueSeed {
        ctx,
        refusal: &mut refusal,
        member: false,
    }
    .deserialize(&mut deserializer)
    .and_then(|value| {
        deserializer.end()?;
        Ok(value)
    });
    match parsed {
        Ok(value) => Ok(value),
        Err(error) => match refusal {
            Some(error) => Err(error),
            None => Err(CodecError::Malformed(ctx.format_retained(
                format_args!("invalid F3D configuration JSON {entry_name}: {error}"),
                "f3d configuration JSON diagnostic",
            )?)),
        },
    }
}

#[cfg(test)]
mod tests;
