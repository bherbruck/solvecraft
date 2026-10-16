// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::SketchEntity;

fn text_fixture(frame: bool) -> (SketchRelation, SketchEntity, SketchEntity) {
    let sketch = SketchId::mint("synthetic:test:id#text-allocation-sketch").unwrap();
    let path = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#text-allocation-path").unwrap(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 0.0),
        })
        .unwrap(),
    );
    let text = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#text-allocation-text").unwrap(),
        sketch,
        SketchGeometry::try_from(SketchGeometryDefinition::Text {
            text: cadmpeg_core::text::NonBlankString::new("A").unwrap(),
            font_family: cadmpeg_core::text::NonBlankString::new("Arial").unwrap(),
            font_weight: cadmpeg_ir::sketches::SketchFontWeight::Regular,
            height: Length::new(10.0).unwrap(),
            width_factor: Some(0.8),
            placement: None,
            horizontal_alignment: None,
            vertical_alignment: None,
        })
        .unwrap(),
    );
    let mut glyph = [[0.0; 4]; 4];
    for ordinal in 0..4 {
        glyph[ordinal][ordinal] = 1.0;
    }
    let (state, members, pattern) = if frame {
        (
            0x100_0000_0000,
            vec![
                SketchRelationMember::from_index(2),
                SketchRelationMember::from_index(1),
            ],
            SketchPatternDefinition::TextFrame { text_reference: 2 },
        )
    } else {
        (
            0x200_0000_0000,
            vec![
                SketchRelationMember::from_index(1),
                SketchRelationMember::from_index(2),
            ],
            SketchPatternDefinition::TextPath {
                text_reference: 2,
                glyph_transforms: vec![
                    crate::records::sketch_relations::SketchGlyphTransform::try_from(glyph)
                        .unwrap(),
                ],
            },
        )
    };
    let relation = SketchRelation::try_new(crate::records::sketch_relations::SketchRelationDraft {
        id: "f3d:test:sketch-relation#text-allocation".to_owned(),
        record_index: 3,
        class_tag: crate::records::references::DesignClassTag::try_from("413".to_owned()).unwrap(),
        byte_offset: 0,
        state_offset: 0,
        owner_reference: 1,
        owner_entity_id: None,
        auxiliary_references: crate::records::identity::ReferenceRun::located(vec![
            crate::records::identity::Located {
                value: 2,
                offset: 0,
            },
        ]),
        rectangular_counted_reference_count: None,
        members: members.try_into().unwrap(),
        owner_reference_offset: 0,
        definition: crate::records::sketch_relations::SketchRelationDefinition::new(
            state,
            Some(pattern),
        )
        .unwrap(),
        entity_genesis: Some(2),
        return_members: vec![SketchRelationReturnMember::from_index(1)]
            .try_into()
            .unwrap(),
        raw_bytes: vec![0; 160],
    })
    .unwrap();
    (relation, path, text)
}

fn assert_text_refusal(frame: bool, retained: bool, operation: &'static str) {
    let (relation, path, text) = text_fixture(frame);
    let projected = std::collections::HashMap::from([(("scope", 1), &path), (("scope", 2), &text)]);
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
                exact_text_relation(&relation, "scope", &projected, &ctx)
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == operation && failure.dimension == (if retained { cadmpeg_core::decode::ResourceDimension::RetainedBytes } else { cadmpeg_core::decode::ResourceDimension::CollectionItems })));
    }
}

#[test]
fn text_frame_entity_refuses_collection_limit() {
    assert_text_refusal(true, false, "f3d sketch constraint text frame entity");
}

#[test]
fn text_frame_entity_id_refuses_retained_limit() {
    assert_text_refusal(true, true, "f3d sketch constraint text frame entity id");
}

#[test]
fn text_frame_text_id_refuses_retained_limit() {
    assert_text_refusal(true, true, "f3d sketch constraint text frame text id");
}

#[test]
fn text_path_glyph_refuses_collection_limit() {
    assert_text_refusal(false, false, "f3d sketch constraint text glyph transform");
}

#[test]
fn text_path_text_id_refuses_retained_limit() {
    assert_text_refusal(false, true, "f3d sketch constraint text path text id");
}

#[test]
fn text_path_curve_id_refuses_retained_limit() {
    assert_text_refusal(false, true, "f3d sketch constraint text path curve id");
}
