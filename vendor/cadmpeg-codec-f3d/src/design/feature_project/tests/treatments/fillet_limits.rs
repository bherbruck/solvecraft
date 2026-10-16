// SPDX-License-Identifier: Apache-2.0
use super::{
    localized_fillet_group, localized_fillet_operand_groups, localized_fillet_owner,
    localized_fillet_parameter, localized_fillet_scope,
};
use crate::design::decode::operands::decode_fillet_radius_groups as decode_fillet_radius_groups_charged;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::{DesignParameter, DesignParameterOwner};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::fillet::DesignFilletRadiusGroup;

pub(super) fn decode_fillet_radius_groups(
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    owners: &[DesignParameterOwner],
    parameters: &[DesignParameter],
) -> Vec<DesignFilletRadiusGroup> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    decode_fillet_radius_groups_charged(&ctx, scopes, groups, owners, parameters).unwrap()
}

fn variable_law_parameters() -> Vec<DesignParameter> {
    [
        (1, "StartRadius", Some("mm"), 0.2),
        (2, "EndRadius", Some("mm"), 0.3),
        (3, "MidRadius", Some("mm"), 0.4),
        (4, "MidParams", None, 0.5),
    ]
    .into_iter()
    .map(|(record, kind, unit, value)| {
        let mut parameter = super::parse_design_parameter_record(&super::parameter_record(
            Some(record + 100),
            "value",
            kind,
            unit,
            "d1",
            value,
        ))
        .unwrap();
        parameter.record_index = record;
        parameter
    })
    .collect()
}

fn assert_variable_law_limit(operation: &'static str) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let parameters = variable_law_parameters();
    let controls = parameters
        .iter()
        .enumerate()
        .map(|(ordinal, parameter)| (u32::try_from(ordinal).unwrap(), parameter))
        .collect::<Vec<_>>();
    let mut found = false;
    for limit in 0..32 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        if matches!(crate::design::feature_project::variable_fillet_law(&ctx, &controls),
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == operation
                    && failure.dimension == ResourceDimension::CollectionItems)
        {
            found = true;
            break;
        }
    }
    assert!(found, "no collection refusal at {operation}");
}

macro_rules! variable_law_limit_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            assert_variable_law_limit($operation);
        }
    };
}

variable_law_limit_test!(
    variable_fillet_middle_radii_refuse_collection_limit,
    "f3d variable Fillet middle radii"
);
variable_law_limit_test!(
    variable_fillet_middle_parameters_refuse_collection_limit,
    "f3d variable Fillet middle parameters"
);
variable_law_limit_test!(
    variable_fillet_radius_points_refuse_collection_limit,
    "f3d variable Fillet radius point"
);

fn variable_assignment() -> DesignFilletRadiusGroup {
    use crate::records::topology::fillet::{DesignFilletMidpoint, DesignFilletRadiusLaw};

    DesignFilletRadiusGroup {
        id: "f3d:native/BulkStream.dat:fillet-group#1".to_owned(),
        scope_record_index: 12,
        group_ordinal: 0,
        group_record_index: 2,
        edge_operand_record_indices: Vec::new(),
        law: DesignFilletRadiusLaw::Variable {
            start_radius_parameter_record_index: 1,
            end_radius_parameter_record_index: 2,
            middle: vec![DesignFilletMidpoint {
                radius_parameter_record_index: 3,
                parameter_record_index: 4,
            }],
        },
        tangency_weight_parameter_record_index: None,
    }
}

fn assert_resolved_assignment_limit(operation: &'static str) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let parameters = variable_law_parameters();
    let controls = parameters
        .iter()
        .enumerate()
        .map(|(ordinal, parameter)| (u32::try_from(ordinal).unwrap(), parameter))
        .collect::<Vec<_>>();
    let assignment = variable_assignment();
    let mut found = false;
    for limit in 0..64 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut row_storage = ctx
            .reserve_scoped(0, "test Fillet assignment rows")
            .unwrap();
        if matches!(crate::design::feature_project::resolved_fillet_assignments(&ctx, &mut row_storage, &[&assignment], &controls),
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == operation
                    && failure.dimension == ResourceDimension::CollectionItems)
        {
            found = true;
            break;
        }
    }
    assert!(found, "no collection refusal at {operation}");
}

macro_rules! resolved_assignment_limit_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            assert_resolved_assignment_limit($operation);
        }
    };
}

resolved_assignment_limit_test!(
    fillet_assignment_parameter_index_refuses_collection_limit,
    "f3d Fillet assignment parameter index"
);
resolved_assignment_limit_test!(
    fillet_assigned_parameter_refuses_collection_limit,
    "f3d Fillet assigned parameter"
);
resolved_assignment_limit_test!(
    fillet_variable_control_refuses_collection_limit,
    "f3d Fillet variable control"
);
resolved_assignment_limit_test!(
    fillet_resolved_assignment_refuses_collection_limit,
    "f3d Fillet resolved assignment"
);

fn assert_projected_fillet_limit(
    operation: &'static str,
    dimension: cadmpeg_core::decode::ResourceDimension,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let scope = localized_fillet_scope();
    let native_scope = crate::ids::native_stream(&scope.id).unwrap();
    let parameters = variable_law_parameters();
    let controls = parameters
        .iter()
        .enumerate()
        .map(|(ordinal, parameter)| (u32::try_from(ordinal).unwrap(), parameter))
        .collect::<Vec<_>>();
    let assignment = variable_assignment();
    let inputs = crate::design::feature_project::ProjectInputs {
        native: &parameters,
        scopes: std::slice::from_ref(&scope),
        fillet_radius_groups: std::slice::from_ref(&assignment),
        ..Default::default()
    };
    let mut found = false;
    for limit in 0..128 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        match dimension {
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
            _ => unreachable!(),
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
                (crate::design::feature_project::project_fillet_arm(
                    &ctx,
                    &inputs,
                    &scope,
                    &controls,
                    native_scope,
                ))
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
        if matches!(crate::design::feature_project::project_fillet_arm(&ctx, &inputs, &scope, &controls, native_scope),
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == operation && failure.dimension == dimension)
        {
            found = true;
            break;
        }
    }
    assert!(found, "no resource refusal at {operation}");
}

#[test]
fn fillet_scope_assignment_refuses_collection_limit() {
    assert_projected_fillet_limit(
        "f3d Fillet scope assignment",
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
    );
}

#[test]
fn fillet_projected_group_refuses_collection_limit() {
    assert_projected_fillet_limit(
        "f3d Fillet projected group",
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
    );
}

#[test]
fn fillet_fallback_group_id_refuses_retained_limit() {
    assert_projected_fillet_limit(
        "f3d Fillet fallback edge group ID",
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
    );
}

#[test]
fn fillet_single_radius_scope_id_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let scope = localized_fillet_scope();
    let radius = localized_fillet_parameter(10, 11, "Radius", Some("mm"), 0.5);
    let controls = [(0, &radius)];
    let inputs = crate::design::feature_project::ProjectInputs {
        native: std::slice::from_ref(&radius),
        scopes: std::slice::from_ref(&scope),
        ..Default::default()
    };
    let native_scope = crate::ids::native_stream(&scope.id).unwrap();
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            "f3d Fillet single radius edge scope ID",
            |cap| {
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = cap;
                let arena = DecodeArena::new();

                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                (crate::design::feature_project::project_fillet_arm(
                    &ctx,
                    &inputs,
                    &scope,
                    &controls,
                    native_scope,
                ))
                .map(|_| ())
            },
        );
        assert!(
            matches!(Err::<(), cadmpeg_core::CodecError>(error), Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::RetainedBytes
                    && failure.operation == "f3d Fillet single radius edge scope ID")
        );
    }
}

#[test]
fn face_selection_native_fallback_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let scope = localized_fillet_scope();
    let group = localized_fillet_group(101, 0, vec![201]);
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            "f3d face selection native fallback",
            |cap| {
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = cap;
                let arena = DecodeArena::new();

                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                (crate::design::feature_project::project_face_selection(
                    &ctx,
                    &scope,
                    &group,
                    &[],
                    &[],
                ))
                .map(|_| ())
            },
        );
        assert!(
            matches!(Err::<(), cadmpeg_core::CodecError>(error), Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::RetainedBytes
                    && failure.operation == "f3d face selection native fallback")
        );
    }
}

#[test]
fn fillet_radius_group_collections_and_ids_refuse_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let scope = localized_fillet_scope();
    let groups = localized_fillet_operand_groups();
    let parameter = localized_fillet_parameter;
    let owner = localized_fillet_owner;
    let scenarios = [
        (
            vec![
                parameter(10, 11, "Radius", Some("mm"), 0.5),
                parameter(30, 31, "TangencyWeight", None, 1.0),
            ],
            vec![owner(10, 11, 0), owner(30, 31, 1)],
        ),
        (
            vec![parameter(10, 11, "ChordLen", Some("mm"), 0.5)],
            vec![owner(10, 11, 0)],
        ),
        (
            vec![
                parameter(10, 11, "EdgeOffset1", Some("mm"), 0.2),
                parameter(20, 21, "EdgeOffset2", Some("mm"), 0.3),
                parameter(30, 31, "TangencyWeight", None, 1.0),
            ],
            vec![owner(10, 11, 0), owner(20, 21, 1), owner(30, 31, 2)],
        ),
        (
            vec![
                parameter(10, 11, "StartRadius", Some("mm"), 0.2),
                parameter(20, 21, "EndRadius", Some("mm"), 0.3),
                parameter(30, 31, "MidRadius", Some("mm"), 0.4),
                parameter(40, 41, "MidParams", None, 0.5),
                parameter(50, 51, "TangencyWeight", None, 1.0),
            ],
            vec![
                owner(10, 11, 0),
                owner(20, 21, 1),
                owner(30, 31, 2),
                owner(40, 41, 3),
                owner(50, 51, 4),
            ],
        ),
    ];
    let mut refused = std::collections::HashSet::new();
    for (parameters, owners) in &scenarios {
        for limit in 0..32 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_collection_items = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            match decode_fillet_radius_groups_charged(
                &ctx,
                std::slice::from_ref(&scope),
                &groups[..1],
                owners,
                parameters,
            ) {
                Err(CodecError::ResourceLimit(failure)) => {
                    assert_eq!(failure.dimension, ResourceDimension::CollectionItems);
                    refused.insert(failure.operation);
                }
                Ok(assignments) => assert_eq!(assignments.len(), 1),
                Err(error) => panic!("unexpected Fillet decode error: {error}"),
            }
        }
    }
    for operation in [
        "f3d Fillet parameter index",
        "f3d Fillet scope groups",
        "f3d Fillet owned parameters",
        "f3d Fillet radius parameters",
        "f3d Fillet weight parameters",
        "f3d Fillet chord lengths",
        "f3d Fillet asymmetric offsets",
        "f3d Fillet variable parameters",
        "f3d Fillet middle parameters",
        "f3d Fillet edge operand indices",
        "f3d Fillet group output",
    ] {
        assert!(
            refused.contains(operation),
            "no limit refusal at {operation}"
        );
    }

    let stream = "f3d:native/BulkStream.dat";
    let group_id_len = stream.len() + ":design-fillet-radius-group#".len() + 3;
    for (limit, operation) in [
        (stream.len() - 1, "f3d Fillet group stream ID"),
        (group_id_len - 1, "f3d Fillet group ID suffix"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = u64::try_from(limit).unwrap();
        let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            operation,
            |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match ResourceDimension::RetainedBytes {
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
                (decode_fillet_radius_groups_charged(
                    &ctx,
                    std::slice::from_ref(&scope),
                    &groups[..1],
                    &scenarios[0].1,
                    &scenarios[0].0,
                ))
                .map(|_| ())
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match ResourceDimension::RetainedBytes {
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
        let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            operation,
            |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match ResourceDimension::RetainedBytes {
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
                (decode_fillet_radius_groups_charged(
                    &ctx,
                    std::slice::from_ref(&scope),
                    &groups[..1],
                    &scenarios[0].1,
                    &scenarios[0].0,
                ))
                .map(|_| ())
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match ResourceDimension::RetainedBytes {
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
            decode_fillet_radius_groups_charged(&ctx, std::slice::from_ref(&scope), &groups[..1], &scenarios[0].1, &scenarios[0].0),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::RetainedBytes && failure.operation == operation
        ));
    }
}
