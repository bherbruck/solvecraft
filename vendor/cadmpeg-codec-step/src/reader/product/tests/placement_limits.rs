// SPDX-License-Identifier: Apache-2.0
//! Collection limits for occurrence placement lookup.

const CONTEXT_SOURCE: &[u8] = include_bytes!("../../../../tests/fixtures/ap242_assembly.p21");
const MAPPED_SOURCE: &[u8] = include_bytes!("../../../../tests/fixtures/ap242_mapped_assembly.p21");

fn duplicate_context_source() -> String {
    String::from_utf8_lossy(CONTEXT_SOURCE).replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#39=CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#37,#13);\nENDSEC;\nEND-ISO-10303-21;",
    )
}

fn occurrence_mapped_source(duplicate: bool) -> String {
    let source = String::from_utf8_lossy(MAPPED_SOURCE)
        .replace(
            "#22=SHAPE_REPRESENTATION('root shape',(#40),#21);",
            if duplicate {
                "#22=SHAPE_REPRESENTATION('root shape',(#40,#42),#21);"
            } else {
                "#22=SHAPE_REPRESENTATION('root shape',(#40),#21);"
            },
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#13=PRODUCT_DEFINITION_SHAPE('','',#12);\n#41=SHAPE_DEFINITION_REPRESENTATION(#13,#22);\n#42=MAPPED_ITEM('Second child',#39,#35);\nENDSEC;\nEND-ISO-10303-21;",
        );
    source
}

fn competing_source() -> String {
    String::from_utf8_lossy(CONTEXT_SOURCE)
        .replace(
            "#22=SHAPE_REPRESENTATION('root shape',(),#21);",
            "#22=SHAPE_REPRESENTATION('root shape',(#45),#21);",
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#43=SHAPE_DEFINITION_REPRESENTATION(#13,#22);\n#44=REPRESENTATION_MAP(#34,#23);\n#45=MAPPED_ITEM('Mapped child',#44,#35);\nENDSEC;\nEND-ISO-10303-21;",
        )
}

#[test]
fn context_candidate_groups_refuse_collection_limit() {
    super::product_collection_refuses_source(CONTEXT_SOURCE, "step_context_candidate_groups");
}

#[test]
fn context_candidate_members_refuse_collection_limit() {
    super::product_collection_refuses_source(CONTEXT_SOURCE, "step_context_candidate_members");
}

#[test]
fn occurrence_placement_results_refuse_collection_limit() {
    super::product_collection_refuses_source(CONTEXT_SOURCE, "step_occurrence_placement_results");
}

#[test]
fn sibling_usage_counts_refuse_collection_limit() {
    super::product_collection_refuses_source(CONTEXT_SOURCE, "step_sibling_usage_counts");
}

#[test]
fn ambiguous_context_source_copy_refuses_collection_limit() {
    let source = duplicate_context_source();
    super::product_collection_refuses_source(
        source.as_bytes(),
        "step_ambiguous_context_source_copy",
    );
}

#[test]
fn ambiguous_placement_groups_refuse_collection_limit() {
    let source = duplicate_context_source();
    super::product_collection_refuses_source(source.as_bytes(), "step_ambiguous_placement_groups");
}

#[test]
fn occurrence_representation_groups_refuse_collection_limit() {
    let source = occurrence_mapped_source(false);
    super::product_collection_refuses_source(
        source.as_bytes(),
        "step_occurrence_representation_groups",
    );
}

#[test]
fn occurrence_representation_members_refuse_collection_limit() {
    let source = occurrence_mapped_source(false);
    super::product_collection_refuses_source(
        source.as_bytes(),
        "step_occurrence_representation_members",
    );
}

#[test]
fn occurrence_placement_candidates_refuse_collection_limit() {
    let source = occurrence_mapped_source(false);
    super::product_collection_refuses_source(
        source.as_bytes(),
        "step_occurrence_placement_candidates",
    );
}

#[test]
fn ambiguous_mapped_sources_refuse_collection_limit() {
    let source = occurrence_mapped_source(true);
    super::product_collection_refuses_source(source.as_bytes(), "step_ambiguous_mapped_sources");
}

#[test]
fn competing_context_source_copy_refuses_collection_limit() {
    let source = competing_source();
    super::product_collection_refuses_source(
        source.as_bytes(),
        "step_competing_context_source_copy",
    );
}

#[test]
fn competing_mapped_sources_refuse_collection_limit() {
    let source = competing_source();
    super::product_collection_refuses_source(source.as_bytes(), "step_competing_mapped_sources");
}

#[test]
fn competing_source_copy_refuses_collection_limit() {
    let source = competing_source();
    super::product_collection_refuses_source(source.as_bytes(), "step_competing_source_copy");
}

#[test]
fn competing_placement_groups_refuse_collection_limit() {
    let source = competing_source();
    super::product_collection_refuses_source(source.as_bytes(), "step_competing_placement_groups");
}

#[test]
fn fallback_occurrence_placements_refuse_collection_limit() {
    super::product_collection_refuses_source(MAPPED_SOURCE, "step_fallback_occurrence_placements");
}

#[test]
fn ambiguous_placement_source_text_refuses_retained_limit() {
    let source = duplicate_context_source();
    super::product_retained_refuses_source(
        source.as_bytes(),
        "step_ambiguous_placement_source_text",
    );
}

#[test]
fn ambiguous_placement_loss_text_refuses_retained_limit() {
    let source = duplicate_context_source();
    super::product_retained_refuses_source(source.as_bytes(), "step_ambiguous_placement_loss_text");
}

#[test]
fn competing_placement_source_text_refuses_retained_limit() {
    let source = competing_source();
    super::product_retained_refuses_source(
        source.as_bytes(),
        "step_competing_placement_source_text",
    );
}

#[test]
fn competing_placement_loss_text_refuses_retained_limit() {
    let source = competing_source();
    super::product_retained_refuses_source(source.as_bytes(), "step_competing_placement_loss_text");
}

#[test]
fn body_conflict_source_text_refuses_retained_limit() {
    let source = super::mapped_body_placement_source();
    super::product_retained_refuses_source(source.as_bytes(), "step_body_conflict_source_text");
}

#[test]
fn body_conflict_loss_text_refuses_retained_limit() {
    let source = super::mapped_body_placement_source();
    super::product_retained_refuses_source(source.as_bytes(), "step_body_conflict_loss_text");
}

#[test]
fn missing_shape_body_text_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits retained policy");
    assert!(matches!(
        super::super::join_product_texts(["body-one", "body-two"], &ctx, "step_missing_shape_body_text"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_missing_shape_body_text"
    ));
}

#[test]
fn missing_shape_body_loss_text_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits retained policy");
    assert!(matches!(
        ctx.format_retained(format_args!("body omitted uncommitted shape body reference(s): {}", "body-one"), "step_missing_shape_body_loss_text"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_missing_shape_body_loss_text"
    ));
}
