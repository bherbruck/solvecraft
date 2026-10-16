use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn limited_context(arena: &DecodeArena) -> DecodeContext<'_> {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    DecodeContext::from_root_bytes(&[], arena, &policy)
        .unwrap()
        .0
}

#[test]
fn validation_native_arena_reload_refuses_collection_limit() {
    let board = crate::history_records::AsmBulletinBoard {
        id: "f3d:native:bulletin#1".into(),
        parent: "f3d:native:state#1".into(),
        byte_offset: 0,
        owner_ref: 0,
        number: 0,
        changes: Vec::new(),
    };
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.native
        .namespace_mut("f3d")
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "asm_bulletin_boards",
            &[board],
        )
        .unwrap();
    let arena = DecodeArena::new();
    let ctx = limited_context(&arena);
    let result = super::super::reload_native_arena::<crate::history_records::AsmBulletinBoard>(
        &ctx,
        &ir,
        "asm_bulletin_boards",
    );
    assert!(
        matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "load typed native record")
    );
}

#[test]
fn validation_map_index_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = limited_context(&arena);
    let error = ctx
        .collect_hash_map([("key", 1)], "index F3D test map")
        .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "index F3D test map"
    ));
}

#[test]
fn validation_set_index_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = limited_context(&arena);
    let error = ctx
        .collect_hash_set(["key"], "index F3D test set")
        .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "index F3D test set"
    ));
}

#[test]
fn validation_design_header_index_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = limited_context(&arena);
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let native = crate::native::F3dNative {
        design_record_headers: vec![crate::records::decal::DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:design-record-header#1".into(),
            record_index: 1,
            class_tag: crate::records::references::DesignClassTag::try_from("123".to_owned())
                .unwrap(),
            byte_offset: 0,
        }],
        ..crate::native::F3dNative::default()
    };
    let error = super::super::Ctx::new(&ir, &native, &ctx).err().unwrap();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "index F3D design headers"
    ));
}

#[test]
fn validation_typed_sketch_index_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = limited_context(&arena);
    let error = ctx
        .collect_hash_set(
            [("Design/BulkStream.dat", 1)],
            "index F3D typed sketch records",
        )
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D typed sketch records")
    );
}

#[test]
fn validation_sketch_operand_index_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = limited_context(&arena);
    let error = ctx
        .collect_hash_map(
            [(("Design/BulkStream.dat", 1), 1)],
            "index F3D sketch operands",
        )
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D sketch operands")
    );
}

#[test]
fn validation_sketch_relation_owner_index_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = limited_context(&arena);
    let mut owners = std::collections::HashMap::new();
    let error = &ctx
        .insert_hash_map(
            &mut owners,
            ("Design/BulkStream.dat", 1),
            2,
            "index F3D sketch relation owners",
        )
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D sketch relation owners")
    );
}

#[test]
fn validation_sketch_owner_finding_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = limited_context(&arena);
    let mut findings = Vec::new();
    let error = super::super::emit_sketch_relation_finding(
        &ctx,
        &mut findings,
        "f3d:native:sketch#1",
        "conflicting owner",
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D sketch owner findings")
    );
}

#[test]
fn validation_sketch_owner_finding_id_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D sketch owner finding ID",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut findings = Vec::new();
            super::super::emit_sketch_relation_finding(
                &ctx,
                &mut findings,
                "f3d:native:sketch#1",
                "conflicting owner",
            )
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut findings = Vec::new();
    let error = super::super::emit_sketch_relation_finding(
        &ctx,
        &mut findings,
        "f3d:native:sketch#1",
        "conflicting owner",
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D sketch owner finding ID")
    );
}

#[test]
fn validation_profile_face_group_index_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = limited_context(&arena);
    let error = ctx
        .collect_hash_set(
            [("Design/BulkStream.dat", 1)],
            "index F3D decoded profile face groups",
        )
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D decoded profile face groups")
    );
}

#[test]
fn validation_face_group_member_index_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = limited_context(&arena);
    let error = ctx
        .collect_hash_set(
            [("Design/BulkStream.dat", 1, 2)],
            "index F3D face group members",
        )
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D face group members")
    );
}

#[test]
fn native_configuration_name_index_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let configuration = crate::test_support::with_decode_context(|ctx| {
        crate::records::configuration::DesignConfiguration::try_new_charged(
            ctx,
            "sample.dsgcfgrule".into(),
            crate::records::configuration::DesignConfigurationKind::Rule,
            Vec::new(),
            serde_json::Map::new(),
        )
    })
    .unwrap();
    let mut native = crate::native::F3dNative::default();
    native.design_configurations.push(configuration);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let ctx = super::super::Ctx::new(&ir, &native, &decode).unwrap();
    let error = super::super::validate_configurations(&ctx, &mut Vec::new()).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D configuration entry names")
    );
}

#[test]
fn native_duplicate_configuration_id_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let configuration = crate::test_support::with_decode_context(|ctx| {
        crate::records::configuration::DesignConfiguration::try_new_charged(
            ctx,
            "sample.dsgcfgrule".into(),
            crate::records::configuration::DesignConfigurationKind::Rule,
            Vec::new(),
            serde_json::Map::new(),
        )
    })
    .unwrap();
    let mut native = crate::native::F3dNative::default();
    native.design_configurations.push(configuration.clone());
    native.design_configurations.push(configuration);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D configuration entry ID",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let ctx = super::super::Ctx::new(&ir, &native, &decode).unwrap();
            super::super::validate_configurations(&ctx, &mut Vec::new())
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let ctx = super::super::Ctx::new(&ir, &native, &decode).unwrap();
    let error = super::super::validate_configurations(&ctx, &mut Vec::new()).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D configuration entry ID")
    );
}

#[test]
fn native_duplicate_configuration_finding_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let configuration = crate::test_support::with_decode_context(|ctx| {
        crate::records::configuration::DesignConfiguration::try_new_charged(
            ctx,
            "sample.dsgcfgrule".into(),
            crate::records::configuration::DesignConfigurationKind::Rule,
            Vec::new(),
            serde_json::Map::new(),
        )
    })
    .unwrap();
    let mut native = crate::native::F3dNative::default();
    native.design_configurations.push(configuration.clone());
    native.design_configurations.push(configuration);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let ctx = super::super::Ctx::new(&ir, &native, &decode).unwrap();
    let error = super::super::validate_configurations(&ctx, &mut Vec::new()).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

fn validation_parameter() -> crate::records::parameters::DesignParameter {
    use crate::records::identity::{Located, RecordedValue};
    use crate::records::parameters::{
        DesignParameter, DesignParameterDiscriminator, DesignParameterDraft, DesignParameterSource,
    };
    DesignParameter::try_from(DesignParameterDraft {
        id: "f3d:native:parameter#0".into(),
        byte_offset: 100,
        class_tag: crate::records::references::DesignClassTag::try_from("305".to_owned()).unwrap(),
        record_index: 1,
        source_ordinal: 0,
        source: DesignParameterSource::User {
            family_discriminator: Located {
                value: DesignParameterDiscriminator::Code0,
                offset: 122,
            },
        },
        expression: "-1 mm".into(),
        expression_offset: 140,
        source_kind_offset: 160,
        unit: Some(RecordedValue {
            value: "mm".into(),
            offset: 170,
        }),
        name: "Width".into(),
        name_offset: 180,
        evaluated_value: -1.0,
        evaluated_value_offset: 190,
    })
    .unwrap()
}

#[test]
fn native_parameter_validator_index_refuses_collection_limit() {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = crate::native::F3dNative::default();
        native.design_parameters.push(validation_parameter());
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        let error = super::super::validate_parameters(&ctx, &mut Vec::new()).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D validation parameters")
        );
    })
}

#[test]
fn native_parameter_validator_finding_refuses_collection_limit() {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let parameter = validation_parameter();
        let mut native = crate::native::F3dNative::default();
        native.design_parameters.push(parameter.clone());
        native.design_parameters.push(parameter);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        let error = super::super::validate_parameters(&ctx, &mut Vec::new()).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
        );
    })
}

#[test]
fn native_parameter_validator_entity_refuses_retained_limit() {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let parameter = validation_parameter();
        let mut native = crate::native::F3dNative::default();
        native.design_parameters.push(parameter.clone());
        native.design_parameters.push(parameter);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            "retain F3D parameter finding entity",
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = cap;
                let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
                ctx.decode = &decode;
                super::super::validate_parameters(&ctx, &mut Vec::new())
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        let error = super::super::validate_parameters(&ctx, &mut Vec::new()).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D parameter finding entity")
        );
    })
}

#[test]
fn validation_native_arena_reload_releases_scoped_storage() {
    let board = crate::history_records::AsmBulletinBoard {
        id: "f3d:native:bulletin#1".into(),
        parent: "f3d:native:state#1".into(),
        byte_offset: 0,
        owner_ref: 0,
        number: 0,
        changes: Vec::new(),
    };
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.native
        .namespace_mut("f3d")
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "asm_bulletin_boards",
            &[board],
        )
        .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    // Reload admits the native object backing nodes within scoped storage.
    policy.limits.max_materialized_bytes = 16384;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (records, storage) = super::super::reload_native_arena::<
        crate::history_records::AsmBulletinBoard,
    >(&ctx, &ir, "asm_bulletin_boards")
    .unwrap();
    assert_eq!(records[0].id, "f3d:native:bulletin#1");
    drop(records);
    drop(storage);
    let full = ctx
        .reserve_scoped(
            policy.limits.max_materialized_bytes,
            "reuse validation storage",
        )
        .unwrap();
    drop(full);
}
