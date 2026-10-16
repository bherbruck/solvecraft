// SPDX-License-Identifier: Apache-2.0
mod admission_limits;
mod components;
mod draft;
pub(crate) mod faces;
pub(crate) mod sheets;
pub(crate) mod shells;
mod wires;

mod indexing;
mod numerical_range;
mod representation_bodies;
mod seed_cache;

#[test]
fn topology_failure_count_refuses_overflow() {
    let mut outcome = super::BuildOutcome::Partial {
        built: Vec::new(),
        failures: super::BuildFailures {
            count: std::num::NonZeroUsize::new(usize::MAX).expect("nonzero maximum"),
            first: None,
        },
    };
    assert!(format!("{:?}", outcome.fail(None)).contains("ResourceLimit"));
}
