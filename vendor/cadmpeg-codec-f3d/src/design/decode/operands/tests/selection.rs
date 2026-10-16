// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::operands::bind_lost_edge_groups;
use crate::design::decode::operands::parse_construction_operand_identity;
use crate::design::decode::operands::parse_entity_selection_operand;
use crate::design::decode::operands::parse_extrude_selection_group;

use crate::design::decode::operands::parse_sketch_profile;
use crate::design::decode::operands::parse_sketch_profile_region_selection;

use crate::records::decal::DesignRecordHeader;
use crate::records::entity_header::DesignEntityHeader;
use crate::records::entity_header::DESIGN_MODULE_SKETCH;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::references::LostEdgeReference;

use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::construction::DesignConstructionOperandGroupFrame;
use crate::records::topology::construction::DesignConstructionOperandIdentity;
use crate::records::topology::extrude_selection::DesignOperandRole;

use crate::test_support::indexed_header;
use crate::test_support::lp_utf16;

#[test]
fn sketch_profile_frame_resolves_its_decimal_entity_suffix() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"308");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&103u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "e72ed0d8-58b4-4b8e-800d-5eaeea9c0c4b");
    lp_utf16(&mut bytes, "172");
    let tail_at = bytes.len();
    bytes.extend_from_slice(&[0; 94]);
    let paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    let header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("308".to_owned()).unwrap(),
        record_index: 100,
    };
    let entity = DesignEntityHeader {
        id: "f3d:Design/BulkStream.dat:entity#172".into(),
        byte_offset: 1000,

        entity_id: crate::records::identity::DesignEntityId::try_from("0_172".to_owned())
            .expect("valid entity ID"),
        class_tag: crate::records::references::DesignClassTag::try_from("269".to_owned()).unwrap(),
        optional_slot_present: false,
        registration: crate::records::entity_header::DesignEntityRegistration::new(
            Some(DESIGN_MODULE_SKETCH.to_owned()),
            Some(crate::records::entity_header::SketchHeaderReferences {
                record_reference: Some(200),
                record_reference_offset: 1010,
                references: Vec::new(),
            }),
            crate::records::identity::ReferenceRun::unlocated(Vec::new()),
        )
        .expect("valid module registration"),
    };

    let profile = parse_sketch_profile(
        &ctx,
        &bytes,
        "f3d:Design/BulkStream.dat",
        4,
        &header,
        std::slice::from_ref(&entity),
    )
    .transpose()
    .unwrap()
    .expect("sketch-profile operand");
    assert_eq!(profile.scope_reference_ordinal, 4);
    assert_eq!(profile.entity_id.suffix(), 172);
    assert_eq!(profile.entity_id.as_str(), "0_172");
    assert_eq!(profile.paired_byte_offset(), u64_from_index(paired_at));

    bytes.truncate(paired_at - 94);
    bytes[4..7].copy_from_slice(b"319");
    let mut compact_tail = vec![0; 93];
    compact_tail[0] = 1;
    compact_tail[8..12].copy_from_slice(&1u32.to_le_bytes());
    compact_tail[12] = 1;
    compact_tail[13..17].copy_from_slice(&500u32.to_le_bytes());
    compact_tail[41..45].copy_from_slice(&99u32.to_le_bytes());
    compact_tail[53..57].copy_from_slice(&99u32.to_le_bytes());
    compact_tail[57] = 1;
    compact_tail[58..62].copy_from_slice(&102u32.to_le_bytes());
    compact_tail[70] = 1;
    compact_tail[71..75].copy_from_slice(&101u32.to_le_bytes());
    compact_tail[82] = 1;
    compact_tail[83..87].copy_from_slice(&777u32.to_le_bytes());
    bytes.extend_from_slice(&compact_tail);
    let compact_paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"258");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    let compact_header = DesignRecordHeader {
        class_tag: crate::records::references::DesignClassTag::try_from("319".to_owned()).unwrap(),
        ..header
    };
    let compact = parse_sketch_profile(
        &ctx,
        &bytes,
        "f3d:Design/BulkStream.dat",
        2,
        &compact_header,
        std::slice::from_ref(&entity),
    )
    .transpose()
    .unwrap()
    .expect("compact sketch-profile operand");
    assert_eq!(compact.scope_reference_ordinal, 2);
    assert_eq!(
        compact.paired_byte_offset(),
        u64_from_index(compact_paired_at)
    );

    bytes.truncate(tail_at);
    let mut omitted_ordinal_tail = vec![0; 89];
    omitted_ordinal_tail[0] = 1;
    omitted_ordinal_tail[8..12].copy_from_slice(&1u32.to_le_bytes());
    omitted_ordinal_tail[12] = 1;
    omitted_ordinal_tail[13..17].copy_from_slice(&500u32.to_le_bytes());
    omitted_ordinal_tail[41..45].copy_from_slice(&99u32.to_le_bytes());
    omitted_ordinal_tail[53] = 1;
    omitted_ordinal_tail[54..58].copy_from_slice(&102u32.to_le_bytes());
    omitted_ordinal_tail[66] = 1;
    omitted_ordinal_tail[67..71].copy_from_slice(&101u32.to_le_bytes());
    omitted_ordinal_tail[78] = 1;
    omitted_ordinal_tail[79..83].copy_from_slice(&777u32.to_le_bytes());
    bytes.extend_from_slice(&omitted_ordinal_tail);
    let omitted_paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"258");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    let omitted = parse_sketch_profile(
        &ctx,
        &bytes,
        "f3d:Design/BulkStream.dat",
        2,
        &compact_header,
        std::slice::from_ref(&entity),
    )
    .transpose()
    .unwrap()
    .expect("omitted-ordinal sketch-profile operand");
    assert_eq!(
        omitted.paired_byte_offset(),
        u64_from_index(omitted_paired_at)
    );
}

#[test]
fn sketch_profile_text_copies_refuse_retained_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"308");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&103u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "e72ed0d8-58b4-4b8e-800d-5eaeea9c0c4b");
    lp_utf16(&mut bytes, "00000000000000000000000000000172");
    bytes.extend_from_slice(&[0; 94]);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    let header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("308".to_owned()).unwrap(),
        record_index: 100,
    };
    let entity = DesignEntityHeader {
        id: "f3d:Design/BulkStream.dat:entity#172".into(),
        byte_offset: 1000,
        entity_id: crate::records::identity::DesignEntityId::try_from("0_172".to_owned()).unwrap(),
        class_tag: crate::records::references::DesignClassTag::try_from("269".to_owned()).unwrap(),
        optional_slot_present: false,
        registration: crate::records::entity_header::DesignEntityRegistration::new(
            Some(DESIGN_MODULE_SKETCH.to_owned()),
            Some(crate::records::entity_header::SketchHeaderReferences {
                record_reference: Some(200),
                record_reference_offset: 1010,
                references: Vec::new(),
            }),
            crate::records::identity::ReferenceRun::unlocated(Vec::new()),
        )
        .unwrap(),
    };
    for (retained_limit, operation) in [
        (35, "f3d Design UTF-16 text"),
        (40, "f3d sketch profile entity ID"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = retained_limit;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            parse_sketch_profile(
                &ctx, &bytes, "f3d:Design/BulkStream.dat", 4,
                &header, std::slice::from_ref(&entity),
            ).transpose(),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::RetainedBytes
                    && failure.operation == operation
        ));
    }
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let profile = parse_sketch_profile(
        &ctx,
        &bytes,
        "f3d:Design/BulkStream.dat",
        4,
        &header,
        std::slice::from_ref(&entity),
    )
    .transpose()
    .unwrap()
    .expect("profile with leading-zero suffix");
    assert_eq!(profile.entity_id.suffix(), 172);
}

#[test]
fn generated_base_flange_profile_frame_resolves() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (bytes, _) = crate::test_support::streams_test::generated_design_base_flange_bulkstream();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let profile_offset = records
        .offsets(1501)
        .first()
        .copied()
        .expect("generated BaseFlange profile");
    let header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#1501".into(),
        byte_offset: u64_from_index(profile_offset),
        class_tag: crate::records::references::DesignClassTag::try_from("377".to_owned()).unwrap(),
        record_index: 1501,
    };
    let entity = DesignEntityHeader {
        id: "f3d:Design/BulkStream.dat:entity#800".into(),
        byte_offset: 0,

        entity_id: crate::records::identity::DesignEntityId::try_from("Sketch_800".to_owned())
            .expect("valid entity ID"),
        class_tag: crate::records::references::DesignClassTag::try_from("365".to_owned()).unwrap(),
        optional_slot_present: false,
        registration: crate::records::entity_header::DesignEntityRegistration::new(
            Some(DESIGN_MODULE_SKETCH.to_owned()),
            None,
            crate::records::identity::ReferenceRun::unlocated(Vec::new()),
        )
        .expect("valid module registration"),
    };
    let profile = parse_sketch_profile(
        &ctx,
        &bytes,
        "f3d:Design/BulkStream.dat",
        1,
        &header,
        std::slice::from_ref(&entity),
    )
    .transpose()
    .unwrap()
    .expect("generated BaseFlange profile operand");
    assert_eq!(profile.entity_id.as_str(), "Sketch_800");
    assert_eq!(profile.entity_id.suffix(), 800);
}

#[test]
fn lost_edge_stream_and_run_copies_refuse_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let edge = LostEdgeReference::new(
        "f3d:Design/BulkStream.dat:lost-edge-reference#152".into(),
        152,
        "419".into(),
        299,
        "326".into(),
        300,
    )
    .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "f3d lost-edge stream records",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            match ResourceDimension::CollectionItems {
                cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                    policy.limits.max_retained_bytes = cap;
                }
                cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                    policy.limits.max_collection_items = cap;
                }
                cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                    policy.limits.max_materialized_bytes = cap;
                }
                cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                    policy.limits.max_work_units = cap;
                }
                dimension => panic!("unsupported refusal dimension: {dimension:?}"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            (crate::design::decode::operands::collect_stream_lost_edges(
                &ctx,
                std::slice::from_ref(&edge),
                "f3d:Design/BulkStream.dat",
            ))
            .map(|_| ())
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
    match ResourceDimension::CollectionItems {
        cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
            policy.limits.max_retained_bytes = refusal_cap;
        }
        cadmpeg_core::decode::ResourceDimension::CollectionItems => {
            policy.limits.max_collection_items = refusal_cap;
        }
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
            policy.limits.max_materialized_bytes = refusal_cap;
        }
        cadmpeg_core::decode::ResourceDimension::WorkUnits => {
            policy.limits.max_work_units = refusal_cap;
        }
        dimension => panic!("unsupported refusal dimension: {dimension:?}"),
    }
    let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "f3d lost-edge stream records",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            match ResourceDimension::CollectionItems {
                cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                    policy.limits.max_retained_bytes = cap;
                }
                cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                    policy.limits.max_collection_items = cap;
                }
                cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                    policy.limits.max_materialized_bytes = cap;
                }
                cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                    policy.limits.max_work_units = cap;
                }
                dimension => panic!("unsupported refusal dimension: {dimension:?}"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            (crate::design::decode::operands::collect_stream_lost_edges(
                &ctx,
                std::slice::from_ref(&edge),
                "f3d:Design/BulkStream.dat",
            ))
            .map(|_| ())
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
    match ResourceDimension::CollectionItems {
        cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
            policy.limits.max_retained_bytes = refusal_cap;
        }
        cadmpeg_core::decode::ResourceDimension::CollectionItems => {
            policy.limits.max_collection_items = refusal_cap;
        }
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
            policy.limits.max_materialized_bytes = refusal_cap;
        }
        cadmpeg_core::decode::ResourceDimension::WorkUnits => {
            policy.limits.max_work_units = refusal_cap;
        }
        dimension => panic!("unsupported refusal dimension: {dimension:?}"),
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::design::decode::operands::collect_stream_lost_edges(
            &ctx, std::slice::from_ref(&edge), "f3d:Design/BulkStream.dat",
        ),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d lost-edge stream records"
    ));
    let run = [&edge];
    let arena = DecodeArena::new();
    let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "f3d lost-edge run IDs",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            match ResourceDimension::CollectionItems {
                cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                    policy.limits.max_retained_bytes = cap;
                }
                cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                    policy.limits.max_collection_items = cap;
                }
                cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                    policy.limits.max_materialized_bytes = cap;
                }
                cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                    policy.limits.max_work_units = cap;
                }
                dimension => panic!("unsupported refusal dimension: {dimension:?}"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            (crate::design::decode::operands::copy_lost_edge_run_ids(&ctx, &run)).map(|_| ())
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
    match ResourceDimension::CollectionItems {
        cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
            policy.limits.max_retained_bytes = refusal_cap;
        }
        cadmpeg_core::decode::ResourceDimension::CollectionItems => {
            policy.limits.max_collection_items = refusal_cap;
        }
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
            policy.limits.max_materialized_bytes = refusal_cap;
        }
        cadmpeg_core::decode::ResourceDimension::WorkUnits => {
            policy.limits.max_work_units = refusal_cap;
        }
        dimension => panic!("unsupported refusal dimension: {dimension:?}"),
    }
    let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "f3d lost-edge run IDs",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            match ResourceDimension::CollectionItems {
                cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                    policy.limits.max_retained_bytes = cap;
                }
                cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                    policy.limits.max_collection_items = cap;
                }
                cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                    policy.limits.max_materialized_bytes = cap;
                }
                cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                    policy.limits.max_work_units = cap;
                }
                dimension => panic!("unsupported refusal dimension: {dimension:?}"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            (crate::design::decode::operands::copy_lost_edge_run_ids(&ctx, &run)).map(|_| ())
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
    match ResourceDimension::CollectionItems {
        cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
            policy.limits.max_retained_bytes = refusal_cap;
        }
        cadmpeg_core::decode::ResourceDimension::CollectionItems => {
            policy.limits.max_collection_items = refusal_cap;
        }
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
            policy.limits.max_materialized_bytes = refusal_cap;
        }
        cadmpeg_core::decode::ResourceDimension::WorkUnits => {
            policy.limits.max_work_units = refusal_cap;
        }
        dimension => panic!("unsupported refusal dimension: {dimension:?}"),
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::design::decode::operands::copy_lost_edge_run_ids(&ctx, &run),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d lost-edge run IDs"
    ));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;
    policy.limits.max_retained_bytes = u64::try_from(edge.id.len() - 1).unwrap();
    let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "f3d lost-edge run ID text",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            match ResourceDimension::RetainedBytes {
                cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                    policy.limits.max_retained_bytes = cap;
                }
                cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                    policy.limits.max_collection_items = cap;
                }
                cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                    policy.limits.max_materialized_bytes = cap;
                }
                cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                    policy.limits.max_work_units = cap;
                }
                dimension => panic!("unsupported refusal dimension: {dimension:?}"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            (crate::design::decode::operands::copy_lost_edge_run_ids(&ctx, &run)).map(|_| ())
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
    match ResourceDimension::RetainedBytes {
        cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
            policy.limits.max_retained_bytes = refusal_cap;
        }
        cadmpeg_core::decode::ResourceDimension::CollectionItems => {
            policy.limits.max_collection_items = refusal_cap;
        }
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
            policy.limits.max_materialized_bytes = refusal_cap;
        }
        cadmpeg_core::decode::ResourceDimension::WorkUnits => {
            policy.limits.max_work_units = refusal_cap;
        }
        dimension => panic!("unsupported refusal dimension: {dimension:?}"),
    }
    let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "f3d lost-edge run ID text",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            match ResourceDimension::RetainedBytes {
                cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                    policy.limits.max_retained_bytes = cap;
                }
                cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                    policy.limits.max_collection_items = cap;
                }
                cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                    policy.limits.max_materialized_bytes = cap;
                }
                cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                    policy.limits.max_work_units = cap;
                }
                dimension => panic!("unsupported refusal dimension: {dimension:?}"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            (crate::design::decode::operands::copy_lost_edge_run_ids(&ctx, &run)).map(|_| ())
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
    match ResourceDimension::RetainedBytes {
        cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
            policy.limits.max_retained_bytes = refusal_cap;
        }
        cadmpeg_core::decode::ResourceDimension::CollectionItems => {
            policy.limits.max_collection_items = refusal_cap;
        }
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
            policy.limits.max_materialized_bytes = refusal_cap;
        }
        cadmpeg_core::decode::ResourceDimension::WorkUnits => {
            policy.limits.max_work_units = refusal_cap;
        }
        dimension => panic!("unsupported refusal dimension: {dimension:?}"),
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::design::decode::operands::copy_lost_edge_run_ids(&ctx, &run),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::RetainedBytes
                && failure.operation == "f3d lost-edge run ID text"
    ));
}

#[test]
fn extrude_operand_identity_walks_shared_wrapper_grammar_to_a_fixed_leaf() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: "f3d:Design/BulkStream.dat:operand-group#100".into(),
            scope_record_index: 12,
            scope_reference_ordinal: 0,
            record_index: 100,
            byte_offset: 1000,
            class_tag: crate::records::references::DesignClassTag::try_from("332".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: 200,
                offset: 1026,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 1021,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: vec![crate::records::identity::Located {
                        value: 300,
                        offset: 1043,
                    }],
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 180,
                    opaque_index_offset: 1071,
                    opaque_scalar: 0.125,
                    opaque_scalar_offset: 1075,
                    variant: false,
                },
            )
            .unwrap(),
            operand_role: crate::records::topology::construction::DesignConstructionOperandRole::ExtrudeBodiesB,
            role_offset: 1053,

            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "259".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 1124,
        },
    )
    .unwrap();
    let wrapper_header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#300".into(),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("326".to_owned()).unwrap(),
        record_index: 300,
    };
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"326", 300);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&[1, 1, 0]);
    indexed_header(&mut bytes, *b"326", 305);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&[1, 1, 0]);
    indexed_header(&mut bytes, *b"324", 400);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&586u64.to_le_bytes());
    lp_utf16(&mut bytes, "df9087bd-02a6-4a3f-a132-7e69990f323c");
    lp_utf16(&mut bytes, "0b2382d1-caaf-4eb9-b40d-a6322a7ed829");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 5]);
    indexed_header(&mut bytes, *b"301", 900);

    for (limit, operation) in [
        (0, "f3d construction identity wrapper visited keys"),
        (1, "f3d construction identity wrappers"),
    ] {
        let limited_arena = cadmpeg_core::decode::DecodeArena::new();
        let mut limited_policy = cadmpeg_core::decode::DecodePolicy::default();
        limited_policy.limits.max_collection_items = limit;
        let (limited_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &limited_arena,
            &limited_policy,
        )
        .unwrap();
        assert!(matches!(
            parse_construction_operand_identity(&limited_ctx, &bytes, &group, &wrapper_header),
            Some(Err(cadmpeg_core::CodecError::ResourceLimit(failure)))
                if failure.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                    && failure.operation == operation
        ));
    }

    let identity = parse_construction_operand_identity(&ctx, &bytes, &group, &wrapper_header)
        .expect("identity chain")
        .unwrap();
    assert_eq!(
        identity
            .wrappers()
            .iter()
            .map(|wrapper| wrapper.record_index)
            .collect::<Vec<_>>(),
        [300, 305]
    );
    assert_eq!(
        identity
            .wrappers()
            .iter()
            .map(|wrapper| wrapper.byte_offset)
            .collect::<Vec<_>>(),
        [0, 24]
    );
    assert_eq!(identity.following_record_index(), 400);
    assert_eq!(identity.following_byte_offset(), 48);
    let persistent = identity
        .persistent_identity()
        .expect("fixed persistent identity leaf");
    assert_eq!(persistent.local_id, 586);
    assert_eq!(persistent.next_record_index, 900);
    assert_eq!(persistent.next_byte_offset(), 238);

    let mut expanded_bytes = bytes[..233].to_vec();
    expanded_bytes.extend_from_slice(&[0; 4]);
    expanded_bytes.push(1);
    expanded_bytes.extend_from_slice(&900u32.to_le_bytes());
    expanded_bytes.extend_from_slice(&[0; 6]);
    indexed_header(&mut expanded_bytes, *b"301", 900);
    let expanded =
        parse_construction_operand_identity(&ctx, &expanded_bytes, &group, &wrapper_header)
            .expect("identity chain with expanded tail reference")
            .unwrap();
    let persistent = expanded
        .persistent_identity()
        .expect("expanded persistent identity leaf");
    assert_eq!(persistent.tail_slot_offset(), 233);
    assert_eq!(persistent.next_record_index, 900);
    assert_eq!(persistent.next_byte_offset(), 248);

    let mut bound_group = group;
    let mut terminating_identity = identity;
    terminating_identity.id =
        "f3d:Design/BulkStream.dat:design-construction-operand-identity#200".into();
    let mut draft = terminating_identity.into_draft();
    for wrapper in &mut draft.wrappers {
        wrapper.byte_offset += 200;
    }
    draft.following_byte_offset += 200;
    if let Some(persistent) = draft.persistent_identity.take() {
        let mut leaf = persistent.into_draft();
        leaf.local_id_offset += 200;
        leaf.asset_id_offset += 200;
        leaf.context_id_offset += 200;
        leaf.tail_slot_offset += 200;
        leaf.next_byte_offset += 200;
        draft.persistent_identity = Some(
            crate::records::topology::construction::DesignConstructionPersistentIdentity::try_new(
                leaf,
            )
            .unwrap(),
        );
    }
    terminating_identity = DesignConstructionOperandIdentity::try_new(draft).unwrap();
    bind_lost_edge_groups(
        &ctx,
        std::slice::from_mut(&mut bound_group),
        std::slice::from_ref(&terminating_identity),
        &[LostEdgeReference::new(
            "f3d:Design/BulkStream.dat:lost-edge-reference#152".into(),
            152,
            "419".into(),
            299,
            "326".into(),
            300,
        )
        .expect("valid lost-edge record layout")],
    )
    .expect("lost-edge run terminates at the group identity");
    assert_eq!(
        bound_group.lost_edge_references,
        ["f3d:Design/BulkStream.dat:lost-edge-reference#152"]
    );
}

#[test]
fn nested_entity_selection_member_retains_compact_and_expanded_identities() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: "f3d:Design/BulkStream.dat:operand-group#90".into(),
            scope_record_index: 80,
            scope_reference_ordinal: 0,
            record_index: 90,
            byte_offset: 900,
            class_tag: crate::records::references::DesignClassTag::try_from("269".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: 100,
                offset: 926,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 921,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: vec![crate::records::identity::Located {
                        value: 200,
                        offset: 943,
                    }],
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 971,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 975,
                    variant: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    DesignOperandRole::ROLE_0X5,
                ),
            role_offset: 953,

            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "265".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 1024,
        },
    )
    .unwrap();
    let record = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("333".to_owned()).unwrap(),
        record_index: 100,
    };
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"333", 100);
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&103u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "53aa8ab4-194a-434b-bd52-8c6d761dc147");
    lp_utf16(&mut bytes, "8e685642-4d68-4909-96d0-0dd4437491b6");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&[1, 0, 0]);
    indexed_header(&mut bytes, *b"265", 100);
    indexed_header(&mut bytes, *b"301", 101);
    indexed_header(&mut bytes, *b"446", 102);
    let identity_at = bytes.len();
    indexed_header(&mut bytes, *b"429", 103);
    bytes.extend_from_slice(&[0; 18]);
    bytes.extend_from_slice(&1331u64.to_le_bytes());
    bytes.extend_from_slice(&183u64.to_le_bytes());
    let next_at = bytes.len();
    indexed_header(&mut bytes, *b"311", 104);

    for limit in [35, 71] {
        let limited_arena = cadmpeg_core::decode::DecodeArena::new();
        let mut limited_policy = cadmpeg_core::decode::DecodePolicy::default();
        limited_policy.limits.max_retained_bytes = limit;
        let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            "f3d Design UTF-16 text",
            |cap| {
                let mut limited_policy = cadmpeg_core::decode::DecodePolicy::service();
                match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        limited_policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        limited_policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        limited_policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        limited_policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (limited_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                    &[],
                    &limited_arena,
                    &limited_policy,
                )
                .unwrap();
                ((parse_entity_selection_operand(&limited_ctx, &bytes, &group, 0, &record))
                    .transpose())
                .map(|_| ())
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        limited_policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                limited_policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                limited_policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                limited_policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                limited_policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            "f3d Design UTF-16 text",
            |cap| {
                let mut limited_policy = cadmpeg_core::decode::DecodePolicy::service();
                match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        limited_policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        limited_policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        limited_policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        limited_policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (limited_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                    &[],
                    &limited_arena,
                    &limited_policy,
                )
                .unwrap();
                ((parse_entity_selection_operand(&limited_ctx, &bytes, &group, 0, &record))
                    .transpose())
                .map(|_| ())
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        limited_policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                limited_policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                limited_policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                limited_policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                limited_policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let (limited_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &limited_arena,
            &limited_policy,
        )
        .unwrap();
        assert!(matches!(
            parse_entity_selection_operand(&limited_ctx, &bytes, &group, 0, &record),
            Some(Err(cadmpeg_core::CodecError::ResourceLimit(failure)))
                if failure.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                    && failure.operation == "f3d Design UTF-16 text"
        ));
    }
    let operand = parse_entity_selection_operand(&ctx, &bytes, &group, 0, &record)
        .expect("nested entity-selection frame")
        .unwrap();
    assert_eq!(operand.primary_identity, 1331);
    assert_eq!(
        operand
            .secondary()
            .map(|secondary| secondary.identity.value),
        Some(183)
    );
    assert_eq!(
        operand.clone().into_draft().identity_record_offset,
        u64_from_index(identity_at)
    );
    assert_eq!(operand.next_byte_offset(), u64_from_index(next_at));

    let mut compact = bytes[..identity_at].to_vec();
    indexed_header(&mut compact, *b"429", 103);
    compact.extend_from_slice(&[0; 10]);
    compact.extend_from_slice(&1331u64.to_le_bytes());
    let compact_next_at = compact.len();
    indexed_header(&mut compact, *b"311", 109);
    let compact_operand = parse_entity_selection_operand(&ctx, &compact, &group, 0, &record)
        .expect("compact nested entity-selection frame")
        .unwrap();
    assert_eq!(compact_operand.primary_identity, 1331);
    assert_eq!(
        compact_operand
            .secondary()
            .map(|secondary| secondary.identity.value),
        None
    );
    assert_eq!(
        compact_operand.clone().into_draft().identity_record_offset,
        u64_from_index(identity_at)
    );
    assert_eq!(compact_operand.next_record_index(), 109);
    assert_eq!(
        compact_operand.next_byte_offset(),
        u64_from_index(compact_next_at)
    );

    let mut curve_identity = bytes[..identity_at].to_vec();
    indexed_header(&mut curve_identity, *b"429", 103);
    curve_identity.extend_from_slice(&[0; 10]);
    curve_identity.extend_from_slice(&77u64.to_le_bytes());
    curve_identity.extend_from_slice(&1331u64.to_le_bytes());
    curve_identity.extend_from_slice(&183u64.to_le_bytes());
    let curve_next_at = curve_identity.len();
    indexed_header(&mut curve_identity, *b"311", 104);
    let curve_operand = parse_entity_selection_operand(&ctx, &curve_identity, &group, 0, &record)
        .expect("expanded Sketch-curve entity-selection frame")
        .unwrap();
    assert_eq!(curve_operand.primary_identity, 1331);
    assert_eq!(
        curve_operand
            .secondary()
            .map(|secondary| secondary.identity.value),
        Some(183)
    );
    assert_eq!(
        curve_operand
            .secondary()
            .and_then(|secondary| secondary.curve_identity)
            .map(|identity| identity.value),
        Some(77)
    );
    assert_eq!(
        curve_operand
            .secondary()
            .and_then(|secondary| secondary.curve_identity)
            .map(|identity| identity.offset),
        Some(u64_from_index(identity_at) + 21)
    );
    assert_eq!(
        curve_operand.next_byte_offset(),
        u64_from_index(curve_next_at)
    );

    let mut class_338_curve_identity = bytes[..identity_at].to_vec();
    class_338_curve_identity[4..7].copy_from_slice(b"338");
    indexed_header(&mut class_338_curve_identity, *b"361", 103);
    class_338_curve_identity.extend_from_slice(&[0; 9]);
    class_338_curve_identity.push(1);
    class_338_curve_identity.extend_from_slice(&[0; 12]);
    class_338_curve_identity.extend_from_slice(&949u32.to_le_bytes());
    class_338_curve_identity.extend_from_slice(&0u32.to_le_bytes());
    class_338_curve_identity.extend_from_slice(&249u32.to_le_bytes());
    class_338_curve_identity.extend_from_slice(&0u32.to_le_bytes());
    let class_338_next_at = class_338_curve_identity.len();
    indexed_header(&mut class_338_curve_identity, *b"268", 104);
    let class_338_record = DesignRecordHeader {
        class_tag: crate::records::references::DesignClassTag::try_from("338".to_owned()).unwrap(),
        ..record
    };
    let class_338_operand = parse_entity_selection_operand(
        &ctx,
        &class_338_curve_identity,
        &group,
        0,
        &class_338_record,
    )
    .expect("class-338 Sketch-curve entity-selection frame")
    .unwrap();
    assert_eq!(class_338_operand.primary_identity, 949);
    assert_eq!(
        class_338_operand
            .secondary()
            .map(|secondary| secondary.identity.value),
        Some(249)
    );
    assert_eq!(
        class_338_operand
            .secondary()
            .and_then(|secondary| secondary.curve_identity)
            .map(|identity| identity.value),
        None
    );
    assert_eq!(
        class_338_operand.primary_identity_offset(),
        u64_from_index(identity_at) + 33
    );
    assert_eq!(
        class_338_operand
            .secondary()
            .map(|secondary| secondary.identity.offset),
        Some(u64_from_index(identity_at) + 41)
    );
    assert_eq!(
        class_338_operand.next_byte_offset(),
        u64_from_index(class_338_next_at)
    );

    let mut invalid_class_338 = class_338_curve_identity.clone();
    invalid_class_338[identity_at + 20] = 0;
    assert!(
        parse_entity_selection_operand(&ctx, &invalid_class_338, &group, 0, &class_338_record)
            .is_none()
    );

    let stream_name = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let archive = crate::test_support::zip_test::f3d_with_configuration(
        &crate::test_support::smbh_header_test::synthetic_smbh(),
        stream_name,
        &bytes,
    );
    let mut group = group;
    group.id = crate::ids::native_scoped_id(stream_name, "operand-group", 90);
    let header = DesignRecordHeader {
        id: crate::ids::native_scoped_id(stream_name, "record", 100),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("333".to_owned()).unwrap(),
        record_index: 100,
    };
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_collection_items = 1;
        let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "f3d entity selection operand output",
            |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match cadmpeg_core::decode::ResourceDimension::CollectionItems {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) =
                    cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                        .unwrap();
                (crate::design::decode::operands::decode_entity_selection_operands(
                    &ctx,
                    scan,
                    std::slice::from_ref(&group),
                    std::slice::from_ref(&header),
                ))
                .map(|_| ())
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match cadmpeg_core::decode::ResourceDimension::CollectionItems {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "f3d entity selection operand output",
            |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match cadmpeg_core::decode::ResourceDimension::CollectionItems {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) =
                    cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                        .unwrap();
                (crate::design::decode::operands::decode_entity_selection_operands(
                    &ctx,
                    scan,
                    std::slice::from_ref(&group),
                    std::slice::from_ref(&header),
                ))
                .map(|_| ())
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match cadmpeg_core::decode::ResourceDimension::CollectionItems {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            crate::design::decode::operands::decode_entity_selection_operands(
                &ctx, scan, std::slice::from_ref(&group), std::slice::from_ref(&header),
            ),
            Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                if failure.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                    && failure.operation == "f3d entity selection operand output"
        ));
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        let id_len = crate::ids::native_scope(stream_name).len()
            + ":design-entity-selection-operand#".len()
            + 1;
        policy.limits.max_retained_bytes = u64::try_from(72 + id_len - 1).unwrap();
        let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            "f3d entity selection operand ID",
            |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) =
                    cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                        .unwrap();
                (crate::design::decode::operands::decode_entity_selection_operands(
                    &ctx,
                    scan,
                    std::slice::from_ref(&group),
                    std::slice::from_ref(&header),
                ))
                .map(|_| ())
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            "f3d entity selection operand ID",
            |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) =
                    cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                        .unwrap();
                (crate::design::decode::operands::decode_entity_selection_operands(
                    &ctx,
                    scan,
                    std::slice::from_ref(&group),
                    std::slice::from_ref(&header),
                ))
                .map(|_| ())
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            crate::design::decode::operands::decode_entity_selection_operands(
                &ctx, scan, std::slice::from_ref(&group), std::slice::from_ref(&header),
            ),
            Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                if failure.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                    && failure.operation == "f3d entity selection operand ID"
        ));
    });
}

#[test]
fn edge_identity_text_refuses_retained_limit() {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"278", 5887);
    bytes.extend_from_slice(&[0; 12]);
    bytes.push(1);
    bytes.extend_from_slice(&5890u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "ad3001bb-a0fc-44c2-9b7a-c8b8fb70bfc0");
    lp_utf16(&mut bytes, "1d8b67fc-c638-4af3-b13d-776dce4f472d");

    for limit in [35, 71] {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = limit;

        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            crate::design::decode::operands::parse_edge_identity_member(&ctx, &bytes, 0),
            Some(Err(cadmpeg_core::CodecError::ResourceLimit(failure)))
                if failure.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                    && failure.operation == "f3d Design UTF-16 text"
        ));
    }
}

fn counted_extrude_selection_fixture() -> (Vec<u8>, DesignParameterScope, DesignRecordHeader) {
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat",
        crate::records::feature::scope::DesignFeatureKind::Extrude,
        12,
    );
    let record = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("331".to_owned()).unwrap(),
        record_index: 100,
    };
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"331", 100);
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for member in [200u32, 201] {
        bytes.push(1);
        bytes.extend_from_slice(&member.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&180u32.to_le_bytes());
    bytes.extend_from_slice(&0.25f64.to_le_bytes());
    bytes.extend_from_slice(&180u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&102u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&[1, 1, 0, 1]);
    bytes.extend_from_slice(&101u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 7]);
    bytes.push(1);
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    indexed_header(&mut bytes, *b"259", 100);
    (bytes, scope, record)
}

#[test]
fn extrude_selection_group_member_copies_refuse_collection_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let (bytes, scope, record) = counted_extrude_selection_fixture();
    for (limit, operation) in [
        (1, "parse F3D extrude selection members"),
        (3, "parse F3D extrude selection member offsets"),
        (5, "index F3D extrude selection members"),
        (7, "admit F3D extrude selection members"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            parse_extrude_selection_group(&ctx, &bytes, &scope, 0, &record),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == operation
        ));
    }
}

#[test]
fn extrude_selection_group_output_refuses_collection_and_id_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let (bytes, scope, record) = counted_extrude_selection_fixture();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let group = parse_extrude_selection_group(&ctx, &bytes, &scope, 0, &record)
        .unwrap()
        .expect("counted Extrude selection group");
    let stream = "Design/BulkStream.dat";
    let native_scope_len = u64::try_from(crate::ids::native_scope(stream).len()).unwrap();
    for (collection_limit, retained_limit, dimension, operation) in [
        (
            0,
            u64::MAX,
            ResourceDimension::CollectionItems,
            "f3d extrude selection group output",
        ),
        (
            1,
            native_scope_len,
            ResourceDimension::RetainedBytes,
            "f3d extrude selection group ID",
        ),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = collection_limit;
        policy.limits.max_retained_bytes = retained_limit;
        let refusal_cap =
            match cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match dimension {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let mut out = Vec::new();
                crate::design::decode::operands::push_extrude_selection_group(
                    &ctx,
                    &mut out,
                    group.clone(),
                    stream,
                    0,
                )
            }) {
                cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
                error => panic!("unexpected refusal: {error:?}"),
            };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match dimension {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let refusal_cap =
            match cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match dimension {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let mut out = Vec::new();
                crate::design::decode::operands::push_extrude_selection_group(
                    &ctx,
                    &mut out,
                    group.clone(),
                    stream,
                    0,
                )
            }) {
                cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
                error => panic!("unexpected refusal: {error:?}"),
            };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match dimension {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut out = Vec::new();
        assert!(matches!(
            crate::design::decode::operands::push_extrude_selection_group(
                &ctx, &mut out, group.clone(), stream, 0,
            ),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == dimension && failure.operation == operation
        ));
        assert!(out.is_empty());
    }
}

fn region_member(bytes: &mut Vec<u8>, curve_primary_id: u32, incidence: [u32; 3]) {
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&curve_primary_id.to_le_bytes());
    bytes.extend_from_slice(&[0; 12]);
    for word in incidence {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.extend_from_slice(&[0; 8]);
}

fn region_selection_frame() -> (Vec<u8>, usize, usize, usize) {
    let profile_record_index = 100;
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"266", profile_record_index);
    indexed_header(&mut bytes, *b"263", profile_record_index + 1);
    indexed_header(&mut bytes, *b"259", profile_record_index + 2);
    let selection_at = bytes.len();
    indexed_header(&mut bytes, *b"327", profile_record_index + 3);
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&profile_record_index.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    region_member(&mut bytes, 70, [1, 1, 1]);
    let second_region_marker = bytes.len();
    bytes.push(1);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    region_member(&mut bytes, 80, [0, 1, 2]);
    region_member(&mut bytes, 81, [0, 2, 2]);
    let terminator = bytes.len();
    bytes.extend_from_slice(&[0; 5]);
    indexed_header(&mut bytes, *b"261", profile_record_index + 3);
    (bytes, selection_at, second_region_marker, terminator)
}

#[test]
fn sketch_profile_regions_and_members_refuse_collection_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let (bytes, _, _, _) = region_selection_frame();
    for (limit, operation) in [
        (1, "f3d sketch profile regions"),
        (2, "f3d sketch profile region members"),
        (4, "f3d sketch profile region members"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            parse_sketch_profile_region_selection(&ctx, &bytes, 100, 0).transpose(),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == operation
        ));
    }
}

#[test]
fn sketch_profile_region_selection_preserves_region_and_curve_order() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (bytes, selection_at, _, _) = region_selection_frame();
    let selection = parse_sketch_profile_region_selection(&ctx, &bytes, 100, 0)
        .transpose()
        .unwrap()
        .expect("profile-region selection");
    assert_eq!(selection.record_index, 103);
    assert_eq!(selection.byte_offset, u64_from_index(selection_at));
    assert_eq!(selection.class_tag.as_str(), "327");
    assert_eq!(selection.companion_class_tag.as_str(), "261");
    assert_eq!(
        selection
            .regions
            .iter()
            .map(|region| {
                region
                    .members
                    .iter()
                    .map(|member| member.curve_primary_id.get())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>(),
        [vec![70], vec![80, 81]]
    );
}

#[test]
fn sketch_profile_region_selection_requires_every_delimiter() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (bytes, _, second_region_marker, terminator) = region_selection_frame();
    for offset in [second_region_marker, terminator] {
        let mut changed = bytes.clone();
        changed[offset] = 2;
        assert_eq!(
            parse_sketch_profile_region_selection(&ctx, &changed, 100, 0)
                .transpose()
                .unwrap(),
            None
        );
    }
}

#[test]
fn sketch_profile_region_selection_derives_companion_after_header_shaped_member() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (mut bytes, selection_at, _, _) = region_selection_frame();
    let curve_primary_id_offset = selection_at + 48;
    bytes[curve_primary_id_offset..curve_primary_id_offset + 4].copy_from_slice(b"123X");

    let selection = parse_sketch_profile_region_selection(&ctx, &bytes, 100, 0)
        .transpose()
        .unwrap()
        .expect("profile-region selection");

    assert_eq!(
        u64::from(selection.regions[0].members[0].curve_primary_id.get()),
        u64::from(u32::from_le_bytes(*b"123X"))
    );
}

mod counted_frames;
