// SPDX-License-Identifier: Apache-2.0
//! STEP geometric validation-property tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::StepCodec;

const VALIDATION_LIMIT_SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=PROPERTY_DEFINITION('geometric validation property','description',$);#2=REPRESENTATION('unused',(),$);#3=PROPERTY_DEFINITION_REPRESENTATION(#1,#2);ENDSEC;END-ISO-10303-21;";
const VALIDATION_COLLECTION_SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=PROPERTY_DEFINITION('geometric validation property','description',$);#2=REPRESENTATION('unused',(#4),$);#3=PROPERTY_DEFINITION_REPRESENTATION(#1,#2);#4=CARTESIAN_POINT('point',(1.,2.,3.));#5=ITEM(#4);ENDSEC;END-ISO-10303-21;";
const VALIDATION_UNSUPPORTED_SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=PROPERTY_DEFINITION('geometric validation property','description',$);#2=REPRESENTATION('unused',(#4),$);#3=PROPERTY_DEFINITION_REPRESENTATION(#1,#2);#4=ITEM();ENDSEC;END-ISO-10303-21;";

fn validation_resource_refuses(
    source: &[u8],
    operation: &str,
    dimension: cadmpeg_core::decode::ResourceDimension,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid validation collection source");
    let setup_arena = DecodeArena::new();
    let setup_policy = DecodePolicy::service();
    let (setup_ctx, _) = DecodeContext::from_root_bytes(source, &setup_arena, &setup_policy)
        .expect("root fits setup policy");
    let mut setup_ir = cadmpeg_ir::document::CadIr::empty();
    let (geometry, _) = crate::reader::geometry::decode(&exchange, &mut setup_ir, &setup_ctx)
        .expect("geometry setup decodes");
    let refused = {
        let error =
            cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |limit| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                match dimension {
                    ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = limit;
                    }
                    ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
                    _ => unreachable!("test only selects collection or retained limits"),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
                    .expect("root fits selected policy");
                let mut ir = setup_ir.clone();
                (super::decode(&exchange, &geometry.value, &mut ir, &ctx)).map(|_| ())
            });
        matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == dimension
                    && refusal.operation == operation)
    };
    assert!(refused, "no {dimension:?} limit refused {operation}");
}

macro_rules! validation_collection_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            validation_resource_refuses(
                VALIDATION_COLLECTION_SOURCE,
                $operation,
                cadmpeg_core::decode::ResourceDimension::CollectionItems,
            );
        }
    };
}

validation_collection_test!(
    validation_representation_items_refuse_collection_limit,
    "step_validation_representation_items"
);
validation_collection_test!(
    validation_representations_refuse_collection_limit,
    "step_validation_representations"
);
validation_collection_test!(
    validation_used_representations_refuse_collection_limit,
    "step_validation_used_representations"
);
validation_collection_test!(
    validation_points_refuse_collection_limit,
    "step_validation_points"
);
validation_collection_test!(
    validation_claims_refuse_collection_limit,
    "step_validation_claims"
);
validation_collection_test!(
    validation_referenced_points_refuse_collection_limit,
    "step_validation_referenced_points"
);
validation_collection_test!(
    validation_notes_refuse_collection_limit,
    "step_validation_notes"
);

#[test]
fn validation_note_text_refuses_retained_limit() {
    validation_resource_refuses(
        VALIDATION_COLLECTION_SOURCE,
        "step_validation_note_text",
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
    );
}

#[test]
fn validation_losses_refuse_collection_limit() {
    validation_resource_refuses(
        VALIDATION_UNSUPPORTED_SOURCE,
        "step_validation_losses",
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
    );
}

fn validation_limit_result(
    retained_limit: Option<u64>,
    collection_limit: Option<u64>,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let (exchange, _) = crate::test_support::with_service_context(
        VALIDATION_LIMIT_SOURCE,
        crate::parse::parse_inner,
    )
    .expect("valid validation-property exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    if let Some(limit) = retained_limit {
        policy.limits.max_retained_bytes = limit;
    }
    if let Some(limit) = collection_limit {
        policy.limits.max_collection_items = limit;
    }
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let geometry_policy = DecodePolicy::service();
    let (geometry_ctx, _) =
        DecodeContext::from_root_bytes(VALIDATION_LIMIT_SOURCE, &arena, &geometry_policy)
            .expect("root fits geometry policy");
    let (geometry, _) = crate::reader::geometry::decode(&exchange, &mut ir, &geometry_ctx)?;
    let (ctx, _) = DecodeContext::from_root_bytes(VALIDATION_LIMIT_SOURCE, &arena, &policy)
        .expect("root fits selected policy");
    super::decode(&exchange, &geometry.value, &mut ir, &ctx)?;
    Ok(())
}

#[test]
fn validation_property_name_refuses_retained_limit() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    assert!(matches!(
        validation_limit_result(Some(1), None),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_string_text"
    ));
}

#[test]
fn validation_property_description_refuses_retained_limit() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    assert!(matches!(
        validation_limit_result(Some(cadmpeg_core::decode::u64_from_index("geometric validation property".len())), None),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_string_text"
    ));
}

#[test]
fn validation_property_map_refuses_collection_limit() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    assert!(matches!(
        validation_limit_result(None, Some(0)),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_validation_properties"
    ));
}

#[test]
fn validation_mesh_edges_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let source = include_bytes!("../../../tests/fixtures/ap242_tessellation.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("mesh validation source decodes");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("root fits collection policy");
    assert!(matches!(
        super::mesh_properties(result.ir(), &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_validation_mesh_edges"
    ));
}

#[test]
fn validation_mesh_triangles_refuse_work_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let source = include_bytes!("../../../tests/fixtures/ap242_tessellation.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("mesh validation source decodes");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("root fits work policy");
    assert!(matches!(
        super::mesh_properties(result.ir(), &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::WorkUnits
                && refusal.operation == "step_validation_mesh_triangles"
    ));
}

#[test]
fn complex_validation_measure_carrier_is_decoded() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#42=REPRESENTATION('surface area',(#43),#2);",
        "#42=(REPRESENTATION('surface area',(#43),#2) SHAPE_REPRESENTATION());",
    )
    .replace(
        "#43=MEASURE_REPRESENTATION_ITEM('surface area measure',AREA_MEASURE(50.),#44);",
        "#43=(MEASURE_REPRESENTATION_ITEM() MEASURE_WITH_UNIT(AREA_MEASURE(50.),#44) REPRESENTATION_ITEM('surface area measure'));",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex validation measure");

    assert!(result.report().notes.iter().any(|note| {
        note == "geometric validation surface area triangle sheet: expected 50, tessellation approximation 50"
    }));
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("geometric validation property #41 has an unsupported value")
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn direct_area_and_volume_unit_subtypes_scale_validation_measures() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace("#44=DERIVED_UNIT((#55));", "#44=AREA_UNIT((#55));")
    .replace("#53=DERIVED_UNIT((#56));", "#53=VOLUME_UNIT((#56));");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode direct validation unit subtypes");

    assert!(result.report().notes.iter().any(|note| {
        note == "geometric validation surface area triangle sheet: expected 50, tessellation approximation 50"
    }));
    assert!(result.report().notes.iter().any(|note| {
        note == "geometric validation volume open sheet volume: expected 0, tessellation approximation 0"
    }));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.message.contains("unit scale did not resolve") }));
    let unknowns = result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown records");
    for id in [44, 53, 55, 56] {
        assert!(
            !unknowns
                .iter()
                .any(|record| record.id.as_str().ends_with(&format!("#{id}"))),
            "validation unit carrier #{id} was not typed"
        );
    }
}

#[test]
fn validation_representation_decodes_all_measure_items() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#42=REPRESENTATION('surface area',(#43),#2);",
        "#42=REPRESENTATION('surface area',(#43,#52),#2);",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode validation representation with multiple items");

    assert!(result.report().notes.iter().any(|note| {
        note == "geometric validation surface area triangle sheet: expected 50, tessellation approximation 50"
    }));
    assert!(result.report().notes.iter().any(|note| {
        note == "geometric validation volume triangle sheet: expected 0, tessellation approximation 0"
    }));
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("geometric validation property #41 has unsupported item")
    }));
}

#[test]
fn ps06_combined_validation_items_ignore_set_order_and_keep_siblings() {
    let decode_fixture = |bytes: &[u8]| {
        StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode PS-06 fixture")
    };
    let validation_notes = |result: &cadmpeg_ir::codec::DecodeResult| {
        let mut notes = result
            .report()
            .notes
            .iter()
            .filter(|note| note.starts_with("geometric validation "))
            .cloned()
            .collect::<Vec<_>>();
        notes.sort();
        notes
    };
    let unsupported_items = |result: &cadmpeg_ir::codec::DecodeResult| {
        result
            .report()
            .losses
            .iter()
            .filter(|loss| {
                loss.message
                    .contains("geometric validation property #41 has unsupported item")
            })
            .map(|loss| loss.message.clone())
            .collect::<Vec<_>>()
    };

    let source_order = decode_fixture(include_bytes!(
        "tests/data/ps06_combined_validation_properties_source_order.p21"
    ));
    let reordered = decode_fixture(include_bytes!(
        "tests/data/ps06_combined_validation_properties_reordered.p21"
    ));

    let expected_notes = vec![
        "geometric validation centroid combined validation: expected (1,2,3)".to_owned(),
        "geometric validation surface area combined validation: expected 50".to_owned(),
        "geometric validation volume combined validation: expected 8".to_owned(),
    ];
    assert_eq!(validation_notes(&source_order), expected_notes);
    assert_eq!(validation_notes(&reordered), expected_notes);
    assert_eq!(
        unsupported_items(&source_order),
        ["geometric validation property #41 has unsupported item #60"]
    );
    assert_eq!(
        unsupported_items(&source_order),
        unsupported_items(&reordered)
    );
}

#[test]
fn validation_shape_representation_with_parameters_uses_inherited_items() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#42=REPRESENTATION('surface area',(#43),#2);",
        "#42=SHAPE_REPRESENTATION_WITH_PARAMETERS('surface area',(#43),#2);",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode parameterized validation representation");

    assert!(result.report().notes.iter().any(|note| {
        note == "geometric validation surface area triangle sheet: expected 50, tessellation approximation 50"
    }));
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("geometric validation property #41 has unsupported item")
    }));
}

#[test]
fn numerical_followup_mesh_volume_and_centroid_are_translation_invariant() {
    use cadmpeg_ir::{
        math::Point3,
        tessellation::{Tessellation, TessellationMesh},
    };
    let source = include_bytes!("../../../tests/fixtures/ap242_tessellation.p21");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    assert_eq!(ir.model.bodies.len(), 1);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("root fits selected policy");
    for offset in [0.0, 1e6, 1e9] {
        let points = [[0., 0., 0.], [2., 0., 0.], [0., 1., 0.], [0., 0., 1.]]
            .map(|p| Point3::new(p[0] + offset, p[1] + offset, p[2] + offset))
            .to_vec();
        let mesh = TessellationMesh::from_list_lanes(
            points,
            vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
            None,
        )
        .unwrap();
        let mut tessellation = Tessellation::new(
            cadmpeg_ir::tessellation::TessellationId::mint(
                "test:step:tessellation#numerical-followup",
            )
            .expect("valid identity"),
            mesh,
            Vec::new(),
        )
        .unwrap();
        tessellation.body = Some(ir.model.bodies[0].id.clone());
        ir.model.tessellations = vec![tessellation];
        let properties = super::mesh_properties(&ir, &ctx)
            .expect("mesh budget admits calculation")
            .expect("mesh properties exist");
        assert!((properties.volume - 1.0 / 3.0).abs() <= f64::EPSILON);
        assert_eq!(
            properties.centroid,
            Point3::new(offset + 0.5, offset + 0.25, offset + 0.25)
        );
    }
}

#[test]
fn numerical_seventh_mesh_mass_properties_preserve_uniform_scale() {
    use cadmpeg_ir::{
        math::Point3,
        tessellation::{Tessellation, TessellationMesh},
    };
    let source = include_bytes!("../../../tests/fixtures/ap242_tessellation.p21");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    assert_eq!(ir.model.bodies.len(), 1);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("root fits selected policy");
    for scale in [1.0e-90, 1.0, 1.0e90] {
        let points = [[0., 0., 0.], [2., 0., 0.], [0., 1., 0.], [0., 0., 1.]]
            .map(|p| Point3::new(p[0] * scale, p[1] * scale, p[2] * scale))
            .to_vec();
        let mesh = TessellationMesh::from_list_lanes(
            points,
            vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
            None,
        )
        .unwrap();
        let mut tessellation = Tessellation::new(
            cadmpeg_ir::tessellation::TessellationId::mint(
                "test:step:tessellation#numerical-followup",
            )
            .expect("valid identity"),
            mesh,
            Vec::new(),
        )
        .unwrap();
        tessellation.body = Some(ir.model.bodies[0].id.clone());
        ir.model.tessellations = vec![tessellation];
        let properties = super::mesh_properties(&ir, &ctx)
            .expect("mesh budget admits calculation")
            .expect("mesh properties exist");
        assert!(
            (properties.volume / scale / scale / scale - 1.0 / 3.0).abs() <= 8.0 * f64::EPSILON
        );
        for (actual, expected) in [
            (properties.centroid.x / scale, 0.5),
            (properties.centroid.y / scale, 0.25),
            (properties.centroid.z / scale, 0.25),
        ] {
            assert!((actual - expected).abs() <= 8.0 * f64::EPSILON);
        }
    }
}
