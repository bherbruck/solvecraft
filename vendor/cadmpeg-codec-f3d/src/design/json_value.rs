// SPDX-License-Identifier: Apache-2.0
//! Admit each text copy, member and recursive step while building a JSON value.

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use std::fmt;

/// Bounds `serde_json` scratch growth by twice the wire width, with its eight-byte minimum.
pub(in crate::design) fn reserve_json_scratch<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    length: usize,
    operation: &'static str,
) -> Result<ScopedReservation<'ctx>, CodecError> {
    let bound = length
        .checked_mul(2)
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?
        .max(8);
    ctx.reserve_scoped(cadmpeg_core::decode::u64_from_index(bound), operation)
}

pub(in crate::design) struct TextSeed<'a, 'b> {
    pub(in crate::design) ctx: &'a DecodeContext<'b>,
    pub(in crate::design) refusal: &'a mut Option<CodecError>,
    pub(in crate::design) operation: &'static str,
    pub(in crate::design) entry: bool,
    pub(in crate::design) expected: &'static str,
}

impl<'de> DeserializeSeed<'de> for TextSeed<'_, '_> {
    type Value = String;
    fn deserialize<D>(self, deserializer: D) -> Result<String, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        if self.entry {
            {
                let ctx = self.ctx;
                if let Err(error) =
                    ctx.charge_collection_items(1, "f3d configuration JSON object entry")
                {
                    *self.refusal = Some(error);
                    return Err(serde::de::Error::custom(
                        "configuration JSON resource limit",
                    ));
                }
            }
        }
        deserializer.deserialize_str(self)
    }
}

impl Visitor<'_> for TextSeed<'_, '_> {
    type Value = String;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.expected)
    }
    fn visit_str<E>(self, text: &str) -> Result<String, E>
    where
        E: serde::de::Error,
    {
        match self.ctx.copy_retained_text(text, self.operation) {
            Ok(text) => Ok(text),
            Err(error) => {
                *self.refusal = Some(error);
                Err(serde::de::Error::custom(
                    "configuration JSON resource limit",
                ))
            }
        }
    }
}

pub(in crate::design) struct ValueSeed<'a, 'b> {
    pub(in crate::design) ctx: &'a DecodeContext<'b>,
    pub(in crate::design) refusal: &'a mut Option<CodecError>,
    pub(in crate::design) member: bool,
}

impl ValueSeed<'_, '_> {
    fn admit<T, E: serde::de::Error>(&mut self, result: Result<T, CodecError>) -> Result<T, E> {
        result.map_err(|error| {
            *self.refusal = Some(error);
            E::custom("configuration JSON resource limit")
        })
    }
}

impl<'de> DeserializeSeed<'de> for ValueSeed<'_, '_> {
    type Value = Value;
    fn deserialize<D>(mut self, deserializer: D) -> Result<Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let ctx = self.ctx;
        let _depth = self.admit(ctx.enter_nested("f3d configuration JSON depth"))?;
        self.admit(ctx.charge_work(1, "f3d configuration JSON value work"))?;
        if self.member {
            self.admit(ctx.charge_collection_items(1, "f3d configuration JSON array member"))?;
        }
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for ValueSeed<'_, '_> {
    type Value = Value;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any valid JSON value")
    }
    fn visit_bool<E>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }
    fn visit_i64<E>(self, value: i64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }
    fn visit_u64<E>(self, value: u64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }
    fn visit_f64<E>(self, value: f64) -> Result<Value, E> {
        Ok(serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number))
    }
    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_str<E>(mut self, text: &str) -> Result<Value, E>
    where
        E: serde::de::Error,
    {
        let copied = self
            .ctx
            .copy_retained_text(text, "f3d configuration JSON text");
        self.admit(copied).map(Value::String)
    }
    fn visit_seq<S>(mut self, mut sequence: S) -> Result<Value, S::Error>
    where
        S: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(ValueSeed {
            ctx: self.ctx,
            refusal: &mut *self.refusal,
            member: true,
        })? {
            self.admit(self.ctx.reserve_capacity(
                &mut values,
                1,
                "f3d configuration JSON array allocation",
            ))?;
            values.push(value);
        }
        Ok(Value::Array(values))
    }
    fn visit_map<M>(mut self, mut map: M) -> Result<Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        let mut fields = serde_json::Map::new();
        while let Some(key) = map.next_key_seed(TextSeed {
            ctx: self.ctx,
            refusal: &mut *self.refusal,
            operation: "f3d configuration JSON key",
            entry: true,
            expected: "a string key",
        })? {
            // serde_json's raw-value feature interprets this first-key carrier.
            if fields.is_empty() && key == "$serde_json::private::RawValue" {
                let raw = map.next_value_seed(TextSeed {
                    ctx: self.ctx,
                    refusal: &mut *self.refusal,
                    operation: "f3d configuration raw JSON text",
                    entry: false,
                    expected: "raw value",
                })?;
                let _raw_scratch = self.admit(reserve_json_scratch(
                    self.ctx,
                    raw.len(),
                    "f3d configuration raw JSON scratch",
                ))?;
                let mut deserializer = serde_json::Deserializer::from_str(&raw);
                let value = ValueSeed {
                    ctx: self.ctx,
                    refusal: &mut *self.refusal,
                    member: false,
                }
                .deserialize(&mut deserializer)
                .map_err(serde::de::Error::custom)?;
                deserializer.end().map_err(serde::de::Error::custom)?;
                return Ok(value);
            }
            self.admit(self.ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(fields.len()),
                "f3d configuration JSON object lookup",
            ))?;
            if !fields.contains_key(&key) {
                self.admit(self.ctx.admit_btree_node_storage::<String, Value>(
                    fields.len(),
                    "f3d configuration JSON object allocation",
                ))?;
            }
            let value = map.next_value_seed(ValueSeed {
                ctx: self.ctx,
                refusal: &mut *self.refusal,
                member: false,
            })?;
            // discarded-value: duplicate JSON object keys retain the last value.
            let _ = fields.insert(key, value);
        }
        Ok(Value::Object(fields))
    }
}
