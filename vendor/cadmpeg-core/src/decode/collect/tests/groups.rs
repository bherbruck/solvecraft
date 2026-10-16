// SPDX-License-Identifier: Apache-2.0

use super::{context, operation_context};
use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use crate::CodecError;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
fn manual_group_with_limit(
    configure: impl FnOnce(&mut crate::decode::DecodePolicy),
) -> Result<(), crate::CodecError> {
    let arena = crate::decode::DecodeArena::new();
    let mut policy = crate::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = crate::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test operation succeeds");
    let mut reservation = ctx.reserve_scoped(0, "NX feature operation group indexes")?;
    let mut grouped = std::collections::BTreeMap::new();
    ctx.push_scoped_btree_group(
        &mut reservation,
        &mut grouped,
        "operation",
        || 1u32,
        0,
        "NX feature operation group index",
    )?;
    ctx.push_scoped_btree_group(
        &mut reservation,
        &mut grouped,
        "operation",
        || 2u32,
        0,
        "NX feature operation group index",
    )?;
    assert_eq!(grouped["operation"], [1, 2]);
    Ok(())
}

#[test]
fn manual_operation_group_refuses_collection_limit() {
    let error = manual_group_with_limit(|policy| policy.limits.max_collection_items = 0)
        .expect_err("test operation refuses");
    assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
        if limit.dimension == crate::decode::ResourceDimension::CollectionItems));
}

#[test]
fn manual_operation_group_refuses_scoped_limit() {
    let error = manual_group_with_limit(|policy| policy.limits.max_materialized_bytes = 0)
        .expect_err("test operation refuses");
    assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
        if limit.dimension == crate::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn manual_operation_group_refuses_work_limit() {
    let error = manual_group_with_limit(|policy| policy.limits.max_work_units = 0)
        .expect_err("test operation refuses");
    assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
        if limit.dimension == crate::decode::ResourceDimension::WorkUnits));
}

#[test]
fn jt_rendered_node_path_refuses_retained_limit() {
    let arena = crate::decode::DecodeArena::new();
    let mut policy = crate::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 4;
    let (ctx, _) = crate::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    let error = ctx
        .join_display_retained([12, 34].iter(), "-", "nx JT rendered node path")
        .expect_err("five text bytes exceed the four-byte retained limit");
    assert!(matches!(
        error,
        crate::CodecError::ResourceLimit(limit)
            if limit.dimension == crate::decode::ResourceDimension::RetainedBytes
                && limit.operation == "nx JT rendered node path"
    ));
    {
        let arena = crate::decode::DecodeArena::new();
        let (service, _) = crate::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &crate::decode::DecodePolicy::service(),
        )
        .expect("test decode context");
        assert_eq!(
            service
                .join_display_retained([12, 34].iter(), "-", "nx JT rendered node path")
                .expect("test operation succeeds"),
            "12-34"
        );
    }
}

#[test]
fn jt_tessellation_channel_bytes_refuse_collection_limit() {
    let arena = crate::decode::DecodeArena::new();
    let mut policy = crate::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (ctx, _) = crate::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    let mut bytes = Vec::<u8>::new();
    let error = ctx
        .reserve_vec(&mut bytes, 3, "nx JT tessellation colors")
        .expect_err("three color bytes exceed two collection items");
    assert!(matches!(
        error,
        crate::CodecError::ResourceLimit(limit)
            if limit.dimension == crate::decode::ResourceDimension::CollectionItems
                && limit.operation == "nx JT tessellation colors"
    ));
    assert!(bytes.is_empty());
}

#[test]
fn retained_vector_slots_do_not_charge_entities() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root fits policy");
    let mut values = Vec::<u8>::new();
    ctx.reserve_vec(&mut values, 1, "test retained slots")
        .expect("vector slot is not an entity");
    assert!(values.is_empty());
}

#[test]
fn collect_scoped_btree_groups_refuses_before_allocating_first_group() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = crate::decode::u64_from_index(
        std::mem::size_of::<(u8, Vec<u8>)>() + std::mem::size_of::<u8>(),
    ) - 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test operation succeeds");
    let mut yielded = 0;
    let values = [(1u8, 2u8), (1, 3)].into_iter().inspect(|_| yielded += 1);
    assert!(
        matches!(ctx.collect_scoped_btree_groups(values, "test scoped groups"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::MaterializedBytes)
    );
    assert_eq!(yielded, 1);
}

#[test]
fn collect_scoped_btree_groups_succeeds_under_service_profile() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test operation succeeds");
    let (groups, _reservation) = ctx
        .collect_scoped_btree_groups([(1u8, 2u8), (1, 3)], "test scoped groups")
        .expect("test operation succeeds");
    assert_eq!(groups[&1], [2, 3]);
}

#[test]
fn collect_scoped_btree_map_refuses_before_allocating_first_entry() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes =
        crate::decode::u64_from_index(std::mem::size_of::<(u8, u8)>()) - 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test operation succeeds");
    let mut yielded = 0;
    let values = [(1u8, 2u8), (1, 3)].into_iter().inspect(|_| yielded += 1);
    assert!(
        matches!(ctx.collect_scoped_btree_map(values, "test scoped map"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::MaterializedBytes)
    );
    assert_eq!(yielded, 1);
}

#[test]
fn collect_scoped_btree_map_succeeds_under_service_profile() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test operation succeeds");
    let (entries, _reservation) = ctx
        .collect_scoped_btree_map([(1u8, 2u8), (1, 3)], "test scoped map")
        .expect("test operation succeeds");
    assert_eq!(entries[&1], 3);
}

fn last_record_with_limit(
    configure: impl FnOnce(&mut crate::decode::DecodePolicy),
) -> Result<(), crate::CodecError> {
    let records = [("operation", 1u32), ("operation", 2u32)];
    let arena = crate::decode::DecodeArena::new();
    let mut policy = crate::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = crate::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test operation succeeds");
    let (index, _reservation) = ctx.collect_scoped_btree_map(records, "NX last-record index")?;
    assert_eq!(index["operation"], 2);
    Ok(())
}

#[test]
fn last_record_index_refuses_collection_limit() {
    let error = last_record_with_limit(|policy| policy.limits.max_collection_items = 0)
        .expect_err("test operation refuses");
    assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
        if limit.dimension == crate::decode::ResourceDimension::CollectionItems));
}

#[test]
fn last_record_index_refuses_scoped_limit() {
    let error = last_record_with_limit(|policy| policy.limits.max_materialized_bytes = 0)
        .expect_err("test operation refuses");
    assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
        if limit.dimension == crate::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn last_record_index_refuses_work_limit() {
    let error = last_record_with_limit(|policy| policy.limits.max_work_units = 0)
        .expect_err("test operation refuses");
    assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
        if limit.dimension == crate::decode::ResourceDimension::WorkUnits));
}

fn grouped_records_with_limit(
    configure: impl FnOnce(&mut crate::decode::DecodePolicy),
) -> Result<(), crate::CodecError> {
    let records = [("first".to_owned(), 1), ("first".to_owned(), 2)];
    let arena = crate::decode::DecodeArena::new();
    let mut policy = crate::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = crate::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test operation succeeds");
    let (grouped, _reservation) = ctx.collect_scoped_btree_groups(
        records.iter().map(|record| (record.0.as_str(), record)),
        "NX operation record index",
    )?;
    assert_eq!(
        grouped["first"]
            .iter()
            .map(|record| record.1)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    Ok(())
}

#[test]
fn operation_record_index_refuses_collection_limit() {
    let error = grouped_records_with_limit(|policy| policy.limits.max_collection_items = 0)
        .expect_err("test operation refuses");
    assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
        if limit.dimension == crate::decode::ResourceDimension::CollectionItems));
}

#[test]
fn operation_record_index_refuses_scoped_limit() {
    let error = grouped_records_with_limit(|policy| policy.limits.max_materialized_bytes = 0)
        .expect_err("test operation refuses");
    assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
        if limit.dimension == crate::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn operation_record_index_refuses_work_limit() {
    let error = grouped_records_with_limit(|policy| policy.limits.max_work_units = 0)
        .expect_err("test operation refuses");
    assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
        if limit.dimension == crate::decode::ResourceDimension::WorkUnits));
}

#[test]
fn admit_retained_btree_record_refuses_one_below_need_before_allocation() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One node has eleven key/value lanes, sixteen pointer widths and two alignment widths.
    let node_bytes = 11 * (std::mem::size_of::<String>() + std::mem::size_of::<u16>())
        + 16 * std::mem::size_of::<usize>()
        + 2 * std::mem::align_of::<String>();
    policy.limits.max_retained_bytes = crate::decode::u64_from_index(node_bytes + 3) - 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test operation succeeds");
    assert!(
        matches!(ctx.admit_retained_btree_record::<String, u16>(3, "test retained tree record"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes)
    );
}

#[test]
fn admit_retained_btree_record_succeeds_under_service_profile() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test operation succeeds");
    ctx.admit_retained_btree_record::<String, u16>(3, "test retained tree record")
        .expect("test operation succeeds");
}

fn attribute_lookup_with_limit(
    configure: impl FnOnce(&mut crate::decode::DecodePolicy),
) -> Result<(), crate::CodecError> {
    let records = [("first", 1_u8), ("second", 2_u8)];
    let arena = crate::decode::DecodeArena::new();
    let mut policy = crate::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = crate::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)?;

    let (indexed, _index_reservation) = ctx.collect_scoped_btree_map(
        records.iter().map(|record| (record.0, record)),
        "NX Parasolid attribute record index",
    )?;
    let (grouped, _group_reservation) = ctx.collect_scoped_btree_groups(
        records.iter().map(|record| (record.0, record)),
        "NX Parasolid attribute use groups",
    )?;
    assert_eq!(indexed.len(), 2);
    assert_eq!(grouped.len(), 2);
    Ok(())
}

#[test]
fn attribute_lookup_refuses_collection_limit() {
    let error = attribute_lookup_with_limit(|policy| policy.limits.max_collection_items = 0)
        .expect_err("test operation refuses");
    assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
        if limit.dimension == crate::decode::ResourceDimension::CollectionItems));
}

#[test]
fn attribute_lookup_refuses_scoped_limit() {
    let error = attribute_lookup_with_limit(|policy| policy.limits.max_materialized_bytes = 0)
        .expect_err("test operation refuses");
    assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
        if limit.dimension == crate::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn attribute_lookup_refuses_work_limit() {
    let error = attribute_lookup_with_limit(|policy| policy.limits.max_work_units = 0)
        .expect_err("test operation refuses");
    assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
        if limit.dimension == crate::decode::ResourceDimension::WorkUnits));
}
#[test]
fn append_formatted_retained_refuses_one_below_need_before_allocation() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let mut output = String::new();
    let error = ctx
        .append_formatted_retained(
            &mut output,
            format_args!("{}", 123),
            "test formatted append",
        )
        .expect_err("three bytes exceed two");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes));
    assert!(output.is_empty());
    assert_eq!(output.capacity(), 0);
}

#[test]
fn append_formatted_retained_succeeds_under_service_profile() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    let mut output = String::from("prefix:");
    ctx.append_formatted_retained(
        &mut output,
        format_args!("{}", 123),
        "test formatted append",
    )
    .expect("service profile admits text");
    assert_eq!(output, "prefix:123");
}

#[test]
fn push_scoped_vec_refuses_one_below_need_before_allocation() {
    let arena = DecodeArena::new();
    let ctx = operation_context(&arena, ResourceDimension::MaterializedBytes, 1);
    let mut values = Vec::<u16>::new();
    let mut storage = ctx
        .reserve_scoped(0, "test scoped push")
        .expect("test reservation");
    let error = ctx
        .push_scoped_vec(&mut storage, &mut values, 7, "test scoped push")
        .expect_err("one below required storage");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes)
    );
    assert!(values.is_empty());
    assert_eq!(values.capacity(), 0);
}

#[test]
fn push_scoped_vec_succeeds_under_service_profile() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    let mut values = Vec::<u16>::new();
    let mut storage = ctx
        .reserve_scoped(0, "test scoped push")
        .expect("test reservation");
    ctx.push_scoped_vec(&mut storage, &mut values, 7, "test scoped push")
        .expect("service admission");
    assert_eq!(values, [7]);
}

#[test]
fn collect_btree_set_refuses_one_below_need_before_allocation() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);

    let error = ctx
        .collect_btree_set([7u16], "test ordered set")
        .expect_err("one below required storage");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems)
    );
}

#[test]
fn collect_btree_set_succeeds_under_service_profile() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");

    let result = ctx
        .collect_btree_set([7u16, 9, 7], "test ordered set")
        .expect("service admission");
    assert_eq!(result, BTreeSet::from([7, 9]));
}

#[test]
fn push_btree_group_refuses_one_below_need_before_allocation() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 1);
    let mut values = BTreeMap::<u8, Vec<u16>>::new();
    let error = ctx
        .push_btree_group(&mut values, 1, 7, "test group", "test member")
        .expect_err("one below required storage");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems)
    );
    assert!(values.is_empty());
}

#[test]
fn push_btree_group_succeeds_under_service_profile() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    let mut values = BTreeMap::<u8, Vec<u16>>::new();
    ctx.push_btree_group(&mut values, 1, 7, "test group", "test member")
        .expect("service admission");
    assert_eq!(values[&1], [7]);
    ctx.push_btree_group(&mut values, 1, 9, "test group", "test member")
        .expect("second member");
    assert_eq!(values[&1], [7, 9]);
}

#[test]
fn insert_btree_group_set_refuses_one_below_need_before_allocation() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 1);
    let mut values = BTreeMap::<u8, BTreeSet<u16>>::new();
    let error = ctx
        .insert_btree_group_set(&mut values, 1, 7, "test group", "test member")
        .expect_err("one below required storage");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems)
    );
    assert!(values.is_empty());
}

#[test]
fn insert_btree_group_set_succeeds_under_service_profile() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    let mut values = BTreeMap::<u8, BTreeSet<u16>>::new();
    ctx.insert_btree_group_set(&mut values, 1, 7, "test group", "test member")
        .expect("service admission");
    assert_eq!(values[&1], BTreeSet::from([7]));
    ctx.insert_btree_group_set(&mut values, 1, 7, "test group", "test member")
        .expect("duplicate member");
    assert_eq!(values[&1], BTreeSet::from([7]));
}

#[test]
fn vector_storage_refuses_one_below_need_before_allocation() {
    let arena = DecodeArena::new();
    let ctx = operation_context(&arena, ResourceDimension::RetainedBytes, 3);
    assert!(
        matches!(ctx.vector_storage::<u16>(2, "test retained admitted storage"),
            Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes)
    );
}

#[test]
fn vector_storage_succeeds_under_service_profile() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    let mut values = ctx
        .vector_storage(2, "test retained admitted storage")
        .expect("service admission");
    values.extend([7u16, 9]);
    assert_eq!(values, [7, 9]);
}

#[test]
fn scoped_vector_storage_refuses_one_below_need_before_allocation() {
    let arena = DecodeArena::new();
    let ctx = operation_context(&arena, ResourceDimension::MaterializedBytes, 3);
    assert!(
        matches!(ctx.scoped_vector_storage::<u16>(2, "test scoped admitted storage"),
            Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn scoped_vector_storage_succeeds_under_service_profile() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    let (mut values, _reservation) = ctx
        .scoped_vector_storage(2, "test scoped admitted storage")
        .expect("service admission");
    values.extend([7u16, 9]);
    assert_eq!(values, [7, 9]);
}

#[test]
fn charge_formatted_retained_refuses_one_below_need_before_allocation() {
    let arena = DecodeArena::new();
    let ctx = operation_context(&arena, ResourceDimension::RetainedBytes, 2);
    assert!(
        matches!(ctx.charge_formatted_retained(format_args!("a{}", 12), "test formatted admission"),
            Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes && limit.additional == 3)
    );
}

#[test]
fn charge_formatted_retained_succeeds_under_service_profile() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    ctx.charge_formatted_retained(format_args!("a{}", 12), "test formatted admission")
        .expect("service admission");
}

#[test]
fn insert_retained_hash_set_refuses_one_below_need_before_allocation() {
    let arena = DecodeArena::new();
    let ctx = operation_context(&arena, ResourceDimension::RetainedBytes, 1);
    let mut values = HashSet::new();
    assert!(
        matches!(ctx.insert_hash_set(&mut values, 7u16, "test retained set"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes)
    );
    assert!(values.is_empty());
    assert_eq!(values.capacity(), 0);
}

#[test]
fn insert_retained_hash_set_succeeds_under_service_profile() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    let mut values = HashSet::new();
    assert!(ctx
        .insert_hash_set(&mut values, 7u16, "test retained set")
        .expect("service admission"));
    assert!(!ctx
        .insert_hash_set(&mut values, 7u16, "test retained set")
        .expect("duplicate"));
    assert_eq!(values, HashSet::from([7]));
}

#[test]
fn insert_scoped_btree_value_refuses_one_below_need_before_allocation() {
    let arena = DecodeArena::new();
    let ctx = operation_context(&arena, ResourceDimension::MaterializedBytes, 1);
    let mut reservation = ctx
        .reserve_scoped(0, "test scoped value")
        .expect("empty reserve");
    let mut values = BTreeSet::new();
    assert!(
        matches!(ctx.insert_scoped_btree_value(&mut reservation, &mut values, 7u16, "test scoped value"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::MaterializedBytes)
    );
    assert!(values.is_empty());
}

#[test]
fn insert_scoped_btree_value_succeeds_under_service_profile() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    let mut reservation = ctx
        .reserve_scoped(0, "test scoped value")
        .expect("empty reserve");
    let mut values = BTreeSet::new();
    assert!(ctx
        .insert_scoped_btree_value(&mut reservation, &mut values, 7u16, "test scoped value")
        .expect("service admission"));
    assert!(!ctx
        .insert_scoped_btree_value(&mut reservation, &mut values, 7u16, "test scoped value")
        .expect("duplicate"));
    assert_eq!(values, BTreeSet::from([7]));
}

#[test]
fn extend_hash_set_refuses_one_below_need_before_allocation() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let mut values = HashSet::new();
    assert!(
        matches!(ctx.extend_hash_set(&mut values, [7u16], "test extend set"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems)
    );
    assert!(values.is_empty());
    assert_eq!(values.capacity(), 0);
}

#[test]
fn extend_hash_set_succeeds_under_service_profile() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    let mut values = HashSet::from([7u16]);
    ctx.extend_hash_set(&mut values, [7, 9, 9], "test extend set")
        .expect("service admission");
    assert_eq!(values, HashSet::from([7, 9]));
}

#[test]
fn reserve_scoped_collection_refuses_one_below_need_before_allocation() {
    let arena = DecodeArena::new();
    let ctx = operation_context(&arena, ResourceDimension::MaterializedBytes, 3);
    assert!(
        matches!(ctx.reserve_scoped_collection::<u16>(2, "test scoped collection"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::MaterializedBytes && limit.additional == 4)
    );
}

#[test]
fn reserve_scoped_collection_succeeds_under_service_profile() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    let reservation = ctx
        .reserve_scoped_collection::<u16>(2, "test scoped collection")
        .expect("service admission");
    drop(reservation);
}
#[test]
fn push_hash_group_refuses_one_below_need_before_allocation() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 1);
    let mut groups = HashMap::<u8, Vec<u8>>::new();
    let error = ctx
        .push_hash_group(&mut groups, 1, 7, "group index", "group value")
        .expect_err("two slots exceed one");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "group value"));
    assert!(groups.is_empty());
    assert_eq!(groups.capacity(), 0);
}

#[test]
fn push_hash_group_succeeds_under_service_profile() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
    let mut groups = HashMap::<u8, Vec<u8>>::new();
    ctx.push_hash_group(&mut groups, 1, 7, "group index", "group value")
        .expect("first value");
    ctx.push_hash_group(&mut groups, 1, 8, "group index", "group value")
        .expect("same group");
    assert_eq!(groups.get(&1), Some(&vec![7, 8]));
}
#[test]
fn resize_retained_bytes_refuses_one_below_need_before_allocation() {
    let arena = DecodeArena::new();
    let ctx = operation_context(&arena, ResourceDimension::RetainedBytes, 1);
    let mut values = Vec::new();
    let error = ctx
        .resize_retained_bytes(&mut values, 2, 7, "test retained resize")
        .expect_err("one below required storage");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes && limit.additional == 8));
    assert!(values.is_empty());
    assert_eq!(values.capacity(), 0);
}

#[test]
fn resize_retained_bytes_succeeds_under_service_profile() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    let mut values = Vec::new();
    ctx.resize_retained_bytes(&mut values, 2, 7, "test retained resize")
        .expect("service admission");
    assert_eq!(values, [7, 7]);
    ctx.resize_retained_bytes(&mut values, 4, 9, "test retained resize")
        .expect("growth");
    assert_eq!(values, [7, 7, 9, 9]);
    ctx.resize_retained_bytes(&mut values, 1, 0, "test retained resize")
        .expect("truncate");
    assert_eq!(values, [7]);
}
