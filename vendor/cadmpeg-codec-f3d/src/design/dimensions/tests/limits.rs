// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::dimensions::{
    container_only_dimension_companions, project_dimension_constraints,
    retain_planar_dimension_constraints, DimensionConstraintInputs,
};
use crate::design::test_support::parameter_record;
use crate::records::dimensions::{
    DesignDimensionAnnotationFrame, DesignDimensionAnnotationFrameDraft,
    DesignDimensionAnnotationOperand, DesignDimensionLocus, DesignDimensionLocusGroup,
    DesignDimensionLocusPair, DesignDimensionLocusPairDraft, DesignDimensionRecipeRecord,
};
use crate::records::parameters::{
    DesignCompanionPayload, DesignParameter, DesignParameterCompanion, DesignParameterOwner,
    DesignParameterOwnerWire,
};
use crate::records::recipes::ConstructionRecipeKind;
use crate::records::sketch_geometry::SketchCurveIdentity;
use crate::records::sketch_placement::{
    DesignSketchFrame, DesignSketchFrameForm, DesignSketchPlacement,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SpatialSketch,
    SpatialSketchEntity, SpatialSketchEntityId, SpatialSketchGeometry,
};

const EPS_NATIVE_FALLBACK_LINEAR: f64 = 1.0e-6;

struct Fixture {
    placement: DesignSketchPlacement,
    parameter: DesignParameter,
    owner: DesignParameterOwner,
    curve: SketchCurveIdentity,
    entity: SketchEntity,
    spatial: SpatialSketch,
}

impl Fixture {
    fn inputs(&self) -> DimensionConstraintInputs<'_> {
        DimensionConstraintInputs {
            placements: std::slice::from_ref(&self.placement),
            parameters: std::slice::from_ref(&self.parameter),
            owners: std::slice::from_ref(&self.owner),
            pairs: &[],
            groups: &[],
            annotation_frames: &[],
            null_pairs: &[],
            companions: &[],
            recipe_records: &[],
            points: &[],
            curves: std::slice::from_ref(&self.curve),
            entities: std::slice::from_ref(&self.entity),
        }
    }

    fn spatial_entity(&self) -> SpatialSketchEntity {
        SpatialSketchEntity::new(
            SpatialSketchEntityId::mint("synthetic:test:id#spatial-dimension-curve").unwrap(),
            self.spatial.id.clone(),
            SpatialSketchGeometry::try_line_from_parts(
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 0.0, 0.0)).unwrap(),
            )
            .unwrap(),
        )
        .with_native_ref(Some(self.curve.id.clone()))
    }
}

fn fixture() -> Fixture {
    let stream = "f3d:Design/BulkStream.dat";
    let placement = DesignSketchPlacement {
        frame: DesignSketchFrame::new(0, DesignSketchFrameForm::ScopeCompact).unwrap(),
        id: format!("{stream}:placement#7"),
        scope_record_index: Some(10),
        entity_id: crate::records::identity::DesignEntityId::try_from("Sketch_7".to_owned())
            .unwrap(),
        visibility: None,
        class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned()).unwrap(),
        record_index: 7,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("260".to_owned())
            .unwrap(),
    };
    let mut parameter = parse_design_parameter_record(&parameter_record(
        Some(21),
        "1 mm",
        "Linear Dimension",
        Some("mm"),
        "d1",
        0.1,
    ))
    .unwrap();
    parameter.id = format!("{stream}:design-parameter#20");
    parameter.record_index = 20;
    let owner = DesignParameterOwner::try_from(DesignParameterOwnerWire {
        id: format!("{stream}:design-parameter-owner#21"),
        byte_offset: 0,
        frame_length: 104,
        class_tag: crate::records::references::DesignClassTag::try_from("292".to_owned()).unwrap(),
        record_index: 21,
        scope_record_index: 10,
        local_ordinal: 0,
        evaluated_value: 0.1,
        evaluated_value_offset: 40,
        parameter_record_index: 20,
        owned_ordinal: 0,
        variant: Some(0),
        companion_record_index: 22,
    })
    .unwrap();
    let curve = SketchCurveIdentity {
        id: format!("{stream}:sketch-curve#30"),
        record_index: 30,
        owner_reference: Some(10),
        class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned()).unwrap(),
        byte_offset: 0,
        geometry_offset: 0,
        entity_genesis: None,
        primary_id: std::num::NonZeroU64::new(30).unwrap(),
        secondary_id: 1,
        geometry: None,
    };
    let entity = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#dimension-curve").unwrap(),
        crate::ids::neutral_sketch_id(&placement),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        })
        .unwrap(),
    )
    .with_native_ref(Some(curve.id.clone()));
    let spatial = SpatialSketch {
        id: crate::ids::neutral_spatial_sketch_id(&placement),
        name: None,
        configuration: None,
        visible: None,
        profiles: Vec::new(),
        native_ref: Some(placement.id.clone()),
    };
    Fixture {
        placement,
        parameter,
        owner,
        curve,
        entity,
        spatial,
    }
}

fn native_fallback_curves(fixture: &mut Fixture) -> [SketchCurveIdentity; 2] {
    fixture.curve.owner_reference = Some(7);
    let mut second = fixture.curve.clone();
    second.id = "f3d:Design/BulkStream.dat:sketch-curve#31".into();
    second.record_index = 31;
    [fixture.curve.clone(), second]
}

fn native_fallback_pair() -> DesignDimensionLocusPair {
    let mut draft = companion_pair().into_draft();
    draft.governing_companion_record_index = 22;
    draft.opaque_index = Some(crate::records::identity::Located {
        value: 0,
        offset: 35,
    });
    draft.loci = [
        DesignDimensionAnnotationOperand {
            geometry_record_index: std::num::NonZeroU32::new(30),
            geometry_reference_offset: 40,
            role: 1,
            role_offset: 50,
        },
        DesignDimensionAnnotationOperand {
            geometry_record_index: std::num::NonZeroU32::new(31),
            geometry_reference_offset: 55,
            role: 2,
            role_offset: 65,
        },
    ];
    DesignDimensionLocusPair::try_new(draft).unwrap()
}

fn native_fallback_group() -> DesignDimensionLocusGroup {
    DesignDimensionLocusGroup {
        id: "f3d:Design/BulkStream.dat:dimension-locus-group#32".into(),
        companion_record_index: 22,
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("423".to_owned()).unwrap(),
        record_index: 32,
        frame_length: 100,
        loci: vec![DesignDimensionLocus {
            returned: crate::records::identity::Located {
                value: 31,
                offset: 10,
            },
            geometry_record_index: 30,
            geometry_reference_offset: 20,
            role: 1,
            role_offset: 30,
        }],
        owner_reference: 7,
        owner_reference_offset: 40,
        owner_role: 0,
        owner_role_offset: 50,
        state: 1,
        state_offset: 60,
        next_class_tag: crate::records::references::DesignClassTag::try_from("259".to_owned())
            .unwrap(),
        next_record_index: 33,
        next_byte_offset: 100,
    }
}

fn assert_exact_pair_variant_refusal(
    variant: &str,
    operation: &'static str,
    dimension: ResourceDimension,
) {
    let mut fixture = fixture();
    let curves = native_fallback_curves(&mut fixture);
    if variant == "points" {
        fixture.entity.geometry = SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(0.0, 0.0),
        })
        .unwrap();
    }
    if variant == "angle" {
        let mut parameter = parse_design_parameter_record(&parameter_record(
            Some(21),
            "0.1 rad",
            "Angular Dimension",
            Some("rad"),
            "a1",
            0.1,
        ))
        .unwrap();
        parameter.id = fixture.parameter.id.clone();
        parameter.record_index = fixture.parameter.record_index;
        fixture.parameter = parameter;
    }
    let second_geometry = match variant {
        "points" => SketchGeometryDefinition::Point {
            position: Point2::new(0.6, 0.8),
        },
        "angle" => SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(0.1_f64.cos(), 0.1_f64.sin()),
        },
        _ => SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 1.0),
            end: Point2::new(1.0, 1.0),
        },
    };
    let second = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#dimension-second-curve").unwrap(),
        fixture.entity.sketch.clone(),
        SketchGeometry::try_from(second_geometry).unwrap(),
    )
    .with_native_ref(Some(curves[1].id.clone()));
    let entities = [fixture.entity.clone(), second];
    let pair = native_fallback_pair();
    let mut inputs = fixture.inputs();
    inputs.curves = &curves;
    inputs.entities = &entities;
    inputs.pairs = std::slice::from_ref(&pair);
    for limit in 0..16_384 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        match dimension {
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
            _ => panic!("unsupported exact pair limit"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match project_dimension_constraints(&ctx, &inputs, &[], EPS_NATIVE_FALLBACK_LINEAR) {
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == dimension && failure.operation == operation =>
            {
                return
            }
            Err(CodecError::ResourceLimit(_)) => {}
            Ok(_) => panic!("expected {operation} refusal, got success"),
            Err(error) => panic!("expected {operation} refusal: {error}"),
        }
    }
    panic!("no {operation} refusal");
}

fn assert_exact_pair_refusal(operation: &'static str, dimension: ResourceDimension) {
    assert_exact_pair_variant_refusal("parallel", operation, dimension);
}

#[test]
fn exact_dimension_entity_refuses_collection_limit() {
    assert_exact_pair_refusal(
        "f3d exact dimension entity",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn exact_directional_parameter_refuses_retained_limit() {
    assert_exact_pair_refusal(
        "f3d exact directional parameter id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn exact_distance_entity_id_refuses_retained_limit() {
    assert_exact_pair_refusal(
        "f3d exact distance entity id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn exact_distance_entity_refuses_collection_limit() {
    assert_exact_pair_refusal(
        "f3d exact distance entity",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn exact_pair_companion_refuses_collection_limit() {
    assert_exact_pair_refusal(
        "f3d exact pair companion",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn exact_first_distance_locus_refuses_retained_limit() {
    assert_exact_pair_variant_refusal(
        "points",
        "f3d exact first distance locus id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn exact_second_distance_locus_refuses_retained_limit() {
    assert_exact_pair_variant_refusal(
        "points",
        "f3d exact second distance locus id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn exact_first_angle_entity_refuses_retained_limit() {
    assert_exact_pair_variant_refusal(
        "angle",
        "f3d exact first angle entity id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn exact_second_angle_entity_refuses_retained_limit() {
    assert_exact_pair_variant_refusal(
        "angle",
        "f3d exact second angle entity id",
        ResourceDimension::RetainedBytes,
    );
}

fn native_fallback_null_pair() -> DesignDimensionLocusPair {
    let mut draft = companion_pair().into_draft();
    draft.governing_companion_record_index = 22;
    draft.loci[1].geometry_record_index = std::num::NonZeroU32::new(30);
    DesignDimensionLocusPair::try_new(draft).unwrap()
}

fn native_fallback_annotation() -> DesignDimensionAnnotationFrame {
    crate::test_support::with_decode_context(|ctx| {
        DesignDimensionAnnotationFrame::try_new_charged(
            ctx,
            DesignDimensionAnnotationFrameDraft {
                id: "f3d:Design/BulkStream.dat:design-dimension-annotation-frame#34".into(),
                companion_record_index: Some(22),
                governing_companion_record_index: 22,
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("256".to_owned())
                    .unwrap(),
                record_index: 34,
                frame_length: 100,
                operands: vec![
                    DesignDimensionAnnotationOperand {
                        geometry_record_index: None,
                        geometry_reference_offset: 25,
                        role: 3,
                        role_offset: 35,
                    },
                    DesignDimensionAnnotationOperand {
                        geometry_record_index: std::num::NonZeroU32::new(30),
                        geometry_reference_offset: 40,
                        role: 2,
                        role_offset: 50,
                    },
                ],
                entity_genesis: 0,
                annotation_bytes: Vec::new(),
                annotation_byte_offset: 111,
                governing_owner_record_index: 21,
                governing_owner_reference_offset: 112,
                return_members: vec![crate::records::identity::Located {
                    value: std::num::NonZeroU32::new(30).unwrap(),
                    offset: 127,
                }],
                paired_class_tag: crate::records::references::DesignClassTag::try_from(
                    "256".to_owned(),
                )
                .unwrap(),
                paired_byte_offset: 100,
                owner_reference: 7,
                owner_reference_offset: 120,
            },
        )
    })
    .unwrap()
}

fn dimension_recipe_record(index: u32) -> DesignDimensionRecipeRecord {
    DesignDimensionRecipeRecord {
        id: format!("f3d:Design/BulkStream.dat:dimension-recipe#{index}"),
        companion_record_index: 22,
        recipe_ordinal: index,
        recipe_id: format!("f3d:Design/BulkStream.dat:construction-recipe#{index}"),
        recipe_kind: ConstructionRecipeKind::Edge,
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("423".to_owned()).unwrap(),
        record_index: index,
        frame_length: 10,
        prefix_offset: 0,
        prefix_bytes: Vec::new().into(),
        references: Vec::new(),
        program_offset: 0,
        program: vec![-1],
        matching_edge_operand_ids: Vec::new(),
    }
}

fn assert_recipe_group_refusal(operation: &'static str) {
    let fixture = fixture();
    let recipes = [dimension_recipe_record(40), dimension_recipe_record(41)];
    let mut inputs = fixture.inputs();
    inputs.recipe_records = &recipes;
    for limit in 0..128 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match project_dimension_constraints(&ctx, &inputs, &[], EPS_NATIVE_FALLBACK_LINEAR) {
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == operation =>
            {
                return
            }
            Err(CodecError::ResourceLimit(_)) => {}
            Ok(_) => panic!("expected {operation} refusal, got success"),
            Err(error) => panic!("expected {operation} refusal: {error}"),
        }
    }
    panic!("no {operation} refusal");
}

fn assert_recipe_projection_refusal(operation: &'static str, dimension: ResourceDimension) {
    let mut fixture = fixture();
    let mut parameter = parse_design_parameter_record(&parameter_record(
        Some(21),
        "0.1 rad",
        "Angular Dimension",
        Some("rad"),
        "a1",
        0.1,
    ))
    .unwrap();
    parameter.id = fixture.parameter.id.clone();
    parameter.record_index = fixture.parameter.record_index;
    fixture.parameter = parameter;
    let companion = parameter_companion();
    let recipes = [dimension_recipe_record(40), dimension_recipe_record(41)];
    let mut inputs = fixture.inputs();
    inputs.companions = std::slice::from_ref(&companion);
    inputs.recipe_records = &recipes;
    for limit in 0..16_384 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        match dimension {
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
            _ => panic!("unsupported recipe projection limit"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match project_dimension_constraints(&ctx, &inputs, &[], EPS_NATIVE_FALLBACK_LINEAR) {
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == dimension && failure.operation == operation =>
            {
                return
            }
            Err(CodecError::ResourceLimit(_)) => {}
            Ok(_) => panic!("expected {operation} refusal, got success"),
            Err(error) => panic!("expected {operation} refusal: {error}"),
        }
    }
    panic!("no {operation} refusal");
}

fn assert_companion_projection_refusal(operation: &'static str, dimension: ResourceDimension) {
    let mut fixture = fixture();
    let mut parameter = parse_design_parameter_record(&parameter_record(
        Some(21),
        "0.1 rad",
        "Angular Dimension",
        Some("rad"),
        "a1",
        0.1,
    ))
    .unwrap();
    parameter.id = fixture.parameter.id.clone();
    parameter.record_index = fixture.parameter.record_index;
    fixture.parameter = parameter;
    let companion = parameter_companion().bound(DesignCompanionPayload::new(58, 1, Vec::new()));
    let mut inputs = fixture.inputs();
    inputs.companions = std::slice::from_ref(&companion);
    for limit in 0..16_384 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        match dimension {
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
            _ => panic!("unsupported companion projection limit"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match project_dimension_constraints(&ctx, &inputs, &[], EPS_NATIVE_FALLBACK_LINEAR) {
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == dimension && failure.operation == operation =>
            {
                return
            }
            Err(CodecError::ResourceLimit(_)) => {}
            Ok(_) => panic!("expected {operation} refusal, got success"),
            Err(error) => panic!("expected {operation} refusal: {error}"),
        }
    }
    panic!("no {operation} refusal");
}

#[test]
fn companion_dimension_sketch_refuses_retained_limit() {
    assert_companion_projection_refusal(
        "f3d companion dimension sketch id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn companion_native_parameter_refuses_retained_limit() {
    assert_companion_projection_refusal(
        "f3d companion native parameter id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn companion_native_operand_reference_refuses_retained_limit() {
    assert_companion_projection_refusal(
        "f3d companion native operand reference",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn companion_constraint_native_reference_refuses_retained_limit() {
    assert_companion_projection_refusal(
        "f3d companion constraint native reference",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn companion_dimension_constraint_refuses_collection_limit() {
    assert_companion_projection_refusal(
        "f3d companion dimension constraint",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn parallel_group_parameter_refuses_retained_limit() {
    let mut fixture = fixture();
    let mut parameter = parse_design_parameter_record(&parameter_record(
        Some(21),
        "0.1 rad",
        "Angular Dimension",
        Some("rad"),
        "a1",
        0.1,
    ))
    .unwrap();
    parameter.id = fixture.parameter.id.clone();
    parameter.record_index = fixture.parameter.record_index;
    fixture.parameter = parameter;
    let companion = parameter_companion().bound(DesignCompanionPayload::new(58, 1, Vec::new()));
    let mut group = native_fallback_group();
    group.loci[0].geometry_record_index = 99;
    group.owner_reference = 999;
    let mut inputs = fixture.inputs();
    inputs.companions = std::slice::from_ref(&companion);
    inputs.groups = std::slice::from_ref(&group);
    let operation = "f3d parallel group parameter id";
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                project_dimension_constraints(&ctx, &inputs, &[], EPS_NATIVE_FALLBACK_LINEAR)
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == operation && failure.dimension == (ResourceDimension::RetainedBytes)));
    }
}

#[test]
fn recipe_dimension_sketch_refuses_retained_limit() {
    assert_recipe_projection_refusal(
        "f3d recipe dimension sketch id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn recipe_repeated_parameter_refuses_retained_limit() {
    assert_recipe_projection_refusal(
        "f3d recipe repeated parameter id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn recipe_native_operand_reference_refuses_retained_limit() {
    assert_recipe_projection_refusal(
        "f3d recipe native operand reference",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn recipe_native_operand_refuses_collection_limit() {
    assert_recipe_projection_refusal(
        "f3d recipe native operand",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn recipe_constraint_native_reference_refuses_retained_limit() {
    assert_recipe_projection_refusal(
        "f3d recipe constraint native reference",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn recipe_dimension_constraint_refuses_collection_limit() {
    assert_recipe_projection_refusal(
        "f3d recipe dimension constraint",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn dimension_recipe_group_refuses_collection_limit() {
    assert_recipe_group_refusal("f3d dimension recipe group");
}

#[test]
fn dimension_recipe_group_member_refuses_collection_limit() {
    assert_recipe_group_refusal("f3d dimension recipe group member");
}

#[test]
fn projected_dimension_parameter_id_refuses_retained_limit() {
    assert_native_fallback_refusal(
        false,
        "f3d projected dimension parameter id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn projected_dimension_parameter_refuses_collection_limit() {
    assert_native_fallback_refusal(
        false,
        "f3d projected dimension parameter",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn group_dimension_constraint_refuses_collection_limit() {
    assert_native_fallback_refusal(
        true,
        "f3d group dimension constraint",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn pair_dimension_output_refuses_collection_limit() {
    assert_native_fallback_refusal(
        false,
        "f3d dimension constraint output",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn group_dimension_output_refuses_collection_limit() {
    assert_native_fallback_refusal(
        true,
        "f3d dimension constraint output",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn annotation_dimension_output_refuses_collection_limit() {
    assert_native_auxiliary_refusal(
        true,
        "f3d dimension constraint output",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn annotation_dimension_index_refuses_collection_limit() {
    assert_native_auxiliary_refusal(
        true,
        "f3d annotation dimension index",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn group_dimension_locus_index_refuses_collection_limit() {
    assert_native_fallback_refusal(
        true,
        "f3d group dimension locus index",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn null_pair_dimension_output_refuses_collection_limit() {
    assert_native_auxiliary_refusal(
        false,
        "f3d dimension constraint output",
        ResourceDimension::CollectionItems,
    );
}

fn assert_native_auxiliary_refusal(
    annotation: bool,
    operation: &'static str,
    dimension: ResourceDimension,
) {
    let fixture = fixture();
    let pair = native_fallback_null_pair();
    let frame = native_fallback_annotation();
    let mut inputs = fixture.inputs();
    if annotation {
        inputs.annotation_frames = std::slice::from_ref(&frame);
    } else {
        inputs.null_pairs = std::slice::from_ref(&pair);
    }
    for limit in 0..16_384 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        match dimension {
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
            _ => panic!("unsupported native auxiliary limit"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match project_dimension_constraints(&ctx, &inputs, &[], EPS_NATIVE_FALLBACK_LINEAR) {
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == dimension && failure.operation == operation =>
            {
                return
            }
            Err(CodecError::ResourceLimit(_)) => {}
            Ok(_) => panic!("expected {operation} refusal, got success"),
            Err(error) => panic!("expected {operation} refusal: {error}"),
        }
    }
    panic!("no {operation} refusal");
}

fn assert_native_fallback_refusal(
    group: bool,
    operation: &'static str,
    dimension: ResourceDimension,
) {
    let mut fixture = fixture();
    let curves = native_fallback_curves(&mut fixture);
    let pair = native_fallback_pair();
    let locus_group = native_fallback_group();
    let mut inputs = fixture.inputs();
    inputs.curves = &curves;
    if group {
        inputs.groups = std::slice::from_ref(&locus_group);
    } else {
        inputs.pairs = std::slice::from_ref(&pair);
    }
    for limit in 0..16_384 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        match dimension {
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
            _ => panic!("unsupported native fallback limit"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match project_dimension_constraints(&ctx, &inputs, &[], EPS_NATIVE_FALLBACK_LINEAR) {
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == dimension && failure.operation == operation =>
            {
                return
            }
            Err(CodecError::ResourceLimit(_)) => {}
            Ok(_) => panic!("expected {operation} refusal, got success"),
            Err(error) => panic!("expected {operation} refusal: {error}"),
        }
    }
    panic!("no {operation} refusal");
}

fn assert_exact_group_variant_refusal(
    variant: &str,
    operation: &'static str,
    dimension: ResourceDimension,
) {
    let mut fixture = fixture();
    let curves = native_fallback_curves(&mut fixture);
    let mut group = native_fallback_group();
    if variant == "offset" {
        group.state = 0x20;
    } else if variant == "radial-call" {
        group.state = 0;
    }
    if variant == "angular" {
        let mut parameter = parse_design_parameter_record(&parameter_record(
            Some(21),
            "0.1 rad",
            "Angular Dimension",
            Some("rad"),
            "a1",
            0.1,
        ))
        .unwrap();
        parameter.id = fixture.parameter.id.clone();
        parameter.record_index = fixture.parameter.record_index;
        fixture.parameter = parameter;
    }
    let mut inputs = fixture.inputs();
    inputs.curves = &curves;
    inputs.groups = std::slice::from_ref(&group);
    for limit in 0..16_384 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        match dimension {
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
            _ => panic!("unsupported exact group limit"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match project_dimension_constraints(&ctx, &inputs, &[], EPS_NATIVE_FALLBACK_LINEAR) {
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == dimension && failure.operation == operation =>
            {
                return
            }
            Err(CodecError::ResourceLimit(_)) => {}
            Ok(_) => panic!("expected {operation} refusal, got success"),
            Err(error) => panic!("expected {operation} refusal: {error}"),
        }
    }
    panic!("no {operation} refusal");
}

fn assert_radial_extension_refusal(operation: &'static str, dimension: ResourceDimension) {
    let mut fixture = fixture();
    let mut parameter = parse_design_parameter_record(&parameter_record(
        Some(21),
        "1 mm",
        "Radial Dimension-2",
        Some("mm"),
        "r1",
        0.1,
    ))
    .unwrap();
    parameter.id = fixture.parameter.id.clone();
    parameter.record_index = fixture.parameter.record_index;
    fixture.parameter = parameter;
    let [line_curve, point_curve] = native_fallback_curves(&mut fixture);
    let mut circle_curve = line_curve.clone();
    circle_curve.id = "f3d:Design/BulkStream.dat:sketch-curve#32".into();
    circle_curve.record_index = 32;
    let curves = [line_curve, point_curve, circle_curve];
    fixture.entity.geometry = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(6.0, 0.0),
    })
    .unwrap();
    let point = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#radial-extension-point").unwrap(),
        fixture.entity.sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(6.5, 0.0),
        })
        .unwrap(),
    )
    .with_native_ref(Some(curves[1].id.clone()));
    let circle = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#radial-measurement-circle").unwrap(),
        fixture.entity.sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: cadmpeg_ir::scalar::Length::new(1.0).unwrap(),
        })
        .unwrap(),
    )
    .with_native_ref(Some(curves[2].id.clone()));
    let entities = [fixture.entity.clone(), point, circle];
    let mut group = native_fallback_group();
    group.state = 0;
    let mut point_locus = group.loci[0].clone();
    point_locus.geometry_record_index = 31;
    point_locus.role = 1;
    let mut line_locus = group.loci[0].clone();
    line_locus.geometry_record_index = 30;
    line_locus.role = 2;
    group.loci = vec![point_locus, line_locus];
    let mut inputs = fixture.inputs();
    inputs.curves = &curves;
    inputs.entities = &entities;
    inputs.groups = std::slice::from_ref(&group);
    for limit in 0..16_384 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        match dimension {
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
            _ => panic!("unsupported radial extension limit"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match project_dimension_constraints(&ctx, &inputs, &[], EPS_NATIVE_FALLBACK_LINEAR) {
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == dimension && failure.operation == operation =>
            {
                return
            }
            Err(CodecError::ResourceLimit(_)) => {}
            Ok(_) => panic!("expected {operation} refusal, got success"),
            Err(error) => panic!("expected {operation} refusal: {error}"),
        }
    }
    panic!("no {operation} refusal");
}

#[test]
fn radial_extension_locus_index_refuses_collection_limit() {
    assert_radial_extension_refusal(
        "f3d radial dimension locus index",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn radial_extension_group_refuses_collection_limit() {
    assert_radial_extension_refusal(
        "f3d radial extension group",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn radial_extension_sketch_refuses_retained_limit() {
    assert_radial_extension_refusal(
        "f3d dimension radial sketch id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn exact_group_locus_entity_refuses_collection_limit() {
    assert_exact_group_variant_refusal(
        "linear",
        "f3d exact group locus entity",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn exact_group_directional_parameter_refuses_retained_limit() {
    assert_exact_group_variant_refusal(
        "linear",
        "f3d exact group directional parameter id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn exact_group_offset_entity_index_refuses_collection_limit() {
    assert_exact_group_variant_refusal(
        "offset",
        "f3d exact group offset entity index",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn exact_group_offset_secondary_index_refuses_collection_limit() {
    assert_exact_group_variant_refusal(
        "offset",
        "f3d exact group offset secondary index",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn exact_group_angular_index_refuses_collection_limit() {
    assert_exact_group_variant_refusal(
        "angular",
        "f3d exact group angular index",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn exact_group_angular_parameter_refuses_retained_limit() {
    assert_exact_group_variant_refusal(
        "angular",
        "f3d exact group angular parameter id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn radial_group_parameter_refuses_retained_limit() {
    assert_exact_group_variant_refusal(
        "radial-call",
        "f3d radial group parameter id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn radial_group_locus_entity_refuses_collection_limit() {
    assert_exact_group_variant_refusal(
        "radial-call",
        "f3d radial group locus entity",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn projected_group_parameter_refuses_retained_limit() {
    assert_exact_group_variant_refusal(
        "linear",
        "f3d projected group parameter id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn group_constraint_parameter_refuses_retained_limit() {
    assert_exact_group_variant_refusal(
        "linear",
        "f3d group constraint parameter id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn pair_exact_parameter_refuses_retained_limit() {
    assert_native_fallback_refusal(
        false,
        "f3d pair exact parameter id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn annotation_exact_parameter_refuses_retained_limit() {
    assert_native_auxiliary_refusal(
        true,
        "f3d annotation exact parameter id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn null_pair_exact_parameter_refuses_retained_limit() {
    assert_native_auxiliary_refusal(
        false,
        "f3d null pair exact parameter id",
        ResourceDimension::RetainedBytes,
    );
}

fn assert_projected_companion_refusal(kind: &str, operation: &'static str) {
    let mut fixture = fixture();
    let curves = native_fallback_curves(&mut fixture);
    let pair = native_fallback_pair();
    let null_pair = native_fallback_null_pair();
    let group = native_fallback_group();
    let frame = native_fallback_annotation();
    let mut inputs = fixture.inputs();
    inputs.curves = &curves;
    match kind {
        "pair" => inputs.pairs = std::slice::from_ref(&pair),
        "null-pair" => inputs.null_pairs = std::slice::from_ref(&null_pair),
        "group" => inputs.groups = std::slice::from_ref(&group),
        "annotation" => inputs.annotation_frames = std::slice::from_ref(&frame),
        _ => panic!("unknown companion kind"),
    }
    for limit in 0..256 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match project_dimension_constraints(&ctx, &inputs, &[], EPS_NATIVE_FALLBACK_LINEAR) {
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == operation =>
            {
                return
            }
            Err(CodecError::ResourceLimit(_)) => {}
            Ok(_) => panic!("expected {operation} refusal, got success"),
            Err(error) => panic!("expected {operation} refusal: {error}"),
        }
    }
    panic!("no {operation} refusal");
}

#[test]
fn projected_pair_companion_refuses_collection_limit() {
    assert_projected_companion_refusal("pair", "f3d projected pair companion");
}

#[test]
fn projected_null_pair_companion_refuses_collection_limit() {
    assert_projected_companion_refusal("null-pair", "f3d projected null-pair companion");
}

#[test]
fn projected_group_companion_refuses_collection_limit() {
    assert_projected_companion_refusal("group", "f3d projected group companion");
}

#[test]
fn projected_annotation_companion_refuses_collection_limit() {
    assert_projected_companion_refusal("annotation", "f3d projected annotation companion");
}

#[test]
fn native_pair_entity_id_refuses_retained_limit() {
    assert_native_fallback_refusal(
        false,
        "f3d native dimension entity id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn native_pair_entity_refuses_collection_limit() {
    assert_native_fallback_refusal(
        false,
        "f3d native dimension entity",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn native_pair_operand_reference_refuses_retained_limit() {
    assert_native_fallback_refusal(
        false,
        "f3d dimension native operand reference",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn native_pair_operand_refuses_collection_limit() {
    assert_native_fallback_refusal(
        false,
        "f3d native dimension operand",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn native_pair_output_refuses_collection_limit() {
    assert_native_fallback_refusal(
        false,
        "f3d pair dimension constraint",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn native_pair_sketch_id_refuses_retained_limit() {
    assert_native_fallback_refusal(
        false,
        "f3d dimension pair sketch id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn native_pair_constraint_reference_refuses_retained_limit() {
    assert_native_fallback_refusal(
        false,
        "f3d dimension pair native reference",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn native_group_constraint_reference_refuses_retained_limit() {
    assert_native_fallback_refusal(
        true,
        "f3d dimension group native reference",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn annotation_constraint_reference_refuses_retained_limit() {
    assert_native_auxiliary_refusal(
        true,
        "f3d dimension annotation native reference",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn null_pair_constraint_reference_refuses_retained_limit() {
    assert_native_auxiliary_refusal(
        false,
        "f3d dimension null pair native reference",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn exact_null_pair_constraint_reference_refuses_retained_limit() {
    let mut fixture = fixture();
    let mut parameter = parse_design_parameter_record(&parameter_record(
        Some(21),
        "1 mm",
        "Radius Dimension",
        Some("mm"),
        "r1",
        0.1,
    ))
    .unwrap();
    parameter.id = fixture.parameter.id.clone();
    parameter.record_index = fixture.parameter.record_index;
    fixture.parameter = parameter;
    fixture.entity.geometry = SketchGeometry::try_from(SketchGeometryDefinition::Circle {
        center: Point2::new(0.0, 0.0),
        radius: cadmpeg_ir::scalar::Length::new(1.0).unwrap(),
    })
    .unwrap();
    let pair = native_fallback_null_pair();
    let mut inputs = fixture.inputs();
    inputs.null_pairs = std::slice::from_ref(&pair);
    let operation = "f3d dimension null pair native reference";
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                project_dimension_constraints(&ctx, &inputs, &[], EPS_NATIVE_FALLBACK_LINEAR)
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == operation && failure.dimension == (ResourceDimension::RetainedBytes)));
    }
}

#[test]
fn exact_radial_parameter_refuses_retained_limit() {
    let mut fixture = fixture();
    let mut parameter = parse_design_parameter_record(&parameter_record(
        Some(21),
        "1 mm",
        "Radius Dimension",
        Some("mm"),
        "r1",
        0.1,
    ))
    .unwrap();
    parameter.id = fixture.parameter.id.clone();
    parameter.record_index = fixture.parameter.record_index;
    fixture.parameter = parameter;
    fixture.entity.geometry = SketchGeometry::try_from(SketchGeometryDefinition::Circle {
        center: Point2::new(0.0, 0.0),
        radius: cadmpeg_ir::scalar::Length::new(1.0).unwrap(),
    })
    .unwrap();
    let frame = native_fallback_annotation();
    let mut inputs = fixture.inputs();
    inputs.annotation_frames = std::slice::from_ref(&frame);
    let operation = "f3d exact radial parameter id";
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                project_dimension_constraints(&ctx, &inputs, &[], EPS_NATIVE_FALLBACK_LINEAR)
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == operation && failure.dimension == (ResourceDimension::RetainedBytes)));
    }
}

#[test]
fn native_group_sketch_id_refuses_retained_limit() {
    assert_native_fallback_refusal(
        true,
        "f3d dimension group sketch id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn annotation_sketch_id_refuses_retained_limit() {
    assert_native_auxiliary_refusal(
        true,
        "f3d dimension annotation sketch id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn null_pair_sketch_id_refuses_retained_limit() {
    assert_native_auxiliary_refusal(
        false,
        "f3d dimension null pair sketch id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn native_group_locus_operand_refuses_collection_limit() {
    assert_native_fallback_refusal(
        true,
        "f3d native group locus operand",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn native_group_owner_operand_refuses_collection_limit() {
    assert_native_fallback_refusal(
        true,
        "f3d native group owner operand",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn native_group_return_operand_refuses_collection_limit() {
    assert_native_fallback_refusal(
        true,
        "f3d native group return operand",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn annotation_native_operand_refuses_collection_limit() {
    assert_native_auxiliary_refusal(
        true,
        "f3d annotation native operand",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn annotation_native_entity_id_refuses_retained_limit() {
    assert_native_auxiliary_refusal(
        true,
        "f3d annotation native entity id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn annotation_native_entity_refuses_collection_limit() {
    assert_native_auxiliary_refusal(
        true,
        "f3d annotation native entity",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn null_pair_native_operand_refuses_collection_limit() {
    assert_native_auxiliary_refusal(
        false,
        "f3d null pair native operand",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn null_pair_native_entity_id_refuses_retained_limit() {
    assert_native_auxiliary_refusal(
        false,
        "f3d null pair native entity id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn null_pair_native_entity_refuses_collection_limit() {
    assert_native_auxiliary_refusal(
        false,
        "f3d null pair native entity",
        ResourceDimension::CollectionItems,
    );
}

fn parameter_companion() -> DesignParameterCompanion {
    DesignParameterCompanion::unbound(
        "f3d:Design/BulkStream.dat:design-parameter-companion#22".to_owned(),
        0,
        crate::records::references::DesignClassTag::try_from("408".to_owned()).unwrap(),
        22,
        21,
        std::num::NonZeroU64::new(1).unwrap(),
        42,
    )
    .bound(DesignCompanionPayload::new(58, 0, Vec::new()))
}

fn assert_refusal(operation: &'static str) {
    let fixture = fixture();
    for limit in 0..32 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match project_dimension_constraints(
            &ctx,
            &fixture.inputs(),
            std::slice::from_ref(&fixture.spatial),
            1.0e-6,
        ) {
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == operation
                    && failure.dimension == ResourceDimension::CollectionItems =>
            {
                return
            }
            Err(CodecError::ResourceLimit(_)) => {}
            Ok(_) => panic!("expected {operation} refusal, got success"),
            Err(error) => panic!("expected {operation} refusal: {error}"),
        }
    }
    panic!("no {operation} refusal");
}

#[test]
fn planar_spatial_sketch_index_refuses_collection_limit() {
    assert_refusal("f3d planar spatial sketch index");
}

#[test]
fn dimension_sketch_index_refuses_collection_limit() {
    assert_refusal("f3d dimension sketch index");
}

#[test]
fn dimension_scope_sketch_index_refuses_collection_limit() {
    assert_refusal("f3d dimension scope sketch index");
}

#[test]
fn dimension_parameter_index_refuses_collection_limit() {
    assert_refusal("f3d dimension parameter index");
}

#[test]
fn dimension_companion_index_refuses_collection_limit() {
    assert_refusal("f3d dimension companion index");
}

#[test]
fn dimension_native_geometry_index_refuses_collection_limit() {
    assert_refusal("f3d dimension native geometry index");
}

#[test]
fn dimension_native_reference_index_refuses_collection_limit() {
    assert_refusal("f3d dimension native reference index");
}

#[test]
fn dimension_projected_entity_index_refuses_collection_limit() {
    assert_refusal("f3d dimension projected entity index");
}

#[test]
fn dimension_curve_secondary_index_refuses_collection_limit() {
    assert_refusal("f3d dimension curve secondary index");
}

#[test]
fn dimension_recipe_owner_index_refuses_collection_limit() {
    assert_refusal("f3d dimension recipe owner index");
}

#[test]
fn dimension_recipe_companion_index_refuses_collection_limit() {
    let fixture = fixture();
    let companion = parameter_companion();
    let mut inputs = fixture.inputs();
    inputs.companions = std::slice::from_ref(&companion);
    let operation = "f3d dimension recipe companion index";
    for limit in 0..64 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match project_dimension_constraints(&ctx, &inputs, &[], EPS_NATIVE_FALLBACK_LINEAR) {
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == operation =>
            {
                return
            }
            Err(CodecError::ResourceLimit(_)) => {}
            Ok(_) => panic!("expected {operation} refusal, got success"),
            Err(error) => panic!("expected {operation} refusal: {error}"),
        }
    }
    panic!("no {operation} refusal");
}

#[test]
fn planar_dimension_output_refuses_collection_limit() {
    let constraint = cadmpeg_ir::sketches::SketchConstraint {
        id: cadmpeg_ir::sketches::SketchConstraintId::mint(
            "synthetic:test:id#dimension-limit-constraint",
        )
        .unwrap(),
        sketch: cadmpeg_ir::sketches::SketchId::mint("synthetic:test:id#dimension-limit-sketch")
            .unwrap(),
        definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
            cadmpeg_ir::sketches::SketchConstraintDefinitionInput::Disabled {},
        )
        .unwrap(),
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error =
        retain_planar_dimension_constraints(&ctx, &[], &[], vec![constraint.clone()]).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(failure)
        if failure.operation == "f3d planar dimension output"
            && failure.dimension == ResourceDimension::CollectionItems));
    let output = crate::test_support::with_decode_context(|decode_ctx| {
        retain_planar_dimension_constraints(decode_ctx, &[], &[], vec![constraint.clone()])
    })
    .unwrap();
    assert_eq!(output, [constraint]);
}

fn companion_pair() -> DesignDimensionLocusPair {
    DesignDimensionLocusPair::try_new(DesignDimensionLocusPairDraft {
        id: "f3d:Design/BulkStream.dat:design-dimension-locus-pair#31".to_owned(),
        companion_record_index: 30,
        governing_companion_record_index: 99,
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("423".to_owned()).unwrap(),
        record_index: 31,
        frame_length: 100,
        opaque_index: None,
        loci: [
            DesignDimensionAnnotationOperand {
                geometry_record_index: None,
                geometry_reference_offset: 25,
                role: 14,
                role_offset: 35,
            },
            DesignDimensionAnnotationOperand {
                geometry_record_index: std::num::NonZeroU32::new(40),
                geometry_reference_offset: 40,
                role: 3,
                role_offset: 50,
            },
        ],
        paired_class_tag: crate::records::references::DesignClassTag::try_from("259".to_owned())
            .unwrap(),
        paired_byte_offset: 100,
    })
    .unwrap()
}

fn assert_companion_refusal(limit: u64, operation: &'static str) {
    let pair = companion_pair();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = limit;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let pairs = [pair];
    let result = container_only_dimension_companions(&ctx, &pairs, &[], &[], &[], &[]);
    assert!(matches!(result, Err(CodecError::ResourceLimit(failure))
        if failure.operation == operation
            && failure.dimension == ResourceDimension::CollectionItems));
}

#[test]
fn physical_dimension_companion_refuses_collection_limit() {
    assert_companion_refusal(0, "f3d physical dimension companion");
}

#[test]
fn governed_dimension_companion_refuses_collection_limit() {
    assert_companion_refusal(1, "f3d governed dimension companion");
}

#[test]
fn container_only_dimension_companion_refuses_collection_limit() {
    assert_companion_refusal(2, "f3d container-only dimension companion");
}

#[test]
fn parameterized_offset_companion_refuses_collection_limit() {
    let mut fixture = fixture();
    let curves = native_fallback_curves(&mut fixture);
    let second = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#offset-result").unwrap(),
        fixture.entity.sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 1.0),
            end: Point2::new(1.0, 1.0),
        })
        .unwrap(),
    )
    .with_native_ref(Some(curves[1].id.clone()));
    let entities = [fixture.entity.clone(), second];
    let mut group = native_fallback_group();
    group.state = 0x20;
    group.loci[0].returned.value = 30;
    let mut result = group.loci[0].clone();
    result.geometry_record_index = 31;
    result.returned.value = 31;
    result.role = 0;
    group.loci.push(result);
    let mut inputs = fixture.inputs();
    inputs.curves = &curves;
    inputs.entities = &entities;
    inputs.groups = std::slice::from_ref(&group);
    for limit in 0..64 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match project_dimension_constraints(&ctx, &inputs, &[], EPS_NATIVE_FALLBACK_LINEAR) {
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == "f3d parameterized offset companion" =>
            {
                return
            }
            Err(CodecError::ResourceLimit(_)) => {}
            result => panic!("expected offset companion refusal: {result:?}"),
        }
    }
    panic!("no offset companion refusal");
}

fn assert_counted_offset_refusal(
    operation: &'static str,
    dimension: ResourceDimension,
    limit: u64,
) {
    let fixture = fixture();
    let circle = |name, radius| {
        SketchEntity::new(
            SketchEntityId::mint(name).unwrap(),
            fixture.entity.sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center: Point2::new(0.0, 0.0),
                radius: cadmpeg_ir::scalar::Length::new(radius).unwrap(),
            })
            .unwrap(),
        )
    };
    let source = circle("synthetic:test:id#offset-source", 1.0);
    let result = circle("synthetic:test:id#offset-result", 2.0);
    let entities = std::collections::HashMap::from([(30, &source), (31, &result)]);
    let mut loci = native_fallback_group().loci;
    loci[0].returned.value = 30;
    let mut result_locus = loci[0].clone();
    result_locus.geometry_record_index = 31;
    result_locus.returned.value = 31;
    result_locus.role = 0;
    loci.push(result_locus);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    match dimension {
        ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
        ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
        _ => panic!("unsupported counted offset limit"),
    }
    let refusal_cap =
        match cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            match dimension {
                cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                    policy.limits.max_retained_bytes = cap;
                }
                cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                    policy.limits.max_collection_items = cap;
                }
                cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                    policy.limits.max_materialized_bytes = cap;
                }
                cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                    policy.limits.max_work_units = cap;
                }
                dimension => panic!("unsupported refusal dimension: {dimension:?}"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            (crate::design::dimensions::exact_counted_offset(
                &ctx,
                &loci,
                &entities,
                &std::collections::HashMap::new(),
                EPS_NATIVE_FALLBACK_LINEAR,
            )
            .transpose())
            .map(|_| ())
        }) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
    policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
    match dimension {
        cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
            policy.limits.max_retained_bytes = refusal_cap;
        }
        cadmpeg_core::decode::ResourceDimension::CollectionItems => {
            policy.limits.max_collection_items = refusal_cap;
        }
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
            policy.limits.max_materialized_bytes = refusal_cap;
        }
        cadmpeg_core::decode::ResourceDimension::WorkUnits => {
            policy.limits.max_work_units = refusal_cap;
        }
        dimension => panic!("unsupported refusal dimension: {dimension:?}"),
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::design::dimensions::exact_counted_offset(&ctx, &loci, &entities,
            &std::collections::HashMap::new(), EPS_NATIVE_FALLBACK_LINEAR).transpose(),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == operation && failure.dimension == dimension
    ));
}

#[test]
fn counted_offset_source_record_refuses_collection_limit() {
    assert_counted_offset_refusal(
        "f3d counted offset source record",
        ResourceDimension::CollectionItems,
        0,
    );
}

#[test]
fn counted_offset_result_record_refuses_collection_limit() {
    assert_counted_offset_refusal(
        "f3d counted offset result record",
        ResourceDimension::CollectionItems,
        1,
    );
}

#[test]
fn counted_offset_used_source_refuses_collection_limit() {
    assert_counted_offset_refusal(
        "f3d counted offset used source",
        ResourceDimension::CollectionItems,
        2,
    );
}

#[test]
fn counted_offset_used_result_refuses_collection_limit() {
    assert_counted_offset_refusal(
        "f3d counted offset used result",
        ResourceDimension::CollectionItems,
        3,
    );
}

#[test]
fn counted_offset_pair_refuses_collection_limit() {
    assert_counted_offset_refusal(
        "f3d counted offset pair",
        ResourceDimension::CollectionItems,
        4,
    );
}

#[test]
fn counted_offset_source_id_refuses_retained_limit() {
    assert_counted_offset_refusal(
        "f3d counted offset source id",
        ResourceDimension::RetainedBytes,
        u64_from_index("synthetic:test:id#offset-source".len()) - 1,
    );
}

#[test]
fn counted_offset_result_id_refuses_retained_limit() {
    assert_counted_offset_refusal(
        "f3d counted offset result id",
        ResourceDimension::RetainedBytes,
        u64_from_index(
            "synthetic:test:id#offset-source".len() + "synthetic:test:id#offset-result".len(),
        ) - 1,
    );
}

mod source_kind_limits;

mod spatial;
