// SPDX-License-Identifier: Apache-2.0

use crate::records::sketch_placement::DesignSketchPlacement;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::features::DistinctMembers;
use cadmpeg_ir::features::Feature;
use cadmpeg_ir::features::FeatureContent;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::ids::RegionId;
use cadmpeg_ir::topology::Body;
use cadmpeg_ir::topology::BodyKind;
use cadmpeg_ir::topology::Region;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::Native;

use crate::f3z::merge::{
    append_feature_history, compose_transforms, extend_native, occurrence_key,
    reparent_component_roots, rescope_record, OccurrenceScope,
};
use crate::records::xref::XrefReference;
use cadmpeg_ir::features::FeatureOperation;

fn feature(id: &str, ordinal: u64) -> Feature {
    Feature {
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal,
        name: None,
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "test".into(),
                parameters: std::collections::BTreeMap::new(),
            }),
        ),
        native_ref: None,
    }
}

#[test]
fn component_feature_history_follows_the_parent_without_losing_relative_order() {
    let mut parent = Model::default();
    parent.features = vec![
        feature("f3d:test:feature#parent-0", 4),
        feature("f3d:test:feature#parent-1", 8),
    ];
    let mut component = Model::default();
    component.features = vec![
        feature("f3d:test:feature#component-0", 10),
        feature("f3d:test:feature#component-1", 12),
    ];

    crate::test_support::with_decode_context(|ctx| {
        append_feature_history(ctx, &parent, &mut component)
    })
    .unwrap();

    assert_eq!(
        component
            .features
            .iter()
            .map(|feature| feature.ordinal)
            .collect::<Vec<_>>(),
        vec![9, 11]
    );
}

#[test]
fn component_feature_history_refuses_an_exhausted_ordinal_domain() {
    let mut parent = Model::default();
    parent.features = vec![feature("f3d:test:feature#parent", u64::MAX)];
    let mut component = Model::default();
    component.features = vec![feature("f3d:test:feature#component", 0)];

    let error = crate::test_support::with_decode_context(|ctx| {
        append_feature_history(ctx, &parent, &mut component)
    })
    .unwrap_err();

    assert!(error
        .to_string()
        .contains("merged F3Z feature ordinal exceeds u64::MAX"));
}

#[test]
fn occurrence_transform_composes_outside_existing_body_transform() {
    let outer = Transform::affine([
        [0.0, -1.0, 0.0, 20.0],
        [1.0, 0.0, 0.0, 30.0],
        [0.0, 0.0, 1.0, 40.0],
    ])
    .expect("affine transform");
    let inner = Transform::affine([
        [1.0, 0.0, 0.0, 5.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .expect("affine transform");

    assert_eq!(
        compose_transforms(outer, inner).unwrap().rows(),
        [
            [0.0, -1.0, 0.0, 20.0],
            [1.0, 0.0, 0.0, 35.0],
            [0.0, 0.0, 1.0, 40.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    );
}

#[test]
fn merged_component_root_occurrences_become_children_of_the_outer_instance() {
    use cadmpeg_ir::ids::OccurrenceId;
    use cadmpeg_ir::products::{Occurrence, OccurrenceParent, PrototypeReference};

    let outer = OccurrenceId::mint("f3d:model:occurrence#xref-0-0").unwrap();
    let root_child = OccurrenceId::mint("f3d:xref/outer/model:occurrence#xref-0-0").unwrap();
    let nested_child = OccurrenceId::mint("f3d:xref/outer/model:occurrence#xref-1-0").unwrap();
    let mut occurrences = vec![
        Occurrence {
            id: root_child.clone(),
            prototype: PrototypeReference::Unresolved {},
            parent: OccurrenceParent::Root {},
            ordinal: 0,
            transform: Transform::identity(),
            linked_prototype: None,
            scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
            name: None,
            visible: None,
            link: None,
            native_ref: None,
        },
        Occurrence {
            id: nested_child.clone(),
            prototype: PrototypeReference::Unresolved {},
            parent: OccurrenceParent::Occurrence {
                occurrence: root_child.clone(),
            },
            ordinal: 0,
            transform: Transform::identity(),
            linked_prototype: None,
            scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
            name: None,
            visible: None,
            link: None,
            native_ref: None,
        },
    ];

    reparent_component_roots(
        &cadmpeg_test_support::service_decode_context(),
        &mut occurrences,
        &outer,
    )
    .unwrap();

    assert!(matches!(
        occurrences[0].parent,
        OccurrenceParent::Occurrence { ref occurrence } if occurrence == &outer
    ));
    assert!(matches!(
        occurrences[1].parent,
        OccurrenceParent::Occurrence { ref occurrence } if occurrence == &root_child
    ));
}

#[test]
fn reparent_component_root_refuses_retained_identity_limit() {
    use cadmpeg_ir::ids::OccurrenceId;
    use cadmpeg_ir::products::{Occurrence, OccurrenceParent, PrototypeReference};

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    let parent = OccurrenceId::mint("f3d:model:occurrence#xref-0-0").unwrap();
    policy.limits.max_retained_bytes = u64::try_from(parent.as_str().len() - 1).unwrap();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut occurrences = vec![Occurrence {
        id: OccurrenceId::mint("f3d:model:occurrence#child").unwrap(),
        prototype: PrototypeReference::Unresolved {},
        parent: OccurrenceParent::Root {},
        ordinal: 0,
        transform: Transform::identity(),
        linked_prototype: None,
        scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
        name: None,
        visible: None,
        link: None,
        native_ref: None,
    }];

    let error = reparent_component_roots(&ctx, &mut occurrences, &parent).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3Z parent occurrence identity")
    );
    assert!(matches!(occurrences[0].parent, OccurrenceParent::Root {}));
}

#[test]
fn repeated_occurrence_merge_remaps_typed_graphs_disjointly() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut merged = Model::default();
    let mut component = Model::default();
    component.bodies = vec![Body {
        id: BodyId::mint("f3d:brep:entity#1").expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: vec![RegionId::mint("f3d:brep:entity#2").expect("identity grammar")],
        transform: None,
        name: None,
        color: None,
        visible: None,
    }];
    component.regions = vec![Region {
        id: RegionId::mint("f3d:brep:entity#2").expect("identity grammar"),
        body: BodyId::mint("f3d:brep:entity#1").expect("identity grammar"),
        shells: Vec::new(),
    }];
    for ordinal in 0..2 {
        let occurrence = format!("role/occurrence-{ordinal}");
        let mut scope = OccurrenceScope {
            ctx: &ctx,
            occurrence: &occurrence,
        };
        merged
            .extend_rewritten(
                &ctx,
                component.clone(),
                &mut scope,
                "append F3Z model entities",
            )
            .expect("merge component arenas");
    }

    for ordinal in 0..2 {
        let prefix = format!("f3d:xref/role/occurrence-{ordinal}/brep:entity#");
        assert_eq!(merged.bodies[ordinal].id.as_str(), format!("{prefix}1"));
        assert_eq!(
            merged.bodies[ordinal].regions[0].as_str(),
            format!("{prefix}2")
        );
        assert_eq!(merged.regions[ordinal].id.as_str(), format!("{prefix}2"));
        assert_eq!(merged.regions[ordinal].body.as_str(), format!("{prefix}1"));
    }
}

#[test]
fn occurrence_merge_preserves_a_body_name_that_spells_its_identity() {
    use cadmpeg_ir::document::EntityRewrite;
    let ctx = cadmpeg_test_support::service_decode_context();
    let source_id = "f3d:model:body#source";
    let body = Body {
        id: BodyId::mint(source_id).unwrap(),
        kind: BodyKind::Solid,
        regions: vec![RegionId::mint("f3d:model:region#source").unwrap()],
        transform: None,
        name: Some(source_id.into()),
        color: None,
        visible: None,
    };
    let scoped = OccurrenceScope {
        ctx: &ctx,
        occurrence: "component-0",
    }
    .rewrite(body)
    .unwrap();
    assert_eq!(scoped.id.as_str(), "f3d:xref/component-0/model:body#source");
    assert_eq!(
        scoped.regions[0].as_str(),
        "f3d:xref/component-0/model:region#source"
    );
    assert_eq!(scoped.name.as_deref(), Some(source_id));
}

#[test]
fn occurrence_model_identity_rescope_refuses_retained_limit() {
    use cadmpeg_ir::document::EntityRewrite;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let body = Body {
        id: BodyId::mint("f3d:model:body#source").unwrap(),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    };
    let error = OccurrenceScope {
        ctx: &ctx,
        occurrence: "component-0",
    }
    .rewrite(body)
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "rescope F3Z identity")
    );
}

#[test]
fn occurrence_native_identity_rescope_refuses_retained_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    let record = cadmpeg_ir::NativeRecord::new(
        cadmpeg_ir::ids::Identity::new("f3d:model:native#source").expect("fixture identity"),
        serde_json::Map::new(),
    )
    .unwrap();
    policy.limits.max_retained_bytes =
        u64::try_from(serde_json::to_vec(&record).unwrap().len()).unwrap();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = rescope_record(&ctx, &record, "unknowns", "component-0").unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "rescope F3Z identity")
    );
}

#[test]
fn occurrence_native_field_clone_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut fields = serde_json::Map::new();
    fields.insert("links".into(), serde_json::json!(["f3d:model:body#source"]));
    let record = cadmpeg_ir::NativeRecord::new(
        cadmpeg_ir::ids::Identity::new("f3d:model:native#source").expect("fixture identity"),
        fields,
    )
    .unwrap();
    let error = rescope_record(&ctx, &record, "unknowns", "component-0").unwrap_err();
    assert!(
        matches!(&error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy native field sequence")
    );
    let cadmpeg_core::CodecError::ResourceLimit(first) = error else {
        unreachable!()
    };
    assert!(matches!(ctx.finish_session(),
        Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == first));
}

#[test]
fn occurrence_merge_remaps_and_retains_native_records() {
    let placement = DesignSketchPlacement {
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            42,
            crate::records::sketch_placement::DesignSketchFrameForm::MemberCompact {
                paired_byte_offset: 76,
            },
        )
        .unwrap(),
        id: "f3d:Design/BulkStream.dat:design-sketch-placement#42".into(),
        scope_record_index: None,
        entity_id: crate::records::identity::DesignEntityId::try_from("Sketch_1".to_owned())
            .expect("valid entity ID"),

        visibility: None,

        class_tag: crate::records::references::DesignClassTag::try_from("001".to_owned()).unwrap(),
        record_index: 7,

        paired_class_tag: crate::records::references::DesignClassTag::try_from("002".to_owned())
            .unwrap(),
    };
    let mut component = Native::default();
    component
        .namespace_mut("f3d")
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "design_sketch_placements",
            &[placement],
        )
        .expect("store component native");
    let mut root = Native::default();
    extend_native(
        &cadmpeg_test_support::service_decode_context(),
        &mut root,
        component,
        "role/occurrence-0",
    )
    .unwrap();

    let merged: Vec<DesignSketchPlacement> = root
        .namespace("f3d")
        .expect("merged f3d namespace")
        .arena_as("design_sketch_placements")
        .expect("read merged arena");
    assert_eq!(
        merged[0].id,
        "f3d:xref/role/occurrence-0/Design/BulkStream.dat:design-sketch-placement#42"
    );
}

#[test]
fn occurrence_merge_refuses_native_record_collection_limit() {
    let mut component = Native::default();
    component
        .namespace_mut("f3d")
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "xref_designs",
            &[crate::records::xref::XrefDesign {
                id: "f3d:xref:design#0".into(),
                ordinal: 0,
                file_version: 1,
                target_file_name: "part.f3d".to_owned().try_into().unwrap(),
                display_name: "Part".into(),
                lineage_urn: "lineage".into(),
                version_urn: "version".into(),
            }],
        )
        .unwrap();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut root = Native::default();

    let error = extend_native(&ctx, &mut root, component, "component-0").unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "append F3Z native records")
    );
}

#[test]
fn occurrence_configuration_survives_document_and_typed_native_admission() {
    use crate::records::configuration::{DesignConfiguration, DesignConfigurationKind};
    let configuration = crate::test_support::with_decode_context(|ctx| {
        DesignConfiguration::try_new_charged(
            ctx,
            "Design/table.dsgcfg".into(),
            DesignConfigurationKind::Table,
            Vec::new(),
            serde_json::Map::new(),
        )
    })
    .unwrap();
    let mut component = Native::default();
    component
        .namespace_mut("f3d")
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "design_configurations",
            &[configuration],
        )
        .unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    extend_native(
        &cadmpeg_test_support::service_decode_context(),
        &mut ir.native,
        component,
        "component-0",
    )
    .unwrap();
    let wire = ir.to_canonical_json().unwrap();
    let admitted = cadmpeg_ir::CadIr::from_json(&wire).unwrap();
    let configurations = admitted
        .native
        .namespace("f3d")
        .unwrap()
        .arena_as::<DesignConfiguration>("design_configurations")
        .unwrap();
    assert_eq!(configurations.len(), 1);
    assert_eq!(configurations[0].entry_name(), "Design/table.dsgcfg");
    assert_eq!(
        configurations[0].id(),
        "f3d:xref/component-0/configuration:entry#Design/table.dsgcfg"
    );
    assert_eq!(configurations[0].payload(), serde_json::Map::new());
}

#[test]
fn occurrence_merge_scopes_admitted_native_references_and_preserves_configuration_text() {
    use crate::records::{
        bodies::BodyVisibility,
        configuration::{DesignConfiguration, DesignConfigurationKind},
    };

    let configuration_payload = serde_json::json!({
        "raw_text": "f3d:brep:entity#3",
        "nested": {
            "f3d:brep:entity#4": "f3d:brep:entity#5",
        },
    })
    .as_object()
    .expect("object configuration payload")
    .clone();
    let configuration = crate::test_support::with_decode_context(|ctx| {
        DesignConfiguration::try_new_charged(
            ctx,
            "Design/table.dsgcfg".into(),
            DesignConfigurationKind::Table,
            Vec::new(),
            configuration_payload.clone(),
        )
    })
    .expect("admitted configuration payload");
    let visibility = BodyVisibility::try_from(crate::records::bodies::BodyVisibilityWire {
        id: "f3d:Design/BulkStream.dat:body-visibility#3".into(),
        body: BodyId::mint("f3d:brep:entity#1").expect("identity grammar"),
        stream: "Design/BulkStream.dat".into(),
        byte_offset: 10,
        asm_body_key_offset: 20,
        asm_body_key: 3,
        entity_suffix: 1,
        visible: true,
    })
    .unwrap();
    let mut component = Native::default();
    component
        .namespace_mut("f3d")
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "design_configurations",
            &[configuration],
        )
        .expect("store configuration");
    component
        .namespace_mut("f3d")
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "body_visibilities",
            &[visibility],
        )
        .expect("store typed native reference");

    let mut root = Native::default();
    extend_native(
        &cadmpeg_test_support::service_decode_context(),
        &mut root,
        component,
        "role/occurrence-0",
    )
    .unwrap();

    let merged_visibility: Vec<BodyVisibility> = root
        .namespace("f3d")
        .expect("merged f3d namespace")
        .arena_as("body_visibilities")
        .expect("read merged visibility arena");
    assert_eq!(
        merged_visibility[0].id(),
        "f3d:xref/role/occurrence-0/Design/BulkStream.dat:body-visibility#3"
    );
    assert_eq!(
        merged_visibility[0].body.as_str(),
        "f3d:xref/role/occurrence-0/brep:entity#1"
    );
    assert_eq!(merged_visibility[0].stream(), "Design/BulkStream.dat");

    let merged_configurations: Vec<DesignConfiguration> = root
        .namespace("f3d")
        .expect("merged f3d namespace")
        .arena_as("design_configurations")
        .expect("read merged configuration arena");
    assert_eq!(
        merged_configurations[0].id(),
        "f3d:xref/role/occurrence-0/configuration:entry#Design/table.dsgcfg"
    );
    assert_eq!(
        merged_configurations[0].payload(),
        configuration_payload,
        "configuration text and extension map keys are source payload, not identities"
    );
}

#[test]
fn occurrence_key_preserves_authored_roles() {
    assert!(crate::records::xref::RequiredXrefText::try_from(String::new()).is_err());
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let reference = |role: &str, ordinal: u32| XrefReference {
        id: "f3d:xref:reference#1".into(),
        ordinal,
        occurrence_ordinal: 0,
        from: "root.f3d".to_owned().try_into().unwrap(),
        relative_path: "part.f3d".to_owned().try_into().unwrap(),
        neutron_role: role.to_owned().try_into().unwrap(),
        neutron_data: String::new(),
        transform: None,
    };

    assert_eq!(
        occurrence_key(&ctx, &reference("ordinal-7", 7)).unwrap(),
        "role-ordinal-7/reference-7/occurrence-0"
    );
    assert_eq!(
        occurrence_key(&ctx, &reference("role /#: value", 7)).unwrap(),
        "role-role%20%2F%23%3A%20value/reference-7/occurrence-0"
    );
    let key = occurrence_key(&ctx, &reference("role /#: value", 7)).unwrap();
    cadmpeg_ir::ids::Identity::new(format!("f3d:xref/{key}/native:record#1"))
        .expect("encoded occurrence key remains an admitted identity scope");
}

#[test]
fn occurrence_key_separates_same_role_references_with_reset_ordinals() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let reference = |ordinal| XrefReference {
        id: format!("f3d:xref:reference#{ordinal}"),
        ordinal,
        occurrence_ordinal: 0,
        from: "root.f3d".to_owned().try_into().unwrap(),
        relative_path: format!("part-{ordinal}.f3d").try_into().unwrap(),
        neutron_role: "same-role".to_owned().try_into().unwrap(),
        neutron_data: String::new(),
        transform: None,
    };

    assert_ne!(
        occurrence_key(&ctx, &reference(0)).unwrap(),
        occurrence_key(&ctx, &reference(1)).unwrap(),
        "reference ordinal is part of the occurrence owner when role ordinals reset"
    );
}

#[test]
fn occurrence_key_refuses_retained_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let reference = XrefReference {
        id: "f3d:xref:reference#1".into(),
        ordinal: 1,
        occurrence_ordinal: 0,
        from: "root.f3d".to_owned().try_into().unwrap(),
        relative_path: "part.f3d".to_owned().try_into().unwrap(),
        neutron_role: "role /#: value".to_owned().try_into().unwrap(),
        neutron_data: String::new(),
        transform: None,
    };
    let error = occurrence_key(&ctx, &reference).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3Z occurrence key")
    );
}

#[test]
fn occurrence_merge_refuses_destination_growth_before_rewriting() {
    let mut component = Model::default();
    component
        .features
        .push(feature("f3d:test:feature#child", 0));
    let mut parent = Model::default();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let mut scope = OccurrenceScope {
            ctx,
            occurrence: "child",
        };
        let error = parent
            .extend_rewritten(ctx, component, &mut scope, "append F3Z model entities")
            .unwrap_err();
        let cadmpeg_core::CodecError::ResourceLimit(limit) = cadmpeg_core::CodecError::from(error)
        else {
            panic!("destination growth must refuse through the caller context");
        };
        assert_eq!(limit.operation, "append F3Z model entities");
        assert_eq!(Some(limit), ctx.resource_refusal());
        assert!(parent.features.is_empty());
    });
}

#[test]
fn feature_history_scans_preserve_work_refusals() {
    let mut parent = Model::default();
    parent.features = vec![
        feature("f3d:test:feature#parent", 5),
        feature("f3d:test:feature#other", 6),
    ];
    for (work, operation) in [
        (0, "scan F3Z component feature ordinals"),
        (1, "scan F3Z parent feature ordinals"),
        (3, "rewrite F3Z feature ordinals"),
    ] {
        let mut component = Model::default();
        component.features = vec![feature("f3d:test:feature#component", 10)];
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = work;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let error = append_feature_history(ctx, &parent, &mut component).unwrap_err();
            let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
                panic!("ordinal scan must refuse");
            };
            assert_eq!(limit.operation, operation);
            assert_eq!(Some(limit), ctx.resource_refusal());
            assert_eq!(component.features[0].ordinal, 10);
        });
    }
}

#[test]
fn occurrence_body_composition_preserves_work_refusal() {
    let mut model = Model::default();
    model.bodies.push(Body {
        id: BodyId::mint("f3d:brep:body#1").unwrap(),
        name: None,
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        visible: None,
        color: None,
    });
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let transform = crate::records::xref::XrefPlacementTransform::try_from([
            [1., 0., 0., 0.],
            [0., 1., 0., 0.],
            [0., 0., 1., 0.],
            [0., 0., 0., 1.],
        ])
        .unwrap();
        let error =
            super::super::apply_occurrence_transform(ctx, &mut model, transform).unwrap_err();
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("body composition must refuse");
        };
        assert_eq!(limit.operation, "compose F3Z body transforms");
        assert_eq!(Some(limit), ctx.resource_refusal());
        assert!(model.bodies[0].transform.is_none());
    });
}
