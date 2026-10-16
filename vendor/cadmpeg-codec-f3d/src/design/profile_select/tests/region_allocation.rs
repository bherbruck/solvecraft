// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

fn profile_operand(
    placement: &DesignSketchPlacement,
    regions: Option<Vec<DesignSketchProfileRegion>>,
) -> DesignSketchProfileOperand {
    DesignSketchProfileOperand::try_new(
        crate::records::topology::sketch_profile::DesignSketchProfileOperandDraft {
            scope_reference_ordinal: 0,
            record_index: 10,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned())
                .unwrap(),
            asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 32,
            entity_id: placement.entity_id.clone(),
            entity_reference_offset: 80,
            region_selection: regions.map(|regions| DesignSketchProfileRegionSelection {
                record_index: 11,
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned())
                    .unwrap(),
                region_count_offset: 0,
                regions,
                companion_class_tag: crate::records::references::DesignClassTag::try_from(
                    "302".to_owned(),
                )
                .unwrap(),
                companion_byte_offset: 0,
            }),
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "303".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 160,
        },
    )
    .unwrap()
}

fn collection_context(limit: u64) -> (DecodeArena, DecodePolicy) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = limit;
    (arena, policy)
}

#[test]
fn selected_planar_sketch_region_refuses_collection_limit() {
    let placement = placement();
    let sketch_id = neutral_sketch_id(&placement);
    let curve = curve(30, 100, 101);
    let entity_id = neutral_sketch_curve_id(&sketch_id, 100, 101);
    let entity = SketchEntity::new(
        entity_id.clone(),
        sketch_id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        })
        .unwrap(),
    );
    let sketch = Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![vec![SketchEntityUse {
            entity: entity_id,
            reversed: false,
        }]])
        .unwrap(),
        native_ref: None,
    };
    let operand = profile_operand(
        &placement,
        Some(vec![DesignSketchProfileRegion {
            member_count_offset: 0,
            members: vec![profile_region_member(100)],
        }]),
    );
    let (arena, policy) = collection_context(0);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        resolved_sketch_profile_regions("stream", &operand, &sketch, &[curve], &[entity], &ctx),
        Err(CodecError::ResourceLimit(failure)) if failure.operation == "f3d selected planar sketch profile region"
    ));
}

#[test]
fn selected_spatial_sketch_region_refuses_collection_limit() {
    let placement = placement();
    let sketch_id = neutral_spatial_sketch_id(&placement);
    let curve = curve(30, 100, 101);
    let entity_id = neutral_spatial_sketch_curve_id(&sketch_id, 100, 101);
    let entity = SpatialSketchEntity::new(
        entity_id.clone(),
        sketch_id.clone(),
        SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line {
            start: Point3::new(0.0, 0.0, 0.0),
            end: Point3::new(1.0, 0.0, 0.0),
        })
        .unwrap(),
    );
    let sketch = SpatialSketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        profiles: vec![SpatialSketchProfile::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            vec![SpatialSketchEntityUse {
                entity: entity_id,
                reversed: false,
            }],
            &cadmpeg_test_support::service_decode_context(),
            "spatial profile uniqueness",
        )
        .expect("fixture collection admission")
        .unwrap()],
        native_ref: None,
    };
    let operand = profile_operand(
        &placement,
        Some(vec![DesignSketchProfileRegion {
            member_count_offset: 0,
            members: vec![profile_region_member(100)],
        }]),
    );
    let curves = [curve];
    let entities = [entity];
    let resolution = SketchProfileResolution {
        entities: &[],
        entity_selection_operands: &[],
        placements: std::slice::from_ref(&placement),
        curve_identities: &curves,
        sketches: &[],
        sketch_entities: &[],
        spatial_sketches: std::slice::from_ref(&sketch),
        spatial_sketch_entities: &entities,
        linear_tolerance: 1.0e-7,
        angular_tolerance: 1.0e-9,
    };
    let (arena, policy) = collection_context(0);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        resolved_spatial_sketch_profile_regions("stream", &operand, &sketch, &resolution, &ctx),
        Err(CodecError::ResourceLimit(failure)) if failure.operation == "f3d selected spatial sketch profile region"
    ));
}

#[test]
fn all_spatial_sketch_regions_refuse_collection_limit() {
    let placement = placement();
    let sketch_id = neutral_spatial_sketch_id(&placement);
    let entity_id = neutral_spatial_sketch_curve_id(&sketch_id, 100, 101);
    let sketch = SpatialSketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        profiles: vec![SpatialSketchProfile::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            vec![SpatialSketchEntityUse {
                entity: entity_id,
                reversed: false,
            }],
            &cadmpeg_test_support::service_decode_context(),
            "spatial profile uniqueness",
        )
        .expect("fixture collection admission")
        .unwrap()],
        native_ref: None,
    };
    let operand = profile_operand(&placement, None);
    let resolution = SketchProfileResolution {
        entities: &[],
        entity_selection_operands: &[],
        placements: std::slice::from_ref(&placement),
        curve_identities: &[],
        sketches: &[],
        sketch_entities: &[],
        spatial_sketches: std::slice::from_ref(&sketch),
        spatial_sketch_entities: &[],
        linear_tolerance: 1.0e-7,
        angular_tolerance: 1.0e-9,
    };
    let (arena, policy) = collection_context(0);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        resolved_spatial_sketch_profile_regions("stream", &operand, &sketch, &resolution, &ctx),
        Err(CodecError::ResourceLimit(failure)) if failure.operation == "f3d all spatial sketch profile regions"
    ));
}
