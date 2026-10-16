// SPDX-License-Identifier: Apache-2.0
//! XREF and BREP-less document tests.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use cadmpeg_core::decode::u64_from_index;

use cadmpeg_test_support::EditableDecodeResult;

use std::io::{Cursor, Write};

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use zip::CompressionMethod;

use crate::loss::F3dLossCode;
use crate::test_support::assembly_test::{
    f3d_with_redirections_json, f3d_without_brep, redirections_json, XREF_ROLE,
};
use crate::test_support::manifest_test::write_synthetic_manifests;
use crate::test_support::native_test::f3d_native;
use crate::test_support::streams_test::design_metastream_with_records;
use crate::test_support::{cross_document_reference, local_reference};
use crate::F3dCodec;

use super::OccurrencePlacement;

const EPS_PLACEMENT_TRANSLATION: f64 = 1.0e-12;

fn redirections_limit_context(
    arena: &cadmpeg_core::decode::DecodeArena,
    max_collection_items: u64,
) -> cadmpeg_core::decode::DecodeContext<'_> {
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = max_collection_items;
    cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], arena, &policy)
        .unwrap()
        .0
}

fn with_docstruct_scan(
    f: impl FnOnce(&cadmpeg_core::decode::DecodeArena, &crate::container::ContainerScan<'_>),
) {
    let bytes = f3d_without_brep("assembly-design", "root.f3d", &[]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let scan = crate::container::scan(&ctx, root).unwrap();
    f(&arena, &scan);
}

#[test]
fn docstruct_json_refuses_materialized_limit() {
    with_docstruct_scan(|arena, scan| {
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        let ctx = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], arena, &policy)
            .unwrap()
            .0;
        let error = super::docstruct(&ctx, scan).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "parse F3D properties JSON")
        );
    });
}

#[test]
fn docstruct_json_refuses_collection_limit() {
    with_docstruct_scan(|arena, scan| {
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let ctx = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], arena, &policy)
            .unwrap()
            .0;
        let error = super::docstruct(&ctx, scan).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "parse F3D properties JSON")
        );
    });
}

#[test]
fn docstruct_json_refuses_recursion_limit() {
    with_docstruct_scan(|arena, scan| {
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_recursion_depth = 0;
        let ctx = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], arena, &policy)
            .unwrap()
            .0;
        let error = super::docstruct(&ctx, scan).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "parse F3D properties JSON")
        );
    });
}

#[test]
fn docstruct_type_refuses_retained_limit() {
    with_docstruct_scan(|arena, scan| {
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let ctx = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], arena, &policy)
            .unwrap()
            .0;
        let error = super::docstruct(&ctx, scan).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "copy F3D docstruct type")
        );
    });
}

#[test]
fn docstruct_subtype_refuses_retained_limit() {
    with_docstruct_scan(|arena, scan| {
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = u64_from_index("assembly-design".len());
        let ctx = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], arena, &policy)
            .unwrap()
            .0;
        let error = super::docstruct(&ctx, scan).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "copy F3D docstruct subtype")
        );
    });
}

#[test]
fn redirections_design_collection_refuses_limit() {
    let bytes = redirections_json("root.f3d", &[("part.f3d", "role")]);
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "admit F3D xref designs",
        0,
        |ctx| super::parse(ctx, bytes.as_bytes()),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "admit F3D xref designs")
    );
}

#[test]
fn redirections_json_refuses_materialized_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let ctx = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .unwrap()
        .0;
    let bytes = redirections_json("root.f3d", &[]);
    let error = super::parse(&ctx, bytes.as_bytes()).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "parse F3D redirections JSON")
    );
}

#[test]
fn redirections_reference_collection_refuses_limit() {
    let bytes = redirections_json("root.f3d", &[("part.f3d", "role")]);
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "admit F3D xref references",
        0,
        |ctx| super::parse(ctx, bytes.as_bytes()),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "admit F3D xref references")
    );
}

#[test]
fn redirections_design_id_refuses_retained_limit() {
    let bytes = redirections_json("root.f3d", &[]);
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D xref record ID",
        0,
        |ctx| super::parse(ctx, bytes.as_bytes()),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D xref record ID")
    );
}

#[test]
fn redirections_reference_id_refuses_retained_limit() {
    let bytes = redirections_json("root.f3d", &[("part.f3d", "role")]);
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D xref record ID",
        2,
        |ctx| super::parse(ctx, bytes.as_bytes()),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D xref record ID")
    );
}

#[test]
fn xref_occurrence_id_refuses_retained_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let ctx = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .unwrap()
        .0;
    let error = ctx
        .format_retained(
            format_args!("f3d:xref:reference#{}-occurrence-{}", 0, 0),
            "retain F3D xref record ID",
        )
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D xref record ID")
    );
}

#[test]
fn xref_occurrence_projection_refuses_collection_limit() {
    let bytes = redirections_json("root.f3d", &[("part.f3d", "role")]);
    let table = super::parse(
        &cadmpeg_test_support::service_decode_context(),
        bytes.as_bytes(),
    )
    .unwrap();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let ctx = redirections_limit_context(&arena, 0);
    let error = super::project_occurrences(&ctx, &table).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "project F3D xref occurrence")
    );
}

#[test]
fn xref_occurrence_path_refuses_retained_limit() {
    let bytes = redirections_json("root.f3d", &[("part.f3d", "role")]);
    let table = super::parse(
        &cadmpeg_test_support::service_decode_context(),
        bytes.as_bytes(),
    )
    .unwrap();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy F3D xref path",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let ctx = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                .unwrap()
                .0;
            super::project_occurrences(&ctx, &table).map(|_| ())
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let ctx = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .unwrap()
        .0;
    let error = super::project_occurrences(&ctx, &table).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D xref path")
    );
}

#[test]
fn xref_occurrence_native_reference_refuses_retained_limit() {
    let bytes = redirections_json("root.f3d", &[("part.f3d", "role")]);
    let table = super::parse(
        &cadmpeg_test_support::service_decode_context(),
        bytes.as_bytes(),
    )
    .unwrap();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy F3D xref native reference",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let ctx = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                .unwrap()
                .0;
            super::project_occurrences(&ctx, &table).map(|_| ())
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let ctx = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .unwrap()
        .0;
    let error = super::project_occurrences(&ctx, &table).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D xref native reference")
    );
}

#[test]
fn xref_record_frame_index_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let ctx = redirections_limit_context(&arena, 0);
    let mut bytes = vec![0; 15];
    bytes[..4].copy_from_slice(&3_u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"123");
    let error = super::indexed_records(&ctx, &bytes).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D xref record frame")
    );
}

#[test]
fn xref_record_frame_scan_preserves_boundaries() {
    let mut bytes = vec![0; 35];
    bytes[..4].copy_from_slice(&3_u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"123");
    bytes[16..20].copy_from_slice(&3_u32.to_le_bytes());
    bytes[20..23].copy_from_slice(b"456");
    let frames =
        super::indexed_records(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap();
    assert_eq!(frames.len(), 2);
    assert_eq!((frames[0].offset, frames[0].end), (0, 16));
    assert_eq!((frames[1].offset, frames[1].end), (16, 35));
}

fn one_typed_placement_metastream() -> crate::metastream::MetaStream {
    let bytes = design_metastream_with_records(
        &[(
            super::OCCURRENCE_PLACEMENT_TYPE_GUID,
            "",
            2,
            "Component",
            &[10],
        )],
        &[(10, 0)],
    );
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    crate::metastream::parse(&ctx, &bytes, "typed-placement-test").unwrap()
}

#[test]
fn xref_placement_entity_index_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let ctx = redirections_limit_context(&arena, 0);
    let error = super::typed_occurrence_placement_offsets(&ctx, &one_typed_placement_metastream())
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D xref placement entities")
    );
}

#[test]
fn xref_placement_offset_index_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let ctx = redirections_limit_context(&arena, 1);
    let error = super::typed_occurrence_placement_offsets(&ctx, &one_typed_placement_metastream())
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D xref placement offsets")
    );
}

#[test]
fn xref_structured_transform_selection_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let ctx = redirections_limit_context(&arena, 0);
    let placement = OccurrencePlacement {
        link_names: vec!["role".into()],
        transform: None,
    };
    let error = super::occurrence_transforms(&ctx, &[placement], "role").unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "select F3D structured xref transforms")
    );
}

#[test]
fn xref_direct_transform_conversion_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let ctx = redirections_limit_context(&arena, 0);
    let direct = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let error =
        super::occurrence_transforms_with_precedence(&ctx, vec![direct], &[], "role").unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "select F3D direct xref transforms")
    );
}

#[test]
fn xref_component_insert_selection_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let ctx = redirections_limit_context(&arena, 0);
    let construction =
        crate::records::feature::assembly_features::DesignComponentInsertConstruction {
            relation_record_index: 1,
            carrier_record_index: 2,
            occurrence_identity: None,
            neutron_role: "role".into(),
            neutron_role_offset: 0,
            placement: None,
        };
    let error = super::select_component_insert_transforms(
        &ctx,
        [("stream", &construction)],
        "stream",
        "role",
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "select F3D component insert transforms")
    );
}

#[test]
fn xref_occurrence_transform_aggregation_refuses_collection_limit() {
    let identity = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let archive = crate::test_support::assembly_test::f3d_without_brep_with_xref_placement(
        "assembly-design",
        "root.f3d",
        "part.f3d",
        XREF_ROLE,
        identity,
    );
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (scan_ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&archive, &arena, &policy).unwrap();
    let scan = crate::container::scan(&scan_ctx, root).unwrap();
    let table_bytes = redirections_json("root.f3d", &[("part.f3d", XREF_ROLE)]);
    let mut table = super::parse(&scan_ctx, table_bytes.as_bytes()).unwrap();
    let limit_arena = cadmpeg_core::decode::DecodeArena::new();
    let limit_ctx = redirections_limit_context(&limit_arena, 12);
    let error = super::bind_occurrences(&limit_ctx, &scan, &mut table, &[]).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D xref occurrence transforms")
    );
}

#[test]
fn xref_stream_collection_refuses_collection_limit() {
    let identity = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let archive = crate::test_support::assembly_test::f3d_without_brep_with_xref_placement(
        "assembly-design",
        "root.f3d",
        "part.f3d",
        XREF_ROLE,
        identity,
    );
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (scan_ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&archive, &arena, &policy).unwrap();
    let scan = crate::container::scan(&scan_ctx, root).unwrap();
    let table_bytes = redirections_json("root.f3d", &[("part.f3d", XREF_ROLE)]);
    let mut table = super::parse(&scan_ctx, table_bytes.as_bytes()).unwrap();
    let limit_arena = cadmpeg_core::decode::DecodeArena::new();
    let limit_ctx = redirections_limit_context(&limit_arena, 10);
    let error = super::bind_occurrences(&limit_ctx, &scan, &mut table, &[]).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D xref streams")
    );
}

#[test]
fn xref_stream_scope_refuses_retained_limit() {
    let identity = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let archive = crate::test_support::assembly_test::f3d_without_brep_with_xref_placement(
        "assembly-design",
        "root.f3d",
        "part.f3d",
        XREF_ROLE,
        identity,
    );
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (scan_ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&archive, &arena, &policy).unwrap();
    let scan = crate::container::scan(&scan_ctx, root).unwrap();
    let table_bytes = redirections_json("root.f3d", &[("part.f3d", XREF_ROLE)]);
    let mut table = super::parse(&scan_ctx, table_bytes.as_bytes()).unwrap();
    let limit_arena = cadmpeg_core::decode::DecodeArena::new();
    let mut limit_policy = cadmpeg_core::decode::DecodePolicy::service();
    // MetaStream parsing and serializer lookup retain their header fields.

    // Both placement attempts retain one class tag and two copies of the role.

    limit_policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D native scope",
        |cap| {
            let mut table = table.clone();
            let mut limit_policy = cadmpeg_core::decode::DecodePolicy::service();
            // MetaStream parsing and serializer lookup retain their header fields.

            // Both placement attempts retain one class tag and two copies of the role.

            limit_policy.limits.max_retained_bytes = cap;
            let limit_ctx = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &[],
                &limit_arena,
                &limit_policy,
            )
            .unwrap()
            .0;
            super::bind_occurrences(&limit_ctx, &scan, &mut table, &[])
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let limit_ctx =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &limit_arena, &limit_policy)
            .unwrap()
            .0;
    let error = super::bind_occurrences(&limit_ctx, &scan, &mut table, &[]).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D native scope")
    );
}

#[test]
fn xref_expanded_reference_refuses_collection_limit() {
    let archive = f3d_without_brep("assembly-design", "root.f3d", &[("part.f3d", XREF_ROLE)]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (scan_ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&archive, &arena, &policy).unwrap();
    let scan = crate::container::scan(&scan_ctx, root).unwrap();
    let table_bytes = redirections_json("root.f3d", &[("part.f3d", XREF_ROLE)]);
    let mut table = super::parse(&scan_ctx, table_bytes.as_bytes()).unwrap();
    let limit_arena = cadmpeg_core::decode::DecodeArena::new();
    let limit_ctx = redirections_limit_context(&limit_arena, 0);
    let error = super::bind_occurrences(&limit_ctx, &scan, &mut table, &[]).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "expand F3D xref references")
    );
}

#[test]
fn xref_reference_copy_refuses_retained_limit() {
    let archive = f3d_without_brep("assembly-design", "root.f3d", &[("part.f3d", XREF_ROLE)]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (scan_ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&archive, &arena, &policy).unwrap();
    let scan = crate::container::scan(&scan_ctx, root).unwrap();
    let table_bytes = redirections_json("root.f3d", &[("part.f3d", XREF_ROLE)]);
    let mut table = super::parse(&scan_ctx, table_bytes.as_bytes()).unwrap();
    let limit_arena = cadmpeg_core::decode::DecodeArena::new();
    let mut limit_policy = cadmpeg_core::decode::DecodePolicy::service();
    limit_policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy F3D xref reference",
        |cap| {
            let mut table = table.clone();
            let mut limit_policy = cadmpeg_core::decode::DecodePolicy::service();
            limit_policy.limits.max_retained_bytes = cap;
            let limit_ctx = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &[],
                &limit_arena,
                &limit_policy,
            )
            .unwrap()
            .0;
            super::bind_occurrences(&limit_ctx, &scan, &mut table, &[])
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let limit_ctx =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &limit_arena, &limit_policy)
            .unwrap()
            .0;
    let error = super::bind_occurrences(&limit_ctx, &scan, &mut table, &[]).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D xref reference")
    );
}

#[test]
fn xref_placement_collection_refuses_collection_limit() {
    let bytes = occurrence_record("role", 10, &[1], None);
    let records =
        super::indexed_records(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let ctx = redirections_limit_context(&arena, 2);
    let error =
        super::occurrence_placements_with_failures(&ctx, &bytes, &records, None, None).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D xref placements")
    );
}

#[test]
fn xref_placement_failure_collection_refuses_collection_limit() {
    let mut bytes = occurrence_record("role", 10, &[1], None);
    bytes.pop();
    let records =
        super::indexed_records(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let ctx = redirections_limit_context(&arena, 3);
    let error =
        super::occurrence_placements_with_failures(&ctx, &bytes, &records, None, None).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D xref placement failures")
    );
}

#[test]
fn xref_legacy_failure_role_refuses_collection_limit() {
    let mut bytes = legacy_occurrence_record("aaaabbbb-cccc-dddd-eeee-ffff00001111", 10, None);
    bytes.pop();
    let records =
        super::indexed_records(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let ctx = redirections_limit_context(&arena, 0);
    let error =
        super::occurrence_placements_with_failures(&ctx, &bytes, &records, None, None).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D legacy xref role")
    );
}

#[test]
fn redirections_keep_neutron_role_and_data_independent() {
    let table = super::parse(&cadmpeg_test_support::service_decode_context(),
        br#"{"name":"RedirectionsStream","schema-version":0,"designs":[{"file-version":1,"targetFileName":"root.f3d","displayName":"root","lineageUrn":"urn:l","versionUrn":"urn:v"}],"references":[{"from":"root.f3d","relativePath":"part.f3d","type":"XREF","properties":[{"neutronRole":{"value":"role-guid","dataType":"STRING"}},{"neutronData":{"value":"data-guid","dataType":"STRING"}}]}]}"#,
    )
    .expect("redirections JSON");
    assert_eq!(table.references.len(), 1);
    assert_eq!(table.references[0].neutron_role.as_str(), "role-guid");
    assert_eq!(table.references[0].neutron_data, "data-guid");
}

#[test]
fn malformed_redirections_shapes_are_not_admitted_as_leaf_tables() {
    let cases: [&[u8]; 5] = [
        br#"{"name":"RedirectionsStream","schema-version":0,"designs":[{"file-version":1,"targetFileName":"root.f3d","displayName":"root","lineageUrn":"urn:l","versionUrn":"urn:v"}],"references":null}"#,
        br#"{"name":"RedirectionsStream","schema-version":0,"designs":[{"file-version":1,"targetFileName":"root.f3d","displayName":"root","lineageUrn":"urn:l","versionUrn":"urn:v"}],"references":{"unexpected":1}}"#,
        br#"{"name":"RedirectionsStream","schema-version":0,"designs":[{"file-version":1,"targetFileName":"root.f3d","displayName":"root","lineageUrn":"urn:l","versionUrn":"urn:v"}],"references":[{}]}"#,
        br#"{"name":"RedirectionsStream","schema-version":0,"designs":[{"file-version":1,"targetFileName":"root.f3d","displayName":"root","lineageUrn":"urn:l","versionUrn":"urn:v"}],"references":[{"from":"root.f3d","relativePath":"part.f3d","type":"XREF","properties":[]}]}"#,
        br#"{"name":"RedirectionsStream","schema-version":0,"designs":[{"file-version":1,"targetFileName":"root.f3d","displayName":"root","lineageUrn":"urn:l","versionUrn":"urn:v"}],"references":[{"from":"root.f3d","relativePath":"part.f3d","type":"XREF","properties":[{"neutronRole":{"value":"role","dataType":"NUMBER"}},{"neutronData":{"value":"data","dataType":"STRING"}}]}]}"#,
    ];
    for bytes in cases {
        assert!(
            super::parse(&cadmpeg_test_support::service_decode_context(), bytes).is_err(),
            "malformed table admitted: {bytes:?}"
        );
    }
}

#[test]
fn malformed_redirections_are_reported_by_complete_and_container_routes() {
    let redirections = br#"{"name":"RedirectionsStream","schema-version":0,"designs":[{"file-version":1,"targetFileName":"root.f3d","displayName":"root","lineageUrn":"urn:l","versionUrn":"urn:v"}],"references":[{"from":"root.f3d","relativePath":"part.f3d","type":"XREF","properties":[]}] }"#;
    for options in [
        DecodeOptions::default(),
        DecodeOptions {
            container_only: true,
            ..DecodeOptions::default()
        },
    ] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_redirections_json("assembly-design", redirections)),
                &options,
            )
            .expect("malformed Redirections remains a reported decode");
        let loss = decoded
            .report()
            .losses
            .iter()
            .find(|loss| loss.code == F3dLossCode::XrefTableUndecoded.kind())
            .expect("malformed Redirections loss");
        assert!(loss.message.contains("RedirectionsStream.dat"), "{loss:?}");
        assert!(loss.message.contains("properties"), "{loss:?}");
    }
}

#[test]
fn redirections_property_keys_are_not_collapsed_before_admission() {
    const ROLE: &str = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let envelope = redirections_json("root.f3d", &[("part.f3d", ROLE)]);
    let (prefix, _) = envelope.split_once(",\"references\":").unwrap();
    let role = format!(r#""neutronRole":{{"value":"{ROLE}","dataType":"STRING"}}"#);
    let data = r#""neutronData":{"value":"independent","dataType":"STRING"}"#;
    let empty_data = r#""neutronData":{"value":"","dataType":"STRING"}"#;
    for (label, properties, valid) in [
        ("role then data", format!("[{{{role}}},{{{data}}}]"), true),
        ("data then role", format!("[{{{data}}},{{{role}}}]"), true),
        ("empty independent data", format!("[{{{role}}},{{{empty_data}}}]"), true),
        ("repeated role key", format!("[{{{role},{role}}},{{{data}}}]"), false),
        ("repeated data key", format!("[{{{role}}},{{{data},{data}}}]"), false),
        ("two different keys", format!("[{{{role},{data}}}]"), false),
        ("repeated role item", format!("[{{{role}}},{{{role}}},{{{data}}}]"), false),
        ("empty property object", format!("[{{}},{{{role}}},{{{data}}}]"), false),
        ("missing role", format!("[{{{data}}}]"), false),
        ("unknown property", format!("[{{\"extension\":{{\"value\":\"x\",\"dataType\":\"STRING\"}}}},{{{role}}},{{{data}}}]"), false),
        ("duplicate type", format!("[{{{role}}},{{\"neutronData\":{{\"value\":\"independent\",\"dataType\":\"STRING\",\"dataType\":\"STRING\"}}}}]"), false),
        ("wrong type", format!("[{{{role}}},{{\"neutronData\":{{\"value\":\"independent\",\"dataType\":\"NUMBER\"}}}}]"), false),
    ] {
        let json = format!(r#"{prefix},"references":[{{"from":"root.f3d","relativePath":"part.f3d","type":"XREF","properties":{properties}}}]}}"#);
        assert_eq!(super::parse(&cadmpeg_test_support::service_decode_context(), json.as_bytes()).is_ok(), valid, "{label}");
        let document = f3d_with_redirections_json("assembly-design", json.as_bytes());
        for container_only in [false, true] {
            let decoded = EditableDecodeResult::from(F3dCodec.decode(
                &mut Cursor::new(&document),
                &DecodeOptions { container_only, ..DecodeOptions::default() },
            ).expect("property refusal preserves the source document"));
            let losses = decoded.report().losses.iter()
                .filter(|loss| loss.code == F3dLossCode::XrefTableUndecoded.kind())
                .collect::<Vec<_>>();
            assert_eq!(losses.len(), usize::from(!valid), "{label}, container_only={container_only}: {losses:?}");
            for loss in losses {
                assert!(loss.message.contains("RedirectionsStream.dat"), "{loss:?}");
            }
            if !container_only {
                let native = f3d_native(decoded.ir());
                assert_eq!(native.xref_references.len(), usize::from(valid), "{label}");
                if valid {
                    assert_eq!(native.xref_references[0].neutron_role.as_str(), ROLE);
                    assert_eq!(native.xref_references[0].neutron_data,
                        if label == "empty independent data" { "" } else { "independent" });
                }
            }
            assert_eq!(decoded.source_fidelity()
                .retained_record(crate::ids::FILE_SOURCE_IMAGE_ID)
                .and_then(|record| record.data()), Some(document.as_slice()), "{label}");
            let _: cadmpeg_ir::CadIr = serde_json::from_slice(
                &serde_json::to_vec(decoded.ir()).unwrap()
            ).expect("complete CADIR admission");
        }
    }
}

#[test]
fn external_reference_placements_project_as_root_occurrences_in_millimetres() {
    let transform = [
        [0.0, -1.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 2.0],
        [0.0, 0.0, 1.0, 3.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let table = super::XrefTable {
        designs: Vec::new(),
        references: vec![crate::records::xref::XrefReference {
            id: "f3d:xref:reference#0-occurrence-0".into(),
            ordinal: 0,
            occurrence_ordinal: 0,
            from: "root.f3d".to_owned().try_into().unwrap(),
            relative_path: "part.f3d".to_owned().try_into().unwrap(),
            neutron_role: "role".to_owned().try_into().unwrap(),
            neutron_data: "data".into(),
            transform: Some(transform.try_into().unwrap()),
        }],
        placement_failures: Vec::new(),
        placement_overrides: Vec::new(),
    };

    let occurrences =
        super::project_occurrences(&cadmpeg_test_support::service_decode_context(), &table)
            .unwrap();

    assert_eq!(occurrences.len(), 1);
    assert_eq!(occurrences[0].id.as_str(), "f3d:model:occurrence#xref-0-0");
    assert_eq!(
        occurrences[0].transform.rows(),
        [
            [0.0, -1.0, 0.0, 10.0],
            [1.0, 0.0, 0.0, 20.0],
            [0.0, 0.0, 1.0, 30.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    );
    assert_eq!(
        occurrences[0].parent,
        cadmpeg_ir::products::OccurrenceParent::Root {}
    );
    assert_eq!(
        occurrences[0].prototype,
        cadmpeg_ir::products::PrototypeReference::External {
            document: cadmpeg_ir::products::ExternalDocument::path("part.f3d"),
            object: None,
        }
    );
}

#[test]
fn external_reference_admission_and_projection_check_affine_transforms() {
    let mut table = super::parse(&cadmpeg_test_support::service_decode_context(),
        br#"{"name":"RedirectionsStream","schema-version":0,"designs":[{"file-version":1,"targetFileName":"root.f3d","displayName":"root","lineageUrn":"urn:l","versionUrn":"urn:v"}],"references":[{"from":"root.f3d","relativePath":"part.f3d","type":"XREF","properties":[{"neutronRole":{"value":"role","dataType":"STRING"}},{"neutronData":{"value":"data","dataType":"STRING"}}]}]}"#,
    ).unwrap();
    let mut rows = cadmpeg_ir::transform::Transform::identity().rows();
    rows[0][3] = 1.0;
    table.references[0].transform = Some(rows.try_into().unwrap());
    let wire = serde_json::to_value(&table.references[0]).unwrap();
    assert_eq!(
        serde_json::from_value::<crate::records::xref::XrefReference>(wire.clone()).unwrap(),
        table.references[0]
    );
    let mut invalid = wire;
    invalid["transform"][3][0] = serde_json::json!(1.0);
    assert!(serde_json::from_value::<crate::records::xref::XrefReference>(invalid).is_err());
    let reflected = serde_json::json!({
        "id": "f3d:xref:reference#0",
        "ordinal": 0,
        "occurrence_ordinal": 0,
        "from": "root.f3d",
        "relative_path": "part.f3d",
        "neutron_role": "role",
        "neutron_data": "data",
        "transform": [
            [-1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0]
        ]
    });
    assert!(serde_json::from_value::<crate::records::xref::XrefReference>(reflected).is_err());
    rows[0][3] = f64::MAX;
    table.references[0].transform = Some(rows.try_into().unwrap());
    assert!(matches!(
        super::project_occurrences(&cadmpeg_test_support::service_decode_context(), &table),
        Err(cadmpeg_core::CodecError::NotImplemented(message))
            if message.contains("finite affine transform")
    ));
}

#[test]
fn reflected_matrix_is_not_a_placement() {
    let reflection = [
        [-1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let bytes = occurrence_record("reflection-role", 10, &[1], Some(reflection));
    let placements = super::occurrence_placements(
        &bytes,
        &super::indexed_records(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap(),
        None,
    );
    assert!(placements.is_empty(), "reflected placement admitted");
}

#[test]
fn reflected_matrix_is_reported_by_complete_document_admission() {
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(document_with_modern_placement(
                role,
                [
                    [-1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ],
            )),
            &DecodeOptions::default(),
        )
        .expect("reflected placement is reported");
    let native = f3d_native(decoded.ir());
    assert_eq!(native.xref_references.len(), 1);
    assert!(native.xref_references[0].transform.is_none());
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::XrefPlacementUndecoded.kind()));
}

#[test]
fn component_reference_data_is_an_open_json_object() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let (value, _reservation) = super::parse_component_reference_data(
        &ctx,
        br#"{"schema":7,"references":[{"id":"component"}],"extension":{"x":true}}"#,
    )
    .expect("open component-reference object");
    assert_eq!(value["schema"], 7);
    assert!(super::parse_component_reference_data(&ctx, br"[]").is_err());
    assert!(super::parse_component_reference_data(&ctx, b"not-json").is_err());
}

#[test]
fn component_reference_data_json_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let ctx = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .unwrap()
        .0;
    let error = super::parse_component_reference_data(&ctx, br#"{"items":[1,2]}"#).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "parse F3D component reference JSON")
    );
}

#[test]
fn invalid_json_utf8_refuses_work_before_validation() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let cadmpeg_core::CodecError::ResourceLimit(limit) =
        super::parse_component_reference_data(&ctx, &[0xff]).unwrap_err()
    else {
        panic!("work admission must precede malformed UTF-8");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "validate F3D JSON UTF-8");
    assert!(ctx.charge_work(0, "fused context").is_err());
}

/// One occurrence-placement record: a target path whose last element
/// carries `role` as its cross-document link name, the identity marker,
/// and the three closing reference runs.
fn occurrence_record(
    role: &str,
    entity_id: u64,
    discriminators: &[u32],
    transform: Option<[[f64; 4]; 4]>,
) -> Vec<u8> {
    occurrence_record_with_serializer_magic(role, entity_id, discriminators, transform, None)
}

#[test]
fn charged_reference_refuses_retained_limit() {
    let bytes = cross_document_reference(7, "aaaabbbb-cccc-dddd-eeee-ffff00001111");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let error = crate::bytes::take_reference_charged(&ctx, &bytes, &mut 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D UTF-16 string")
    );
}

#[test]
fn occurrence_path_refuses_link_collection_limit() {
    let bytes = occurrence_record("aaaabbbb-cccc-dddd-eeee-ffff00001111", 7, &[1], None);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let error = super::occurrence_path(&ctx, &bytes).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D xref placement link names")
    );
}

fn occurrence_record_with_serializer_magic(
    role: &str,
    entity_id: u64,
    discriminators: &[u32],
    transform: Option<[[f64; 4]; 4]>,
    serializer_magic: Option<u32>,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"380");
    bytes.extend_from_slice(&entity_id.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(
        &(u32::try_from(discriminators.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    for (ordinal, discriminator) in discriminators.iter().enumerate() {
        let target = 100 + u64_from_index(ordinal);
        if ordinal + 1 == discriminators.len() {
            bytes.extend(cross_document_reference(target, role));
        } else {
            bytes.extend(local_reference(target));
        }
        bytes.extend_from_slice(&discriminator.to_le_bytes());
    }
    bytes.push(0);
    match transform {
        Some(transform) => {
            bytes.push(0);
            for value in transform.into_iter().flatten() {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        None => bytes.push(1),
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend(local_reference(7));
    if serializer_magic == Some(crate::metastream::MODERN_SERIALIZER_MAGIC) {
        bytes.push(2);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&42_u32.to_le_bytes());
        bytes.extend(local_reference(8));
    }
    bytes.extend(local_reference(3));
    bytes.extend(local_reference(6));
    bytes
}

fn document_with_modern_placement(role: &str, matrix: [[f64; 4]; 4]) -> Vec<u8> {
    let mut placement = occurrence_record_with_serializer_magic(
        role,
        10,
        &[1],
        Some(matrix),
        Some(crate::metastream::MODERN_SERIALIZER_MAGIC),
    );
    placement[4..7].copy_from_slice(b"256");
    let properties =
        br#"{"docstruct":{"version":"1.0.0","type":"assembly-design","subtype":"synthetic","attributes":{}}}"#;
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("Properties.dat", stored).unwrap();
    zip.write_all(
        &(u32::try_from(properties.len()).expect("fixture value fits u32")).to_le_bytes(),
    )
    .unwrap();
    zip.write_all(properties).unwrap();
    zip.start_file("RedirectionsStream.dat", stored).unwrap();
    zip.write_all(redirections_json("root.f3d", &[("part.f3d", role)]).as_bytes())
        .unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&design_metastream_with_records(
        &[(
            super::OCCURRENCE_PLACEMENT_TYPE_GUID,
            "",
            2,
            "Component",
            &[10],
        )],
        &[(10, 0)],
    ))
    .unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&placement).unwrap();
    zip.finish().unwrap().into_inner()
}

fn repeated_target_occurrence_record(
    role: &str,
    entity_id: u64,
    envelope_discriminator: u32,
    transform: Option<[[f64; 4]; 4]>,
) -> Vec<u8> {
    repeated_target_occurrence_record_with_path_role(
        role,
        role,
        entity_id,
        envelope_discriminator,
        transform,
    )
}

fn repeated_target_occurrence_record_with_path_role(
    path_role: &str,
    role: &str,
    entity_id: u64,
    envelope_discriminator: u32,
    transform: Option<[[f64; 4]; 4]>,
) -> Vec<u8> {
    let component_guid = "11111111-2222-3333-4444-555555555555";
    let type_guid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let metadata_guid_a = "66666666-7777-8888-9999-aaaaaaaaaaaa";
    let metadata_guid_b = "bbbbbbbb-cccc-dddd-eeee-ffffffffffff";
    let mut bytes = occurrence_record(path_role, entity_id, &[1], None);
    let path_end = crate::test_support::with_decode_context(|decode_ctx| {
        super::occurrence_path(decode_ctx, &bytes)
    })
    .unwrap()
    .expect("synthetic path")
    .1;
    bytes.truncate(path_end);
    bytes.extend_from_slice(&envelope_discriminator.to_le_bytes());
    bytes.extend(
        crate::bytes::lp_utf16_bytes(metadata_guid_a)
            .expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.extend(
        crate::bytes::lp_utf16_bytes(metadata_guid_b)
            .expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.extend_from_slice(&[0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0]);
    bytes.extend(
        crate::bytes::lp_utf16_bytes(component_guid)
            .expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.push(0);
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    bytes.extend_from_slice(type_guid.as_bytes());
    bytes.extend(
        crate::bytes::lp_utf16_bytes(role).expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.push(0);
    match transform {
        Some(transform) => {
            bytes.push(0);
            for value in transform.into_iter().flatten() {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        None => bytes.push(1),
    }
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend(
        crate::bytes::lp_utf16_bytes(role).expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.push(0);
    bytes.extend(local_reference(3));
    bytes
}

#[test]
fn repeated_target_placements_decode_identity_and_matrix_forms() {
    let matrix = [
        [0.0, -1.0, 0.0, 2.0],
        [1.0, 0.0, 0.0, 3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let identity_1 = repeated_target_occurrence_record(role, 10, 1, None);
    let identity_2 = repeated_target_occurrence_record(role, 11, 2, None);
    let identity_3 = repeated_target_occurrence_record(role, 12, 3, None);
    let matrix_4 = repeated_target_occurrence_record(role, 13, 4, Some(matrix));
    let matrix_5 = repeated_target_occurrence_record(role, 14, 5, Some(matrix));
    let matrix_6 = repeated_target_occurrence_record(role, 15, 6, Some(matrix));
    assert_eq!(identity_1.len(), 695);
    assert_eq!(identity_2.len(), 695);
    assert_eq!(identity_3.len(), 695);
    assert_eq!(matrix_4.len(), 823);
    assert_eq!(matrix_5.len(), 823);
    assert_eq!(matrix_6.len(), 823);
    let (matrix_role, _, matrix_offset) = super::repeated_target_component_insert(
        &cadmpeg_test_support::service_decode_context(),
        &matrix_6,
        0,
        matrix_6.len(),
        15,
        matrix,
    )
    .unwrap()
    .expect("matrix carrier matching the scope transform");
    assert_eq!(matrix_role, role);
    assert!(matrix_offset.is_some());
    let mut bytes = identity_1;
    bytes.extend(identity_2);
    bytes.extend(identity_3);
    bytes.extend(matrix_4);
    bytes.extend(matrix_5);
    bytes.extend(matrix_6);

    let placements = super::occurrence_placements(
        &bytes,
        &super::indexed_records(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap(),
        None,
    );

    assert_eq!(placements.len(), 6);
    assert_eq!(
        super::occurrence_transforms(
            &cadmpeg_test_support::service_decode_context(),
            &placements,
            role
        )
        .unwrap(),
        vec![None, None, None, Some(matrix), Some(matrix), Some(matrix)]
    );

    let retained_role = format!("{role}_urn:example:component");
    let path_role = "11112222-3333-4444-5555-666677778888_urn:example:path-component";
    let local_carrier =
        repeated_target_occurrence_record_with_path_role(path_role, &retained_role, 16, 1, None);
    let (decoded_role, role_offset, transform_offset) = super::repeated_target_component_insert(
        &cadmpeg_test_support::service_decode_context(),
        &local_carrier,
        0,
        local_carrier.len(),
        16,
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    )
    .unwrap()
    .expect("identity carrier with an independent retained role");
    assert_eq!(decoded_role, retained_role);
    assert_eq!(transform_offset, None);
    let encoded_role = crate::bytes::lp_utf16_bytes(&retained_role)
        .expect("fixture UTF-16 code-unit count fits u32");
    assert_eq!(
        &local_carrier[role_offset - 4..role_offset - 4 + encoded_role.len()],
        encoded_role
    );
}

fn grouped_identity_carrier(role: &str, record_index: u32) -> Vec<u8> {
    let component_guid = "11111111-2222-3333-4444-555555555555";
    let type_guid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let metadata_guid_a = "66666666-7777-8888-9999-aaaaaaaaaaaa";
    let metadata_guid_b = "bbbbbbbb-cccc-dddd-eeee-ffffffffffff";
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"382");
    bytes.extend_from_slice(&record_index.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&17_u64.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend(
        crate::bytes::lp_utf16_bytes(component_guid)
            .expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.push(0);
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    bytes.extend_from_slice(type_guid.as_bytes());
    bytes.extend(
        crate::bytes::lp_utf16_bytes(role).expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.extend_from_slice(&[0, 1, 0, 0, 0, 0, 1, 0, 0, 0]);
    bytes.extend(
        crate::bytes::lp_utf16_bytes(metadata_guid_a)
            .expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.extend(
        crate::bytes::lp_utf16_bytes(metadata_guid_b)
            .expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.extend_from_slice(&[0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0]);
    bytes.extend(
        crate::bytes::lp_utf16_bytes(component_guid)
            .expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.push(0);
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    bytes.extend_from_slice(type_guid.as_bytes());
    bytes.extend(
        crate::bytes::lp_utf16_bytes(role).expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.extend_from_slice(&[0, 1, 0, 0, 0, 0]);
    bytes.extend(
        crate::bytes::lp_utf16_bytes(role).expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.extend_from_slice(&[0, 1, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(bytes.len(), 695);
    bytes
}

#[test]
fn grouped_identity_carriers_decode_as_identity_placements() {
    let role = "cccccccc-dddd-eeee-ffff-000000000000";
    let bytes = grouped_identity_carrier(role, 10);
    let placements = super::occurrence_placements(
        &bytes,
        &super::indexed_records(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap(),
        None,
    );

    assert_eq!(
        placements,
        vec![OccurrencePlacement {
            link_names: vec![role.into()],
            transform: None,
        }]
    );
}

fn legacy_occurrence_reference(target: u64, identity: u64) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend_from_slice(&target.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&identity.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    bytes.extend_from_slice(b"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    bytes.push(0);
    bytes
}

/// One legacy typed placement envelope. The target-reference fields are
/// deliberately built from independent values; only the role and transform
/// are projected by the placement reader.
fn legacy_occurrence_record(
    role: &str,
    entity_id: u64,
    transform: Option<[[f64; 4]; 4]>,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"380");
    bytes.extend_from_slice(&entity_id.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend(legacy_occurrence_reference(3, 0x0102_0304_0506_0708));
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend(
        crate::bytes::lp_utf16_bytes("11111111-2222-3333-4444-555555555555")
            .expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.extend(
        crate::bytes::lp_utf16_bytes("66666666-7777-8888-9999-aaaaaaaaaaaa")
            .expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.push(0);
    bytes.extend(legacy_occurrence_reference(3, 0x1112_1314_1516_1718));
    match transform {
        Some(transform) => {
            bytes.push(0);
            for value in transform.into_iter().flatten() {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        None => bytes.push(1),
    }
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend(
        crate::bytes::lp_utf16_bytes(role).expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.extend_from_slice(&[0, 1, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    bytes
}

#[test]
fn legacy_typed_placements_decode_identity_and_matrix_forms() {
    let matrix = [
        [0.0, -1.0, 0.0, 2.0],
        [1.0, 0.0, 0.0, 3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let identity = legacy_occurrence_record(role, 10, None);
    let matrix_record = legacy_occurrence_record(role, 11, Some(matrix));
    assert_eq!(identity.len(), 403);
    assert_eq!(matrix_record.len(), 531);
    let mut bytes = identity;
    bytes.extend(matrix_record);
    let placements = super::occurrence_placements(
        &bytes,
        &super::indexed_records(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap(),
        None,
    );

    assert_eq!(placements.len(), 2);
    assert_eq!(
        super::occurrence_transforms(
            &cadmpeg_test_support::service_decode_context(),
            &placements,
            role
        )
        .unwrap(),
        vec![None, Some(matrix)]
    );
}

#[test]
fn malformed_legacy_typed_placement_reports_its_role() {
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let mut bytes = legacy_occurrence_record(role, 10, None);
    bytes.pop();
    let (placements, failures) = super::occurrence_placements_with_failures(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        &super::indexed_records(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap(),
        None,
        None,
    )
    .unwrap();

    assert!(placements.is_empty());
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].link_names, vec![role]);
}

#[test]
fn occurrence_records_expand_shared_roles_and_decode_rigid_matrices() {
    let first = [
        [0.0, -1.0, 0.0, 2.0],
        [1.0, 0.0, 0.0, 3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let second = [
        [1.0, 0.0, 0.0, -5.0],
        [0.0, 1.0, 0.0, 6.0],
        [0.0, 0.0, 1.0, 7.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let mut bytes = occurrence_record("role", 10, &[1], Some(first));
    bytes.extend_from_slice(&occurrence_record("role", 11, &[1, 2], Some(second)));
    let placements = super::occurrence_placements(
        &bytes,
        &super::indexed_records(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap(),
        None,
    );

    assert_eq!(
        super::occurrence_transforms(
            &cadmpeg_test_support::service_decode_context(),
            &placements,
            "role"
        )
        .unwrap(),
        vec![Some(first), Some(second)]
    );
}

#[test]
fn identity_marked_placement_stores_no_matrix() {
    let mut bytes = occurrence_record("role", 10, &[1], None);
    bytes.extend_from_slice(&occurrence_record("role", 11, &[3], None));
    let placements = super::occurrence_placements(
        &bytes,
        &super::indexed_records(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap(),
        None,
    );

    assert_eq!(placements.len(), 2);
    assert_eq!(
        super::occurrence_transforms(
            &cadmpeg_test_support::service_decode_context(),
            &placements,
            "role"
        )
        .unwrap(),
        vec![None, None]
    );
}

#[test]
fn malformed_role_placement_is_retained_as_a_decode_failure() {
    let mut bytes = occurrence_record("role", 10, &[1], None);
    bytes.pop();
    let records =
        super::indexed_records(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap();

    let (placements, failures) = super::occurrence_placements_with_failures(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        &records,
        None,
        None,
    )
    .unwrap();

    assert!(placements.is_empty());
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].link_names, vec!["role"]);
}

#[test]
fn tagged_placement_tail_requires_the_modern_serializer_magic() {
    let modern = occurrence_record_with_serializer_magic(
        "role",
        10,
        &[1],
        None,
        Some(crate::metastream::MODERN_SERIALIZER_MAGIC),
    );
    let records =
        super::indexed_records(&cadmpeg_test_support::service_decode_context(), &modern).unwrap();
    assert_eq!(
        super::occurrence_placements(
            &modern,
            &records,
            Some(crate::metastream::MODERN_SERIALIZER_MAGIC)
        )
        .len(),
        1
    );
    assert!(
        super::occurrence_placements(&modern, &records, Some(999)).is_empty(),
        "a legacy MetaStream must not admit the modern tagged tail"
    );

    let legacy = occurrence_record("role", 11, &[2], None);
    let records =
        super::indexed_records(&cadmpeg_test_support::service_decode_context(), &legacy).unwrap();
    assert_eq!(
        super::occurrence_placements(&legacy, &records, Some(999)).len(),
        1
    );
    assert!(
        super::occurrence_placements(
            &legacy,
            &records,
            Some(crate::metastream::MODERN_SERIALIZER_MAGIC)
        )
        .is_empty(),
        "the modern form requires its tagged tail"
    );
}

#[test]
fn paired_design_metastream_selects_the_tagged_placement_form() {
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let properties =
        br#"{"docstruct":{"version":"1.0.0","type":"assembly-design","subtype":"synthetic","attributes":{}}}"#;
    let placement = occurrence_record_with_serializer_magic(
        role,
        10,
        &[1],
        Some([
            [1.0, 0.0, 0.0, 7.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]),
        Some(crate::metastream::MODERN_SERIALIZER_MAGIC),
    );
    let mut placement = placement;
    placement[4..7].copy_from_slice(b"256");
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("Properties.dat", stored).unwrap();
    zip.write_all(
        &(u32::try_from(properties.len()).expect("fixture value fits u32")).to_le_bytes(),
    )
    .unwrap();
    zip.write_all(properties).unwrap();
    zip.start_file("RedirectionsStream.dat", stored).unwrap();
    zip.write_all(redirections_json("root.f3d", &[("part.f3d", role)]).as_bytes())
        .unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&design_metastream_with_records(
        &[(
            super::OCCURRENCE_PLACEMENT_TYPE_GUID,
            "",
            2,
            "Component",
            &[10],
        )],
        &[(10, 0)],
    ))
    .unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&placement).unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect("synthetic XRef archive");
    let native = f3d_native(decoded.ir());
    assert_eq!(native.xref_references.len(), 1);
    let transform = native.xref_references[0]
        .transform
        .expect("tagged placement transform");
    assert!((transform.rows()[0][3] - 7.0).abs() < EPS_PLACEMENT_TRANSLATION);
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != F3dLossCode::XrefPlacementUndecoded.kind()));
}

#[test]
fn paired_design_metastream_selects_the_legacy_typed_placement_form() {
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let properties =
        br#"{"docstruct":{"version":"1.0.0","type":"assembly-design","subtype":"synthetic","attributes":{}}}"#;
    let placement = legacy_occurrence_record(
        role,
        10,
        Some([
            [1.0, 0.0, 0.0, 7.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]),
    );
    let mut placement = placement;
    placement[4..7].copy_from_slice(b"256");
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("Properties.dat", stored).unwrap();
    zip.write_all(
        &(u32::try_from(properties.len()).expect("fixture value fits u32")).to_le_bytes(),
    )
    .unwrap();
    zip.write_all(properties).unwrap();
    zip.start_file("RedirectionsStream.dat", stored).unwrap();
    zip.write_all(redirections_json("root.f3d", &[("part.f3d", role)]).as_bytes())
        .unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&design_metastream_with_records(
        &[(
            super::OCCURRENCE_PLACEMENT_TYPE_GUID,
            "",
            2,
            "Component",
            &[10],
        )],
        &[(10, 0)],
    ))
    .unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&placement).unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect("synthetic legacy XRef archive");
    let native = f3d_native(decoded.ir());
    assert_eq!(native.xref_references.len(), 1);
    let transform = native.xref_references[0]
        .transform
        .expect("legacy placement transform");
    assert!((transform.rows()[0][3] - 7.0).abs() < EPS_PLACEMENT_TRANSLATION);
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != F3dLossCode::XrefPlacementUndecoded.kind()));
}

#[test]
fn malformed_typed_role_placement_reports_a_loss() {
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let properties =
        br#"{"docstruct":{"version":"1.0.0","type":"assembly-design","subtype":"synthetic","attributes":{}}}"#;
    let mut placement = occurrence_record_with_serializer_magic(
        role,
        10,
        &[1],
        None,
        Some(crate::metastream::MODERN_SERIALIZER_MAGIC),
    );
    placement.pop();
    placement[4..7].copy_from_slice(b"256");

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("Properties.dat", stored).unwrap();
    zip.write_all(
        &(u32::try_from(properties.len()).expect("fixture value fits u32")).to_le_bytes(),
    )
    .unwrap();
    zip.write_all(properties).unwrap();
    zip.start_file("RedirectionsStream.dat", stored).unwrap();
    zip.write_all(redirections_json("root.f3d", &[("part.f3d", role)]).as_bytes())
        .unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&design_metastream_with_records(
        &[(
            super::OCCURRENCE_PLACEMENT_TYPE_GUID,
            "",
            2,
            "Component",
            &[10],
        )],
        &[(10, 0)],
    ))
    .unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&placement).unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect("synthetic malformed placement archive");
    let loss = decoded
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == F3dLossCode::XrefPlacementUndecoded.kind())
        .expect("typed placement loss");
    assert!(loss.message.contains("part.f3d"));
    assert!(loss.message.contains(role));
}

#[test]
fn placement_keeps_the_link_name_of_a_multi_element_path() {
    let bytes = occurrence_record("role", 10, &[7, 4, 2], None);
    let placements = super::occurrence_placements(
        &bytes,
        &super::indexed_records(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap(),
        None,
    );

    assert_eq!(placements[0].link_names, vec!["role".to_owned()]);
}

#[test]
fn a_placement_that_does_not_close_on_the_record_end_is_not_a_placement() {
    let mut bytes = occurrence_record("role", 10, &[1], None);
    bytes.push(0);
    let records =
        super::indexed_records(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap();

    assert_eq!(
        super::occurrence_placements(&bytes, &records, None),
        Vec::new()
    );
}

#[test]
fn a_nonrigid_matrix_is_not_a_placement() {
    let mut nonrigid = [[0.0; 4]; 4];
    nonrigid[0][0] = 2.0;
    nonrigid[1][1] = 1.0;
    nonrigid[2][2] = 1.0;
    nonrigid[3][3] = 1.0;
    let bytes = occurrence_record("role", 10, &[1], Some(nonrigid));
    let records =
        super::indexed_records(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap();

    assert_eq!(
        super::occurrence_placements(&bytes, &records, None),
        Vec::new()
    );
}

#[test]
fn a_role_that_no_path_element_names_places_nothing() {
    let bytes = occurrence_record("role", 10, &[1], None);
    let placements = super::occurrence_placements(
        &bytes,
        &super::indexed_records(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap(),
        None,
    );

    assert_eq!(
        super::occurrence_transforms(
            &cadmpeg_test_support::service_decode_context(),
            &placements,
            "other"
        )
        .unwrap(),
        Vec::new()
    );
}

#[test]
fn exact_component_insert_carriers_precede_structured_placements() {
    let direct = [
        [1.0, 0.0, 0.0, 7.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let structured = OccurrencePlacement {
        link_names: vec!["role".into()],
        transform: Some([
            [1.0, 0.0, 0.0, -5.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]),
    };

    assert_eq!(
        super::occurrence_transforms_with_precedence(
            &cadmpeg_test_support::service_decode_context(),
            vec![direct],
            &[structured.clone()],
            "role"
        )
        .unwrap(),
        vec![Some(direct)]
    );
    assert_eq!(
        super::superseded_placement_count(
            std::slice::from_ref(&direct),
            std::slice::from_ref(&structured),
            "role"
        ),
        1
    );
}

#[test]
fn component_insert_selection_uses_stream_and_role_not_class_tag() {
    let selected = [
        [1.0, 0.0, 0.0, 7.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let ignored = [
        [1.0, 0.0, 0.0, 9.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let selected_construction =
        crate::records::feature::assembly_features::DesignComponentInsertConstruction {
            relation_record_index: 1,
            carrier_record_index: 2,
            occurrence_identity: None,
            neutron_role: "role".into(),
            neutron_role_offset: 0,
            placement: Some(
                crate::records::feature::assembly_features::DesignComponentInsertMatrix {
                    scope: crate::records::identity::Located {
                        value: selected.try_into().unwrap(),
                        offset: 0,
                    },
                    carrier_offset: Some(0),
                },
            ),
        };
    let ignored_construction =
        crate::records::feature::assembly_features::DesignComponentInsertConstruction {
            neutron_role: "other".into(),
            placement: Some(
                crate::records::feature::assembly_features::DesignComponentInsertMatrix {
                    scope: crate::records::identity::Located {
                        value: ignored.try_into().unwrap(),
                        offset: 0,
                    },
                    carrier_offset: Some(0),
                },
            ),
            ..selected_construction.clone()
        };

    assert_eq!(
        super::select_component_insert_transforms(
            &cadmpeg_test_support::service_decode_context(),
            [
                ("stream", &selected_construction),
                ("stream", &ignored_construction),
                ("other-stream", &selected_construction),
            ],
            "stream",
            "role"
        )
        .unwrap(),
        vec![selected]
    );
}

#[test]
fn redirections_leaf_form_parses_empty_object_references() {
    let table = crate::xref::parse(&cadmpeg_test_support::service_decode_context(),
        br#"{"name":"RedirectionsStream","schema-version":0,"designs":[{"file-version":1,"targetFileName":"part.f3d","displayName":"part","lineageUrn":"urn:l","versionUrn":"urn:v"}],"references":{}}"#,
    )
    .unwrap();
    assert_eq!(table.designs.len(), 1);
    assert_eq!(table.designs[0].target_file_name.as_str(), "part.f3d");
    assert!(table.references.is_empty());
}

mod retained_text;

mod schema;

mod document_admission;
