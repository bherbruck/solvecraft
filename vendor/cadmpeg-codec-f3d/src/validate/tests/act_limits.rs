// SPDX-License-Identifier: Apache-2.0

const GUID: &str = "01234567-89ab-cdef-0123-456789abcdef";

fn act_entity() -> crate::records::act::ActEntity {
    use crate::records::act::{ActChannelGroup, ActEntity};
    use crate::records::identity::Located;
    let mut channels = std::collections::BTreeMap::new();
    channels.insert(
        "Appearance".into(),
        Located {
            value: GUID.to_owned().try_into().unwrap(),
            offset: 24,
        },
    );
    let group = ActChannelGroup::try_new(
        10,
        Some(100),
        "261".to_owned().try_into().unwrap(),
        channels,
        None,
    )
    .unwrap();
    ActEntity::try_new(
        "f3d:native:act-entity#1".into(),
        1,
        "0_3".into(),
        None,
        group,
    )
    .unwrap()
}

fn act_guid(ordinal: u32) -> crate::records::act::ActGuid {
    crate::records::act::ActGuid::new("f3d:native:act-guid#20".into(), 20, ordinal, GUID.into())
        .unwrap()
}

fn act_table_reference(ordinal: u32) -> crate::records::act::ActTableReference {
    crate::records::act::ActTableReference::new(
        "f3d:native:act-table-reference#30".into(),
        ordinal,
        30,
        7,
    )
    .unwrap()
}

fn act_registry_channel(ordinal: u32) -> crate::records::act::ActRegistryChannel {
    crate::records::act::ActRegistryChannel::new(
        "f3d:native:act-registry-channel#40".into(),
        ordinal,
        40,
        "Appearance".into(),
        GUID.into(),
    )
    .unwrap()
}

fn act_root() -> crate::records::act::ActRootComponent {
    use crate::records::act::{ActRegistryFlag, ActRootComponent, ActRootLayout};
    ActRootComponent::try_new(
        "f3d:native:act-root-component#50".into(),
        1,
        "261".to_owned().try_into().unwrap(),
        2,
        4,
        ActRegistryFlag::Off,
        ActRootLayout::new(50, "0_3".into(), "Root".into(), 1).unwrap(),
    )
    .unwrap()
}

fn act_error(
    native: crate::native::F3dNative,
    max_items: u64,
    max_retained: u64,
) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        policy.limits.max_retained_bytes = max_retained;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_act(&ctx, &mut Vec::new()).unwrap_err()
    })
}

#[test]
fn act_stream_index_refuses_collection_limit() {
    let mut native = crate::native::F3dNative::default();
    native.act_entities.push(act_entity());
    let error = act_error(native, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D ACT streams")
    );
}

#[test]
fn act_record_index_refuses_collection_limit() {
    let mut native = crate::native::F3dNative::default();
    native.act_entities.push(act_entity());
    let error = act_error(native, 1, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D ACT record indices")
    );
}

#[test]
fn act_guid_stream_index_refuses_collection_limit() {
    let mut native = crate::native::F3dNative::default();
    native.act_guids.push(act_guid(0));
    let error = act_error(native, 1, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D ACT GUID streams")
    );
}

#[test]
fn act_guid_ordinal_index_refuses_collection_limit() {
    let mut native = crate::native::F3dNative::default();
    native.act_guids.push(act_guid(0));
    let error = act_error(native, 2, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D ACT GUID ordinals")
    );
}

#[test]
fn act_guid_offset_index_refuses_collection_limit() {
    let mut native = crate::native::F3dNative::default();
    native.act_guids.push(act_guid(0));
    let error = act_error(native, 3, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D ACT GUID offsets")
    );
}

#[test]
fn act_table_stream_index_refuses_collection_limit() {
    let mut native = crate::native::F3dNative::default();
    native.act_table_references.push(act_table_reference(0));
    let error = act_error(native, 1, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D ACT table streams")
    );
}

#[test]
fn act_table_ordinal_index_refuses_collection_limit() {
    let mut native = crate::native::F3dNative::default();
    native.act_table_references.push(act_table_reference(0));
    let error = act_error(native, 2, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D ACT table ordinals")
    );
}

#[test]
fn act_table_offset_index_refuses_collection_limit() {
    let mut native = crate::native::F3dNative::default();
    native.act_table_references.push(act_table_reference(0));
    let error = act_error(native, 3, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D ACT table offsets")
    );
}

#[test]
fn act_registry_stream_index_refuses_collection_limit() {
    let mut native = crate::native::F3dNative::default();
    native.act_registry_channels.push(act_registry_channel(0));
    let error = act_error(native, 1, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D ACT registry streams")
    );
}

#[test]
fn act_registry_ordinal_index_refuses_collection_limit() {
    let mut native = crate::native::F3dNative::default();
    native.act_registry_channels.push(act_registry_channel(0));
    let error = act_error(native, 2, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D ACT registry ordinals")
    );
}

#[test]
fn act_registry_offset_index_refuses_collection_limit() {
    let mut native = crate::native::F3dNative::default();
    native.act_registry_channels.push(act_registry_channel(0));
    let error = act_error(native, 3, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D ACT registry offsets")
    );
}

#[test]
fn act_registry_name_index_refuses_collection_limit() {
    let mut native = crate::native::F3dNative::default();
    native.act_registry_channels.push(act_registry_channel(0));
    let error = act_error(native, 4, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D ACT registry names")
    );
}

#[test]
fn act_root_count_index_refuses_collection_limit() {
    let mut native = crate::native::F3dNative::default();
    native.act_root_components.push(act_root());
    let error = act_error(native, 1, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D ACT root counts")
    );
}

#[test]
fn act_missing_root_finding_refuses_collection_limit() {
    let mut native = crate::native::F3dNative::default();
    native.act_guids.push(act_guid(0));
    let error = act_error(native, 4, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn act_missing_root_witness_refuses_retained_limit() {
    let mut native = crate::native::F3dNative::default();
    native.act_guids.push(act_guid(0));
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D validation entity",
        |cap| Err::<(), cadmpeg_core::CodecError>(act_error(native.clone(), u64::MAX, cap)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

#[test]
fn act_noncontiguous_guid_ordinal_finding_refuses_collection_limit() {
    let mut native = crate::native::F3dNative::default();
    native.act_guids.push(act_guid(1));
    native.act_root_components.push(act_root());
    let error = act_error(native, 6, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}
