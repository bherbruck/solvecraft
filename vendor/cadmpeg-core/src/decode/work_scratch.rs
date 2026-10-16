// SPDX-License-Identifier: Apache-2.0
//! Temporary evaluator storage governed by its work slice's decode session.

use super::budget::{DecodeBudget, ScopedReservation};
use super::error::ResourceLimit;

/// A temporary reservation whose lifetime covers evaluator-owned scratch.
#[derive(Debug)]
pub struct WorkScratch<'a> {
    reservation: Option<ScopedReservation<'a>>,
}

impl<'a> WorkScratch<'a> {
    pub(super) fn new(
        session: Option<&'a DecodeBudget>,
        bytes: u64,
        operation: &'static str,
    ) -> Result<Self, ResourceLimit> {
        let reservation = session
            .map(|session| session.reserve_scoped_limit(bytes, operation))
            .transpose()?;
        Ok(Self { reservation })
    }

    /// Admit additional scratch bytes before growing evaluator storage.
    ///
    /// # Errors
    /// Returns the original resource refusal from an attached decode session.
    pub fn grow(&mut self, bytes: u64) -> Result<(), ResourceLimit> {
        if let Some(reservation) = &mut self.reservation {
            reservation.grow_limit(bytes)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::arena::DecodeArena;
    use super::super::context::DecodeContext;
    use super::super::error::ResourceDimension;
    use super::super::policy::DecodePolicy;

    #[test]
    fn evaluator_scratch_tracks_live_growth_and_preserves_refusal() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 8;
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test operation");
        let budget = ctx.work_budget(10);
        {
            let mut scratch = budget
                .reserve_scratch(4, "test evaluator scratch")
                .expect("admitted test operation");
            scratch.grow(4).expect("admitted test operation");
        }
        let mut scratch = budget
            .reserve_scratch(8, "test evaluator scratch")
            .expect("admitted test operation");
        let error = scratch.grow(1).expect_err("test operation must refuse");
        assert_eq!(error.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(error.used, 8);
        assert_eq!(error.additional, 1);
        drop(scratch);
        assert!(
            matches!(ctx.finish_session(), Err(crate::CodecError::ResourceLimit(limit)) if limit == error)
        );
    }
}
