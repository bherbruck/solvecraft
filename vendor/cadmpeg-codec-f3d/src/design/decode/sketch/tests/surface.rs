// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::sketch::parse_sketch_surface;
use cadmpeg_ir::math::Point3;

fn tested_parse_sketch_surface(
    payload: &[u8],
    record_at: usize,
) -> Result<Option<crate::design::decode::sketch::ParsedSketchSurface>, cadmpeg_core::CodecError> {
    crate::design::test_support::with_test_decode_context(|ctx| {
        parse_sketch_surface(ctx, payload, record_at)
    })
}

fn canonical_surface_payload() -> Vec<u8> {
    let mut payload = vec![0; 315];
    payload[20] = 1;
    payload[21..25].copy_from_slice(&2u32.to_le_bytes());
    payload[25..29].copy_from_slice(&13u32.to_le_bytes());
    payload[29..42].copy_from_slice(b"EntityGenesis");
    payload[42..46].copy_from_slice(&23u32.to_le_bytes());
    payload[46..69].copy_from_slice(b"IntrinsicMetaTypeuint64");
    payload[69..77].copy_from_slice(&17u64.to_le_bytes());
    payload[77..81].copy_from_slice(&11u32.to_le_bytes());
    payload[81..92].copy_from_slice(b"surface_tag");
    payload[92..96].copy_from_slice(&23u32.to_le_bytes());
    payload[96..119].copy_from_slice(b"IntrinsicMetaTypeuint64");
    payload[119..127].copy_from_slice(&29u64.to_le_bytes());
    payload[127..131].copy_from_slice(&4u32.to_le_bytes());
    let coordinates = [
        0.0f64, 0.0, 0.0, 0.0, 2.0, 0.0, 3.0, 0.0, 0.0, 3.0, 2.0, 1.0,
    ];
    for (index, coordinate) in coordinates.into_iter().enumerate() {
        let at = 131 + index * 8;
        payload[at..at + 8].copy_from_slice(&coordinate.to_le_bytes());
    }
    let degrees_at = 131 + coordinates.len() * 8;
    payload[degrees_at..degrees_at + 4].copy_from_slice(&1u32.to_le_bytes());
    payload[degrees_at + 4..degrees_at + 8].copy_from_slice(&1u32.to_le_bytes());
    payload[degrees_at + 8..degrees_at + 12].copy_from_slice(&4u32.to_le_bytes());
    let mut at = degrees_at + 12;
    for knot in [0.0f64, 0.0, 1.0, 1.0] {
        payload[at..at + 8].copy_from_slice(&knot.to_le_bytes());
        at += 8;
    }
    payload[at..at + 4].copy_from_slice(&4u32.to_le_bytes());
    at += 4;
    for knot in [0.0f64, 0.0, 1.0, 1.0] {
        payload[at..at + 8].copy_from_slice(&knot.to_le_bytes());
        at += 8;
    }
    payload[at..at + 4].copy_from_slice(&2u32.to_le_bytes());
    payload[at + 4..at + 8].copy_from_slice(&2u32.to_le_bytes());

    payload
}

#[test]
fn sketch_surface_parser_recovers_tensor_product_grid() {
    let payload = canonical_surface_payload();
    let surface = tested_parse_sketch_surface(&payload, 0)
        .expect("surface admission")
        .expect("canonical surface payload");
    assert_eq!(surface.entity_genesis, Some(17));
    assert_eq!(surface.persistent_id.get(), 29);
    assert_eq!(
        (
            surface.geometry.u_degree.get(),
            surface.geometry.v_degree.get()
        ),
        (1, 1)
    );
    assert_eq!(
        surface
            .geometry
            .u_knots
            .iter()
            .copied()
            .map(cadmpeg_ir::scalar::FiniteReal::get)
            .collect::<Vec<_>>(),
        [0.0, 0.0, 1.0, 1.0]
    );
    assert_eq!(
        surface
            .geometry
            .v_knots
            .iter()
            .copied()
            .map(cadmpeg_ir::scalar::FiniteReal::get)
            .collect::<Vec<_>>(),
        [0.0, 0.0, 1.0, 1.0]
    );
    assert_eq!(surface.geometry.control_points.len(), 2);
    assert_eq!(surface.geometry.control_points[0].len(), 2);
    assert_eq!(
        surface.geometry.control_points[1][1].get(),
        Point3::new(30.0, 20.0, 10.0)
    );
}

#[test]
fn sketch_surface_parser_refuses_scaled_coordinate_overflow() {
    let mut payload = canonical_surface_payload();
    payload[131..139].copy_from_slice(&f64::MAX.to_le_bytes());
    let error = tested_parse_sketch_surface(&payload, 0).expect_err("scaled coordinate overflow");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    assert!(error
        .to_string()
        .contains("control point 0 overflows millimetres"));
}

#[test]
fn sketch_surface_collections_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let payload = canonical_surface_payload();
    for (limit, operation) in [
        (11, "f3d sketch surface scalar values"),
        (15, "f3d sketch surface scalar values"),
        (19, "f3d sketch surface scalar values"),
        (23, "f3d sketch surface scaled points"),
        (25, "f3d sketch surface rows"),
        (27, "f3d sketch surface row points"),
        (29, "f3d sketch surface row points"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = parse_sketch_surface(&ctx, &payload, 0)
            .expect_err("collection limit must refuse surface geometry");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(failure)
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == operation)
        );
    }
}

#[test]
fn sketch_surface_decoder_keeps_constructor_refusals_in_the_outer_result() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let payload = canonical_surface_payload();
    for allowance in [0, 2, 8, 12, 15] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = allowance;
        let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(original)) = parse_sketch_surface(&ctx, &payload, 0)
        else {
            panic!("constructor refusal must not disappear");
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.used, allowance);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original)
        );
    }
}
