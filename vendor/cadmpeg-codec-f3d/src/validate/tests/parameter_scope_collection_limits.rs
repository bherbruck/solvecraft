// SPDX-License-Identifier: Apache-2.0

#[derive(Clone, Copy)]
enum Case {
    Standard,
    Compact,
    Flange,
    ToObject,
}

fn scope(case: Case) -> crate::records::feature::scope::DesignParameterScope {
    use crate::records::{
        feature::{
            scope::{DesignFeatureKind, DesignParameterScope, DesignScopePayload},
            thread::{
                DesignThreadConstruction, DesignThreadDiameters, DesignThreadForm,
                DesignThreadNominalSize,
            },
        },
        identity::ReferenceRun,
    };
    let (kind, payload, references) = match case {
        Case::Standard | Case::Compact => {
            let compact = matches!(case, Case::Compact);
            let references = if compact {
                vec![1, 2, 3, 4]
            } else {
                vec![1, 2]
            };
            let construction = DesignThreadConstruction {
                form: if compact {
                    DesignThreadForm::Compact(None)
                } else {
                    DesignThreadForm::Standard
                },
                designation_offset: 38,
                designation: cadmpeg_core::text::NonBlankString::new("M30x3.5").unwrap(),
                nominal_size: DesignThreadNominalSize::try_from("30.0".to_owned()).unwrap(),
                profile: cadmpeg_core::text::NonBlankString::new("ISO Metric profile").unwrap(),
                pitch: cadmpeg_ir::scalar::PositiveReal::new(0.35).unwrap(),
                face_group_record_indices: if compact { vec![1, 3] } else { vec![1] },
                diameters: DesignThreadDiameters::new(2.97345, 2.5732, 2.7568).unwrap(),
            };
            (
                DesignFeatureKind::Thread,
                DesignScopePayload::Thread(Some(construction)),
                references,
            )
        }
        Case::Flange | Case::ToObject => {
            let mut wire = serde_json::json!({
                "edge_wrapper_record_indices":[10,11],
                "edge_group_record_indices":[20,21],
                "edge_operand_record_indices":[23,24],
                "aggregate_group_record_index":30,
                "aggregate_operand_record_indices":[34,35],
                "height_owner_record_index":40,
                "angle_owner_record_index":41,
                "width_mode":"full_edge",
                "width_distance_owner_record_indices":[],
                "width_distance_owner_record_indices_by_edge":[],
                "settings_record_index":42,
                "bend_radius":0.25,
                "bend_radius_offset":50,
                "reference_side_code":4,
                "height_datum":"inner_faces",
                "bend_position":"adjacent"
            });
            let mut references = vec![10, 11, 20, 21, 23, 24, 34, 35, 30, 40, 41, 42];
            if matches!(case, Case::ToObject) {
                wire["height_extent"] = serde_json::json!({
                    "kind":"to_object",
                    "value":{
                        "target_group_record_index":60,
                        "target_operand_record_index":63,
                        "offset_owner_record_index":70,
                        "reference_record_indices":[71,72]
                    }
                });
                references.extend([60, 63, 70]);
            }
            let operation = serde_json::from_value(wire).unwrap();
            (
                DesignFeatureKind::EdgeFlange,
                DesignScopePayload::EdgeFlange(Some(operation)),
                references,
            )
        }
    };
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        kind,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.payload = payload;
            draft.reference_members = ReferenceRun::unlocated(references);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn scope_error(case: Case, max_items: u64) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = crate::native::F3dNative {
            design_parameter_scopes: vec![scope(case)],
            ..Default::default()
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_parameter_scopes(&ctx, &mut Vec::new()).unwrap_err()
    })
}

#[test]
fn standard_thread_group_refuses_collection_limit() {
    let error = scope_error(Case::Standard, 1);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D standard thread face groups")
    );
}

#[test]
fn compact_thread_group_refuses_collection_limit() {
    let error = scope_error(Case::Compact, 1);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D compact thread face groups")
    );
}

#[test]
fn edge_flange_claimed_reference_refuses_collection_limit() {
    let error = scope_error(Case::Flange, 1);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D edge flange claimed references")
    );
}

#[test]
fn edge_flange_target_reference_refuses_collection_limit() {
    let error = scope_error(Case::ToObject, 13);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D edge flange target references")
    );
}

#[test]
fn edge_flange_claimed_index_refuses_collection_limit() {
    let error = scope_error(Case::Flange, 13);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D edge flange claimed references")
    );
}
