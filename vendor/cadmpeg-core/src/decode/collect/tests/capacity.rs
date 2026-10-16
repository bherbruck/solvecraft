// SPDX-License-Identifier: Apache-2.0

use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use crate::CodecError;
use std::collections::HashSet;

#[test]
fn reserve_vec_charges_minimum_capacity_and_added_growth_bytes() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 64;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test context");
    let mut values = Vec::<u64>::new();
    ctx.reserve_vec(&mut values, 1, "minimum vector capacity")
        .expect("four slots");
    assert_eq!(values.capacity(), 4);
    ctx.charge_retained(0, "check minimum charge")
        .expect("32 bytes remain");
    values.extend([1, 2, 3, 4]);
    ctx.reserve_vec(&mut values, 1, "grown vector capacity")
        .expect("eight slots");
    assert_eq!(values.capacity(), 8);
    let error = ctx
        .charge_retained(1, "check exact growth charge")
        .expect_err("64 bytes used");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes && limit.used == 64 && limit.additional == 1));
}

#[test]
fn reserve_vec_refuses_retained_limit_before_allocation_and_fuses() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 31;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test context");
    let mut values = Vec::<u64>::new();
    let error = ctx
        .reserve_vec(&mut values, 1, "minimum vector capacity")
        .expect_err("four slots need 32 bytes");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes && limit.used == 0 && limit.additional == 32
                && ctx.resource_refusal() == Some(limit)));
    assert_eq!(values.capacity(), 0);
    assert!(values.is_empty());
}

#[test]
fn retained_capacity_reservation_and_push_charge_storage_and_item_once() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        4 * u64::try_from(std::mem::size_of::<u64>()).expect("admitted test operation");
    policy.limits.max_collection_items = 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test operation");
    let mut values = Vec::new();
    ctx.reserve_capacity_limit(&mut values, 1, "reserve retained capacity")
        .expect("admitted test operation");
    assert_eq!(values.capacity(), 4);
    ctx.push_vec(&mut values, 7u64, "admit retained item")
        .expect("admitted test operation");
    assert_eq!(values, [7]);
    let error = ctx
        .push_vec(&mut values, 8u64, "refuse second item")
        .expect_err("test operation refuses");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.used == 1 && limit.additional == 1)
    );
    assert_eq!(values, [7]);
}

#[test]
fn flat_copies_do_not_charge_storage_for_empty_or_zero_sized_values() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test operation");
    assert_eq!(
        ctx.copy_slice(&[(); 3], "zero sized copy")
            .expect("admitted test operation"),
        vec![(); 3]
    );
    assert!(ctx
        .copy_retained_set(&HashSet::<u64>::new(), "empty set copy")
        .expect("admitted test operation")
        .is_empty());
    assert!(ctx.finish_session().is_ok());
}

#[test]
fn retained_vector_size_overflow_fuses_in_the_storage_dimension() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test operation");
    let mut values = Vec::<u64>::new();
    let error = ctx
        .reserve_vec(&mut values, usize::MAX, "oversized retained vector")
        .expect_err("test operation refuses");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes)
    );
    assert!(values.is_empty());
    assert!(ctx.finish_session().is_err());
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test operation");
    let error = ctx
        .with_scoped_storage("oversized scoped vector", || {
            ctx.reserve_vec(&mut values, usize::MAX, "oversized scoped vector")
        })
        .expect_err("test operation refuses");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes)
    );
    assert!(ctx.finish_session().is_err());
}
