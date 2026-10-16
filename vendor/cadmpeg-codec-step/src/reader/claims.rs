// SPDX-License-Identifier: Apache-2.0
//! Source claims retain their admitted stage storage.

use std::collections::HashSet;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

pub(super) trait SourceClaims {
    fn contains(&self, id: u64) -> bool;
    fn remove(&mut self, id: u64) -> bool;
    fn retain(&mut self, keep: &mut dyn FnMut(&u64) -> bool);
}

impl SourceClaims for HashSet<u64> {
    fn contains(&self, id: u64) -> bool {
        HashSet::contains(self, &id)
    }
    fn remove(&mut self, id: u64) -> bool {
        HashSet::remove(self, &id)
    }
    fn retain(&mut self, keep: &mut dyn FnMut(&u64) -> bool) {
        HashSet::retain(self, keep);
    }
}

#[derive(Default)]
pub(super) struct ClaimSets {
    sets: Vec<HashSet<u64>>,
}

impl ClaimSets {
    pub(super) fn admit(
        &mut self,
        claims: HashSet<u64>,
        ctx: &DecodeContext<'_>,
    ) -> Result<(), CodecError> {
        ctx.charge_work(1, "step_stage_claims")?;
        if !claims.is_empty() {
            ctx.push_vec(&mut self.sets, claims, "step_stage_claims")?;
        }
        Ok(())
    }
}

impl SourceClaims for ClaimSets {
    fn contains(&self, id: u64) -> bool {
        self.sets.iter().any(|set| set.contains(&id))
    }
    fn remove(&mut self, id: u64) -> bool {
        let mut removed = false;
        for set in &mut self.sets {
            removed |= set.remove(&id);
        }
        removed
    }
    fn retain(&mut self, keep: &mut dyn FnMut(&u64) -> bool) {
        for set in &mut self.sets {
            set.retain(&mut *keep);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ClaimSets, SourceClaims};
    use cadmpeg_core::decode::DecodePolicy;

    #[test]
    fn stage_storage_transfers_without_copying_claims_and_withdraws_every_copy() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 2002;
        crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
            let first = ctx
                .collect_hash_set(0..1000, "fixture claims")
                .expect("first admitted set");
            let second = ctx
                .collect_hash_set(500..1500, "fixture claims")
                .expect("second admitted set");
            let mut claims = ClaimSets::default();
            claims.admit(first, ctx).expect("first owned set");
            claims.admit(second, ctx).expect("second owned set");
            for id in 0..1500 {
                assert!(claims.contains(id));
            }
            assert!(!claims.contains(1500));
            assert!(claims.remove(750));
            assert!(!claims.contains(750));
            claims.retain(&mut |id| id % 2 == 0);
            assert!(claims.contains(748));
            assert!(!claims.contains(749));
            assert!(!claims.contains(750));
        });
    }
}
