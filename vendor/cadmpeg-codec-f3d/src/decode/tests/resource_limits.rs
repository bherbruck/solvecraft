use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn context(arena: &DecodeArena, max_collection_items: u64) -> DecodeContext<'_> {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_collection_items;
    DecodeContext::from_root_bytes(&[], arena, &policy)
        .unwrap()
        .0
}

#[test]
fn related_record_index_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let error = super::super::collect_related_indices(&ctx, [("f3d:Design", 1)]).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D related record indices")
    );
}

#[test]
fn related_record_stream_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::collect_related_indices(&ctx, [("f3d:Design", 1)]).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D related record stream")
    );
}

#[test]
fn existing_record_header_index_refuses_collection_limit() {
    let bytes = crate::test_support::zip_test::synthetic_f3d(true);
    let arena = DecodeArena::new();
    let (scan_ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let scan = crate::container::scan(&scan_ctx, root).unwrap();
    let mut native = crate::native::F3dNative::default();
    native
        .design_record_headers
        .push(crate::records::decal::DesignRecordHeader {
            id: "f3d:Design:design-record-header#1".into(),
            record_index: 1,
            class_tag: crate::records::references::DesignClassTag::try_from("310".to_owned())
                .unwrap(),
            byte_offset: 1,
        });
    let limited = context(&arena, 0);
    let error =
        super::super::append_related_record_headers(&limited, &scan, &mut native, &[]).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D existing record headers")
    );
}

#[test]
fn document_digest_attribute_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let mut attributes = std::collections::BTreeMap::new();
    let error = ctx
        .insert_btree_map(
            &mut attributes,
            cadmpeg_core::nonblank_const!(cadmpeg_ir::hash::DOCUMENT_LOCAL_DIGEST_ATTRIBUTE),
            "0".repeat(64),
            "record F3D document digest",
        )
        .map(|_| ())
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "record F3D document digest")
    );
    assert!(attributes.is_empty());
}

fn appearance_binding_ir(
    target: cadmpeg_ir::appearance::AppearanceTarget,
) -> cadmpeg_ir::document::CadIr {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding};
    use cadmpeg_ir::ids::AppearanceId;
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let appearance = AppearanceId::mint("f3d:test:appearance#one").unwrap();
    ir.model.appearances.push(Appearance {
        id: appearance.clone(),
        name: None,
        asset_guid: None,
        library_id: None,
        textures: Vec::new(),
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Some(cadmpeg_ir::topology::Color::from_rgba8(1, 2, 3, 255)),
        properties: Default::default(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: cadmpeg_ir::ids::AppearanceBindingId::mint("f3d:test:binding#one").unwrap(),
        target,
        appearance,
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: Default::default(),
    });
    ir
}

#[test]
fn appearance_color_index_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let body = cadmpeg_ir::ids::BodyId::mint("f3d:test:body#one").unwrap();
    let mut ir = appearance_binding_ir(cadmpeg_ir::appearance::AppearanceTarget::Body(body));
    let error = super::super::apply_appearance_base_colors(&ctx, &mut ir).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D appearance colors")
    );
}

#[test]
fn body_appearance_color_index_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 1);
    let body = cadmpeg_ir::ids::BodyId::mint("f3d:test:body#one").unwrap();
    let mut ir = appearance_binding_ir(cadmpeg_ir::appearance::AppearanceTarget::Body(body));
    let error = super::super::apply_appearance_base_colors(&ctx, &mut ir).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D body appearance colors")
    );
}

#[test]
fn face_appearance_color_index_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 1);
    let face = cadmpeg_ir::ids::FaceId::mint("f3d:test:face#one").unwrap();
    let mut ir = appearance_binding_ir(cadmpeg_ir::appearance::AppearanceTarget::Face(face));
    let error = super::super::apply_appearance_base_colors(&ctx, &mut ir).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D face appearance colors")
    );
}

fn face_assignment() -> crate::materials::FaceAppearanceAssignment {
    crate::materials::FaceAppearanceAssignment {
        face_guid: "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb".into(),
        visual_guid: crate::records::references::DesignVisualToken::try_from(
            "11111111-2222-3333-4444-555555555555_Post2015".to_owned(),
        )
        .unwrap(),
        color: None,
    }
}

fn face_assignment_ir() -> cadmpeg_ir::document::CadIr {
    use cadmpeg_ir::appearance::Appearance;
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
    let assignment = face_assignment();
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.attributes.push(SourceAttribute {
        id: cadmpeg_ir::ids::AttributeId::mint("f3d:test:attribute#face").unwrap(),
        target: AttributeTarget::Face(cadmpeg_ir::ids::FaceId::mint("f3d:test:face#one").unwrap()),
        name: "ATTRIB_CUSTOM-attrib".into(),
        values: vec![
            AttributeValue::String("NEUTRON_Material_attrib_def".into()),
            AttributeValue::String(assignment.face_guid),
        ],
    });
    ir.model.appearances.push(Appearance {
        id: cadmpeg_ir::ids::AppearanceId::mint("f3d:test:appearance#one").unwrap(),
        name: None,
        asset_guid: None,
        library_id: None,
        textures: Vec::new(),
        visual_guid: Some(assignment.visual_guid.to_string()),
        physical_token: None,
        schema: None,
        category: None,
        base_color: None,
        properties: Default::default(),
    });
    ir
}

macro_rules! face_join_refuses_collection_limit {
    ($name:ident, $limit:expr, $operation:literal) => {
        #[test]
        fn $name() {
            let arena = DecodeArena::new();
            let ctx = context(&arena, $limit);
            let mut ir = face_assignment_ir();
            let error = super::super::resolve_face_appearance_bindings(&ctx, &mut ir, &[face_assignment()]).unwrap_err();
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == $operation));
        }
    };
}

face_join_refuses_collection_limit!(
    face_material_guid_index_refuses_collection_limit,
    1,
    "index F3D face material GUIDs"
);
face_join_refuses_collection_limit!(
    faces_by_material_guid_index_refuses_collection_limit,
    2,
    "index F3D faces by material GUID"
);
face_join_refuses_collection_limit!(
    face_material_group_refuses_collection_limit,
    3,
    "collect F3D faces by material GUID"
);
face_join_refuses_collection_limit!(
    new_face_binding_index_refuses_collection_limit,
    4,
    "index F3D new appearance faces"
);
face_join_refuses_collection_limit!(
    face_binding_collection_refuses_collection_limit,
    5,
    "collect F3D face appearance bindings"
);
face_join_refuses_collection_limit!(
    face_binding_append_refuses_collection_limit,
    6,
    "append F3D face appearance bindings"
);

#[test]
fn face_assignment_index_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let error = super::super::resolve_face_appearance_bindings(&ctx, &mut ir, &[face_assignment()])
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D face appearance assignments")
    );
}

#[test]
fn face_appearance_binding_id_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let assignment = face_assignment();
    let face = cadmpeg_ir::ids::FaceId::mint("f3d:test:face#one").unwrap();
    let error = crate::ids::face_appearance_binding_id_charged(
        &ctx,
        &assignment.face_guid,
        &assignment.visual_guid,
        &face,
    )
    .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
}

#[test]
fn face_appearance_binding_id_preserves_identity_text() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default()).unwrap();
    let assignment = face_assignment();
    let face = cadmpeg_ir::ids::FaceId::mint("f3d:test:face#one").unwrap();
    let charged = crate::ids::face_appearance_binding_id_charged(
        &ctx,
        &assignment.face_guid,
        &assignment.visual_guid,
        &face,
    )
    .unwrap();
    let original = crate::ids::face_appearance_binding_id(
        &assignment.face_guid,
        assignment.visual_guid.identity_key(),
        &face,
    )
    .unwrap();
    assert_eq!(charged, original);
}

#[test]
fn annotation_provenance_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let stream = cadmpeg_ir::annotations::StreamHandle::new(
        &cadmpeg_test_support::service_decode_context(),
        cadmpeg_ir::stream_name!("f3d:native"),
        "fixture stream handle",
    )
    .unwrap();
    let mut annotations = cadmpeg_ir::annotations::AnnotationBuilder::new();
    let error = annotations
        .note(&ctx, "f3d:test:entity#one", &stream, 0, Some("entity"))
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect source provenance")
    );
}

#[test]
fn annotation_provenance_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let stream = cadmpeg_ir::annotations::StreamHandle::new(
        &cadmpeg_test_support::service_decode_context(),
        cadmpeg_ir::stream_name!("f3d:native"),
        "fixture stream handle",
    )
    .unwrap();
    let mut annotations = cadmpeg_ir::annotations::AnnotationBuilder::new();
    let error = annotations
        .note(&ctx, "f3d:test:entity#one", &stream, 0, Some("entity"))
        .unwrap_err();
    // Copying the map key admits retained bytes before the provenance node.
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain source provenance identity")
    );
}

#[test]
fn annotation_exactness_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let mut annotations = cadmpeg_ir::annotations::AnnotationBuilder::new();
    let error = annotations
        .derived(&ctx, "f3d:test:entity#one", "definition")
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_ir::annotations::AnnotationFieldError::Resource(limit)
        if limit.operation == "collect source exactness entities")
    );
}

#[test]
fn annotation_stream_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let error = super::super::annotation_stream(&ctx, "Design/BulkStream.dat").unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "allocate annotation stream handle")
    );
}

#[test]
fn annotation_stream_name_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::annotation_stream(&ctx, "Design/BulkStream.dat").unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D native scope")
    );
}

#[test]
fn native_annotation_route_refuses_collection_limit() {
    let bytes = crate::test_support::zip_test::synthetic_f3d(true);
    let arena = DecodeArena::new();
    let (scan_ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let scan = crate::container::scan(&scan_ctx, root).unwrap();
    let mut native = crate::native::F3dNative::default();
    native
        .design_record_headers
        .push(crate::records::decal::DesignRecordHeader {
            id: "f3d:Design:design-record-header#1".into(),
            record_index: 1,
            class_tag: crate::records::references::DesignClassTag::try_from("310".to_owned())
                .unwrap(),
            byte_offset: 1,
        });
    let limited = context(&arena, 1);
    let error = super::super::populate_annotations(
        &limited,
        &cadmpeg_ir::document::CadIr::empty(),
        &scan,
        &native,
        None,
        &[],
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect source provenance")
    );
}

fn annotation_placement() -> crate::records::sketch_placement::DesignSketchPlacement {
    crate::records::sketch_placement::DesignSketchPlacement {
        id: "f3d:test/BulkStream.dat:design-sketch-placement#4".into(),
        scope_record_index: None,
        entity_id: crate::records::identity::DesignEntityId::from_parts("sketch", 7),
        visibility: None,
        class_tag: "330".to_owned().try_into().unwrap(),
        record_index: 4,
        paired_class_tag: "330".to_owned().try_into().unwrap(),
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            0,
            crate::records::sketch_placement::DesignSketchFrameForm::ScopeCompact,
        )
        .unwrap(),
    }
}

#[test]
fn sketch_annotation_id_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = crate::ids::neutral_sketch_id_charged(&ctx, &annotation_placement()).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
}

#[test]
fn spatial_sketch_annotation_id_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error =
        crate::ids::neutral_spatial_sketch_id_charged(&ctx, &annotation_placement()).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
}

#[test]
fn sketch_constraint_annotation_id_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error =
        crate::ids::neutral_sketch_constraint_id_charged(&ctx, &annotation_placement().id, 4)
            .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
}

#[test]
fn charged_sketch_annotation_ids_preserve_identity_text() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default()).unwrap();
    let placement = annotation_placement();
    assert_eq!(
        crate::ids::neutral_sketch_id_charged(&ctx, &placement).unwrap(),
        crate::ids::neutral_sketch_id(&placement)
    );
    assert_eq!(
        crate::ids::neutral_spatial_sketch_id_charged(&ctx, &placement).unwrap(),
        crate::ids::neutral_spatial_sketch_id(&placement)
    );
    assert_eq!(
        crate::ids::neutral_sketch_constraint_id_charged(&ctx, &placement.id, 4).unwrap(),
        crate::ids::neutral_sketch_constraint_id(&placement.id, 4)
    );
}

macro_rules! append_refuses_collection_limit {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            let arena = DecodeArena::new();
            let ctx = context(&arena, 1);
            let mut target = vec![1u32];
            let error = ctx.extend_vec(
                &mut target,
                vec![2u32, 3u32],
                $operation,
            )
            .unwrap_err();
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == $operation));
            assert_eq!(target, [1]);
        }
    };
}

append_refuses_collection_limit!(
    subd_loss_append_refuses_collection_limit,
    "append F3D T-spline losses"
);
append_refuses_collection_limit!(
    dimension_constraint_append_refuses_collection_limit,
    "append F3D dimension constraints"
);
append_refuses_collection_limit!(
    spatial_dimension_constraint_append_refuses_collection_limit,
    "append F3D spatial dimension constraints"
);
append_refuses_collection_limit!(
    material_note_append_refuses_collection_limit,
    "append F3D material notes"
);
append_refuses_collection_limit!(
    local_component_append_refuses_collection_limit,
    "append F3D local components"
);
append_refuses_collection_limit!(
    local_occurrence_append_refuses_collection_limit,
    "append F3D local occurrences"
);
append_refuses_collection_limit!(
    unresolved_occurrence_append_refuses_collection_limit,
    "append F3D unresolved occurrences"
);

#[test]
fn model_brep_candidate_index_refuses_collection_limit() {
    let bytes = crate::test_support::zip_test::synthetic_f3d(true);
    let arena = DecodeArena::new();
    let (scan_ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let scan = crate::container::scan(&scan_ctx, root).unwrap();
    let blob_names: Vec<String> = crate::container::design_breps(&scan)
        .map(|brep| brep.name.rsplit('/').next().unwrap().to_owned())
        .collect();
    assert!(!blob_names.is_empty());
    let limited = context(&arena, 0);
    let error = super::super::model_brep_candidates(&limited, &scan, &blob_names).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D model BREP candidates")
    );
}

#[test]
fn undecoded_brep_loss_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let mut report = cadmpeg_ir::codec::DecodeBody::new(
        cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {},
    );
    let error = super::super::push_loss_vec(
        &ctx,
        &mut report.losses,
        crate::loss::F3dLossCode::BrepBlobUndecoded,
        format_args!("1 Design-referenced BREP blob(s) could not be decoded."),
        "collect F3D undecoded BREP loss",
        "retain F3D undecoded BREP loss",
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D undecoded BREP loss")
    );
}

#[test]
fn primary_brep_name_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = ctx
        .copy_retained_text("Breps.BlobParts/BREP0.smb", "retain F3D primary BREP name")
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D primary BREP name")
    );
}

#[test]
fn asm_history_collection_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let mut histories = Vec::new();
    let error = ctx
        .push_vec(&mut histories, 1_u32, "collect F3D ASM histories")
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D ASM histories")
    );
    assert!(histories.is_empty());
}

#[test]
fn source_image_copy_refuses_retained_limit() {
    let bytes = crate::test_support::zip_test::synthetic_f3d(true);
    let arena = DecodeArena::new();
    let (scan_ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let scan = crate::container::scan(&scan_ctx, root).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(bytes.len()).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::preserve_source_image(&limited, &scan).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D source image")
    );
}

#[test]
fn unique_asset_append_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let asset = cadmpeg_ir::assets::Asset::try_new(
        cadmpeg_ir::assets::AssetId::mint("f3d:model:asset#one").unwrap(),
        Some("one.png".into()),
        Some("image/png".into()),
        cadmpeg_ir::assets::AssetContent::Embedded {
            data: cadmpeg_ir::assets::AssetData::new(vec![1]).unwrap(),
        },
        None,
    )
    .unwrap();
    let mut assets = Vec::new();
    let error = super::super::extend_unique_assets(&ctx, &mut assets, vec![asset]).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "append F3D unique assets")
    );
    assert!(assets.is_empty());
}

#[test]
fn selected_body_blob_index_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let mut index = std::collections::HashMap::new();
    let error =
        super::super::index_selected_body_key(&ctx, &mut index, "BREP0.smb", 7).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D selected body blobs")
    );
    assert!(index.is_empty());
}

#[test]
fn selected_body_key_index_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 1);
    let mut index = std::collections::HashMap::new();
    let error =
        super::super::index_selected_body_key(&ctx, &mut index, "BREP0.smb", 7).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D selected body keys")
    );
    assert!(index.is_empty());
}

#[test]
fn selected_body_blob_name_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D selected body blob name",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut index = std::collections::HashMap::new();
            super::super::index_selected_body_key(&ctx, &mut index, "BREP0.smb", 7)
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut index = std::collections::HashMap::new();
    let error =
        super::super::index_selected_body_key(&ctx, &mut index, "BREP0.smb", 7).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D selected body blob name")
    );
    assert!(index.is_empty());
}

#[test]
fn body_visibility_lookup_refuses_scoped_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let index = std::collections::HashMap::new();
    let error = super::super::body_visibility_for(&ctx, &index, "BREP0.smb", 7).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "look up F3D body visibility")
    );
}

#[test]
fn body_visibility_collection_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let mut visibilities = Vec::new();
    let error = ctx
        .push_vec(&mut visibilities, 7_u64, "collect F3D body visibilities")
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D body visibilities")
    );
}

macro_rules! mesh_outcome_loss_refuses_collection_limit {
    ($name:ident, $outcome:expr, $operation:literal) => {
        #[test]
        fn $name() {
            let arena = DecodeArena::new();
            let ctx = context(&arena, 0);
            let mut bodies = Vec::new();
            let mut report = cadmpeg_ir::codec::DecodeBody::new(
                cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {},
            );
            let error = super::super::collect_mesh_outcome(
                &ctx,
                &mut bodies,
                &mut report,
                $outcome,
            )
            .unwrap_err();
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == $operation));
            assert!(report.losses.is_empty());
        }
    };
}

mesh_outcome_loss_refuses_collection_limit!(
    unjoined_mesh_loss_refuses_collection_limit,
    crate::design::decode::mesh::MeshContainerOutcome::Unjoined {
        entry_name: "mesh.paramesh".into(),
    },
    "collect F3D unjoined mesh loss"
);
mesh_outcome_loss_refuses_collection_limit!(
    undecoded_mesh_loss_refuses_collection_limit,
    crate::design::decode::mesh::MeshContainerOutcome::Failed {
        entry_name: "mesh.paramesh".into(),
        error: cadmpeg_core::CodecError::Malformed("bad mesh".into()),
    },
    "collect F3D undecoded mesh loss"
);
mesh_outcome_loss_refuses_collection_limit!(
    missing_mesh_loss_refuses_collection_limit,
    crate::design::decode::mesh::MeshContainerOutcome::Missing {
        entry_name: "mesh.paramesh".into(),
    },
    "collect F3D missing mesh loss"
);

#[test]
fn failed_mesh_resource_limit_propagates() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let mut bodies = Vec::new();
    let mut report = cadmpeg_ir::codec::DecodeBody::new(
        cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {},
    );
    let error = super::super::collect_mesh_outcome(
        &ctx,
        &mut bodies,
        &mut report,
        crate::design::decode::mesh::MeshContainerOutcome::Failed {
            entry_name: "mesh.paramesh".into(),
            error: ctx.refuse_codec_limit("synthetic mesh refusal", 0, 1),
        },
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "synthetic mesh refusal")
    );
    assert!(report.losses.is_empty());
}

#[test]
fn mesh_texture_asset_bytes_refuse_retained_limit() {
    let bytes = crate::test_support::zip_test::synthetic_f3d(true);
    let arena = DecodeArena::new();
    let (scan_ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let scan = crate::container::scan(&scan_ctx, root).unwrap();
    let entry_name = &scan.entries.first().unwrap().name;
    assert!(!scan.entry_bytes(entry_name).unwrap().is_empty());
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::mesh_texture_asset_bytes(&limited, &scan, entry_name).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D mesh texture bytes")
    );
}

#[test]
fn mesh_texture_asset_collection_refuses_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let mut assets = Vec::new();
    let error = ctx
        .push_vec(&mut assets, 1_u32, "collect F3D mesh texture assets")
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D mesh texture assets")
    );
}

fn one_texture_table() -> Vec<(String, cadmpeg_ir::assets::AssetId)> {
    vec![(
        "resource:one".into(),
        cadmpeg_ir::assets::AssetId::mint("f3d:model:asset#one").unwrap(),
    )]
}

#[test]
fn mesh_texture_table_copy_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let error = super::super::clone_mesh_texture_table(&ctx, &one_texture_table()).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D mesh texture table")
    );
}

#[test]
fn mesh_texture_table_index_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let mut tables = std::collections::HashMap::new();
    let error = super::super::insert_mesh_texture_table(
        &ctx,
        &mut tables,
        "tessellation:one",
        &one_texture_table(),
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D mesh texture tables")
    );
    assert!(tables.is_empty());
}

#[test]
fn mesh_texture_table_key_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D mesh texture table key",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut tables = std::collections::HashMap::new();
            super::super::insert_mesh_texture_table(
                &ctx,
                &mut tables,
                "tessellation:one",
                &one_texture_table(),
            )
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut tables = std::collections::HashMap::new();
    let error = super::super::insert_mesh_texture_table(
        &ctx,
        &mut tables,
        "tessellation:one",
        &one_texture_table(),
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D mesh texture table key")
    );
    assert!(tables.is_empty());
}

macro_rules! mesh_projection_item_refuses_collection_limit {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            let arena = DecodeArena::new();
            let ctx = context(&arena, 0);
            let mut items = Vec::new();
            let error = ctx.push_vec(&mut items, 1_u32, $operation)
                .unwrap_err();
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == $operation));
        }
    };
}

mesh_projection_item_refuses_collection_limit!(
    mesh_triangle_group_refuses_collection_limit,
    "collect F3D mesh triangle groups"
);
mesh_projection_item_refuses_collection_limit!(
    mesh_corner_normal_refuses_collection_limit,
    "collect F3D mesh corner normals"
);
mesh_projection_item_refuses_collection_limit!(
    mesh_tessellation_refuses_collection_limit,
    "collect F3D mesh tessellations"
);

#[test]
fn mesh_scope_tessellation_index_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let mut index = std::collections::HashMap::new();
    let error = super::super::insert_mesh_scope_tessellations(
        &ctx,
        &mut index,
        "f3d:Design/BulkStream.dat",
        10,
        ["tessellation:one"],
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D mesh scope tessellations")
    );
    assert!(index.is_empty());
}

#[test]
fn mesh_feature_scope_index_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 1);
    let mut index = std::collections::HashMap::new();
    let error = super::super::insert_mesh_scope_tessellations(
        &ctx,
        &mut index,
        "f3d:Design/BulkStream.dat",
        10,
        ["tessellation:one"],
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D mesh feature scopes")
    );
    assert!(index.is_empty());
}

#[test]
fn unresolved_mesh_attribute_loss_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let mut report = cadmpeg_ir::codec::DecodeBody::new(
        cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {},
    );
    let unresolved =
        std::collections::BTreeMap::from([(crate::paramesh::MeshAttributeDomain::Vertex, 1)]);
    let error = super::super::report_unresolved_mesh_attributes(&ctx, &mut report, &unresolved)
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D unresolved mesh attribute loss")
    );
}

#[test]
fn unresolved_mesh_attribute_loss_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D unresolved mesh attribute loss",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut report = cadmpeg_ir::codec::DecodeBody::new(
                cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {},
            );
            let unresolved = std::collections::BTreeMap::from([(
                crate::paramesh::MeshAttributeDomain::Vertex,
                1,
            )]);
            super::super::report_unresolved_mesh_attributes(&ctx, &mut report, &unresolved)
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut report = cadmpeg_ir::codec::DecodeBody::new(
        cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {},
    );
    let unresolved =
        std::collections::BTreeMap::from([(crate::paramesh::MeshAttributeDomain::Vertex, 1)]);
    let error = super::super::report_unresolved_mesh_attributes(&ctx, &mut report, &unresolved)
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D unresolved mesh attribute loss")
    );
}

#[test]
fn source_attribute_map_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let mut attributes = std::collections::BTreeMap::new();
    let error = ctx
        .insert_btree_map(
            &mut attributes,
            "active_brep".to_owned(),
            "BREP0.smb".to_owned(),
            "collect F3D source attributes",
        )
        .map(|_| ())
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D source attributes")
    );
    assert!(attributes.is_empty());
}

#[test]
fn source_attribute_value_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut attributes = std::collections::BTreeMap::new();
    let error = ctx
        .copy_retained_text("BREP0.smb", "retain F3D source attribute value")
        .and_then(|copy| {
            ctx.insert_btree_map(
                &mut attributes,
                "active_brep".to_owned(),
                copy,
                "collect F3D source attributes",
            )
        })
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D source attribute value")
    );
    assert!(attributes.is_empty());
}

#[test]
fn metadata_source_attributes_propagate_collection_limit() {
    let bytes = crate::test_support::zip_test::synthetic_f3d(true);
    let arena = DecodeArena::new();
    let (scan_ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let scan = crate::container::scan(&scan_ctx, root).unwrap();
    let limited = context(&arena, 0);
    let error = super::super::build_metadata_ir(&limited, &scan)
        .err()
        .expect("metadata attribute must refuse");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D source attributes")
    );
}

#[test]
fn geometry_loss_collection_refuses_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let error = super::super::geometry_losses(&ctx, &crate::brep::Brep::default()).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D geometry losses")
    );
}

#[test]
fn geometry_loss_text_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D geometry loss",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            super::super::geometry_losses(&ctx, &crate::brep::Brep::default()).map(|_| ())
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::geometry_losses(&ctx, &crate::brep::Brep::default()).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D geometry loss")
    );
}

#[test]
fn geometry_kind_counts_keep_sorted_report_text() {
    let counts = std::collections::BTreeMap::from([("plane".into(), 2), ("spline".into(), 3)]);
    assert_eq!(
        super::super::KindCounts(&counts).to_string(),
        "plane=2, spline=3"
    );
}

#[test]
fn metadata_unknown_collection_refuses_limit() {
    let bytes = crate::test_support::zip_test::synthetic_f3d(true);
    let arena = DecodeArena::new();
    let (scan_ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let scan = crate::container::scan(&scan_ctx, root).unwrap();
    let brep = crate::container::select_fallback_brep(&scan).unwrap();
    let limited = context(&arena, 0);
    let mut unknowns = Vec::new();
    let error = super::super::append_metadata_unknown(&limited, &mut unknowns, brep).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D metadata unknowns")
    );
    assert!(unknowns.is_empty());
}

#[test]
fn metadata_unknown_id_refuses_retained_limit() {
    let bytes = crate::test_support::zip_test::synthetic_f3d(true);
    let arena = DecodeArena::new();
    let (scan_ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let scan = crate::container::scan(&scan_ctx, root).unwrap();
    let brep = crate::container::select_fallback_brep(&scan).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut unknowns = Vec::new();
    let error = super::super::append_metadata_unknown(&limited, &mut unknowns, brep).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D native record ID")
    );
}

#[test]
fn archive_member_dialect_clone_refuses_collection_limit() {
    let bytes = crate::test_support::zip_test::synthetic_f3d(true);
    let arena = DecodeArena::new();
    let (scan_ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let scan = crate::container::scan(&scan_ctx, root).unwrap();
    let layers = cadmpeg_core::dialect::DialectLayers::of(scan.kind.dialect().clone());
    let limited = context(&arena, 0);
    let error = super::super::decode_archive_member(&limited, &scan, &layers)
        .err()
        .expect("dialect copy must refuse");
    assert!(
        matches!(&error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy dialect layers"),
        "{error:?}"
    );
}

#[test]
fn text_brep_fact_name_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = ctx
        .copy_retained_text(
            "Breps.BlobParts/BREP0.sat",
            "retain F3D text B-rep fact name",
        )
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D text B-rep fact name")
    );
}

#[test]
fn text_brep_loss_text_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = ctx
        .format_retained(
            format_args!("text carrier {}", "BREP0.sat"),
            "report F3D text geometry loss",
        )
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "report F3D text geometry loss")
    );
}

#[test]
fn joined_text_brep_names_refuse_retained_limit() {
    let bytes = crate::test_support::assembly_test::f3d_with_text_brep(&[
        "FusionAssetName[Active]/Breps.BlobParts/BREP0.sat",
    ]);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let scan = crate::container::scan(&ctx, root).unwrap();
    let mut limited_policy = DecodePolicy::service();
    limited_policy.limits.max_retained_bytes = 0;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &limited_policy).unwrap();
    let error = super::super::join_text_brep_names(&limited, &scan).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "join F3D text B-rep names")
    );
}

fn dimension_native() -> crate::native::F3dNative {
    use crate::records::parameters::{
        DesignCompanionPayload, DesignParameter, DesignParameterDraft, DesignParameterOwner,
        DesignParameterOwnerWire, DesignParameterSource,
    };
    use crate::records::references::DesignClassTag;

    let stream = "f3d:test/BulkStream.dat";
    let parameter = DesignParameter::try_from(DesignParameterDraft {
        id: format!("{stream}:design-parameter#28"),
        byte_offset: 0,
        class_tag: DesignClassTag::try_from("305".to_owned()).unwrap(),
        record_index: 28,
        source_ordinal: 0,
        source: DesignParameterSource::new(
            "Linear Dimension-2".into(),
            Some(29),
            Some(crate::records::identity::Located {
                value: crate::records::parameters::DesignParameterDiscriminator::Code0,
                offset: 22,
            }),
        )
        .unwrap(),
        expression: "5 mm".into(),
        expression_offset: 40,
        source_kind_offset: 60,
        unit: Some(crate::records::identity::RecordedValue {
            value: "mm".into(),
            offset: 90,
        }),
        name: "d1".into(),
        name_offset: 100,
        evaluated_value: 0.5,
        evaluated_value_offset: 110,
    })
    .unwrap();
    let owner = DesignParameterOwner::try_from(DesignParameterOwnerWire {
        id: format!("{stream}:design-parameter-owner#29"),
        byte_offset: 120,
        frame_length: 104,
        class_tag: DesignClassTag::try_from("292".to_owned()).unwrap(),
        record_index: 29,
        scope_record_index: 1,
        local_ordinal: 0,
        evaluated_value: 0.5,
        evaluated_value_offset: 160,
        parameter_record_index: 28,
        owned_ordinal: 0,
        variant: Some(0),
        companion_record_index: 30,
    })
    .unwrap();
    let companion = crate::records::parameters::DesignParameterCompanion::unbound(
        format!("{stream}:design-parameter-companion#30"),
        220,
        DesignClassTag::try_from("408".to_owned()).unwrap(),
        30,
        29,
        std::num::NonZeroU64::new(1).unwrap(),
        262,
    )
    .bound(DesignCompanionPayload::new(278, 100, Vec::new()));
    crate::native::F3dNative {
        design_parameters: vec![parameter],
        design_parameter_owners: vec![owner],
        design_parameter_companions: vec![companion],
        ..crate::native::F3dNative::default()
    }
}

#[test]
fn container_only_dimension_companion_index_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let error = ctx
        .collect_hash_set(
            [("f3d:Design", 30)],
            "index F3D container-only dimension companions",
        )
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D container-only dimension companions")
    );
}

#[test]
fn container_only_neutral_parameter_id_refuses_retained_limit() {
    let native = dimension_native();
    let parameter = native.design_parameters.first().unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = crate::ids::neutral_parameter_id_charged(&ctx, parameter).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
}

#[test]
fn container_only_neutral_parameter_id_matches_identity_shape() {
    let native = dimension_native();
    let parameter = native.design_parameters.first().unwrap();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default()).unwrap();
    assert_eq!(
        crate::ids::neutral_parameter_id_charged(&ctx, parameter).unwrap(),
        crate::ids::neutral_parameter_id(parameter),
    );
}

#[test]
fn dimension_owner_index_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 1);
    let native = dimension_native();
    let ir = cadmpeg_ir::document::CadIr::empty();
    let error = super::super::unresolved_dimension_companion_count(&ctx, &native, &ir).unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "index F3D dimension owners"
    ));
}

#[test]
fn unresolved_dimension_loss_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 2);
    let native = dimension_native();
    let ir = cadmpeg_ir::document::CadIr::empty();
    let mut report =
        cadmpeg_ir::codec::DecodeBody::new(cadmpeg_ir::report::decode::DecodeTransfer::full(true));
    let error =
        super::super::report_unresolved_dimension_companions(&ctx, &mut report, &native, &ir)
            .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "report unresolved F3D dimensions"
    ));
}

#[test]
fn typed_dimension_index_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let native = crate::native::F3dNative {
        design_dimension_recipe_records: vec![
            crate::records::dimensions::DesignDimensionRecipeRecord {
                id: "f3d:test:design-dimension-recipe-record#1".into(),
                companion_record_index: 1,
                recipe_ordinal: 0,
                recipe_id: "f3d:test:construction-recipe#1".into(),
                recipe_kind: crate::records::recipes::ConstructionRecipeKind::Edge,
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("423".to_owned())
                    .unwrap(),
                record_index: 1,
                frame_length: 100,
                prefix_offset: 20,
                prefix_bytes: Vec::new().into(),
                references: Vec::new(),
                program_offset: 40,
                program: vec![-1],
                matching_edge_operand_ids: Vec::new(),
            },
        ],
        ..crate::native::F3dNative::default()
    };
    let ir = cadmpeg_ir::document::CadIr::empty();
    let error = super::super::unresolved_dimension_companion_count(&ctx, &native, &ir).unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "index F3D typed dimension companions"
    ));
}

#[test]
fn archive_entries_do_not_hide_new_model_entities() {
    let bytes = crate::test_support::assembly_test::f3d_without_brep(
        "assembly",
        "root.f3d",
        &[("part.f3d", crate::test_support::assembly_test::XREF_ROLE)],
    );
    crate::test_support::with_decode_context(|scan_ctx| {
        let scan =
            crate::container::scan(scan_ctx, cadmpeg_core::decode::View::over_retained(&bytes))
                .unwrap();
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = cadmpeg_core::decode::u64_from_index(scan.entries.len());
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let error = super::super::decode_scanned_document(
                ctx,
                &scan,
                crate::report::ReportScope::Standalone,
            )
            .err()
            .unwrap();
            let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
                panic!("model population must refuse");
            };
            assert_eq!(
                limit.dimension,
                cadmpeg_core::decode::ResourceDimension::Entities
            );
            assert_eq!(limit.operation, "admit F3D entities");
            assert_eq!(Some(limit), ctx.resource_refusal());
        });
    });
}
