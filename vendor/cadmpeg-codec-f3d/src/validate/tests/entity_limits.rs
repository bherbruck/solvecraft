// SPDX-License-Identifier: Apache-2.0

fn validation_entity_header(
    references: Vec<crate::records::identity::Located<u32>>,
) -> crate::records::entity_header::DesignEntityHeader {
    use crate::records::entity_header::{
        DesignEntityHeader, DesignEntityRegistration, SketchHeaderReferences,
    };
    use crate::records::identity::{DesignEntityId, ReferenceRun};
    DesignEntityHeader {
        id: "f3d:Design/BulkStream.dat:entity-header#1".into(),
        byte_offset: 10,
        entity_id: DesignEntityId::from_parts("sketch", 1),
        class_tag: crate::records::references::DesignClassTag::try_from("112".to_owned()).unwrap(),
        optional_slot_present: false,
        registration: DesignEntityRegistration::new(
            Some(crate::records::entity_header::DESIGN_MODULE_SKETCH.into()),
            (!references.is_empty()).then_some(SketchHeaderReferences {
                record_reference: None,
                record_reference_offset: 10,
                references,
            }),
            ReferenceRun::unlocated(Vec::new()),
        )
        .unwrap(),
    }
}

#[test]
fn native_entity_suffix_index_refuses_collection_limit() {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = crate::native::F3dNative::default();
        native
            .design_entity_headers
            .push(validation_entity_header(Vec::new()));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        let error = super::super::validate_entity_headers(&ctx, &mut Vec::new()).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D design entity suffixes")
        );
    })
}

#[test]
fn native_entity_duplicate_finding_refuses_collection_limit() {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let header = validation_entity_header(Vec::new());
        let mut native = crate::native::F3dNative::default();
        native.design_entity_headers.push(header.clone());
        native.design_entity_headers.push(header);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        let error = super::super::validate_entity_headers(&ctx, &mut Vec::new()).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
        );
    })
}

#[test]
fn native_entity_reference_finding_refuses_retained_limit() {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = crate::native::F3dNative::default();
        native
            .design_entity_headers
            .push(validation_entity_header(vec![
                crate::records::identity::Located {
                    value: 999,
                    offset: 12,
                },
            ]));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        let error = super::super::validate_entity_headers(&ctx, &mut Vec::new()).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
        );
    })
}

#[test]
fn native_sketch_relation_finding_refuses_retained_limit() {
    crate::test_support::with_decode_context(|service_ctx| {
        use crate::records::identity::ReferenceRun;
        use crate::records::sketch_relations::{
            SketchRelation, SketchRelationDefinition, SketchRelationDraft, SketchRelationMembers,
            SketchRelationReturnMembers,
        };
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let service = cadmpeg_test_support::service_decode_context();
        let relation = SketchRelation::try_new(SketchRelationDraft {
            id: "f3d:Design/BulkStream.dat:sketch-relation#1".into(),
            record_index: 1,
            class_tag: crate::records::references::DesignClassTag::try_from("296".to_owned())
                .unwrap(),
            byte_offset: 10,
            state_offset: 0,
            owner_reference: 999,
            owner_entity_id: None,
            auxiliary_references: ReferenceRun::from_columns(vec![1], vec![4], "auxiliary")
                .unwrap(),
            rectangular_counted_reference_count: None,
            members: SketchRelationMembers::from_indices(&service, std::iter::empty()).unwrap(),
            owner_reference_offset: 8,
            definition: SketchRelationDefinition::new(0, None).unwrap(),
            entity_genesis: None,
            return_members: SketchRelationReturnMembers::from_indices(&service, std::iter::empty())
                .unwrap(),
            raw_bytes: vec![0; 24],
        })
        .unwrap();
        let mut native = crate::native::F3dNative::default();
        native.sketch_relations.push(relation);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        let error = super::super::validate_sketch_relations(&ctx, &mut Vec::new()).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
        );
    })
}

fn sketch_geometry_fixture() -> crate::native::F3dNative {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    let source = crate::test_support::zip_test::f3d_with_smbh_and_protein(
        &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
    );
    let decoded = crate::F3dCodec
        .decode(&mut std::io::Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let (ir, _, _) = decoded.into_parts();
    crate::native::F3dNative::load(ir.native.namespace("f3d").unwrap()).unwrap()
}

fn sketch_geometry_error(
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
        super::super::validate_sketch_geometry_identities(&ctx, &mut Vec::new()).unwrap_err()
    })
}

#[test]
fn sketch_point_identity_index_refuses_collection_limit() {
    let mut native = sketch_geometry_fixture();
    native.sketch_points.truncate(1);
    native.sketch_points[0].owner_reference = Some(100);
    native.sketch_curve_identities.clear();
    native.sketch_surfaces.clear();
    let error = sketch_geometry_error(native, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D sketch point identities")
    );
}

#[test]
fn sketch_geometry_record_index_refuses_collection_limit() {
    let mut native = sketch_geometry_fixture();
    native.sketch_points.truncate(1);
    native.sketch_points[0].owner_reference = None;
    native.sketch_curve_identities.clear();
    native.sketch_surfaces.clear();
    let error = sketch_geometry_error(native, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D sketch geometry records")
    );
}

#[test]
fn sketch_curve_identity_index_refuses_collection_limit() {
    let mut native = sketch_geometry_fixture();
    native.sketch_points.clear();
    native.sketch_curve_identities.truncate(1);
    native.sketch_curve_identities[0].owner_reference = Some(100);
    native.sketch_surfaces.clear();
    let error = sketch_geometry_error(native, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D sketch curve identities")
    );
}

fn validation_sketch_surface() -> crate::records::sketch_geometry::SketchSurface {
    use crate::records::sketch_geometry::{SketchSurface, SketchSurfaceGeometry};
    SketchSurface {
        id: "f3d:Design/BulkStream.dat:sketch-surface#1".into(),
        record_index: 1,
        owner_reference: Some(100),
        class_tag: crate::records::references::DesignClassTag::try_from("296".to_owned()).unwrap(),
        byte_offset: 10,
        entity_genesis: None,
        persistent_id: std::num::NonZeroU64::new(1).unwrap(),
        geometry: SketchSurfaceGeometry {
            u_degree: std::num::NonZeroU32::new(1).unwrap(),
            v_degree: std::num::NonZeroU32::new(1).unwrap(),
            u_knots: Vec::new(),
            v_knots: Vec::new(),
            control_points: Vec::new(),
        },
    }
}

#[test]
fn sketch_surface_identity_index_refuses_collection_limit() {
    let mut native = crate::native::F3dNative::default();
    native.sketch_surfaces.push(validation_sketch_surface());
    let error = sketch_geometry_error(native, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D sketch surface identities")
    );
}

#[test]
fn sketch_point_identity_finding_refuses_collection_limit() {
    let mut native = sketch_geometry_fixture();
    native.sketch_points.truncate(1);
    native.sketch_points[0].owner_reference = Some(100);
    let duplicate = native.sketch_points[0].clone();
    native.sketch_points.push(duplicate);
    native.sketch_curve_identities.clear();
    native.sketch_surfaces.clear();
    let error = sketch_geometry_error(native, 2, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn sketch_point_identity_finding_refuses_retained_limit() {
    let mut native = sketch_geometry_fixture();
    native.sketch_points.truncate(1);
    native.sketch_points[0].owner_reference = Some(100);
    let duplicate = native.sketch_points[0].clone();
    native.sketch_points.push(duplicate);
    native.sketch_curve_identities.clear();
    native.sketch_surfaces.clear();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D validation entity",
        |cap| {
            Err::<(), cadmpeg_core::CodecError>(sketch_geometry_error(
                native.clone(),
                u64::MAX,
                cap,
            ))
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

#[test]
fn sketch_curve_identity_finding_refuses_collection_limit() {
    let mut native = sketch_geometry_fixture();
    native.sketch_points.clear();
    native.sketch_curve_identities.truncate(1);
    native.sketch_curve_identities[0].owner_reference = Some(100);
    let duplicate = native.sketch_curve_identities[0].clone();
    native.sketch_curve_identities.push(duplicate);
    native.sketch_surfaces.clear();
    let error = sketch_geometry_error(native, 2, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn sketch_surface_identity_finding_refuses_collection_limit() {
    let surface = validation_sketch_surface();
    let mut native = crate::native::F3dNative::default();
    native.sketch_surfaces.push(surface.clone());
    native.sketch_surfaces.push(surface);
    let error = sketch_geometry_error(native, 2, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn sketch_geometry_alias_finding_refuses_collection_limit() {
    let mut native = sketch_geometry_fixture();
    native.sketch_points.truncate(1);
    native.sketch_points[0].owner_reference = None;
    let duplicate = native.sketch_points[0].clone();
    native.sketch_points.push(duplicate);
    native.sketch_curve_identities.clear();
    native.sketch_surfaces.clear();
    let error = sketch_geometry_error(native, 1, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

fn validation_body_bounds(with_binding: bool) -> crate::native::F3dNative {
    use crate::records::bodies::{
        DesignBodyBinding, DesignBodyBindingWire, DesignBodyBounds, DesignBodyBoundsWire,
    };
    use crate::records::entity_header::{
        DesignEntityHeader, DesignEntityRegistration, DESIGN_MODULE_BODY,
    };
    use crate::records::identity::{DesignEntityId, ReferenceRun};

    let binding_id = "f3d:Design/BulkStream.dat:design-body-binding#50";
    let bounds = DesignBodyBounds::try_from(DesignBodyBoundsWire {
        id: "f3d:Design/BulkStream.dat:design-body-bounds#10".into(),
        entity_suffix: 1,
        entity_byte_offset: 10,
        record_indices: [2, 3, 4],
        record_byte_offsets: [20, 30, 40],
        value_byte_offsets: [21, 31, 41],
        body_binding_ids: if with_binding {
            vec![binding_id.to_owned().try_into().unwrap()]
        } else {
            Default::default()
        },
        maximum: cadmpeg_ir::math::Point3::new(1.0, 1.0, 1.0),
        minimum: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
    })
    .unwrap();
    let mut native = crate::native::F3dNative::default();
    native.design_body_bounds.push(bounds);
    if with_binding {
        native.design_body_bindings.push(
            DesignBodyBinding::try_from(DesignBodyBindingWire {
                id: binding_id.into(),
                stream: "Design/BulkStream.dat".into(),
                pair_count: 1,
                pair_ordinal: 0,
                asm_body_key: 1,
                asm_body_key_offset: 50,
                entity_suffix: 1,
                entity_suffix_offset: 58,
                blob_name: "BREP.body".into(),
                blob_name_offset: 60,
                body: None,
            })
            .unwrap(),
        );
        native.design_entity_headers.push(DesignEntityHeader {
            id: "f3d:Design/BulkStream.dat:entity-header#1".into(),
            byte_offset: 10,
            entity_id: DesignEntityId::from_parts("body", 1),
            class_tag: crate::records::references::DesignClassTag::try_from("112".to_owned())
                .unwrap(),
            optional_slot_present: false,
            registration: DesignEntityRegistration::new(
                Some(DESIGN_MODULE_BODY.into()),
                None,
                ReferenceRun::unlocated(Vec::new()),
            )
            .unwrap(),
        });
    }
    native
}

fn body_bounds_error(
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
        super::super::validate_body_bounds(&ctx, &mut Vec::new()).unwrap_err()
    })
}

#[test]
fn body_bounds_binding_collection_refuses_collection_limit() {
    let error = body_bounds_error(validation_body_bounds(true), 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D expected body bounds bindings")
    );
}

#[test]
fn body_bounds_index_refuses_collection_limit() {
    let error = body_bounds_error(validation_body_bounds(true), 1, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D bounded bodies")
    );
}

#[test]
fn body_bounds_finding_refuses_collection_limit() {
    let error = body_bounds_error(validation_body_bounds(false), 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn body_bounds_entity_refuses_retained_limit() {
    let error = body_bounds_error(validation_body_bounds(false), u64::MAX, 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

#[test]
fn body_bounds_valid_binding_order_has_no_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = validation_body_bounds(true);
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        let mut findings = Vec::new();
        super::super::validate_body_bounds(&ctx, &mut findings).unwrap();
        assert!(findings.is_empty());
    })
}

fn body_binding_error(
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
        super::super::validate_body_bindings(&ctx, &mut Vec::new()).unwrap_err()
    })
}

#[test]
fn body_binding_offset_index_refuses_collection_limit() {
    let error = body_binding_error(validation_body_bounds(true), 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D body binding offsets")
    );
}

#[test]
fn body_binding_group_index_refuses_collection_limit() {
    let error = body_binding_error(validation_body_bounds(true), 1, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D body binding groups")
    );
}

#[test]
fn body_binding_group_member_refuses_collection_limit() {
    let error = body_binding_error(validation_body_bounds(true), 2, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D body binding group members")
    );
}

#[test]
fn body_binding_invalid_finding_refuses_collection_limit() {
    let mut native = validation_body_bounds(true);
    // The binding names a valid body but has no matching native body source.
    native.design_body_bindings[0].body = Some(
        cadmpeg_ir::examples::unit_cube().unwrap().model.bodies[0]
            .id
            .clone(),
    );
    let error = body_binding_error(native, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn body_binding_invalid_entity_refuses_retained_limit() {
    let mut native = validation_body_bounds(true);
    // The binding names a valid body but has no matching native body source.
    native.design_body_bindings[0].body = Some(
        cadmpeg_ir::examples::unit_cube().unwrap().model.bodies[0]
            .id
            .clone(),
    );
    let error = body_binding_error(native, u64::MAX, 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

#[test]
fn body_binding_incomplete_group_finding_refuses_collection_limit() {
    use crate::records::bodies::{DesignBodyBinding, DesignBodyBindingWire};
    let mut native = validation_body_bounds(true);
    native.design_body_bindings[0] = DesignBodyBinding::try_from(DesignBodyBindingWire {
        id: "f3d:Design/BulkStream.dat:design-body-binding#50".into(),
        stream: "Design/BulkStream.dat".into(),
        pair_count: 2,
        pair_ordinal: 0,
        asm_body_key: 1,
        asm_body_key_offset: 50,
        entity_suffix: 1,
        entity_suffix_offset: 58,
        blob_name: "BREP.body".into(),
        blob_name_offset: 60,
        body: None,
    })
    .unwrap();
    let error = body_binding_error(native, 3, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn body_binding_valid_group_has_no_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = validation_body_bounds(true);
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        let mut findings = Vec::new();
        super::super::validate_body_bindings(&ctx, &mut findings).unwrap();
        assert!(findings.is_empty());
    })
}

fn validation_occurrence(
    record_index: u32,
    occurrence_guid: &str,
) -> crate::records::feature::assembly_features::DesignComponentOccurrence {
    use crate::records::feature::assembly_features::{
        DesignComponentOccurrence, DesignComponentOccurrenceDraft,
        DesignComponentOccurrencePlacement,
    };
    DesignComponentOccurrence::try_new(DesignComponentOccurrenceDraft {
        id: format!("f3d:Design/BulkStream.dat:design-component-occurrence#{record_index}"),
        class_tag: crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        record_index,
        byte_offset: u64::from(record_index) * 10,
        component_record_index: 700,
        component_guid: "11111111-2222-4333-8444-555555555555"
            .to_owned()
            .try_into()
            .unwrap(),
        occurrence_guid: occurrence_guid.to_owned().try_into().unwrap(),
        placement: DesignComponentOccurrencePlacement::Base,
    })
    .unwrap()
}

fn occurrence_error(
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
        super::super::validate_component_occurrences(&ctx, &mut Vec::new()).unwrap_err()
    })
}

#[test]
fn occurrence_guid_key_refuses_retained_limit() {
    let mut native = crate::native::F3dNative::default();
    native
        .design_component_occurrences
        .push(validation_occurrence(
            100,
            "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee",
        ));
    let error = occurrence_error(native, u64::MAX, 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D occurrence GUID index key")
    );
}

#[test]
fn occurrence_guid_index_refuses_collection_limit() {
    let mut native = crate::native::F3dNative::default();
    native
        .design_component_occurrences
        .push(validation_occurrence(
            100,
            "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee",
        ));
    let error = occurrence_error(native, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D occurrence GUIDs")
    );
}

#[test]
fn occurrence_record_index_refuses_collection_limit() {
    let mut native = crate::native::F3dNative::default();
    native
        .design_component_occurrences
        .push(validation_occurrence(
            100,
            "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee",
        ));
    let error = occurrence_error(native, 1, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D occurrence record indices")
    );
}

#[test]
fn occurrence_duplicate_finding_refuses_collection_limit() {
    let mut native = crate::native::F3dNative::default();
    native
        .design_component_occurrences
        .push(validation_occurrence(
            100,
            "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee",
        ));
    native
        .design_component_occurrences
        .push(validation_occurrence(
            101,
            "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE",
        ));
    let error = occurrence_error(native, 2, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn occurrence_duplicate_entity_refuses_retained_limit() {
    let mut native = crate::native::F3dNative::default();
    native
        .design_component_occurrences
        .push(validation_occurrence(
            100,
            "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee",
        ));
    native
        .design_component_occurrences
        .push(validation_occurrence(
            101,
            "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE",
        ));
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D validation entity",
        |cap| Err::<(), cadmpeg_core::CodecError>(occurrence_error(native.clone(), u64::MAX, cap)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

fn invalid_history_error(max_items: u64, max_retained: u64) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = crate::native::F3dNative::default();
        native
            .asm_histories
            .push(crate::history_records::AsmHistory {
                id: "f3d:history:asm-history#1".into(),
                byte_offset: 0,
                preamble: None,
                record_table_binding_budget_exceeded: false,
                states: Vec::new(),
            });
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        policy.limits.max_retained_bytes = max_retained;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_history_graphs(&decode, &ctx, &mut Vec::new()).unwrap_err()
    })
}

#[test]
fn invalid_history_finding_refuses_collection_limit() {
    let error = invalid_history_error(0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn invalid_history_entity_refuses_retained_limit() {
    let error = invalid_history_error(u64::MAX, 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

fn validation_placement(
    scope_record_index: Option<u32>,
    ordinal: Option<u32>,
    member: bool,
) -> crate::records::sketch_placement::DesignSketchPlacement {
    use crate::records::sketch_placement::{
        DesignSketchFrame, DesignSketchFrameForm, DesignSketchPlacement, DesignSketchVisibility,
    };
    DesignSketchPlacement {
        id: "f3d:Design/BulkStream.dat:sketch-placement#1".into(),
        scope_record_index,
        entity_id: crate::records::identity::DesignEntityId::from_parts("sketch", 1),
        visibility: ordinal.map(|ordinal| {
            DesignSketchVisibility::new(std::num::NonZeroU32::new(ordinal).unwrap(), 100, true)
                .unwrap()
        }),
        class_tag: crate::records::references::DesignClassTag::try_from("112".to_owned()).unwrap(),
        record_index: 1,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("112".to_owned())
            .unwrap(),
        frame: DesignSketchFrame::new(
            10,
            if member {
                DesignSketchFrameForm::MemberCompact {
                    paired_byte_offset: 20,
                }
            } else {
                DesignSketchFrameForm::ScopeCompact
            },
        )
        .unwrap(),
    }
}

fn placement_native(
    placement: crate::records::sketch_placement::DesignSketchPlacement,
) -> crate::native::F3dNative {
    let mut native = crate::native::F3dNative::default();
    if placement.visibility.is_some() {
        native
            .design_entity_headers
            .push(validation_entity_header(Vec::new()));
    }
    native.design_sketch_placements.push(placement);
    native
}

fn placement_error(
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
        super::super::validate_sketch_placements(&ctx, &mut Vec::new()).unwrap_err()
    })
}

#[test]
fn placement_record_index_refuses_collection_limit() {
    let error = placement_error(
        placement_native(validation_placement(None, None, true)),
        0,
        u64::MAX,
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D sketch placement records")
    );
}

#[test]
fn placement_scope_index_refuses_collection_limit() {
    let error = placement_error(
        placement_native(validation_placement(Some(1), None, true)),
        1,
        u64::MAX,
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D sketch placement scopes")
    );
}

#[test]
fn placement_visibility_ordinal_index_refuses_collection_limit() {
    let error = placement_error(
        placement_native(validation_placement(None, Some(1), true)),
        1,
        u64::MAX,
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D sketch visibility ordinals")
    );
}

#[test]
fn placement_visibility_offset_index_refuses_collection_limit() {
    let error = placement_error(
        placement_native(validation_placement(None, Some(1), true)),
        2,
        u64::MAX,
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D sketch visibility offsets")
    );
}

#[test]
fn placement_visibility_range_refuses_collection_limit() {
    let error = placement_error(
        placement_native(validation_placement(None, Some(1), true)),
        3,
        u64::MAX,
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D sketch visibility ordinal ranges")
    );
}

#[test]
fn placement_invalid_finding_refuses_collection_limit() {
    let error = placement_error(
        placement_native(validation_placement(None, None, false)),
        1,
        u64::MAX,
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn placement_invalid_entity_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D validation entity",
        |cap| {
            Err::<(), cadmpeg_core::CodecError>(placement_error(
                placement_native(validation_placement(None, None, false)),
                u64::MAX,
                cap,
            ))
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

#[test]
fn placement_noncontiguous_visibility_finding_refuses_collection_limit() {
    let error = placement_error(
        placement_native(validation_placement(None, Some(2), true)),
        4,
        u64::MAX,
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn placement_contiguous_visibility_has_no_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = placement_native(validation_placement(None, Some(1), true));
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        let mut findings = Vec::new();
        super::super::validate_sketch_placements(&ctx, &mut findings).unwrap();
        assert!(findings.is_empty());
    })
}

fn validation_parameter_owner() -> crate::records::parameters::DesignParameterOwner {
    use crate::records::parameters::{DesignParameterOwner, DesignParameterOwnerWire};
    DesignParameterOwner::try_from(DesignParameterOwnerWire {
        id: "f3d:Design/BulkStream.dat:parameter-owner#100".into(),
        byte_offset: 1_000,
        frame_length: 99,
        class_tag: crate::records::references::DesignClassTag::try_from("268".to_owned()).unwrap(),
        record_index: 100,
        scope_record_index: 200,
        local_ordinal: 1,
        evaluated_value: 6.0,
        evaluated_value_offset: 1_040,
        parameter_record_index: 101,
        owned_ordinal: 0,
        variant: None,
        companion_record_index: 102,
    })
    .unwrap()
}

fn parameter_owner_error(max_items: u64, max_retained: u64) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = crate::native::F3dNative::default();
        native
            .design_parameter_owners
            .push(validation_parameter_owner());
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        policy.limits.max_retained_bytes = max_retained;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_parameter_owners(&ctx, &mut Vec::new()).unwrap_err()
    })
}

#[test]
fn parameter_owner_index_refuses_collection_limit() {
    let error = parameter_owner_error(0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D parameter owners")
    );
}

#[test]
fn parameter_owner_ordinal_index_refuses_collection_limit() {
    let error = parameter_owner_error(1, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D parameter owner local ordinals")
    );
}

#[test]
fn parameter_owner_finding_refuses_collection_limit() {
    let error = parameter_owner_error(2, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn parameter_owner_entity_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D validation entity",
        |cap| Err::<(), cadmpeg_core::CodecError>(parameter_owner_error(u64::MAX, cap)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

fn validation_companion(with_recipe: bool) -> crate::native::F3dNative {
    use crate::records::parameters::{DesignCompanionPayload, DesignParameterCompanion};
    let mut companion = DesignParameterCompanion::unbound(
        "f3d:Design/BulkStream.dat:parameter-companion#102".into(),
        1_200,
        crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap(),
        102,
        100,
        std::num::NonZeroU64::new(1).unwrap(),
        1_242,
    );
    let mut native = crate::native::F3dNative::default();
    if with_recipe {
        let recipe_id = "f3d:Design/BulkStream.dat:construction-recipe#1".to_owned();
        companion = companion.bound(DesignCompanionPayload::new(
            1_258,
            100,
            vec![recipe_id.clone()],
        ));
        native
            .construction_recipes
            .push(crate::records::recipes::ConstructionRecipe {
                id: recipe_id,
                byte_offset: 1_260,
                kind: crate::records::recipes::ConstructionRecipeKind::Face,
                design: None,
                recipe_index: 1,
                record_index: None,
            });
    }
    native.design_parameter_companions.push(companion);
    native
}

fn companion_error(
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
        super::super::validate_parameter_companions(&ctx, &mut Vec::new()).unwrap_err()
    })
}

#[test]
fn companion_recipe_collection_refuses_collection_limit() {
    let error = companion_error(validation_companion(true), 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D companion expected recipes")
    );
}

#[test]
fn companion_index_refuses_collection_limit() {
    let error = companion_error(validation_companion(false), 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D parameter companions")
    );
}

#[test]
fn companion_owner_index_refuses_collection_limit() {
    let error = companion_error(validation_companion(false), 1, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D companion owners")
    );
}

#[test]
fn companion_finding_refuses_collection_limit() {
    let error = companion_error(validation_companion(false), 2, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn companion_entity_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D validation entity",
        |cap| {
            Err::<(), cadmpeg_core::CodecError>(companion_error(
                validation_companion(false),
                u64::MAX,
                cap,
            ))
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}
