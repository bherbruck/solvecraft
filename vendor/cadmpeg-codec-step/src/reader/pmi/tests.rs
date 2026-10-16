// SPDX-License-Identifier: Apache-2.0
//! STEP semantic and presentation PMI tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use cadmpeg_test_support::{edit, EditableDecodeResult};

const EPS_PMI_NUMERIC: f64 = 1.0e-12;

use std::fmt::Write as _;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::export::write_step;
use crate::loss::StepLossCode;
use crate::test_support::exchange::{decode_inline, decode_inline_result};
use crate::{StepCodec, StepSchema, StepWriteOptions};

mod collection_limits;
mod string_limits;

#[test]
pub(crate) fn decode_transfers_ap242_semantic_pmi() {
    use cadmpeg_ir::pmi::{DimensionTolerance, GeometricToleranceKind, PmiDefinition, PmiQuantity};

    let bytes = include_bytes!("../../../tests/fixtures/ap242_semantic_pmi.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode AP242 semantic PMI");
    let mut result = EditableDecodeResult::from(result);

    assert_eq!(result.ir().model.pmi.len(), 5);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::MetadataStringInvalid.kind()));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("PLUS_MINUS_TOLERANCE #26")));
    let dimension = result
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("width"))
        .unwrap();
    let PmiDefinition::Dimension(relation) = &dimension.definition else {
        panic!("width is not a dimension")
    };
    let nominal = relation.nominal().expect("width nominal");
    let Some(DimensionTolerance::PlusMinusFit { lower, upper, fit }) = relation.tolerance() else {
        panic!("width tolerance")
    };
    assert_eq!(nominal.value.get(), 12.0);
    assert_eq!(lower.value.get(), -0.1);
    assert_eq!(upper.value.get(), 0.2);
    assert_eq!(fit.form_variance, "H");
    assert_eq!(fit.zone_variance, "");
    assert_eq!(fit.grade, "7");
    assert_eq!(fit.source, "ISO 286");
    assert!(result.ir().model.pmi.iter().any(|annotation| matches!(
        &annotation.definition,
        PmiDefinition::Dimension(relation) if matches!(relation.kind(), cadmpeg_ir::pmi::DimensionKind::Diameter)
            && relation.nominal().is_none()
            && matches!(relation.tolerance(), Some(DimensionTolerance::PlusMinus { .. }))
    )));
    let tolerance = result
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("surface flatness"))
        .unwrap();
    let datum_system = result
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("primary system"))
        .expect("datum system");
    assert!(matches!(
        &datum_system.definition,
        PmiDefinition::DatumSystem { references }
            if references.as_slice().len() == 1
                && references.as_slice()[0].precedence.get() == 1
                && references.as_slice()[0].modifiers == ["maximum_material_requirement", "distance:0.2"]
    ));
    assert!(matches!(
        tolerance.definition,
        PmiDefinition::GeometricTolerance {
            tolerance: GeometricToleranceKind::Flatness,
            magnitude,
            datum_system: None,
            ..
        } if magnitude.get().value.get() == 0.05 && magnitude.get().quantity == PmiQuantity::Length
    ));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
    let semantic = dimension.id.clone();
    result.ir_mut().model.pmi.push(cadmpeg_ir::PmiAnnotation {
        id: cadmpeg_ir::ids::PmiId::mint("test:model:pmi#test:pmi:presentation")
            .expect("identity grammar"),
        name: Some("width note".into()),
        visible: Some(false),
        targets: Vec::new(),
        definition: PmiDefinition::Presentation {
            text: Some("12 mm".into()),
            placement: Some(cadmpeg_ir::transform::Transform::identity()),
            semantics: vec![semantic],
        },
    });
    let mut output = Vec::new();
    let report = write_step(
        result.ir(),
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write semantic PMI");
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.message.contains("PMI annotation")));
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode written semantic PMI");
    assert_eq!(roundtrip.ir().model.pmi.len(), result.ir().model.pmi.len());
    assert!(roundtrip.ir().model.pmi.iter().any(|annotation| matches!(
        &annotation.definition,
        PmiDefinition::DatumSystem { references }
            if references.as_slice().len() == 1
                && references.as_slice()[0].modifiers
                    == ["maximum_material_requirement", "distance:0.2"]
    )));
    assert!(roundtrip.ir().model.pmi.iter().any(|annotation| matches!(
        &annotation.definition,
        PmiDefinition::Presentation { semantics, .. } if semantics.len() == 1
    )));
    assert_eq!(
        roundtrip
            .ir()
            .model
            .pmi
            .iter()
            .find(|annotation| annotation.name.as_deref() == Some("width note"))
            .expect("roundtripped presentation annotation")
            .visible,
        Some(false)
    );
    assert!(roundtrip.ir().model.pmi.iter().any(|annotation| matches!(
        &annotation.definition,
        PmiDefinition::Dimension(relation)
            if relation.nominal().is_some_and(|nominal| nominal.quantity == PmiQuantity::Length && nominal.value.get() == 12.0)
                && matches!(relation.tolerance(), Some(DimensionTolerance::PlusMinusFit { lower, upper, .. })
                    if lower.value.get() == -0.1 && upper.value.get() == 0.2)
    )));
}

#[test]
fn complex_datum_feature_remains_a_dimension_target() {
    use cadmpeg_ir::pmi::{PmiDefinition, PmiTarget};

    let result = decode_inline(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#5=PRODUCT_DEFINITION_SHAPE('PMI shape','',#99);
#6=(COMPOSITE_SHAPE_ASPECT() DATUM_FEATURE() SHAPE_ASPECT('feature','',#5,.T.));
#10=DIMENSIONAL_SIZE(#6,'width');
#99=UNRESOLVED_PRODUCT();
#1000=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#1001=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(12.0),#1000);
#1002=SHAPE_DIMENSION_REPRESENTATION('nominal',(#1001),$);
#1003=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#10,#1002);",
    );
    let dimension = result
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("width"))
        .expect("complex datum feature dimension");
    assert!(matches!(&dimension.definition, PmiDefinition::Dimension(_)));
    assert_eq!(
        dimension.targets,
        vec![PmiTarget::ShapeAspect {
            source_id: cadmpeg_core::text::NonBlankString::new("#6")
                .expect("nonempty source identity")
        }]
    );
}

#[test]
fn simple_shape_aspect_subtypes_remain_dimension_targets() {
    use cadmpeg_ir::pmi::PmiTarget;

    let result = decode_inline(
        "#5=PRODUCT_DEFINITION_SHAPE('PMI shape','',#99);
#6=COMPOSITE_SHAPE_ASPECT('composite feature','',#5,.T.);
#7=DATUM_TARGET('datum target','',#5,.T.,'A');
#10=DIMENSIONAL_SIZE(#6,'composite width');
#11=DIMENSIONAL_SIZE(#7,'target width');
#99=UNRESOLVED_PRODUCT();
#1000=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#1001=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(12.0),#1000);
#1002=SHAPE_DIMENSION_REPRESENTATION('nominal',(#1001),$);
#1003=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#10,#1002);
#1004=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#11,#1002);",
    );
    for (name, source_id) in [("composite width", "#6"), ("target width", "#7")] {
        let dimension = result
            .ir()
            .model
            .pmi
            .iter()
            .find(|annotation| annotation.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing dimension {name}"));
        assert_eq!(
            dimension.targets,
            vec![PmiTarget::ShapeAspect {
                source_id: cadmpeg_core::text::NonBlankString::new(source_id)
                    .expect("nonempty source identity")
            }]
        );
    }
}

#[test]
fn datum_target_transfers_form_and_identification() {
    use cadmpeg_ir::pmi::{DatumTargetForm, PmiDefinition, PmiTarget};

    let result = decode_inline(
        "#5=PRODUCT_DEFINITION_SHAPE('PMI shape','',#99);
#6=DATUM_TARGET('point target','point',#5,.F.,'A');
#7=PLACED_DATUM_TARGET_FEATURE('circle target','circle',#5,.F.,'B');
#8=(PLACED_DATUM_TARGET_FEATURE() DATUM_TARGET('C') SHAPE_ASPECT('rectangle target','rectangle',#5,.F.));
#99=UNRESOLVED_PRODUCT();",
    );

    for (name, form, identification, source_id) in [
        ("point target", DatumTargetForm::Point, "A", "#6"),
        ("circle target", DatumTargetForm::Circle, "B", "#7"),
        ("rectangle target", DatumTargetForm::Rectangle, "C", "#8"),
    ] {
        let target = result
            .ir()
            .model
            .pmi
            .iter()
            .find(|annotation| annotation.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing datum target {name}"));
        assert_eq!(
            target.targets,
            [PmiTarget::ShapeAspect {
                source_id: cadmpeg_core::text::NonBlankString::new(source_id)
                    .expect("nonempty source identity")
            }]
        );
        assert!(matches!(
            &target.definition,
            PmiDefinition::DatumTarget {
                form: actual_form,
                identification: actual_id,
                ..
            } if actual_form == &form && actual_id == identification
        ));
    }
}

#[test]
fn complex_dimension_inherits_kind_targets_and_nominal_value() {
    use cadmpeg_ir::pmi::{DimensionKind, PmiDefinition, PmiQuantity};

    let result = decode_inline(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));
#5=PRODUCT_DEFINITION_SHAPE('PMI shape','',#99);
#6=SHAPE_ASPECT('feature','',#5,.T.);
#10=(DIMENSIONAL_LOCATION() DIMENSIONAL_LOCATION_WITH_PATH(#6) DIRECTED_DIMENSIONAL_LOCATION() SHAPE_ASPECT_RELATIONSHIP('centre distance','',#6,#6));
#13=(LENGTH_MEASURE_WITH_UNIT() MEASURE_REPRESENTATION_ITEM() MEASURE_WITH_UNIT(POSITIVE_LENGTH_MEASURE(5.0),#1) REPRESENTATION_ITEM('nominal value'));
#14=SHAPE_DIMENSION_REPRESENTATION('distance value',(#13),#2);
#15=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#10,#14);
#99=UNRESOLVED_PRODUCT();",
    );
    let dimension = result
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("centre distance"))
        .expect("complex dimensional location");
    assert!(matches!(
        &dimension.definition,
        PmiDefinition::Dimension(relation)
            if matches!(relation.kind(), DimensionKind::Location)
                && relation.nominal().is_some_and(|value| value.quantity == PmiQuantity::Length && value.value.get() == 5.0)
    ));
    assert_eq!(
        dimension.targets,
        vec![cadmpeg_ir::pmi::PmiTarget::ShapeAspect {
            source_id: cadmpeg_core::text::NonBlankString::new("#6")
                .expect("nonempty source identity")
        }]
    );
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("preserved 1 MEASURE_REPRESENTATION_ITEM instance")
    }));
}

#[test]
fn dimensional_characteristic_selects_the_named_nominal_measure() {
    use cadmpeg_ir::pmi::{DimensionKind, PmiDefinition, PmiQuantity};

    let result = decode_inline(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));
#5=PRODUCT_DEFINITION_SHAPE('PMI shape','',#99);
#6=SHAPE_ASPECT('feature','',#5,.T.);
#10=DIMENSIONAL_SIZE(#6,'width');
#11=(LENGTH_MEASURE_WITH_UNIT() MEASURE_REPRESENTATION_ITEM() MEASURE_WITH_UNIT(POSITIVE_LENGTH_MEASURE(11.8),#1) REPRESENTATION_ITEM('lower limit'));
#12=(LENGTH_MEASURE_WITH_UNIT() MEASURE_REPRESENTATION_ITEM() MEASURE_WITH_UNIT(POSITIVE_LENGTH_MEASURE(12.2),#1) REPRESENTATION_ITEM('upper limit'));
#13=(LENGTH_MEASURE_WITH_UNIT() MEASURE_REPRESENTATION_ITEM() MEASURE_WITH_UNIT(POSITIVE_LENGTH_MEASURE(12.0),#1) REPRESENTATION_ITEM('nominal value'));
#14=SHAPE_DIMENSION_REPRESENTATION('limits',(#11,#12,#13),#2);
#15=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#10,#14);
#99=UNRESOLVED_PRODUCT();",
    );
    assert!(result.ir().model.pmi.iter().any(|annotation| matches!(
        &annotation.definition,
        PmiDefinition::Dimension(relation)
            if matches!(relation.kind(), DimensionKind::Size)
                && relation.nominal().is_some_and(|value| value.quantity == PmiQuantity::Length && (value.value.get() - 12.0).abs() < EPS_PMI_NUMERIC)
    )));
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("unnamed measure values; the nominal is ambiguous")
    }));
}

#[test]
fn step_dimension_rejects_incompatible_tolerance_quantities_as_malformed() {
    let bytes = "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#2=(PLANE_ANGLE_UNIT() NAMED_UNIT(*) SI_UNIT($,.RADIAN.));
#5=PRODUCT_DEFINITION_SHAPE('PMI shape','',#99);
#6=SHAPE_ASPECT('feature','',#5,.T.);
#10=DIMENSIONAL_SIZE(#6,'width');
#11=PLANE_ANGLE_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(-0.1),#2);
#12=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.2),#1);
#13=TOLERANCE_VALUE(#11,#12);
#14=PLUS_MINUS_TOLERANCE(#13,#10);
#99=UNRESOLVED_PRODUCT();";
    let error = crate::test_support::exchange::decode_inline_result(bytes)
        .expect_err("mixed tolerance quantities");
    assert!(
        matches!(&error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(message))
        if message.contains("dimension tolerance quantities disagree")),
        "{error:?}"
    );
}

#[test]
fn dimensional_nominal_selection_ignores_set_order_and_rejects_ambiguity() {
    use cadmpeg_ir::pmi::PmiDefinition;

    let decode = |bytes: &[u8]| {
        StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode AP-02 nominal-selection witness")
    };
    let nominal = |result: &cadmpeg_ir::codec::DecodeResult| {
        result.ir().model.pmi.iter().find_map(|annotation| {
            let PmiDefinition::Dimension(relation) = &annotation.definition else {
                return None;
            };
            relation.nominal().map(|value| value.value.get())
        })
    };

    let named_first = decode(include_bytes!("tests/data/ap02_named_nominal_first.p21"));
    let named_reordered = decode(include_bytes!(
        "tests/data/ap02_named_nominal_reordered.p21"
    ));
    assert_eq!(nominal(&named_first), Some(12.0));
    assert_eq!(nominal(&named_reordered), Some(12.0));
    assert!(!named_first
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("nominal is ambiguous")));
    assert!(!named_reordered
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("nominal is ambiguous")));

    let unnamed_single = decode(include_bytes!("tests/data/ap02_unnamed_single.p21"));
    assert_eq!(nominal(&unnamed_single), Some(7.5));
    assert!(!unnamed_single
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("nominal is ambiguous")));

    let unnamed_first = decode(include_bytes!(
        "tests/data/ap02_unnamed_ambiguous_first.p21"
    ));
    let unnamed_reordered = decode(include_bytes!(
        "tests/data/ap02_unnamed_ambiguous_reordered.p21"
    ));
    assert_eq!(nominal(&unnamed_first), None);
    assert_eq!(nominal(&unnamed_reordered), None);
    for result in [&unnamed_first, &unnamed_reordered] {
        assert!(result.report().losses.iter().any(|loss| {
            loss.message
                .contains("unnamed measure values; the nominal is ambiguous")
        }));
    }
}

#[test]
fn complex_geometric_tolerance_reads_its_inherited_magnitude() {
    use cadmpeg_ir::pmi::{GeometricToleranceKind, PmiDefinition, PmiQuantity};

    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_semantic_pmi.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#12=FLATNESS_TOLERANCE('surface flatness','',#11,#6,#8);",
        "#12=(FLATNESS_TOLERANCE() GEOMETRIC_TOLERANCE('surface flatness','',#11,#6) GEOMETRIC_TOLERANCE_WITH_DEFINED_AREA_UNIT(.CIRCULAR.,$) GEOMETRIC_TOLERANCE_WITH_DEFINED_UNIT(#11));",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex geometric tolerance");
    let tolerance = result
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("surface flatness"))
        .expect("complex flatness tolerance");
    assert!(matches!(
        tolerance.definition,
        PmiDefinition::GeometricTolerance {
            tolerance: GeometricToleranceKind::Flatness,
            magnitude,
            ..
        } if magnitude.get().value.get() == 0.05 && magnitude.get().quantity == PmiQuantity::Length
    ));
    let PmiDefinition::GeometricTolerance {
        defined_unit,
        defined_area_unit,
        defined_area_second_unit,
        ..
    } = &tolerance.definition
    else {
        panic!("complex flatness tolerance has the wrong definition")
    };
    assert_eq!(
        defined_unit,
        &Some(cadmpeg_ir::PmiValue::new(0.05, PmiQuantity::Length).expect("finite value"))
    );
    assert_eq!(defined_area_unit.as_deref(), Some("circular"));
    assert!(defined_area_second_unit.is_none());
    let mut output = Vec::new();
    write_step(
        result.ir(),
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write complex geometric tolerance units");
    let output = String::from_utf8(output).expect("STEP output is UTF-8");
    assert!(output.contains("GEOMETRIC_TOLERANCE_WITH_DEFINED_UNIT"));
    assert!(output.contains("GEOMETRIC_TOLERANCE_WITH_DEFINED_AREA_UNIT"));
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("FLATNESS_TOLERANCE+GEOMETRIC_TOLERANCE")
    }));
}

#[test]
fn complex_geometric_tolerance_uses_the_leaf_not_a_tolerance_mixin() {
    use cadmpeg_ir::pmi::{GeometricToleranceKind, PmiDefinition};

    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_semantic_pmi.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#12=FLATNESS_TOLERANCE('surface flatness','',#11,#6,#8);",
        "#12=(FAKE_TOLERANCE() FLATNESS_TOLERANCE() GEOMETRIC_TOLERANCE('surface flatness','',#11,#6));",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex geometric tolerance with mixin");
    let tolerance = result
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("surface flatness"))
        .expect("complex flatness tolerance");
    assert!(matches!(
        tolerance.definition,
        PmiDefinition::GeometricTolerance {
            tolerance: GeometricToleranceKind::Flatness,
            ..
        }
    ));
}

#[test]
fn geometric_tolerance_kind_uses_exact_leaf_and_retains_abstract_base_opaque() {
    use cadmpeg_ir::pmi::{GeometricToleranceKind, PmiDefinition, PmiQuantity};

    let decode = |bytes| {
        StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode geometric tolerance witness")
    };
    let canonical = decode(include_bytes!(
        "tests/data/ap03_geometric_tolerance_canonical.p21"
    ));
    let reordered = decode(include_bytes!(
        "tests/data/ap03_geometric_tolerance_reordered.p21"
    ));

    for result in [&canonical, &reordered] {
        let tolerance = result
            .ir()
            .model
            .pmi
            .iter()
            .find(|annotation| annotation.name.as_deref() == Some("surface flatness"))
            .expect("complex flatness tolerance");
        let PmiDefinition::GeometricTolerance {
            tolerance: kind,
            magnitude,
            modifiers,
            ..
        } = &tolerance.definition
        else {
            panic!("complex flatness tolerance has the wrong definition")
        };
        assert_eq!(kind, &GeometricToleranceKind::Flatness);
        assert_eq!(magnitude.get().quantity, PmiQuantity::Length);
        assert_eq!(magnitude.get().value.get(), 0.05);
        assert_eq!(modifiers, &["free_state"]);
        assert_eq!(
            result
                .ir()
                .model
                .pmi
                .iter()
                .filter(|annotation| {
                    matches!(
                        annotation.definition,
                        PmiDefinition::GeometricTolerance { .. }
                    )
                })
                .count(),
            1
        );
        assert!(result
            .ir()
            .native_unknowns("step")
            .expect("STEP unknown records")
            .iter()
            .any(|record| record.id.as_str() == "step:data:geometric_tolerance#13"));
    }
    assert!(!canonical
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::ParseNoncanonicalSyntax.kind()));
    assert!(reordered
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::ParseNoncanonicalSyntax.kind()));
}

#[test]
fn supported_geometric_tolerance_kinds_emit_matching_leaf_entities() {
    use cadmpeg_ir::ids::PmiId;
    use cadmpeg_ir::pmi::{GeometricToleranceKind, PmiDefinition};

    let base = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!(
                "../../../tests/fixtures/ap242_semantic_pmi.p21"
            )),
            &DecodeOptions::default(),
        )
        .expect("decode geometric tolerance template")
        .into_parts()
        .0;
    let template = base
        .model
        .pmi
        .iter()
        .find(|annotation| {
            matches!(
                annotation.definition,
                PmiDefinition::GeometricTolerance { .. }
            )
        })
        .cloned()
        .expect("geometric tolerance template");
    let kinds = [
        (
            GeometricToleranceKind::Straightness,
            "STRAIGHTNESS_TOLERANCE",
        ),
        (GeometricToleranceKind::Flatness, "FLATNESS_TOLERANCE"),
        (GeometricToleranceKind::Roundness, "ROUNDNESS_TOLERANCE"),
        (
            GeometricToleranceKind::Cylindricity,
            "CYLINDRICITY_TOLERANCE",
        ),
        (GeometricToleranceKind::Coaxiality, "COAXIALITY_TOLERANCE"),
        (
            GeometricToleranceKind::LineProfile,
            "LINE_PROFILE_TOLERANCE",
        ),
        (
            GeometricToleranceKind::SurfaceProfile,
            "SURFACE_PROFILE_TOLERANCE",
        ),
        (GeometricToleranceKind::Angularity, "ANGULARITY_TOLERANCE"),
        (
            GeometricToleranceKind::Perpendicularity,
            "PERPENDICULARITY_TOLERANCE",
        ),
        (GeometricToleranceKind::Parallelism, "PARALLELISM_TOLERANCE"),
        (GeometricToleranceKind::Position, "POSITION_TOLERANCE"),
        (
            GeometricToleranceKind::Concentricity,
            "CONCENTRICITY_TOLERANCE",
        ),
        (GeometricToleranceKind::Symmetry, "SYMMETRY_TOLERANCE"),
        (
            GeometricToleranceKind::CircularRunout,
            "CIRCULAR_RUNOUT_TOLERANCE",
        ),
        (
            GeometricToleranceKind::TotalRunout,
            "TOTAL_RUNOUT_TOLERANCE",
        ),
    ];

    for (ordinal, (kind, entity)) in kinds.into_iter().enumerate() {
        let mut ir = base.clone();
        ir.model.pmi.clear();
        let mut annotation = template.clone();
        annotation.id =
            PmiId::mint(format!("test:pmi:tolerance#{ordinal}")).expect("identity grammar");
        let PmiDefinition::GeometricTolerance {
            tolerance,
            datum_system,
            defined_unit,
            defined_area_unit,
            defined_area_second_unit,
            modifiers,
            ..
        } = &mut annotation.definition
        else {
            panic!("geometric tolerance template has the wrong definition")
        };
        *tolerance = kind;
        *datum_system = None;
        *defined_unit = None;
        *defined_area_unit = None;
        *defined_area_second_unit = None;
        modifiers.clear();
        ir.model.pmi.push(annotation);

        let mut output = Vec::new();
        write_step(
            &ir,
            &mut output,
            StepSchema::Ap242Edition3,
            &StepWriteOptions::default(),
        )
        .expect("write geometric tolerance leaf");
        let output = String::from_utf8(output).expect("STEP output is UTF-8");
        assert!(output.contains(entity), "kind {ordinal} emitted:\n{output}");
    }
}

#[test]
fn annotation_text_requires_one_reachable_carrier() {
    use cadmpeg_ir::pmi::PmiDefinition;

    let decode = |bytes: &[u8]| {
        StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode annotation text witness")
    };
    let single = decode(include_bytes!("tests/data/ap04_single_text.p21"));
    let PmiDefinition::Presentation { ref text, .. } = single.ir().model.pmi[0].definition else {
        panic!("single text annotation has the wrong definition")
    };
    assert_eq!(text.as_deref(), Some("single text"));
    assert!(!single
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::PresentationAnnotationTextUnordered.kind()));
    assert!(!single
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown records")
        .iter()
        .any(|record| record.id.as_str().ends_with("#1")));

    let first = decode(include_bytes!("tests/data/ap04_composite_text_first.p21"));
    let reordered = decode(include_bytes!(
        "tests/data/ap04_composite_text_reordered.p21"
    ));
    for result in [&first, &reordered] {
        let PmiDefinition::Presentation { ref text, .. } = result.ir().model.pmi[0].definition
        else {
            panic!("composite text annotation has the wrong definition")
        };
        assert!(text.is_none());
        assert!(result.report().losses.iter().any(|loss| {
            loss.code == StepLossCode::PresentationAnnotationTextUnordered.kind()
                && loss.message.contains("2 reachable text carriers")
        }));
        let unknowns = result
            .ir()
            .native_unknowns("step")
            .expect("STEP unknown records");
        for id in [1, 2, 3] {
            assert!(
                unknowns
                    .iter()
                    .any(|record| record.id.as_str().ends_with(&format!("#{id}"))),
                "ambiguous text carrier #{id} was not retained"
            );
        }
    }
}

#[test]
fn composite_presentation_placement_does_not_depend_on_set_order() {
    use cadmpeg_ir::pmi::PmiDefinition;

    let decode = |bytes: &[u8]| {
        StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode composite placement witness")
    };
    let first = decode(include_bytes!(
        "tests/data/ap12_composite_placement_first.p21"
    ));
    let reordered = decode(include_bytes!(
        "tests/data/ap12_composite_placement_reordered.p21"
    ));
    for result in [&first, &reordered] {
        let PmiDefinition::Presentation {
            ref text,
            ref placement,
            ..
        } = result.ir().model.pmi[0].definition
        else {
            panic!("composite annotation has the wrong definition")
        };
        assert!(text.is_none());
        assert!(placement.is_none());
        assert!(result.report().losses.iter().any(|loss| {
            loss.code == StepLossCode::PresentationAnnotationPlacementAmbiguous.kind()
                && loss.message.contains("2 reachable placement carriers")
        }));
        let unknowns = result
            .ir()
            .native_unknowns("step")
            .expect("STEP unknown records");
        for id in [11, 12, 13] {
            assert!(
                unknowns
                    .iter()
                    .any(|record| record.id.as_str().ends_with(&format!("#{id}"))),
                "ambiguous presentation carrier #{id} was not retained"
            );
        }
    }
}

#[test]
fn associated_curve_placement_does_not_create_presentation_ambiguity() {
    use cadmpeg_ir::pmi::PmiDefinition;

    const EPS_PLACEMENT_COORDINATE: f64 = 1.0e-12;

    let result = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!(
                "tests/data/ap12_associated_curve_placement.p21"
            )),
            &DecodeOptions::default(),
        )
        .expect("decode associated-curve placement witness");
    let PmiDefinition::Presentation {
        ref text,
        ref placement,
        ..
    } = result.ir().model.pmi[0].definition
    else {
        panic!("associated-curve annotation has the wrong definition")
    };
    assert_eq!(text.as_deref(), Some("note"));
    let transform = placement.as_ref().expect("text placement");
    assert!((transform.rows()[0][3] - 10.0).abs() < EPS_PLACEMENT_COORDINATE);
    assert!((transform.rows()[1][3] - 0.0).abs() < EPS_PLACEMENT_COORDINATE);
    assert!((transform.rows()[2][3] - 0.0).abs() < EPS_PLACEMENT_COORDINATE);
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::PresentationAnnotationPlacementAmbiguous.kind()
    }));
    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id.as_str() == "step:data:curve#9"));
}

#[test]
fn coaxiality_tolerance_decodes_and_writes_as_a_native_leaf() {
    use cadmpeg_ir::pmi::{GeometricToleranceKind, PmiDefinition};

    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_semantic_pmi.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#12=FLATNESS_TOLERANCE('surface flatness','',#11,#6,#8);",
        "#12=COAXIALITY_TOLERANCE('coaxiality','',#11,#6,#8);",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode coaxiality tolerance");
    let tolerance = result
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("coaxiality"))
        .expect("coaxiality tolerance");
    assert!(matches!(
        tolerance.definition,
        PmiDefinition::GeometricTolerance {
            tolerance: GeometricToleranceKind::Coaxiality,
            ..
        }
    ));
    let mut output = Vec::new();
    write_step(
        result.ir(),
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write coaxiality tolerance");
    assert!(String::from_utf8(output)
        .expect("STEP output is UTF-8")
        .contains("COAXIALITY_TOLERANCE"));
}

#[test]
fn complex_geometric_tolerance_links_its_inherited_datum_system() {
    use cadmpeg_ir::pmi::{GeometricToleranceKind, PmiDefinition};

    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_semantic_pmi.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#12=FLATNESS_TOLERANCE('surface flatness','',#11,#6,#8);",
        "#12=(GEOMETRIC_TOLERANCE('surface flatness','',#11,#6) GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE((#8)) GEOMETRIC_TOLERANCE_WITH_MODIFIERS((.MAXIMUM_MATERIAL_REQUIREMENT.,.FREE_STATE.)) FLATNESS_TOLERANCE());",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex geometric tolerance datum system");
    let tolerance = result
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("surface flatness"))
        .expect("complex flatness tolerance");
    assert!(matches!(
        &tolerance.definition,
        PmiDefinition::GeometricTolerance {
            tolerance: GeometricToleranceKind::Flatness,
            datum_system: Some(system),
            ..
        } if system.as_str() == "step:presentation:pmi#8"
    ));
    let PmiDefinition::GeometricTolerance { modifiers, .. } = &tolerance.definition else {
        panic!("complex flatness tolerance has the wrong definition")
    };
    assert_eq!(
        modifiers,
        &[
            "maximum_material_requirement".to_string(),
            "free_state".to_string()
        ]
    );
    assert!(result
        .ir()
        .model
        .pmi
        .iter()
        .any(|annotation| matches!(annotation.definition, PmiDefinition::DatumSystem { .. })));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);

    let mut output = Vec::new();
    let report = crate::export::write_step(
        result.ir(),
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write complex geometric tolerance with report policy");
    assert!(!report.losses.iter().any(|loss| loss.code
        == StepLossCode::PmiAnnotationNotWritten.kind()
        || loss.code == StepLossCode::SemanticAnnotationOmitted.kind()));
    let output = String::from_utf8(output).expect("STEP output is UTF-8");
    assert!(output.contains("GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE"));
    assert!(output.contains("GEOMETRIC_TOLERANCE_WITH_MODIFIERS"));
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode written complex geometric tolerance");
    let tolerance = roundtrip
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("surface flatness"))
        .expect("roundtripped flatness tolerance");
    assert!(matches!(
        &tolerance.definition,
        PmiDefinition::GeometricTolerance {
            datum_system: Some(_),
            modifiers,
            ..
        } if modifiers == &["maximum_material_requirement", "free_state"]
    ));
}

#[test]
pub(crate) fn decode_transfers_ap242_presentation_pmi() {
    use cadmpeg_ir::pmi::PmiDefinition;

    let bytes = include_bytes!("../../../tests/fixtures/ap242_presentation_pmi.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode AP242 presentation PMI");

    assert_eq!(result.ir().model.pmi.len(), 1);
    let PmiDefinition::Presentation {
        ref text,
        ref placement,
        ..
    } = result.ir().model.pmi[0].definition
    else {
        panic!("annotation occurrence is not presentation PMI")
    };
    assert_eq!(text.as_deref(), Some("inspect surface"));
    let transform = placement.as_ref().unwrap();
    assert_eq!(transform.rows()[0][3], 10.0);
    assert_eq!(transform.rows()[1][3], 20.0);
    assert_eq!(transform.rows()[2][3], 30.0);
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);

    let mut output = Vec::new();
    let report = write_step(
        result.ir(),
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write presentation PMI");
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.message.contains("PMI annotation")));
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode written presentation PMI");
    assert_eq!(roundtrip.ir().model.pmi.len(), 1);
    assert!(matches!(
        &roundtrip.ir().model.pmi[0].definition,
        PmiDefinition::Presentation {
            text: Some(text),
            placement: Some(transform),
            ..
        } if text == "inspect surface"
            && transform.rows()[0][3] == 10.0
            && transform.rows()[1][3] == 20.0
            && transform.rows()[2][3] == 30.0
    ));
}

#[test]
fn annotation_occurrence_with_leader_line_visibility_is_transferred() {
    let result = decode_inline(
        "#1=ANNOTATION_PLACEHOLDER_OCCURRENCE_WITH_LEADER_LINE('hidden placeholder',(),$,$);\n\
#2=INVISIBILITY((#1));",
    );
    let annotation = result
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("hidden placeholder"))
        .expect("placeholder occurrence is presentation PMI");
    assert_eq!(annotation.visible, Some(false));
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::DecodeWarning.kind()
            && loss
                .message
                .contains("INVISIBILITY #2 targets unsupported item #1")
    }));
    assert!(!result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown records")
        .iter()
        .any(|record| record.id.as_str() == "step:data:invisibility#2"));
}

#[test]
fn malformed_zero_partial_pmi_reference_is_non_panicking() {
    let source = b"ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('test'),'2;1');
FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');
FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));
ENDSEC;
DATA;
#5=();
#10=ANNOTATION_OCCURRENCE('',(),#5);
ENDSEC;
END-ISO-10303-21;
";
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("omit the bounded empty instance and its PMI dependent");
    let result = EditableDecodeResult::from(result);
    assert!(result.ir().model.pmi.is_empty());
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::ParseRecordOmitted.kind()
            && loss.message.contains("expected name")
    }));
    let retained = result
        .ir()
        .native_unknowns("step")
        .expect("retained source");
    for bytes in [
        b"#5=();".as_slice(),
        b"#10=ANNOTATION_OCCURRENCE('',(),#5);",
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
pub(crate) fn unresolved_lower_tolerance_does_not_shift_upper_deviation() {
    use cadmpeg_ir::pmi::PmiDefinition;

    let result = decode_inline(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#5=PRODUCT_DEFINITION_SHAPE('','',#99);
#6=SHAPE_ASPECT('feature','',#5,.T.);
#10=DIMENSIONAL_SIZE(#6,'width');
#16=UNRESOLVED_MEASURE();
#17=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.2),#1);
#18=TOLERANCE_VALUE(#16,#17);
#19=PLUS_MINUS_TOLERANCE(#18,#10);
#99=UNRESOLVED_PRODUCT();",
    );
    assert!(result.ir().model.pmi.iter().any(|annotation| matches!(
        &annotation.definition,
        PmiDefinition::Dimension(relation) if relation.nominal().is_none() && relation.tolerance().is_none()
    )));
}

#[test]
pub(crate) fn typed_pmi_measure_uses_its_explicit_conversion_unit() {
    use cadmpeg_ir::pmi::PmiDefinition;

    let result = decode_inline(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));
#5=PRODUCT_DEFINITION_SHAPE('PMI shape','',#99);
#6=DATUM_FEATURE('feature','',#5,.T.);
#10=DIMENSIONAL_SIZE(#6,'width');
#30=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#1);
#31=(CONVERSION_BASED_UNIT('inch',#30) LENGTH_UNIT() NAMED_UNIT(*));
#13=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(5.0),#31);
#14=SHAPE_DIMENSION_REPRESENTATION('width value',(#13),#2);
#15=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#10,#14);
#99=UNRESOLVED_PRODUCT();",
    );
    assert!(result.ir().model.pmi.iter().any(|annotation| matches!(
        &annotation.definition,
        PmiDefinition::Dimension(relation)
            if relation.nominal().is_some_and(|value| (value.value.get() - 127.0).abs() < EPS_PMI_NUMERIC)
    )));
}

#[test]
fn failed_pmi_measure_branches_do_not_poison_sibling_carriers() {
    use cadmpeg_ir::pmi::PmiDefinition;

    let mut records = String::from(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));
#3=PRODUCT_DEFINITION_SHAPE('PMI shape','',#300);
#4=SHAPE_ASPECT('feature','',#3,.T.);
#5=DIMENSIONAL_SIZE(#4,'width');
#6=TOLERANCE_VALUE(#20,#100);
#7=SHAPE_DIMENSION_REPRESENTATION('width value',(#6),#2);
#8=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#5,#7);
#300=UNRESOLVED_PRODUCT();
",
    );
    for id in 20..280 {
        writeln!(records, "#{id}=UNRESOLVED_MEASURE(#{next});", next = id + 1)
            .expect("write recursive measure carrier");
    }
    records.push_str("#280=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.4),#1);\n");

    let result = decode_inline(&records);
    assert!(result.ir().model.pmi.iter().any(|annotation| matches!(
        &annotation.definition,
        PmiDefinition::Dimension(relation)
            if relation.nominal().is_some_and(|value| (value.value.get() - 0.4).abs() < EPS_PMI_NUMERIC)
    )));
}

#[test]
pub(crate) fn ap242_dimension_kinds_emit_concrete_schema_entities() {
    use cadmpeg_ir::ids::PmiId;
    use cadmpeg_ir::pmi::{DimensionKind, GeometricToleranceKind, PmiDefinition};

    let mut ir = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!(
                "../../../tests/fixtures/ap242_semantic_pmi.p21"
            )),
            &DecodeOptions::default(),
        )
        .expect("decode semantic PMI")
        .into_parts()
        .0;
    let template = ir
        .model
        .pmi
        .iter()
        .find(|annotation| matches!(annotation.definition, PmiDefinition::Dimension(_)))
        .cloned()
        .expect("dimension template");
    ir.model.pmi.clear();
    for (ordinal, kind) in [
        DimensionKind::Diameter,
        DimensionKind::Radius,
        DimensionKind::Location,
    ]
    .into_iter()
    .enumerate()
    {
        let mut annotation = template.clone();
        annotation.id =
            PmiId::mint(format!("test:pmi:dimension#{ordinal}")).expect("identity grammar");
        annotation.name = Some(format!("dimension {ordinal}"));
        let PmiDefinition::Dimension(relation) = &mut annotation.definition else {
            unreachable!()
        };
        relation.set_kind(kind).expect("compatible dimension kind");
        ir.model.pmi.push(annotation);
    }
    let mut unsupported = template;
    unsupported.id = PmiId::mint("test:pmi:tolerance#other").expect("identity grammar");
    unsupported.definition = PmiDefinition::GeometricTolerance {
        tolerance: GeometricToleranceKind::Other("vendor_tolerance".into()),
        magnitude: cadmpeg_ir::pmi::PmiMagnitude::new(
            cadmpeg_ir::PmiValue::new(0.1, cadmpeg_ir::PmiQuantity::Length).expect("finite value"),
        )
        .expect("nonnegative magnitude"),
        datum_system: None,
        defined_unit: None,
        defined_area_unit: None,
        defined_area_second_unit: None,
        modifiers: Vec::new(),
    };
    ir.model.pmi.push(unsupported);

    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write dimensions");
    let text = String::from_utf8(output.clone()).unwrap();
    assert!(!text.contains("DIAMETER_SIZE"));
    assert!(!text.contains("RADIUS_SIZE"));
    assert!(!text.contains(" = GEOMETRIC_TOLERANCE("));
    assert!(text.contains(",'diameter')"));
    assert!(text.contains(",'radius')"));
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(&output, crate::parse::parse_inner).unwrap();
    assert!(diagnostics.is_empty());
    let location = exchange
        .records()
        .values()
        .find(|record| record.partials.first().name == "DIMENSIONAL_LOCATION")
        .expect("dimensional location");
    assert_eq!(location.partials[0].parameters.len(), 4);
    assert!(matches!(
        location.partials[0].parameters[0],
        crate::parse::Value::String(_)
    ));
    assert!(matches!(
        location.partials[0].parameters[1],
        crate::parse::Value::Omitted
    ));
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains("PMI annotation")));
}

#[test]
pub(crate) fn common_datum_compartment_round_trips_as_one_precedence() {
    use cadmpeg_ir::ids::PmiId;
    use cadmpeg_ir::pmi::{DatumReference, PmiDefinition};

    let mut ir = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!(
                "../../../tests/fixtures/ap242_semantic_pmi.p21"
            )),
            &DecodeOptions::default(),
        )
        .expect("decode semantic PMI")
        .into_parts()
        .0;
    let datum_a = ir
        .model
        .pmi
        .iter()
        .find(|annotation| matches!(annotation.definition, PmiDefinition::Datum { .. }))
        .cloned()
        .expect("datum A");
    let mut datum_b = datum_a.clone();
    datum_b.id = PmiId::mint("test:model:pmi#datum-b").expect("identity grammar");
    datum_b.definition = PmiDefinition::Datum {
        identification: "B".into(),
    };
    ir.model.pmi.push(datum_b.clone());
    let system = ir
        .model
        .pmi
        .iter_mut()
        .find(|annotation| matches!(annotation.definition, PmiDefinition::DatumSystem { .. }))
        .expect("datum system");
    let PmiDefinition::DatumSystem { references } = &mut system.definition else {
        unreachable!()
    };
    let modifiers = references.as_slice()[0].modifiers.clone();
    {
        let replacement = vec![
            DatumReference {
                datum: datum_a.id,
                precedence: std::num::NonZeroU32::MIN,
                common_group: Some(7),
                modifiers: modifiers.clone(),
            },
            DatumReference {
                datum: datum_b.id,
                precedence: std::num::NonZeroU32::MIN,
                common_group: Some(7),
                modifiers: vec!["least_material_requirement".into()],
            },
        ];
        edit::replace(references, |_| {
            cadmpeg_ir::pmi::DatumReferences::try_from(replacement)
        })
    }
    .expect("valid common datum compartment");
    let validation =
        cadmpeg_ir::validate_neutral(&ir, Vec::new()).expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);

    let mut output = Vec::new();
    write_step(
        &ir,
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write common datum");
    assert!(String::from_utf8_lossy(&output).contains("COMMON_DATUM_LIST(("));
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode common datum");
    assert!(roundtrip.ir().model.pmi.iter().any(|annotation| matches!(
        &annotation.definition,
        PmiDefinition::DatumSystem { references }
            if references.as_slice().len() == 2
            && references
                .as_slice()
                .iter()
                .all(|reference| reference.precedence.get() == 1)
                && references.as_slice().iter().all(|reference| reference.common_group == Some(1))
                && references.as_slice()[0].modifiers != references.as_slice()[1].modifiers
    )));
}

#[test]
fn invalid_datum_system_records_a_loss_and_preserves_good_annotations() {
    use cadmpeg_ir::pmi::PmiDefinition;

    let result = decode_inline(
        "#4=DATUM_SYSTEM('first good system','',#5,.F.,(#20));
#5=PRODUCT_DEFINITION_SHAPE('PMI shape','',#99);
#6=(DATUM('A') SHAPE_ASPECT('','',#5,.F.));
#7=(DATUM('B') SHAPE_ASPECT('','',#5,.F.));
#8=DATUM_SYSTEM('bad system','',#5,.F.,(#24));
#9=DATUM_SYSTEM('last good system','',#5,.F.,(#23));
#20=DATUM_REFERENCE_COMPARTMENT('',$,#5,.F.,#6,());
#21=DATUM_REFERENCE_ELEMENT('',$,#5,.F.,#6,());
#22=DATUM_REFERENCE_ELEMENT('',$,#5,.F.,#5,());
#23=DATUM_REFERENCE_COMPARTMENT('',$,#5,.F.,#7,());
#24=DATUM_REFERENCE_COMPARTMENT('',$,#5,.F.,COMMON_DATUM_LIST((#21,#22)),());
#99=UNRESOLVED_PRODUCT();",
    );
    let annotations = &result.ir().model.pmi;
    assert_eq!(annotations.len(), 4);
    for (system, datum) in [(4, 6), (9, 7)] {
        let annotation = annotations
            .iter()
            .find(|annotation| annotation.id.as_str() == format!("step:presentation:pmi#{system}"))
            .expect("valid datum system survives");
        assert!(matches!(
            &annotation.definition,
            PmiDefinition::DatumSystem { references }
                if references.as_slice().len() == 1
                    && references.as_slice()[0].datum.as_str() == format!("step:presentation:pmi#{datum}")
                    && references.as_slice()[0].common_group.is_none()
        ));
    }
    for identification in ["A", "B"] {
        assert!(annotations.iter().any(|annotation| matches!(
            &annotation.definition,
            PmiDefinition::Datum { identification: actual } if actual == identification
        )));
    }
    assert!(!annotations
        .iter()
        .any(|annotation| annotation.id.as_str() == "step:presentation:pmi#8"));
    let losses = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == StepLossCode::PmiDatumSystemInvalid.kind())
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("DATUM_SYSTEM #8"));
    assert!(losses[0].message.contains("common_group"));
}

#[test]
fn complex_datum_names_use_the_inherited_shape_aspect_name() {
    use cadmpeg_ir::pmi::PmiDefinition;

    let result = decode_inline(
        "#5=PRODUCT_DEFINITION_SHAPE('PMI shape','',#99);
#7=(DATUM('A') SHAPE_ASPECT('','',#5,.F.));
#8=(DATUM_SYSTEM((#20)) SHAPE_ASPECT('system name','',#5,.F.));
#20=DATUM_REFERENCE_COMPARTMENT('',$,#5,.F.,#7,());
#99=UNRESOLVED_PRODUCT();",
    );
    let names = result
        .ir()
        .model
        .pmi
        .iter()
        .filter(|annotation| {
            matches!(
                annotation.definition,
                PmiDefinition::Datum { .. } | PmiDefinition::DatumSystem { .. }
            )
        })
        .map(|annotation| annotation.name.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(names, [None, Some("system name")]);
}

#[test]
fn complex_datum_reads_identification_from_its_named_partial() {
    use cadmpeg_ir::pmi::{PmiDefinition, PmiTarget};

    let canonical = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!(
                "tests/data/ap01_complex_datum_canonical.p21"
            )),
            &DecodeOptions::default(),
        )
        .expect("decode canonical complex datum");
    let reordered = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!(
                "tests/data/ap01_complex_datum_reordered.p21"
            )),
            &DecodeOptions::default(),
        )
        .expect("decode reordered complex datum");

    let datum = canonical
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.id.as_str() == "step:presentation:pmi#7")
        .expect("complex datum");
    assert_eq!(datum.name, None);
    assert!(matches!(
        &datum.definition,
        PmiDefinition::Datum { identification } if identification == "A"
    ));
    assert_eq!(
        datum.targets,
        vec![PmiTarget::ShapeAspect {
            source_id: cadmpeg_core::text::NonBlankString::new("#7")
                .expect("nonempty source identity")
        }]
    );

    let reordered_datum = reordered
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.id.as_str() == "step:presentation:pmi#7")
        .expect("reordered complex datum");
    assert_eq!(reordered_datum.name, datum.name);
    assert_eq!(reordered_datum.targets, datum.targets);
    assert_eq!(reordered_datum.definition, datum.definition);
    assert!(reordered.report().losses.iter().any(|loss| loss.code
        == StepLossCode::ParseNoncanonicalSyntax.kind()
        && loss
            .message
            .contains("complex partial records are not alphabetical")));
}

#[test]
fn geometric_item_usage_adds_typed_topology_targets_to_pmi() {
    use cadmpeg_ir::pmi::{DatumTargetForm, DimensionKind, PmiDefinition, PmiTarget};

    const EPS_POINT_COORDINATE: f64 = 1.0e-12;

    let source =
        String::from_utf8(include_bytes!("../../../tests/fixtures/ap203_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "ENDSEC;\nEND-ISO-10303-21;",
                "#38=PRODUCT_DEFINITION_SHAPE('PMI shape','',$);\n#39=SHAPE_ASPECT('dimension feature','',#38,.T.);\n#40=SHAPE_ASPECT('geometric feature','',#38,.T.);\n#41=DIMENSIONAL_SIZE(#39,'diameter');\n#42=SHAPE_ASPECT_RELATIONSHIP('','',#39,#40);\n#43=GEOMETRIC_ITEM_SPECIFIC_USAGE('','',#40,#32,#29);\n#44=GEOMETRIC_ITEM_SPECIFIC_USAGE('','',#39,#32,#6);\n#45=DATUM_TARGET('datum target','circle',#38,.F.,'A');\n#46=GEOMETRIC_ITEM_SPECIFIC_USAGE('','',#45,#32,#29);\n#47=SHAPE_ASPECT('datum basis','DATUM TARGET',#38,.T.);\n#48=FEATURE_FOR_DATUM_TARGET_RELATIONSHIP('','',#47,#45);\n#49=GEOMETRIC_ITEM_SPECIFIC_USAGE('','',#47,#32,#29);\n#50=CARTESIAN_POINT('isolated PMI point',(1.,2.,3.));\n#51=SHAPE_ASPECT('point feature','',#38,.T.);\n#52=DIMENSIONAL_SIZE(#51,'point dimension');\n#53=GEOMETRIC_ITEM_SPECIFIC_USAGE('','',#51,#32,#50);\n#54=SHAPE_ASPECT('curve feature','',#38,.T.);\n#55=DIMENSIONAL_SIZE(#54,'curve dimension');\n#56=GEOMETRIC_ITEM_SPECIFIC_USAGE('','',#54,#32,#16);\n#1000=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n#1001=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(12.0),#1000);\n#1002=SHAPE_DIMENSION_REPRESENTATION('nominal',(#1001),$);\n#1003=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#41,#1002);\n#1004=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#52,#1002);\n#1005=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#55,#1002);\nENDSEC;\nEND-ISO-10303-21;",
            );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode geometric item usage");
    let dimension = result
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.id.as_str() == "step:presentation:pmi#41")
        .expect("dimension annotation");
    assert!(matches!(
        dimension.definition,
        PmiDefinition::Dimension(ref relation) if matches!(relation.kind(), DimensionKind::Diameter)
    ));
    assert!(dimension.targets.contains(&PmiTarget::ShapeAspect {
        source_id: cadmpeg_core::text::NonBlankString::new("#39")
            .expect("nonempty source identity")
    }));
    assert!(dimension.targets.contains(&PmiTarget::Face {
        face: cadmpeg_ir::ids::FaceId::mint("step:data:face#29").expect("identity grammar")
    }));
    assert!(dimension.targets.contains(&PmiTarget::Vertex {
        vertex: cadmpeg_ir::ids::VertexId::mint("step:data:vertex#6").expect("identity grammar")
    }));
    assert!(!result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| {
            matches!(
                record.id.as_str(),
                "step:data:geometric_item_specific_usage#43"
                    | "step:data:geometric_item_specific_usage#44"
                    | "step:data:geometric_item_specific_usage#46"
                    | "step:data:geometric_item_specific_usage#49"
                    | "step:data:geometric_item_specific_usage#53"
                    | "step:data:geometric_item_specific_usage#56"
            )
        }));
    let datum_target = result
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("datum target"))
        .expect("datum target annotation");
    assert!(matches!(
        &datum_target.definition,
        PmiDefinition::DatumTarget {
            form: DatumTargetForm::Circle,
            identification,
            ..
        } if identification == "A"
    ));
    assert!(datum_target.targets.iter().any(|target| matches!(
        target,
        PmiTarget::Face { face } if face.as_str() == "step:data:face#29"
    )));
    assert!(matches!(
        &datum_target.definition,
        PmiDefinition::DatumTarget { basis, .. }
            if basis.contains(&PmiTarget::ShapeAspect {
                source_id: cadmpeg_core::text::NonBlankString::new("#47").expect("nonempty source identity")})
    ));
    let point_dimension = result
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("point dimension"))
        .expect("point dimension annotation");
    assert!(point_dimension.targets.contains(&PmiTarget::Point {
        point: "step:data:point#50".try_into().expect("valid identity")
    }));
    let point = result
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id.as_str() == "step:data:point#50")
        .expect("isolated PMI point");
    assert!((point.position().get().x - 1.0).abs() < EPS_POINT_COORDINATE);
    assert!((point.position().get().y - 2.0).abs() < EPS_POINT_COORDINATE);
    assert!((point.position().get().z - 3.0).abs() < EPS_POINT_COORDINATE);
    let curve_dimension = result
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("curve dimension"))
        .expect("curve dimension annotation");
    assert!(curve_dimension.targets.contains(&PmiTarget::Curve {
        curve: "step:data:curve#16".try_into().expect("valid identity")
    }));

    let mut output = Vec::new();
    let report = write_step(
        result.ir(),
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write geometric item usage");
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::PmiAnnotationNotWritten.kind()));
    let output_text = String::from_utf8_lossy(&output);
    assert!(output_text.contains("GEOMETRIC_ITEM_SPECIFIC_USAGE"));
    assert!(output_text.contains("DATUM_TARGET("));
    assert!(output_text.contains("FEATURE_FOR_DATUM_TARGET_RELATIONSHIP"));
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode written geometric item usage");
    let roundtripped_dimension = roundtrip
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("diameter"))
        .expect("roundtripped dimension");
    assert!(roundtripped_dimension.targets.iter().any(|target| matches!(
        target,
        PmiTarget::Face { face } if face.as_str().starts_with("step:data:face#")
    )));
    let roundtripped_datum_target = roundtrip
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("datum target"))
        .expect("roundtripped datum target");
    assert!(roundtripped_datum_target
        .targets
        .iter()
        .any(|target| matches!(
            target,
            PmiTarget::Face { face } if face.as_str().starts_with("step:data:face#")
        )));
    assert!(matches!(
        &roundtripped_datum_target.definition,
        PmiDefinition::DatumTarget { basis, .. }
            if basis
                .iter()
                .any(|target| matches!(target, PmiTarget::ShapeAspect { .. }))
    ));
    let roundtripped_point_dimension = roundtrip
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("point dimension"))
        .expect("roundtripped point dimension");
    assert!(roundtripped_point_dimension
        .targets
        .iter()
        .any(|target| matches!(target, PmiTarget::Point { .. })));
    let roundtripped_curve_dimension = roundtrip
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("curve dimension"))
        .expect("roundtripped curve dimension");
    assert!(roundtripped_curve_dimension
        .targets
        .iter()
        .any(|target| matches!(target, PmiTarget::Curve { .. })));
}

#[test]
fn datum_target_writes_and_round_trips() {
    use cadmpeg_ir::pmi::{DatumTargetForm, PmiDefinition, PmiTarget};

    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_semantic_pmi.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#30=DATUM_TARGET('datum target','circle',#5,.F.,'A');\nENDSEC;\nEND-ISO-10303-21;",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode datum target");
    let target = result
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("datum target"))
        .expect("datum target annotation");
    assert_eq!(
        target.targets,
        [PmiTarget::ShapeAspect {
            source_id: cadmpeg_core::text::NonBlankString::new("#30")
                .expect("nonempty source identity")
        }]
    );
    assert!(matches!(
        &target.definition,
        PmiDefinition::DatumTarget {
            form: DatumTargetForm::Circle,
            identification,
            ..
        } if identification == "A"
    ));

    let mut output = Vec::new();
    let report = write_step(
        result.ir(),
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write datum target");
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::PmiAnnotationNotWritten.kind()));
    assert!(String::from_utf8_lossy(&output).contains("DATUM_TARGET("));

    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode written datum target");
    assert!(roundtrip.ir().model.pmi.iter().any(|annotation| matches!(
        &annotation.definition,
        PmiDefinition::DatumTarget {
            form: DatumTargetForm::Circle,
            identification,
            ..
        } if annotation.name.as_deref() == Some("datum target")
            && annotation
                .targets
                .iter()
                .any(|target| matches!(target, PmiTarget::ShapeAspect { .. }))
            && identification == "A"
    )));

    let mut source_less_ir = result.ir().clone();
    source_less_ir
        .model
        .pmi
        .iter_mut()
        .find(|annotation| annotation.name.as_deref() == Some("datum target"))
        .expect("source datum target")
        .targets
        .clear();
    let mut source_less_output = Vec::new();
    let source_less_report = write_step(
        &source_less_ir,
        &mut source_less_output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write source-less datum target");
    assert!(!source_less_report
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::PmiAnnotationNotWritten.kind()));
    assert!(String::from_utf8_lossy(&source_less_output).contains("DATUM_TARGET("));
}

#[test]
fn nonfinite_pmi_placement_omits_records_and_retains_source() {
    let result = decode_inline_result(
        "#1=CARTESIAN_POINT('',(1E400,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('text placement',#1,#2,#3);
#5=TEXT_LITERAL_WITH_ASSOCIATED_CURVES('note',#4,'left',.RIGHT.,$,());
#6=ANNOTATION_TEXT_OCCURRENCE('annotation',(),#5);",
    )
    .expect("omit the bounded point and its PMI dependents");
    let result = EditableDecodeResult::from(result);
    assert!(result.ir().model.points.is_empty());
    assert!(result.ir().model.pmi.is_empty());
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::ParseRecordOmitted.kind()
            && loss.message.contains("finite binary64 range")
    }));
    let retained = result
        .ir()
        .native_unknowns("step")
        .expect("retained source");
    for bytes in [
        b"#1=CARTESIAN_POINT('',(1E400,0.,0.));".as_slice(),
        b"#4=AXIS2_PLACEMENT_3D('text placement',#1,#2,#3);",
        b"#5=TEXT_LITERAL_WITH_ASSOCIATED_CURVES('note',#4,'left',.RIGHT.,$,());",
        b"#6=ANNOTATION_TEXT_OCCURRENCE('annotation',(),#5);",
    ] {
        assert!(retained.iter().any(|record| {
            result
                .source_fidelity()
                .retained_record(record.id.as_str())
                .is_some_and(|source| source.data() == Some(bytes))
        }));
    }
}
