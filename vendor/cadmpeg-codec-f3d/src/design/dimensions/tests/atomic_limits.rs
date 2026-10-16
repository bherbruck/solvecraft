// SPDX-License-Identifier: Apache-2.0
use crate::design::dimensions::exact_atomic_constraint;
use crate::records::sketch_relations::SketchConstraintKind;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};

fn entity(suffix: &str, geometry: SketchGeometry) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("generated:test:atomic-limit#{suffix}")).unwrap(),
        SketchId::mint("generated:test:sketch#atomic-limit").unwrap(),
        geometry,
    )
}

fn point(suffix: &str, x: f64, y: f64) -> SketchEntity {
    entity(
        suffix,
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(x, y),
        })
        .unwrap(),
    )
}

fn line(suffix: &str, start: Point2, end: Point2) -> SketchEntity {
    entity(
        suffix,
        SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).unwrap(),
    )
}

fn pair_lines() -> Vec<SketchEntity> {
    vec![
        line("first", Point2::new(0.0, 0.0), Point2::new(2.0, 0.0)),
        line("second", Point2::new(0.0, 1.0), Point2::new(2.0, 1.0)),
    ]
}

fn symmetry() -> Vec<SketchEntity> {
    vec![
        point("left", -2.0, 3.0),
        line("axis", Point2::new(0.0, -5.0), Point2::new(0.0, 5.0)),
        point("right", 2.0, 3.0),
    ]
}

fn midpoint() -> Vec<SketchEntity> {
    vec![
        line("midline", Point2::new(0.0, 0.0), Point2::new(2.0, 0.0)),
        point("midpoint", 1.0, 0.0),
    ]
}

fn polygon() -> Vec<SketchEntity> {
    vec![
        point("a", 0.0, 0.0),
        point("b", 1.0, 0.0),
        point("c", 0.0, 1.0),
    ]
}

fn assert_refusal(
    kind: SketchConstraintKind,
    entities: &[SketchEntity],
    operation: &'static str,
    dimension: ResourceDimension,
) {
    let references = entities.iter().collect::<Vec<_>>();
    for limit in 0..256 {
        let mut policy = DecodePolicy::default();
        match dimension {
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
            _ => panic!("unsupported dimension"),
        }
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match exact_atomic_constraint(kind, &references, &ctx) {
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == operation && failure.dimension == dimension =>
            {
                return
            }
            Err(CodecError::ResourceLimit(_)) => {}
            Ok(_) => panic!("expected {operation} refusal"),
            Err(error) => panic!("expected {operation} refusal: {error}"),
        }
    }
    panic!("no {operation} refusal");
}

#[test]
fn atomic_first_entity_id_refuses_retained_limit() {
    assert_refusal(
        SketchConstraintKind::Colinear,
        &pair_lines(),
        "f3d atomic first entity id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn atomic_second_entity_id_refuses_retained_limit() {
    assert_refusal(
        SketchConstraintKind::Colinear,
        &pair_lines(),
        "f3d atomic second entity id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn atomic_single_entity_id_refuses_retained_limit() {
    assert_refusal(
        SketchConstraintKind::Horizontal,
        &pair_lines()[..1],
        "f3d atomic single entity id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn atomic_entity_uniqueness_refuses_collection_limit() {
    assert_refusal(
        SketchConstraintKind::Coincident,
        &pair_lines(),
        "f3d atomic entity uniqueness",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn atomic_member_entity_id_refuses_retained_limit() {
    assert_refusal(
        SketchConstraintKind::Coincident,
        &pair_lines(),
        "f3d atomic member entity id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn atomic_member_refuses_collection_limit() {
    assert_refusal(
        SketchConstraintKind::Coincident,
        &pair_lines(),
        "f3d atomic member",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn atomic_symmetry_first_id_refuses_retained_limit() {
    assert_refusal(
        SketchConstraintKind::Symmetry,
        &symmetry(),
        "f3d atomic symmetry first id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn atomic_symmetry_second_id_refuses_retained_limit() {
    assert_refusal(
        SketchConstraintKind::Symmetry,
        &symmetry(),
        "f3d atomic symmetry second id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn atomic_symmetry_axis_id_refuses_retained_limit() {
    assert_refusal(
        SketchConstraintKind::Symmetry,
        &symmetry(),
        "f3d atomic symmetry axis id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn midpoint_point_id_refuses_retained_limit() {
    assert_refusal(
        SketchConstraintKind::Midpoint,
        &midpoint(),
        "f3d midpoint point id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn midpoint_line_id_refuses_retained_limit() {
    assert_refusal(
        SketchConstraintKind::Midpoint,
        &midpoint(),
        "f3d midpoint line id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn atomic_polygon_uniqueness_refuses_collection_limit() {
    assert_refusal(
        SketchConstraintKind::Polygon,
        &polygon(),
        "f3d atomic polygon uniqueness",
        ResourceDimension::CollectionItems,
    );
}
