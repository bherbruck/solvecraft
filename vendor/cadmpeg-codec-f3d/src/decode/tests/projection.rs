// SPDX-License-Identifier: Apache-2.0
//! Decode-module projection and completeness unit tests.
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

use cadmpeg_test_support::wire;

use super::super::{
    bind_mesh_feature_definitions, face_selection_is_resolved, feature_definition_is_incomplete,
    incomplete_feature_families, mesh_attribute_channels, mesh_texture_assignments,
    report_design_projection_gaps, MeshProjection,
};
use crate::loss::F3dLossCode;
use crate::native::F3dNative;
use crate::records::feature::scope::DesignParameterScope;

fn with_test_ctx<T>(run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("test decode context");
    run(&ctx)
}

fn design_projection_gaps(
    ir: &cadmpeg_ir::document::CadIr,
    native: &F3dNative,
) -> super::super::DesignProjectionGaps {
    with_test_ctx(|ctx| super::super::design_projection_gaps(ctx, ir, native).unwrap())
}

#[test]
fn active_face_substitutions_have_a_distinct_loss_note() {
    let ir = cadmpeg_ir::document::CadIr::empty();
    let mut native = F3dNative::default();
    native.design_face_operands.push(
        serde_json::from_value(serde_json::json!({
            "id": "f3d:test:face-operand#200",
            "scope_record_index": 100,
            "scope_reference_ordinal": 0,
            "record_index": 200,
            "byte_offset": 0,
            "class_tag": "346",
            "paired_byte_offset": 325,
            "paired_class_tag": "262",
            "recipe_record_index": 203,
            "recipe_record_byte_offset": 341,
            "recipe_id": "f3d:test:recipe#201",
            "recipe_prefix_offset": 352,
            "recipe_prefix_bytes": "",
            "recipe_references": [],
            "recipe_kind": "bounded_face",
            "recipe_program_offset": 0,
            "recipe_program": [],
            "recipe_node_offsets": [],
            "recipe_nodes": [],
            "resolved_active_face": "f3d:brep:entity#30",
            "next_record_index": 202,
            "next_byte_offset": 469
        }))
        .expect("active face operand"),
    );
    let mut report = cadmpeg_ir::codec::DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(true),
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: Default::default(),
    };

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    report_design_projection_gaps(&ctx, &mut report, &ir, &native).unwrap();

    let loss = report
        .losses
        .iter()
        .find(|loss| loss.code == F3dLossCode::FeatureFaceSelectionActiveSubstituted.kind())
        .expect("active-face substitution loss");
    assert_eq!(loss.message, "1 legacy face operand(s) use a current active-BREP face because no unique preceding-state face slot resolved.");
}

#[test]
fn mesh_feature_binds_tessellations_in_design_body_order() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

    let scope_id = "f3d:Design/BulkStream.dat:design-parameter-scope#10";
    let mut scope = DesignParameterScope::empty(
        scope_id,
        crate::records::feature::scope::DesignFeatureKind::BaseMeshFeature,
        10,
    );
    // The feature's owning entity reference is distinct from its scope index.
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![221]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut features = vec![Feature {
        id: FeatureId::mint("test:model:feature#mesh-import").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: Some("Base Mesh Feature".into()),
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "Base Mesh Feature".into(),
                parameters: std::collections::BTreeMap::new(),
            }),
        ),
        native_ref: Some(scope_id.into()),
    }];
    let projection = MeshProjection {
        count: 2,
        tessellations_by_scope: std::collections::HashMap::from([(
            ("f3d:Design/BulkStream.dat".into(), 10),
            vec!["tessellation:z-body".into(), "tessellation:a-body".into()],
        )]),
    };

    with_test_ctx(|ctx| bind_mesh_feature_definitions(ctx, &mut features, &[scope], &projection))
        .unwrap();

    assert_eq!(
        *features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::MeshImport {
            tessellations: vec!["tessellation:z-body".into(), "tessellation:a-body".into(),]
                .try_into()
                .unwrap(),
        })
    );
    assert!(!feature_definition_is_incomplete(
        features[0].evaluation.definition()
    ));
}

#[test]
fn mesh_feature_tessellation_collection_refuses_limit() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

    let scope_id = "f3d:Design/BulkStream.dat:design-parameter-scope#10";
    let scope = DesignParameterScope::empty(
        scope_id,
        crate::records::feature::scope::DesignFeatureKind::BaseMeshFeature,
        10,
    );
    let mut features = vec![Feature {
        id: FeatureId::mint("test:model:feature#mesh-import-limit").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Base Mesh Feature".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "Base Mesh Feature".into(),
                parameters: Default::default(),
            }),
        ),
        native_ref: Some(scope_id.into()),
    }];
    let projection = MeshProjection {
        count: 1,
        tessellations_by_scope: std::collections::HashMap::from([(
            ("f3d:Design/BulkStream.dat".into(), 10),
            vec!["tessellation:one".into()],
        )]),
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error =
        bind_mesh_feature_definitions(&ctx, &mut features, &[scope], &projection).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D mesh feature tessellations")
    );
}

#[test]
fn mesh_feature_tessellation_lookup_refuses_scoped_limit() {
    let projection = MeshProjection {
        count: 0,
        tessellations_by_scope: std::collections::HashMap::new(),
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::mesh_feature_tessellations(
        &ctx,
        &projection,
        "f3d:Design/BulkStream.dat",
        10,
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "look up F3D mesh feature tessellations")
    );
}

#[test]
fn mesh_texture_ids_resolve_through_design_table_order() {
    use cadmpeg_ir::assets::AssetId;
    use cadmpeg_ir::tessellation::TessellationTextureAssignment;

    let first = AssetId::mint("synthetic:test:id#asset:first").expect("identity grammar");
    let second = AssetId::mint("synthetic:test:id#asset:second").expect("identity grammar");
    let textures = [
        ("resource:first".into(), first.clone()),
        ("resource:second".into(), second.clone()),
        ("resource:third".into(), first.clone()),
    ];
    assert_eq!(
        with_test_ctx(|ctx| mesh_texture_assignments(ctx, Some(&[0, 2, 1, 3, 2]), &textures, 5))
            .expect("texture assignments"),
        vec![
            TessellationTextureAssignment {
                source_id: Some("resource:first".into()),
                texture: first,
                triangles: vec![2],
            },
            TessellationTextureAssignment {
                source_id: Some("resource:second".into()),
                texture: second,
                triangles: vec![1, 4],
            },
            TessellationTextureAssignment {
                source_id: Some("resource:third".into()),
                texture: AssetId::mint("synthetic:test:id#asset:first").expect("identity grammar"),
                triangles: vec![3],
            },
        ]
    );
    assert!(matches!(
        with_test_ctx(|ctx| mesh_texture_assignments(
            ctx,
            Some(&[2]),
            &[(
                "resource:only".into(),
                AssetId::mint("synthetic:test:id#asset:only").expect("identity grammar")
            )],
            1,
        )),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn mesh_texture_assignments_report_collection_limit() {
    let asset =
        cadmpeg_ir::assets::AssetId::mint("synthetic:test:id#asset:one").expect("identity grammar");
    let textures = [("resource:one".into(), asset)];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("test decode context");
    let error = mesh_texture_assignments(&ctx, Some(&[1]), &textures, 1)
        .expect_err("one texture group exceeds the collection limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && limit.operation == "f3d mesh texture assignments")
    );
}

fn one_mesh_texture() -> [(String, cadmpeg_ir::assets::AssetId); 1] {
    [(
        "resource:one".into(),
        cadmpeg_ir::assets::AssetId::mint("synthetic:test:id#asset:one").unwrap(),
    )]
}

#[test]
fn mesh_texture_triangle_group_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = mesh_texture_assignments(&ctx, Some(&[1]), &one_mesh_texture(), 1).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D mesh texture triangles")
    );
}

#[test]
fn mesh_texture_output_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = mesh_texture_assignments(&ctx, Some(&[1]), &one_mesh_texture(), 1).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D mesh texture assignments")
    );
}

#[test]
fn mesh_texture_source_id_refuses_retained_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy F3D mesh texture source ID",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            mesh_texture_assignments(&ctx, Some(&[1]), &one_mesh_texture(), 1).map(|_| ())
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = mesh_texture_assignments(&ctx, Some(&[1]), &one_mesh_texture(), 1).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D mesh texture source ID")
    );
}

#[test]
fn mesh_texture_asset_id_refuses_retained_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy F3D mesh texture asset ID",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            mesh_texture_assignments(&ctx, Some(&[1]), &one_mesh_texture(), 1).map(|_| ())
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = mesh_texture_assignments(&ctx, Some(&[1]), &one_mesh_texture(), 1).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D mesh texture asset ID")
    );
}

#[test]
fn indexed_mesh_channels_project_default_and_override_selectors() {
    let attribute = crate::paramesh::MeshAttribute {
        role: 4,
        resource_guid: None,
        authored_name: None,
        groups: crate::paramesh::UniqueFaceGroups::default(),
        elements: crate::paramesh::MeshElements::Float {
            width: crate::paramesh::FloatWidth::Quad,
            values: (0..80).collect(),
        },
        addressing: crate::paramesh::MeshAttributeAddressing::Corner(vec![0, 2]),
    };
    let mut unresolved = std::collections::BTreeMap::new();
    let channels = crate::test_support::with_decode_context(|ctx| {
        mesh_attribute_channels(ctx, &[attribute], 3, &[[0, 1, 2]], &mut unresolved)
            .expect("mesh attribute budget")
    });

    assert!(unresolved.is_empty());
    assert_eq!(channels.len(), 1);
    assert!(matches!(
        wire::field::<cadmpeg_ir::tessellation::ChannelAddressing>(&channels[0], "addressing"),
        cadmpeg_ir::tessellation::ChannelAddressing::Corner { .. }
    ));
    assert_eq!(channels[0].count(), 5);
    assert_eq!(channels[0].indices(), [3, 1, 4]);
    assert_eq!(channels[0].data(), (0..80).collect::<Vec<_>>());
}

#[test]
fn presentation_timeline_objects_are_not_incomplete_modeling_features() {
    let native = |kind: &str| {
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Native {
                kind: kind.into(),
                parameters: std::collections::BTreeMap::new(),
            },
        )
    };

    assert!(!feature_definition_is_incomplete(&native("Canvas")));
    assert!(!feature_definition_is_incomplete(&native("Decal")));
    assert!(feature_definition_is_incomplete(&native("Fillet")));
}

#[test]
fn full_round_fillet_with_automatic_sides_is_complete() {
    use cadmpeg_ir::features::{
        edge_treatments::FullRoundSideSelection, FaceSelection, Feature, FeatureDefinition,
        FeatureId, FeatureOperation,
    };

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.features.push(Feature {
        id: FeatureId::mint("test:model:feature#full-round").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: Some("Fillet".into()),
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::FullRoundFillet {
                groups: vec![
                    cadmpeg_ir::features::edge_treatments::FullRoundFilletGroup::new(
                        FaceSelection::Resolved {
                            faces: vec!["test:model:face#center"
                                .try_into()
                                .expect("valid identity")],
                            native: "native:center-group".into(),
                        },
                        FullRoundSideSelection::Automatic,
                        FullRoundSideSelection::Automatic,
                        &cadmpeg_test_support::service_decode_context(),
                    )
                    .expect("operand admission")
                    .unwrap(),
                ]
                .try_into()
                .unwrap(),
            }),
        ),
        native_ref: None,
    });

    assert!(!feature_definition_is_incomplete(
        ir.model.features[0].evaluation.definition()
    ));
    assert_eq!(
        design_projection_gaps(&ir, &F3dNative::default()).incomplete_features,
        0
    );
}

#[test]
fn extrude_completeness_requires_resolved_profile_start_and_termination() {
    let extrude =
        |profile: serde_json::Value, start: serde_json::Value, termination: serde_json::Value| {
            serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(serde_json::json!({
                "definition": "extrude",
                "profile": profile,
                "start": start,
                "extent": {
                    "kind": "one_sided",
                    "side": {"termination": termination}
                },
                "solid": true,
                "op": "new_body"
            }))
            .expect("Extrude definition")
        };
    let sketch_profile = serde_json::json!({
        "kind": "sketch_profiles",
        "value": {"sketch": "test:model:sketch#1", "profiles": [0]}
    });
    let profile_start = serde_json::json!({"kind": "profile_plane"});
    let blind = serde_json::json!({"kind": "blind", "length": 10.0});

    assert!(!feature_definition_is_incomplete(&extrude(
        sketch_profile.clone(),
        profile_start.clone(),
        blind.clone(),
    )));
    assert!(feature_definition_is_incomplete(&extrude(
        serde_json::json!({"kind": "native", "value": "native:profile"}),
        profile_start.clone(),
        blind.clone(),
    )));
    assert!(feature_definition_is_incomplete(&extrude(
        sketch_profile.clone(),
        serde_json::json!({"kind": "unresolved"}),
        blind,
    )));
    assert!(feature_definition_is_incomplete(&extrude(
        sketch_profile,
        profile_start,
        serde_json::json!({"kind": "to_face", "face": {"kind": "unresolved"}}),
    )));
}

#[test]
fn hole_completeness_requires_support_placement_size_and_extent() {
    let complete: cadmpeg_ir::features::FeatureDefinition =
        serde_json::from_value(serde_json::json!({
            "definition": "hole",
            "face": {"kind": "faces", "value": ["test:model:face#support"]},
            "placements": [{
                "kind": "directed",
                "position": {"x": 1.0, "y": 2.0, "z": 3.0},
                "direction": {"x": 0.0, "y": 0.0, "z": -1.0}
            }],
            "shape": {
                "construction": {
                    "construction": "form",
                    "kind": {"kind": "simple_drilled", "drill_point_angle": 2.0}
                },
                "diameter": 5.0
            },
            "extent": {"kind": "blind", "length": 10.0}
        }))
        .expect("complete Hole definition");
    assert!(!feature_definition_is_incomplete(&complete));

    let mut missing_placement = complete.clone();
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Hole { placements, .. },
    ) = &mut missing_placement
    else {
        panic!("Hole definition");
    };
    *placements = None;
    assert!(feature_definition_is_incomplete(&missing_placement));

    let mut native_support = complete.clone();
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Hole { face, .. },
    ) = &mut native_support
    else {
        panic!("Hole definition");
    };
    *face = Some(cadmpeg_ir::features::FaceSelection::Native(
        "native:support".into(),
    ));
    assert!(feature_definition_is_incomplete(&native_support));

    let mut missing_extent = complete;
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Hole { extent, .. },
    ) = &mut missing_extent
    else {
        panic!("Hole definition");
    };
    *extent = None;
    assert!(feature_definition_is_incomplete(&missing_extent));
}

#[test]
fn face_selection_resolution_accepts_complete_generated_and_partial_members() {
    use cadmpeg_ir::features::{FaceSelection, FeatureId, GeneratedFaceRef};
    use cadmpeg_ir::ids::{FeatureInputTopologyId, HistoricalFaceId};

    assert!(face_selection_is_resolved(
        &FaceSelection::generated(
            vec![GeneratedFaceRef::new(
                FeatureId::mint("test:model:feature#source").expect("identity grammar"),
                "test:model:face#1".into(),
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("selection reference admission")
            .unwrap()],
            "native:generated-face".into(),
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("selection reference admission")
        .unwrap()
    ));
    assert!(!face_selection_is_resolved(
        &FaceSelection::historical_partial(
            FeatureInputTopologyId::mint("test:model:feature-input#state:1")
                .expect("identity grammar"),
            vec![HistoricalFaceId::mint("test:model:face#1").expect("identity grammar")],
            vec!["native:missing-face".into()],
            "native:historical-face".into(),
            &cadmpeg_test_support::service_decode_context()
        )
        .expect("selection storage is admitted")
        .unwrap()
    ));
}

#[test]
fn filled_surface_completeness_requires_boundary_conditions_support_and_merge() {
    use cadmpeg_ir::features::{
        FaceSelection, FeatureDefinition, FeatureOperation, PathRef, SurfaceBoundary,
        SurfaceContinuity,
    };
    use cadmpeg_ir::ids::{EdgeId, FaceId};

    let surface = |support_faces, continuity, merge_result| {
        FeatureDefinition::Operation(FeatureOperation::FilledSurface {
            boundary: SurfaceBoundary::Path(PathRef::Edges(vec![EdgeId::mint(
                "test:model:edge#1",
            )
            .expect("identity grammar")])),
            support_faces,
            continuity: cadmpeg_ir::features::FilledSurfaceContinuityState::uniform(continuity),
            merge_result,
        })
    };

    assert!(!feature_definition_is_incomplete(&surface(
        FaceSelection::Faces(Vec::new()),
        SurfaceContinuity::Contact,
        Some(false),
    )));
    assert!(feature_definition_is_incomplete(&surface(
        FaceSelection::Faces(Vec::new()),
        SurfaceContinuity::Contact,
        None,
    )));
    assert!(feature_definition_is_incomplete(&surface(
        FaceSelection::Faces(Vec::new()),
        SurfaceContinuity::Tangent,
        Some(true),
    )));
    assert!(!feature_definition_is_incomplete(&surface(
        FaceSelection::Faces(vec![
            FaceId::mint("test:model:face#support").expect("identity grammar")
        ]),
        SurfaceContinuity::Curvature,
        Some(true),
    )));
}

#[test]
fn sheet_metal_completeness_requires_neutral_profiles_and_edges() {
    let definition = |value| {
        serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(value)
            .expect("sheet-metal definition")
    };
    let base_flange = |profile| {
        definition(serde_json::json!({
            "definition": "sheet_metal_base_flange",
            "profile": profile,
            "thickness": 2.5,
            "side": "forward"
        }))
    };
    let edge_flange = |edges| {
        definition(serde_json::json!({
            "definition": "sheet_metal_edge_flange",
            "edges": edges,
            "height": {"kind": "distance", "value": 25.0},
            "angle": std::f64::consts::FRAC_PI_2,
            "height_datum": "outer_faces",
            "bend_position": "inside",
            "width": {"kind": "full_edge"},
            "bend_radius": 2.5
        }))
    };
    let hem = |edges| {
        definition(serde_json::json!({
            "definition": "sheet_metal_hem",
            "edges": edges,
            "form": {"kind": "flat", "value": {"length": 10.0}},
            "direction": "forward",
            "bend_radius": 2.5
        }))
    };

    assert!(!feature_definition_is_incomplete(&base_flange(
        serde_json::json!({"kind": "sketch", "value": "test:model:sketch#1"}),
    )));
    assert!(feature_definition_is_incomplete(&base_flange(
        serde_json::json!({"kind": "native", "value": "native:profile"}),
    )));
    assert!(!feature_definition_is_incomplete(&edge_flange(
        serde_json::json!({"kind": "edges", "value": ["test:model:edge#1"]}),
    )));
    assert!(feature_definition_is_incomplete(&edge_flange(
        serde_json::json!({"kind": "native", "value": "native:edges"}),
    )));
    assert!(!feature_definition_is_incomplete(&hem(
        serde_json::json!({"kind": "edges", "value": ["test:model:edge#1"]}),
    )));
    assert!(feature_definition_is_incomplete(&hem(
        serde_json::json!({"kind": "native", "value": "native:edges"}),
    )));
}

#[test]
fn selected_face_and_edge_features_require_neutral_operands() {
    let definition = |value| {
        serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(value)
            .expect("selected feature definition")
    };
    let fillet = |edges| {
        definition(serde_json::json!({
            "definition": "fillet",
            "groups": [{
                "edges": edges,
                "radius": {"kind": "constant", "radius": 5.0},
                "tangency_weight": 1.0
            }]
        }))
    };

    assert!(!feature_definition_is_incomplete(&fillet(
        serde_json::json!({"kind": "edges", "value": ["test:model:edge#1"]}),
    )));
    assert!(feature_definition_is_incomplete(&fillet(
        serde_json::json!({"kind": "native", "value": "native:edges"}),
    )));
    assert!(!feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "delete_face",
            "faces": {"kind": "faces", "value": ["test:model:face#1"]},
            "heal": true
        }),
    )));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "delete_face",
            "faces": {"kind": "native", "value": "native:faces"},
            "heal": true
        }),
    )));
    assert!(!feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "offset_surface",
            "faces": {"kind": "faces", "value": ["test:model:face#1"]},
            "distance": 2.0
        }),
    )));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "offset_surface",
            "faces": {"kind": "faces", "value": ["test:model:face#1"]},
            "distance": null
        }),
    )));
}

#[test]
fn form_and_primitive_completeness_requires_construction_payloads() {
    let definition = |value| {
        serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(value)
            .expect("construction definition")
    };

    assert!(!feature_definition_is_incomplete(&definition(
        serde_json::json!({"definition": "form", "cages": ["test:model:subd#1"]}),
    )));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({"definition": "form", "cages": []}),
    )));
    assert!(!feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "block",
            "dimensions": [30.0, 40.0, 20.0],
            "placement": [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0]
            ],
            "op": "join"
        }),
    )));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "block",
            "dimensions": [30.0, 40.0, 20.0],
            "placement": null,
            "op": "join"
        }),
    )));
    assert!(!feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "primitive",
            "solid": {
                "kind": "cylinder",
                "radius": 15.0,
                "height": 7.0,
                "angle": std::f64::consts::TAU
            },
            "op": "join"
        }),
    )));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "primitive",
            "solid": {
                "kind": "cylinder",
                "radius": 15.0,
                "height": 7.0,
                "angle": std::f64::consts::TAU
            },
            "op": "unresolved"
        }),
    )));
}

#[test]
fn profile_and_boolean_features_require_resolved_operation_inputs() {
    let definition = |value| {
        serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(value)
            .expect("profile or Boolean definition")
    };

    let sweep = definition(serde_json::json!({
        "definition": "sweep",
        "shape": {
            "mode": "solid",
            "op": "join",
            "section": {
                "kind": "profile",
                "value": {"kind": "sketch", "value": "test:model:sketch#section"}
            }
        },
        "path": {"kind": "edges", "value": ["test:model:edge#path"]}
    }));
    assert!(!feature_definition_is_incomplete(&sweep));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "sweep",
            "shape": {
                "mode": "solid",
                "op": "join",
                "section": {
                    "kind": "profile",
                    "value": {"kind": "native", "value": "native:section"}
                }
            },
            "path": {"kind": "edges", "value": ["test:model:edge#path"]}
        }),
    )));

    let chamfer = definition(serde_json::json!({
        "definition": "chamfer",
        "groups": [{
            "edges": {"kind": "edges", "value": ["test:model:edge#1"]},
            "spec": {"kind": "distance", "distance": 2.0}
        }]
    }));
    assert!(!feature_definition_is_incomplete(&chamfer));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "chamfer",
            "groups": [{
                "edges": {"kind": "native", "value": "native:edges"},
                "spec": {"kind": "distance", "distance": 2.0}
            }]
        }),
    )));

    let combine = definition(serde_json::json!({
        "definition": "combine",
        "operands": {
            "target": {"kind": "bodies", "value": ["test:model:body#target"]},
            "tools": {"kind": "bodies", "value": ["test:model:body#tool"]}
        },
        "op": "cut"
    }));
    assert!(!feature_definition_is_incomplete(&combine));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "combine",
            "operands": {
                "target": {"kind": "native", "value": "native:target"},
                "tools": {"kind": "bodies", "value": ["test:model:body#tool"]}
            },
            "op": "cut"
        }),
    )));

    let revolve = definition(serde_json::json!({
        "definition": "revolve",
        "construction": {
            "state": "resolved",
            "profile": {"kind": "sketch", "value": "test:model:sketch#profile"},
            "axis": {
                "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                "direction": {"x": 0.0, "y": 0.0, "z": 1.0}
            },
            "extent": {
                "kind": "one_sided",
                "termination": {"kind": "angle", "angle": std::f64::consts::PI}
            }
        },
        "op": "new_body"
    }));
    assert!(!feature_definition_is_incomplete(&revolve));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "revolve",
            "construction": {
                "state": "resolved",
                "profile": {"kind": "native", "value": "native:profile"},
                "axis": {
                    "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                    "direction": {"x": 0.0, "y": 0.0, "z": 1.0}
                },
                "extent": {
                    "kind": "one_sided",
                    "termination": {"kind": "angle", "angle": std::f64::consts::PI}
                }
            },
            "op": "new_body"
        }),
    )));
}

#[test]
fn datum_point_completeness_requires_a_resolved_construction_rule() {
    let definition = |construction: Option<serde_json::Value>| {
        let mut value = serde_json::json!({
            "definition": "datum_point",
            "position": {"x": 1.0, "y": 2.0, "z": 3.0}
        });
        if let Some(construction) = construction {
            value
                .as_object_mut()
                .expect("DatumPoint object")
                .insert("construction".into(), construction);
        }
        serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(value)
            .expect("DatumPoint definition")
    };

    assert!(!feature_definition_is_incomplete(&definition(Some(
        serde_json::json!({
            "kind": "circle_center",
            "edge": {"kind": "edges", "value": ["test:model:edge#1"]}
        }),
    ))));
    assert!(feature_definition_is_incomplete(&definition(None)));
    assert!(feature_definition_is_incomplete(&definition(Some(
        serde_json::json!({
            "kind": "distance_on_edge",
            "edge": {"kind": "native", "value": "native:edge"},
            "fraction": 0.5
        }),
    ))));
}

#[test]
fn datum_plane_completeness_accepts_direct_frames_and_resolved_construction() {
    let definition = |value| {
        serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(value)
            .expect("datum-plane definition")
    };

    assert!(!feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "datum_plane",
            "frame": {
                "origin": {"x": 0.0, "y": 0.0, "z": 5.0},
                "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
                "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0}
            }
        }),
    )));
    let three_point = |points: [serde_json::Value; 3]| {
        serde_json::json!({
            "definition": "datum_three_point_plane",
            "frame": {
                "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
                "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0}
            },
            "points": points
        })
    };
    assert!(!feature_definition_is_incomplete(&definition(three_point(
        std::array::from_fn(|index| serde_json::json!({
            "kind": "historical",
            "value": {
                "state": "test:model:feature-input#state:1",
                "vertex": format!("test:model:vertex#{index}"),
                "native": format!("native:{index}")
            }
        })),
    ))));
    assert!(feature_definition_is_incomplete(&definition(three_point(
        std::array::from_fn(
            |index| serde_json::json!({"kind": "native", "value": format!("native:{index}")})
        ),
    ))));
    assert!(!feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "datum_principal_plane",
            "plane": "top"
        }),
    )));
    assert!(!feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "datum_offset_plane",
            "reference": {"reference": "feature", "feature": "test:model:feature#plane"},
            "distance": 5.0
        }),
    )));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "datum_offset_plane",
            "distance": 5.0
        }),
    )));
}

#[test]
fn coil_completeness_requires_neutral_placement_and_boolean_targets() {
    use cadmpeg_ir::features::{
        BodySelection, CoilConstruction, CoilExtent, CoilPlacement, CoilResult, CoilSection,
        CoilSectionPlacement, FeatureDefinition, FeatureOperation,
    };
    use cadmpeg_ir::ids::BodyId;
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::scalar::{Angle, Length};

    let construction = CoilConstruction {
        placement: CoilPlacement::Explicit {
            frame: cadmpeg_ir::features::FeatureUnitPlaneFrame::new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        },
        diameter: cadmpeg_ir::scalar::PositiveLength::new(10.0).unwrap(),
        extent: CoilExtent::RevolutionsHeight {
            revolutions: cadmpeg_ir::scalar::PositiveReal::new(2.0).unwrap(),
            height: Length::new(5.0).unwrap(),
        },
        section: CoilSection::Circular {
            diameter: cadmpeg_ir::scalar::PositiveLength::new(1.0).unwrap(),
        },
        section_placement: CoilSectionPlacement::Center,
        clockwise: false,
        taper: Angle::new(0.0).unwrap(),
    };
    let definition = |construction, result| {
        FeatureDefinition::Operation(FeatureOperation::Coil {
            construction,
            result,
        })
    };

    assert!(!feature_definition_is_incomplete(&definition(
        construction.clone(),
        CoilResult::NewBody {},
    )));

    let mut native_placement = construction.clone();
    native_placement.placement = CoilPlacement::Native {
        native_ref: cadmpeg_ir::features::SelectionReference::try_from(String::from(
            "native:placement",
        ))
        .unwrap(),
    };
    assert!(feature_definition_is_incomplete(&definition(
        native_placement,
        CoilResult::NewBody {},
    )));

    let native_target = definition(
        construction.clone(),
        CoilResult::Boolean {
            operation: cadmpeg_ir::features::BooleanKind::Join,
            targets: BodySelection::Native("native:target".into()),
        },
    );
    assert!(feature_definition_is_incomplete(&native_target));

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.features.push(cadmpeg_ir::features::Feature {
        id: cadmpeg_ir::features::FeatureId::mint("test:model:feature#coil")
            .expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("CoilPrimitive".into()),
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(native_target),
        native_ref: None,
    });
    let gaps = design_projection_gaps(&ir, &F3dNative::default());
    assert_eq!(gaps.incomplete_features, 1);
    assert_eq!(gaps.body_selections, 1);

    assert!(!feature_definition_is_incomplete(&definition(
        construction,
        CoilResult::Boolean {
            operation: cadmpeg_ir::features::BooleanKind::Cut,
            targets: BodySelection::Bodies(
                cadmpeg_ir::features::DistinctMembers::try_from(
                    vec![BodyId::mint("test:model:body#1").expect("identity grammar")],
                    &cadmpeg_test_support::service_decode_context()
                )
                .expect("distinct bodies")
            ),
        },
    )));
}

#[test]
fn draft_completeness_requires_material_side() {
    let complete: cadmpeg_ir::features::FeatureDefinition =
        serde_json::from_value(serde_json::json!({
            "definition": "draft",
            "faces": {"kind": "faces", "value": ["test:model:face#drafted"]},
            "anchor": {
                "kind": "neutral_plane",
                "plane": {"kind": "faces", "value": ["test:model:face#neutral"]}
            },
            "angle": 0.1,
            "outward": true
        }))
        .expect("complete neutral-plane Draft");
    assert!(!feature_definition_is_incomplete(&complete));

    let mut incomplete = complete;
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Draft { outward, .. },
    ) = &mut incomplete
    else {
        panic!("Draft definition");
    };
    *outward = None;
    assert!(feature_definition_is_incomplete(&incomplete));
}

#[test]
fn loft_completeness_and_gap_counts_require_resolved_sections_and_paths() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};

    let resolved: FeatureDefinition = serde_json::from_value(serde_json::json!({
        "definition": "loft",
        "sections": [
            {
                "kind": "spatial_sketch_profiles",
                "value": {"sketch": "test:model:spatial-sketch#1", "profiles": [2, 3]}
            },
            {
                "kind": "spatial_sketch_profiles",
                "value": {"sketch": "test:model:spatial-sketch#1", "profiles": [1, 4]}
            }
        ],
        "guidance": {"kind": "guides", "path": [{
            "kind": "spatial_sketch_curves",
            "value": {"sketch": "test:model:spatial-sketch#1", "curves": ["test:model:spatial-entity#curve"]}
        }]},
        "op": "join"
    }))
    .expect("resolved Loft definition");
    assert!(!feature_definition_is_incomplete(&resolved));

    let unresolved: FeatureDefinition = serde_json::from_value(serde_json::json!({
        "definition": "loft",
        "sections": [
            {"kind": "native", "value": "native:profile"},
            {
                "kind": "spatial_sketch_profiles",
                "value": {"sketch": "test:model:spatial-sketch#1", "profiles": [1, 4]}
            }
        ],
        "guidance": {"kind": "guides", "path": [{"kind": "native", "value": "native:guide"}]},
        "op": "join"
    }))
    .expect("unresolved Loft definition");
    assert!(feature_definition_is_incomplete(&unresolved));

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.features.push(Feature {
        id: FeatureId::mint("test:model:feature#loft").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: Some("Loft".into()),
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(unresolved),
        native_ref: None,
    });

    let gaps = design_projection_gaps(&ir, &F3dNative::default());
    assert_eq!(gaps.incomplete_features, 1);
    assert_eq!(gaps.profile_selections, 1);
    assert_eq!(gaps.path_selections, 1);
}

#[test]
fn incomplete_feature_families_are_counted_by_source_operation() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let feature = |id: &str, source_tag: Option<&str>, kind: &str| Feature {
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: source_tag.map(str::to_owned),
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: kind.into(),
                parameters: std::collections::BTreeMap::new(),
            }),
        ),
        native_ref: None,
    };
    ir.model.features.push(feature(
        "synthetic:test:id#feature:1",
        Some("EdgeFlange"),
        "native-a",
    ));
    ir.model.features.push(feature(
        "synthetic:test:id#feature:2",
        Some("EdgeFlange"),
        "native-b",
    ));
    ir.model
        .features
        .push(feature("synthetic:test:id#feature:3", None, "Hem"));
    ir.model.features.push(feature(
        "synthetic:test:id#feature:4",
        Some("Canvas"),
        "Canvas",
    ));

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        incomplete_feature_families(&ctx, &ir).unwrap(),
        std::collections::BTreeMap::from([("EdgeFlange", 2), ("Hem", 1)])
    );
}

#[test]
fn incomplete_feature_family_index_refuses_collection_limit() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.features.push(Feature {
        id: FeatureId::mint("synthetic:test:feature#1").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("EdgeFlange".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "EdgeFlange".into(),
                parameters: Default::default(),
            }),
        ),
        native_ref: None,
    });
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = incomplete_feature_families(&ctx, &ir).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index incomplete F3D feature families")
    );
}

#[test]
fn projection_loss_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut report = cadmpeg_ir::codec::DecodeBody::new(
        cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {},
    );
    let error = super::super::push_loss_vec(
        &ctx,
        &mut report.losses,
        crate::loss::F3dLossCode::FeatureDefinitionIncomplete,
        format_args!("one incomplete feature"),
        "collect F3D projection losses",
        "retain F3D projection loss",
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D projection losses")
    );
}

#[test]
fn projection_loss_refuses_retained_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D projection loss",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut report = cadmpeg_ir::codec::DecodeBody::new(
                cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {},
            );
            super::super::push_loss_vec(
                &ctx,
                &mut report.losses,
                crate::loss::F3dLossCode::FeatureDefinitionIncomplete,
                format_args!("one incomplete feature"),
                "collect F3D projection losses",
                "retain F3D projection loss",
            )
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut report = cadmpeg_ir::codec::DecodeBody::new(
        cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {},
    );
    let error = super::super::push_loss_vec(
        &ctx,
        &mut report.losses,
        crate::loss::F3dLossCode::FeatureDefinitionIncomplete,
        format_args!("one incomplete feature"),
        "collect F3D projection losses",
        "retain F3D projection loss",
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D projection loss")
    );
}

#[test]
fn body_copy_features_require_resolved_body_selection() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::ids::BodyId;

    let resolved = BodySelection::Resolved {
        bodies: cadmpeg_ir::features::DistinctMembers::try_from(
            vec![BodyId::mint("test:model:body#result").expect("identity grammar")],
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("distinct bodies"),
        native: "native:body-selection".into(),
    };
    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: resolved.clone(),
        })
    ));
    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::InsertBodies {
            bodies: cadmpeg_ir::features::InsertedBodies::Resolved {
                native: "native:body-selection".into(),
            },
        })
    ));

    let unresolved = BodySelection::Native("native:body-selection".into());
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::BaseFeature { bodies: unresolved })
    ));
}

#[test]
fn split_body_requires_resolved_target_and_tool_selections() {
    use cadmpeg_ir::features::{BodySelection, FaceSelection, FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::ids::{BodyId, FaceId};

    let resolved_target = BodySelection::Resolved {
        bodies: cadmpeg_ir::features::DistinctMembers::try_from(
            vec![BodyId::mint("test:model:body#target").expect("identity grammar")],
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("distinct bodies"),
        native: "native:target".into(),
    };
    let resolved_tool = FaceSelection::Resolved {
        faces: vec![FaceId::mint("test:model:face#tool").expect("identity grammar")],
        native: "native:tool".into(),
    };
    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::SplitBody {
            targets: resolved_target.clone(),
            tools: resolved_tool,
        })
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::SplitBody {
            targets: resolved_target.clone(),
            tools: FaceSelection::Native("native:tool".into()),
        })
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::SplitBody {
            targets: BodySelection::Native("native:target".into()),
            tools: FaceSelection::Resolved {
                faces: vec![FaceId::mint("test:model:face#tool").expect("identity grammar")],
                native: "native:tool".into(),
            },
        })
    ));
}
