// SPDX-License-Identifier: Apache-2.0

fn body_link(
    target: cadmpeg_ir::attributes::AttributeTarget,
    ordinal: u32,
) -> crate::records::sketch_links::PersistentDesignLink {
    crate::records::sketch_links::PersistentDesignLink {
        id: format!("f3d:asm:persistent-design-link#{ordinal}"),
        target,
        design_id: "301".to_owned().try_into().unwrap(),
        design_reference: 1,
        ordinal,
    }
}

fn subentity_tag(
    target: cadmpeg_ir::attributes::AttributeTarget,
    ordinal: u32,
) -> crate::records::sketch_links::PersistentSubentityTag {
    crate::records::sketch_links::PersistentSubentityTag {
        id: format!("f3d:asm:persistent-subentity-tag#{ordinal}"),
        target,
        selector: 1,
        token: cadmpeg_core::text::NonBlankString::new("97").unwrap(),
        design_references: vec![1],
        ordinal,
    }
}

fn body_link_error(
    valid: bool,
    ordinal: u32,
    extra_items: u64,
    retained: u64,
) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let target = if valid {
            cadmpeg_ir::attributes::AttributeTarget::Body(ir.model.bodies[0].id.clone())
        } else {
            cadmpeg_ir::attributes::AttributeTarget::Face(ir.model.faces[0].id.clone())
        };
        let mut native = crate::native::F3dNative::default();
        native
            .persistent_design_links
            .push(body_link(target, ordinal));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = if retained != u64::MAX {
            DecodePolicy::service().limits.max_collection_items
        } else {
            u64::try_from(ir.model.bodies.len()).unwrap() + extra_items
        };
        policy.limits.max_retained_bytes = retained;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_body_links(&ctx, &mut Vec::new()).unwrap_err()
    })
}

fn subentity_tag_error(
    valid: bool,
    ordinal: u32,
    extra_items: u64,
    retained: u64,
) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let target = if valid {
            cadmpeg_ir::attributes::AttributeTarget::Face(ir.model.faces[0].id.clone())
        } else {
            cadmpeg_ir::attributes::AttributeTarget::Body(ir.model.bodies[0].id.clone())
        };
        let mut native = crate::native::F3dNative::default();
        native
            .persistent_subentity_tags
            .push(subentity_tag(target, ordinal));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = if retained != u64::MAX {
            DecodePolicy::service().limits.max_collection_items
        } else {
            u64::try_from(ir.model.faces.len() + ir.model.edges.len()).unwrap() + extra_items
        };
        policy.limits.max_retained_bytes = retained;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_subentity_tags(&ctx, &mut Vec::new()).unwrap_err()
    })
}

#[test]
fn persistent_body_target_index_refuses_collection_limit() {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = crate::native::F3dNative::default();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        let error = super::super::validate_body_links(&ctx, &mut Vec::new()).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D persistent body targets")
        );
    })
}

#[test]
fn persistent_body_group_index_refuses_collection_limit() {
    let error = body_link_error(true, 0, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D persistent body link groups")
    );
}

#[test]
fn persistent_body_group_member_refuses_collection_limit() {
    let error = body_link_error(true, 0, 1, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D persistent body link members")
    );
}

#[test]
fn persistent_body_invalid_finding_refuses_collection_limit() {
    let error = body_link_error(false, 0, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn persistent_body_invalid_entity_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D validation entity",
        |cap| Err::<(), cadmpeg_core::CodecError>(body_link_error(false, 0, 0, cap)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

#[test]
fn persistent_body_order_finding_refuses_collection_limit() {
    let error = body_link_error(true, 1, 2, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn persistent_face_target_index_refuses_collection_limit() {
    let error = subentity_target_index_error(0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D persistent face targets")
    );
}

fn subentity_target_index_error(max_items: u64) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = crate::native::F3dNative::default();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_subentity_tags(&ctx, &mut Vec::new()).unwrap_err()
    })
}

#[test]
fn persistent_edge_target_index_refuses_collection_limit() {
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let error = subentity_target_index_error(u64::try_from(ir.model.faces.len()).unwrap());
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D persistent edge targets")
    );
}

#[test]
fn persistent_subentity_group_index_refuses_collection_limit() {
    let error = subentity_tag_error(true, 0, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D persistent subentity tag groups")
    );
}

#[test]
fn persistent_subentity_group_member_refuses_collection_limit() {
    let error = subentity_tag_error(true, 0, 1, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D persistent subentity tag members")
    );
}

#[test]
fn persistent_subentity_invalid_finding_refuses_collection_limit() {
    let error = subentity_tag_error(false, 0, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn persistent_subentity_invalid_entity_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D validation entity",
        |cap| Err::<(), cadmpeg_core::CodecError>(subentity_tag_error(false, 0, 0, cap)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

#[test]
fn persistent_subentity_order_finding_refuses_collection_limit() {
    let error = subentity_tag_error(true, 1, 2, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}
