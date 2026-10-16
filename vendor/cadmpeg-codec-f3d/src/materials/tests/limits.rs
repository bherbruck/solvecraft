// SPDX-License-Identifier: Apache-2.0
//! Schema appearance resource-limit tests.

use super::{appearance_connected_to, appearance_record, texture_record};

fn material_context_with_limits<T>(
    max_items: u64,
    max_retained: u64,
    run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    policy.limits.max_retained_bytes = max_retained;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    run(&ctx)
}

#[test]
fn material_utf16_string_refuses_retained_limit() {
    let mut bytes = Vec::new();
    super::lp_utf16(&mut bytes, "Alpha");
    let error = material_context_with_limits(u64::MAX, 4, |ctx| {
        super::super::lp_utf16_strings(ctx, &bytes).unwrap_err()
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D UTF-16 string")
    );
}

#[test]
fn material_utf16_string_index_refuses_collection_limit() {
    let mut bytes = Vec::new();
    super::lp_utf16(&mut bytes, "Alpha");
    let error = material_context_with_limits(0, u64::MAX, |ctx| {
        super::super::lp_utf16_strings(ctx, &bytes).unwrap_err()
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D UTF-16 strings")
    );
}

#[test]
fn material_printable_ascii_refuses_retained_limit() {
    let mut bytes = Vec::new();
    super::lp_ascii(&mut bytes, "Body");
    let error = material_context_with_limits(u64::MAX, 3, |ctx| {
        super::super::lp_ascii_printable_charged(ctx, &bytes, 0).unwrap_err()
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D printable ASCII string")
    );
}

fn definition_catalog_merge_error(max_items: u64, max_retained: u64) -> cadmpeg_core::CodecError {
    material_context_with_limits(max_items, max_retained, |ctx| {
        let mut definitions = std::collections::HashMap::new();
        super::super::merge_definition_catalog_record(
            ctx,
            &mut definitions,
            super::super::DefinitionCatalog {
                schema: "Schema".into(),
                asset_id: "Asset".into(),
                category: None,
            },
        )
        .unwrap_err()
    })
}

#[test]
fn material_definition_asset_key_refuses_retained_limit() {
    let error = definition_catalog_merge_error(u64::MAX, 4);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D definition asset ID")
    );
}

#[test]
fn material_definition_schema_key_refuses_retained_limit() {
    let error = definition_catalog_merge_error(u64::MAX, 10);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D definition schema")
    );
}

#[test]
fn material_definition_index_refuses_collection_limit() {
    let error = definition_catalog_merge_error(0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D definition catalog")
    );
}

#[test]
fn material_body_id_copy_refuses_retained_limit() {
    let id = cadmpeg_ir::ids::BodyId::mint("f3d:design:body#one").unwrap();
    let error = material_context_with_limits(u64::MAX, 0, |ctx| {
        id.try_clone_for_decode(ctx, "copy F3D material body ID")
            .unwrap_err()
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D material body ID")
    );
}

#[test]
fn material_appearance_id_copy_refuses_retained_limit() {
    let id = cadmpeg_ir::ids::AppearanceId::mint("f3d:appearance:asset#one").unwrap();
    let error = material_context_with_limits(u64::MAX, 0, |ctx| {
        id.try_clone_for_decode(ctx, "copy F3D material appearance ID")
            .unwrap_err()
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D material appearance ID")
    );
}

fn named_channel_error(max_items: u64, max_retained: u64) -> cadmpeg_core::CodecError {
    let channels = std::collections::BTreeMap::from([("name".to_owned(), "guid".to_owned())]);
    material_context_with_limits(max_items, max_retained, |ctx| {
        super::super::named_act_channels(ctx, format_args!("owner"), Some(&channels)).unwrap_err()
    })
}

#[test]
fn material_act_channel_name_refuses_retained_limit() {
    let error = named_channel_error(u64::MAX, 3);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D ACT channel name")
    );
}

#[test]
fn material_act_channel_guid_refuses_retained_limit() {
    let error = named_channel_error(u64::MAX, 7);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D ACT channel GUID")
    );
}

#[test]
fn material_act_channel_copy_refuses_collection_limit() {
    let error = named_channel_error(0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D ACT channel map")
    );
}

#[test]
fn material_named_act_channel_refuses_collection_limit() {
    let error = named_channel_error(1, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "named entry map nodes")
    );
}

#[test]
fn material_fixed_appearance_vector_refuses_collection_limit() {
    let error = material_context_with_limits(0, u64::MAX, |ctx| {
        let mut appearances = Vec::new();
        ctx.push_vec(&mut appearances, (), "collect F3D fixed appearances")
            .unwrap_err()
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D fixed appearances")
    );
}

fn schema_appearance_error(
    records: &[cadmpeg_protein::DecodedRecord],
    max_items: u64,
    max_retained: u64,
) -> cadmpeg_core::CodecError {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    policy.limits.max_retained_bytes = max_retained;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    super::super::appearances_from_schema_records(&ctx, records).unwrap_err()
}

#[test]
fn schema_appearance_property_map_refuses_collection_limit() {
    let property = cadmpeg_protein::property::DecodedProperty {
        value_offset: 0,
        content: cadmpeg_protein::property::PropertyContent::Value {
            value: cadmpeg_protein::property::PropertyValue::Float(
                cadmpeg_ir::scalar::FiniteReal::new(0.5).expect("finite"),
            ),
            connections: Vec::new(),
        },
    };
    let record = appearance_record(
        "GenericSchema",
        std::collections::BTreeMap::from([("generic_reflectivity".into(), property)]),
    );
    let error = schema_appearance_error(&[record], 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D appearance properties")
    );
}

#[test]
fn schema_appearance_property_name_refuses_retained_limit() {
    let property = cadmpeg_protein::property::DecodedProperty {
        value_offset: 0,
        content: cadmpeg_protein::property::PropertyContent::Value {
            value: cadmpeg_protein::property::PropertyValue::Float(
                cadmpeg_ir::scalar::FiniteReal::new(0.5).expect("finite"),
            ),
            connections: Vec::new(),
        },
    };
    let record = appearance_record(
        "GenericSchema",
        std::collections::BTreeMap::from([("generic_reflectivity".into(), property)]),
    );
    let error = schema_appearance_error(&[record], u64::MAX, 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D appearance property name")
    );
}

#[test]
fn schema_appearance_vector_refuses_collection_limit() {
    let record = appearance_record("GenericSchema", Default::default());
    let error = schema_appearance_error(&[record], 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D appearances")
    );
}

#[test]
fn schema_texture_index_refuses_collection_limit() {
    let record = texture_record("aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb", "textures/a.png");
    let error = schema_appearance_error(&[record], 1, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D texture assets")
    );
}

#[test]
fn schema_texture_key_refuses_retained_limit() {
    let guid = "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb";
    let path = "textures/a.png";
    let record = texture_record(guid, path);

    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy F3D texture asset key",
        |cap| {
            Err::<(), cadmpeg_core::CodecError>(schema_appearance_error(
                std::slice::from_ref(&record),
                u64::MAX,
                cap,
            ))
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D texture asset key")
    );
}

#[test]
fn material_library_id_refuses_retained_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    let error = super::super::library_id(&ctx, "L").unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D appearance library ID")
    );
}

#[test]
fn browser_node_candidate_fold_refuses_materialized_limit() {
    let marker = "C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C";
    let strings = vec![
        (0, marker.to_owned()),
        (1, "node-a".to_owned()),
        (2, "visual".to_owned()),
    ];
    let nodes = std::collections::HashMap::from([("node-a".to_owned(), 7)]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    let error = super::super::body_node_candidate(&ctx, &strings, 2, &nodes).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "fold F3D browser node candidate")
    );
}

fn fixed_appearance_error(asset_lib_id: &str, max_retained: u64) -> cadmpeg_core::CodecError {
    let mut record = super::super::RECORD_MARKER.to_vec();
    for value in ["PhysMatSchema", "g", "b", asset_lib_id] {
        super::super::push_lp(&mut record, value).unwrap();
    }
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = max_retained;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    super::super::decode_fixed_record(&ctx, &record).unwrap_err()
}

#[test]
fn fixed_appearance_guid_copy_refuses_retained_limit() {
    let error = fixed_appearance_error("", 38);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D fixed appearance GUID")
    );
}

#[test]
fn fixed_appearance_library_copy_refuses_retained_limit() {
    let error = fixed_appearance_error("L", 40);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D appearance library ID")
    );
}

#[test]
fn schema_connected_texture_vector_refuses_collection_limit() {
    let guid = "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb";
    let records = [
        texture_record(guid, "textures/a.png"),
        appearance_connected_to(guid),
    ];
    let error = schema_appearance_error(&records, 4, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D connected textures")
    );
}

macro_rules! schema_appearance_text_limit_test {
    ($name:ident, $limit:expr, $operation:literal) => {
        #[test]
        fn $name() {
            let record = appearance_record("GenericSchema", Default::default());
            let error = schema_appearance_error(&[record], u64::MAX, $limit);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == $operation));
        }
    };
}

schema_appearance_text_limit_test!(
    schema_appearance_name_refuses_retained_limit,
    58,
    "copy F3D appearance name"
);
schema_appearance_text_limit_test!(
    schema_appearance_guid_refuses_retained_limit,
    67,
    "copy F3D appearance GUID"
);
schema_appearance_text_limit_test!(
    schema_appearance_visual_guid_refuses_retained_limit,
    103,
    "copy F3D appearance visual GUID"
);
schema_appearance_text_limit_test!(
    schema_appearance_schema_refuses_retained_limit,
    139,
    "copy F3D appearance schema"
);

#[test]
fn material_note_format_refuses_retained_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    let note = "Protein A record 1 rejected: B";
    policy.limits.max_retained_bytes = u64::try_from(note.len() - 1).unwrap();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    let error = ctx
        .format_retained(
            format_args!("Protein {} record {} rejected: {}", "A", 1, "B"),
            "retain F3D protein rejection note",
        )
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D protein rejection note")
    );
}

#[test]
fn material_note_vector_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    let mut notes = Vec::new();
    let error = ctx
        .push_vec(
            &mut notes,
            "note".to_owned(),
            "collect F3D protein rejection notes",
        )
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D protein rejection notes")
    );
}

#[test]
fn material_appearance_merge_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    let mut merged = Vec::new();
    let error = ctx
        .append_vec(
            &mut merged,
            &mut { vec![1, 2] },
            "merge F3D fixed appearances",
        )
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "merge F3D fixed appearances")
    );
}

#[test]
fn material_asset_appearance_vector_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    let mut appearances = Vec::new();
    let error = ctx
        .append_vec(
            &mut appearances,
            &mut { vec![1, 2] },
            "collect F3D asset appearances",
        )
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D asset appearances")
    );
}

#[test]
fn material_schema_id_index_refuses_collection_limit() {
    let (appearances, _) =
        super::schema_appearances(&[appearance_record("GenericSchema", Default::default())])
            .expect("schema appearance");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    let error = ctx
        .collect_hash_set(
            appearances.iter().map(|appearance| appearance.id.as_str()),
            "index F3D schema appearance IDs",
        )
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D schema appearance IDs")
    );
}

macro_rules! material_item_limit_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_collection_items = 0;
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                .expect("test decode context");
            let mut items = Vec::new();
            let error = ctx.push_vec(&mut items, 1, $operation)
                .unwrap_err();
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == $operation));
        }
    };
}

material_item_limit_test!(
    material_assignment_vector_refuses_collection_limit,
    "collect F3D material assignments"
);
material_item_limit_test!(
    material_browser_appearance_vector_refuses_collection_limit,
    "collect F3D browser body appearances"
);
material_item_limit_test!(
    material_body_override_vector_refuses_collection_limit,
    "collect F3D body appearance overrides"
);
material_item_limit_test!(
    material_assignment_appearance_vector_refuses_collection_limit,
    "collect F3D assignment appearances"
);
material_item_limit_test!(
    material_override_binding_vector_refuses_collection_limit,
    "collect F3D override appearance bindings"
);

#[test]
fn material_assignment_id_refuses_retained_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    let error = crate::ids::native_scoped_id_charged(&ctx, "BulkStream", "material-assignment", 1)
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D native record ID")
    );
}

fn browser_node_bytes(members: &[u64]) -> Vec<u8> {
    let guid = "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb";
    let mut bytes = Vec::new();
    for member in members {
        bytes.extend_from_slice(&36_u32.to_le_bytes());
        for unit in guid.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes.extend_from_slice(&[0, 1, 1]);
        bytes.extend_from_slice(&member.to_le_bytes());
    }
    bytes
}

fn browser_node_error(members: &[u64], max_items: u64) -> cadmpeg_core::CodecError {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    crate::design::decode::body::scanned_browser_node_entities(&ctx, &browser_node_bytes(members))
        .unwrap_err()
}

#[test]
fn browser_node_scan_refuses_collection_limit() {
    let error = browser_node_error(&[1], 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D browser node identities")
    );
}

#[test]
fn browser_node_entity_index_refuses_collection_limit() {
    let error = browser_node_error(&[1], 1);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D browser node entities")
    );
}

#[test]
fn browser_node_ambiguity_index_refuses_collection_limit() {
    let error = browser_node_error(&[1, 2], 3);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D ambiguous browser nodes")
    );
}
