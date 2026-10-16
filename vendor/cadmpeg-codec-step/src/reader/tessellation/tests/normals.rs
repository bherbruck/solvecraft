// SPDX-License-Identifier: Apache-2.0
//! Tessellation normal-row normalization and scratch admission.

use super::decode_tessellation_under_policy;
use crate::parse::Value;
use cadmpeg_core::decode::{
    u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FiniteVector3;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::scalar::FiniteReal;

#[test]
fn tessellation_normal_rows_preserve_extreme_finite_directions() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service())
        .expect("empty test root");
    let rows = Value::List(vec![
        Value::List(vec![
            Value::Real(FiniteReal::new(f64::MAX).expect("finite fixture")),
            Value::Real(FiniteReal::ZERO),
            Value::Real(FiniteReal::ZERO),
        ]),
        Value::List(vec![
            Value::Real(FiniteReal::new(2.0_f64.powi(-800)).expect("finite fixture")),
            Value::Real(FiniteReal::ZERO),
            Value::Real(FiniteReal::ZERO),
        ]),
        Value::List(vec![
            Value::Real(FiniteReal::new(f64::from_bits(1)).expect("finite fixture")),
            Value::Real(FiniteReal::ZERO),
            Value::Real(FiniteReal::ZERO),
        ]),
    ]);
    assert_eq!(
        super::super::normal_rows(Some(&rows), &ctx)
            .expect("normal rows fit the service profile")
            .map(|(normals, _bytes)| normals
                .into_iter()
                .map(FiniteVector3::get)
                .collect::<Vec<_>>()),
        Some(vec![
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        ])
    );
}

#[test]
fn tessellation_normal_rows_reserve_temporary_bytes_before_collection() {
    let rows = Value::List(vec![Value::List(vec![
        Value::Real(FiniteReal::ZERO),
        Value::Real(FiniteReal::ZERO),
        Value::Real(FiniteReal::ONE),
    ])]);
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &service).expect("service root admission");
    assert!(super::super::normal_rows(Some(&rows), &ctx)
        .expect("service admits normal row")
        .is_some());
    let mut limited = service;
    limited.limits.max_materialized_bytes = u64_from_index(std::mem::size_of::<Vector3>() - 1);
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &limited).expect("limited root admission");
    let error = super::super::normal_rows(Some(&rows), &ctx)
        .expect_err("one normal row exceeds the temporary allowance");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "step_tessellation_normal_rows")
    );
}

#[test]
fn tessellation_normal_replication_reserves_temporary_bytes_before_allocation() {
    let records = "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,3,((0.,0.,1.)),$,((1,2,3)));";
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(records, service)
        .expect("service admits three replicated normals");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "step_tessellation_normal_replication",
        |cap| {
            let mut policy = service;
            policy.limits.max_materialized_bytes = cap;
            let arena = DecodeArena::new();
            let (ctx, _) =
                DecodeContext::from_root_bytes(b"", &arena, &policy).expect("owner context");
            super::super::replicated_normals(
                3,
                cadmpeg_ir::features::FiniteVector3::new(Vector3::new(0.0, 0.0, 1.0))
                    .expect("finite normal"),
                &ctx,
            )
            .map(|_| ())
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "step_tessellation_normal_replication")
    );
}
