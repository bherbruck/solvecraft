// SPDX-License-Identifier: Apache-2.0

use super::super::ExactVec;
use super::context;
use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use crate::CodecError;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, VecDeque};
collection_case!(
    collection_vec_charges_before_allocation,
    2,
    |ctx: &DecodeContext<'_>| ctx
        .collection_vec::<u8>(2, "test collection vec")
        .map(|_| ()),
    |ctx: &DecodeContext<'_>| ctx
        .collection_vec::<u8>(2, "test collection vec")
        .map(|_| ())
);

#[test]
fn exact_vec_charges_before_allocation_and_requires_full_count() {
    let bytes = [0_u8, 0];
    let count = crate::decode::View::over_retained(&bytes)
        .counted(2, 1)
        .expect("two bytes prove two items");
    let arena = DecodeArena::new();
    let ctx = context(&arena, 1);
    assert!(matches!(
        ExactVec::<u8>::new(&ctx, count, "test exact vec"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
    ));
    let arena = DecodeArena::new();
    let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
    let mut values = ExactVec::new(&ctx, count, "test exact vec").expect("service profile");
    values.push(1_u8).expect("first item fits");
    values.push(2_u8).expect("second item fits");
    assert!(values.push(3_u8).is_err());
    assert_eq!(values.finish().expect("exact count"), [1, 2]);
    let mut short = ExactVec::new(&ctx, count, "test exact vec").expect("service profile");
    short.push(1_u8).expect("first item fits");
    assert!(short.finish().is_err());
}
collection_case!(
    reserve_vec_charges_before_growth,
    1,
    |ctx: &DecodeContext<'_>| ctx.reserve_vec(&mut Vec::<u8>::new(), 1, "test reserve vec"),
    |ctx: &DecodeContext<'_>| ctx.reserve_vec(&mut Vec::<u8>::new(), 1, "test reserve vec")
);
collection_case!(
    push_vec_charges_before_growth,
    1,
    |ctx: &DecodeContext<'_>| ctx.push_vec(&mut Vec::new(), 7_u8, "test push vec"),
    |ctx: &DecodeContext<'_>| ctx.push_vec(&mut Vec::new(), 7_u8, "test push vec")
);
collection_case!(
    push_formatted_retained_charges_before_growth,
    1,
    |ctx: &DecodeContext<'_>| ctx.push_formatted_retained(
        &mut Vec::new(),
        format_args!("a"),
        "test note slots",
        "test note text"
    ),
    |ctx: &DecodeContext<'_>| ctx.push_formatted_retained(
        &mut Vec::new(),
        format_args!("a"),
        "test note slots",
        "test note text"
    )
);
collection_case!(
    append_vec_charges_before_growth,
    2,
    |ctx: &DecodeContext<'_>| ctx.append_vec(
        &mut Vec::new(),
        &mut vec![1_u8, 2],
        "test append vec"
    ),
    |ctx: &DecodeContext<'_>| ctx.append_vec(
        &mut Vec::new(),
        &mut vec![1_u8, 2],
        "test append vec"
    )
);
collection_case!(
    extend_vec_charges_before_growth,
    2,
    |ctx: &DecodeContext<'_>| ctx.extend_vec(&mut Vec::new(), vec![1_u8, 2], "test extend vec"),
    |ctx: &DecodeContext<'_>| ctx.extend_vec(&mut Vec::new(), vec![1_u8, 2], "test extend vec")
);
collection_case!(
    collect_vec_charges_before_growth,
    2,
    |ctx: &DecodeContext<'_>| ctx.collect_vec([1_u8, 2], "test collect vec").map(|_| ()),
    |ctx: &DecodeContext<'_>| ctx.collect_vec([1_u8, 2], "test collect vec").map(|_| ())
);
collection_case!(
    try_collect_vec_charges_before_growth,
    2,
    |ctx: &DecodeContext<'_>| ctx
        .try_collect_vec([Ok::<u8, CodecError>(1), Ok(2)], "test try collect vec")
        .map(|_| ()),
    |ctx: &DecodeContext<'_>| ctx
        .try_collect_vec([Ok::<u8, CodecError>(1), Ok(2)], "test try collect vec")
        .map(|_| ())
);
collection_case!(
    insert_hash_set_charges_before_growth,
    1,
    |ctx: &DecodeContext<'_>| ctx
        .insert_hash_set(&mut HashSet::new(), 1_u8, "test insert set")
        .map(|_| ()),
    |ctx: &DecodeContext<'_>| ctx
        .insert_hash_set(&mut HashSet::new(), 1_u8, "test insert set")
        .map(|_| ())
);
collection_case!(
    insert_string_set_charges_before_growth,
    1,
    |ctx: &DecodeContext<'_>| ctx
        .insert_string_set(&mut HashSet::new(), "a", "test insert string")
        .map(|_| ()),
    |ctx: &DecodeContext<'_>| ctx
        .insert_string_set(&mut HashSet::new(), "a", "test insert string")
        .map(|_| ())
);
collection_case!(
    collect_hash_set_charges_before_growth,
    2,
    |ctx: &DecodeContext<'_>| ctx
        .collect_hash_set([1_u8, 2], "test collect set")
        .map(|_| ()),
    |ctx: &DecodeContext<'_>| ctx
        .collect_hash_set([1_u8, 2], "test collect set")
        .map(|_| ())
);
collection_case!(
    admit_hash_map_entry_charges_before_growth,
    1,
    |ctx: &DecodeContext<'_>| ctx.admit_hash_map_entry(
        &mut HashMap::<u8, u8>::new(),
        &1,
        "test admit map"
    ),
    |ctx: &DecodeContext<'_>| ctx.admit_hash_map_entry(
        &mut HashMap::<u8, u8>::new(),
        &1,
        "test admit map"
    )
);
collection_case!(
    insert_hash_map_charges_before_growth,
    1,
    |ctx: &DecodeContext<'_>| ctx
        .insert_hash_map(&mut HashMap::new(), 1_u8, 2_u8, "test insert map")
        .map(|_| ()),
    |ctx: &DecodeContext<'_>| ctx
        .insert_hash_map(&mut HashMap::new(), 1_u8, 2_u8, "test insert map")
        .map(|_| ())
);
collection_case!(
    collect_hash_map_charges_before_growth,
    2,
    |ctx: &DecodeContext<'_>| ctx
        .collect_hash_map([(1_u8, 2_u8), (3, 4)], "test collect map")
        .map(|_| ()),
    |ctx: &DecodeContext<'_>| ctx
        .collect_hash_map([(1_u8, 2_u8), (3, 4)], "test collect map")
        .map(|_| ())
);
collection_case!(
    push_back_charges_before_growth,
    1,
    |ctx: &DecodeContext<'_>| ctx.push_back(&mut VecDeque::new(), 1_u8, "test push back"),
    |ctx: &DecodeContext<'_>| ctx.push_back(&mut VecDeque::new(), 1_u8, "test push back")
);
#[test]
fn push_front_refuses_before_allocation() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let mut values = VecDeque::new();
    assert!(
        matches!(ctx.push_front(&mut values, 1_u8, "test push front"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems)
    );
    assert_eq!(values.capacity(), 0);
    assert!(values.is_empty());
}

#[test]
fn push_front_service_profile_preserves_order() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
    let mut values = VecDeque::from([2_u8]);
    ctx.push_front(&mut values, 1, "test push front")
        .expect("service profile");
    assert_eq!(values, VecDeque::from([1, 2]));
}

collection_case!(
    reserve_heap_charges_before_growth,
    1,
    |ctx: &DecodeContext<'_>| ctx.reserve_heap(
        &mut BinaryHeap::<u8>::new(),
        1,
        "test reserve heap"
    ),
    |ctx: &DecodeContext<'_>| ctx.reserve_heap(
        &mut BinaryHeap::<u8>::new(),
        1,
        "test reserve heap"
    )
);
collection_case!(
    copy_slice_charges_before_allocation,
    2,
    |ctx: &DecodeContext<'_>| ctx.copy_slice(&[1_u8, 2], "test copy slice").map(|_| ()),
    |ctx: &DecodeContext<'_>| ctx.copy_slice(&[1_u8, 2], "test copy slice").map(|_| ())
);
collection_case!(
    collect_options_charges_before_growth,
    2,
    |ctx: &DecodeContext<'_>| ctx
        .collect_options([Some(1_u8), Some(2)], "test collect options")
        .map(|_| ()),
    |ctx: &DecodeContext<'_>| ctx
        .collect_options([Some(1_u8), Some(2)], "test collect options")
        .map(|_| ())
);
collection_case!(
    collect_fallible_options_charges_before_growth,
    2,
    |ctx: &DecodeContext<'_>| ctx
        .collect_fallible_options(
            [Ok::<Option<u8>, CodecError>(Some(1)), Ok(Some(2))],
            "test fallible options"
        )
        .map(|_| ()),
    |ctx: &DecodeContext<'_>| ctx
        .collect_fallible_options(
            [Ok::<Option<u8>, CodecError>(Some(1)), Ok(Some(2))],
            "test fallible options"
        )
        .map(|_| ())
);
collection_case!(
    collect_string_set_charges_before_growth,
    1,
    |ctx: &DecodeContext<'_>| ctx
        .collect_string_set(["one"], "test string set")
        .map(|_| ()),
    |ctx: &DecodeContext<'_>| ctx
        .collect_string_set(["one"], "test string set")
        .map(|_| ())
);
collection_case!(
    reserve_set_charges_before_growth,
    1,
    |ctx: &DecodeContext<'_>| ctx.reserve_set(&mut HashSet::<u8>::new(), 1, "test reserve set"),
    |ctx: &DecodeContext<'_>| ctx.reserve_set(&mut HashSet::<u8>::new(), 1, "test reserve set")
);
collection_case!(
    reserve_map_charges_before_growth,
    1,
    |ctx: &DecodeContext<'_>| ctx.reserve_map(&mut HashMap::<u8, u8>::new(), 1, "test reserve map"),
    |ctx: &DecodeContext<'_>| ctx.reserve_map(&mut HashMap::<u8, u8>::new(), 1, "test reserve map")
);
collection_case!(
    admit_btree_entry_charges_before_growth,
    1,
    |ctx: &DecodeContext<'_>| ctx.admit_btree_entry(
        &BTreeMap::<u8, u8>::new(),
        &1,
        "test admit btree"
    ),
    |ctx: &DecodeContext<'_>| ctx.admit_btree_entry(
        &BTreeMap::<u8, u8>::new(),
        &1,
        "test admit btree"
    )
);
collection_case!(
    insert_btree_map_charges_before_growth,
    1,
    |ctx: &DecodeContext<'_>| ctx
        .insert_btree_map(&mut BTreeMap::new(), 1_u8, 2_u8, "test insert btree map")
        .map(|_| ()),
    |ctx: &DecodeContext<'_>| ctx
        .insert_btree_map(&mut BTreeMap::new(), 1_u8, 2_u8, "test insert btree map")
        .map(|_| ())
);
collection_case!(
    insert_btree_set_charges_before_growth,
    1,
    |ctx: &DecodeContext<'_>| ctx
        .insert_btree_set(&mut BTreeSet::new(), 1_u8, "test insert btree set")
        .map(|_| ()),
    |ctx: &DecodeContext<'_>| ctx
        .insert_btree_set(&mut BTreeSet::new(), 1_u8, "test insert btree set")
        .map(|_| ())
);
collection_case!(
    extend_retained_bytes_charges_before_growth,
    1,
    |ctx: &DecodeContext<'_>| ctx.extend_retained_bytes(&mut Vec::new(), b"a", "test extend bytes"),
    |ctx: &DecodeContext<'_>| ctx.extend_retained_bytes(&mut Vec::new(), b"a", "test extend bytes")
);
collection_case!(
    temporary_vec_charges_before_growth,
    1,
    |ctx: &DecodeContext<'_>| ctx.temporary_vec::<u8>(1, "test temporary vec").map(|_| ()),
    |ctx: &DecodeContext<'_>| ctx.temporary_vec::<u8>(1, "test temporary vec").map(|_| ())
);
collection_case!(
    copy_retained_strings_charges_before_growth,
    2,
    |ctx: &DecodeContext<'_>| ctx
        .copy_retained_strings(
            &[String::from("a"), String::from("b")],
            "test retained strings"
        )
        .map(|_| ()),
    |ctx: &DecodeContext<'_>| ctx
        .copy_retained_strings(
            &[String::from("a"), String::from("b")],
            "test retained strings"
        )
        .map(|_| ())
);
collection_case!(
    optional_collection_vec_charges_before_allocation,
    2,
    |ctx: &DecodeContext<'_>| ctx
        .optional_collection_vec::<u8>(true, 2, "test optional collection")
        .map(|_| ()),
    |ctx: &DecodeContext<'_>| ctx
        .optional_collection_vec::<u8>(true, 2, "test optional collection")
        .map(|_| ())
);
collection_case!(
    collect_indexed_vec_charges_before_allocation,
    2,
    |ctx: &DecodeContext<'_>| ctx
        .collect_indexed_vec(2, "test indexed vec", |index| u8::try_from(index)
            .map_err(CodecError::malformed))
        .map(|_| ()),
    |ctx: &DecodeContext<'_>| ctx
        .collect_indexed_vec(2, "test indexed vec", |index| u8::try_from(index)
            .map_err(CodecError::malformed))
        .map(|_| ())
);
