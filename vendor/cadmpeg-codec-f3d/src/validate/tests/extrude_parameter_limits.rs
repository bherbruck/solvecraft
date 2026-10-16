// SPDX-License-Identifier: Apache-2.0

fn extrude_error(
    max_items: u64,
    max_retained_bytes: u64,
    operation: crate::records::feature::extrude::DesignExtrudeOperation,
) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use crate::records::{
            feature::{
                extrude::{DesignExtrudeExtent, DesignExtrudePrologue, DesignExtrudeStart},
                scope::{DesignExtrudeScope, DesignScopePayload},
            },
            topology::{
                construction::DesignConstructionOperandRole,
                extrude_selection::{DesignExtrudeFaceEncoding, DesignExtrudeFaceRole},
            },
        };
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = super::construction_group_limits::native(true, false);
        native.design_parameter_scopes[0]
            .try_edit(|draft| {
                draft.payload = DesignScopePayload::Extrude(Some(DesignExtrudeScope {
                    extrude_prologue: Some(DesignExtrudePrologue::ReferenceAware {
                        reference: None,
                        operation,
                        operation_offset: 25,
                        direction_face_extend_values: [1, 2],
                        side_extent_discriminators: [1, 0],
                        side_extent_discriminator_offsets: [77, 90],
                        first_side_target_ordinal: None,
                        extent: DesignExtrudeExtent::OneSidedDistance,
                        direction_face_extend_offsets: [32, 36],
                        direction_reversed: false,
                        direction_reversed_offset: 40,
                        solid_operation: true,
                        solid_operation_offset: 41,
                        start: DesignExtrudeStart::ProfilePlane,
                        start_offset: 42,
                    }),
                    ..DesignExtrudeScope::default()
                }));
            })
            .unwrap();
        native.design_construction_operand_groups[0].operand_role =
            DesignConstructionOperandRole::ExtrudeFaces {
                encoding: DesignExtrudeFaceEncoding::Faces,
                usage: DesignExtrudeFaceRole::Termination,
            };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        policy.limits.max_retained_bytes = max_retained_bytes;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_extrude_parameter_operands(&ctx, &mut Vec::new()).unwrap_err()
    })
}

#[test]
fn extrude_face_groups_refuse_collection_limit() {
    use crate::records::feature::extrude::DesignExtrudeOperation;
    let error = extrude_error(0, u64::MAX, DesignExtrudeOperation::NewBody);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D Extrude face operand groups")
    );
}

#[test]
fn extrude_invalid_operation_finding_refuses_collection_limit() {
    use crate::records::feature::extrude::DesignExtrudeOperation;
    let error = extrude_error(0, u64::MAX, DesignExtrudeOperation::Join);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn extrude_invalid_operation_entity_refuses_retained_limit() {
    use crate::records::feature::extrude::DesignExtrudeOperation;
    let error = extrude_error(u64::MAX, 0, DesignExtrudeOperation::Join);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}
