// SPDX-License-Identifier: Apache-2.0

fn native() -> crate::native::F3dNative {
    use crate::records::{
        feature::scope::{DesignFeatureKind, DesignParameterScope},
        topology::{
            construction::DesignConstructionOperandRole, extrude_selection::DesignOperandRole,
        },
    };
    let mut native = super::construction_group_limits::native(true, false);
    native.design_parameter_scopes.clear();
    native
        .design_parameter_scopes
        .push(DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#10",
            DesignFeatureKind::Fillet,
            10,
        ));
    native.design_construction_operand_groups[0].operand_role =
        DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_B);
    native
}

fn group_error(max_items: u64, max_retained: u64) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        policy.limits.max_retained_bytes = max_retained;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_fillet_operand_groups(
            &ctx,
            &mut Vec::new(),
            &std::collections::HashSet::new(),
        )
        .unwrap_err()
    })
}

#[test]
fn fillet_missing_radius_finding_refuses_collection_limit() {
    let error = group_error(0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn fillet_missing_radius_entity_refuses_retained_limit() {
    let error = group_error(u64::MAX, 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

fn full_round_native() -> crate::native::F3dNative {
    use crate::records::{
        recipes::ConstructionRecipeKind,
        topology::{
            body_recipe::DesignOperandGroup,
            construction::DesignConstructionOperandRole,
            extrude_selection::DesignOperandRole,
            face::{DesignFaceOperand, DesignFaceOperandDraft},
        },
    };
    let mut native = native();
    native.design_construction_operand_groups[0].operand_role =
        DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_A);
    native.design_face_operands.push(
        DesignFaceOperand::try_new(DesignFaceOperandDraft {
            id: "f3d:Design/BulkStream.dat:design-face-operand#101".into(),
            scope_record_index: 10,
            scope_reference_ordinal: 0,
            group: Some(DesignOperandGroup {
                group_record_index: 100,
                group_member_ordinal: 0,
            }),
            record_index: 101,
            byte_offset: 1_000,
            class_tag: "365".to_owned().try_into().unwrap(),
            paired_byte_offset: 1_016,
            paired_class_tag: "366".to_owned().try_into().unwrap(),
            recipe_record_index: 104,
            recipe_record_byte_offset: 1_032,
            recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#0".into(),
            recipe_prefix_offset: 1_043,
            recipe_prefix_bytes: Vec::new(),
            recipe_references: Vec::new(),
            recipe_kind: ConstructionRecipeKind::BoundedFace,
            recipe_program_offset: 1_063,
            recipe_program: vec![0, -1],
            recipe_nodes: Vec::new(),
            candidate_faces: Vec::new(),
            unreferenced_candidate_faces: Vec::new(),
            alternate_selector_candidate_faces: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            changed_candidate_faces: Vec::new(),
            historical_support_contexts: Vec::new(),
            resolved_face_slots: Vec::new(),
            resolved_active_face: None,
            next_record_index: 105,
            next_byte_offset: 1_071,
        })
        .unwrap(),
    );
    native
}

fn full_round_error(max_items: u64, max_retained: u64) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = full_round_native();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        policy.limits.max_retained_bytes = max_retained;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_fillet_operand_groups(
            &ctx,
            &mut Vec::new(),
            &std::collections::HashSet::new(),
        )
        .unwrap_err()
    })
}

#[test]
fn fillet_full_round_finding_refuses_collection_limit() {
    let error = full_round_error(0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn fillet_full_round_entity_refuses_retained_limit() {
    let error = full_round_error(u64::MAX, 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

fn add_radius_parameter(
    native: &mut crate::native::F3dNative,
    record_index: u32,
    source_kind: &str,
    value: f64,
    unit: Option<&str>,
) {
    use crate::records::{
        identity::RecordedValue,
        parameters::{
            DesignParameter, DesignParameterDraft, DesignParameterOwner, DesignParameterOwnerWire,
            DesignParameterSource,
        },
    };
    let owner_index = record_index - 1;
    let base = 10_000 + u64::from(record_index) * 100;
    let parameter = DesignParameter::try_from(DesignParameterDraft {
        id: format!("f3d:Design/BulkStream.dat:design-parameter#{record_index}"),
        byte_offset: base,
        class_tag: "305".to_owned().try_into().unwrap(),
        record_index,
        source_ordinal: 0,
        source: DesignParameterSource::new(source_kind.to_owned(), Some(owner_index), None)
            .unwrap(),
        expression: format!("{value}"),
        expression_offset: base + 12,
        source_kind_offset: base + 32,
        unit: unit.map(|unit| RecordedValue {
            value: unit.into(),
            offset: base + 52,
        }),
        name: source_kind.into(),
        name_offset: base + 62,
        evaluated_value: value,
        evaluated_value_offset: base + 72,
    })
    .unwrap();
    let owner = DesignParameterOwner::try_from(DesignParameterOwnerWire {
        id: format!("f3d:Design/BulkStream.dat:design-parameter-owner#{owner_index}"),
        byte_offset: base - 99,
        frame_length: 99,
        class_tag: "268".to_owned().try_into().unwrap(),
        record_index: owner_index,
        scope_record_index: 10,
        local_ordinal: 0,
        evaluated_value: value,
        evaluated_value_offset: base - 59,
        parameter_record_index: record_index,
        owned_ordinal: 0,
        variant: None,
        companion_record_index: record_index + 1,
    })
    .unwrap();
    native.design_parameters.push(parameter);
    native.design_parameter_owners.push(owner);
}

fn radius_native(valid: bool) -> crate::native::F3dNative {
    use crate::records::topology::fillet::{DesignFilletRadiusGroup, DesignFilletRadiusLaw};
    let mut native = native();
    if valid {
        add_radius_parameter(&mut native, 201, "Radius", 2.0, Some("mm"));
    }
    native
        .design_fillet_radius_groups
        .push(DesignFilletRadiusGroup {
            id: "f3d:Design/BulkStream.dat:design-fillet-radius-group#100".into(),
            scope_record_index: 10,
            group_ordinal: 0,
            group_record_index: 100,
            edge_operand_record_indices: vec![101],
            law: DesignFilletRadiusLaw::Constant {
                radius_parameter_record_index: 201,
            },
            tangency_weight_parameter_record_index: None,
        });
    native
}

fn radius_error(valid: bool, max_items: u64, max_retained: u64) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = radius_native(valid);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        policy.limits.max_retained_bytes = max_retained;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_fillet_radius_groups(&ctx, &mut Vec::new()).unwrap_err()
    })
}

#[test]
fn fillet_radius_record_index_refuses_collection_limit() {
    let error = radius_error(true, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D Fillet radius group records")
    );
}

#[test]
fn fillet_radius_slot_refuses_collection_limit() {
    let error = radius_error(true, 1, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D Fillet radius group slots")
    );
}

#[test]
fn fillet_radius_invalid_finding_refuses_collection_limit() {
    let error = radius_error(false, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn fillet_radius_invalid_entity_refuses_retained_limit() {
    let error = radius_error(false, u64::MAX, 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

#[test]
fn fillet_radius_valid_assignment_has_no_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = radius_native(true);
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        let mut findings = Vec::new();
        super::super::validate_fillet_radius_groups(&ctx, &mut findings).unwrap();
        assert!(findings.is_empty());
    })
}

fn variable_radius_native(increasing: bool) -> crate::native::F3dNative {
    use crate::records::topology::fillet::{DesignFilletMidpoint, DesignFilletRadiusLaw};
    let mut native = radius_native(true);
    for (index, kind, value, unit) in [
        (202, "StartRadius", 0.0, Some("mm")),
        (203, "EndRadius", 0.0, Some("mm")),
        (204, "MidRadius", 1.0, Some("mm")),
        (205, "MidParams", 0.25, None),
        (206, "MidRadius", 1.0, Some("mm")),
        (207, "MidParams", if increasing { 0.75 } else { 0.1 }, None),
    ] {
        add_radius_parameter(&mut native, index, kind, value, unit);
    }
    native.design_fillet_radius_groups[0].law = DesignFilletRadiusLaw::Variable {
        start_radius_parameter_record_index: 202,
        end_radius_parameter_record_index: 203,
        middle: vec![
            DesignFilletMidpoint {
                radius_parameter_record_index: 204,
                parameter_record_index: 205,
            },
            DesignFilletMidpoint {
                radius_parameter_record_index: 206,
                parameter_record_index: 207,
            },
        ],
    };
    native
}

#[test]
fn fillet_variable_radius_midpoints_keep_order_validation() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let valid_native = variable_radius_native(true);
        let valid_ctx = super::super::Ctx::new(&ir, &valid_native, service_ctx).unwrap();
        let mut valid_findings = Vec::new();
        super::super::validate_fillet_radius_groups(&valid_ctx, &mut valid_findings).unwrap();
        assert!(valid_findings.is_empty());

        let invalid_native = variable_radius_native(false);
        let invalid_ctx = super::super::Ctx::new(&ir, &invalid_native, service_ctx).unwrap();
        let mut invalid_findings = Vec::new();
        super::super::validate_fillet_radius_groups(&invalid_ctx, &mut invalid_findings).unwrap();
        assert!(invalid_findings.iter().any(|finding| finding.message
            == "Fusion Design Fillet radius group has an invalid parameter assignment"));
    })
}
