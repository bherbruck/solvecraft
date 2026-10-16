// SPDX-License-Identifier: Apache-2.0

fn limited_context(
    max_items: u64,
    max_retained: u64,
) -> cadmpeg_core::decode::DecodeContext<'static> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = Box::leak(Box::new(DecodeArena::new()));
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    policy.limits.max_retained_bytes = max_retained;
    let policy = Box::leak(Box::new(policy));
    DecodeContext::from_root_bytes(&[], arena, policy)
        .unwrap()
        .0
}

fn body_brep(with_region: bool) -> cadmpeg_asm::brep::AsmBrep {
    use cadmpeg_ir::ids::{BodyId, RegionId};
    use cadmpeg_ir::topology::{Body, BodyKind};
    let regions = if with_region {
        vec![RegionId::mint("f3d:brep:entity#2").unwrap()]
    } else {
        Vec::new()
    };
    let body = Body {
        id: BodyId::mint("f3d:brep:entity#1").unwrap(),
        kind: BodyKind::Solid,
        regions,
        transform: None,
        name: None,
        color: None,
        visible: None,
    };
    cadmpeg_asm::brep::AsmBrep {
        bodies: vec![body],
        ..Default::default()
    }
}

#[test]
fn historical_topology_references_refuse_collection_limit() {
    let ctx = limited_context(0, u64::MAX);
    let error = super::super::historical_topology(&ctx, &body_brep(false)).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D historical topology references")
    );
}

#[test]
fn historical_topology_relation_members_refuse_collection_limit() {
    let ctx = limited_context(1, u64::MAX);
    let error = super::super::historical_topology(&ctx, &body_brep(true)).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D historical topology references")
    );
}

#[test]
fn historical_topology_relation_rows_refuse_collection_limit() {
    let ctx = limited_context(1, u64::MAX);
    let error = super::super::historical_topology(&ctx, &body_brep(false)).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D historical topology relations")
    );
}

fn tagged_brep() -> crate::brep::Brep {
    use cadmpeg_ir::attributes::AttributeTarget;
    use cadmpeg_ir::ids::FaceId;
    crate::brep::Brep {
        persistent_subentity_tags: vec![crate::records::sketch_links::PersistentSubentityTag {
            id: "native:tag".into(),
            target: AttributeTarget::Face(FaceId::mint("f3d:brep:entity#4").unwrap()),
            selector: 1,
            token: cadmpeg_core::text::NonBlankString::new("tag").unwrap(),
            design_references: vec![2],
            ordinal: 0,
        }],
        ..Default::default()
    }
}

#[test]
fn historical_tag_references_refuse_collection_limit() {
    let ctx = limited_context(0, u64::MAX);
    let error = super::super::historical_topology_with_tags(&ctx, &tagged_brep()).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D historical tag design references")
    );
}

#[test]
fn historical_tag_token_refuses_retained_limit() {
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy F3D historical tag token",
        0,
        |ctx| super::super::historical_topology_with_tags(ctx, &tagged_brep()),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D historical tag token")
    );
}

#[test]
fn historical_tags_refuse_collection_limit() {
    let ctx = limited_context(1, u64::MAX);
    let error = super::super::historical_topology_with_tags(&ctx, &tagged_brep()).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D historical persistent tags")
    );
}
