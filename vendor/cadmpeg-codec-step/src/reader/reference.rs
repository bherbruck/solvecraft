// SPDX-License-Identifier: Apache-2.0
//! Fallible, allocation-free traversal of STEP entity references.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, DepthGuard};
use cadmpeg_core::CodecError;

use crate::parse::Value;

/// Fixed traversal storage; deeper values refuse before another frame is used.
const MAX_VALUE_FRAMES: usize = 513;

struct Frame<'a, 'ctx> {
    value: &'a Value,
    next_child: usize,
    /// Keeps caller depth admission live until this value is left.
    _depth: DepthGuard<'ctx>,
}

pub(super) struct References<'a, 'ctx, 'arena> {
    frames: [Option<Frame<'a, 'ctx>>; MAX_VALUE_FRAMES],
    active: usize,
    root: Option<&'a Value>,
    ctx: &'ctx DecodeContext<'arena>,
}

pub(super) fn references<'a, 'ctx, 'arena>(
    value: &'a Value,
    ctx: &'ctx DecodeContext<'arena>,
) -> References<'a, 'ctx, 'arena> {
    References {
        frames: std::array::from_fn(|_| None),
        active: 0,
        root: Some(value),
        ctx,
    }
}

impl<'a> References<'a, '_, '_> {
    fn push_frame(&mut self, value: &'a Value) -> Result<(), CodecError> {
        if self.active == MAX_VALUE_FRAMES {
            return Err(self.ctx.refuse_codec_limit(
                "step_reference_value_frames",
                u64_from_index(MAX_VALUE_FRAMES),
                u64_from_index(self.active + 1),
            ));
        }
        let depth = self.ctx.enter_nested("step_reference_value_walk")?;
        self.frames[self.active] = Some(Frame {
            value,
            next_child: 0,
            _depth: depth,
        });
        self.active += 1;
        Ok(())
    }

    fn advance(&mut self) -> Result<Option<u64>, CodecError> {
        if let Some(root) = self.root.take() {
            self.push_frame(root)?;
        }
        while self.active > 0 {
            self.ctx.charge_work(1, "step_reference_value_walk")?;
            let Some(frame) = self.frames[self.active - 1].as_mut() else {
                return Err(CodecError::malformed(
                    "STEP reference traversal has no active frame",
                ));
            };
            let child = match frame.value {
                Value::Reference(id) => {
                    let id = *id;
                    self.active -= 1;
                    self.frames[self.active] = None;
                    return Ok(Some(id));
                }
                Value::List(values) => {
                    let child = values.get(frame.next_child);
                    if child.is_some() {
                        frame.next_child += 1;
                    }
                    child
                }
                Value::Typed(_, value) if frame.next_child == 0 => {
                    frame.next_child = 1;
                    Some(value.as_ref())
                }
                _ => None,
            };
            if let Some(child) = child {
                self.push_frame(child)?;
            } else {
                self.active -= 1;
                self.frames[self.active] = None;
            }
        }
        Ok(None)
    }
}

impl Iterator for References<'_, '_, '_> {
    type Item = Result<u64, CodecError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.advance() {
            Ok(Some(id)) => Some(Ok(id)),
            Ok(None) => None,
            Err(error) => {
                self.active = 0;
                self.root = None;
                self.frames = std::array::from_fn(|_| None);
                Some(Err(error))
            }
        }
    }
}

pub(super) fn first_matching<'a>(
    values: impl IntoIterator<Item = &'a Value>,
    ctx: &DecodeContext<'_>,
    mut predicate: impl FnMut(u64) -> bool,
) -> Result<Option<u64>, CodecError> {
    for value in values {
        for reference in references(value, ctx) {
            let id = reference?;
            ctx.charge_work(1, "step_reference_predicate")?;
            if predicate(id) {
                return Ok(Some(id));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::{first_matching, references, MAX_VALUE_FRAMES};
    use crate::parse::Value;
    use crate::test_support::{with_policy_context, with_service_context};
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    #[test]
    fn nested_references_keep_source_order_without_a_collection() {
        let value = Value::List(vec![
            Value::Reference(1),
            Value::Typed(
                "WRAPPER".into(),
                Box::new(Value::List(vec![Value::Reference(2), Value::Integer(3)])),
            ),
            Value::Reference(4),
        ]);
        with_service_context(b"", |_, ctx| {
            assert_eq!(
                references(&value, ctx)
                    .collect::<Result<Vec<_>, _>>()
                    .expect("admitted traversal"),
                [1, 2, 4]
            );
            assert_eq!(
                first_matching([&value], ctx, |id| id > 1).expect("matching traversal"),
                Some(2)
            );
        });
    }

    #[test]
    fn admitted_nested_references_reach_the_leaf() {
        let mut value = Value::Reference(9);
        for _ in 0..256 {
            value = Value::Typed("WRAPPER".into(), Box::new(value));
        }
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 1024;
        with_policy_context(b"", &policy, |_, ctx| {
            assert_eq!(
                references(&value, ctx)
                    .collect::<Result<Vec<_>, _>>()
                    .expect("admitted traversal"),
                [9]
            );
        });
    }

    #[test]
    fn reference_iterator_frame_ceiling_refuses_instead_of_ending() {
        let mut value = Value::Reference(9);
        for _ in 0..MAX_VALUE_FRAMES {
            value = Value::List(vec![value]);
        }
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 1024;
        with_policy_context(b"", &policy, |_, ctx| {
            assert!(
                matches!(references(&value, ctx).collect::<Result<Vec<_>, _>>(),
                Err(CodecError::ResourceLimit(refusal)) if refusal.operation == "step_reference_value_frames")
            );
        });
        with_policy_context(b"", &policy, |_, ctx| {
            assert!(matches!(first_matching([&value], ctx, |_| true),
                Err(CodecError::ResourceLimit(refusal)) if refusal.operation == "step_reference_value_frames"));
        });
    }

    #[test]
    fn reference_matching_propagates_caller_depth_refusal() {
        let value = Value::List(vec![Value::Typed(
            "WRAPPER".into(),
            Box::new(Value::Reference(9)),
        )]);
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 2;
        with_policy_context(b"", &policy, |_, ctx| {
            assert!(matches!(first_matching([&value], ctx, |_| true),
                Err(CodecError::ResourceLimit(refusal)) if refusal.dimension == ResourceDimension::RecursionDepth
                    && refusal.operation == "step_reference_value_walk"));
        });
    }

    #[test]
    fn reference_walk_propagates_caller_work_refusal() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        with_policy_context(b"", &policy, |_, ctx| {
            assert!(
                matches!(first_matching([&Value::Reference(9)], ctx, |_| true),
                Err(CodecError::ResourceLimit(refusal)) if refusal.dimension == ResourceDimension::WorkUnits
                    && refusal.operation == "step_reference_value_walk")
            );
        });
    }

    #[test]
    fn early_matching_releases_reference_depth_guards() {
        let value = Value::List(vec![Value::Reference(9)]);
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 2;
        with_policy_context(b"", &policy, |_, ctx| {
            assert_eq!(
                first_matching([&value], ctx, |_| true).expect("matching traversal"),
                Some(9)
            );
            let _first = ctx
                .enter_nested("test first released level")
                .expect("root guard was released");
            let _second = ctx
                .enter_nested("test second released level")
                .expect("child guard was released");
        });
    }
}
