// SPDX-License-Identifier: Apache-2.0
//! Work admission for in-place unstable sorting.

use std::cmp::Ordering;

use super::{u64_from_index, DecodeContext};
use crate::CodecError;

pub(super) const INLINE_SORT_LIMIT: usize = 20;

/// Admit a linear check for a large run before allocating a sort permutation.
pub(super) fn admit_ordered_run<T>(
    ctx: &DecodeContext<'_>,
    values: &[T],
    compare: &mut impl FnMut(&T, &T) -> Ordering,
    key_bytes: &impl Fn(&T) -> usize,
    operation: &'static str,
) -> Result<bool, CodecError> {
    // Small unstable runs keep their fixed admission estimate, including
    // runs collected from hash tables.
    if values.len() <= INLINE_SORT_LIMIT {
        return Ok(false);
    }
    for pair in values.windows(2) {
        let work = u64_from_index(key_bytes(&pair[0]))
            .checked_add(u64_from_index(key_bytes(&pair[1])))
            .and_then(|work| {
                work.checked_add(u64_from_index(std::mem::size_of::<T>()).checked_mul(2)?)
            })
            .and_then(|work| work.checked_add(1))
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
        ctx.charge_work(work, operation)?;
        if compare(&pair[0], &pair[1]).is_gt() {
            return Ok(false);
        }
    }
    Ok(true)
}

impl DecodeContext<'_> {
    /// Sorts admitted values in place without allocating scratch.
    ///
    /// `key_bytes` states the external bytes a comparison can read from each value.
    pub fn sort_unstable_by<T>(
        &self,
        values: &mut [T],
        mut compare: impl FnMut(&T, &T) -> Ordering,
        key_bytes: impl Fn(&T) -> usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let count = u64_from_index(values.len());
        self.charge_work(count, operation)?;
        if admit_ordered_run(self, values, &mut compare, &key_bytes, operation)? {
            return Ok(());
        }
        let bytes = values
            .iter()
            .try_fold(0u64, |bytes, value| {
                bytes.checked_add(u64_from_index(key_bytes(value)))
            })
            .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
        let levels = u64::from(u64::BITS - count.leading_zeros()) + 1;
        let work = count
            .checked_mul(u64_from_index(std::mem::size_of::<T>()))
            .and_then(|storage| storage.checked_add(bytes.checked_mul(2)?))
            .and_then(|work| work.checked_mul(levels))
            .and_then(|work| work.checked_mul(8))
            .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
        self.charge_work(work, operation)?;
        values.sort_unstable_by(compare);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::DecodeContext;
    use crate::decode::{DecodeArena, DecodePolicy, ResourceDimension};
    use crate::CodecError;

    #[test]
    fn charged_unstable_sort_work_refusal_preserves_input() {
        let mut values = [3u64, 1, 2];
        let expected = values;
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Three elements, eight bytes each, three levels, eight work units per byte.
        policy.limits.max_work_units = 3 + 3 * 8 * 3 * 8 - 1;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        assert!(matches!(
            ctx.sort_unstable_by(&mut values, Ord::cmp, |_| 0, "test unstable sort"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits
        ));
        assert_eq!(values, expected);
    }

    #[test]
    fn charged_unstable_sort_orders_values_without_scratch() {
        let mut values = [3u64, 1, 2, 1];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        ctx.sort_unstable_by(&mut values, Ord::cmp, |_| 0, "test unstable sort")
            .expect("sort is admitted");
        assert_eq!(values, [1, 1, 2, 3]);
    }

    #[test]
    fn charged_unstable_sort_key_byte_overflow_preserves_input() {
        let mut values = [2u8, 1];
        let expected = values;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("empty root is admitted");
        assert!(matches!(
            ctx.sort_unstable_by(&mut values, Ord::cmp, |_| usize::MAX, "test unstable sort"),
            Err(CodecError::ResourceLimit(_))
        ));
        assert_eq!(values, expected);
    }
}
