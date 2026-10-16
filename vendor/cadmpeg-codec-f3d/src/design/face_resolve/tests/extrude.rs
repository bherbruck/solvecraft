// SPDX-License-Identifier: Apache-2.0
use super::{face, start_geometry_fixture};
use crate::design::face_resolve::{
    extrude_start_plane_geometry_candidates, extrude_target_plane_candidate,
    retain_face_operand_resolution, ExtrudeFaceResolution,
};
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::{
    construction::DesignConstructionOperandGroup, extrude_selection::DesignOperandRole,
    face::DesignFaceOperand,
};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{FaceId, ShellId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchId};
use cadmpeg_ir::topology::{Face, Sense};

const EPS_FACE_TEST_TARGET_LINEAR_E9: f64 = 1.0e-9;
const EPS_FACE_TEST_TARGET_ANGULAR_E9: f64 = 1.0e-9;
const EPS_EXTRUDE_START_LINEAR: f64 = 1.0e-6;
const EPS_EXTRUDE_START_ANGULAR: f64 = 1.0e-10;

#[test]
fn extrude_start_plane_geometry_fallback_requires_complete_nested_recipe() {
    let (operand, group, faces) = start_geometry_fixture();
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            extrude_start_plane_geometry_candidates(
                decode_ctx,
                &group,
                std::slice::from_ref(&operand),
                &faces,
            )
        })
        .unwrap(),
        Some(vec![face(10)])
    );
    let mut bound = operand.clone();
    assert!(
        crate::test_support::with_decode_context(|decode_ctx| retain_face_operand_resolution(
            decode_ctx,
            &group,
            std::slice::from_mut(&mut bound),
            &face(10)
        ))
        .unwrap()
    );
    assert_eq!(bound.resolved_active_face, Some(face(10)));

    let mut incomplete = operand;
    incomplete.recipe_nodes.clear();
    assert!(crate::test_support::with_decode_context(|decode_ctx| {
        extrude_start_plane_geometry_candidates(decode_ctx, &group, &[incomplete], &faces)
    })
    .unwrap()
    .is_none());
}

#[test]
fn start_plane_candidate_id_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (operand, group, faces) = start_geometry_fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        extrude_start_plane_geometry_candidates(
            &ctx, &group, std::slice::from_ref(&operand), &faces),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d start plane candidate ID"
                && failure.dimension == ResourceDimension::RetainedBytes
    ));
}

#[test]
fn start_plane_candidate_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (operand, group, faces) = start_geometry_fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        extrude_start_plane_geometry_candidates(
            &ctx, &group, std::slice::from_ref(&operand), &faces),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d start plane candidate"
                && failure.dimension == ResourceDimension::CollectionItems
    ));
}

#[test]
fn retained_start_face_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (mut operand, group, _) = start_geometry_fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        retain_face_operand_resolution(
            &ctx, &group, std::slice::from_mut(&mut operand), &face(10)),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d retained operand active face"
                && failure.dimension == ResourceDimension::RetainedBytes
    ));
    assert!(operand.resolved_active_face.is_none());
}

fn start_binder_fixture(
    nested: bool,
) -> (
    cadmpeg_ir::features::Feature,
    Sketch,
    DesignConstructionOperandGroup,
    DesignFaceOperand,
    Vec<Face>,
    Vec<Surface>,
) {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, FaceSelection,
        Feature, FeatureDefinition, FeatureEvaluation, FeatureOperation, LinearTermination,
        PlanarProfileRef, ProfileRef,
    };
    let (nested_operand, mut group, faces) = start_geometry_fixture();
    group.operand_role = crate::records::topology::construction::DesignConstructionOperandRole::ExtrudeFaces {
        encoding: crate::records::topology::extrude_selection::DesignExtrudeFaceEncoding::SelectedStart,
        usage: crate::records::topology::extrude_selection::DesignExtrudeFaceRole::Start,
    };
    let operand = if nested {
        nested_operand
    } else {
        target_plane_operand(&[10])
    };
    let surface_id = faces[0].surface.clone();
    let surfaces = vec![Surface {
        id: surface_id,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 2.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    }];
    let sketch = Sketch {
        id: SketchId::mint("synthetic:test:id#start-sketch").unwrap(),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 2.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    };
    let feature = Feature {
        id: cadmpeg_ir::features::FeatureId::mint("test:model:feature#start").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Extrude {
                profile: ProfileRef::Planar(PlanarProfileRef::Sketch(sketch.id.clone())),
                direction: ExtrudeDirection::ProfileNormal {},
                start: ExtrudeStart::FromFace {
                    face: FaceSelection::Native(group.id.clone()),
                    offset: None,
                },
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::ThroughAll {},
                        draft: None,
                    },
                },
                op: BooleanOp::NewBody,
                solid: None,
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            },
        )),
        native_ref: None,
    };
    (feature, sketch, group, operand, faces, surfaces)
}

fn assert_start_binder_refusal(
    operation: &'static str,
    dimension: cadmpeg_core::decode::ResourceDimension,
    nested: bool,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::features::{ExtrudeStart, FaceSelection, FeatureDefinition, FeatureOperation};

    let (feature, sketch, group, operand, faces, surfaces) = start_binder_fixture(nested);
    let run = |ctx: &DecodeContext<'_>| {
        let mut features = [feature.clone()];
        let mut operands = [operand.clone()];
        let mut resolution = ExtrudeFaceResolution {
            faces: &faces,
            surfaces: &surfaces,
            groups: std::slice::from_ref(&group),
            operands: &mut operands,
            linear_tolerance: EPS_FACE_TEST_TARGET_LINEAR_E9,
            angular_tolerance: EPS_FACE_TEST_TARGET_ANGULAR_E9,
        };
        let result = crate::design::face_resolve::bind_extrude_start_planes(
            ctx,
            &mut features,
            std::slice::from_ref(&sketch),
            &mut resolution,
        );
        (result, features)
    };
    let (result, features) = crate::test_support::with_decode_context(|decode_ctx| run(decode_ctx));
    result.unwrap();
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            start: ExtrudeStart::FromFace {
                face: FaceSelection::Resolved { .. },
                ..
            },
            ..
        })
    ));
    let mut found = false;
    for limit in 0..256 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        match dimension {
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
            _ => unreachable!(),
        }

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        if matches!(run(&ctx).0,
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == operation && failure.dimension == dimension)
        {
            found = true;
            break;
        }
    }
    assert!(found, "no resource refusal at {operation}");
}

#[test]
fn start_operand_face_id_refuses_retained_limit() {
    assert_start_binder_refusal(
        "f3d start plane operand face ID",
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        false,
    );
}

#[test]
fn start_operand_candidate_refuses_collection_limit() {
    assert_start_binder_refusal(
        "f3d start plane operand candidate",
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        false,
    );
}

#[test]
fn coincident_start_face_refuses_collection_limit() {
    assert_start_binder_refusal(
        "f3d coincident start plane face",
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        true,
    );
}

#[test]
fn selected_start_face_id_refuses_retained_limit() {
    assert_start_binder_refusal(
        "f3d selected start plane face ID",
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        true,
    );
}

#[test]
fn selected_start_native_id_refuses_retained_limit() {
    assert_start_binder_refusal(
        "f3d selected start plane native ID",
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        true,
    );
}

#[test]
fn target_native_id_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::features::{
        ExtrudeExtent, ExtrudeStart, FaceSelection, FeatureDefinition, FeatureOperation,
        LinearTermination,
    };

    let (mut feature, sketch, mut group, operand, faces, mut surfaces) =
        start_binder_fixture(false);
    group.operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::ExtrudeFaces {
            encoding: crate::records::topology::extrude_selection::DesignExtrudeFaceEncoding::Faces,
            usage: crate::records::topology::extrude_selection::DesignExtrudeFaceRole::Termination,
        };
    surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    ));
    feature.evaluation.edit(|definition, _| {
        if let FeatureDefinition::Operation(FeatureOperation::Extrude {
            start,
            extent: ExtrudeExtent::OneSided { side },
            ..
        }) = definition
        {
            *start = ExtrudeStart::ProfilePlane {};
            side.termination = LinearTermination::ToFace {
                face: FaceSelection::Native(group.id.clone()),
                offset: None,
            };
        }
    });
    let run = |ctx: &DecodeContext<'_>| {
        let mut features = [feature.clone()];
        let mut operands = [operand.clone()];
        let mut resolution = ExtrudeFaceResolution {
            faces: &faces,
            surfaces: &surfaces,
            groups: std::slice::from_ref(&group),
            operands: &mut operands,
            linear_tolerance: EPS_FACE_TEST_TARGET_LINEAR_E9,
            angular_tolerance: EPS_FACE_TEST_TARGET_ANGULAR_E9,
        };
        let result = crate::design::face_resolve::bind_extrude_target_faces(
            ctx,
            &mut features,
            std::slice::from_ref(&sketch),
            &mut resolution,
        );
        (result, features)
    };
    let (result, features) = crate::test_support::with_decode_context(|decode_ctx| run(decode_ctx));
    result.unwrap();
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: LinearTermination::ToFace {
                        face: FaceSelection::Resolved { .. },
                        ..
                    },
                    ..
                },
            },
            ..
        })
    ));
    let mut found = false;
    for limit in 0..256 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = limit;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        if matches!(run(&ctx).0,
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == "f3d target face native ID"
                    && failure.dimension == ResourceDimension::RetainedBytes)
        {
            found = true;
            break;
        }
    }
    assert!(found, "no retained-byte refusal at target native ID");
}

#[test]
fn selected_face_start_requires_unique_sketch_plane_coincidence() {
    let sketch = Sketch {
        id: SketchId::mint("synthetic:test:id#sketch").unwrap(),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 2.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    };
    let face = |id: &str, surface: &str| Face {
        id: FaceId::mint(format!("test:model:face#{id}")).expect("identity grammar"),
        shell: ShellId::mint("test:model:shell#shell").expect("identity grammar"),
        surface: SurfaceId::mint(format!("test:model:surface#{surface}"))
            .expect("identity grammar"),
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
        name: None,
        color: None,
        tolerance: None,
    };
    let plane = |id: &str, origin: Point3, normal: Vector3| Surface {
        id: SurfaceId::mint(format!("test:model:surface#{id}")).expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                origin,
                normal.unit().unwrap(),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    };
    let faces = [
        face("coincident", "surface-coincident"),
        face("offset", "surface-offset"),
        face("tilted", "surface-tilted"),
    ];
    let surfaces = [
        plane(
            "surface-coincident",
            Point3::new(5.0, -3.0, 2.0),
            Vector3::new(0.0, 0.0, -2.0),
        ),
        plane(
            "surface-offset",
            Point3::new(0.0, 0.0, 2.1),
            Vector3::new(0.0, 0.0, 1.0),
        ),
        plane(
            "surface-tilted",
            Point3::new(0.0, 0.0, 2.0),
            Vector3::new(0.0, 1.0, 0.0),
        ),
    ];

    assert!(crate::design::face_resolve::face_coincident_with_sketch(
        &faces[0].id,
        &sketch,
        &faces,
        &surfaces,
        EPS_EXTRUDE_START_LINEAR,
        EPS_EXTRUDE_START_ANGULAR,
    ));
    for candidate in &faces[1..] {
        assert!(!crate::design::face_resolve::face_coincident_with_sketch(
            &candidate.id,
            &sketch,
            &faces,
            &surfaces,
            EPS_EXTRUDE_START_LINEAR,
            EPS_EXTRUDE_START_ANGULAR,
        ));
    }
}

fn target_plane_operand(candidates: &[i64]) -> DesignFaceOperand {
    let candidate_faces = candidates
        .iter()
        .map(|slot| face(*slot).into_string())
        .collect::<Vec<_>>();
    serde_json::from_value(serde_json::json!({
        "id": "f3d:test:face-operand#200",
        "scope_record_index": 100,
        "scope_reference_ordinal": 0,
        "group_record_index": 150,
        "group_member_ordinal": 0,
        "record_index": 200,
        "byte_offset": 0,
        "class_tag": "271",
        "paired_byte_offset": 325,
        "paired_class_tag": "261",
        "recipe_record_index": 203,
        "recipe_record_byte_offset": 341,
        "recipe_id": "f3d:test:recipe#201",
        "recipe_prefix_offset": 352,
        "recipe_prefix_bytes": "",
        "recipe_references": [],
        "recipe_kind": "face",
        "recipe_program_offset": 0,
        "recipe_program": [],
        "recipe_node_offsets": [],
        "recipe_nodes": [],
        "candidate_faces": candidate_faces,
        "unreferenced_candidate_faces": [],
        "alternate_selector_candidate_faces": [],
        "preceding_candidate_faces": [],
        "changed_candidate_faces": [],
        "historical_support_contexts": [],
        "resolved_face_slots": [],
        "next_record_index": 202,
        "next_byte_offset": 469
    }))
    .expect("target face operand")
}

fn target_face_group() -> DesignConstructionOperandGroup {
    serde_json::from_value(serde_json::json!({
        "id": "f3d:test:construction-group#150",
        "scope_record_index": 100,
        "scope_reference_ordinal": 0,
        "record_index": 150,
        "byte_offset": 0,
        "class_tag": "338",
        "members": [200],
        "member_offsets": [0],
        "frame": {
            "member_count_offset": 0,
            "auxiliary_record_indices": [],
            "auxiliary_record_offsets": [],
            "auxiliary_paths": [],
            "trailing_record_indices": [],
            "trailing_record_offsets": [],
            "trailing_transforms": [],
            "trailing_dual_transforms": [],
            "trailing_flags": [],
            "opaque_index": 1,
            "opaque_index_offset": 18,
            "opaque_scalar": 0.0,
            "opaque_scalar_offset": 22,
            "variant": false
        },
        "role": DesignOperandRole::FACES.raw(),
        "extrude_role": "faces",
        "extrude_face_role": "termination",
        "role_offset": 0,
        "paired_class_tag": "261",
        "paired_byte_offset": 0
    }))
    .expect("target face group")
}

#[test]
fn extrude_target_geometry_requires_one_forward_parallel_plane() {
    const TARGET_LINEAR_TOLERANCE: f64 = 1.0e-9;
    const TARGET_ANGULAR_TOLERANCE: f64 = 1.0e-9;

    let face_with_surface = |slot: i64, surface: &str| Face {
        id: face(slot),
        shell: ShellId::mint("test:model:shell#shell").expect("identity grammar"),
        surface: SurfaceId::mint(format!("test:model:surface#{surface}"))
            .expect("identity grammar"),
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
        name: None,
        color: None,
        tolerance: None,
    };
    let plane = |id: &str, origin: Point3, normal: Vector3| Surface {
        id: SurfaceId::mint(format!("test:model:surface#{id}")).expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                origin,
                normal,
                Vector3::new(0.0, 0.0, 1.0),
            )
            .unwrap(),
        )),
        source_object: None,
    };
    let faces = [
        face_with_surface(1, "surface-forward"),
        face_with_surface(2, "surface-forward-2"),
        face_with_surface(3, "surface-backward"),
        face_with_surface(4, "surface-tilted"),
    ];
    let surfaces = [
        plane(
            "surface-forward",
            Point3::new(3.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        ),
        plane(
            "surface-forward-2",
            Point3::new(5.0, 0.0, 0.0),
            Vector3::new(-1.0, 0.0, 0.0),
        ),
        plane(
            "surface-backward",
            Point3::new(-2.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        ),
        plane(
            "surface-tilted",
            Point3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        ),
    ];
    let group = target_face_group();
    let origin = Point3::new(0.0, 0.0, 0.0);
    let sweep_direction = Vector3::new(1.0, 0.0, 0.0);
    let candidate = |candidates: &[i64]| {
        let mut operands = vec![target_plane_operand(candidates)];
        let resolution = ExtrudeFaceResolution {
            faces: &faces,
            surfaces: &surfaces,
            groups: &[],
            operands: &mut operands,
            linear_tolerance: TARGET_LINEAR_TOLERANCE,
            angular_tolerance: TARGET_ANGULAR_TOLERANCE,
        };
        crate::test_support::with_decode_context(|decode_ctx| {
            extrude_target_plane_candidate(decode_ctx, &group, &resolution, origin, sweep_direction)
        })
        .unwrap()
    };

    assert_eq!(candidate(&[1, 3, 4]), Some(face(1)));
    assert!(candidate(&[1, 2, 3, 4]).is_none());
    assert!(candidate(&[3, 4]).is_none());
}

#[test]
fn target_plane_face_id_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let surface_id = SurfaceId::mint("test:model:surface#forward").unwrap();
    let faces = [Face {
        id: face(1),
        shell: ShellId::mint("test:model:shell#shell").unwrap(),
        surface: surface_id.clone(),
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
        name: None,
        color: None,
        tolerance: None,
    }];
    let surfaces = [Surface {
        id: surface_id,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(3.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .unwrap(),
        )),
        source_object: None,
    }];
    let mut operands = [target_plane_operand(&[1])];
    let resolution = ExtrudeFaceResolution {
        faces: &faces,
        surfaces: &surfaces,
        groups: &[],
        operands: &mut operands,
        linear_tolerance: EPS_FACE_TEST_TARGET_LINEAR_E9,
        angular_tolerance: EPS_FACE_TEST_TARGET_ANGULAR_E9,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        extrude_target_plane_candidate(
            &ctx, &target_face_group(), &resolution,
            Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d target plane face ID"
                && failure.dimension == ResourceDimension::RetainedBytes
    ));
}

fn extrude_root_fixture() -> (DesignParameterScope, [DesignConstructionOperandGroup; 2]) {
    let scope = DesignParameterScope::empty(
        "f3d:test:scope#12",
        crate::records::feature::scope::DesignFeatureKind::Extrude,
        12,
    );
    let root: DesignConstructionOperandGroup = serde_json::from_value(serde_json::json!({
        "id": "f3d:test:group#100",
        "scope_record_index": 12,
        "scope_reference_ordinal": 0,
        "record_index": 100,
        "byte_offset": 1000,
        "class_tag": "332",
        "members": [101],
        "member_offsets": [1026],
        "frame": {
            "member_count_offset": 1021,
            "opaque_index": 1,
            "opaque_index_offset": 1072,
            "opaque_scalar": 0.0,
            "opaque_scalar_offset": 1076,
            "variant": false
        },
        "role": DesignOperandRole::PROFILE.raw(),
        "extrude_role": "profile",
        "role_offset": 1054,
        "paired_class_tag": "259",
        "paired_byte_offset": 1125
    }))
    .unwrap();
    let mut child = root.clone();
    child.id = "f3d:test:group#101".into();
    child.record_index = 101;
    child.scope_reference_ordinal = 1;
    child
        .try_set_members(vec![crate::records::identity::Located {
            value: 200,
            offset: 1026,
        }])
        .unwrap();
    (scope, [root, child])
}

fn assert_extrude_root_collection_limit(operation: &'static str) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (scope, groups) = extrude_root_fixture();
    let roots = crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::face_resolve::extrude_profile_group_roots(decode_ctx, &scope, &groups)
    })
    .unwrap()
    .unwrap();
    assert_eq!(
        roots
            .iter()
            .map(|group| group.record_index)
            .collect::<Vec<_>>(),
        [100]
    );
    for limit in 0..16 {
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let arena = DecodeArena::new();

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        if matches!(crate::design::face_resolve::extrude_profile_group_roots(&ctx, &scope, &groups),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == operation
        ) {
            return;
        }
    }
    panic!("no Extrude profile hierarchy refusal at {operation}");
}

#[test]
fn extrude_profile_group_refuses_collection_limit() {
    assert_extrude_root_collection_limit("f3d Extrude profile group");
}

#[test]
fn extrude_profile_group_index_refuses_collection_limit() {
    assert_extrude_root_collection_limit("f3d Extrude profile group index");
}

#[test]
fn extrude_profile_parent_index_refuses_collection_limit() {
    assert_extrude_root_collection_limit("f3d Extrude profile parent index");
}

#[test]
fn extrude_profile_root_refuses_collection_limit() {
    assert_extrude_root_collection_limit("f3d Extrude profile root");
}

#[test]
fn extrude_profile_visited_group_refuses_collection_limit() {
    assert_extrude_root_collection_limit("f3d Extrude profile visited group");
}

#[test]
fn extrude_profile_hierarchy_refuses_depth_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (scope, groups) = extrude_root_fixture();
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 1;
    let arena = DecodeArena::new();

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(crate::design::face_resolve::extrude_profile_group_roots(&ctx, &scope, &groups),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::RecursionDepth
                && failure.operation == "f3d Extrude profile hierarchy")
    );
}

#[test]
fn extrude_profile_hierarchy_refuses_work_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (scope, groups) = extrude_root_fixture();
    let mut policy = DecodePolicy::default();
    // The two profile groups already have increasing scope ordinals. Their
    // stable sort charges two items and one comparison, with no permutation;
    // the first one-unit hierarchy visit then exceeds the limit.
    policy.limits.max_work_units = 2 + 1;
    let arena = DecodeArena::new();

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(crate::design::face_resolve::extrude_profile_group_roots(&ctx, &scope, &groups),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::WorkUnits
                && failure.operation == "f3d Extrude profile hierarchy")
    );
}

fn extrude_leaf_fixture() -> (
    DesignConstructionOperandGroup,
    Vec<DesignConstructionOperandGroup>,
    Vec<DesignFaceOperand>,
) {
    let (_, groups) = extrude_root_fixture();
    let operand = serde_json::from_value(serde_json::json!({
        "id": "f3d:test:face-operand#200",
        "scope_record_index": 12,
        "scope_reference_ordinal": 1,
        "group_record_index": 101,
        "group_member_ordinal": 0,
        "record_index": 200,
        "byte_offset": 0,
        "class_tag": "297",
        "paired_byte_offset": 16,
        "paired_class_tag": "259",
        "recipe_record_index": 203,
        "recipe_record_byte_offset": 32,
        "recipe_id": "f3d:test:recipe#203",
        "recipe_prefix_offset": 43,
        "recipe_prefix_bytes": "",
        "recipe_references": [],
        "recipe_kind": "bounded_face",
        "recipe_program_offset": 0,
        "recipe_program": [0, -1, 1],
        "recipe_node_offsets": [0],
        "recipe_nodes": [{
            "byte_offset": 0,
            "end_byte_offset": 12,
            "program": [0, -1, 1],
            "recipe_structure": {
                "root": 0,
                "prelude": [0, 0],
                "sides": [
                    {"field_count": 1, "header_value": 0, "payload_entry_count": 0, "payload_prefix": [], "scalars": [], "entries": []},
                    {"field_count": 1, "header_value": 0, "payload_entry_count": 0, "payload_prefix": [], "scalars": [], "entries": []}
                ],
                "postlude": []
            }
        }],
        "candidate_faces": ["f3d:brep:entity#10"],
        "next_record_index": 204,
        "next_byte_offset": 160
    })).unwrap();
    (groups[0].clone(), groups.to_vec(), vec![operand])
}

fn assert_extrude_leaf_collection_limit(operation: &'static str) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (root, groups, operands) = extrude_leaf_fixture();
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::face_resolve::extrude_profile_group_operand_indices(
                decode_ctx, &root, &groups, &operands,
            )
        })
        .unwrap()
        .unwrap(),
        [0]
    );
    for limit in 0..24 {
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let arena = DecodeArena::new();

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        if matches!(crate::design::face_resolve::extrude_profile_group_operand_indices(&ctx, &root, &groups, &operands), Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == operation)
        {
            return;
        }
    }
    panic!("no Extrude leaf refusal at {operation}");
}

#[test]
fn extrude_leaf_profile_group_refuses_collection_limit() {
    assert_extrude_leaf_collection_limit("f3d Extrude leaf profile group");
}

#[test]
fn extrude_leaf_group_index_refuses_collection_limit() {
    assert_extrude_leaf_collection_limit("f3d Extrude leaf group index");
}

#[test]
fn extrude_leaf_visited_group_refuses_collection_limit() {
    assert_extrude_leaf_collection_limit("f3d Extrude leaf visited group");
}

#[test]
fn extrude_leaf_index_refuses_collection_limit() {
    assert_extrude_leaf_collection_limit("f3d Extrude leaf index");
}

#[test]
fn extrude_leaf_hierarchy_refuses_depth_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (root, groups, operands) = extrude_leaf_fixture();
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 1;
    let arena = DecodeArena::new();

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(crate::design::face_resolve::extrude_profile_group_operand_indices(&ctx, &root,
        &groups, &operands), Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::RecursionDepth
                && failure.operation == "f3d Extrude leaf hierarchy")
    );
}

#[test]
fn extrude_leaf_hierarchy_refuses_work_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (root, groups, operands) = extrude_leaf_fixture();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = 1;
    let arena = DecodeArena::new();

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(crate::design::face_resolve::extrude_profile_group_operand_indices(&ctx, &root,
        &groups, &operands), Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::WorkUnits
                && failure.operation == "f3d Extrude leaf hierarchy")
    );
}

#[test]
fn extrude_active_face_id_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (_, _, operands) = extrude_leaf_fixture();
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::face_resolve::resolved_extrude_profile_active_faces(
                decode_ctx,
                &[0],
                &operands,
            )
        })
        .unwrap()
        .unwrap(),
        [face(10)]
    );
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(crate::design::face_resolve::resolved_extrude_profile_active_faces(&ctx, &[0], &operands),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::RetainedBytes
                && failure.operation == "f3d Extrude active face id")
    );
}

#[test]
fn extrude_active_face_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (_, _, operands) = extrude_leaf_fixture();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let arena = DecodeArena::new();

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(crate::design::face_resolve::resolved_extrude_profile_active_faces(&ctx, &[0], &operands),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d Extrude active face")
    );
}
