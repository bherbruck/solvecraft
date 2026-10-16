// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use std::collections::{HashMap, HashSet};

#[test]
fn brep_hash_graph_callbacks_admit_key_work_and_keep_refusals() {
    for (stored, query, work, found) in [
        ("alpha", "z-long-unread-tail", 37, false),
        ("alpha", "al", 5, false),
        ("alpha", "alpha", 11, true),
        ("é", "ê", 5, false),
        ("", "", 1, true),
        ("", "unread", 13, false),
    ] {
        for allowance in 0..=work {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = allowance;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            policy.limits.max_recursion_depth = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let values = HashSet::from([stored.to_owned()]);
            let result = super::contains_graph_id(&ctx, &values, query, "find graph fixture ID");
            if allowance < work {
                let CodecError::ResourceLimit(original) = result.unwrap_err() else {
                    panic!("comparison must refuse");
                };
                assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                assert_eq!(original.operation, "find graph fixture ID");
                assert_eq!(original.additional, work);
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
                );
            } else {
                assert_eq!(result.unwrap(), found);
                ctx.finish_session().unwrap();
            }
        }
    }
}

#[test]
fn brep_graph_searches_keep_original_fused_capsule() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let original = ctx
        .charge_work_limit(1, "original graph refusal")
        .unwrap_err();
    assert!(
        matches!(super::contains_graph_id(&ctx, &HashSet::new(), "absent", "empty graph lookup"), Err(CodecError::ResourceLimit(limit)) if limit == original)
    );
    assert!(
        matches!(super::insert_graph_id(&ctx, &mut HashSet::new(), "candidate".to_owned(), "empty graph insert"), Err(CodecError::ResourceLimit(limit)) if limit == original)
    );
    assert!(
        matches!(super::super::insert_brep_adjacency(&ctx, &mut HashMap::new(), "source", "target"), Err(CodecError::ResourceLimit(limit)) if limit == original)
    );
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
    );
}

#[test]
fn an_existing_dependency_pair_requires_no_new_storage() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut adjacency = HashMap::from([("source".into(), HashSet::from(["target".into()]))]);
    super::super::insert_brep_adjacency(&ctx, &mut adjacency, "source", "target").unwrap();
    assert_eq!(adjacency["source"], HashSet::from(["target".into()]));
    ctx.finish_session().unwrap();
}

#[test]
fn long_identity_selection_preserves_source_order_without_sorting_keys() {
    let prefix = "x".repeat(512);
    let rows = (0..2_000)
        .rev()
        .map(|index| format!("{prefix}#{index:04}"))
        .collect::<Vec<_>>();
    let reachable = rows
        .iter()
        .enumerate()
        .filter(|(index, _)| index % 3 != 0)
        .map(|(_, id)| id.clone())
        .collect::<HashSet<_>>();
    let expected = rows
        .iter()
        .enumerate()
        .filter(|(index, _)| index % 3 != 0)
        .map(|(_, id)| id.clone())
        .collect::<Vec<_>>();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One indexed membership lookup per row reads a bounded identity key.
    // Sorting the same long keys bills more than this three-million-unit slice.
    policy.limits.max_work_units = 3_000_000;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let retained = super::select_rows(&ctx, rows, &reachable, String::as_str).unwrap();
    assert_eq!(retained, expected);
    ctx.finish_session().unwrap();
}
