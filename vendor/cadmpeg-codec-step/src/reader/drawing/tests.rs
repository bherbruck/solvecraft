// SPDX-License-Identifier: Apache-2.0
//! STEP drawing-graph tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::loss::StepLossCode;
use crate::test_support::exchange::decode_inline;
use crate::StepCodec;

mod collection_limits;
mod string_limits;

#[test]
fn drawing_graph_transfers_pages_revisions_views_and_opaque_items() {
    let result = decode_inline(
        "#1=DRAWING_DEFINITION('Main','detail');
#2=DRAWING_REVISION('A',#1,'rev');
#3=REPRESENTATION_CONTEXT('','');
#4=PRESENTATION_VIEW('Front',(#5),#3);
#5=ITEM('opaque');
#6=DRAWING_SHEET_REVISION('Sheet',(#4),#3,#2);
#7=DRAWING_SHEET_REVISION_USAGE(#6,#2,'1');
#8=PRESENTATION_SIZE(#6,#9);
#9=DESCRIPTIVE_REPRESENTATION_ITEM('A3','');
#10=DRAUGHTING_MODEL('Drawing model',(#4),#3);
#11=ITEM('semantic');
#12=DRAUGHTING_MODEL_ITEM_ASSOCIATION('','',#11,#10,(#4));",
    );

    assert_eq!(result.ir().model.drawings.len(), 6);
    let page = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.runtime_type == "DRAWING_SHEET_REVISION")
        .expect("drawing sheet");
    assert!(matches!(page.kind, cadmpeg_ir::drawings::DrawingKind::Page));
    assert_eq!(page.parameters["name"], "Sheet");
    assert_eq!(page.parameters["usage_7_sequence"], "1");
    assert!(page.relationships["items"]
        .iter()
        .any(|target| { target.local_target() == Some("step:drawing:presentation_view#4") }));
    assert!(page.relationships["drawing_revision"]
        .iter()
        .any(|target| { target.local_target() == Some("step:drawing:drawing_revision#2") }));

    let view = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.runtime_type == "PRESENTATION_VIEW")
        .expect("presentation view");
    assert!(view.relationships["items"]
        .iter()
        .any(|target| { target.local_target() == Some("step:data:item#5") }));
    assert!(view.relationships["presentation_context"]
        .iter()
        .any(|target| { target.local_target() == Some("step:data:representation_context#3") }));
    assert_eq!(view.parameters["presentation_context"], "#3");

    let model = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.runtime_type == "DRAUGHTING_MODEL")
        .expect("draughting model");
    assert!(model.relationships["semantic_definition"]
        .iter()
        .any(|target| { target.local_target() == Some("step:data:item#11") }));
    assert!(model.relationships["associated_items"]
        .iter()
        .any(|target| { target.local_target() == Some("step:drawing:presentation_view#4") }));

    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
    assert!(result
        .ir()
        .native_unknowns("step")
        .expect("STEP native namespace")
        .iter()
        .any(|record| record.id.as_str() == "step:data:item#5"));
    assert!(result.report().losses.iter().all(|loss| {
        loss.code != StepLossCode::DrawingSheetRevisionUnresolved.kind()
            && loss.code != StepLossCode::DrawingRevisionSheetUnresolved.kind()
            && loss.code != StepLossCode::DrawingRelationshipUntypedTarget.kind()
    }));
}

#[test]
fn complex_draughting_model_reads_inherited_representation_attributes() {
    let result = decode_inline(
        "#1=REPRESENTATION_CONTEXT('','');
#2=(CHARACTERIZED_REPRESENTATION() DRAUGHTING_MODEL() REPRESENTATION('Drawing model',(#3),#1) SHAPE_REPRESENTATION() TESSELLATED_SHAPE_REPRESENTATION());
#3=ITEM('opaque');",
    );
    let model = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.runtime_type == "DRAUGHTING_MODEL")
        .expect("complex draughting model");
    assert_eq!(model.parameters["name"], "Drawing model");
    assert_eq!(model.parameters["presentation_context"], "#1");
    assert!(model.relationships["items"]
        .iter()
        .any(|target| target.local_target() == Some("step:data:item#3")));
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::DrawingRecordTooFewParameters.kind()
            && loss.message.contains("#2")
    }));
}

#[test]
fn complex_draughting_callout_reads_inherited_name() {
    let result = decode_inline(
        "#1=(DRAUGHTING_CALLOUT((#2)) DRAUGHTING_ELEMENTS() GEOMETRIC_REPRESENTATION_ITEM() LEADER_DIRECTED_CALLOUT() REPRESENTATION_ITEM('Callout'));
#2=ITEM('opaque');",
    );
    let callout = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.runtime_type == "DRAUGHTING_CALLOUT")
        .expect("complex draughting callout");
    assert_eq!(callout.parameters["name"], "Callout");
    assert!(callout.relationships["contents"]
        .iter()
        .any(|target| target.local_target() == Some("step:data:item#2")));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == StepLossCode::DrawingRecordTooFewParameters.kind() }));
}

#[test]
fn draughting_callout_visibility_is_transferred_from_invisibility() {
    let result = decode_inline(
        "#1=DRAUGHTING_CALLOUT('Callout',());
#2=INVISIBILITY((#1));",
    );
    let callout = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.runtime_type == "DRAUGHTING_CALLOUT")
        .expect("draughting callout");
    assert_eq!(callout.visible, Some(false));
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::DecodeWarning.kind()
            && loss
                .message
                .contains("INVISIBILITY #2 targets unsupported item #1")
    }));
    assert!(!result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.as_str() == "step:data:invisibility#2"));
}

#[test]
fn drawing_associations_preserve_shape_aspects_and_placeholders() {
    let result = decode_inline(
        "#1=REPRESENTATION_CONTEXT('','');
#2=DRAUGHTING_MODEL('Model',(),#1);
#3=SHAPE_ASPECT('feature','',#4,.T.);
#4=ITEM('shape');
#5=DRAUGHTING_CALLOUT('Callout',());
#6=DIMENSIONAL_SIZE(#3,'width');
#7=ANNOTATION_PLACEHOLDER_OCCURRENCE('placeholder',(),#8,.GPS_DATA.,$);
#8=ITEM('placeholder geometry');
#9=DRAUGHTING_MODEL_ITEM_ASSOCIATION('','',#3,#2,#5);
#10=DRAUGHTING_MODEL_ITEM_ASSOCIATION_WITH_PLACEHOLDER('','',#3,#2,#5,#7);
#11=(DRAUGHTING_MODEL_ITEM_ASSOCIATION_WITH_PLACEHOLDER() ITEM_IDENTIFIED_REPRESENTATION_USAGE('','',#3,#2,#5) ANNOTATION_PLACEHOLDER_OCCURRENCE(#7));
#1000=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#1001=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(12.0),#1000);
#1002=SHAPE_DIMENSION_REPRESENTATION('nominal',(#1001),$);
#1003=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#6,#1002);",
    );
    let model = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.runtime_type == "DRAUGHTING_MODEL")
        .expect("draughting model");
    assert!(model.relationships["semantic_definition"]
        .iter()
        .filter_map(cadmpeg_ir::ReferenceSelection::local_target)
        .any(|target| target == "step:data:shape_aspect#3"));
    let drawing_targets = &result
        .ir()
        .native
        .namespace("step")
        .expect("STEP native namespace")
        .arenas()["drawing_targets"];
    assert!(drawing_targets
        .iter()
        .any(|record| record.id() == "step:data:shape_aspect#3"));
    assert_eq!(
        model.relationships["associated_items"]
            .iter()
            .filter_map(cadmpeg_ir::ReferenceSelection::local_target)
            .filter(|target| *target == "step:drawing:draughting_callout#5")
            .count(),
        3
    );
    assert_eq!(
        model.relationships["annotation_placeholder"]
            .iter()
            .filter_map(cadmpeg_ir::ReferenceSelection::local_target)
            .filter(|target| *target == "step:presentation:pmi#7")
            .count(),
        2
    );
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::DraughtingSemanticDefinitionUntyped.kind()
            || loss.code == StepLossCode::DraughtingAssociatedItemUntyped.kind()
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
    assert!(result
        .ir()
        .native_unknowns("step")
        .expect("STEP native namespace")
        .iter()
        .all(|record| {
            !record.id.as_str().ends_with("draughting_model_item_association#9")
                && !record
                    .id
                    .as_str()
                    .ends_with("draughting_model_item_association_with_placeholder#10")
                && !record
                    .id
                    .as_str()
                    .ends_with("draughting_model_item_association_with_placeholder+item_identified_representation_usage+annotation_placeholder_occurrence#11")
        }));
}

#[test]
fn drawing_association_uses_product_definition_shape_view_scope() {
    let decode_fixture = |bytes: &[u8]| {
        StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode DR-01 fixture")
    };
    let target_for = |result: &cadmpeg_ir::codec::DecodeResult, name: &str| {
        result
            .ir()
            .model
            .drawings
            .iter()
            .find(|drawing| {
                drawing
                    .parameters
                    .get("name")
                    .is_some_and(|value| value == name)
            })
            .and_then(|drawing| drawing.relationships.get("semantic_definition"))
            .and_then(|targets| targets.first())
            .and_then(cadmpeg_ir::ReferenceSelection::local_target)
            .map(str::to_owned)
    };

    let source_order = decode_fixture(include_bytes!(
        "tests/data/dr01_product_view_scope_source_order.p21"
    ));
    let reordered = decode_fixture(include_bytes!(
        "tests/data/dr01_product_view_scope_reordered.p21"
    ));

    for result in [&source_order, &reordered] {
        assert_eq!(
            target_for(result, "Design model"),
            Some("step:product:product#3-definition-6".into())
        );
        assert_eq!(
            target_for(result, "Manufacturing model"),
            Some("step:product:product#3-definition-8".into())
        );
        assert!(!result
            .report()
            .losses
            .iter()
            .any(|loss| { loss.code == StepLossCode::DrawingRelationshipTargetAmbiguous.kind() }));
    }
}

#[test]
fn drawing_relationships_resolve_unique_wrapper_carriers() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=AXIS2_PLACEMENT_3D('',#1,#2,$);
#4=PLANE('annotation support',#3);
#5=ANNOTATION_PLANE('annotation plane',(),#4,());
#6=REPRESENTATION_CONTEXT('','');
#7=DRAUGHTING_MODEL('Annotation model',(#5),#6);
#8=(CHARACTERIZED_REPRESENTATION() REPRESENTATION('mapped plane',(#4),#6) SHAPE_REPRESENTATION());
#9=REPRESENTATION_MAP(#3,#8);
#10=MAPPED_ITEM('mapped annotation',#9,#3);
#11=DRAUGHTING_MODEL('Mapped model',(#10),#6);
#12=CARTESIAN_POINT('',(0.,1.,0.));
#13=AXIS2_PLACEMENT_3D('',#12,#2,$);
#14=PLANE('second support',#13);
#15=SHAPE_REPRESENTATION('ambiguous plane',(#4,#14),#6);
#16=REPRESENTATION_MAP(#3,#15);
#17=MAPPED_ITEM('ambiguous mapped annotation',#16,#3);
#18=DRAUGHTING_MODEL('Ambiguous model',(#17),#6);
#19=SHAPE_REPRESENTATION('cyclic mapped item',(#21),#6);
#20=REPRESENTATION_MAP(#3,#19);
#21=MAPPED_ITEM('cyclic mapped annotation',#20,#3);
#22=DRAUGHTING_MODEL('Cyclic model',(#21),#6);",
    );

    let annotation_model = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.parameters.get("name") == Some(&"Annotation model".into()))
        .expect("annotation drawing model");
    assert!(annotation_model.relationships["items"]
        .iter()
        .any(|target| target.local_target() == Some("step:data:surface#4")));

    let mapped_model = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.parameters.get("name") == Some(&"Mapped model".into()))
        .expect("mapped drawing model");
    assert!(mapped_model.relationships["items"]
        .iter()
        .any(|target| target.local_target() == Some("step:data:surface#4")));

    let ambiguous_model = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.parameters.get("name") == Some(&"Ambiguous model".into()))
        .expect("ambiguous drawing model");
    assert!(!ambiguous_model.relationships.contains_key("items"));
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::DrawingRelationshipTargetAmbiguous.kind()
            && loss.message.contains("#17")
            && loss.message.contains("multiple neutral identities")
    }));
    let cyclic_model = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.parameters.get("name") == Some(&"Cyclic model".into()))
        .expect("cyclic drawing model");
    assert!(cyclic_model.relationships["items"]
        .iter()
        .any(|target| target.local_target() == Some("step:data:mapped_item#21")));
    let drawing_targets = &result
        .ir()
        .native
        .namespace("step")
        .expect("STEP native namespace")
        .arenas()["drawing_targets"];
    assert!(drawing_targets
        .iter()
        .any(|record| record.id() == "step:data:mapped_item#21"));
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::DrawingRelationshipUntypedTarget.kind()
            && loss.message.contains("#21")
    }));
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::DrawingRelationshipUntypedTarget.kind()
            && (loss.message.contains("#5") || loss.message.contains("#10"))
    }));
}

#[test]
fn drawing_relationships_retain_unresolved_wrapper_identity() {
    let result = decode_inline(
        "#1=REPRESENTATION_CONTEXT('','');
#2=ITEM('opaque');
#3=REPRESENTATION('unresolved mapped',(#2),#1);
#4=CARTESIAN_POINT('',(0.,0.,0.));
#5=DIRECTION('',(0.,0.,1.));
#6=AXIS2_PLACEMENT_3D('',#4,#5,$);
#7=REPRESENTATION_MAP(#6,#3);
#8=MAPPED_ITEM('unresolved mapped item',#7,#6);
#9=DRAUGHTING_MODEL('Unresolved mapped model',(#8),#1);",
    );

    let model = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.parameters.get("name") == Some(&"Unresolved mapped model".into()))
        .expect("unresolved mapped drawing model");
    assert!(model.relationships["items"]
        .iter()
        .any(|target| target.local_target() == Some("step:data:mapped_item#8")));
    let drawing_targets = &result
        .ir()
        .native
        .namespace("step")
        .expect("STEP native namespace")
        .arenas()["drawing_targets"];
    assert!(drawing_targets
        .iter()
        .any(|record| record.id() == "step:data:mapped_item#8"));
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::DrawingRelationshipUntypedTarget.kind()
            && loss.message.contains("#8")
    }));
}

#[test]
fn drawing_relationships_resolve_mapped_brep_carriers() {
    let source = include_str!("../../../tests/fixtures/ap242_vertex_loop.p21");
    let records = source
        .split_once("DATA;\n")
        .and_then(|(_, source)| source.split_once("ENDSEC;"))
        .map(|(records, _)| {
            format!(
                "{records}#20=REPRESENTATION_MAP(#6,#19);\n#21=MAPPED_ITEM('mapped body',#20,#6);\n#22=DRAUGHTING_MODEL('mapped body',(#21),#2);"
            )
        })
        .expect("vertex-loop fixture DATA section");
    let result = decode_inline(&records);
    let model = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.parameters.get("name") == Some(&"mapped body".into()))
        .expect("mapped body drawing model");
    assert!(!model.relationships.contains_key("items"));
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::DrawingRelationshipTargetAmbiguous.kind()
            && loss.message.contains("#21")
            && loss.message.contains("multiple neutral identities")
    }));
}

#[test]
fn deep_drawing_wrapper_graph_resolves_without_call_stack_recursion() {
    use std::fmt::Write as _;

    let mut source = String::from(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;\n\
         #1=PLANE('leaf',#4);\n\
         #2=CARTESIAN_POINT('',(0.,0.,0.));\n\
         #3=DIRECTION('',(0.,0.,1.));\n\
         #4=AXIS2_PLACEMENT_3D('',#2,#3,$);\n\
         #5=REPRESENTATION_CONTEXT('','');\n",
    );
    let mut target = 1;
    for level in 0..8192 {
        let representation = 10 + level * 3;
        let map = representation + 1;
        let item = map + 1;
        write!(
            source,
            "#{representation}=SHAPE_REPRESENTATION('',(#{target}),#5);\n\
             #{map}=REPRESENTATION_MAP(#4,#{representation});\n\
             #{item}=MAPPED_ITEM('',#{map},#4);\n"
        )
        .expect("write mapped records");
        target = item;
    }
    source.push_str("ENDSEC;END-ISO-10303-21;");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("deep mapped graph");
    let identities = std::collections::BTreeMap::from([(
        1,
        std::collections::BTreeSet::from(["step:data:surface#1".into()]),
    )]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
            .expect("root fits service policy");
    let resolved = super::target_resolution(
        target,
        &identities,
        &std::collections::HashSet::new(),
        &exchange,
        &std::collections::BTreeMap::new(),
        &ctx,
    )
    .expect("target resolution fits service policy");
    let super::TargetResolution::Resolved(resolved) = resolved else {
        panic!("deep mapped graph lost its unique surface target");
    };
    assert_eq!(resolved.local_target(), Some("step:data:surface#1"));
}
