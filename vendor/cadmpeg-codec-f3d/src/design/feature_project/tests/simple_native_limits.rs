// SPDX-License-Identifier: Apache-2.0
use super::boundary_fill_limits::group;
use crate::design::feature_project::{project_move, project_remove_body, project_surface_stitch};
use crate::records::feature::direct_face::{DesignMoveForm, DesignMoveOperation};
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayload, DesignScopePayloadMut,
};
use crate::records::feature::surface_ops::DesignSurfaceStitchOperation;
use crate::records::identity::ReferenceRun;
use crate::records::sketch_placement::SketchPlacementMatrix;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn context(limit: u64, dimension: ResourceDimension) -> (DecodeArena, DecodePolicy) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    match dimension {
        ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
        ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
        _ => panic!("unsupported dimension"),
    }
    (arena, policy)
}

#[test]
fn move_body_group_id_refuses_retained_limit() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#100",
        DesignFeatureKind::Move,
        100,
    );
    if let DesignScopePayloadMut::Move(slot) = scope.payload_mut() {
        *slot = Some(DesignMoveOperation {
            transform: SketchPlacementMatrix::IDENTITY,
            transform_offset: 0,
            transform_record_index: 200,
            form: DesignMoveForm::Form1,
            form_offset: 0,
        });
    }
    let body_group = group(100, 0, &[200], DesignOperandRole::BODIES_A);
    let (arena, policy) = context(0, ResourceDimension::RetainedBytes);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(project_move(&ctx, &scope, std::slice::from_ref(&body_group)),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::RetainedBytes
                && failure.operation == "f3d Move body group id")
    );
}

#[test]
fn remove_body_group_id_refuses_retained_limit() {
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#100",
        DesignFeatureKind::RemoveBody,
        100,
    );
    let body_group = group(100, 0, &[200], DesignOperandRole::BODIES_A);
    let (arena, policy) = context(0, ResourceDimension::RetainedBytes);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(project_remove_body(&ctx, &scope, std::slice::from_ref(&body_group)),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::RetainedBytes
                && failure.operation == "f3d RemoveBody group id")
    );
}

fn stitch_fixture() -> (
    DesignParameterScope,
    crate::records::topology::construction::DesignConstructionOperandGroup,
) {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#100",
        DesignFeatureKind::RemoveBody,
        100,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = ReferenceRun::unlocated(vec![100, 200, 300, 301]);
            draft.payload = DesignScopePayload::SurfaceStitch(DesignSurfaceStitchOperation {
                gap_tolerance: cadmpeg_ir::scalar::PositiveReal::new(0.01).unwrap(),
                gap_tolerance_offset: 40,
                tolerance_record_index: 300,
                settings_record_index: 301,
            });
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    (scope, group(100, 0, &[200], DesignOperandRole::ROLE_0X5))
}

#[test]
fn surface_stitch_group_refuses_collection_limit() {
    let (scope, stitch_group) = stitch_fixture();
    let (arena, policy) = context(0, ResourceDimension::CollectionItems);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(project_surface_stitch(&ctx, &scope,
        std::slice::from_ref(&stitch_group)),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d SurfaceStitch group"));
}

#[test]
fn surface_stitch_native_id_refuses_retained_limit() {
    let (scope, stitch_group) = stitch_fixture();
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::RetainedBytes,
        "f3d SurfaceStitch native id",
        0,
        |ctx| project_surface_stitch(ctx, &scope, std::slice::from_ref(&stitch_group)),
    );
    assert!(matches!(error, CodecError::ResourceLimit(failure)
        if failure.dimension == ResourceDimension::RetainedBytes
            && failure.operation == "f3d SurfaceStitch native id"));
}
