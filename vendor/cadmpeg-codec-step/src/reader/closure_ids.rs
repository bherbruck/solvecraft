// SPDX-License-Identifier: Apache-2.0
//! Checked sparse membership for STEP record closures.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

#[derive(Debug, Default)]
pub(super) struct ClosureIds {
    blocks: BTreeMap<u64, u64>,
}

impl ClosureIds {
    pub(super) fn contains(&self, id: u64) -> bool {
        self.blocks
            .get(&(id / u64::from(u64::BITS)))
            .is_some_and(|bits| bits & (1_u64 << (id % u64::from(u64::BITS))) != 0)
    }

    pub(super) fn insert(&mut self, id: u64, ctx: &DecodeContext<'_>) -> Result<bool, CodecError> {
        let block = id / u64::from(u64::BITS);
        let mask = 1_u64 << (id % u64::from(u64::BITS));
        ctx.admit_btree_entry(&self.blocks, &block, "step_record_closure_ids")?;
        let bits = self.blocks.entry(block).or_default();
        let inserted = *bits & mask == 0;
        *bits |= mask;
        Ok(inserted)
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    use super::ClosureIds;

    #[test]
    fn membership_charges_one_allocated_block_and_checks_the_next() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
            let mut ids = ClosureIds::default();
            for id in 0..64 {
                assert!(ids.insert(id, ctx).expect("one admitted block"));
                assert!(ids.contains(id));
                assert!(!ids.insert(id, ctx).expect("existing bit"));
            }
            assert!(!ids.contains(64));
            assert!(
                matches!(ids.insert(64, ctx), Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "step_record_closure_ids")
            );
            assert!(!ids.contains(64));
        });
    }

    #[test]
    fn sparse_and_maximum_identities_do_not_require_dense_storage() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 3;
        crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
            let mut ids = ClosureIds::default();
            for id in [0, 63, 64, 127, u64::MAX - 1, u64::MAX] {
                assert!(ids.insert(id, ctx).expect("three sparse blocks"));
                assert!(ids.contains(id));
            }
            assert!(!ids.contains(128));
            assert!(!ids.contains(u64::MAX - 2));
        });
    }
}
