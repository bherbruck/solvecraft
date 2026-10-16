// SPDX-License-Identifier: Apache-2.0
//! Test-only charge probes. Normal builds contain no probe state or calls.

use std::cell::RefCell;

use super::{ResourceDimension, ResourceLimits};

#[derive(Debug)]
struct Target {
    dimension: ResourceDimension,
    operation: String,
    skip: usize,
    peak: u64,
}

thread_local! {
    static TARGET: RefCell<Option<Target>> = const { RefCell::new(None) };
}

#[derive(Debug)]
struct RestoreTarget(Option<Target>);

impl Drop for RestoreTarget {
    fn drop(&mut self) {
        TARGET.with(|target| *target.borrow_mut() = self.0.take());
    }
}

/// Refuse a named charge at a new budget boundary, one unit below its need.
///
/// `skip` counts matching boundaries admitted before probing. A charge is
/// reachable by a normal ceiling only when its need exceeds all earlier needs
/// in this dimension. Released storage can make later needs smaller.
/// Only budgets whose selected policy ceiling is `u64::MAX` participate.
/// Fixture setup with a separate fixed policy keeps its normal behavior.
///
/// The probe can only lower a policy ceiling. It affects this thread until
/// `run` returns or unwinds; nested probes restore the outer target. Tests
/// must repeat the decode with the discovered policy ceiling and no probe.
pub fn at_charge<T>(
    dimension: ResourceDimension,
    operation: &str,
    skip: usize,
    run: impl FnOnce() -> T,
) -> T {
    with_target(
        Some(Target {
            dimension,
            operation: operation.to_owned(),
            skip,
            peak: 0,
        }),
        run,
    )
}

/// Run a normal policy check while suspending any enclosing charge probe.
/// The outer probe is restored when `run` returns or unwinds.
pub fn without_probe<T>(run: impl FnOnce() -> T) -> T {
    with_target(None, run)
}

fn with_target<T>(replacement: Option<Target>, run: impl FnOnce() -> T) -> T {
    let previous = TARGET.with(|target| target.replace(replacement));
    let guard = RestoreTarget(previous);
    let result = run();
    drop(guard);
    result
}

pub(super) fn charge_limit(
    limits: &ResourceLimits,
    dimension: ResourceDimension,
    operation: &str,
    used: u64,
    additional: u64,
) -> Option<u64> {
    if additional == 0 {
        return None;
    }
    TARGET.with(|target| {
        let mut target = target.borrow_mut();
        let target = target.as_mut()?;
        if target.dimension != dimension {
            return None;
        }
        let ceiling = match dimension {
            ResourceDimension::InputBytes => limits.max_input_bytes,
            ResourceDimension::DecompressedBytes => limits.max_decompressed_bytes_total,
            ResourceDimension::MaterializedBytes => limits.max_materialized_bytes,
            ResourceDimension::RetainedBytes => limits.max_retained_bytes,
            ResourceDimension::Entities => limits.max_entities,
            ResourceDimension::CollectionItems => limits.max_collection_items,
            ResourceDimension::RecursionDepth => limits.max_recursion_depth,
            ResourceDimension::WorkUnits => limits.max_work_units,
            ResourceDimension::Codec(_) => return None,
        };
        if ceiling != u64::MAX {
            return None;
        }
        let need = used.checked_add(additional)?;
        if need <= target.peak {
            return None;
        }
        target.peak = need;
        if target.operation != operation {
            return None;
        }
        if target.skip > 0 {
            target.skip -= 1;
            return None;
        }
        need.checked_sub(1)
    })
}

#[cfg(test)]
mod tests {
    use super::{at_charge, without_probe};
    use crate::decode::budget::DecodeBudget;
    use crate::decode::{DecodePolicy, ResourceDimension, ResourceLimit};

    fn work_probe_policy() -> DecodePolicy {
        let mut policy = DecodePolicy::desktop();
        policy.limits.max_work_units = u64::MAX;
        policy
    }

    fn run(policy: DecodePolicy) -> Result<(), ResourceLimit> {
        let budget = DecodeBudget::new(policy, 0);
        budget.charge_work_limit(2, "before")?;
        budget.charge_work_limit(0, "target")?;
        budget.charge_work_limit(2, "target")
    }

    #[test]
    fn probe_matches_normal_policy_refusal_and_does_not_raise_limits() {
        let found = at_charge(ResourceDimension::WorkUnits, "target", 0, || {
            run(work_probe_policy())
        })
        .expect_err("target is probed");
        assert_eq!(
            (found.operation, found.used, found.additional),
            ("target", 2, 2)
        );
        let mut policy = DecodePolicy::desktop();
        policy.limits.max_work_units = found.used + found.additional - 1;
        assert_eq!(run(policy), Err(found));
        policy.limits.max_work_units = 0;
        let earlier = at_charge(ResourceDimension::WorkUnits, "target", 0, || run(policy))
            .expect_err("real ceiling still refuses before target");
        assert_eq!((earlier.operation, earlier.limit), ("before", 0));
        assert!(run(work_probe_policy()).is_ok());
    }

    #[test]
    fn probes_are_thread_local_and_nested_scopes_restore_the_target() {
        at_charge(ResourceDimension::WorkUnits, "target", 0, || {
            let child = std::thread::spawn(|| run(work_probe_policy()));
            assert!(child.join().expect("child completes").is_ok());
            assert!(
                at_charge(ResourceDimension::CollectionItems, "other", 0, || {
                    run(work_probe_policy())
                })
                .is_ok()
            );
            assert!(without_probe(|| run(work_probe_policy())).is_ok());
            assert!(run(work_probe_policy()).is_err());
        });
        assert!(run(work_probe_policy()).is_ok());
    }

    #[test]
    fn probe_counts_matching_positive_charges_only() {
        let found = at_charge(ResourceDimension::WorkUnits, "target", 1, || {
            let budget = DecodeBudget::new(work_probe_policy(), 0);
            budget.charge_work_limit(0, "target")?;
            budget.charge_work_limit(3, "target")?;
            budget.charge_work_limit(2, "other")?;
            budget.charge_work_limit(7, "target")
        })
        .expect_err("second positive target charge is probed");
        assert_eq!((found.used, found.additional, found.limit), (5, 7, 11));
        let mut policy = DecodePolicy::desktop();
        policy.limits.max_work_units = found.limit;
        let budget = DecodeBudget::new(policy, 0);
        budget
            .charge_work_limit(3, "target")
            .expect("first target fits");
        budget
            .charge_work_limit(2, "other")
            .expect("other charge fits");
        assert_eq!(budget.charge_work_limit(7, "target"), Err(found));
    }

    #[test]
    fn released_storage_skips_boundaries_hidden_by_an_earlier_peak() {
        fn run_materialized(policy: DecodePolicy) -> Result<(), ResourceLimit> {
            let budget = DecodeBudget::new(policy, 0);
            drop(budget.reserve_scoped_limit(7, "before")?);
            drop(budget.reserve_scoped_limit(2, "target")?);
            budget.reserve_scoped_limit(9, "target").map(drop)
        }
        let found = at_charge(ResourceDimension::MaterializedBytes, "target", 0, || {
            let mut policy = DecodePolicy::desktop();
            policy.limits.max_materialized_bytes = u64::MAX;
            run_materialized(policy)
        })
        .expect_err("first reachable target boundary is probed");
        assert_eq!((found.used, found.additional, found.limit), (0, 9, 8));
        let mut policy = DecodePolicy::desktop();
        policy.limits.max_materialized_bytes = found.limit;
        assert_eq!(run_materialized(policy), Err(found));
    }

    #[test]
    fn an_unwind_restores_the_outer_target() {
        at_charge(ResourceDimension::WorkUnits, "target", 0, || {
            let result = std::panic::catch_unwind(|| {
                at_charge(ResourceDimension::CollectionItems, "other", 0, || {
                    panic!("test unwind");
                });
            });
            assert!(result.is_err());
            assert!(run(work_probe_policy()).is_err());
        });
        assert!(run(work_probe_policy()).is_ok());
    }

    #[test]
    fn fixed_policy_fixture_setup_does_not_hide_or_refuse_the_tested_budget() {
        let found = at_charge(ResourceDimension::WorkUnits, "target", 0, || {
            let setup = DecodeBudget::new(DecodePolicy::service(), 0);
            setup.charge_work_limit(1000, "before")?;
            setup.charge_work_limit(1000, "target")?;
            run(work_probe_policy())
        })
        .expect_err("only the budget selected by the helper is probed");
        assert_eq!(
            (found.operation, found.used, found.additional, found.limit),
            ("target", 2, 2, 3)
        );
    }
}
