// SPDX-License-Identifier: Apache-2.0
use super::{
    dimension_recipe_record, fixture, native_fallback_annotation, native_fallback_curves,
    native_fallback_group, native_fallback_null_pair, native_fallback_pair, parameter_companion,
    EPS_NATIVE_FALLBACK_LINEAR,
};
use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::dimensions::{
    project_dimension_constraints, project_spatial_dimension_constraints,
};
use crate::design::test_support::parameter_record;
use crate::records::parameters::DesignCompanionPayload;
use cadmpeg_core::decode::ResourceDimension;

fn planar(kind: &str) {
    let mut fixture = fixture();
    let curves = native_fallback_curves(&mut fixture);
    let pair = native_fallback_pair();
    let group = native_fallback_group();
    let null_pair = native_fallback_null_pair();
    let annotation = native_fallback_annotation();
    let companion = parameter_companion().bound(DesignCompanionPayload::new(58, 1, Vec::new()));
    let recipes = [dimension_recipe_record(40), dimension_recipe_record(41)];
    if matches!(kind, "recipe" | "companion") {
        let mut parameter = parse_design_parameter_record(&parameter_record(
            Some(21),
            "0.1 rad",
            "Angular Dimension",
            Some("rad"),
            "a1",
            0.1,
        ))
        .unwrap();
        parameter.id = fixture.parameter.id.clone();
        parameter.record_index = fixture.parameter.record_index;
        fixture.parameter = parameter;
    }
    let mut inputs = fixture.inputs();
    let operation = match kind {
        "group" => {
            inputs.curves = &curves;
            inputs.groups = std::slice::from_ref(&group);
            "f3d group source kind"
        }
        "pair" => {
            inputs.curves = &curves;
            inputs.pairs = std::slice::from_ref(&pair);
            "f3d pair source kind"
        }
        "annotation" => {
            inputs.annotation_frames = std::slice::from_ref(&annotation);
            "f3d annotation source kind"
        }
        "null pair" => {
            inputs.null_pairs = std::slice::from_ref(&null_pair);
            "f3d null pair source kind"
        }
        "recipe" => {
            inputs.companions = std::slice::from_ref(&companion);
            inputs.recipe_records = &recipes;
            "f3d recipe source kind"
        }
        "companion" => {
            inputs.companions = std::slice::from_ref(&companion);
            "f3d companion source kind"
        }
        _ => panic!("unknown source kind fixture"),
    };
    super::super::assert_dimension_refusal(operation, ResourceDimension::RetainedBytes, |ctx| {
        project_dimension_constraints(ctx, &inputs, &[], EPS_NATIVE_FALLBACK_LINEAR).map(|_| ())
    });
}

#[test]
fn group_source_kind_refuses_retained_limit() {
    planar("group");
}
#[test]
fn pair_source_kind_refuses_retained_limit() {
    planar("pair");
}
#[test]
fn annotation_source_kind_refuses_retained_limit() {
    planar("annotation");
}
#[test]
fn null_pair_source_kind_refuses_retained_limit() {
    planar("null pair");
}
#[test]
fn recipe_source_kind_refuses_retained_limit() {
    planar("recipe");
}
#[test]
fn companion_source_kind_refuses_retained_limit() {
    planar("companion");
}
#[test]
fn spatial_companion_source_kind_refuses_retained_limit() {
    let fixture = fixture();
    let companion = parameter_companion();
    let mut inputs = fixture.inputs();
    inputs.companions = std::slice::from_ref(&companion);
    super::super::assert_dimension_refusal(
        "f3d spatial companion source kind",
        ResourceDimension::RetainedBytes,
        |ctx| {
            project_spatial_dimension_constraints(
                ctx,
                &inputs,
                std::slice::from_ref(&fixture.spatial),
                &[],
                EPS_NATIVE_FALLBACK_LINEAR,
            )
            .map(|_| ())
        },
    );
}
