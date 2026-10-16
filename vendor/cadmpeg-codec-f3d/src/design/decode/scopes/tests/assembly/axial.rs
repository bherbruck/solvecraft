// SPDX-License-Identifier: Apache-2.0

use super::{
    append_axial_test_component_operand, axial_test_alignment, axial_test_component_scope,
};
use crate::design::decode::scopes::axial_assembly::{
    bind_axial_assembly_operand_targets, bind_joint_origin_frames_from_assemblies,
};
use crate::records::feature::assembly::DesignAssemblyAxialOperandTarget;
use crate::records::feature::scope::{
    DesignFeatureKind, DesignJointOriginTransform, DesignParameterScope, DesignScopePayloadMut,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

fn joint_origin_collection_context(arena: &DecodeArena) -> DecodeContext<'_> {
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    DecodeContext::from_root_bytes(&[], arena, &policy)
        .unwrap()
        .0
}

fn axial_binding_context(arena: &DecodeArena) -> DecodeContext<'_> {
    DecodeContext::from_root_bytes(&[], arena, &DecodePolicy::default())
        .unwrap()
        .0
}

#[test]
fn axial_assembly_bindings_refuse_collection_limit() {
    let identity = crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY.rows();
    let mut assembly = DesignParameterScope::empty("assembly", DesignFeatureKind::Assemble, 10);
    assembly
        .try_edit(|draft| {
            draft.frame_length = 705;
            draft.paired_byte_offset = 705;
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let DesignScopePayloadMut::Assemble(slot) = assembly.payload_mut() {
        *slot = Some(axial_test_alignment([identity, identity]));
    }
    let origins = [70_u32, 80_u32].map(|index| {
        let mut origin =
            DesignParameterScope::empty("origin", DesignFeatureKind::JointOrigin, index);
        origin.with_joint_origin_transform(identity.try_into().unwrap());
        origin
    });
    let mut scopes = vec![assembly];
    scopes.extend(origins);
    let arena = DecodeArena::new();
    let ctx = joint_origin_collection_context(&arena);
    let records = crate::design::test_support::indexed_record_offsets_for_test(&[]);
    let error = bind_axial_assembly_operand_targets(&ctx, &[], &records, &mut scopes).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(failure)
        if failure.dimension == ResourceDimension::CollectionItems
            && failure.operation == "f3d axial assembly bindings")
    );
}

#[test]
fn joint_origin_frame_candidates_refuse_collection_limit() {
    let mut assembly = DesignParameterScope::empty("assembly", DesignFeatureKind::Assemble, 10);
    let identity = crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY.rows();
    if let DesignScopePayloadMut::Assemble(slot) = assembly.payload_mut() {
        *slot = Some(axial_test_alignment([identity, identity]));
    }
    let arena = DecodeArena::new();
    let ctx = joint_origin_collection_context(&arena);
    let error = bind_joint_origin_frames_from_assemblies(&ctx, &[], &mut [assembly]).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(failure)
        if failure.dimension == ResourceDimension::CollectionItems
            && failure.operation == "f3d joint-origin frame candidates")
    );
}

#[test]
fn joint_origin_assembly_envelopes_refuse_collection_limit() {
    let mut assembly = DesignParameterScope::empty("assembly", DesignFeatureKind::Assemble, 10);
    assembly.class_tag = "276".to_owned().try_into().unwrap();
    assembly.paired_class_tag = "258".to_owned().try_into().unwrap();
    assembly
        .try_edit(|draft| {
            draft.frame_length = 604;
            draft.paired_byte_offset = 604;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut bytes = vec![0_u8; 604];
    bytes[24] = 1;
    bytes[25..29].copy_from_slice(&90_u32.to_le_bytes());
    bytes[164] = 1;
    bytes[165..169].copy_from_slice(&91_u32.to_le_bytes());
    bytes[175..179].copy_from_slice(&1_u32.to_le_bytes());
    for (ordinal, row) in crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY
        .rows()
        .into_iter()
        .enumerate()
    {
        for (column, value) in row.into_iter().enumerate() {
            let at = 36 + (ordinal * 4 + column) * 8;
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
    }
    let arena = DecodeArena::new();
    let ctx = joint_origin_collection_context(&arena);
    let error =
        bind_joint_origin_frames_from_assemblies(&ctx, &bytes, &mut [assembly]).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(failure)
        if failure.dimension == ResourceDimension::CollectionItems
            && failure.operation == "f3d joint-origin assembly envelopes")
    );
}

#[test]
fn resolved_joint_origins_refuse_collection_limit() {
    let mut origin = DesignParameterScope::empty("origin", DesignFeatureKind::JointOrigin, 91);
    if let DesignScopePayloadMut::JointOrigin(slot) = origin.payload_mut() {
        *slot = Some(DesignJointOriginTransform {
            joint_origin_transform:
                crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
            joint_origin_transform_offset: 0,
            reference: None,
        });
    }
    let arena = DecodeArena::new();
    let ctx = joint_origin_collection_context(&arena);
    let error = bind_joint_origin_frames_from_assemblies(&ctx, &[], &mut [origin]).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(failure)
        if failure.dimension == ResourceDimension::CollectionItems
            && failure.operation == "f3d resolved joint origins")
    );
}

#[test]
fn axial_external_reference_text_refuses_retained_limit() {
    let transform = crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY.rows();
    let first_role = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
    let second_role = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";
    let mut bytes = vec![0; 772];
    let first_members = append_axial_test_component_operand(
        &mut bytes,
        70,
        [10, 30],
        transform,
        7_001,
        first_role,
        false,
    );
    let second_members = append_axial_test_component_operand(
        &mut bytes,
        80,
        [100, 120],
        transform,
        8_001,
        second_role,
        true,
    );
    let mut assembly = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:assembly#500",
        DesignFeatureKind::Assemble,
        500,
    );
    assembly
        .try_edit(|draft| {
            draft.frame_length = 772;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(
                first_members
                    .into_iter()
                    .chain(second_members)
                    .chain([90, 91])
                    .collect(),
            );
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let DesignScopePayloadMut::Assemble(slot) = assembly.payload_mut() {
        *slot = Some(axial_test_alignment([transform, transform]));
    }
    let mut scopes = vec![
        assembly,
        axial_test_component_scope(200, first_role),
        axial_test_component_scope(300, second_role),
    ];
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    // Two selector GUID copies precede the external-reference text.
    policy.limits.max_retained_bytes = 2 * 36 + 35;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error =
        bind_axial_assembly_operand_targets(&ctx, &bytes, &records, &mut scopes).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(failure)
        if failure.dimension == ResourceDimension::RetainedBytes
            && failure.operation == "f3d Design UTF-16 text")
    );
}

#[test]
fn axial_assembly_selectors_bind_component_insert_occurrences_exactly() {
    let arena = DecodeArena::new();
    let ctx = axial_binding_context(&arena);
    let first_transform = crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY.rows();
    let mut second_transform =
        crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY.rows();
    second_transform[2][3] = 4.25;
    let first_role = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
    let second_role = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";
    let mut bytes = vec![0; 772];
    let first_members = append_axial_test_component_operand(
        &mut bytes,
        70,
        [10, 30],
        first_transform,
        7_001,
        first_role,
        false,
    );
    let second_members = append_axial_test_component_operand(
        &mut bytes,
        80,
        [100, 120],
        second_transform,
        8_001,
        second_role,
        true,
    );
    let mut assembly = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:assembly#500",
        crate::records::feature::scope::DesignFeatureKind::Assemble,
        500,
    );
    assembly
        .try_edit(|draft| {
            draft.frame_length = 772;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(
                first_members
                    .into_iter()
                    .chain(second_members)
                    .chain([90, 91])
                    .collect(),
            );
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::Assemble(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::AsBuilt(slot) =
        assembly.payload_mut()
    {
        *slot = Some(axial_test_alignment([first_transform, second_transform]));
    }
    let mut scopes = vec![
        assembly,
        axial_test_component_scope(200, first_role),
        axial_test_component_scope(300, second_role),
    ];
    let unresolved_scopes = scopes.clone();

    bind_axial_assembly_operand_targets(
        &ctx,
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &mut scopes,
    )
    .unwrap();
    let targets = scopes[0]
        .assembly_alignment()
        .and_then(|alignment| {
            let crate::records::feature::assembly::DesignAssemblyAlignmentForm::Qualified(operands) =
                alignment.form.as_ref()?
            else {
                return None;
            };
            let [first, second] = operands.each_ref().map(|operand| match &operand.qualifier {
                crate::records::feature::assembly::DesignAssemblyOperandQualifier::AxialTarget { target } => {
                    Some(target.clone())
                }
                _ => None,
            });
            Some([first?, second?])
        })
        .expect("two exact pathless assembly targets");
    let DesignAssemblyAxialOperandTarget::ComponentInsertOccurrence {
        component_insert_scope_record_index,
        construction_byte_offset,
        construction_transform_offset,
        axis_record_index_offsets,
        construction_paired_byte_offset,
        selectors,
        ..
    } = targets[0].clone()
    else {
        panic!("first operand must select a component insertion");
    };
    assert_eq!(component_insert_scope_record_index, 200);
    assert_eq!(construction_transform_offset, construction_byte_offset + 48);
    assert_eq!(axis_record_index_offsets[0], construction_byte_offset + 193);
    assert_eq!(axis_record_index_offsets[1], construction_byte_offset + 209);
    assert_eq!(
        construction_paired_byte_offset,
        construction_byte_offset + 380
    );
    assert_eq!(selectors[0].axis_paired_class_tag.as_str(), "261");
    assert_eq!(selectors[0].selector_paired_class_tag.as_str(), "261");
    assert_eq!(selectors[0].occurrence_reference, 10_001);
    assert_eq!(selectors[1].occurrence_reference, 10_002);
    assert_eq!(selectors[0].external_object_reference, 7_001);
    assert!(selectors[0].external_version.is_none());
    let DesignAssemblyAxialOperandTarget::ComponentInsertOccurrence {
        component_insert_scope_record_index,
        selectors: versioned_selectors,
        ..
    } = targets[1].clone()
    else {
        panic!("second operand must select a component insertion");
    };
    assert_eq!(component_insert_scope_record_index, 300);
    assert!(versioned_selectors[0].external_version.is_some());
    assert_eq!(
        versioned_selectors[0]
            .external_version
            .as_ref()
            .map(|version| version.version_urn.value.as_str()),
        Some("urn:test:version:2")
    );

    let mut mismatched = bytes.clone();
    let mismatch_at =
        usize::try_from(selectors[1].external_object_reference_offset).expect("test offset");
    mismatched[mismatch_at..mismatch_at + 8].copy_from_slice(&7_002_u64.to_le_bytes());
    let mut mismatched_scopes = unresolved_scopes;
    bind_axial_assembly_operand_targets(
        &ctx,
        &mismatched,
        &crate::design::test_support::indexed_record_offsets_for_test(&mismatched),
        &mut mismatched_scopes,
    )
    .unwrap();
    assert!(mismatched_scopes[0]
        .assembly_alignment()
        .is_some_and(|alignment| !matches!(
            alignment.form.as_ref(),
            Some(crate::records::feature::assembly::DesignAssemblyAlignmentForm::Qualified([
                crate::records::feature::assembly::DesignQualifiedAssemblyOperand {
                    qualifier: crate::records::feature::assembly::DesignAssemblyOperandQualifier::AxialTarget { .. },
                    ..
                },
                crate::records::feature::assembly::DesignQualifiedAssemblyOperand {
                    qualifier: crate::records::feature::assembly::DesignAssemblyOperandQualifier::AxialTarget { .. },
                    ..
                },
            ]))
        )));
}

#[test]
fn axial_assembly_selector_binds_a_document_root_joint_origin() {
    let arena = DecodeArena::new();
    let ctx = axial_binding_context(&arena);
    let first_transform = crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY.rows();
    let mut second_transform =
        crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY.rows();
    second_transform[1][3] = 2.5;
    let role = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
    let mut bytes = vec![0; 705];
    let members = append_axial_test_component_operand(
        &mut bytes,
        70,
        [10, 30],
        first_transform,
        7_001,
        role,
        false,
    );
    let mut assembly = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:assembly#500",
        crate::records::feature::scope::DesignFeatureKind::Assemble,
        500,
    );
    assembly
        .try_edit(|draft| {
            draft.frame_length = 705;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(
                members.into_iter().chain([90, 91]).collect(),
            );
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::Assemble(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::AsBuilt(slot) =
        assembly.payload_mut()
    {
        *slot = Some(axial_test_alignment([first_transform, second_transform]));
    }
    let mut origin = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:joint-origin#80",
        crate::records::feature::scope::DesignFeatureKind::JointOrigin,
        80,
    );
    origin.with_joint_origin_transform(second_transform.try_into().unwrap());
    let mut scopes = vec![assembly, axial_test_component_scope(200, role), origin];

    bind_axial_assembly_operand_targets(
        &ctx,
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &mut scopes,
    )
    .unwrap();
    let targets = scopes[0]
        .assembly_alignment()
        .and_then(|alignment| {
            let crate::records::feature::assembly::DesignAssemblyAlignmentForm::Qualified(operands) =
                alignment.form.as_ref()?
            else {
                return None;
            };
            let [first, second] = operands.each_ref().map(|operand| match &operand.qualifier {
                crate::records::feature::assembly::DesignAssemblyOperandQualifier::AxialTarget { target } => {
                    Some(target.clone())
                }
                _ => None,
            });
            Some([first?, second?])
        })
        .expect("component and root assembly targets");
    assert!(matches!(
        &targets[0],
        DesignAssemblyAxialOperandTarget::ComponentInsertOccurrence {
            component_insert_scope_record_index: 200,
            ..
        }
    ));
    assert_eq!(
        targets[1],
        DesignAssemblyAxialOperandTarget::DocumentRootJointOrigin {
            scope_record_index: 80
        }
    );
}
