// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::surface_ops::DesignSurfaceExtendMethod;
use crate::records::feature::surface_ops::DesignSurfaceExtendOperation;
use cadmpeg_ir::features::FaceSelection;
use cadmpeg_ir::features::Feature;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::features::FeatureOperation;

const EPS_SURFACE_DISTANCE_MM: f64 = 1.0e-12;

#[test]
fn dispatcher_projects_perpendicular_surface_extend() {
    let mut scope = DesignParameterScope::empty(
        "f3d:native/BulkStream.dat:parameter-scope#surface-extend",
        crate::records::feature::scope::DesignFeatureKind::SurfaceExtend,
        12,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::SurfaceExtend(slot) =
        scope.payload_mut()
    {
        *slot = Some(DesignSurfaceExtendOperation {
            distance: crate::test_support::real(0.04),
            distance_offset: 40,
            distance_record_index: 400,
            method: DesignSurfaceExtendMethod::Perpendicular,
            method_offset: 102,
            boundary_record_index: 500,
            boundary_reference_record_index: 900,
            boundary_reference_offset: 106,
            edge_record_indices: vec![503, 507],
            tolerance: cadmpeg_ir::scalar::PositiveReal::new(f64::EPSILON)
                .expect("checked fixture value"),
            tolerance_offset: 139,
        });
    }
    let (features, _) = crate::test_support::with_decode_context(|ctx| {
        let scopes = &[scope];
        let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            ctx,
            &crate::design::feature_project::ProjectInputs {
                scopes,
                timelines: &timelines,
                ..Default::default()
            },
        )
        .expect("test projection has a synthetic exact timeline")
    });

    let [Feature { evaluation, .. }] = features.as_slice() else {
        panic!("perpendicular SurfaceExtend did not project as a typed feature");
    };
    let FeatureDefinition::Operation(FeatureOperation::ExtendSurface {
        faces: FaceSelection::Native(native),
        distance: Some(distance),
        method: cadmpeg_ir::features::SurfaceExtension::Perpendicular,
    }) = evaluation.definition()
    else {
        panic!("perpendicular SurfaceExtend did not project as a typed feature");
    };
    assert!(native.ends_with(":design-record#500"));
    assert!((distance.get() - 0.4).abs() < EPS_SURFACE_DISTANCE_MM);
}

#[test]
fn surface_extend_boundary_id_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let mut scope = DesignParameterScope::empty(
        "f3d:native/BulkStream.dat:parameter-scope#surface-extend",
        crate::records::feature::scope::DesignFeatureKind::SurfaceExtend,
        12,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::SurfaceExtend(slot) =
        scope.payload_mut()
    {
        *slot = Some(DesignSurfaceExtendOperation {
            distance: crate::test_support::real(0.04),
            distance_offset: 40,
            distance_record_index: 400,
            method: DesignSurfaceExtendMethod::Perpendicular,
            method_offset: 102,
            boundary_record_index: 500,
            boundary_reference_record_index: 900,
            boundary_reference_offset: 106,
            edge_record_indices: vec![503, 507],
            tolerance: cadmpeg_ir::scalar::PositiveReal::new(f64::EPSILON).unwrap(),
            tolerance_offset: 139,
        });
    }
    let mut found = false;
    for limit in 0..2048 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = limit;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        if matches!(super::project_single_scope_with_context(&ctx, &scope),
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == "f3d surface extend boundary id"
                    && failure.dimension == ResourceDimension::RetainedBytes)
        {
            found = true;
            break;
        }
    }
    assert!(found, "surface extend boundary charge was not reached");
}

#[test]
fn surface_offset_boundary_id_refuses_retained_limit() {
    use crate::records::feature::surface_ops::{
        DesignSurfaceOffsetOperation, DesignSurfaceOffsetSupport,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let scope = DesignParameterScope::empty(
        "f3d:test:surface-offset#1",
        crate::records::feature::scope::DesignFeatureKind::SurfaceOffset,
        1,
    );
    let operation = DesignSurfaceOffsetOperation {
        distance: crate::test_support::real(0.04),
        distance_offset: 40,
        distance_record_index: 50,
        support: DesignSurfaceOffsetSupport::BoundaryCarrier {
            boundary_record_index: 500,
            boundary_reference_record_index: 900,
            boundary_reference_offset: 106,
            edge_record_indices: vec![503, 507],
            tolerance: cadmpeg_ir::scalar::PositiveReal::new(f64::EPSILON).unwrap(),
            tolerance_offset: 139,
        },
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::design::feature_project::project_surface_offset(
            &ctx, &scope, &operation, &[], &[],
        ),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d surface offset boundary id"
                && failure.dimension == ResourceDimension::RetainedBytes
    ));
}
