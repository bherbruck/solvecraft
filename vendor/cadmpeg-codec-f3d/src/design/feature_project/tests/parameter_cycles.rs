use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use std::io::{Cursor, Write};

use crate::design::feature_project::{cyclic_parameter_components, normalize_parameter_ordinals};
use crate::loss::F3dLossCode;
use crate::test_support::lp_utf16;
use crate::test_support::manifest_test::write_synthetic_manifests;
use crate::F3dCodec;

fn decode_parameters(records: &[(u32, &str, &str)]) -> DecodeResult {
    let mut bulk = Vec::new();
    for (ordinal, name, expression) in records {
        let start = bulk.len();
        // Discriminated document parameter, as specified in F3D section 3.1.
        bulk.extend_from_slice(&3_u32.to_le_bytes());
        bulk.extend_from_slice(b"305");
        bulk.extend_from_slice(&(100 + ordinal).to_le_bytes());
        bulk.extend_from_slice(&[0; 11]);
        bulk.extend_from_slice(&6_u64.to_le_bytes());
        bulk.push(0);
        bulk.extend_from_slice(&ordinal.to_le_bytes());
        bulk.push(0);
        lp_utf16(&mut bulk, expression);
        bulk.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 1]);
        lp_utf16(&mut bulk, "User Parameter");
        bulk.extend_from_slice(&0_u32.to_le_bytes());
        lp_utf16(&mut bulk, "mm");
        lp_utf16(&mut bulk, name);
        bulk.extend_from_slice(&1.0_f64.to_le_bytes());
        bulk.extend_from_slice(&[0, 1, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert!(
            crate::design::decode::parameters::parse_design_parameter_record(&bulk[start..])
                .is_some()
        );
    }
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&bulk).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.parameters.len(), records.len());
    decoded
}

fn dependencies(decoded: &DecodeResult) -> std::collections::BTreeMap<&str, Vec<&str>> {
    let parameters = &decoded.ir().model.parameters;
    parameters
        .iter()
        .map(|parameter| {
            (
                parameter.name.as_str(),
                parameter
                    .dependencies
                    .iter()
                    .map(|id| {
                        parameters
                            .iter()
                            .find(|candidate| candidate.id == *id)
                            .unwrap()
                            .name
                            .as_str()
                    })
                    .collect(),
            )
        })
        .collect()
}

fn two_parameter_cycle() -> Vec<cadmpeg_ir::features::DesignParameter> {
    let decoded = decode_parameters(&[(0, "A", "B"), (1, "B", "A")]);
    let mut parameters = decoded.ir().model.parameters.clone();
    let first = parameters[0].id.clone();
    let second = parameters[1].id.clone();
    parameters[0].dependencies.clear();
    parameters[0]
        .dependencies
        .insert(
            &cadmpeg_test_support::service_decode_context(),
            second,
            "insert fixture member",
        )
        .expect("member insertion admission");
    parameters[1].dependencies.clear();
    parameters[1]
        .dependencies
        .insert(
            &cadmpeg_test_support::service_decode_context(),
            first,
            "insert fixture member",
        )
        .expect("member insertion admission");
    parameters
}

fn owned_parameter_cycle() -> Vec<cadmpeg_ir::features::DesignParameter> {
    let mut parameters = two_parameter_cycle();
    let owner = crate::ids::neutral_feature_id_parts("test", "cycle", 0, 0);
    parameters[0].owner = Some(owner.clone());
    parameters[1].owner = Some(owner);
    parameters
}

fn assert_normalization_refusal(
    operation: &'static str,
    dimension: cadmpeg_core::decode::ResourceDimension,
) {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    let parameters = owned_parameter_cycle();
    let owners = parameters
        .iter()
        .map(|parameter| (parameter.id.clone(), parameter.owner.clone()))
        .collect();
    let dimension = if dimension == ResourceDimension::RetainedBytes {
        ResourceDimension::MaterializedBytes
    } else {
        dimension
    };
    let error = crate::test_support::resource_refusal_at(dimension, operation, 0, |ctx| {
        let mut copy = parameters.clone();
        normalize_parameter_ordinals(ctx, &mut copy, &owners)
    });
    assert!(matches!(error, CodecError::ResourceLimit(failure)
        if failure.operation == operation && failure.dimension == dimension));
}

macro_rules! normalization_limit_test {
    ($name:ident, $operation:literal, $dimension:ident) => {
        #[test]
        fn $name() {
            assert_normalization_refusal(
                $operation,
                cadmpeg_core::decode::ResourceDimension::$dimension,
            );
        }
    };
}

normalization_limit_test!(
    parameter_owner_id_refuses_retained_limit,
    "f3d parameter owner ID",
    RetainedBytes
);
normalization_limit_test!(
    parameter_owner_member_refuses_collection_limit,
    "f3d parameter owner member",
    CollectionItems
);
normalization_limit_test!(
    parameter_owner_group_refuses_collection_limit,
    "f3d parameter owner group",
    CollectionItems
);
normalization_limit_test!(
    parameter_group_ordinal_refuses_collection_limit,
    "f3d parameter group ordinal",
    CollectionItems
);
normalization_limit_test!(
    parameter_unresolved_index_refuses_collection_limit,
    "f3d parameter unresolved index",
    CollectionItems
);
normalization_limit_test!(
    parameter_readiness_refuses_work_limit,
    "f3d parameter readiness",
    WorkUnits
);
normalization_limit_test!(
    parameter_ready_index_refuses_collection_limit,
    "f3d parameter ready index",
    CollectionItems
);
normalization_limit_test!(
    parameter_cycle_member_id_refuses_retained_limit,
    "f3d parameter cycle member ID",
    RetainedBytes
);
normalization_limit_test!(
    parameter_cycle_member_refuses_collection_limit,
    "f3d parameter cycle member",
    CollectionItems
);
normalization_limit_test!(
    parameter_resolved_id_refuses_retained_limit,
    "f3d parameter resolved ID",
    RetainedBytes
);
normalization_limit_test!(
    parameter_resolved_index_refuses_collection_limit,
    "f3d parameter resolved index",
    CollectionItems
);
normalization_limit_test!(
    parameter_sorted_order_refuses_collection_limit,
    "f3d parameter sorted order",
    CollectionItems
);

fn assert_cycle_collection_refusal(limit: u64, operation: &'static str) {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    let parameters = two_parameter_cycle();
    let unresolved = std::collections::HashSet::from([0, 1]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = limit;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(cyclic_parameter_components(&ctx, &parameters, &unresolved),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == operation
                && failure.dimension == ResourceDimension::CollectionItems)
    );
}

macro_rules! cycle_collection_limit_test {
    ($name:ident, $limit:literal, $operation:literal) => {
        #[test]
        fn $name() {
            assert_cycle_collection_refusal($limit, $operation);
        }
    };
}

cycle_collection_limit_test!(
    cycle_indices_refuse_collection_limit,
    0,
    "f3d parameter cycle indices"
);
cycle_collection_limit_test!(
    cycle_local_index_refuses_collection_limit,
    2,
    "f3d parameter cycle local index"
);
cycle_collection_limit_test!(
    cycle_dependency_edge_refuses_collection_limit,
    4,
    "f3d parameter cycle dependency edge"
);
cycle_collection_limit_test!(
    cycle_edge_list_refuses_collection_limit,
    5,
    "f3d parameter cycle edge list"
);
cycle_collection_limit_test!(
    cycle_incoming_list_refuses_collection_limit,
    8,
    "f3d parameter cycle incoming list"
);
cycle_collection_limit_test!(
    cycle_incoming_edge_refuses_collection_limit,
    10,
    "f3d parameter cycle incoming edge"
);
cycle_collection_limit_test!(
    cycle_pending_visit_refuses_collection_limit,
    12,
    "f3d parameter cycle pending visit"
);
cycle_collection_limit_test!(
    cycle_visited_node_refuses_collection_limit,
    13,
    "f3d parameter cycle visited node"
);
cycle_collection_limit_test!(
    cycle_finish_order_refuses_collection_limit,
    19,
    "f3d parameter cycle finish order"
);
cycle_collection_limit_test!(
    cycle_assigned_node_refuses_collection_limit,
    21,
    "f3d parameter cycle assigned node"
);
cycle_collection_limit_test!(
    cycle_reverse_pending_refuses_collection_limit,
    22,
    "f3d parameter cycle reverse pending"
);
cycle_collection_limit_test!(
    cycle_component_member_refuses_collection_limit,
    24,
    "f3d parameter cycle component member"
);
cycle_collection_limit_test!(
    cycle_component_refuses_collection_limit,
    26,
    "f3d parameter cycle component"
);

fn assert_cycle_work_refusal(limit: u64, operation: &'static str) {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    let parameters = two_parameter_cycle();
    let unresolved = std::collections::HashSet::from([0, 1]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = limit;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(cyclic_parameter_components(&ctx, &parameters, &unresolved),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == operation
                && failure.dimension == ResourceDimension::WorkUnits)
    );
}

#[test]
fn cycle_visit_refuses_work_limit() {
    assert_cycle_work_refusal(0, "f3d parameter cycle visit");
}

#[test]
fn cycle_reverse_visit_refuses_work_limit() {
    assert_cycle_work_refusal(5, "f3d parameter cycle reverse visit");
}

#[test]
fn parameter_cycle_preserves_an_acyclic_dependency_into_the_cycle() {
    let records = [(0, "C", "A"), (1, "A", "B"), (2, "B", "A")];
    for order in [[0, 1, 2], [2, 1, 0], [1, 2, 0]] {
        let decoded = decode_parameters(&order.map(|index| records[index]));
        assert_eq!(
            dependencies(&decoded),
            std::collections::BTreeMap::from([("A", vec![]), ("B", vec!["A"]), ("C", vec!["A"])])
        );
        let parameters = &decoded.ir().model.parameters;
        let ordinal = |name| parameters.iter().find(|p| p.name == name).unwrap().ordinal;
        assert!(ordinal("A") < ordinal("C"));
        assert!(ordinal("A") < ordinal("B"));
        let loss = decoded
            .report()
            .losses
            .iter()
            .find(|loss| loss.code == F3dLossCode::ParameterExpressionUnbound.kind())
            .unwrap();
        assert!(loss
            .message
            .starts_with("1 decoded parameter expression symbol(s)"));
        let native = crate::test_support::native_test::f3d_native(decoded.ir());
        assert_eq!(native.design_parameters.len(), 3);
        assert!(native
            .design_parameters
            .iter()
            .any(|parameter| parameter.name() == "C" && parameter.expression() == "A"));
    }
}

#[test]
fn parameter_cycle_preserves_dependencies_between_distinct_cycles() {
    let decoded = decode_parameters(&[
        (0, "A", "B + D"),
        (1, "B", "A"),
        (2, "C", "D"),
        (3, "D", "C"),
    ]);
    assert_eq!(
        dependencies(&decoded),
        std::collections::BTreeMap::from([
            ("A", vec!["D"]),
            ("B", vec!["A"]),
            ("C", vec![]),
            ("D", vec!["C"]),
        ])
    );
    let parameters = &decoded.ir().model.parameters;
    let ordinal = |name| parameters.iter().find(|p| p.name == name).unwrap().ordinal;
    assert!(ordinal("C") < ordinal("D"));
    assert!(ordinal("D") < ordinal("A"));
    assert!(ordinal("A") < ordinal("B"));
}

#[test]
fn parameter_cycle_ordering_resumes_before_breaking_another_cycle() {
    let decoded = decode_parameters(&[(0, "A", "D"), (1, "B", "C"), (2, "C", "B"), (3, "D", "A")]);
    let mut ordered = decoded.ir().model.parameters.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|parameter| parameter.ordinal);
    assert_eq!(
        ordered
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect::<Vec<_>>(),
        ["A", "D", "B", "C"]
    );
}
