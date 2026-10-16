// SPDX-License-Identifier: Apache-2.0

use super::graph_ops::insert_brep_adjacency;
use super::{persistent_design_links, persistent_subentity_tags, Brep};
use crate::records::recipes::CreationTimestamp;
use crate::records::sketch_links::{PersistentDesignLink, PersistentSubentityTag, SketchCurveLink};
use cadmpeg_asm::brep::annotations::AnnotationRecord;
use cadmpeg_asm::brep::records::BodyNativeKey;
use cadmpeg_asm::brep::AsmBrep;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::ids::{FaceId, RegionId};
use cadmpeg_ir::topology::{Body, BodyKind, Region};
use std::collections::{HashMap, HashSet};

fn with_limits<T>(
    max_items: u64,
    max_retained: u64,
    run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    policy.limits.max_retained_bytes = max_retained;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    run(&ctx)
}

fn with_context<T>(run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T) -> T {
    with_limits(u64::MAX, u64::MAX, run)
}

fn with_materialized_limit<T>(
    max_materialized: u64,
    run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = max_materialized;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    run(&ctx)
}

#[test]
fn persistent_subentity_token_refuses_retained_limit() {
    let attribute = generic_tag_attribute(
        AttributeTarget::Face(FaceId::mint("f3d:test:face#1").unwrap()),
        (2, 2),
        1,
        vec![
            AttributeValue::Integer(7),
            AttributeValue::String("97".into()),
            AttributeValue::Integer(0),
            AttributeValue::Integer(1),
            AttributeValue::Integer(302),
        ],
    );
    let error = with_limits(u64::MAX, 1, |ctx| {
        persistent_subentity_tags(ctx, &attribute).unwrap_err()
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D persistent subentity token")
    );
}

#[test]
fn persistent_subentity_references_refuse_collection_limit() {
    let attribute = generic_tag_attribute(
        AttributeTarget::Face(FaceId::mint("f3d:test:face#1").unwrap()),
        (2, 2),
        1,
        vec![
            AttributeValue::Integer(7),
            AttributeValue::String("97".into()),
            AttributeValue::Integer(0),
            AttributeValue::Integer(1),
            AttributeValue::Integer(302),
        ],
    );
    let error = with_limits(0, u64::MAX, |ctx| {
        persistent_subentity_tags(ctx, &attribute).unwrap_err()
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D subentity references")
    );
}

#[test]
fn persistent_design_id_refuses_retained_limit() {
    let attribute = generic_tag_attribute(
        AttributeTarget::Body(BodyId::mint("f3d:test:body#1").unwrap()),
        (2, 2),
        1,
        vec![
            AttributeValue::Integer(3),
            AttributeValue::String("301".into()),
            AttributeValue::Integer(1),
            AttributeValue::Integer(0),
        ],
    );
    let error = with_limits(u64::MAX, 2, |ctx| {
        persistent_design_links(ctx, &attribute).unwrap_err()
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D persistent design ID")
    );
}

fn generic_tag_attribute(
    target: AttributeTarget,
    versions: (i64, i64),
    group_count: i64,
    groups: Vec<AttributeValue>,
) -> SourceAttribute {
    SourceAttribute {
        id: "f3d:brep:attribute#1".try_into().expect("valid identity"),
        target,
        name: "ATTRIB_CUSTOM-attrib".into(),
        values: [
            AttributeValue::String("generic_tag_attrib_def".into()),
            AttributeValue::Integer(versions.0),
            AttributeValue::Integer(versions.1),
            AttributeValue::Integer(-1),
            AttributeValue::String("generic_tag_attrib_def ".into()),
            AttributeValue::Integer(group_count),
        ]
        .into_iter()
        .chain(groups)
        .collect(),
    }
}

#[test]
fn generic_tag_payload_accepts_both_equal_envelope_versions() {
    for (version, groups) in [
        (
            2,
            vec![
                AttributeValue::Integer(7),
                AttributeValue::String("97".into()),
                AttributeValue::Integer(0),
                AttributeValue::Integer(1),
                AttributeValue::Integer(302),
            ],
        ),
        (
            3,
            vec![
                AttributeValue::Integer(7),
                AttributeValue::String("97".into()),
                AttributeValue::Integer(0),
                AttributeValue::Integer(1),
                AttributeValue::Integer(302),
                AttributeValue::Integer(0),
            ],
        ),
    ] {
        let attribute = generic_tag_attribute(
            AttributeTarget::Face(FaceId::mint("f3d:test:face#1").expect("identity grammar")),
            (version, version),
            1,
            groups,
        );
        assert_eq!(
            with_context(|ctx| persistent_subentity_tags(ctx, &attribute).unwrap()).len(),
            1
        );
        assert_eq!(
            with_context(|ctx| persistent_subentity_tags(ctx, &attribute).unwrap())[0]
                .design_references,
            [302]
        );
    }
}

#[test]
fn generic_tag_payload_rejects_mixed_or_unsupported_envelope_versions() {
    let groups = vec![
        AttributeValue::Integer(7),
        AttributeValue::String("97".into()),
        AttributeValue::Integer(0),
        AttributeValue::Integer(0),
        AttributeValue::Integer(0),
    ];
    for versions in [(2, 3), (1, 1), (4, 4)] {
        let attribute = generic_tag_attribute(
            AttributeTarget::Face(FaceId::mint("f3d:test:face#1").expect("identity grammar")),
            versions,
            1,
            groups.clone(),
        );
        assert!(with_context(|ctx| persistent_subentity_tags(ctx, &attribute).unwrap()).is_empty());
    }
}

#[test]
fn generic_tag_payload_binds_modern_body_design_links() {
    let attribute = generic_tag_attribute(
        AttributeTarget::Body(BodyId::mint("f3d:test:body#1").expect("identity grammar")),
        (2, 2),
        1,
        vec![
            AttributeValue::Integer(3),
            AttributeValue::String("301".into()),
            AttributeValue::Integer(1),
            AttributeValue::Integer(0),
        ],
    );
    let links = with_context(|ctx| persistent_design_links(ctx, &attribute).unwrap());
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].design_id.as_str(), "301");
    assert_eq!(links[0].design_reference, 1);
}

#[test]
fn generic_tag_payload_binds_legacy_body_design_links() {
    let attribute = generic_tag_attribute(
        AttributeTarget::Body(BodyId::mint("f3d:test:body#1").expect("identity grammar")),
        (3, 3),
        1,
        vec![
            AttributeValue::Integer(3),
            AttributeValue::String("301".into()),
            AttributeValue::Integer(1),
            AttributeValue::Integer(0),
            AttributeValue::Integer(0),
        ],
    );
    assert_eq!(
        with_context(|ctx| persistent_design_links(ctx, &attribute).unwrap()).len(),
        1
    );
}

#[test]
fn brep_qualification_rewrites_owned_ids_and_cross_references() {
    let body = BodyId::mint("f3d:brep:entity#1").expect("identity grammar");
    let region = RegionId::mint("f3d:brep:entity#2").expect("identity grammar");
    let mut brep = Brep {
        asm: AsmBrep {
            bodies: vec![Body {
                id: body.clone(),
                kind: BodyKind::default(),
                regions: vec![region.clone()],
                transform: None,
                name: None,
                color: None,
                visible: None,
            }],
            regions: vec![Region {
                id: region,
                body: body.clone(),
                shells: Vec::new(),
            }],

            body_native_keys: vec![BodyNativeKey {
                source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                    crate::ids::ID_FORMAT,
                ),
                body,
                record_index: 1,
                body_ordinal: 0,
                source_brep: Some("BREP.source.smbh".into()),
                asm_body_key: Some(7),
            }],
            annotation_records: vec![AnnotationRecord {
                id: "f3d:brep:entity#1".into(),
                stream: "asset/BREP.source.smbh".into(),
                offset: 10,
                tag: cadmpeg_asm::brep::annotations::AnnotationTag::Record("body".into()),
                derived_fields: Vec::new(),
            }],
            ..AsmBrep::default()
        },
        ..Brep::default()
    };

    with_context(|ctx| brep.qualify_ids(ctx, crate::ids::ID_FORMAT, "source"))
        .expect("qualify BREP");

    let qualified = BodyId::mint("f3d:brep/source/brep:entity#1").expect("identity grammar");
    assert_eq!(brep.asm.bodies[0].id, qualified);
    assert_eq!(brep.asm.regions[0].body, qualified);
    assert_eq!(brep.asm.body_native_keys[0].body, qualified);
    assert_eq!(
        brep.asm
            .body_native_keys
            .iter()
            .find(|record| record.body == qualified)
            .and_then(|record| record.asm_body_key.as_ref()),
        Some(&7)
    );
    assert_eq!(brep.asm.annotation_records[0].id, qualified.as_str());
    assert_eq!(
        brep.asm.body_native_keys[0].source_brep.as_deref(),
        Some("BREP.source.smbh")
    );
}

fn one_body_brep() -> Brep {
    Brep {
        asm: AsmBrep {
            bodies: vec![Body {
                id: BodyId::mint("f3d:brep:entity#1").unwrap(),
                kind: BodyKind::default(),
                regions: Vec::new(),
                transform: None,
                name: None,
                color: None,
                visible: None,
            }],
            ..AsmBrep::default()
        },
        ..Brep::default()
    }
}

#[test]
fn brep_owned_id_copy_refuses_retained_limit() {
    let mut brep = one_body_brep();
    let error = with_limits(u64::MAX, 0, |ctx| {
        brep.qualify_ids(ctx, crate::ids::ID_FORMAT, "source")
            .unwrap_err()
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn brep_remapped_id_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy F3D BREP remapped ID",
        |cap| {
            let mut brep = one_body_brep();
            Err::<(), cadmpeg_core::CodecError>(with_limits(u64::MAX, cap, |ctx| {
                brep.qualify_ids(ctx, crate::ids::ID_FORMAT, "source")
                    .unwrap_err()
            }))
        },
    );
    // The boundary oracle reaches the remapped identity after preceding map storage.
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn brep_qualification_projection_refuses_materialized_limit() {
    let mut brep = one_body_brep();
    let error = with_materialized_limit(0, |ctx| {
        brep.qualify_ids(ctx, crate::ids::ID_FORMAT, "source")
            .unwrap_err()
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn brep_adjacency_index_refuses_collection_limit() {
    let error = with_limits(0, u64::MAX, |ctx| {
        insert_brep_adjacency(ctx, &mut HashMap::new(), "source", "target").unwrap_err()
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D BREP adjacency")
    );
}

#[test]
fn brep_adjacent_ids_refuse_collection_limit() {
    let error = with_limits(0, u64::MAX, |ctx| {
        let mut adjacency = HashMap::from([("source".to_owned(), HashSet::new())]);
        insert_brep_adjacency(ctx, &mut adjacency, "source", "target").unwrap_err()
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D BREP adjacent IDs")
    );
}

#[test]
fn brep_reachable_id_set_refuses_collection_limit() {
    let error = with_limits(0, u64::MAX, |ctx| {
        ctx.insert_hash_set(
            &mut HashSet::new(),
            "f3d:brep:entity#1".to_owned(),
            "collect F3D reachable BREP IDs",
        )
        .map(|_| ())
        .unwrap_err()
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D reachable BREP IDs")
    );
}

#[test]
fn brep_retained_sketch_links_refuse_collection_limit() {
    let mut brep = Brep {
        sketch_curve_links: vec![SketchCurveLink {
            id: "f3d:design:sketch-curve-link#1".into(),
            target: AttributeTarget::Document,
            sketch_curve_id: 1,
            ref_b: 0,
            sense: None,
            role: 0,
            closure: 0,
        }],
        ..Brep::default()
    };
    let error = with_limits(0, u64::MAX, |ctx| {
        brep.retain_body_keys(ctx, &HashSet::new()).unwrap_err()
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D retained sketch links")
    );
}

#[test]
fn brep_retained_design_links_refuse_collection_limit() {
    let mut brep = Brep {
        persistent_design_links: vec![PersistentDesignLink {
            id: "f3d:design:persistent-design-link#1".into(),
            target: AttributeTarget::Document,
            design_id: "301".to_owned().try_into().unwrap(),
            design_reference: 1,
            ordinal: 0,
        }],
        ..Brep::default()
    };
    let error = with_limits(0, u64::MAX, |ctx| {
        brep.retain_body_keys(ctx, &HashSet::new()).unwrap_err()
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D retained design links")
    );
}

#[test]
fn brep_retained_subentity_tags_refuse_collection_limit() {
    let mut brep = Brep {
        persistent_subentity_tags: vec![PersistentSubentityTag {
            id: "f3d:design:persistent-subentity-tag#1".into(),
            target: AttributeTarget::Document,
            selector: 1,
            token: cadmpeg_core::text::NonBlankString::new("97").unwrap(),
            design_references: vec![1],
            ordinal: 0,
        }],
        ..Brep::default()
    };
    let error = with_limits(0, u64::MAX, |ctx| {
        brep.retain_body_keys(ctx, &HashSet::new()).unwrap_err()
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D retained subentity tags")
    );
}

#[test]
fn brep_retained_timestamps_refuse_collection_limit() {
    let mut brep = Brep {
        creation_timestamps: vec![CreationTimestamp {
            id: "f3d:design:creation-timestamp#1".into(),
            target: AttributeTarget::Document,
            record_index: 1,
            unix_microseconds: cadmpeg_ir::scalar::FiniteReal::new(1.0).unwrap(),
        }],
        ..Brep::default()
    };
    let error = with_limits(0, u64::MAX, |ctx| {
        brep.retain_body_keys(ctx, &HashSet::new()).unwrap_err()
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D retained timestamps")
    );
}

#[test]
fn body_key_retention_keeps_only_the_selected_connected_graph() {
    let body = |index, region| Body {
        id: BodyId::mint(format!("f3d:brep:entity#{index}")).expect("identity grammar"),
        kind: BodyKind::default(),
        regions: vec![
            RegionId::mint(format!("f3d:brep:entity#{region}")).expect("identity grammar")
        ],
        transform: None,
        name: None,
        color: None,
        visible: None,
    };
    let native_key = |index, key| BodyNativeKey {
        source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
            crate::ids::ID_FORMAT,
        ),
        body: BodyId::mint(format!("f3d:brep:entity#{index}")).expect("identity grammar"),
        record_index: index,
        body_ordinal: index - 1,
        source_brep: Some("BREP.source.smbh".into()),
        asm_body_key: Some(key),
    };
    let mut brep = Brep {
        asm: AsmBrep {
            bodies: vec![
                Body {
                    name: Some("f3d:brep:entity#3".into()),
                    ..body(1, 2)
                },
                body(3, 4),
            ],
            regions: vec![
                Region {
                    id: RegionId::mint("f3d:brep:entity#2").expect("identity grammar"),
                    body: BodyId::mint("f3d:brep:entity#1").expect("identity grammar"),
                    shells: Vec::new(),
                },
                Region {
                    id: RegionId::mint("f3d:brep:entity#4").expect("identity grammar"),
                    body: BodyId::mint("f3d:brep:entity#3").expect("identity grammar"),
                    shells: Vec::new(),
                },
            ],

            body_native_keys: vec![native_key(1, 10), native_key(3, 20)],
            ..AsmBrep::default()
        },
        ..Brep::default()
    };

    with_context(|ctx| brep.retain_body_keys(ctx, &HashSet::from([20])))
        .expect("retain body graph");

    assert_eq!(brep.asm.bodies.len(), 1);
    assert_eq!(brep.asm.bodies[0].id.as_str(), "f3d:brep:entity#3");
    assert_eq!(brep.asm.regions.len(), 1);
    assert_eq!(brep.asm.regions[0].id.as_str(), "f3d:brep:entity#4");
    assert_eq!(brep.asm.body_native_keys.len(), 1);
    assert_eq!(
        brep.asm
            .body_native_keys
            .iter()
            .filter(|record| record.asm_body_key.is_some())
            .count(),
        1
    );
}

#[test]
fn body_key_retention_preserves_derived_links_for_reachable_targets() {
    let body = |index| Body {
        id: BodyId::mint(format!("f3d:brep:entity#{index}")).expect("identity grammar"),
        kind: BodyKind::default(),
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    };
    let native_key = |index, key| BodyNativeKey {
        source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
            crate::ids::ID_FORMAT,
        ),
        body: BodyId::mint(format!("f3d:brep:entity#{index}")).expect("identity grammar"),
        record_index: index,
        body_ordinal: index - 1,
        source_brep: Some("BREP.source.smbh".into()),
        asm_body_key: Some(key),
    };
    let target = |index| {
        AttributeTarget::Body(
            BodyId::mint(format!("f3d:brep:entity#{index}")).expect("identity grammar"),
        )
    };
    let mut brep = Brep {
        asm: AsmBrep {
            bodies: vec![body(1), body(3)],

            body_native_keys: vec![native_key(1, 10), native_key(3, 20)],
            ..AsmBrep::default()
        },
        sketch_curve_links: vec![
            SketchCurveLink {
                id: "link-retained".into(),
                target: target(1),
                sketch_curve_id: 1,
                ref_b: 0,
                sense: None,
                role: 0,
                closure: 0,
            },
            SketchCurveLink {
                id: "link-dropped".into(),
                target: target(3),
                sketch_curve_id: 3,
                ref_b: 0,
                sense: None,
                role: 0,
                closure: 0,
            },
        ],
        persistent_design_links: vec![
            PersistentDesignLink {
                id: "design-retained".into(),
                target: target(1),
                design_id: "301".to_owned().try_into().unwrap(),

                design_reference: 1,
                ordinal: 0,
            },
            PersistentDesignLink {
                id: "design-dropped".into(),
                target: target(3),
                design_id: "303".to_owned().try_into().unwrap(),

                design_reference: 3,
                ordinal: 0,
            },
        ],
        persistent_subentity_tags: vec![
            PersistentSubentityTag {
                id: "tag-retained".into(),
                target: target(1),
                selector: 1,
                token: cadmpeg_core::text::NonBlankString::new("97").unwrap(),
                design_references: vec![1],
                ordinal: 0,
            },
            PersistentSubentityTag {
                id: "tag-dropped".into(),
                target: target(3),
                selector: 1,
                token: cadmpeg_core::text::NonBlankString::new("97").unwrap(),
                design_references: vec![3],
                ordinal: 0,
            },
        ],
        creation_timestamps: vec![
            CreationTimestamp {
                id: "time-retained".into(),
                target: target(1),
                record_index: 1,
                unix_microseconds: cadmpeg_ir::scalar::FiniteReal::new(1.0).unwrap(),
            },
            CreationTimestamp {
                id: "time-dropped".into(),
                target: target(3),
                record_index: 3,
                unix_microseconds: cadmpeg_ir::scalar::FiniteReal::new(3.0).unwrap(),
            },
        ],
    };

    with_context(|ctx| brep.retain_body_keys(ctx, &HashSet::from([10])))
        .expect("retain body graph");

    assert_eq!(brep.sketch_curve_links.len(), 1);
    assert_eq!(brep.persistent_design_links.len(), 1);
    assert_eq!(brep.persistent_subentity_tags.len(), 1);
    assert_eq!(brep.creation_timestamps.len(), 1);
    assert_eq!(brep.persistent_subentity_tags[0].id, "tag-retained");
}

#[test]
fn body_key_retention_preserves_selectorless_neutral_roots() {
    let native_body = BodyId::mint("f3d:brep:entity#1").expect("identity grammar");
    let projected_body = BodyId::mint("f3d:brep:saved-edge-body#5").expect("identity grammar");
    let mut brep = Brep {
        asm: AsmBrep {
            bodies: vec![
                Body {
                    id: native_body.clone(),
                    kind: BodyKind::Solid,
                    regions: Vec::new(),
                    transform: None,
                    name: None,
                    color: None,
                    visible: None,
                },
                Body {
                    id: projected_body.clone(),
                    kind: BodyKind::Wire,
                    regions: Vec::new(),
                    transform: None,
                    name: None,
                    color: None,
                    visible: None,
                },
            ],

            body_native_keys: vec![BodyNativeKey {
                source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                    crate::ids::ID_FORMAT,
                ),
                body: native_body,
                record_index: 1,
                body_ordinal: 0,
                source_brep: Some("BREP.source.smbh".into()),
                asm_body_key: Some(10),
            }],
            ..AsmBrep::default()
        },
        ..Brep::default()
    };

    with_context(|ctx| brep.retain_body_keys(ctx, &HashSet::from([10])))
        .expect("retain body graph");

    assert_eq!(brep.asm.bodies.len(), 2);
    assert!(brep.asm.bodies.iter().any(|body| body.id == projected_body));
}

#[test]
fn body_selectors_use_ordinals_only_for_an_all_null_key_lane() {
    let native_key = |ordinal, key| BodyNativeKey {
        source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
            crate::ids::ID_FORMAT,
        ),
        body: BodyId::mint(format!("f3d:brep:entity#{ordinal}")).expect("identity grammar"),
        record_index: ordinal,
        body_ordinal: ordinal,
        source_brep: Some("BREP.source.smb".into()),
        asm_body_key: key,
    };
    let mut brep = Brep {
        asm: AsmBrep {
            body_native_keys: vec![native_key(0, None), native_key(1, None)],
            ..AsmBrep::default()
        },
        ..Brep::default()
    };

    assert_eq!(
        with_context(|ctx| brep.body_selectors(ctx).unwrap()).len(),
        2
    );
    assert_eq!(
        with_context(|ctx| brep.body_selectors(ctx).unwrap())
            [&BodyId::mint("f3d:brep:entity#1").expect("identity grammar")],
        1
    );

    brep.asm.body_native_keys[1].asm_body_key = Some(7);
    assert_eq!(
        with_context(|ctx| brep.body_selectors(ctx).unwrap()),
        HashMap::from([(
            BodyId::mint("f3d:brep:entity#1").expect("identity grammar"),
            7
        )])
    );
}

#[test]
fn body_selector_id_copy_refuses_retained_limit() {
    let brep = Brep {
        asm: AsmBrep {
            body_native_keys: vec![BodyNativeKey {
                source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                    crate::ids::ID_FORMAT,
                ),
                body: BodyId::mint("f3d:brep:entity#1").unwrap(),
                record_index: 1,
                body_ordinal: 0,
                source_brep: None,
                asm_body_key: Some(7),
            }],
            ..AsmBrep::default()
        },
        ..Brep::default()
    };
    let error = with_limits(u64::MAX, 0, |ctx| brep.body_selectors(ctx).unwrap_err());
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D BREP body ID")
    );
}

#[test]
fn body_selector_index_refuses_collection_limit() {
    let brep = Brep {
        asm: AsmBrep {
            body_native_keys: vec![BodyNativeKey {
                source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                    crate::ids::ID_FORMAT,
                ),
                body: BodyId::mint("f3d:brep:entity#1").unwrap(),
                record_index: 1,
                body_ordinal: 0,
                source_brep: None,
                asm_body_key: Some(7),
            }],
            ..AsmBrep::default()
        },
        ..Brep::default()
    };
    let error = with_limits(0, u64::MAX, |ctx| brep.body_selectors(ctx).unwrap_err());
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D BREP body selectors")
    );
}

#[test]
fn design_body_selectors_prefer_exact_keys_then_fall_back_to_ordinals() {
    let native_key = |ordinal, key| BodyNativeKey {
        source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
            crate::ids::ID_FORMAT,
        ),
        body: BodyId::mint(format!("f3d:brep:entity#{ordinal}")).expect("identity grammar"),
        record_index: ordinal,
        body_ordinal: ordinal,
        source_brep: Some("BREP.source.smb".into()),
        asm_body_key: Some(key),
    };
    let mut brep = Brep {
        asm: AsmBrep {
            body_native_keys: vec![native_key(0, 1), native_key(1, 0)],
            ..AsmBrep::default()
        },
        ..Brep::default()
    };

    assert_eq!(
        with_context(|ctx| brep.body_selectors_for(ctx, &HashSet::from([0])).unwrap()),
        HashMap::from([(
            BodyId::mint("f3d:brep:entity#1").expect("identity grammar"),
            0
        )])
    );

    brep.asm.body_native_keys = vec![native_key(0, 436)];
    assert_eq!(
        with_context(|ctx| brep.body_selectors_for(ctx, &HashSet::from([0])).unwrap()),
        HashMap::from([(
            BodyId::mint("f3d:brep:entity#0").expect("identity grammar"),
            0
        )])
    );
}

#[test]
fn selected_body_index_refuses_collection_limit() {
    let brep = Brep {
        asm: AsmBrep {
            body_native_keys: vec![BodyNativeKey {
                source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                    crate::ids::ID_FORMAT,
                ),
                body: BodyId::mint("f3d:brep:entity#1").unwrap(),
                record_index: 1,
                body_ordinal: 0,
                source_brep: None,
                asm_body_key: Some(7),
            }],
            ..AsmBrep::default()
        },
        ..Brep::default()
    };
    let error = with_limits(0, u64::MAX, |ctx| {
        brep.body_selectors_for(ctx, &HashSet::from([7]))
            .unwrap_err()
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D selected BREP bodies")
    );
}

#[test]
fn brep_append_refuses_body_collection_limit() {
    let mut target = Brep::default();
    let source = Brep {
        asm: AsmBrep {
            bodies: vec![Body {
                id: BodyId::mint("f3d:brep:entity#1").unwrap(),
                kind: BodyKind::default(),
                regions: Vec::new(),
                transform: None,
                name: None,
                color: None,
                visible: None,
            }],
            ..AsmBrep::default()
        },
        ..Brep::default()
    };
    let error = with_limits(0, u64::MAX, |ctx| target.append(ctx, source).unwrap_err());
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "merge F3D BREP bodies")
    );
}

#[test]
fn brep_append_refuses_statistic_index_limit() {
    let mut target = Brep::default();
    let mut source = Brep::default();
    source
        .asm
        .stats
        .missing_face_surface_kinds
        .insert("plane".into(), 1);
    let error = with_limits(0, u64::MAX, |ctx| target.append(ctx, source).unwrap_err());
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "merge F3D BREP statistic kinds")
    );
}

mod work;

#[test]
fn fusion_attribute_family_scan_preserves_work_refusal() {
    let attribute = SourceAttribute {
        id: "f3d:brep:attribute#1".try_into().unwrap(),
        target: AttributeTarget::Document,
        name: "foreign".into(),
        values: vec![AttributeValue::String("foreign payload".repeat(32))],
    };
    for family in [
        "sketch_attrib_def",
        "generic_tag_attrib_def",
        "Timestamp_attrib_def",
    ] {
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = 1;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let error = super::attribute_family(ctx, &attribute, family).unwrap_err();
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "compare Fusion attribute family"
                    && ctx.resource_refusal() == Some(limit))
            );
        });
    }
    let mut asm = AsmBrep::default();
    asm.attributes.push(attribute);
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let error = super::Brep::from_asm(ctx, asm).err().expect("scan refusal");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "scan Fusion attribute family" && ctx.resource_refusal() == Some(limit))
        );
    });
}
