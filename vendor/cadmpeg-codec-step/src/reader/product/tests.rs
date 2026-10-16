// SPDX-License-Identifier: Apache-2.0
//! STEP product, occurrence, and assembly tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

mod occurrence_limits;
mod placement_limits;

use cadmpeg_test_support::EditableDecodeResult;

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::transform::Transform;

use crate::export::write_step;
use crate::loss::StepLossCode;
use crate::test_support::exchange::{decode_inline, export};
use crate::{StepCodec, StepSchema, StepWriteOptions};

const DRAWING_OWNED_SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=DRAUGHTING_MODEL('',(#2),$);#2=REPRESENTATION('',(#3),$);#3=MAPPED_ITEM();ENDSEC;END-ISO-10303-21;";

fn drawing_owned_refuses(operation: &str) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (exchange, _) =
        crate::test_support::with_service_context(DRAWING_OWNED_SOURCE, crate::parse::parse_inner)
            .expect("valid drawing owned source");
    let refused = (0..=16).any(|limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(DRAWING_OWNED_SOURCE, &arena, &policy)
            .expect("root fits selected policy");
        matches!(
            super::drawing_owned_items(&exchange, &ctx),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == operation
        )
    });
    assert!(refused, "no collection limit refused {operation}");
}

#[test]
fn drawing_owned_pending_refuses_collection_limit() {
    drawing_owned_refuses("step_drawing_owned_pending");
}

#[test]
fn drawing_owned_visited_refuses_collection_limit() {
    drawing_owned_refuses("step_drawing_owned_visited");
}

#[test]
fn drawing_owned_items_refuse_collection_limit() {
    drawing_owned_refuses("step_drawing_owned_items");
}

#[test]
fn drawing_reference_walk_refuses_depth_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (exchange, _) =
        crate::test_support::with_service_context(DRAWING_OWNED_SOURCE, crate::parse::parse_inner)
            .expect("valid drawing owned source");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(DRAWING_OWNED_SOURCE, &arena, &policy)
        .expect("root fits selected policy");
    assert!(matches!(
        super::drawing_owned_items(&exchange, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_drawing_reference_walk"
    ));
}

#[test]
fn pending_occurrence_refuses_caller_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(b"occurrence", &arena, &policy)
            .expect("root fits selected policy");
    let id =
        cadmpeg_ir::ids::OccurrenceId::mint("step:data:occurrence#1").expect("valid occurrence id");
    let mut pending = std::collections::VecDeque::new();
    let error = ctx
        .push_back(&mut pending, (1, id), "step_pending_occurrence")
        .expect_err("one pending occurrence exceeds zero collection items");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && limit.operation == "step_pending_occurrence"
    ));
    assert!(pending.is_empty());
}

const PRODUCT_STRING_LIMIT_SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=APPLICATION_CONTEXT('mechanical design');#2=PRODUCT_CONTEXT('',#1,'mechanical');#3=PRODUCT('P','Part name','',(#2));#4=PRODUCT_DEFINITION_FORMATION('','',#3);#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');#6=PRODUCT_DEFINITION('part','Description',#4,#5);#7=PRODUCT('C','Child name','',(#2));#8=PRODUCT_DEFINITION_FORMATION('','',#7);#9=PRODUCT_DEFINITION('child','',#8,#5);#10=NEXT_ASSEMBLY_USAGE_OCCURRENCE('u','Child instance','',#6,#9,$);ENDSEC;END-ISO-10303-21;";

fn product_collection_refuses_source(source: &[u8], operation: &str) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid product exchange");
    let refused = (0..=1024).any(|limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
            .expect("root fits collection policy");
        matches!(
            crate::reader::decode_exchange(
                source,
                exchange.clone(),
                &diagnostics,
                &ctx,
                crate::reader::Packaging::Bare,
            ),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == operation
        )
    });
    assert!(refused, "no collection limit refused {operation}");
}

fn product_collection_refuses(operation: &str) {
    product_collection_refuses_source(PRODUCT_STRING_LIMIT_SOURCE, operation);
}

#[test]
fn product_formation_map_refuses_collection_limit() {
    product_collection_refuses("step_product_formations");
}

#[test]
fn product_definition_map_refuses_collection_limit() {
    product_collection_refuses("step_product_definitions");
}

#[test]
fn product_definition_group_map_refuses_collection_limit() {
    product_collection_refuses("step_product_definition_groups");
}

#[test]
fn product_definition_group_members_refuse_collection_limit() {
    product_collection_refuses("step_product_definition_group_members");
}

#[test]
fn product_child_definitions_refuse_collection_limit() {
    product_collection_refuses("step_product_child_definitions");
}

#[test]
fn product_usage_parent_groups_refuse_collection_limit() {
    product_collection_refuses("step_product_usage_parent_groups");
}

#[test]
fn product_usage_parent_members_refuse_collection_limit() {
    product_collection_refuses("step_product_usage_parent_members");
}

fn product_representation_collection_refuses(operation: &str) {
    let source = String::from_utf8_lossy(PRODUCT_STRING_LIMIT_SOURCE).replace(
        "ENDSEC;END-ISO-10303-21;",
        "#11=PRODUCT_DEFINITION_SHAPE('','',#9);#12=SHAPE_DEFINITION_REPRESENTATION(#11,#13);#13=SHAPE_REPRESENTATION('',(),$);ENDSEC;END-ISO-10303-21;",
    );
    product_collection_refuses_source(source.as_bytes(), operation);
}

#[test]
fn shape_binding_shapes_refuse_collection_limit() {
    product_representation_collection_refuses("step_shape_binding_shapes");
}

#[test]
fn body_placement_shapes_refuse_collection_limit() {
    product_representation_collection_refuses("step_body_placement_shapes");
}

#[test]
fn occurrence_placement_shapes_refuse_collection_limit() {
    product_representation_collection_refuses("step_occurrence_placement_shapes");
}

#[test]
fn definition_representation_groups_refuse_collection_limit() {
    product_representation_collection_refuses("step_definition_representation_groups");
}

#[test]
fn definition_representation_members_refuse_collection_limit() {
    product_representation_collection_refuses("step_definition_representation_members");
}

#[test]
fn assembly_representations_refuse_collection_limit() {
    product_representation_collection_refuses("step_assembly_representations");
}

#[test]
fn represented_definition_groups_refuse_collection_limit() {
    product_representation_collection_refuses("step_represented_definition_groups");
}

#[test]
fn represented_definition_members_refuse_collection_limit() {
    product_representation_collection_refuses("step_represented_definition_members");
}

#[test]
fn body_placement_indices_refuse_collection_limit() {
    product_collection_refuses_source(
        include_bytes!("../../../tests/fixtures/ap214_sheet.p21"),
        "step_body_placement_indices",
    );
}

#[test]
fn product_definition_ir_items_refuse_collection_limit() {
    product_collection_refuses("step_product_definition_ir_items");
}

#[test]
fn product_source_groups_refuse_collection_limit() {
    product_collection_refuses("step_product_source_groups");
}

#[test]
fn product_source_group_members_refuse_collection_limit() {
    product_collection_refuses("step_product_source_group_members");
}

#[test]
fn product_losses_refuse_collection_limit() {
    product_collection_refuses("step_product_losses");
}

fn shape_binding_collection_refuses(operation: &str) {
    let source = String::from_utf8_lossy(include_bytes!(
        "../../../tests/fixtures/ap214_sheet.p21"
    ))
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#80=APPLICATION_CONTEXT('mechanical design');\n#81=PRODUCT_CONTEXT('',#80,'mechanical');\n#82=PRODUCT('P','Shape part','',(#81));\n#83=PRODUCT_DEFINITION_FORMATION('','',#82);\n#84=PRODUCT_DEFINITION_CONTEXT('part definition',#80,'design');\n#85=PRODUCT_DEFINITION('part','',#83,#84);\n#86=PRODUCT_DEFINITION_SHAPE('','',#85);\n#87=SHAPE_DEFINITION_REPRESENTATION(#86,#32);\nENDSEC;\nEND-ISO-10303-21;",
    );
    product_collection_refuses_source(source.as_bytes(), operation);
}

#[test]
fn shape_binding_groups_refuse_collection_limit() {
    shape_binding_collection_refuses("step_shape_binding_groups");
}

#[test]
fn shape_binding_bodies_refuse_collection_limit() {
    shape_binding_collection_refuses("step_shape_binding_bodies");
}

fn mapped_body_placement_source() -> String {
    String::from_utf8_lossy(include_bytes!(
        "../../../tests/fixtures/ap214_sheet.p21"
    ))
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#70=CARTESIAN_POINT('',(20.,0.,0.));\n#71=CARTESIAN_POINT('',(40.,0.,0.));\n#72=AXIS2_PLACEMENT_3D('',#70,#9,#10);\n#73=AXIS2_PLACEMENT_3D('',#71,#9,#10);\n#74=REPRESENTATION_MAP(#27,#32);\n#75=MAPPED_ITEM('first',#74,#72);\n#76=MAPPED_ITEM('second',#74,#73);\nENDSEC;\nEND-ISO-10303-21;",
    )
}

fn mapped_body_placement_collection_refuses(operation: &str) {
    let source = mapped_body_placement_source();
    product_collection_refuses_source(source.as_bytes(), operation);
}

#[test]
fn body_placement_groups_refuse_collection_limit() {
    mapped_body_placement_collection_refuses("step_body_placement_groups");
}

#[test]
fn body_placement_group_members_refuse_collection_limit() {
    mapped_body_placement_collection_refuses("step_body_placement_group_members");
}

#[test]
fn unique_body_placements_refuse_collection_limit() {
    mapped_body_placement_collection_refuses("step_unique_body_placements");
}

#[test]
fn product_definition_descriptions_refuse_collection_limit() {
    product_collection_refuses("step_product_definition_descriptions");
}

#[test]
fn product_definition_counts_refuse_collection_limit() {
    product_collection_refuses("step_product_definition_counts");
}

#[test]
fn product_definition_prototypes_refuse_collection_limit() {
    product_collection_refuses("step_product_definition_prototypes");
}

#[test]
fn product_shape_prototypes_refuse_collection_limit() {
    let source = String::from_utf8_lossy(PRODUCT_STRING_LIMIT_SOURCE).replace(
        "ENDSEC;END-ISO-10303-21;",
        "#11=PRODUCT_DEFINITION_SHAPE('','',#6);ENDSEC;END-ISO-10303-21;",
    );
    product_collection_refuses_source(source.as_bytes(), "step_product_shape_prototypes");
}

#[test]
fn product_typed_claims_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits collection policy");
    assert!(matches!(
        ctx.insert_hash_set(&mut std::collections::HashSet::new(), 1, "step_product_typed_claims").map(|_| ()),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_product_typed_claims"
    ));
}

#[test]
fn product_string_text_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (exchange, diagnostics) = crate::test_support::with_service_context(
        PRODUCT_STRING_LIMIT_SOURCE,
        crate::parse::parse_inner,
    )
    .expect("valid product exchange");
    let arena = DecodeArena::new();
    let refused = {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "step_string_text",
            |limit| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = limit;
                let (ctx, _) =
                    DecodeContext::from_root_bytes(PRODUCT_STRING_LIMIT_SOURCE, &arena, &policy)
                        .expect("root fits retained policy");

                (crate::reader::decode_exchange(
                    PRODUCT_STRING_LIMIT_SOURCE,
                    exchange.clone(),
                    &diagnostics,
                    &ctx,
                    crate::reader::Packaging::Bare,
                ))
                .map(|_| ())
            },
        );
        matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "step_string_text")
    };
    assert!(refused, "no retained limit refused a product string");
}

fn product_retained_refuses_source(source: &[u8], operation: &str) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid product exchange");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        operation,
        |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy).expect("root");
            crate::reader::decode_exchange(
                source,
                exchange.clone(),
                &diagnostics,
                &ctx,
                crate::reader::Packaging::Bare,
            )
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::RetainedBytes && refusal.operation == operation));
}

fn product_copy_refuses_retained_limit(operation: &str) {
    let source = String::from_utf8_lossy(PRODUCT_STRING_LIMIT_SOURCE).replace(
        "PRODUCT('P','Part name',''",
        "PRODUCT('P','Part name','Summary'",
    );
    product_retained_refuses_source(source.as_bytes(), operation);
}

#[test]
fn product_definition_description_copy_refuses_retained_limit() {
    product_copy_refuses_retained_limit("step_product_definition_description_copy");
}

#[test]
fn product_description_copy_refuses_retained_limit() {
    product_copy_refuses_retained_limit("step_product_description_copy");
}

#[test]
fn product_source_name_copy_refuses_retained_limit() {
    product_copy_refuses_retained_limit("step_product_source_name_copy");
}

#[test]
fn product_label_copy_refuses_retained_limit() {
    product_copy_refuses_retained_limit("step_product_label_copy");
}

#[test]
fn product_part_number_copy_refuses_retained_limit() {
    product_copy_refuses_retained_limit("step_product_part_number_copy");
}

#[test]
fn product_occurrence_name_copy_refuses_retained_limit() {
    product_copy_refuses_retained_limit("step_product_occurrence_name_copy");
}

#[test]
fn product_usage_entries_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (exchange, diagnostics) = crate::test_support::with_service_context(
        PRODUCT_STRING_LIMIT_SOURCE,
        crate::parse::parse_inner,
    )
    .expect("valid product exchange");
    let arena = DecodeArena::new();
    let refused = (0..512).any(|limit| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(PRODUCT_STRING_LIMIT_SOURCE, &arena, &policy)
            .expect("root fits collection policy");
        matches!(
            crate::reader::decode_exchange(
                PRODUCT_STRING_LIMIT_SOURCE,
                exchange.clone(),
                &diagnostics,
                &ctx,
                crate::reader::Packaging::Bare,
            ),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "step_product_usage_entries"
        )
    });
    assert!(refused, "no collection limit refused product usage entries");
}

#[test]
fn product_descriptions_transfer_from_product_and_definition() {
    let decoded = decode_inline(
        "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('P','Part','Product description',(#2));
#4=PRODUCT_DEFINITION_FORMATION('','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION('part','Definition description',#4,#5);
#7=PRODUCT('Q','Second part','',(#2));
#8=PRODUCT_DEFINITION_FORMATION('','',#7);
#9=PRODUCT_DEFINITION('second','Fallback description',#8,#5);",
    );
    let descriptions = decoded
        .ir()
        .model
        .product_definitions
        .iter()
        .map(|product| product.description.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(
        descriptions,
        [Some("Product description"), Some("Fallback description")]
    );

    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model
        .product_definitions
        .push(cadmpeg_ir::products::ProductDefinition {
            id: "test:model:product#described"
                .try_into()
                .expect("valid identity"),
            kind: cadmpeg_ir::products::ProductDefinitionKind::Part,
            source_name: Some("Described part".into()),
            label: Some("Described part".into()),
            description: Some("Round-tripped description".into()),
            part_number: Some("DESCRIBED".into()),
            bom_properties: std::collections::BTreeMap::new(),
            bodies: vec![ir.model.bodies[0].id.clone()],
            native_ref: None,
        });
    let mut output = Vec::new();
    write_step(
        &ir,
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write described product");
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode described product");
    assert_eq!(
        roundtrip.ir().model.product_definitions[0]
            .description
            .as_deref(),
        Some("Round-tripped description")
    );
}

#[test]
fn product_definition_views_keep_distinct_prototypes_and_metadata() {
    use cadmpeg_ir::products::PrototypeReference;

    let result = decode_inline(
        "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('P','Part','Product description',(#2));
#4=PRODUCT_DEFINITION_FORMATION('v1','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION('design view','Design view description',#4,#5);
#7=PRODUCT_DEFINITION_FORMATION('v2','',#3);
#8=PRODUCT_DEFINITION('manufacturing view','Manufacturing view description',#7,#5);",
    );

    assert_eq!(result.ir().model.product_definitions.len(), 2);
    assert_eq!(
        result
            .ir()
            .model
            .product_definitions
            .iter()
            .map(|definition| definition.description.as_deref())
            .collect::<Vec<_>>(),
        [
            Some("Design view description"),
            Some("Manufacturing view description")
        ]
    );
    assert_eq!(result.ir().model.occurrences.len(), 2);
    let prototypes = result
        .ir()
        .model
        .occurrences
        .iter()
        .filter_map(|occurrence| match &occurrence.prototype {
            PrototypeReference::Local { definition } => Some(definition.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(prototypes.len(), 2);
    assert_ne!(prototypes[0], prototypes[1]);
    assert!(prototypes
        .iter()
        .all(|id| id.as_str().contains("-definition-")));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
pub(crate) fn decode_builds_product_occurrences_with_relative_placement() {
    use cadmpeg_ir::products::OccurrenceParent;

    let bytes = include_bytes!("../../../tests/fixtures/ap242_assembly.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode AP242 assembly");

    assert_eq!(result.ir().model.product_definitions.len(), 2);
    assert_eq!(result.ir().model.occurrences.len(), 2);
    let child = result
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("Placed child"))
        .unwrap();
    assert!(matches!(child.parent, OccurrenceParent::Occurrence { .. }));
    assert_eq!(child.transform.rows()[0][3], 25.0);
    assert_eq!(child.transform.rows()[1][3], 0.0);
    assert_eq!(child.transform.rows()[2][3], 0.0);
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);

    let mut output = Vec::new();
    write_step(
        result.ir(),
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write product graph");
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode written product graph");
    assert_eq!(roundtrip.ir().model.product_definitions.len(), 2);
    assert_eq!(roundtrip.ir().model.occurrences.len(), 2);
    let child = roundtrip
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("Placed child"))
        .expect("round-tripped child occurrence");
    assert!(matches!(child.parent, OccurrenceParent::Occurrence { .. }));
    assert_eq!(child.transform.rows()[0][3], 25.0);
}

#[test]
fn occurrence_transform_direction_follows_relationship_endpoints() {
    let source = String::from_utf8(include_bytes!(
        "../../../tests/fixtures/ap242_assembly.p21"
    )
    .to_vec())
    .expect("fixture is UTF-8")
    .replace(
        "#37=(REPRESENTATION_RELATIONSHIP('','',#23,#22) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#36) SHAPE_REPRESENTATION_RELATIONSHIP());",
        "#37=(REPRESENTATION_RELATIONSHIP('','',#22,#23) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#36) SHAPE_REPRESENTATION_RELATIONSHIP());",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode endpoint-reversed assembly relationship");

    let child = result
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("Placed child"))
        .expect("placed child occurrence");
    assert_eq!(child.transform.rows()[0][3], -25.0);
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::NauoPlacementUnresolved.kind()
            && loss.message.contains("NAUO #12")
    }));
}

#[test]
fn ps02_item_defined_transform_items_follow_relationship_endpoint_contexts() {
    let decode_fixture = |bytes: &[u8]| {
        StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode PS-02 fixture")
    };

    let child_to_parent = decode_fixture(include_bytes!(
        "tests/data/ps02_child_to_parent_transform.p21"
    ));
    let child = child_to_parent
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.id.as_str().contains("#12"))
        .expect("child-to-parent occurrence");
    assert_eq!(child.transform.rows()[0][3], 25.0);
    assert!(!child_to_parent
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == StepLossCode::NauoPlacementUnresolved.kind() }));

    let parent_to_child = decode_fixture(include_bytes!(
        "tests/data/ps02_parent_to_child_transform.p21"
    ));
    let child = parent_to_child
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.id.as_str().contains("#12"))
        .expect("parent-to-child occurrence");
    assert_eq!(child.transform.rows()[0][3], -25.0);
    assert!(!parent_to_child
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == StepLossCode::NauoPlacementUnresolved.kind() }));
}

#[test]
fn occurrence_transform_requires_direct_definition_representation_endpoints() {
    let source = String::from_utf8(include_bytes!(
        "../../../tests/fixtures/ap242_assembly.p21"
    )
    .to_vec())
    .expect("fixture is UTF-8")
    .replace(
        "#37=(REPRESENTATION_RELATIONSHIP('','',#23,#22) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#36) SHAPE_REPRESENTATION_RELATIONSHIP());",
        "#37=(REPRESENTATION_RELATIONSHIP('','',#40,#22) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#36) SHAPE_REPRESENTATION_RELATIONSHIP());",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#40=SHAPE_REPRESENTATION('placed child',(),#21);\n#41=SHAPE_REPRESENTATION_RELATIONSHIP('','',#23,#40);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode bridge-only shape representation");

    let child = result
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("Placed child"))
        .expect("unresolved child occurrence remains represented");
    assert_eq!(child.transform, Transform::identity());
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::NauoPlacementUnresolved.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Error
            && loss.message.contains("NAUO #12")
    }));
}

#[test]
fn occurrence_transform_accepts_cartesian_operator_endpoints() {
    let source =
        String::from_utf8(include_bytes!("../../../tests/fixtures/ap242_assembly.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#34=AXIS2_PLACEMENT_3D('',#30,#32,#33);",
                "#34=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#33,$,#30,1.,#32);",
            )
            .replace(
                "#35=AXIS2_PLACEMENT_3D('',#31,#32,#33);",
                "#35=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#33,$,#31,1.,#32);",
            );
    let result = StepCodec::default()
        .decode(
            &mut Cursor::new(source.into_bytes()),
            &DecodeOptions::default(),
        )
        .expect("decode operator-based occurrence transform");

    let child = result
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("Placed child"))
        .expect("placed child occurrence");
    assert_eq!(child.transform.rows()[0][3], 25.0);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::NauoPlacementUnresolved.kind()));
}

#[test]
fn unresolved_occurrence_transform_is_reported_as_error() {
    let result = decode_inline(
        "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('P','parent','',(#2));
#4=PRODUCT_DEFINITION_FORMATION('','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION('parent','',#4,#5);
#7=PRODUCT('C','child','',(#2));
#8=FINAL_SOLUTION('','',#7,'complete');
#9=PRODUCT_DEFINITION('child','',#8,#5);
#10=NEXT_ASSEMBLY_USAGE_OCCURRENCE('u','child instance','',#6,#9,$);",
    );

    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::NauoPlacementUnresolved.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Error
            && loss.message.contains("NAUO #10")
    }));
}

#[test]
fn ps07_duplicate_context_placements_remain_opaque_in_any_order() {
    for input in [
        include_bytes!("tests/data/ps07_duplicate_context_placement_first.p21").as_slice(),
        include_bytes!("tests/data/ps07_duplicate_context_placement_reordered.p21").as_slice(),
    ] {
        let result = EditableDecodeResult::from(
            StepCodec::default()
                .decode(&mut Cursor::new(input), &DecodeOptions::default())
                .expect("decode PS-07 fixture"),
        );
        let occurrence = result
            .ir()
            .model
            .occurrences
            .iter()
            .find(|occurrence| occurrence.id.as_str().contains("#12"));
        assert!(
            occurrence.is_none(),
            "ambiguous occurrence must not be admitted"
        );
        assert!(result.report().losses.iter().any(|loss| {
            loss.code == StepLossCode::NauoPlacementAmbiguous.kind()
                && loss.severity == cadmpeg_ir::report::Severity::Error
                && loss.message.contains("NAUO #12")
                && loss.message.contains("#38")
                && loss.message.contains("#43")
                && loss.message.contains("no neutral occurrence was admitted")
                && loss.message.contains("remain opaque")
        }));
        assert!(!result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == StepLossCode::NauoPlacementUnresolved.kind()));
        let unknowns = result.ir().native_unknowns("step").unwrap();
        let unknown_ids = unknowns
            .iter()
            .map(|record| record.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            unknown_ids,
            std::collections::BTreeSet::from([
                "step:data:context_dependent_shape_representation#38",
                "step:data:context_dependent_shape_representation#43",
                "step:data:next_assembly_usage_occurrence#12",
            ])
        );
        for id in &unknown_ids {
            let retained = result
                .source_fidelity()
                .retained_record(id)
                .expect("ambiguous placement source record is retained");
            assert_eq!(
                retained.data(),
                Some(
                    &input[usize::try_from(retained.offset()).expect("retained offset fits memory")
                        ..usize::try_from(retained.offset() + retained.byte_len())
                            .expect("retained end fits memory")]
                )
            );
        }
        let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("resource allocation did not fail");
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn ps08_mixed_placement_mechanisms_remain_opaque_in_any_order() {
    for input in [
        include_bytes!("tests/data/ps08_mixed_placement_mechanisms_first.p21").as_slice(),
        include_bytes!("tests/data/ps08_mixed_placement_mechanisms_reordered.p21").as_slice(),
    ] {
        let result = EditableDecodeResult::from(
            StepCodec::default()
                .decode(&mut Cursor::new(input), &DecodeOptions::default())
                .expect("decode PS-08 fixture"),
        );
        assert!(result
            .ir()
            .model
            .occurrences
            .iter()
            .all(|occurrence| !occurrence.id.as_str().contains("#12")));
        assert!(result.report().losses.iter().any(|loss| {
            loss.code == StepLossCode::NauoPlacementAmbiguous.kind()
                && loss.severity == cadmpeg_ir::report::Severity::Error
                && loss.message.contains("NAUO #12")
                && loss
                    .message
                    .contains("resolved context-dependent and occurrence-owned mapped placements")
                && loss.message.contains("#43")
                && loss.message.contains("#54")
                && loss.message.contains("no neutral occurrence was admitted")
                && loss.message.contains("remain opaque")
        }));
        assert!(!result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == StepLossCode::NauoPlacementUnresolved.kind()));
        let unknowns = result.ir().native_unknowns("step").unwrap();
        let unknown_ids = unknowns
            .iter()
            .map(|record| record.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            unknown_ids,
            std::collections::BTreeSet::from([
                "step:data:context_dependent_shape_representation#54",
                "step:data:next_assembly_usage_occurrence#12",
                "step:data:shape_definition_representation#43",
            ])
        );
        for id in &unknown_ids {
            let retained = result
                .source_fidelity()
                .retained_record(id)
                .expect("competing placement source record is retained");
            assert_eq!(
                retained.data(),
                Some(
                    &input[usize::try_from(retained.offset()).expect("retained offset fits memory")
                        ..usize::try_from(retained.offset() + retained.byte_len())
                            .expect("retained end fits memory")]
                )
            );
        }
        let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("resource allocation did not fail");
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
pub(crate) fn decode_builds_occurrence_placement_from_mapped_item() {
    let bytes = include_bytes!("../../../tests/fixtures/ap242_mapped_assembly.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode mapped-item assembly");

    let child = result
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("Mapped child"))
        .unwrap();
    assert_eq!(child.transform.rows()[0][3], 40.0);
    assert_eq!(child.transform.rows()[1][3], 5.0);
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_product_relationships_preserve_mapped_occurrence_placement() {
    let source = String::from_utf8(include_bytes!(
        "../../../tests/fixtures/ap242_mapped_assembly.p21"
    )
    .to_vec())
    .expect("fixture is UTF-8")
    .replace(
        "#7=PRODUCT_DEFINITION_SHAPE('','',#6);",
        "#7=(PRODUCT_DEFINITION_SHAPE('','',#6) PROPERTY_DEFINITION());",
    )
    .replace(
        "#11=PRODUCT_DEFINITION_SHAPE('','',#10);",
        "#11=(PRODUCT_DEFINITION_SHAPE('','',#10) PROPERTY_DEFINITION());",
    )
    .replace(
        "#12=NEXT_ASSEMBLY_USAGE_OCCURRENCE('occ-1','Mapped child','',#6,#10,$);",
        "#12=(ASSEMBLY_COMPONENT_USAGE() NEXT_ASSEMBLY_USAGE_OCCURRENCE('occ-1','Mapped child','',#6,#10,$));",
    )
    .replace(
        "#24=SHAPE_DEFINITION_REPRESENTATION(#7,#22);",
        "#24=(PROPERTY_DEFINITION_REPRESENTATION() SHAPE_DEFINITION_REPRESENTATION(#7,#22));",
    )
    .replace(
        "#25=SHAPE_DEFINITION_REPRESENTATION(#11,#23);",
        "#25=(PROPERTY_DEFINITION_REPRESENTATION() SHAPE_DEFINITION_REPRESENTATION(#11,#23));",
    )
    .replace(
        "#40=MAPPED_ITEM('Mapped child',#39,#35);",
        "#40=(MAPPED_ITEM('Mapped child',#39,#35) REPRESENTATION_ITEM());",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex mapped-item assembly");

    let child = result
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("Mapped child"))
        .expect("mapped child occurrence");
    assert_eq!(child.transform.rows()[0][3], 40.0);
    assert_eq!(child.transform.rows()[1][3], 5.0);
}

#[test]
fn conflicting_standalone_mapped_body_placements_are_not_overwritten() {
    let source = String::from_utf8(include_bytes!("../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#70=CARTESIAN_POINT('',(20.,0.,0.));\n#71=CARTESIAN_POINT('',(40.,0.,0.));\n#72=AXIS2_PLACEMENT_3D('',#70,#9,#10);\n#73=AXIS2_PLACEMENT_3D('',#71,#9,#10);\n#74=REPRESENTATION_MAP(#27,#32);\n#75=MAPPED_ITEM('first',#74,#72);\n#76=MAPPED_ITEM('second',#74,#73);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode conflicting standalone body mappings");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert!(result.ir().model.bodies[0].transform.is_none());
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::BodyConflictingMappedPlacements.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Error
            && loss
                .message
                .contains("conflicting standalone MAPPED_ITEM placements")
            && loss.message.contains("#75")
            && loss.message.contains("#76")
    }));
}

#[test]
fn ps03_repeated_mapped_body_placements_require_one_cadir_transform() {
    let decode_fixture = |bytes: &[u8]| {
        StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode PS-03 fixture")
    };

    let same_transform = decode_fixture(include_bytes!(
        "tests/data/ps03_repeated_mapped_placements_same_transform.p21"
    ));
    assert_eq!(same_transform.ir().model.bodies.len(), 1);
    assert_eq!(
        same_transform.ir().model.bodies[0]
            .transform
            .expect("one shared body transform")
            .rows()[0][3],
        20.0
    );
    assert!(!same_transform
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == StepLossCode::BodyConflictingMappedPlacements.kind() }));

    for bytes in [
        include_bytes!("tests/data/ps03_repeated_mapped_placements.p21").as_slice(),
        include_bytes!("tests/data/ps03_repeated_mapped_placements_reordered.p21").as_slice(),
    ] {
        let conflicting = decode_fixture(bytes);
        assert_eq!(conflicting.ir().model.bodies.len(), 1);
        assert!(conflicting.ir().model.bodies[0].transform.is_none());
        assert!(conflicting.report().losses.iter().any(|loss| {
            loss.code == StepLossCode::BodyConflictingMappedPlacements.kind()
                && loss.severity == cadmpeg_ir::report::Severity::Error
                && loss.message.contains("#39")
                && loss.message.contains("#40")
        }));
    }
}

#[test]
fn drawing_mapped_items_do_not_place_exact_bodies() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_vertex_loop.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#20=ITEM('camera mapping');\n#21=REPRESENTATION_MAP(#20,#19);\n#22=MAPPED_ITEM('',#21,#6);\n#23=DRAUGHTING_MODEL('Drawing',(#22),#2);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode drawing mapped item");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert!(result.ir().model.bodies[0].transform.is_none());
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("MAPPED_ITEM #22 has no resolved body placement")
    }));
}

#[test]
fn nested_drawing_mapped_items_do_not_place_exact_bodies() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_vertex_loop.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#20=ITEM('camera mapping');\n#21=REPRESENTATION_MAP(#20,#19);\n#22=MAPPED_ITEM('',#21,#6);\n#23=GEOMETRIC_SET('Drawing items',(#22));\n#24=DRAUGHTING_MODEL('Drawing',(#23),#2);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode nested drawing mapped item");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert!(result.ir().model.bodies[0].transform.is_none());
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("MAPPED_ITEM #22 has no resolved body placement")
    }));
}

#[test]
fn two_dimensional_mapping_does_not_change_body_placement() {
    let mut source = export(&unit_cube().expect("unit cube fixture is admitted"));
    let representation_line = source
        .lines()
        .find(|line| line.contains("ADVANCED_BREP_SHAPE_REPRESENTATION("))
        .expect("written body representation");
    let representation = representation_line
        .split_once('=')
        .and_then(|(id, _)| id.trim().strip_prefix('#'))
        .and_then(|id| id.parse::<u64>().ok())
        .expect("body representation id");
    let next_id = source
        .lines()
        .filter_map(|line| {
            line.strip_prefix('#')
                .and_then(|line| line.split_once('='))
                .and_then(|(id, _)| id.trim().parse::<u64>().ok())
        })
        .max()
        .expect("written STEP entity")
        + 1;
    let origin_point = next_id;
    let origin_direction = next_id + 1;
    let origin = next_id + 2;
    let map = next_id + 3;
    let target_point = next_id + 4;
    let target = next_id + 5;
    let mapped_item = next_id + 6;
    let records = format!(
        "#{origin_point}=CARTESIAN_POINT('',(0.,0.));\n\
#{origin_direction}=DIRECTION('',(1.,0.));\n\
#{origin}=AXIS2_PLACEMENT_2D('',#{origin_point},#{origin_direction});\n\
#{map}=REPRESENTATION_MAP(#{origin},#{representation});\n\
#{target_point}=CARTESIAN_POINT('',(10.,0.));\n\
#{target}=AXIS2_PLACEMENT_2D('',#{target_point},#{origin_direction});\n\
#{mapped_item}=MAPPED_ITEM('',#{map},#{target});\n"
    );
    let end = source.rfind("ENDSEC;").expect("STEP data section end");
    source.insert_str(end, &records);

    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode 2D mapped presentation item");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert!(result.ir().model.bodies[0].transform.is_none());
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("MAPPED_ITEM has no resolved body placement")
    }));
}

#[test]
fn decode_builds_mapped_item_placement_from_canonical_cartesian_operator() {
    let bytes = include_bytes!("../../../tests/fixtures/ap242_mapped_assembly.p21");
    let mut source = String::from_utf8(bytes.to_vec()).expect("fixture is UTF-8");
    source = source.replace(
        "#30=CARTESIAN_POINT('',(0.,0.,0.));",
        "#30=CARTESIAN_POINT('',(10.,0.,0.));",
    );
    source = source.replace(
        "#33=DIRECTION('',(1.,0.,0.));",
        "#33=DIRECTION('',(1.,0.,0.));\n#36=DIRECTION('',(0.,1.,0.));",
    );
    source = source.replace(
        "#35=AXIS2_PLACEMENT_3D('',#31,#32,#33);",
        "#35=CARTESIAN_TRANSFORMATION_OPERATOR_3D('','','',#33,#36,#31,2.,#32);",
    );
    let result = StepCodec::default()
        .decode(
            &mut Cursor::new(source.into_bytes()),
            &DecodeOptions::default(),
        )
        .expect("decode canonical mapped-item assembly");

    let child = result
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("Mapped child"))
        .expect("mapped child occurrence");
    assert_eq!(child.transform.rows()[0], [2.0, 0.0, 0.0, 20.0]);
    assert_eq!(child.transform.rows()[1], [0.0, 2.0, 0.0, 5.0]);
    assert_eq!(child.transform.rows()[2], [0.0, 0.0, 2.0, 0.0]);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::NauoPlacementUnresolved.kind()));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_builds_repeated_occurrence_placements_from_their_shape_representations() {
    let bytes = include_bytes!("../../../tests/fixtures/ap242_occurrence_mapped_assembly.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode occurrence-mapped assembly");

    let mut children = result
        .ir()
        .model
        .occurrences
        .iter()
        .filter(|occurrence| occurrence.name.is_some())
        .collect::<Vec<_>>();
    children.sort_by(|left, right| left.name.cmp(&right.name));
    assert_eq!(children.len(), 2);
    assert_eq!(children[0].name.as_deref(), Some("First child"));
    assert_eq!(children[0].transform.rows()[0][3], 25.0);
    assert_eq!(children[0].transform.rows()[1][3], 0.0);
    assert_eq!(children[1].name.as_deref(), Some("Second child"));
    assert_eq!(children[1].transform.rows()[0][3], -10.0);
    assert_eq!(children[1].transform.rows()[1][3], 4.0);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::NauoPlacementUnresolved.kind()));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_infers_unlinked_occurrence_placements_from_parent_shape_items() {
    let result = decode_inline(
        "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('ROOT','Root assembly','',(#2));
#4=PRODUCT_DEFINITION_FORMATION('','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION('root definition','',#4,#5);
#7=PRODUCT_DEFINITION_SHAPE('','',#6);
#8=PRODUCT('ONE','First child','',(#2));
#9=PRODUCT_DEFINITION_FORMATION('','',#8);
#10=PRODUCT_DEFINITION('first definition','',#9,#5);
#11=PRODUCT_DEFINITION_SHAPE('','',#10);
#12=PRODUCT('TWO','Second child','',(#2));
#13=PRODUCT_DEFINITION_FORMATION('','',#12);
#14=PRODUCT_DEFINITION('second definition','',#13,#5);
#15=PRODUCT_DEFINITION_SHAPE('','',#14);
#16=NEXT_ASSEMBLY_USAGE_OCCURRENCE('one','First child','',#6,#10,$);
#17=NEXT_ASSEMBLY_USAGE_OCCURRENCE('two','Second child','',#6,#14,$);
#20=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#21=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#20)) REPRESENTATION_CONTEXT('model','3D'));
#22=(CHARACTERIZED_REPRESENTATION() REPRESENTATION('root',(#39,#41),#21) SHAPE_REPRESENTATION());
#23=SHAPE_REPRESENTATION('first',(),#21);
#24=SHAPE_REPRESENTATION('second',(),#21);
#25=SHAPE_DEFINITION_REPRESENTATION(#7,#22);
#26=SHAPE_DEFINITION_REPRESENTATION(#11,#23);
#27=SHAPE_DEFINITION_REPRESENTATION(#15,#24);
#30=CARTESIAN_POINT('',(0.,0.,0.));
#31=CARTESIAN_POINT('',(25.,0.,0.));
#32=CARTESIAN_POINT('',(-10.,4.,0.));
#33=DIRECTION('',(0.,0.,1.));
#34=DIRECTION('',(1.,0.,0.));
#35=AXIS2_PLACEMENT_3D('',#30,#33,#34);
#36=AXIS2_PLACEMENT_3D('',#31,#33,#34);
#37=AXIS2_PLACEMENT_3D('',#32,#33,#34);
#38=REPRESENTATION_MAP(#35,#23);
#39=MAPPED_ITEM('First child',#38,#36);
#40=REPRESENTATION_MAP(#35,#24);
#41=MAPPED_ITEM('Second child',#40,#37);",
    );

    let mut children = result
        .ir()
        .model
        .occurrences
        .iter()
        .filter(|occurrence| {
            occurrence.id.as_str().contains("#16") || occurrence.id.as_str().contains("#17")
        })
        .collect::<Vec<_>>();
    children.sort_by_key(|occurrence| occurrence.id.clone());
    assert_eq!(children.len(), 2);
    assert_eq!(children[0].transform.rows()[0][3], 25.0);
    assert_eq!(children[1].transform.rows()[0][3], -10.0);
    assert_eq!(children[1].transform.rows()[1][3], 4.0);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == StepLossCode::NauoPlacementUnresolved.kind() }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn ps09_parent_mapped_items_bind_by_child_definition_not_set_order() {
    for input in [
        include_bytes!("tests/data/ps09_unique_parent_items_first.p21").as_slice(),
        include_bytes!("tests/data/ps09_unique_parent_items_reordered.p21").as_slice(),
    ] {
        let result = StepCodec::default()
            .decode(&mut Cursor::new(input), &DecodeOptions::default())
            .expect("decode PS-09 fixture");
        let first = result
            .ir()
            .model
            .occurrences
            .iter()
            .find(|occurrence| occurrence.id.as_str().contains("#16"))
            .expect("first child occurrence");
        assert_eq!(first.transform.rows()[0][3], 25.0);
        assert_eq!(first.transform.rows()[1][3], 0.0);
        let second = result
            .ir()
            .model
            .occurrences
            .iter()
            .find(|occurrence| occurrence.id.as_str().contains("#17"))
            .expect("second child occurrence");
        assert_eq!(second.transform.rows()[0][3], -10.0);
        assert_eq!(second.transform.rows()[1][3], 4.0);
        assert!(!result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == StepLossCode::NauoPlacementUnresolved.kind()));
        let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("resource allocation did not fail");
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn ps09_ambiguous_occurrence_owned_placements_remain_opaque() {
    let input = include_bytes!("tests/data/ps09_ambiguous_occurrence_items.p21");
    let result = EditableDecodeResult::from(
        StepCodec::default()
            .decode(&mut Cursor::new(input), &DecodeOptions::default())
            .expect("decode PS-09 ambiguous occurrence fixture"),
    );

    assert!(!result
        .ir()
        .model
        .occurrences
        .iter()
        .any(|occurrence| occurrence.id.as_str().contains("#16")));
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::NauoPlacementAmbiguous.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Error
            && loss.message.contains("NAUO #16")
            && loss.message.contains("occurrence-owned mapped")
            && loss.message.contains("#47")
            && loss.message.contains("#48")
            && loss.message.contains("no neutral occurrence was admitted")
            && loss.message.contains("remain opaque")
    }));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::NauoPlacementUnresolved.kind()));

    let unknowns = result.ir().native_unknowns("step").unwrap();
    let unknown_ids = unknowns
        .iter()
        .map(|record| record.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        unknown_ids,
        std::collections::BTreeSet::from([
            "step:data:next_assembly_usage_occurrence#16",
            "step:data:shape_definition_representation#47",
            "step:data:shape_definition_representation#48",
        ])
    );
    for id in &unknown_ids {
        let retained = result
            .source_fidelity()
            .retained_record(id)
            .expect("ambiguous occurrence placement source record is retained");
        assert_eq!(
            retained.data(),
            Some(
                &input[usize::try_from(retained.offset()).expect("retained offset fits memory")
                    ..usize::try_from(retained.offset() + retained.byte_len())
                        .expect("retained end fits memory")]
            )
        );
    }
}

#[test]
fn unrelated_representation_mapping_does_not_place_an_occurrence() {
    let result = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!(
                "tests/data/ps05_unrelated_mapped_representation.p21"
            )),
            &DecodeOptions::default(),
        )
        .expect("decode PS-05 fixture");

    let occurrence = result
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.id.as_str().contains("#16"))
        .expect("child occurrence");
    assert_eq!(
        occurrence.transform,
        cadmpeg_ir::transform::Transform::identity()
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::NauoPlacementUnresolved.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Error
            && loss.message.contains("NAUO #16")
    }));
}

#[test]
fn repeated_child_uses_without_owned_placements_remain_unresolved() {
    let result = decode_inline(
        "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('ROOT','Root assembly','',(#2));
#4=PRODUCT_DEFINITION_FORMATION('','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION('root definition','',#4,#5);
#7=PRODUCT_DEFINITION_SHAPE('','',#6);
#8=PRODUCT('CHILD','Child','',(#2));
#9=PRODUCT_DEFINITION_FORMATION('','',#8);
#10=PRODUCT_DEFINITION('child definition','',#9,#5);
#11=PRODUCT_DEFINITION_SHAPE('','',#10);
#16=NEXT_ASSEMBLY_USAGE_OCCURRENCE('one','First child','',#6,#10,$);
#17=NEXT_ASSEMBLY_USAGE_OCCURRENCE('two','Second child','',#6,#10,$);
#20=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#21=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#20)) REPRESENTATION_CONTEXT('model','3D'));
#22=SHAPE_REPRESENTATION('root',(#39,#41),#21);
#23=SHAPE_REPRESENTATION('child',(),#21);
#25=SHAPE_DEFINITION_REPRESENTATION(#7,#22);
#26=SHAPE_DEFINITION_REPRESENTATION(#11,#23);
#30=CARTESIAN_POINT('',(0.,0.,0.));
#31=CARTESIAN_POINT('',(25.,0.,0.));
#32=CARTESIAN_POINT('',(-10.,4.,0.));
#33=DIRECTION('',(0.,0.,1.));
#34=DIRECTION('',(1.,0.,0.));
#35=AXIS2_PLACEMENT_3D('',#30,#33,#34);
#36=AXIS2_PLACEMENT_3D('',#31,#33,#34);
#37=AXIS2_PLACEMENT_3D('',#32,#33,#34);
#38=REPRESENTATION_MAP(#35,#23);
#39=MAPPED_ITEM('First child',#38,#36);
#40=REPRESENTATION_MAP(#35,#23);
#41=MAPPED_ITEM('Second child',#40,#37);",
    );

    let children = result
        .ir()
        .model
        .occurrences
        .iter()
        .filter(|occurrence| {
            occurrence.id.as_str().contains("#16") || occurrence.id.as_str().contains("#17")
        })
        .collect::<Vec<_>>();
    assert_eq!(children.len(), 2);
    assert!(children
        .iter()
        .all(|occurrence| occurrence.transform == cadmpeg_ir::transform::Transform::identity()));
    for usage_id in [16, 17] {
        assert!(result.report().losses.iter().any(|loss| {
            loss.code == StepLossCode::NauoPlacementUnresolved.kind()
                && loss.severity == cadmpeg_ir::report::Severity::Error
                && loss.message.contains(&format!("NAUO #{usage_id}"))
        }));
    }
}

#[test]
fn ps01_repeated_child_binding_requires_occurrence_identity() {
    let decode_fixture = |bytes: &[u8]| {
        StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode PS-01 fixture")
    };

    let single = decode_fixture(include_bytes!(
        "tests/data/ps01_single_child_parent_mapping.p21"
    ));
    let single_child = single
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.id.as_str().contains("#12"))
        .expect("single child occurrence");
    assert_eq!(single_child.transform.rows()[0][3], 25.0);
    assert_eq!(single_child.transform.rows()[1][3], 0.0);
    assert!(!single
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::NauoPlacementUnresolved.kind()));

    for bytes in [
        &include_bytes!("tests/data/ps01_repeated_child_parent_mapping.p21")[..],
        &include_bytes!("tests/data/ps01_repeated_child_parent_mapping_reordered.p21")[..],
    ] {
        let repeated = decode_fixture(bytes);
        let children = repeated
            .ir()
            .model
            .occurrences
            .iter()
            .filter(|occurrence| {
                occurrence.id.as_str().contains("#12") || occurrence.id.as_str().contains("#13")
            })
            .collect::<Vec<_>>();
        assert_eq!(children.len(), 2);
        assert!(children
            .iter()
            .all(|occurrence| occurrence.transform == Transform::identity()));
        for usage_id in [12, 13] {
            assert!(repeated.report().losses.iter().any(|loss| {
                loss.code == StepLossCode::NauoPlacementUnresolved.kind()
                    && loss.message.contains(&format!("NAUO #{usage_id}"))
            }));
        }
    }

    let occurrence_owned = decode_fixture(include_bytes!(
        "tests/data/ps01_repeated_child_occurrence_mapping.p21"
    ));
    let mut children = occurrence_owned
        .ir()
        .model
        .occurrences
        .iter()
        .filter(|occurrence| {
            occurrence.id.as_str().contains("#12") || occurrence.id.as_str().contains("#13")
        })
        .collect::<Vec<_>>();
    children.sort_by_key(|occurrence| occurrence.id.clone());
    assert_eq!(children.len(), 2);
    assert_eq!(children[0].transform.rows()[0][3], 25.0);
    assert_eq!(children[0].transform.rows()[1][3], 0.0);
    assert_eq!(children[1].transform.rows()[0][3], -10.0);
    assert_eq!(children[1].transform.rows()[1][3], 4.0);
    assert!(!occurrence_owned
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::NauoPlacementUnresolved.kind()));
}

#[test]
fn mapped_child_unique_per_parent_uses_parent_local_uniqueness() {
    let result = decode_inline(
        "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('ROOT','Root assembly','',(#2));
#4=PRODUCT_DEFINITION_FORMATION('','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION('root definition','',#4,#5);
#7=PRODUCT_DEFINITION_SHAPE('','',#6);
#8=PRODUCT('PARENT-A','Parent A','',(#2));
#9=PRODUCT_DEFINITION_FORMATION('','',#8);
#10=PRODUCT_DEFINITION('parent A definition','',#9,#5);
#11=PRODUCT_DEFINITION_SHAPE('','',#10);
#12=PRODUCT('PARENT-B','Parent B','',(#2));
#13=PRODUCT_DEFINITION_FORMATION('','',#12);
#14=PRODUCT_DEFINITION('parent B definition','',#13,#5);
#15=PRODUCT_DEFINITION_SHAPE('','',#14);
#16=PRODUCT('CHILD','Child','',(#2));
#17=PRODUCT_DEFINITION_FORMATION('','',#16);
#18=PRODUCT_DEFINITION('child definition','',#17,#5);
#19=PRODUCT_DEFINITION_SHAPE('','',#18);
#20=NEXT_ASSEMBLY_USAGE_OCCURRENCE('a','Child A','',#10,#18,$);
#21=NEXT_ASSEMBLY_USAGE_OCCURRENCE('b','Child B','',#14,#18,$);
#22=NEXT_ASSEMBLY_USAGE_OCCURRENCE('pa','Parent A','',#6,#10,$);
#23=NEXT_ASSEMBLY_USAGE_OCCURRENCE('pb','Parent B','',#6,#14,$);
#30=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#31=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#30)) REPRESENTATION_CONTEXT('model','3D'));
#32=SHAPE_REPRESENTATION('root',(#53,#55),#31);
#33=SHAPE_REPRESENTATION('parent A',(#57),#31);
#34=SHAPE_REPRESENTATION('parent B',(#59),#31);
#35=SHAPE_REPRESENTATION('child',(),#31);
#36=SHAPE_DEFINITION_REPRESENTATION(#7,#32);
#37=SHAPE_DEFINITION_REPRESENTATION(#11,#33);
#38=SHAPE_DEFINITION_REPRESENTATION(#15,#34);
#39=SHAPE_DEFINITION_REPRESENTATION(#19,#35);
#40=CARTESIAN_POINT('',(0.,0.,0.));
#41=CARTESIAN_POINT('',(100.,0.,0.));
#42=CARTESIAN_POINT('',(200.,0.,0.));
#43=CARTESIAN_POINT('',(10.,0.,0.));
#44=CARTESIAN_POINT('',(20.,0.,0.));
#45=DIRECTION('',(0.,0.,1.));
#46=DIRECTION('',(1.,0.,0.));
#47=AXIS2_PLACEMENT_3D('',#40,#45,#46);
#48=AXIS2_PLACEMENT_3D('',#41,#45,#46);
#49=AXIS2_PLACEMENT_3D('',#42,#45,#46);
#50=AXIS2_PLACEMENT_3D('',#43,#45,#46);
#51=AXIS2_PLACEMENT_3D('',#44,#45,#46);
#52=REPRESENTATION_MAP(#47,#33);
#53=MAPPED_ITEM('Parent A',#52,#48);
#54=REPRESENTATION_MAP(#47,#34);
#55=MAPPED_ITEM('Parent B',#54,#49);
#56=REPRESENTATION_MAP(#47,#35);
#57=MAPPED_ITEM('Child A',#56,#50);
#58=REPRESENTATION_MAP(#47,#35);
#59=MAPPED_ITEM('Child B',#58,#51);",
    );

    let children = result
        .ir()
        .model
        .occurrences
        .iter()
        .filter(|occurrence| occurrence.name.is_some())
        .collect::<Vec<_>>();
    assert_eq!(children.len(), 4);
    assert!(children.iter().any(|occurrence| {
        occurrence.name.as_deref() == Some("Child A") && occurrence.transform.rows()[0][3] == 10.0
    }));
    assert!(children.iter().any(|occurrence| {
        occurrence.name.as_deref() == Some("Child B") && occurrence.transform.rows()[0][3] == 20.0
    }));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::NauoPlacementUnresolved.kind()));
}

#[test]
pub(crate) fn repeated_subassembly_instances_each_receive_the_subtree() {
    use cadmpeg_ir::products::{OccurrenceParent, PrototypeReference};

    let result = decode_inline(
        "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('P','parent','',(#2));
#4=PRODUCT_DEFINITION_FORMATION('','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION('parent','',#4,#5);
#7=PRODUCT('S','subassembly','',(#2));
#8=PRODUCT_DEFINITION_FORMATION('','',#7);
#9=PRODUCT_DEFINITION('subassembly','',#8,#5);
#10=PRODUCT('L','leaf','',(#2));
#11=PRODUCT_DEFINITION_FORMATION('','',#10);
#12=PRODUCT_DEFINITION('leaf','',#11,#5);
#20=NEXT_ASSEMBLY_USAGE_OCCURRENCE('u1','sub one','',#6,#9,$);
#21=NEXT_ASSEMBLY_USAGE_OCCURRENCE('u2','sub two','',#6,#9,$);
#22=NEXT_ASSEMBLY_USAGE_OCCURRENCE('u3','leaf','',#9,#12,$);",
    );
    assert_eq!(result.ir().model.occurrences.len(), 5);
    let subassemblies = result
        .ir()
        .model
        .occurrences
        .iter()
        .filter(|occurrence| {
            matches!(
                &occurrence.prototype,
                PrototypeReference::Local { definition }
                    if definition.as_str() == "step:product:product#7"
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(subassemblies.len(), 2);
    for subassembly in subassemblies {
        assert_eq!(
            result
                .ir()
                .model
                .occurrences
                .iter()
                .filter(|occurrence| matches!(
                    &occurrence.parent,
                    OccurrenceParent::Occurrence { occurrence: parent }
                        if parent == &subassembly.id
                ))
                .count(),
            1
        );
    }
}

#[test]
pub(crate) fn ap203_specified_source_formations_build_occurrence_tree() {
    let result = decode_inline(
        "#1=APPLICATION_CONTEXT('configuration controlled design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('A','assembly','',(#2));
#4=PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE('','',#3,.NOT_KNOWN.);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION('assembly','',#4,#5);
#7=PRODUCT('P','part','',(#2));
#8=PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE('','',#7,.NOT_KNOWN.);
#9=PRODUCT_DEFINITION('part','',#8,#5);
#10=NEXT_ASSEMBLY_USAGE_OCCURRENCE('u1','part instance','',#6,#9,$);",
    );

    assert_eq!(result.ir().model.product_definitions.len(), 2);
    assert_eq!(result.ir().model.occurrences.len(), 2);
    assert!(result
        .ir()
        .model
        .occurrences
        .iter()
        .any(|occurrence| matches!(
            &occurrence.prototype,
            cadmpeg_ir::products::PrototypeReference::Local { definition }
                if definition.as_str() == "step:product:product#7"
        )));
    assert!(!result
        .ir()
        .native_unknowns("step")
        .unwrap()
        .iter()
        .any(|record| {
            record.id.as_str().contains("product_definition_formation")
                || record
                    .id
                    .as_str()
                    .contains("next_assembly_usage_occurrence")
        }));
}

#[test]
fn product_definition_subtypes_preserve_assembly_occurrences() {
    use cadmpeg_ir::products::OccurrenceParent;

    let result = decode_inline(
        "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('ROOT','Root assembly','',(#2));
#4=PRODUCT_DEFINITION_FORMATION('','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION_WITH_ASSOCIATED_DOCUMENTS('root definition','',#4,#5,(#15));
#7=PRODUCT('CHILD','Child part','',(#2));
#8=PRODUCT_DEFINITION_FORMATION('','',#7);
#9=PRODUCT_DEFINITION('child definition','',#8,#5);
#10=NEXT_ASSEMBLY_USAGE_OCCURRENCE('occ-1','Placed child','',#6,#9,$);
#15=DOCUMENT('manual','assembly manual','');",
    );

    assert_eq!(result.ir().model.product_definitions.len(), 2);
    assert_eq!(result.ir().model.occurrences.len(), 2);
    let child = result
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("Placed child"))
        .expect("subtype-backed child occurrence");
    assert!(matches!(child.parent, OccurrenceParent::Occurrence { .. }));
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("NAUO #10 references an unresolved child definition")
    }));
}

#[test]
fn geometric_bounded_surface_representation_reaches_its_product() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_geometric_set.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#20=PRODUCT('P','bounded part','',());\n#21=PRODUCT_DEFINITION_FORMATION('','',#20);\n#22=APPLICATION_CONTEXT('mechanical design');\n#23=PRODUCT_DEFINITION_CONTEXT('part definition',#22,'design');\n#24=PRODUCT_DEFINITION('part','',#21,#23);\n#25=PRODUCT_DEFINITION_SHAPE('','',#24);\n#26=SHAPE_DEFINITION_REPRESENTATION(#25,#13);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode product-bound bounded surface");

    assert_eq!(decoded.ir().model.product_definitions.len(), 1);
    assert_eq!(decoded.ir().model.product_definitions[0].bodies.len(), 1);
    assert_eq!(
        decoded.ir().model.product_definitions[0].bodies[0].as_str(),
        "step:data:body#13"
    );
}

#[test]
fn shape_representation_relationship_reaches_its_product_body() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_geometric_set.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#14=PRODUCT('P','related shape part','',());\n#15=PRODUCT_DEFINITION_FORMATION('','',#14);\n#16=APPLICATION_CONTEXT('mechanical design');\n#17=PRODUCT_DEFINITION_CONTEXT('part definition',#16,'design');\n#18=PRODUCT_DEFINITION('part','',#15,#17);\n#19=PRODUCT_DEFINITION_SHAPE('','',#18);\n#20=SHAPE_DEFINITION_REPRESENTATION(#19,#21);\n#21=SHAPE_REPRESENTATION('',(),#2);\n#22=SHAPE_REPRESENTATION_RELATIONSHIP('','',#21,#13);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode related shape representation");

    assert_eq!(decoded.ir().model.product_definitions.len(), 1);
    assert_eq!(decoded.ir().model.product_definitions[0].bodies.len(), 1);
    assert_eq!(
        decoded.ir().model.product_definitions[0].bodies[0].as_str(),
        "step:data:body#13"
    );
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("has a shape representation with no committed topology body")
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_shape_representation_relationship_inherits_references() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_geometric_set.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#14=PRODUCT('P','complex related shape part','',());\n#15=PRODUCT_DEFINITION_FORMATION('','',#14);\n#16=APPLICATION_CONTEXT('mechanical design');\n#17=PRODUCT_DEFINITION_CONTEXT('part definition',#16,'design');\n#18=PRODUCT_DEFINITION('part','',#15,#17);\n#19=PRODUCT_DEFINITION_SHAPE('','',#18);\n#20=SHAPE_DEFINITION_REPRESENTATION(#19,#21);\n#21=SHAPE_REPRESENTATION('',(),#2);\n#22=(REPRESENTATION_RELATIONSHIP('','',#21,#13) SHAPE_REPRESENTATION_RELATIONSHIP());\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex related shape representation");

    assert_eq!(decoded.ir().model.product_definitions.len(), 1);
    assert_eq!(decoded.ir().model.product_definitions[0].bodies.len(), 1);
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("has a shape representation with no committed topology body")
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_applies_canonical_cartesian_operator_to_mapped_body() {
    let transform = cadmpeg_ir::transform::Transform::affine([
        [0.0, -1.0, 0.0, 15.0],
        [1.0, 0.0, 0.0, 4.0],
        [0.0, 0.0, 1.0, 2.0],
    ])
    .expect("affine transform");
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.bodies[0].transform = Some(transform);
    let mut output = Vec::new();
    write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write placed body");
    let mut source = String::from_utf8(output).expect("STEP output is UTF-8");

    let mapped_line = source
        .lines()
        .find(|line| line.contains("MAPPED_ITEM('cadmpeg body placement'"))
        .expect("mapped body item");
    let target = mapped_line
        .trim_end_matches(';')
        .trim_end_matches(')')
        .rsplit_once(',')
        .and_then(|(_, reference)| reference.strip_prefix('#'))
        .expect("mapped target reference")
        .parse::<u64>()
        .expect("mapped target id");
    let target_line = source
        .lines()
        .find(|line| {
            line.split_once('=')
                .is_some_and(|(id, _)| id.trim() == format!("#{target}"))
        })
        .expect("mapped target record");
    let parameters = target_line
        .split_once('(')
        .and_then(|(_, value)| value.strip_suffix(");"))
        .expect("mapped target parameters")
        .split(',')
        .collect::<Vec<_>>();
    assert_eq!(parameters.len(), 4, "unexpected placement target");
    let origin = parameters[1];
    let axis_z = parameters[2];
    let axis_x = parameters[3];
    let next_id = source
        .lines()
        .filter_map(|line| {
            line.strip_prefix('#')
                .and_then(|line| line.split_once('='))
                .and_then(|(id, _)| id.trim().parse::<u64>().ok())
        })
        .max()
        .expect("STEP entity ids")
        + 1;
    let axis_y = format!("#{next_id}");
    let replacement = format!(
        "#{target}=CARTESIAN_TRANSFORMATION_OPERATOR_3D('','','',{axis_x},{axis_y},{origin},1.,{axis_z});"
    );
    source = source.replace(target_line, &replacement);
    let insert_at = source.rfind("ENDSEC;").expect("data section terminator");
    source.insert_str(
        insert_at,
        &format!(
            "#{next_id}=DIRECTION('',({},{},{}));\n",
            transform.rows()[0][1],
            transform.rows()[1][1],
            transform.rows()[2][1]
        ),
    );

    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(source.into_bytes()),
            &DecodeOptions::default(),
        )
        .expect("decode mapped body with canonical operator");
    assert_eq!(decoded.ir().model.bodies[0].transform, Some(transform));
}

#[test]
fn representation_relationships_and_mapped_children_do_not_duplicate_definition_bodies() {
    let sheet = std::str::from_utf8(include_bytes!("../../../tests/fixtures/ap214_sheet.p21"))
        .expect("sheet source");
    let products = "#100=APPLICATION_CONTEXT('design');#101=PRODUCT_CONTEXT('',#100,'mechanical');#102=PRODUCT('P','Parent','',(#101));#103=PRODUCT('C','Child','',(#101));#104=PRODUCT_DEFINITION_FORMATION('','',#102);#105=PRODUCT_DEFINITION_FORMATION('','',#103);#106=PRODUCT_DEFINITION_CONTEXT('',#100,'design');#107=PRODUCT_DEFINITION('','',#104,#106);#108=PRODUCT_DEFINITION('','',#105,#106);#109=PRODUCT_DEFINITION_SHAPE('','',#107);#110=PRODUCT_DEFINITION_SHAPE('','',#108);#111=SHAPE_REPRESENTATION('parent',(),#2);#112=SHAPE_DEFINITION_REPRESENTATION(#109,#111);#113=SHAPE_DEFINITION_REPRESENTATION(#110,#32);#114=NEXT_ASSEMBLY_USAGE_OCCURRENCE('child','','',#107,#108,$);";
    for relationship in [
        "#115=SHAPE_REPRESENTATION_RELATIONSHIP('','',#111,#32);",
        "#115=SHAPE_REPRESENTATION_RELATIONSHIP('','',#32,#111);",
        "#115=REPRESENTATION_MAP(#27,#32);#116=MAPPED_ITEM('',#115,#27);",
    ] {
        let mut source = sheet.replace(
            "ENDSEC;
END-ISO-10303-21;",
            &format!(
                "{products}{relationship}ENDSEC;
END-ISO-10303-21;"
            ),
        );
        if relationship.contains("MAPPED_ITEM") {
            source = source.replace(
                "SHAPE_REPRESENTATION('parent',(),#2)",
                "SHAPE_REPRESENTATION('parent',(#116),#2)",
            );
        }
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("assembly");
        let parent = decoded
            .ir()
            .model
            .product_definitions
            .iter()
            .find(|p| p.source_name.as_deref() == Some("Parent"))
            .expect("parent");
        let child = decoded
            .ir()
            .model
            .product_definitions
            .iter()
            .find(|p| p.source_name.as_deref() == Some("Child"))
            .expect("child");
        assert!(
            parent.bodies.is_empty(),
            "parent owns the occurrence, not its child's body"
        );
        assert_eq!(child.bodies.len(), 1);
        assert_eq!(decoded.ir().model.faces.len(), 1);
    }
}
