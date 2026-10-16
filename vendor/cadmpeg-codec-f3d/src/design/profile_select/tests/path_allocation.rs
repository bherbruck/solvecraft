// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

fn assert_planar_path_refusal(operation: &'static str, profile: bool) {
    let placement = placement();
    let sketch_id = neutral_sketch_id(&placement);
    let curves = [curve(30, 100, 101), curve(31, 200, 201)];
    let entities = [
        SketchEntity::new(
            neutral_sketch_curve_id(&sketch_id, 100, 101),
            sketch_id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(1.0, 0.0),
            })
            .unwrap(),
        ),
        SketchEntity::new(
            neutral_sketch_curve_id(&sketch_id, 200, 201),
            sketch_id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(1.0, 0.0),
                end: Point2::new(1.0, 1.0),
            })
            .unwrap(),
        ),
    ];
    let profiles = if profile {
        cadmpeg_ir::sketches::SketchProfiles::try_from(vec![
            vec![SketchEntityUse {
                entity: entities[0].id().clone(),
                reversed: false,
            }],
            vec![SketchEntityUse {
                entity: entities[1].id().clone(),
                reversed: false,
            }],
        ])
        .unwrap()
    } else {
        cadmpeg_ir::sketches::SketchProfiles::default()
    };
    let sketches = [Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles,
        native_ref: None,
    }];
    let mut group = group();
    if profile {
        group.operand_role =
            crate::records::topology::construction::DesignConstructionOperandRole::Other(
                DesignOperandRole::PROFILE,
            );
    }
    let operands = [operand(10, 0, 100), operand(11, 1, 200)];
    let placements = [placement];
    let setup = planar_resolution(&operands, &placements, &curves, &sketches, &entities);
    let resolution = setup.path_resolution();
    for limit in 0..32 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = if profile {
            resolve_entity_selection_profile(&group, &resolution, &ctx).map(|_| ())
        } else {
            resolve_entity_selection_path(&group, &resolution, &ctx).map(|_| ())
        };
        match result {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected planar path refusal at {operation}: {other:?}"),
        }
    }
    panic!("no planar path refusal at {operation}");
}

macro_rules! planar_path_refusal {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            assert_planar_path_refusal($operation, false);
        }
    };
}

planar_path_refusal!(
    path_member_record_refuses_collection_limit,
    "f3d entity path member record"
);
planar_path_refusal!(
    path_selected_identity_refuses_collection_limit,
    "f3d entity path selected identity"
);
planar_path_refusal!(
    path_curve_identity_refuses_collection_limit,
    "f3d entity path curve identity"
);
planar_path_refusal!(
    path_curve_pair_refuses_collection_limit,
    "f3d entity path curve pair"
);
planar_path_refusal!(
    path_planar_output_curve_refuses_collection_limit,
    "f3d entity path planar output curve"
);

#[test]
fn path_selected_planar_profile_refuses_collection_limit() {
    assert_planar_path_refusal("f3d entity path selected planar profile", true);
}

fn assert_spatial_path_refusal(operation: &'static str, retained: bool, profile: bool) {
    let placement = placement();
    let sketch_id = neutral_spatial_sketch_id(&placement);
    let curves = [curve(30, 100, 101), curve(31, 200, 201)];
    let entities = curves
        .iter()
        .map(|curve| {
            SpatialSketchEntity::new(
                neutral_spatial_sketch_curve_id(
                    &sketch_id,
                    curve.primary_id.get(),
                    curve.secondary_id,
                ),
                sketch_id.clone(),
                SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line {
                    start: Point3::new(0.0, 0.0, 0.0),
                    end: Point3::new(1.0, 0.0, 0.0),
                })
                .unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let profiles = if profile {
        vec![SpatialSketchProfile::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            entities
                .iter()
                .map(|entity| SpatialSketchEntityUse {
                    entity: entity.id().clone(),
                    reversed: false,
                })
                .collect(),
            &cadmpeg_test_support::service_decode_context(),
            "spatial profile uniqueness",
        )
        .expect("fixture collection admission")
        .unwrap()]
    } else {
        Vec::new()
    };
    let sketches = [SpatialSketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        profiles,
        native_ref: None,
    }];
    let mut group = group();
    if profile {
        group.operand_role =
            crate::records::topology::construction::DesignConstructionOperandRole::Other(
                DesignOperandRole::PROFILE,
            );
    }
    let operands = [operand(10, 0, 100), operand(11, 1, 200)];
    let resolution = EntitySelectionPathResolution {
        operands: &operands,
        placements: std::slice::from_ref(&placement),
        curve_identities: &curves,
        sketches: &[],
        sketch_entities: &[],
        spatial_sketches: &sketches,
        spatial_sketch_entities: &entities,
    };
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            if retained {
                cadmpeg_core::decode::ResourceDimension::RetainedBytes
            } else {
                cadmpeg_core::decode::ResourceDimension::CollectionItems
            },
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                if retained {
                    policy.limits.max_retained_bytes = cap;
                } else {
                    policy.limits.max_collection_items = cap;
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                if profile {
                    resolve_entity_selection_profile(&group, &resolution, &ctx).map(|_| ())
                } else {
                    resolve_entity_selection_path(&group, &resolution, &ctx).map(|_| ())
                }
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == operation && failure.dimension == (if retained { cadmpeg_core::decode::ResourceDimension::RetainedBytes } else { cadmpeg_core::decode::ResourceDimension::CollectionItems })));
    }
}

#[test]
fn path_spatial_curve_index_refuses_collection_limit() {
    assert_spatial_path_refusal("f3d entity path spatial curve index", false, false);
}

#[test]
fn path_spatial_output_curve_refuses_collection_limit() {
    assert_spatial_path_refusal("f3d entity path spatial output curve", false, false);
}

#[test]
fn path_spatial_sketch_id_refuses_retained_limit() {
    assert_spatial_path_refusal("f3d profile spatial sketch id", true, false);
}

#[test]
fn path_selected_spatial_profile_refuses_collection_limit() {
    assert_spatial_path_refusal("f3d entity path selected spatial profile", false, true);
}
