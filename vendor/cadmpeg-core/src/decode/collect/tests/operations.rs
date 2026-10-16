// SPDX-License-Identifier: Apache-2.0

use super::{context, operation_context};
use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use crate::CodecError;
use std::collections::{BTreeMap, BTreeSet};
operation_case!(
    reserve_retained_vec_refuses_before_growth,
    reserve_retained_vec_succeeds_under_service_profile,
    ResourceDimension::RetainedBytes,
    2,
    |ctx: &DecodeContext<'_>| {
        let mut values = Vec::<u8>::new();
        let result = ctx.reserve_vec(&mut values, 2, "test retained growth");
        if result.is_err() {
            assert_eq!(values.capacity(), 0);
        }
        result
    }
);
operation_case!(
    retained_vec_refuses_before_allocation,
    retained_vec_succeeds_under_service_profile,
    ResourceDimension::RetainedBytes,
    2,
    |ctx: &DecodeContext<'_>| ctx.collection_vec::<u8>(2, "test retained vec").map(|_| ())
);
operation_case!(
    reserve_capacity_refuses_before_growth,
    reserve_capacity_succeeds_under_service_profile,
    ResourceDimension::RetainedBytes,
    2,
    |ctx: &DecodeContext<'_>| {
        let mut values = Vec::<u8>::new();
        let result = ctx.reserve_capacity(&mut values, 2, "test retained admitted vec");
        if result.is_err() {
            assert_eq!(values.capacity(), 0);
        }
        result
    }
);
operation_case!(
    reserve_scoped_vec_refuses_before_growth,
    reserve_scoped_vec_succeeds_under_service_profile,
    ResourceDimension::MaterializedBytes,
    2,
    |ctx: &DecodeContext<'_>| {
        let mut reservation = ctx.reserve_scoped(0, "test scoped vec")?;
        let mut values = Vec::<u8>::new();
        let result = ctx.reserve_scoped_vec(&mut reservation, &mut values, 2, "test scoped vec");
        if result.is_err() {
            assert_eq!(values.capacity(), 0);
        }
        result
    }
);
operation_case!(
    reserve_temporary_vec_refuses_before_growth,
    reserve_temporary_vec_succeeds_under_service_profile,
    ResourceDimension::MaterializedBytes,
    2,
    |ctx: &DecodeContext<'_>| {
        let mut values = Vec::<u8>::new();
        let result = ctx
            .reserve_temporary_vec(&mut values, 2, "test temporary vec")
            .map(|_| ())
            .map_err(CodecError::from);
        if result.is_err() {
            assert_eq!(values.capacity(), 0);
        }
        result
    }
);
operation_case!(
    collect_retained_texts_refuses_at_byte_limit,
    collect_retained_texts_succeeds_under_service_profile,
    ResourceDimension::RetainedBytes,
    crate::decode::u64_from_index(std::mem::size_of::<String>() + 2),
    |ctx: &DecodeContext<'_>| ctx
        .collect_retained_texts(["ab"], "test retained texts")
        .map(|_| ())
);
operation_case!(
    collect_scoped_texts_refuses_at_byte_limit,
    collect_scoped_texts_succeeds_under_service_profile,
    ResourceDimension::MaterializedBytes,
    crate::decode::u64_from_index(std::mem::size_of::<String>() + 2),
    |ctx: &DecodeContext<'_>| ctx
        .collect_scoped_texts(["ab"], "test scoped texts")
        .map(|_| ())
);
operation_case!(
    format_retained_with_work_refuses_before_formatting,
    format_retained_with_work_succeeds_under_service_profile,
    ResourceDimension::WorkUnits,
    2,
    |ctx: &DecodeContext<'_>| ctx
        .format_retained(format_args!("ab"), "test formatted work")
        .map(|_| ())
);
operation_case!(
    format_scoped_text_with_work_refuses_before_formatting,
    format_scoped_text_with_work_succeeds_under_service_profile,
    ResourceDimension::WorkUnits,
    2,
    |ctx: &DecodeContext<'_>| {
        let mut reservation = ctx.reserve_scoped(0, "test scoped formatted work")?;
        ctx.format_scoped_text(
            &mut reservation,
            format_args!("ab"),
            "test scoped formatted work",
        )
        .map(|_| ())
    }
);
operation_case!(
    reserve_record_vec_refuses_before_growth,
    reserve_record_vec_succeeds_under_service_profile,
    ResourceDimension::Entities,
    2,
    |ctx: &DecodeContext<'_>| {
        let mut values = Vec::<u8>::new();
        let result = ctx.reserve_record_vec(&mut values, 2, 0, "test record vec");
        if result.is_err() {
            assert_eq!(values.capacity(), 0);
        }
        result
    }
);

#[test]
fn copy_admitted_text_preserves_utf8() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
    assert_eq!(
        ctx.copy_retained_text("aé", "test admitted text")
            .expect("admitted text"),
        "aé"
    );
}
#[test]
fn copy_temporary_slice_refuses_before_allocation_and_clone() {
    #[derive(Debug, Copy)]
    struct ObservedClone<'a>(&'a std::cell::Cell<usize>);
    // The observer detects any Clone call before allocation admission.
    #[allow(clippy::expl_impl_clone_on_copy, clippy::non_canonical_clone_impl)]
    impl Clone for ObservedClone<'_> {
        fn clone(&self) -> Self {
            self.0.set(self.0.get() + 1);
            Self(self.0)
        }
    }
    let cloned = std::cell::Cell::new(0);
    let arena = DecodeArena::new();
    let need = crate::decode::u64_from_index(std::mem::size_of::<ObservedClone<'_>>());
    let ctx = operation_context(&arena, ResourceDimension::MaterializedBytes, need - 1);
    let error = ctx
        .copy_temporary_slice(&[ObservedClone(&cloned)], "test scoped copy")
        .expect_err("copy exceeds scoped storage");
    assert_eq!(error.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(cloned.get(), 0);
}

#[test]
fn copy_temporary_slice_succeeds_under_service_profile() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("empty service root is admitted");
    let (copy, reservation) = ctx
        .copy_temporary_slice(&[1u8, 2], "test scoped copy")
        .expect("copy fits service profile");
    assert_eq!(copy, [1, 2]);
    drop(reservation);
}

operation_case!(
    collect_retained_vec_refuses_before_first_allocation,
    collect_retained_vec_succeeds_under_service_profile,
    ResourceDimension::RetainedBytes,
    2,
    |ctx: &DecodeContext<'_>| ctx
        .collect_retained_vec([1u16], "test retained collection")
        .map(|_| ())
);
operation_case!(
    copy_slice_with_work_refuses_before_copy,
    copy_slice_with_work_succeeds_under_service_profile,
    ResourceDimension::WorkUnits,
    2,
    |ctx: &DecodeContext<'_>| ctx.copy_slice(&[1u8, 2], "test work copy").map(|_| ())
);

#[test]
fn admitted_string_reserve_preserves_prefix_under_service_profile() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);

    let mut text = String::from("a");
    ctx.try_reserve_retained_text(&mut text, 2, "test admitted text slots")
        .expect("text allocation");
    assert_eq!(text, "a");
    assert!(text.capacity() >= 3);
}
#[test]
fn admitted_string_reserve_follows_one_below_limit_refusal_before_growth() {
    let arena = DecodeArena::new();
    let ctx = operation_context(&arena, ResourceDimension::RetainedBytes, 1);
    let mut text = String::new();
    let result = ctx.try_reserve_retained_text(&mut text, 2, "test admitted text slots");
    assert!(
        matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes)
    );
    assert_eq!(text.capacity(), 0);
}

#[test]
fn push_retained_vec_refuses_one_below_storage_before_allocation() {
    let arena = DecodeArena::new();
    let ctx = operation_context(&arena, ResourceDimension::RetainedBytes, 1);
    let mut values = Vec::<u16>::new();
    let error = ctx
        .push_vec(&mut values, 7, "test retained push")
        .expect_err("test operation refuses");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes)
    );
    assert_eq!(values.capacity(), 0);
    assert!(values.is_empty());
}

#[test]
fn push_retained_vec_keeps_value_under_service_profile() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
    let mut values = Vec::<u16>::new();
    ctx.push_vec(&mut values, 7, "test retained push")
        .expect("test operation succeeds");
    assert_eq!(values, [7]);
}

#[test]
fn scoped_tree_set_refuses_one_below_storage_before_insertion() {
    let arena = DecodeArena::new();
    let ctx = operation_context(&arena, ResourceDimension::MaterializedBytes, 3);
    let mut reservation = ctx
        .reserve_scoped(0, "test scoped tree")
        .expect("test operation succeeds");
    let mut values = BTreeSet::new();
    let error = ctx
        .insert_scoped_btree_set(
            &mut reservation,
            &mut values,
            7u8,
            "test scoped lookup",
            "test scoped tree",
        )
        .expect_err("test operation refuses");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes)
    );
    assert!(values.is_empty());
}

#[test]
fn scoped_tree_set_preserves_unique_values_under_service_profile() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
    let mut reservation = ctx
        .reserve_scoped(0, "test scoped tree")
        .expect("test operation succeeds");
    let mut values = BTreeSet::new();
    assert!(ctx
        .insert_scoped_btree_set(
            &mut reservation,
            &mut values,
            7u8,
            "test scoped lookup",
            "test scoped tree"
        )
        .expect("test operation succeeds"));
    assert!(!ctx
        .insert_scoped_btree_set(
            &mut reservation,
            &mut values,
            7u8,
            "test scoped lookup",
            "test scoped tree"
        )
        .expect("test operation succeeds"));
    assert_eq!(values, BTreeSet::from([7]));
}

#[test]
fn scoped_tree_map_refuses_one_below_storage_before_insertion() {
    let arena = DecodeArena::new();
    let ctx = operation_context(&arena, ResourceDimension::MaterializedBytes, 7);
    let mut reservation = ctx
        .reserve_scoped(0, "test scoped tree")
        .expect("test operation succeeds");
    let mut values = BTreeMap::new();
    let error = ctx
        .insert_scoped_btree_map_if_vacant(
            &mut reservation,
            &mut values,
            7u8,
            9u8,
            "test scoped lookup",
            "test scoped tree",
        )
        .expect_err("test operation refuses");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes)
    );
    assert!(values.is_empty());
}

#[test]
fn scoped_tree_map_preserves_first_entry_under_service_profile() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
    let mut reservation = ctx
        .reserve_scoped(0, "test scoped tree")
        .expect("test operation succeeds");
    let mut values = BTreeMap::new();
    assert!(ctx
        .insert_scoped_btree_map_if_vacant(
            &mut reservation,
            &mut values,
            7u8,
            9u8,
            "test scoped lookup",
            "test scoped tree"
        )
        .expect("test operation succeeds"));
    assert!(!ctx
        .insert_scoped_btree_map_if_vacant(
            &mut reservation,
            &mut values,
            7u8,
            11u8,
            "test scoped lookup",
            "test scoped tree"
        )
        .expect("test operation succeeds"));
    assert_eq!(values, BTreeMap::from([(7, 9)]));
}

#[test]
fn scoped_group_refuses_one_below_storage_before_allocation() {
    let arena = DecodeArena::new();
    let need = crate::decode::u64_from_index(
        std::mem::size_of::<(u8, Vec<u16>)>() + std::mem::size_of::<u16>() + 3,
    );
    let ctx = operation_context(&arena, ResourceDimension::MaterializedBytes, need - 1);
    let mut reservation = ctx
        .reserve_scoped(0, "test scoped group")
        .expect("test operation succeeds");
    let mut groups = BTreeMap::new();
    let built = std::cell::Cell::new(false);
    let error = ctx
        .push_scoped_btree_group(
            &mut reservation,
            &mut groups,
            1u8,
            || {
                built.set(true);
                7u16
            },
            3,
            "test scoped group",
        )
        .expect_err("test operation refuses");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes)
    );
    assert!(groups.is_empty());
    assert!(!built.get());
}

#[test]
fn scoped_group_preserves_member_order_under_service_profile() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
    let mut reservation = ctx
        .reserve_scoped(0, "test scoped group")
        .expect("test operation succeeds");
    let mut groups = BTreeMap::new();
    ctx.push_scoped_btree_group(
        &mut reservation,
        &mut groups,
        1u8,
        || 7u16,
        3,
        "test scoped group",
    )
    .expect("test operation succeeds");
    ctx.push_scoped_btree_group(
        &mut reservation,
        &mut groups,
        1u8,
        || 9u16,
        3,
        "test scoped group",
    )
    .expect("test operation succeeds");
    assert_eq!(groups[&1], [7, 9]);
}

#[test]
fn scoped_string_refuses_one_below_storage_before_allocation() {
    let arena = DecodeArena::new();
    let ctx = operation_context(&arena, ResourceDimension::MaterializedBytes, 1);
    let mut reservation = ctx
        .reserve_scoped(0, "test scoped string")
        .expect("test operation succeeds");
    let mut text = String::new();
    let error = ctx
        .reserve_scoped_string(&mut reservation, &mut text, 2, "test scoped string")
        .expect_err("test operation refuses");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes)
    );
    assert_eq!(text.capacity(), 0);
}

#[test]
fn scoped_string_keeps_prefix_under_service_profile() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
    let mut reservation = ctx
        .reserve_scoped(0, "test scoped string")
        .expect("test operation succeeds");
    let mut text = String::from("a");
    ctx.reserve_scoped_string(&mut reservation, &mut text, 2, "test scoped string")
        .expect("test operation succeeds");
    text.push_str("bc");
    assert_eq!(text, "abc");
}
