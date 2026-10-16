// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::CodecError;

#[test]
fn context_only_edge_candidate_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::design::edge_resolve::context_only_edge_group_candidates(
            [(Some(17), &[17][..])], &ctx),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d context-only edge candidate"
    ));
}
