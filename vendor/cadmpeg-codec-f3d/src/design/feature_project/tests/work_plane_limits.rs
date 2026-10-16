// SPDX-License-Identifier: Apache-2.0

#[test]
fn work_plane_vertex_recipe_id_refuses_retained_limit() {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    use crate::records::feature::work_geometry::{DesignVertexRecipe, DesignWorkPlaneConstruction};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let recipe = |record_index, vertex| {
        DesignVertexRecipe::try_new(
            crate::records::feature::work_geometry::DesignVertexRecipeDraft {
                record_index,
                byte_offset: u64::from(record_index),
                class_tag: crate::records::references::DesignClassTag::try_from("306".to_owned())
                    .unwrap(),
                paired_byte_offset: u64::from(record_index) + 16,
                paired_class_tag: crate::records::references::DesignClassTag::try_from(
                    "261".to_owned(),
                )
                .unwrap(),
                recipe_record_index: record_index + 3,
                recipe_record_byte_offset: u64::from(record_index) + 32,
                recipe_id: format!("f3d:test:recipe#{record_index}"),
                recipe_prefix_offset: u64::from(record_index) + 43,
                recipe_prefix_bytes: Vec::new(),
                recipe_references: Vec::new(),
                recipe_program_offset: 4,
                recipe_program: vec![0],
                resolution: Some(
                    crate::records::feature::work_geometry::DesignVertexResolution::new(4, vertex)
                        .unwrap(),
                ),
                next_record_index: record_index + 5,
                next_byte_offset: u64::from(record_index) + 200,
            },
        )
        .unwrap()
    };
    let mut plane =
        DesignParameterScope::empty("f3d:test:scope#20", DesignFeatureKind::WorkPlane, 20);
    plane.with_work_plane_transform(
        crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
    );
    if let Some(frame) = plane.work_plane_frame_mut() {
        frame.work_plane_construction = Some(
            DesignWorkPlaneConstruction::try_new(
                21,
                Box::new([recipe(22, 43), recipe(27, 64), recipe(32, 84)]),
            )
            .unwrap(),
        );
    }
    let transform = crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY.into();
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| super::super::project_work_plane(
            decode_ctx, &plane, transform
        ))
        .unwrap(),
        FeatureDefinition::Operation(FeatureOperation::DatumThreePointPlane { .. })
    ));
    let mut policy = DecodePolicy::default();
    let feature = crate::ids::neutral_feature_id(&plane);
    let prefix = crate::ids::history_input_prefix(
        &feature.key(),
        plane.work_plane_construction().unwrap().inputs()[0]
            .resolution
            .unwrap()
            .state_id,
    );
    policy.limits.max_retained_bytes =
        u64::try_from(feature.as_str().len() + prefix.as_str().len()).unwrap();
    let arena = DecodeArena::new();

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(super::super::project_work_plane(&ctx, &plane, transform),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::RetainedBytes
                && failure.operation == "f3d WorkPlane vertex recipe id")
    );
}
