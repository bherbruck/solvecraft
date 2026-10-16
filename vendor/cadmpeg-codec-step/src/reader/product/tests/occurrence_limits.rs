// SPDX-License-Identifier: Apache-2.0
//! Collection limits for product occurrence expansion.

#[test]
fn root_occurrence_items_refuse_collection_limit() {
    super::product_collection_refuses("step_root_occurrence_items");
}

#[test]
fn root_occurrence_path_map_refuses_collection_limit() {
    super::product_collection_refuses("step_root_occurrence_path_map");
}

#[test]
fn root_occurrence_path_members_refuse_collection_limit() {
    super::product_collection_refuses("step_root_occurrence_path_members");
}

#[test]
fn usage_instance_counts_refuse_collection_limit() {
    super::product_collection_refuses("step_usage_instance_counts");
}

#[test]
fn child_occurrence_ordinals_refuse_collection_limit() {
    super::product_collection_refuses("step_child_occurrence_ordinals");
}

#[test]
fn missing_placement_reports_refuse_collection_limit() {
    super::product_collection_refuses("step_missing_placement_reports");
}

#[test]
fn child_occurrence_items_refuse_collection_limit() {
    super::product_collection_refuses("step_child_occurrence_items");
}

#[test]
fn child_occurrence_path_members_refuse_collection_limit() {
    super::product_collection_refuses("step_child_occurrence_path_members");
}

#[test]
fn child_occurrence_path_map_refuses_collection_limit() {
    super::product_collection_refuses("step_child_occurrence_path_map");
}

fn occurrence_output_refuses(existing: usize) {
    crate::test_support::with_service_context(super::PRODUCT_STRING_LIMIT_SOURCE, |source, ctx| {
        let (exchange, _) = crate::parse::parse_inner(source, ctx).expect("valid product source");
        let mut ir = cadmpeg_ir::CadIr::empty();
        let (geometry, _) = crate::reader::geometry::decode(&exchange, &mut ir, ctx).unwrap();
        let index = crate::reader::index::CarrierIndex::from_ir(&ir, ctx).unwrap();
        let topology = crate::reader::topology::decode(&exchange, &mut ir, &index, ctx).unwrap();
        let mut admitted = 0;
        super::super::decode(
            &exchange,
            &geometry.value,
            &topology.value,
            &mut ir,
            ctx,
            &mut admitted,
        )
        .expect("initial assembly fits");
        ir.model
            .occurrences
            .resize(existing, ir.model.occurrences[0].clone());
        let model_input =
            serde_json::to_vec(&ir).expect("synthetic existing assembly budget envelope");
        let error = crate::test_support::with_service_context(&model_input, |_, ctx| {
            super::super::decode(
                &exchange,
                &geometry.value,
                &topology.value,
                &mut ir,
                ctx,
                &mut admitted,
            )
            .err()
            .expect("occurrence slice must refuse")
        });
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.operation == "step_assembly_occurrence_limit"
                && refusal.limit == 100_000 && refusal.additional == 1)
        );
        assert_eq!(ir.model.occurrences.len(), 100_000);
    });
}

#[test]
fn root_occurrence_output_slice_refuses_before_insertion() {
    occurrence_output_refuses(100_000);
}

#[test]
fn child_occurrence_output_slice_refuses_instead_of_partial_success() {
    occurrence_output_refuses(99_999);
}

#[test]
fn assembly_depth_slice_refuses_instead_of_skipping_child() {
    crate::test_support::with_service_context(super::PRODUCT_STRING_LIMIT_SOURCE, |source, ctx| {
        let (exchange, _) = crate::parse::parse_inner(source, ctx).unwrap();
        let mut ir = cadmpeg_ir::CadIr::empty();
        let (geometry, _) = crate::reader::geometry::decode(&exchange, &mut ir, ctx).unwrap();
        let index = crate::reader::index::CarrierIndex::from_ir(&ir, ctx).unwrap();
        let topology = crate::reader::topology::decode(&exchange, &mut ir, &index, ctx).unwrap();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_recursion_depth = 1;
        crate::test_support::with_policy_context(source, &policy, |_, limited| {
            let error = super::super::decode(
                &exchange,
                &geometry.value,
                &topology.value,
                &mut ir,
                limited,
                &mut 0,
            )
            .err()
            .expect("child exceeds depth one");
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
                if refusal.operation == "step_assembly_depth_limit" && refusal.limit == 1)
            );
        });
    });
}
