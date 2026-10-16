// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]
use super::{find_color, style_application_order, ColorResolution, StyleDomain};
use std::collections::BTreeSet;

mod cache_storage;
mod collection_limits;
mod string_limits;
mod surface_styles;

/// A style whose override walk does not terminate states no depth. Its
/// position is stated as absence and sorts after every stated depth; the
/// decoder does not read it as the deepest style.
#[test]
fn a_style_with_no_stated_depth_takes_a_position_of_its_own() {
    crate::test_support::with_service_context(b"", |_, ctx| {
        let (exchange, _) = crate::test_support::with_service_context(
        b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;\
#1=COLOUR_RGB('surface',1.,0.,0.);\
#2=SURFACE_STYLE_FILL_AREA(#1);\
#3=PRESENTATION_STYLE_ASSIGNMENT((#2));\
#4=CARTESIAN_POINT('',(0.,0.,0.));\
#5=STYLED_ITEM('',(#3),#4);\
#6=OVER_RIDING_STYLED_ITEM('',(#3),#4,#6);\
ENDSEC;END-ISO-10303-21;",
        crate::parse::parse_inner)
    .expect("parse style graph");
        let graph_limit = 64;

        assert_eq!(
            style_application_order(5, &exchange, graph_limit, ctx).expect("local style depth"),
            (false, Some(0))
        );
        assert_eq!(
            style_application_order(6, &exchange, graph_limit, ctx).expect("local style depth"),
            (true, None)
        );
        let mut styles = vec![6_u64, 5_u64];
        styles.sort_by_key(|id| {
            style_application_order(*id, &exchange, graph_limit, ctx).expect("local style depth")
        });
        assert_eq!(styles, [5, 6]);
    });
}

#[test]
fn surface_color_search_ignores_curve_style_colors() {
    crate::test_support::with_service_context(b"", |_, ctx| {
        let (exchange, _) = crate::test_support::with_service_context(
        b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;\
#1=COLOUR_RGB('curve',0.,0.,1.);\
#2=CURVE_STYLE('',#1);\
#3=COLOUR_RGB('surface',1.,0.,0.);\
#4=SURFACE_STYLE_FILL_AREA(#3);\
#5=PRESENTATION_STYLE_ASSIGNMENT((#2,#4));\
ENDSEC;END-ISO-10303-21;",
        crate::parse::parse_inner)
    .expect("parse style graph");
        let color = find_color(
            5,
            &exchange,
            StyleDomain::Surface,
            super::ColorSearchState {
                storage: &std::cell::RefCell::new(
                    ctx.reserve_scoped(0, "color search fixture")
                        .expect("scope"),
                ),
                active: &mut BTreeSet::new(),
                cache: &mut super::ColorCache::default(),
                losses: &mut Vec::new(),
                invalid_surface_sides: &mut BTreeSet::new(),
            },
            0,
            ctx,
        )
        .expect("colour search fits local resources")
        .expect("surface color");
        let ColorResolution::Candidate(color) = color else {
            panic!("expected one surface color");
        };
        assert_eq!(color.color.r(), 1.0);
        assert_eq!(color.color.b(), 0.0);
    });
}

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::transform::Transform;

use crate::export::write_step;
use crate::loss::StepLossCode;
use crate::test_support::exchange::{decode_inline, export};
use crate::{StepCodec, StepSchema, StepWriteOptions};

#[test]
fn presentation_layer_expands_all_product_definition_views() {
    use cadmpeg_ir::presentation::PresentationItem;

    let result = decode_inline(
        "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('P','Part','',(#2));
#4=PRODUCT_DEFINITION_FORMATION('v1','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#8=PRODUCT_DEFINITION('manufacturing view','',#7,#5);
#7=PRODUCT_DEFINITION_FORMATION('v2','',#3);
#6=PRODUCT_DEFINITION('design view','',#4,#5);
#9=PRESENTATION_LAYER_ASSIGNMENT('definition views','',(#3));",
    );

    let layer = result
        .ir()
        .model
        .presentation_layers
        .first()
        .expect("product presentation layer");
    assert!(matches!(
        layer.items.as_slice(),
        [
            PresentationItem::Product { product: first },
            PresentationItem::Product { product: second },
        ] if first.as_str() == "step:product:product#3-definition-8"
            && second.as_str() == "step:product:product#3-definition-6"
    ));
    assert!(result
        .ir()
        .model
        .product_definitions
        .iter()
        .any(|definition| {
            definition.id.as_str() == "step:product:product#3-definition-8"
                && definition.native_ref.as_deref() == Some("#8")
        }));
    assert!(result
        .ir()
        .model
        .product_definitions
        .iter()
        .any(|definition| {
            definition.id.as_str() == "step:product:product#3-definition-6"
                && definition.native_ref.as_deref() == Some("#6")
        }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn ps04_product_definition_views_keep_identity_when_records_reordered() {
    use cadmpeg_ir::presentation::PresentationItem;

    let decode_fixture = |bytes: &[u8]| {
        StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode PS-04 fixture")
    };
    let product_views = |result: &cadmpeg_ir::codec::DecodeResult| {
        let mut views = result
            .ir()
            .model
            .product_definitions
            .iter()
            .map(|definition| {
                (
                    definition.id.as_str().to_owned(),
                    definition.description.clone(),
                    definition.native_ref.clone(),
                )
            })
            .collect::<Vec<_>>();
        views.sort_by(|left, right| left.0.cmp(&right.0));
        views
    };
    let layer_items = |result: &cadmpeg_ir::codec::DecodeResult| {
        result
            .ir()
            .model
            .presentation_layers
            .first()
            .expect("PS-04 layer")
            .items
            .iter()
            .map(|item| match item {
                PresentationItem::Product { product } => product.as_str().to_owned(),
                other => panic!("unexpected PS-04 layer item: {other:?}"),
            })
            .collect::<Vec<_>>()
    };

    let source_order = decode_fixture(include_bytes!(
        "tests/data/ps04_product_definition_views_source_order.p21"
    ));
    let reordered = decode_fixture(include_bytes!(
        "tests/data/ps04_product_definition_views_reordered.p21"
    ));

    assert_eq!(product_views(&source_order), product_views(&reordered));
    assert_eq!(
        layer_items(&source_order),
        [
            "step:product:product#3-definition-8",
            "step:product:product#3-definition-6",
        ]
    );
    assert_eq!(
        layer_items(&reordered),
        [
            "step:product:product#3-definition-6",
            "step:product:product#3-definition-8",
        ]
    );
    let mut source_members = layer_items(&source_order);
    let mut reordered_members = layer_items(&reordered);
    source_members.sort();
    reordered_members.sort();
    assert_eq!(source_members, reordered_members);
    for result in [&source_order, &reordered] {
        let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("resource allocation did not fail");
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn presentation_layer_preserves_empty_label_and_visibility() {
    use cadmpeg_ir::presentation::PresentationItem;

    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=PRESENTATION_LAYER_ASSIGNMENT('','empty label description',(#1));
#3=INVISIBILITY((#2));",
    );

    let layer = result
        .ir()
        .model
        .presentation_layers
        .first()
        .expect("empty-label presentation layer");
    assert!(layer.name.is_empty());
    assert_eq!(
        layer.description.as_deref(),
        Some("empty label description")
    );
    assert_eq!(layer.visible, Some(false));
    assert!(matches!(
        layer.items.as_slice(),
        [PresentationItem::Source { source_id }] if source_id == "#1"
    ));

    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
pub(crate) fn step_color_assets_round_trip_names_and_tessellation_targets_strictly() {
    let cases: [(&[u8], StepSchema, &[&str]); 2] = [
        (
            include_bytes!("../../../tests/fixtures/ap214_sheet.p21"),
            StepSchema::Ap214,
            &["override red", "blue green"],
        ),
        (
            include_bytes!("../../../tests/fixtures/ap242_tessellation.p21"),
            StepSchema::Ap242Edition3,
            &["mesh green"],
        ),
    ];
    for (source, schema, expected_names) in cases {
        let ir = StepCodec::default()
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("decode styled STEP")
            .into_parts()
            .0;
        let mut bytes = Vec::new();
        write_step(
            &ir,
            &mut bytes,
            schema,
            &StepWriteOptions {
                ..StepWriteOptions::default()
            },
        )
        .expect("strict styled STEP write");
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode written styled STEP");
        let names = decoded
            .ir()
            .model
            .appearances
            .iter()
            .filter_map(|appearance| appearance.name.as_deref())
            .collect::<std::collections::BTreeSet<_>>();
        for expected in expected_names {
            assert!(names.contains(expected), "missing color name {expected}");
        }
        if expected_names == ["mesh green"] {
            assert!(decoded
                .ir()
                .model
                .appearance_bindings
                .iter()
                .any(|binding| {
                    matches!(
                        binding.target,
                        cadmpeg_ir::appearance::AppearanceTarget::Tessellation(_)
                    )
                }));
        }
    }
}

#[test]
fn mapped_presentation_does_not_report_body_placement_loss() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.));
#2=DIRECTION('',(1.,0.));
#3=AXIS2_PLACEMENT_2D('',#1,#2);
#4=REPRESENTATION_MAP(#3,#5);
#5=PRESENTATION_VIEW('',(),#6);
#6=REPRESENTATION_CONTEXT('','');
#7=CARTESIAN_POINT('',(10.,0.));
#8=AXIS2_PLACEMENT_2D('',#7,#2);
#9=MAPPED_ITEM('',#4,#8);",
    );
    assert!(!result.report().losses.iter().any(|loss| loss
        .message
        .contains("MAPPED_ITEM #9 has no resolved body placement")));
}

#[test]
fn presentation_layers_target_complex_tessellation_surface_sets() {
    let mut source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#7=COMPLEX_TRIANGULATED_FACE('strip and fan',#6,4,((1.,0.,0.),(0.,1.,0.),(0.,0.,1.),(0.,0.,-1.)),$,(4,3,2,1),((1,2,3,4)),((1,2,4)));",
        "#7=COMPLEX_TRIANGULATED_SURFACE_SET('strip and fan',#6,4,((1.,0.,0.),(0.,1.,0.),(0.,0.,1.),(0.,0.,-1.)),(4,3,2,1),((1,2,3,4)),((1,2,4)));",
    );
    let end = source.rfind("ENDSEC;").expect("STEP data section end");
    source.insert_str(
        end,
        "#78=PRESENTATION_LAYER_ASSIGNMENT('mesh layer','',(#7));\n",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode layer target");

    let layer = decoded
        .ir()
        .model
        .presentation_layers
        .iter()
        .find(|layer| layer.name == "mesh layer")
        .expect("mesh presentation layer");
    assert!(layer.items.iter().any(|item| matches!(
        item,
        cadmpeg_ir::presentation::PresentationItem::Tessellation { tessellation }
            if tessellation.ends_with("#7")
    )));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);

    let mut output = Vec::new();
    let report = write_step(
        decoded.ir(),
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write tessellation layer");
    assert!(!report.losses.iter().any(|loss| {
        loss.message
            .contains("layer 'mesh layer' has 1 item(s) without a writable STEP carrier")
    }));
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode tessellation layer");
    assert!(roundtrip
        .ir()
        .model
        .presentation_layers
        .iter()
        .any(|layer| {
            layer.name == "mesh layer"
                && layer.items.iter().any(|item| {
                    matches!(
                        item,
                        cadmpeg_ir::presentation::PresentationItem::Tessellation { .. }
                    )
                })
        }));
}

#[test]
fn complex_presentation_annotation_inherits_text_and_placement() {
    use cadmpeg_ir::pmi::PmiDefinition;

    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_presentation_pmi.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#7=TEXT_LITERAL('inspect surface',#6,'left',.RIGHT.,$);",
        "#7=(GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('') TEXT_LITERAL('inspect surface',#6,'left',.RIGHT.,$));",
    )
    .replace(
        "#8=ANNOTATION_TEXT_OCCURRENCE('surface note',(),#7);",
        "#8=(ANNOTATION_TEXT_OCCURRENCE() ANNOTATION_OCCURRENCE() DRAUGHTING_ANNOTATION_OCCURRENCE() GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('surface note') MAPPED_ITEM(#7,#7));",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex presentation PMI");

    assert_eq!(result.ir().model.pmi.len(), 1);
    let PmiDefinition::Presentation {
        ref text,
        ref placement,
        ..
    } = result.ir().model.pmi[0].definition
    else {
        panic!("complex annotation occurrence is not presentation PMI")
    };
    assert_eq!(
        result.ir().model.pmi[0].name.as_deref(),
        Some("surface note")
    );
    assert_eq!(text.as_deref(), Some("inspect surface"));
    let transform = placement.as_ref().expect("annotation placement");
    assert_eq!(transform.rows()[0][3], 10.0);
    assert_eq!(transform.rows()[1][3], 20.0);
    assert_eq!(transform.rows()[2][3], 30.0);
}

#[test]
fn composite_presentation_text_does_not_depend_on_set_order() {
    use cadmpeg_ir::pmi::PmiDefinition;

    let result = decode_inline(
        "#1=TEXT_LITERAL('first',$,'left',.RIGHT.,$);
#2=TEXT_LITERAL('second',$,'left',.RIGHT.,$);
#3=COMPOSITE_TEXT('composite',(#1,#2));
#4=ANNOTATION_TEXT_OCCURRENCE('note',(),#3);",
    );

    let PmiDefinition::Presentation { ref text, .. } = result.ir().model.pmi[0].definition else {
        panic!("composite annotation is not presentation PMI")
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

#[test]
fn presentation_graph_search_does_not_hide_unmodeled_tessellated_carriers() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_presentation_pmi.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#7=TEXT_LITERAL('inspect surface',#6,'left',.RIGHT.,$);",
        "#7=(GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('') TEXT_LITERAL('inspect surface',#6,'left',.RIGHT.,$));",
    )
    .replace(
        "#8=ANNOTATION_TEXT_OCCURRENCE('surface note',(),#7);",
        "#8=(TESSELLATED_GEOMETRIC_SET((#11)) ANNOTATION_TEXT_OCCURRENCE() ANNOTATION_OCCURRENCE() DRAUGHTING_ANNOTATION_OCCURRENCE() GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('surface note') MAPPED_ITEM(#7,#7));",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#11=(GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('tess carrier') TESSELLATED_GEOMETRIC_SET((#12)) TESSELLATED_ITEM());\n#12=TESSELLATED_CURVE_SET('tess curve',#13,((1,2)));\n#13=COORDINATES_LIST('',((0.,0.,0.),(1.,0.,0.)));\nENDSEC;\nEND-ISO-10303-21;",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode presentation graph with tessellated carrier");

    let unknowns = result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown records");
    assert!(
        unknowns
            .iter()
            .any(|record| record.id.as_str().ends_with("#11")),
        "unmodeled tessellated wrapper #11 was not retained"
    );
    for id in [12, 13] {
        assert!(
            !unknowns
                .iter()
                .any(|record| record.id.as_str().ends_with(&format!("#{id}"))),
            "decoded tessellated curve carrier #{id} was retained as opaque"
        );
    }
    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id.as_str() == "step:data:curve#12"));
}

#[test]
fn styled_free_curve_is_a_reachable_source_carrier() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
         #2=CARTESIAN_POINT('',(1.,0.,0.));
         #7=POLYLINE('',(#1,#2));
         #10=STYLED_ITEM('',(),#7);",
    );
    let curve = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#7")
        .expect("styled polyline carrier");
    assert_eq!(
        curve
            .source_object
            .as_ref()
            .map(|source| source.object_id.as_str()),
        Some("#10")
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(!validation.findings.iter().any(|finding| {
        finding.check == cadmpeg_ir::report::check::Check::CarrierReachability
            && finding.entity.as_deref() == Some("step:data:curve#7")
    }));
}

#[test]
fn overriding_style_suppresses_the_base_binding() {
    let result = decode_inline(
        "#1=COLOUR_RGB('blue',0.,0.,1.);
#2=PRESENTATION_STYLE_ASSIGNMENT((#1));
#3=COLOUR_RGB('red',1.,0.,0.);
#4=PRESENTATION_STYLE_ASSIGNMENT((#3));
#10=STYLED_ITEM('',(#2),#20);
#11=OVER_RIDING_STYLED_ITEM('',(#4),#20,#10);
#20=SOURCE_ITEM();",
    );
    assert_eq!(result.ir().model.appearance_bindings.len(), 1);
    let binding = &result.ir().model.appearance_bindings[0];
    let appearance = result
        .ir()
        .model
        .appearances
        .iter()
        .find(|appearance| appearance.id == binding.appearance)
        .expect("overriding appearance");
    let color = appearance.base_color.expect("override color");
    assert_eq!((color.r(), color.g(), color.b()), (1.0, 0.0, 0.0));
}

#[test]
fn independent_face_styles_keep_bindings_without_source_order_scalar_color() {
    let source = String::from_utf8(include_bytes!("../../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#68=STYLED_ITEM('',(#66),#19);",
            "#68=STYLED_ITEM('',(#66),#19);\n#69=COLOUR_RGB('independent blue',0.,0.,1.);\n#70=FILL_AREA_STYLE_COLOUR('',#69);\n#71=FILL_AREA_STYLE('',(#70));\n#72=SURFACE_STYLE_FILL_AREA(#71);\n#73=SURFACE_SIDE_STYLE('',(#72));\n#74=SURFACE_STYLE_USAGE(.BOTH.,#73);\n#75=PRESENTATION_STYLE_ASSIGNMENT((#74));\n#76=STYLED_ITEM('',(#75),#29);",
        );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode independent face styles");

    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.as_str() == "step:data:face#29")
        .expect("styled face");
    assert!(face.color.is_none());
    assert_eq!(
        result
            .ir()
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| {
                matches!(
                    &binding.target,
                    cadmpeg_ir::appearance::AppearanceTarget::Face(face)
                        if face.as_str() == "step:data:face#29"
                )
            })
            .count(),
        2
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::ConflictingScalarColors.kind()
            && loss.message.contains("#47")
            && loss.message.contains("#76")
            && loss.message.contains("scalar color omitted")
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn independent_face_style_permutations_do_not_select_by_instance_order() {
    for input in [
        include_bytes!("tests/data/ap05_independent_styles_first.p21").as_slice(),
        include_bytes!("tests/data/ap05_independent_styles_reordered.p21").as_slice(),
    ] {
        let result = StepCodec::default()
            .decode(&mut Cursor::new(input), &DecodeOptions::default())
            .expect("decode independent style permutation");
        let face = result
            .ir()
            .model
            .faces
            .iter()
            .find(|face| face.id.as_str() == "step:data:face#29")
            .expect("styled face");
        assert!(face.color.is_none());

        let bindings = result
            .ir()
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| {
                matches!(
                    &binding.target,
                    cadmpeg_ir::appearance::AppearanceTarget::Face(face)
                        if face.as_str() == "step:data:face#29"
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(bindings.len(), 2);
        for (red, green, blue) in [(1.0, 0.0, 0.0), (0.0, 0.0, 1.0)] {
            assert!(result.ir().model.appearances.iter().any(|appearance| {
                appearance.base_color.is_some_and(|color| {
                    color.r() == red && color.g() == green && color.b() == blue
                })
            }));
        }
        assert!(result.report().losses.iter().any(|loss| {
            loss.code == StepLossCode::ConflictingScalarColors.kind()
                && loss.message.contains("#47")
                && loss.message.contains("#76")
                && loss.message.contains("scalar color omitted")
        }));
        let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("resource allocation did not fail");
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn independent_same_rgb_styles_choose_lower_alpha_for_scalar_color() {
    const EPS_ALPHA: f32 = 0.000_001;

    let source = String::from_utf8(
        include_bytes!("tests/data/ap05_independent_styles_first.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#60=COLOUR_RGB('independent blue',0.,0.,1.);",
        "#60=COLOUR_RGB('independent red transparent',1.,0.,0.);",
    )
    .replace(
        "#43=SURFACE_STYLE_FILL_AREA(#42);",
        "#43=SURFACE_STYLE_RENDERING_WITH_PROPERTIES(.CONSTANT_SHADING.,#40,(#77));",
    )
    .replace(
        "#63=SURFACE_STYLE_FILL_AREA(#62);",
        "#63=SURFACE_STYLE_RENDERING_WITH_PROPERTIES(.CONSTANT_SHADING.,#60,(#78));",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#77=SURFACE_STYLE_TRANSPARENT(0.25);\n#78=SURFACE_STYLE_TRANSPARENT(0.75);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode same-RGB independent styles");
    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.as_str() == "step:data:face#29")
        .expect("styled face");
    let color = face.color.expect("same-RGB scalar face color");
    assert!((color.a() - 0.25).abs() < EPS_ALPHA);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == StepLossCode::ConflictingScalarColors.kind() }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn same_type_surface_colors_do_not_select_by_alpha_or_source_order() {
    let source = "#1=COLOUR_RGB('red',1.,0.,0.);
#2=COLOUR_RGB('blue',0.,0.,1.);
#3=SURFACE_STYLE_TRANSPARENT(0.25);
#4=SURFACE_STYLE_RENDERING_WITH_PROPERTIES(.CONSTANT_SHADING.,#1,(#3));
#5=SURFACE_STYLE_TRANSPARENT(0.75);
#6=SURFACE_STYLE_RENDERING_WITH_PROPERTIES(.CONSTANT_SHADING.,#2,(#5));
#7=PRESENTATION_STYLE_ASSIGNMENT((#4,#6));
#8=STYLED_ITEM('',(#7),#9);
#9=(ADVANCED_FACE() FACE_SURFACE());";
    let reordered = source.replace("(#4,#6)", "(#6,#4)");

    for input in [source, reordered.as_str()] {
        let result = decode_inline(input);
        assert!(result.ir().model.appearance_bindings.is_empty());
        assert!(result.report().losses.iter().any(|loss| {
            loss.code == StepLossCode::ConflictingScalarColors.kind()
                && loss.message.contains("STYLED_ITEM #8")
                && loss.message.contains("equal-precedence")
        }));
        assert!(result
            .ir()
            .model
            .faces
            .iter()
            .all(|face| face.color.is_none()));
    }
}

#[test]
fn context_dependent_styles_are_not_flattened_without_context() {
    let result = decode_inline(
        "#1=COLOUR_RGB('shaded red',1.,0.,0.);
#2=PRESENTATION_STYLE_ASSIGNMENT((#1));
#3=COLOUR_RGB('wire blue',0.,0.,1.);
#4=PRESENTATION_STYLE_ASSIGNMENT((#3));
#5=CARTESIAN_POINT('shaded context',(0.,0.,0.));
#6=CARTESIAN_POINT('wire context',(1.,0.,0.));
#7=PRESENTATION_STYLE_BY_CONTEXT((#2),#5);
#8=PRESENTATION_STYLE_BY_CONTEXT((#4),#6);
#9=STYLED_ITEM('',(#7,#8),#10);
#10=CARTESIAN_POINT('styled point',(0.,1.,0.));",
    );

    assert!(result.ir().model.appearances.is_empty());
    assert!(result.ir().model.appearance_bindings.is_empty());
    let unknowns = result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena");
    for id in [
        "step:data:styled_item#9",
        "step:data:presentation_style_by_context#7",
        "step:data:presentation_style_by_context#8",
    ] {
        assert!(
            unknowns.iter().any(|record| record.id.as_str() == id),
            "missing {id}"
        );
    }
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::ContextDependentStyleUnresolved.kind()
            && loss.message.contains("#7 in #5")
            && loss.message.contains("#8 in #6")
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn context_dependent_styles_remain_native_for_distinct_contexts() {
    let result = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("tests/data/ap08_context_styles.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode context-qualified style witness");

    assert_eq!(result.ir().model.appearances.len(), 1);
    assert_eq!(result.ir().model.appearance_bindings.len(), 1);
    assert!(matches!(
        &result.ir().model.appearance_bindings[0].target,
        cadmpeg_ir::appearance::AppearanceTarget::Point(point)
            if point.as_str() == "step:data:point#13"
    ));
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::ContextDependentStyleUnresolved.kind()
            && loss.message.contains("#10 in #5")
            && loss.message.contains("#11 in #7")
    }));
    let unknowns = result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena");
    for id in [
        "step:data:styled_item#12",
        "step:data:presentation_style_by_context#10",
        "step:data:presentation_style_by_context#11",
    ] {
        assert!(
            unknowns.iter().any(|record| record.id.as_str() == id),
            "missing {id}"
        );
    }
    assert!(!unknowns
        .iter()
        .any(|record| record.id.as_str() == "step:data:styled_item#15"));
    assert!(result
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.id.as_str() == "step:data:point#3"));
}

#[test]
fn context_style_retention_is_independent_of_style_set_order() {
    for source in [
        include_bytes!("tests/data/ap08_context_styles.p21").as_slice(),
        include_bytes!("tests/data/ap08_context_styles_reordered.p21").as_slice(),
    ] {
        let result = StepCodec::default()
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("decode context style order witness");
        assert!(result.ir().model.appearance_bindings.iter().all(|binding| {
            !matches!(
                &binding.target,
                cadmpeg_ir::appearance::AppearanceTarget::Point(point)
                    if point.as_str() == "step:data:point#3"
            )
        }));
        assert!(result.report().losses.iter().any(|loss| {
            loss.code == StepLossCode::ContextDependentStyleUnresolved.kind()
                && loss.message.contains("#10 in #5")
                && loss.message.contains("#11 in #7")
        }));
        let unknowns = result
            .ir()
            .native_unknowns("step")
            .expect("STEP unknown arena");
        for id in [
            "step:data:styled_item#12",
            "step:data:presentation_style_by_context#10",
            "step:data:presentation_style_by_context#11",
        ] {
            assert!(
                unknowns.iter().any(|record| record.id.as_str() == id),
                "missing {id}"
            );
        }
    }
}

#[test]
fn complex_representation_invisibility_reaches_surface_body() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#39=MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#38),#2);",
        "#39=(CHARACTERIZED_REPRESENTATION() REPRESENTATION('',(#38),#2) MANIFOLD_SURFACE_SHAPE_REPRESENTATION());",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#91=INVISIBILITY((#39));\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex representation invisibility");

    let body = decoded
        .ir()
        .model
        .bodies
        .iter()
        .find(|body| body.id.as_str() == "step:data:body#38")
        .expect("surface body");
    assert_eq!(body.visible, Some(false));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::DecodeWarning.kind()
            && loss
                .message
                .contains("INVISIBILITY #91 targets unsupported item #39")
    }));
}

#[test]
fn styled_item_invisibility_is_binding_scoped() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('surface origin',(0.,0.,0.));
#2=DIRECTION('surface normal',(0.,0.,1.));
#3=DIRECTION('surface u',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('surface placement',#1,#2,#3);
#5=PLANE('styled surface',#4);
#6=COLOUR_RGB('red',1.,0.,0.);
#7=SURFACE_STYLE_RENDERING(#6,$,$,$,$,$);
#8=PRESENTATION_STYLE_ASSIGNMENT((#7));
#9=STYLED_ITEM('',(#8),#5);
#10=COLOUR_RGB('blue',0.,0.,1.);
#11=SURFACE_STYLE_RENDERING(#10,$,$,$,$,$);
#12=PRESENTATION_STYLE_ASSIGNMENT((#11));
#13=STYLED_ITEM('',(#12),#5);
#14=INVISIBILITY((#13));",
    );
    let bindings = result
        .ir()
        .model
        .appearance_bindings
        .iter()
        .filter(|binding| {
            matches!(
                &binding.target,
                cadmpeg_ir::appearance::AppearanceTarget::Surface(surface)
                    if surface.as_str() == "step:data:surface#5"
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(bindings.len(), 2);
    assert!(bindings.iter().any(|binding| {
        binding.source_entity_id.as_deref() == Some("#13") && binding.visible == Some(false)
    }));
    assert!(bindings.iter().any(|binding| {
        binding.source_entity_id.as_deref() == Some("#9") && binding.visible.is_none()
    }));
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::DecodeWarning.kind()
            && loss
                .message
                .contains("INVISIBILITY #14 targets unsupported item #13")
    }));
    assert!(!result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.as_str() == "step:data:invisibility#14"));
}

#[test]
fn surface_style_transparency_transfers_to_appearance_alpha() {
    const EPS_ALPHA: f32 = 0.000_001;

    let result = decode_inline(
        "#1=CARTESIAN_POINT('surface origin',(0.,0.,0.));
#2=DIRECTION('surface normal',(0.,0.,1.));
#3=DIRECTION('surface u',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('surface placement',#1,#2,#3);
#5=PLANE('styled surface',#4);
#6=COLOUR_RGB('red',1.,0.,0.);
#7=SURFACE_STYLE_TRANSPARENT(0.25);
#8=SURFACE_STYLE_RENDERING_WITH_PROPERTIES(.CONSTANT_SHADING.,#6,(#7));
#9=SURFACE_SIDE_STYLE('',(#8));
#10=SURFACE_STYLE_USAGE(.BOTH.,#9);
#11=PRESENTATION_STYLE_ASSIGNMENT((#10));
#12=STYLED_ITEM('',(#11),#5);
#13=SURFACE_STYLE_TRANSPARENT(0.75);
#14=SURFACE_STYLE_RENDERING_WITH_PROPERTIES(.CONSTANT_SHADING.,#6,(#13));
#15=SURFACE_SIDE_STYLE('',(#14));
#16=SURFACE_STYLE_USAGE(.BOTH.,#15);
#17=PRESENTATION_STYLE_ASSIGNMENT((#16));
#18=STYLED_ITEM('',(#17),#5);",
    );

    let alphas = result
        .ir()
        .model
        .appearances
        .iter()
        .filter_map(|appearance| appearance.base_color.map(cadmpeg_ir::topology::Color::a))
        .collect::<Vec<_>>();
    assert_eq!(alphas.len(), 2);
    assert!(alphas.iter().any(|alpha| (*alpha - 0.75).abs() < EPS_ALPHA));
    assert!(alphas.iter().any(|alpha| (*alpha - 0.25).abs() < EPS_ALPHA));
    assert!(result
        .ir()
        .model
        .appearance_bindings
        .iter()
        .any(|binding| binding.source_entity_id.as_deref() == Some("#12")));
}

#[test]
fn duplicate_surface_transparency_properties_do_not_select_by_set_order() {
    for input in [
        include_bytes!("tests/data/ap10_duplicate_transparency_first.p21").as_slice(),
        include_bytes!("tests/data/ap10_duplicate_transparency_reordered.p21").as_slice(),
    ] {
        let result = StepCodec::default()
            .decode(&mut Cursor::new(input), &DecodeOptions::default())
            .expect("decode duplicate transparency witness");
        let appearance = result
            .ir()
            .model
            .appearances
            .iter()
            .find(|appearance| {
                appearance
                    .base_color
                    .is_some_and(|color| color.r() == 1.0 && color.g() == 0.0 && color.b() == 0.0)
            })
            .expect("red appearance");
        assert_eq!(appearance.base_color.expect("appearance color").a(), 1.0);
        assert!(result.report().losses.iter().any(|loss| {
            loss.code == StepLossCode::SurfaceTransparencyConflict.kind()
                && loss.message.contains("rendering #12")
                && loss.message.contains("#10=0.25")
                && loss.message.contains("#11=0.75")
                && loss.message.contains("transparency omitted")
        }));
        let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("resource allocation did not fail");
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn point_style_invisibility_on_a_point_set_is_binding_scoped() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('point',(0.,0.,0.));
#2=GEOMETRIC_CURVE_SET('point set',(#1));
#3=COLOUR_RGB('red',1.,0.,0.);
#4=PRE_DEFINED_MARKER('dot');
#5=POINT_STYLE('point style',#4,1.,#3);
#6=PRESENTATION_STYLE_ASSIGNMENT((#5));
#7=STYLED_ITEM('',(#6),#2);
#8=INVISIBILITY((#7));",
    );
    let binding = result
        .ir()
        .model
        .appearance_bindings
        .iter()
        .find(|binding| binding.source_entity_id.as_deref() == Some("#7"))
        .expect("point styled item appearance binding");
    assert_eq!(binding.visible, Some(false));
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::DecodeWarning.kind()
            && loss
                .message
                .contains("INVISIBILITY #8 targets unsupported item #7")
    }));
}

#[test]
fn invisibility_on_a_base_styled_item_hides_overriding_bindings() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('start',(0.,0.,0.));
#2=CARTESIAN_POINT('end',(1.,0.,0.));
#3=POLYLINE('line',(#1,#2));
#4=COLOUR_RGB('red',1.,0.,0.);
#5=CURVE_STYLE('line style',1.,.POSITIVE.,#4);
#6=PRESENTATION_STYLE_ASSIGNMENT((#5));
#7=STYLED_ITEM('',(#6),#3);
#8=OVER_RIDING_STYLED_ITEM('',(#6),#3,#7);
#9=INVISIBILITY((#7));",
    );
    let binding = result
        .ir()
        .model
        .appearance_bindings
        .iter()
        .find(|binding| binding.source_entity_id.as_deref() == Some("#8"))
        .expect("overriding styled item appearance binding");
    assert_eq!(binding.visible, Some(false));
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::DecodeWarning.kind()
            && loss
                .message
                .contains("INVISIBILITY #9 targets unsupported item #7")
    }));
}

#[test]
fn presentation_layer_invisibility_is_layer_scoped() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('layer point',(0.,0.,0.));
#2=PRESENTATION_LAYER_ASSIGNMENT('hidden layer','',(#1));
#3=INVISIBILITY((#2));",
    );
    let layer = result
        .ir()
        .model
        .presentation_layers
        .iter()
        .find(|layer| layer.name == "hidden layer")
        .expect("hidden presentation layer");
    assert_eq!(layer.visible, Some(false));
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::DecodeWarning.kind()
            && loss
                .message
                .contains("INVISIBILITY #3 targets unsupported item #2")
    }));
    assert!(!result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.as_str() == "step:data:invisibility#3"));
}

#[test]
fn presentation_records_retain_non_color_geometry_owners() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(1.,0.,0.));
#3=POLYLINE('styled curve',(#1,#2));
#4=CURVE_STYLE('line style',$,$,$);
#5=PRESENTATION_STYLE_ASSIGNMENT((#4));
#6=STYLED_ITEM('',(#5),#3);
#7=DIRECTION('',(0.,0.,1.));
#8=AXIS2_PLACEMENT_3D('',#1,#7,$);
#9=PLANE('annotation support',#8);
#10=ANNOTATION_PLANE('annotation plane',(),#9,());",
    );

    let curve = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#3")
        .expect("styled curve");
    assert_eq!(
        curve
            .source_object
            .as_ref()
            .expect("styled curve owner")
            .object_id,
        "#6"
    );
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#9")
        .expect("annotation support surface");
    assert_eq!(
        surface
            .source_object
            .as_ref()
            .expect("annotation plane owner")
            .object_id,
        "#10"
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_styled_item_decodes_color_and_owns_its_curve() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(1.,0.,0.));
#3=POLYLINE('styled curve',(#1,#2));
#4=COLOUR_RGB('red',1.,0.,0.);
#5=PRESENTATION_STYLE_ASSIGNMENT((#4));
#6=(ANNOTATION_CURVE_OCCURRENCE() STYLED_ITEM((#5),#3));
#7=COLOUR_RGB('blue',0.,0.,1.);
#8=PRESENTATION_STYLE_ASSIGNMENT((#7));
#9=(ANNOTATION_CURVE_OCCURRENCE() OVER_RIDING_STYLED_ITEM((#8),#3,#6));",
    );

    let curve = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#3")
        .expect("complex styled curve");
    assert_eq!(
        curve
            .source_object
            .as_ref()
            .expect("complex styled curve owner")
            .object_id,
        "#6"
    );
    assert!(result.ir().model.appearance_bindings.iter().any(|binding| {
        matches!(
            binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Curve(ref curve)
                if curve.as_str() == "step:data:curve#3"
        )
    }));
    assert_eq!(result.ir().model.appearance_bindings.len(), 1);
    let appearance = result
        .ir()
        .model
        .appearances
        .iter()
        .find(|appearance| appearance.id == result.ir().model.appearance_bindings[0].appearance)
        .expect("overriding appearance");
    assert_eq!(appearance.name.as_deref(), Some("blue"));
    assert!(result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .all(|record| record.id.as_str() != "step:data:styled_item#6"));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_colour_rgb_inherits_name_and_components() {
    let result = decode_inline(
        "#1=(COLOUR_RGB(1.,0.,0.) COLOUR_SPECIFICATION('red') COLOUR());
#2=PRESENTATION_STYLE_ASSIGNMENT((#1));
#3=STYLED_ITEM('',(#2),#4);
#4=SOURCE_ITEM();",
    );
    assert_eq!(result.ir().model.appearance_bindings.len(), 1);
    let appearance = result
        .ir()
        .model
        .appearances
        .first()
        .expect("complex RGB appearance");
    assert_eq!(appearance.name.as_deref(), Some("red"));
    assert_eq!(appearance.base_color.unwrap().r(), 1.0);
}

#[test]
fn complex_surface_targets_use_surface_style_domain() {
    let result = decode_inline(
        "#1=COLOUR_RGB('red',1.,0.,0.);
#2=COLOUR_RGB('blue',0.,0.,1.);
#3=CURVE_STYLE('',#1,$,$);
#4=SURFACE_STYLE_RENDERING(#2,$,$,$,$,$);
#5=PRESENTATION_STYLE_ASSIGNMENT((#3,#4));
#6=STYLED_ITEM('',(#5),#7);
#7=(ADVANCED_FACE() FACE_SURFACE());",
    );
    assert_eq!(result.ir().model.appearances.len(), 1);
    assert_eq!(
        result.ir().model.appearances[0].base_color,
        Some(cadmpeg_ir::topology::Color::new(0.0, 0.0, 1.0, 1.0).expect("valid color"))
    );
}

#[test]
fn surface_style_usage_prefers_positive_side_over_set_order() {
    let result = decode_inline(
        "#1=COLOUR_RGB('negative',1.,0.,0.);
#2=SURFACE_STYLE_RENDERING(#1,$,$,$,$,$);
#3=SURFACE_SIDE_STYLE('',(#2));
#4=SURFACE_STYLE_USAGE(.NEGATIVE.,#3);
#5=COLOUR_RGB('positive',0.,1.,0.);
#6=SURFACE_STYLE_RENDERING(#5,$,$,$,$,$);
#7=SURFACE_SIDE_STYLE('',(#6));
#8=SURFACE_STYLE_USAGE(.POSITIVE.,#7);
#9=PRESENTATION_STYLE_ASSIGNMENT((#4,#8));
#10=STYLED_ITEM('',(#9),#11);
#11=(ADVANCED_FACE() FACE_SURFACE());",
    );
    assert_eq!(result.ir().model.appearances.len(), 1);
    assert_eq!(
        result.ir().model.appearances[0].base_color,
        Some(cadmpeg_ir::topology::Color::new(0.0, 1.0, 0.0, 1.0).expect("valid color"))
    );
}

#[test]
fn surface_style_usage_permutations_keep_positive_side_scalar_color() {
    for input in [
        include_bytes!("tests/data/ap06_surface_sides_positive_negative_first.p21").as_slice(),
        include_bytes!("tests/data/ap06_surface_sides_positive_negative_reordered.p21").as_slice(),
    ] {
        let result = StepCodec::default()
            .decode(&mut Cursor::new(input), &DecodeOptions::default())
            .expect("decode surface side permutation");
        assert_eq!(result.ir().model.appearances.len(), 1);
        assert_eq!(
            result.ir().model.appearances[0].base_color,
            Some(cadmpeg_ir::topology::Color::new(0.0, 1.0, 0.0, 1.0).expect("valid color"))
        );
        let face = result
            .ir()
            .model
            .faces
            .iter()
            .find(|face| face.id.as_str() == "step:data:face#29")
            .expect("styled face");
        assert_eq!(
            face.color,
            Some(cadmpeg_ir::topology::Color::new(0.0, 1.0, 0.0, 1.0).expect("valid color"))
        );
        assert!(result.report().losses.is_empty());
        let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("resource allocation did not fail");
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn curve_targets_use_curve_style_domain() {
    let result = decode_inline(
        "#1=COLOUR_RGB('surface',1.,0.,0.);
#2=SURFACE_STYLE_RENDERING(#1,$,$,$,$,$);
#3=COLOUR_RGB('curve',0.,1.,0.);
#4=CURVE_STYLE('',#3,$,$);
#5=PRESENTATION_STYLE_ASSIGNMENT((#2,#4));
#6=STYLED_ITEM('',(#5),#7);
#7=POLYLINE('styled curve',());",
    );
    assert_eq!(result.ir().model.appearances.len(), 1);
    assert_eq!(
        result.ir().model.appearances[0].base_color,
        Some(cadmpeg_ir::topology::Color::new(0.0, 1.0, 0.0, 1.0).expect("valid color"))
    );
}

#[test]
fn point_targets_use_point_style_domain() {
    let result = decode_inline(
        "#1=COLOUR_RGB('surface',1.,0.,0.);
#2=SURFACE_STYLE_RENDERING(#1,$,$,$,$,$);
#3=COLOUR_RGB('point',0.,1.,0.);
#4=POINT_STYLE('',.DOT.,POSITIVE_LENGTH_MEASURE(1.),#3);
#5=PRESENTATION_STYLE_ASSIGNMENT((#2,#4));
#6=STYLED_ITEM('',(#5),#7);
#7=CARTESIAN_POINT('styled point',(0.,0.,0.));",
    );
    assert_eq!(result.ir().model.appearances.len(), 1);
    assert_eq!(
        result.ir().model.appearances[0].base_color,
        Some(cadmpeg_ir::topology::Color::new(0.0, 1.0, 0.0, 1.0).expect("valid color"))
    );
}

#[test]
fn null_style_branch_does_not_suppress_a_sibling_color() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=COLOUR_RGB('red',1.,0.,0.);
#3=PRESENTATION_STYLE_ASSIGNMENT((NULL_STYLE(.NULL.),#2));
#4=STYLED_ITEM('',(#3),#1);",
    );
    assert_eq!(result.ir().model.appearance_bindings.len(), 1);
}

#[test]
fn complex_null_style_inherited_partial_suppresses_false_color_warning() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=(PRESENTATION_STYLE_ASSIGNMENT(()) STYLE_ASSIGNMENT((NULL_STYLE(.NULL.))));
#3=STYLED_ITEM('',(#2),#1);",
    );
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("STYLED_ITEM #3 has no resolved surface color")
    }));
}

#[test]
fn body_layers_and_visibility_cover_every_region_shape_item() {
    use cadmpeg_ir::ids::LayerId;
    use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};

    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let body = ir.model.bodies[0].id.clone();
    let mut region = ir.model.regions[0].clone();
    region.id = "zzzz:test:region#second"
        .try_into()
        .expect("valid identity");
    ir.model.bodies[0].regions.push(region.id.clone());
    ir.model.regions.push(region);
    ir.model.bodies[0].visible = Some(false);
    ir.model.presentation_layers.push(PresentationLayer {
        id: LayerId::mint("test:model:layer#body").expect("identity grammar"),
        name: "all body regions".into(),
        description: None,
        visible: None,
        items: vec![PresentationItem::Body { body }],
    });

    let mut bytes = Vec::new();
    write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write body presentation");
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(&bytes, crate::parse::parse_inner)
            .expect("parse body presentation");
    assert!(diagnostics.is_empty());
    let layer = exchange
        .records()
        .values()
        .find(|record| {
            record
                .partials
                .iter()
                .any(|partial| partial.name == "PRESENTATION_LAYER_ASSIGNMENT")
        })
        .expect("body presentation layer");
    let layer_partial = layer
        .partials
        .iter()
        .find(|partial| partial.name == "PRESENTATION_LAYER_ASSIGNMENT")
        .unwrap();
    let crate::parse::Value::List(layer_items) = &layer_partial.parameters[2] else {
        panic!("layer items are not an aggregate")
    };
    assert_eq!(layer_items.len(), 2);
    let visibility = exchange
        .records()
        .values()
        .find(|record| {
            record
                .partials
                .iter()
                .any(|partial| partial.name == "INVISIBILITY")
        })
        .expect("body invisibility");
    let visibility_partial = visibility
        .partials
        .iter()
        .find(|partial| partial.name == "INVISIBILITY")
        .unwrap();
    let crate::parse::Value::List(hidden_items) = &visibility_partial.parameters[0] else {
        panic!("visibility items are not an aggregate")
    };
    assert_eq!(hidden_items.len(), 2);
}

#[test]
pub(crate) fn presentation_reader_normalizes_invalid_layer_and_common_datum_inputs() {
    use cadmpeg_ir::pmi::PmiDefinition;
    use cadmpeg_ir::presentation::PresentationItem;

    let result = decode_inline(
        "#1=PRESENTATION_LAYER_ASSIGNMENT('','',());
#5=PRODUCT_DEFINITION_SHAPE('PMI shape','',#99);
#7=DATUM('',$,#5,.F.,'A');
#8=DATUM_SYSTEM('system','',#5,.F.,(#20));
#20=DATUM_REFERENCE_COMPARTMENT('',$,#5,.F.,COMMON_DATUM_LIST((#21)),());
#21=DATUM_REFERENCE_ELEMENT('',$,#5,.F.,#7,());
#30=PLUS_MINUS_TOLERANCE(#31,#32);
#31=UNKNOWN_LIMIT();
#32=UNKNOWN_CHARACTERISTIC();
#40=PRESENTATION_LAYER_ASSIGNMENT('inspection','',(#30));
#41=PRESENTATION_LAYER_ASSIGNMENT('invalid empty set','',());
#99=UNRESOLVED_PRODUCT();",
    );
    assert_eq!(result.ir().model.presentation_layers.len(), 1);
    assert_eq!(result.ir().model.presentation_layers[0].name, "inspection");
    assert!(matches!(
        result.ir().model.presentation_layers[0].items.as_slice(),
        [PresentationItem::Source { source_id }] if source_id == "#30"
    ));
    assert!(result.ir().model.pmi.iter().any(|annotation| matches!(
        &annotation.definition,
        PmiDefinition::DatumSystem { references }
            if references.as_slice().len() == 1 && references.as_slice()[0].common_group.is_none()
    )));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn presentation_reader_resolves_complex_datum_reference_inheritance() {
    use cadmpeg_ir::pmi::PmiDefinition;

    let result = decode_inline(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#5=PRODUCT_DEFINITION_SHAPE('PMI shape','',#99);
#7=DATUM('',$,#5,.F.,'A');
#8=DATUM_SYSTEM('system','',#5,.F.,(#20));
#20=(DATUM_REFERENCE_COMPARTMENT() GENERAL_DATUM_REFERENCE(COMMON_DATUM_LIST((#21)),(#22)) SHAPE_ASPECT('','',#5,.F.));
#21=(DATUM_REFERENCE_ELEMENT() GENERAL_DATUM_REFERENCE(#7,()) SHAPE_ASPECT('','',#5,.F.));
#22=DATUM_REFERENCE_MODIFIER_WITH_VALUE(.DISTANCE.,#23);
#23=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.2),#1);
#99=UNRESOLVED_PRODUCT();",
    );
    let system = result
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("system"))
        .expect("complex datum system");
    assert!(matches!(
        &system.definition,
        PmiDefinition::DatumSystem { references }
            if references.as_slice().len() == 1
                && references.as_slice()[0].datum.as_str() == "step:presentation:pmi#7"
                && references.as_slice()[0].common_group.is_none()
                && references.as_slice()[0].modifiers == ["distance:0.2"]
    ));
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| { !loss.message.contains("DATUM_REFERENCE_COMPARTMENT #20") }));
}

#[test]
pub(crate) fn hidden_body_geometry_and_visibility_round_trip() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.bodies[0].visible = Some(false);
    let mut buf = Vec::new();
    let report = write_step(
        &ir,
        &mut buf,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.contains("MANIFOLD_SOLID_BREP"));
    assert!(s.contains("ADVANCED_FACE"));
    assert!(s.contains("INVISIBILITY"));
    assert!(report.losses.is_empty());
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(s.into_bytes()), &DecodeOptions::default())
        .expect("decode hidden body");
    assert_eq!(decoded.ir().model.bodies[0].visible, Some(false));

    let mut transformed = unit_cube().expect("unit cube fixture is admitted");
    transformed.model.bodies[0].visible = Some(false);
    transformed.model.bodies[0].transform = Some(
        cadmpeg_ir::transform::Transform::affine([
            [1.0, 0.0, 0.0, 10.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .expect("affine transform"),
    );
    let transformed_text = export(&transformed);
    assert!(transformed_text.contains("MAPPED_ITEM"));
    assert!(!transformed_text.contains("ADVANCED_BREP_SHAPE_REPRESENTATION"));
    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(transformed_text),
            &DecodeOptions::default(),
        )
        .expect("decode hidden transformed body");
    assert_eq!(decoded.ir().model.bodies[0].visible, Some(false));

    // An explicitly visible body exports unchanged.
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.bodies[0].visible = Some(true);
    let s = export(&ir);
    assert!(s.contains("MANIFOLD_SOLID_BREP"));
}

#[test]
pub(crate) fn body_color_becomes_per_face_styled_item_presentation() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.bodies[0].color =
        Some(cadmpeg_ir::topology::Color::new(0.25, 0.5, 0.75, 1.0).expect("valid color"));
    let face_count = ir.model.faces.len();
    let s = export(&ir);
    assert!(s.contains("COLOUR_RGB('',0.25,0.5,0.75)"));
    assert!(s.contains("MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION"));
    // The body color is pushed down onto every face: one STYLED_ITEM per face,
    // each targeting an ADVANCED_FACE rather than the solid. OCCT/VTK viewers
    // (e.g. f3d) read colors only from faces, not MANIFOLD_SOLID_BREP.
    let styled: Vec<&str> = s.lines().filter(|l| l.contains("STYLED_ITEM")).collect();
    assert_eq!(styled.len(), face_count);
    let solid = s
        .lines()
        .find(|line| line.contains("MANIFOLD_SOLID_BREP"))
        .and_then(|line| line.split(" =").next())
        .unwrap()
        .to_string();
    for item in &styled {
        let target = item
            .rsplit_once(',')
            .map(|(_, tail)| tail.trim_end_matches(");").to_string())
            .unwrap();
        assert_ne!(target, solid, "body color must not style the solid");
        assert!(
            s.lines()
                .any(|line| line.starts_with(&format!("{target} = ADVANCED_FACE"))),
            "styled item must reference a face"
        );
    }
}

#[test]
pub(crate) fn face_appearance_binding_styles_the_advanced_face() {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::AppearanceId;

    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let face = ir.model.faces[0].id.clone();
    ir.model.appearances.push(Appearance {
        id: AppearanceId::mint("test:model:appearance#black".to_string())
            .expect("identity grammar"),
        name: None,
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Some(
            cadmpeg_ir::topology::Color::new(0.125, 0.125, 0.125, 1.0).expect("valid color"),
        ),
        properties: std::collections::BTreeMap::default(),
        textures: Vec::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "test:model:appearance-binding#face"
            .try_into()
            .expect("valid identity"),
        target: AppearanceTarget::Face(face),
        appearance: AppearanceId::mint("test:model:appearance#black".to_string())
            .expect("identity grammar"),
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: std::collections::BTreeMap::default(),
    });
    let s = export(&ir);
    assert!(s.contains("COLOUR_RGB('',0.125,0.125,0.125)"));
    let styled: Vec<&str> = s.lines().filter(|l| l.contains("STYLED_ITEM")).collect();
    assert_eq!(styled.len(), 1);
    // The styled item targets an ADVANCED_FACE instance.
    let target = styled[0]
        .rsplit_once(',')
        .map(|(_, tail)| tail.trim_end_matches(");").to_string())
        .unwrap();
    let face_line = s
        .lines()
        .find(|line| line.starts_with(&format!("{target} = ADVANCED_FACE")));
    assert!(face_line.is_some(), "styled item must reference a face");
}

#[test]
fn vertex_appearance_binding_styles_the_vertex_point() {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::AppearanceId;

    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let vertex = ir.model.vertices[0].id.clone();
    ir.model.appearances.push(Appearance {
        id: AppearanceId::mint("test:model:appearance#vertex".to_string())
            .expect("identity grammar"),
        name: Some("vertex green".to_string()),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Some(
            cadmpeg_ir::topology::Color::new(0.125, 0.75, 0.25, 1.0).expect("valid color"),
        ),
        properties: std::collections::BTreeMap::default(),
        textures: Vec::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "test:model:appearance-binding#vertex"
            .try_into()
            .expect("valid identity"),
        target: AppearanceTarget::Vertex(vertex),
        appearance: AppearanceId::mint("test:model:appearance#vertex".to_string())
            .expect("identity grammar"),
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: std::collections::BTreeMap::default(),
    });

    let text = export(&ir);
    assert!(text.contains("POINT_STYLE"));
    assert!(text.contains("COLOUR_RGB('vertex green',0.125,0.75,0.25)"));

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(text), &DecodeOptions::default())
        .expect("decode vertex appearance");
    assert!(decoded
        .ir()
        .model
        .appearance_bindings
        .iter()
        .any(|binding| {
            matches!(
                &binding.target,
                AppearanceTarget::Vertex(vertex) if vertex.as_str().starts_with("step:data:vertex#")
            )
        }));
}

#[test]
fn point_presentation_layer_writes_the_cartesian_point_carrier() {
    use cadmpeg_ir::ids::LayerId;
    use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};

    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let point = ir.model.points[0].id.clone();
    ir.model.presentation_layers.push(PresentationLayer {
        id: LayerId::mint("test:model:layer#point".to_string()).expect("identity grammar"),
        name: "point layer".to_string(),
        description: Some("standalone points".to_string()),
        visible: None,
        items: vec![PresentationItem::Point { point }],
    });

    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write point layer");
    assert!(!report.losses.iter().any(|loss| {
        loss.message
            .contains("layer 'point layer' has 1 item(s) without a writable STEP carrier")
    }));
    let text = String::from_utf8(bytes).expect("STEP is UTF-8");
    assert!(text.contains("PRESENTATION_LAYER_ASSIGNMENT('point layer','standalone points',"));
}

#[test]
fn presentation_layer_round_trips_product_occurrence_and_pmi_items() {
    use cadmpeg_ir::ids::{LayerId, OccurrenceId, PmiId, ProductDefinitionId};
    use cadmpeg_ir::pmi::{PmiAnnotation, PmiDefinition};
    use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};
    use cadmpeg_ir::products::{Occurrence, OccurrenceParent, ProductDefinition};

    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let body = ir.model.bodies[0].id.clone();
    let parent_product =
        ProductDefinitionId::mint("test:model:product#parent").expect("identity grammar");
    let child_product =
        ProductDefinitionId::mint("test:model:product#child").expect("identity grammar");
    ir.model.product_definitions.extend([
        ProductDefinition {
            id: parent_product.clone(),
            kind: cadmpeg_ir::products::ProductDefinitionKind::Part,
            source_name: Some("Parent assembly".into()),
            label: Some("Parent assembly".into()),
            description: None,
            part_number: None,
            bom_properties: std::collections::BTreeMap::default(),
            bodies: Vec::new(),
            native_ref: None,
        },
        ProductDefinition {
            id: child_product.clone(),
            kind: cadmpeg_ir::products::ProductDefinitionKind::Part,
            source_name: Some("Child part".into()),
            label: Some("Child part".into()),
            description: None,
            part_number: None,
            bom_properties: std::collections::BTreeMap::default(),
            bodies: vec![body],
            native_ref: None,
        },
    ]);
    let root = OccurrenceId::mint("test:model:occurrence#root").expect("identity grammar");
    let child = OccurrenceId::mint("test:model:occurrence#child").expect("identity grammar");
    ir.model.occurrences.extend([
        Occurrence {
            id: root.clone(),
            prototype: cadmpeg_ir::products::PrototypeReference::Local {
                definition: parent_product.clone(),
            },
            parent: OccurrenceParent::Root {},
            ordinal: 0,
            transform: Transform::identity(),
            linked_prototype: None,
            scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
            name: Some("Root assembly".into()),
            visible: None,
            link: None,
            native_ref: None,
        },
        Occurrence {
            id: child.clone(),
            prototype: cadmpeg_ir::products::PrototypeReference::Local {
                definition: child_product,
            },
            parent: OccurrenceParent::Occurrence { occurrence: root },
            ordinal: 0,
            transform: Transform::affine([
                [1.0, 0.0, 0.0, 25.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ])
            .expect("affine transform"),
            linked_prototype: None,
            scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
            name: Some("Child occurrence".into()),
            visible: None,
            link: None,
            native_ref: None,
        },
    ]);
    let annotation = PmiId::mint("test:model:pmi#note").expect("identity grammar");
    ir.model.pmi.push(PmiAnnotation {
        id: annotation.clone(),
        name: Some("inspection note".into()),
        visible: None,
        targets: Vec::new(),
        definition: PmiDefinition::Presentation {
            text: Some("inspect this assembly".into()),
            placement: Some(Transform::identity()),
            semantics: Vec::new(),
        },
    });
    ir.model.presentation_layers.push(PresentationLayer {
        id: LayerId::mint("test:model:layer#mixed").expect("identity grammar"),
        name: "mixed layer".into(),
        description: None,
        visible: None,
        items: vec![
            PresentationItem::Product {
                product: parent_product,
            },
            PresentationItem::Occurrence { occurrence: child },
            PresentationItem::Pmi { annotation },
        ],
    });

    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions {
            ..StepWriteOptions::default()
        },
    )
    .expect("write mixed presentation layer");
    assert!(!report.losses.iter().any(|loss| {
        loss.message
            .contains("layer 'mixed layer' has 3 item(s) without a writable STEP carrier")
    }));

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode mixed presentation layer");
    let layer = decoded
        .ir()
        .model
        .presentation_layers
        .iter()
        .find(|layer| layer.name == "mixed layer")
        .expect("mixed presentation layer");
    assert_eq!(layer.items.len(), 3);
    assert!(matches!(layer.items[0], PresentationItem::Product { .. }));
    assert!(matches!(
        layer.items[1],
        PresentationItem::Occurrence { .. }
    ));
    assert!(matches!(layer.items[2], PresentationItem::Pmi { .. }));
}

/// The soccer-ball case: a body carries a base color and one face overrides it.
/// Every face must be styled (body color pushed down onto the faces that do not
/// override it), and the overriding face must carry its own color.
#[test]
pub(crate) fn face_override_wins_over_body_color_and_body_fills_the_rest() {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::AppearanceId;

    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let face_count = ir.model.faces.len();
    // White body base color.
    ir.model.bodies[0].color =
        Some(cadmpeg_ir::topology::Color::new(1.0, 1.0, 1.0, 1.0).expect("valid color"));
    // Black override on a single face, via an appearance binding.
    let face = ir.model.faces[0].id.clone();
    ir.model.appearances.push(Appearance {
        id: AppearanceId::mint("test:model:appearance#black".to_string())
            .expect("identity grammar"),
        name: None,
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Some(
            cadmpeg_ir::topology::Color::new(0.0, 0.0, 0.0, 1.0).expect("valid color"),
        ),
        properties: std::collections::BTreeMap::default(),
        textures: Vec::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "test:model:appearance-binding#face"
            .try_into()
            .expect("valid identity"),
        target: AppearanceTarget::Face(face),
        appearance: AppearanceId::mint("test:model:appearance#black".to_string())
            .expect("identity grammar"),
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: std::collections::BTreeMap::default(),
    });

    let s = export(&ir);
    // Both colors are present, and every face is styled.
    assert!(s.contains("COLOUR_RGB('',1.,1.,1.)"));
    assert!(s.contains("COLOUR_RGB('',0.,0.,0.)"));
    let styled: Vec<&str> = s.lines().filter(|l| l.contains("STYLED_ITEM")).collect();
    assert_eq!(styled.len(), face_count);
    // Each color's style chain is emitted once and shared; grouping the styled
    // items by their style ref must yield exactly two groups sized 1 and
    // face_count - 1 (the lone override plus every inherited face).
    let mut per_style = std::collections::BTreeMap::<String, usize>::default();
    for item in &styled {
        // STYLED_ITEM('color',(#psa),#face)
        let psa = item
            .split_once(",(")
            .and_then(|(_, tail)| tail.split(')').next())
            .unwrap()
            .to_string();
        *per_style.entry(psa).or_default() += 1;
    }
    let mut counts: Vec<usize> = per_style.values().copied().collect();
    counts.sort_unstable();
    assert_eq!(counts, vec![1, face_count - 1]);
}
