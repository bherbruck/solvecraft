// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]

use cadmpeg_core::decode::u64_from_index;

use std::io::{Cursor, Write};

use zip::CompressionMethod;

use crate::test_support::lp_ascii;
use crate::test_support::manifest_test::write_synthetic_manifests;
use crate::test_support::streams_test::design_metastream_with_records;
use crate::test_support::zip_test::with_scan;

fn decode_component_naming_spaces(
    scan: &crate::container::ContainerScan<'_>,
) -> Result<Vec<crate::records::recipes::DesignComponentNamingSpace>, cadmpeg_core::CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)?;
    super::decode_component_naming_spaces(&ctx, scan)
}

#[test]
fn bulk_metadata_reuses_the_parsed_type_table() {
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let meta = design_metastream_with_records(
        &[(
            "11111111-2222-3333-4444-555555555555",
            "21F379C8-CAFD-4985-B461-767673A4C502",
            0,
            "Component",
            &[17],
        )],
        &[],
    );
    let bulk_name = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(bulk_name, stored).unwrap();
    zip.write_all(&[]).unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&meta).unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    with_scan(&bytes, |scan| {
        let ctx = cadmpeg_test_support::service_decode_context();
        let first = super::metadata_for_bulk_stream(&ctx, scan, bulk_name)?.unwrap();
        let second = super::metadata_for_bulk_stream(&ctx, scan, bulk_name)?.unwrap();
        assert!(std::rc::Rc::ptr_eq(&first, &second));
        assert_eq!(first.types.len(), 1);
        assert_eq!(
            first.types[0]
                .entities
                .values()
                .copied()
                .collect::<Vec<_>>(),
            [17]
        );
        Ok::<_, cadmpeg_core::CodecError>(())
    })
    .unwrap();
}

#[test]
fn design_type_copy_refuses_table_entities_module_and_id_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let meta_name = "FusionAssetName[Active]/Design1/MetaStream.dat";
    let meta = design_metastream_with_records(
        &[(
            "11111111-2222-3333-4444-555555555555",
            "21F379C8-CAFD-4985-B461-767673A4C502",
            0,
            "Component",
            &[17],
        )],
        &[],
    );
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&[]).unwrap();
    zip.start_file(meta_name, stored).unwrap();
    zip.write_all(&meta).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let arena = DecodeArena::new();
    for (allowance, operation) in [
        (3, "f3d design type table"),
        (4, "f3d design type registered entities"),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = allowance;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = with_scan(&archive, |scan| super::decode_types(&ctx, scan))
            .err()
            .unwrap();
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == operation
        ));
    }
    for operation in [
        "f3d design type module",
        "f3d native stream key",
        "f3d design type id suffix",
    ] {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            operation,
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                with_scan(&archive, |scan| super::decode_types(&ctx, scan))
            },
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == operation)
        );
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let types = with_scan(&archive, |scan| super::decode_types(&ctx, scan)).unwrap();
    assert_eq!(types.len(), 1);
    assert_eq!(types[0].module, "Component");
    assert_eq!(
        types[0].entities.values().copied().collect::<Vec<_>>(),
        [17]
    );
    assert_eq!(
        types[0].id(),
        &crate::ids::native_design_type_id(meta_name, types[0].byte_offset)
    );
}

#[test]
fn stream_type_indexes_refuse_limits_and_match_escaped_scope() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let prefix = "Fusion% Asset:Name[Active]/Design1/";
    let bulk_name = format!("{prefix}BulkStream.dat");
    let meta_name = format!("{prefix}MetaStream.dat");
    let type_guid = "11111111-2222-3333-4444-555555555555";
    let design_type =
        crate::design::test_support::design_type(type_guid, None, 7, "Fusion", vec![17]);
    let design_type = crate::records::entity_header::SegmentType::try_new(
        crate::ids::native_design_type_id(&meta_name, 0),
        design_type,
    )
    .unwrap();
    let types = [design_type];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::stream_types_by_entity(&ctx, &types, &bulk_name)
        .err()
        .unwrap();
    assert!(matches!(error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "f3d stream types by entity"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::stream_types_by_class_tag(&ctx, &types, &bulk_name)
        .err()
        .unwrap();
    assert!(matches!(error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "f3d stream types by class tag"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let by_entity = super::stream_types_by_entity(&ctx, &types, &bulk_name).unwrap();
    assert_eq!(by_entity.get(&17), Some(&(type_guid, 7)));
    let by_class = super::stream_types_by_class_tag(&ctx, &types, &bulk_name).unwrap();
    assert_eq!(
        by_class
            .get(&256)
            .map(|design_type| design_type.type_guid.as_str()),
        Some(type_guid)
    );
    assert!(
        super::stream_types_by_entity(&ctx, &types, "other/BulkStream.dat")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn design_primary_frames_charge_registration_and_frame_storage() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut bytes = Vec::new();
    lp_ascii(&mut bytes, "256");
    bytes.extend_from_slice(&42_u32.to_le_bytes());
    let meta = crate::metastream::MetaStream {
        types: vec![crate::design::test_support::design_type(
            "00000000-0000-0000-0000-000000000001",
            None,
            0,
            "Test",
            vec![42],
        )],
        records: vec![crate::design::test_support::primary_record(42, 0)],
        secondary_records: Vec::new(),
    };
    let arena = DecodeArena::new();
    for (allowance, operation) in [
        (0, "frame F3D primary records"),
        (1, "index F3D primary entities"),
        (2, "f3d registered primary entities"),
        (3, "f3d design primary frames"),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = allowance;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::design_primary_frames(&ctx, &bytes, &meta)
            .err()
            .unwrap();
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == operation
        ));
    }
    crate::design::test_support::with_test_decode_context(|ctx| {
        let frames = super::design_primary_frames(ctx, &bytes, &meta).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].entity_id, 42);
    });
}

#[test]
fn typed_primary_frames_charge_all_collections() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut bytes = Vec::new();
    lp_ascii(&mut bytes, "256");
    bytes.extend_from_slice(&42_u32.to_le_bytes());
    let type_guid = "00000000-0000-0000-0000-000000000001";
    let meta = crate::metastream::MetaStream {
        types: vec![crate::design::test_support::design_type(
            type_guid,
            None,
            0,
            "Test",
            vec![42],
        )],
        records: vec![crate::design::test_support::primary_record(42, 0)],
        secondary_records: Vec::new(),
    };
    let arena = DecodeArena::new();
    for (allowance, operation) in [
        (0, "f3d typed primary entities"),
        (1, "frame F3D primary records"),
        (2, "index F3D primary entities"),
        (3, "f3d registered primary entities"),
        (4, "f3d design primary frames"),
        (5, "f3d resolved primary entities"),
        (6, "f3d typed primary frames"),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = allowance;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::typed_primary_frames(&ctx, &bytes, &meta, type_guid, "test")
            .err()
            .unwrap();
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == operation
        ));
    }
    crate::design::test_support::with_test_decode_context(|ctx| {
        let frames = super::typed_primary_frames(ctx, &bytes, &meta, type_guid, "test").unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].entity_id, 42);
    });
}

#[test]
fn feature_timeline_item_limit_refuses_before_counted_vector_allocation() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use std::collections::HashMap;

    let mut bulk = Vec::new();
    lp_ascii(&mut bulk, "256");
    bulk.extend_from_slice(&35_u64.to_le_bytes());
    lp_ascii(&mut bulk, "Timeline");
    bulk.extend_from_slice(&[0, 0]);
    for target in [17_u64, 101, 102] {
        bulk.push(1);
        bulk.extend_from_slice(&target.to_le_bytes());
        bulk.extend_from_slice(&[0, 0]);
        if target == 17 {
            bulk.extend_from_slice(&2_u32.to_le_bytes());
        }
    }
    let frame = 0..bulk.len();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::parse_feature_timeline_record(
        &limited,
        &bulk,
        "Design/BulkStream.dat",
        frame.clone(),
        ("256", 35),
        0,
        &HashMap::new(),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "admit F3D timeline item slots"
    ));

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let admitted = super::parse_feature_timeline_record(
        &service,
        &bulk,
        "Design/BulkStream.dat",
        frame,
        ("256", 35),
        0,
        &HashMap::new(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        admitted
            .frame()
            .items()
            .iter()
            .map(|item| item.value)
            .collect::<Vec<_>>(),
        [101, 102]
    );
}

#[test]
fn feature_timeline_id_refuses_prefix_and_suffix_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use std::collections::HashMap;

    let stream = "Fusion% Asset:Name[Active]/Design1/BulkStream.dat";
    let mut bulk = Vec::new();
    lp_ascii(&mut bulk, "256");
    bulk.extend_from_slice(&35_u64.to_le_bytes());
    lp_ascii(&mut bulk, "Timeline");
    bulk.extend_from_slice(&[0, 0, 1]);
    bulk.extend_from_slice(&17_u64.to_le_bytes());
    bulk.extend_from_slice(&[0, 0]);
    bulk.extend_from_slice(&0_u32.to_le_bytes());
    let arena = DecodeArena::new();
    let prefix_len = u64_from_index(crate::ids::native_scope(stream).len());
    let suffix_len = u64_from_index(":design-feature-timeline#0".len());
    for (allowance, operation) in [
        (prefix_len - 1, "f3d native stream key"),
        (prefix_len + suffix_len - 1, "retain F3D timeline identity"),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = allowance;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::parse_feature_timeline_record(
            &ctx,
            &bulk,
            stream,
            0..bulk.len(),
            ("256", 35),
            0,
            &HashMap::new(),
        )
        .err()
        .unwrap();
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == operation
        ));
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let timeline = super::parse_feature_timeline_record(
        &ctx,
        &bulk,
        stream,
        0..bulk.len(),
        ("256", 35),
        0,
        &HashMap::new(),
    )
    .unwrap()
    .unwrap();
    let expected_id = crate::ids::native_design_feature_timeline_id(stream, 0);
    assert_eq!(timeline.id(), &expected_id);
}

#[test]
fn timeline_collection_growth_refuses_at_map_child_and_output() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut bulk = Vec::new();
    lp_ascii(&mut bulk, "256");
    bulk.extend_from_slice(&35_u64.to_le_bytes());
    lp_ascii(&mut bulk, "Timeline");
    bulk.extend_from_slice(&[0, 0, 1]);
    bulk.extend_from_slice(&17_u64.to_le_bytes());
    bulk.extend_from_slice(&[0, 0]);
    bulk.extend_from_slice(&0_u32.to_le_bytes());
    let meta = design_metastream_with_records(
        &[
            (
                super::FEATURE_TIMELINE_TYPE_GUID,
                super::FEATURE_TIMELINE_BASE_TYPE_GUID,
                super::FEATURE_TIMELINE_TYPE_VERSIONS[0],
                "Fusion",
                &[35],
            ),
            (
                "11111111-2222-3333-4444-555555555555",
                "",
                0,
                "Fusion",
                &[17],
            ),
        ],
        &[(35, 0)],
    );
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&bulk).unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&meta).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let arena = DecodeArena::new();
    for (allowance, operation) in [
        (5, "index F3D timeline entity"),
        (6, "index F3D timeline type GUID"),
        (9, "retain F3D feature timeline"),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = allowance;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = with_scan(&archive, |scan| super::decode_feature_timelines(&ctx, scan))
            .err()
            .unwrap();
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == operation
        ));
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let admitted = with_scan(&archive, |scan| super::decode_feature_timelines(&ctx, scan)).unwrap();
    assert_eq!(admitted.len(), 1);
    assert_eq!(admitted[0].record_index.get(), 35);
}

#[test]
fn component_naming_space_binds_component_entity_to_context_uuid() {
    const COMPONENT_TYPE_GUID: &str = "11111111-2222-3333-4444-555555555555";
    const CONTEXT_UUID: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";

    fn archive(bulk: &[u8]) -> Vec<u8> {
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        let meta = design_metastream_with_records(
            &[(
                COMPONENT_TYPE_GUID,
                "21F379C8-CAFD-4985-B461-767673A4C502",
                0,
                "Component",
                &[17],
            )],
            &[],
        );
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
            .unwrap();
        zip.write_all(bulk).unwrap();
        zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
            .unwrap();
        zip.write_all(&meta).unwrap();
        zip.finish().unwrap().into_inner()
    }

    fn binding(out: &mut Vec<u8>, component: u64, reserved_len: usize, context_uuid: &str) {
        out.push(1);
        out.extend_from_slice(&component.to_le_bytes());
        out.extend(std::iter::repeat_n(0, reserved_len));
        out.extend_from_slice(&36_u32.to_le_bytes());
        for code_unit in context_uuid.encode_utf16() {
            out.extend_from_slice(&code_unit.to_le_bytes());
        }
    }

    fn typed_binding(out: &mut Vec<u8>, component: u64, context_uuid: &str) {
        out.push(1);
        out.extend_from_slice(&component.to_le_bytes());
        out.extend_from_slice(
            &(u32::try_from(COMPONENT_TYPE_GUID.len()).expect("fixture value fits u32"))
                .to_le_bytes(),
        );
        out.extend_from_slice(COMPONENT_TYPE_GUID.as_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&36_u32.to_le_bytes());
        for code_unit in context_uuid.encode_utf16() {
            out.extend_from_slice(&code_unit.to_le_bytes());
        }
    }

    for reserved_len in [2, 3] {
        let mut bulk = vec![0xaa, 0xbb];
        let marker = bulk.len();
        binding(&mut bulk, 17, reserved_len, CONTEXT_UUID);
        let decoded = with_scan(&archive(&bulk), decode_component_naming_spaces)
            .expect("component naming space");
        let [space] = decoded.as_slice() else {
            panic!("expected one component naming space");
        };
        assert_eq!(space.component_record_index, 17);
        assert_eq!(space.context_uuid.as_str(), CONTEXT_UUID);
        assert_eq!(space.byte_offset, u64_from_index(marker));
        assert_eq!(
            space.context_uuid_offset,
            u64_from_index(marker + 9 + reserved_len)
        );
    }

    let mut typed = vec![0xaa, 0xbb];
    let typed_marker = typed.len();
    typed_binding(&mut typed, 17, CONTEXT_UUID);
    let decoded = with_scan(&archive(&typed), |scan| {
        decode_component_naming_spaces(scan)
    })
    .expect("typed component naming space");
    let [space] = decoded.as_slice() else {
        panic!("expected one typed component naming space");
    };
    assert_eq!(space.component_record_index, 17);
    assert_eq!(space.context_uuid.as_str(), CONTEXT_UUID);
    assert_eq!(space.byte_offset, u64_from_index(typed_marker));

    let mut overlapping_reference = vec![1];
    binding(
        &mut overlapping_reference,
        17,
        2,
        "ffffffff-eeee-4ddd-8ccc-bbbbbbbbbbbb",
    );
    typed_binding(&mut overlapping_reference, 17, CONTEXT_UUID);
    let decoded = with_scan(&archive(&overlapping_reference), |scan| {
        decode_component_naming_spaces(scan)
    })
    .expect("typed binding beside an overlapping 01 01 reference");
    let [space] = decoded.as_slice() else {
        panic!("expected one component naming space");
    };
    assert_eq!(space.context_uuid.as_str(), CONTEXT_UUID);

    let mut conflicting = Vec::new();
    binding(&mut conflicting, 17, 3, CONTEXT_UUID);
    binding(
        &mut conflicting,
        17,
        3,
        "ffffffff-eeee-4ddd-8ccc-bbbbbbbbbbbb",
    );
    let error = with_scan(&archive(&conflicting), |scan| {
        decode_component_naming_spaces(scan)
    })
    .expect_err("conflicting component UUIDs must be rejected");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
}

#[test]
fn component_naming_space_refuses_each_collection_and_id_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let bulk_name = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let context_uuid = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
    let meta = design_metastream_with_records(
        &[(
            "11111111-2222-3333-4444-555555555555",
            "21F379C8-CAFD-4985-B461-767673A4C502",
            0,
            "Component",
            &[17],
        )],
        &[],
    );
    let mut bulk = vec![0xaa, 0xbb, 1];
    bulk.extend_from_slice(&17_u64.to_le_bytes());
    bulk.extend_from_slice(&[0, 0]);
    crate::test_support::lp_utf16(&mut bulk, context_uuid);
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(bulk_name, stored).unwrap();
    zip.write_all(&bulk).unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&meta).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let arena = DecodeArena::new();
    for (allowance, operation) in [
        (3, "f3d component naming registered entities"),
        (4, "f3d component naming spaces by entity"),
        (5, "f3d component naming spaces output"),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = allowance;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = with_scan(&archive, |scan| {
            super::decode_component_naming_spaces(&ctx, scan)
        })
        .err()
        .unwrap();
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == operation
        ));
    }
    for operation in [
        "f3d native stream key",
        "f3d component naming space id suffix",
    ] {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            operation,
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                with_scan(&archive, |scan| {
                    super::decode_component_naming_spaces(&ctx, scan)
                })
            },
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == operation)
        );
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let spaces = with_scan(&archive, |scan| {
        super::decode_component_naming_spaces(&ctx, scan)
    })
    .unwrap();
    assert_eq!(spaces.len(), 1);
    assert_eq!(spaces[0].context_uuid.as_str(), context_uuid);
    assert_eq!(
        spaces[0].id,
        crate::ids::native_design_component_naming_space_id(bulk_name, 2)
    );
}

#[test]
fn component_naming_uuid_refuses_retained_limit_in_both_reference_forms() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    const TYPE_GUID: &str = "11111111-2222-3333-4444-555555555555";
    const UUID: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
    let meta = design_metastream_with_records(
        &[(
            TYPE_GUID,
            "21F379C8-CAFD-4985-B461-767673A4C502",
            0,
            "Component",
            &[17],
        )],
        &[],
    );
    for inline_type in [false, true] {
        let mut bulk = vec![0xaa, 0xbb, 1];
        bulk.extend_from_slice(&17u64.to_le_bytes());
        if inline_type {
            bulk.extend_from_slice(
                &(u32::try_from(TYPE_GUID.len()).expect("fixture value fits u32")).to_le_bytes(),
            );
            bulk.extend_from_slice(TYPE_GUID.as_bytes());
        }
        bulk.extend_from_slice(&[0, 0]);
        crate::test_support::lp_utf16(&mut bulk, UUID);
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
            .unwrap();
        zip.write_all(&bulk).unwrap();
        zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
            .unwrap();
        zip.write_all(&meta).unwrap();
        let archive = zip.finish().unwrap().into_inner();
        let arena = DecodeArena::new();
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "f3d Design UTF-16 text",
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                with_scan(&archive, |scan| {
                    super::decode_component_naming_spaces(&ctx, scan)
                })
            },
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "f3d Design UTF-16 text")
        );
    }
}

#[test]
fn design_feature_timeline_versions_share_variable_width_local_references() {
    const INLINE_TYPE_GUID: &str = "11111111-2222-3333-4444-555555555555";

    fn local_reference(out: &mut Vec<u8>, target: u64, inline_type: bool) {
        out.push(1);
        out.extend_from_slice(&target.to_le_bytes());
        if inline_type {
            lp_ascii(out, INLINE_TYPE_GUID);
        }
        out.extend_from_slice(&[0, 0]);
    }
    fn archive(meta: &[u8], bulk: &[u8]) -> Vec<u8> {
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
            .unwrap();
        zip.write_all(bulk).unwrap();
        zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
            .unwrap();
        zip.write_all(meta).unwrap();
        zip.finish().unwrap().into_inner()
    }

    let mut bulk = Vec::new();
    lp_ascii(&mut bulk, "256");
    bulk.extend_from_slice(&35_u64.to_le_bytes());
    lp_ascii(&mut bulk, "Timeline");
    bulk.extend_from_slice(&[0, 0]);
    local_reference(&mut bulk, 17, false);
    bulk.extend_from_slice(&2_u32.to_le_bytes());
    local_reference(&mut bulk, 101, false);
    local_reference(&mut bulk, 102, true);
    for version in crate::design::decode::meta::FEATURE_TIMELINE_TYPE_VERSIONS {
        let meta = design_metastream_with_records(
            &[
                (
                    crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID,
                    crate::design::decode::meta::FEATURE_TIMELINE_BASE_TYPE_GUID,
                    version,
                    "Fusion",
                    &[35],
                ),
                (INLINE_TYPE_GUID, "", 0, "Fusion", &[17, 101, 102]),
            ],
            &[(35, 0)],
        );
        let decoded = with_scan(&archive(&meta, &bulk), |scan| {
            crate::design::decode::meta::decode_feature_timelines(
                &cadmpeg_test_support::service_decode_context(),
                scan,
            )
        })
        .expect("exact feature timeline");
        let [timeline] = decoded.as_slice() else {
            panic!("expected one timeline record");
        };
        assert_eq!(timeline.record_index.get(), 35);
        assert_eq!(timeline.context_record_index.get(), 17);
        assert_eq!(
            timeline
                .frame()
                .items()
                .iter()
                .map(|item| item.value)
                .collect::<Vec<_>>(),
            [101, 102]
        );
        assert_eq!(timeline.frame().frame_length(), u64_from_index(bulk.len()));
        for item in timeline.frame().items() {
            assert_eq!(
                u64::from_le_bytes(
                    bulk[usize::try_from(item.offset).expect("fixture offset fits address space")
                        ..usize::try_from(item.offset).expect("fixture offset fits address space")
                            + 8]
                        .try_into()
                        .expect("timeline target")
                ),
                item.value
            );
        }

        let mut duplicate = bulk.clone();
        let second_offset = usize::try_from(timeline.frame().items()[1].offset)
            .expect("fixture offset fits address space");
        duplicate[second_offset..second_offset + 8].copy_from_slice(&101_u64.to_le_bytes());
        let error = with_scan(&archive(&meta, &duplicate), |scan| {
            crate::design::decode::meta::decode_feature_timelines(
                &cadmpeg_test_support::service_decode_context(),
                scan,
            )
        })
        .expect_err("duplicate timeline items must be rejected");
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));

        let mut mismatched_inline_type = bulk.clone();
        let inline_type_at = mismatched_inline_type
            .windows(INLINE_TYPE_GUID.len())
            .position(|window| window == INLINE_TYPE_GUID.as_bytes())
            .expect("inline type GUID");
        mismatched_inline_type[inline_type_at] = b'2';
        let error = with_scan(&archive(&meta, &mismatched_inline_type), |scan| {
            crate::design::decode::meta::decode_feature_timelines(
                &cadmpeg_test_support::service_decode_context(),
                scan,
            )
        })
        .expect_err("an inline type GUID must match the target registration");
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    }

    let unsupported_meta = design_metastream_with_records(
        &[(
            crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID,
            crate::design::decode::meta::FEATURE_TIMELINE_BASE_TYPE_GUID,
            4,
            "Fusion",
            &[35],
        )],
        &[(35, 0)],
    );
    let error = with_scan(&archive(&unsupported_meta, &bulk), |scan| {
        crate::design::decode::meta::decode_feature_timelines(
            &cadmpeg_test_support::service_decode_context(),
            scan,
        )
    })
    .expect_err("an unsupported timeline version must not use a known frame speculatively");
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));

    for (base_type_guid, module) in [
        ("22222222-3333-4444-5555-666666666666", "Fusion"),
        (
            crate::design::decode::meta::FEATURE_TIMELINE_BASE_TYPE_GUID,
            "Other",
        ),
    ] {
        let incompatible_meta = design_metastream_with_records(
            &[(
                crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID,
                base_type_guid,
                crate::design::decode::meta::FEATURE_TIMELINE_TYPE_VERSIONS[1],
                module,
                &[35],
            )],
            &[(35, 0)],
        );
        let error = with_scan(&archive(&incompatible_meta, &bulk), |scan| {
            crate::design::decode::meta::decode_feature_timelines(
                &cadmpeg_test_support::service_decode_context(),
                scan,
            )
        })
        .expect_err("incompatible timeline registration metadata must be rejected");
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    }
}
