// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use cadmpeg_test_support::edit;

use super::{
    historical_profile_face_candidates, resolve_entity_selection_path,
    resolve_entity_selection_profile, resolved_loft_entity_selection_path,
    resolved_sketch_profile_regions, resolved_spatial_extrude_profile_selection,
    resolved_spatial_sketch_profile_regions, spatial_profile_containing_entity,
    EntitySelectionPathResolution, ExtrudeProfileResolution, SketchProfileResolution,
};
use crate::design::geometry::{region_containing_points, MAX_ARRANGEMENT_WALK_WORK};
use crate::ids::{
    neutral_sketch_curve_id, neutral_sketch_id, neutral_spatial_sketch_curve_id,
    neutral_spatial_sketch_id,
};
use crate::records::{
    sketch_geometry::SketchCurveIdentity,
    sketch_placement::DesignSketchPlacement,
    sketch_relations::SketchRelationOperand,
    topology::{
        construction::DesignConstructionOperandGroup,
        construction::DesignConstructionOperandGroupFrame,
        entity_selection::DesignEntitySelectionOperand,
        extrude_selection::DesignExtrudeSelectionGroup,
        extrude_selection::DesignExtrudeSelectionMember, extrude_selection::DesignOperandRole,
        sketch_profile::DesignSketchProfileOperand, sketch_profile::DesignSketchProfileRegion,
        sketch_profile::DesignSketchProfileRegionMember,
        sketch_profile::DesignSketchProfileRegionSelection,
    },
};
use cadmpeg_core::decode::WorkBudget;
use cadmpeg_ir::features::{PathRef, SketchProfileRegion};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchPlacement, SpatialSketch, SpatialSketchEntity,
    SpatialSketchEntityUse, SpatialSketchGeometry, SpatialSketchGeometryDefinition,
    SpatialSketchProfile,
};
use std::collections::{HashMap, HashSet};

mod extrude_bind_allocation;
mod historical_allocation;
mod historical_candidate_allocation;
mod loft_bind_allocation;
mod path_allocation;
mod region_allocation;
mod spatial_transition;

fn group() -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: "stream:group".into(),
            scope_record_index: 7,
            scope_reference_ordinal: 0,
            record_index: 9,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("277".to_owned())
                .unwrap(),
            members: vec![
                crate::records::identity::Located {
                    value: 10,
                    offset: 0,
                },
                crate::records::identity::Located {
                    value: 11,
                    offset: 11,
                },
            ],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 0,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 18,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 22,
                    variant: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    DesignOperandRole::ROLE_0X5,
                ),
            role_offset: 0,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "277".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap()
}

fn operand(
    record_index: u32,
    ordinal: u32,
    secondary_identity: u64,
) -> DesignEntitySelectionOperand {
    DesignEntitySelectionOperand::try_new(
        crate::records::topology::entity_selection::DesignEntitySelectionOperandDraft {
            id: format!("stream:operand-{record_index}"),
            scope_record_index: 7,
            group_record_index: 9,
            group_member_ordinal: ordinal,
            record_index,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("277".to_owned())
                .unwrap(),
            asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 0,
            context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
            )
            .unwrap(),
            context_id_offset: 0,
            identity_record_index: record_index + 3,
            identity_record_offset: 0,
            primary_identity: 42,
            primary_identity_offset: 29,
            secondary: Some(crate::records::identity::DesignSecondaryIdentity {
                identity: crate::records::identity::Located {
                    value: secondary_identity,
                    offset: 37,
                },
                curve_identity: None,
            }),
            historical_edge_candidates: Vec::new(),
            historical_face_candidates: Vec::new(),
            resolved_edge_slot: None,
            next_record_index: record_index + 4,
            next_byte_offset: 45,
        },
    )
    .unwrap()
}

fn placement() -> DesignSketchPlacement {
    DesignSketchPlacement {
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            0,
            crate::records::sketch_placement::DesignSketchFrameForm::ScopeCompact,
        )
        .unwrap(),
        id: "stream:placement".into(),
        scope_record_index: Some(7),
        entity_id: crate::records::identity::DesignEntityId::try_from("Sketch_42".to_owned())
            .expect("valid entity ID"),

        visibility: None,

        class_tag: crate::records::references::DesignClassTag::try_from("277".to_owned()).unwrap(),
        record_index: 20,

        paired_class_tag: crate::records::references::DesignClassTag::try_from("277".to_owned())
            .unwrap(),
    }
}

fn curve(record_index: u32, primary_id: u64, secondary_id: u64) -> SketchCurveIdentity {
    SketchCurveIdentity {
        id: format!("stream:curve-{record_index}"),
        record_index,
        owner_reference: Some(42),
        class_tag: crate::records::references::DesignClassTag::try_from("450".to_owned()).unwrap(),
        byte_offset: 0,
        geometry_offset: 0,
        entity_genesis: None,
        primary_id: std::num::NonZeroU64::new(primary_id).unwrap(),
        secondary_id,
        geometry: None,
    }
}

fn planar_resolution<'a>(
    operands: &'a [DesignEntitySelectionOperand],
    placements: &'a [DesignSketchPlacement],
    curve_identities: &'a [SketchCurveIdentity],
    sketches: &'a [Sketch],
    sketch_entities: &'a [SketchEntity],
) -> SketchProfileResolution<'a> {
    SketchProfileResolution {
        entities: &[],
        entity_selection_operands: operands,
        placements,
        curve_identities,
        sketches,
        sketch_entities,
        spatial_sketches: &[],
        spatial_sketch_entities: &[],
        linear_tolerance: 1.0e-7,
        angular_tolerance: 1.0e-9,
    }
}

fn profile_region_member(curve_primary_id: u32) -> DesignSketchProfileRegionMember {
    DesignSketchProfileRegionMember {
        kind_offset: 0,
        curve_primary_id: std::num::NonZeroU32::new(curve_primary_id).expect("curve identity"),
        curve_primary_id_offset: 0,
        incidence_flag: false,
        incidence_values: [crate::records::topology::sketch_profile::DesignRegionIncidence::One; 2],
        incidence_words_offset: 0,
    }
}

fn spatial_line(
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    primary_id: u64,
    start: Point3,
    end: Point3,
) -> SpatialSketchEntity {
    SpatialSketchEntity::new(
        neutral_spatial_sketch_curve_id(sketch, primary_id, 0),
        sketch.clone(),
        SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line { start, end })
            .unwrap(),
    )
}

fn spatial_profile(
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    primary_ids: &[u64],
) -> SpatialSketchProfile {
    SpatialSketchProfile::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        primary_ids
            .iter()
            .map(|primary_id| SpatialSketchEntityUse {
                entity: neutral_spatial_sketch_curve_id(sketch, *primary_id, 0),
                reversed: false,
            })
            .collect(),
        &cadmpeg_test_support::service_decode_context(),
        "spatial profile uniqueness",
    )
    .expect("fixture collection admission")
    .unwrap()
}

#[test]
fn spatial_extrude_profile_uses_persistent_curve_member_without_history() {
    crate::test_support::with_decode_context(|decode_ctx| {
        let sketch_id =
            cadmpeg_ir::sketches::SpatialSketchId::mint("f3d:model:spatial-sketch#selection")
                .unwrap();
        let sketch = SpatialSketch {
            id: sketch_id.clone(),
            name: None,
            configuration: None,
            visible: None,
            profiles: vec![
                spatial_profile(&sketch_id, &[100, 101, 102]),
                spatial_profile(&sketch_id, &[200, 201, 202]),
            ],
            native_ref: None,
        };
        let group = DesignExtrudeSelectionGroup::try_from(
            crate::records::topology::extrude_selection::DesignExtrudeSelectionGroupWire {
                id: "f3d:Design/BulkStream.dat:selection-group#9".into(),
                scope_record_index: 7,
                scope_reference_ordinal: 0,
                record_index: 9,
                byte_offset: 0,
                class_tag: "277".to_owned(),
                member_count_offset: 32,
                members: vec![10],
                member_offsets: vec![37],
                opaque_index: 1,
                opaque_index_offset: 47,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 51,
                variant: false,
                paired_class_tag: "277".to_owned(),
                paired_byte_offset: 100,
            },
        )
        .unwrap();
        let mut member = DesignExtrudeSelectionMember::try_new(
            crate::records::topology::extrude_selection::DesignExtrudeSelectionMemberDraft {
                id: "f3d:Design/BulkStream.dat:selection-member#10".into(),
                group_record_index: group.record_index,
                group_member_ordinal: 0,
                record_index: 10,
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("278".to_owned())
                    .unwrap(),
                local_id: 200,
                local_id_offset: 21,
                asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
                )
                .unwrap(),
                asset_id_offset: 33,
                context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
                )
                .unwrap(),
                context_id_offset: 109,
                tail_slot_present: false,
                tail_slot_offset: 0,
                resolved_geometry: Some(SketchRelationOperand::Curve {
                    record_index: 20,
                    primary_id: 200,
                    secondary_id: 0,
                }),
                operand_identity_ids: Vec::new(),
                historical: None,
                next_record_index: 11,
                next_byte_offset: 190,
            },
        )
        .unwrap();
        let arrangement_budget = WorkBudget::new(MAX_ARRANGEMENT_WALK_WORK);
        let scope_histories = HashMap::new();
        let resolution = ExtrudeProfileResolution {
            entities: &[],
            spatial_sketches: &[],
            spatial_entities: &[],
            histories: &[],
            scope_histories: &scope_histories,
            linear_tolerance: 1.0e-6,
            angular_tolerance: 1.0e-9,
            arrangement_budget: &arrangement_budget,
            ctx: decode_ctx,
        };
        let scoped_resolution = resolution.scoped(&[]);

        assert_eq!(
            resolved_spatial_extrude_profile_selection(
                &group,
                std::slice::from_ref(&member),
                &sketch,
                &[],
                scoped_resolution,
                None,
                None,
            )
            .unwrap(),
            Some(1)
        );

        let mut conflicting_group = group.clone();
        conflicting_group.try_set_members(vec![10, 11]).unwrap();
        let mut conflicting_member = member.clone();
        conflicting_member.id = "f3d:Design/BulkStream.dat:selection-member#11".into();
        conflicting_member.group_member_ordinal = 1;
        let mut draft = conflicting_member.into_draft();
        draft.record_index = 11;

        conflicting_member =
            crate::records::topology::extrude_selection::DesignExtrudeSelectionMember::try_new(
                draft,
            )
            .unwrap();
        conflicting_member.local_id = 100;
        conflicting_member.resolved_geometry = Some(SketchRelationOperand::Curve {
            record_index: 21,
            primary_id: 100,
            secondary_id: 0,
        });
        assert_eq!(
            resolved_spatial_extrude_profile_selection(
                &conflicting_group,
                &[member.clone(), conflicting_member],
                &sketch,
                &[],
                scoped_resolution,
                None,
                None,
            )
            .unwrap(),
            None
        );

        member.resolved_geometry = None;
        assert_eq!(
            resolved_spatial_extrude_profile_selection(
                &group,
                std::slice::from_ref(&member),
                &sketch,
                &[],
                scoped_resolution,
                None,
                None,
            )
            .unwrap(),
            None
        );
        let single_profile = SpatialSketch {
            profiles: vec![sketch.profiles[0].clone()],
            ..sketch
        };
        assert_eq!(
            resolved_spatial_extrude_profile_selection(
                &group,
                &[member],
                &single_profile,
                &[],
                scoped_resolution,
                None,
                None,
            )
            .unwrap(),
            Some(0)
        );
    });
}

#[test]
fn loft_spatial_profile_regions_collapse_coincident_curve_revisions() {
    let placement = placement();
    let sketch_id = neutral_spatial_sketch_id(&placement);
    let curves = [curve(30, 100, 0), curve(31, 200, 0), curve(32, 201, 0)];
    let entity_id = |primary| neutral_spatial_sketch_curve_id(&sketch_id, primary, 0);
    let circle = |primary, radius, normal| {
        SpatialSketchEntity::new(
            entity_id(primary),
            sketch_id.clone(),
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Circle {
                center: Point3::new(0.0, 0.0, 0.0),
                normal,
                reference_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: Length::new(radius).unwrap(),
            })
            .unwrap(),
        )
    };
    let spatial_entities = [
        circle(100, 2.0, Vector3::new(0.0, 0.0, 1.0)),
        circle(200, 1.0, Vector3::new(0.0, 0.0, 1.0)),
        circle(201, 1.0, Vector3::new(0.0, 0.0, -1.0)),
    ];
    let profile = |primary| {
        SpatialSketchProfile::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            vec![SpatialSketchEntityUse {
                entity: entity_id(primary),
                reversed: false,
            }],
            &cadmpeg_test_support::service_decode_context(),
            "spatial profile uniqueness",
        )
        .expect("fixture collection admission")
        .unwrap()
    };
    let spatial_sketches = [SpatialSketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        profiles: vec![profile(100), profile(200), profile(201)],
        native_ref: None,
    }];
    let profile_operand = DesignSketchProfileOperand::try_new(
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
            region_selection: Some(DesignSketchProfileRegionSelection {
                record_index: 13,
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("303".to_owned())
                    .unwrap(),
                region_count_offset: 0,
                regions: vec![
                    DesignSketchProfileRegion {
                        member_count_offset: 0,
                        members: vec![profile_region_member(100)],
                    },
                    DesignSketchProfileRegion {
                        member_count_offset: 0,
                        members: vec![
                            profile_region_member(200),
                            profile_region_member(201),
                            profile_region_member(200),
                        ],
                    },
                ],
                companion_class_tag: crate::records::references::DesignClassTag::try_from(
                    "304".to_owned(),
                )
                .unwrap(),
                companion_byte_offset: 0,
            }),
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "301".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 160,
        },
    )
    .unwrap();
    let placements = [placement];
    let resolution = SketchProfileResolution {
        entities: &[],
        entity_selection_operands: &[],
        placements: &placements,
        curve_identities: &curves,
        sketches: &[],
        sketch_entities: &[],
        spatial_sketches: &spatial_sketches,
        spatial_sketch_entities: &spatial_entities,
        linear_tolerance: 1.0e-7,
        angular_tolerance: 1.0e-9,
    };

    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            resolved_spatial_sketch_profile_regions(
                "stream",
                &profile_operand,
                &spatial_sketches[0],
                &resolution,
                decode_ctx,
            )
        })
        .unwrap(),
        Some(vec![0, 1])
    );

    let whole_sketch_operand = DesignSketchProfileOperand::try_new(
        crate::records::topology::sketch_profile::DesignSketchProfileOperandDraft {
            region_selection: None,
            ..profile_operand.clone().into_draft()
        },
    )
    .unwrap();
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            resolved_spatial_sketch_profile_regions(
                "stream",
                &whole_sketch_operand,
                &spatial_sketches[0],
                &resolution,
                decode_ctx,
            )
        })
        .unwrap(),
        Some(vec![0, 1, 2])
    );
    assert_eq!(
        spatial_profile_containing_entity(&spatial_sketches[0], &entity_id(100)),
        Some(0)
    );
    let repeated_profile = SpatialSketch {
        profiles: vec![profile(100), profile(100)],
        ..spatial_sketches[0].clone()
    };
    assert_eq!(
        spatial_profile_containing_entity(&repeated_profile, &entity_id(100)),
        None
    );

    let mut noncoincident_entities = spatial_entities.to_vec();
    edit::replace(&mut noncoincident_entities[2].geometry, |previous| {
        let mut definition = previous.definition().to_raw();
        {
            let definition: &mut cadmpeg_ir::sketches::SpatialSketchGeometryDefinition =
                &mut definition;

            let SpatialSketchGeometryDefinition::Circle { center, .. } = definition else {
                unreachable!()
            };
            center.x = 0.1;
        };
        definition.try_into()
    })
    .unwrap();
    let noncoincident_resolution = SketchProfileResolution {
        spatial_sketch_entities: &noncoincident_entities,
        ..resolution
    };
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            resolved_spatial_sketch_profile_regions(
                "stream",
                &profile_operand,
                &spatial_sketches[0],
                &noncoincident_resolution,
                decode_ctx,
            )
        })
        .unwrap(),
        None
    );
}

#[test]
fn loft_multi_member_planar_entity_path_preserves_order_and_requires_complete_proof() {
    let placement = placement();
    let sketch = neutral_sketch_id(&placement);
    let curves = [curve(30, 100, 101), curve(31, 200, 201)];
    let sketch_entities = [
        SketchEntity::new(
            neutral_sketch_curve_id(&sketch, 100, 101),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(1.0, 0.0),
            })
            .unwrap(),
        ),
        SketchEntity::new(
            neutral_sketch_curve_id(&sketch, 200, 201),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(1.0, 0.0),
                end: Point2::new(1.0, 1.0),
            })
            .unwrap(),
        ),
    ];
    let sketches = [Sketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    }];
    let group = group();
    let operands = [operand(10, 0, 100), operand(11, 1, 200)];
    let placement_list = [placement];
    let resolution = planar_resolution(
        &operands,
        &placement_list,
        &curves,
        &sketches,
        &sketch_entities,
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| resolved_loft_entity_selection_path(
            &group,
            &resolution,
            decode_ctx
        ))
        .unwrap(),
        Some(
            PathRef::sketch_curves(
                sketch.clone(),
                vec![
                    neutral_sketch_curve_id(&sketch, 100, 101),
                    neutral_sketch_curve_id(&sketch, 200, 201),
                ],
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("profile membership admission")
            .unwrap()
        )
    );

    let mut mixed_operands = operands.to_vec();
    mixed_operands[1].primary_identity = 43;
    let mixed_resolution = planar_resolution(
        &mixed_operands,
        &placement_list,
        &curves,
        &sketches,
        &sketch_entities,
    );
    assert!(crate::test_support::with_decode_context(|decode_ctx| {
        resolved_loft_entity_selection_path(&group, &mixed_resolution, decode_ctx)
    })
    .unwrap()
    .is_none());

    let incomplete_resolution = planar_resolution(
        &operands,
        &placement_list,
        &curves[..1],
        &sketches,
        &sketch_entities,
    );
    assert!(crate::test_support::with_decode_context(|decode_ctx| {
        resolved_loft_entity_selection_path(&group, &incomplete_resolution, decode_ctx)
    })
    .unwrap()
    .is_none());
}

#[test]
fn entity_selection_path_uses_spatial_sketch_for_nonplanar_owner() {
    let placement = placement();
    let spatial_sketch = neutral_spatial_sketch_id(&placement);
    let curves = [curve(30, 100, 101), curve(31, 200, 201)];
    let spatial_entities = curves
        .iter()
        .map(|curve| {
            SpatialSketchEntity::new(
                neutral_spatial_sketch_curve_id(
                    &spatial_sketch,
                    curve.primary_id.get(),
                    curve.secondary_id,
                ),
                spatial_sketch.clone(),
                SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line {
                    start: Point3::new(0.0, 0.0, 0.0),
                    end: Point3::new(1.0, 0.0, 0.0),
                })
                .unwrap(),
            )
            .with_native_ref(Some(curve.id.clone()))
        })
        .collect::<Vec<_>>();
    let group = group();
    let operands = [operand(10, 0, 100), operand(11, 1, 200)];
    let sketches = [];
    let spatial_sketches = [SpatialSketch {
        id: spatial_sketch.clone(),
        name: None,
        configuration: None,
        visible: None,
        profiles: Vec::new(),
        native_ref: Some(placement.id.clone()),
    }];
    let resolution = EntitySelectionPathResolution {
        operands: &operands,
        placements: std::slice::from_ref(&placement),
        curve_identities: &curves,
        sketches: &sketches,
        sketch_entities: &[],
        spatial_sketches: &spatial_sketches,
        spatial_sketch_entities: &spatial_entities,
    };

    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| resolve_entity_selection_path(
            &group,
            &resolution,
            decode_ctx
        ))
        .unwrap(),
        Some(
            PathRef::spatial_sketch_curves(
                spatial_sketch.clone(),
                curves
                    .iter()
                    .map(|curve| {
                        neutral_spatial_sketch_curve_id(
                            &spatial_sketch,
                            curve.primary_id.get(),
                            curve.secondary_id,
                        )
                    })
                    .collect(),
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("profile membership admission")
            .unwrap()
        )
    );
}

#[test]
fn entity_selection_profile_requires_unique_profile_membership() {
    let placement = placement();
    let sketch = neutral_sketch_id(&placement);
    let curves = [curve(30, 100, 101), curve(31, 200, 201)];
    let curve_ids = curves
        .iter()
        .map(|curve| neutral_sketch_curve_id(&sketch, curve.primary_id.get(), curve.secondary_id))
        .collect::<Vec<_>>();
    let unselected = SketchEntityId::mint("synthetic:test:sketch-entity#unselected").unwrap();
    let sketch_entities = [
        SketchEntity::new(
            curve_ids[0].clone(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(1.0, 0.0),
            })
            .unwrap(),
        ),
        SketchEntity::new(
            curve_ids[1].clone(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(1.0, 0.0),
                end: Point2::new(0.0, 0.0),
            })
            .unwrap(),
        ),
        SketchEntity::new(
            unselected.clone(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(2.0, 0.0),
            })
            .unwrap(),
        ),
    ];
    let mut sketches = [Sketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![
            vec![SketchEntityUse {
                entity: unselected,
                reversed: false,
            }],
            curve_ids
                .iter()
                .cloned()
                .map(|entity| SketchEntityUse {
                    entity,
                    reversed: false,
                })
                .collect(),
        ])
        .unwrap(),
        native_ref: None,
    }];
    let mut group = group();
    group.operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            DesignOperandRole::PROFILE,
        );
    let operands = [operand(10, 0, 100), operand(11, 1, 200)];
    let resolution = EntitySelectionPathResolution {
        operands: &operands,
        placements: std::slice::from_ref(&placement),
        curve_identities: &curves,
        sketches: &sketches,
        sketch_entities: &sketch_entities,
        spatial_sketches: &[],
        spatial_sketch_entities: &[],
    };
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| resolve_entity_selection_profile(
            &group,
            &resolution,
            decode_ctx
        ))
        .unwrap(),
        Some(cadmpeg_ir::features::ProfileRef::Planar(
            cadmpeg_ir::features::PlanarProfileRef::sketch_profiles(
                sketch.clone(),
                vec![1],
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("profile membership admission")
            .unwrap()
        ))
    );

    sketches[0]
        .profiles
        .edit(|profiles| {
            profiles[0].push(SketchEntityUse {
                entity: curve_ids[0].clone(),
                reversed: false,
            });
        })
        .unwrap();
    let ambiguous_resolution = EntitySelectionPathResolution {
        operands: &operands,
        placements: std::slice::from_ref(&placement),
        curve_identities: &curves,
        sketches: &sketches,
        sketch_entities: &sketch_entities,
        spatial_sketches: &[],
        spatial_sketch_entities: &[],
    };
    assert!(crate::test_support::with_decode_context(|decode_ctx| {
        resolve_entity_selection_profile(&group, &ambiguous_resolution, decode_ctx)
    })
    .unwrap()
    .is_none());
}

#[test]
fn entity_selection_profile_retains_an_open_curve_as_ordered_entities() {
    let placement = placement();
    let sketch = neutral_sketch_id(&placement);
    let curve = curve(30, 100, 101);
    let entity_id = neutral_sketch_curve_id(&sketch, curve.primary_id.get(), curve.secondary_id);
    let sketch_entities = [SketchEntity::new(
        entity_id.clone(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        })
        .unwrap(),
    )];
    let sketches = [Sketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    }];
    let mut group = group();
    group.operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            DesignOperandRole::PROFILE,
        );
    group
        .try_set_members(
            vec![10]
                .into_iter()
                .enumerate()
                .map(|(index, value)| crate::records::identity::Located {
                    value,
                    offset: u64_from_index(index) * 11,
                })
                .collect(),
        )
        .unwrap();
    let operands = [operand(10, 0, 100)];
    let resolution = EntitySelectionPathResolution {
        operands: &operands,
        placements: std::slice::from_ref(&placement),
        curve_identities: std::slice::from_ref(&curve),
        sketches: &sketches,
        sketch_entities: &sketch_entities,
        spatial_sketches: &[],
        spatial_sketch_entities: &[],
    };

    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| resolve_entity_selection_profile(
            &group,
            &resolution,
            decode_ctx
        ))
        .unwrap(),
        Some(cadmpeg_ir::features::ProfileRef::Planar(
            cadmpeg_ir::features::PlanarProfileRef::sketch_entities(
                sketch,
                vec![entity_id],
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("profile membership admission")
            .unwrap()
        ))
    );
}

#[test]
fn planar_profile_regions_resolve_by_persistent_curve_members() {
    let placement = placement();
    let sketch = neutral_sketch_id(&placement);
    let curves = [curve(30, 100, 101), curve(31, 200, 201)];
    let first_entity = neutral_sketch_curve_id(&sketch, 100, 101);
    let second_entity = neutral_sketch_curve_id(&sketch, 200, 201);
    let sketch_entities = [
        SketchEntity::new(
            first_entity.clone(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(1.0, 0.0),
            })
            .unwrap(),
        ),
        SketchEntity::new(
            second_entity.clone(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 1.0),
                end: Point2::new(1.0, 1.0),
            })
            .unwrap(),
        ),
    ];
    let mut source = Sketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![
            vec![SketchEntityUse {
                entity: first_entity.clone(),
                reversed: false,
            }],
            vec![SketchEntityUse {
                entity: second_entity.clone(),
                reversed: false,
            }],
        ])
        .unwrap(),
        native_ref: None,
    };
    let operand = DesignSketchProfileOperand::try_new(
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
            entity_id: placement.entity_id,
            entity_reference_offset: 80,
            region_selection: Some(DesignSketchProfileRegionSelection {
                record_index: 11,
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned())
                    .unwrap(),
                region_count_offset: 0,
                regions: vec![
                    DesignSketchProfileRegion {
                        member_count_offset: 0,
                        members: vec![profile_region_member(200)],
                    },
                    DesignSketchProfileRegion {
                        member_count_offset: 0,
                        members: vec![profile_region_member(100)],
                    },
                ],
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
    .unwrap();

    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| resolved_sketch_profile_regions(
            "stream",
            &operand,
            &source,
            &curves,
            &sketch_entities,
            decode_ctx
        ))
        .unwrap(),
        Some(vec![1, 0])
    );

    let mut mixed_region = operand.clone();
    mixed_region
        .region_selection
        .as_mut()
        .expect("region selection")
        .regions[0]
        .members
        .push(profile_region_member(100));
    assert!(crate::test_support::with_decode_context(
        |decode_ctx| resolved_sketch_profile_regions(
            "stream",
            &mixed_region,
            &source,
            &curves,
            &sketch_entities,
            decode_ctx
        )
    )
    .unwrap()
    .is_none());

    source
        .profiles
        .edit(|profiles| {
            profiles[1].push(SketchEntityUse {
                entity: first_entity,
                reversed: false,
            });
        })
        .unwrap();
    assert!(crate::test_support::with_decode_context(
        |decode_ctx| resolved_sketch_profile_regions(
            "stream",
            &operand,
            &source,
            &curves,
            &sketch_entities,
            decode_ctx
        )
    )
    .unwrap()
    .is_none());
}

#[test]
fn historical_points_on_profile_boundaries_are_ambiguous() {
    let sketch_id = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let entity_id = SketchEntityId::mint("synthetic:test:id#line").unwrap();
    let mut sketch = Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(10.0, 20.0, 5.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![vec![SketchEntityUse {
            entity: entity_id.clone(),
            reversed: false,
        }]])
        .unwrap(),
        native_ref: None,
    };
    let entity = SketchEntity::new(
        entity_id,
        sketch_id,
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(2.0, 0.0),
        })
        .unwrap(),
    );
    let point = Point3::new(11.0, 20.0, 9.0);
    let arrangement_budget = WorkBudget::new(MAX_ARRANGEMENT_WALK_WORK);
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| region_containing_points(
            &sketch,
            std::slice::from_ref(&entity),
            &[point],
            1.0e-6,
            decode_ctx
        ))
        .unwrap(),
        None
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::profile_select::selection_containing_points(
                &sketch,
                std::slice::from_ref(&entity),
                &[point],
                1.0e-6,
                &arrangement_budget,
                decode_ctx,
            )
        })
        .unwrap(),
        Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![0]))
    );

    let mut branched_sketch = sketch.clone();
    let start_branch_id = SketchEntityId::mint("synthetic:test:id#start-branch").unwrap();
    let end_branch_id = SketchEntityId::mint("synthetic:test:id#end-branch").unwrap();
    branched_sketch
        .profiles
        .edit(|profiles| {
            profiles.extend([
                vec![SketchEntityUse {
                    entity: start_branch_id.clone(),
                    reversed: false,
                }],
                vec![SketchEntityUse {
                    entity: end_branch_id.clone(),
                    reversed: false,
                }],
            ]);
        })
        .unwrap();
    let branch_entity = |id, start, end| {
        SketchEntity::new(
            id,
            branched_sketch.id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).unwrap(),
        )
    };
    let branched_entities = [
        entity.clone(),
        branch_entity(
            start_branch_id,
            Point2::new(0.0, 0.0),
            Point2::new(0.0, 1.0),
        ),
        branch_entity(end_branch_id, Point2::new(2.0, 0.0), Point2::new(2.0, 1.0)),
    ];
    let endpoints = [Point3::new(10.0, 20.0, 5.0), Point3::new(12.0, 20.0, 5.0)];
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::profile_select::selection_containing_points(
                &branched_sketch,
                &branched_entities,
                &endpoints,
                1.0e-6,
                &arrangement_budget,
                decode_ctx,
            )
        })
        .unwrap(),
        Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![0]))
    );

    sketch
        .profiles
        .try_push(sketch.profiles[0].clone())
        .unwrap();
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| region_containing_points(
            &sketch,
            std::slice::from_ref(&entity),
            &[point],
            1.0e-6,
            decode_ctx
        ))
        .unwrap(),
        None
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::profile_select::selection_containing_points(
                &sketch,
                std::slice::from_ref(&entity),
                &[point],
                1.0e-6,
                &arrangement_budget,
                decode_ctx,
            )
        })
        .unwrap(),
        None
    );
}

#[test]
fn historical_selection_preserves_first_member_region_order() {
    let region = |outer| {
        SketchProfileRegion::loops(
            outer,
            Vec::new(),
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("fixture loop-region admission")
        .unwrap()
    };
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::profile_select::ordered_unique_profile_selections(
                [
                    Some(
                        crate::design::profile_select::ResolvedProfileSelection::Regions(vec![
                            region(3),
                        ]),
                    ),
                    Some(
                        crate::design::profile_select::ResolvedProfileSelection::Regions(vec![
                            region(1),
                        ]),
                    ),
                    Some(
                        crate::design::profile_select::ResolvedProfileSelection::Regions(vec![
                            region(3),
                        ]),
                    ),
                    Some(
                        crate::design::profile_select::ResolvedProfileSelection::Regions(vec![
                            region(2),
                        ]),
                    ),
                ],
                decode_ctx,
            )
        })
        .unwrap(),
        Some(
            crate::design::profile_select::ResolvedProfileSelection::Regions(vec![
                region(3),
                region(1),
                region(2),
            ])
        )
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::profile_select::ordered_unique_profile_selections(
                [
                    Some(
                        crate::design::profile_select::ResolvedProfileSelection::Regions(vec![
                            region(3),
                        ]),
                    ),
                    None,
                ],
                decode_ctx,
            )
        })
        .unwrap(),
        None
    );
}

#[test]
fn multiple_extrude_profile_groups_merge_only_exact_same_kind_selections() {
    let sketch = SketchId::mint("f3d:model:sketch#multi-profile").unwrap();
    let loops = [
        cadmpeg_ir::features::ProfileRef::Planar(
            cadmpeg_ir::features::PlanarProfileRef::sketch_profiles(
                sketch.clone(),
                vec![3, 1],
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("profile membership admission")
            .unwrap(),
        ),
        cadmpeg_ir::features::ProfileRef::Planar(
            cadmpeg_ir::features::PlanarProfileRef::sketch_profiles(
                sketch.clone(),
                vec![1, 2],
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("profile membership admission")
            .unwrap(),
        ),
    ];
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::profile_select::merge_resolved_profile_selections(
                &sketch, &loops, decode_ctx,
            )
        })
        .unwrap(),
        Some(cadmpeg_ir::features::ProfileRef::Planar(
            cadmpeg_ir::features::PlanarProfileRef::sketch_profiles(
                sketch.clone(),
                vec![3, 1, 2],
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("profile membership admission")
            .unwrap()
        ))
    );

    let regions = [
        cadmpeg_ir::features::ProfileRef::Planar(
            cadmpeg_ir::features::PlanarProfileRef::sketch_regions(
                sketch.clone(),
                vec![SketchProfileRegion::loops(
                    4,
                    vec![5],
                    &cadmpeg_test_support::service_decode_context(),
                )
                .expect("fixture loop-region admission")
                .unwrap()],
            )
            .unwrap(),
        ),
        cadmpeg_ir::features::ProfileRef::Planar(
            cadmpeg_ir::features::PlanarProfileRef::sketch_regions(
                sketch.clone(),
                vec![SketchProfileRegion::loops(
                    2,
                    Vec::new(),
                    &cadmpeg_test_support::service_decode_context(),
                )
                .expect("fixture loop-region admission")
                .unwrap()],
            )
            .unwrap(),
        ),
    ];
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::profile_select::merge_resolved_profile_selections(
                &sketch, &regions, decode_ctx,
            )
        })
        .unwrap(),
        Some(cadmpeg_ir::features::ProfileRef::Planar(
            cadmpeg_ir::features::PlanarProfileRef::sketch_regions(
                sketch.clone(),
                vec![
                    SketchProfileRegion::loops(
                        4,
                        vec![5],
                        &cadmpeg_test_support::service_decode_context()
                    )
                    .expect("fixture loop-region admission")
                    .unwrap(),
                    SketchProfileRegion::loops(
                        2,
                        Vec::new(),
                        &cadmpeg_test_support::service_decode_context()
                    )
                    .expect("fixture loop-region admission")
                    .unwrap(),
                ]
            )
            .unwrap()
        ))
    );

    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::profile_select::merge_resolved_profile_selections(
                &sketch,
                &[loops[0].clone(), regions[0].clone()],
                decode_ctx,
            )
        })
        .unwrap(),
        None
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::profile_select::merge_resolved_profile_selections(
                &sketch,
                &[
                    loops[0].clone(),
                    cadmpeg_ir::features::ProfileRef::Planar(
                        cadmpeg_ir::features::PlanarProfileRef::sketch_selection(
                            sketch.clone(),
                            vec!["native-group".into()],
                            &cadmpeg_test_support::service_decode_context(),
                        )
                        .expect("profile membership admission")
                        .unwrap(),
                    ),
                ],
                decode_ctx,
            )
        })
        .unwrap(),
        None
    );
}

#[test]
fn historical_profile_members_resolve_through_topology_ownership() {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalOptionalCarrierBinding,
        AsmHistoricalRelation, AsmHistoricalTopology,
    };
    use crate::records::topology::body_recipe::AsmHistoricalEntityKind;

    let topology = AsmHistoricalTopology {
        faces: vec![10, 20],
        loops: vec![11, 21],
        coedges: vec![12, 22],
        edges: vec![30],
        surfaces: vec![40],
        pcurves: vec![50],
        face_loops: vec![
            AsmHistoricalRelation {
                owner_ref: 10,
                member_refs: vec![11],
            },
            AsmHistoricalRelation {
                owner_ref: 20,
                member_refs: vec![21],
            },
        ],
        coedge_topology: vec![
            AsmHistoricalCoedge {
                coedge: 12,
                owner_loop: 11,
                edge: 30,
                previous: 12,
                next: 12,
                radial_next: 22,
            },
            AsmHistoricalCoedge {
                coedge: 22,
                owner_loop: 21,
                edge: 30,
                previous: 22,
                next: 22,
                radial_next: 12,
            },
        ],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 10,
            carrier: 40,
        }],
        coedge_pcurves: vec![AsmHistoricalOptionalCarrierBinding {
            entity: 12,
            carrier: Some(50),
        }],
        ..AsmHistoricalTopology::default()
    };

    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| historical_profile_face_candidates(
            Some(AsmHistoricalEntityKind::Pcurve),
            50,
            &topology,
            decode_ctx
        ))
        .unwrap(),
        HashSet::from([10])
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| historical_profile_face_candidates(
            Some(AsmHistoricalEntityKind::Surface),
            40,
            &topology,
            decode_ctx
        ))
        .unwrap(),
        HashSet::from([10])
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| historical_profile_face_candidates(
            Some(AsmHistoricalEntityKind::Edge),
            30,
            &topology,
            decode_ctx
        ))
        .unwrap(),
        HashSet::from([10, 20])
    );
}

#[test]
fn historical_face_points_require_complete_boundary_topology() {
    let mut topology = crate::history_records::AsmHistoricalTopology {
        faces: vec![10],
        loops: vec![11],
        coedges: vec![12, 13, 14],
        edges: vec![20, 21, 22],
        vertices: vec![30, 31, 32],
        points: vec![40, 41, 42],
        face_loops: vec![crate::history_records::AsmHistoricalRelation {
            owner_ref: 10,
            member_refs: vec![11],
        }],
        loop_coedges: vec![crate::history_records::AsmHistoricalRelation {
            owner_ref: 11,
            member_refs: vec![12, 13, 14],
        }],
        coedge_topology: vec![
            crate::history_records::AsmHistoricalCoedge {
                coedge: 12,
                owner_loop: 11,
                edge: 20,
                next: 13,
                previous: 14,
                radial_next: 12,
            },
            crate::history_records::AsmHistoricalCoedge {
                coedge: 13,
                owner_loop: 11,
                edge: 21,
                next: 14,
                previous: 12,
                radial_next: 13,
            },
            crate::history_records::AsmHistoricalCoedge {
                coedge: 14,
                owner_loop: 11,
                edge: 22,
                next: 12,
                previous: 13,
                radial_next: 14,
            },
        ],
        edge_vertices: vec![
            crate::history_records::AsmHistoricalEdge {
                edge: 20,
                start_vertex: 30,
                end_vertex: 31,
            },
            crate::history_records::AsmHistoricalEdge {
                edge: 21,
                start_vertex: 31,
                end_vertex: 32,
            },
            crate::history_records::AsmHistoricalEdge {
                edge: 22,
                start_vertex: 32,
                end_vertex: 30,
            },
        ],
        vertex_points: vec![
            crate::history_records::AsmHistoricalCarrierBinding {
                entity: 30,
                carrier: 40,
            },
            crate::history_records::AsmHistoricalCarrierBinding {
                entity: 31,
                carrier: 41,
            },
            crate::history_records::AsmHistoricalCarrierBinding {
                entity: 32,
                carrier: 42,
            },
        ],
        point_positions: vec![
            crate::history_records::AsmHistoricalPoint {
                point: 40,
                position: Point3::new(0.0, 0.0, 0.0),
            },
            crate::history_records::AsmHistoricalPoint {
                point: 41,
                position: Point3::new(2.0, 0.0, 0.0),
            },
            crate::history_records::AsmHistoricalPoint {
                point: 42,
                position: Point3::new(0.0, 1.0, 0.0),
            },
        ],
        ..crate::history_records::AsmHistoricalTopology::default()
    };
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::profile_select::historical_face_points(10, &topology, decode_ctx)
        })
        .unwrap(),
        Some(vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ])
    );

    topology.point_positions.pop();
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::profile_select::historical_face_points(10, &topology, decode_ctx)
        })
        .unwrap(),
        None
    );
}

#[test]
fn inserted_cylinder_selects_its_exact_circular_sketch_profile() {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalCylinder, AsmHistoricalEdge,
        AsmHistoricalPoint, AsmHistoricalRelation, AsmHistoricalTopology,
    };

    let sketch_id = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let circle_id = SketchEntityId::mint("synthetic:test:id#circle").unwrap();
    let circle = SketchEntity::new(
        circle_id.clone(),
        sketch_id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(2.0).unwrap(),
        })
        .unwrap(),
    );
    let sketch = Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![vec![SketchEntityUse {
            entity: circle_id,
            reversed: false,
        }]])
        .unwrap(),
        native_ref: None,
    };
    let topology = AsmHistoricalTopology {
        faces: vec![10],
        loops: vec![11],
        coedges: vec![12, 13, 14],
        edges: vec![20, 21, 22],
        vertices: vec![30, 31, 32],
        points: vec![40, 41, 42],
        surfaces: vec![50],
        face_loops: vec![AsmHistoricalRelation {
            owner_ref: 10,
            member_refs: vec![11],
        }],
        loop_coedges: vec![AsmHistoricalRelation {
            owner_ref: 11,
            member_refs: vec![12, 13, 14],
        }],
        coedge_topology: vec![
            AsmHistoricalCoedge {
                coedge: 12,
                owner_loop: 11,
                edge: 20,
                next: 13,
                previous: 14,
                radial_next: 12,
            },
            AsmHistoricalCoedge {
                coedge: 13,
                owner_loop: 11,
                edge: 21,
                next: 14,
                previous: 12,
                radial_next: 13,
            },
            AsmHistoricalCoedge {
                coedge: 14,
                owner_loop: 11,
                edge: 22,
                next: 12,
                previous: 13,
                radial_next: 14,
            },
        ],
        edge_vertices: vec![
            AsmHistoricalEdge {
                edge: 20,
                start_vertex: 30,
                end_vertex: 31,
            },
            AsmHistoricalEdge {
                edge: 21,
                start_vertex: 31,
                end_vertex: 32,
            },
            AsmHistoricalEdge {
                edge: 22,
                start_vertex: 32,
                end_vertex: 30,
            },
        ],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 10,
            carrier: 50,
        }],
        vertex_points: vec![
            AsmHistoricalCarrierBinding {
                entity: 30,
                carrier: 40,
            },
            AsmHistoricalCarrierBinding {
                entity: 31,
                carrier: 41,
            },
            AsmHistoricalCarrierBinding {
                entity: 32,
                carrier: 42,
            },
        ],
        point_positions: vec![
            AsmHistoricalPoint {
                point: 40,
                position: Point3::new(2.0, 0.0, 0.0),
            },
            AsmHistoricalPoint {
                point: 41,
                position: Point3::new(0.0, 2.0, 1.0),
            },
            AsmHistoricalPoint {
                point: 42,
                position: Point3::new(-2.0, 0.0, 0.0),
            },
        ],
        surface_cylinders: vec![AsmHistoricalCylinder {
            surface: 50,
            origin: Point3::new(0.0, 0.0, 3.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            radius: 2.0,
        }],
        ..AsmHistoricalTopology::default()
    };

    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::profile_select::inserted_cylindrical_profile_selection(
                &sketch,
                std::slice::from_ref(&circle),
                &topology,
                10,
                1.0e-6,
                1.0e-9,
                decode_ctx,
            )
        })
        .unwrap(),
        Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![0]))
    );
    let mut tilted = topology;
    tilted.surface_cylinders[0].axis = Vector3::new(0.0, 1.0, 0.0);
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::profile_select::inserted_cylindrical_profile_selection(
                &sketch,
                std::slice::from_ref(&circle),
                &tilted,
                10,
                1.0e-6,
                1.0e-9,
                decode_ctx,
            )
        })
        .unwrap(),
        None
    );
}

#[test]
fn deleted_profile_family_requires_one_complete_multi_face_carrier() {
    use crate::history_records::{AsmHistoricalCarrierBinding, AsmHistoricalTopology};

    let topology = AsmHistoricalTopology {
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 100,
            },
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 100,
            },
            AsmHistoricalCarrierBinding {
                entity: 20,
                carrier: 200,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::profile_select::unique_multi_face_deleted_carrier_family(
                &[20, 11, 10],
                &topology,
                decode_ctx,
            )
        })
        .unwrap(),
        Some(vec![10, 11])
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::profile_select::unique_multi_face_deleted_carrier_family(
                &[10, 10],
                &topology,
                decode_ctx,
            )
        })
        .unwrap(),
        None
    );

    let mut ambiguous = topology.clone();
    ambiguous.face_surfaces.extend([
        AsmHistoricalCarrierBinding {
            entity: 30,
            carrier: 300,
        },
        AsmHistoricalCarrierBinding {
            entity: 31,
            carrier: 300,
        },
    ]);
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::profile_select::unique_multi_face_deleted_carrier_family(
                &[10, 11, 30, 31],
                &ambiguous,
                decode_ctx,
            )
        })
        .unwrap(),
        None
    );

    let mut incomplete = topology;
    incomplete
        .face_surfaces
        .retain(|binding| binding.entity != 20);
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::profile_select::unique_multi_face_deleted_carrier_family(
                &[10, 11, 20],
                &incomplete,
                decode_ctx,
            )
        })
        .unwrap(),
        None
    );
}

mod carrier_boundaries;
