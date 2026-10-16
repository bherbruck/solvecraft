// SPDX-License-Identifier: Apache-2.0
//! Resource refusals from STEP PMI string fields.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

fn source(records: &str) -> String {
    format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;")
}

fn pmi_result(records: &str, retained_limit: u64) -> Result<(), CodecError> {
    let source = source(records);
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid PMI exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("root fits retained policy");
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let (geometry, _) = crate::reader::geometry::decode(&exchange, &mut ir, &ctx)?;
    let index = crate::reader::index::CarrierIndex::from_ir(&ir, &ctx)?;
    let topology = crate::reader::topology::decode(&exchange, &mut ir, &index, &ctx)?;
    super::super::decode(&exchange, &geometry.value, &topology.value, &mut ir, &ctx)?;
    Ok(())
}

macro_rules! pmi_string_limit_test {
    ($name:ident, $records:literal, $limit:expr) => {
        #[test]
        fn $name() {
            assert!(matches!(
                Err::<(), CodecError>(cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::RetainedBytes, "step_string_text", |cap| pmi_result($records, cap))),
                Err(CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::RetainedBytes
                        && refusal.operation == "step_string_text"
            ));
        }
    };
}

pmi_string_limit_test!(
    datum_identification_refuses_retained_limit,
    "#1=DATUM('identifier');",
    1
);
pmi_string_limit_test!(
    datum_name_refuses_retained_limit,
    "#1=(DATUM('') SHAPE_ASPECT('datum name','',#2,.F.));#2=ITEM();",
    1
);
pmi_string_limit_test!(
    datum_target_form_refuses_retained_limit,
    "#1=DATUM_TARGET('','rectangle',#2,.F.,'');#2=ITEM();",
    1
);
pmi_string_limit_test!(
    datum_target_identification_refuses_retained_limit,
    "#1=DATUM_TARGET('','',#2,.F.,'identifier');#2=ITEM();",
    1
);
pmi_string_limit_test!(
    datum_target_name_refuses_retained_limit,
    "#1=DATUM_TARGET('target name','',#2,.F.,'');#2=ITEM();",
    1
);
pmi_string_limit_test!(
    datum_system_name_refuses_retained_limit,
    "#1=DATUM_SYSTEM('system name','',#2,.F.,());#2=ITEM();",
    1
);
pmi_string_limit_test!(
    dimension_name_refuses_retained_limit,
    "#1=DIMENSIONAL_SIZE(#2,'dimension name');#2=ITEM();",
    1
);
pmi_string_limit_test!(
    dimension_category_refuses_retained_limit,
    "#1=DIMENSIONAL_SIZE_WITH_DATUM_FEATURE(#2,'diameter');#2=ITEM();",
    8
);
pmi_string_limit_test!(
    limits_form_variance_refuses_retained_limit,
    "#1=PLUS_MINUS_TOLERANCE(#2);#2=LIMITS_AND_FITS('form variance','','','');",
    1
);
pmi_string_limit_test!(
    limits_zone_variance_refuses_retained_limit,
    "#1=PLUS_MINUS_TOLERANCE(#2);#2=LIMITS_AND_FITS('','zone variance','','');",
    1
);
pmi_string_limit_test!(
    limits_grade_refuses_retained_limit,
    "#1=PLUS_MINUS_TOLERANCE(#2);#2=LIMITS_AND_FITS('','','grade value','');",
    1
);
pmi_string_limit_test!(
    limits_source_refuses_retained_limit,
    "#1=PLUS_MINUS_TOLERANCE(#2);#2=LIMITS_AND_FITS('','','','source value');",
    1
);
pmi_string_limit_test!(geometric_tolerance_name_refuses_retained_limit, "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));#2=(LENGTH_MEASURE_WITH_UNIT() MEASURE_WITH_UNIT(LENGTH_MEASURE(0.05),#1));#3=ITEM();#4=FLATNESS_TOLERANCE('tol name','',#2,#3);", 1);
pmi_string_limit_test!(
    presentation_annotation_name_refuses_retained_limit,
    "#1=ANNOTATION_TEXT_OCCURRENCE('annotation name',());",
    1
);

#[test]
fn annotation_text_refuses_retained_limit() {
    let source = source("#1=TEXT_LITERAL('annotation text',$,'left',.RIGHT.,$);");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid text exchange");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "step_string_text",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
                .expect("root fits retained policy");
            let mut visited = BTreeSet::new();
            let mut candidates = BTreeMap::new();
            let mut losses = Vec::<LossNote>::new();
            super::super::collect_annotation_text(
                1,
                &exchange,
                &mut visited,
                &mut candidates,
                &mut losses,
                0,
                &ctx,
            )
        },
    );
    assert!(
        matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_string_text")
    );
}

#[test]
fn measure_item_name_refuses_retained_limit() {
    let source = source("#1=(MEASURE_REPRESENTATION_ITEM() REPRESENTATION_ITEM('measure name'));");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid measure exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("root fits retained policy");
    let record = exchange.records().get(&1).expect("measure record");
    assert!(matches!(
        super::super::measure_item_name(1, record, &exchange, &mut Vec::new(), &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_string_text"
    ));
}

fn annotation_collection_result(limit: u64, consume_text: bool) -> Result<(), CodecError> {
    let source = source("#1=TEXT_LITERAL('annotation text',$,'left',.RIGHT.,$);");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid text exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("root fits collection policy");
    let mut visited = BTreeSet::new();
    let mut losses = Vec::new();
    if consume_text {
        super::super::find_annotation_text(
            1,
            &exchange,
            &mut visited,
            &mut BTreeSet::new(),
            &mut losses,
            0,
            &ctx,
        )?;
    } else {
        super::super::collect_annotation_text(
            1,
            &exchange,
            &mut visited,
            &mut BTreeMap::new(),
            &mut losses,
            0,
            &ctx,
        )?;
    }
    Ok(())
}

macro_rules! annotation_collection_limit_test {
    ($name:ident, $limit:expr, $consume:expr, $operation:literal) => {
        #[test]
        fn $name() {
            assert!(matches!(
                annotation_collection_result($limit, $consume),
                Err(CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::CollectionItems
                        && refusal.operation == $operation
            ));
        }
    };
}

annotation_collection_limit_test!(
    annotation_text_visited_refuses_collection_limit,
    0,
    false,
    "step_pmi_annotation_text_visited"
);
annotation_collection_limit_test!(
    annotation_text_candidates_refuse_collection_limit,
    1,
    false,
    "step_pmi_annotation_text_candidates"
);
annotation_collection_limit_test!(
    annotation_text_used_refuses_collection_limit,
    2,
    true,
    "step_pmi_annotation_text_used"
);

#[test]
fn characteristic_measure_values_refuse_collection_limit() {
    let source = source("#1=ITEM();");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("root fits collection policy");
    let mut losses = Vec::new();
    let mut measurements = super::super::MeasureContext {
        length_scale: 1.0,
        angle_scale: 1.0,
        graph_limit: 64,
        losses: &mut losses,
    };
    let value = crate::parse::Value::Real(
        cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite fixture"),
    );
    assert!(matches!(
        super::super::characteristic_measure_values(&super::super::MeasureParameters::Items(std::slice::from_ref(&value)), &exchange, &mut measurements, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_pmi_measure_values"
    ));
}

#[test]
fn characteristic_value_map_refuses_collection_limit() {
    const RECORDS: &str = "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));#5=PRODUCT_DEFINITION_SHAPE('PMI shape','',#99);#6=SHAPE_ASPECT('feature','',#5,.T.);#10=DIMENSIONAL_SIZE(#6,'width');#13=(LENGTH_MEASURE_WITH_UNIT() MEASURE_REPRESENTATION_ITEM() MEASURE_WITH_UNIT(POSITIVE_LENGTH_MEASURE(5.0),#1) REPRESENTATION_ITEM('nominal value'));#14=SHAPE_DIMENSION_REPRESENTATION('value',(#13),#2);#15=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#10,#14);#99=ITEM();";
    let source = source(RECORDS);
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid characteristic exchange");
    let arena = DecodeArena::new();
    let refused = (0..512).any(|limit| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
            .expect("root fits collection policy");
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        let Ok((geometry, _)) = crate::reader::geometry::decode(&exchange, &mut ir, &ctx) else {
            return false;
        };
        let mut losses = Vec::new();
        matches!(
            super::super::characteristic_values(&exchange, &geometry.value, &mut losses, 64, &ctx),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "step_pmi_characteristic_values"
        )
    });
    assert!(
        refused,
        "no collection limit refused the characteristic map entry"
    );
}
