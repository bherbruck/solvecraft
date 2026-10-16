// SPDX-License-Identifier: Apache-2.0
//! One-table indexes that exclude every repeated key.

use super::{u64_from_index, DecodeContext, ScopedReservation};
use crate::CodecError;
use std::collections::HashMap;
use std::hash::Hash;

impl DecodeContext<'_> {
    /// Builds a scoped table. Repeated keys become tombstones until the final
    /// in-place removal, so a third occurrence cannot restore a duplicate.
    /// Surviving values are Some; no second table is allocated.
    pub fn unique_index<K: Eq + Hash, V>(
        &self,
        entries: impl IntoIterator<Item = (K, V)>,
        key_work: impl Fn(&K) -> Result<u64, CodecError>,
        operation: &'static str,
    ) -> Result<(HashMap<K, Option<V>>, ScopedReservation<'_>), CodecError> {
        let mut table = HashMap::<K, Option<V>>::new();
        let mut storage = self.reserve_scoped(0, operation)?;
        for (key, value) in entries {
            self.charge_work(1, operation)?;
            self.charge_work(key_work(&key)?, operation)?;
            if let Some(previous) = table.get_mut(&key) {
                *previous = None;
                continue;
            }
            self.charge_collection_items(1, operation)?;
            if table.len() == table.capacity() {
                self.charge_work(u64_from_index(table.capacity()), operation)?;
                for stored in table.keys() {
                    self.charge_work(key_work(stored)?, operation)?;
                }
                // A hash table reserves at most four buckets per requested
                // entry, including load-factor rounding. Each bucket holds
                // its pair and control bytes. The extra 32 bytes cover
                // alignment and control-group padding. Keep the old and new
                // tables admitted together while fallible growth rehashes.
                let required = table
                    .len()
                    .checked_add(1)
                    .and_then(|count| count.checked_mul(4))
                    .and_then(|count| {
                        count.checked_mul(
                            std::mem::size_of::<(K, Option<V>)>()
                                .max(1)
                                .checked_add(32)?,
                        )
                    })
                    .ok_or_else(|| {
                        CodecError::from(self.budget.scoped_size_overflow_limit(operation))
                    })?;
                storage.grow(u64_from_index(required))?;
                table.try_reserve(1).map_err(|_| {
                    self.budget
                        .scoped_allocation_failed(u64_from_index(required), operation)
                })?;
            }
            self.charge_work(key_work(&key)?, operation)?;
            // discarded-value: the key was absent before the admitted insertion.
            let _ = table.insert(key, Some(value));
        }
        self.charge_work(u64_from_index(table.capacity()), operation)?;
        table.retain(|_, value| value.is_some());
        Ok((table, storage))
    }
}

#[cfg(test)]
mod tests {
    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use crate::CodecError;

    #[test]
    fn unique_index_removes_all_duplicate_occurrences_in_one_table() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 2;
        policy.limits.max_materialized_bytes =
            super::u64_from_index(4 * (std::mem::size_of::<(u32, Option<i32>)>() + 32));
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let (table, storage) = ctx
            .unique_index(
                [(1_u32, 2), (1, 3), (1, 4), (2, 5)],
                |_| Ok(4),
                "unique test",
            )
            .expect("two slots");
        assert_eq!(table.len(), 1);
        assert_eq!(table.get(&2), Some(&Some(5)));
        assert!(!table.contains_key(&1));
        drop((table, storage));
        ctx.reserve_scoped(policy.limits.max_materialized_bytes, "released table")
            .expect("storage released");
    }

    #[test]
    fn unique_index_one_slot_keeps_one_record() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let (table, _storage) = ctx
            .unique_index([(7_u32, 7)], |_| Ok(4), "unique test")
            .expect("one table slot");
        assert_eq!(table.get(&7), Some(&Some(7)));
    }

    #[test]
    fn unique_index_propagates_slot_workspace_and_key_work_refusals() {
        for dimension in [
            ResourceDimension::CollectionItems,
            ResourceDimension::MaterializedBytes,
            ResourceDimension::WorkUnits,
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 3,
                _ => panic!("test dimension"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
            assert!(
                matches!(ctx.unique_index([("long key", 1)], |key| Ok(super::u64_from_index(key.len())), "unique test"),
                Err(CodecError::ResourceLimit(limit)) if limit.dimension == dimension)
            );
        }
    }
}
