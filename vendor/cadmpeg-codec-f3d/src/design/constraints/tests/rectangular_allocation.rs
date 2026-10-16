// SPDX-License-Identifier: Apache-2.0

use super::*;

#[derive(Clone, Copy)]
enum Limit {
    Collection,
    Retained,
    Work,
}

fn assert_rectangular_refusal(dimension: Limit, operation: &'static str) {
    let seed = point_entity("generated:test:point#rectangular-alloc-seed", 2.0);
    let second = point_entity("generated:test:point#rectangular-alloc-second", 17.0);
    let third = point_entity("generated:test:point#rectangular-alloc-third", 32.0);
    let relation =
        rectangular_point_relation(3, 1.5, RectangularPatternDistanceForm::AdjacentSpacing);
    let parameters = rectangular_parameters(3, 1.5);
    for limit in 0..512 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        match dimension {
            Limit::Collection => policy.limits.max_collection_items = limit,
            Limit::Retained => policy.limits.max_retained_bytes = limit,
            Limit::Work => policy.limits.max_work_units = limit,
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match exact_rectangular_pattern(
            &relation,
            "native",
            &parameters,
            &[&seed, &second, &third],
            &ctx,
        ) {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected refusal at {operation}: {other:?}"),
        }
    }
    panic!("no refusal at {operation}");
}

#[test]
fn rectangular_distance_parameter_id_refuses_retained_limit() {
    assert_rectangular_refusal(
        Limit::Retained,
        "f3d rectangular pattern distance parameter id",
    );
}

#[test]
fn rectangular_count_parameter_id_refuses_retained_limit() {
    assert_rectangular_refusal(
        Limit::Retained,
        "f3d rectangular pattern count parameter id",
    );
}

#[test]
fn rectangular_candidate_match_refuses_work_limit() {
    assert_rectangular_refusal(Limit::Work, "f3d rectangular pattern candidate match");
}

#[test]
fn rectangular_instance_entity_id_refuses_retained_limit() {
    assert_rectangular_refusal(
        Limit::Retained,
        "f3d rectangular pattern instance entity id",
    );
}

#[test]
fn rectangular_instance_entity_refuses_collection_limit() {
    assert_rectangular_refusal(Limit::Collection, "f3d rectangular pattern instance entity");
}

#[test]
fn rectangular_row_refuses_collection_limit() {
    assert_rectangular_refusal(Limit::Collection, "f3d rectangular pattern row");
}

#[test]
fn rectangular_instance_refuses_collection_limit() {
    assert_rectangular_refusal(Limit::Collection, "f3d rectangular pattern instance");
}
