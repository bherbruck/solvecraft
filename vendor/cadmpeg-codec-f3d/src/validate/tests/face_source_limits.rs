// SPDX-License-Identifier: Apache-2.0

fn source_member(
    record_index: u32,
    byte_offset: u64,
) -> crate::records::topology::face::DesignFaceSourceMember {
    use crate::records::{
        mesh::DesignRelaxedGuidText,
        topology::{
            construction::{
                DesignConstructionPersistentIdentity, DesignConstructionPersistentIdentityDraft,
            },
            face::DesignFaceSourceMember,
        },
    };
    let identity =
        DesignConstructionPersistentIdentity::try_new(DesignConstructionPersistentIdentityDraft {
            local_id: 1,
            local_id_offset: byte_offset + 21,
            asset_id: DesignRelaxedGuidText::try_from(
                "11111111-2222-4333-8444-555555555555".to_owned(),
            )
            .unwrap(),
            asset_id_offset: byte_offset + 33,
            context_id: DesignRelaxedGuidText::try_from(
                "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".to_owned(),
            )
            .unwrap(),
            context_id_offset: byte_offset + 80,
            tail_slot_present: false,
            tail_slot_offset: byte_offset + 185,
            next_record_index: record_index + 1,
            next_byte_offset: byte_offset + 190,
        })
        .unwrap();
    DesignFaceSourceMember {
        record_index,
        byte_offset,
        class_tag: "400".to_owned().try_into().unwrap(),
        persistent_identity: identity,
    }
}

fn native() -> crate::native::F3dNative {
    use crate::records::{
        identity::{Located, NonEmptyByteSpan},
        topology::face::DesignFaceSourceGroup,
    };
    let group = DesignFaceSourceGroup {
        id: "f3d:Design/BulkStream.dat:design-face-source-group#90".into(),
        scope_record_index: 10,
        carrier_reference_ordinal: 0,
        carrier_record_index: 90,
        carrier_span: NonEmptyByteSpan::new(900, 1300).unwrap(),
        carrier_class_tag: "394".to_owned().try_into().unwrap(),
        paired_record_index: 91,
        paired_class_tag: "311".to_owned().try_into().unwrap(),
        source_members: vec![
            Located {
                value: source_member(100, 1_000),
                offset: 936,
            },
            Located {
                value: source_member(200, 1_200),
                offset: 947,
            },
        ],
    };
    let mut native = crate::native::F3dNative::default();
    native.design_face_source_groups.push(group);
    native
}

fn source_error(max_items: u64, max_retained: u64) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        policy.limits.max_retained_bytes = max_retained;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_face_source_groups(&ctx, &mut Vec::new()).unwrap_err()
    })
}

#[test]
fn face_source_member_index_refuses_collection_limit() {
    let error = source_error(0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D Face source member records")
    );
}

#[test]
fn face_source_carrier_index_refuses_collection_limit() {
    let error = source_error(2, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D Face source carriers")
    );
}

#[test]
fn face_source_invalid_finding_refuses_collection_limit() {
    let error = source_error(3, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn face_source_invalid_entity_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D validation entity",
        |cap| Err::<(), cadmpeg_core::CodecError>(source_error(u64::MAX, cap)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}
