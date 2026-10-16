// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

#[test]
fn edge_assignment_visits_refuse_materialized_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_materialized_bytes = 7;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::bipartite_assignment(&[vec![17]], None, &ctx)
        .expect_err("one visited edge needs eight bytes");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.operation == "f3d edge assignment visits"
    ));
}

#[test]
fn edge_assignment_members_refuse_materialized_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    // Four i64 buckets, four controls, sixteen trailing controls and fifteen padding bytes.
    policy.limits.max_materialized_bytes = 4 * 8 + 4 + 16 + 15;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::bipartite_assignment(&[vec![17]], None, &ctx)
        .expect_err("member table exceeds the live visited-set storage allowance");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.operation == "f3d edge assignment members"
    ));
}

#[test]
fn edge_assignment_search_refuses_recursion_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::bipartite_assignment(&[vec![17]], None, &ctx)
        .expect_err("one search frame needs one recursion level");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RecursionDepth
                && limit.operation == "f3d edge assignment search"
    ));
}

#[test]
fn edge_assignment_candidate_refuses_work_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::bipartite_assignment(&[vec![17]], None, &ctx)
        .expect_err("one candidate needs one work unit");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "f3d edge assignment candidate"
    ));
}
