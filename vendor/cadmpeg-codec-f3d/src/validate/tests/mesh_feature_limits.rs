// SPDX-License-Identifier: Apache-2.0

fn mesh_feature() -> crate::records::mesh::DesignMeshFeature {
    let identity = serde_json::json!({
        "class_tag": "256", "record_index": 104, "byte_offset": 100, "frame_length": 200
    });
    serde_json::from_value(serde_json::json!({
        "id": "mesh-feature", "scope_record": identity,
        "scope_base_record": {"class_tag":"256","record_index":104,"byte_offset":270,"frame_length":30},
        "collection_record": {"class_tag":"256","record_index":104,"byte_offset":0,"frame_length":73},
        "collection_base_record": {"class_tag":"256","record_index":104,"byte_offset":38,"frame_length":35},
        "texture_table_record": {"class_tag":"256","record_index":104,"byte_offset":100,"frame_length":29},
        "body_count_offsets": [121,21,58],
        "body_record_indices": [], "scope_body_reference_offsets": [],
        "collection_body_reference_offsets": [], "texture_table_reference_offset":27,
        "collection_owner_record": {"class_tag":"256","record_index":104,"byte_offset":100,"frame_length":273},
        "collection_owner_reference_offset":62, "collection_owner_backlink_offset":362,
        "scope_owner_record_index":109, "scope_owner_reference_offset":289,
        "texture_flags_count_offset":121, "texture_filename_count_offset":125,
        "bodies":[], "textures":[]
    })).unwrap()
}

fn mesh_feature_with_body(projected: bool) -> crate::records::mesh::DesignMeshFeature {
    let mut wire = serde_json::to_value(mesh_feature()).unwrap();
    let identity = serde_json::json!({
        "class_tag":"256", "record_index":104, "byte_offset":100, "frame_length":200
    });
    let mut body = serde_json::json!({
        "body_record":{"class_tag":"256","record_index":104,"byte_offset":100,"frame_length":575},
        "entry_name_record":{"class_tag":"256","record_index":104,"byte_offset":100,"frame_length":62},
        "guid_record":identity,
        "wrapper_record":{"class_tag":"256","record_index":104,"byte_offset":100,"frame_length":40},
        "scene_state_record":{"class_tag":"256","record_index":104,"byte_offset":100,"frame_length":95},
        "scene_node_record":{"class_tag":"256","record_index":104,"byte_offset":100,"frame_length":133},
        "scene_auxiliary_record":identity, "owner_record":identity,
        "entry_name":"mesh.paramesh", "entry_name_offset":136,
        "fusion_uuid":"AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE", "fusion_uuid_offset":136,
        "transform":[[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]],
        "transform_offsets":[142,271], "scope_reference_offset":608,
        "wrapper_reference_offset":619, "owner_reference_offset":630,
        "guid_reference_offset":641, "scene_node_reference_offset":653,
        "collection_reference_offset":664, "wrapper_body_reference_offset":121,
        "entry_guid_reference_offset":121, "guid_entry_reference_offset":172,
        "scene_state_reference_offset":133, "scene_auxiliary_reference_offset":148
    });
    if projected {
        body["tessellation_id"] = serde_json::json!("f3d:model:tessellation#one");
    }
    wire["collection_record"]["frame_length"] = serde_json::json!(84);
    wire["collection_base_record"]["frame_length"] = serde_json::json!(46);
    wire["collection_owner_reference_offset"] = serde_json::json!(73);
    wire["body_record_indices"] = serde_json::json!([104]);
    wire["scope_body_reference_offsets"] = serde_json::json!([125]);
    wire["collection_body_reference_offsets"] = serde_json::json!([62]);
    wire["bodies"] = serde_json::json!([body]);
    serde_json::from_value(wire).unwrap()
}

fn mesh_feature_with_textures() -> crate::records::mesh::DesignMeshFeature {
    let mut wire = serde_json::to_value(mesh_feature()).unwrap();
    let row = |ordinal, filename_ordinal, guid, flags_guid, filename_guid| {
        serde_json::json!({
            "ordinal":ordinal,"resource_guid":guid,"flags_guid_offset":flags_guid,
            "flags":7,"flags_offset":flags_guid + 36,
            "filename_ordinal":filename_ordinal,"filename_guid_offset":filename_guid,
            "filename_record":{"class_tag":"256","record_index":8,"byte_offset":300,"frame_length":35},
            "filename_record_reference_offset":filename_guid + 36,
            "filename":"a.png","filename_offset":325,
            "archive_entry_name":"Textures/a.png","asset":"test:model:asset#texture"
        })
    };
    wire["texture_table_record"]["frame_length"] = serde_json::json!(219);
    wire["texture_filename_count_offset"] = serde_json::json!(213);
    wire["textures"] = serde_json::json!([
        row(1, 0, "BBBBBBBB-BBBB-4CCC-8DDD-EEEEEEEEEEEE", 173, 221),
        row(0, 1, "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE", 129, 272)
    ]);
    serde_json::from_value(wire).unwrap()
}

fn mesh_error(
    feature: bool,
    asset: bool,
    tessellation: bool,
    max_items: u64,
    max_retained_bytes: u64,
) -> cadmpeg_core::CodecError {
    let feature: Option<fn() -> crate::records::mesh::DesignMeshFeature> =
        feature.then_some(mesh_feature);
    mesh_error_with_feature(feature, asset, tessellation, max_items, max_retained_bytes)
}

fn mesh_error_with_feature(
    feature: Option<fn() -> crate::records::mesh::DesignMeshFeature>,
    asset: bool,
    tessellation: bool,
    max_items: u64,
    max_retained_bytes: u64,
) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
        if asset {
            ir.model.assets.push(
                cadmpeg_ir::assets::Asset::try_new(
                    cadmpeg_ir::assets::AssetId::mint("f3d:model:asset#one").unwrap(),
                    None,
                    None,
                    cadmpeg_ir::assets::AssetContent::External {
                        uri: cadmpeg_core::text::NonBlankString::new("asset").unwrap(),
                    },
                    None,
                )
                .unwrap(),
            );
        }
        if tessellation {
            ir.model.tessellations.push(
                cadmpeg_ir::tessellation::Tessellation::new(
                    cadmpeg_ir::tessellation::TessellationId::mint("f3d:model:tessellation#one")
                        .expect("valid identity"),
                    cadmpeg_ir::tessellation::TessellationMesh::List {
                        vertices: Vec::new(),
                        triangles: Vec::new(),
                    },
                    Vec::new(),
                )
                .unwrap(),
            );
        }
        let mut native = crate::native::F3dNative::default();
        if let Some(feature) = feature {
            native.design_mesh_features.push(feature());
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        policy.limits.max_retained_bytes = max_retained_bytes;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_mesh_features(&ctx, &mut Vec::new()).unwrap_err()
    })
}

fn body_feature() -> crate::records::mesh::DesignMeshFeature {
    mesh_feature_with_body(false)
}
fn projected_body_feature() -> crate::records::mesh::DesignMeshFeature {
    mesh_feature_with_body(true)
}

#[test]
fn mesh_asset_id_refuses_collection_limit() {
    let error = mesh_error(false, true, false, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D mesh asset IDs")
    );
}

#[test]
fn mesh_tessellation_id_refuses_collection_limit() {
    let error = mesh_error(false, false, true, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D mesh tessellation IDs")
    );
}

#[test]
fn mesh_feature_id_refuses_collection_limit() {
    let error = mesh_error(true, false, false, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D mesh feature IDs")
    );
}

#[test]
fn mesh_scope_record_refuses_collection_limit() {
    let error = mesh_error(true, false, false, 1, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D mesh scope records")
    );
}

#[test]
fn mesh_collection_record_refuses_collection_limit() {
    let error = mesh_error(true, false, false, 2, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D mesh collection records")
    );
}

#[test]
fn mesh_texture_table_refuses_collection_limit() {
    let error = mesh_error(true, false, false, 3, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D mesh texture tables")
    );
}

#[test]
fn mesh_collection_owner_refuses_collection_limit() {
    let error = mesh_error(true, false, false, 4, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D mesh collection owners")
    );
}

#[test]
fn mesh_invalid_finding_refuses_collection_limit() {
    let error = mesh_error(true, false, false, 5, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn mesh_invalid_entity_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D validation entity",
        |cap| Err::<(), cadmpeg_core::CodecError>(mesh_error(true, false, false, u64::MAX, cap)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

macro_rules! body_limit {
    ($name:ident, $limit:expr, $operation:literal) => {
        #[test]
        fn $name() {
            let error = mesh_error_with_feature(Some(body_feature), false, false, $limit, u64::MAX);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == $operation));
        }
    };
}

body_limit!(
    mesh_body_owner_refuses_collection_limit,
    5,
    "index F3D mesh body owner records"
);
body_limit!(
    mesh_body_record_refuses_collection_limit,
    6,
    "index F3D mesh body records"
);
body_limit!(
    mesh_entry_record_refuses_collection_limit,
    7,
    "index F3D mesh entry records"
);
body_limit!(
    mesh_guid_record_refuses_collection_limit,
    8,
    "index F3D mesh GUID records"
);
body_limit!(
    mesh_wrapper_record_refuses_collection_limit,
    9,
    "index F3D mesh wrapper records"
);
body_limit!(
    mesh_scene_state_refuses_collection_limit,
    10,
    "index F3D mesh scene states"
);
body_limit!(
    mesh_scene_node_refuses_collection_limit,
    11,
    "index F3D mesh scene nodes"
);
body_limit!(
    mesh_scene_auxiliary_refuses_collection_limit,
    12,
    "index F3D mesh scene auxiliary records"
);

#[test]
fn mesh_projected_tessellation_refuses_collection_limit() {
    let error = mesh_error_with_feature(Some(projected_body_feature), false, true, 14, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D mesh projected tessellations")
    );
}

#[test]
fn mesh_texture_resource_refuses_collection_limit() {
    let error =
        mesh_error_with_feature(Some(mesh_feature_with_textures), false, false, 5, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D mesh texture resources")
    );
}

#[test]
fn mesh_filename_record_refuses_collection_limit() {
    let error =
        mesh_error_with_feature(Some(mesh_feature_with_textures), false, false, 7, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D mesh filename records")
    );
}
