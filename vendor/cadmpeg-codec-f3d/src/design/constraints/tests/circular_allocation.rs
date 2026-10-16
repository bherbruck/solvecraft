// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::sketches::SketchEntity;

#[derive(Clone, Copy)]
enum Limit {
    Collection,
    Retained,
}

fn circular_fixture() -> (SketchRelation, [SketchEntity; 4]) {
    let sketch = SketchId::mint("synthetic:test:id#circular-allocation-sketch").unwrap();
    let entity = |id: &str, geometry| {
        SketchEntity::new(SketchEntityId::mint(id).unwrap(), sketch.clone(), geometry)
    };
    let center = entity(
        "synthetic:test:id#circular-allocation-center",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(2.0, -3.0),
        })
        .unwrap(),
    );
    let circle = |id: &str, angle: f64| {
        entity(
            id,
            SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center: Point2::new(2.0 + 5.0 * angle.cos(), -3.0 + 5.0 * angle.sin()),
                radius: cadmpeg_ir::scalar::Length::new(0.75).unwrap(),
            })
            .unwrap(),
        )
    };
    let seed = circle("synthetic:test:id#circular-allocation-seed", 0.0);
    let middle = circle(
        "synthetic:test:id#circular-allocation-middle",
        std::f64::consts::FRAC_PI_2,
    );
    let last = circle(
        "synthetic:test:id#circular-allocation-last",
        std::f64::consts::PI,
    );
    let relation = SketchRelation::try_new(crate::records::sketch_relations::SketchRelationDraft {
        id: "f3d:native:sketch-relation#circular-allocation".to_owned(),
        record_index: 10,
        class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned()).unwrap(),
        byte_offset: 0,
        state_offset: 0,
        owner_reference: 1,
        owner_entity_id: Some(cadmpeg_core::text::NonBlankString::new("0_1").unwrap()),
        auxiliary_references: crate::records::identity::ReferenceRun::located(vec![
            crate::records::identity::Located {
                value: 20,
                offset: 0,
            },
            crate::records::identity::Located {
                value: 21,
                offset: 0,
            },
        ]),
        rectangular_counted_reference_count: None,
        members: vec![
            SketchRelationMember::from_index(1),
            SketchRelationMember::from_index(2),
            SketchRelationMember::from_index(3),
            SketchRelationMember::from_index(4),
        ]
        .try_into()
        .unwrap(),
        owner_reference_offset: 0,
        definition: crate::records::sketch_relations::SketchRelationDefinition::new(
            0x1000_0000,
            Some(
                crate::records::sketch_relations::SketchPatternDefinition::Circular {
                    angle_parameter: 20,
                    count_parameter: 21,
                    evaluated_angle: cadmpeg_ir::scalar::FiniteReal::new(std::f64::consts::PI)
                        .unwrap(),
                    evaluated_count:
                        crate::records::sketch_relations::SketchPatternCount::try_from(3).unwrap(),
                },
            ),
        )
        .unwrap(),
        entity_genesis: None,
        return_members: vec![
            SketchRelationReturnMember::from_index(2),
            SketchRelationReturnMember::from_index(3),
            SketchRelationReturnMember::from_index(4),
            SketchRelationReturnMember::from_index(1),
        ]
        .try_into()
        .unwrap(),
        raw_bytes: vec![0; 160],
    })
    .unwrap();
    (relation, [center, seed, middle, last])
}

fn assert_circular_refusal(dimension: Limit, operation: &'static str) {
    let (relation, [center, seed, middle, last]) = circular_fixture();
    let members = [&center, &seed, &middle, &last];
    let returned = [&seed, &middle, &last, &center];
    for limit in 0..1024 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        match dimension {
            Limit::Collection => policy.limits.max_collection_items = limit,
            Limit::Retained => policy.limits.max_retained_bytes = limit,
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match exact_circular_pattern(&relation, "native", &[], &members, &returned, &ctx) {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected refusal at {operation}: {other:?}"),
        }
    }
    panic!("no refusal at {operation}");
}

#[test]
fn circular_member_id_refuses_collection_limit() {
    assert_circular_refusal(Limit::Collection, "f3d circular pattern member id");
}

#[test]
fn circular_returned_id_refuses_collection_limit() {
    assert_circular_refusal(Limit::Collection, "f3d circular pattern returned id");
}

#[test]
fn circular_patterned_entity_refuses_collection_limit() {
    assert_circular_refusal(Limit::Collection, "f3d circular pattern entity");
}

#[test]
fn circular_instance_entity_id_refuses_retained_limit() {
    assert_circular_refusal(Limit::Retained, "f3d circular pattern instance entity id");
}

#[test]
fn circular_instance_entity_refuses_collection_limit() {
    assert_circular_refusal(Limit::Collection, "f3d circular pattern instance entity");
}

#[test]
fn circular_instance_refuses_collection_limit() {
    assert_circular_refusal(Limit::Collection, "f3d circular pattern instance");
}

#[test]
fn circular_seed_entity_id_refuses_retained_limit() {
    assert_circular_refusal(Limit::Retained, "f3d circular pattern seed entity id");
}

#[test]
fn circular_seed_entity_refuses_collection_limit() {
    assert_circular_refusal(Limit::Collection, "f3d circular pattern seed entity");
}

#[test]
fn circular_center_id_refuses_retained_limit() {
    assert_circular_refusal(Limit::Retained, "f3d circular pattern center id");
}

#[test]
fn circular_candidate_refuses_collection_limit() {
    assert_circular_refusal(Limit::Collection, "f3d circular pattern candidate");
}
