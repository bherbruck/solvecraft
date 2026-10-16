// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::operands::{
    bind_vertex_recipe_candidates, bind_work_plane_constructions, bind_work_point_input_carriers,
    decode_edge_identity_operands, decode_edge_operands, decode_edge_treatment_vertex_operands,
    decode_face_operands,
};
use crate::records::decal::DesignRecordHeader;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn fixture_scope(
    kind: crate::records::feature::scope::DesignFeatureKind,
) -> crate::records::feature::scope::DesignParameterScope {
    use crate::records::feature::scope::{DesignParameterScope, DesignParameterScopeDraft};
    DesignParameterScope::try_new(
        DesignParameterScopeDraft {
            id: "f3d:Design/BulkStream.dat:scope#12".to_owned(),
            byte_offset: 1000,
            class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned())
                .unwrap(),
            record_index: 12,
            frame_length: 200,
            kind_offset: 1100,
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            history_state_id: None,
            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: 1080,
            reference_members: crate::records::identity::ReferenceRun::from_columns(
                vec![100],
                vec![1085],
                "reference_members",
            )
            .unwrap(),
            payload: kind.try_into().unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "261".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 1200,
        }
        .with_fixture_layout(),
    )
    .unwrap()
}

#[test]
fn edge_operand_header_and_offset_indices_refuse_collection_limits() {
    let archive = crate::test_support::zip_test::f3d_with_smbh_and_protein(
        &crate::test_support::smbh_header_test::synthetic_smbh(),
    );
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let header = DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:record#7".to_owned(),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("001".to_owned())
                .unwrap(),
            record_index: 7,
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        for (limit, operation) in [
            (0, "f3d edge operand header index"),
            (1, "f3d edge operand offset stream"),
            (2, "f3d edge operand stream offset"),
        ] {
            policy.limits.max_collection_items = limit;

            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert!(matches!(
                decode_edge_operands(&ctx, scan, &[], &[], std::slice::from_ref(&header), &[]),
                Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::CollectionItems
                        && failure.operation == operation
            ));
        }
    });
}

#[test]
fn edge_operand_member_index_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut indices = std::collections::HashSet::new();
    assert!(matches!(
        &ctx.insert_hash_set(&mut indices, 7, "f3d edge operand member index").map(|_| ()),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d edge operand member index"
    ));
}

#[test]
fn vertex_operand_header_index_refuses_collection_limit() {
    let archive = crate::test_support::zip_test::f3d_with_smbh_and_protein(
        &crate::test_support::smbh_header_test::synthetic_smbh(),
    );
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let header = DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:record#7".to_owned(),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("001".to_owned())
                .unwrap(),
            record_index: 7,
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            decode_edge_treatment_vertex_operands(
                &ctx, scan, &[], &[], std::slice::from_ref(&header), &[],
            ),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == "f3d vertex operand header index"
        ));
    });
}

#[test]
fn work_plane_header_index_refuses_collection_limit() {
    let archive = crate::test_support::zip_test::f3d_with_smbh_and_protein(
        &crate::test_support::smbh_header_test::synthetic_smbh(),
    );
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let header = DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:record#7".to_owned(),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("001".to_owned())
                .unwrap(),
            record_index: 7,
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            bind_work_plane_constructions(&ctx, scan, &mut [], std::slice::from_ref(&header),
                &[], &[], &[]),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == "f3d work plane header index"
        ));
    });
}

#[test]
fn work_point_header_index_refuses_collection_limit() {
    let archive = crate::test_support::zip_test::f3d_with_smbh_and_protein(
        &crate::test_support::smbh_header_test::synthetic_smbh(),
    );
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let header = DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:record#7".to_owned(),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("001".to_owned())
                .unwrap(),
            record_index: 7,
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            bind_work_point_input_carriers(&ctx, scan, &mut [], std::slice::from_ref(&header), &[], &[], &[]),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == "f3d WorkPoint header index"
        ));
    });
}

#[test]
fn work_point_plane_indices_refuse_collection_limits() {
    let archive = crate::test_support::zip_test::f3d_with_smbh_and_protein(
        &crate::test_support::smbh_header_test::synthetic_smbh(),
    );
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let mut scope = fixture_scope(crate::records::feature::scope::DesignFeatureKind::WorkPlane);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        for (limit, operation) in [
            (0, "f3d WorkPoint work-plane stream index"),
            (1, "f3d WorkPoint work-plane index"),
        ] {
            policy.limits.max_collection_items = limit;

            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert!(matches!(
                bind_work_point_input_carriers(&ctx, scan, std::slice::from_mut(&mut scope), &[], &[], &[], &[]),
                Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::CollectionItems
                        && failure.operation == operation
            ));
        }
    });
}

#[test]
fn vertex_recipe_scope_identity_refuses_materialized_limit() {
    let mut scope = fixture_scope(crate::records::feature::scope::DesignFeatureKind::WorkPoint);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_materialized_bytes = u64_from_index(scope.id.len()) - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        bind_vertex_recipe_candidates(&ctx, std::slice::from_mut(&mut scope), &[]),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::MaterializedBytes
    ));
}

#[test]
fn edge_identity_header_index_refuses_collection_limit() {
    let archive = crate::test_support::zip_test::f3d_with_smbh_and_protein(
        &crate::test_support::smbh_header_test::synthetic_smbh(),
    );
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let header = DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:record#7".to_owned(),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("001".to_owned())
                .unwrap(),
            record_index: 7,
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            decode_edge_identity_operands(&ctx, scan, &[], &[], std::slice::from_ref(&header)),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == "f3d edge identity header index"
        ));
    });
}

#[test]
fn face_operand_header_and_scope_indices_refuse_collection_limits() {
    let archive = crate::test_support::zip_test::f3d_with_smbh_and_protein(
        &crate::test_support::smbh_header_test::synthetic_smbh(),
    );
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let header = DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:record#7".to_owned(),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("001".to_owned())
                .unwrap(),
            record_index: 7,
        };
        let scope = fixture_scope(crate::records::feature::scope::DesignFeatureKind::Extrude);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        for (limit, operation) in [
            (0, "f3d face operand header index"),
            (1, "f3d face operand scope index"),
        ] {
            policy.limits.max_collection_items = limit;

            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert!(matches!(
                decode_face_operands(&ctx, scan, std::slice::from_ref(&scope), &[],
                    std::slice::from_ref(&header), &[]),
                Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::CollectionItems
                        && failure.operation == operation
            ));
        }
    });
}

#[test]
fn face_operand_seen_key_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut seen = std::collections::HashSet::new();
    assert!(matches!(
        &ctx.insert_hash_set(&mut seen, ("stream", 12, 7), "f3d face operand seen key"),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d face operand seen key"
    ));
}
