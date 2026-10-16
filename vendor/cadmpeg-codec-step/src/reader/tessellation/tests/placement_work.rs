// SPDX-License-Identifier: Apache-2.0
//! Admission of placement comparison work.

use super::{CodecError, DecodePolicy};
use crate::test_support::with_service_context;

#[test]
fn distinct_placement_comparisons_refuse_before_quadratic_scan() {
    let placements = [cadmpeg_ir::transform::Transform::identity(); 32];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    crate::test_support::with_policy_context(&[], &policy, |_, ctx| {
        let error = super::super::distinct_placement_count(&placements, ctx)
            .expect_err("comparison work must be admitted");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.operation == "step_tessellation_distinct_placements"
                && Some(limit) == ctx.resource_refusal()));
    });
    with_service_context(&[], |_, ctx| {
        assert_eq!(
            super::super::distinct_placement_count(&placements, ctx).expect("admitted comparisons"),
            1
        );
    });
}
