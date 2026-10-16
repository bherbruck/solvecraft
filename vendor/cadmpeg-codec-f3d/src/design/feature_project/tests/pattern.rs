// SPDX-License-Identifier: Apache-2.0

use crate::design::feature_project::{
    project_circular_pattern, project_rectangular_pattern_scalars,
};
use crate::records::{
    feature::{patterns::DesignRectangularPatternConstruction, scope::DesignParameterScope},
    topology::{
        construction::DesignConstructionOperandGroup,
        construction::DesignConstructionOperandGroupFrame, extrude_selection::DesignOperandRole,
    },
};
use cadmpeg_ir::features::{
    patterns::{PatternSeed, PatternTransform},
    BodySelection, FaceSelection, FeatureDefinition, FeatureOperation,
};

const EPS_SPACING: f64 = 1.0e-12;

fn group(
    scope_record_index: u32,
    record_index: u32,
    role: DesignOperandRole,
) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: format!(
                "f3d:Design/BulkStream.dat:design-construction-operand-group#{record_index}"
            ),
            scope_record_index,
            scope_reference_ordinal: 1,
            record_index,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("313".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: record_index + 1,
                offset: 0,
            }],
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
                    opaque_index: 99,
                    opaque_index_offset: 18,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 22,
                    variant: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(role),
            role_offset: 0,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "263".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap()
}

fn rectangular_scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::RPattern,
        10,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::RPattern(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::RectangularPattern(slot) =
        scope.payload_mut()
    {
        *slot = Some(
            DesignRectangularPatternConstruction::try_from(
                crate::records::feature::patterns::DesignRectangularPatternConstructionWire {
                    u_count: 3,
                    v_count: 1,
                    u_extent: 10.0,
                    v_extent: 0.0,
                    owner_record_indices: [11, 12, 13, 14],
                    value_offsets: [101, 102, 103, 104],
                    instances: None,
                },
            )
            .unwrap(),
        );
    }
    scope
}

fn circular_scope() -> DesignParameterScope {
    use crate::records::feature::patterns::{
        DesignCircularPatternAxis, DesignCircularPatternConstruction,
    };
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::CPattern,
        10,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::CPattern(slot) =
        scope.payload_mut()
    {
        *slot = Some(DesignCircularPatternConstruction {
            count: std::num::NonZeroU32::new(3).unwrap(),
            count_record_index: 11,
            count_offset: 0,
            angle: cadmpeg_ir::scalar::PositiveAngle::new(std::f64::consts::TAU).unwrap(),
            angle_record_index: 12,
            angle_offset: 0,
            axis: DesignCircularPatternAxis::Inline {
                origin: crate::test_support::reals([0.0, 0.0, 0.0]),
                origin_offset: 0,
                direction: cadmpeg_ir::units::UnitVector3::normalized(
                    cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
                direction_offset: 0,
            },
            axis_record_index: 13,
            selection_record_index: 14,
        });
    }
    scope
}

#[test]
fn circular_pattern_seed_role_selects_body_or_face() {
    for (role, expected_seed) in [
        (
            DesignOperandRole::BODIES_B,
            PatternSeed::Bodies(BodySelection::Native(
                "f3d:Design/BulkStream.dat:design-construction-operand-group#20".into(),
            )),
        ),
        (
            DesignOperandRole::BODIES_A,
            PatternSeed::Faces(FaceSelection::Native(
                "f3d:Design/BulkStream.dat:design-construction-operand-group#20".into(),
            )),
        ),
    ] {
        let definition = crate::test_support::with_decode_context(|decode_ctx| {
            project_circular_pattern(decode_ctx, &circular_scope(), &[group(10, 20, role)], &[])
        })
        .unwrap()
        .expect("circular pattern");
        let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) = definition
        else {
            panic!("circular pattern definition");
        };
        assert_eq!(seeds, vec![expected_seed]);
        assert!(matches!(
            pattern.definition(),
            PatternTransform::Circular { .. }
        ));
    }
}

fn assert_circular_seed_refusal(role: DesignOperandRole, operation: &'static str) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let scope = circular_scope();
    let seed_group = group(10, 20, role);
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            operation,
            |cap| {
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = cap;
                let arena = DecodeArena::new();

                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                (project_circular_pattern(&ctx, &scope, std::slice::from_ref(&seed_group), &[]))
                    .map(|_| ())
            },
        );
        assert!(
            matches!(Err::<(), cadmpeg_core::CodecError>(error), Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::RetainedBytes
                    && failure.operation == operation)
        );
    }
}

#[test]
fn circular_body_seed_id_refuses_retained_limit() {
    assert_circular_seed_refusal(DesignOperandRole::BODIES_B, "f3d circular body seed id");
}

#[test]
fn circular_face_seed_id_refuses_retained_limit() {
    assert_circular_seed_refusal(DesignOperandRole::BODIES_A, "f3d circular face seed id");
}

fn assert_linear_seed(definition: FeatureDefinition, expected_seed: PatternSeed) {
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) = definition
    else {
        panic!("rectangular pattern definition");
    };
    assert_eq!(seeds, vec![expected_seed]);
    let PatternTransform::Linear {
        direction,
        spacing,
        count,
        second,
    } = pattern.definition().clone()
    else {
        panic!("linear rectangular pattern");
    };
    assert!(direction.is_none());
    assert!((spacing.get() - 50.0).abs() < EPS_SPACING);
    assert_eq!(count, 3);
    assert!(second.is_none());
}

#[test]
fn rectangular_pattern_seed_role_selects_body_or_face() {
    let body_scope = rectangular_scope();
    let body_group = group(10, 20, DesignOperandRole::BODIES_B);
    let body_definition = crate::test_support::with_decode_context(|decode_ctx| {
        project_rectangular_pattern_scalars(decode_ctx, &body_scope, &[body_group], &[])
    })
    .unwrap()
    .expect("body rectangular pattern");
    assert_linear_seed(
        body_definition,
        PatternSeed::Bodies(BodySelection::Native(
            "f3d:Design/BulkStream.dat:design-construction-operand-group#20".into(),
        )),
    );

    let face_scope = rectangular_scope();
    let face_group = group(10, 30, DesignOperandRole::BODIES_A);
    let face_definition = crate::test_support::with_decode_context(|decode_ctx| {
        project_rectangular_pattern_scalars(decode_ctx, &face_scope, &[face_group], &[])
    })
    .unwrap()
    .expect("face rectangular pattern");
    assert_linear_seed(
        face_definition,
        PatternSeed::Faces(FaceSelection::Native(
            "f3d:Design/BulkStream.dat:design-construction-operand-group#30".into(),
        )),
    );
}

fn assert_rectangular_seed_refusal(role: DesignOperandRole, operation: &'static str) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let scope = rectangular_scope();
    let seed_group = group(10, 20, role);
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            operation,
            |cap| {
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = cap;
                let arena = DecodeArena::new();

                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                (project_rectangular_pattern_scalars(
                    &ctx,
                    &scope,
                    std::slice::from_ref(&seed_group),
                    &[],
                ))
                .map(|_| ())
            },
        );
        assert!(
            matches!(Err::<(), cadmpeg_core::CodecError>(error), Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::RetainedBytes
                    && failure.operation == operation)
        );
    }
}

#[test]
fn rectangular_face_seed_id_refuses_retained_limit() {
    assert_rectangular_seed_refusal(DesignOperandRole::BODIES_A, "f3d rectangular face seed id");
}

#[test]
fn rectangular_body_seed_id_refuses_retained_limit() {
    assert_rectangular_seed_refusal(DesignOperandRole::BODIES_B, "f3d rectangular body seed id");
}

#[test]
fn rectangular_pattern_seed_output_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let scope = rectangular_scope();
    let seed_group = group(10, 20, DesignOperandRole::BODIES_B);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(project_rectangular_pattern_scalars(&ctx, &scope,
        std::slice::from_ref(&seed_group), &[]), Err(CodecError::ResourceLimit(failure))
        if failure.dimension == ResourceDimension::CollectionItems && failure.operation == "f3d rectangular pattern seeds"));
}
