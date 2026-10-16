// SPDX-License-Identifier: Apache-2.0

use super::super::{copy_bound_path, copy_bound_profile};
use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{PlanarProfileRef, ProfileRef};
use cadmpeg_ir::sketches::{SketchEntityId, SketchId, SpatialSketchEntityId, SpatialSketchId};

fn assert_refusal(
    operation: &'static str,
    retained: bool,
    mut call: impl FnMut(&DecodeContext<'_>) -> Result<(), CodecError>,
) {
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            if retained {
                cadmpeg_core::decode::ResourceDimension::RetainedBytes
            } else {
                cadmpeg_core::decode::ResourceDimension::CollectionItems
            },
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                if retained {
                    policy.limits.max_retained_bytes = cap;
                } else {
                    policy.limits.max_collection_items = cap;
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                call(&ctx)
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == operation && failure.dimension == (if retained { cadmpeg_core::decode::ResourceDimension::RetainedBytes } else { cadmpeg_core::decode::ResourceDimension::CollectionItems })));
    }
}

#[test]
fn loft_profile_index_refuses_collection_limit() {
    assert_refusal("f3d loft resolved profile index", false, |ctx| {
        let mut map = HashMap::new();
        ctx.insert_hash_map(&mut map, "group", 1, "f3d loft resolved profile index")
            .map(|_| ())
    });
}

#[test]
fn loft_path_index_refuses_collection_limit() {
    assert_refusal("f3d loft resolved path index", false, |ctx| {
        let mut map = HashMap::new();
        ctx.insert_hash_map(&mut map, "group", 1, "f3d loft resolved path index")
            .map(|_| ())
    });
}

#[test]
fn loft_header_index_refuses_collection_limit() {
    assert_refusal("f3d loft header index", false, |ctx| {
        let mut map = HashMap::new();
        ctx.insert_hash_map(&mut map, ("stream", 1), 1, "f3d loft header index")
            .map(|_| ())
    });
}

#[test]
fn bound_planar_profile_id_refuses_retained_limit() {
    let profile = ProfileRef::Planar(PlanarProfileRef::Sketch(
        SketchId::mint("f3d:model:sketch#1").unwrap(),
    ));
    assert_refusal("f3d profile sketch id", true, |ctx| {
        copy_bound_profile(&profile, ctx).map(|_| ())
    });
}

#[test]
fn bound_native_profile_id_refuses_retained_limit() {
    let profile = ProfileRef::Planar(PlanarProfileRef::Native(
        "f3d:Design/BulkStream.dat:group#1".into(),
    ));
    assert_refusal("f3d bound native profile id", true, |ctx| {
        copy_bound_profile(&profile, ctx).map(|_| ())
    });
}

#[test]
fn bound_spatial_profile_index_refuses_collection_limit() {
    let profile = ProfileRef::spatial_sketch_profiles(
        SpatialSketchId::mint("f3d:model:spatial-sketch#1").unwrap(),
        vec![0, 1],
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("profile membership admission")
    .unwrap();
    assert_refusal("f3d bound spatial profile index", false, |ctx| {
        copy_bound_profile(&profile, ctx).map(|_| ())
    });
}

#[test]
fn bound_spatial_selection_id_refuses_retained_limit() {
    let profile = ProfileRef::spatial_sketch_selection(
        SpatialSketchId::mint("f3d:model:spatial-sketch#1").unwrap(),
        vec!["f3d:Design/BulkStream.dat:group#1".into()],
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("profile membership admission")
    .unwrap();
    assert_refusal("f3d bound spatial selection id", true, |ctx| {
        copy_bound_profile(&profile, ctx).map(|_| ())
    });
}

#[test]
fn bound_spatial_selection_refuses_collection_limit() {
    let profile = ProfileRef::spatial_sketch_selection(
        SpatialSketchId::mint("f3d:model:spatial-sketch#1").unwrap(),
        vec!["f3d:Design/BulkStream.dat:group#1".into()],
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("profile membership admission")
    .unwrap();
    assert_refusal("f3d bound spatial selection", false, |ctx| {
        copy_bound_profile(&profile, ctx).map(|_| ())
    });
}

#[test]
fn bound_planar_path_curve_id_refuses_retained_limit() {
    let path = PathRef::sketch_curves(
        SketchId::mint("f3d:model:sketch#1").unwrap(),
        vec![SketchEntityId::mint("f3d:model:sketch-entity#1").unwrap()],
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("profile membership admission")
    .unwrap();
    assert_refusal("f3d bound planar curve id", true, |ctx| {
        copy_bound_path(&path, ctx).map(|_| ())
    });
}

#[test]
fn bound_planar_path_curve_refuses_collection_limit() {
    let path = PathRef::sketch_curves(
        SketchId::mint("f3d:model:sketch#1").unwrap(),
        vec![SketchEntityId::mint("f3d:model:sketch-entity#1").unwrap()],
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("profile membership admission")
    .unwrap();
    assert_refusal("f3d bound planar path curve", false, |ctx| {
        copy_bound_path(&path, ctx).map(|_| ())
    });
}

#[test]
fn bound_spatial_path_curve_id_refuses_retained_limit() {
    let path = PathRef::spatial_sketch_curves(
        SpatialSketchId::mint("f3d:model:spatial-sketch#1").unwrap(),
        vec![SpatialSketchEntityId::mint("f3d:model:spatial-sketch-entity#1").unwrap()],
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("profile membership admission")
    .unwrap();
    assert_refusal("f3d bound spatial curve id", true, |ctx| {
        copy_bound_path(&path, ctx).map(|_| ())
    });
}

#[test]
fn bound_spatial_path_curve_refuses_collection_limit() {
    let path = PathRef::spatial_sketch_curves(
        SpatialSketchId::mint("f3d:model:spatial-sketch#1").unwrap(),
        vec![SpatialSketchEntityId::mint("f3d:model:spatial-sketch-entity#1").unwrap()],
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("profile membership admission")
    .unwrap();
    assert_refusal("f3d bound spatial path curve", false, |ctx| {
        copy_bound_path(&path, ctx).map(|_| ())
    });
}
