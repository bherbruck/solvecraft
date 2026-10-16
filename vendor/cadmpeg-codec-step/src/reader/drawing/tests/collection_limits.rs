// SPDX-License-Identifier: Apache-2.0
//! Collection and nesting limits for drawing discovery.

use std::collections::{BTreeMap, HashSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const HEADER: &str = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;";
const TAIL: &str = "ENDSEC;END-ISO-10303-21;";

fn drawing_refuses(records: &str, operation: &str) {
    let source = format!("{HEADER}{records}{TAIL}");
    drawing_refuses_source(source.as_bytes(), operation);
}

fn drawing_refuses_source(source: &[u8], operation: &str) {
    drawing_refuses_source_with_typed(source, operation, &[]);
}

fn drawing_refuses_source_with_typed(source: &[u8], operation: &str, typed: &[u64]) {
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid drawing exchange");
    let known_typed = typed.iter().copied().collect::<HashSet<_>>();
    let refused = (0..=256).any(|limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
            .expect("root fits collection policy");
        matches!(
            super::super::decode(
                &exchange,
                &mut cadmpeg_ir::document::CadIr::empty(),
                &known_typed,
                &BTreeMap::new(),
                &ctx,
            ),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == operation
        )
    });
    assert!(refused, "no collection limit refused {operation}");
}

fn drawing_retained_refuses_with_typed(records: &str, operation: &str, typed: &[u64]) {
    let source = format!("{HEADER}{records}{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid drawing exchange");
    let known_typed = typed.iter().copied().collect::<HashSet<_>>();
    let refused = {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            operation,
            |limit| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = limit;
                let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
                    .expect("root fits retained policy");

                (super::super::decode(
                    &exchange,
                    &mut cadmpeg_ir::document::CadIr::empty(),
                    &known_typed,
                    &BTreeMap::new(),
                    &ctx,
                ))
                .map(|_| ())
            },
        );
        matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == operation)
    };
    assert!(refused, "no retained limit refused {operation}");
}

#[test]
fn drawing_candidates_refuse_collection_limit() {
    drawing_refuses(
        "#1=DRAWING_DEFINITION('Main','detail');",
        "step_drawing_candidates",
    );
}

#[test]
fn drawing_discovery_losses_refuse_collection_limit() {
    drawing_refuses("#1=DRAWING_DEFINITION('Only one');", "step_drawing_losses");
}

#[test]
fn drawing_ids_refuse_collection_limit() {
    drawing_refuses(
        "#1=DRAWING_DEFINITION('Main','detail');",
        "step_drawing_ids",
    );
}

#[test]
fn hidden_drawing_ids_refuse_collection_limit() {
    drawing_refuses(
        "#1=DRAWING_DEFINITION('Main','detail');#2=INVISIBILITY((#1));",
        "step_hidden_drawing_ids",
    );
}

#[test]
fn drawing_reference_walk_refuses_depth_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits depth policy");
    let value = crate::parse::Value::List(vec![crate::parse::Value::Reference(1)]);
    assert!(matches!(
        super::super::visit_drawing_references(&value, &ctx, &mut |_| Ok(())),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_drawing_reference_walk"
    ));
}

#[test]
fn drawing_target_groups_refuse_collection_limit() {
    drawing_refuses(
        "#1=DRAWING_DEFINITION('Main','detail');",
        "step_drawing_target_groups",
    );
}

#[test]
fn drawing_target_members_refuse_collection_limit() {
    drawing_refuses(
        "#1=DRAWING_DEFINITION('Main','detail');",
        "step_drawing_target_members",
    );
}

#[test]
fn drawing_external_documents_refuse_collection_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;#100=<part.step#root>;ENDSEC;DATA;#1=DRAWING_DEFINITION('Main','detail');ENDSEC;END-ISO-10303-21;";
    drawing_refuses_source(source, "step_drawing_external_documents");
}

#[test]
fn drawing_stored_parameters_refuse_collection_limit() {
    drawing_refuses(
        "#1=DRAWING_DEFINITION('Main','detail');",
        "step_drawing_stored_parameters",
    );
}

#[test]
fn drawing_entries_refuse_collection_limit() {
    drawing_refuses(
        "#1=DRAWING_DEFINITION('Main','detail');",
        "step_drawing_entries",
    );
}

#[test]
fn drawing_typed_claims_refuse_collection_limit() {
    drawing_refuses(
        "#1=DRAWING_DEFINITION('Main','detail');",
        "step_drawing_typed_claims",
    );
}

#[test]
fn drawing_ir_items_refuse_collection_limit() {
    drawing_refuses(
        "#1=DRAWING_DEFINITION('Main','detail');",
        "step_drawing_ir_items",
    );
}

#[test]
fn drawing_referenced_targets_refuse_collection_limit() {
    drawing_refuses(
        "#1=DRAWING_DEFINITION('Main','detail');#2=DRAWING_REVISION('A',#1,'revision');",
        "step_drawing_referenced_targets",
    );
}

const TYPED_TARGET_SOURCE: &str =
    "#1=REPRESENTATION_CONTEXT('','');#2=PRESENTATION_VIEW('Front',(#3),#1);#3=ITEM('opaque');";

#[test]
fn drawing_source_type_text_refuses_retained_limit() {
    drawing_retained_refuses_with_typed(TYPED_TARGET_SOURCE, "step_drawing_source_type_text", &[3]);
}

#[test]
fn drawing_native_target_identity_copy_refuses_retained_limit() {
    drawing_retained_refuses_with_typed(
        TYPED_TARGET_SOURCE,
        "step_drawing_native_target_identity_copy",
        &[3],
    );
}

#[test]
fn drawing_native_target_items_refuse_collection_limit() {
    let source = format!("{HEADER}{TYPED_TARGET_SOURCE}{TAIL}");
    drawing_refuses_source_with_typed(source.as_bytes(), "step_drawing_native_target_items", &[3]);
}

#[test]
fn drawing_native_target_groups_refuse_collection_limit() {
    let source = format!("{HEADER}{TYPED_TARGET_SOURCE}{TAIL}");
    drawing_refuses_source_with_typed(source.as_bytes(), "step_drawing_native_target_groups", &[3]);
}

#[test]
fn drawing_native_target_members_refuse_collection_limit() {
    let source = format!("{HEADER}{TYPED_TARGET_SOURCE}{TAIL}");
    drawing_refuses_source_with_typed(
        source.as_bytes(),
        "step_drawing_native_target_members",
        &[3],
    );
}

#[test]
fn drawing_native_arena_refuses_collection_limit() {
    let source = format!("{HEADER}{TYPED_TARGET_SOURCE}{TAIL}");
    drawing_refuses_source_with_typed(source.as_bytes(), "step_drawing_native_arena", &[3]);
}

#[test]
fn drawing_native_arena_items_refuse_collection_limit() {
    let source = format!("{HEADER}{TYPED_TARGET_SOURCE}{TAIL}");
    drawing_refuses_source_with_typed(source.as_bytes(), "step_drawing_native_arena_items", &[3]);
}

#[test]
fn drawing_ambiguous_identities_text_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits retained policy");
    let identities = ["first", "second"].into_iter().map(str::to_owned).collect();
    assert!(matches!(
        super::super::note_ambiguous_target(
            &mut Vec::new(), "drawing #1", "items", 2, &identities, &ctx,
        ),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_drawing_ambiguous_identities_text"
    ));
}

#[test]
fn drawing_ambiguous_loss_text_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "step_drawing_ambiguous_loss_text",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
                .expect("empty root fits retained policy");
            let identities = ["first", "second"].into_iter().map(str::to_owned).collect();
            super::super::note_ambiguous_target(
                &mut Vec::new(),
                "drawing #1",
                "items",
                2,
                &identities,
                &ctx,
            )
        },
    );
    assert!(
        matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_drawing_ambiguous_loss_text")
    );
}

#[test]
fn drawing_ambiguous_loss_slot_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits collection policy");
    let identities = ["first", "second"].into_iter().map(str::to_owned).collect();
    assert!(matches!(
        super::super::note_ambiguous_target(
            &mut Vec::new(), "drawing #1", "items", 2, &identities, &ctx,
        ),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_drawing_losses"
    ));
}

#[test]
fn drawing_relationship_groups_refuse_collection_limit() {
    drawing_refuses(
        "#1=DRAWING_DEFINITION('Main','detail');#2=DRAWING_REVISION('A',#1,'revision');",
        "step_drawing_relationship_groups",
    );
}

#[test]
fn drawing_relationship_members_refuse_collection_limit() {
    drawing_refuses(
        "#1=DRAWING_DEFINITION('Main','detail');#2=DRAWING_REVISION('A',#1,'revision');",
        "step_drawing_relationship_members",
    );
}

#[test]
fn drawing_usage_sequences_refuse_collection_limit() {
    drawing_refuses(
        "#1=DRAWING_DEFINITION('Main','detail');#2=DRAWING_REVISION('A',#1,'revision');#3=REPRESENTATION_CONTEXT('','');#4=DRAWING_SHEET_REVISION('Sheet',(),#3,#2);#5=DRAWING_SHEET_REVISION_USAGE(#4,#2,'one');",
        "step_drawing_usage_sequences",
    );
}

#[test]
fn drawing_association_claims_refuse_collection_limit() {
    drawing_refuses(
        "#1=REPRESENTATION_CONTEXT('','');#2=DRAUGHTING_MODEL('Model',(),#1);#3=ITEM('semantic');#4=DRAUGHTING_MODEL_ITEM_ASSOCIATION('','',#3,#2,(#3));",
        "step_drawing_typed_claims",
    );
}

fn wrapper_refuses_collection(operation: &str) {
    let source = format!("{HEADER}#1=ANNOTATION_PLANE('','',#2);#2=ITEM();{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid wrapper exchange");
    let targets = BTreeMap::from([(2, ["target".to_owned()].into_iter().collect())]);
    let refused = (0..=16).any(|limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
            .expect("root fits collection policy");
        matches!(
            super::super::wrapper_target_resolution(1, &targets, &exchange, &ctx),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == operation
        )
    });
    assert!(refused, "no collection limit refused {operation}");
}

#[test]
fn drawing_wrapper_pending_refuses_collection_limit() {
    wrapper_refuses_collection("step_drawing_wrapper_pending");
}

#[test]
fn drawing_wrapper_active_refuses_collection_limit() {
    wrapper_refuses_collection("step_drawing_wrapper_active");
}

#[test]
fn drawing_wrapper_complete_refuses_collection_limit() {
    wrapper_refuses_collection("step_drawing_wrapper_complete");
}

#[test]
fn drawing_wrapper_identities_refuse_collection_limit() {
    wrapper_refuses_collection("step_drawing_wrapper_identities");
}

#[test]
fn drawing_wrapper_identity_text_refuses_retained_limit() {
    let source = format!("{HEADER}#1=ANNOTATION_PLANE('','',#2);#2=ITEM();{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid wrapper exchange");
    let targets = BTreeMap::from([(2, ["target".to_owned()].into_iter().collect())]);
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "step_drawing_wrapper_identity_text",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
                .expect("root fits retained policy");
            (super::super::wrapper_target_resolution(1, &targets, &exchange, &ctx)).map(|_| ())
        },
    );
    assert!(
        matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_drawing_wrapper_identity_text")
    );
}

#[test]
fn drawing_ambiguous_identity_copy_refuses_collection_limit() {
    let source = format!("{HEADER}#1=ITEM();{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid target exchange");
    let targets = BTreeMap::from([(
        1,
        ["first".to_owned(), "second".to_owned()]
            .into_iter()
            .collect(),
    )]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("root fits collection policy");
    assert!(matches!(
        super::super::target_resolution(1, &targets, &HashSet::from([1]), &exchange, &BTreeMap::new(), &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_drawing_ambiguous_identity_copy"
    ));
}

#[test]
fn drawing_ambiguous_identity_text_refuses_retained_limit() {
    let source = format!("{HEADER}#1=ITEM();{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid target exchange");
    let targets = BTreeMap::from([(
        1,
        ["first".to_owned(), "second".to_owned()]
            .into_iter()
            .collect(),
    )]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("root fits retained policy");
    assert!(matches!(
        super::super::target_resolution(1, &targets, &HashSet::from([1]), &exchange, &BTreeMap::new(), &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_drawing_ambiguous_identity_text"
    ));
}

#[test]
fn drawing_local_target_text_refuses_retained_limit() {
    let source = format!("{HEADER}#1=ITEM();{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid target exchange");
    let targets = BTreeMap::from([(1, ["local-target".to_owned()].into_iter().collect())]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("root fits retained policy");
    assert!(matches!(
        super::super::target_resolution(1, &targets, &HashSet::new(), &exchange, &BTreeMap::new(), &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_drawing_local_target_text"
    ));
}

#[test]
fn drawing_external_target_text_refuses_retained_limit() {
    let source = format!("{HEADER}#1=ITEM();{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid target exchange");
    let documents = BTreeMap::from([(1, "long-external-uri")]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("root fits retained policy");
    assert!(matches!(
        super::super::target_resolution(1, &BTreeMap::new(), &HashSet::new(), &exchange, &documents, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_drawing_external_target_text"
    ));
}

#[test]
fn drawing_untyped_relationship_loss_refuses_collection_limit() {
    let source = format!("{HEADER}#1=REPRESENTATION_CONTEXT('','');#2=PRESENTATION_VIEW('Front',(#3),#1);#3=ITEM();{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid drawing exchange");
    let record = exchange.records().get(&2).expect("view record");
    let parameters = super::super::source_parameters(record, "PRESENTATION_VIEW");
    let refused = (0..=128).any(|limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
            .expect("root fits collection policy");
        let identities = BTreeMap::new();
        let typed = HashSet::from([3]);
        let documents = BTreeMap::new();
        let targets = super::super::TargetContext {
            target_identities: &identities,
            known_typed: &typed,
            exchange: &exchange,
            external_documents: &documents,
            ctx: &ctx,
        };
        matches!(
            super::super::add_reference_fields(
                &mut BTreeMap::new(), "PRESENTATION_VIEW", parameters, 2, &targets, &mut Vec::new(),
            ),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "step_drawing_losses"
        )
    });
    assert!(
        refused,
        "no collection limit refused the drawing relationship loss"
    );
}

fn decoded_drawings(
    source: &[u8],
    exchange: &crate::parse::Exchange,
) -> BTreeMap<u64, cadmpeg_ir::drawings::Drawing> {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("root fits default policy");
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    super::super::decode(exchange, &mut ir, &HashSet::new(), &BTreeMap::new(), &ctx)
        .expect("valid drawing graph");
    ir.model
        .drawings
        .into_iter()
        .map(|drawing| {
            let id = drawing
                .native_ref
                .rsplit_once('#')
                .expect("drawing source identity")
                .1
                .parse()
                .expect("numeric source identity");
            (id, drawing)
        })
        .collect()
}

fn sheet_usage_loss_refuses(typed_id: u64) {
    let source = format!("{HEADER}#1=DRAWING_DEFINITION('Main','detail');#2=DRAWING_REVISION('A',#1,'revision');#3=REPRESENTATION_CONTEXT('','');#4=DRAWING_SHEET_REVISION('Sheet',(),#3,#2);#5=DRAWING_SHEET_REVISION_USAGE(#4,#2,'one');{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid sheet exchange");
    let baseline = decoded_drawings(source.as_bytes(), &exchange);
    let refused = (0..=128).any(|limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
            .expect("root fits collection policy");
        let identities = BTreeMap::new();
        let typed = HashSet::from([typed_id]);
        let documents = BTreeMap::new();
        let targets = super::super::TargetContext {
            target_identities: &identities,
            known_typed: &typed,
            exchange: &exchange,
            external_documents: &documents,
            ctx: &ctx,
        };
        matches!(
            super::super::add_sheet_revision_usages(&exchange, &mut baseline.clone(), &targets, &mut Vec::new(), &ctx),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "step_drawing_losses"
        )
    });
    assert!(refused, "no collection limit refused sheet usage loss");
}

#[test]
fn drawing_sheet_unresolved_loss_refuses_collection_limit() {
    sheet_usage_loss_refuses(2);
}

#[test]
fn drawing_revision_unresolved_loss_refuses_collection_limit() {
    sheet_usage_loss_refuses(4);
}

fn association_loss_refuses(typed_id: u64) {
    let source = format!("{HEADER}#1=REPRESENTATION_CONTEXT('','');#2=DRAUGHTING_MODEL('Model',(),#1);#3=ITEM('semantic');#4=DRAUGHTING_MODEL_ITEM_ASSOCIATION_WITH_PLACEHOLDER('','',#3,#2,(#5),#6);#5=ITEM('associated');#6=ANNOTATION_PLACEHOLDER_OCCURRENCE('placeholder',(),#5,.GPS_DATA.,$);{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid association exchange");
    let baseline = decoded_drawings(source.as_bytes(), &exchange);
    let refused = (0..=128).any(|limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
            .expect("root fits collection policy");
        let identities = BTreeMap::new();
        let typed_records = HashSet::from([typed_id]);
        let documents = BTreeMap::new();
        let targets = super::super::TargetContext {
            target_identities: &identities,
            known_typed: &typed_records,
            exchange: &exchange,
            external_documents: &documents,
            ctx: &ctx,
        };
        matches!(
            super::super::add_draughting_model_associations(
                &exchange, &mut baseline.clone(), &targets, &mut Vec::new(), &mut HashSet::new(),
            ),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "step_drawing_losses"
        )
    });
    assert!(refused, "no collection limit refused association loss");
}

#[test]
fn drawing_semantic_definition_loss_refuses_collection_limit() {
    association_loss_refuses(3);
}

#[test]
fn drawing_associated_item_loss_refuses_collection_limit() {
    association_loss_refuses(5);
}

#[test]
fn drawing_placeholder_loss_refuses_collection_limit() {
    association_loss_refuses(6);
}
