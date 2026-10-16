// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::u64_from_index;

use crate::design::feature_project::bind_sketch_feature_geometry;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation, PlanarProfileRef,
    SheetMetalThicknessSide, SketchFeatureBinding,
};
use cadmpeg_ir::sketches::SketchId;
use std::collections::BTreeMap;

fn fixture() -> Vec<Feature> {
    let sketch_id = SketchId::mint("synthetic:test:id#f3d:sketch:planar").unwrap();
    let feature = |id: &str, ordinal, definition| Feature {
        id: FeatureId::mint(id).unwrap(),
        ordinal,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::from_definition(definition),
        native_ref: None,
    };
    vec![
        feature(
            "synthetic:test:id#f3d:feature:sketch",
            0,
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: SketchFeatureBinding::Planar(Some(sketch_id.clone())),
            }),
        ),
        feature(
            "synthetic:test:id#f3d:feature:flange",
            1,
            FeatureDefinition::Operation(FeatureOperation::SheetMetalBaseFlange {
                profile: PlanarProfileRef::Sketch(sketch_id),
                thickness: cadmpeg_ir::scalar::PositiveLength::new(1.0).unwrap(),
                side: SheetMetalThicknessSide::Forward,
            }),
        ),
    ]
}

fn assert_refusal(operation: &'static str, retained: bool) {
    let materialized = retained && operation.starts_with("f3d sketch feature index");

    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            if materialized {
                ResourceDimension::MaterializedBytes
            } else if retained {
                ResourceDimension::RetainedBytes
            } else {
                ResourceDimension::CollectionItems
            },
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                if materialized {
                    policy.limits.max_materialized_bytes = cap;
                } else if retained {
                    policy.limits.max_retained_bytes = cap;
                } else {
                    policy.limits.max_collection_items = cap;
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let mut features = fixture();
                bind_sketch_feature_geometry(&ctx, &mut features, &[], &[], &[], &[])
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == operation && failure.dimension == (if materialized { ResourceDimension::MaterializedBytes } else if retained { ResourceDimension::RetainedBytes } else { ResourceDimension::CollectionItems })));
    }
}

#[test]
fn sketch_feature_index_key_refuses_retained_limit() {
    assert_refusal("f3d sketch feature index key", true);
}

#[test]
fn sketch_feature_index_id_refuses_retained_limit() {
    assert_refusal("f3d sketch feature index id", true);
}

#[test]
fn sketch_feature_index_refuses_collection_limit() {
    assert_refusal("f3d sketch feature index", false);
}

#[test]
fn sketch_binding_dependency_refuses_collection_limit() {
    assert_refusal("f3d feature dependency", false);
}

#[test]
fn sketch_binding_dependency_id_refuses_retained_limit() {
    assert_refusal("f3d feature dependency id", true);
}

fn spatial_fixture() -> (
    Vec<Feature>,
    crate::records::feature::scope::DesignParameterScope,
    crate::records::sketch_placement::DesignSketchPlacement,
    cadmpeg_ir::sketches::SpatialSketch,
) {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    use crate::records::sketch_placement::{
        DesignSketchFrame, DesignSketchFrameForm, DesignSketchPlacement,
    };
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, LinearTermination, ProfileRef,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#11",
        DesignFeatureKind::Extrude,
        11,
    );
    let placement = DesignSketchPlacement {
        frame: DesignSketchFrame::new(0, DesignSketchFrameForm::ScopeCompact).unwrap(),
        id: "f3d:Design/BulkStream.dat:placement#7".to_owned(),
        scope_record_index: Some(11),
        entity_id: crate::records::identity::DesignEntityId::try_from("Sketch_7".to_owned())
            .unwrap(),
        visibility: None,
        class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned()).unwrap(),
        record_index: 7,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("260".to_owned())
            .unwrap(),
    };
    let spatial_sketch = cadmpeg_ir::sketches::SpatialSketch {
        id: crate::ids::neutral_spatial_sketch_id(&placement),
        name: None,
        configuration: None,
        visible: None,
        profiles: vec![cadmpeg_ir::sketches::SpatialSketchProfile::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            vec![cadmpeg_ir::sketches::SpatialSketchEntityUse {
                entity: cadmpeg_ir::sketches::SpatialSketchEntityId::mint(
                    "synthetic:test:spatial-entity#profile",
                )
                .unwrap(),
                reversed: false,
            }],
            &cadmpeg_test_support::service_decode_context(),
            "spatial profile uniqueness",
        )
        .expect("fixture collection admission")
        .unwrap()],
        native_ref: Some(placement.id.clone()),
    };
    let feature = Feature {
        id: FeatureId::mint("synthetic:test:id#f3d:feature:extrude").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Extrude {
                profile: ProfileRef::Planar(PlanarProfileRef::Sketch(
                    crate::ids::neutral_sketch_id(&placement),
                )),
                direction: Default::default(),
                start: Default::default(),
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::ThroughAll {},
                        draft: None,
                    },
                },
                op: BooleanOp::NewBody,
                solid: Some(true),
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            },
        )),
        native_ref: Some(scope.id.clone()),
    };
    (vec![feature], scope, placement, spatial_sketch)
}

fn assert_spatial_refusal(operation: &'static str, retained: bool) {
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            if retained {
                ResourceDimension::RetainedBytes
            } else {
                ResourceDimension::CollectionItems
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
                let (mut features, scope, placement, spatial) = spatial_fixture();
                let result = bind_sketch_feature_geometry(
                    &ctx,
                    &mut features,
                    std::slice::from_ref(&scope),
                    std::slice::from_ref(&placement),
                    &[],
                    std::slice::from_ref(&spatial),
                );
                result
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == operation && failure.dimension == (if retained { ResourceDimension::RetainedBytes } else { ResourceDimension::CollectionItems })));
    }
}

#[test]
fn extrude_spatial_sketch_id_refuses_retained_limit() {
    assert_spatial_refusal("f3d extrude spatial sketch id", true);
}

#[test]
fn extrude_spatial_profile_index_refuses_collection_limit() {
    assert_spatial_refusal("f3d extrude spatial profile index", false);
}

#[test]
fn extrude_spatial_selection_ref_refuses_retained_limit() {
    let stream = "f3d:Design/BulkStream.dat";
    let expected = format!("{stream}:design-record-header#42");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = u64_from_index(expected.len()) - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = &ctx
        .format_retained(
            format_args!("{}{}{}", stream, ":design-record-header#", 42),
            "f3d extrude spatial selection ref",
        )
        .unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(failure)
        if failure.dimension == ResourceDimension::RetainedBytes
            && failure.operation == "f3d extrude spatial selection ref"));
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| decode_ctx.format_retained(
            format_args!("{}{}{}", stream, ":design-record-header#", 42),
            "f3d extrude spatial selection ref"
        ))
        .unwrap(),
        expected
    );
}
