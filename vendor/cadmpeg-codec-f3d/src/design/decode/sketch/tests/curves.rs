// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::sketch::{
    decode_circular_arc, decode_line, SketchCurveClass, CURRENT_SKETCH_NURBS_TYPE,
    SKETCH_CIRCULAR_TYPES, SKETCH_LINE_TYPES, SKETCH_TEXT_FRAME_LINE_TYPE_GUID,
};
use crate::records::sketch_geometry::SketchCurveGeometry;
use cadmpeg_ir::math::{Point3, Vector3};

fn tested_decode_sketch_curve_geometry(
    payload: &[u8],
    geometry_shift: usize,
    record_index: u32,
    class: SketchCurveClass,
    record_at: usize,
) -> Result<
    Option<crate::design::decode::sketch::DecodedSketchCurveGeometry>,
    cadmpeg_core::CodecError,
> {
    crate::design::test_support::with_test_decode_context(|ctx| {
        crate::design::decode::sketch::decode_sketch_curve_geometry(
            ctx,
            payload,
            geometry_shift,
            record_index,
            class,
            record_at,
        )
    })
}

#[test]
fn line_components_refuse_unrepresentable_scaled_endpoints() {
    let mut values = [0.0; 12];
    values[3] = 1.0;
    values[6] = 1.0;
    values[11] = 1.0;
    values[0] = 1.0e308;
    let error = crate::design::test_support::with_test_decode_context(|ctx| {
        crate::design::decode::sketch::decode_line_components(
            ctx,
            &values,
            Vector3::new(0.0, 0.0, 1.0),
            17,
        )
    })
    .expect_err("scaled start must fit");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    assert!(error.to_string().contains("byte 17"));
    values[0] = 0.0;
    values[3] = 1.0e308;
    let error = crate::design::test_support::with_test_decode_context(|ctx| {
        crate::design::decode::sketch::decode_line_components(
            ctx,
            &values,
            Vector3::new(0.0, 0.0, 1.0),
            17,
        )
    })
    .expect_err("scaled end must fit");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    assert!(error.to_string().contains("byte 17"));
}

fn analytic_payload(values: [f64; 12]) -> Vec<u8> {
    let mut payload = vec![0; 133];
    for value in values {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload
}

#[test]
fn typed_line_source_reports_scaled_start_overflow_at_record() {
    let payload = analytic_payload([
        f64::MAX,
        0.0,
        0.0,
        1.0,
        0.0,
        0.0,
        1.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ]);
    match tested_decode_sketch_curve_geometry(&payload, 0, 41, SketchCurveClass::Line, 17) {
        Err(cadmpeg_core::CodecError::Malformed(message)) => {
            assert!(message.contains("byte 17"));
        }
        _ => panic!("scaled line start must be malformed at its source record"),
    }
}

#[test]
fn circular_arc_refuses_unrepresentable_scaled_center_at_source_record() {
    let payload = analytic_payload([
        f64::MAX,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        1.0,
        0.0,
        0.0,
        1.0,
        0.0,
        1.0,
    ]);
    let error = crate::design::test_support::with_test_decode_context(|ctx| {
        decode_circular_arc(ctx, &payload, 17)
    })
    .expect_err("scaled center must fit");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    assert!(error.to_string().contains("byte 17"));
}

#[test]
fn stable_type_guid_selects_line_when_the_scalar_payload_also_accepts_as_an_arc() {
    let payload = analytic_payload([
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        1.0,
        0.0,
        0.0,
        std::f64::consts::FRAC_1_SQRT_2,
        0.0,
        std::f64::consts::FRAC_1_SQRT_2,
    ]);
    assert!(
        crate::design::test_support::with_test_decode_context(|ctx| decode_line(ctx, &payload, 0))
            .expect("line parse")
            .is_some()
    );
    assert!(
        crate::design::test_support::with_test_decode_context(|ctx| decode_circular_arc(
            ctx, &payload, 0
        ))
        .expect("arc parse")
        .is_some()
    );

    let line = tested_decode_sketch_curve_geometry(&payload, 0, 41, SketchCurveClass::Line, 0)
        .expect("line admission")
        .expect("typed line payload");
    assert_eq!(line.geometry_offset, 133);
    assert_eq!(
        line.geometry,
        SketchCurveGeometry::line(
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 10.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap()
    );

    let circular =
        tested_decode_sketch_curve_geometry(&payload, 0, 41, SketchCurveClass::Circular, 0)
            .expect("arc admission")
            .expect("typed circular payload");
    assert!(matches!(circular.geometry, SketchCurveGeometry::Arc { .. }));
}

#[test]
fn typed_line_accepts_the_referenced_compact_planar_form() {
    let mut payload = vec![0; 133];
    payload.push(1);
    payload.extend_from_slice(&42u32.to_le_bytes());
    payload.extend_from_slice(&[0; 6]);
    for value in [0.5, 0.875, 0.0, 0.0, -1.75, 0.0, 0.0, -1.0, 0.0] {
        payload.extend_from_slice(&f64::to_le_bytes(value));
    }
    payload.push(1);
    payload.extend_from_slice(&37u32.to_le_bytes());
    payload.extend_from_slice(&[0; 6]);

    let parsed = tested_decode_sketch_curve_geometry(&payload, 0, 41, SketchCurveClass::Line, 0)
        .expect("line admission")
        .expect("typed referenced compact line");
    assert_eq!(parsed.geometry_offset, 144);
    assert!(matches!(parsed.geometry, SketchCurveGeometry::Line { .. }));
}

#[test]
fn curve_type_versions_select_only_their_settled_grammars() {
    for (type_guid, version, module) in SKETCH_LINE_TYPES {
        assert_eq!(
            SketchCurveClass::of(type_guid, version, module),
            Some(SketchCurveClass::Line)
        );
    }
    for (type_guid, version, module) in SKETCH_CIRCULAR_TYPES {
        assert_eq!(
            SketchCurveClass::of(type_guid, version, module),
            Some(SketchCurveClass::Circular)
        );
    }
    assert_eq!(
        SketchCurveClass::of(
            CURRENT_SKETCH_NURBS_TYPE.0,
            CURRENT_SKETCH_NURBS_TYPE.1,
            CURRENT_SKETCH_NURBS_TYPE.2,
        ),
        Some(SketchCurveClass::Nurbs)
    );
    assert_eq!(
        SketchCurveClass::of(SKETCH_TEXT_FRAME_LINE_TYPE_GUID, 0, "MSketch"),
        Some(SketchCurveClass::TextFrameLine)
    );
    assert_eq!(
        SketchCurveClass::of(SKETCH_LINE_TYPES[0].0, 0, "Geometry"),
        None
    );
    assert_eq!(
        SketchCurveClass::of(SKETCH_LINE_TYPES[0].0, 3, "Geometry"),
        None
    );
    assert_eq!(
        SketchCurveClass::of(SKETCH_CIRCULAR_TYPES[0].0, 1, "Geometry"),
        None
    );
    assert_eq!(
        SketchCurveClass::of(CURRENT_SKETCH_NURBS_TYPE.0, 2, CURRENT_SKETCH_NURBS_TYPE.2,),
        None
    );
    assert_eq!(
        SketchCurveClass::of(SKETCH_TEXT_FRAME_LINE_TYPE_GUID, 1, "MSketch"),
        None
    );
    assert_eq!(
        SketchCurveClass::of(SKETCH_LINE_TYPES[0].0, 2, "MSketch"),
        None
    );
}

#[test]
fn line_start_overflow_diagnostic_refuses_retained_limit() {
    assert_line_diagnostic_limit(
        [
            f64::MAX,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
        ],
        "F3D sketch line at byte 17 overflows millimetres",
    );
}

#[test]
fn line_end_overflow_diagnostic_refuses_retained_limit() {
    assert_line_diagnostic_limit(
        [
            0.0,
            0.0,
            0.0,
            f64::MAX,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
        ],
        "F3D sketch line at byte 17 overflows millimetres",
    );
}

#[test]
fn line_displacement_overflow_diagnostic_refuses_retained_limit() {
    assert_line_diagnostic_limit(
        [
            -1.0e307, 0.0, 0.0, 2.0e307, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ],
        "F3D sketch line at byte 17 has an overflowing displacement",
    );
}

fn assert_line_diagnostic_limit(values: [f64; 12], expected: &str) {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_retained_bytes = u64::try_from(expected.len() - 1).unwrap();

    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(crate::design::decode::sketch::decode_line_components(&ctx, &values,
        Vector3::new(0.0, 0.0, 1.0), 17), Err(cadmpeg_core::CodecError::ResourceLimit(failure))
        if failure.operation == "f3d Design diagnostic")
    );
    policy.limits.max_retained_bytes += 1;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(crate::design::decode::sketch::decode_line_components(&ctx, &values,
        Vector3::new(0.0, 0.0, 1.0), 17), Err(cadmpeg_core::CodecError::Malformed(message)) if message == expected)
    );
}

#[test]
fn arc_center_overflow_diagnostic_refuses_retained_limit() {
    assert_arc_diagnostic_limit([
        f64::MAX,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        1.0,
        0.0,
        0.0,
        1.0,
        0.0,
        1.0,
    ]);
}

#[test]
fn arc_radius_overflow_diagnostic_refuses_retained_limit() {
    assert_arc_diagnostic_limit([
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        1.0,
        0.0,
        0.0,
        f64::MAX,
        0.0,
        1.0,
    ]);
}

fn assert_arc_diagnostic_limit(values: [f64; 12]) {
    let payload = analytic_payload(values);
    let expected = "F3D sketch arc at byte 17 overflows millimetres";
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_retained_bytes = u64::try_from(expected.len() - 1).unwrap();

    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
    assert!(matches!(decode_circular_arc(&ctx, &payload, 17),
        Err(cadmpeg_core::CodecError::ResourceLimit(failure)) if failure.operation == "f3d Design diagnostic"));
    policy.limits.max_retained_bytes += 1;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
    assert!(matches!(decode_circular_arc(&ctx, &payload, 17),
        Err(cadmpeg_core::CodecError::Malformed(message)) if message == expected));
}

#[test]
fn sketch_nurbs_decoder_keeps_constructor_refusals_in_the_outer_result() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_core::CodecError;
    for allowance in 0..=6 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = allowance;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = crate::design::decode::sketch::admit_source_sketch_nurbs(
            &ctx,
            1,
            0.0,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![],
            &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            0,
        );
        let Err(CodecError::ResourceLimit(original)) = result else {
            panic!("constructor refusal must not disappear");
        };
        assert_eq!(original.used, allowance);
        assert_eq!(original.additional, 1);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original)
        );
    }
}
