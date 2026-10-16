// SPDX-License-Identifier: Apache-2.0
//! AP242 indexed tessellation tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::fmt::Write as _;
use std::io::Cursor;

use cadmpeg_core::decode::{
    u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::{Point3, Vector3};

use crate::loss::StepLossCode;
use crate::parse::{parse_inner, Value};
use crate::test_support::exchange::{decode_inline, decode_inline_result};
use crate::test_support::with_service_context;
use crate::StepCodec;

mod normals;
mod retained_body;

const EPS_SAME_POINT: f64 = 1.0e-12;

fn assert_point3_close(actual: Point3, expected: Point3) {
    assert!((actual.x - expected.x).abs() < EPS_SAME_POINT);
    assert!((actual.y - expected.y).abs() < EPS_SAME_POINT);
    assert!((actual.z - expected.z).abs() < EPS_SAME_POINT);
}

fn assert_vector3_close(actual: Vector3, expected: Vector3) {
    assert!((actual.x - expected.x).abs() < EPS_SAME_POINT);
    assert!((actual.y - expected.y).abs() < EPS_SAME_POINT);
    assert!((actual.z - expected.z).abs() < EPS_SAME_POINT);
}

fn decode_tessellation_under_policy(
    records: &str,
    policy: DecodePolicy,
) -> Result<CadIr, CodecError> {
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;"
    );
    let (exchange, _) =
        with_service_context(source.as_bytes(), parse_inner).expect("test exchange parses");
    let mut ir = CadIr::empty();
    let topology_arena = DecodeArena::new();
    let (topology_ctx, _) = DecodeContext::from_root_bytes(
        source.as_bytes(),
        &topology_arena,
        &DecodePolicy::service(),
    )?;
    let geometry = super::super::geometry::decode(&exchange, &mut ir, &topology_ctx)
        .expect("resource allocation did not fail")
        .0
        .value;
    let index = super::super::index::CarrierIndex::from_ir(&ir, &topology_ctx)?;
    let topology = super::super::topology::decode(&exchange, &mut ir, &index, &topology_ctx)
        .expect("test topology decodes")
        .value;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)?;
    super::decode(&exchange, &geometry, &topology, &mut ir, &ctx)?;
    Ok(ir)
}

fn assert_tessellation_collection_refusal(records: &str, admitted: u64, operation: &'static str) {
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(records, service).expect("service admits tessellation");
    let mut limited = service;
    limited.limits.max_collection_items = admitted;
    let error = decode_tessellation_under_policy(records, limited)
        .expect_err("selected collection limit refuses the next tessellation allocation");
    assert!(
        matches!(&error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == operation),
        "unexpected refusal: {error}"
    );
}

const ONE_TRIANGLE: &str = "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,3,$,$,((1,2,3)));";
const ONE_TRIANGLE_WITH_PNINDEX: &str =
    "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,3,$,(3,2,1),((1,2,3)));";
const ONE_TRIANGLE_IN_CONTAINER: &str =
    "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,3,$,$,((1,2,3)));
#3=TESSELLATED_SOLID('',(#2),$);";
const MISSING_CONTAINER_ITEMS: &str = "#1=TESSELLATED_SOLID('',$,$);";

#[test]
fn tessellation_loss_note_collection_is_admitted_before_creation() {
    assert_tessellation_collection_refusal(
        MISSING_CONTAINER_ITEMS,
        0,
        "step_tessellation_loss_notes",
    );
}

#[test]
fn tessellation_loss_note_bytes_are_admitted_before_formatting() {
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(MISSING_CONTAINER_ITEMS, service)
        .expect("service admits the loss note");
    let mut limited = service;
    limited.limits.max_retained_bytes = 0;
    let error = decode_tessellation_under_policy(MISSING_CONTAINER_ITEMS, limited)
        .expect_err("loss note bytes exceed the retained limit");
    assert!(
        matches!(&error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "step_tessellation_loss_notes"),
        "unexpected refusal: {error}"
    );
}

#[test]
fn tessellation_mesh_key_is_admitted_before_identity_composition() {
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(ONE_TRIANGLE, service).expect("service admits mesh identity");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "step_tessellation_mesh_key",
        |cap| {
            let mut policy = service;
            policy.limits.max_materialized_bytes = cap;
            let arena = DecodeArena::new();
            let (ctx, _) =
                DecodeContext::from_root_bytes(b"", &arena, &policy).expect("owner context");
            super::admitted_mesh_id(2, &ctx).map(|_| ())
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "step_tessellation_mesh_key")
    );
}

#[test]
fn tessellation_mesh_id_is_charged_before_retention() {
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(ONE_TRIANGLE, service).expect("service admits mesh identity");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "step_tessellation_mesh_id",
        |cap| {
            let mut policy = service;
            policy.limits.max_retained_bytes = cap;
            decode_tessellation_under_policy(ONE_TRIANGLE, policy)
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "step_tessellation_mesh_id")
    );
}

#[test]
fn tessellation_surface_id_is_reserved_before_lookup() {
    let records = "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=COMPLEX_TRIANGULATED_FACE('',#1,3,$,#10000000000000000000,(1,2,3),((1,2,3)),());
#10000000000000000000=TESSELLATED_ITEM();";
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(records, service)
        .expect("service admits the complex face surface lookup");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "step_tessellation_surface_id",
        |cap| {
            let mut policy = service;
            policy.limits.max_materialized_bytes = cap;
            let arena = DecodeArena::new();
            let (ctx, _) =
                DecodeContext::from_root_bytes(b"", &arena, &policy).expect("owner context");
            super::admitted_surface_id(10_000_000_000_000_000_000, &ctx).map(|_| ())
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "step_tessellation_surface_id")
    );
}

#[test]
fn tessellation_source_object_id_is_charged_before_formatting() {
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(ONE_TRIANGLE, service)
        .expect("service admits detached source association");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "step_tessellation_source_object_id",
        |limit| {
            let mut policy = service;
            policy.limits.max_retained_bytes = limit;
            decode_tessellation_under_policy(ONE_TRIANGLE, policy)
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "step_tessellation_source_object_id")
    );
}
const PLACED_ANNOTATION: &str = "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,3,((0.,0.,1.),(0.,0.,1.),(0.,0.,1.)),$,((1,2,3)));
#3=CARTESIAN_POINT('',(1.,0.,0.));
#4=DIRECTION('',(0.,0.,1.));
#5=DIRECTION('',(1.,0.,0.));
#6=AXIS2_PLACEMENT_3D('',#3,#4,#5);
#7=(GEOMETRIC_REPRESENTATION_ITEM() REPOSITIONED_TESSELLATED_ITEM(#6) REPRESENTATION_ITEM('repositioned') TESSELLATED_GEOMETRIC_SET((#2)) TESSELLATED_ITEM());
#8=TESSELLATED_ANNOTATION_OCCURRENCE('',(),#7);";

fn tessellation_fixture_records() -> &'static str {
    let source = std::str::from_utf8(include_bytes!(
        "../../../tests/fixtures/ap242_tessellation.p21"
    ))
    .expect("tessellation fixture is UTF-8");
    source
        .split_once("\nDATA;\n")
        .expect("fixture has DATA section")
        .1
        .split_once("\nENDSEC;")
        .expect("fixture has DATA end")
        .0
}
const PRODUCT_LINKS: &str = "#2=TESSELLATED_ITEM();
#10=PRODUCT_DEFINITION_SHAPE('','',$);
#11=SHAPE_DEFINITION_REPRESENTATION(#10,#20);
#20=TESSELLATED_SHAPE_REPRESENTATION('',(#2),$);
#21=SHAPE_REPRESENTATION('',(),$);
#22=SHAPE_REPRESENTATION_RELATIONSHIP('','',#20,#21);";
const PRODUCT_ITEMS_SOURCE: &str = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));ENDSEC;DATA;#2=TESSELLATED_ITEM();#10=PRODUCT_DEFINITION_SHAPE('','',$);#11=SHAPE_DEFINITION_REPRESENTATION(#10,#20);#20=TESSELLATED_SHAPE_REPRESENTATION('',(#2),$);ENDSEC;END-ISO-10303-21;";

fn product_link_refusal(admitted: u64, operation: &'static str) {
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));ENDSEC;DATA;{PRODUCT_LINKS}ENDSEC;END-ISO-10303-21;"
    );
    let (exchange, _) =
        with_service_context(source.as_bytes(), parse_inner).expect("product links parse");
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &service)
        .expect("service root admission");
    let (linked, _bytes) = super::product_linked_representations(&exchange, &ctx)
        .expect("service admits related product representations");
    assert_eq!(linked, [20, 21].into());
    let mut limited = service;
    limited.limits.max_collection_items = admitted;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &limited)
        .expect("limited root admission");
    let error = super::product_linked_representations(&exchange, &ctx)
        .expect_err("selected product collection limit refuses the next allocation");
    assert!(
        matches!(&error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == operation),
        "unexpected refusal: {error}"
    );
}

#[test]
fn tessellation_product_definitions_charge_before_insertion() {
    product_link_refusal(0, "step_tessellation_product_definitions");
}

#[test]
fn tessellation_product_representations_charge_before_insertion() {
    product_link_refusal(1, "step_tessellation_product_representations");
}

#[test]
fn tessellation_relationship_nodes_charge_before_insertion() {
    product_link_refusal(2, "step_tessellation_relationship_nodes");
}

#[test]
fn tessellation_relationship_edges_charge_before_insertion() {
    product_link_refusal(3, "step_tessellation_relationship_edges");
}

#[test]
fn tessellation_product_pending_charges_before_collection() {
    product_link_refusal(6, "step_tessellation_product_pending");
}

#[test]
fn tessellation_relationship_expansion_charges_the_new_representation() {
    product_link_refusal(7, "step_tessellation_product_representations");
}

#[test]
fn tessellation_relationship_expansion_charges_the_pending_item() {
    product_link_refusal(8, "step_tessellation_product_pending");
}

#[test]
fn tessellation_product_items_charge_before_insertion() {
    let (exchange, _) = with_service_context(PRODUCT_ITEMS_SOURCE.as_bytes(), parse_inner)
        .expect("product items parse");
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(PRODUCT_ITEMS_SOURCE.as_bytes(), &arena, &service)
            .expect("service root admission");
    let (linked, _links_bytes) = super::product_linked_representations(&exchange, &ctx)
        .expect("service admits product links");
    let (items, _item_bytes) = super::product_representation_items(&exchange, &linked, &ctx)
        .expect("service admits product items");
    assert_eq!(items, [2].into());
    let mut limited = service;
    limited.limits.max_collection_items = 4;
    let (ctx, _) =
        DecodeContext::from_root_bytes(PRODUCT_ITEMS_SOURCE.as_bytes(), &arena, &limited)
            .expect("limited root admission");
    let (linked, _links_bytes) = super::product_linked_representations(&exchange, &ctx)
        .expect("three prior product items fit");
    let error = super::product_representation_items(&exchange, &linked, &ctx)
        .expect_err("one product item exceeds four admitted items");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "step_tessellation_product_items")
    );
}

#[test]
fn tessellation_representation_items_charge_before_collection() {
    let (exchange, _) = with_service_context(PRODUCT_ITEMS_SOURCE.as_bytes(), parse_inner)
        .expect("product items parse");
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(PRODUCT_ITEMS_SOURCE.as_bytes(), &arena, &service)
            .expect("service root admission");
    let (linked, _links_bytes) = super::product_linked_representations(&exchange, &ctx)
        .expect("service admits product links");
    super::product_representation_items(&exchange, &linked, &ctx)
        .expect("service admits representation item vector");
    let mut limited = service;
    limited.limits.max_collection_items = 3;
    let (ctx, _) =
        DecodeContext::from_root_bytes(PRODUCT_ITEMS_SOURCE.as_bytes(), &arena, &limited)
            .expect("limited root admission");
    let (linked, _links_bytes) = super::product_linked_representations(&exchange, &ctx)
        .expect("three prior product items fit");
    let error = super::product_representation_items(&exchange, &linked, &ctx)
        .expect_err("one representation item exceeds three admitted items");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "step_tessellation_representation_items")
    );
}

#[test]
fn tessellation_declared_items_charge_before_insertion() {
    assert_tessellation_collection_refusal(
        ONE_TRIANGLE_IN_CONTAINER,
        5,
        "step_tessellation_declared_items",
    );
}

#[test]
fn tessellation_unresolved_containers_charge_before_insertion() {
    assert_tessellation_collection_refusal(
        ONE_TRIANGLE_IN_CONTAINER,
        6,
        "step_tessellation_unresolved_containers",
    );
}

#[test]
fn tessellation_active_items_charge_before_insertion() {
    assert_tessellation_collection_refusal(
        ONE_TRIANGLE_IN_CONTAINER,
        7,
        "step_tessellation_active_items",
    );
}

#[test]
fn tessellation_body_context_items_charge_before_insertion() {
    assert_tessellation_collection_refusal(
        ONE_TRIANGLE_IN_CONTAINER,
        8,
        "step_tessellation_body_context_items",
    );
}

#[test]
fn tessellation_item_body_entries_charge_before_insertion() {
    assert_tessellation_collection_refusal(
        ONE_TRIANGLE_IN_CONTAINER,
        9,
        "step_tessellation_item_body_entries",
    );
}

#[test]
fn tessellation_linked_bodies_charge_before_cloning() {
    assert_tessellation_collection_refusal(
        tessellation_fixture_records(),
        11,
        "step_tessellation_linked_bodies",
    );
}

#[test]
fn tessellation_body_candidates_charge_before_cloning() {
    assert_tessellation_collection_refusal(
        tessellation_fixture_records(),
        12,
        "step_tessellation_body_candidates",
    );
}

#[test]
fn tessellation_item_body_links_charge_before_cloning() {
    assert_tessellation_collection_refusal(
        tessellation_fixture_records(),
        16,
        "step_tessellation_item_body_links",
    );
}

#[test]
fn tessellation_claims_charge_before_insertion() {
    assert_tessellation_collection_refusal(
        ONE_TRIANGLE_IN_CONTAINER,
        10,
        "step_tessellation_claims",
    );
}

#[test]
fn tessellation_unresolved_placements_charge_before_insertion() {
    let source = std::str::from_utf8(include_bytes!(
        "tests/data/ts01_repositioned_unresolved_placement.p21"
    ))
    .expect("unresolved placement fixture is UTF-8");
    let records = source
        .split_once("\nDATA;\n")
        .expect("fixture has DATA section")
        .1
        .split_once("\nENDSEC;")
        .expect("fixture has DATA end")
        .0;
    assert_tessellation_collection_refusal(records, 5, "step_tessellation_unresolved_placements");
}

#[test]
fn tessellation_placement_entries_charge_before_insertion() {
    assert_tessellation_collection_refusal(
        PLACED_ANNOTATION,
        9,
        "step_tessellation_placement_entries",
    );
}

#[test]
fn tessellation_placements_charge_before_push() {
    assert_tessellation_collection_refusal(PLACED_ANNOTATION, 10, "step_tessellation_placements");
}

#[test]
fn tessellation_placed_vertices_charge_before_projection() {
    assert_tessellation_collection_refusal(
        PLACED_ANNOTATION,
        27,
        "step_tessellation_placed_vertices",
    );
}

#[test]
fn tessellation_placed_normals_charge_before_projection() {
    assert_tessellation_collection_refusal(
        PLACED_ANNOTATION,
        30,
        "step_tessellation_placed_normals",
    );
}

#[test]
fn tessellation_coordinate_rows_charge_before_collection() {
    assert_tessellation_collection_refusal(ONE_TRIANGLE, 2, "step_tessellation_coordinate_rows");
}

#[test]
fn tessellation_coordinate_lists_charge_before_insertion() {
    assert_tessellation_collection_refusal(ONE_TRIANGLE, 3, "step_tessellation_coordinate_lists");
}

#[test]
fn tessellation_coordinate_rows_reserve_temporary_bytes_before_collection() {
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(ONE_TRIANGLE, service)
        .expect("service admits coordinate rows");
    let mut limited = service;
    limited.limits.max_materialized_bytes = u64_from_index(3 * std::mem::size_of::<Point3>() - 1);
    let error = decode_tessellation_under_policy(ONE_TRIANGLE, limited)
        .expect_err("coordinate row bytes exceed the selected temporary allowance");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "step_tessellation_coordinate_rows")
    );
}

#[test]
fn tessellation_triangle_rows_reserve_temporary_bytes_before_collection() {
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(ONE_TRIANGLE, service).expect("service admits triangle rows");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "step_tessellation_triangle_rows",
        |cap| {
            let mut policy = service;
            policy.limits.max_materialized_bytes = cap;
            decode_tessellation_under_policy(ONE_TRIANGLE, policy)
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "step_tessellation_triangle_rows")
    );
}

#[test]
fn tessellation_container_items_reserve_temporary_bytes_before_collection() {
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(ONE_TRIANGLE_IN_CONTAINER, service)
        .expect("service admits one container item");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "step_tessellation_container_items",
        |cap| {
            let mut policy = service;
            policy.limits.max_materialized_bytes = cap;
            decode_tessellation_under_policy(ONE_TRIANGLE_IN_CONTAINER, policy)
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "step_tessellation_container_items")
    );
}

#[test]
fn tessellation_pnindex_charges_before_collection() {
    assert_tessellation_collection_refusal(
        ONE_TRIANGLE_WITH_PNINDEX,
        7,
        "step_tessellation_pnindex",
    );
}

#[test]
fn tessellation_pn_vertices_charge_before_projection() {
    assert_tessellation_collection_refusal(
        ONE_TRIANGLE_WITH_PNINDEX,
        10,
        "step_tessellation_pn_vertices",
    );
}

#[test]
fn tessellation_pn_triangles_charge_before_projection() {
    assert_tessellation_collection_refusal(
        ONE_TRIANGLE_WITH_PNINDEX,
        11,
        "step_tessellation_pn_triangles",
    );
}

#[test]
fn tessellation_coordinate_indices_charge_before_insertion() {
    assert_tessellation_collection_refusal(ONE_TRIANGLE, 7, "step_tessellation_coordinate_indices");
}

#[test]
fn tessellation_local_index_charges_before_projection() {
    assert_tessellation_collection_refusal(ONE_TRIANGLE, 10, "step_tessellation_local_index");
}

#[test]
fn tessellation_local_vertices_charge_before_projection() {
    assert_tessellation_collection_refusal(ONE_TRIANGLE, 13, "step_tessellation_local_vertices");
}

#[test]
fn tessellation_local_triangles_charge_before_projection() {
    assert_tessellation_collection_refusal(ONE_TRIANGLE, 14, "step_tessellation_local_triangles");
}

#[test]
fn tessellation_normal_rows_charge_before_collection() {
    let records = "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,3,((1.,0.,0.),(0.,1.,0.),(0.,0.,1.)),$,((1,2,3)));";
    assert_tessellation_collection_refusal(records, 17, "step_tessellation_normal_rows");
}

#[test]
fn tessellation_projected_normals_charge_before_collection() {
    let records = "#1=COORDINATES_LIST('',4,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.),(1.,1.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,4,((1.,0.,0.),(0.,1.,0.),(0.,0.,1.),(0.,0.,-1.)),$,((1,2,3)));";
    assert_tessellation_collection_refusal(records, 22, "step_tessellation_projected_normals");
}

#[test]
fn tessellation_shaded_rows_charge_before_pairing() {
    let records = "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,3,((1.,0.,0.),(0.,1.,0.),(0.,0.,1.)),$,((1,2,3)));";
    assert_tessellation_collection_refusal(records, 20, "step_tessellation_shaded_rows");
}

#[test]
fn tessellation_admitted_vertices_charge_before_conversion() {
    assert_tessellation_collection_refusal(ONE_TRIANGLE, 17, "step_tessellation_admitted_vertices");
}

#[test]
fn tessellation_admitted_normals_charge_before_conversion() {
    let records = "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,3,((1.,0.,0.),(0.,1.,0.),(0.,0.,1.)),$,((1,2,3)));";
    assert_tessellation_collection_refusal(records, 26, "step_tessellation_admitted_normals");
}

#[test]
fn tessellation_validation_triangles_charge_before_copy() {
    assert_tessellation_collection_refusal(
        ONE_TRIANGLE,
        18,
        "step_tessellation_validation_triangles",
    );
}

#[test]
fn tessellation_mesh_list_charges_before_push() {
    assert_tessellation_collection_refusal(ONE_TRIANGLE, 20, "step_tessellation_mesh_list");
}

#[test]
fn tessellation_mesh_body_refuses_retained_limit_before_copy() {
    let body = cadmpeg_ir::ids::BodyId::mint("step:data:body#1").expect("valid body identity");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64_from_index(body.as_str().len()) - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits selected policy");
    let error = body
        .try_clone_for_decode(&ctx, "step_tessellation_mesh_body")
        .expect_err("body copy exceeds retained limit");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "step_tessellation_mesh_body"
    ));
}

#[test]
fn tessellation_mesh_entity_is_admitted_before_creation() {
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(ONE_TRIANGLE, service).expect("service admits mesh entity");
    let mut limited = service;
    limited.limits.max_entities = 0;
    let error = decode_tessellation_under_policy(ONE_TRIANGLE, limited)
        .expect_err("one mesh entity exceeds zero admitted entities");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::Entities && limit.operation == "step_tessellation_mesh_entity")
    );
}

#[test]
fn tessellation_ir_mesh_charges_retained_bytes_before_creation() {
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(ONE_TRIANGLE, service).expect("service admits mesh bytes");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "step_tessellation_ir_mesh",
        |cap| {
            let mut policy = service;
            policy.limits.max_retained_bytes = cap;
            decode_tessellation_under_policy(ONE_TRIANGLE, policy)
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "step_tessellation_ir_mesh")
    );
}

#[test]
fn tessellation_local_triangles_commit_retained_bytes_before_push() {
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(ONE_TRIANGLE, service).expect("service admits triangle bytes");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "step_tessellation_local_triangles",
        |cap| {
            let mut policy = service;
            policy.limits.max_retained_bytes = cap;
            decode_tessellation_under_policy(ONE_TRIANGLE, policy)
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "step_tessellation_local_triangles")
    );
}

#[test]
fn tessellation_pn_triangles_commit_retained_bytes_before_push() {
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(ONE_TRIANGLE_WITH_PNINDEX, service)
        .expect("service admits PN triangle bytes");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "step_tessellation_pn_triangles",
        |cap| {
            let mut policy = service;
            policy.limits.max_retained_bytes = cap;
            decode_tessellation_under_policy(ONE_TRIANGLE_WITH_PNINDEX, policy)
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "step_tessellation_pn_triangles")
    );
}

#[test]
fn tessellation_pnindex_reserves_temporary_bytes_before_collection() {
    let values = Value::List(vec![
        Value::Integer(1),
        Value::Integer(2),
        Value::Integer(3),
    ]);
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &service).expect("service root admission");
    assert!(super::index_list(Some(&values), &ctx)
        .expect("service admits PNINDEX")
        .is_some());
    let mut limited = service;
    limited.limits.max_materialized_bytes = u64_from_index(3 * std::mem::size_of::<u32>() - 1);
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &limited).expect("limited root admission");
    let error = super::index_list(Some(&values), &ctx)
        .expect_err("three indices exceed the temporary allowance");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "step_tessellation_pnindex")
    );
}

fn one_complex_strip() -> Value {
    Value::List(vec![Value::List(vec![
        Value::Integer(1),
        Value::Integer(2),
        Value::Integer(3),
    ])])
}

#[test]
fn complex_tessellation_rows_reserve_temporary_bytes_before_collection() {
    let strips = one_complex_strip();
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &service).expect("service root admission");
    super::index_rows(
        Some(&strips),
        "COMPLEX_TRIANGULATED_SURFACE_SET",
        1,
        "strip",
        &ctx,
    )
    .expect("service admits strip row");
    let mut limited = service;
    limited.limits.max_materialized_bytes = u64_from_index(std::mem::size_of::<Vec<u32>>() - 1);
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &limited).expect("limited root admission");
    let error = super::index_rows(
        Some(&strips),
        "COMPLEX_TRIANGULATED_SURFACE_SET",
        1,
        "strip",
        &ctx,
    )
    .expect_err("outer strip row exceeds the temporary allowance");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "step_complex_tessellation_rows")
    );
}

#[test]
fn complex_tessellation_indices_reserve_temporary_bytes_before_collection() {
    let strips = one_complex_strip();
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &service).expect("service root admission");
    super::index_rows(
        Some(&strips),
        "COMPLEX_TRIANGULATED_SURFACE_SET",
        1,
        "strip",
        &ctx,
    )
    .expect("service admits three strip indices");
    let mut limited = service;
    limited.limits.max_materialized_bytes =
        u64_from_index(std::mem::size_of::<Vec<u32>>() + 3 * std::mem::size_of::<u32>() - 1);
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &limited).expect("limited root admission");
    let error = super::index_rows(
        Some(&strips),
        "COMPLEX_TRIANGULATED_SURFACE_SET",
        1,
        "strip",
        &ctx,
    )
    .expect_err("three strip indices exceed the temporary allowance");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "step_complex_tessellation_indices")
    );
}

#[test]
fn complex_tessellation_triangles_reserve_temporary_bytes_before_collection() {
    let strips = one_complex_strip();
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &service).expect("service root admission");
    assert!(super::complex_triangles(
        Some(&strips),
        None,
        "COMPLEX_TRIANGULATED_SURFACE_SET",
        1,
        &ctx
    )
    .expect("service admits one triangle")
    .is_some());
    let mut limited = service;
    limited.limits.max_materialized_bytes = u64_from_index(
        std::mem::size_of::<Vec<u32>>()
            + 3 * std::mem::size_of::<u32>()
            + std::mem::size_of::<[u32; 3]>()
            - 1,
    );
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &limited).expect("limited root admission");
    let error = super::complex_triangles(
        Some(&strips),
        None,
        "COMPLEX_TRIANGULATED_SURFACE_SET",
        1,
        &ctx,
    )
    .expect_err("one complex triangle exceeds the temporary allowance");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "step_complex_tessellation_triangles")
    );
}

#[test]
fn tessellation_invalid_normal_rows_are_reported_and_omitted() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "((1.,0.,0.),(0.,1.,0.),(0.,0.,1.),(0.,0.,-1.))",
        "((0.,0.,0.),(0.,1.,0.),(0.,0.,1.),(0.,0.,-1.))",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode invalid normal-row tessellation");
    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#7")
        .expect("invalid-normal tessellation");
    assert!(mesh.vertex_normals().is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::DecodeWarning.kind()
            && loss
                .message
                .contains("COMPLEX_TRIANGULATED_FACE #7 has invalid normal rows; normals omitted")
    }));
}

#[test]
fn tessellation_empty_normal_rows_mean_unshaded_without_a_warning() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#4=TRIANGULATED_FACE('triangle',#3,3,((0.,0.,1.)),$,(),((1,2,3)));",
        "#4=TRIANGULATED_FACE('triangle',#3,3,(),$,(),((1,2,3)));",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode empty-normal tessellation");
    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#4")
        .expect("empty-normal tessellation");
    assert!(mesh.vertex_normals().is_empty());
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::DecodeWarning.kind()
            && loss
                .message
                .contains("TRIANGULATED_FACE #4 has invalid normal rows")
    }));
}

#[test]
pub(crate) fn decode_transfers_ap242_one_based_tessellation_indices() {
    let bytes = include_bytes!("../../../tests/fixtures/ap242_tessellation.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode AP242 tessellation");

    assert_eq!(result.ir().model.tessellations.len(), 2);
    assert_eq!(result.ir().model.bodies.len(), 1);
    let mesh = &result.ir().model.tessellations[0];
    assert_eq!(mesh.vertices().len(), 3);
    assert!((mesh.vertices()[1].x - 10.0).abs() < EPS_SAME_POINT);
    assert_eq!(mesh.triangles(), [[0, 1, 2]]);
    assert_eq!(mesh.vertex_normals().len(), 3);
    assert_eq!(
        mesh.body.as_ref().map(cadmpeg_ir::ids::BodyId::as_str),
        Some("step:data:body#38")
    );
    let complex = result
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.as_str().ends_with("#7"))
        .unwrap();
    assert_eq!(complex.triangles(), [[0, 1, 2], [2, 1, 3], [0, 1, 3]]);
    assert_point3_close(complex.vertices()[0].get(), Point3::new(10.0, 10.0, 0.0));
    assert_eq!(complex.vertex_normals().len(), 4);
    assert!((complex.vertex_normals()[0].x - 1.0).abs() < EPS_SAME_POINT);
    assert!(result
        .ir()
        .model
        .appearance_bindings
        .iter()
        .any(|binding| matches!(
            binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Tessellation(_)
        )));
    assert!(result
        .report()
        .notes
        .iter()
        .any(|note| note
            == "geometric validation surface area triangle sheet: expected 50, tessellation approximation 50"));
    assert!(result.report().notes.iter().any(|note| note.starts_with(
        "geometric validation centroid triangle centroid: expected (3.333333333333333,3.333333333333333,0), tessellation approximation distance"
    )));
    assert!(result.report().notes.iter().any(
        |note| note == "geometric validation volume open sheet volume: expected 0, tessellation approximation 0"
    ));
    assert!(!result.report().losses.iter().any(|loss| loss
        .message
        .contains("does not match transferred tessellation")));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_tessellated_face_retains_its_surface_carrier() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#7=COMPLEX_TRIANGULATED_FACE('strip and fan',#6,4,((1.,0.,0.),(0.,1.,0.),(0.,0.,1.),(0.,0.,-1.)),$,(4,3,2,1),((1,2,3,4)),((1,2,4)));",
        "#7=COMPLEX_TRIANGULATED_FACE('strip and fan',#6,4,((1.,0.,0.),(0.,1.,0.),(0.,0.,1.),(0.,0.,-1.)),#90,(4,3,2,1),((1,2,3,4)),((1,2,4)));",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#90=PLANE('',#34);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode tessellated face surface");

    let surface = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#90")
        .expect("tessellated face surface");
    assert_eq!(
        surface
            .source_object
            .as_ref()
            .map(|source| source.object_id.as_str()),
        Some("#7")
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_tessellation_partials_transfer_coordinates_and_indices() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#3=COORDINATES_LIST('triangle coordinates',3,((0.,0.,0.),(10.,0.,0.),(0.,10.,0.)));",
        "#3=(COORDINATES_LIST(3,((0.,0.,0.),(10.,0.,0.),(0.,10.,0.))) GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('triangle coordinates') TESSELLATED_ITEM());",
    )
    .replace(
        "#4=TRIANGULATED_FACE('triangle',#3,3,((0.,0.,1.)),$,(),((1,2,3)));",
        "#4=(GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('triangle') TESSELLATED_FACE(#3,3,((0.,0.,1.)),$) TESSELLATED_ITEM() TESSELLATED_STRUCTURED_ITEM() TRIANGULATED_FACE((),((1,2,3))));",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex tessellation partials");

    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.as_str().ends_with("#4"))
        .expect("complex tessellated face");
    assert_eq!(mesh.vertices().len(), 3);
    assert_point3_close(mesh.vertices()[1].get(), Point3::new(10.0, 0.0, 0.0));
    assert_eq!(mesh.triangles(), [[0, 1, 2]]);
    assert_eq!(mesh.vertex_normals().len(), 3);
    assert_eq!(
        mesh.body.as_ref().map(cadmpeg_ir::ids::BodyId::as_str),
        Some("step:data:body#38")
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn tessellation_geometry_sets_transfer_flag_and_invalid_pnindex_is_rejected() {
    let result = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!(
                "../../../tests/fixtures/ap242_tessellation.p21"
            )),
            &DecodeOptions::default(),
        )
        .expect("decode tessellation fixture");
    assert!(result.report().geometry_transferred());
    assert!(result
        .ir()
        .model
        .tessellations
        .iter()
        .any(|mesh| mesh.id.as_str() == "step:tessellation:mesh#7" && mesh.body.is_none()));
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemUndeclared.kind()
            && loss.message.contains("mesh retained as detached")
    }));

    let malformed = decode_inline(
        "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,3,$,('bad'),((1,2,3)));",
    );
    assert!(malformed.ir().model.tessellations.is_empty());
    assert!(malformed
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("invalid pnindex")));
}

#[test]
fn product_linked_bodyless_tessellated_representation_declares_mesh() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#80=PRODUCT_DEFINITION_SHAPE('', '', #81);\n#81=PRODUCT_DEFINITION('', '', '', #82);\n#82=PRODUCT_DEFINITION_FORMATION('', '', #83);\n#83=PRODUCT('', '', '', ());\n#84=SHAPE_DEFINITION_REPRESENTATION(#80,#8);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode product-linked bodyless tessellated representation");
    assert!(decoded
        .ir()
        .model
        .tessellations
        .iter()
        .any(|mesh| mesh.id.as_str() == "step:tessellation:mesh#7" && mesh.body.is_none()));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemUndeclared.kind()
            && loss.message.contains("tessellation item #7")
    }));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemBodyUnresolved.kind()
            && loss.message.contains("tessellation item #7")
    }));
}

#[test]
fn generic_representation_relationship_does_not_admit_product_tessellation() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#39=MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#38),#2);",
        "#39=SHAPE_REPRESENTATION('carrier',(#10),#2);",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#80=PRODUCT_DEFINITION_SHAPE('', '', #81);\n#81=PRODUCT_DEFINITION('', '', '', #82);\n#82=PRODUCT_DEFINITION_FORMATION('', '', #83);\n#83=PRODUCT('', '', '', ());\n#84=SHAPE_DEFINITION_REPRESENTATION(#80,#39);\n#90=REPRESENTATION_RELATIONSHIP('generic bridge','',#39,#8);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode generic representation relationship witness");
    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#7")
        .expect("generic bridge tessellation");
    assert!(mesh.body.is_none());
    assert_eq!(
        mesh.source_object
            .as_ref()
            .map(|source| source.object_id.as_str()),
        Some("#7")
    );
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemUndeclared.kind()
            && loss.message.contains("tessellation item #7")
    }));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemBodyUnresolved.kind()
            && loss.message.contains("tessellation item #7")
    }));
}

#[test]
fn shape_representation_relationship_admits_product_tessellation() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#39=MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#38),#2);",
        "#39=SHAPE_REPRESENTATION('carrier',(#10),#2);",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#80=PRODUCT_DEFINITION_SHAPE('', '', #81);\n#81=PRODUCT_DEFINITION('', '', '', #82);\n#82=PRODUCT_DEFINITION_FORMATION('', '', #83);\n#83=PRODUCT('', '', '', ());\n#84=SHAPE_DEFINITION_REPRESENTATION(#80,#39);\n#90=(REPRESENTATION_RELATIONSHIP('typed shape bridge','',#39,#8) SHAPE_REPRESENTATION_RELATIONSHIP());\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode typed shape representation relationship witness");
    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#7")
        .expect("typed bridge tessellation");
    assert!(mesh.body.is_none());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemBodyUnresolved.kind()
            && loss.message.contains("tessellation item #7")
    }));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemUndeclared.kind()
            && loss.message.contains("tessellation item #7")
    }));
}

#[test]
fn accuracy_parameter_representation_uses_inherited_items_and_context() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#8=TESSELLATED_SHAPE_REPRESENTATION('complex mesh',(#7),#2);",
        "#8=TESSELLATED_SHAPE_REPRESENTATION_WITH_ACCURACY_PARAMETERS('complex mesh',(#7),#2,(CHORDAL_DEVIATION(0.1)));",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#90=SHAPE_REPRESENTATION_RELATIONSHIP('', '', #39, #8);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode accuracy-parameter tessellated representation");

    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#7")
        .expect("accuracy-parameter tessellation");
    assert_eq!(
        mesh.body.as_ref().map(cadmpeg_ir::ids::BodyId::as_str),
        Some("step:data:body#38")
    );
    assert!(!decoded.report().losses.iter().any(|loss| {
        (loss.code == StepLossCode::TessellationItemUndeclared.kind()
            || loss.code == StepLossCode::TessellationItemBodyUnresolved.kind())
            && loss.message.contains("tessellation item #7")
    }));
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP native namespace")
        .iter()
        .any(|record| {
            record.id.as_str()
                == "step:data:tessellated_shape_representation_with_accuracy_parameters#8"
        }));
}

#[test]
fn repositioned_annotation_mesh_transfers_one_placement() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#80=CARTESIAN_POINT('',(100.,200.,300.));\n#81=DIRECTION('',(0.,0.,1.));\n#82=DIRECTION('',(1.,0.,0.));\n#83=AXIS2_PLACEMENT_3D('annotation placement',#80,#81,#82);\n#84=(GEOMETRIC_REPRESENTATION_ITEM() REPOSITIONED_TESSELLATED_ITEM(#83) REPRESENTATION_ITEM('repositioned mesh') TESSELLATED_GEOMETRIC_SET((#7)) TESSELLATED_ITEM());\n#85=TESSELLATED_ANNOTATION_OCCURRENCE('repositioned mesh',(),#84);\n#86=(GEOMETRIC_REPRESENTATION_ITEM() REPOSITIONED_TESSELLATED_ITEM(#83) REPRESENTATION_ITEM('repositioned exact mesh') TESSELLATED_GEOMETRIC_SET((#4)) TESSELLATED_ITEM());\n#87=TESSELLATED_ANNOTATION_OCCURRENCE('repositioned exact mesh',(),#86);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode repositioned annotation tessellation");
    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#7")
        .expect("repositioned annotation mesh");
    assert_point3_close(mesh.vertices()[0].get(), Point3::new(110.0, 210.0, 300.0));
    assert_vector3_close(mesh.vertex_normals()[0].get(), Vector3::new(1.0, 0.0, 0.0));
    assert!(mesh.body.is_none());
    let exact_mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#4")
        .expect("exact body mesh");
    assert_point3_close(exact_mesh.vertices()[0].get(), Point3::new(0.0, 0.0, 0.0));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemUndeclared.kind()
            && loss.message.contains("tessellation item #7")
    }));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemBodyUnresolved.kind()
            && loss.message.contains("tessellation item #7")
    }));
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP native namespace")
        .iter()
        .any(|record| record.id.as_str().ends_with("#84")));
}

#[test]
fn repositioned_annotation_mesh_preserves_extreme_source_normals() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "((1.,0.,0.),(0.,1.,0.),(0.,0.,1.),(0.,0.,-1.))",
        "((1.E308,0.,0.),(0.,1.,0.),(0.,0.,1.),(0.,0.,-1.))",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#80=CARTESIAN_POINT('',(100.,200.,300.));\n#81=DIRECTION('',(0.,0.,1.));\n#82=DIRECTION('',(1.,0.,0.));\n#83=AXIS2_PLACEMENT_3D('annotation placement',#80,#81,#82);\n#84=(GEOMETRIC_REPRESENTATION_ITEM() REPOSITIONED_TESSELLATED_ITEM(#83) REPRESENTATION_ITEM('repositioned mesh') TESSELLATED_GEOMETRIC_SET((#7)) TESSELLATED_ITEM());\n#85=TESSELLATED_ANNOTATION_OCCURRENCE('repositioned mesh',(),#84);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode repositioned extreme-normal tessellation");
    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#7")
        .expect("repositioned extreme-normal mesh");
    assert_point3_close(mesh.vertices()[0].get(), Point3::new(110.0, 210.0, 300.0));
    assert_vector3_close(mesh.vertex_normals()[0].get(), Vector3::new(1.0, 0.0, 0.0));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::DecodeWarning.kind()
            && loss
                .message
                .contains("normal placement could not produce finite unit normals")
    }));
}

#[test]
fn repositioned_annotation_mesh_with_invalid_or_missing_placement_keeps_source_coordinates() {
    for (source, unresolved_placement) in [
        (
            include_bytes!("tests/data/ts01_repositioned_missing_placement.p21").as_slice(),
            false,
        ),
        (
            include_bytes!("tests/data/ts01_repositioned_missing_placement_slot.p21").as_slice(),
            false,
        ),
        (
            include_bytes!("tests/data/ts01_repositioned_unresolved_placement.p21").as_slice(),
            true,
        ),
    ] {
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("decode invalid repositioned placement tessellation");
        let mesh = decoded
            .ir()
            .model
            .tessellations
            .iter()
            .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#4")
            .expect("invalid-placement tessellation");
        assert_point3_close(mesh.vertices()[1].get(), Point3::new(10.0, 0.0, 0.0));
        assert!(decoded.report().losses.iter().any(|loss| {
            loss.code == StepLossCode::TessellationPlacementUnresolved.kind()
                && loss.message.contains("repositioned tessellated item #5")
                && loss.message.contains("unresolved placement is not applied")
        }));
        assert!(decoded
            .ir()
            .native_unknowns("step")
            .expect("STEP native namespace")
            .iter()
            .any(|record| record.id.as_str().ends_with("#5")));
        if unresolved_placement {
            assert!(decoded
                .ir()
                .native_unknowns("step")
                .expect("STEP native namespace")
                .iter()
                .any(|record| record.id.as_str() == "step:data:axis2_placement_3d#99"));
        }
        let validation =
            cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
                .expect("resource allocation did not fail");
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn unresolved_outer_repositioning_preserves_inner_valid_placement() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#80=CARTESIAN_POINT('',(100.,200.,300.));\n#81=DIRECTION('',(0.,0.,1.));\n#82=DIRECTION('',(1.,0.,0.));\n#83=AXIS2_PLACEMENT_3D('inner placement',#80,#81,#82);\n#84=(GEOMETRIC_REPRESENTATION_ITEM() REPOSITIONED_TESSELLATED_ITEM(#83) REPRESENTATION_ITEM('inner mesh') TESSELLATED_GEOMETRIC_SET((#7)) TESSELLATED_ITEM());\n#85=TESSELLATED_ANNOTATION_OCCURRENCE('inner mesh',(),#84);\n#88=(GEOMETRIC_REPRESENTATION_ITEM() REPOSITIONED_TESSELLATED_ITEM(#85) REPRESENTATION_ITEM('unresolved outer placement') TESSELLATED_GEOMETRIC_SET((#84)) TESSELLATED_ITEM());\n#89=TESSELLATED_ANNOTATION_OCCURRENCE('unresolved outer placement',(),#88);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode nested unresolved repositioning");
    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#7")
        .expect("nested repositioned annotation mesh");
    assert_point3_close(mesh.vertices()[0].get(), Point3::new(110.0, 210.0, 300.0));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationPlacementUnresolved.kind()
            && loss.message.contains("repositioned tessellated item #88")
            && loss.message.contains("unresolved placement is not applied")
    }));
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP native namespace")
        .iter()
        .any(|record| record.id.as_str().ends_with("#88")));
}

#[test]
fn repositioned_annotation_mesh_rejects_conflicting_placements() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#80=CARTESIAN_POINT('',(100.,200.,300.));\n#81=DIRECTION('',(0.,0.,1.));\n#82=DIRECTION('',(1.,0.,0.));\n#83=AXIS2_PLACEMENT_3D('first placement',#80,#81,#82);\n#84=(GEOMETRIC_REPRESENTATION_ITEM() REPOSITIONED_TESSELLATED_ITEM(#83) REPRESENTATION_ITEM('first repositioned mesh') TESSELLATED_GEOMETRIC_SET((#7)) TESSELLATED_ITEM());\n#85=TESSELLATED_ANNOTATION_OCCURRENCE('first repositioned mesh',(),#84);\n#86=CARTESIAN_POINT('',(-100.,-200.,-300.));\n#87=AXIS2_PLACEMENT_3D('second placement',#86,#81,#82);\n#88=(GEOMETRIC_REPRESENTATION_ITEM() REPOSITIONED_TESSELLATED_ITEM(#87) REPRESENTATION_ITEM('second repositioned mesh') TESSELLATED_GEOMETRIC_SET((#7)) TESSELLATED_ITEM());\n#89=TESSELLATED_ANNOTATION_OCCURRENCE('second repositioned mesh',(),#88);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode conflicting repositioned annotation tessellation");
    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#7")
        .expect("conflicting repositioned annotation mesh");
    assert_point3_close(mesh.vertices()[0].get(), Point3::new(10.0, 10.0, 0.0));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationPlacementAmbiguous.kind()
            && loss.message.contains("tessellation item #7")
    }));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemUndeclared.kind()
            && loss.message.contains("tessellation item #7")
    }));
}

#[test]
fn tessellated_shape_relationship_supplies_exact_body_owner() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#40=TESSELLATED_SHELL('sheet mesh',(#4),#37);",
        "#40=TESSELLATED_SHELL('sheet mesh',(#4),$);",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#90=SHAPE_REPRESENTATION_RELATIONSHIP('','',#39,#5);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode related tessellation");

    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#4")
        .expect("related mesh");
    assert_eq!(
        mesh.body.as_ref().map(cadmpeg_ir::ids::BodyId::as_str),
        Some("step:data:body#38")
    );
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemBodyUnresolved.kind()
            && loss.message.contains("tessellation item #4")
    }));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::DecodeWarning.kind()
            && loss.message.contains("TESSELLATED_SHELL #40")
    }));
}

#[test]
fn direct_tessellated_representation_item_uses_exact_body_relationship() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#90=SHAPE_REPRESENTATION_RELATIONSHIP('', '', #39, #8);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode directly represented tessellation");

    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#7")
        .expect("directly represented mesh");
    assert_eq!(
        mesh.body.as_ref().map(cadmpeg_ir::ids::BodyId::as_str),
        Some("step:data:body#38")
    );
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemUndeclared.kind()
            && loss.message.contains("tessellation item #7")
    }));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemBodyUnresolved.kind()
            && loss.message.contains("tessellation item #7")
    }));
}

#[test]
fn nested_tessellated_body_container_uses_exact_body_link() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#40=TESSELLATED_SHELL('sheet mesh',(#4),#37);",
        "#40=TESSELLATED_SHELL('sheet mesh',(#91),#37);",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#91=TESSELLATED_GEOMETRIC_SET('nested mesh',(#4));\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode nested body-container tessellation");

    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#4")
        .expect("nested body-container mesh");
    assert_eq!(
        mesh.body.as_ref().map(cadmpeg_ir::ids::BodyId::as_str),
        Some("step:data:body#38")
    );
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemBodyUnresolved.kind()
            && loss.message.contains("tessellation item #4")
    }));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemUndeclared.kind()
            && loss.message.contains("tessellation item #4")
    }));
}

#[test]
fn nested_tessellated_representation_item_uses_exact_body_relationship() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#8=TESSELLATED_SHAPE_REPRESENTATION('complex mesh',(#7),#2);",
        "#8=TESSELLATED_SHAPE_REPRESENTATION('complex mesh',(#91),#2);",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#91=TESSELLATED_GEOMETRIC_SET('nested mesh',(#7));\n#90=SHAPE_REPRESENTATION_RELATIONSHIP('', '', #39, #8);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode nested represented tessellation");

    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#7")
        .expect("nested represented mesh");
    assert_eq!(
        mesh.body.as_ref().map(cadmpeg_ir::ids::BodyId::as_str),
        Some("step:data:body#38")
    );
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemUndeclared.kind()
            && loss.message.contains("tessellation item #7")
    }));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemBodyUnresolved.kind()
            && loss.message.contains("tessellation item #7")
    }));
}

#[test]
fn complex_tessellated_shape_representation_inherits_items() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#5=TESSELLATED_SHAPE_REPRESENTATION('mesh',(#40),#2);",
        "#5=(CHARACTERIZED_REPRESENTATION() REPRESENTATION('mesh',(#40),#2) TESSELLATED_SHAPE_REPRESENTATION());",
    )
    .replace(
        "#40=TESSELLATED_SHELL('sheet mesh',(#4),#37);",
        "#40=TESSELLATED_SHELL('sheet mesh',(#4),$);",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#90=SHAPE_REPRESENTATION_RELATIONSHIP('','',#39,#5);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex tessellated representation");

    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#4")
        .expect("complex representation mesh");
    assert_eq!(
        mesh.body.as_ref().map(cadmpeg_ir::ids::BodyId::as_str),
        Some("step:data:body#38")
    );
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemBodyUnresolved.kind()
            && loss.message.contains("tessellation item #4")
    }));
}

#[test]
fn shared_tessellation_item_is_not_assigned_to_an_arbitrary_body() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#80=WIRE_SHELL('',(#32));\n#81=SHELL_BASED_WIREFRAME_MODEL('',(#80));\n#82=TESSELLATED_SHELL('shared mesh',(#4),#80);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode shared tessellation item");
    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#4")
        .expect("shared mesh");
    assert!(mesh.body.is_none());
    assert!(
        decoded.report().losses.iter().any(|loss| {
            loss.code == StepLossCode::TessellationItemBodyUnresolved.kind()
                && loss.message.contains("multiple candidate bodies")
        }),
        "{:#?}",
        decoded.report().losses
    );
}

#[test]
fn malformed_complex_strip_refuses_the_aggregate() {
    let error = decode_inline_result(
        "#1=COORDINATES_LIST('',4,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.),(1.,1.,0.)));
#2=COMPLEX_TRIANGULATED_SURFACE_SET('',#1,4,$,$,((1,2),(1,2,3,4)),());",
    )
    .expect_err("a short strip must refuse the aggregate");
    assert!(
        matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(message)) if message.contains("#2 strip row 1"))
    );
}

#[test]
fn tri_ext1_2_nonreference_tessellation_item_refuses_the_aggregate() {
    let error = decode_inline_result(
        "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,3,$,$,((1,2,3)));
#3=TESSELLATED_SOLID('',(#2,$),$);",
    )
    .expect_err("a non-reference item must refuse the aggregate");
    assert!(
        matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(message)) if message.contains("TESSELLATED_SOLID #3 item 2"))
    );
}

#[test]
fn malformed_complex_fan_refuses_the_aggregate() {
    let error = decode_inline_result(
        "#1=COORDINATES_LIST('',4,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.),(1.,1.,0.)));
#2=COMPLEX_TRIANGULATED_SURFACE_SET('',#1,4,$,$,(),((1,2),(1,2,3,4)));",
    )
    .expect_err("a short fan must refuse the aggregate");
    assert!(
        matches!(error, cadmpeg_ir::DecodeFailure::Codec(CodecError::Malformed(message)) if message.contains("#2 fan row 1"))
    );
}

#[test]
fn nested_tessellation_container_nonreference_item_is_refused() {
    let error = decode_inline_result(
        "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,3,$,$,((1,2,3)));
#3=TESSELLATED_GEOMETRIC_SET('',(#2,$));
#4=TESSELLATED_SOLID('',(#3),$);",
    )
    .expect_err("nested non-reference item must refuse the aggregate");
    assert!(
        matches!(error, cadmpeg_ir::DecodeFailure::Codec(CodecError::Malformed(message)) if message.contains("TESSELLATED_GEOMETRIC_SET #3 item 2"))
    );
}

#[test]
fn tessellation_association_depth_uses_the_session_limit() {
    let mut records = String::from(
        "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));\n#2=TRIANGULATED_SURFACE_SET('',#1,3,$,$,((1,2,3)));\n",
    );
    for id in 3..=15 {
        writeln!(
            records,
            "#{id}=TESSELLATED_GEOMETRIC_SET('',(#{}));",
            id - 1
        )
        .expect("write nested set");
    }
    records.push_str("#16=TESSELLATED_SOLID('',(#15),$);");
    let service = DecodePolicy::service();
    let accepted = decode_tessellation_under_policy(&records, service)
        .expect("service depth admits the association chain");
    assert_eq!(accepted.model.tessellations.len(), 1);
    let mut limited = service;
    limited.limits.max_recursion_depth = 13;
    let error = decode_tessellation_under_policy(&records, limited)
        .expect_err("association chain exceeds the selected depth");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RecursionDepth && limit.operation == "step_tessellation_association")
    );
}

#[test]
fn tessellation_representation_body_walk_uses_the_session_limit() {
    let mut records = String::from("#1=TESSELLATED_SHAPE_REPRESENTATION('mesh',(),$);\n");
    for id in 2..=15 {
        writeln!(records, "#{id}=SHAPE_REPRESENTATION('',(),$);").expect("write representation");
        writeln!(
            records,
            "#{}=SHAPE_REPRESENTATION_RELATIONSHIP('','',#{},#{id});",
            id + 100,
            id - 1
        )
        .expect("write relationship");
    }
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(&records, service)
        .expect("service depth admits the representation chain");
    let mut limited = service;
    limited.limits.max_recursion_depth = 13;
    let error = decode_tessellation_under_policy(&records, limited)
        .expect_err("representation chain exceeds the selected depth");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RecursionDepth && limit.operation == "step_representation_body_walk")
    );
}

#[test]
fn single_tessellation_normal_replication_charges_collection_items() {
    let records = "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,3,((0.,0.,1.)),$,((1,2,3)));";
    let service = DecodePolicy::service();
    let accepted = decode_tessellation_under_policy(records, service)
        .expect("service items admit three replicated normals");
    assert_eq!(accepted.model.tessellations[0].vertex_normals().len(), 3);
    let mut limited = service;
    limited.limits.max_collection_items = 18;
    let error = decode_tessellation_under_policy(records, limited)
        .expect_err("three normal copies exceed the selected item limit");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "step_tessellation_normal_replication")
    );
}

#[test]
fn tessellation_triangle_rows_charge_before_collection() {
    let records = "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,3,$,$,((1,2,3)));";
    let service = DecodePolicy::service();
    let accepted = decode_tessellation_under_policy(records, service)
        .expect("service admits one triangle row");
    assert_eq!(accepted.model.tessellations[0].triangles(), [[0, 1, 2]]);
    let mut limited = service;
    limited.limits.max_collection_items = 4;
    let error = decode_tessellation_under_policy(records, limited)
        .expect_err("one triangle row exceeds four prior admitted items");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "step_tessellation_triangle_rows")
    );
}

#[test]
fn tessellation_container_items_charge_before_collection() {
    let records = "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,3,$,$,((1,2,3)));
#3=TESSELLATED_SOLID('',(#2,#2),$);";
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(records, service).expect("service admits both items");
    let mut limited = service;
    limited.limits.max_collection_items = 5;
    let error = decode_tessellation_under_policy(records, limited)
        .expect_err("two container items exceed one admitted item");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "step_tessellation_container_items")
    );
}

#[test]
fn complex_tessellation_rows_charge_before_collection() {
    let records = "#1=COORDINATES_LIST('',4,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.),(1.,1.,0.)));
#2=COMPLEX_TRIANGULATED_SURFACE_SET('',#1,4,$,$,((1,2,3),(1,2,3,4)),());";
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(records, service).expect("service admits both strips");
    let mut limited = service;
    limited.limits.max_collection_items = 6;
    let error = decode_tessellation_under_policy(records, limited)
        .expect_err("two strip rows exceed one admitted item");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "step_complex_tessellation_rows")
    );
}

#[test]
fn complex_tessellation_indices_charge_before_collection() {
    let records = "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=COMPLEX_TRIANGULATED_SURFACE_SET('',#1,3,$,$,((1,2,3)),());";
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(records, service).expect("service admits three indices");
    let mut limited = service;
    limited.limits.max_collection_items = 7;
    let error = decode_tessellation_under_policy(records, limited)
        .expect_err("one row plus three indices exceed three admitted items");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "step_complex_tessellation_indices")
    );
}

#[test]
fn complex_tessellation_triangles_charge_before_collection() {
    let records = "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=COMPLEX_TRIANGULATED_SURFACE_SET('',#1,3,$,$,((1,2,3)),());";
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(records, service).expect("service admits one triangle");
    let mut limited = service;
    limited.limits.max_collection_items = 8;
    let error = decode_tessellation_under_policy(records, limited)
        .expect_err("one triangle exceeds the four prior admitted items");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "step_complex_tessellation_triangles")
    );
}

#[test]
fn complex_triangle_strip_alternates_winding() {
    let result = decode_inline(
        "#1=COORDINATES_LIST('',4,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.),(1.,1.,0.)));
#2=COMPLEX_TRIANGULATED_SURFACE_SET('',#1,4,$,$,((1,2,3,4)),());",
    );

    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert_eq!(
        result.ir().model.tessellations[0].triangles(),
        [[0, 1, 2], [2, 1, 3]]
    );
}

#[test]
fn complex_strip_and_malformed_strip_witnesses_preserve_winding() {
    let valid = include_bytes!("tests/data/ap07_complex_strip_and_fan.p21").as_slice();
    let result = StepCodec::default()
        .decode(&mut Cursor::new(valid), &DecodeOptions::default())
        .expect("decode strip and fan witness");
    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert_eq!(
        result.ir().model.tessellations[0].triangles(),
        [[0, 1, 2], [2, 1, 3], [0, 3, 4]]
    );
    let malformed = include_bytes!("tests/data/ap07_malformed_short_strip.p21").as_slice();
    let error = StepCodec::default()
        .decode(&mut Cursor::new(malformed), &DecodeOptions::default())
        .expect_err("short strip must refuse the aggregate");
    assert!(
        matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(message)) if message.contains("#2 strip row 1"))
    );
}

#[test]
fn non_finite_tessellation_coordinates_omit_dependents_and_retain_source() {
    let result = decode_inline_result(
        "#1=COORDINATES_LIST('',1,((1E400,0.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,1,$,$,((1,1,1)));",
    )
    .expect("bounded nonfinite coordinate and its dependent are omitted");
    let result = cadmpeg_test_support::EditableDecodeResult::from(result);
    assert!(result.ir().model.tessellations.is_empty());
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::ParseRecordOmitted.kind()
            && loss.message.contains("finite binary64 range")
    }));
    let retained = result
        .ir()
        .native_unknowns("step")
        .expect("retained source");
    for bytes in [
        b"#1=COORDINATES_LIST('',1,((1E400,0.,0.)));".as_slice(),
        b"#2=TRIANGULATED_SURFACE_SET('',#1,1,$,$,((1,1,1)));",
    ] {
        assert!(retained.iter().any(|record| {
            result
                .source_fidelity()
                .retained_record(record.id.as_str())
                .is_some_and(|source| source.data() == Some(bytes))
        }));
    }
}
#[test]
fn complex_tessellated_face_keeps_exact_support_surface_reachable() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#7=COMPLEX_TRIANGULATED_FACE('strip and fan',#6,4,((1.,0.,0.),(0.,1.,0.),(0.,0.,1.),(0.,0.,-1.)),$,(4,3,2,1),((1,2,3,4)),((1,2,4)));",
        "#7=COMPLEX_TRIANGULATED_FACE('strip and fan',#6,4,((1.,0.,0.),(0.,1.,0.),(0.,0.,1.),(0.,0.,-1.)),#79,(4,3,2,1),((1,2,3,4)),((1,2,4)));",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#79=PLANE('exact support',#34);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let result = StepCodec::default()
        .decode(
            &mut Cursor::new(source.as_bytes()),
            &DecodeOptions::default(),
        )
        .expect("decode complex tessellated support");
    let support = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#79")
        .expect("exact support surface");
    assert_eq!(
        support
            .source_object
            .as_ref()
            .map(|source| source.object_id.as_str()),
        Some("#7")
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(!validation.findings.iter().any(|finding| {
        finding.check == cadmpeg_ir::report::check::Check::CarrierReachability
            && finding.entity.as_deref() == Some("step:data:surface#79")
    }));
}

mod placement_work;
