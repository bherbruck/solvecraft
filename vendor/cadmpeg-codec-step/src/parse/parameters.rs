// SPDX-License-Identifier: Apache-2.0
//! Inline storage for short entity parameter lists.

use super::{CodecError, DecodeContext, Value};

pub(super) trait ParameterValues: Default {
    fn is_empty(&self) -> bool;
    fn push_value(&mut self, value: Value, ctx: &DecodeContext<'_>) -> Result<(), CodecError>;
}

impl ParameterValues for Vec<Value> {
    fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }

    fn push_value(&mut self, value: Value, ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
        ctx.push_vec(self, value, "step_parse_parameter")
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct RecordParameters(Storage);

#[derive(Debug, Clone, Default, PartialEq)]
enum Storage {
    #[default]
    Empty,
    One(Value),
    Pair([Value; 2]),
    Many(Vec<Value>),
}

impl RecordParameters {
    pub(crate) fn as_slice(&self) -> &[Value] {
        self
    }

    pub(super) fn prepend(
        &mut self,
        value: Value,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        match &mut self.0 {
            Storage::Empty => self.0 = Storage::One(value),
            Storage::One(first) => {
                let first = std::mem::replace(first, Value::Omitted);
                self.0 = Storage::Pair([value, first]);
            }
            Storage::Pair(pair) => {
                let mut values = ctx.collection_vec(3, operation)?;
                let pair = std::mem::replace(pair, [Value::Omitted, Value::Omitted]);
                values.push(value);
                values.extend(pair);
                self.0 = Storage::Many(values);
            }
            Storage::Many(values) => {
                ctx.reserve_vec(values, 1, operation)?;
                values.insert(0, value);
            }
        }
        Ok(())
    }
}

impl ParameterValues for RecordParameters {
    fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }

    fn push_value(&mut self, value: Value, ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
        match &mut self.0 {
            Storage::Empty => self.0 = Storage::One(value),
            Storage::One(first) => {
                let first = std::mem::replace(first, Value::Omitted);
                self.0 = Storage::Pair([first, value]);
            }
            Storage::Pair(pair) => {
                let mut values = ctx.collection_vec(3, "step_parse_parameter")?;
                let pair = std::mem::replace(pair, [Value::Omitted, Value::Omitted]);
                values.extend(pair);
                values.push(value);
                self.0 = Storage::Many(values);
            }
            Storage::Many(values) => ctx.push_vec(values, value, "step_parse_parameter")?,
        }
        Ok(())
    }
}

impl std::ops::Deref for RecordParameters {
    type Target = [Value];

    fn deref(&self) -> &Self::Target {
        match &self.0 {
            Storage::Empty => &[],
            Storage::One(value) => std::slice::from_ref(value),
            Storage::Pair(pair) => pair,
            Storage::Many(values) => values,
        }
    }
}

impl std::ops::DerefMut for RecordParameters {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match &mut self.0 {
            Storage::Empty => &mut [],
            Storage::One(value) => std::slice::from_mut(value),
            Storage::Pair(pair) => pair,
            Storage::Many(values) => values,
        }
    }
}

impl<'a> IntoIterator for &'a RecordParameters {
    type Item = &'a Value;
    type IntoIter = std::slice::Iter<'a, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a> IntoIterator for &'a mut RecordParameters {
    type Item = &'a mut Value;
    type IntoIter = std::slice::IterMut<'a, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};

    use super::{CodecError, ParameterValues, RecordParameters, Value};
    use crate::test_support::with_policy_context;

    #[test]
    fn short_parameters_are_inline_and_heap_growth_remains_checked() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        with_policy_context(b"", &policy, |_, ctx| {
            let mut values = RecordParameters::default();
            values
                .push_value(Value::Integer(1), ctx)
                .expect("inline first");
            values
                .push_value(Value::Integer(2), ctx)
                .expect("inline second");
            assert!(matches!(values.push_value(Value::Integer(3), ctx),
                Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems && limit.additional == 3));
            assert_eq!(values.as_slice(), [Value::Integer(1), Value::Integer(2)]);
        });
        policy.limits.max_collection_items = 4;
        with_policy_context(b"", &policy, |_, ctx| {
            let mut values = RecordParameters::default();
            for value in 1..=4 {
                values
                    .push_value(Value::Integer(value), ctx)
                    .expect("checked heap population");
            }
            assert_eq!(
                values.as_slice(),
                [
                    Value::Integer(1),
                    Value::Integer(2),
                    Value::Integer(3),
                    Value::Integer(4)
                ]
            );
        });
    }

    #[test]
    fn prepend_uses_inline_storage_and_preserves_values_on_refusal() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        with_policy_context(b"", &policy, |_, ctx| {
            let mut values = RecordParameters::default();
            values
                .push_value(Value::Integer(2), ctx)
                .expect("inline first");
            values
                .prepend(Value::Integer(1), ctx, "test prepend")
                .expect("inline leading value");
            assert_eq!(values.as_slice(), [Value::Integer(1), Value::Integer(2)]);
            assert!(values
                .prepend(Value::Integer(0), ctx, "test prepend")
                .is_err());
            assert_eq!(values.as_slice(), [Value::Integer(1), Value::Integer(2)]);
        });
    }
}
