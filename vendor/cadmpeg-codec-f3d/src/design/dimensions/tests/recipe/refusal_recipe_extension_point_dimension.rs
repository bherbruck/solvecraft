// SPDX-License-Identifier: Apache-2.0
use super::{
    Point2, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};
use cadmpeg_core::decode::ResourceDimension;

fn fixture(operation: &'static str, dimension: ResourceDimension) {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let parameter = cadmpeg_ir::features::ParameterId::mint("synthetic:test:id#parameter")
        .expect("identity grammar");
    let point = |name: &str, u, v| {
        SketchEntity::new(
            SketchEntityId::mint(name).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(u, v),
            })
            .unwrap(),
        )
    };
    let entities = vec![
        SketchEntity::new(
            SketchEntityId::mint("synthetic:test:id#carrier").unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(2.0, 0.0),
                end: Point2::new(0.0, 0.0),
            })
            .unwrap(),
        ),
        point("synthetic:test:id#carrier-start", 2.0, 0.0),
        point("synthetic:test:id#carrier-end", 0.0, 0.0),
        point("synthetic:test:id#extension", 4.0, 0.0),
        point("synthetic:test:id#off-carrier-horizontal", 2.0, 3.0),
        point("synthetic:test:id#off-carrier-vertical", 4.0, 2.0),
    ];
    let candidates = crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::dimensions::recipe_linear_dimension_candidates(
            decode_ctx, &entities, &sketch, 2.0, &parameter, 0.0,
        )
    })
    .unwrap();
    assert!(candidates.len() > 2);
    super::super::assert_dimension_refusal(operation, dimension, |ctx| {
        crate::design::dimensions::recipe_extension_point_dimension(
            ctx,
            &candidates,
            &entities,
            &sketch,
        )
        .transpose()
        .map(|_| ())
    });
}

#[test]
fn recipe_extension_first_locus_refuses_retained_limit() {
    fixture(
        "f3d recipe extension first locus",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn recipe_extension_second_locus_refuses_retained_limit() {
    fixture(
        "f3d recipe extension second locus",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn recipe_extension_parameter_id_refuses_retained_limit() {
    fixture(
        "f3d recipe extension parameter id",
        ResourceDimension::RetainedBytes,
    );
}
