// SPDX-License-Identifier: Apache-2.0

use super::extrude_limits::{profile_group, scope};
use crate::design::feature_project::project_fixed_loft;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::path_features::DesignLoftConstruction;
use crate::records::feature::scope::DesignScopePayload;
use crate::records::topology::construction::DesignConstructionOperandRole;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::decode::{DecodeContext, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, LoftSection};

fn loft_input(
    roles: [DesignOperandRole; 2],
) -> (
    crate::records::feature::scope::DesignParameterScope,
    [crate::records::topology::construction::DesignConstructionOperandGroup; 2],
) {
    let mut scope = scope();
    scope
        .try_edit(|draft| {
            draft.payload = DesignScopePayload::Loft(Some(DesignLoftConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: 128,
            }));
        })
        .unwrap();
    let mut first = profile_group(100, 0);
    first.operand_role = DesignConstructionOperandRole::Other(roles[0]);
    let mut second = profile_group(101, 1);
    second.operand_role = DesignConstructionOperandRole::Other(roles[1]);
    (scope, [first, second])
}

fn project(
    ctx: &DecodeContext<'_>,
    scope: &crate::records::feature::scope::DesignParameterScope,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
) -> Result<Option<FeatureDefinition>, CodecError> {
    project_fixed_loft(scope, groups, &[], &[], &[], &[], ctx)
}

fn assert_loft_limit(operation: &'static str, dimension: ResourceDimension) {
    assert_loft_limit_with_roles(
        operation,
        dimension,
        [DesignOperandRole::PROFILE, DesignOperandRole::PROFILE],
        false,
    );
}

fn assert_loft_limit_with_roles(
    operation: &'static str,
    dimension: ResourceDimension,
    roles: [DesignOperandRole; 2],
    has_point: bool,
) {
    let (scope, groups) = loft_input(roles);
    let definition =
        crate::test_support::with_decode_context(|decode_ctx| project(decode_ctx, &scope, &groups))
            .unwrap()
            .unwrap();
    assert!(matches!(definition,
        FeatureDefinition::Operation(FeatureOperation::Loft { sections, .. })
            if sections.len() == 2
                && sections.iter().any(|section| matches!(section, LoftSection::Point(_))) == has_point
    ));
    let error = crate::test_support::resource_refusal_at(dimension, operation, 0, |ctx| {
        project(ctx, &scope, &groups)
    });
    assert!(
        matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(failure))
                if failure.dimension == dimension && failure.operation == operation)
    );
}

#[test]
fn loft_scope_group_refuses_collection_limit() {
    assert_loft_limit("f3d Loft scope group", ResourceDimension::CollectionItems);
}

#[test]
fn loft_operand_group_refuses_collection_limit() {
    assert_loft_limit("f3d Loft operand group", ResourceDimension::CollectionItems);
}

#[test]
fn loft_profile_group_refuses_collection_limit() {
    assert_loft_limit("f3d Loft profile group", ResourceDimension::CollectionItems);
}

#[test]
fn loft_profile_id_refuses_retained_limit() {
    assert_loft_limit(
        "f3d Loft profile group id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn loft_section_refuses_collection_limit() {
    assert_loft_limit("f3d Loft section", ResourceDimension::CollectionItems);
}

#[test]
fn loft_point_section_id_refuses_retained_limit() {
    assert_loft_limit_with_roles(
        "f3d Loft section group id",
        ResourceDimension::RetainedBytes,
        [DesignOperandRole::ROLE_0X43, DesignOperandRole::ROLE_0X5],
        true,
    );
}

#[test]
fn loft_native_section_id_refuses_retained_limit() {
    assert_loft_limit_with_roles(
        "f3d Loft section group id",
        ResourceDimension::RetainedBytes,
        [DesignOperandRole::ROLE_0X5, DesignOperandRole::ROLE_0X5],
        false,
    );
}
