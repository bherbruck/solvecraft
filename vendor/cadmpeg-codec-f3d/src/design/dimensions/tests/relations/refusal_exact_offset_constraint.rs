// SPDX-License-Identifier: Apache-2.0
use super::{
    exact_offset_constraint, HashMap, Point2, SketchEntityId, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchRelation, SketchRelationOperand,
};
use cadmpeg_core::decode::ResourceDimension;

fn fixture(operation: &'static str, dimension: ResourceDimension) {
    let entity = |id: &str, geometry: SketchGeometry| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            geometry,
        )
    };
    let source_horizontal = entity(
        "generated:test:line#source-horizontal",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 0.0),
        })
        .unwrap(),
    );
    let result_horizontal = entity(
        "generated:test:line#result-horizontal",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(2.0, -2.0),
            end: Point2::new(8.0, -2.0),
        })
        .unwrap(),
    );
    let source_vertical = entity(
        "generated:test:line#source-vertical",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 10.0),
            end: Point2::new(0.0, 0.0),
        })
        .unwrap(),
    );
    let result_vertical = entity(
        "generated:test:line#result-vertical",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(2.0, 2.0),
            end: Point2::new(2.0, 8.0),
        })
        .unwrap(),
    );
    let curve = |record_index, secondary_id| SketchRelationOperand::Curve {
        record_index,
        primary_id: u64::from(record_index),
        secondary_id,
    };
    let relation = SketchRelation::try_new(crate::records::sketch_relations::SketchRelationDraft {
        id: "f3d:native:sketch-relation#0".into(),
        record_index: 10,
        class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned()).unwrap(),
        byte_offset: 0,
        state_offset: 100,
        owner_reference: 1,
        owner_entity_id: Some(cadmpeg_core::text::NonBlankString::new("0_1").unwrap()),
        auxiliary_references: crate::records::identity::ReferenceRun::located(vec![
            crate::records::identity::Located {
                value: 0,
                offset: 80,
            },
        ]),
        rectangular_counted_reference_count: None,
        members: ([(1, 25, 3), (2, 40, 5), (3, 55, 1), (4, 70, 1)]
            .into_iter()
            .map(|(record_index, offset, relation_ordinal)| {
                crate::records::sketch_relations::SketchRelationMember {
                    reference: crate::records::sketch_relations::SketchRelationReference::Index(
                        record_index,
                    ),
                    offset,
                    relation_ordinal: Some(relation_ordinal),
                }
            })
            .collect::<Vec<_>>())
        .try_into()
        .expect("uniform member resolution"),
        owner_reference_offset: 90,
        definition: crate::records::sketch_relations::SketchRelationDefinition::new(
            0x20_0000_0000,
            None,
        )
        .expect("valid relation definition"),
        entity_genesis: None,
        return_members: ([
            (1, 120, curve(1, 10)),
            (3, 131, curve(3, 30)),
            (2, 142, curve(2, 20)),
            (4, 153, curve(4, 40)),
        ]
        .into_iter()
        .map(|(_record_index, offset, resolved)| {
            crate::records::sketch_relations::SketchRelationReturnMember {
                reference: crate::records::sketch_relations::SketchRelationReference::Resolved(
                    resolved,
                ),
                offset,
            }
        })
        .collect::<Vec<_>>())
        .try_into()
        .expect("uniform member resolution"),
        raw_bytes: vec![0; 160],
    })
    .unwrap();
    let projected = HashMap::from([
        (("native", 1), &source_horizontal),
        (("native", 2), &source_vertical),
        (("native", 3), &result_horizontal),
        (("native", 4), &result_vertical),
    ]);

    super::super::assert_dimension_refusal(operation, dimension, |ctx| {
        exact_offset_constraint(ctx, &relation, "native", &projected)
            .transpose()
            .map(|_| ())
    });
}

#[test]
fn relation_offset_used_source_refuses_collection_limit() {
    fixture(
        "f3d relation offset used source",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn relation_offset_used_result_refuses_collection_limit() {
    fixture(
        "f3d relation offset used result",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn relation_offset_source_id_refuses_retained_limit() {
    fixture(
        "f3d relation offset source id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn relation_offset_result_id_refuses_retained_limit() {
    fixture(
        "f3d relation offset result id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn relation_offset_pair_refuses_collection_limit() {
    fixture(
        "f3d relation offset pair",
        ResourceDimension::CollectionItems,
    );
}
